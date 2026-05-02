//! Spec §9.2: walk a tarball arriving as a passed file descriptor and
//! materialize it into `dest_dir` under strict structural rules. Every
//! rejected case maps to a `HelperError` variant introduced in plan #5
//! task 1. The unpacker NEVER follows symlinks; it refuses both symlink
//! and hardlink entries up-front.

use boxpilot_ipc::{
    ActivationManifest, HelperError, HelperResult, BUNDLE_MAX_FILE_BYTES, BUNDLE_MAX_FILE_COUNT,
    BUNDLE_MAX_NESTING_DEPTH, BUNDLE_MAX_TOTAL_BYTES,
};
use boxpilot_platform::AuxStream;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use tar::{Archive, EntryType};

/// Outcome of a successful unpack. `manifest` is parsed from
/// `manifest.json`; `bytes_written` is the running total enforced
/// against `BUNDLE_MAX_TOTAL_BYTES`. Both fields are consumed by
/// `profile::activate` (plan #5 task 10) — until that task lands, the
/// dead-code warnings here are expected.
#[derive(Debug)]
#[allow(dead_code)] // fields consumed by profile::activate (plan #5 task 10)
pub struct UnpackReport {
    pub manifest: ActivationManifest,
    pub bytes_written: u64,
    pub file_count: u32,
}

