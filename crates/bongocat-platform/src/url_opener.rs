use url::Url;

const MAX_EXTERNAL_URL_BYTES: usize = 2_048;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ExternalUrlOpenError {
    #[error("{}", Self::InvalidUrl.as_str())]
    InvalidUrl,
    #[error("{}", Self::LaunchFailed.as_str())]
    LaunchFailed,
}

impl ExternalUrlOpenError {
    pub const ALL: [Self; 2] = [Self::InvalidUrl, Self::LaunchFailed];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidUrl => "external_url_open_invalid_url",
            Self::LaunchFailed => "external_url_open_launch_failed",
        }
    }
}

/// Ask the operating system to open an HTTPS URL, and report whether it took it.
///
/// The call is synchronous and it **must not be made from a GUI callback that is
/// holding the framework's application state**, which is a property of the
/// platform rather than of this adapter, so it is stated here rather than left
/// for a caller to discover:
///
/// * Windows resolves the URL with `ShellExecuteW`, which **pumps the calling
///   process's message queue** before it returns (issue #1081). The pump
///   delivers the messages a GUI framework's own windows handle, and handling
///   them runs that framework's pending tasks on this thread, inside the
///   callback. Those tasks ask for the application's state, which the callback is
///   still holding. The macOS path runs the `open` command and pumps nothing,
///   which is why the identical control was fine there.
/// * Both platforms block until the browser they started has taken the URL, so
///   the call has no place on a thread that has to keep painting either.
///
/// The caller therefore owns the threading and must run this off the thread that
/// asked for the link. `bongocat_ui::external_link` is the one place in the
/// product that does; nothing else should call this from a view callback.
pub fn open_external_url(value: &str) -> Result<(), ExternalUrlOpenError> {
    open_external_url_with(value, launch_url)
}

fn open_external_url_with(
    value: &str,
    launch: impl FnOnce(&str) -> Result<(), ExternalUrlOpenError>,
) -> Result<(), ExternalUrlOpenError> {
    let url = parse_external_url(value)?;
    launch(url.as_str())
}

fn launch_url(url: &str) -> Result<(), ExternalUrlOpenError> {
    opener::open(url).map_err(|_| ExternalUrlOpenError::LaunchFailed)
}

fn parse_external_url(value: &str) -> Result<Url, ExternalUrlOpenError> {
    if value.is_empty() || value.len() > MAX_EXTERNAL_URL_BYTES {
        return Err(ExternalUrlOpenError::InvalidUrl);
    }
    let url = Url::parse(value).map_err(|_| ExternalUrlOpenError::InvalidUrl)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(ExternalUrlOpenError::InvalidUrl);
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_url_parser_allows_only_https_without_credentials() {
        let valid = parse_external_url("https://docs.example.invalid/guide#section")
            .expect("valid HTTPS URL");
        assert_eq!(valid.as_str(), "https://docs.example.invalid/guide#section");

        for value in [
            "",
            "https://",
            "http://docs.example.invalid/guide",
            "file:///private/data",
            "https://user@example.invalid/guide",
            "https://user:password@example.invalid/guide",
        ] {
            assert_eq!(
                parse_external_url(value),
                Err(ExternalUrlOpenError::InvalidUrl),
                "{value}"
            );
        }
    }

    #[test]
    fn external_url_parser_rejects_oversized_values_without_echoing_them() {
        let value = format!(
            "https://example.invalid/{}",
            "a".repeat(MAX_EXTERNAL_URL_BYTES)
        );
        assert_eq!(
            parse_external_url(&value),
            Err(ExternalUrlOpenError::InvalidUrl)
        );
        assert!(
            !ExternalUrlOpenError::InvalidUrl
                .to_string()
                .contains("example.invalid")
        );
    }

    #[test]
    fn external_url_errors_have_stable_anonymous_codes() {
        for (error, expected) in [
            (
                ExternalUrlOpenError::InvalidUrl,
                "external_url_open_invalid_url",
            ),
            (
                ExternalUrlOpenError::LaunchFailed,
                "external_url_open_launch_failed",
            ),
        ] {
            assert_eq!(error.as_str(), expected);
            assert_eq!(error.to_string(), expected);
        }
        assert_eq!(ExternalUrlOpenError::ALL.len(), 2);
    }

    #[test]
    fn adapter_delegates_the_validated_url_to_the_system_opener() {
        let value = "https://docs.example.invalid/guide?value=one%20two#section";
        let mut launched = None;

        let result = open_external_url_with(value, |url| {
            launched = Some(url.to_owned());
            Ok(())
        });

        assert_eq!(result, Ok(()));
        assert_eq!(launched.as_deref(), Some(value));
    }
}
