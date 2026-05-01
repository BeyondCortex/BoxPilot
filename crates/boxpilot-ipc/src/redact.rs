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

/// Keys whose presence inside an `outbounds[*]` object is allowed to pass
/// through without modification. Everything else is redacted (default-deny).
const OUTBOUND_PUBLIC_ALLOWLIST: &[&str] = &[
    "type",
    "tag",
    "network",
    "transport",
    "domain_strategy",
    "detour",
    "fallback",
    "fallback_delay",
    "udp_over_tcp",
    "packet_encoding",
    "tls",
    "multiplex",
    // Numerics / structural that are explicitly *not* secret in §14.
    "ip_version",
];

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
                    let keys: Vec<String> = ob_map.keys().cloned().collect();
                    for key in keys {
                        match key.as_str() {
                            // Numeric ports redact to 0 (preserves type so consumers can parse).
                            "server_port" | "override_port" => {
                                ob_map.insert(key, Value::Number(0u64.into()));
                            }
                            // Strings always redacted to "***".
                            "server" | "override_address" => {
                                ob_map.insert(key, Value::String(REDACTED.to_string()));
                            }
                            // Default-deny under outbounds[*]: pass-through only the allowlist.
                            k if OUTBOUND_PUBLIC_ALLOWLIST.contains(&k) => {
                                // leave as-is
                            }
                            // Everything else (including known-secret keys like
                            // password / uuid / private_key) is redacted.
                            _ => {
                                ob_map.insert(key, Value::String(REDACTED.to_string()));
                            }
                        }
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

    #[test]
    fn outbound_unknown_key_is_redacted_by_default() {
        let mut v = json!({
            "outbounds": [
                {"type": "future_protocol", "secret_handshake": "leak"},
            ]
        });
        redact_singbox_config(&mut v);
        assert_eq!(v["outbounds"][0]["secret_handshake"], json!("***"));
    }

    #[test]
    fn outbound_known_structural_keys_pass_through() {
        let mut v = json!({
            "outbounds": [
                {
                    "type": "vless",
                    "tag": "main",
                    "network": "tcp",
                    "transport": {"type": "ws"},
                    "tls": {"enabled": true, "server_name": "example.com"},
                    "multiplex": {"enabled": true, "protocol": "smux"},
                    "domain_strategy": "ipv4_only",
                    "detour": "next",
                    "udp_over_tcp": true
                }
            ]
        });
        redact_singbox_config(&mut v);
        assert_eq!(v["outbounds"][0]["type"], json!("vless"));
        assert_eq!(v["outbounds"][0]["tag"], json!("main"));
        assert_eq!(v["outbounds"][0]["network"], json!("tcp"));
        assert_eq!(v["outbounds"][0]["domain_strategy"], json!("ipv4_only"));
        assert_eq!(v["outbounds"][0]["detour"], json!("next"));
        assert_eq!(v["outbounds"][0]["transport"]["type"], json!("ws"));
        assert_eq!(v["outbounds"][0]["tls"]["server_name"], json!("example.com"));
        assert_eq!(v["outbounds"][0]["multiplex"]["enabled"], json!(true));
    }
}
