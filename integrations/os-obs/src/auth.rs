//! OBS WebSocket v5 challenge authentication.
//!
//! Computes the obs-websocket 5.x challenge response:
//! `base64(sha256(base64(sha256(password + salt)) + challenge))`.
//!
//! Hard rule: the raw connection password exists only as the `&str` borrow
//! handed in by the vault-resolved [`openstream_domain::secret::SecretValue`]
//! guard for the duration of this computation. It is never copied into any
//! owned buffer, frame, log, or error value; only the derived hash leaves
//! this module.

use base64::Engine as _;
use sha2::{Digest, Sha256};

/// The standard unpadded base64 engine used by obs-websocket.
fn b64(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn sha256_hex_b64(data: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().to_vec()
}

/// Derives the authentication hash for one challenge/salt pair.
#[must_use]
pub fn challenge_response(password: &str, salt: &str, challenge: &str) -> String {
    let mut first = password.to_string();
    first.push_str(salt);
    let inner = b64(&sha256_hex_b64(first.as_bytes()));
    let mut second = inner;
    second.push_str(challenge);
    b64(&sha256_hex_b64(second.as_bytes()))
}
