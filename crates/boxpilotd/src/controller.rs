//! Controller-user model (spec §6.2, §6.6). The first authorized mutating
//! caller becomes the controller; mismatches/orphans are reported through
//! [`ControllerState`].

use boxpilot_ipc::ControllerStatus;

/// User lookup is split out behind a trait so unit tests don't depend on
/// real `/etc/passwd` state.
pub trait UserLookup: Send + Sync {
    fn lookup_username(&self, uid: u32) -> Option<String>;
}

pub struct PasswdLookup;

impl UserLookup for PasswdLookup {
    fn lookup_username(&self, uid: u32) -> Option<String> {
        nix::unistd::User::from_uid(nix::unistd::Uid::from_raw(uid))
            .ok()
            .flatten()
            .map(|u| u.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControllerState {
    Unset,
    Set { uid: u32, username: String },
    Orphaned { uid: u32 },
}

impl ControllerState {
    pub fn from_uid(uid: Option<u32>, lookup: &dyn UserLookup) -> Self {
        match uid {
            None => ControllerState::Unset,
            Some(uid) => match lookup.lookup_username(uid) {
                Some(username) => ControllerState::Set { uid, username },
                None => ControllerState::Orphaned { uid },
            },
        }
    }

    #[allow(dead_code)] // used in plan #2 (controller ownership checks)
    pub fn is_controller(&self, caller_uid: u32) -> bool {
        matches!(self, ControllerState::Set { uid, .. } if *uid == caller_uid)
    }

    pub fn to_status(&self) -> ControllerStatus {
        match self {
            ControllerState::Unset => ControllerStatus::Unset,
            ControllerState::Set { uid, username } => ControllerStatus::Set {
                uid: *uid,
                username: username.clone(),
            },
            ControllerState::Orphaned { uid } => ControllerStatus::Orphaned { uid: *uid },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct Fixed(Mutex<HashMap<u32, String>>);
    impl Fixed {
        fn new(rows: &[(u32, &str)]) -> Self {
            Self(Mutex::new(
                rows.iter().map(|(u, n)| (*u, n.to_string())).collect(),
            ))
        }
    }
    impl UserLookup for Fixed {
        fn lookup_username(&self, uid: u32) -> Option<String> {
            self.0.lock().unwrap().get(&uid).cloned()
        }
    }

    #[test]
    fn no_uid_is_unset() {
        let lookup = Fixed::new(&[]);
        assert_eq!(
            ControllerState::from_uid(None, &lookup),
            ControllerState::Unset
        );
    }

    #[test]
    fn live_uid_is_set() {
        let lookup = Fixed::new(&[(1000, "alice")]);
        let s = ControllerState::from_uid(Some(1000), &lookup);
        assert_eq!(
            s,
            ControllerState::Set {
                uid: 1000,
                username: "alice".into()
            }
        );
    }

    #[test]
    fn missing_uid_is_orphaned() {
        let lookup = Fixed::new(&[]);
        let s = ControllerState::from_uid(Some(1500), &lookup);
        assert_eq!(s, ControllerState::Orphaned { uid: 1500 });
    }

    #[test]
    fn is_controller_only_when_set_and_matching() {
        let lookup = Fixed::new(&[(1000, "alice")]);
        let s = ControllerState::from_uid(Some(1000), &lookup);
        assert!(s.is_controller(1000));
        assert!(!s.is_controller(1001));

        let unset = ControllerState::from_uid(None, &lookup);
        assert!(!unset.is_controller(1000));

        let orphan = ControllerState::Orphaned { uid: 1000 };
        assert!(!orphan.is_controller(1000));
    }
}