/// Read the full bundle out of `aux` and materialize into `dest_dir`,
/// which must NOT pre-exist. The directory is created with mode 0o700.
///
/// Internally this spools the `AuxStream` (which may be a Linux memfd
/// or a generic `AsyncRead`) into a tempfile so that the existing
/// seekable `tar::Archive` iteration logic can keep working without
/// being rewritten as an async-streaming parser.
pub async fn unpack_into(
    aux: AuxStream,
    dest_dir: &Path,
    expected_total_bytes: Option<u64>,
) -> HelperResult<UnpackReport> {
    if dest_dir.exists() {
        return Err(HelperError::Ipc {
            message: format!("staging dest already exists: {}", dest_dir.display()),
        });
    }

    // Spool to a tempfile so the rest of the unpacker can use sync
    // `tar::Archive` against a seekable handle. The tempfile is
    // deleted-on-drop via `tempfile::NamedTempFile`.
    let temp = tempfile::NamedTempFile::new().map_err(|e| HelperError::Ipc {
        message: format!("tempfile: {e}"),
    })?;
    let temp_path = temp.path().to_path_buf();

    let mut reader = aux.into_async_read();
    let mut tempfile_async = tokio::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&temp_path)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("open spool: {e}"),
        })?;
    let copied = tokio::io::copy(&mut reader, &mut tempfile_async)
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("copy aux: {e}"),
        })?;
    drop(tempfile_async);

    let total_size = copied;
    if total_size > BUNDLE_MAX_TOTAL_BYTES {
        return Err(HelperError::BundleTooLarge {
            total: total_size,
            limit: BUNDLE_MAX_TOTAL_BYTES,
        });
    }
    // Treat `expected_total_bytes` as a soft upper bound (per the IPC field
    // doc). Reject when the actual stream is larger than the caller said it
    // would be — that would mean the producer or a man-in-the-middle has
    // grown the bundle behind our back. A caller passing a conservative
    // upper-bound estimate is fine.
    if let Some(hint) = expected_total_bytes {
        if total_size > hint {
            return Err(HelperError::Ipc {
                message: format!(
                    "actual bundle bytes {total_size} exceed expected_total_bytes hint {hint}"
                ),
            });
        }
    }

    let mut file = File::open(&temp_path).map_err(|e| HelperError::Ipc {
        message: format!("reopen spool: {e}"),
    })?;

    create_dir_0700(dest_dir)?;

    let mut archive = Archive::new(&mut file);
    archive.set_preserve_permissions(false);
    archive.set_preserve_mtime(false);

    let mut total_bytes: u64 = 0;
    let mut file_count: u32 = 0;
    let mut manifest_bytes: Option<Vec<u8>> = None;
    let mut on_disk_sha: BTreeMap<String, String> = BTreeMap::new();

    for entry in archive.entries().map_err(io_to_helper)? {
        let mut entry = entry.map_err(io_to_helper)?;
        let header_size = entry.header().size().map_err(io_to_helper)?;

        // Pre-body checks.
        let entry_path = entry.path().map_err(io_to_helper)?.into_owned();
        check_entry_path(&entry_path)?;
        let entry_type = entry.header().entry_type();
        if !is_allowed_entry(entry_type) {
            return Err(HelperError::BundleEntryRejected {
                reason: format!(
                    "unsupported entry type {:?} for {}",
                    entry_type,
                    entry_path.display()
                ),
            });
        }
        if header_size > BUNDLE_MAX_FILE_BYTES {
            return Err(HelperError::BundleEntryRejected {
                reason: format!(
                    "{} exceeds per-file size {} > {}",
                    entry_path.display(),
                    header_size,
                    BUNDLE_MAX_FILE_BYTES
                ),
            });
        }
        let depth = entry_path.iter().count() as u32;
        if depth > BUNDLE_MAX_NESTING_DEPTH {
            return Err(HelperError::BundleEntryRejected {
                reason: format!(
                    "{} nesting depth {} > {}",
                    entry_path.display(),
                    depth,
                    BUNDLE_MAX_NESTING_DEPTH
                ),
            });
        }

        let dst = safe_join(dest_dir, &entry_path)?;

        match entry_type {
            EntryType::Directory => {
                if !dst.exists() {
                    create_dir_0700(&dst)?;
                }
                continue;
            }
            EntryType::Regular => {
                if let Some(parent) = dst.parent() {
                    if parent != dest_dir && !parent.exists() {
                        create_dir_all_0700(parent, dest_dir)?;
                    }
                }
                file_count = file_count.saturating_add(1);
                if file_count > BUNDLE_MAX_FILE_COUNT {
                    return Err(HelperError::BundleEntryRejected {
                        reason: format!("file count {} > {}", file_count, BUNDLE_MAX_FILE_COUNT),
                    });
                }

                let mut buf = Vec::with_capacity(header_size as usize);
                entry.read_to_end(&mut buf).map_err(io_to_helper)?;
                let actual_size = buf.len() as u64;
                if actual_size > BUNDLE_MAX_FILE_BYTES {
                    return Err(HelperError::BundleEntryRejected {
                        reason: format!("{} body exceeds per-file size", entry_path.display()),
                    });
                }
                total_bytes = total_bytes.saturating_add(actual_size);
                if total_bytes > BUNDLE_MAX_TOTAL_BYTES {
                    return Err(HelperError::BundleTooLarge {
                        total: total_bytes,
                        limit: BUNDLE_MAX_TOTAL_BYTES,
                    });
                }

                if entry_path == Path::new("manifest.json") {
                    manifest_bytes = Some(buf.clone());
                }

                let mut h = Sha256::new();
                h.update(&buf);
                let sha = hex::encode(h.finalize());
                let key = relpath_string(&entry_path);
                on_disk_sha.insert(key, sha);

                let mut f = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&dst)
                    .map_err(io_to_helper)?;
                std::io::Write::write_all(&mut f, &buf).map_err(io_to_helper)?;
            }
            _ => unreachable!("filtered above by is_allowed_entry"),
        }
    }

    let manifest_bytes = manifest_bytes.ok_or_else(|| HelperError::BundleEntryRejected {
        reason: "manifest.json missing from bundle".into(),
    })?;
    let manifest: ActivationManifest =
        serde_json::from_slice(&manifest_bytes).map_err(|e| HelperError::BundleEntryRejected {
            reason: format!("manifest.json parse: {e}"),
        })?;
    if manifest.schema_version != boxpilot_ipc::ACTIVATION_MANIFEST_SCHEMA_VERSION {
        return Err(HelperError::UnsupportedSchemaVersion {
            got: manifest.schema_version,
        });
    }

    // §9.2: every asset listed in the manifest must match the on-disk
    // sha after unpacking. This catches a hostile bundle where the
    // manifest claims clean assets but the tar body is poisoned, and
    // an honest bundle where prepare_bundle disagreed with itself.
    for asset in &manifest.assets {
        let key = format!("assets/{}", asset.path.trim_start_matches('/'));
        match on_disk_sha.get(&key) {
            Some(actual) if actual == &asset.sha256 => {}
            _ => {
                return Err(HelperError::BundleAssetMismatch {
                    path: asset.path.clone(),
                })
            }
        }
    }

    Ok(UnpackReport {
        manifest,
        bytes_written: total_bytes,
        file_count,
    })
}

