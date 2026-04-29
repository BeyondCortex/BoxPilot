//! User-side profile store, editor, and activation-bundle composer.

pub mod store;
pub use store::{ensure_dir_0700, write_file_0600_atomic, ProfileStorePaths};

#[cfg(test)]
mod sanity {
    #[test]
    fn crate_compiles() {
        assert_eq!(2 + 2, 4);
    }
}
