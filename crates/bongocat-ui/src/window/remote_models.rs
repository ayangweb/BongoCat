//! The remote model library section of the Model library page.
//!
//! The section is one titled row plus the same self-drawn card grid the local
//! catalog uses. Everything it shows comes from the snapshot's remote
//! projection: the card renders its status rather than asking for it, so one
//! poll drives previews, download progress and the import stages alike.

use std::sync::Arc;

use gpui_kit::component::Sizable as _;
use gpui_kit::{AnyElement, relative};

use super::*;

/// How tall a remote card's preview box stands, matched to the local cover so
/// one grid row stays level across the two sections.
const REMOTE_PREVIEW_HEIGHT: f32 = 140.0;

pub(super) fn remote_content(
    view: &mut SettingsView,
    window: &mut Window,
    cx: &mut Context<SettingsView>,
    snapshot: Option<&SettingsSnapshot>,
    tokens: Tokens,
) -> Stateful<Div> {
    let language = view.display_language();
    let remote = snapshot.map_or_else(SettingsRemoteModels::default, |snapshot| {
        snapshot.remote_models.clone()
    });
    let text_locale = language.catalog_locale();
    let header = div()
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .child(
            div()
                .text_sm()
                .font_medium()
                .text_color(tokens.text)
                .child(SharedString::from(bongocat_i18n::text(
                    text_locale,
                    "models.remote.title",
                ))),
        )
        .child(
            Button::new("remote-models-refresh-button")
                .label(bongocat_i18n::text(
                    text_locale,
                    "models.remote.actions.refresh",
                ))
                .small()
                .disabled(remote.catalog == SettingsRemoteCatalogStatus::Loading)
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.request_remote_models_refresh(cx);
                })),
        );
    let body: AnyElement = match remote.catalog {
        SettingsRemoteCatalogStatus::Unloaded | SettingsRemoteCatalogStatus::Loading => div()
            .w_full()
            .py_2()
            .text_sm()
            .text_color(tokens.muted)
            .child(bongocat_i18n::text(
                text_locale,
                "models.remote.catalog.loading",
            ))
            .into_any_element(),
        SettingsRemoteCatalogStatus::Failed => div()
            .w_full()
            .py_2()
            .text_sm()
            .text_color(tokens.danger)
            .child(bongocat_i18n::text(
                text_locale,
                "models.remote.catalog.failed",
            ))
            .into_any_element(),
        SettingsRemoteCatalogStatus::Ready if remote.entries.is_empty() => div()
            .w_full()
            .py_2()
            .text_sm()
            .text_color(tokens.muted)
            .child(bongocat_i18n::text(
                text_locale,
                "models.remote.catalog.empty",
            ))
            .into_any_element(),
        SettingsRemoteCatalogStatus::Ready => {
            let cards = remote
                .entries
                .iter()
                .enumerate()
                .map(|(index, entry)| remote_card(view, entry, index, language, cx, &tokens))
                .collect::<Vec<_>>();
            super::models::model_grid(
                super::models::model_grid_columns_for_window(window.viewport_size().width),
                cards,
            )
            .into_any_element()
        }
    };
    div()
        .id("remote-models-content")
        .w_full()
        .flex()
        .flex_col()
        .gap_3()
        .mt_2()
        .child(header)
        .child(body)
}

fn remote_card(
    view: &mut SettingsView,
    entry: &SettingsRemoteModelEntry,
    index: usize,
    language: SettingsLanguage,
    cx: &mut Context<SettingsView>,
    tokens: &Tokens,
) -> AnyElement {
    let text_locale = language.catalog_locale();
    div()
        .id(("remote-model-card", index))
        .w_full()
        .flex()
        .flex_col()
        .gap_2()
        .p_2()
        .rounded_lg()
        .border_1()
        .border_color(tokens.border)
        .bg(tokens.canvas)
        .child(remote_card_preview(entry, index, language, tokens))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .text_sm()
                        .font_medium()
                        .text_color(tokens.text)
                        .text_ellipsis()
                        .child(SharedString::from(entry.name.clone())),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(tokens.muted)
                        .text_ellipsis()
                        .child(SharedString::from(entry.author.clone())),
                ),
        )
        .child(remote_card_status(
            view,
            entry,
            index,
            text_locale,
            cx,
            tokens,
        ))
        .into_any_element()
}

fn remote_card_preview(
    entry: &SettingsRemoteModelEntry,
    index: usize,
    language: SettingsLanguage,
    tokens: &Tokens,
) -> AnyElement {
    let frame = div()
        .id(("remote-model-preview", index))
        .relative()
        .flex_none()
        .w_full()
        .h(px(REMOTE_PREVIEW_HEIGHT))
        .flex()
        .items_center()
        .justify_center()
        .overflow_hidden()
        .rounded_md()
        .border_1()
        .border_color(tokens.border)
        .bg(tokens.canvas);
    let placeholder_key = match &entry.preview {
        SettingsRemotePreview::Ready(image) => {
            let format = match image.format() {
                SettingsRemoteImageFormat::Png => gpui_kit::ImageFormat::Png,
                SettingsRemoteImageFormat::Jpeg => gpui_kit::ImageFormat::Jpeg,
                SettingsRemoteImageFormat::Webp => gpui_kit::ImageFormat::Webp,
                SettingsRemoteImageFormat::Gif => gpui_kit::ImageFormat::Gif,
            };
            let image = Arc::new(gpui_kit::Image::from_bytes(format, image.bytes().to_vec()));
            return frame
                .child(
                    img(ImageSource::Image(image))
                        .w_full()
                        .h_full()
                        .object_fit(ObjectFit::Cover),
                )
                .into_any_element();
        }
        SettingsRemotePreview::Pending => "models.remote.preview.pending",
        SettingsRemotePreview::Unavailable => "models.remote.preview.unavailable",
    };
    frame
        .child(
            div()
                .text_sm()
                .text_color(tokens.muted)
                .child(bongocat_i18n::text(
                    language.catalog_locale(),
                    placeholder_key,
                )),
        )
        .into_any_element()
}

