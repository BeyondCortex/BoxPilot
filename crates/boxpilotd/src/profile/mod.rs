//! Plan #5: activation pipeline. Implements:
//!
//! - spec §9.2 (bundle transport + safety filters);
//! - spec §10 (atomic rename + rollback);
//! - spec §7.2 (verify window);
//! - spec §13 startup-side drift hooks.

pub mod activate;
pub mod checker;
pub mod gc;
pub mod recovery;
pub mod release;
pub mod rollback;
pub mod unpack;
pub mod verifier;
