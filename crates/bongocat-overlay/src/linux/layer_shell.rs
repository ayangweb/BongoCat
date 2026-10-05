//! KDE/wlroots layer-shell surface used when the overlay must stay above normal windows.

use super::*;
use raw_window_handle::{
    DisplayHandle, RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle,
    WindowHandle,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer,
    delegate_registry, delegate_seat, delegate_shm, delegate_xdg_popup, delegate_xdg_shell,
    globals::GlobalData,
    output::{OutputData, OutputHandler, OutputInfo, OutputState},
    reexports::{
        calloop::EventLoop as CalloopEventLoop,
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
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers},
        pointer::{BTN_LEFT, BTN_RIGHT, PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure, SurfaceKind,
        },
        xdg::{
            XdgShell,
            popup::{Popup, PopupConfigure, PopupHandler},
            window::{Window, WindowConfigure, WindowHandler},
        },
    },
    shm::{Shm, ShmHandler},
};
use std::{num::NonZeroU32, ptr::NonNull};
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle,
    globals::{BindError, registry_queue_init},
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface},
};

const INITIAL_CONFIGURE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Default)]
struct LayerEvents {
    pointer_events: Vec<LayerPointerEvent>,
}

enum LayerPointerEvent {
    CancelGestures,
    LeftPressed((f64, f64)),
    LeftReleased((f64, f64)),
    RelativeMoved((f64, f64)),
    RightPressed(PopupTrigger),
    RightMoved((f64, f64)),
    RightReleased(PopupTrigger),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct LayerOutputSelector {
    registry_id: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct LayerRelocation {
    pub(super) output: LayerOutputSelector,
    pub(super) bounds: OverlayWindowBounds,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LogicalLayerOutput {
    selector: LayerOutputSelector,
    position: (i32, i32),
    size: (u32, u32),
}

struct LayerMoveDrag {
    press_position: (f64, f64),
    intended: (i32, i32),
    remainder: (f64, f64),
}

struct PointerRegistration {
    seat: wl_seat::WlSeat,
    pointer: wl_pointer::WlPointer,
    relative_pointer: Option<ZwpRelativePointerV1>,
}

struct KeyboardRegistration {
    seat: wl_seat::WlSeat,
    keyboard: wl_keyboard::WlKeyboard,
    focused_surface: Option<wl_surface::WlSurface>,
}

struct LayerState {
    registry_state: RegistryState,
    compositor_state: CompositorState,
    output_state: OutputState,
    seat_state: SeatState,
    xdg_shell: XdgShell,
    layer: LayerSurface,
    context_menu: ContextMenuState,
    relative_pointer_manager: Option<ZwpRelativePointerManagerV1>,
    pointers: Vec<PointerRegistration>,
    keyboards: Vec<KeyboardRegistration>,
    left_drag_pointer: Option<wl_pointer::WlPointer>,
    right_drag_pointer: Option<wl_pointer::WlPointer>,
    right_press: Option<PopupTrigger>,
    assigned_output: Option<wl_output::WlOutput>,
    configured: bool,
    closed: bool,
    scale_factor: u32,
    events: LayerEvents,
}

impl LayerState {
    fn output_logical_size(&self) -> Option<(u32, u32)> {
        self.assigned_output
            .as_ref()
            .and_then(|output| self.output_state.info(output))
            .and_then(logical_output_size)
    }

    fn logical_outputs(&self) -> Vec<LogicalLayerOutput> {
        self.output_state
            .outputs()
            .filter_map(|output| logical_layer_output(self.output_state.info(&output)?))
            .collect()
    }

    fn assigned_output_selector(&self) -> Option<LayerOutputSelector> {
        self.assigned_output.as_ref().and_then(output_selector)
    }

    fn remove_pointer_for_seat(&mut self, seat: &wl_seat::WlSeat) {
        let Some(index) = self
            .pointers
            .iter()
            .position(|registration| &registration.seat == seat)
        else {
            return;
        };
        let registration = self.pointers.swap_remove(index);
        if let Some(relative_pointer) = registration.relative_pointer {
            relative_pointer.destroy();
        }
        if self.left_drag_pointer.as_ref() == Some(&registration.pointer) {
            self.left_drag_pointer = None;
            self.events
                .pointer_events
                .push(LayerPointerEvent::CancelGestures);
        }
        if self.right_drag_pointer.as_ref() == Some(&registration.pointer) {
            self.right_drag_pointer = None;
            self.events
                .pointer_events
                .push(LayerPointerEvent::CancelGestures);
        }
        registration.pointer.release();
    }

    fn remove_keyboard_for_seat(&mut self, seat: &wl_seat::WlSeat) {
        let Some(index) = self
            .keyboards
            .iter()
            .position(|registration| &registration.seat == seat)
        else {
            return;
        };
        let registration = self.keyboards.swap_remove(index);
        if let Some(surface) = registration.focused_surface {
            self.context_menu
                .keyboard_leave(&surface, &registration.seat);
        }
        registration.keyboard.release();
    }
}

impl CompositorHandler for LayerState {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        if surface == self.layer.wl_surface() {
            self.scale_factor = u32::try_from(new_factor).unwrap_or(1).max(1);
        } else if self.context_menu.handles_surface(surface) {
            self.context_menu
                .set_scale_factor(surface, u32::try_from(new_factor).unwrap_or(1).max(1));
        }
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        output: &wl_output::WlOutput,
    ) {
        if surface == self.layer.wl_surface() && self.assigned_output.is_none() {
            self.assigned_output = Some(output.clone());
        }
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for LayerState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        if self.assigned_output.as_ref() == Some(&output) {
            self.assigned_output = None;
        }
    }
}

impl LayerShellHandler for LayerState {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.closed = true;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        _configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        self.configured = true;
    }
}

impl SeatHandler for LayerState {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer
            && !self
                .pointers
                .iter()
                .any(|registration| registration.seat == seat)
            && let Ok(pointer) = self.seat_state.get_pointer(qh, &seat)
        {
            let relative_pointer = self
                .relative_pointer_manager
                .as_ref()
                .map(|manager| manager.get_relative_pointer(&pointer, qh, pointer.clone()));
            self.pointers.push(PointerRegistration {
                seat: seat.clone(),
                pointer,
                relative_pointer,
            });
        }
        if capability == Capability::Keyboard
            && !self
                .keyboards
                .iter()
                .any(|registration| registration.seat == seat)
            && let Ok(keyboard) = self.seat_state.get_keyboard(qh, &seat, None)
        {
            self.keyboards.push(KeyboardRegistration {
                seat,
                keyboard,
                focused_surface: None,
            });
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            self.remove_pointer_for_seat(&seat);
        } else if capability == Capability::Keyboard {
            self.remove_keyboard_for_seat(&seat);
        }
    }

