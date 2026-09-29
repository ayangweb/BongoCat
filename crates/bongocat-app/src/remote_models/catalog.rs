//! Fetching the remote model library document and its preview images.
//!
//! The catalog lives in a GitHub repository the product does not control, so
//! every fetch is bounded in bytes and time, and every fetch has a CDN fallback:
//! the primary host is unreachable on a meaningful share of the product's
//! networks, and a catalog the reader cannot load is a page that does not work.

use std::{io::Read, sync::Arc, time::Duration};

use bongocat_ui_protocol::SettingsRemoteImageFormat;

/// The catalog document, primary host first.
const CATALOG_DOCUMENT_URLS: [&str; 2] = [
    "https://raw.githubusercontent.com/ayangweb/Awesome-BongoCat/master/README.md",
    "https://cdn.jsdelivr.net/gh/ayangweb/Awesome-BongoCat@master/README.md",
];

/// The catalog document is prose a human maintains; anything past this is not a
/// catalog and refusing it early keeps a runaway response out of memory.
const CATALOG_DOCUMENT_MAXIMUM_BYTES: usize = 2 * 1024 * 1024;

/// One preview image. Larger is not a preview any more.
const PREVIEW_MAXIMUM_BYTES: usize = 5 * 1024 * 1024;

/// How long a single connection may stay silent before the fetch gives up.
const READ_TIMEOUT: Duration = Duration::from_secs(15);

/// How long connecting may take before the fetch gives up.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// The shared HTTP agent every remote fetch goes through.
pub(super) fn build_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout_read(READ_TIMEOUT)
        .build()
}

/// The catalog fetches: the document first, then one preview at a time.
pub(super) struct CatalogFetcher<'a> {
    agent: &'a ureq::Agent,
}

impl<'a> CatalogFetcher<'a> {
    pub(super) fn new(agent: &'a ureq::Agent) -> Self {
        Self { agent }
    }

    /// The catalog document from the first host that answers.
    pub(super) fn fetch_document(&self, is_stopped: &impl Fn() -> bool) -> Option<String> {
        for url in CATALOG_DOCUMENT_URLS {
            if is_stopped() {
                return None;
            }
            if let Some(document) = read_text(self.agent, url, CATALOG_DOCUMENT_MAXIMUM_BYTES) {
                return Some(document);
            }
        }
        None
    }

    /// One preview image with its sniffed encoding, or `None` when the fetch or
    /// the bytes are not something the window can render.
    pub(super) fn fetch_preview(
        &self,
        url: &str,
        is_stopped: &impl Fn() -> bool,
    ) -> Option<(SettingsRemoteImageFormat, Arc<[u8]>)> {
        if is_stopped() {
            return None;
        }
        let bytes = read_bytes(self.agent, url, PREVIEW_MAXIMUM_BYTES)?;
        let format = sniff_image_format(&bytes)?;
        Some((format, bytes.into()))
    }
}

fn read_text(agent: &ureq::Agent, url: &str, maximum_bytes: usize) -> Option<String> {
    let bytes = read_bytes(agent, url, maximum_bytes)?;
    String::from_utf8(bytes).ok()
}

fn read_bytes(agent: &ureq::Agent, url: &str, maximum_bytes: usize) -> Option<Vec<u8>> {
    let response = agent.get(url).call().ok()?;
    let mut reader = response.into_reader();
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = reader.read(&mut buffer).ok()?;
        if read == 0 {
            return Some(bytes);
        }
        if bytes.len() + read > maximum_bytes {
            return None;
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
}

/// Recognizes the image encodings the renderer accepts by their magic bytes.
///
/// A catalog preview is whatever the model author uploaded, so the encoding is
/// decided from the bytes rather than from the URL: the CDN that hosts them does
/// not keep the extension honest.
pub(super) fn sniff_image_format(bytes: &[u8]) -> Option<SettingsRemoteImageFormat> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some(SettingsRemoteImageFormat::Png)
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(SettingsRemoteImageFormat::Jpeg)
    } else if bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some(SettingsRemoteImageFormat::Webp)
    } else if bytes.starts_with(b"GIF8") {
        Some(SettingsRemoteImageFormat::Gif)
    } else {
        None
    }
}

/// Mirrors a GitHub raw download URL onto the CDN fallback.
///
/// `https://github.com/<owner>/<repo>/raw/<branch>/<path>` becomes
/// `https://cdn.jsdelivr.net/gh/<owner>/<repo>@<branch>/<path>`. Anything that is
/// not that exact shape has no fallback, and the caller simply tries the
/// original URL alone.
pub(super) fn fallback_download_url(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://github.com/")?;
    let (repo, path) = rest.split_once("/raw/")?;
    if repo.is_empty() || path.is_empty() || repo.matches('/').count() != 1 {
        return None;
    }
    Some(format!("https://cdn.jsdelivr.net/gh/{repo}@{path}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_renderable_image_encodings() {
        assert_eq!(
            sniff_image_format(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A]),
            Some(SettingsRemoteImageFormat::Png)
        );
        assert_eq!(
            sniff_image_format(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some(SettingsRemoteImageFormat::Jpeg)
        );
        let mut webp = Vec::from(b"RIFF____WEBPVP8 ".as_slice());
        webp[4..8].copy_from_slice(b"\x10\x00\x00\x00");
        assert_eq!(
            sniff_image_format(&webp),
            Some(SettingsRemoteImageFormat::Webp)
        );
        assert_eq!(
            sniff_image_format(b"GIF89a\x01\x00"),
            Some(SettingsRemoteImageFormat::Gif)
        );
        assert_eq!(sniff_image_format(b"<html>"), None);
        assert_eq!(sniff_image_format(&[]), None);
    }

    #[test]
    fn mirrors_github_raw_urls_onto_the_cdn() {
        assert_eq!(
            fallback_download_url(
                "https://github.com/ayangweb/Awesome-BongoCat/raw/master/models/Chinese/%E7%BB%8F%E5%85%B8.zip"
            ),
            Some(
                "https://cdn.jsdelivr.net/gh/ayangweb/Awesome-BongoCat@master/models/Chinese/%E7%BB%8F%E5%85%B8.zip"
                    .to_owned()
            )
        );
    }

    #[test]
    fn refuses_urls_without_a_fallback_shape() {
        assert_eq!(
            fallback_download_url("https://raw.githubusercontent.com/a/b/master/x.zip"),
            None
        );
        assert_eq!(
            fallback_download_url("https://github.com/only-owner/"),
            None
        );
        assert_eq!(
            fallback_download_url("https://github.com/a/b/blob/main/x.zip"),
            None
        );
        assert_eq!(
            fallback_download_url("http://github.com/a/b/raw/main/x.zip"),
            None
        );
    }
}
