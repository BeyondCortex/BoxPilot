use crate::traits::env::{EnvError, EnvProvider};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct FixedEnv {
    pub system_root: PathBuf,
    pub user_root: PathBuf,
}

impl FixedEnv {
    pub fn under(tmp: &std::path::Path) -> Self {
        Self {
            system_root: tmp.to_path_buf(),
            user_root: tmp.join("user"),
        }
    }
}

impl EnvProvider for FixedEnv {
    fn system_root(&self) -> Result<PathBuf, EnvError> {
        Ok(self.system_root.clone())
    }
    fn user_root(&self) -> Result<PathBuf, EnvError> {
        Ok(self.user_root.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::env::EnvProvider;
    use tempfile::tempdir;

    #[test]
    fn under_tmp_returns_system_and_user_root_under_tmp() {
        let tmp = tempdir().unwrap();
        let env = FixedEnv::under(tmp.path());
        assert_eq!(env.system_root().unwrap(), tmp.path());
        assert_eq!(env.user_root().unwrap(), tmp.path().join("user"));
    }
}