    fn remove_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        self.remove_pointer_for_seat(&seat);
        self.remove_keyboard_for_seat(&seat);
    }
}

impl PointerHandler for LayerState {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            if self.context_menu.handles_surface(&event.surface) {
                let seat = self
                    .pointers
                    .iter()
                    .find(|registration| &registration.pointer == pointer)
                    .map(|registration| registration.seat.clone());
                self.context_menu.pointer_event(event, seat.as_ref());
                continue;
            }
            if &event.surface != self.layer.wl_surface() {
                continue;
            }
            match event.kind {
                PointerEventKind::Press {
                    button: BTN_LEFT, ..
                } if self.left_drag_pointer.is_none() => {
                    self.context_menu.dismiss();
                    self.left_drag_pointer = Some(pointer.clone());
                    self.events
                        .pointer_events
                        .push(LayerPointerEvent::LeftPressed(event.position));
                }
                PointerEventKind::Release {
                    button: BTN_LEFT, ..
                } if self.left_drag_pointer.as_ref() == Some(pointer) => {
                    self.left_drag_pointer = None;
                    self.events
                        .pointer_events
                        .push(LayerPointerEvent::LeftReleased(event.position));
                }
                PointerEventKind::Press {
                    button: BTN_RIGHT,
                    serial,
                    ..
                } if self.right_drag_pointer.is_none() => {
                    let Some(seat) = self
                        .pointers
                        .iter()
                        .find(|registration| &registration.pointer == pointer)
                        .map(|registration| registration.seat.clone())
                    else {
                        continue;
                    };
                    let trigger = PopupTrigger {
                        seat,
                        serial,
                        position: event.position,
                    };
                    self.right_drag_pointer = Some(pointer.clone());
                    self.right_press = Some(trigger.clone());
                    self.events
                        .pointer_events
                        .push(LayerPointerEvent::RightPressed(trigger));
                }
                PointerEventKind::Release {
                    button: BTN_RIGHT, ..
                } if self.right_drag_pointer.as_ref() == Some(pointer) => {
                    self.right_drag_pointer = None;
                    self.events
                        .pointer_events
                        .push(LayerPointerEvent::RightMoved(event.position));
                    if let Some(trigger) = self.right_press.take() {
                        self.events
                            .pointer_events
                            .push(LayerPointerEvent::RightReleased(trigger));
                    }
                }
                _ if self.right_drag_pointer.as_ref() == Some(pointer) => {
                    self.events
                        .pointer_events
                        .push(LayerPointerEvent::RightMoved(event.position));
                }
                _ => {}
            }
        }
    }
}

impl KeyboardHandler for LayerState {
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
        _layout: u32,
    ) {
    }
}

impl ShmHandler for LayerState {
    fn shm_state(&mut self) -> &mut Shm {
        self.context_menu.shm()
    }
}

impl PopupHandler for LayerState {
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

impl WindowHandler for LayerState {
    fn request_close(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _window: &Window) {}

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _window: &Window,
        _configure: WindowConfigure,
        _serial: u32,
    ) {
    }
}

impl Dispatch<ZwpRelativePointerManagerV1, GlobalData> for LayerState {
    fn event(
        _state: &mut Self,
        _proxy: &ZwpRelativePointerManagerV1,
        _event: <ZwpRelativePointerManagerV1 as Proxy>::Event,
        _data: &GlobalData,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpRelativePointerV1, wl_pointer::WlPointer> for LayerState {
    fn event(
        state: &mut Self,
        _proxy: &ZwpRelativePointerV1,
        event: <ZwpRelativePointerV1 as Proxy>::Event,
        pointer: &wl_pointer::WlPointer,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if state.left_drag_pointer.as_ref() == Some(pointer)
            && let zwp_relative_pointer_v1::Event::RelativeMotion { dx, dy, .. } = event
        {
            state
                .events
                .pointer_events
                .push(LayerPointerEvent::RelativeMoved((dx, dy)));
        }
    }
}

delegate_compositor!(LayerState);
delegate_keyboard!(LayerState);
delegate_layer!(LayerState);
delegate_output!(LayerState);
delegate_pointer!(LayerState);
delegate_registry!(LayerState);
delegate_seat!(LayerState);
delegate_shm!(LayerState);
delegate_xdg_popup!(LayerState);
delegate_xdg_shell!(LayerState);

impl ProvidesRegistryState for LayerState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState, SeatState];
}

