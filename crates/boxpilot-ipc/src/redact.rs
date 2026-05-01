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
/// (or `0` for numeric ports). Two passes run in order:
///
/// 1. **Structural** ([`walk`]): top-level branches handle the §14 fields by
///    JSON path (e.g. `outbounds[*].server_port` → `0`,
///    `dns.servers[*].address` → host redacted), with default-deny under
///    `outbounds[*]` and `inbounds[*].users[*]`.
/// 2. **Recursive name-based scrub** ([`scrub_nested`]): walks the entire
///    tree and redacts any object key whose lowercase name appears in
///    [`NESTED_SENSITIVE_KEYS`] (e.g. `password`, `private_key`, `secret`).
///    This catches credentials nested inside allowlisted parents — e.g.
///    `outbounds[*].tls.reality.private_key` and
///    `outbounds[*].transport.headers.Authorization`.
///
/// Both passes are bounded by [`MAX_DEPTH`]; subtrees deeper than that are
/// collapsed to [`REDACTED`].
///
/// Operates in-place on `&mut Value`. Callers that need to keep the
/// original should clone first.
pub fn redact_singbox_config(value: &mut Value) {
    walk(value, 0);
    scrub_nested(value, 0);
}

/// Public-allowlist for entries in `inbounds[*].users[*]`. Username is the
/// only field §14 explicitly leaves through.
const INBOUND_USER_ALLOWLIST: &[&str] = &["name"];

/// Keys whose presence inside an `outbounds[*]` object is allowed to pass
/// through without modification. Everything else is redacted (default-deny).
/// Allowlisted *containers* (`tls`, `multiplex`, `transport`) still get
/// their nested values scrubbed by [`scrub_nested`].
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

