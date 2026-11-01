//! Room keys.
//!
//! A token is only ever compared against another token, never used as a key or
//! fed into anything cryptographic, so the requirement is unpredictability for
//! someone on the network rather than resistance to offline attack.

use std::io::Read;

/// 32 bytes from the OS entropy pool, rendered as lowercase hex.
///
/// Falls back to a weaker derivation only when the entropy device cannot be
/// opened, which should not happen on the platforms this ships to; the fallback
/// exists so a relay can still start on an exotic host rather than refusing.
pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    if let Ok(mut entropy) = std::fs::File::open("/dev/urandom")
        && entropy.read_exact(&mut bytes).is_ok()
    {
        return to_hex(&bytes);
    }
    fallback_token()
}

fn fallback_token() -> String {
    // Mixing the clock with the pid keeps repeated starts from colliding; it is
    // deliberately not claimed to be strong, only better than a constant.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos() as u64)
        .unwrap_or_default();
    let pid = std::process::id() as u64;
    let mut state = nanos ^ (pid << 32) ^ 0x9E37_79B9_7F4A_7C15;
    let mut bytes = [0u8; 32];
    for chunk in bytes.chunks_mut(8) {
        // xorshift64*: cheap, and its poor statistical quality does not matter
        // for a value that is only ever compared for equality.
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let mixed = state.wrapping_mul(0x2545_F491_4F6C_DD1D);
        chunk.copy_from_slice(&mixed.to_le_bytes()[..chunk.len()]);
    }
    to_hex(&bytes)
}

fn to_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_hex_and_unique() {
        let first = generate_token();
        let second = generate_token();
        assert_eq!(first.len(), 64, "32 bytes render as 64 hex characters");
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(first, second, "two calls must not collide");
    }
}