struct WaylandGpuTarget {
    connection: Connection,
    layer: LayerSurface,
}

impl HasDisplayHandle for WaylandGpuTarget {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let pointer = NonNull::new(self.connection.backend().display_ptr().cast())
            .ok_or(HandleError::Unavailable)?;
        let handle = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(pointer));
        // SAFETY: `connection` owns this wl_display for at least the returned borrow.
        Ok(unsafe { DisplayHandle::borrow_raw(handle) })
    }
}

impl HasWindowHandle for WaylandGpuTarget {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let pointer = NonNull::new(self.layer.wl_surface().id().as_ptr().cast())
            .ok_or(HandleError::Unavailable)?;
        let handle = RawWindowHandle::Wayland(WaylandWindowHandle::new(pointer));
        // SAFETY: `layer` owns this wl_surface for at least the returned borrow.
        Ok(unsafe { WindowHandle::borrow_raw(handle) })
    }
}

pub(super) struct LayerOverlay {
    // The renderer must be dropped before the protocol state that owns its wl_surface.
    renderer: Renderer,
    target: Arc<WaylandGpuTarget>,
    event_loop: CalloopEventLoop<'static, LayerState>,
    qh: QueueHandle<LayerState>,
    state: LayerState,
    presentation: OverlayPresentationState,
    logical_width: u32,
    logical_height: u32,
    margin_x: i32,
    margin_y: i32,
    presented_margin: (i32, i32),
    applied_scale_factor: u32,
    visible: bool,
    applied_click_through: bool,
    context_menu_keyboard_interactive: bool,
    keep_inside_screen: bool,
    center_when_output_known: bool,
    resize_base: ResizeBase,
    resize_drag: Option<ResizeDrag>,
    move_drag: Option<LayerMoveDrag>,
    pending_relocation: Option<LayerRelocation>,
    resize_sender: Option<SyncSender<OverlayResizeOutcome>>,
}

impl LayerOverlay {
    pub(super) fn available() -> Result<bool, OverlayError> {
        let connection = Connection::connect_to_env()
            .map_err(|error| gpu_error("connect to Wayland compositor", error))?;
        let (globals, event_queue) = registry_queue_init::<LayerState>(&connection)
            .map_err(|error| gpu_error("read Wayland globals", error))?;
        let qh = event_queue.handle();
        match LayerShell::bind(&globals, &qh) {
            Ok(_) => Ok(true),
            Err(BindError::NotPresent) => Ok(false),
            Err(error) => Err(gpu_error("bind Wayland layer shell", error)),
        }
    }

