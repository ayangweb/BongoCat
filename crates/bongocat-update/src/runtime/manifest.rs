//! Where the manifest is requested from.
//!
//! One shared manifest announces a `<os>-<arch>` entry per shipped target, and
//! this is the code that asks for it through each proxy prefix in turn before
//! falling back to the official endpoint. A fetch that fails is remembered: the
//! prefixes are tried once per run, not once per source per request, or a
//! machine behind a proxy that blocks the first would pay for it on every update
//! check.

use super::*;

/// One manifest source: an endpoint and the proxy prefix that produced it.
///
/// `proxy` is `None` for the official GitHub endpoint, which is tried last and
/// needs no download-URL conversion.
pub(crate) struct ManifestSource {
    pub(crate) proxy: Option<&'static str>,
    pub(crate) endpoint: Url,
}

/// What one successful manifest fetch produced.
#[derive(Debug)]
pub(crate) enum ManifestFetch<T> {
    /// The manifest is readable and announces nothing newer than this build.
    UpToDate,
    /// A newer release is offered, with its download URL already converted to the
    /// proxy that served the manifest (no conversion on the official endpoint).
    Offered(T),
}
