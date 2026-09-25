use crate::SettingsLanguage;
use gpui_kit::{SharedString, assets::IconName};

/// The product-owned information architecture of the settings sidebar.
///
/// The order is part of the visible contract: frequent product choices come
/// first, system preferences follow, and About remains the final utility entry.
/// Titles and icons live here so the renderer, smoke checks, and search
/// aliases cannot drift into three independent lists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SettingsNavigationPage {
    Appearance,
    ModelLibrary,
    ModelBehavior,
    ModelWindow,
    InputInteraction,
    Shortcuts,
    AppSystem,
    About,
}

impl SettingsNavigationPage {
    pub(super) const ALL: [Self; 8] = [
        Self::Appearance,
        Self::ModelLibrary,
        Self::ModelBehavior,
        Self::ModelWindow,
        Self::InputInteraction,
        Self::Shortcuts,
        Self::AppSystem,
        Self::About,
    ];

    pub(super) const fn title_key(self) -> &'static str {
        match self {
            Self::Appearance => "navigation.appearance.title",
            Self::ModelLibrary => "navigation.model_library.title",
            Self::ModelBehavior => "navigation.model_behavior.title",
            Self::ModelWindow => "navigation.model_window.title",
            Self::InputInteraction => "navigation.input_interaction.title",
            Self::Shortcuts => "navigation.shortcuts.title",
            Self::AppSystem => "navigation.app_system.title",
            Self::About => "navigation.about.title",
        }
    }

    pub(super) fn title(self, language: SettingsLanguage) -> SharedString {
        bongocat_i18n::text(language.catalog_locale(), self.title_key()).into()
    }

    pub(super) const fn icon(self) -> IconName {
        match self {
            Self::Appearance => IconName::Settings,
            Self::ModelLibrary => IconName::Cat,
            Self::ModelBehavior => IconName::SlidersHorizontal,
            Self::ModelWindow => IconName::AppWindow,
            Self::InputInteraction => IconName::MousePointer2,
            Self::Shortcuts => IconName::Keyboard,
            Self::AppSystem => IconName::Cog,
            Self::About => IconName::Info,
        }
    }

    /// Search terms for one page or group.
    ///
    /// `gpui-kit` searches setting-item titles, descriptions, and explicit
    /// keywords, but not page or group titles. Supplying those names here keeps
    /// the visible hierarchy searchable. The former page labels remain aliases
    /// so a user who learned the old navigation can still find its destination.
    pub(super) fn search_keywords<I>(
        self,
        language: SettingsLanguage,
        group_keys: I,
    ) -> Vec<SharedString>
    where
        I: IntoIterator<Item = &'static str>,
    {
        let locale = language.catalog_locale();
        let mut keywords = Vec::new();
        keywords.push(self.title(language));
        keywords.extend(
            group_keys
                .into_iter()
                .map(|key| bongocat_i18n::text(locale, key).into()),
        );
        keywords.extend(self.legacy_aliases().iter().map(|alias| (*alias).into()));
        keywords
    }

    const fn legacy_aliases(self) -> &'static [&'static str] {
        match self {
            Self::Appearance => &["General", "通用"],
            Self::ModelLibrary => &["Models", "Model management", "模型", "模型管理"],
            Self::ModelBehavior => &[
                "Models",
                "Model management",
                "模型",
                "模型管理",
                "Display & behavior",
                "显示与行为",
            ],
            Self::ModelWindow => &["Overlay"],
            Self::InputInteraction => &["Interaction", "Input", "交互", "输入"],
            Self::Shortcuts => &[],
            Self::AppSystem => &[
                "Application",
                "应用",
                "Startup and updates",
                "启动与更新",
                "System icons",
                "系统图标",
            ],
            Self::About => &["About BongoCat", "关于 BongoCat"],
        }
    }
}

/// Search terms for the model library, including the currently visible model
/// names. The names are intentionally keywords only; the card grid still owns
/// the visible model text and the typed model commands still own activation.
pub(super) fn model_library_search_keywords<I, S>(
    language: SettingsLanguage,
    model_titles: I,
) -> Vec<SharedString>
where
    I: IntoIterator<Item = S>,
    S: Into<SharedString>,
{
    let mut keywords =
        SettingsNavigationPage::ModelLibrary.search_keywords(language, std::iter::empty());
    keywords.extend(model_titles.into_iter().map(Into::into));
    keywords
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn navigation_uses_the_agreed_task_order_with_about_last() {
        assert_eq!(
            SettingsNavigationPage::ALL,
            [
                SettingsNavigationPage::Appearance,
                SettingsNavigationPage::ModelLibrary,
                SettingsNavigationPage::ModelBehavior,
                SettingsNavigationPage::ModelWindow,
                SettingsNavigationPage::InputInteraction,
                SettingsNavigationPage::Shortcuts,
                SettingsNavigationPage::AppSystem,
                SettingsNavigationPage::About,
            ]
        );
    }

    #[test]
    fn every_localized_page_title_is_present_and_distinct() {
        for language in SettingsLanguage::ALL {
            let titles = SettingsNavigationPage::ALL
                .into_iter()
                .map(|page| page.title(language).to_string())
                .collect::<Vec<_>>();
            assert!(titles.iter().all(|title| !title.is_empty()));
            assert_eq!(
                titles.iter().collect::<BTreeSet<_>>().len(),
                titles.len(),
                "two sidebar destinations would collapse into the same visible label: {titles:?}"
            );
        }
    }

    #[test]
    fn model_library_and_model_behavior_are_independent_destinations() {
        assert_eq!(
            SettingsNavigationPage::ModelLibrary.title(SettingsLanguage::EnglishUnitedStates),
            "Model library"
        );
        assert_eq!(
            SettingsNavigationPage::ModelBehavior.title(SettingsLanguage::EnglishUnitedStates),
            "Model behavior"
        );
        assert_eq!(
            SettingsNavigationPage::ModelLibrary.title(SettingsLanguage::ChineseSimplified),
            "模型库"
        );
        assert_eq!(
            SettingsNavigationPage::ModelBehavior.title(SettingsLanguage::ChineseSimplified),
            "模型行为"
        );
    }

    #[test]
    fn search_includes_the_page_group_and_previous_page_name() {
        let keywords = SettingsNavigationPage::InputInteraction.search_keywords(
            SettingsLanguage::EnglishUnitedStates,
            ["settings.input_interaction.mouse.title"],
        );
        assert!(
            keywords
                .iter()
                .any(|value| value.as_ref() == "Input & interaction")
        );
        assert!(keywords.iter().any(|value| value.as_ref() == "Mouse"));
        assert!(keywords.iter().any(|value| value.as_ref() == "Interaction"));
        assert!(keywords.iter().any(|value| value.as_ref() == "Input"));
    }

    #[test]
    fn model_library_search_includes_visible_model_names() {
        let keywords = model_library_search_keywords(
            SettingsLanguage::EnglishUnitedStates,
            ["standard", "Keyboard mode"],
        );
        assert!(
            keywords
                .iter()
                .any(|value| value.as_ref() == "Model library")
        );
        assert!(keywords.iter().any(|value| value.as_ref() == "standard"));
        assert!(
            keywords
                .iter()
                .any(|value| value.as_ref() == "Keyboard mode")
        );
    }
}
