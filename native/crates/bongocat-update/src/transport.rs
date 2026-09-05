use crate::{
    MAX_UPDATE_MANIFEST_BYTES, UPDATE_MANIFEST_KEY_ID_HEADER, UPDATE_MANIFEST_SIGNATURE_HEADER,
    UpdateError, UpdateErrorCode, UpdateManifestEnvelope,
};
use std::{fmt, io::Read, time::Duration};
use ureq::{Agent, config::Config};
use url::Url;

pub const MAX_UPDATE_MANIFEST_ENDPOINT_BYTES: usize = 2_048;
pub const UPDATE_MANIFEST_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// An immutable release-manifest endpoint supplied by product build metadata.
///
/// The endpoint is intentionally separate from user configuration. Only an
/// HTTPS URL without credentials or a fragment can cross this boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateManifestEndpoint(String);

impl UpdateManifestEndpoint {
    pub fn new(value: impl Into<String>) -> Result<Self, UpdateManifestFetchError> {
        let value = value.into();
        if value.is_empty() || value.len() > MAX_UPDATE_MANIFEST_ENDPOINT_BYTES {
            return Err(UpdateManifestFetchError::transport(
                UpdateManifestTransportErrorCode::EndpointInvalid,
            ));
        }

        let url = Url::parse(&value).map_err(|_| {
            UpdateManifestFetchError::transport(UpdateManifestTransportErrorCode::EndpointInvalid)
        })?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(UpdateManifestFetchError::transport(
                UpdateManifestTransportErrorCode::EndpointInvalid,
            ));
        }

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Stable categories for failures before a signed manifest envelope exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateManifestTransportErrorCode {
    EndpointInvalid,
    HttpStatusInvalid,
    ResponseBodyReadFailed,
    TransportFailed,
}

impl UpdateManifestTransportErrorCode {
    pub const ALL: [Self; 4] = [
        Self::EndpointInvalid,
        Self::HttpStatusInvalid,
        Self::ResponseBodyReadFailed,
        Self::TransportFailed,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::EndpointInvalid => "update_manifest_endpoint_invalid",
            Self::HttpStatusInvalid => "update_manifest_http_status_invalid",
            Self::ResponseBodyReadFailed => "update_manifest_response_body_read_failed",
            Self::TransportFailed => "update_manifest_transport_failed",
        }
    }
}

/// A fetch failure never exposes a response body, URL, status text, or HTTP
/// client type. Envelope validation retains the existing verifier error code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateManifestFetchError {
    Manifest(UpdateError),
    Transport(UpdateManifestTransportErrorCode),
}

impl UpdateManifestFetchError {
    const fn transport(code: UpdateManifestTransportErrorCode) -> Self {
        Self::Transport(code)
    }

    pub const fn code(&self) -> &'static str {
        match self {
            Self::Manifest(error) => error.code.as_str(),
            Self::Transport(error) => error.as_str(),
        }
    }
}

impl fmt::Display for UpdateManifestFetchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for UpdateManifestFetchError {}

/// Fetches one raw signed manifest envelope from a fixed endpoint.
///
/// This trait exists so the verification coordinator can be tested without
/// networking and so an app-owned worker does not need to know the HTTP client.
pub trait UpdateManifestSource {
    fn fetch(
        &self,
        endpoint: &UpdateManifestEndpoint,
    ) -> Result<UpdateManifestEnvelope, UpdateManifestFetchError>;
}

/// A blocking HTTPS transport for the signed manifest envelope.
///
/// Callers must execute `fetch` on a dedicated update worker; this type never
/// belongs on the GPUI executor. It sends one GET, follows no redirects, keeps
/// compressed transfer encodings disabled, and returns raw bounded bytes only.
#[derive(Clone)]
pub struct UreqUpdateManifestSource {
    agent: Agent,
}

impl Default for UreqUpdateManifestSource {
    fn default() -> Self {
        Self::new()
    }
}

impl UreqUpdateManifestSource {
    pub fn new() -> Self {
        let agent = Config::builder()
            .https_only(true)
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_global(Some(UPDATE_MANIFEST_REQUEST_TIMEOUT))
            .build()
            .into();
        Self { agent }
    }