    pub(super) fn create_optional(
        frame: &RenderFrame,
        options: OverlaySessionOptions,
        bounds: Option<OverlayWindowBounds>,
        resize_sender: Option<SyncSender<OverlayResizeOutcome>>,
        target_output: Option<LayerOutputSelector>,
    ) -> Result<Option<Self>, OverlayError> {
        let connection = Connection::connect_to_env()
            .map_err(|error| gpu_error("connect to Wayland compositor", error))?;
        let (globals, event_queue) = registry_queue_init(&connection)
            .map_err(|error| gpu_error("read Wayland globals", error))?;
        let qh = event_queue.handle();
        let layer_shell = match LayerShell::bind(&globals, &qh) {
            Ok(layer_shell) => layer_shell,
            Err(BindError::NotPresent) => return Ok(None),
            Err(error) => return Err(gpu_error("bind Wayland layer shell", error)),
        };
        let compositor_state = CompositorState::bind(&globals, &qh)
            .map_err(|error| gpu_error("bind Wayland compositor", error))?;
        let xdg_shell = XdgShell::bind(&globals, &qh)
            .map_err(|error| gpu_error("bind Wayland xdg shell", error))?;
        let shm = Shm::bind(&globals, &qh)
            .map_err(|error| gpu_error("bind Wayland shared memory", error))?;
        let context_menu = ContextMenuState::new(shm)?;
        let registry_state = RegistryState::new(&globals);
        let output_state = OutputState::new(&globals, &qh);
        let selected_output = target_output.and_then(|selector| {
            output_state
                .outputs()
                .find(|output| output_selector(output) == Some(selector))
        });
        if target_output.is_some() && selected_output.is_none() {
            return Ok(None);
        }
        let seat_state = SeatState::new(&globals, &qh);
        let relative_pointer_manager = globals.bind(&qh, 1..=1, GlobalData).ok();
        let surface = compositor_state.create_surface(&qh);
        let layer = layer_shell.create_layer_surface(
            &qh,
            surface,
            Layer::Top,
            Some(WAYLAND_APPLICATION_ID),
            selected_output.as_ref(),
        );

        let (default_width, default_height) = if options.scale_percent == 100 {
            default_overlay_window_dimensions(frame.snapshot.canvas)
        } else {
            model_window_dimensions(frame.snapshot.canvas, options.scale_percent)
        };
        let (logical_width, logical_height) = bounds
            .map(|bounds| (bounds.width, bounds.height))
            .unwrap_or((default_width, default_height));
        layer.set_anchor(Anchor::TOP | Anchor::LEFT);
        layer.set_size(logical_width, logical_height);
        // Ignore panels' exclusive zones so output-relative bounds describe the
        // full display, matching the cross-platform keep-inside-screen contract.
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.set_margin(0, 0, 0, 0);
        apply_input_region(&compositor_state, &layer, true)?;
        layer.commit();

        let mut event_loop = CalloopEventLoop::try_new()
            .map_err(|error| gpu_error("create layer-shell event loop", error))?;
        WaylandSource::new(connection.clone(), event_queue)
            .insert(event_loop.handle())
            .map_err(|error| gpu_error("attach layer-shell event source", error))?;
        let mut state = LayerState {
            registry_state,
            compositor_state,
            output_state,
            seat_state,
            xdg_shell,
            layer: layer.clone(),
            context_menu,
            relative_pointer_manager,
            pointers: Vec::new(),
            keyboards: Vec::new(),
            left_drag_pointer: None,
            right_drag_pointer: None,
            right_press: None,
            assigned_output: selected_output,
            configured: false,
            closed: false,
            scale_factor: 1,
            events: LayerEvents::default(),
        };
        let configure_deadline = Instant::now() + INITIAL_CONFIGURE_TIMEOUT;
        while !state.configured && !state.closed {
            let Some(remaining) = configure_deadline.checked_duration_since(Instant::now()) else {
                return Err(OverlayError::new(
                    "Wayland compositor did not configure the layer surface",
                ));
            };
            event_loop
                .dispatch(remaining.min(Duration::from_millis(50)), &mut state)
                .map_err(|error| gpu_error("wait for layer-shell configure", error))?;
        }
        if state.closed {
            return Err(OverlayError::new(
                "Wayland compositor closed the layer surface during startup",
            ));
        }

        let output_size = state.output_logical_size();
        let center_when_output_known = bounds.is_none() && output_size.is_none();
        let (margin_x, margin_y) = initial_margins(
            bounds,
            output_size,
            logical_width,
            logical_height,
            options.keep_inside_screen,
        );
        layer.set_margin(margin_y, 0, 0, margin_x);
        let scale_factor = state.scale_factor.max(1);
        layer
            .set_buffer_scale(scale_factor)
            .map_err(|_| OverlayError::new("Wayland surface does not support buffer scaling"))?;
        let target = Arc::new(WaylandGpuTarget { connection, layer });
        let physical_width = logical_width.saturating_mul(scale_factor).max(1);
        let physical_height = logical_height.saturating_mul(scale_factor).max(1);
        let mut renderer = Renderer::create(
            Arc::clone(&target),
            physical_width,
            physical_height,
            frame,
            options,
        )?;
        renderer.set_presentation_opacity(0.0);
        let resize_base = ResizeBase::new(
            f64::from(default_overlay_window_dimensions(frame.snapshot.canvas).0),
            f64::from(default_overlay_window_dimensions(frame.snapshot.canvas).1),
        )
        .ok_or_else(|| OverlayError::new("Wayland overlay has an invalid resize base"))?;
        Ok(Some(Self {
            renderer,
            target,
            event_loop,
            qh,
            state,
            presentation: OverlayPresentationState::default(),
            logical_width,
            logical_height,
            margin_x,
            margin_y,
            presented_margin: (margin_x, margin_y),
            applied_scale_factor: scale_factor,
            visible: false,
            applied_click_through: true,
            context_menu_keyboard_interactive: false,
            keep_inside_screen: options.keep_inside_screen,
            center_when_output_known,
            resize_base,
            resize_drag: None,
            move_drag: None,
            pending_relocation: None,
            resize_sender,
        }))
    }

