pub mod commands;
pub mod helper_client;
pub mod profile_cmds;

use std::sync::Arc;

pub fn run() {
    init_tracing();

    let paths = boxpilot_platform::Paths::system().expect("read system paths");
    let store = boxpilot_profile::ProfileStore::new(
        boxpilot_profile::ProfileStorePaths::from_paths(&paths),
    );
    let profile_state = profile_cmds::ProfileState {
        store: Arc::new(store),
        fetcher: Arc::new(boxpilot_profile::ReqwestFetcher::default()),
        last_bundle: tokio::sync::Mutex::new(None),
    };

    tauri::Builder::default()
        .manage(paths)
        .manage(profile_state)
        .invoke_handler(tauri::generate_handler![
            commands::helper_service_status,
            commands::helper_home_status,
            commands::helper_ping,
            commands::helper_core_discover,
            commands::helper_core_install_managed,
            commands::helper_core_upgrade_managed,
            commands::helper_core_rollback_managed,
            commands::helper_core_adopt,
            commands::helper_service_start,
            commands::helper_service_stop,
            commands::helper_service_restart,
            commands::helper_service_enable,
            commands::helper_service_disable,
            commands::helper_service_install_managed,
            commands::helper_service_logs,
            commands::helper_legacy_observe_service,
            commands::helper_legacy_migrate_prepare,
            commands::helper_legacy_migrate_cutover,
            commands::helper_diagnostics_export,
            profile_cmds::profile_list,
            profile_cmds::profile_get_source,
            profile_cmds::profile_import_file,
            profile_cmds::profile_import_dir,
            profile_cmds::profile_import_remote,
            profile_cmds::profile_refresh_remote,
            profile_cmds::profile_save_source,
            profile_cmds::profile_apply_patch_json,
            profile_cmds::profile_revert,
            profile_cmds::profile_prepare_bundle,
            profile_cmds::profile_check,
            profile_cmds::profile_activate,
            profile_cmds::profile_rollback,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter =
        EnvFilter::try_from_env("BOXPILOT_LOG").unwrap_or_else(|_| EnvFilter::new("boxpilot=info"));
    let _ = fmt().with_env_filter(filter).with_target(false).try_init();
}
