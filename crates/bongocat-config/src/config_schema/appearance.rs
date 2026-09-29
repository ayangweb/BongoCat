//! What the user sees.
//!
//! The language is resolved rather than stored as a bare string: the document
//! records what the user picked, and `from_system_locale` is what turns an unset
//! or unusable value into the language the operating system actually reports.

use super::*;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct AppearanceConfig {
    pub theme: Theme,
    pub language: Language,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(any(test, feature = "schema-generation"), derive(JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum Language {
    #[default]
    System,
    #[serde(rename = "zh-CN")]
    ChineseSimplified,
    #[serde(rename = "en-US")]
    EnglishUnitedStates,
    // Arabic. The stored form is the bare `ar` subtag rather than a
    // region-qualified one, so a machine reporting `ar-EG`, `ar-SA` or plain
    // `ar` all resolve here and one catalog serves every Arabic locale.
    //
    // A plain comment rather than a doc comment is deliberate: `schemars`
    // describes a documented variant and leaves the others bare, which turns
    // the generated `Language` schema from a flat `enum` into a `oneOf`. The
    // checked-in schema is a contract other tools read, so it keeps the shape
    // that reads best and the reasoning lives here instead.
    #[serde(rename = "ar")]
    Arabic,
    // Vietnamese. Like Arabic, the stored form is the bare `vi` subtag rather
    // than a region-qualified one, so a machine reporting `vi-VN` and one
    // reporting plain `vi` both resolve to the single shipped catalog. The
    // endonym `Tiếng Việt` is written with its own diacritics in every
    // catalog, so a reader who cannot read the current window language still
    // finds their own.
    #[serde(rename = "vi")]
    Vietnamese,
}

impl Language {
    pub const ALL: [Self; 5] = [
        Self::System,
        Self::ChineseSimplified,
        Self::EnglishUnitedStates,
        Self::Arabic,
        Self::Vietnamese,
    ];

    pub const fn code(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::ChineseSimplified => "zh-CN",
            Self::EnglishUnitedStates => "en-US",
            Self::Arabic => "ar",
            Self::Vietnamese => "vi",
        }
    }

    pub fn from_system_locale(locale: &str) -> Self {
        let locale = locale.replace('_', "-").to_ascii_lowercase();
        let subtags = locale.split('-').collect::<Vec<_>>();
        if subtags.first() == Some(&"zh")
            && !subtags
                .iter()
                .any(|subtag| matches!(*subtag, "hant" | "tw" | "hk" | "mo"))
        {
            Self::ChineseSimplified
        } else if subtags.first() == Some(&"ar") {
            Self::Arabic
        } else if subtags.first() == Some(&"vi") {
            Self::Vietnamese
        } else {
            Self::EnglishUnitedStates
        }
    }

    pub const fn resolve(self, system_language: Self) -> Self {
        match self {
            Self::System => match system_language {
                Self::ChineseSimplified => Self::ChineseSimplified,
                Self::Arabic => Self::Arabic,
                Self::Vietnamese => Self::Vietnamese,
                Self::System | Self::EnglishUnitedStates => Self::EnglishUnitedStates,
            },
            Self::ChineseSimplified => Self::ChineseSimplified,
            Self::EnglishUnitedStates => Self::EnglishUnitedStates,
            Self::Arabic => Self::Arabic,
            Self::Vietnamese => Self::Vietnamese,
        }
    }
}
