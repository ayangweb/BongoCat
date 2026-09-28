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
}

impl Language {
    pub const ALL: [Self; 3] = [
        Self::System,
        Self::ChineseSimplified,
        Self::EnglishUnitedStates,
    ];

    pub const fn code(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::ChineseSimplified => "zh-CN",
            Self::EnglishUnitedStates => "en-US",
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
        } else {
            Self::EnglishUnitedStates
        }
    }

    pub const fn resolve(self, system_language: Self) -> Self {
        match self {
            Self::System => match system_language {
                Self::ChineseSimplified => Self::ChineseSimplified,
                Self::System | Self::EnglishUnitedStates => Self::EnglishUnitedStates,
            },
            Self::ChineseSimplified => Self::ChineseSimplified,
            Self::EnglishUnitedStates => Self::EnglishUnitedStates,
        }
    }
}
