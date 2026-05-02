//! UID → username resolution. Linux: `getpwuid` via nix. Windows: SID-based
//! `LookupAccountSid` (Sub-project #2).
//!
//! The trait keeps the existing Linux signature (uid → Option<String>) since
//! `controller_uid` is `u32` in `boxpilot.toml` schema v1; Sub-project #2
//! introduces a SID-aware variant alongside the schema bump.

pub trait UserLookup: Send + Sync {
    fn lookup_username(&self, uid: u32) -> Option<String>;
}
