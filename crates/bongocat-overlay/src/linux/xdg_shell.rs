//! SCTK xdg-shell overlay used when layer-shell placement is not requested.

use super::*;
use raw_window_handle::{
    DisplayHandle, RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle,
    WindowHandle,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, Region},
    delegate_compositor, delegate_keyboard, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm, delegate_xdg_popup, delegate_xdg_shell, delegate_xdg_window,
    output::{OutputHandler, OutputState},
    reexports::{calloop::EventLoop as CalloopEventLoop, calloop_wayland_source::WaylandSource},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers},
        pointer::{BTN_LEFT, BTN_RIGHT, PointerEvent, PointerEventKind, PointerHandler},
    },
    shell::{
        WaylandSurface,
        xdg::{
            XdgShell, XdgSurface,
            popup::{Popup, PopupConfigure, PopupHandler},
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
        },
    },
    shm::{Shm, ShmHandler},
};
use std::{ptr::NonNull, sync::mpsc::SyncSender};
use wayland_client::{
    Connection, Proxy, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface},
};

const INITIAL_CONFIGURE_TIMEOUT: Duration = Duration::from_secs(2);

enum XdgPointerEvent {
    CancelGesture,
    RightPressed(PopupTrigger),
    RightMoved((f64, f64)),
    RightReleased(PopupTrigger),
}

struct PointerRegistration {
    seat: wl_seat::WlSeat,
    pointer: wl_pointer::WlPointer,
}

struct KeyboardRegistration {
    seat: wl_seat::WlSeat,
    keyboard: wl_keyboard::WlKeyboard,
    focused_surface: Option<wl_surface::WlSurface>,
}

struct XdgState {
    registry_state: RegistryState,
    compositor_state: CompositorState,
    output_state: OutputState,
    seat_state: SeatState,
    xdg_shell: XdgShell,
    window: Window,
    context_menu: ContextMenuState,
    pointers: Vec<PointerRegistration>,
    keyboards: Vec<KeyboardRegistration>,
    right_drag_pointer: Option<wl_pointer::WlPointer>,
    right_press: Option<PopupTrigger>,
    pointer_events: Vec<XdgPointerEvent>,
    configured: bool,
    close_requested: bool,
    scale_factor: u32,
}

