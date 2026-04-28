pub mod commands;
pub mod helper_client;

pub fn run() {
    init_tracing();
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::helper_service_status,
            commands::helper_ping,
            commands::helper_core_discover,
            commands::helper_core_install_managed,
            commands::helper_core_upgrade_managed,
            commands::helper_core_rollback_managed,
            commands::helper_core_adopt,
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