    pub fn fetch(
        &self,
        endpoint: &UpdateManifestEndpoint,
    ) -> Result<UpdateManifestEnvelope, UpdateManifestFetchError> {
        let mut response = self.agent.get(endpoint.as_str()).call().map_err(|_| {
            UpdateManifestFetchError::transport(UpdateManifestTransportErrorCode::TransportFailed)
        })?;
        if response.status().as_u16() != 200 {
            return Err(UpdateManifestFetchError::transport(
                UpdateManifestTransportErrorCode::HttpStatusInvalid,
            ));
        }

        let key_id = response
            .headers()
            .get(UPDATE_MANIFEST_KEY_ID_HEADER)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let signature = response
            .headers()
            .get(UPDATE_MANIFEST_SIGNATURE_HEADER)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let body = read_manifest_body(response.body_mut())?;

        UpdateManifestEnvelope::from_headers(body, key_id, &signature)
            .map_err(UpdateManifestFetchError::Manifest)
    }

    #[cfg(test)]
    fn config(&self) -> &ureq::config::Config {
        self.agent.config()
    }
}

impl UpdateManifestSource for UreqUpdateManifestSource {
    fn fetch(
        &self,
        endpoint: &UpdateManifestEndpoint,
    ) -> Result<UpdateManifestEnvelope, UpdateManifestFetchError> {
        Self::fetch(self, endpoint)
    }
}

fn read_manifest_body(body: &mut ureq::Body) -> Result<Vec<u8>, UpdateManifestFetchError> {
    if body
        .content_length()
        .is_some_and(|length| length > MAX_UPDATE_MANIFEST_BYTES as u64)
    {
        return Err(UpdateManifestFetchError::Manifest(UpdateError {
            code: UpdateErrorCode::ManifestTooLarge,
        }));
    }

    let mut bytes = Vec::with_capacity(
        body.content_length()
            .unwrap_or_default()
            .min(MAX_UPDATE_MANIFEST_BYTES as u64) as usize,
    );
    body.as_reader()
        .take(MAX_UPDATE_MANIFEST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| {
            UpdateManifestFetchError::transport(
                UpdateManifestTransportErrorCode::ResponseBodyReadFailed,
            )
        })?;
    if bytes.is_empty() || bytes.len() > MAX_UPDATE_MANIFEST_BYTES {
        return Err(UpdateManifestFetchError::Manifest(UpdateError {
            code: UpdateErrorCode::ManifestTooLarge,
        }));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_UPDATE_MANIFEST_ENDPOINT_BYTES, UpdateManifestEndpoint, UpdateManifestFetchError,
        UpdateManifestTransportErrorCode, UreqUpdateManifestSource,
    };
    use crate::UpdateErrorCode;
    use std::collections::BTreeSet;

    #[test]
    fn endpoint_accepts_only_bounded_credential_free_https_urls() {
        let endpoint = UpdateManifestEndpoint::new("https://updates.example.invalid/manifest.json")
            .expect("HTTPS endpoint");
        assert_eq!(
            endpoint.as_str(),
            "https://updates.example.invalid/manifest.json"
        );

        for invalid in [
            "http://updates.example.invalid/manifest.json",
            "https://user:secret@updates.example.invalid/manifest.json",
            "https://updates.example.invalid/manifest.json#fragment",
            "https://",
        ] {
            assert!(matches!(
                UpdateManifestEndpoint::new(invalid),
                Err(UpdateManifestFetchError::Transport(
                    UpdateManifestTransportErrorCode::EndpointInvalid
                ))
            ));
        }
        assert!(matches!(
            UpdateManifestEndpoint::new(format!(
                "https://updates.example.invalid/{}",
                "x".repeat(MAX_UPDATE_MANIFEST_ENDPOINT_BYTES)
            )),
            Err(UpdateManifestFetchError::Transport(
                UpdateManifestTransportErrorCode::EndpointInvalid
            ))
        ));
    }

    #[test]
    fn source_forces_https_raw_responses_and_no_redirects() {
        let source = UreqUpdateManifestSource::new();
        assert!(source.config().https_only());
        assert!(!source.config().http_status_as_error());
        assert_eq!(source.config().max_redirects(), 0);
    }

    #[test]
    fn fetch_errors_expose_only_stable_codes() {
        let manifest = UpdateManifestFetchError::Manifest(crate::UpdateError {
            code: UpdateErrorCode::ManifestTooLarge,
        });
        assert_eq!(manifest.code(), "manifest_too_large");

        let codes = UpdateManifestTransportErrorCode::ALL
            .into_iter()
            .map(UpdateManifestTransportErrorCode::as_str)
            .collect::<BTreeSet<_>>();
        assert_eq!(codes.len(), UpdateManifestTransportErrorCode::ALL.len());
        assert!(
            codes
                .iter()
                .all(|code| code.starts_with("update_manifest_"))
        );
    }
}
