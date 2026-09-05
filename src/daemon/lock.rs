//! A single-instance lock, so only one daemon ever reports.
//!
//! herdr re-runs a plugin's `[[startup]]` hooks on a live handoff, and nothing
//! stops someone starting a second daemon by hand. Two daemons reporting to
//! the same metadata source would fight over the sequence number and halve the
//! effective refresh rate for no benefit.
//!
//! The lock is an OS advisory lock on a file in the plugin's state directory:
//! `flock(LOCK_EX | LOCK_NB)` on Unix, `LockFileEx` with
//! `LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY` on Windows. Both are
//! released by the kernel when the holding process exits or is killed, so
//! there is no stale pid to reason about and nothing to clean up after a
//! crash.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// The lock file name, inside [`lock_dir`].
pub const LOCK_FILE_NAME: &str = "herdr-mem-cpu-load.lock";

/// Where the lock file lives: the plugin's herdr state directory, or the
/// system temporary directory for a daemon started by hand.
#[must_use]
pub fn lock_dir() -> PathBuf {
    std::env::var_os("HERDR_PLUGIN_STATE_DIR")
        .filter(|dir| !dir.is_empty())
        .map_or_else(std::env::temp_dir, PathBuf::from)
}

/// The full path of the lock file.
#[must_use]
pub fn lock_path() -> PathBuf {
    lock_dir().join(LOCK_FILE_NAME)
}

/// A held lock. Dropping it closes the file, which releases the lock.
#[derive(Debug)]
pub struct DaemonLock {
    // Held for the daemon's lifetime: the lock belongs to the open file, not
    // to the path, so closing this file is what unlocks it.
    file: std::fs::File,
    path: PathBuf,
}

impl DaemonLock {
    /// Take the lock at `path`.
    ///
    /// Returns `Ok(None)` when another process already holds it, which is the
    /// ordinary "a daemon is already running" case rather than a failure.
    ///
    /// # Errors
    ///
    /// Returns an [`io::Error`] when the lock file cannot be created or the
    /// lock call fails for a reason other than contention.
    pub fn acquire(path: &Path) -> io::Result<Option<Self>> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)?;

        if !try_lock(&file)? {
            return Ok(None);
        }

        // Purely for a human reading the file; nothing depends on it.
        let _ = file.set_len(0);
        let _ = writeln!(file, "{}", std::process::id());
        let _ = file.flush();

        Ok(Some(Self {
            file,
            path: path.to_path_buf(),
        }))
    }

    /// The file this lock is held on.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The pid written into the lock file, for tests and diagnostics.
    #[must_use]
    pub fn pid(&self) -> u32 {
        std::process::id()
    }
}

impl Drop for DaemonLock {
    fn drop(&mut self) {
        // Closing the file is what releases the lock; make it explicit that
        // the handle is the resource being held.
        let _ = self.file.flush();
    }
}

#[cfg(unix)]
fn try_lock(file: &std::fs::File) -> io::Result<bool> {
    use std::os::unix::io::AsRawFd;

    // SAFETY: `as_raw_fd` hands out a descriptor that stays open for as long
    // as `file` is alive, which outlives this call.
    let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if locked == 0 {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        // EAGAIN and EWOULDBLOCK are the same number on every Unix that
        // matters, but both spellings exist; matching the error kind covers
        // whichever libc names it.
        Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN => Ok(false),
        _ => Err(error),
    }
}

#[cfg(windows)]
fn try_lock(file: &std::fs::File) -> io::Result<bool> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        LockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY,
    };
    use windows_sys::Win32::System::IO::OVERLAPPED;

    /// `ERROR_LOCK_VIOLATION`: somebody else holds the range.
    const ERROR_LOCK_VIOLATION: i32 = 33;

    // SAFETY: the handle stays open for as long as `file` is alive, and the
    // OVERLAPPED is a zeroed local that outlives the (immediate) call.
    let locked = unsafe {
        let mut overlapped: OVERLAPPED = std::mem::zeroed();
        LockFileEx(
            file.as_raw_handle().cast(),
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            u32::MAX,
            u32::MAX,
            &raw mut overlapped,
        )
    };
    if locked != 0 {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(ERROR_LOCK_VIOLATION) {
        Ok(false)
    } else {
        Err(error)
    }
}

#[cfg(not(any(unix, windows)))]
fn try_lock(_file: &std::fs::File) -> io::Result<bool> {
    // No advisory locking here; let the daemon start rather than refusing to.
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::{lock_path, DaemonLock, LOCK_FILE_NAME};
    use std::path::PathBuf;

    fn temp_lock(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "herdr-mem-cpu-load-lock-{}-{name}",
            std::process::id()
        ));
        path
    }

    #[test]
    fn the_lock_path_follows_the_plugin_state_directory() {
        // The environment is shared between tests in a process, so this only
        // checks the shape rather than setting the variable.
        assert!(lock_path().ends_with(LOCK_FILE_NAME));
    }

    #[cfg(unix)]
    #[test]
    fn a_second_handle_on_the_same_file_cannot_lock_it() {
        let path = temp_lock("second-handle");
        std::fs::remove_file(&path).ok();

        let first = DaemonLock::acquire(&path)
            .expect("the lock file opens")
            .expect("the first acquire wins");

        // flock() belongs to the open file description, so a second open() of
        // the same path is a different holder even inside one process.
        let second = DaemonLock::acquire(&path).expect("the lock file opens");
        assert!(second.is_none(), "the second acquire must find it held");

        let written = std::fs::read_to_string(&path).expect("the pid was written");
        assert_eq!(written.trim(), std::process::id().to_string());
        assert_eq!(first.path(), path);
        assert_eq!(first.pid(), std::process::id());

        drop(first);
        let third = DaemonLock::acquire(&path).expect("the lock file opens");
        assert!(third.is_some(), "dropping the lock releases it");
        drop(third);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn the_lock_file_and_its_directory_are_created_on_demand() {
        let mut dir = temp_lock("nested-dir");
        dir.push("state");
        let path = dir.join(LOCK_FILE_NAME);
        std::fs::remove_dir_all(dir.parent().expect("has a parent")).ok();

        let lock = DaemonLock::acquire(&path)
            .expect("the directory is created")
            .expect("the lock is free");
        assert!(path.exists());
        drop(lock);
        std::fs::remove_dir_all(dir.parent().expect("has a parent")).ok();
    }
}
