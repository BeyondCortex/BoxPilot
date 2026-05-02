//! Linux `LogReader` impl that shells out to `journalctl`. Verbatim port
//! from `boxpilotd::systemd::JournalctlProcess`.

use crate::traits::logs::LogReader;
use async_trait::async_trait;
use boxpilot_ipc::HelperError;

pub struct JournalctlProcess;

#[async_trait]
impl LogReader for JournalctlProcess {
    async fn tail(&self, unit_name: &str, lines: u32) -> Result<Vec<String>, HelperError> {
        let n_str = lines.to_string();
        let out = tokio::process::Command::new("journalctl")
            .arg("--no-pager")
            .arg("-u")
            .arg(unit_name)
            .arg("-n")
            .arg(&n_str)
            // --output=short keeps the format `Apr 28 12:34:56 host unit[pid]: msg`
            // which is what `journalctl` defaults to anyway, but pinning it makes
            // the format stable across distros that change defaults.
            .arg("--output=short")
            .output()
            .await
            .map_err(|e| HelperError::Ipc {
                message: format!("spawn journalctl: {e}"),
            })?;
        if !out.status.success() {
            return Err(HelperError::Ipc {
                message: format!(
                    "journalctl exit {:?}: {}",
                    out.status.code(),
                    String::from_utf8_lossy(&out.stderr).trim()
                ),
            });
        }
        let text = String::from_utf8_lossy(&out.stdout);
        Ok(text.lines().map(|l| l.to_string()).collect())
    }
}
