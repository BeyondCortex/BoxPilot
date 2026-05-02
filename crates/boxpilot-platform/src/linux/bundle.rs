//! Linux memfd-backed bundle builder. Tarballs the staging directory
//! into a sealed `memfd` and exposes it as an `AuxStream`. Per COQ8,
//! this is the platform-side companion to the type defined in
//! `crate::traits::bundle_aux`.

use crate::traits::bundle_aux::AuxStream;
use boxpilot_ipc::HelperError;
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::{Path, PathBuf};

/// Build a sealed memfd containing a tar of `staging_dir` and return it
/// as an `AuxStream::LinuxFd` plus the byte size of the tar (used by
/// the IPC layer's `expected_total_bytes` hint).
pub async fn build_sealed_memfd_aux(staging_dir: &Path) -> Result<(AuxStream, u64), HelperError> {
    let staging = staging_dir.to_path_buf();
    let (fd, size) = tokio::task::spawn_blocking(move || create_sealed_bundle_memfd_owned(&staging))
        .await
        .map_err(|e| HelperError::Ipc {
            message: format!("memfd build join: {e}"),
        })?
        .map_err(|e| HelperError::Ipc {
            message: format!("memfd build: {e}"),
        })?;
    Ok((AuxStream::from_owned_fd(fd), size))
}

/// Lower-level helper that returns the `OwnedFd` directly so tests can
/// inspect seal bits before the value is moved into an `AuxStream`.
pub(crate) fn create_sealed_bundle_memfd_owned(
    staging_root: &Path,
) -> std::io::Result<(OwnedFd, u64)> {
    use std::ffi::CString;
    let fd = nix::sys::memfd::memfd_create(
        CString::new("boxpilot-bundle").unwrap().as_c_str(),
        nix::sys::memfd::MemFdCreateFlag::MFD_CLOEXEC
            | nix::sys::memfd::MemFdCreateFlag::MFD_ALLOW_SEALING,
    )
    .map_err(std::io::Error::from)?;

    {
        let mut file = std::fs::File::from(fd.try_clone()?);
        let mut builder = tar::Builder::new(&mut file);
        builder.mode(tar::HeaderMode::Deterministic);
        append_dir_sorted(&mut builder, staging_root, Path::new(""))?;
        builder.finish()?;
    }

    let seals = nix::fcntl::SealFlag::F_SEAL_WRITE
        | nix::fcntl::SealFlag::F_SEAL_GROW
        | nix::fcntl::SealFlag::F_SEAL_SHRINK
        | nix::fcntl::SealFlag::F_SEAL_SEAL;
    nix::fcntl::fcntl(fd.as_raw_fd(), nix::fcntl::FcntlArg::F_ADD_SEALS(seals))
        .map_err(std::io::Error::from)?;

    let size = nix::sys::stat::fstat(fd.as_raw_fd())
        .map_err(std::io::Error::from)?
        .st_size as u64;
    Ok((fd, size))
}

fn append_dir_sorted(
    b: &mut tar::Builder<&mut std::fs::File>,
    abs_root: &Path,
    rel: &Path,
) -> std::io::Result<()> {
    let abs = abs_root.join(rel);
    let mut entries: Vec<_> = std::fs::read_dir(&abs)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|a| a.file_name());
    for e in entries {
        let path = e.path();
        let name = e.file_name();
        let rel_child = if rel.as_os_str().is_empty() {
            PathBuf::from(&name)
        } else {
            rel.join(&name)
        };
        let ft = std::fs::metadata(&path)?.file_type();
        if ft.is_dir() {
            let mut h = tar::Header::new_ustar();
            h.set_size(0);
            h.set_mode(0o700);
            h.set_entry_type(tar::EntryType::Directory);
            h.set_cksum();
            b.append_data(&mut h, &rel_child, std::io::empty())?;
            append_dir_sorted(b, abs_root, &rel_child)?;
        } else if ft.is_file() {
            let bytes = std::fs::read(&path)?;
            let mut h = tar::Header::new_ustar();
            h.set_size(bytes.len() as u64);
            h.set_mode(0o600);
            h.set_entry_type(tar::EntryType::Regular);
            h.set_cksum();
            b.append_data(&mut h, &rel_child, bytes.as_slice())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Seek;

    #[test]
    fn sealed_memfd_has_all_four_seals_and_expected_tar_layout() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("config.json"), br#"{"log":{}}"#).unwrap();
        std::fs::write(tmp.path().join("manifest.json"), br#"{"schema_version":1}"#).unwrap();

        let (fd, size) = create_sealed_bundle_memfd_owned(tmp.path()).expect("build");
        assert!(size > 0);

        let seals = nix::fcntl::fcntl(fd.as_raw_fd(), nix::fcntl::FcntlArg::F_GET_SEALS).unwrap();
        let mask = libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
        assert_eq!(seals & mask, mask, "all four seals must be set");

        let mut file = std::fs::File::from(fd.try_clone().unwrap());
        file.seek(std::io::SeekFrom::Start(0)).unwrap();
        let mut archive = tar::Archive::new(&mut file);
        let names: Vec<String> = archive
            .entries()
            .unwrap()
            .filter_map(|e| {
                e.ok()
                    .and_then(|e| e.path().ok().map(|p| p.to_string_lossy().into_owned()))
            })
            .collect();
        assert!(
            names.iter().any(|n| n == "config.json"),
            "tar must contain config.json; got {names:?}"
        );
        assert!(
            names.iter().any(|n| n == "manifest.json"),
            "tar must contain manifest.json; got {names:?}"
        );
    }

    #[tokio::test]
    async fn build_sealed_memfd_aux_returns_aux_stream_with_size() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("config.json"), br#"{}"#).unwrap();
        std::fs::write(tmp.path().join("manifest.json"), br#"{}"#).unwrap();

        let (aux, size) = build_sealed_memfd_aux(tmp.path()).await.expect("aux");
        assert!(size > 0);
        // Pull bytes through the AsyncRead end and confirm we get the same size.
        let mut reader = aux.into_async_read();
        let mut buf = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut reader, &mut buf)
            .await
            .unwrap();
        assert_eq!(buf.len() as u64, size);
        // Sanity: tar contains the files we wrote.
        let mut archive = tar::Archive::new(std::io::Cursor::new(buf));
        let names: Vec<String> = archive
            .entries()
            .unwrap()
            .filter_map(|e| {
                e.ok()
                    .and_then(|e| e.path().ok().map(|p| p.to_string_lossy().into_owned()))
            })
            .collect();
        assert!(names.iter().any(|n| n == "config.json"));
        assert!(names.iter().any(|n| n == "manifest.json"));
    }
}
