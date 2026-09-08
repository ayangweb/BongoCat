use super::*;

impl SettingsView {
    pub(super) fn sync_shortcut_row_focus(
        &mut self,
        shortcuts: &SettingsShortcuts,
        active_model: Option<&SettingsModelKey>,
        entries: &[SettingsModelEntry],
        commands_blocked: bool,
        cx: &mut Context<Self>,
    ) {
        let rows = shortcut_rows(shortcuts, active_model, entries);
        let targets = rows
            .iter()
            .map(|row| row.target.clone())
            .collect::<Vec<_>>();
        let target_set = targets.iter().cloned().collect::<BTreeSet<_>>();
        self.shortcut_row_focus
            .retain(|target, _| target_set.contains(target));
        self.shortcut_clear_focus
            .retain(|target, _| target_set.contains(target));
        if self
            .shortcut_capture
            .as_ref()
            .is_some_and(|target| !target_set.contains(target))
        {
            self.shortcut_capture = None;
        }
        for (index, row) in rows.into_iter().enumerate() {
            let target = row.target;
            let tab_index = shortcut_capture_tab_index(index);
            let focus = self
                .shortcut_row_focus
                .entry(target.clone())
                .or_insert_with(|| cx.focus_handle());
            *focus = focus
                .clone()
                .tab_index(tab_index)
                .tab_stop(!commands_blocked);
            let clear_focus = self
                .shortcut_clear_focus
                .entry(target)
                .or_insert_with(|| cx.focus_handle());
            *clear_focus = clear_focus
                .clone()
                .tab_index(shortcut_clear_tab_index(index))
                .tab_stop(!commands_blocked && row.shortcut.is_some());
        }
    }

    pub(super) fn begin_shortcut_capture(
        &mut self,
        target: ShortcutCaptureTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.shortcut_commands_available() {
            return;
        }
        let Some(focus) = self.shortcut_row_focus.get(&target).cloned() else {
            return;
        };
        self.shortcut_capture = Some(target);
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(super) fn capture_shortcut(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self.shortcut_capture.clone() else {
            return;
        };
        if is_capture_cancel(event) {
            self.shortcut_capture = None;
            cx.notify();
            return;
        }
        let Some(shortcut) = shortcut_from_key_event(event) else {
            self.show_shortcut_capture_error(ShortcutCaptureError::UnsupportedKey, window, cx);
            cx.notify();
            return;
        };
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        let mut shortcuts = snapshot.shortcuts.clone();
        if !replace_shortcut(&mut shortcuts, &target, shortcut.clone()) {
            self.shortcut_capture = None;
            cx.notify();
            return;
        }
        if shortcut_conflicts(&shortcuts) {
            self.show_shortcut_capture_error(
                ShortcutCaptureError::AlreadyAssigned(shortcut),
                window,
                cx,
            );
            cx.notify();
            return;
        }
        let Some(expected_config_revision) = snapshot.config_revision else {
            return;
        };
        self.shortcut_capture = None;
        self.start_request(
            PendingOperation::SetShortcuts,
            Some(SettingValue::Shortcuts {
                expected_config_revision,
                shortcuts,
            }),
            cx,
        );
    }

    pub(super) fn clear_shortcut(&mut self, target: ShortcutCaptureTarget, cx: &mut Context<Self>) {
        if !self.shortcut_commands_available() {
            return;
        }
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        let mut shortcuts = snapshot.shortcuts.clone();
        if !clear_shortcut(&mut shortcuts, &target) {
            return;
        }
        let Some(expected_config_revision) = snapshot.config_revision else {
            return;
        };
        self.shortcut_capture = None;
        self.start_request(
            PendingOperation::SetShortcuts,
            Some(SettingValue::Shortcuts {
                expected_config_revision,
                shortcuts,
            }),
            cx,
        );
    }

    fn show_shortcut_capture_error(
        &self,
        error: ShortcutCaptureError,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let language = self
            .snapshot
            .as_ref()
            .map_or(SettingsLanguage::EnglishUnitedStates, |snapshot| {
                snapshot.resolved_language
            });
        window.push_notification(
            Notification::new()
                .id::<ShortcutCaptureNotification>()
                .message(shortcut_capture_error(language, &error))
                .with_type(NotificationType::Error),
            cx,
        );
    }
}
