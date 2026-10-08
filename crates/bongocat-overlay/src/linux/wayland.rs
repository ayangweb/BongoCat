//! Wayland window ownership and popup attachment, adapted from EEEntity PR #1096.
//! The renderer retains the handle owner; the application owns menu actions.
use super::context_menu::{ContextMenuState, PopupTrigger};
use super::{OverlayError, OverlayWindowBounds, err};
use bongocat_platform::{SystemMenuAction, SystemMenuPalette, SystemMenuPresentation};
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
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{BTN_LEFT, BTN_RIGHT, PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        xdg::{
            XdgShell, XdgSurface,
            popup::{Popup, PopupConfigure, PopupHandler},
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
        },
    },
    shm::{Shm, ShmHandler},
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
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface},
};

pub(super) enum PointerInput {
    Motion((f64, f64)),
    RightPressed(PopupTrigger),
    RightReleased,
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

enum WindowRole {
    Layer(LayerSurface),
    Ordinary(Window),
}
impl WindowRole {
    fn surface(&self) -> &wl_surface::WlSurface {
        match self {
            Self::Layer(layer) => layer.wl_surface(),
            Self::Ordinary(window) => window.wl_surface(),
        }
    }
}

/// wgpu retains this owner until its presentation surface has been released.
pub(super) struct WindowTarget {
    connection: Connection,
    role: WindowRole,
    compositor: Arc<CompositorState>,
    scale: AtomicU32,
    geometry: Mutex<Geometry>,
}
impl WindowTarget {
    pub(super) fn scale_factor(&self) -> f64 {
        f64::from(self.scale.load(Ordering::Relaxed))
    }
    pub(super) fn physical_size(&self) -> (u32, u32) {
        let geometry = self.geometry.lock().unwrap();
        let scale = self.scale.load(Ordering::Relaxed);
        (
            geometry.size.0.saturating_mul(scale),
            geometry.size.1.saturating_mul(scale),
        )
    }
    pub(super) fn resize(&self, width: u32, height: u32) {
        self.geometry.lock().unwrap().size = (width, height);
        match &self.role {
            WindowRole::Layer(layer) => layer.set_size(width, height),
            WindowRole::Ordinary(window) => window.set_window_geometry(0, 0, width, height),
        }
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
        let WindowRole::Layer(layer) = &self.role else {
            self.role.surface().commit();
            return;
        };
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
        layer.set_margin(
            geometry.margin.1.round() as i32,
            0,
            0,
            geometry.margin.0.round() as i32,
        );
        drop(geometry);
        self.role.surface().commit();
    }
    pub(super) fn set_cursor_hittest(&self, enabled: bool) -> Result<(), OverlayError> {
        if enabled {
            self.role.surface().set_input_region(None);
        } else {
            let region = Region::new(self.compositor.as_ref()).map_err(err)?;
            self.role
                .surface()
                .set_input_region(Some(region.wl_region()));
        }
        self.role.surface().commit();
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
impl HasDisplayHandle for WindowTarget {
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
impl HasWindowHandle for WindowTarget {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let pointer = NonNull::new(self.role.surface().id().as_ptr().cast())
            .ok_or(HandleError::Unavailable)?;
        // SAFETY: this owner retains the window role and wl_surface throughout the borrow.
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
struct KeyboardRegistration {
    seat: wl_seat::WlSeat,
    keyboard: wl_keyboard::WlKeyboard,
    focused_surface: Option<wl_surface::WlSurface>,
}
struct WaylandState {
    registry: RegistryState,
    outputs: OutputState,
    seats: SeatState,
    target: Arc<WindowTarget>,
    xdg_shell: XdgShell,
    context_menu: ContextMenuState,
    keyboards: Vec<KeyboardRegistration>,
    right_press: Option<(wl_pointer::WlPointer, PopupTrigger)>,
    keyboard_interactive: bool,
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
impl WaylandState {
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
            if self
                .right_press
                .as_ref()
                .is_some_and(|(active, _)| active == &pointer.pointer)
            {
                self.right_press = None;
            }
            self.context_menu.dismiss();
            pointer.pointer.release();
            self.inputs.push(PointerInput::Cancel);
        }
    }
    fn remove_keyboard(&mut self, seat: &wl_seat::WlSeat) {
        if let Some(index) = self
            .keyboards
            .iter()
            .position(|registration| &registration.seat == seat)
        {
            let registration = self.keyboards.swap_remove(index);
            if let Some(surface) = registration.focused_surface {
                self.context_menu
                    .keyboard_leave(&surface, &registration.seat);
            }
            registration.keyboard.release();
        }
    }
    fn sync_keyboard_interactivity(&mut self) {
        let WindowRole::Layer(layer) = &self.target.role else {
            return;
        };
        let interactive = self.context_menu.has_popup();
        if self.keyboard_interactive != interactive {
            layer.set_keyboard_interactivity(if interactive {
                KeyboardInteractivity::OnDemand
            } else {
                KeyboardInteractivity::None
            });
            layer.commit();
            self.keyboard_interactive = interactive;
        }
    }
}
impl CompositorHandler for WaylandState {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        factor: i32,
    ) {
        let factor = u32::try_from(factor).unwrap_or(1).max(1);
        if surface != self.target.role.surface() {
            self.context_menu.set_scale_factor(surface, factor);
            return;
        }
        self.target.role.surface().set_buffer_scale(factor as i32);
        self.target.scale.store(factor, Ordering::Relaxed);
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wayland_client::protocol::wl_output::Transform,
    ) {
    }
    fn frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        _: u32,
    ) {
        if surface == self.target.role.surface() {
            self.frame_pending = false;
        }
    }
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        output: &wl_output::WlOutput,
    ) {
        if surface != self.target.role.surface() {
            return;
        }
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
impl OutputHandler for WaylandState {
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
impl LayerShellHandler for WaylandState {
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
impl WindowHandler for WaylandState {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.closed = true;
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &Window,
        configure: WindowConfigure,
        _: u32,
    ) {
        let mut geometry = self.target.geometry.lock().unwrap();
        if let Some(width) = configure.new_size.0 {
            geometry.size.0 = width.get();
        }
        if let Some(height) = configure.new_size.1 {
            geometry.size.1 = height.get();
        }
        self.configured = true;
    }
}
impl SeatHandler for WaylandState {
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
                seat: seat.clone(),
                pointer,
                relative,
            });
        }
        if capability == Capability::Keyboard {
            match self.seats.get_keyboard(qh, &seat, None) {
                Ok(keyboard) => self.keyboards.push(KeyboardRegistration {
                    seat,
                    keyboard,
                    focused_surface: None,
                }),
                Err(error) => self.failure = Some(err(error)),
            }
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
        } else if capability == Capability::Keyboard {
            self.remove_keyboard(&seat);
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        self.remove_pointer(&seat);
        self.remove_keyboard(&seat);
    }
}
impl PointerHandler for WaylandState {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        let seat = self
            .pointers
            .iter()
            .find(|registration| &registration.pointer == pointer)
            .map(|registration| registration.seat.clone());
        for event in events {
            if self.context_menu.handles_surface(&event.surface) {
                self.context_menu.pointer_event(event, seat.as_ref());
                continue;
            }
            if &event.surface != self.target.role.surface() {
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
                    button: BTN_LEFT,
                    serial,
                    ..
                } => {
                    self.context_menu.dismiss();
                    match &self.target.role {
                        WindowRole::Layer(_) => {
                            self.left_drag = Some((pointer.clone(), event.position))
                        }
                        WindowRole::Ordinary(window) => {
                            if let Some(seat) = &seat {
                                window.move_(seat, serial);
                            }
                        }
                    }
                }
                PointerEventKind::Release {
                    button: BTN_LEFT, ..
                } => self.left_drag = None,
                PointerEventKind::Press {
                    button: BTN_RIGHT,
                    serial,
                    ..
                } if self.right_press.is_none() => {
                    if let Some(seat) = &seat {
                        let trigger = PopupTrigger {
                            seat: seat.clone(),
                            serial,
                            position: event.position,
                        };
                        self.right_press = Some((pointer.clone(), trigger.clone()));
                        self.inputs.push(PointerInput::RightPressed(trigger));
                    }
                }
                PointerEventKind::Release {
                    button: BTN_RIGHT, ..
                } if self
                    .right_press
                    .as_ref()
                    .is_some_and(|(active, _)| active == pointer) =>
                {
                    self.right_press = None;
                    self.inputs.push(PointerInput::Motion(event.position));
                    self.inputs.push(PointerInput::RightReleased);
                }
                PointerEventKind::Leave { .. }
                    if self.left_drag.is_none() && self.right_press.is_none() =>
                {
                    self.inputs.push(PointerInput::Cancel)
                }
                _ => {}
            }
        }
    }
}
impl KeyboardHandler for WaylandState {
    fn enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        keyboard: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _serial: u32,
        _raw: &[u32],
        _keysyms: &[Keysym],
    ) {
        if let Some(registration) = self
            .keyboards
            .iter_mut()
            .find(|registration| &registration.keyboard == keyboard)
        {
            registration.focused_surface = Some(surface.clone());
            self.context_menu
                .keyboard_enter(surface, &registration.seat);
        }
    }

    fn leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        keyboard: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _serial: u32,
    ) {
        if let Some(registration) = self
            .keyboards
            .iter_mut()
            .find(|registration| &registration.keyboard == keyboard)
            && registration.focused_surface.as_ref() == Some(surface)
        {
            registration.focused_surface = None;
            self.context_menu
                .keyboard_leave(surface, &registration.seat);
        }
    }

    fn press_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        keyboard: &wl_keyboard::WlKeyboard,
        serial: u32,
        event: KeyEvent,
    ) {
        let focus = self
            .keyboards
            .iter()
            .find(|registration| {
                &registration.keyboard == keyboard
                    && registration
                        .focused_surface
                        .as_ref()
                        .is_some_and(|surface| self.context_menu.handles_surface(surface))
            })
            .and_then(|registration| {
                Some((
                    registration.seat.clone(),
                    registration.focused_surface.clone()?,
                ))
            });
        if let Some((seat, surface)) = focus {
            self.context_menu
                .keyboard_key(event.keysym, &seat, &surface, serial);
        }
    }

    fn repeat_key(
        &mut self,
        conn: &Connection,
        qh: &QueueHandle<Self>,
        keyboard: &wl_keyboard::WlKeyboard,
        serial: u32,
        event: KeyEvent,
    ) {
        self.press_key(conn, qh, keyboard, serial, event);
    }

    fn release_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        _event: KeyEvent,
    ) {
    }

    fn update_modifiers(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        _modifiers: Modifiers,
        _raw_modifiers: RawModifiers,
        _layout: u32,
    ) {
    }
}

