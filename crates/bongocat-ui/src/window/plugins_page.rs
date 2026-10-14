//! Plugins page rendering.

use super::*;

/// Render the plugins page content.
pub fn content_static(language: SettingsLanguage) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .p_4()
        .child(div().text_xl().font_semibold().child(bongocat_i18n::text(
            language.catalog_locale(),
            "navigation.plugins.title",
        )))
        .child(
            div()
                .text_sm()
                .child("Plugin system is under development. Stay tuned!"),
        )
}
