//! Test double for `VersionChecker`. Returns a canned stdout (or canned
//! error) for every `.check()` call regardless of input path. Mirrors the
//! existing boxpilotd `FixedVersionChecker` so existing call sites need only
//! a single import-path change.

use crate::traits::version::VersionChecker;
use std::io;
use std::path::Path;
use std::sync::Mutex;

pub struct FixedVersionChecker {
    pub stdout: Mutex<Result<String, String>>,
}

impl FixedVersionChecker {
    pub fn ok(s: impl Into<String>) -> Self {
        Self {
            stdout: Mutex::new(Ok(s.into())),
        }
    }
    pub fn err(s: impl Into<String>) -> Self {
        Self {
            stdout: Mutex::new(Err(s.into())),
        }
    }
}

impl VersionChecker for FixedVersionChecker {
    fn check(&self, _binary: &Path) -> io::Result<String> {
        self.stdout.lock().unwrap().clone().map_err(io::Error::other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn fixed_ok_returns_stdout() {
        let v = FixedVersionChecker::ok("sing-box version 1.10.0");
        assert!(v.check(Path::new("/x")).unwrap().starts_with("sing-box"));
    }

    #[test]
    fn fixed_err_returns_io_error() {
        let v = FixedVersionChecker::err("crashed");
        let r = v.check(Path::new("/x"));
        assert!(r.is_err());
    }
}
