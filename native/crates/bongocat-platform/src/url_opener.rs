use std::fmt;

#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::{
    process::{Command, Stdio},
    thread,
};

use url::Url;

const MAX_EXTERNAL_URL_BYTES: usize = 2_048;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalUrlOpenError {
    UnsupportedPlatform,
    InvalidUrl,
    LaunchFailed,
}

impl fmt::Display for ExternalUrlOpenError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnsupportedPlatform => "external URL opening is unsupported on this platform",
            Self::InvalidUrl => "external URL is invalid",
            Self::LaunchFailed => "external URL opener could not be launched",
        })
    }
}

impl std::error::Error for ExternalUrlOpenError {}

pub fn open_external_url(value: &str) -> Result<(), ExternalUrlOpenError> {
    let url = parse_external_url(value)?;

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = url;
        return Err(ExternalUrlOpenError::UnsupportedPlatform);
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let mut child = external_url_open_command(url.as_str())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| ExternalUrlOpenError::LaunchFailed)?;
        thread::Builder::new()
            .name("bongocat-external-url-opener-reaper".to_owned())
            .spawn(move || {
                let _ = child.wait();
            })
            .map_err(|_| ExternalUrlOpenError::LaunchFailed)?;
        Ok(())
    }
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

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn external_url_open_command(url: &str) -> Command {
    #[cfg(target_os = "macos")]
    let mut command = Command::new("/usr/bin/open");
    #[cfg(target_os = "windows")]
    let mut command = Command::new("explorer.exe");
    command.arg(url);
    command
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

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn platform_opener_receives_the_url_as_one_argument_without_a_shell() {
        #[cfg(target_os = "macos")]
        let expected_program = std::ffi::OsStr::new("/usr/bin/open");
        #[cfg(target_os = "windows")]
        let expected_program = std::ffi::OsStr::new("explorer.exe");
        let url = "https://docs.example.invalid/guide?value=one%20two&safe=true";
        let command = external_url_open_command(url);

        assert_eq!(command.get_program(), expected_program);
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            vec![std::ffi::OsStr::new(url)]
        );
    }
}
