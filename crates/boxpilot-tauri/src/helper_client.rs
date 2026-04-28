//! Tauri-side D-Bus client. Calls `app.boxpilot.Helper1` as the running GUI
//! user and surfaces the typed JSON response back to Vue.

use boxpilot_ipc::ServiceStatusResponse;
use thiserror::Error;
use zbus::{proxy, Connection};

#[proxy(
    interface = "app.boxpilot.Helper1",
    default_service = "app.boxpilot.Helper",
    default_path = "/app/boxpilot/Helper"
)]
trait Helper {
    fn service_status(&self) -> zbus::Result<String>;
}

#[derive(Debug, Error)]
pub enum ClientError {
    /// Method-level error from the helper. `code` is the suffix of the
    /// helper's error name (e.g. `"not_authorized"`); `message` is the
    /// human-readable detail.
    #[error("{code}: {message}")]
    Method { code: String, message: String },
    #[error("connect to system bus: {0}")]
    Connect(zbus::Error),
    #[error("decode response: {0}")]
    Decode(String),
}

impl From<zbus::Error> for ClientError {
    fn from(e: zbus::Error) -> Self {
        // boxpilotd encodes typed errors as
        //   zbus::fdo::Error::Failed("app.boxpilot.Helper1.<Code>: <message>")
        // which arrives here as Error::MethodError with
        //   name   = "org.freedesktop.DBus.Error.Failed"
        //   detail = "app.boxpilot.Helper1.<Code>: <message>"
        if let zbus::Error::MethodError(_, Some(detail), _) = &e {
            const PREFIX: &str = "app.boxpilot.Helper1.";
            if let Some(rest) = detail.strip_prefix(PREFIX) {
                if let Some((code, message)) = rest.split_once(": ") {
                    return ClientError::Method {
                        code: snake_case(code),
                        message: message.to_string(),
                    };
                }
            }
        }
        ClientError::Connect(e)
    }
}

/// Convert a CamelCase or PascalCase code (e.g. "NotAuthorized") into
/// snake_case ("not_authorized") so the GUI matches the JSON tag from
/// `boxpilot_ipc::HelperError`.
fn snake_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, ch) in s.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

pub struct HelperClient {
    conn: Connection,
}

impl HelperClient {
    pub async fn connect() -> Result<Self, ClientError> {
        Ok(Self {
            conn: Connection::system().await?,
        })
    }

    pub async fn service_status(&self) -> Result<ServiceStatusResponse, ClientError> {
        let proxy = HelperProxy::new(&self.conn).await?;
        let json = proxy.service_status().await?;
        serde_json::from_str(&json).map_err(|e| ClientError::Decode(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_case_basic() {
        assert_eq!(snake_case("NotAuthorized"), "not_authorized");
        assert_eq!(snake_case("ControllerOrphaned"), "controller_orphaned");
        assert_eq!(snake_case("Busy"), "busy");
    }

    #[test]
    fn snake_case_already_snake_idempotent() {
        // We don't expect snake input but be defensive.
        assert_eq!(snake_case("not_found"), "not_found");
    }

    #[test]
    fn snake_case_consecutive_caps_split_per_char() {
        // Consecutive caps split on every uppercase boundary; this is fine
        // because our error codes are unambiguously CamelCase (no acronyms).
        assert_eq!(
            snake_case("UnsupportedSchemaVersion"),
            "unsupported_schema_version"
        );
    }
}
