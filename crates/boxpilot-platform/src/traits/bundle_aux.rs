//! `AuxStream` — bytes-handle plumbing for IPC verbs that ship bundles
//! alongside their typed body. Per spec COQ8: opaque struct with
//! crate-private accessors; Linux preserves zero-copy via the cfg-gated
//! `from_owned_fd` constructor.

use tokio::io::AsyncRead;

pub struct AuxStream {
    repr: AuxStreamRepr,
}

pub(crate) enum AuxStreamRepr {
    None,
    AsyncRead(Box<dyn AsyncRead + Send + Unpin>),
    #[cfg(target_os = "linux")]
    LinuxFd(std::os::fd::OwnedFd),
}

impl AuxStream {
    pub fn none() -> Self {
        Self {
            repr: AuxStreamRepr::None,
        }
    }

    pub fn from_async_read(r: impl AsyncRead + Send + Unpin + 'static) -> Self {
        Self {
            repr: AuxStreamRepr::AsyncRead(Box::new(r)),
        }
    }

    #[cfg(target_os = "linux")]
    pub fn from_owned_fd(fd: std::os::fd::OwnedFd) -> Self {
        Self {
            repr: AuxStreamRepr::LinuxFd(fd),
        }
    }

    pub fn is_none(&self) -> bool {
        matches!(self.repr, AuxStreamRepr::None)
    }

    /// Linux-only: pull the underlying memfd back out for callers that
    /// still need to hand it to a legacy zbus/D-Bus surface that takes a
    /// raw fd. Returns `None` for non-fd-backed streams (e.g. tests).
    /// Slated for removal once PR 11 inverts the IPC layer.
    #[cfg(target_os = "linux")]
    pub fn into_owned_fd(self) -> Option<std::os::fd::OwnedFd> {
        match self.repr {
            AuxStreamRepr::LinuxFd(fd) => Some(fd),
            _ => None,
        }
    }

    #[allow(dead_code)] // surfaced for crate-internal platform impls in later PRs
    pub(crate) fn into_repr(self) -> AuxStreamRepr {
        self.repr
    }

    /// Consume the stream as a uniform `AsyncRead`. On Linux, FD-backed
    /// streams are wrapped in `tokio::fs::File`. The helper-side dispatch
    /// uses this to hash-while-reading without caring how the bytes
    /// arrived.
    pub fn into_async_read(self) -> Box<dyn AsyncRead + Send + Unpin> {
        match self.repr {
            AuxStreamRepr::None => Box::new(tokio::io::empty()),
            AuxStreamRepr::AsyncRead(r) => r,
            #[cfg(target_os = "linux")]
            AuxStreamRepr::LinuxFd(fd) => {
                use std::io::Seek;
                let mut std_file = std::fs::File::from(fd);
                // Memfd-backed FDs are returned with the cursor parked at the
                // end of the tar — rewind so AsyncRead consumers see all bytes.
                let _ = std_file.seek(std::io::SeekFrom::Start(0));
                Box::new(tokio::fs::File::from_std(std_file))
            }
        }
    }
}

impl std::fmt::Debug for AuxStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.repr {
            AuxStreamRepr::None => write!(f, "AuxStream::None"),
            AuxStreamRepr::AsyncRead(_) => write!(f, "AuxStream::AsyncRead(<opaque>)"),
            #[cfg(target_os = "linux")]
            AuxStreamRepr::LinuxFd(fd) => {
                write!(
                    f,
                    "AuxStream::LinuxFd({:?})",
                    std::os::fd::AsRawFd::as_raw_fd(fd)
                )
            }
        }
    }
}