    pub(super) fn pump_events(&mut self) -> Result<bool, OverlayError> {
        self.event_loop
            .dispatch(Duration::ZERO, &mut self.state)
            .map_err(|error| gpu_error("pump layer-shell events", error))?;
        self.state.context_menu.finish_dispatch();
        if let Some(trigger) = self.state.context_menu.take_submenu_trigger() {
            self.open_context_submenu(trigger)?;
        }
        self.sync_context_menu_keyboard_interactivity();
        if let Some(error) = self.state.context_menu.take_error() {
            return Err(error);
        }
        if self.state.closed {
            return Ok(true);
        }
        if self.state.scale_factor != self.applied_scale_factor {
            self.applied_scale_factor = self.state.scale_factor.max(1);
            self.state
                .layer
                .set_buffer_scale(self.applied_scale_factor)
                .map_err(|_| {
                    OverlayError::new("Wayland surface does not support buffer scaling")
                })?;
            self.resize_renderer();
        }
        if self.center_when_output_known
            && let Some(output_size) = self.state.output_logical_size()
        {
            (self.margin_x, self.margin_y) = initial_margins(
                None,
                Some(output_size),
                self.logical_width,
                self.logical_height,
                self.keep_inside_screen,
            );
            self.center_when_output_known = false;
            self.apply_margins();
        } else if self.clamp_to_output() {
            self.apply_margins();
        }

        for event in std::mem::take(&mut self.state.events.pointer_events) {
            match event {
                LayerPointerEvent::CancelGestures => {
                    self.move_drag = None;
                    self.resize_drag = None;
                    self.state.context_menu.dismiss();
                }
                LayerPointerEvent::LeftPressed(position) => {
                    self.move_drag = Some(LayerMoveDrag {
                        press_position: position,
                        intended: (self.margin_x, self.margin_y),
                        remainder: (0.0, 0.0),
                    });
                }
                LayerPointerEvent::RelativeMoved(delta) => {
                    let Some(drag) = self.move_drag.as_mut() else {
                        continue;
                    };
                    drag.remainder.0 += delta.0;
                    drag.remainder.1 += delta.1;
                    let delta_x = drag.remainder.0.round() as i32;
                    let delta_y = drag.remainder.1.round() as i32;
                    drag.remainder.0 -= f64::from(delta_x);
                    drag.remainder.1 -= f64::from(delta_y);
                    if (delta_x, delta_y) != (0, 0) {
                        drag.intended.0 = drag.intended.0.saturating_add(delta_x);
                        drag.intended.1 = drag.intended.1.saturating_add(delta_y);
                        (self.margin_x, self.margin_y) = clamp_margins_if_enabled(
                            drag.intended,
                            self.state.output_logical_size(),
                            self.logical_width,
                            self.logical_height,
                            self.keep_inside_screen,
                        );
                        self.apply_margins();
                    }
                }
                LayerPointerEvent::LeftReleased(position) => {
                    if let Some(mut drag) = self.move_drag.take() {
                        // The relative-pointer and wl_pointer streams are not
                        // ordered with respect to each other. Reconstruct the
                        // final intended origin from the last presented surface
                        // position and the release point so no final movement is
                        // lost when the relative event arrives late.
                        drag.intended = released_intended_margin(
                            self.presented_margin,
                            drag.press_position,
                            position,
                        );
                        (self.margin_x, self.margin_y) = clamp_margins_if_enabled(
                            drag.intended,
                            self.state.output_logical_size(),
                            self.logical_width,
                            self.logical_height,
                            self.keep_inside_screen,
                        );
                        self.apply_margins();
                        if let Some(source) = self.state.assigned_output_selector() {
                            self.pending_relocation = relocation_for_drag(
                                &self.state.logical_outputs(),
                                source,
                                drag.intended,
                                drag.press_position,
                                (self.logical_width, self.logical_height),
                                self.keep_inside_screen,
                            );
                        }
                    }
                }
                LayerPointerEvent::RightPressed(trigger) => {
                    self.open_context_menu(trigger.clone())?;
                    let scale = self.resize_base.scale_percent_for_width(self.logical_width);
                    self.resize_drag =
                        Some(ResizeDrag::begin(trigger.position, self.resize_base, scale));
                }
                LayerPointerEvent::RightMoved(position) => {
                    if let Some(resize) = self
                        .resize_drag
                        .as_mut()
                        .and_then(|drag| drag.observe(position))
                    {
                        self.state.context_menu.dismiss();
                        let bounds = OverlayWindowBounds::new(
                            self.margin_x,
                            self.margin_y,
                            resize.width,
                            resize.height,
                        );
                        debug_assert!(bounds_match_scale(
                            bounds,
                            self.resize_base,
                            resize.scale_percent
                        ));
                        self.resize(bounds);
                    }
                }
                LayerPointerEvent::RightReleased(trigger) => {
                    if let Some(drag) = self.resize_drag.take() {
                        if drag.dragging() {
                            if let Some(scale_percent) = drag.finish()
                                && let Some(sender) = &self.resize_sender
                            {
                                let _ = sender.try_send(OverlayResizeOutcome { scale_percent });
                            }
                        } else {
                            let _ = trigger;
                            self.state.context_menu.reveal()?;
                        }
                    }
                }
            }
        }
        self.sync_context_menu_keyboard_interactivity();
        Ok(false)
    }

    pub(super) fn draw(&mut self, verify: bool) -> Result<(), OverlayError> {
        self.renderer.draw(verify)?;
        self.presented_margin = (self.margin_x, self.margin_y);
        self.presentation.record_presented_frame();
        Ok(())
    }

