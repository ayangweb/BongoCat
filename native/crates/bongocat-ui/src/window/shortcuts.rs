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
            .is_some_and(|capture| !target_set.contains(&capture.target))
        {
            self.cancel_shortcut_capture(cx);
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
        if self.shortcut_capture.is_some() || !self.shortcut_commands_available() {
            return;
        }
        let Some(expected_config_revision) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.config_revision)
        else {
            return;
        };
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        let mut shortcuts_without_capture_target = snapshot.shortcuts.clone();
        clear_shortcut(&mut shortcuts_without_capture_target, &target);
        self.pending = Some(PendingOperation::BeginShortcutCapture);
        cx.notify();
        let client = self.client.clone();
        let resume_client = client.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = client
                .suspend_shortcut_capture(
                    expected_config_revision,
                    shortcuts_without_capture_target,
                )
                .await;
            let Some(view) = this.upgrade() else {
                if result.is_ok() {
                    let _ = resume_client.resume_shortcut_capture().await;
                }
                return;
            };
            let _ = cx.update_window_entity(&view, |view, window, cx| {
                if view.pending != Some(PendingOperation::BeginShortcutCapture) {
                    return;
                }
                view.pending = None;
                match result {
                    Ok(snapshot) => {
                        if accepts_snapshot_revision(
                            view.snapshot.as_ref().map(|current| current.revision),
                            snapshot.revision,
                        ) {
                            view.snapshot = Some(snapshot);
                        }
                        view.shortcut_capture = Some(ShortcutCapture::new(target.clone()));
                        let Some(focus) = view.shortcut_row_focus.get(&target).cloned() else {
                            view.cancel_shortcut_capture(cx);
                            return;
                        };
                        let blur_target = target.clone();
                        view.shortcut_capture_blur_subscription =
                            Some(cx.on_blur(&focus, window, move |view, _, cx| {
                                if view
                                    .shortcut_capture
                                    .as_ref()
                                    .is_some_and(|capture| capture.target == blur_target)
                                {
                                    view.cancel_shortcut_capture(cx);
                                }
                            }));
                        window.focus(&focus, cx);
                    }
                    Err(error) => view.pending_notification = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn begin_shortcut_capture_from_accessibility(
        &mut self,
        target: ShortcutCaptureTarget,
        cx: &mut Context<Self>,
    ) {
        if self.shortcut_capture.is_some() || !self.shortcut_commands_available() {
            return;
        }
        let Some(expected_config_revision) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.config_revision)
        else {
            return;
        };
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        let mut shortcuts_without_capture_target = snapshot.shortcuts.clone();
        clear_shortcut(&mut shortcuts_without_capture_target, &target);
        self.pending = Some(PendingOperation::BeginShortcutCapture);
        cx.notify();
        let client = self.client.clone();
        let resume_client = client.clone();
        cx.spawn(async move |this, cx| {
            let result = client
                .suspend_shortcut_capture(
                    expected_config_revision,
                    shortcuts_without_capture_target,
                )
                .await;
            let Some(view) = this.upgrade() else {
                if result.is_ok() {
                    let _ = resume_client.resume_shortcut_capture().await;
                }
                return;
            };
            view.update(cx, |view, cx| {
                if view.pending != Some(PendingOperation::BeginShortcutCapture) {
                    return;
                }
                view.pending = None;
                match result {
                    Ok(snapshot) => {
                        if accepts_snapshot_revision(
                            view.snapshot.as_ref().map(|current| current.revision),
                            snapshot.revision,
                        ) {
                            view.snapshot = Some(snapshot);
                        }
                        view.shortcut_capture = Some(ShortcutCapture::new(target));
                    }
                    Err(error) => view.pending_notification = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn capture_shortcut(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.keystroke.key.eq_ignore_ascii_case("escape") {
            self.cancel_shortcut_capture(cx);
            return;
        }
        {
            let Some(capture) = self.shortcut_capture.as_mut() else {
                return;
            };
            capture.modifiers = event.keystroke.modifiers;
            if let Some(key) = capture_key(event.keystroke.key.as_str()) {
                capture.keys.insert(key);
            }
        }
        self.finish_shortcut_capture_if_valid(window, cx);
    }

    pub(super) fn update_shortcut_capture_on_key_up(
        &mut self,
        event: &KeyUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        {
            let Some(capture) = self.shortcut_capture.as_mut() else {
                return;
            };
            capture.modifiers = event.keystroke.modifiers;
            if let Some(key) = capture_key(event.keystroke.key.as_str()) {
                capture.keys.remove(&key);
            }
        }
        self.finish_shortcut_capture_if_valid(window, cx);
    }

    pub(super) fn update_shortcut_capture_modifiers(
        &mut self,
        modifiers: Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(capture) = self.shortcut_capture.as_mut() else {
            return;
        };
        capture.modifiers = modifiers;
        self.finish_shortcut_capture_if_valid(window, cx);
    }

    fn finish_shortcut_capture_if_valid(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((target, shortcut)) = self.shortcut_capture.as_ref().and_then(|capture| {
            shortcut_from_capture(&capture.modifiers, &capture.keys)
                .map(|shortcut| (capture.target.clone(), shortcut))
        }) else {
            cx.notify();
            return;
        };
        let Some(snapshot) = self.snapshot.as_ref() else {
            cx.notify();
            return;
        };
        let mut shortcuts = snapshot.shortcuts.clone();
        if !replace_shortcut(&mut shortcuts, &target, shortcut.clone()) {
            cx.notify();
            return;
        }
        if let Some(conflict) = conflicting_shortcut(&shortcuts) {
            if let Some(capture) = self.shortcut_capture.as_mut() {
                capture.clear_temporary_input();
            }
            window.push_notification(
                Notification::new()
                    .id::<ShortcutConflictNotification>()
                    .message(shortcut_conflict_message(
                        snapshot.resolved_language,
                        &shortcut_display(&conflict),
                    ))
                    .with_type(NotificationType::Error),
                cx,
            );
            cx.notify();
            return;
        }
        let Some(expected_config_revision) = snapshot.config_revision else {
            return;
        };
        self.shortcut_capture = None;
        self.shortcut_capture_blur_subscription = None;
        window.blur(cx);
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
        self.shortcut_capture_blur_subscription = None;
        self.start_request(
            PendingOperation::SetShortcuts,
            Some(SettingValue::Shortcuts {
                expected_config_revision,
                shortcuts,
            }),
            cx,
        );
    }

    pub(super) fn cancel_shortcut_capture(&mut self, cx: &mut Context<Self>) {
        let capture_is_being_started = self.pending == Some(PendingOperation::BeginShortcutCapture);
        if self.shortcut_capture.take().is_none() && !capture_is_being_started {
            return;
        }
        self.shortcut_capture_blur_subscription = None;
        self.pending = Some(PendingOperation::CancelShortcutCapture);
        cx.notify();
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let result = client.resume_shortcut_capture().await;
            let _ = this.update(cx, |view, cx| {
                view.pending = None;
                match result {
                    Ok(snapshot)
                        if accepts_snapshot_revision(
                            view.snapshot.as_ref().map(|current| current.revision),
                            snapshot.revision,
                        ) =>
                    {
                        view.snapshot = Some(snapshot)
                    }
                    Ok(_) => {}
                    Err(error) => view.pending_notification = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