fn is_allowed_entry(t: EntryType) -> bool {
    matches!(t, EntryType::Regular | EntryType::Directory)
}

fn check_entry_path(p: &Path) -> HelperResult<()> {
    if p.is_absolute() {
        return Err(HelperError::BundleEntryRejected {
            reason: format!("absolute path: {}", p.display()),
        });
    }
    let s = p.to_string_lossy();
    for ch in s.chars() {
        if ch == '\0' || ch.is_ascii_control() || ch == '\\' || ch == '\u{2215}' || ch == '\u{FF0F}'
        {
            return Err(HelperError::BundleEntryRejected {
                reason: format!("forbidden character in path: {}", p.display()),
            });
        }
    }
    for comp in p.iter() {
        if comp.to_string_lossy() == ".." {
            return Err(HelperError::BundleEntryRejected {
                reason: format!("path traversal: {}", p.display()),
            });
        }
    }
    Ok(())
}

fn safe_join(root: &Path, rel: &Path) -> HelperResult<PathBuf> {
    let root_canon = root.canonicalize().map_err(|e| HelperError::Ipc {
        message: format!("canonicalize root: {e}"),
    })?;
    let mut out = root_canon.clone();
    for comp in rel.iter() {
        out.push(comp);
    }
    if !out.starts_with(&root_canon) {
        return Err(HelperError::BundleEntryRejected {
            reason: format!("escapes staging root: {}", rel.display()),
        });
    }
    Ok(out)
}

