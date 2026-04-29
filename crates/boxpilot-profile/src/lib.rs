//! User-side profile store, editor, and activation-bundle composer.

pub mod store;
pub use store::{ensure_dir_0700, write_file_0600_atomic, ProfileStorePaths};

pub mod meta;
pub use meta::{read_metadata, write_metadata, ProfileMetadata, METADATA_SCHEMA_VERSION};

pub mod list;
pub use list::{ProfileStore, StoreError};

pub mod remotes;
pub use remotes::{
    read_remotes, remote_id_for_url, write_remotes, RemoteEntry, RemotesFile,
    REMOTES_SCHEMA_VERSION,
};

pub mod ui_state;
pub use ui_state::{read_ui_state, write_ui_state, UiState, UI_STATE_SCHEMA_VERSION};

#[cfg(test)]
mod sanity {
    #[test]
    fn crate_compiles() {
        assert_eq!(2 + 2, 4);
    }
}
