use super::*;

impl SettingsView {
    pub(super) fn sync_shortcut_row_focus(
        &mut self,
        shortcuts: &SettingsShortcuts,
        commands_blocked: bool,
        cx: &mut Context<Self>,
    ) {
        let targets = shortcut_targets(shortcuts);
        let target_set = targets.iter().cloned().collect::<BTreeSet<_>>();
        self.shortcut_row_focus
            .retain(|target, _| target_set.contains(target));
        if self
            .shortcut_capture
            .as_ref()
            .is_some_and(|target| !target_set.contains(target))
        {
            self.shortcut_capture = None;
            self.shortcut_capture_error = None;
        }
        for (index, target) in targets.into_iter().enumerate() {
            let tab_index = shortcut_capture_tab_index(index);
            let focus = self
                .shortcut_row_focus
                .entry(target)
                .or_insert_with(|| cx.focus_handle());
            *focus = focus
                .clone()
                .tab_index(tab_index)
                .tab_stop(!commands_blocked);
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
        self.shortcut_capture_error = None;
        window.focus(&focus, cx);
        cx.notify();
    }

    pub(super) fn capture_shortcut(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(target) = self.shortcut_capture.clone() else {
            return;
        };
        if is_capture_cancel(event) {
            self.shortcut_capture = None;
            self.shortcut_capture_error = None;
            cx.notify();
            return;
        }
        let Some(shortcut) = shortcut_from_key_event(event) else {
            self.shortcut_capture_error = Some(ShortcutCaptureError::UnsupportedKey);
            cx.notify();
            return;
        };
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        let mut shortcuts = snapshot.shortcuts.clone();
        if !replace_shortcut(&mut shortcuts, &target, shortcut) {
            self.shortcut_capture = None;
            self.shortcut_capture_error = None;
            cx.notify();
            return;
        }
        if shortcut_conflicts(&shortcuts) {
            self.shortcut_capture_error = Some(ShortcutCaptureError::AlreadyAssigned);
            cx.notify();
            return;
        }
        let Some(expected_config_revision) = snapshot.config_revision else {
            return;
        };
        self.shortcut_capture = None;
        self.shortcut_capture_error = None;
        self.start_request(
            PendingOperation::SetShortcuts,
            Some(SettingValue::Shortcuts {
                expected_config_revision,
                shortcuts,
            }),
            cx,
        );
    }
}
