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
                let std_file = std::fs::File::from(fd);
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
