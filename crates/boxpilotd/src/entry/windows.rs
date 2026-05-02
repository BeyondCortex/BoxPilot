//! Windows `boxpilotd.exe` entry. Registers the binary as a Windows
//! Service via `service_dispatcher::start` and bridges
//! `ServiceControl::Stop` to a `tokio::sync::Notify` that unblocks the
//! named-pipe `IpcServer::run`.
//!
//! ## Logging
//! Windows has no journald; per spec §6 / COQ5 we use `tracing-appender`'s
//! daily-rolling file sink under `%ProgramData%\BoxPilot\logs\`. The file
//! sink is paired with an `EnvFilter` reading `BOXPILOTD_LOG` so admins
//! can crank verbosity without rebuilding.
//!
//! ## Dispatch wiring
//! Unlike the Linux entry (which still threads everything through the
//! legacy zbus `iface::Helper` shell), the Windows IpcServer calls
//! `HelperDispatch::handle` directly with a `CallerPrincipal::WindowsSid`
//! resolved from the named-pipe client token. So the `callers` field on
//! `HelperContext` is unused on Windows — we plug in a no-op resolver that
//! always errors so any accidental Linux-style call site surfaces loudly.
//!
//! ## Sub-project #1 caveats
//! Several `HelperContext` trait fields point at boxpilotd-internal Linux
//! impls (reqwest-backed `ReqwestGithubClient` / `ReqwestDownloader`,
//! `StdFsFragmentReader`, `StdConfigReader`, `JournalctlProcess`,
//! `ProcessChecker`, `DefaultVerifier`). Sub-project #2 will re-home those
//! into `boxpilot-platform` or split them per-target. Until then the
//! Windows boxpilotd build is intentionally not part of CI's
//! `cargo check --workspace` gate (PR 14 only checks `boxpilot-ipc` +
//! `boxpilot-platform`); the file is shaped correctly so the eventual
//! Windows build only needs the missing per-trait impls landed.

#![cfg(target_os = "windows")]

use anyhow::{Context, Result};
use std::ffi::OsString;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info};

use crate::context::HelperContext;
use crate::dispatch_handler::DispatchHandler;

const SERVICE_NAME: &str = "BoxPilotHelper";

/// Windows entry. `main.rs` calls this directly (no tokio runtime needed
/// at this level — the runtime is built inside `service_main` once the
/// SCM has handed us a worker thread).
pub fn run() -> Result<()> {
    init_tracing()?;
    info!(version = env!("CARGO_PKG_VERSION"), "boxpilotd starting (windows)");

    // Register the service entry point with the SCM. This blocks the
    // current thread until `ServiceControl::Stop` arrives. If the binary
    // was launched outside the SCM (e.g. a developer running it in a
    // console), `service_dispatcher::start` returns an error which we
    // surface to the caller — `boxpilotctl run-foreground` (PR 14b) is
    // the supported way to drive the helper without the SCM.
    windows_service::service_dispatcher::start(SERVICE_NAME, ffi_service_main)
        .context("service_dispatcher::start")?;
    Ok(())
}

windows_service::define_windows_service!(ffi_service_main, service_main);

/// Background-thread service entry. Errors here are logged to the
/// tracing-appender file sink and converted to a Stopped status so the
/// SCM does not keep restarting a misconfigured install.
fn service_main(_arguments: Vec<OsString>) {
    if let Err(e) = run_service() {
        error!("service_main: {e:?}");
    }
}

fn run_service() -> Result<()> {
    use windows_service::service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    };
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};

    // Build the tokio runtime in the worker thread so I/O work spawned
    // from the IpcServer (named-pipe `connect()` futures, dispatch
    // handler tasks) lives for the lifetime of the service.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("build tokio runtime")?;

    let stop = Arc::new(tokio::sync::Notify::new());
    let stop_for_handler = stop.clone();

    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            ServiceControl::Stop | ServiceControl::Shutdown => {
                stop_for_handler.notify_waiters();
                ServiceControlHandlerResult::NoError
            }
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    let status_handle = service_control_handler::register(SERVICE_NAME, event_handler)
        .context("service_control_handler::register")?;

    status_handle
        .set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::Running,
            controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        })
        .context("set_service_status(Running)")?;

    // Drive the actual helper I/O on the runtime. On error we still try
    // to report Stopped so the SCM cleans up.
    let result = runtime.block_on(async move { run_helper(stop).await });

    let _ = status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    });

    result
}