/// Object keys (matched case-insensitively) whose values are redacted
/// anywhere they appear in the tree. This is the safety net that catches
/// credentials nested inside containers we otherwise pass through, e.g.
/// `outbounds[*].tls.reality.private_key` (reality keys), or
/// `transport.headers.Authorization` (HTTP Authorization headers).
///
/// Keep this list narrow on purpose: a too-broad list would also redact
/// structural fields with similar-looking names. Adding a key here is
/// always a focused decision tied to a specific sing-box field.
const NESTED_SENSITIVE_KEYS: &[&str] = &[
    "password",
    "uuid",
    "private_key",
    "secret",
    "token",
    "passphrase",
    "pre_shared_key",
    "authorization",
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

/// Recursively walk the tree and redact any object key whose lowercase
/// name is in [`NESTED_SENSITIVE_KEYS`]. This is the second pass after
/// [`walk`]; it catches credentials nested under allowlisted parents that
/// the structural pass intentionally lets through.
fn scrub_nested(value: &mut Value, depth: usize) {
    if depth >= MAX_DEPTH {
        *value = Value::String(REDACTED.to_string());
        tracing::warn!(target: "redact", "scrub_nested depth cap hit");
        return;
    }
    match value {
        Value::Object(map) => {
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                let lower = key.to_ascii_lowercase();
                if NESTED_SENSITIVE_KEYS.iter().any(|n| *n == lower) {
                    map.insert(key, Value::String(REDACTED.to_string()));
                } else if let Some(v) = map.get_mut(&key) {
                    scrub_nested(v, depth + 1);
                }
            }
        }
        Value::Array(arr) => {
            for v in arr {
                scrub_nested(v, depth + 1);
            }
        }
        _ => {}
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
        let s0 = v["dns"]["servers"][0]["address"]
            .as_str()
            .unwrap()
            .to_string();
        let s1 = v["dns"]["servers"][1]["address"]
            .as_str()
            .unwrap()
            .to_string();
        let s2 = v["dns"]["servers"][2]["address"]
            .as_str()
            .unwrap()
            .to_string();
        // url::Url percent-encodes asterisks in host, so check for either form.
        let host_redacted = |s: &str| s.contains("***") || s.contains("%2A%2A%2A");
        assert!(host_redacted(&s0), "url-shaped: {s0}");
        assert!(!s0.contains("1.1.1.1"), "url-shaped should hide host: {s0}");
        assert!(host_redacted(&s1), "tls scheme: {s1}");
        assert!(
            !s1.contains("example.com"),
            "tls scheme should hide host: {s1}"
        );
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
    fn deep_nesting_hits_depth_cap_in_scrub_nested() {
        // scrub_nested recurses into allowlisted parents like `tls`, so a
        // synthetic deep object beyond MAX_DEPTH triggers the cap and the
        // subtree is collapsed to "***". The structural pass leaves `tls`
        // alone (it's on the allowlist); the nested-scrub pass walks in
        // and redacts when too deep.
        let mut deep = json!({"leaf": "value"});
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
        // Walk down nested chains until we either hit "***" (collapsed) or
        // run out of `nested` keys. Below the cap we should see the depth
        // guard fire: the leaf gets replaced with the redacted token.
        let mut cursor = &v["outbounds"][0]["tls"];
        let mut steps = 0;
        while let Some(next) = cursor.get("nested") {
            cursor = next;
            steps += 1;
            if steps > MAX_DEPTH + 8 {
                break;
            }
        }
        // At some point cursor should be the string "***" (the collapsed
        // subtree), proving the depth cap fired.
        assert!(
            v.to_string().contains("\"***\""),
            "expected depth cap to fire and produce a *** marker somewhere"
        );
    }

    #[test]
    fn redacts_reality_private_key_nested_under_outbound_tls() {
        // §14: `private_key` is sensitive. sing-box's reality protocol
        // places it at `outbounds[*].tls.reality.private_key`, which is
        // nested *inside* the `tls` allowlist passthrough. The recursive
        // scrub pass must catch it.
        let mut v = json!({
            "outbounds": [
                {
                    "type": "vless",
                    "tls": {
                        "enabled": true,
                        "server_name": "example.com",
                        "reality": {
                            "enabled": true,
                            "public_key": "leakable-but-public",
                            "private_key": "MUST-BE-REDACTED",
                            "short_id": "abc123"
                        }
                    }
                }
            ]
        });
        redact_singbox_config(&mut v);
        assert_eq!(
            v["outbounds"][0]["tls"]["reality"]["private_key"],
            json!("***")
        );
        assert_eq!(
            v["outbounds"][0]["tls"]["reality"]["public_key"],
            json!("leakable-but-public"),
            "public_key is not in NESTED_SENSITIVE_KEYS and should pass"
        );
        assert_eq!(
            v["outbounds"][0]["tls"]["server_name"],
            json!("example.com")
        );
    }

    #[test]
    fn redacts_authorization_header_nested_under_transport() {
        let mut v = json!({
            "outbounds": [
                {
                    "type": "vless",
                    "transport": {
                        "type": "ws",
                        "headers": {
                            "Authorization": "Bearer leak-this-token",
                            "User-Agent": "harmless"
                        }
                    }
                }
            ]
        });
        redact_singbox_config(&mut v);
        assert_eq!(
            v["outbounds"][0]["transport"]["headers"]["Authorization"],
            json!("***")
        );
        assert_eq!(
            v["outbounds"][0]["transport"]["headers"]["User-Agent"],
            json!("harmless")
        );
    }

    #[test]
    fn redacts_nested_password_in_multiplex() {
        let mut v = json!({
            "outbounds": [
                {
                    "type": "vless",
                    "multiplex": {
                        "enabled": true,
                        "password": "shadowtls-password-leak"
                    }
                }
            ]
        });
        redact_singbox_config(&mut v);
        assert_eq!(v["outbounds"][0]["multiplex"]["password"], json!("***"));
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
        assert_eq!(
            v["outbounds"][0]["tls"]["server_name"],
            json!("example.com")
        );
        assert_eq!(v["outbounds"][0]["multiplex"]["enabled"], json!(true));
    }
}