fn relpath_string(p: &Path) -> String {
    p.iter()
        .map(|c| c.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn create_dir_0700(p: &Path) -> HelperResult<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .mode(0o700)
        .recursive(false)
        .create(p)
        .map_err(io_to_helper)
}

fn create_dir_all_0700(p: &Path, root: &Path) -> HelperResult<()> {
    if p == root || p.exists() {
        return Ok(());
    }
    if let Some(parent) = p.parent() {
        if parent != root {
            create_dir_all_0700(parent, root)?;
        }
    }
    create_dir_0700(p)
}

fn io_to_helper(e: impl std::fmt::Display) -> HelperError {
    HelperError::Ipc {
        message: format!("unpack: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use boxpilot_ipc::{
        ActivationManifest, AssetEntry, SourceKind, ACTIVATION_MANIFEST_SCHEMA_VERSION,
    };
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, OwnedFd};
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    fn tar_aux(entries: Vec<(&str, tar::EntryType, Vec<u8>)>) -> AuxStream {
        AuxStream::from_owned_fd(tar_memfd(entries))
    }

    fn make_manifest(profile_id: &str, assets: Vec<AssetEntry>) -> Vec<u8> {
        let m = ActivationManifest {
            schema_version: ACTIVATION_MANIFEST_SCHEMA_VERSION,
            activation_id: "test-id".into(),
            profile_id: profile_id.into(),
            profile_name: None,
            profile_sha256: "deadbeef".into(),
            config_sha256: "cafebabe".into(),
            source_kind: SourceKind::Local,
            source_url_redacted: None,
            core_path_at_activation: "/var/lib/boxpilot/cores/current/sing-box".into(),
            core_version_at_activation: "1.10.0".into(),
            created_at: "2026-04-30T00:00:00-07:00".into(),
            assets,
        };
        serde_json::to_vec_pretty(&m).unwrap()
    }

    fn tar_memfd(entries: Vec<(&str, tar::EntryType, Vec<u8>)>) -> OwnedFd {
        let fd = nix::sys::memfd::memfd_create(
            CString::new("test-bundle").unwrap().as_c_str(),
            nix::sys::memfd::MemFdCreateFlag::MFD_CLOEXEC,
        )
        .expect("memfd_create");

        {
            let mut f = File::from(fd.try_clone().unwrap());
            for (path, ty, body) in entries.iter() {
                write_raw_tar_entry(&mut f, path, *ty, body);
            }
            // Two zero blocks terminate a tar archive.
            std::io::Write::write_all(&mut f, &[0u8; 1024]).unwrap();
        }
        fd
    }

    /// Bypass tar::Builder's input validation so tests can inject the
    /// hostile entries §9.2 requires us to refuse (absolute paths,
    /// `..` traversal, etc.). Constructs a 512-byte ustar header plus
    /// padded body directly.
    fn write_raw_tar_entry(f: &mut File, path: &str, ty: tar::EntryType, body: &[u8]) {
        use std::io::Write;
        let mut header = [0u8; 512];
        let pbytes = path.as_bytes();
        let n = pbytes.len().min(100);
        header[..n].copy_from_slice(&pbytes[..n]);
        write_octal(&mut header[100..108], 0o600, 7);
        write_octal(&mut header[108..116], 0, 7);
        write_octal(&mut header[116..124], 0, 7);
        write_octal(&mut header[124..136], body.len() as u64, 11);
        write_octal(&mut header[136..148], 0, 11);
        // chksum placeholder (spaces) for now
        for b in &mut header[148..156] {
            *b = b' ';
        }
        header[156] = ty.as_byte();
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        let sum: u32 = header.iter().map(|&b| b as u32).sum();
        write_octal(&mut header[148..156], sum as u64, 6);
        header[154] = 0;
        header[155] = b' ';

        f.write_all(&header).unwrap();
        f.write_all(body).unwrap();
        let pad = (512 - (body.len() % 512)) % 512;
        if pad > 0 {
            f.write_all(&vec![0u8; pad]).unwrap();
        }
    }

    fn write_octal(buf: &mut [u8], val: u64, width: usize) {
        let s = format!("{:0width$o}", val, width = width);
        let bytes = s.as_bytes();
        let n = bytes.len().min(buf.len() - 1);
        buf[..n].copy_from_slice(&bytes[bytes.len() - n..]);
    }

    #[tokio::test]
    async fn happy_path_unpacks_config_assets_and_manifest() {
        let manifest = make_manifest(
            "p",
            vec![AssetEntry {
                path: "geosite.db".into(),
                sha256: hex::encode(Sha256::digest(b"GEO")),
                size: 3,
            }],
        );
        let aux = tar_aux(vec![
            (
                "config.json",
                tar::EntryType::Regular,
                br#"{"log":{}}"#.to_vec(),
            ),
            ("assets", tar::EntryType::Directory, Vec::new()),
            (
                "assets/geosite.db",
                tar::EntryType::Regular,
                b"GEO".to_vec(),
            ),
            ("manifest.json", tar::EntryType::Regular, manifest),
        ]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("staging-id");
        let report = unpack_into(aux, &dest, None).await.unwrap();
        assert!(dest.join("config.json").exists());
        assert!(dest.join("assets/geosite.db").exists());
        assert_eq!(report.file_count, 3);
    }

    #[tokio::test]
    async fn refuses_absolute_path_entry() {
        let aux = tar_aux(vec![(
            "/etc/passwd",
            tar::EntryType::Regular,
            b"x".to_vec(),
        )]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        let err = unpack_into(aux, &dest, None).await.unwrap_err();
        assert!(matches!(err, HelperError::BundleEntryRejected { .. }));
    }

    #[tokio::test]
    async fn refuses_dotdot_traversal() {
        let aux = tar_aux(vec![(
            "../escape.txt",
            tar::EntryType::Regular,
            b"x".to_vec(),
        )]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        let err = unpack_into(aux, &dest, None).await.unwrap_err();
        assert!(matches!(err, HelperError::BundleEntryRejected { .. }));
    }

    #[tokio::test]
    async fn refuses_symlink_entry() {
        let aux = tar_aux(vec![("link", tar::EntryType::Symlink, b"".to_vec())]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        let err = unpack_into(aux, &dest, None).await.unwrap_err();
        assert!(matches!(
            err,
            HelperError::BundleEntryRejected { reason } if reason.to_lowercase().contains("symlink")
        ));
    }

    #[tokio::test]
    async fn refuses_hardlink_entry() {
        let aux = tar_aux(vec![("link", tar::EntryType::Link, b"".to_vec())]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        let err = unpack_into(aux, &dest, None).await.unwrap_err();
        assert!(matches!(err, HelperError::BundleEntryRejected { .. }));
    }

    #[tokio::test]
    async fn refuses_fifo_entry() {
        let aux = tar_aux(vec![("p", tar::EntryType::Fifo, b"".to_vec())]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        assert!(matches!(
            unpack_into(aux, &dest, None).await.unwrap_err(),
            HelperError::BundleEntryRejected { .. }
        ));
    }

    #[tokio::test]
    async fn refuses_char_device_entry() {
        let aux = tar_aux(vec![("c", tar::EntryType::Char, b"".to_vec())]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        assert!(matches!(
            unpack_into(aux, &dest, None).await.unwrap_err(),
            HelperError::BundleEntryRejected { .. }
        ));
    }

    #[tokio::test]
    async fn refuses_block_device_entry() {
        let aux = tar_aux(vec![("b", tar::EntryType::Block, b"".to_vec())]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        assert!(matches!(
            unpack_into(aux, &dest, None).await.unwrap_err(),
            HelperError::BundleEntryRejected { .. }
        ));
    }

    #[tokio::test]
    async fn refuses_path_with_backslash() {
        let aux = tar_aux(vec![("a\\b", tar::EntryType::Regular, b"x".to_vec())]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        assert!(matches!(
            unpack_into(aux, &dest, None).await.unwrap_err(),
            HelperError::BundleEntryRejected { .. }
        ));
    }

    #[tokio::test]
    async fn refuses_path_with_division_slash() {
        let aux = tar_aux(vec![("a\u{2215}b", tar::EntryType::Regular, b"x".to_vec())]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        assert!(matches!(
            unpack_into(aux, &dest, None).await.unwrap_err(),
            HelperError::BundleEntryRejected { .. }
        ));
    }

    #[tokio::test]
    async fn refuses_path_with_fullwidth_solidus() {
        let aux = tar_aux(vec![("a\u{FF0F}b", tar::EntryType::Regular, b"x".to_vec())]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        assert!(matches!(
            unpack_into(aux, &dest, None).await.unwrap_err(),
            HelperError::BundleEntryRejected { .. }
        ));
    }

    #[tokio::test]
    async fn rejects_too_many_files() {
        let names: Vec<String> = (0..(BUNDLE_MAX_FILE_COUNT + 1))
            .map(|i| format!("f{i}.txt"))
            .collect();
        let entries: Vec<(&str, tar::EntryType, Vec<u8>)> = names
            .iter()
            .map(|n| (n.as_str(), tar::EntryType::Regular, b"x".to_vec()))
            .collect();
        let aux = tar_aux(entries);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        let err = unpack_into(aux, &dest, None).await.unwrap_err();
        assert!(matches!(err, HelperError::BundleEntryRejected { .. }));
    }

    #[tokio::test]
    async fn rejects_too_deep_nesting() {
        let depth = (BUNDLE_MAX_NESTING_DEPTH + 1) as usize;
        let path: String = std::iter::repeat("d")
            .take(depth)
            .collect::<Vec<_>>()
            .join("/")
            + "/leaf.txt";
        let aux = tar_aux(vec![(
            path.as_str(),
            tar::EntryType::Regular,
            b"x".to_vec(),
        )]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        let err = unpack_into(aux, &dest, None).await.unwrap_err();
        assert!(matches!(err, HelperError::BundleEntryRejected { .. }));
    }

    #[tokio::test]
    async fn rejects_missing_manifest() {
        let aux = tar_aux(vec![(
            "config.json",
            tar::EntryType::Regular,
            b"{}".to_vec(),
        )]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        let err = unpack_into(aux, &dest, None).await.unwrap_err();
        assert!(matches!(
            err,
            HelperError::BundleEntryRejected { reason } if reason.contains("manifest")
        ));
    }

    #[tokio::test]
    async fn rejects_manifest_asset_sha_mismatch() {
        let manifest = make_manifest(
            "p",
            vec![AssetEntry {
                path: "geosite.db".into(),
                sha256: "0000".into(),
                size: 3,
            }],
        );
        let aux = tar_aux(vec![
            ("config.json", tar::EntryType::Regular, b"{}".to_vec()),
            ("assets", tar::EntryType::Directory, Vec::new()),
            (
                "assets/geosite.db",
                tar::EntryType::Regular,
                b"GEO".to_vec(),
            ),
            ("manifest.json", tar::EntryType::Regular, manifest),
        ]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        let err = unpack_into(aux, &dest, None).await.unwrap_err();
        assert!(matches!(err, HelperError::BundleAssetMismatch { .. }));
    }

    #[tokio::test]
    async fn rejects_manifest_unknown_schema_version() {
        let m_bytes = make_manifest("p", vec![]);
        let s = String::from_utf8(m_bytes)
            .unwrap()
            .replace("\"schema_version\": 1", "\"schema_version\": 99");
        let m_bytes = s.into_bytes();
        let aux = tar_aux(vec![
            ("config.json", tar::EntryType::Regular, b"{}".to_vec()),
            ("manifest.json", tar::EntryType::Regular, m_bytes),
        ]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        let err = unpack_into(aux, &dest, None).await.unwrap_err();
        assert!(matches!(
            err,
            HelperError::UnsupportedSchemaVersion { got: 99 }
        ));
    }

    #[tokio::test]
    async fn rejects_when_dest_exists() {
        let aux = tar_aux(vec![]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("already-here");
        std::fs::create_dir(&dest).unwrap();
        let err = unpack_into(aux, &dest, None).await.unwrap_err();
        assert!(matches!(err, HelperError::Ipc { .. }));
    }

    #[tokio::test]
    async fn expected_total_bytes_under_actual_aborts_early() {
        // Hint says "≤ N" but the bundle is actually larger — reject. This is
        // the safety case: a producer or MITM that grew the bundle must not
        // sneak past the daemon's pre-spool size guard.
        let fd = tar_memfd(vec![
            ("config.json", tar::EntryType::Regular, b"{}".to_vec()),
            (
                "manifest.json",
                tar::EntryType::Regular,
                make_manifest("p", vec![]),
            ),
        ]);
        let actual = nix::sys::stat::fstat(fd.as_raw_fd()).unwrap().st_size as u64;
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        let err = unpack_into(
            AuxStream::from_owned_fd(fd),
            &dest,
            Some(actual.saturating_sub(1)),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, HelperError::Ipc { .. }));
    }

    #[tokio::test]
    async fn expected_total_bytes_over_actual_is_accepted_as_upper_bound() {
        // Hint > actual is fine: the field's contract is "soft hint to
        // short-circuit oversized bundles", so a conservative upper bound
        // from the producer must not be rejected.
        let fd = tar_memfd(vec![
            ("config.json", tar::EntryType::Regular, b"{}".to_vec()),
            (
                "manifest.json",
                tar::EntryType::Regular,
                make_manifest("p", vec![]),
            ),
        ]);
        let actual = nix::sys::stat::fstat(fd.as_raw_fd()).unwrap().st_size as u64;
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        unpack_into(AuxStream::from_owned_fd(fd), &dest, Some(actual + 4096))
            .await
            .expect("upper-bound hint must be accepted");
    }

    #[tokio::test]
    async fn happy_path_creates_dest_with_0700() {
        let aux = tar_aux(vec![
            ("config.json", tar::EntryType::Regular, b"{}".to_vec()),
            (
                "manifest.json",
                tar::EntryType::Regular,
                make_manifest("p", vec![]),
            ),
        ]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        unpack_into(aux, &dest, None).await.unwrap();
        let mode = std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[tokio::test]
    async fn unpacked_files_are_0600() {
        let aux = tar_aux(vec![
            ("config.json", tar::EntryType::Regular, b"{}".to_vec()),
            (
                "manifest.json",
                tar::EntryType::Regular,
                make_manifest("p", vec![]),
            ),
        ]);
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        unpack_into(aux, &dest, None).await.unwrap();
        let mode = std::fs::metadata(dest.join("config.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[tokio::test]
    async fn aux_stream_from_async_read_unpacks() {
        // Cross-platform path: AuxStream::from_async_read carrying raw tar
        // bytes is unpacked the same as a Linux memfd-backed stream.
        let manifest = make_manifest("p", vec![]);
        let fd = tar_memfd(vec![
            ("config.json", tar::EntryType::Regular, b"{}".to_vec()),
            ("manifest.json", tar::EntryType::Regular, manifest),
        ]);
        // Read out the bytes via a normal File so we can hand them to
        // `from_async_read` and exercise the non-FD branch end-to-end.
        let mut file = File::from(fd);
        use std::io::Seek;
        file.seek(std::io::SeekFrom::Start(0)).unwrap();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut file, &mut bytes).unwrap();
        let aux = AuxStream::from_async_read(std::io::Cursor::new(bytes));
        let dir = tempdir().unwrap();
        let dest = dir.path().join("s");
        unpack_into(aux, &dest, None).await.unwrap();
        assert!(dest.join("config.json").exists());
    }
}
