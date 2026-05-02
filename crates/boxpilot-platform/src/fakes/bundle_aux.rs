//! Test-only constructors for `AuxStream`. Cross-platform so helper-side
//! unit tests can fabricate a stream without depending on memfd.

use crate::traits::bundle_aux::AuxStream;
use std::io::Cursor;

/// Build an `AuxStream` from raw tar bytes — for tests. `Cursor<Vec<u8>>`
/// satisfies `AsyncRead + Send + Unpin` via tokio's blanket impl on
/// `std::io::Read + Unpin`.
pub fn aux_from_bytes(bytes: Vec<u8>) -> AuxStream {
    AuxStream::from_async_read(Cursor::new(bytes))
}