async fn run_helper(stop: Arc<tokio::sync::Notify>) -> Result<()> {
    let paths = boxpilot_platform::Paths::system().context("read system paths from env")?;
    let ctx = build_helper_context_windows(paths)?;

    let dispatch: Arc<dyn boxpilot_platform::traits::ipc::HelperDispatch> =
        Arc::new(DispatchHandler::new(ctx));

    let server = boxpilot_platform::windows::ipc::NamedPipeIpcServer { stop };

    info!("boxpilotd ready (named-pipe ipc)");
    use boxpilot_platform::traits::ipc::IpcServer;
    server
        .run(dispatch)
        .await
        .map_err(|e| anyhow::anyhow!("ipc server: {e}"))?;
    info!("shutting down");
    Ok(())
}

/// Wire a `HelperContext` from the boxpilot-platform Windows real impls
/// (PR 12). Several trait slots still resolve to boxpilotd-internal Linux
/// impls — Sub-project #2 re-homes those (see file-level comment).
fn build_helper_context_windows(paths: boxpilot_platform::Paths) -> Result<Arc<HelperContext>> {
    use boxpilot_platform::windows::{
        active::MarkerFileActivePointer, authority::AlwaysAllowAuthority,
        fs_meta::StdFsMetadataProvider, logs::EventLogReader, service::ScmServiceManager,
        user_lookup::PasswdLookup, version::ProcessVersionChecker,
    };

    let authority_subject = Arc::new(crate::authority::ZbusSubject::new());
    let active = Arc::new(MarkerFileActivePointer {
        active: paths.active_symlink(),
        releases_dir: paths.releases_dir(),
    });

    let ctx = HelperContext::new(
        paths,
        // Windows IpcServer resolves the caller SID from the named-pipe
        // handle directly, so the iface-side `CallerResolver` slot is
        // never consulted. Plug a fail-loud no-op so an accidental
        // Linux-style call surfaces.
        Arc::new(NoopCallerResolver),
        Arc::new(AlwaysAllowAuthority::new_with_warn()),
        authority_subject,
        Arc::new(ScmServiceManager),
        Arc::new(EventLogReader),
        Arc::new(PasswdLookup),
        Arc::new(crate::core::github::ReqwestGithubClient::new().map_err(|e| {
            anyhow::anyhow!("github client: {e}")
        })?),
        Arc::new(crate::core::download::ReqwestDownloader::new().map_err(|e| {
            anyhow::anyhow!("downloader: {e}")
        })?),
        Arc::new(StdFsMetadataProvider),
        Arc::new(ProcessVersionChecker),
        Arc::new(crate::profile::checker::ProcessChecker),
        Arc::new(crate::profile::verifier::DefaultVerifier),
        Arc::new(crate::legacy::observe::StdFsFragmentReader),
        Arc::new(crate::legacy::migrate::StdConfigReader),
        active,
        None,
    );
    Ok(Arc::new(ctx))
}

/// Stand-in for `crate::credentials::CallerResolver`. Always errors
/// because the Windows IpcServer never invokes it (resolution happens at
/// the named-pipe layer). If a future call site does invoke it, the
/// error tells you exactly where to look.
struct NoopCallerResolver;

#[async_trait::async_trait]
impl crate::credentials::CallerResolver for NoopCallerResolver {
    async fn resolve(&self, sender: &str) -> Result<u32, boxpilot_ipc::HelperError> {
        Err(boxpilot_ipc::HelperError::Ipc {
            message: format!(
                "windows: CallerResolver invoked unexpectedly (sender={sender}); \
                 SID resolution lives in NamedPipeIpcServer"
            ),
        })
    }
}

fn init_tracing() -> Result<()> {
    use tracing_subscriber::{fmt, EnvFilter};
    let paths = boxpilot_platform::Paths::system().context("read system paths from env")?;
    let log_dir = paths.system_root_join("logs");
    std::fs::create_dir_all(&log_dir).context("create log dir")?;

    // Daily-rolling file sink keyed off the binary name. Use a non-blocking
    // writer so the helper's hot path is never serialized on disk I/O; the
    // returned guard is leaked deliberately (we're inside the service
    // worker thread for the rest of the process lifetime).
    let file_appender = tracing_appender::rolling::daily(&log_dir, "boxpilotd.log");
    let (nb, guard) = tracing_appender::non_blocking(file_appender);
    std::mem::forget(guard);

    let filter = EnvFilter::try_from_env("BOXPILOTD_LOG")
        .unwrap_or_else(|_| EnvFilter::new("boxpilotd=info"));
    fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(nb)
        .with_ansi(false)
        .init();
    Ok(())
}
