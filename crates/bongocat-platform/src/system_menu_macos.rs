use crate::{SystemMenuAction, SystemMenuError, SystemMenuPresentation};
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained, runtime::AnyObject, sel,
};
use objc2_app_kit::{
    NSControlStateValueOff, NSControlStateValueOn, NSEvent, NSImage, NSMenu, NSMenuItem,
    NSStatusBar, NSStatusItem, NSVariableStatusItemLength,
};
use objc2_foundation::{NSData, NSObject, NSObjectProtocol, NSPoint, NSSize, NSString};
use std::sync::mpsc::{self, Receiver, Sender};

struct SystemMenuTargetIvars {
    sender: Sender<SystemMenuAction>,
}

define_class!(
    // SAFETY: The target remains retained by SystemMenu while AppKit holds weak references.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = SystemMenuTargetIvars]
    struct SystemMenuTarget;
    unsafe impl NSObjectProtocol for SystemMenuTarget {}
    impl SystemMenuTarget {
        #[unsafe(method(openSettings:))] fn open_settings(&self, _: Option<&AnyObject>) { let _ = self.ivars().sender.send(SystemMenuAction::OpenSettings); }
        #[unsafe(method(toggleOverlayVisibility:))] fn toggle_overlay_visibility(&self, _: Option<&AnyObject>) { let _ = self.ivars().sender.send(SystemMenuAction::ToggleOverlayVisibility); }
        #[unsafe(method(toggleClickThrough:))] fn toggle_click_through(&self, _: Option<&AnyObject>) { let _ = self.ivars().sender.send(SystemMenuAction::ToggleClickThrough); }
        #[unsafe(method(checkForUpdates:))] fn check_for_updates(&self, _: Option<&AnyObject>) { let _ = self.ivars().sender.send(SystemMenuAction::CheckForUpdates); }
        #[unsafe(method(openSource:))] fn open_source(&self, _: Option<&AnyObject>) { let _ = self.ivars().sender.send(SystemMenuAction::OpenSource); }
        #[unsafe(method(restart:))] fn restart(&self, _: Option<&AnyObject>) { let _ = self.ivars().sender.send(SystemMenuAction::Restart); }
        #[unsafe(method(quit:))] fn quit(&self, _: Option<&AnyObject>) { let _ = self.ivars().sender.send(SystemMenuAction::Quit); }
    }
);

impl SystemMenuTarget {
    fn new(mtm: MainThreadMarker, sender: Sender<SystemMenuAction>) -> Retained<Self> {
        let target = Self::alloc(mtm).set_ivars(SystemMenuTargetIvars { sender });
        // SAFETY: NSObject designated initializer has the generated signature.
        unsafe { msg_send![super(target), init] }
    }
}

pub struct SystemMenu {
    status_bar: Retained<NSStatusBar>,
    status_item: Option<Retained<NSStatusItem>>,
    status_image: Retained<NSImage>,
    menu: Retained<NSMenu>,
    target: Retained<SystemMenuTarget>,
    sender: Sender<SystemMenuAction>,
    receiver: Receiver<SystemMenuAction>,
    presentation: SystemMenuPresentation,
}

