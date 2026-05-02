//! Debug IPC client for both Linux (D-Bus) and Windows (Named Pipe). Used
//! for AC5 verification per spec COQ6.
//!
//! Usage:
//!   boxpilotctl <method.dotted.name> [<json-body>]
//!
//! Examples:
//!   boxpilotctl service.status
//!   boxpilotctl core.install_managed '{"version":"1.10.3","arch":"x86_64"}'

use boxpilot_ipc::HelperMethod;
use boxpilot_platform::traits::bundle_aux::AuxStream;
use boxpilot_platform::traits::ipc::IpcClient;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: boxpilotctl <method> [<json-body>]");
        eprintln!();
        eprintln!("methods:");
        for m in HelperMethod::ALL {
            eprintln!("  {}", m.as_logical());
        }
        std::process::exit(2);
    }

    let method_str = &args[1];
    let method = HelperMethod::ALL
        .iter()
        .copied()
        .find(|m| m.as_logical() == method_str.as_str())
        .ok_or_else(|| anyhow::anyhow!("unknown method: {}", method_str))?;
    let body = args.get(2).cloned().unwrap_or_default().into_bytes();

    let client: Arc<dyn IpcClient> = connect_platform().await?;

    match client.call(method, body, AuxStream::none()).await {
        Ok(resp) => {
            println!("{}", String::from_utf8_lossy(&resp));
        }
        Err(e) => {
            eprintln!("error: {e:?}");
            std::process::exit(1);
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
async fn connect_platform() -> anyhow::Result<Arc<dyn IpcClient>> {
    let c = boxpilot_platform::linux::ipc::ZbusIpcClient::connect_system()
        .await
        .map_err(|e| anyhow::anyhow!("connect: {e:?}"))?;
    Ok(Arc::new(c))
}

#[cfg(target_os = "windows")]
async fn connect_platform() -> anyhow::Result<Arc<dyn IpcClient>> {
    let c = boxpilot_platform::windows::ipc::NamedPipeIpcClient::connect()
        .map_err(|e| anyhow::anyhow!("connect: {e:?}"))?;
    Ok(Arc::new(c))
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
async fn connect_platform() -> anyhow::Result<Arc<dyn IpcClient>> {
    Err(anyhow::anyhow!("unsupported platform"))
}