impl XdgState {
    fn remove_pointer_for_seat(&mut self, seat: &wl_seat::WlSeat) {
        let Some(index) = self
            .pointers
            .iter()
            .position(|registration| &registration.seat == seat)
        else {
            return;
        };
        let registration = self.pointers.swap_remove(index);
        if self.right_drag_pointer.as_ref() == Some(&registration.pointer) {
            self.right_drag_pointer = None;
            self.right_press = None;
            self.pointer_events.push(XdgPointerEvent::CancelGesture);
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

impl CompositorHandler for XdgState {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        if surface == self.window.wl_surface() {
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
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
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

impl OutputHandler for XdgState {
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
        _output: wl_output::WlOutput,
    ) {
    }
}

impl WindowHandler for XdgState {
    fn request_close(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _window: &Window) {
        self.close_requested = true;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _window: &Window,
        _configure: WindowConfigure,
        _serial: u32,
    ) {
        self.configured = true;
    }
}

impl SeatHandler for XdgState {
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
            self.pointers.push(PointerRegistration {
                seat: seat.clone(),
                pointer,
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

impl PointerHandler for XdgState {
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
            if &event.surface != self.window.wl_surface() {
                continue;
            }
            match event.kind {
                PointerEventKind::Press {
                    button: BTN_LEFT,
                    serial,
                    ..
                } => {
                    self.context_menu.dismiss();
                    if let Some(seat) = self
                        .pointers
                        .iter()
                        .find(|registration| &registration.pointer == pointer)
                        .map(|registration| &registration.seat)
                    {
                        self.window.move_(seat, serial);
                    }
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
                    self.pointer_events
                        .push(XdgPointerEvent::RightPressed(trigger));
                }
                PointerEventKind::Release {
                    button: BTN_RIGHT, ..
                } if self.right_drag_pointer.as_ref() == Some(pointer) => {
                    self.right_drag_pointer = None;
                    self.pointer_events
                        .push(XdgPointerEvent::RightMoved(event.position));
                    if let Some(trigger) = self.right_press.take() {
                        self.pointer_events
                            .push(XdgPointerEvent::RightReleased(trigger));
                    }
                }
                _ if self.right_drag_pointer.as_ref() == Some(pointer) => self
                    .pointer_events
                    .push(XdgPointerEvent::RightMoved(event.position)),
                _ => {}
            }
        }
    }
}

impl KeyboardHandler for XdgState {
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

impl ShmHandler for XdgState {
    fn shm_state(&mut self) -> &mut Shm {
        self.context_menu.shm()
    }
}

impl PopupHandler for XdgState {
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

delegate_compositor!(XdgState);
delegate_keyboard!(XdgState);
delegate_output!(XdgState);
delegate_pointer!(XdgState);
delegate_registry!(XdgState);
delegate_seat!(XdgState);
delegate_shm!(XdgState);
delegate_xdg_popup!(XdgState);
delegate_xdg_shell!(XdgState);
delegate_xdg_window!(XdgState);

impl ProvidesRegistryState for XdgState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState, SeatState];
}

struct XdgGpuTarget {
    connection: Connection,
    window: Window,
}

impl HasDisplayHandle for XdgGpuTarget {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let pointer = NonNull::new(self.connection.backend().display_ptr().cast())
            .ok_or(HandleError::Unavailable)?;
        let handle = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(pointer));
        // SAFETY: `connection` owns this wl_display for at least the returned borrow.
        Ok(unsafe { DisplayHandle::borrow_raw(handle) })
    }
}

impl HasWindowHandle for XdgGpuTarget {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let pointer = NonNull::new(self.window.wl_surface().id().as_ptr().cast())
            .ok_or(HandleError::Unavailable)?;
        let handle = RawWindowHandle::Wayland(WaylandWindowHandle::new(pointer));
        // SAFETY: `window` owns this wl_surface for at least the returned borrow.
        Ok(unsafe { WindowHandle::borrow_raw(handle) })
    }
}

pub(super) struct SctkXdgOverlay {
    // The renderer must be dropped before the protocol state that owns its wl_surface.
    renderer: Renderer,
    target: Arc<XdgGpuTarget>,
    event_loop: CalloopEventLoop<'static, XdgState>,
    qh: QueueHandle<XdgState>,
    state: XdgState,
    presentation: OverlayPresentationState,
    logical_width: u32,
    logical_height: u32,
    applied_scale_factor: u32,
    visible: bool,
    applied_click_through: bool,
    resize_base: ResizeBase,
    resize_drag: Option<ResizeDrag>,
    resize_sender: Option<SyncSender<OverlayResizeOutcome>>,
}

impl SctkXdgOverlay {
    pub(super) fn create(
        frame: &RenderFrame,
        options: OverlaySessionOptions,
        bounds: Option<OverlayWindowBounds>,
        resize_sender: Option<SyncSender<OverlayResizeOutcome>>,
    ) -> Result<Self, OverlayError> {
        validate_options(options)?;
        let connection = Connection::connect_to_env()
            .map_err(|error| gpu_error("connect to Wayland compositor", error))?;
        let (globals, event_queue) = registry_queue_init(&connection)
            .map_err(|error| gpu_error("read Wayland globals", error))?;
        let qh = event_queue.handle();
        let compositor_state = CompositorState::bind(&globals, &qh)
            .map_err(|error| gpu_error("bind Wayland compositor", error))?;
        let xdg_shell = XdgShell::bind(&globals, &qh)
            .map_err(|error| gpu_error("bind Wayland xdg shell", error))?;
        let shm = Shm::bind(&globals, &qh)
            .map_err(|error| gpu_error("bind Wayland shared memory", error))?;
        let context_menu = ContextMenuState::new(shm)?;
        let surface = compositor_state.create_surface(&qh);
        let window = xdg_shell.create_window(surface, WindowDecorations::None, &qh);

        let (default_width, default_height) = if options.scale_percent == 100 {
            default_overlay_window_dimensions(frame.snapshot.canvas)
        } else {
            model_window_dimensions(frame.snapshot.canvas, options.scale_percent)
        };
        let (logical_width, logical_height) = bounds
            .map(|bounds| (bounds.width, bounds.height))
            .unwrap_or((default_width, default_height));
        window.set_title("BongoCat");
        window.set_app_id(WAYLAND_APPLICATION_ID);
        window.set_min_size(Some((logical_width, logical_height)));
        window.set_max_size(Some((logical_width, logical_height)));
        window
            .xdg_surface()
            .set_window_geometry(0, 0, logical_width as i32, logical_height as i32);
        apply_input_region(&compositor_state, &window, true)?;
        window.commit();

        let mut event_loop = CalloopEventLoop::try_new()
            .map_err(|error| gpu_error("create xdg-shell event loop", error))?;
        WaylandSource::new(connection.clone(), event_queue)
            .insert(event_loop.handle())
            .map_err(|error| gpu_error("attach xdg-shell event source", error))?;
        let mut state = XdgState {
            registry_state: RegistryState::new(&globals),
            compositor_state,
            output_state: OutputState::new(&globals, &qh),
            seat_state: SeatState::new(&globals, &qh),
            xdg_shell,
            window: window.clone(),
            context_menu,
            pointers: Vec::new(),
            keyboards: Vec::new(),
            right_drag_pointer: None,
            right_press: None,
            pointer_events: Vec::new(),
            configured: false,
            close_requested: false,
            scale_factor: 1,
        };
        let deadline = Instant::now() + INITIAL_CONFIGURE_TIMEOUT;
        while !state.configured && !state.close_requested {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return Err(OverlayError::new(
                    "Wayland compositor did not configure the xdg surface",
                ));
            };
            event_loop
                .dispatch(remaining.min(Duration::from_millis(50)), &mut state)
                .map_err(|error| gpu_error("wait for xdg-shell configure", error))?;
        }
        if state.close_requested {
            return Err(OverlayError::new(
                "Wayland compositor closed the xdg surface during startup",
            ));
        }

        let scale_factor = state.scale_factor.max(1);
        window
            .set_buffer_scale(scale_factor)
            .map_err(|_| OverlayError::new("Wayland surface does not support buffer scaling"))?;
        let target = Arc::new(XdgGpuTarget { connection, window });
        let mut renderer = Renderer::create(
            Arc::clone(&target),
            logical_width.saturating_mul(scale_factor).max(1),
            logical_height.saturating_mul(scale_factor).max(1),
            frame,
            options,
        )?;
        renderer.set_presentation_opacity(0.0);
        let resize_base = ResizeBase::new(
            f64::from(default_overlay_window_dimensions(frame.snapshot.canvas).0),
            f64::from(default_overlay_window_dimensions(frame.snapshot.canvas).1),
        )
        .ok_or_else(|| OverlayError::new("Wayland overlay has an invalid resize base"))?;
        Ok(Self {
            renderer,
            target,
            event_loop,
            qh,
            state,
            presentation: OverlayPresentationState::default(),
            logical_width,
            logical_height,
            applied_scale_factor: scale_factor,
            visible: false,
            applied_click_through: true,
            resize_base,
            resize_drag: None,
            resize_sender,
        })
    }

    pub(super) fn pump_events(&mut self) -> Result<bool, OverlayError> {
        self.event_loop
            .dispatch(Duration::ZERO, &mut self.state)
            .map_err(|error| gpu_error("pump xdg-shell events", error))?;
        self.state.context_menu.finish_dispatch();
        if let Some(trigger) = self.state.context_menu.take_submenu_trigger() {
            self.open_context_submenu(trigger)?;
        }
        if let Some(error) = self.state.context_menu.take_error() {
            return Err(error);
        }
        if self.state.scale_factor != self.applied_scale_factor {
            self.applied_scale_factor = self.state.scale_factor.max(1);
            self.state
                .window
                .set_buffer_scale(self.applied_scale_factor)
                .map_err(|_| {
                    OverlayError::new("Wayland surface does not support buffer scaling")
                })?;
            self.resize_renderer();
        }
        for event in std::mem::take(&mut self.state.pointer_events) {
            match event {
                XdgPointerEvent::CancelGesture => {
                    self.resize_drag = None;
                    self.state.context_menu.dismiss();
                }
                XdgPointerEvent::RightPressed(trigger) => {
                    self.open_context_menu(trigger.clone())?;
                    let scale = self.resize_base.scale_percent_for_width(self.logical_width);
                    self.resize_drag =
                        Some(ResizeDrag::begin(trigger.position, self.resize_base, scale));
                }
                XdgPointerEvent::RightMoved(position) => {
                    if let Some(resize) = self
                        .resize_drag
                        .as_mut()
                        .and_then(|drag| drag.observe(position))
                    {
                        self.state.context_menu.dismiss();
                        self.resize(OverlayWindowBounds::new(0, 0, resize.width, resize.height));
                    }
                }
                XdgPointerEvent::RightReleased(trigger) => {
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
        Ok(std::mem::take(&mut self.state.close_requested))
    }

    pub(super) fn draw(&mut self, verify: bool) -> Result<(), OverlayError> {
        self.renderer.draw(verify)?;
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
            self.renderer
                .set_presentation_opacity(f32::from(options.opacity_percent) / 100.0);
            self.set_click_through(options.click_through)?;
            self.visible = true;
        } else if self.visible {
            self.state.context_menu.dismiss();
            self.set_click_through(true)?;
            self.state.window.wl_surface().attach(None, 0, 0);
            self.state.window.commit();
            self.state.configured = false;
            self.visible = false;
        }
        Ok(())
    }

    pub(super) fn prepare_show(
        &mut self,
        options: OverlaySessionOptions,
    ) -> Result<(), OverlayError> {
        if !self.state.configured {
            self.state.window.commit();
            let deadline = Instant::now() + INITIAL_CONFIGURE_TIMEOUT;
            while !self.state.configured && !self.state.close_requested {
                let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                    return Err(OverlayError::new(
                        "Wayland compositor did not reconfigure the xdg surface",
                    ));
                };
                self.event_loop
                    .dispatch(remaining.min(Duration::from_millis(50)), &mut self.state)
                    .map_err(|error| gpu_error("wait for xdg-shell reconfigure", error))?;
            }
            if self.state.close_requested {
                return Err(OverlayError::new(
                    "Wayland compositor closed the xdg surface while remapping it",
                ));
            }
        }
        self.renderer
            .set_presentation_opacity(f32::from(options.opacity_percent) / 100.0);
        self.set_click_through(options.click_through)
    }

    pub(super) fn set_click_through(&mut self, click_through: bool) -> Result<(), OverlayError> {
        if click_through {
            self.state.context_menu.dismiss();
        }
        if click_through != self.applied_click_through {
            apply_input_region(
                &self.state.compositor_state,
                &self.state.window,
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
            .window
            .set_min_size(Some((self.logical_width, self.logical_height)));
        self.state
            .window
            .set_max_size(Some((self.logical_width, self.logical_height)));
        self.state.window.xdg_surface().set_window_geometry(
            0,
            0,
            self.logical_width as i32,
            self.logical_height as i32,
        );
        self.state.window.commit();
        self.resize_renderer();
    }

    pub(super) fn set_resize_base(&mut self, canvas: CanvasInfo) {
        let (width, height) = default_overlay_window_dimensions(canvas);
        if let Some(base) = ResizeBase::new(f64::from(width), f64::from(height)) {
            self.resize_base = base;
        }
    }

    pub(super) const fn bounds(&self) -> OverlayWindowBounds {
        OverlayWindowBounds::new(0, 0, self.logical_width, self.logical_height)
    }

    pub(super) fn renderer(&self) -> &Renderer {
        &self.renderer
    }

    pub(super) fn renderer_mut(&mut self) -> &mut Renderer {
        &mut self.renderer
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
    }

    pub(super) fn take_system_menu_action(
        &mut self,
    ) -> Option<bongocat_platform::SystemMenuAction> {
        self.state.context_menu.take_action()
    }

    pub(super) fn dismiss_context_menu(&mut self) {
        self.state.context_menu.dismiss();
    }

    fn resize_renderer(&mut self) {
        self.renderer.resize(
            self.logical_width.saturating_mul(self.applied_scale_factor),
            self.logical_height
                .saturating_mul(self.applied_scale_factor),
        );
    }

    fn open_context_menu(&mut self, trigger: PopupTrigger) -> Result<(), OverlayError> {
        if !self.state.context_menu.has_presentation() {
            return Ok(());
        }
        self.state.context_menu.dismiss();
        let positioner = self.state.context_menu.positioner(
            &self.state.xdg_shell,
            (self.logical_width, self.logical_height),
            trigger.position,
        )?;
        let surface = self.state.compositor_state.create_surface(&self.qh);
        let popup = Popup::from_surface(
            Some(self.state.window.xdg_surface()),
            &positioner,
            &self.qh,
            surface,
            &self.state.xdg_shell,
        )
        .map_err(|error| gpu_error("create xdg context menu", error))?;
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
        .map_err(|error| gpu_error("create xdg context submenu", error))?;
        popup.xdg_popup().grab(&trigger.seat, trigger.serial);
        popup.wl_surface().commit();
        self.state
            .context_menu
            .install_submenu(popup, self.applied_scale_factor);
        Ok(())
    }
}

fn apply_input_region(
    compositor: &CompositorState,
    window: &Window,
    click_through: bool,
) -> Result<(), OverlayError> {
    if click_through {
        let empty = Region::new(compositor)
            .map_err(|error| gpu_error("create Wayland input region", error))?;
        window.set_input_region(Some(empty.wl_region()));
    } else {
        window.set_input_region(None);
    }
    Ok(())
}
