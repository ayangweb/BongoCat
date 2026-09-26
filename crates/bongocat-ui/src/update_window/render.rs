//! How the update window looks.
//!
//! One frame per phase, and a phase that is not available says why rather than
//! offering a button that would fail. The changelog is rendered as markdown
//! because a release's notes are written as markdown by whoever published it.

use super::*;

impl Render for UpdateView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        if viewport.width <= px(0.) || viewport.height <= px(0.) {
            return div().size_full().into_any_element();
        }
        let language = self.language;
        let locale = language.catalog_locale();
        window.set_window_title(text(locale, "update.window.title"));
        self.sync_component_theme(self.appearance_theme, window, cx);

        let tokens = Tokens::from_theme(cx);
        let phase = self.snapshot.phase.clone();
        let current_version = self.snapshot.current_version.clone();

        let body = match &phase {
            UpdatePhase::Unavailable { reason } => div()
                .flex()
                .flex_col()
                .gap_2()
                .child(status_line(unavailable_message(locale, *reason), tokens)),
            UpdatePhase::Idle => div()
                .flex()
                .flex_col()
                .gap_2()
                .child(status_line(text(locale, "update.status.idle"), tokens)),
            UpdatePhase::Checking => div()
                .flex()
                .flex_col()
                .gap_3()
                .child(status_line(text(locale, "update.status.checking"), tokens))
                .child(
                    Progress::new("update-check-progress")
                        .loading(true)
                        .w_full(),
                ),
            UpdatePhase::UpToDate => div().flex().flex_col().gap_2().child(status_line(
                format_text(
                    locale,
                    "update.status.up_to_date",
                    &[("version", current_version.clone())],
                ),
                tokens,
            )),
            UpdatePhase::Available { release } => {
                div().flex().flex_col().gap_2().child(status_line(
                    format_text(
                        locale,
                        "update.status.available",
                        &[("version", release.version.clone())],
                    ),
                    tokens,
                ))
            }
            UpdatePhase::Downloading { release, progress } => div()
                .flex()
                .flex_col()
                .gap_3()
                .child(status_line(download_message(locale, *progress), tokens))
                .child(
                    Progress::new("update-download-progress")
                        .value(progress.fraction().map_or(0.0, |fraction| fraction * 100.0))
                        .loading(progress.fraction().is_none())
                        .w_full(),
                )
                .child(hint_line(
                    format_text(
                        locale,
                        "update.status.downloading_detail",
                        &[
                            ("version", release.version.clone()),
                            ("downloaded", human_bytes(progress.downloaded_bytes)),
                            (
                                "total",
                                progress
                                    .total_bytes
                                    .map_or_else(|| "?".to_owned(), human_bytes),
                            ),
                        ],
                    ),
                    tokens,
                )),
            UpdatePhase::Verifying { release } => div()
                .flex()
                .flex_col()
                .gap_3()
                .child(status_line(
                    format_text(
                        locale,
                        "update.status.verifying",
                        &[("version", release.version.clone())],
                    ),
                    tokens,
                ))
                .child(
                    Progress::new("update-verify-progress")
                        .loading(true)
                        .w_full(),
                ),
            UpdatePhase::Installing { release } => div()
                .flex()
                .flex_col()
                .gap_3()
                .child(status_line(
                    format_text(
                        locale,
                        "update.status.installing",
                        &[("version", release.version.clone())],
                    ),
                    tokens,
                ))
                .child(
                    Progress::new("update-install-progress")
                        .loading(true)
                        .w_full(),
                ),
            UpdatePhase::Installed {
                version,
                restart_required,
            } => {
                let key = if *restart_required {
                    "update.status.installed_restarting"
                } else {
                    "update.status.installed_relaunching"
                };
                div().flex().flex_col().gap_2().child(status_line(
                    format_text(locale, key, &[("version", version.clone())]),
                    tokens,
                ))
            }
            UpdatePhase::Failed {
                stage,
                code,
                release,
            } => {
                // Before a check succeeds there is no release version to name, so the
                // message falls back to the running build's version.
                let version = release.as_ref().map_or_else(
                    || current_version.clone(),
                    |release| release.version.clone(),
                );
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(status_line(
                        format_text(locale, stage_message_key(*stage), &[("version", version)]),
                        tokens,
                    ))
                    .child(hint_line(
                        text(locale, update_error_message_key(*code)),
                        tokens,
                    ))
            }
        };

        let release = phase.release().cloned();
        let notes = release
            .as_ref()
            .and_then(|release| release.notes.as_ref())
            .cloned();
        let release_page = release
            .as_ref()
            .and_then(|release| release.release_page_url.clone());

        let mut footer = div()
            .id("update-footer")
            .test_support()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .w_full();
        if phase.offers_check() {
            // Idle 是首次检查；UpToDate 和 Failed 都意味着至少检查过一次，动作是"再来一次"。
            // 用"重新检查"而不是"重试"：点击走的是完整的 check → available → 重新下载，
            // 不是续传，措辞必须与真实动作一致。
            let check_key = if matches!(phase, UpdatePhase::UpToDate | UpdatePhase::Failed { .. }) {
                "update.action.recheck"
            } else {
                "update.action.check"
            };
            footer = footer.child(
                command_button(
                    text(locale, check_key),
                    "update-check",
                    &self.primary_focus,
                    tokens,
                    false,
                )
                .on_click(cx.listener(|view, _, _, cx| view.check(cx))),
            );
        }
        if phase.offers_install() {
            footer = footer.child(
                command_button(
                    text(locale, "update.action.install"),
                    "update-install",
                    &self.primary_focus,
                    tokens,
                    false,
                )
                .on_click(cx.listener(|view, _, _, cx| view.install(cx))),
            );
        }
        if phase.offers_restart() {
            footer = footer.child(
                command_button(
                    text(locale, "update.action.restart"),
                    "update-restart",
                    &self.primary_focus,
                    tokens,
                    false,
                )
                .on_click(cx.listener(|view, _, _, cx| view.restart(cx))),
            );
        }
        if let Some(url) = release_page
            && matches!(
                phase,
                UpdatePhase::Available { .. }
                    | UpdatePhase::Downloading { .. }
                    | UpdatePhase::Verifying { .. }
                    | UpdatePhase::Installing { .. }
            )
        {
            footer = footer.child(
                command_button(
                    text(locale, "update.action.view_on_github"),
                    "update-release-notes",
                    &self.link_focus,
                    tokens,
                    false,
                )
                .on_click(move |_, _window, _cx| {
                    let _ = bongocat_platform::open_external_url(&url);
                }),
            );
        }
        footer = footer.child(div().flex_1()).child(
            command_button(
                text(locale, "actions.close"),
                "update-close",
                &self.close_focus,
                tokens,
                false,
            )
            .on_click(cx.listener(|view, _, window, cx| view.close(window, cx))),
        );

        let measure = self.content_height.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .bg(tokens.canvas)
            .text_color(tokens.text)
            // The height this window should have is only knowable once the frame has
            // laid out, so the layout pass records it and the following frame acts on
            // it. Resizing from inside the layout pass would change the very size the
            // frame is being laid out against.
            .on_children_prepainted({
                let measure = measure.clone();
                move |children, window, _| {
                    measure.record(&children);
                    let Some(target) = measure.target(window.viewport_size().height) else {
                        return;
                    };
                    window.on_next_frame(move |window, _| {
                        let viewport = window.viewport_size();
                        if viewport.height == target {
                            return;
                        }
                        window.resize(size(viewport.width, target));
                    });
                }
            })
            .child(
                // `flex_shrink_0` is what makes the measurement honest: the column
                // keeps the height its content needs even when the window is too
                // short, which is what lets the frame report that the window is too
                // short instead of quietly compressing the changelog to fit.
                div()
                    .id("update-content")
                    .test_support()
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_lg()
                                    .child(text(locale, "update.heading.title").to_owned()),
                            )
                            .child(hint_line(
                                format_text(
                                    locale,
                                    "update.current_version",
                                    &[("version", current_version.clone())],
                                ),
                                tokens,
                            )),
                    )
                    .child(body)
                    .child(notes_section(locale, notes, tokens, cx)),
            )
            // The leftover height collects here rather than stretching the
            // changelog, so the actions stay where a dialog's actions belong
            // whether the window is oversized, exact, or shrunk by the user — and
            // so the gap below the content is a real distance the frame can report
            // rather than a constant this file has to keep in step with the style.
            .child(div().flex_1())
            .child(footer)
            .into_any_element()
    }
}
