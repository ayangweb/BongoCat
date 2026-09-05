use super::*;

pub(super) fn content(snapshot: Option<&SettingsSnapshot>) -> Stateful<Div> {
    let language = snapshot.map_or(SettingsLanguage::EnglishUnitedStates, |snapshot| {
        snapshot.resolved_language
    });
    div()
        .id("about-content")
        .min_w_0()
        .w_full()
        .flex()
        .flex_col()
        .gap_4()
        .child(div().text_sm().child(snapshot.map_or_else(
            || ui_text(language, UiText::BuildInformation).to_owned(),
            |snapshot| build_info_detail(language, &snapshot.build_info),
        )))
        .children(ABOUT_SECTIONS.into_iter().map(|section| {
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_sm().child(ui_text(language, section.title)))
                .child(
                    div()
                        .text_sm()
                        .child(ui_text(language, section.description)),
                )
        }))
}
