//! Durable file helpers for local backups (`PORTABILITY_BUNDLES.md` §9).
//!
//! A `.openstream` file written by [`write_bundle_file`] follows the same
//! discipline as the persistence layer's migrations: the bytes land in a
//! sibling temporary file that is fully synced before anything replaces the
//! destination, and the previous destination is rotated aside (`.prev`)
//! rather than destroyed first. Every crash window therefore leaves either
//! the old file intact, or the new file plus a recoverable `.prev` — never
//! a half-written backup.
//!
//! IO failures map to [`BundleError::IoFailed`] with a bare stage token; OS
//! messages and absolute paths never enter error values, matching the
//! redaction posture of `openstream-persistence`.

use crate::error::{self, BundleError};
use crate::limits::MAX_BUNDLE_FILE_BYTES;
use std::fs::File;
use std::io::Read;
use std::path::Path;

/// Writes `bytes` durably to `path`, rotating any previous file aside as
/// `<path>.prev`. On success the previous rotation is cleaned up; on any
/// failure the caller's existing file is untouched whenever the OS allowed
/// it, with the exact recovery story documented in §9.
///
/// # Errors
/// [`BundleError::TooLarge`] when `bytes` exceed
/// [`MAX_BUNDLE_FILE_BYTES`]; otherwise [`BundleError::IoFailed`] naming
/// the failed stage.
pub fn write_bundle_file(path: &Path, bytes: &[u8]) -> Result<(), BundleError> {
    if bytes.len() > MAX_BUNDLE_FILE_BYTES {
        return Err(BundleError::TooLarge {
            what: "bundle file",
            limit: MAX_BUNDLE_FILE_BYTES,
        });
    }
    let tmp = tmp_sibling(path)?;
    {
        let mut file = error::io_failed("create", File::create(&tmp))?;
        error::io_failed("write", std::io::Write::write_all(&mut file, bytes))?;
        // A returned Ok must survive immediate process death: sync the full
        // file contents (including metadata) before any rename happens.
        error::io_failed("sync", file.sync_all())?;
    }
    let prev = prev_sibling(path);
    // Rotate the old artifact aside BEFORE replacing it; if the process dies
    // right after this rename the data lives in `.prev` (§9).
    if prev.exists() {
        let _ = std::fs::remove_file(&prev);
    }
    if path.exists() {
        error::io_failed("rotate", std::fs::rename(path, &prev))?;
    }
    if std::fs::rename(&tmp, path).is_err() {
        // Best-effort rollback: put the rotated copy back so the user keeps
        // their previous backup.
        if prev.exists() {
            let _ = std::fs::rename(&prev, path);
        }
        return Err(BundleError::IoFailed { stage: "replace" });
    }
    let _ = std::fs::remove_file(&prev);
    Ok(())
}

/// Reads one serialized bundle from disk, enforcing the file-size cap from
/// the directory entry itself before any allocation.
///
/// # Errors
/// [`BundleError::TooLarge`] or [`BundleError::IoFailed`].
pub fn read_bundle_file(path: &Path) -> Result<Vec<u8>, BundleError> {
    let mut file = error::io_failed("open", File::open(path))?;
    let metadata = error::io_failed("metadata", file.metadata())?;
    if metadata.len() > MAX_BUNDLE_FILE_BYTES as u64 {
        return Err(BundleError::TooLarge {
            what: "bundle file",
            limit: MAX_BUNDLE_FILE_BYTES,
        });
    }
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    error::io_failed("read", file.read_to_end(&mut bytes))?;
    Ok(bytes)
}

fn tmp_sibling(path: &Path) -> Result<std::path::PathBuf, BundleError> {
    let file_name = path
        .file_name()
        .ok_or(BundleError::IoFailed { stage: "resolve" })?;
    Ok(path.with_file_name(format!("{}.tmp", file_name.to_string_lossy())))
}

fn prev_sibling(path: &Path) -> std::path::PathBuf {
    match path.file_name() {
        Some(file_name) => path.with_file_name(format!("{}.prev", file_name.to_string_lossy())),
        None => path.with_extension("prev"),
    }
}
