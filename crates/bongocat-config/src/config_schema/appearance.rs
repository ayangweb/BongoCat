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
    // Arabic. The region subtag nominates the primary variety rather than
    // claiming a localisation the catalog does not carry, so the name is
    // `ar-SA` while one catalog serves every Arabic locale — the same
    // relationship `en-US` already has to `en-GB`. A machine reporting
    // `ar-EG`, `ar-SA` or plain `ar` all resolve here on their primary subtag.
    //
    // A plain comment rather than a doc comment is deliberate: `schemars`
    // describes a documented variant and leaves the others bare, which turns
    // the generated `Language` schema from a flat `enum` into a `oneOf`. The
    // checked-in schema is a contract other tools read, so it keeps the shape
    // that reads best and the reasoning lives here instead.
    #[serde(rename = "ar-SA")]
    Arabic,
    // Vietnamese, the same shape as Arabic: `vi` and `vi-VN` both reach the
    // one shipped catalog. The endonym `Tiếng Việt` keeps its diacritics in
    // every catalog, so a reader who cannot read the current window language
    // still finds their own.
    #[serde(rename = "vi-VN")]
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
            Self::Arabic => "ar-SA",
            Self::Vietnamese => "vi-VN",
        }
    }

    /// Classify the locale the operating system reports onto a shipped language.
    ///
    /// Matching the primary subtag is the RFC 4647 language-subtag fallback:
    /// every region variant of a shipped language reaches the same catalog, so
    /// `ar-EG` and `vi` land where a user expects instead of on the English
    /// default. Simplified Chinese is the one subtag that cannot decide this
    /// alone, because Simplified and Traditional are different written forms
    /// rather than regional variants, and only the Simplified catalog ships.
    pub fn from_system_locale(locale: &str) -> Self {
        let locale = locale.replace('_', "-").to_ascii_lowercase();
        let subtags = locale.split('-').collect::<Vec<_>>();
        match subtags.first() {
            Some(&"zh")
                if !subtags
                    .iter()
                    .any(|subtag| matches!(*subtag, "hant" | "tw" | "hk" | "mo")) =>
            {
                Self::ChineseSimplified
            }
            Some(&"ar") => Self::Arabic,
            Some(&"vi") => Self::Vietnamese,
            _ => Self::EnglishUnitedStates,
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
