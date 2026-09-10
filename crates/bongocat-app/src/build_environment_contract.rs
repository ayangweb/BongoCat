use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuildEnvironmentSelection {
    Development,
    Production,
}

impl BuildEnvironmentSelection {
    pub const fn cfg_value(self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::Production => "production",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BuildEnvironmentSelectionError {
    Missing,
    Unknown(String),
}

impl fmt::Display for BuildEnvironmentSelectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => formatter.write_str(
                "BONGOCAT_BUILD_ENV must be explicitly set to 'development' or 'production'",
            ),
            Self::Unknown(value) => write!(
                formatter,
                "BONGOCAT_BUILD_ENV must be 'development' or 'production', got {value:?}"
            ),
        }
    }
}

pub fn select_build_environment(
    value: Option<&str>,
) -> Result<BuildEnvironmentSelection, BuildEnvironmentSelectionError> {
    match value {
        Some("development") => Ok(BuildEnvironmentSelection::Development),
        Some("production") => Ok(BuildEnvironmentSelection::Production),
        Some(value) => Err(BuildEnvironmentSelectionError::Unknown(value.to_owned())),
        None => Err(BuildEnvironmentSelectionError::Missing),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_requires_an_explicit_known_environment() {
        assert_eq!(
            select_build_environment(Some("development")),
            Ok(BuildEnvironmentSelection::Development)
        );
        assert_eq!(
            select_build_environment(Some("production")),
            Ok(BuildEnvironmentSelection::Production)
        );
        assert_eq!(
            select_build_environment(Some("development"))
                .expect("development selection")
                .cfg_value(),
            "development"
        );
        assert_eq!(
            select_build_environment(Some("production"))
                .expect("production selection")
                .cfg_value(),
            "production"
        );
        assert_eq!(
            select_build_environment(None),
            Err(BuildEnvironmentSelectionError::Missing)
        );
        assert_eq!(
            select_build_environment(Some("")),
            Err(BuildEnvironmentSelectionError::Unknown(String::new()))
        );
        assert_eq!(
            select_build_environment(Some("Production")),
            Err(BuildEnvironmentSelectionError::Unknown(
                "Production".to_owned()
            ))
        );
    }
}
