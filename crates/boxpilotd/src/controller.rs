//! Controller-user model (spec §6.2, §6.6). The first authorized mutating
//! caller becomes the controller; mismatches/orphans are reported through
//! [`ControllerState`].

use boxpilot_ipc::ControllerStatus;

pub use boxpilot_platform::traits::user_lookup::UserLookup;
#[cfg(target_os = "linux")]
pub use boxpilot_platform::linux::user_lookup::PasswdLookup;
#[cfg(target_os = "windows")]
pub use boxpilot_platform::windows::user_lookup::PasswdLookup;

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
pub mod testing {
    pub use boxpilot_platform::fakes::user_lookup::*;
}

#[cfg(test)]
mod tests {
    use super::testing::Fixed;
    use super::*;

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
