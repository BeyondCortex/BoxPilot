//! Windows `UserLookup` stub. `controller_uid` is a Linux-only u32 in the
//! current schema; Sub-project #2 introduces a SID-aware lookup alongside the
//! `boxpilot.toml` schema bump.

use crate::traits::user_lookup::UserLookup;

pub struct PasswdLookup;

impl UserLookup for PasswdLookup {
    fn lookup_username(&self, _uid: u32) -> Option<String> {
        None
    }
}