impl ShmHandler for WaylandState {
    fn shm_state(&mut self) -> &mut Shm {
        self.context_menu.shm()
    }
}

impl PopupHandler for WaylandState {
    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        popup: &Popup,
        configure: PopupConfigure,
    ) {
        self.context_menu.configure(popup, configure);
    }

    fn done(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, popup: &Popup) {
        self.context_menu.popup_done(popup);
    }
}

struct RelativeManagerData;
struct RelativePointerData(wl_pointer::WlPointer);
impl Dispatch2<ZwpRelativePointerManagerV1, WaylandState> for RelativeManagerData {
    fn event(
        &self,
        _: &mut WaylandState,
        _: &ZwpRelativePointerManagerV1,
        _: <ZwpRelativePointerManagerV1 as Proxy>::Event,
        _: &Connection,
        _: &QueueHandle<WaylandState>,
    ) {
    }
}
impl Dispatch2<ZwpRelativePointerV1, WaylandState> for RelativePointerData {
    fn event(
        &self,
        state: &mut WaylandState,
        _: &ZwpRelativePointerV1,
        event: <ZwpRelativePointerV1 as Proxy>::Event,
        _: &Connection,
        _: &QueueHandle<WaylandState>,
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
smithay_client_toolkit::delegate_dispatch2!(WaylandState);
delegate_registry!(WaylandState);
impl ProvidesRegistryState for WaylandState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}

pub(super) struct WaylandWindow {
    pub(super) target: Arc<WindowTarget>,
    event_loop: EventLoop<'static, WaylandState>,
    qh: QueueHandle<WaylandState>,
    state: WaylandState,
}
impl WaylandWindow {
    pub(super) fn available() -> Result<bool, OverlayError> {
        let connection = Connection::connect_to_env().map_err(err)?;
        let (globals, queue) = registry_queue_init::<WaylandState>(&connection).map_err(err)?;
        match LayerShell::bind(&globals, &queue.handle()) {
            Ok(_) => Ok(true),
            Err(BindError::NotPresent) => Ok(false),
            Err(error) => Err(err(error)),
        }
    }
    pub(super) fn create(
        width: u32,
        height: u32,
        use_layer: bool,
        bounds: Option<OverlayWindowBounds>,
        keep_inside: bool,
    ) -> Result<Self, OverlayError> {
        let connection = Connection::connect_to_env().map_err(err)?;
        let (globals, queue) = registry_queue_init::<WaylandState>(&connection).map_err(err)?;
        let qh = queue.handle();
        let xdg_shell = XdgShell::bind(&globals, &qh).map_err(err)?;
        let compositor = Arc::new(CompositorState::bind(&globals, &qh).map_err(err)?);
        let surface = compositor.create_surface(&qh);
        let role = if use_layer {
            let shell = LayerShell::bind(&globals, &qh).map_err(err)?;
            let layer = shell.create_layer_surface(
                &qh,
                surface,
                Layer::Top,
                Some(bongocat_config::BUNDLE_ID),
                None,
            );
            layer.set_anchor(Anchor::TOP | Anchor::LEFT);
            layer.set_exclusive_zone(-1);
            layer.set_keyboard_interactivity(KeyboardInteractivity::None);
            layer.set_size(width, height);
            WindowRole::Layer(layer)
        } else {
            let window = xdg_shell.create_window(surface, WindowDecorations::None, &qh);
            window.set_title("BongoCat");
            window.set_app_id(bongocat_config::BUNDLE_ID);
            window.set_window_geometry(0, 0, width, height);
            WindowRole::Ordinary(window)
        };
        let target = Arc::new(WindowTarget {
            connection: connection.clone(),
            role,
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
        let mut state = WaylandState {
            registry: RegistryState::new(&globals),
            outputs: OutputState::new(&globals, &qh),
            seats: SeatState::new(&globals, &qh),
            target: target.clone(),
            xdg_shell,
            context_menu: ContextMenuState::new(Shm::bind(&globals, &qh).map_err(err)?)?,
            keyboards: Vec::new(),
            right_press: None,
            keyboard_interactive: false,
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
                .ok_or_else(|| err("compositor did not configure the Wayland surface"))?;
            event_loop
                .dispatch(remaining.min(Duration::from_millis(50)), &mut state)
                .map_err(err)?;
        }
        if state.closed {
            return Err(err("compositor closed the Wayland surface during creation"));
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
        self.state.context_menu.finish_dispatch();
        if let Some(trigger) = self.state.context_menu.take_submenu_trigger() {
            self.open_context_submenu(trigger)?;
        }
        self.state.sync_keyboard_interactivity();
        if let Some(error) = self.state.context_menu.take_error() {
            return Err(error);
        }
        Ok((self.state.closed, std::mem::take(&mut self.state.inputs)))
    }
    pub(super) fn presented(&mut self) {
        if !self.state.frame_pending {
            self.target.role.surface().frame(
                &self.qh,
                FrameCallbackData(self.target.role.surface().clone()),
            );
            self.target.role.surface().commit();
            self.state.frame_pending = true;
        }
    }
    pub(super) fn cancel_move(&mut self) {
        self.state.left_drag = None;
        self.state.right_press = None;
        self.state.context_menu.dismiss();
        self.state.sync_keyboard_interactivity();
    }
    pub(super) fn dismiss_context_menu(&mut self) {
        self.state.context_menu.dismiss();
        self.state.sync_keyboard_interactivity();
    }
    pub(super) fn reveal_context_menu(&mut self) -> Result<(), OverlayError> {
        if self.state.context_menu.has_popup() {
            self.state.context_menu.reveal()?;
        }
        Ok(())
    }
    pub(super) fn is_layer(&self) -> bool {
        matches!(self.target.role, WindowRole::Layer(_))
    }
    pub(super) fn set_presentation(
        &mut self,
        presentation: SystemMenuPresentation,
        palette: SystemMenuPalette,
    ) {
        self.state
            .context_menu
            .set_presentation(presentation, palette);
        self.state.sync_keyboard_interactivity();
    }
    pub(super) fn take_menu_action(&mut self) -> Option<SystemMenuAction> {
        self.state.context_menu.take_action()
    }
    pub(super) fn open_context_menu(&mut self, trigger: PopupTrigger) -> Result<(), OverlayError> {
        if !self.state.context_menu.has_presentation() {
            return Ok(());
        }
        self.dismiss_context_menu();
        let size = self.target.geometry.lock().unwrap().size;
        let positioner =
            self.state
                .context_menu
                .positioner(&self.state.xdg_shell, size, trigger.position)?;
        let parent = match &self.target.role {
            WindowRole::Ordinary(window) => Some(window.xdg_surface()),
            WindowRole::Layer(_) => None,
        };
        let popup = Popup::from_surface(
            parent,
            &positioner,
            &self.qh,
            self.target.compositor.create_surface(&self.qh),
            &self.state.xdg_shell,
        )
        .map_err(err)?;
        if let WindowRole::Layer(layer) = &self.target.role {
            layer.get_popup(popup.xdg_popup());
        }
        popup.xdg_popup().grab(&trigger.seat, trigger.serial);
        self.state.context_menu.install_popup(
            popup.clone(),
            self.target.scale.load(Ordering::Relaxed),
            trigger.seat,
            trigger.serial,
        );
        self.state.sync_keyboard_interactivity();
        popup.wl_surface().commit();
        Ok(())
    }
    fn open_context_submenu(&mut self, trigger: PopupTrigger) -> Result<(), OverlayError> {
        let Some(parent) = self.state.context_menu.root_popup() else {
            return Ok(());
        };
        let Some(positioner) = self
            .state
            .context_menu
            .submenu_positioner(&self.state.xdg_shell)?
        else {
            return Ok(());
        };
        let surface = self.target.compositor.create_surface(&self.qh);
        let popup = Popup::from_surface(
            Some(parent.xdg_surface()),
            &positioner,
            &self.qh,
            surface,
            &self.state.xdg_shell,
        )
        .map_err(|error| menu_error("create xdg context submenu", error))?;
        popup.xdg_popup().grab(&trigger.seat, trigger.serial);
        popup.wl_surface().commit();
        self.state
            .context_menu
            .install_submenu(popup, self.target.scale.load(Ordering::Relaxed));
        Ok(())
    }
}

impl Drop for WaylandWindow {
    fn drop(&mut self) {
        // Destroy grabbed children/root before the model surface and connection.
        self.state.context_menu.dismiss();
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

pub(super) fn menu_error(context: &str, error: impl std::fmt::Display) -> OverlayError {
    OverlayError::new(format!("{context}: {error}"))
}
