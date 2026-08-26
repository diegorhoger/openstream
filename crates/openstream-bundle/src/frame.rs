//! Binary container framing for `.openstream` bundles
//! (`PORTABILITY_BUNDLES.md` §3).
//!
//! The frame is a fixed magic, a u32 container version, and a sequence of
//! length-prefixed members:
//!
//! ```text
//! [u8; 8]   magic "OSTRBNDL"
//! [u32 LE]  container format version (1)
//! [u32 LE]  member count
//! member*:
//!   [u32 LE] name_len | name bytes (closed vocabulary, see member.rs)
//!   [u32 LE] raw_len  (uncompressed size in bytes)
//!   [u8]     compression (0 = stored, 1 = deflate)
//!   [u32 LE] stored_len (bytes that follow)
//!   payload[stored_len]
//! ```
//!
//! Every bound from [`crate::limits`] is enforced before the allocation or
//! decompression it protects: caps on file size, member count, name length,
//! declared raw lengths, summed uncompressed sizes, and deflation ratios.
//! Decoded members must match their declared `raw_len` exactly, trailing
//! bytes after the last member reject, and reading never interprets content
//! — hashing, manifest parsing, and document decoding happen strictly above
//! this layer.

use crate::error::BundleError;
use crate::limits::{
    BUNDLE_FORMAT_VERSION, MAGIC, MAX_BUNDLE_FILE_BYTES, MAX_BUNDLE_UNCOMPRESSED_BYTES,
    MAX_DECOMPRESSION_RATIO, MAX_MEMBER_COUNT, MAX_MEMBER_NAME_BYTES,
    MAX_MEMBER_UNCOMPRESSED_BYTES, RATIO_GUARD_FLOOR_BYTES,
};

/// Compression codec of one framed member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    /// Payload bytes are used verbatim. This is the only codec the builder
    /// writes, which keeps exports deterministic.
    Stored,
    /// Payload bytes are a DEFLATE stream (RFC 1951) expanding exactly to
    /// `raw_len` bytes. Accepted on import under strict ratio and absolute
    /// caps so third-party compressed bundles cannot become bombs.
    Deflate,
}

impl Codec {
    fn from_byte(byte: u8) -> Result<Self, BundleError> {
        match byte {
            0 => Ok(Self::Stored),
            1 => Ok(Self::Deflate),
            _ => Err(BundleError::MalformedFrame {
                reason: "unknown compression byte",
            }),
        }
    }

    const fn to_byte(self) -> u8 {
        match self {
            Self::Stored => 0,
            Self::Deflate => 1,
        }
    }
}

/// One framed member before content interpretation.
#[derive(Debug, Clone)]
pub(crate) struct FramedMember {
    /// Validated closed-vocabulary name.
    pub name: String,
    /// Decompressed payload bytes (length exactly the declared `raw_len`).
    pub raw: Vec<u8>,
}

