//! Install / upgrade pipeline. Single function, branches on whether
//! `current` exists. The caller (iface.rs) already holds the global lock
//! via `dispatch::authorize`'s AuthorizedCall.

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
    use boxpilot_platform::linux::core_assets::LinuxCoreAssetNaming;
    use boxpilot_platform::traits::core_assets::CoreAssetNaming;
    LinuxCoreAssetNaming.asset_name(version, arch)
}

pub fn release_url(version: &str, arch: &str) -> String {
    format!(
        "https://github.com/SagerNet/sing-box/releases/download/v{version}/{}",
        tarball_filename(version, arch)
    )
}

use crate::core::commit::{StateCommit, TomlUpdates};
use crate::core::download::Downloader;
use crate::core::github::{parse_sha256sums, GithubClient};
use crate::core::state::read_state;
use crate::core::trust::{
    default_allowed_prefixes, verify_executable_path, FsMetadataProvider, TrustError,
    VersionChecker,
};
use crate::dispatch::ControllerWrites;
use boxpilot_platform::Paths;
use boxpilot_ipc::{
    CoreInstallRequest, CoreInstallResponse, CoreKind, CoreSource, CoreState, DiscoveredCore,
    InstallSourceJson, ManagedCoreEntry,
};
use chrono::Utc;

pub struct InstallDeps<'a> {
    pub paths: Paths,
    pub github: &'a dyn GithubClient,
    pub downloader: &'a dyn Downloader,
    pub fs: &'a dyn FsMetadataProvider,
    pub version_checker: &'a dyn VersionChecker,
}

pub async fn install_or_upgrade(
    req: &CoreInstallRequest,
    deps: &InstallDeps<'_>,
    controller: Option<ControllerWrites>,
) -> HelperResult<CoreInstallResponse> {
    let version = resolve_version(&req.version, deps.github).await?;
    let arch_filename = resolve_arch(&req.architecture)?;
    let url = release_url(&version, arch_filename);
    let filename = tarball_filename(&version, arch_filename);

    // Stage directory
    let staging = deps
        .paths
        .cores_staging_dir()
        .join(format!("{version}-{}", random_suffix()));
    tokio::fs::create_dir_all(&staging)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("mkdir staging: {e}"),
        })?;

    let tarball_path = staging.join(&filename);
    let tarball_sha = deps
        .downloader
        .download_to_file(&url, &tarball_path)
        .await?;

    // Upstream checksum verification
    let upstream_sums = deps.github.fetch_sha256sums(&version).await?;
    let upstream_sha256_match: Option<bool> = match &upstream_sums {
        Some(body) => match parse_sha256sums(body, &filename) {
            Some(expected) => {
                if expected.eq_ignore_ascii_case(&tarball_sha) {
                    Some(true)
                } else {
                    return Err(HelperError::Ipc {
                        message: format!(
                            "upstream sha256 mismatch for {filename}: expected {expected}, got {tarball_sha}"
                        ),
                    });
                }
            }
            None => None,
        },
        None => None,
    };

    // Extract sing-box only
    let bin_path = staging.join("sing-box");
    extract_singbox(&tarball_path, &bin_path).await?;
    let bin_sha = sha256_file_pub(&bin_path).await?;

    // Trust check + version smoke
    // (For an under-construction binary inside .staging-cores, the
    // allowed-prefix list must include the staging directory; we add it
    // just for this one check.)
    let mut prefixes = default_allowed_prefixes();
    prefixes.push(deps.paths.cores_staging_dir());
    verify_executable_path(deps.fs, &bin_path, &prefixes).map_err(map_trust_err)?;
    let stdout = deps
        .version_checker
        .check(&bin_path)
        .map_err(|e| TrustError::VersionCheckFailed(e.to_string()))
        .map_err(map_trust_err)?;
    let reported = parse_singbox_version_pub(&stdout).ok_or_else(|| HelperError::Ipc {
        message: format!("could not parse version from: {stdout}"),
    })?;
    if reported != version {
        return Err(HelperError::Ipc {
            message: format!("version mismatch: requested {version}, binary reports {reported}"),
        });
    }

    // Write per-core sidecar files
    tokio::fs::write(staging.join("sha256"), &bin_sha)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("write sha256: {e}"),
        })?;
    let install_source = InstallSourceJson {
        schema_version: 1,
        kind: CoreKind::ManagedInstalled,
        version: version.clone(),
        architecture: arch_filename.to_string(),
        url: Some(url.clone()),
        source_path: None,
        upstream_sha256_match,
        computed_sha256_tarball: Some(tarball_sha.clone()),
        computed_sha256_binary: bin_sha.clone(),
        installed_at: Utc::now().to_rfc3339(),
        user_agent_used: format!("boxpilot/{}", env!("CARGO_PKG_VERSION")),
    };
    tokio::fs::write(
        staging.join("install-source.json"),
        serde_json::to_string_pretty(&install_source).unwrap(),
    )
    .await
    .map_err(|e| HelperError::Ipc {
        message: format!("write install-source.json: {e}"),
    })?;

    // Drop the tarball before promotion — the per-version dir keeps
    // sing-box, sha256, install-source.json and nothing else.
    tokio::fs::remove_file(&tarball_path)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("rm tarball: {e}"),
        })?;

    // Promote: rename(2) staging dir to cores/<version>/. If the target
    // already exists, treat as idempotent — drop the staged copy and
    // re-use the existing managed core so repeated install/upgrade calls
    // for the same version (e.g. periodic "is `latest` newer?" checks)
    // succeed and still update current/state.
    tokio::fs::create_dir_all(deps.paths.cores_dir())
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("create cores_dir: {e}"),
        })?;
    let target_dir = deps.paths.cores_dir().join(&version);
    if target_dir.is_dir() && target_dir.join("sing-box").exists() {
        let _ = tokio::fs::remove_dir_all(&staging).await;
    } else {
        tokio::fs::rename(&staging, &target_dir)
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("promote {staging:?} -> {target_dir:?}: {e}"),
            })?;
    }

    // Build the new InstallState
    let mut state = read_state(&deps.paths.install_state_json()).await?;
    if !state.managed_cores.iter().any(|m| m.version == version) {
        state.managed_cores.push(ManagedCoreEntry {
            version: version.clone(),
            path: target_dir.join("sing-box").to_string_lossy().to_string(),
            sha256: bin_sha.clone(),
            installed_at: install_source.installed_at.clone(),
            source: "github-sagernet".into(),
        });
    }
    state.current_managed_core = Some(version.clone());

    // StateCommit
    let claimed_controller = controller.is_some();
    let commit = StateCommit {
        paths: deps.paths.clone(),
        toml_updates: TomlUpdates {
            core_path: Some(
                deps.paths
                    .cores_current_symlink()
                    .join("sing-box")
                    .to_string_lossy()
                    .to_string(),
            ),
            core_state: Some(CoreState::ManagedInstalled),
            ..TomlUpdates::default()
        },
        controller,
        install_state: state.clone(),
        current_symlink_target: Some(target_dir.clone()),
    };
    commit.apply().await?;

    Ok(CoreInstallResponse {
        installed: DiscoveredCore {
            kind: CoreKind::ManagedInstalled,
            path: target_dir.join("sing-box").to_string_lossy().to_string(),
            version: version.clone(),
            sha256: bin_sha.clone(),
            installed_at: Some(install_source.installed_at.clone()),
            source: Some(CoreSource {
                url: Some(url),
                source_path: None,
                upstream_sha256_match,
                computed_sha256: bin_sha,
            }),
            label: version,
        },
        became_current: true,
        upstream_sha256_match,
        claimed_controller,
    })
}

