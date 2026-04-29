//! Plan #5: activation pipeline. Implements spec §9.2 (bundle transport
//! + safety filters), §10 (atomic rename + rollback), §7.2 (verify
//! window), and §13 startup-side drift hooks.
//!
//! Submodules are added by subsequent tasks of plan #5.

pub mod activate;
pub mod checker;
pub mod gc;
pub mod recovery;
pub mod release;
pub mod unpack;
pub mod verifier;
