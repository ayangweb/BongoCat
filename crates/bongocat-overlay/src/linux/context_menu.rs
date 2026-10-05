use super::*;
use bongocat_platform::{SystemMenuAction, SystemMenuPalette, SystemMenuPresentation};
use cosmic_text::{
    Align, Attrs, Buffer as TextBuffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache,
    Wrap,
};
use smithay_client_toolkit::{
    reexports::protocols::xdg::shell::client::xdg_positioner,
    seat::pointer::{PointerEvent, PointerEventKind},
    shell::xdg::{
        XdgPositioner, XdgShell,
        popup::{Popup, PopupConfigure},
    },
    shm::{
        Shm,
        slot::{Buffer, SlotPool},
    },
};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};
use wayland_client::{
    Proxy,
    protocol::{wl_seat, wl_shm},
};

const MIN_MENU_WIDTH: u32 = 112;
const ACTION_HEIGHT: u32 = 34;
const SEPARATOR_HEIGHT: u32 = 5;
const MENU_VERTICAL_PADDING: u32 = 2;
const SUBMENU_HOVER_DELAY: Duration = Duration::from_millis(180);
const HORIZONTAL_PADDING: u32 = 12;
const ROOT_LABEL_LEFT: u32 = 20;
const CHILD_CHECK_LEFT: u32 = 20;
const CHILD_LABEL_LEFT: u32 = 44;
const MENU_RIGHT_PADDING: u32 = 20;
const SUBMENU_ARROW_GAP: u32 = 12;
const FONT_SIZE: f32 = 14.0;
const LINE_HEIGHT: f32 = 20.0;

#[derive(Clone)]
pub(super) struct PopupTrigger {
    pub seat: wl_seat::WlSeat,
    pub serial: u32,
    pub position: (f64, f64),
}

#[derive(Clone)]
enum MenuRow {
    Separator,
    Submenu {
        label: String,
        enabled: bool,
    },
    Action {
        action: SystemMenuAction,
        label: String,
        enabled: bool,
        checked: Option<bool>,
    },
}

impl MenuRow {
    const fn height(&self) -> u32 {
        match self {
            Self::Separator => SEPARATOR_HEIGHT,
            Self::Submenu { .. } | Self::Action { .. } => ACTION_HEIGHT,
        }
    }

    const fn enabled(&self) -> bool {
        match self {
            Self::Submenu { enabled, .. } | Self::Action { enabled, .. } => *enabled,
            Self::Separator => false,
        }
    }

