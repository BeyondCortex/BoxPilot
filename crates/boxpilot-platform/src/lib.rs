//! BoxPilot platform-abstraction crate.
//!
//! Houses platform-neutral traits, Linux + Windows implementations gated by
//! `cfg(target_os = "...")`, and cross-platform fakes for tests. See spec
//! `docs/superpowers/specs/2026-05-01-boxpilot-platform-abstraction-design.md`.

pub mod traits;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "windows")]
pub mod windows;

pub mod fakes;

pub mod paths;
pub use paths::Paths;

#[cfg(test)]
mod tests {
    #[test]
    fn crate_compiles() {
        assert_eq!(2 + 2, 4);
    }
}
