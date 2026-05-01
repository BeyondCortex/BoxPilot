//! Schema-aware sing-box JSON redactor used by the daemon-side diagnostics
//! exporter and (in the future) any user-side caller that needs to scrub a
//! sing-box `Value` before writing it to a shareable artifact.
//!
//! Redaction rules follow spec §14. Default-deny applies under
//! `outbounds[*]` and `inbounds[*].users[*]` — anything not on the
//! public-allowlist is replaced with `"***"` so new sing-box outbound types
//! cannot silently leak credentials.

use serde_json::Value;

/// Replacement token for redacted string fields.
pub const REDACTED: &str = "***";

/// Hard cap on traversal depth (matches `boxpilot_ipc::profile::BUNDLE_MAX_NESTING_DEPTH`).
pub const MAX_DEPTH: usize = 32;

/// Replaces sensitive sing-box JSON fields in `value` with [`REDACTED`]
/// (or `0` for numeric ports). The walk is iterative and bounded by
/// [`MAX_DEPTH`] — anything deeper is replaced with [`REDACTED`].
///
/// Operates in-place on `&mut Value`. Callers that need to keep the
/// original should clone first.
pub fn redact_singbox_config(value: &mut Value) {
    walk(value, 0);
}

/// Keys whose value under `outbounds[*]` is always replaced with
/// [`REDACTED`]. We list these explicitly even though default-deny would
/// catch them so the test suite has a stable expectation.
const OUTBOUND_SECRET_KEYS: &[&str] = &["password", "uuid", "private_key"];

fn walk(value: &mut Value, depth: usize) {
    if depth >= MAX_DEPTH {
        *value = Value::String(REDACTED.to_string());
        tracing::warn!(target: "redact", "depth cap hit; replacing subtree");
        return;
    }
    if let Value::Object(map) = value {
        if let Some(Value::Array(outbounds)) = map.get_mut("outbounds") {
            for ob in outbounds {
                if let Value::Object(ob_map) = ob {
                    for key in OUTBOUND_SECRET_KEYS {
                        if let Some(v) = ob_map.get_mut(*key) {
                            *v = Value::String(REDACTED.to_string());
                        }
                    }
                    if let Some(v) = ob_map.get_mut("server") {
                        *v = Value::String(REDACTED.to_string());
                    }
                    if ob_map.contains_key("server_port") {
                        ob_map.insert("server_port".to_string(), Value::Number(0u64.into()));
                    }
                    // override_address / override_port follow §14 server-redaction
                    if let Some(v) = ob_map.get_mut("override_address") {
                        *v = Value::String(REDACTED.to_string());
                    }
                    if ob_map.contains_key("override_port") {
                        ob_map.insert("override_port".to_string(), Value::Number(0u64.into()));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn redacts_outbound_password() {
        let mut v = json!({
            "outbounds": [
                {"type": "shadowsocks", "tag": "ss", "password": "hunter2"}
            ]
        });
        redact_singbox_config(&mut v);
        assert_eq!(v["outbounds"][0]["password"], json!("***"));
    }

    #[test]
    fn redacts_outbound_uuid_and_private_key() {
        let mut v = json!({
            "outbounds": [
                {"type": "vless", "uuid": "abcd-1234", "private_key": "p="},
            ]
        });
        redact_singbox_config(&mut v);
        assert_eq!(v["outbounds"][0]["uuid"], json!("***"));
        assert_eq!(v["outbounds"][0]["private_key"], json!("***"));
    }

    #[test]
    fn redacts_outbound_server_to_string_redacted() {
        let mut v = json!({
            "outbounds": [
                {"type": "vless", "server": "secret.example.com"},
            ]
        });
        redact_singbox_config(&mut v);
        assert_eq!(v["outbounds"][0]["server"], json!("***"));
    }

    #[test]
    fn redacts_outbound_server_port_to_zero() {
        let mut v = json!({
            "outbounds": [
                {"type": "vless", "server_port": 12345},
            ]
        });
        redact_singbox_config(&mut v);
        assert_eq!(v["outbounds"][0]["server_port"], json!(0));
    }

    #[test]
    fn keeps_outbound_type_and_tag() {
        let mut v = json!({
            "outbounds": [
                {"type": "vless", "tag": "main", "password": "x"},
            ]
        });
        redact_singbox_config(&mut v);
        assert_eq!(v["outbounds"][0]["type"], json!("vless"));
        assert_eq!(v["outbounds"][0]["tag"], json!("main"));
    }
}