fn remote_card_status(
    view: &SettingsView,
    entry: &SettingsRemoteModelEntry,
    index: usize,
    text_locale: &str,
    cx: &mut Context<SettingsView>,
    tokens: &Tokens,
) -> AnyElement {
    let row = div()
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .gap_2();
    match &entry.status {
        SettingsRemoteModelStatus::Downloading {
            downloaded_bytes,
            total_bytes,
        } => {
            let (bar, label) = match total_bytes {
                Some(total) if *total > 0 => {
                    let fraction = ((*downloaded_bytes as f64) / (*total as f64)).clamp(0.0, 1.0);
                    let percent = (fraction * 100.0).round() as u64;
                    (
                        progress_bar(fraction as f32, tokens),
                        bongocat_i18n::format_text(
                            text_locale,
                            "models.remote.status.downloading",
                            &[("percent", percent.to_string())],
                        ),
                    )
                }
                _ => (
                    progress_bar(0.0, tokens),
                    bongocat_i18n::format_text(
                        text_locale,
                        "models.remote.status.downloading_unknown",
                        &[("megabytes", (*downloaded_bytes / 1_048_576).to_string())],
                    ),
                ),
            };
            row.child(bar)
                .child(
                    div()
                        .text_xs()
                        .text_color(tokens.muted)
                        .flex_none()
                        .child(SharedString::from(label)),
                )
                .into_any_element()
        }
        SettingsRemoteModelStatus::Importing(_) => row
            .child(
                div()
                    .text_xs()
                    .text_color(tokens.muted)
                    .child(bongocat_i18n::text(
                        text_locale,
                        "models.remote.status.importing",
                    )),
            )
            .into_any_element(),
        SettingsRemoteModelStatus::Installed => row
            .child(
                div()
                    .text_xs()
                    .text_color(tokens.accent)
                    .child(bongocat_i18n::text(
                        text_locale,
                        "models.remote.status.installed",
                    )),
            )
            .into_any_element(),
        SettingsRemoteModelStatus::Failed(failure) => {
            let failure = *failure;
            let id = entry.id;
            row.child(
                div()
                    .flex_1()
                    .text_xs()
                    .text_color(tokens.danger)
                    .text_ellipsis()
                    .child(remote_failure_label(text_locale, failure)),
            )
            .child(
                Button::new(("remote-model-retry", index))
                    .label(bongocat_i18n::text(
                        text_locale,
                        "models.remote.actions.retry",
                    ))
                    .small()
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.download_remote_model(id, cx);
                    })),
            )
            .into_any_element()
        }
        SettingsRemoteModelStatus::Available => {
            let id = entry.id;
            row.child(
                Button::new(("remote-model-download", index))
                    .label(bongocat_i18n::text(
                        text_locale,
                        "models.remote.actions.download",
                    ))
                    .small()
                    .disabled(view.remote_models_in_flight())
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.download_remote_model(id, cx);
                    })),
            )
            .into_any_element()
        }
    }
}

/// The thin self-drawn progress bar under a downloading card. The component
/// library has no determinate progress primitive, and the bar is one box with a
/// fill: drawing it here keeps the card's own tokens and height.
fn progress_bar(fraction: f32, tokens: &Tokens) -> AnyElement {
    div()
        .id("remote-model-progress")
        .flex_1()
        .h_1()
        .rounded_full()
        .bg(tokens.border)
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .w(relative(fraction.clamp(0.02, 1.0)))
                .rounded_full()
                .bg(tokens.accent),
        )
        .into_any_element()
}

fn remote_failure_label(text_locale: &str, failure: SettingsRemoteModelFailure) -> &'static str {
    let key = match failure {
        SettingsRemoteModelFailure::DownloadFailed => "models.remote.status.failed.download",
        SettingsRemoteModelFailure::DownloadTooLarge => "models.remote.status.failed.too_large",
        SettingsRemoteModelFailure::ImportFailed => "models.remote.status.failed.import",
    };
    bongocat_i18n::text(text_locale, key)
}

impl SettingsView {
    /// Whether a remote model is downloading or importing right now. One remote
    /// operation runs at a time, and every download button reads the same fact.
    pub(super) fn remote_models_in_flight(&self) -> bool {
        self.snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.remote_models.has_entry_in_flight())
    }

    /// Ask the service for a fresh remote model catalog.
    ///
    /// The reply carries the loading state; the entries arrive through later
    /// snapshot revisions, so this task only has to place the answer's revision.
    pub(super) fn request_remote_models_refresh(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let result = client.refresh_remote_models().await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(snapshot) => view.apply_snapshot_if_newer(snapshot),
                    Err(error) => view.pending_notification = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Start downloading one remote model catalog entry.
    ///
    /// The reply reports acceptance only; progress and outcome travel through
    /// the entry's status, and a refusal — a second operation, an entry the
    /// catalog no longer carries — becomes a notification like every other.
    pub(super) fn download_remote_model(&mut self, id: u64, cx: &mut Context<Self>) {
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let result = client.download_remote_model(id).await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(snapshot) => view.apply_snapshot_if_newer(snapshot),
                    Err(error) => view.pending_notification = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn apply_snapshot_if_newer(&mut self, snapshot: SettingsSnapshot) {
        if self
            .snapshot
            .as_ref()
            .is_none_or(|current| snapshot.revision >= current.revision)
        {
            self.snapshot = Some(snapshot);
            self.observe_multiplayer_error();
        }
    }
}
