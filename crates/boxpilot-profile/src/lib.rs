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

pub mod redact;
pub use redact::{redact_url_for_display, redact_url_strict};

pub mod import;
pub use import::{
    import_local_dir, import_local_file, new_profile_id, sha256_hex, slugify,
    DirImportError, ImportError, SINGLE_JSON_MAX_BYTES,
};

pub mod remote;
pub use remote::{
    import_remote, refresh_remote, FetchError, FetchedRemote, RemoteFetcher, ReqwestFetcher,
};

pub mod editor;
pub use editor::{apply_patch, patch_in_place, save_edits, EditError};

pub mod asset_check;
pub use asset_check::{
    detect_absolute_paths, extract_asset_refs, verify_asset_refs, AssetCheckError,
};

pub mod check;
pub use check::{run_singbox_check, CheckError, CheckOutput, CHECK_TIMEOUT};

pub mod bundle;
pub use bundle::{prepare_bundle, BundleError, PreparedBundle};

#[cfg(test)]
mod sanity {
    #[test]
    fn crate_compiles() {
        assert_eq!(2 + 2, 4);
    }
}
