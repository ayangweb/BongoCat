//! StatusNotifierItem/DBusMenu adapter. Application commands stay on the GUI side.
use crate::{SystemMenuAction, SystemMenuError, SystemMenuPresentation};
use ksni::blocking::TrayMethods;
use std::sync::mpsc::{self, Receiver, Sender};

pub struct LinuxSystemTray {
    handle: ksni::blocking::Handle<Tray>,
}
impl LinuxSystemTray {
    pub fn start(
        presentation: SystemMenuPresentation,
        visible: bool,
    ) -> Result<(Self, Receiver<SystemMenuAction>), SystemMenuError> {
        let (sender, receiver) = mpsc::channel();
        let image = image::load_from_memory(include_bytes!(
            "../../../../resources/icons/tray-windows.png"
        ))
        .map_err(|_| SystemMenuError::StatusIconImageLoadFailed)?
        .to_rgba8();
        let mut argb = image.as_raw().clone();
        for pixel in argb.as_chunks_mut::<4>().0 {
            pixel.rotate_right(1);
        }
        let icon = ksni::Icon {
            width: image.width() as i32,
            height: image.height() as i32,
            data: argb,
        };
        let handle = Tray {
            presentation,
            visible,
            sender,
            icon,
        }
        .spawn()
        .map_err(|_| SystemMenuError::StatusItemCreateFailed)?;
        Ok((Self { handle }, receiver))
    }
    pub fn set_visible(&self, visible: bool) -> Result<(), SystemMenuError> {
        self.handle
            .update(|tray| tray.visible = visible)
            .ok_or(SystemMenuError::StatusItemUpdateFailed)
    }
    pub fn set_presentation(
        &self,
        presentation: SystemMenuPresentation,
    ) -> Result<(), SystemMenuError> {
        self.handle
            .update(|tray| tray.presentation = presentation)
            .ok_or(SystemMenuError::StatusItemUpdateFailed)
    }
}
impl Drop for LinuxSystemTray {
    fn drop(&mut self) {
        self.handle.shutdown().wait();
    }
}
struct Tray {
    presentation: SystemMenuPresentation,
    visible: bool,
    sender: Sender<SystemMenuAction>,
    icon: ksni::Icon,
}
impl Tray {
    fn send(&self, action: SystemMenuAction) {
        let _ = self.sender.send(action);
    }
}
impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "com.ayangweb.bongo-cat".into()
    }
    fn title(&self) -> String {
        self.presentation.title.clone()
    }
    fn status(&self) -> ksni::Status {
        if self.visible {
            ksni::Status::Active
        } else {
            ksni::Status::Passive
        }
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![self.icon.clone()]
    }
    fn activate(&mut self, _: i32, _: i32) {
        self.send(SystemMenuAction::OpenSettings);
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{CheckmarkItem, StandardItem};
        self.presentation
            .linux_items()
            .into_iter()
            .map(|item| match item {
                crate::LinuxSystemMenuItem::Separator => ksni::MenuItem::Separator,
                crate::LinuxSystemMenuItem::Action {
                    action,
                    label,
                    checked,
                } => {
                    if matches!(
                        action,
                        SystemMenuAction::ToggleOverlayVisibility
                            | SystemMenuAction::ToggleClickThrough
                    ) {
                        CheckmarkItem {
                            label,
                            checked,
                            activate: Box::new(move |tray: &mut Self| tray.send(action)),
                            ..Default::default()
                        }
                        .into()
                    } else {
                        StandardItem {
                            label,
                            activate: Box::new(move |tray: &mut Self| tray.send(action)),
                            ..Default::default()
                        }
                        .into()
                    }
                }
            })
            .collect()
    }
}
