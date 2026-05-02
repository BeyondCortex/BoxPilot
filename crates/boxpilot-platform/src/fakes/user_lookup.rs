//! In-memory `UserLookup` test double. Mirrors the existing
//! `boxpilotd::controller::testing::Fixed` shape so call sites move with a
//! single import-path change.

use crate::traits::user_lookup::UserLookup;
use std::collections::HashMap;
use std::sync::Mutex;

pub struct Fixed(Mutex<HashMap<u32, String>>);

impl Fixed {
    pub fn new(rows: &[(u32, &str)]) -> Self {
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
