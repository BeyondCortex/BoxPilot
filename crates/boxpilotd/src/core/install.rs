//! Install / upgrade pipeline. Single function, branches on whether
//! `current` exists. The caller (iface.rs) already holds the global lock
//! via `dispatch::authorize`'s AuthorizedCall.
#![allow(dead_code)] // task 16 appends install_or_upgrade and adds callers

use boxpilot_ipc::{ArchRequest, HelperError, HelperResult, VersionRequest};

pub fn resolve_arch(req: &ArchRequest) -> HelperResult<&'static str> {
    let arch = match req {
        ArchRequest::Auto => detect_arch()?,
        ArchRequest::Exact { arch } => arch.as_str(),
    };
    match arch {
        "x86_64" | "amd64" => Ok("amd64"), // sing-box releases use amd64 in filenames
        "aarch64" | "arm64" => Ok("arm64"),
        other => Err(HelperError::Ipc {
            message: format!("unsupported architecture: {other}"),
        }),
    }
}

fn detect_arch() -> HelperResult<&'static str> {
    let out = std::process::Command::new("uname")
        .arg("-m")
        .output()
        .map_err(|e| HelperError::Ipc {
            message: format!("uname: {e}"),
        })?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok(match s.as_str() {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        other => {
            return Err(HelperError::Ipc {
                message: format!("unsupported uname -m: {other}"),
            })
        }
    })
}

pub async fn resolve_version(
    req: &VersionRequest,
    github: &dyn crate::core::github::GithubClient,
) -> HelperResult<String> {
    match req {
        VersionRequest::Latest => github.resolve_latest().await,
        VersionRequest::Exact { version } => Ok(version.clone()),
    }
}

pub fn tarball_filename(version: &str, arch: &str) -> String {
    format!("sing-box-{version}-linux-{arch}.tar.gz")
}

pub fn release_url(version: &str, arch: &str) -> String {
    format!(
        "https://github.com/SagerNet/sing-box/releases/download/v{version}/{}",
        tarball_filename(version, arch)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::github::testing::CannedGithubClient;

    #[tokio::test]
    async fn resolve_version_latest_calls_client() {
        let g = CannedGithubClient {
            latest: Ok("1.10.5".into()),
            sha256sums: Ok(None),
        };
        let v = resolve_version(&VersionRequest::Latest, &g).await.unwrap();
        assert_eq!(v, "1.10.5");
    }

    #[tokio::test]
    async fn resolve_version_exact_returns_input() {
        let g = CannedGithubClient {
            latest: Err(HelperError::Ipc {
                message: "should not be called".into(),
            }),
            sha256sums: Ok(None),
        };
        let v = resolve_version(
            &VersionRequest::Exact {
                version: "1.10.0".into(),
            },
            &g,
        )
        .await
        .unwrap();
        assert_eq!(v, "1.10.0");
    }

    #[test]
    fn resolve_arch_exact_x86_64_maps_to_amd64() {
        assert_eq!(
            resolve_arch(&ArchRequest::Exact {
                arch: "x86_64".into()
            })
            .unwrap(),
            "amd64"
        );
    }

    #[test]
    fn resolve_arch_exact_aarch64_maps_to_arm64() {
        assert_eq!(
            resolve_arch(&ArchRequest::Exact {
                arch: "aarch64".into()
            })
            .unwrap(),
            "arm64"
        );
    }

    #[test]
    fn resolve_arch_rejects_unsupported() {
        let r = resolve_arch(&ArchRequest::Exact {
            arch: "armv7".into(),
        });
        assert!(matches!(r, Err(HelperError::Ipc { .. })));
    }

    #[test]
    fn release_url_matches_sagernet_layout() {
        let url = release_url("1.10.0", "amd64");
        assert_eq!(
            url,
            "https://github.com/SagerNet/sing-box/releases/download/v1.10.0/sing-box-1.10.0-linux-amd64.tar.gz"
        );
    }
}
