//! Linux `UserLookup` impl backed by `nix::unistd::User::from_uid` (a thin
//! wrapper over `getpwuid`).

use crate::traits::user_lookup::UserLookup;

pub struct PasswdLookup;

impl UserLookup for PasswdLookup {
    fn lookup_username(&self, uid: u32) -> Option<String> {
        nix::unistd::User::from_uid(nix::unistd::Uid::from_raw(uid))
            .ok()
            .flatten()
            .map(|u| u.name)
    }
}
