//! Windows bundle builder. Future PRs will spool the staging tar to a
//! temp file and ship its `AsyncRead` handle. Stubbed for now per the
//! Sub-project #1 plan.

use crate::traits::bundle_aux::AuxStream;
use boxpilot_ipc::HelperError;
use std::path::Path;

pub async fn build_tempfile_aux(_staging_dir: &Path) -> Result<(AuxStream, u64), HelperError> {
    Err(HelperError::NotImplemented)
}
