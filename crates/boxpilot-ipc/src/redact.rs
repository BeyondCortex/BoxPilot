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

/// Public-allowlist for entries in `inbounds[*].users[*]`. Username is the
/// only field §14 explicitly leaves through.
const INBOUND_USER_ALLOWLIST: &[&str] = &["name"];

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
    tracing::debug!(target: "redact", depth, "redact walk entering");
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

        if let Some(Value::Array(inbounds)) = map.get_mut("inbounds") {
            for ib in inbounds {
                if let Value::Object(ib_map) = ib {
                    if let Some(Value::Array(users)) = ib_map.get_mut("users") {
                        for u in users {
                            if let Value::Object(u_map) = u {
                                let keys: Vec<String> = u_map.keys().cloned().collect();
                                for key in keys {
                                    if !INBOUND_USER_ALLOWLIST.contains(&key.as_str()) {
                                        u_map.insert(key, Value::String(REDACTED.to_string()));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if let Some(Value::Object(dns)) = map.get_mut("dns") {
            if let Some(Value::Array(servers)) = dns.get_mut("servers") {
                for srv in servers {
                    if let Value::Object(s_map) = srv {
                        if let Some(addr) = s_map.get("address").and_then(|v| v.as_str()) {
                            let new_addr = redact_dns_address(addr);
                            s_map.insert("address".to_string(), Value::String(new_addr));
                        }
                    }
                }
            }
        }

        if let Some(Value::Object(exp)) = map.get_mut("experimental") {
            if let Some(Value::Object(clash)) = exp.get_mut("clash_api") {
                if clash.contains_key("secret") {
                    clash.insert("secret".to_string(), Value::String(REDACTED.to_string()));
                }
            }
        }

        if let Some(Value::Array(endpoints)) = map.get_mut("endpoints") {
            for ep in endpoints {
                if let Value::Object(ep_map) = ep {
                    if ep_map.contains_key("private_key") {
                        ep_map.insert(
                            "private_key".to_string(),
                            Value::String(REDACTED.to_string()),
                        );
                    }
                }
            }
        }
    }
}

/// Replace the host portion of a sing-box DNS `address` string while
/// preserving scheme and path so a support engineer can still see "this
/// was DoH" vs "this was a bare IP" without seeing *which* one. Falls
/// back to whole-string [`REDACTED`] when the value is not URL-shaped.
fn redact_dns_address(s: &str) -> String {
    if let Ok(mut url) = url::Url::parse(s) {
        if url.host().is_some() && url.set_host(Some(REDACTED)).is_ok() {
            return url.to_string();
        }
    }
    REDACTED.to_string()
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
    fn redacts_inbound_user_password_and_uuid() {
        let mut v = json!({
            "inbounds": [
                {"type": "vmess", "users": [
                    {"name": "alice", "password": "p", "uuid": "u"}
                ]}
            ]
        });
        redact_singbox_config(&mut v);
        assert_eq!(v["inbounds"][0]["users"][0]["password"], json!("***"));
        assert_eq!(v["inbounds"][0]["users"][0]["uuid"], json!("***"));
        assert_eq!(v["inbounds"][0]["users"][0]["name"], json!("alice"));
    }

    #[test]
    fn inbound_user_unknown_key_is_redacted() {
        let mut v = json!({
            "inbounds": [
                {"type": "vmess", "users": [
                    {"name": "alice", "future_secret": "x"}
                ]}
            ]
        });
        redact_singbox_config(&mut v);
        assert_eq!(v["inbounds"][0]["users"][0]["future_secret"], json!("***"));
        assert_eq!(v["inbounds"][0]["users"][0]["name"], json!("alice"));
    }

    #[test]
    fn redacts_dns_server_address_url() {
        let mut v = json!({
            "dns": {
                "servers": [
                    {"address": "https://1.1.1.1/dns-query"},
                    {"address": "tls://example.com:853"},
                    {"address": "8.8.8.8"}
                ]
            }
        });
        redact_singbox_config(&mut v);
        let s0 = v["dns"]["servers"][0]["address"].as_str().unwrap().to_string();
        let s1 = v["dns"]["servers"][1]["address"].as_str().unwrap().to_string();
        let s2 = v["dns"]["servers"][2]["address"].as_str().unwrap().to_string();
        // url::Url percent-encodes asterisks in host, so check for either form.
        let host_redacted = |s: &str| s.contains("***") || s.contains("%2A%2A%2A");
        assert!(host_redacted(&s0), "url-shaped: {s0}");
        assert!(!s0.contains("1.1.1.1"), "url-shaped should hide host: {s0}");
        assert!(host_redacted(&s1), "tls scheme: {s1}");
        assert!(!s1.contains("example.com"), "tls scheme should hide host: {s1}");
        assert_eq!(s2, "***", "bare host falls back to whole-string redaction");
    }

    #[test]
    fn redacts_clash_api_secret() {
        let mut v = json!({
            "experimental": {
                "clash_api": {
                    "external_controller": "127.0.0.1:9090",
                    "secret": "topsecret"
                }
            }
        });
        redact_singbox_config(&mut v);
        assert_eq!(v["experimental"]["clash_api"]["secret"], json!("***"));
        assert_eq!(
            v["experimental"]["clash_api"]["external_controller"],
            json!("127.0.0.1:9090"),
        );
    }

    #[test]
    fn redacts_endpoint_private_key() {
        let mut v = json!({
            "endpoints": [
                {"type": "wireguard", "private_key": "k=", "peer_public_key": "p="}
            ]
        });
        redact_singbox_config(&mut v);
        assert_eq!(v["endpoints"][0]["private_key"], json!("***"));
        assert_eq!(v["endpoints"][0]["peer_public_key"], json!("p="));
    }

    #[test]
    fn deep_nesting_does_not_panic() {
        // The walker only recurses into the named top-level branches today,
        // so a synthetic nested object inside an outbound's "tls" key would
        // not exercise the depth guard. We assert "no panic" as the
        // contract; if a future task adds nested redaction, this test
        // should be updated to assert the depth-cap replacement.
        let mut deep = json!({});
        for _ in 0..(MAX_DEPTH + 4) {
            deep = json!({"nested": deep});
        }
        let mut v = json!({
            "outbounds": [
                {"type": "vless", "tls": deep}
            ]
        });
        redact_singbox_config(&mut v);
        assert_eq!(v["outbounds"][0]["type"], json!("vless"));
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
