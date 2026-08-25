//! Single-instance guard for the desktop shell (issue #16).
//!
//! Mechanism: one advisory exclusive file lock held for the process
//! lifetime on `<data dir>/openstream.lock` (stable `std::fs` file
//! locking). The operating system releases the lock when the holding
//! process exits for ANY reason — including a crash — so a previous crash
//! can never wedge startup behind a stale marker.
//!
//! A second launch acquires nothing, reports
//! [`InstanceLockError::AlreadyRunning`], and exits before creating any
//! window, tray icon, or journal connection: there is never a second
//! writer to the store and no double tray.

use std::fmt;
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

/// Lock file name inside the data directory.
pub const LOCK_FILE_NAME: &str = "openstream.lock";

/// Typed single-instance failures. Variants carry no OS message text or
/// path fragments (redaction discipline matches the persistence layer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceLockError {
    /// Another OpenStream instance holds the lock.
    AlreadyRunning,
    /// The lock directory or file could not be created or opened.
    LockUnavailable,
}

impl fmt::Display for InstanceLockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyRunning => f.write_str("already-running"),
            Self::LockUnavailable => f.write_str("lock-unavailable"),
        }
    }
}

/// Held exclusive instance lock. The wrapped [`File`] is load-bearing:
/// dropping it releases the OS lock.
pub struct InstanceLock {
    _file: File,
}

impl fmt::Debug for InstanceLock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InstanceLock(held)")
    }
}

impl InstanceLock {
    /// Path of the lock file for a data directory (exposed for docs/tests).
    #[must_use]
    pub fn lock_path(data_dir: &Path) -> PathBuf {
        data_dir.join(LOCK_FILE_NAME)
    }

    /// Acquires the exclusive instance lock inside `data_dir`, creating the
    /// directory when needed.
    ///
    /// # Errors
    /// [`InstanceLockError::AlreadyRunning`] when another process holds the
    /// lock; [`InstanceLockError::LockUnavailable`] when the lock file or
    /// its directory cannot be created/opened/locked for any other reason.
    pub fn acquire(data_dir: &Path) -> Result<Self, InstanceLockError> {
        std::fs::create_dir_all(data_dir).map_err(|_| InstanceLockError::LockUnavailable)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(Self::lock_path(data_dir))
            .map_err(|_| InstanceLockError::LockUnavailable)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            // WouldBlock = another process holds the lock right now. Any
            // other failure class refuses rather than guessing.
            Err(TryLockError::WouldBlock) => Err(InstanceLockError::AlreadyRunning),
            Err(_) => Err(InstanceLockError::LockUnavailable),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{InstanceLock, InstanceLockError};
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn first_instance_acquires_and_second_is_refused() {
        let dir = TempDir::new().expect("temp dir");
        let _first = InstanceLock::acquire(dir.path()).expect("first acquire");
        let second = InstanceLock::acquire(dir.path());
        assert_eq!(second.err(), Some(InstanceLockError::AlreadyRunning));
    }

    #[test]
    fn released_lock_is_reacquirable() {
        let dir = TempDir::new().expect("temp dir");
        {
            let _held = InstanceLock::acquire(dir.path()).expect("first acquire");
        }
        let reacquired = InstanceLock::acquire(dir.path());
        assert!(
            reacquired.is_ok(),
            "a crashed/exited holder must never wedge startup"
        );
    }

    #[test]
    fn creates_missing_data_directory() {
        let dir = TempDir::new().expect("temp dir");
        let nested = dir.path().join("a").join("b");
        let _held = InstanceLock::acquire(&nested).expect("acquire in fresh tree");
        assert!(nested.join(super::LOCK_FILE_NAME).is_file());
    }

    #[test]
    fn unusable_lock_location_fails_closed() {
        let dir = TempDir::new().expect("temp dir");
        // A FILE where the lock DIRECTORY should be makes create_dir_all
        // fail; the guard must refuse, not improvise.
        let blocker = dir.path().join("blocker");
        fs::write(&blocker, b"not a directory").expect("write blocker");
        let outcome = InstanceLock::acquire(&blocker);
        assert_eq!(outcome.err(), Some(InstanceLockError::LockUnavailable));
    }

    #[test]
    fn error_display_stays_in_the_closed_vocabulary() {
        assert_eq!(
            InstanceLockError::AlreadyRunning.to_string(),
            "already-running"
        );
        assert_eq!(
            InstanceLockError::LockUnavailable.to_string(),
            "lock-unavailable"
        );
    }
}