    pub(super) fn set_visible(
        &mut self,
        visible: bool,
        options: OverlaySessionOptions,
    ) -> Result<(), OverlayError> {
        if visible {
            self.presentation.require_presented_frame()?;
            if self.center_when_output_known {
                self.renderer.set_presentation_opacity(0.0);
                self.set_click_through(true)?;
                self.visible = true;
                return Ok(());
            }
            self.renderer
                .set_presentation_opacity(f32::from(options.opacity_percent) / 100.0);
            self.set_click_through(options.click_through)?;
            self.visible = true;
        } else if self.visible {
            self.renderer.set_presentation_opacity(0.0);
            self.set_click_through(true)?;
            match self.draw(false) {
                Ok(()) => self.visible = false,
                Err(error) if error.is_temporary_presentation_unavailable() => return Ok(()),
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    pub(super) fn prepare_show(
        &mut self,
        options: OverlaySessionOptions,
    ) -> Result<(), OverlayError> {
        if self.center_when_output_known {
            self.renderer.set_presentation_opacity(0.0);
            self.set_click_through(true)?;
            return Ok(());
        }
        self.renderer
            .set_presentation_opacity(f32::from(options.opacity_percent) / 100.0);
        self.set_click_through(options.click_through)?;
        Ok(())
    }

    pub(super) fn set_click_through(&mut self, click_through: bool) -> Result<(), OverlayError> {
        if click_through {
            self.state.context_menu.dismiss();
            self.sync_context_menu_keyboard_interactivity();
        }
        if click_through != self.applied_click_through {
            apply_input_region(
                &self.state.compositor_state,
                &self.state.layer,
                click_through,
            )?;
            self.applied_click_through = click_through;
        }
        Ok(())
    }

    pub(super) fn resize(&mut self, bounds: OverlayWindowBounds) {
        self.logical_width = bounds.width;
        self.logical_height = bounds.height;
        self.state
            .layer
            .set_size(self.logical_width, self.logical_height);
        self.clamp_to_output();
        self.apply_margins();
        self.resize_renderer();
    }

    pub(super) fn set_resize_base(&mut self, canvas: CanvasInfo) {
        let (width, height) = default_overlay_window_dimensions(canvas);
        if let Some(base) = ResizeBase::new(f64::from(width), f64::from(height)) {
            self.resize_base = base;
        }
    }

    pub(super) const fn bounds(&self) -> OverlayWindowBounds {
        OverlayWindowBounds::new(
            self.margin_x,
            self.margin_y,
            self.logical_width,
            self.logical_height,
        )
    }

    pub(super) fn renderer(&self) -> &Renderer {
        &self.renderer
    }

    pub(super) fn renderer_mut(&mut self) -> &mut Renderer {
        &mut self.renderer
    }

    pub(super) fn set_keep_inside_screen(&mut self, keep_inside_screen: bool) {
        self.keep_inside_screen = keep_inside_screen;
        self.clamp_to_output();
        self.apply_margins();
    }

    pub(super) const fn resize_base(&self) -> ResizeBase {
        self.resize_base
    }

    pub(super) const fn is_visible(&self) -> bool {
        self.visible
    }

    pub(super) fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        self.target.window_handle()
    }

    pub(super) fn set_system_menu_presentation(
        &mut self,
        presentation: bongocat_platform::SystemMenuPresentation,
    ) {
        self.state.context_menu.set_presentation(presentation);
        self.sync_context_menu_keyboard_interactivity();
    }

    pub(super) fn take_system_menu_action(
        &mut self,
    ) -> Option<bongocat_platform::SystemMenuAction> {
        self.state.context_menu.take_action()
    }

    pub(super) fn take_relocation(&mut self) -> Option<LayerRelocation> {
        self.pending_relocation.take()
    }

    pub(super) fn dismiss_context_menu(&mut self) {
        self.state.context_menu.dismiss();
        self.sync_context_menu_keyboard_interactivity();
    }

    fn open_context_menu(&mut self, trigger: PopupTrigger) -> Result<(), OverlayError> {
        if !self.state.context_menu.has_presentation() {
            return Ok(());
        }
        let positioner = self.state.context_menu.positioner(
            &self.state.xdg_shell,
            (self.logical_width, self.logical_height),
            trigger.position,
        )?;
        self.state.context_menu.dismiss();
        self.set_context_menu_keyboard_interactivity(true);
        let surface = self.state.compositor_state.create_surface(&self.qh);
        let popup = match Popup::from_surface(
            None,
            &positioner,
            &self.qh,
            surface,
            &self.state.xdg_shell,
        ) {
            Ok(popup) => popup,
            Err(error) => {
                self.set_context_menu_keyboard_interactivity(false);
                return Err(gpu_error("create layer context menu", error));
            }
        };
        self.state.layer.get_popup(popup.xdg_popup());
        popup.xdg_popup().grab(&trigger.seat, trigger.serial);
        popup.wl_surface().commit();
        self.state.context_menu.install_popup(
            popup,
            self.applied_scale_factor,
            trigger.seat,
            trigger.serial,
        );
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
        let surface = self.state.compositor_state.create_surface(&self.qh);
        let popup = Popup::from_surface(
            Some(parent.xdg_surface()),
            &positioner,
            &self.qh,
            surface,
            &self.state.xdg_shell,
        )
        .map_err(|error| gpu_error("create layer context submenu", error))?;
        popup.xdg_popup().grab(&trigger.seat, trigger.serial);
        popup.wl_surface().commit();
        self.state
            .context_menu
            .install_submenu(popup, self.applied_scale_factor);
        Ok(())
    }

    fn sync_context_menu_keyboard_interactivity(&mut self) {
        self.set_context_menu_keyboard_interactivity(self.state.context_menu.has_popup());
    }

    fn set_context_menu_keyboard_interactivity(&mut self, interactive: bool) {
        if self.context_menu_keyboard_interactive == interactive {
            return;
        }
        let mode = if interactive {
            match self.state.layer.kind() {
                SurfaceKind::Wlr(surface) if surface.version() >= 4 => {
                    KeyboardInteractivity::OnDemand
                }
                _ => KeyboardInteractivity::Exclusive,
            }
        } else {
            KeyboardInteractivity::None
        };
        self.state.layer.set_keyboard_interactivity(mode);
        self.state.layer.commit();
        self.context_menu_keyboard_interactive = interactive;
    }

    fn resize_renderer(&mut self) {
        self.renderer.resize(
            self.logical_width.saturating_mul(self.applied_scale_factor),
            self.logical_height
                .saturating_mul(self.applied_scale_factor),
        );
    }

    fn clamp_to_output(&mut self) -> bool {
        if !self.keep_inside_screen {
            return false;
        }
        let next = clamp_margins(
            self.margin_x,
            self.margin_y,
            self.state.output_logical_size(),
            self.logical_width,
            self.logical_height,
        );
        let changed = next != (self.margin_x, self.margin_y);
        (self.margin_x, self.margin_y) = next;
        changed
    }

    fn apply_margins(&self) {
        self.state
            .layer
            .set_margin(self.margin_y, 0, 0, self.margin_x);
    }
}

fn apply_input_region(
    compositor: &CompositorState,
    layer: &LayerSurface,
    click_through: bool,
) -> Result<(), OverlayError> {
    if click_through {
        let empty = Region::new(compositor)
            .map_err(|error| gpu_error("create Wayland input region", error))?;
        layer.set_input_region(Some(empty.wl_region()));
    } else {
        layer.set_input_region(None);
    }
    Ok(())
}

fn logical_output_size(info: OutputInfo) -> Option<(u32, u32)> {
    info.logical_size
        .and_then(|(width, height)| Some((u32::try_from(width).ok()?, u32::try_from(height).ok()?)))
        .or_else(|| {
            let scale = u32::try_from(info.scale_factor).ok()?.max(1);
            let mode = info.modes.into_iter().find(|mode| mode.current)?;
            let (physical_width, physical_height) = if matches!(
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
                u32::try_from(physical_width).ok()? / scale,
                u32::try_from(physical_height).ok()? / scale,
            ))
        })
        .filter(|(width, height)| NonZeroU32::new(*width).is_some() && *height > 0)
}

fn output_selector(output: &wl_output::WlOutput) -> Option<LayerOutputSelector> {
    output.data::<OutputData>().map(|data| {
        data.with_output_info(|info| LayerOutputSelector {
            registry_id: info.id,
        })
    })
}

fn logical_layer_output(info: OutputInfo) -> Option<LogicalLayerOutput> {
    let position = info.logical_position?;
    let (width, height) = info.logical_size?;
    let size = (u32::try_from(width).ok()?, u32::try_from(height).ok()?);
    if size.0 == 0 || size.1 == 0 {
        return None;
    }
    Some(LogicalLayerOutput {
        selector: LayerOutputSelector {
            registry_id: info.id,
        },
        position,
        size,
    })
}

fn relocation_for_drag(
    outputs: &[LogicalLayerOutput],
    source_selector: LayerOutputSelector,
    intended_margin: (i32, i32),
    press_position: (f64, f64),
    overlay_size: (u32, u32),
    keep_inside_screen: bool,
) -> Option<LayerRelocation> {
    let source = outputs
        .iter()
        .find(|output| output.selector == source_selector)?;
    let global_origin = (
        i64::from(source.position.0) + i64::from(intended_margin.0),
        i64::from(source.position.1) + i64::from(intended_margin.1),
    );
    let release_point = (
        global_origin.0 as f64 + press_position.0,
        global_origin.1 as f64 + press_position.1,
    );
    if output_contains(source, release_point) {
        return None;
    }
    let target = outputs
        .iter()
        .filter(|output| output_contains(output, release_point))
        .min_by_key(|output| output.selector.registry_id)?;
    let target_margin = (
        saturating_i64_to_i32(global_origin.0 - i64::from(target.position.0)),
        saturating_i64_to_i32(global_origin.1 - i64::from(target.position.1)),
    );
    let target_margin = clamp_margins_if_enabled(
        target_margin,
        Some(target.size),
        overlay_size.0,
        overlay_size.1,
        keep_inside_screen,
    );
    Some(LayerRelocation {
        output: target.selector,
        bounds: OverlayWindowBounds::new(
            target_margin.0,
            target_margin.1,
            overlay_size.0,
            overlay_size.1,
        ),
    })
}

fn output_contains(output: &LogicalLayerOutput, point: (f64, f64)) -> bool {
    let left = f64::from(output.position.0);
    let top = f64::from(output.position.1);
    point.0 >= left
        && point.0 < left + f64::from(output.size.0)
        && point.1 >= top
        && point.1 < top + f64::from(output.size.1)
}

fn saturating_i64_to_i32(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(if value.is_negative() {
        i32::MIN
    } else {
        i32::MAX
    })
}

fn clamp_margins_if_enabled(
    margin: (i32, i32),
    output_size: Option<(u32, u32)>,
    width: u32,
    height: u32,
    keep_inside_screen: bool,
) -> (i32, i32) {
    if keep_inside_screen {
        clamp_margins(margin.0, margin.1, output_size, width, height)
    } else {
        margin
    }
}

fn released_intended_margin(
    presented_margin: (i32, i32),
    press_position: (f64, f64),
    release_position: (f64, f64),
) -> (i32, i32) {
    (
        presented_margin
            .0
            .saturating_add((release_position.0 - press_position.0).round() as i32),
        presented_margin
            .1
            .saturating_add((release_position.1 - press_position.1).round() as i32),
    )
}

fn initial_margins(
    bounds: Option<OverlayWindowBounds>,
    output_size: Option<(u32, u32)>,
    width: u32,
    height: u32,
    keep_inside_screen: bool,
) -> (i32, i32) {
    let (x, y) = bounds.map_or_else(
        || {
            output_size.map_or((0, 0), |(output_width, output_height)| {
                (
                    output_width.saturating_sub(width) as i32 / 2,
                    output_height.saturating_sub(height) as i32 / 2,
                )
            })
        },
        |bounds| (bounds.x, bounds.y),
    );
    if keep_inside_screen {
        clamp_margins(x, y, output_size, width, height)
    } else {
        (x, y)
    }
}

fn clamp_margins(
    x: i32,
    y: i32,
    output_size: Option<(u32, u32)>,
    width: u32,
    height: u32,
) -> (i32, i32) {
    let Some((output_width, output_height)) = output_size else {
        return (x.max(0), y.max(0));
    };
    let maximum_x = i32::try_from(output_width.saturating_sub(width)).unwrap_or(i32::MAX);
    let maximum_y = i32::try_from(output_height.saturating_sub(height)).unwrap_or(i32::MAX);
    (x.clamp(0, maximum_x), y.clamp(0, maximum_y))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(registry_id: u32, position: (i32, i32), size: (u32, u32)) -> LogicalLayerOutput {
        LogicalLayerOutput {
            selector: LayerOutputSelector { registry_id },
            position,
            size,
        }
    }

    #[test]
    fn a_new_layer_surface_is_centered_on_its_output() {
        assert_eq!(
            initial_margins(None, Some((1920, 1080)), 350, 200, true),
            (785, 440)
        );
    }

    #[test]
    fn persisted_layer_margins_are_clamped_without_resizing() {
        assert_eq!(
            initial_margins(
                Some(OverlayWindowBounds::new(1800, -50, 350, 200)),
                Some((1920, 1080)),
                350,
                200,
                true,
            ),
            (1570, 0)
        );
    }

    #[test]
    fn an_overlay_larger_than_the_output_stays_at_the_origin() {
        assert_eq!(clamp_margins(100, 100, Some((320, 180)), 350, 200), (0, 0));
    }

    #[test]
    fn disabled_screen_constraints_preserve_signed_margins() {
        assert_eq!(
            initial_margins(
                Some(OverlayWindowBounds::new(-20, -30, 350, 200)),
                Some((1920, 1080)),
                350,
                200,
                false,
            ),
            (-20, -30)
        );
    }

    #[test]
    fn a_drag_crossing_the_seam_moves_to_the_adjacent_output() {
        let outputs = [
            output(1, (0, 0), (1920, 1080)),
            output(2, (1920, 0), (1920, 1080)),
        ];
        assert_eq!(
            relocation_for_drag(
                &outputs,
                LayerOutputSelector { registry_id: 1 },
                (1870, 440),
                (175.0, 100.0),
                (350, 200),
                true,
            ),
            Some(LayerRelocation {
                output: LayerOutputSelector { registry_id: 2 },
                bounds: OverlayWindowBounds::new(0, 440, 350, 200),
            })
        );
    }

    #[test]
    fn unconstrained_cross_output_drag_preserves_the_intended_origin() {
        let outputs = [
            output(1, (0, 0), (1920, 1080)),
            output(2, (1920, 0), (2560, 1440)),
        ];
        assert_eq!(
            relocation_for_drag(
                &outputs,
                LayerOutputSelector { registry_id: 1 },
                (1870, 440),
                (175.0, 100.0),
                (350, 200),
                false,
            )
            .map(|relocation| relocation.bounds),
            Some(OverlayWindowBounds::new(-50, 440, 350, 200))
        );
    }

    #[test]
    fn a_drag_remaining_on_the_assigned_output_does_not_recreate_the_surface() {
        let outputs = [
            output(1, (0, 0), (1920, 1080)),
            output(2, (1920, 0), (1920, 1080)),
        ];
        assert_eq!(
            relocation_for_drag(
                &outputs,
                LayerOutputSelector { registry_id: 1 },
                (900, 440),
                (175.0, 100.0),
                (350, 200),
                true,
            ),
            None
        );
    }

    #[test]
    fn output_gaps_do_not_guess_a_relocation_target() {
        let outputs = [
            output(1, (0, 0), (1920, 1080)),
            output(2, (2020, 0), (1920, 1080)),
        ];
        assert_eq!(
            relocation_for_drag(
                &outputs,
                LayerOutputSelector { registry_id: 1 },
                (1800, 440),
                (175.0, 100.0),
                (350, 200),
                true,
            ),
            None
        );
    }

    #[test]
    fn release_position_recovers_motion_after_the_surface_reaches_an_edge() {
        assert_eq!(
            released_intended_margin((1570, 440), (175.0, 100.0), (475.0, 100.0)),
            (1870, 440)
        );
    }

    #[test]
    fn negative_output_positions_support_right_to_left_transfer() {
        let outputs = [
            output(1, (-1920, 0), (1920, 1080)),
            output(2, (0, 0), (1920, 1080)),
        ];
        assert_eq!(
            relocation_for_drag(
                &outputs,
                LayerOutputSelector { registry_id: 2 },
                (-300, 440),
                (175.0, 100.0),
                (350, 200),
                true,
            ),
            Some(LayerRelocation {
                output: LayerOutputSelector { registry_id: 1 },
                bounds: OverlayWindowBounds::new(1570, 440, 350, 200),
            })
        );
    }

    #[test]
    fn fractional_pointer_coordinates_do_not_round_across_the_seam() {
        let outputs = [
            output(1, (0, 0), (1920, 1080)),
            output(2, (1920, 0), (1920, 1080)),
        ];
        assert_eq!(
            relocation_for_drag(
                &outputs,
                LayerOutputSelector { registry_id: 1 },
                (1744, 440),
                (175.4, 100.0),
                (350, 200),
                true,
            ),
            None
        );
        assert!(
            relocation_for_drag(
                &outputs,
                LayerOutputSelector { registry_id: 1 },
                (1744, 440),
                (176.6, 100.0),
                (350, 200),
                true,
            )
            .is_some()
        );
    }
}
