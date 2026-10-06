//! Layer-shell ownership and output-relative movement, adapted from PR #1096.
//! The existing renderer and application-owned menu remain outside this adapter.
use super::{OverlayError, OverlayWindowBounds, err};
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle, WindowHandle,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData, Region},
    delegate_registry,
    dispatch2::Dispatch2,
    output::{OutputHandler, OutputInfo, OutputState},
    reexports::{
        calloop::EventLoop,
        calloop_wayland_source::WaylandSource,
        protocols::wp::relative_pointer::zv1::client::{
            zwp_relative_pointer_manager_v1::ZwpRelativePointerManagerV1,
            zwp_relative_pointer_v1::{self, ZwpRelativePointerV1},
        },
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        pointer::{BTN_LEFT, BTN_RIGHT, PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
};
use std::{
    ptr::NonNull,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};
use wayland_client::{
    Connection, Proxy, QueueHandle,
    globals::{BindError, registry_queue_init},
    protocol::{wl_output, wl_pointer, wl_seat, wl_surface},
};

pub(super) enum PointerInput {
    Motion((f64, f64)),
    LeftPressed,
    Right(bool),
    Cancel,
}

struct Geometry {
    size: (u32, u32),
    margin: (f64, f64),
    output: Option<((i32, i32), (u32, u32))>,
    restore_position: Option<(i32, i32)>,
    center: bool,
    keep_inside: bool,
}

/// wgpu retains this owner until its presentation surface has been released.
pub(super) struct LayerTarget {
    connection: Connection,
    layer: LayerSurface,
    compositor: Arc<CompositorState>,
    scale: AtomicU32,
    geometry: Mutex<Geometry>,
}
impl LayerTarget {
    pub(super) fn scale_factor(&self) -> f64 {
        f64::from(self.scale.load(Ordering::Relaxed))
    }
    pub(super) fn physical_size(&self) -> winit::dpi::PhysicalSize<u32> {
        let geometry = self.geometry.lock().unwrap();
        let scale = self.scale.load(Ordering::Relaxed);
        winit::dpi::PhysicalSize::new(
            geometry.size.0.saturating_mul(scale),
            geometry.size.1.saturating_mul(scale),
        )
    }
    pub(super) fn resize(&self, width: u32, height: u32) {
        self.geometry.lock().unwrap().size = (width, height);
        self.layer.set_size(width, height);
        self.apply_margin();
    }
    pub(super) fn set_keep_inside(&self, enabled: bool) {
        let mut geometry = self.geometry.lock().unwrap();
        if geometry.keep_inside == enabled {
            return;
        }
        geometry.keep_inside = enabled;
        drop(geometry);
        self.apply_margin();
    }
    fn move_by(&self, dx: f64, dy: f64) {
        let mut geometry = self.geometry.lock().unwrap();
        geometry.margin.0 += dx;
        geometry.margin.1 += dy;
        drop(geometry);
        self.apply_margin();
    }
    fn apply_margin(&self) {
        let mut geometry = self.geometry.lock().unwrap();
        if geometry.keep_inside
            && let Some((_, output)) = geometry.output
        {
            geometry.margin.0 = geometry
                .margin
                .0
                .clamp(0., f64::from(output.0.saturating_sub(geometry.size.0)));
            geometry.margin.1 = geometry
                .margin
                .1
                .clamp(0., f64::from(output.1.saturating_sub(geometry.size.1)));
        }
        self.layer.set_margin(
            geometry.margin.1.round() as i32,
            0,
            0,
            geometry.margin.0.round() as i32,
        );
        drop(geometry);
        self.layer.commit();
    }
    pub(super) fn set_cursor_hittest(&self, enabled: bool) -> Result<(), OverlayError> {
        if enabled {
            self.layer.wl_surface().set_input_region(None);
        } else {
            let region = Region::new(self.compositor.as_ref()).map_err(err)?;
            self.layer
                .wl_surface()
                .set_input_region(Some(region.wl_region()));
        }
        self.layer.commit();
        Ok(())
    }
    pub(super) fn bounds(&self) -> Result<OverlayWindowBounds, OverlayError> {
        let geometry = self.geometry.lock().unwrap();
        let Some((position, _)) = geometry.output else {
            return Err(err("layer output geometry is not available"));
        };
        Ok(OverlayWindowBounds::new(
            position.0.saturating_add(geometry.margin.0.round() as i32),
            position.1.saturating_add(geometry.margin.1.round() as i32),
            geometry.size.0,
            geometry.size.1,
        ))
    }
}
impl HasDisplayHandle for LayerTarget {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let pointer = NonNull::new(self.connection.backend().display_ptr().cast())
            .ok_or(HandleError::Unavailable)?;
        // SAFETY: this owner keeps the connection alive throughout the borrow.
        Ok(unsafe {
            DisplayHandle::borrow_raw(RawDisplayHandle::Wayland(WaylandDisplayHandle::new(
                pointer,
            )))
        })
    }
}
impl HasWindowHandle for LayerTarget {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let pointer = NonNull::new(self.layer.wl_surface().id().as_ptr().cast())
            .ok_or(HandleError::Unavailable)?;
        // SAFETY: this owner retains the layer role and wl_surface throughout the borrow.
        Ok(unsafe {
            WindowHandle::borrow_raw(RawWindowHandle::Wayland(WaylandWindowHandle::new(pointer)))
        })
    }
}

