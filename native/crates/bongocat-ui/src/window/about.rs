use super::*;

#[derive(Clone, Copy)]
pub(super) struct AboutSection {
    pub(super) title: &'static str,
    pub(super) description: &'static str,
}

pub(super) const ABOUT_SECTIONS: [AboutSection; 4] = [
    AboutSection {
        title: "about.legal.application_license.title",
        description: "about.legal.application_license.description",
    },
    AboutSection {
        title: "about.legal.third_party_licenses.title",
        description: "about.legal.third_party_licenses.description",
    },
    AboutSection {
        title: "about.legal.cubism_attribution.title",
        description: "about.legal.cubism_attribution.description",
    },
    AboutSection {
        title: "about.privacy.title",
        description: "about.privacy.description",
    },
];

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
            || bongocat_i18n::text(language.catalog_locale(), "diagnostics.build.title").to_owned(),
            |snapshot| build_info_detail(language, &snapshot.build_info),
        )))
        .children(ABOUT_SECTIONS.into_iter().map(|section| {
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_sm().child(bongocat_i18n::text(
                    language.catalog_locale(),
                    section.title,
                )))
                .child(div().text_sm().child(bongocat_i18n::text(
                    language.catalog_locale(),
                    section.description,
                )))
        }))
}