fn map_trust_err(e: TrustError) -> HelperError {
    HelperError::Ipc {
        message: format!("trust check failed: {e}"),
    }
}

fn random_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    format!("{nanos:08x}")
}

async fn extract_singbox(tarball: &std::path::Path, dest: &std::path::Path) -> HelperResult<()> {
    use boxpilot_platform::linux::core_assets::TarGzExtractor;
    use boxpilot_platform::traits::core_assets::CoreArchive;
    TarGzExtractor.extract(tarball, dest).await
}

pub(crate) async fn sha256_file_pub(path: &std::path::Path) -> HelperResult<String> {
    use sha2::{Digest, Sha256};
    use tokio::io::AsyncReadExt;
    let mut f = tokio::fs::File::open(path)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("open for sha: {e}"),
        })?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).await.map_err(|e| HelperError::Ipc {
            message: format!("read for sha: {e}"),
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub(crate) fn parse_singbox_version_pub(stdout: &str) -> Option<String> {
    // Expected: "sing-box version 1.10.0\n..."
    let line = stdout.lines().next()?;
    let mut parts = line.split_whitespace();
    let _sing = parts.next()?; // sing-box
    let _kw = parts.next()?; // version
    Some(parts.next()?.trim().to_string())
}

#[cfg(test)]
mod pipeline_tests {
    use super::*;
    use crate::core::download::testing::FixedDownloader;
    use crate::core::github::testing::CannedGithubClient;
    use crate::core::trust::version_testing::FixedVersionChecker;

    fn mk_paths(tmp: &tempfile::TempDir) -> Paths {
        std::fs::create_dir_all(tmp.path().join("etc/boxpilot")).unwrap();
        std::fs::create_dir_all(tmp.path().join("var/lib/boxpilot/cores")).unwrap();
        Paths::with_root(tmp.path())
    }

    /// Like `mk_paths` but does NOT pre-create `var/lib/boxpilot/cores/`.
    /// Used by the issue #6 regression test, which exercises the fresh-install
    /// case where the parent of the rename target does not exist yet.
    fn mk_paths_no_cores(tmp: &tempfile::TempDir) -> Paths {
        std::fs::create_dir_all(tmp.path().join("etc/boxpilot")).unwrap();
        Paths::with_root(tmp.path())
    }

    fn fake_singbox_tarball() -> Vec<u8> {
        // Build a tar.gz containing only `sing-box` with the bytes "stub\n".
        let mut tar_bytes = Vec::new();
        {
            let enc = flate2::write::GzEncoder::new(&mut tar_bytes, flate2::Compression::default());
            let mut builder = tar::Builder::new(enc);
            let mut header = tar::Header::new_gnu();
            let payload = b"stub\n";
            header.set_size(payload.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder
                .append_data(&mut header, "sing-box", &payload[..])
                .unwrap();
            builder.finish().unwrap();
            let inner = builder.into_inner().unwrap();
            inner.finish().unwrap();
        }
        tar_bytes
    }

    #[tokio::test]
    async fn happy_install_creates_target_dir_and_state() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = mk_paths(&tmp);
        let tarball = fake_singbox_tarball();
        let downloader = FixedDownloader::new(tarball.clone());
        let body = format!(
            "{} sing-box-1.10.0-linux-amd64.tar.gz\n",
            downloader.returned_sha
        );
        let github = CannedGithubClient {
            latest: Ok("1.10.0".into()),
            sha256sums: Ok(Some(body)),
        };

        struct PermissiveFs;
        impl FsMetadataProvider for PermissiveFs {
            fn stat(&self, p: &std::path::Path) -> std::io::Result<crate::core::trust::FileStat> {
                use crate::core::trust::{FileKind, FileStat};
                let kind = if p.extension().is_none()
                    && p.file_name().map(|n| n == "sing-box").unwrap_or(false)
                {
                    FileKind::Regular
                } else {
                    FileKind::Directory
                };
                Ok(FileStat {
                    uid: 0,
                    gid: 0,
                    mode: 0o755,
                    kind,
                })
            }
            fn read_link(&self, _: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
                Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "no symlinks",
                ))
            }
        }
        let fs = PermissiveFs;
        let vc = FixedVersionChecker::ok("sing-box version 1.10.0");

        let deps = InstallDeps {
            paths: paths.clone(),
            github: &github,
            downloader: &downloader,
            fs: &fs,
            version_checker: &vc,
        };
        let req = CoreInstallRequest {
            version: VersionRequest::Latest,
            architecture: ArchRequest::Exact {
                arch: "x86_64".into(),
            },
        };
        let resp = install_or_upgrade(&req, &deps, None).await.unwrap();
        assert_eq!(resp.installed.version, "1.10.0");
        assert_eq!(resp.upstream_sha256_match, Some(true));
        let state = read_state(&paths.install_state_json()).await.unwrap();
        assert_eq!(state.current_managed_core.as_deref(), Some("1.10.0"));
        assert_eq!(state.managed_cores.len(), 1);
    }

    #[tokio::test]
    async fn version_mismatch_aborts() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = mk_paths(&tmp);
        let downloader = FixedDownloader::new(fake_singbox_tarball());
        let github = CannedGithubClient {
            latest: Ok("1.10.0".into()),
            sha256sums: Ok(None),
        };
        struct PermissiveFs;
        impl FsMetadataProvider for PermissiveFs {
            fn stat(&self, p: &std::path::Path) -> std::io::Result<crate::core::trust::FileStat> {
                use crate::core::trust::{FileKind, FileStat};
                let kind = if p.file_name().map(|n| n == "sing-box").unwrap_or(false) {
                    FileKind::Regular
                } else {
                    FileKind::Directory
                };
                Ok(FileStat {
                    uid: 0,
                    gid: 0,
                    mode: 0o755,
                    kind,
                })
            }
            fn read_link(&self, _: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
                Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "no symlinks",
                ))
            }
        }
        let fs = PermissiveFs;
        let vc = FixedVersionChecker::ok("sing-box version 9.9.9"); // wrong!

        let deps = InstallDeps {
            paths: paths.clone(),
            github: &github,
            downloader: &downloader,
            fs: &fs,
            version_checker: &vc,
        };
        let req = CoreInstallRequest {
            version: VersionRequest::Exact {
                version: "1.10.0".into(),
            },
            architecture: ArchRequest::Exact {
                arch: "x86_64".into(),
            },
        };
        let r = install_or_upgrade(&req, &deps, None).await;
        assert!(matches!(r, Err(HelperError::Ipc { .. })));
        // Promotion did NOT happen.
        assert!(!paths.cores_dir().join("1.10.0").exists());
    }

    #[tokio::test]
    async fn reinstall_same_version_is_idempotent() {
        // First install creates cores/1.10.0/. Re-running the same request
        // (e.g. periodic "is latest newer?" check) must NOT fail just
        // because the directory already exists; it should reuse and still
        // refresh state/current.
        let tmp = tempfile::tempdir().unwrap();
        let paths = mk_paths(&tmp);
        let tarball = fake_singbox_tarball();

        struct PermissiveFs;
        impl FsMetadataProvider for PermissiveFs {
            fn stat(&self, p: &std::path::Path) -> std::io::Result<crate::core::trust::FileStat> {
                use crate::core::trust::{FileKind, FileStat};
                let kind = if p.file_name().map(|n| n == "sing-box").unwrap_or(false) {
                    FileKind::Regular
                } else {
                    FileKind::Directory
                };
                Ok(FileStat {
                    uid: 0,
                    gid: 0,
                    mode: 0o755,
                    kind,
                })
            }
            fn read_link(&self, _: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
                Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "no symlinks",
                ))
            }
        }

        let req = CoreInstallRequest {
            version: VersionRequest::Exact {
                version: "1.10.0".into(),
            },
            architecture: ArchRequest::Exact {
                arch: "x86_64".into(),
            },
        };

        // First install
        {
            let downloader = FixedDownloader::new(tarball.clone());
            let github = CannedGithubClient {
                latest: Ok("1.10.0".into()),
                sha256sums: Ok(None),
            };
            let fs = PermissiveFs;
            let vc = FixedVersionChecker::ok("sing-box version 1.10.0");
            let deps = InstallDeps {
                paths: paths.clone(),
                github: &github,
                downloader: &downloader,
                fs: &fs,
                version_checker: &vc,
            };
            install_or_upgrade(&req, &deps, None).await.unwrap();
        }
        assert!(paths.cores_dir().join("1.10.0").exists());

        // Second install of same version — must succeed without EEXIST.
        {
            let downloader = FixedDownloader::new(tarball.clone());
            let github = CannedGithubClient {
                latest: Ok("1.10.0".into()),
                sha256sums: Ok(None),
            };
            let fs = PermissiveFs;
            let vc = FixedVersionChecker::ok("sing-box version 1.10.0");
            let deps = InstallDeps {
                paths: paths.clone(),
                github: &github,
                downloader: &downloader,
                fs: &fs,
                version_checker: &vc,
            };
            let resp = install_or_upgrade(&req, &deps, None).await.unwrap();
            assert_eq!(resp.installed.version, "1.10.0");
            assert!(resp.became_current);
        }

        // Ledger should still have exactly one entry for 1.10.0.
        let state = read_state(&paths.install_state_json()).await.unwrap();
        assert_eq!(state.managed_cores.len(), 1);
        assert_eq!(state.current_managed_core.as_deref(), Some("1.10.0"));
    }

    /// Issue #6 regression: install must succeed on a fresh machine where
    /// `/var/lib/boxpilot/cores/` does not yet exist. Pre-fix, the bare
    /// `rename(staging, cores/<version>)` raised ENOENT because rename(2)
    /// will not auto-create the destination's parent.
    #[tokio::test]
    async fn install_creates_cores_dir_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = mk_paths_no_cores(&tmp);
        assert!(!paths.cores_dir().exists(), "precondition: cores_dir absent");

        let tarball = fake_singbox_tarball();
        let downloader = FixedDownloader::new(tarball);
        let github = CannedGithubClient {
            latest: Ok("1.10.0".into()),
            sha256sums: Ok(None),
        };

        struct PermissiveFs;
        impl FsMetadataProvider for PermissiveFs {
            fn stat(&self, p: &std::path::Path) -> std::io::Result<crate::core::trust::FileStat> {
                use crate::core::trust::{FileKind, FileStat};
                let kind = if p.file_name().map(|n| n == "sing-box").unwrap_or(false) {
                    FileKind::Regular
                } else {
                    FileKind::Directory
                };
                Ok(FileStat {
                    uid: 0,
                    gid: 0,
                    mode: 0o755,
                    kind,
                })
            }
            fn read_link(&self, _: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
                Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "no symlinks",
                ))
            }
        }
        let fs = PermissiveFs;
        let vc = FixedVersionChecker::ok("sing-box version 1.10.0");
        let deps = InstallDeps {
            paths: paths.clone(),
            github: &github,
            downloader: &downloader,
            fs: &fs,
            version_checker: &vc,
        };
        let req = CoreInstallRequest {
            version: VersionRequest::Exact {
                version: "1.10.0".into(),
            },
            architecture: ArchRequest::Exact {
                arch: "x86_64".into(),
            },
        };
        install_or_upgrade(&req, &deps, None).await.unwrap();
        assert!(paths.cores_dir().join("1.10.0").join("sing-box").exists());
    }
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