struct PointerRegistration {
    seat: wl_seat::WlSeat,
    pointer: wl_pointer::WlPointer,
    relative: Option<ZwpRelativePointerV1>,
}
struct LayerState {
    registry: RegistryState,
    outputs: OutputState,
    seats: SeatState,
    target: Arc<LayerTarget>,
    relative_manager: Option<ZwpRelativePointerManagerV1>,
    pointers: Vec<PointerRegistration>,
    left_drag: Option<(wl_pointer::WlPointer, (f64, f64))>,
    output: Option<wl_output::WlOutput>,
    configured: bool,
    closed: bool,
    frame_pending: bool,
    failure: Option<OverlayError>,
    inputs: Vec<PointerInput>,
}
impl LayerState {
    fn update_geometry(&mut self) {
        let Some(info) = self
            .output
            .as_ref()
            .and_then(|output| self.outputs.info(output))
        else {
            return;
        };
        let position = info.logical_position.unwrap_or(info.location);
        let Some((width, height)) = logical_output_size(info) else {
            return;
        };
        let mut geometry = self.target.geometry.lock().unwrap();
        geometry.output = Some((position, (width, height)));
        if let Some(saved) = geometry.restore_position.take() {
            geometry.margin = (
                f64::from(saved.0 - position.0),
                f64::from(saved.1 - position.1),
            );
            geometry.center = false;
        } else if geometry.center {
            geometry.margin = (
                f64::from(width.saturating_sub(geometry.size.0)) * 0.5,
                f64::from(height.saturating_sub(geometry.size.1)) * 0.5,
            );
            geometry.center = false;
        }
        drop(geometry);
        self.target.apply_margin();
    }
    fn remove_pointer(&mut self, seat: &wl_seat::WlSeat) {
        if let Some(index) = self
            .pointers
            .iter()
            .position(|pointer| &pointer.seat == seat)
        {
            let pointer = self.pointers.swap_remove(index);
            if let Some(relative) = pointer.relative {
                relative.destroy();
            }
            if self
                .left_drag
                .as_ref()
                .is_some_and(|(active, _)| active == &pointer.pointer)
            {
                self.left_drag = None;
            }
            pointer.pointer.release();
            self.inputs.push(PointerInput::Cancel);
        }
    }
}
impl CompositorHandler for LayerState {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        factor: i32,
    ) {
        let factor = u32::try_from(factor).unwrap_or(1).max(1);
        match self.target.layer.set_buffer_scale(factor) {
            Ok(()) => self.target.scale.store(factor, Ordering::Relaxed),
            Err(_) => self.failure = Some(err("Wayland surface lacks buffer-scale support")),
        }
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wayland_client::protocol::wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
    }
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        output: &wl_output::WlOutput,
    ) {
        if self.output.is_none() {
            self.output = Some(output.clone());
        }
        self.update_geometry();
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
        // The role stays assigned to its output even while an off-screen surface
        // has no intersection. Keep those bounds so re-enabling the screen
        // constraint can bring it back; output_destroyed handles actual removal.
    }
}
impl OutputHandler for LayerState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.update_geometry();
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.update_geometry();
    }
    fn output_destroyed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        if self.output.as_ref() == Some(&output) {
            self.output = None;
            self.target.geometry.lock().unwrap().output = None;
        }
    }
}
impl LayerShellHandler for LayerState {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.closed = true;
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        let mut geometry = self.target.geometry.lock().unwrap();
        if configure.new_size.0 != 0 {
            geometry.size.0 = configure.new_size.0;
        }
        if configure.new_size.1 != 0 {
            geometry.size.1 = configure.new_size.1;
        }
        self.configured = true;
    }
}
impl SeatHandler for LayerState {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seats
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer
            && let Ok(pointer) = self.seats.get_pointer(qh, &seat)
        {
            let relative = self.relative_manager.as_ref().map(|manager| {
                manager.get_relative_pointer(&pointer, qh, RelativePointerData(pointer.clone()))
            });
            self.pointers.push(PointerRegistration {
                seat,
                pointer,
                relative,
            });
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            self.remove_pointer(&seat);
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        self.remove_pointer(&seat);
    }
}
impl PointerHandler for LayerState {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if &event.surface != self.target.layer.wl_surface() {
                continue;
            }
            match event.kind {
                PointerEventKind::Motion { .. } | PointerEventKind::Enter { .. } => {
                    if let Some((active, origin)) = &self.left_drag
                        && active == pointer
                        && self
                            .pointers
                            .iter()
                            .find(|registration| &registration.pointer == pointer)
                            .is_some_and(|registration| registration.relative.is_none())
                    {
                        self.target
                            .move_by(event.position.0 - origin.0, event.position.1 - origin.1);
                    }
                    self.inputs.push(PointerInput::Motion(event.position));
                }
                PointerEventKind::Press {
                    button: BTN_LEFT, ..
                } => {
                    self.left_drag = Some((pointer.clone(), event.position));
                    self.inputs.push(PointerInput::LeftPressed);
                }
                PointerEventKind::Release {
                    button: BTN_LEFT, ..
                } => {
                    self.left_drag = None;
                }
                PointerEventKind::Press {
                    button: BTN_RIGHT, ..
                } => {
                    self.inputs.push(PointerInput::Motion(event.position));
                    self.inputs.push(PointerInput::Right(true));
                }
                PointerEventKind::Release {
                    button: BTN_RIGHT, ..
                } => {
                    self.inputs.push(PointerInput::Right(false));
                }
                PointerEventKind::Leave { .. } if self.left_drag.is_none() => {
                    self.inputs.push(PointerInput::Cancel);
                }
                _ => {}
            }
        }
    }
}
struct RelativeManagerData;
struct RelativePointerData(wl_pointer::WlPointer);
impl Dispatch2<ZwpRelativePointerManagerV1, LayerState> for RelativeManagerData {
    fn event(
        &self,
        _: &mut LayerState,
        _: &ZwpRelativePointerManagerV1,
        _: <ZwpRelativePointerManagerV1 as Proxy>::Event,
        _: &Connection,
        _: &QueueHandle<LayerState>,
    ) {
    }
}
impl Dispatch2<ZwpRelativePointerV1, LayerState> for RelativePointerData {
    fn event(
        &self,
        state: &mut LayerState,
        _: &ZwpRelativePointerV1,
        event: <ZwpRelativePointerV1 as Proxy>::Event,
        _: &Connection,
        _: &QueueHandle<LayerState>,
    ) {
        if state
            .left_drag
            .as_ref()
            .is_some_and(|(active, _)| active == &self.0)
            && let zwp_relative_pointer_v1::Event::RelativeMotion { dx, dy, .. } = event
        {
            state.target.move_by(dx, dy);
        }
    }
}
smithay_client_toolkit::delegate_dispatch2!(LayerState);
delegate_registry!(LayerState);
impl ProvidesRegistryState for LayerState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}

