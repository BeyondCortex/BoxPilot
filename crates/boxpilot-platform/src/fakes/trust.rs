//! Fake [`TrustChecker`] impls for tests. `AlwaysTrust` passes through,
//! `AlwaysReject` returns the configured `TrustError`. Use these to bypass
//! or simulate trust failures without setting up a `FakeFs` full of
//! root-owned 0755 directories.

use crate::traits::trust::{TrustChecker, TrustError};
use std::path::{Path, PathBuf};

pub struct AlwaysTrust;

impl TrustChecker for AlwaysTrust {
    fn check(&self, path: &Path, _: &[PathBuf]) -> Result<PathBuf, TrustError> {
        Ok(path.to_path_buf())
    }
}

pub struct AlwaysReject {
    pub reason: TrustError,
}

impl TrustChecker for AlwaysReject {
    fn check(&self, _: &Path, _: &[PathBuf]) -> Result<PathBuf, TrustError> {
        Err(self.reason.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_trust_returns_path_unchanged() {
        let r = AlwaysTrust.check(Path::new("/anywhere/sing-box"), &[]);
        assert_eq!(r.unwrap(), PathBuf::from("/anywhere/sing-box"));
    }

    #[test]
    fn always_reject_returns_configured_error() {
        let r = AlwaysReject {
            reason: TrustError::DisallowedPrefix(PathBuf::from("/x")),
        }
        .check(Path::new("/x"), &[]);
        assert!(matches!(r, Err(TrustError::DisallowedPrefix(_))));
    }
}