    const fn selectable(&self) -> bool {
        !matches!(self, Self::Separator) && self.enabled()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuLevel {
    Root,
    ModelWindow,
}

struct PopupMenu {
    buffer: Option<Buffer>,
    popup: Popup,
    rows: Vec<MenuRow>,
    hovered: Option<usize>,
    focused: Option<usize>,
    pressed: Option<usize>,
    scale_factor: u32,
    configured_size: (u32, u32),
    configured: bool,
    visible_requested: bool,
    keyboard_focused: bool,
    ignore_next_pointer_enter: bool,
}

impl PopupMenu {
    fn new(
        popup: Popup,
        rows: Vec<MenuRow>,
        width: u32,
        scale_factor: u32,
        visible_requested: bool,
        ignore_next_pointer_enter: bool,
    ) -> Self {
        let configured_size = menu_dimensions(width, &rows);
        Self {
            buffer: None,
            popup,
            rows,
            hovered: None,
            focused: None,
            pressed: None,
            scale_factor: scale_factor.max(1),
            configured_size,
            configured: false,
            visible_requested,
            keyboard_focused: false,
            ignore_next_pointer_enter,
        }
    }

    fn row_at(&self, y: f64) -> Option<usize> {
        if y < 0.0 {
            return None;
        }
        let mut top = f64::from(MENU_VERTICAL_PADDING);
        if y < top {
            return None;
        }
        for (index, row) in self.rows.iter().enumerate() {
            let bottom = top + f64::from(row.height());
            if y < bottom {
                return (!matches!(row, MenuRow::Separator)).then_some(index);
            }
            top = bottom;
        }
        None
    }

    fn first_selectable(&self) -> Option<usize> {
        self.rows.iter().position(MenuRow::selectable)
    }

    fn last_selectable(&self) -> Option<usize> {
        self.rows.iter().rposition(MenuRow::selectable)
    }

    fn move_focus(&mut self, forward: bool) {
        if self.rows.is_empty() {
            self.focused = None;
            return;
        }
        let start = self
            .focused
            .unwrap_or(if forward { self.rows.len() - 1 } else { 0 });
        for offset in 1..=self.rows.len() {
            let index = if forward {
                (start + offset) % self.rows.len()
            } else {
                (start + self.rows.len() - offset % self.rows.len()) % self.rows.len()
            };
            if self.rows[index].selectable() {
                self.focused = Some(index);
                return;
            }
        }
        self.focused = None;
    }

    fn normalize_focus(&mut self) {
        if self
            .focused
            .is_some_and(|index| self.rows.get(index).is_some_and(MenuRow::selectable))
        {
            return;
        }
        self.focused = None;
    }
}

pub(super) struct ContextMenuState {
    shm: Shm,
    pool: SlotPool,
    // Child must be destroyed before root to preserve the grabbed-popup stack.
    child: Option<PopupMenu>,
    root: Option<PopupMenu>,
    presentation: Option<SystemMenuPresentation>,
    actions: VecDeque<SystemMenuAction>,
    grab_seat: Option<wl_seat::WlSeat>,
    grab_serial: Option<u32>,
    submenu_trigger: Option<PopupTrigger>,
    submenu_hover_deadline: Option<Instant>,
    child_close_requested: bool,
    close_requested: bool,
    pending_error: Option<String>,
    text: Option<(FontSystem, SwashCache)>,
}

impl ContextMenuState {
    pub(super) fn new(shm: Shm) -> Result<Self, OverlayError> {
        let pool = SlotPool::new(4, &shm)
            .map_err(|error| gpu_error("create context-menu SHM pool", error))?;
        Ok(Self {
            shm,
            pool,
            child: None,
            root: None,
            presentation: None,
            actions: VecDeque::new(),
            grab_seat: None,
            grab_serial: None,
            submenu_trigger: None,
            submenu_hover_deadline: None,
            child_close_requested: false,
            close_requested: false,
            pending_error: None,
            text: None,
        })
    }

    pub(super) fn shm(&mut self) -> &mut Shm {
        &mut self.shm
    }

    pub(super) fn set_presentation(&mut self, presentation: SystemMenuPresentation) {
        if self.presentation.as_ref() == Some(&presentation) {
            return;
        }
        let next_root_rows = root_rows(&presentation);
        let next_child_rows = model_window_rows(&presentation);
        let root_dimensions = self.root.is_some().then(|| {
            let width = self.measured_menu_width(&next_root_rows);
            menu_dimensions(width, &next_root_rows)
        });
        let child_dimensions = self.child.is_some().then(|| {
            let width = self.measured_menu_width(&next_child_rows);
            menu_dimensions(width, &next_child_rows)
        });
        let geometry_changed = root_dimensions
            .zip(self.root.as_ref())
            .is_some_and(|(dimensions, root)| dimensions != root.configured_size)
            || child_dimensions
                .zip(self.child.as_ref())
                .is_some_and(|(dimensions, child)| dimensions != child.configured_size);
        self.presentation = Some(presentation);
        if geometry_changed {
            self.dismiss();
            return;
        }
        if let Some(root) = self.root.as_mut() {
            root.rows = next_root_rows;
            root.normalize_focus();
        }
        if let Some(child) = self.child.as_mut() {
            child.rows = next_child_rows;
            child.normalize_focus();
        }
        self.redraw();
    }

    pub(super) fn has_presentation(&self) -> bool {
        self.presentation.is_some()
    }

    pub(super) fn has_popup(&self) -> bool {
        self.root.is_some()
    }

    pub(super) fn positioner(
        &mut self,
        shell: &XdgShell,
        parent_size: (u32, u32),
        position: (f64, f64),
    ) -> Result<XdgPositioner, OverlayError> {
        let rows = self
            .presentation
            .as_ref()
            .map(root_rows)
            .unwrap_or_default();
        let width = self.measured_menu_width(&rows);
        let (width, height) = menu_dimensions(width, &rows);
        let parent_width = i32::try_from(parent_size.0.max(1)).unwrap_or(i32::MAX);
        let parent_height = i32::try_from(parent_size.1.max(1)).unwrap_or(i32::MAX);
        let x = (position.0.floor() as i32).clamp(0, parent_width.saturating_sub(1));
        let y = (position.1.floor() as i32).clamp(0, parent_height.saturating_sub(1));
        let positioner = XdgPositioner::new(shell)
            .map_err(|error| gpu_error("create context-menu positioner", error))?;
        positioner.set_size(width as i32, height as i32);
        positioner.set_anchor_rect(x, y, 1, 1);
        positioner.set_anchor(xdg_positioner::Anchor::BottomRight);
        positioner.set_gravity(xdg_positioner::Gravity::BottomRight);
        positioner.set_constraint_adjustment(
            xdg_positioner::ConstraintAdjustment::FlipX
                | xdg_positioner::ConstraintAdjustment::FlipY
                | xdg_positioner::ConstraintAdjustment::SlideX
                | xdg_positioner::ConstraintAdjustment::SlideY,
        );
        Ok(positioner)
    }

    pub(super) fn install_popup(
        &mut self,
        popup: Popup,
        scale_factor: u32,
        seat: wl_seat::WlSeat,
        serial: u32,
    ) {
        debug_assert!(self.root.is_none());
        let rows = self
            .presentation
            .as_ref()
            .map(root_rows)
            .unwrap_or_default();
        let width = self.measured_menu_width(&rows);
        self.grab_seat = Some(seat);
        self.grab_serial = Some(serial);
        self.root = Some(PopupMenu::new(
            popup,
            rows,
            width,
            scale_factor,
            false,
            true,
        ));
    }

    pub(super) fn install_submenu(&mut self, popup: Popup, scale_factor: u32) {
        self.dismiss_child();
        let rows = self
            .presentation
            .as_ref()
            .map(model_window_rows)
            .unwrap_or_default();
        let width = self.measured_menu_width(&rows);
        self.submenu_hover_deadline = None;
        self.child = Some(PopupMenu::new(
            popup,
            rows,
            width,
            scale_factor,
            true,
            false,
        ));
        self.redraw();
    }

    pub(super) fn configure(&mut self, popup: &Popup, configure: PopupConfigure) {
        let Some(menu) = self.menu_for_popup_mut(popup) else {
            return;
        };
        if configure.width > 0 && configure.height > 0 {
            let size = (configure.width as u32, configure.height as u32);
            if menu.configured_size != size {
                menu.configured_size = size;
                menu.buffer = None;
            }
        }
        popup.xdg_surface().set_window_geometry(
            0,
            0,
            menu.configured_size.0 as i32,
            menu.configured_size.1 as i32,
        );
        menu.configured = true;
        popup
            .wl_surface()
            .set_buffer_scale(menu.scale_factor as i32);
        self.redraw();
    }

    pub(super) fn reveal(&mut self) -> Result<(), OverlayError> {
        if let Some(root) = self.root.as_mut() {
            root.visible_requested = true;
        }
        self.draw_level(MenuLevel::Root)
    }

    pub(super) fn set_scale_factor(
        &mut self,
        surface: &wayland_client::protocol::wl_surface::WlSurface,
        scale_factor: u32,
    ) {
        let scale_factor = scale_factor.max(1);
        let Some(menu) = self.menu_for_surface_mut(surface) else {
            return;
        };
        if menu.scale_factor == scale_factor {
            return;
        }
        menu.scale_factor = scale_factor;
        menu.buffer = None;
        menu.popup
            .wl_surface()
            .set_buffer_scale(scale_factor as i32);
        self.redraw();
    }

    pub(super) fn popup_done(&mut self, popup: &Popup) {
        if self.child.as_ref().is_some_and(|menu| &menu.popup == popup) {
            self.dismiss_child();
        } else if self.root.as_ref().is_some_and(|menu| &menu.popup == popup) {
            self.dismiss();
        }
    }

    pub(super) fn handles_surface(
        &self,
        surface: &wayland_client::protocol::wl_surface::WlSurface,
    ) -> bool {
        self.root
            .as_ref()
            .is_some_and(|menu| menu.popup.wl_surface() == surface)
            || self
                .child
                .as_ref()
                .is_some_and(|menu| menu.popup.wl_surface() == surface)
    }

    pub(super) fn pointer_event(&mut self, event: &PointerEvent, seat: Option<&wl_seat::WlSeat>) {
        let Some(level) = self.level_for_surface(&event.surface) else {
            return;
        };
        let row = self
            .menu(level)
            .and_then(|menu| menu.row_at(event.position.1));
        match event.kind {
            PointerEventKind::Enter { .. } => {
                let ignore = self
                    .menu_mut(level)
                    .is_some_and(|menu| std::mem::take(&mut menu.ignore_next_pointer_enter));
                if !ignore {
                    self.update_pointer_hover(level, row);
                }
            }
            PointerEventKind::Motion { .. } => self.update_pointer_hover(level, row),
            PointerEventKind::Leave { .. } => {
                if let Some(menu) = self.menu_mut(level) {
                    menu.hovered = None;
                    menu.pressed = None;
                }
                if level == MenuLevel::Root {
                    self.submenu_hover_deadline = None;
                }
                self.redraw();
            }
            PointerEventKind::Press {
                button: smithay_client_toolkit::seat::pointer::BTN_LEFT,
                serial,
                ..
            } => {
                let enabled = row.filter(|index| self.row_enabled(level, *index));
                if level == MenuLevel::Root
                    && enabled.is_some_and(|index| self.row_is_submenu(level, index))
                    && self.child.is_none()
                    && let Some(seat) = seat
                    && self.grab_seat.as_ref() == Some(seat)
                {
                    self.submenu_hover_deadline = None;
                    self.submenu_trigger = Some(PopupTrigger {
                        seat: seat.clone(),
                        serial,
                        position: event.position,
                    });
                }
                if let Some(menu) = self.menu_mut(level) {
                    menu.pressed = enabled;
                    menu.focused = enabled;
                }
                self.redraw();
            }
            PointerEventKind::Release {
                button: smithay_client_toolkit::seat::pointer::BTN_LEFT,
                ..
            } => {
                let released = row.filter(|index| self.row_enabled(level, *index));
                let pressed = self.menu(level).and_then(|menu| menu.pressed);
                if released.is_some()
                    && released == pressed
                    && let Some(action) = released.and_then(|index| self.row_action(level, index))
                {
                    self.actions.push_back(action);
                    self.close_requested = true;
                }
                if let Some(menu) = self.menu_mut(level) {
                    menu.pressed = None;
                }
            }
            PointerEventKind::Press {
                button: smithay_client_toolkit::seat::pointer::BTN_RIGHT,
                ..
            } => {
                self.close_requested = true;
            }
            _ => {}
        }
    }

    pub(super) fn finish_dispatch(&mut self) {
        if std::mem::take(&mut self.close_requested) {
            self.dismiss();
        } else if std::mem::take(&mut self.child_close_requested) {
            self.dismiss_child();
        } else if self
            .submenu_hover_deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.submenu_hover_deadline = None;
            let submenu_hovered = self
                .root
                .as_ref()
                .and_then(|menu| menu.hovered)
                .is_some_and(|index| self.row_is_submenu(MenuLevel::Root, index));
            if self.child.is_none()
                && submenu_hovered
                && let (Some(seat), Some(serial)) = (self.grab_seat.as_ref(), self.grab_serial)
            {
                self.submenu_trigger = Some(PopupTrigger {
                    seat: seat.clone(),
                    serial,
                    position: (0.0, 0.0),
                });
            }
        }
    }

    pub(super) fn keyboard_enter(
        &mut self,
        surface: &wayland_client::protocol::wl_surface::WlSurface,
        seat: &wl_seat::WlSeat,
    ) {
        if self.grab_seat.as_ref() != Some(seat) {
            return;
        }
        let Some(level) = self.level_for_surface(surface) else {
            return;
        };
        if let Some(menu) = self.menu_mut(level) {
            menu.keyboard_focused = true;
        }
        self.redraw();
    }

    pub(super) fn keyboard_leave(
        &mut self,
        surface: &wayland_client::protocol::wl_surface::WlSurface,
        seat: &wl_seat::WlSeat,
    ) {
        if self.grab_seat.as_ref() != Some(seat) {
            return;
        }
        if let Some(level) = self.level_for_surface(surface)
            && let Some(menu) = self.menu_mut(level)
        {
            menu.keyboard_focused = false;
            self.redraw();
        }
    }

    pub(super) fn keyboard_key(
        &mut self,
        keysym: smithay_client_toolkit::seat::keyboard::Keysym,
        seat: &wl_seat::WlSeat,
        surface: &wayland_client::protocol::wl_surface::WlSurface,
        serial: u32,
    ) {
        use smithay_client_toolkit::seat::keyboard::Keysym;

        if self.grab_seat.as_ref() != Some(seat) {
            return;
        }
        let Some(level) = self.level_for_surface(surface) else {
            return;
        };

        match keysym {
            Keysym::Escape => self.close_requested = true,
            Keysym::Up => {
                if let Some(menu) = self.menu_mut(level) {
                    menu.move_focus(false);
                }
            }
            Keysym::Down => {
                if let Some(menu) = self.menu_mut(level) {
                    menu.move_focus(true);
                }
            }
            Keysym::Home => {
                if let Some(menu) = self.menu_mut(level) {
                    menu.focused = menu.first_selectable();
                }
            }
            Keysym::End => {
                if let Some(menu) = self.menu_mut(level) {
                    menu.focused = menu.last_selectable();
                }
            }
            Keysym::Left if level == MenuLevel::ModelWindow => {
                self.child_close_requested = true;
            }
            Keysym::Right if level == MenuLevel::Root => {
                self.request_focused_submenu(level, seat, serial);
            }
            Keysym::Return | Keysym::KP_Enter | Keysym::space => {
                let focused = self.menu(level).and_then(|menu| menu.focused);
                if let Some(index) = focused {
                    if self.row_is_submenu(level, index) {
                        self.request_focused_submenu(level, seat, serial);
                    } else if let Some(action) = self.row_action(level, index) {
                        self.actions.push_back(action);
                        self.close_requested = true;
                    }
                }
            }
            _ => {}
        }
        self.redraw();
    }

    pub(super) fn take_submenu_trigger(&mut self) -> Option<PopupTrigger> {
        self.submenu_trigger.take()
    }

    pub(super) fn root_popup(&self) -> Option<Popup> {
        self.root.as_ref().map(|menu| menu.popup.clone())
    }

    pub(super) fn submenu_positioner(
        &mut self,
        shell: &XdgShell,
    ) -> Result<Option<XdgPositioner>, OverlayError> {
        let (parent_width, top) = {
            let Some(root) = self.root.as_ref() else {
                return Ok(None);
            };
            let Some(index) = root
                .rows
                .iter()
                .position(|row| matches!(row, MenuRow::Submenu { .. }))
            else {
                return Ok(None);
            };
            (
                root.configured_size.0,
                MENU_VERTICAL_PADDING + root.rows[..index].iter().map(MenuRow::height).sum::<u32>(),
            )
        };
        let rows = self
            .presentation
            .as_ref()
            .map(model_window_rows)
            .unwrap_or_default();
        let width = self.measured_menu_width(&rows);
        let (width, height) = menu_dimensions(width, &rows);
        let positioner = XdgPositioner::new(shell)
            .map_err(|error| gpu_error("create context submenu positioner", error))?;
        positioner.set_size(width as i32, height as i32);
        positioner.set_anchor_rect(0, top as i32, parent_width as i32, ACTION_HEIGHT as i32);
        positioner.set_anchor(xdg_positioner::Anchor::TopRight);
        positioner.set_gravity(xdg_positioner::Gravity::BottomRight);
        positioner.set_constraint_adjustment(
            xdg_positioner::ConstraintAdjustment::FlipX
                | xdg_positioner::ConstraintAdjustment::FlipY
                | xdg_positioner::ConstraintAdjustment::SlideX
                | xdg_positioner::ConstraintAdjustment::SlideY,
        );
        Ok(Some(positioner))
    }

    pub(super) fn take_action(&mut self) -> Option<SystemMenuAction> {
        self.actions.pop_front()
    }

    pub(super) fn take_error(&mut self) -> Option<OverlayError> {
        self.pending_error.take().map(OverlayError::new)
    }

    pub(super) fn dismiss(&mut self) {
        self.child = None;
        self.root = None;
        self.grab_seat = None;
        self.grab_serial = None;
        self.submenu_trigger = None;
        self.submenu_hover_deadline = None;
        self.child_close_requested = false;
        self.close_requested = false;
    }

    fn dismiss_child(&mut self) {
        self.child = None;
        self.child_close_requested = false;
        self.redraw();
    }

    fn measured_menu_width(&mut self, rows: &[MenuRow]) -> u32 {
        let text = self
            .text
            .get_or_insert_with(|| (FontSystem::new(), SwashCache::new()));
        let arrow_width = measure_text_width(&mut text.0, ">").ceil() as u32;
        rows.iter()
            .filter_map(|row| {
                let (label, left, trailing) = match row {
                    MenuRow::Separator => return None,
                    MenuRow::Submenu { label, .. } => (
                        label.as_str(),
                        ROOT_LABEL_LEFT,
                        SUBMENU_ARROW_GAP + arrow_width + MENU_RIGHT_PADDING,
                    ),
                    MenuRow::Action { label, checked, .. } => (
                        label.as_str(),
                        if checked.is_some() {
                            CHILD_LABEL_LEFT
                        } else {
                            ROOT_LABEL_LEFT
                        },
                        MENU_RIGHT_PADDING,
                    ),
                };
                Some(
                    left.saturating_add(measure_text_width(&mut text.0, label).ceil() as u32)
                        .saturating_add(trailing),
                )
            })
            .max()
            .unwrap_or(MIN_MENU_WIDTH)
            .max(MIN_MENU_WIDTH)
    }

    fn update_pointer_hover(&mut self, level: MenuLevel, row: Option<usize>) {
        let hover_changed = self.menu(level).is_some_and(|menu| menu.hovered != row);
        if let Some(menu) = self.menu_mut(level)
            && hover_changed
        {
            menu.hovered = row;
        }
        if level == MenuLevel::Root {
            let submenu_hovered =
                row.is_some_and(|index| self.row_is_submenu(MenuLevel::Root, index));
            if submenu_hovered && self.child.is_none() {
                if hover_changed {
                    self.submenu_hover_deadline = Some(Instant::now() + SUBMENU_HOVER_DELAY);
                }
            } else {
                self.submenu_hover_deadline = None;
                if self.child.is_some() && !submenu_hovered {
                    self.child_close_requested = true;
                }
            }
        }
        if hover_changed {
            self.redraw();
        }
    }

    fn menu(&self, level: MenuLevel) -> Option<&PopupMenu> {
        match level {
            MenuLevel::Root => self.root.as_ref(),
            MenuLevel::ModelWindow => self.child.as_ref(),
        }
    }

    fn menu_mut(&mut self, level: MenuLevel) -> Option<&mut PopupMenu> {
        match level {
            MenuLevel::Root => self.root.as_mut(),
            MenuLevel::ModelWindow => self.child.as_mut(),
        }
    }

    fn level_for_surface(
        &self,
        surface: &wayland_client::protocol::wl_surface::WlSurface,
    ) -> Option<MenuLevel> {
        if self
            .child
            .as_ref()
            .is_some_and(|menu| menu.popup.wl_surface() == surface)
        {
            Some(MenuLevel::ModelWindow)
        } else if self
            .root
            .as_ref()
            .is_some_and(|menu| menu.popup.wl_surface() == surface)
        {
            Some(MenuLevel::Root)
        } else {
            None
        }
    }

    fn menu_for_surface_mut(
        &mut self,
        surface: &wayland_client::protocol::wl_surface::WlSurface,
    ) -> Option<&mut PopupMenu> {
        let level = self.level_for_surface(surface)?;
        self.menu_mut(level)
    }

    fn menu_for_popup_mut(&mut self, popup: &Popup) -> Option<&mut PopupMenu> {
        if self.child.as_ref().is_some_and(|menu| &menu.popup == popup) {
            self.child.as_mut()
        } else if self.root.as_ref().is_some_and(|menu| &menu.popup == popup) {
            self.root.as_mut()
        } else {
            None
        }
    }

    fn row_enabled(&self, level: MenuLevel, index: usize) -> bool {
        self.menu(level)
            .and_then(|menu| menu.rows.get(index))
            .is_some_and(MenuRow::enabled)
    }

    fn row_is_submenu(&self, level: MenuLevel, index: usize) -> bool {
        matches!(
            self.menu(level).and_then(|menu| menu.rows.get(index)),
            Some(MenuRow::Submenu { enabled: true, .. })
        )
    }

    fn row_action(&self, level: MenuLevel, index: usize) -> Option<SystemMenuAction> {
        match self.menu(level).and_then(|menu| menu.rows.get(index)) {
            Some(MenuRow::Action { action, .. }) => Some(*action),
            _ => None,
        }
    }

    fn request_focused_submenu(&mut self, level: MenuLevel, seat: &wl_seat::WlSeat, serial: u32) {
        let focused = self.menu(level).and_then(|menu| menu.focused);
        if self.child.is_none()
            && self.grab_seat.as_ref() == Some(seat)
            && focused.is_some_and(|index| self.row_is_submenu(level, index))
        {
            self.submenu_hover_deadline = None;
            self.submenu_trigger = Some(PopupTrigger {
                seat: seat.clone(),
                serial,
                position: (0.0, 0.0),
            });
        }
    }

    fn redraw(&mut self) {
        for level in [MenuLevel::Root, MenuLevel::ModelWindow] {
            if let Err(error) = self.draw_level(level) {
                self.pending_error = Some(error.to_string());
                break;
            }
        }
    }

    fn draw_level(&mut self, level: MenuLevel) -> Result<(), OverlayError> {
        let palette = self
            .presentation
            .as_ref()
            .map(|presentation| presentation.palette)
            .ok_or_else(|| OverlayError::new("context menu has no presentation"))?;
        let child_open = self.child.is_some();
        let menu = match level {
            MenuLevel::Root => self.root.as_mut(),
            MenuLevel::ModelWindow => self.child.as_mut(),
        };
        let Some(menu) = menu else {
            return Ok(());
        };
        if !menu.configured || !menu.visible_requested {
            return Ok(());
        }
        draw_popup(
            &mut self.pool,
            &mut self.text,
            menu,
            palette,
            level == MenuLevel::Root && child_open,
        )
    }
}

impl Drop for ContextMenuState {
    fn drop(&mut self) {
        self.child = None;
        self.root = None;
    }
}

fn draw_popup(
    pool: &mut SlotPool,
    text: &mut Option<(FontSystem, SwashCache)>,
    menu: &mut PopupMenu,
    palette: SystemMenuPalette,
    submenu_open: bool,
) -> Result<(), OverlayError> {
    let scale = menu.scale_factor.max(1);
    let logical_width = menu.configured_size.0;
    let width = menu.configured_size.0.saturating_mul(scale).max(1);
    let height = menu.configured_size.1.saturating_mul(scale).max(1);
    let stride = width.saturating_mul(4);
    let required = usize::try_from(stride)
        .ok()
        .and_then(|stride| {
            usize::try_from(height)
                .ok()
                .and_then(|height| stride.checked_mul(height))
        })
        .ok_or_else(|| OverlayError::new("context menu is too large"))?;
    if pool.len() < required {
        pool.resize(required)
            .map_err(|error| gpu_error("resize context-menu SHM pool", error))?;
    }

    if menu.buffer.is_none() {
        let (buffer, _) = pool
            .create_buffer(
                width as i32,
                height as i32,
                stride as i32,
                wl_shm::Format::Argb8888,
            )
            .map_err(|error| gpu_error("create context-menu buffer", error))?;
        menu.buffer = Some(buffer);
    }
    let buffer = menu
        .buffer
        .as_mut()
        .expect("context-menu buffer was created");
    let canvas = if let Some(canvas) = pool.canvas(buffer) {
        canvas
    } else {
        let (next, canvas) = pool
            .create_buffer(
                width as i32,
                height as i32,
                stride as i32,
                wl_shm::Format::Argb8888,
            )
            .map_err(|error| gpu_error("double-buffer context menu", error))?;
        *buffer = next;
        canvas
    };

    fill(canvas, palette.surface);
    let mut top = MENU_VERTICAL_PADDING;
    let highlighted = menu
        .hovered
        .or_else(|| menu.keyboard_focused.then_some(menu.focused).flatten())
        .or_else(|| {
            submenu_open.then(|| {
                menu.rows
                    .iter()
                    .position(|row| matches!(row, MenuRow::Submenu { .. }))
            })?
        });
    for (index, row) in menu.rows.iter().enumerate() {
        let row_height = row.height();
        match row {
            MenuRow::Separator => {
                fill_rect(
                    canvas,
                    width,
                    HORIZONTAL_PADDING * scale,
                    (top + SEPARATOR_HEIGHT / 2) * scale,
                    logical_width.saturating_sub(HORIZONTAL_PADDING * 2) * scale,
                    scale.max(1),
                    palette.separator,
                );
            }
            MenuRow::Submenu { enabled, .. } | MenuRow::Action { enabled, .. } => {
                if highlighted == Some(index) && *enabled {
                    fill_rect(
                        canvas,
                        width,
                        4 * scale,
                        (top + 2) * scale,
                        logical_width.saturating_sub(8) * scale,
                        (row_height - 4) * scale,
                        palette.hover_background,
                    );
                }
            }
        }
        top = top.saturating_add(row_height);
    }

    let text = text.get_or_insert_with(|| (FontSystem::new(), SwashCache::new()));
    draw_labels(canvas, width, scale, &menu.rows, highlighted, palette, text);

    if menu.popup.wl_surface().version() >= 4 {
        menu.popup
            .wl_surface()
            .damage_buffer(0, 0, width as i32, height as i32);
    } else {
        menu.popup.wl_surface().damage(
            0,
            0,
            menu.configured_size.0 as i32,
            menu.configured_size.1 as i32,
        );
    }
    buffer
        .attach_to(menu.popup.wl_surface())
        .map_err(|error| gpu_error("attach context-menu buffer", error))?;
    menu.popup.wl_surface().commit();
    Ok(())
}

fn menu_dimensions(width: u32, rows: &[MenuRow]) -> (u32, u32) {
    (
        width,
        rows.iter()
            .map(MenuRow::height)
            .sum::<u32>()
            .saturating_add(MENU_VERTICAL_PADDING * 2)
            .max(1),
    )
}

fn measure_text_width(font_system: &mut FontSystem, label: &str) -> f32 {
    let mut buffer = TextBuffer::new(font_system, Metrics::new(FONT_SIZE, LINE_HEIGHT));
    buffer.set_wrap(Wrap::None);
    buffer.set_size(None, Some(LINE_HEIGHT));
    buffer.set_text(
        label,
        &Attrs::new().family(Family::SansSerif),
        Shaping::Advanced,
        Some(Align::Left),
    );
    buffer
        .line_layout(font_system, 0)
        .map(|lines| lines.iter().map(|line| line.w).fold(0.0_f32, f32::max))
        .unwrap_or(0.0)
}

fn root_rows(presentation: &SystemMenuPresentation) -> Vec<MenuRow> {
    let mut rows = vec![
        action(
            SystemMenuAction::OpenSettings,
            &presentation.open_settings,
            true,
            None,
        ),
        MenuRow::Separator,
        MenuRow::Submenu {
            label: presentation.model_window.clone(),
            enabled: true,
        },
        MenuRow::Separator,
    ];
    if presentation.update_check_available {
        rows.push(action(
            SystemMenuAction::CheckForUpdates,
            &presentation.check_for_updates,
            true,
            None,
        ));
        rows.push(MenuRow::Separator);
    }
    rows.push(action(
        SystemMenuAction::Quit,
        &presentation.quit,
        true,
        None,
    ));
    rows
}

fn model_window_rows(presentation: &SystemMenuPresentation) -> Vec<MenuRow> {
    vec![
        action(
            SystemMenuAction::ToggleOverlayVisibility,
            &presentation.hide_overlay,
            true,
            Some(!presentation.overlay_visible),
        ),
        action(
            SystemMenuAction::ToggleClickThrough,
            &presentation.click_through,
            true,
            Some(presentation.click_through_enabled),
        ),
        action(
            SystemMenuAction::ToggleAlwaysOnTop,
            &presentation.always_on_top,
            presentation.always_on_top_available,
            Some(presentation.always_on_top_available && presentation.always_on_top_enabled),
        ),
        action(
            SystemMenuAction::ToggleHideOnPointerHover,
            &presentation.hide_on_pointer_hover,
            presentation.hide_on_pointer_hover_available,
            Some(
                presentation.hide_on_pointer_hover_available
                    && presentation.hide_on_pointer_hover_enabled,
            ),
        ),
    ]
}

fn action(action: SystemMenuAction, label: &str, enabled: bool, checked: Option<bool>) -> MenuRow {
    MenuRow::Action {
        action,
        label: label.to_owned(),
        enabled,
        checked,
    }
}

fn draw_labels(
    canvas: &mut [u8],
    width: u32,
    scale: u32,
    rows: &[MenuRow],
    hovered: Option<usize>,
    palette: SystemMenuPalette,
    text: &mut (FontSystem, SwashCache),
) {
    let logical_width = width / scale.max(1);
    let arrow_width = measure_text_width(&mut text.0, ">").ceil() as u32;
    let mut top = MENU_VERTICAL_PADDING;
    for (index, row) in rows.iter().enumerate() {
        match row {
            MenuRow::Separator => {
                top = top.saturating_add(row.height());
                continue;
            }
            MenuRow::Submenu { label, enabled } => {
                let color = if !*enabled {
                    text_color(palette.muted_foreground)
                } else if hovered == Some(index) {
                    text_color(palette.hover_foreground)
                } else {
                    text_color(palette.foreground)
                };
                draw_text_line_with_left(
                    canvas,
                    width,
                    logical_width,
                    scale,
                    top,
                    row.height(),
                    label,
                    color,
                    ROOT_LABEL_LEFT,
                    SUBMENU_ARROW_GAP + arrow_width + MENU_RIGHT_PADDING,
                    text,
                );
                draw_text_line_with_left(
                    canvas,
                    width,
                    logical_width,
                    scale,
                    top,
                    row.height(),
                    ">",
                    color,
                    logical_width.saturating_sub(MENU_RIGHT_PADDING + arrow_width),
                    MENU_RIGHT_PADDING,
                    text,
                );
            }
            MenuRow::Action {
                label,
                enabled,
                checked,
                ..
            } => {
                let color = if !*enabled {
                    text_color(palette.muted_foreground)
                } else if hovered == Some(index) {
                    text_color(palette.hover_foreground)
                } else {
                    text_color(palette.foreground)
                };
                if checked == &Some(true) {
                    draw_text_line_with_left(
                        canvas,
                        width,
                        logical_width,
                        scale,
                        top,
                        row.height(),
                        "✓",
                        color,
                        CHILD_CHECK_LEFT,
                        MENU_RIGHT_PADDING,
                        text,
                    );
                }
                draw_text_line_with_left(
                    canvas,
                    width,
                    logical_width,
                    scale,
                    top,
                    row.height(),
                    label,
                    color,
                    if checked.is_some() {
                        CHILD_LABEL_LEFT
                    } else {
                        ROOT_LABEL_LEFT
                    },
                    MENU_RIGHT_PADDING,
                    text,
                );
                top = top.saturating_add(row.height());
                continue;
            }
        }
        top = top.saturating_add(row.height());
    }
}

const fn text_color(color: [u8; 4]) -> Color {
    Color::rgba(color[0], color[1], color[2], color[3])
}

#[allow(clippy::too_many_arguments)]
fn draw_text_line_with_left(
    canvas: &mut [u8],
    width: u32,
    logical_width: u32,
    scale: u32,
    top: u32,
    row_height: u32,
    label: &str,
    color: Color,
    left: u32,
    right: u32,
    text: &mut (FontSystem, SwashCache),
) {
    let font_size = FONT_SIZE * scale as f32;
    let line_height = LINE_HEIGHT * scale as f32;
    let mut buffer = TextBuffer::new(&mut text.0, Metrics::new(font_size, line_height));
    let mut borrowed = buffer.borrow_with(&mut text.0);
    borrowed.set_wrap(Wrap::None);
    borrowed.set_size(
        Some(logical_width.saturating_sub(left + right) as f32 * scale as f32),
        Some(row_height as f32 * scale as f32),
    );
    borrowed.set_text(
        label,
        &Attrs::new().family(Family::SansSerif),
        Shaping::Advanced,
        Some(Align::Left),
    );
    let x_offset = (left * scale) as i32;
    let y_offset = ((top * scale) as f32 + (row_height as f32 * scale as f32 - line_height) / 2.0)
        .round() as i32;
    borrowed.draw(
        &mut text.1,
        color,
        |x, y, glyph_width, glyph_height, glyph_color| {
            blend_rect(
                canvas,
                width,
                x + x_offset,
                y + y_offset,
                glyph_width,
                glyph_height,
                [
                    glyph_color.r(),
                    glyph_color.g(),
                    glyph_color.b(),
                    glyph_color.a(),
                ],
            );
        },
    );
}

fn fill(canvas: &mut [u8], color: [u8; 4]) {
    for pixel in canvas.chunks_exact_mut(4) {
        pixel.copy_from_slice(&[color[2], color[1], color[0], color[3]]);
    }
}

fn fill_rect(
    canvas: &mut [u8],
    canvas_width: u32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    color: [u8; 4],
) {
    let canvas_height = (canvas.len() / 4) as u32 / canvas_width.max(1);
    for row in y.min(canvas_height)..y.saturating_add(height).min(canvas_height) {
        for column in x.min(canvas_width)..x.saturating_add(width).min(canvas_width) {
            let offset = ((row * canvas_width + column) * 4) as usize;
            canvas[offset..offset + 4].copy_from_slice(&[color[2], color[1], color[0], color[3]]);
        }
    }
}

fn blend_rect(
    canvas: &mut [u8],
    canvas_width: u32,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    color: [u8; 4],
) {
    let canvas_height = (canvas.len() / 4) as u32 / canvas_width.max(1);
    for glyph_y in 0..height {
        let target_y = y.saturating_add(glyph_y as i32);
        if target_y < 0 || target_y >= canvas_height as i32 {
            continue;
        }
        for glyph_x in 0..width {
            let target_x = x.saturating_add(glyph_x as i32);
            if target_x < 0 || target_x >= canvas_width as i32 {
                continue;
            }
            let offset = ((target_y as u32 * canvas_width + target_x as u32) * 4) as usize;
            let alpha = u16::from(color[3]);
            for (channel, source) in [color[2], color[1], color[0]].into_iter().enumerate() {
                let destination = u16::from(canvas[offset + channel]);
                canvas[offset + channel] =
                    ((u16::from(source) * alpha + destination * (255 - alpha)) / 255) as u8;
            }
            canvas[offset + 3] = 255;
        }
    }
}