pub(super) struct LayerWindow {
    pub(super) target: Arc<LayerTarget>,
    event_loop: EventLoop<'static, LayerState>,
    qh: QueueHandle<LayerState>,
    state: LayerState,
}
impl LayerWindow {
    pub(super) fn available() -> Result<bool, OverlayError> {
        let connection = Connection::connect_to_env().map_err(err)?;
        let (globals, queue) = registry_queue_init::<LayerState>(&connection).map_err(err)?;
        match LayerShell::bind(&globals, &queue.handle()) {
            Ok(_) => Ok(true),
            Err(BindError::NotPresent) => Ok(false),
            Err(error) => Err(err(error)),
        }
    }
    pub(super) fn create(
        width: u32,
        height: u32,
        bounds: Option<OverlayWindowBounds>,
        keep_inside: bool,
    ) -> Result<Self, OverlayError> {
        let connection = Connection::connect_to_env().map_err(err)?;
        let (globals, queue) = registry_queue_init::<LayerState>(&connection).map_err(err)?;
        let qh = queue.handle();
        let shell = LayerShell::bind(&globals, &qh).map_err(err)?;
        let compositor = Arc::new(CompositorState::bind(&globals, &qh).map_err(err)?);
        let layer = shell.create_layer_surface(
            &qh,
            compositor.create_surface(&qh),
            Layer::Top,
            Some(bongocat_config::BUNDLE_ID),
            None,
        );
        layer.set_anchor(Anchor::TOP | Anchor::LEFT);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_size(width, height);
        let target = Arc::new(LayerTarget {
            connection: connection.clone(),
            layer,
            compositor,
            scale: AtomicU32::new(1),
            geometry: Mutex::new(Geometry {
                size: (width, height),
                margin: (0., 0.),
                output: None,
                restore_position: bounds.map(|bounds| (bounds.x, bounds.y)),
                center: bounds.is_none(),
                keep_inside,
            }),
        });
        target.set_cursor_hittest(false)?;
        let mut state = LayerState {
            registry: RegistryState::new(&globals),
            outputs: OutputState::new(&globals, &qh),
            seats: SeatState::new(&globals, &qh),
            target: target.clone(),
            relative_manager: match globals.bind(&qh, 1..=1, RelativeManagerData) {
                Ok(manager) => Some(manager),
                Err(BindError::NotPresent) => None,
                Err(error) => return Err(err(error)),
            },
            pointers: Vec::new(),
            left_drag: None,
            output: None,
            configured: false,
            closed: false,
            frame_pending: false,
            failure: None,
            inputs: Vec::new(),
        };
        let mut event_loop = EventLoop::try_new().map_err(err)?;
        WaylandSource::new(connection, queue)
            .insert(event_loop.handle())
            .map_err(err)?;
        let deadline = Instant::now() + Duration::from_secs(2);
        while !state.configured && !state.closed {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| err("compositor did not configure the layer surface"))?;
            event_loop
                .dispatch(remaining.min(Duration::from_millis(50)), &mut state)
                .map_err(err)?;
        }
        if state.closed {
            return Err(err("compositor closed the layer surface during creation"));
        }
        if let Some(error) = state.failure.take() {
            return Err(error);
        }
        Ok(Self {
            target,
            event_loop,
            qh,
            state,
        })
    }
    pub(super) fn pump(&mut self) -> Result<(bool, Vec<PointerInput>), OverlayError> {
        self.event_loop
            .dispatch(Duration::ZERO, &mut self.state)
            .map_err(err)?;
        if let Some(error) = self.state.failure.take() {
            return Err(error);
        }
        Ok((self.state.closed, std::mem::take(&mut self.state.inputs)))
    }
    pub(super) fn presented(&mut self) {
        if !self.state.frame_pending {
            self.target.layer.wl_surface().frame(
                &self.qh,
                FrameCallbackData(self.target.layer.wl_surface().clone()),
            );
            self.target.layer.commit();
            self.state.frame_pending = true;
        }
    }
    pub(super) fn cancel_move(&mut self) {
        self.state.left_drag = None;
    }
}

fn logical_output_size(info: OutputInfo) -> Option<(u32, u32)> {
    info.logical_size
        .and_then(|(width, height)| Some((u32::try_from(width).ok()?, u32::try_from(height).ok()?)))
        .or_else(|| {
            let scale = u32::try_from(info.scale_factor).ok()?.max(1);
            let mode = info.modes.into_iter().find(|mode| mode.current)?;
            let (width, height) = if matches!(
                info.transform,
                wl_output::Transform::_90
                    | wl_output::Transform::_270
                    | wl_output::Transform::Flipped90
                    | wl_output::Transform::Flipped270
            ) {
                (mode.dimensions.1, mode.dimensions.0)
            } else {
                mode.dimensions
            };
            Some((
                u32::try_from(width).ok()? / scale,
                u32::try_from(height).ok()? / scale,
            ))
        })
        .filter(|(width, height)| *width > 0 && *height > 0)
}
