//! Windows implementation of [`FileLock`] using `LockFileEx` with the
//! `LOCKFILE_FAIL_IMMEDIATELY` flag for non-blocking acquisition. The
//! lock is bound to the underlying file handle, so a crashed helper
//! releases its lock automatically when the kernel closes its handles.

use crate::traits::lock::FileLock;
use boxpilot_ipc::HelperError;
use std::fs::File;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use windows_sys::Win32::Storage::FileSystem::{
    LockFileEx, UnlockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY,
};
use windows_sys::Win32::System::IO::OVERLAPPED;

pub struct LockFileExLock;

pub struct LockGuard {
    file: File,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        unsafe {
            let mut o: OVERLAPPED = std::mem::zeroed();
            UnlockFileEx(self.file.as_raw_handle() as _, 0, u32::MAX, u32::MAX, &mut o);
        }
    }
}

impl FileLock for LockFileExLock {
    type Guard = LockGuard;

    fn try_acquire(&self, path: &Path) -> Result<LockGuard, HelperError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| HelperError::Ipc {
                message: format!("create lock parent dir: {e}"),
            })?;
        }
        let file = File::create(path).map_err(|e| HelperError::Ipc {
            message: format!("create lock file: {e}"),
        })?;
        unsafe {
            let mut o: OVERLAPPED = std::mem::zeroed();
            let ok = LockFileEx(
                file.as_raw_handle() as _,
                LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                0,
                u32::MAX,
                u32::MAX,
                &mut o,
            );
            if ok == 0 {
                return Err(HelperError::Busy);
            }
        }
        Ok(LockGuard { file })
    }
}