/// Reads the full member sequence out of a whole serialized bundle.
///
/// Every structural defense runs here, before any caller sees member
/// payloads: magic, container version, member count, name grammar, per-
/// member and total size caps, decompression ratio guard, exact-length
/// verification, and trailing-byte rejection.
pub(crate) fn read_frame(input: &[u8]) -> Result<Vec<FramedMember>, BundleError> {
    if input.len() > MAX_BUNDLE_FILE_BYTES {
        return Err(BundleError::TooLarge {
            what: "bundle file",
            limit: MAX_BUNDLE_FILE_BYTES,
        });
    }
    let mut cursor = input;
    let magic = take(&mut cursor, MAGIC.len()).ok_or(malformed("truncated magic"))?;
    if magic != MAGIC {
        return Err(BundleError::InvalidMagic);
    }
    let found_version = read_u32(&mut cursor).ok_or(malformed("truncated version"))?;
    if found_version != BUNDLE_FORMAT_VERSION {
        return Err(BundleError::UnsupportedContainerVersion {
            found: found_version,
            supported: BUNDLE_FORMAT_VERSION,
        });
    }
    let member_count = read_u32(&mut cursor).ok_or(malformed("truncated member count"))?;
    if member_count as usize > MAX_MEMBER_COUNT {
        return Err(BundleError::TooLarge {
            what: "member count",
            limit: MAX_MEMBER_COUNT,
        });
    }

    let mut members = Vec::new();
    // Summed uncompressed size across all members; every member adds its
    // verified raw length and the running total may never pass the cap.
    let mut total_raw: usize = 0;
    for _ in 0..member_count {
        let name_len = read_u32(&mut cursor).ok_or(malformed("truncated name length"))? as usize;
        if name_len == 0 || name_len > MAX_MEMBER_NAME_BYTES {
            return Err(name_error("name length out of range"));
        }
        let name_bytes = take(&mut cursor, name_len).ok_or(malformed("truncated name"))?;
        let Ok(name) = std::str::from_utf8(name_bytes) else {
            return Err(name_error("non-utf8 name"));
        };
        let raw_len = read_u32(&mut cursor).ok_or(malformed("truncated raw length"))? as usize;
        if raw_len > MAX_MEMBER_UNCOMPRESSED_BYTES {
            return Err(BundleError::TooLarge {
                what: "member raw size",
                limit: MAX_MEMBER_UNCOMPRESSED_BYTES,
            });
        }
        let codec = Codec::from_byte(
            take(&mut cursor, 1).ok_or(malformed("truncated compression byte"))?[0],
        )?;
        let stored_len =
            read_u32(&mut cursor).ok_or(malformed("truncated stored length"))? as usize;

        // Ratio guard BEFORE any decompression allocation. Members at or
        // below the floor are exempt because their worst-case expansion is
        // already bounded by the absolute caps above.
        if codec == Codec::Deflate && stored_len as u64 > RATIO_GUARD_FLOOR_BYTES {
            let max_raw = stored_len as u64 * MAX_DECOMPRESSION_RATIO;
            if raw_len as u64 > max_raw {
                return Err(BundleError::CompressionRatioExceeded {
                    max_ratio: MAX_DECOMPRESSION_RATIO,
                });
            }
        }

        let stored = take(&mut cursor, stored_len).ok_or(malformed("truncated payload"))?;
        let raw = decode_payload(codec, stored, raw_len)?;
        total_raw += raw.len();
        if total_raw > MAX_BUNDLE_UNCOMPRESSED_BYTES {
            return Err(BundleError::TooLarge {
                what: "total uncompressed size",
                limit: MAX_BUNDLE_UNCOMPRESSED_BYTES,
            });
        }
        members.push(FramedMember {
            name: name.to_owned(),
            raw,
        });
    }
    if !cursor.is_empty() {
        return Err(malformed("trailing bytes after last member"));
    }
    Ok(members)
}

fn decode_payload(codec: Codec, stored: &[u8], raw_len: usize) -> Result<Vec<u8>, BundleError> {
    match codec {
        Codec::Stored => {
            if stored.len() != raw_len {
                return Err(malformed("stored length lies about raw length"));
            }
            Ok(stored.to_vec())
        }
        Codec::Deflate => {
            use std::io::Read as _;
            let mut decoder = flate2::read::DeflateDecoder::new(stored);
            // Read at most one byte beyond the declaration so a lying
            // `raw_len` fails closed instead of growing unbounded.
            let mut limited = decoder.by_ref().take(raw_len.saturating_add(1) as u64);
            let mut out = Vec::with_capacity(raw_len.min(MAX_MEMBER_UNCOMPRESSED_BYTES));
            limited
                .read_to_end(&mut out)
                .map_err(|_| malformed("deflate failure"))?;
            if out.len() != raw_len {
                return Err(malformed("decompressed length mismatch"));
            }
            Ok(out)
        }
    }
}

/// Serializes members into the deterministic v1 container form. Callers
/// guarantee the closed-vocabulary names and ordering; the frame layer only
/// adds framing bytes.
pub(crate) fn write_frame(members: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&BUNDLE_FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&(members.len() as u32).to_le_bytes());
    for (name, raw) in members {
        out.extend_from_slice(&(name.len() as u32).to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&(raw.len() as u32).to_le_bytes());
        out.push(Codec::Stored.to_byte());
        out.extend_from_slice(&(raw.len() as u32).to_le_bytes());
        out.extend_from_slice(raw);
    }
    out
}

fn malformed(reason: &'static str) -> BundleError {
    BundleError::MalformedFrame { reason }
}

fn name_error(reason: &'static str) -> BundleError {
    BundleError::IllegalMemberName { reason }
}

fn take<'a>(cursor: &mut &'a [u8], len: usize) -> Option<&'a [u8]> {
    if cursor.len() < len {
        return None;
    }
    let (head, tail) = cursor.split_at(len);
    *cursor = tail;
    Some(head)
}

fn read_u32(cursor: &mut &[u8]) -> Option<u32> {
    let bytes = take(cursor, 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}