impl SystemMenu {
    pub fn start_with_presentation(
        visible: bool,
        presentation: SystemMenuPresentation,
    ) -> Result<Self, SystemMenuError> {
        let mtm = MainThreadMarker::new().ok_or(SystemMenuError::WrongThread)?;
        let (sender, receiver) = mpsc::channel();
        let target = SystemMenuTarget::new(mtm, sender.clone());
        let menu =
            NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(&presentation.title));
        let mut menu_owner = Self {
            status_bar: NSStatusBar::systemStatusBar(),
            status_item: None,
            status_image: status_image()?,
            menu,
            target,
            sender,
            receiver,
            presentation,
        };
        menu_owner.rebuild(mtm);
        if visible {
            menu_owner.install_status_item()?;
        }
        Ok(menu_owner)
    }

    pub fn set_presentation(
        &mut self,
        presentation: SystemMenuPresentation,
    ) -> Result<(), SystemMenuError> {
        let mtm = MainThreadMarker::new().ok_or(SystemMenuError::WrongThread)?;
        self.presentation = presentation;
        self.rebuild(mtm);
        if let Some(item) = &self.status_item
            && let Some(button) = item.button(mtm)
        {
            button.setToolTip(Some(&NSString::from_str(&self.presentation.tooltip)));
        }
        Ok(())
    }
    pub fn try_recv(&self) -> Option<SystemMenuAction> {
        self.receiver.try_recv().ok()
    }

    /// Present the same action menu used by the status item at the current pointer location.
    pub fn show_context_menu(&self) -> Result<(), SystemMenuError> {
        let _mtm = MainThreadMarker::new().ok_or(SystemMenuError::WrongThread)?;
        let location: NSPoint = NSEvent::mouseLocation();
        let _ = self
            .menu
            .popUpMenuPositioningItem_atLocation_inView(None, location, None);
        Ok(())
    }
    #[doc(hidden)]
    pub fn request_action_for_smoke(
        &self,
        action: SystemMenuAction,
    ) -> Result<(), SystemMenuError> {
        self.sender
            .send(action)
            .map_err(|_| SystemMenuError::EventQueueClosed)
    }

    pub fn is_visible(&self) -> bool {
        self.status_item.is_some()
    }
    pub fn set_visible(&mut self, visible: bool) -> Result<(), SystemMenuError> {
        if visible == self.is_visible() {
            Ok(())
        } else if visible {
            self.install_status_item()
        } else {
            self.remove_status_item();
            Ok(())
        }
    }
    pub fn shutdown(mut self) -> Result<(), SystemMenuError> {
        self.remove_status_item();
        Ok(())
    }

    fn rebuild(&self, mtm: MainThreadMarker) {
        self.menu.removeAllItems();
        self.menu
            .setTitle(&NSString::from_str(&self.presentation.title));
        self.action(
            mtm,
            &self.presentation.open_settings,
            Some(sel!(openSettings:)),
            false,
            true,
        );
        self.action(
            mtm,
            if self.presentation.overlay_visible {
                &self.presentation.hide_overlay
            } else {
                &self.presentation.show_overlay
            },
            Some(sel!(toggleOverlayVisibility:)),
            false,
            true,
        );
        self.menu.addItem(&NSMenuItem::separatorItem(mtm));
        self.action(
            mtm,
            &self.presentation.click_through,
            Some(sel!(toggleClickThrough:)),
            self.presentation.click_through_enabled,
            true,
        );
        self.menu.addItem(&NSMenuItem::separatorItem(mtm));
        self.action(
            mtm,
            &self.presentation.check_for_updates,
            Some(sel!(checkForUpdates:)),
            false,
            self.presentation.update_check_available,
        );
        self.action(
            mtm,
            &self.presentation.open_source,
            Some(sel!(openSource:)),
            false,
            true,
        );
        self.menu.addItem(&NSMenuItem::separatorItem(mtm));
        self.action(mtm, &self.presentation.version, None, false, false);
        self.action(
            mtm,
            &self.presentation.restart,
            Some(sel!(restart:)),
            false,
            true,
        );
        self.action(mtm, &self.presentation.quit, Some(sel!(quit:)), false, true);
    }

    fn action(
        &self,
        mtm: MainThreadMarker,
        title: &str,
        action: Option<objc2::runtime::Sel>,
        checked: bool,
        enabled: bool,
    ) {
        let empty = NSString::from_str("");
        // SAFETY: each selector passed here is implemented by SystemMenuTarget.
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(title),
                action,
                &empty,
            )
        };
        if action.is_some() {
            unsafe { item.setTarget(Some(&self.target)) };
        }
        item.setState(if checked {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
        item.setEnabled(enabled);
        self.menu.addItem(&item);
    }

    fn remove_status_item(&mut self) {
        if let Some(item) = self.status_item.take() {
            item.setMenu(None);
            self.status_bar.removeStatusItem(&item);
        }
    }
    fn install_status_item(&mut self) -> Result<(), SystemMenuError> {
        let mtm = MainThreadMarker::new().ok_or(SystemMenuError::WrongThread)?;
        let item = self
            .status_bar
            .statusItemWithLength(NSVariableStatusItemLength);
        let Some(button) = item.button(mtm) else {
            self.status_bar.removeStatusItem(&item);
            return Err(SystemMenuError::StatusItemCreateFailed);
        };
        button.setImage(Some(&self.status_image));
        button.setToolTip(Some(&NSString::from_str(&self.presentation.tooltip)));
        item.setMenu(Some(&self.menu));
        self.status_item = Some(item);
        Ok(())
    }
}

fn status_image() -> Result<Retained<NSImage>, SystemMenuError> {
    let data = NSData::with_bytes(include_bytes!("../../../resources/icons/tray-macos.png"));
    let image = NSImage::initWithData(NSImage::alloc(), &data)
        .ok_or(SystemMenuError::StatusIconImageLoadFailed)?;
    image.setTemplate(true);
    image.setSize(NSSize::new(18.0, 18.0));
    Ok(image)
}
impl Drop for SystemMenu {
    fn drop(&mut self) {
        self.remove_status_item();
    }
}
