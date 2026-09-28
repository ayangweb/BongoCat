//! What may leave the process.
//!
//! A Cubism log line can carry a file path or a number that came from a user's
//! document, so nothing reaches the product log without passing here first. The
//! rule is a closed vocabulary rather than a filter: a token is either something
//! the product itself writes — a level, a frame count, a known keyword — or it
//! is not written at all. A blacklist would be the wrong shape, because a
//! blacklist has to have already thought of the sensitive shape.

pub(crate) const MAX_MESSAGE_BYTES: usize = 512;

pub(crate) const MAX_SAFE_CORE_TOKEN_BYTES: usize = 64;

pub(crate) const REDACTED_CORE_TOKEN: &str = "<redacted>";

pub(crate) fn sanitize_message(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(MAX_MESSAGE_BYTES)]);
    let tokens = text.split_whitespace().collect::<Vec<_>>();

    // A Core callback has no typed field boundary. Keep only short, plain
    // diagnostic words and numbers; punctuation-bearing tokens are commonly
    // paths, URLs, assignments, or serialized resource fragments. Redacting the
    // whole message when it contains a sensitive marker also prevents a value
    // following a word such as `token` or `password` from being retained.
    if tokens.iter().any(|token| is_sensitive_core_token(token)) {
        return REDACTED_CORE_TOKEN.to_owned();
    }

    let mut output = String::new();
    for token in tokens {
        let rendered = if is_safe_core_token(token) {
            token
        } else {
            REDACTED_CORE_TOKEN
        };
        if !output.is_empty() {
            if output.len().saturating_add(1) >= MAX_MESSAGE_BYTES {
                break;
            }
            output.push(' ');
        }
        let remaining = MAX_MESSAGE_BYTES.saturating_sub(output.len());
        if remaining == 0 {
            break;
        }
        if rendered.len() > remaining {
            output.push_str(&rendered[..remaining]);
            break;
        }
        output.push_str(rendered);
    }
    if output.is_empty() {
        REDACTED_CORE_TOKEN.to_owned()
    } else {
        output
    }
}

pub(crate) fn is_safe_core_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= MAX_SAFE_CORE_TOKEN_BYTES
        && token.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

pub(crate) fn is_sensitive_core_token(token: &str) -> bool {
    let normalized = token.to_ascii_lowercase();
    [
        "key",
        "token",
        "secret",
        "password",
        "passwd",
        "credential",
        "clipboard",
        "url",
        "uri",
        "path",
        "file",
        "config",
        "payload",
        "content",
        "signature",
        "authorization",
        "bearer",
        "permission",
        "denied",
        "error",
        "failed",
        "failure",
        "invalid",
        "corrupt",
        "unavailable",
        "errno",
        "exception",
        "network",
        "connection",
        "socket",
        "address",
        "dns",
        "http",
        "https",
        "proxy",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}
