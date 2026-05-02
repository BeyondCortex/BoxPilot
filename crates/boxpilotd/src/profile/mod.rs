//! Plan #5: activation pipeline. Implements:
//!
//! - spec §9.2 (bundle transport + safety filters);
//! - spec §10 (atomic rename + rollback);
//! - spec §7.2 (verify window);
//! - spec §13 startup-side drift hooks.
//!
//! Linux-only submodules (activate, release, rollback, unpack) use
//! `std::os::unix` and `OwnedFd`; they are cfg-gated. Platform-neutral
//! submodules (checker, gc, recovery, verifier) compile on all targets.

#[cfg(target_os = "linux")]
pub mod activate;
pub mod checker;
pub mod gc;
pub mod recovery;
#[cfg(target_os = "linux")]
pub mod release;
#[cfg(target_os = "linux")]
pub mod rollback;
#[cfg(target_os = "linux")]
pub mod unpack;
pub mod verifier;
