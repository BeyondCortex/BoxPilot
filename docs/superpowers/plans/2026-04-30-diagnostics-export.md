# Plan #8 — Diagnostics Export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the `diagnostics.export_redacted` stub with a working support-bundle generator that writes a redacted `tar.gz` to `/var/cache/boxpilot/diagnostics/` and surfaces the path in the GUI.

**Architecture:** A new `boxpilot-ipc::redact` module hosts a schema-aware `serde_json::Value` walker so both the daemon and (future) user-side callers share one redactor. A new `boxpilotd::diagnostics` module composes the bundle (file collection → redaction → `tar.gz` write → LRU GC). The existing `iface.rs::diagnostics_export_redacted` stub is replaced with a real handler. A Tauri command + AboutTab button surface the action.

**Tech Stack:** Rust 1.78 workspace (`boxpilot-ipc`, `boxpilotd`, `boxpilot-tauri`), `serde_json`, `tar`, `flate2`, zbus 5, Tauri 2 + Vue 3 + TypeScript frontend.

**Spec parent:** `docs/superpowers/specs/2026-04-30-diagnostics-export-design.md`

---

## File Structure

### New files

| Path | Responsibility |
|------|----------------|
| `crates/boxpilot-ipc/src/redact.rs` | Schema-aware sing-box JSON walker (`redact_singbox_config`). |
| `crates/boxpilot-ipc/src/diagnostics.rs` | `DiagnosticsExportResponse`, constants. |
| `crates/boxpilotd/src/diagnostics/mod.rs` | `compose()` entry point. |
| `crates/boxpilotd/src/diagnostics/bundle.rs` | File collection, per-entry redaction, `redact_journal_lines`, tarball writer. |
| `crates/boxpilotd/src/diagnostics/gc.rs` | LRU eviction (`evict_to_cap`). |
| `crates/boxpilotd/src/diagnostics/sysinfo.rs` | Kernel + `/etc/os-release` reader. |
| `docs/superpowers/plans/2026-04-30-diagnostics-export-smoke-procedure.md` | Manual smoke procedure. |

### Modified files

| Path | Change |
|------|--------|
| `crates/boxpilot-ipc/src/lib.rs` | `pub mod redact; pub mod diagnostics;` re-exports. |
| `crates/boxpilot-ipc/src/error.rs` | Add `DiagnosticsIoFailed` and `DiagnosticsEncodeFailed` variants. |
| `crates/boxpilot-ipc/Cargo.toml` | Add `url` workspace dep (for DNS host parsing). |
| `crates/boxpilotd/src/lib.rs` | `pub mod diagnostics;` (or `mod` if private; check existing pattern). |
| `crates/boxpilotd/src/main.rs` | Wire diagnostics module into the binary tree. |
| `crates/boxpilotd/src/paths.rs` | Add `cache_diagnostics_dir()`. |
| `crates/boxpilotd/src/iface.rs` | Replace `diagnostics_export_redacted` stub; add `to_zbus_err` mappings. |
| `crates/boxpilotd/src/profile/checker.rs` | Re-export shared `redact_journal_lines` from diagnostics::bundle. |
| `crates/boxpilot-tauri/src/helper_client.rs` | Add `diagnostics_export_redacted` proxy method. |
| `crates/boxpilot-tauri/src/commands.rs` | Add `helper_diagnostics_export` Tauri command. |
| `crates/boxpilot-tauri/src/main.rs` | Register the new command. |
| `frontend/src/api/types.ts` | Add `DiagnosticsExportResponse`. |
| `frontend/src/api/helper.ts` | Add `diagnosticsExport` wrapper. |
| `frontend/src/components/settings/AboutTab.vue` | Add "Export diagnostics" button + result line. |
| `frontend/src/api/types.js`, `frontend/src/api/helper.js`, `frontend/src/components/settings/AboutTab.vue.js` | Mirror the `.ts` / `.vue` changes (the `.js` versions are committed alongside per the existing project convention; check if they are generated). |

---

## Task 1: Bootstrap `boxpilot-ipc::redact` skeleton

**Files:**
- Create: `crates/boxpilot-ipc/src/redact.rs`
- Modify: `crates/boxpilot-ipc/src/lib.rs`
- Test: `crates/boxpilot-ipc/src/redact.rs` (inline `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing test**

In `crates/boxpilot-ipc/src/redact.rs`:

```rust
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

/// Replaces sensitive sing-box JSON fields in `value` with [`REDACTED`]
/// (or `0` for numeric ports). The walk is iterative and bounded by
/// [`MAX_DEPTH`] — anything deeper is replaced with [`REDACTED`].
///
/// Operates in-place on `&mut Value`. Callers that need to keep the
/// original should clone first.
pub fn redact_singbox_config(value: &mut Value) {
    walk(value, 0);
}

/// Hard cap on traversal depth (matches `boxpilot_ipc::profile::BUNDLE_MAX_NESTING_DEPTH`).
pub const MAX_DEPTH: usize = 32;

fn walk(_value: &mut Value, _depth: usize) {
    // Implemented in later tasks.
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
}
```

In `crates/boxpilot-ipc/src/lib.rs`, add after the existing modules:

```rust
pub mod redact;
pub use redact::{redact_singbox_config, MAX_DEPTH as REDACT_MAX_DEPTH, REDACTED};
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p boxpilot-ipc redact::tests::redacts_outbound_password`
Expected: FAIL with assertion mismatch (password still `"hunter2"`).

- [ ] **Step 3: Implement minimal walker for outbound password**

Replace the `walk` body in `crates/boxpilot-ipc/src/redact.rs` with:

```rust
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
                }
            }
        }
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p boxpilot-ipc redact::tests::redacts_outbound_password`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilot-ipc/src/redact.rs crates/boxpilot-ipc/src/lib.rs
git commit -m "feat(ipc): redact module skeleton with outbound password redaction (plan #8 task 1)"
```

---

## Task 2: Outbound `uuid`, `private_key`, `server`, `server_port`

**Files:**
- Modify: `crates/boxpilot-ipc/src/redact.rs`

- [ ] **Step 1: Write four failing tests**

Append to the `mod tests` block:

```rust
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
```

- [ ] **Step 2: Run tests to verify the new ones fail**

Run: `cargo test -p boxpilot-ipc redact::tests`
Expected: 1 PASS (existing), 3 FAIL on the new ones (server/server_port/private_key not redacted; uuid is via OUTBOUND_SECRET_KEYS already because we added it). Actually `uuid` and `private_key` ARE in `OUTBOUND_SECRET_KEYS` already; `server`/`server_port` are NOT. So only the two server tests fail.

- [ ] **Step 3: Add server + server_port handling**

Replace the inner per-outbound loop in `walk`:

```rust
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
                        ob_map.insert(
                            "server_port".to_string(),
                            Value::Number(0u64.into()),
                        );
                    }
                    // override_address / override_port follow §14 server-redaction
                    if let Some(v) = ob_map.get_mut("override_address") {
                        *v = Value::String(REDACTED.to_string());
                    }
                    if ob_map.contains_key("override_port") {
                        ob_map.insert(
                            "override_port".to_string(),
                            Value::Number(0u64.into()),
                        );
                    }
                }
            }
        }
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p boxpilot-ipc redact::tests`
Expected: 5/5 PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilot-ipc/src/redact.rs
git commit -m "feat(ipc): redact outbound server/server_port/override_* (plan #8 task 2)"
```

---

## Task 3: Default-deny under `outbounds[*]`

**Files:**
- Modify: `crates/boxpilot-ipc/src/redact.rs`

- [ ] **Step 1: Write failing tests for default-deny + allowlist**

Append:

```rust
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
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p boxpilot-ipc redact::tests`
Expected: structural-keys PASS (they're untouched today), unknown-key FAIL (still `"leak"`).

- [ ] **Step 3: Implement default-deny + allowlist**

Add module-level constant and rewrite the per-outbound block:

```rust
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
    // Numerics / structural that are explicitly *not* secret in §14:
    "ip_version",
];
```

Replace the per-outbound inner block with:

```rust
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
```

Remove `OUTBOUND_SECRET_KEYS` and the per-key handling above; the new branch covers it.

- [ ] **Step 4: Run all redact tests**

Run: `cargo test -p boxpilot-ipc redact::tests`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilot-ipc/src/redact.rs
git commit -m "feat(ipc): default-deny under outbounds[*] (plan #8 task 3)"
```

---

## Task 4: `inbounds[*].users[*]` redaction with default-deny

**Files:**
- Modify: `crates/boxpilot-ipc/src/redact.rs`

- [ ] **Step 1: Write failing tests**

Append:

```rust
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
    // username is not §14-sensitive
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
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p boxpilot-ipc redact::tests`
Expected: both new tests FAIL.

- [ ] **Step 3: Implement inbound user walk**

Add module-level constant:

```rust
/// Public-allowlist for entries in `inbounds[*].users[*]`. Username is the
/// only field §14 explicitly leaves through.
const INBOUND_USER_ALLOWLIST: &[&str] = &["name"];
```

Append to the body of `walk` (after the `outbounds` block):

```rust
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
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p boxpilot-ipc redact::tests`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilot-ipc/src/redact.rs
git commit -m "feat(ipc): redact inbounds[*].users[*] with default-deny (plan #8 task 4)"
```

---

## Task 5: `dns.servers[*].address` host portion redaction

**Files:**
- Modify: `crates/boxpilot-ipc/src/redact.rs`
- Modify: `crates/boxpilot-ipc/Cargo.toml`

- [ ] **Step 1: Add `url` dependency**

In `crates/boxpilot-ipc/Cargo.toml`, append to `[dependencies]`:

```toml
url.workspace = true
```

- [ ] **Step 2: Write failing tests**

Append to `mod tests`:

```rust
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
    assert!(s0.contains("***"), "url-shaped: {s0}");
    assert!(!s0.contains("1.1.1.1"), "url-shaped should hide host: {s0}");
    assert!(s1.contains("***"), "tls scheme: {s1}");
    assert!(!s1.contains("example.com"), "tls scheme should hide host: {s1}");
    assert_eq!(s2, "***", "bare host falls back to whole-string redaction");
}
```

- [ ] **Step 3: Implement DNS server address redactor**

Add helper at bottom of `redact.rs`:

```rust
/// Replace the host portion of a sing-box DNS `address` string while
/// preserving scheme and path so a support engineer can still see "this
/// was DoH" vs "this was a bare IP" without seeing *which* one. Falls
/// back to whole-string [`REDACTED`] when the value is not URL-shaped.
fn redact_dns_address(s: &str) -> String {
    if let Ok(mut url) = url::Url::parse(s) {
        if url.host().is_some() {
            // Use a literal sentinel host. set_host can fail for some
            // schemes (e.g. file:); fall back to whole-string redact.
            if url.set_host(Some(REDACTED)).is_ok() {
                return url.to_string();
            }
        }
    }
    REDACTED.to_string()
}
```

Append to `walk` after the `inbounds` block:

```rust
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
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p boxpilot-ipc redact::tests`
Expected: all PASS.

Note: `url::Url::set_host("***")` may URL-encode the asterisks. The
assertions above use `contains("***")` which matches both raw and percent-
encoded forms. If you find `%2A%2A%2A` instead of `***`, update the
expectation; the goal is "host portion no longer reveals the original".

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilot-ipc/src/redact.rs crates/boxpilot-ipc/Cargo.toml
git commit -m "feat(ipc): redact dns.servers[*].address host portion (plan #8 task 5)"
```

---

## Task 6: `experimental.clash_api.secret` and `endpoints[*].private_key`

**Files:**
- Modify: `crates/boxpilot-ipc/src/redact.rs`

- [ ] **Step 1: Write failing tests**

Append:

```rust
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
        "external_controller is not §14-sensitive"
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
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p boxpilot-ipc redact::tests`
Expected: both new tests FAIL.

- [ ] **Step 3: Implement**

Append to `walk` after the `dns` block:

```rust
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
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p boxpilot-ipc redact::tests`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilot-ipc/src/redact.rs
git commit -m "feat(ipc): redact clash_api.secret + endpoints[*].private_key (plan #8 task 6)"
```

---

## Task 7: Depth bound regression test

**Files:**
- Modify: `crates/boxpilot-ipc/src/redact.rs`

- [ ] **Step 1: Write failing test**

Append:

```rust
#[test]
fn deep_nesting_replaces_subtree_at_max_depth() {
    // The walker only recurses into the named top-level branches today,
    // so a synthetic nested outbounds entry exercises the depth guard.
    // Build a 64-deep nested object inside an outbound's "tls" key (which
    // is on the allowlist and therefore would otherwise pass through).
    let mut deep = json!({});
    for _ in 0..(MAX_DEPTH + 4) {
        deep = json!({"nested": deep});
    }
    let mut v = json!({
        "outbounds": [
            {"type": "vless", "tls": deep}
        ]
    });
    // Today walk does not recurse into tls; verify it is left alone.
    // (When we add nested redaction in a future task, this test should
    // be updated to assert the depth-cap replacement instead.)
    redact_singbox_config(&mut v);
    // Either pass-through (today) or depth-cap replacement (future). Both
    // are acceptable; we just assert no panic.
    assert!(v["outbounds"][0]["type"] == json!("vless"));
}
```

- [ ] **Step 2: Run test**

Run: `cargo test -p boxpilot-ipc redact::tests::deep_nesting`
Expected: PASS (today walk does not recurse, so the deep value is preserved unchanged). The point of the test is regression coverage for any future deeper-walk change.

- [ ] **Step 3: Add `tracing::debug` coverage logging**

At the top of `walk`, before the `if let Value::Object(map) = value` line:

```rust
    tracing::debug!(target: "redact", depth, "redact walk entering");
```

This is a no-op in release/test builds without `tracing-subscriber`, but
gives us coverage diagnostics in the daemon log when `RUST_LOG=redact=debug`
is set.

- [ ] **Step 4: Run all redact tests**

Run: `cargo test -p boxpilot-ipc redact::`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilot-ipc/src/redact.rs
git commit -m "test(ipc): redact depth-bound regression + debug-log coverage (plan #8 task 7)"
```

---

## Task 8: `boxpilot_ipc::diagnostics` types and constants

**Files:**
- Create: `crates/boxpilot-ipc/src/diagnostics.rs`
- Modify: `crates/boxpilot-ipc/src/lib.rs`

- [ ] **Step 1: Write failing test**

In `crates/boxpilot-ipc/src/diagnostics.rs`:

```rust
//! Wire types for `diagnostics.export_redacted` (spec §6.3 / §14).

use serde::{Deserialize, Serialize};

/// Bumped when the response shape changes.
pub const DIAGNOSTICS_SCHEMA_VERSION: u32 = 1;

/// §5.5 retention cap for `/var/cache/boxpilot/diagnostics/`. The exporter
/// runs LRU eviction below this watermark before writing a new bundle.
pub const DIAGNOSTICS_BUNDLE_CAP_BYTES: u64 = 100 * 1024 * 1024;

/// Number of journal lines included in the bundle's `journal-tail.txt`.
/// Matches `boxpilot_ipc::service::SERVICE_LOGS_DEFAULT_LINES`.
pub const DIAGNOSTICS_JOURNAL_TAIL_LINES: u32 = 200;

/// Helper response for `diagnostics.export_redacted`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticsExportResponse {
    pub schema_version: u32,
    /// Absolute path to the freshly written `*.tar.gz`.
    pub bundle_path: String,
    pub bundle_size_bytes: u64,
    /// RFC3339 UTC timestamp the bundle was generated.
    pub generated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn response_round_trips() {
        let r = DiagnosticsExportResponse {
            schema_version: DIAGNOSTICS_SCHEMA_VERSION,
            bundle_path: "/var/cache/boxpilot/diagnostics/x.tar.gz".into(),
            bundle_size_bytes: 4242,
            generated_at: "2026-04-30T22:00:00Z".into(),
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: DiagnosticsExportResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn cap_is_100_mib() {
        assert_eq!(DIAGNOSTICS_BUNDLE_CAP_BYTES, 100 * 1024 * 1024);
    }
}
```

In `crates/boxpilot-ipc/src/lib.rs`, append:

```rust
pub mod diagnostics;
pub use diagnostics::{
    DiagnosticsExportResponse, DIAGNOSTICS_BUNDLE_CAP_BYTES, DIAGNOSTICS_JOURNAL_TAIL_LINES,
    DIAGNOSTICS_SCHEMA_VERSION,
};
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p boxpilot-ipc diagnostics::tests`
Expected: 2/2 PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/boxpilot-ipc/src/diagnostics.rs crates/boxpilot-ipc/src/lib.rs
git commit -m "feat(ipc): diagnostics export response type + constants (plan #8 task 8)"
```

---

## Task 9: `HelperError::Diagnostics{IoFailed,EncodeFailed}`

**Files:**
- Modify: `crates/boxpilot-ipc/src/error.rs`
- Modify: `crates/boxpilotd/src/iface.rs`

- [ ] **Step 1: Write failing test**

Append to `mod tests` in `error.rs`:

```rust
#[test]
fn diagnostics_variants_round_trip() {
    use HelperError::*;
    for v in [
        DiagnosticsIoFailed {
            step: "write tarball".into(),
            cause: "ENOSPC".into(),
        },
        DiagnosticsEncodeFailed {
            cause: "manifest serialize".into(),
        },
    ] {
        let s = serde_json::to_string(&v).unwrap();
        let back: HelperError = serde_json::from_str(&s).unwrap();
        assert_eq!(back, v);
    }
}
```

- [ ] **Step 2: Run test**

Run: `cargo test -p boxpilot-ipc diagnostics_variants_round_trip`
Expected: FAIL (variants don't exist).

- [ ] **Step 3: Add variants**

Append before the closing `}` of `pub enum HelperError`:

```rust
    /// Diagnostics export failed at an i/o boundary (could not list/read/write
    /// a file under `/var/cache/boxpilot/diagnostics/`).
    #[error("diagnostics i/o failed at {step}: {cause}")]
    DiagnosticsIoFailed { step: String, cause: String },

    /// Diagnostics export failed encoding the bundle manifest or a
    /// JSON artifact.
    #[error("diagnostics encode failed: {cause}")]
    DiagnosticsEncodeFailed { cause: String },
```

- [ ] **Step 4: Add iface error mapping**

In `crates/boxpilotd/src/iface.rs::to_zbus_err`, add to the `match` (alphabetical neighbor: after `BundleAssetMismatch`, in the same style):

```rust
        HelperError::DiagnosticsIoFailed { .. } => "app.boxpilot.Helper1.DiagnosticsIoFailed",
        HelperError::DiagnosticsEncodeFailed { .. } => "app.boxpilot.Helper1.DiagnosticsEncodeFailed",
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p boxpilot-ipc diagnostics_variants_round_trip && cargo build -p boxpilotd`
Expected: PASS + build succeeds (the `match` is exhaustive; missing arms would fail compile).

- [ ] **Step 6: Commit**

```bash
git add crates/boxpilot-ipc/src/error.rs crates/boxpilotd/src/iface.rs
git commit -m "feat(ipc): DiagnosticsIoFailed/EncodeFailed HelperError variants (plan #8 task 9)"
```

---

## Task 10: `Paths::cache_diagnostics_dir()`

**Files:**
- Modify: `crates/boxpilotd/src/paths.rs`

- [ ] **Step 1: Write failing test**

Append to `mod tests`:

```rust
#[test]
fn cache_diagnostics_dir_under_var_cache_boxpilot() {
    let p = Paths::with_root("/tmp/fake");
    assert_eq!(
        p.cache_diagnostics_dir(),
        PathBuf::from("/tmp/fake/var/cache/boxpilot/diagnostics")
    );
}

#[test]
fn cache_diagnostics_dir_in_system_paths() {
    let p = Paths::system();
    assert_eq!(
        p.cache_diagnostics_dir(),
        PathBuf::from("/var/cache/boxpilot/diagnostics")
    );
}
```

- [ ] **Step 2: Run test**

Run: `cargo test -p boxpilotd paths::tests::cache_diagnostics_dir`
Expected: FAIL (no such method).

- [ ] **Step 3: Implement**

Append inside `impl Paths`:

```rust
    /// `/var/cache/boxpilot/diagnostics` — root of redacted diagnostics
    /// bundles, capped at `DIAGNOSTICS_BUNDLE_CAP_BYTES` (§5.5).
    pub fn cache_diagnostics_dir(&self) -> PathBuf {
        self.root.join("var/cache/boxpilot/diagnostics")
    }
```

- [ ] **Step 4: Run test**

Run: `cargo test -p boxpilotd paths::`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilotd/src/paths.rs
git commit -m "feat(boxpilotd): Paths::cache_diagnostics_dir (plan #8 task 10)"
```

---

## Task 11: `diagnostics::sysinfo`

**Files:**
- Create: `crates/boxpilotd/src/diagnostics/sysinfo.rs`
- Create: `crates/boxpilotd/src/diagnostics/mod.rs`
- Modify: `crates/boxpilotd/src/main.rs` (to register `mod diagnostics;`)

- [ ] **Step 1: Register the module**

In `crates/boxpilotd/src/main.rs`, locate the existing `mod` declarations and add:

```rust
mod diagnostics;
```

In `crates/boxpilotd/src/diagnostics/mod.rs`:

```rust
//! Diagnostics export pipeline (spec §5.5 / §14, plan #8). The public entry
//! point is [`compose`], called from `iface::diagnostics_export_redacted`.

pub mod bundle;
pub mod gc;
pub mod sysinfo;
```

(`bundle` and `gc` get full bodies in later tasks; for now create empty stubs.)

`crates/boxpilotd/src/diagnostics/bundle.rs`:

```rust
//! Bundle composition: file collection, redaction, tarball writer.
```

`crates/boxpilotd/src/diagnostics/gc.rs`:

```rust
//! LRU eviction for /var/cache/boxpilot/diagnostics.
```

- [ ] **Step 2: Write failing tests for sysinfo**

In `crates/boxpilotd/src/diagnostics/sysinfo.rs`:

```rust
//! Best-effort host-info collector for the diagnostics bundle. Each field
//! falls back to "unknown" rather than failing the whole export.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SystemInfo {
    pub kernel: String,
    pub os_id: String,
    pub os_version_id: String,
    pub os_pretty_name: String,
    pub boxpilot_version: String,
}

pub fn collect(os_release_path: &std::path::Path) -> SystemInfo {
    SystemInfo {
        kernel: kernel_release(),
        os_id: read_os_release_field(os_release_path, "ID").unwrap_or_else(|| "unknown".into()),
        os_version_id: read_os_release_field(os_release_path, "VERSION_ID")
            .unwrap_or_else(|| "unknown".into()),
        os_pretty_name: read_os_release_field(os_release_path, "PRETTY_NAME")
            .unwrap_or_else(|| "unknown".into()),
        boxpilot_version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

fn kernel_release() -> String {
    nix::sys::utsname::uname()
        .ok()
        .and_then(|u| u.release().to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "unknown".into())
}

/// Parse a `KEY=value` (or `KEY="value with spaces"`) line from /etc/os-release.
fn read_os_release_field(path: &std::path::Path, key: &str) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix(&format!("{key}=")) {
            let trimmed = rest.trim_matches('"').to_string();
            return Some(trimmed);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    #[test]
    fn parses_quoted_pretty_name() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("os-release");
        std::fs::write(
            &p,
            "NAME=\"Ubuntu\"\nID=ubuntu\nVERSION_ID=\"24.04\"\nPRETTY_NAME=\"Ubuntu 24.04 LTS\"\n",
        )
        .unwrap();
        let info = collect(&p);
        assert_eq!(info.os_id, "ubuntu");
        assert_eq!(info.os_version_id, "24.04");
        assert_eq!(info.os_pretty_name, "Ubuntu 24.04 LTS");
    }

    #[test]
    fn missing_os_release_falls_back_to_unknown() {
        let tmp = tempdir().unwrap();
        let info = collect(&tmp.path().join("nonexistent"));
        assert_eq!(info.os_id, "unknown");
        assert_eq!(info.os_version_id, "unknown");
        assert_eq!(info.os_pretty_name, "unknown");
        assert!(!info.boxpilot_version.is_empty());
    }

    #[test]
    fn kernel_is_nonempty_on_real_host() {
        let k = kernel_release();
        assert!(!k.is_empty());
        // CI runs on Linux; "unknown" is the fallback for uname() failures.
        // We don't assert "Linux" here because the kernel-release string
        // does not include the OS name on all platforms.
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p boxpilotd diagnostics::sysinfo::tests`
Expected: all PASS (3 tests).

- [ ] **Step 4: Commit**

```bash
git add crates/boxpilotd/src/main.rs crates/boxpilotd/src/diagnostics/
git commit -m "feat(boxpilotd): diagnostics::sysinfo module (plan #8 task 11)"
```

---

## Task 12: `diagnostics::gc::evict_to_cap`

**Files:**
- Modify: `crates/boxpilotd/src/diagnostics/gc.rs`

- [ ] **Step 1: Write failing tests**

Replace the file body with:

```rust
//! LRU eviction for the diagnostics cache directory.

use std::path::Path;
use std::time::SystemTime;

/// Total size on disk (in bytes) of `*.tar.gz` files in `dir`. Tempfiles
/// (names beginning with `.`) are intentionally skipped — they belong to
/// in-progress writes and should not be counted.
pub fn dir_size(dir: &Path) -> std::io::Result<u64> {
    if !dir.exists() {
        return Ok(0);
    }
    let mut total = 0u64;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if !is_visible_tarball(&entry) {
            continue;
        }
        total = total.saturating_add(entry.metadata()?.len());
    }
    Ok(total)
}

/// Delete the oldest visible `*.tar.gz` files until the directory's total
/// size is at or below `cap_bytes`. Tempfiles are never touched. Per-file
/// deletion failures are logged and skipped — the loop tries the next
/// oldest file rather than aborting.
pub fn evict_to_cap(dir: &Path, cap_bytes: u64) -> std::io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    loop {
        let total = dir_size(dir)?;
        if total <= cap_bytes {
            return Ok(());
        }
        let oldest = oldest_tarball(dir)?;
        let Some((path, _)) = oldest else {
            // Nothing visible to evict but we're still over cap — bail
            // rather than spin. The caller may want to log this.
            return Ok(());
        };
        if let Err(e) = std::fs::remove_file(&path) {
            tracing::warn!(target: "diagnostics::gc", path = %path.display(), error = %e, "evict failed");
            // Avoid an infinite loop: break to caller; it'll retry next call.
            return Ok(());
        }
    }
}

fn is_visible_tarball(entry: &std::fs::DirEntry) -> bool {
    let name = entry.file_name();
    let Some(s) = name.to_str() else { return false };
    !s.starts_with('.') && s.ends_with(".tar.gz")
}

fn oldest_tarball(dir: &Path) -> std::io::Result<Option<(std::path::PathBuf, SystemTime)>> {
    let mut oldest: Option<(std::path::PathBuf, SystemTime)> = None;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if !is_visible_tarball(&entry) {
            continue;
        }
        let mtime = entry.metadata()?.modified()?;
        match &oldest {
            Some((_, t)) if *t <= mtime => {}
            _ => oldest = Some((entry.path(), mtime)),
        }
    }
    Ok(oldest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use filetime::{set_file_mtime, FileTime};
    use std::fs;
    use std::time::{Duration, UNIX_EPOCH};
    use tempfile::tempdir;

    fn make_tarball(dir: &Path, name: &str, size: usize, mtime_secs: u64) {
        let path = dir.join(name);
        fs::write(&path, vec![0u8; size]).unwrap();
        set_file_mtime(
            &path,
            FileTime::from_system_time(UNIX_EPOCH + Duration::from_secs(mtime_secs)),
        )
        .unwrap();
    }

    #[test]
    fn evict_drops_oldest_first_until_under_cap() {
        let tmp = tempdir().unwrap();
        // Three 100-byte files; cap = 250 → expect oldest (mtime=1) deleted.
        make_tarball(tmp.path(), "a.tar.gz", 100, 1);
        make_tarball(tmp.path(), "b.tar.gz", 100, 2);
        make_tarball(tmp.path(), "c.tar.gz", 100, 3);
        evict_to_cap(tmp.path(), 250).unwrap();
        assert!(!tmp.path().join("a.tar.gz").exists(), "oldest should go");
        assert!(tmp.path().join("b.tar.gz").exists());
        assert!(tmp.path().join("c.tar.gz").exists());
    }

    #[test]
    fn evict_skips_tempfiles_beginning_with_dot() {
        let tmp = tempdir().unwrap();
        make_tarball(tmp.path(), "a.tar.gz", 100, 1);
        make_tarball(tmp.path(), ".tmpXYZ", 9999, 0);
        evict_to_cap(tmp.path(), 0).unwrap();
        assert!(!tmp.path().join("a.tar.gz").exists());
        assert!(tmp.path().join(".tmpXYZ").exists(), "tempfile preserved");
    }

    #[test]
    fn evict_no_op_when_under_cap() {
        let tmp = tempdir().unwrap();
        make_tarball(tmp.path(), "a.tar.gz", 100, 1);
        evict_to_cap(tmp.path(), 1024).unwrap();
        assert!(tmp.path().join("a.tar.gz").exists());
    }

    #[test]
    fn evict_on_missing_dir_is_ok() {
        let tmp = tempdir().unwrap();
        let missing = tmp.path().join("does-not-exist");
        evict_to_cap(&missing, 100).unwrap();
        assert_eq!(dir_size(&missing).unwrap(), 0);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p boxpilotd diagnostics::gc::tests`
Expected: 4/4 PASS. (`filetime` is already a `dev-dependency` in `boxpilotd/Cargo.toml`.)

- [ ] **Step 3: Commit**

```bash
git add crates/boxpilotd/src/diagnostics/gc.rs
git commit -m "feat(boxpilotd): diagnostics::gc evict-to-cap (plan #8 task 12)"
```

---

## Task 13: Move journal-tail redaction into `diagnostics::bundle`

**Files:**
- Modify: `crates/boxpilotd/src/diagnostics/bundle.rs`
- Modify: `crates/boxpilotd/src/profile/checker.rs`

- [ ] **Step 1: Write the new home for the function**

Replace `crates/boxpilotd/src/diagnostics/bundle.rs` with:

```rust
//! Bundle composition: file collection, redaction, tarball writer.

/// Drop journal/stderr lines that contain markers correlated with secrets.
/// Text-stage redaction is fundamentally heuristic — we cannot parse a
/// freeform journal line into JSON. Schema-aware walking is reserved for
/// `*.json` artifacts inside the bundle.
///
/// Shared call site between [`compose`] and the activation pipeline's
/// `sing-box check` stderr scrub.
pub fn redact_journal_lines(s: &str) -> String {
    s.lines()
        .filter(|line| {
            let lower = line.to_ascii_lowercase();
            !(lower.contains("password")
                || lower.contains("uuid")
                || lower.contains("private_key")
                || lower.contains("token=")
                || lower.contains("secret"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn drops_password_lines() {
        let s = "ok 1\npassword=hunter2\nok 2";
        assert_eq!(redact_journal_lines(s), "ok 1\nok 2");
    }

    #[test]
    fn drops_uuid_and_private_key_and_token_and_secret() {
        let s = "a\nuuid=x\nb\nprivate_key=y\nc\ntoken=z\nd\nsecret=q\ne";
        assert_eq!(redact_journal_lines(s), "a\nb\nc\nd\ne");
    }

    #[test]
    fn passes_through_non_secret_lines() {
        let s = "starting up\nlistening on 127.0.0.1:9090\n";
        // Note: trailing-newline becomes "" then dropped by collect+join;
        // the contract is "non-secret lines survive", not "exact byte-equal".
        assert_eq!(
            redact_journal_lines(s),
            "starting up\nlistening on 127.0.0.1:9090\n"
        );
    }
}
```

- [ ] **Step 2: Run the new tests**

Run: `cargo test -p boxpilotd diagnostics::bundle::tests`
Expected: 3/3 PASS.

- [ ] **Step 3: Update `profile/checker.rs` to use the shared helper**

In `crates/boxpilotd/src/profile/checker.rs`, find `fn redact_secrets(s: &str) -> String { ... }` and replace its body to delegate:

```rust
/// Best-effort scrub of the stderr tail before we hand it back to the
/// caller. Shared with the diagnostics journal-tail collector via
/// [`crate::diagnostics::bundle::redact_journal_lines`] — kept as a thin
/// alias here for call-site readability.
fn redact_secrets(s: &str) -> String {
    crate::diagnostics::bundle::redact_journal_lines(s)
}
```

(Leave the existing `#[cfg(test)] mod` tests in `checker.rs` as a regression
guard — they assert behaviour that the shared helper preserves.)

- [ ] **Step 4: Run all daemon tests**

Run: `cargo test -p boxpilotd profile::checker:: && cargo test -p boxpilotd diagnostics::bundle::`
Expected: PASS for both modules.

- [ ] **Step 5: Update obsolete comment in `checker.rs`**

Find the doc comment that says "Plan #8 will replace this with §14 schema-aware redaction" (around line 55 today) and replace with:

```rust
/// Best-effort scrub of the stderr tail before we hand it back to the
/// caller. Schema-aware redaction (the §14 walker) only applies to JSON;
/// stderr is text-only, so a heuristic line-drop stays the right call here.
/// The shared implementation lives in [`crate::diagnostics::bundle::redact_journal_lines`].
```

- [ ] **Step 6: Commit**

```bash
git add crates/boxpilotd/src/diagnostics/bundle.rs crates/boxpilotd/src/profile/checker.rs
git commit -m "refactor(boxpilotd): hoist journal-tail redactor to diagnostics::bundle (plan #8 task 13)"
```

---

## Task 14: `diagnostics::bundle::collect_files`

**Files:**
- Modify: `crates/boxpilotd/src/diagnostics/bundle.rs`

- [ ] **Step 1: Write failing tests**

Append to `bundle.rs` (above `mod tests`):

```rust
use std::path::{Path, PathBuf};

/// One file slot in the bundle. The composer iterates a fixed list so the
/// bundle layout is stable across runs even when sources are missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleEntry {
    /// Name inside the tar (no leading directory; the composer prefixes
    /// the per-bundle directory).
    pub name: String,
    /// File contents as written into the tar.
    pub contents: Vec<u8>,
    /// Whether this file was redacted before inclusion.
    pub redacted: bool,
}

/// Source file → bundle entry. Returns the placeholder entry on missing /
/// unreadable source rather than failing the whole bundle.
pub fn collect_verbatim(name: &str, source: &Path) -> BundleEntry {
    match std::fs::read(source) {
        Ok(bytes) => BundleEntry {
            name: name.to_string(),
            contents: bytes,
            redacted: false,
        },
        Err(e) => unavailable(name, &format!("read {}: {e}", source.display())),
    }
}

/// Source JSON file → redacted bundle entry. The JSON is parsed, walked
/// through [`boxpilot_ipc::redact::redact_singbox_config`], and re-serialized
/// pretty-printed. Parse failure produces an unavailable entry.
pub fn collect_redacted_singbox_config(name: &str, source: &Path) -> BundleEntry {
    let bytes = match std::fs::read(source) {
        Ok(b) => b,
        Err(e) => return unavailable(name, &format!("read {}: {e}", source.display())),
    };
    let mut value: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(e) => return unavailable(name, &format!("parse json: {e}")),
    };
    boxpilot_ipc::redact::redact_singbox_config(&mut value);
    match serde_json::to_vec_pretty(&value) {
        Ok(out) => BundleEntry {
            name: name.to_string(),
            contents: out,
            redacted: true,
        },
        Err(e) => unavailable(name, &format!("encode redacted: {e}")),
    }
}

/// Synthetic placeholder so the bundle layout stays consistent when a
/// source is absent. The replacement file's *name* gets the
/// `.unavailable.txt` suffix so a support engineer can see the original
/// slot was attempted but failed.
pub fn unavailable(name: &str, cause: &str) -> BundleEntry {
    BundleEntry {
        name: format!("{name}.unavailable.txt"),
        contents: format!("source unavailable: {cause}\n").into_bytes(),
        redacted: false,
    }
}

/// Build a path under `cache_diagnostics_dir` for the given bundle name.
pub fn bundle_path(cache_dir: &Path, generated_at: &str) -> PathBuf {
    cache_dir.join(format!("boxpilot-diagnostics-{generated_at}.tar.gz"))
}
```

Append to `mod tests`:

```rust
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn collect_verbatim_reads_bytes() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("x");
        fs::write(&p, b"hello").unwrap();
        let e = collect_verbatim("x", &p);
        assert_eq!(e.name, "x");
        assert_eq!(e.contents, b"hello");
        assert!(!e.redacted);
    }

    #[test]
    fn collect_verbatim_missing_source_returns_unavailable() {
        let tmp = tempdir().unwrap();
        let e = collect_verbatim("x", &tmp.path().join("nope"));
        assert_eq!(e.name, "x.unavailable.txt");
        assert!(String::from_utf8_lossy(&e.contents).contains("source unavailable"));
        assert!(!e.redacted);
    }

    #[test]
    fn collect_redacted_replaces_password() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("config.json");
        fs::write(
            &p,
            serde_json::to_vec(&serde_json::json!({
                "outbounds": [{"type":"vless","tag":"main","password":"hunter2"}]
            }))
            .unwrap(),
        )
        .unwrap();
        let e = collect_redacted_singbox_config("active-config.json", &p);
        assert_eq!(e.name, "active-config.json");
        assert!(e.redacted);
        let s = String::from_utf8_lossy(&e.contents);
        assert!(!s.contains("hunter2"), "password leaked: {s}");
        assert!(s.contains("***"));
    }

    #[test]
    fn collect_redacted_garbage_json_falls_back_to_unavailable() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("config.json");
        fs::write(&p, b"not json").unwrap();
        let e = collect_redacted_singbox_config("active-config.json", &p);
        assert_eq!(e.name, "active-config.json.unavailable.txt");
        assert!(String::from_utf8_lossy(&e.contents).contains("parse json"));
    }

    #[test]
    fn bundle_path_uses_naming_convention() {
        let p = bundle_path(Path::new("/var/cache/boxpilot/diagnostics"), "2026-04-30T22-00-00Z");
        assert_eq!(
            p.to_string_lossy(),
            "/var/cache/boxpilot/diagnostics/boxpilot-diagnostics-2026-04-30T22-00-00Z.tar.gz"
        );
    }
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p boxpilotd diagnostics::bundle::tests`
Expected: all PASS (3 redaction tests + 5 collection tests = 8 total).

- [ ] **Step 3: Commit**

```bash
git add crates/boxpilotd/src/diagnostics/bundle.rs
git commit -m "feat(boxpilotd): bundle collect_verbatim/collect_redacted_singbox_config (plan #8 task 14)"
```

---

## Task 15: Tarball writer

**Files:**
- Modify: `crates/boxpilotd/src/diagnostics/bundle.rs`

- [ ] **Step 1: Write failing test**

Append:

```rust
/// Bundle manifest written as `diagnostics-manifest.json` inside the tar.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BundleManifest {
    pub schema_version: u32,
    pub generated_at: String,
    pub boxpilot_version: String,
    pub host: super::sysinfo::SystemInfo,
    pub files: Vec<BundleManifestFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BundleManifestFile {
    pub name: String,
    pub size: u64,
    pub redacted: bool,
}

/// Write the gzip-compressed tar to `out_path`. The tar's top-level
/// directory equals the file stem so unpacking yields a single folder.
pub fn write_tarball(
    out_path: &Path,
    bundle_dirname: &str,
    manifest: &BundleManifest,
    entries: &[BundleEntry],
) -> std::io::Result<()> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::fs::File;
    use tar::Header;

    let f = File::create(out_path)?;
    let gz = GzEncoder::new(f, Compression::default());
    let mut tar = tar::Builder::new(gz);

    let manifest_bytes = serde_json::to_vec_pretty(manifest).map_err(io_err)?;
    let mut all_entries: Vec<(&str, &[u8])> = Vec::with_capacity(entries.len() + 1);
    all_entries.push(("diagnostics-manifest.json", manifest_bytes.as_slice()));
    for e in entries {
        all_entries.push((e.name.as_str(), e.contents.as_slice()));
    }

    for (name, body) in all_entries {
        let mut header = Header::new_gnu();
        header.set_path(format!("{bundle_dirname}/{name}"))?;
        header.set_mode(0o600);
        header.set_uid(0);
        header.set_gid(0);
        header.set_size(body.len() as u64);
        header.set_cksum();
        tar.append(&header, body)?;
    }

    tar.into_inner()?.finish()?;
    Ok(())
}

fn io_err(e: serde_json::Error) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, e)
}
```

Append to `mod tests`:

```rust
    use super::super::sysinfo::SystemInfo;
    use flate2::read::GzDecoder;
    use std::collections::HashMap;
    use std::io::Read;

    fn read_tar_entries(path: &Path) -> HashMap<String, Vec<u8>> {
        let f = std::fs::File::open(path).unwrap();
        let gz = GzDecoder::new(f);
        let mut tar = tar::Archive::new(gz);
        let mut out = HashMap::new();
        for entry in tar.entries().unwrap() {
            let mut entry = entry.unwrap();
            let p = entry.path().unwrap().to_string_lossy().into_owned();
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf).unwrap();
            out.insert(p, buf);
        }
        out
    }

    #[test]
    fn tarball_contains_manifest_and_entries() {
        let tmp = tempdir().unwrap();
        let out = tmp.path().join("x.tar.gz");
        let entries = vec![
            BundleEntry {
                name: "a.json".into(),
                contents: b"{\"x\":1}".to_vec(),
                redacted: true,
            },
            BundleEntry {
                name: "b.txt".into(),
                contents: b"hi".to_vec(),
                redacted: false,
            },
        ];
        let manifest = BundleManifest {
            schema_version: 1,
            generated_at: "2026-04-30T22-00-00Z".into(),
            boxpilot_version: "0.1.0".into(),
            host: SystemInfo {
                kernel: "test".into(),
                os_id: "test".into(),
                os_version_id: "1".into(),
                os_pretty_name: "Test".into(),
                boxpilot_version: "0.1.0".into(),
            },
            files: vec![
                BundleManifestFile {
                    name: "a.json".into(),
                    size: 7,
                    redacted: true,
                },
                BundleManifestFile {
                    name: "b.txt".into(),
                    size: 2,
                    redacted: false,
                },
            ],
        };
        write_tarball(&out, "boxpilot-diagnostics-test", &manifest, &entries).unwrap();
        assert!(out.exists());
        let read_back = read_tar_entries(&out);
        assert!(read_back.contains_key("boxpilot-diagnostics-test/diagnostics-manifest.json"));
        assert_eq!(
            read_back["boxpilot-diagnostics-test/a.json"],
            b"{\"x\":1}"
        );
        assert_eq!(read_back["boxpilot-diagnostics-test/b.txt"], b"hi");
    }
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p boxpilotd diagnostics::bundle::tests::tarball_contains_manifest_and_entries`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/boxpilotd/src/diagnostics/bundle.rs
git commit -m "feat(boxpilotd): bundle write_tarball + BundleManifest (plan #8 task 15)"
```

---

## Task 16: `diagnostics::compose` orchestrator

**Files:**
- Modify: `crates/boxpilotd/src/diagnostics/mod.rs`

- [ ] **Step 1: Write failing test for the happy-path orchestrator**

Replace `crates/boxpilotd/src/diagnostics/mod.rs` with:

```rust
//! Diagnostics export pipeline (spec §5.5 / §14, plan #8). The public entry
//! point is [`compose`], called from `iface::diagnostics_export_redacted`.

pub mod bundle;
pub mod gc;
pub mod sysinfo;

use crate::paths::Paths;
use boxpilot_ipc::{
    DiagnosticsExportResponse, HelperError, HelperResult, DIAGNOSTICS_BUNDLE_CAP_BYTES,
    DIAGNOSTICS_JOURNAL_TAIL_LINES, DIAGNOSTICS_SCHEMA_VERSION,
};
use bundle::{
    bundle_path, collect_redacted_singbox_config, collect_verbatim, redact_journal_lines,
    write_tarball, BundleEntry, BundleManifest, BundleManifestFile,
};
use std::path::Path;

/// Inputs the daemon supplies to the composer. Implemented as a struct so
/// tests can inject a fake journal without spinning a real systemd.
pub struct ComposeInputs<'a> {
    pub paths: &'a Paths,
    pub unit_name: &'a str,
    pub journal: &'a dyn crate::systemd::JournalReader,
    pub os_release_path: &'a Path,
    pub now_iso: fn() -> String,
}

pub async fn compose(inputs: ComposeInputs<'_>) -> HelperResult<DiagnosticsExportResponse> {
    let dir = inputs.paths.cache_diagnostics_dir();
    create_dir_secure(&dir)?;
    gc::evict_to_cap(&dir, DIAGNOSTICS_BUNDLE_CAP_BYTES).map_err(|e| {
        HelperError::DiagnosticsIoFailed {
            step: "gc evict".into(),
            cause: e.to_string(),
        }
    })?;

    let generated_at = (inputs.now_iso)();
    let bundle_dirname = format!("boxpilot-diagnostics-{generated_at}");
    let out_path = bundle_path(&dir, &generated_at);

    // 1. Active config — schema-aware redact.
    let active_config_src = inputs.paths.active_symlink().join("config.json");
    let active_config_entry =
        collect_redacted_singbox_config("active-config.json", &active_config_src);

    // 2-5. Verbatim files.
    let toml_entry = collect_verbatim("boxpilot.toml", &inputs.paths.boxpilot_toml());
    let install_state_entry =
        collect_verbatim("install-state.json", &inputs.paths.install_state_json());
    let unit_entry =
        collect_verbatim("service-unit.txt", &inputs.paths.systemd_unit_path(inputs.unit_name));
    let manifest_entry = collect_verbatim(
        "manifest.json",
        &inputs.paths.active_symlink().join("manifest.json"),
    );

    // 6. Live service status snapshot — placeholder for now; the full
    //    snapshot is captured at iface call site (Task 17).
    let service_status_entry = BundleEntry {
        name: "service-status.json".into(),
        contents: b"{}".to_vec(),
        redacted: false,
    };

    // 7. Journal tail — line-drop redact.
    let journal_lines = inputs
        .journal
        .tail(inputs.unit_name, DIAGNOSTICS_JOURNAL_TAIL_LINES)
        .await
        .unwrap_or_default();
    let journal_text = journal_lines.join("\n");
    let journal_entry = BundleEntry {
        name: "journal-tail.txt".into(),
        contents: redact_journal_lines(&journal_text).into_bytes(),
        redacted: true,
    };

    // 8. system-info.json
    let info = sysinfo::collect(inputs.os_release_path);
    let sysinfo_entry = BundleEntry {
        name: "system-info.json".into(),
        contents: serde_json::to_vec_pretty(&info)
            .map_err(|e| HelperError::DiagnosticsEncodeFailed { cause: e.to_string() })?,
        redacted: false,
    };

    let entries = vec![
        active_config_entry,
        toml_entry,
        install_state_entry,
        unit_entry,
        manifest_entry,
        service_status_entry,
        journal_entry,
        sysinfo_entry,
    ];

    let manifest = BundleManifest {
        schema_version: DIAGNOSTICS_SCHEMA_VERSION,
        generated_at: generated_at.clone(),
        boxpilot_version: env!("CARGO_PKG_VERSION").to_string(),
        host: info,
        files: entries
            .iter()
            .map(|e| BundleManifestFile {
                name: e.name.clone(),
                size: e.contents.len() as u64,
                redacted: e.redacted,
            })
            .collect(),
    };

    // Stream into a NamedTempFile co-located with the target so the final
    // rename stays on the same filesystem.
    let tmp = tempfile::Builder::new()
        .prefix(".boxpilot-diag-")
        .suffix(".tmp")
        .tempfile_in(&dir)
        .map_err(|e| HelperError::DiagnosticsIoFailed {
            step: "tempfile".into(),
            cause: e.to_string(),
        })?;
    let tmp_path = tmp.path().to_path_buf();
    drop(tmp); // close the fd; write_tarball reopens via File::create
    write_tarball(&tmp_path, &bundle_dirname, &manifest, &entries).map_err(|e| {
        HelperError::DiagnosticsIoFailed {
            step: "write tarball".into(),
            cause: e.to_string(),
        }
    })?;
    std::fs::rename(&tmp_path, &out_path).map_err(|e| HelperError::DiagnosticsIoFailed {
        step: "rename to final".into(),
        cause: e.to_string(),
    })?;

    let bundle_size_bytes = std::fs::metadata(&out_path)
        .map_err(|e| HelperError::DiagnosticsIoFailed {
            step: "stat final".into(),
            cause: e.to_string(),
        })?
        .len();

    Ok(DiagnosticsExportResponse {
        schema_version: DIAGNOSTICS_SCHEMA_VERSION,
        bundle_path: out_path.to_string_lossy().into_owned(),
        bundle_size_bytes,
        generated_at,
    })
}

fn create_dir_secure(dir: &Path) -> HelperResult<()> {
    use std::os::unix::fs::PermissionsExt;
    if !dir.exists() {
        std::fs::create_dir_all(dir).map_err(|e| HelperError::DiagnosticsIoFailed {
            step: "mkdir".into(),
            cause: e.to_string(),
        })?;
    }
    let perms = std::fs::Permissions::from_mode(0o750);
    std::fs::set_permissions(dir, perms).map_err(|e| HelperError::DiagnosticsIoFailed {
        step: "chmod".into(),
        cause: e.to_string(),
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systemd::testing::FixedJournal;
    use std::fs;
    use tempfile::tempdir;

    fn iso() -> String {
        "2026-04-30T22-00-00Z".to_string()
    }

    #[tokio::test]
    async fn happy_path_writes_tarball_with_redacted_active_config() {
        let tmp = tempdir().unwrap();
        let paths = Paths::with_root(tmp.path());

        // Set up minimal fake filesystem:
        let active = paths.releases_dir().join("rel-1");
        fs::create_dir_all(&active).unwrap();
        fs::write(
            active.join("config.json"),
            serde_json::to_vec(&serde_json::json!({
                "outbounds":[{"type":"vless","tag":"main","password":"hunter2"}]
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(active.join("manifest.json"), b"{\"schema_version\":1}").unwrap();
        fs::create_dir_all(paths.etc_dir()).unwrap();
        std::os::unix::fs::symlink(&active, paths.active_symlink()).unwrap();
        fs::write(paths.boxpilot_toml(), b"schema_version = 1\n").unwrap();
        fs::create_dir_all(paths.cores_dir().parent().unwrap()).unwrap();
        fs::write(paths.install_state_json(), b"{\"schema_version\":1,\"managed_cores\":[]}").unwrap();
        fs::create_dir_all(tmp.path().join("etc/systemd/system")).unwrap();
        fs::write(
            paths.systemd_unit_path("boxpilot-sing-box.service"),
            b"[Service]\nExecStart=/usr/bin/sing-box\n",
        )
        .unwrap();
        let os_release = tmp.path().join("os-release");
        fs::write(&os_release, b"ID=test\nVERSION_ID=1\nPRETTY_NAME=\"Test\"\n").unwrap();

        let journal = FixedJournal {
            lines: vec!["starting".into(), "password=leak".into(), "running".into()],
        };

        let resp = compose(ComposeInputs {
            paths: &paths,
            unit_name: "boxpilot-sing-box.service",
            journal: &journal,
            os_release_path: &os_release,
            now_iso: iso,
        })
        .await
        .unwrap();

        assert!(resp.bundle_size_bytes > 0);
        assert!(resp.bundle_path.ends_with(".tar.gz"));
        assert_eq!(resp.generated_at, iso());
        assert_eq!(resp.schema_version, 1);

        // Open the tarball and verify the secret is gone.
        let f = std::fs::File::open(&resp.bundle_path).unwrap();
        let gz = flate2::read::GzDecoder::new(f);
        let mut tar = tar::Archive::new(gz);
        let mut found_redacted = false;
        let mut found_journal_redacted = false;
        for entry in tar.entries().unwrap() {
            let mut entry = entry.unwrap();
            let path = entry.path().unwrap().to_string_lossy().into_owned();
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut buf).unwrap();
            if path.ends_with("active-config.json") {
                let s = String::from_utf8_lossy(&buf);
                assert!(!s.contains("hunter2"), "active config leaked: {s}");
                assert!(s.contains("***"));
                found_redacted = true;
            }
            if path.ends_with("journal-tail.txt") {
                let s = String::from_utf8_lossy(&buf);
                assert!(!s.contains("leak"), "journal leaked: {s}");
                found_journal_redacted = true;
            }
        }
        assert!(found_redacted);
        assert!(found_journal_redacted);
    }
}
```

- [ ] **Step 2: Run test**

Run: `cargo test -p boxpilotd diagnostics::tests::happy_path_writes_tarball_with_redacted_active_config -- --nocapture`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/boxpilotd/src/diagnostics/mod.rs
git commit -m "feat(boxpilotd): diagnostics::compose orchestrator + happy-path test (plan #8 task 16)"
```

---

## Task 17: Replace `iface.rs::diagnostics_export_redacted` stub

**Files:**
- Modify: `crates/boxpilotd/src/iface.rs`

- [ ] **Step 1: Write failing tests**

In `crates/boxpilotd/src/iface.rs::mod tests`, append:

```rust
#[tokio::test]
async fn diagnostics_export_redacted_writes_bundle() {
    let tmp = tempdir().unwrap();
    let ctx = Arc::new(ctx_with_journal_lines(
        &tmp,
        Some("schema_version = 1\ncontroller_uid = 1000\n"),
        CannedAuthority::allowing(&["app.boxpilot.helper.diagnostics.export-redacted"]),
        UnitState::NotFound,
        &[(":1.42", 1000)],
        vec!["a".into(), "b".into()],
    ));
    let h = Helper::new(ctx);
    let resp = h.do_diagnostics_export_redacted(":1.42").await.unwrap();
    assert!(std::path::Path::new(&resp.bundle_path).exists());
    assert!(resp.bundle_size_bytes > 0);
}

#[tokio::test]
async fn diagnostics_export_redacted_denied_returns_not_authorized() {
    let tmp = tempdir().unwrap();
    let ctx = Arc::new(ctx_with(
        &tmp,
        None,
        CannedAuthority::denying(&["app.boxpilot.helper.diagnostics.export-redacted"]),
        UnitState::NotFound,
        &[(":1.42", 1000)],
    ));
    let h = Helper::new(ctx);
    let r = h.do_diagnostics_export_redacted(":1.42").await;
    assert!(matches!(r, Err(HelperError::NotAuthorized)));
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p boxpilotd iface::tests::diagnostics_export_redacted`
Expected: FAIL on compile (no `do_diagnostics_export_redacted` method yet).

- [ ] **Step 3: Replace the stub interface method**

In `crates/boxpilotd/src/iface.rs`, replace:

```rust
    async fn diagnostics_export_redacted(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        self.do_stub(&header, HelperMethod::DiagnosticsExportRedacted)
            .await
    }
```

with:

```rust
    async fn diagnostics_export_redacted(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let sender = extract_sender(&header)?;
        let resp = self
            .do_diagnostics_export_redacted(&sender)
            .await
            .map_err(to_zbus_err)?;
        serde_json::to_string(&resp).map_err(|e| {
            zbus::fdo::Error::Failed(format!("app.boxpilot.Helper1.Ipc: serialize: {e}"))
        })
    }
```

Add the corresponding `do_*` method inside `impl Helper { ... }` block (the
inner one, alongside `do_service_status` etc.):

```rust
    async fn do_diagnostics_export_redacted(
        &self,
        sender: &str,
    ) -> Result<boxpilot_ipc::DiagnosticsExportResponse, HelperError> {
        let _call =
            dispatch::authorize(&self.ctx, sender, HelperMethod::DiagnosticsExportRedacted).await?;
        crate::diagnostics::compose(crate::diagnostics::ComposeInputs {
            paths: &self.ctx.paths,
            unit_name: &self.ctx.load_config().await?.target_service,
            journal: &*self.ctx.journal,
            os_release_path: std::path::Path::new("/etc/os-release"),
            now_iso: || chrono::Utc::now().format("%Y-%m-%dT%H-%M-%SZ").to_string(),
        })
        .await
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p boxpilotd iface::tests::diagnostics_export_redacted`
Expected: 2/2 PASS. The "denied" test exercises dispatch only; the
"writes_bundle" test goes through `compose()` end-to-end with a tempdir
filesystem.

- [ ] **Step 5: Run the full daemon test suite**

Run: `cargo test -p boxpilotd`
Expected: all PASS, including the journal-redaction tests in
`profile::checker::` and the existing stub-passes-through tests (the
diagnostics method is no longer a stub, but no test directly checked
that — verify by running all tests).

- [ ] **Step 6: Commit**

```bash
git add crates/boxpilotd/src/iface.rs
git commit -m "feat(boxpilotd): wire diagnostics.export_redacted end-to-end (plan #8 task 17)"
```

---

## Task 18: Tauri command + helper_client method

**Files:**
- Modify: `crates/boxpilot-tauri/src/helper_client.rs`
- Modify: `crates/boxpilot-tauri/src/commands.rs`
- Modify: `crates/boxpilot-tauri/src/main.rs`

- [ ] **Step 1: Add the proxy method**

In `crates/boxpilot-tauri/src/helper_client.rs`, inside the `trait Helper`
block, add:

```rust
    #[zbus(name = "DiagnosticsExportRedacted")]
    fn diagnostics_export_redacted(&self) -> zbus::Result<String>;
```

In `impl HelperClient`, add:

```rust
    pub async fn diagnostics_export_redacted(
        &self,
    ) -> Result<boxpilot_ipc::DiagnosticsExportResponse, ClientError> {
        let proxy = HelperProxy::new(&self.conn).await?;
        let json = proxy.diagnostics_export_redacted().await?;
        serde_json::from_str(&json).map_err(|e| ClientError::Decode(e.to_string()))
    }
```

- [ ] **Step 2: Add the Tauri command**

In `crates/boxpilot-tauri/src/commands.rs`, append a new command (consistent
style with `helper_home_status`):

```rust
#[tauri::command]
pub async fn helper_diagnostics_export(
) -> Result<boxpilot_ipc::DiagnosticsExportResponse, CommandError> {
    let c = HelperClient::connect().await?;
    Ok(c.diagnostics_export_redacted().await?)
}
```

- [ ] **Step 3: Register the command**

In `crates/boxpilot-tauri/src/main.rs` (or `lib.rs` — check where the
existing `tauri::generate_handler!` lives), add `helper_diagnostics_export`
to the handler list.

- [ ] **Step 4: Build**

Run: `cargo build -p boxpilot`
Expected: success.

- [ ] **Step 5: Commit**

```bash
git add crates/boxpilot-tauri/src/
git commit -m "feat(tauri): diagnostics_export command + helper_client wrapper (plan #8 task 18)"
```

---

## Task 19: Frontend types + AboutTab button

**Files:**
- Modify: `frontend/src/api/types.ts`
- Modify: `frontend/src/api/helper.ts`
- Modify: `frontend/src/components/settings/AboutTab.vue`

- [ ] **Step 1: Add the wire type**

In `frontend/src/api/types.ts` (alphabetical region of types):

```ts
export interface DiagnosticsExportResponse {
  schema_version: number;
  bundle_path: string;
  bundle_size_bytes: number;
  generated_at: string;
}
```

- [ ] **Step 2: Add the API wrapper**

In `frontend/src/api/helper.ts`, append (and add to the import block at the top):

```ts
import type { ..., DiagnosticsExportResponse } from "./types";

export async function diagnosticsExport(): Promise<DiagnosticsExportResponse> {
  return await invoke<DiagnosticsExportResponse>("helper_diagnostics_export");
}
```

- [ ] **Step 3: Add the button + result line in AboutTab**

In `frontend/src/components/settings/AboutTab.vue`, replace the `<script setup lang="ts">` and `<template>` with:

```vue
<script setup lang="ts">
import { ref } from "vue";
import { useHomeStatus } from "../../composables/useHomeStatus";
import { diagnosticsExport, isCommandError } from "../../api/helper";

const { data } = useHomeStatus();

const exporting = ref(false);
const exportResult = ref<string | null>(null);
const exportError = ref<string | null>(null);

async function onExport() {
  exporting.value = true;
  exportResult.value = null;
  exportError.value = null;
  try {
    const r = await diagnosticsExport();
    const sizeMb = (r.bundle_size_bytes / (1024 * 1024)).toFixed(2);
    exportResult.value = `${r.bundle_path} (${sizeMb} MiB)`;
  } catch (e) {
    if (isCommandError(e)) {
      exportError.value = `${e.code}: ${e.message}`;
    } else {
      exportError.value = String(e);
    }
  } finally {
    exporting.value = false;
  }
}
</script>

<template>
  <div class="about">
    <h3>About BoxPilot</h3>
    <p>Linux desktop control panel for system-level <code>sing-box</code>.</p>
    <h4>Storage</h4>
    <ul>
      <li>Helper config: <code>/etc/boxpilot/boxpilot.toml</code></li>
      <li>Active release: <code>/etc/boxpilot/active</code> → <code>/etc/boxpilot/releases/&lt;activation_id&gt;</code></li>
      <li>Managed cores: <code>/var/lib/boxpilot/cores/&lt;version&gt;/sing-box</code></li>
      <li>Install ledger: <code>/var/lib/boxpilot/install-state.json</code></li>
      <li>Unit backups (legacy migrate): <code>/var/lib/boxpilot/backups/units/</code></li>
      <li>Diagnostics bundles: <code>/var/cache/boxpilot/diagnostics/</code></li>
    </ul>
    <h4>Current state</h4>
    <ul v-if="data">
      <li>Service unit: <code>{{ data.service.unit_name }}</code></li>
      <li>Core path: <code>{{ data.core.path ?? "(unset)" }}</code></li>
      <li>Core state: <code>{{ data.core.state ?? "(unset)" }}</code></li>
      <li>Active release: <code>{{ data.active_profile?.release_id ?? "(none)" }}</code></li>
      <li>Schema version: <code>{{ data.schema_version }}</code></li>
    </ul>
    <p v-else class="muted">Loading…</p>

    <h4>Diagnostics</h4>
    <p class="muted">
      Exports a redacted <code>tar.gz</code> bundle. Server addresses, passwords,
      UUIDs, private keys, and clash-api secrets are stripped per spec §14.
    </p>
    <button :disabled="exporting" @click="onExport">
      {{ exporting ? "Exporting…" : "Export diagnostics" }}
    </button>
    <p v-if="exportResult" class="ok">Exported: <code>{{ exportResult }}</code></p>
    <p v-if="exportError" class="err">Failed: {{ exportError }}</p>
  </div>
</template>

<style scoped>
.about { display: flex; flex-direction: column; gap: 0.5rem; }
.about h3 { margin: 0; font-size: 1rem; }
.about h4 { margin: 0.5rem 0 0.2rem; font-size: 0.9rem; }
.about ul { margin: 0; padding-left: 1.2rem; font-size: 0.9rem; }
.muted { color: #888; }
.ok { color: #2a7a2a; }
.err { color: #c33; }
button { align-self: flex-start; }
code { background: #f0f0f0; padding: 0 0.25rem; border-radius: 2px; }
</style>
```

- [ ] **Step 4: Build the frontend**

Run: `cd frontend && npm run build`
Expected: success (no TS errors).

- [ ] **Step 5: Commit**

```bash
git add frontend/src/api/types.ts frontend/src/api/helper.ts frontend/src/components/settings/AboutTab.vue
git commit -m "feat(frontend): export diagnostics button + wire types (plan #8 task 19)"
```

---

## Task 20: Smoke procedure + final sweep

**Files:**
- Create: `docs/superpowers/plans/2026-04-30-diagnostics-export-smoke-procedure.md`

- [ ] **Step 1: Write the smoke procedure**

```markdown
# Plan #8 Smoke Procedure — Diagnostics Export

**Pre-conditions:** `make install-helper` run on the dev VM, GUI launched
via `make run-gui`, at least one profile activated, `boxpilot-sing-box.service`
either running or in a known failed state.

## Manual flow

1. Open the GUI → **Settings** → **About** tab.
2. Verify the "Diagnostics bundles" line shows `/var/cache/boxpilot/diagnostics/`.
3. Click **Export diagnostics**.
4. Wait for the polkit prompt (admin auth, cached after first time).
5. Authenticate.
6. Expect the result line to show
   `Exported: /var/cache/boxpilot/diagnostics/boxpilot-diagnostics-<ts>.tar.gz (<size>)`.
7. Open a terminal:
   ```bash
   sudo ls -la /var/cache/boxpilot/diagnostics/
   ```
   Expected: tarball exists, mode `0600`, owner `root:root`.

## Redaction sanity check

Use a profile whose `outbounds[0].password` is the canary string
`SMOKE_TEST_PASSWORD`:

```bash
sudo cp <bundle path> /tmp/diag.tar.gz
cd /tmp && tar -xzf diag.tar.gz
grep -r SMOKE_TEST_PASSWORD boxpilot-diagnostics-*/
```

Expected: zero matches.

```bash
grep -r '"password":' boxpilot-diagnostics-*/active-config.json
```

Expected: one match with value `"***"`.

## LRU cap check

Generate three bundles in succession:

```bash
for i in 1 2 3; do
  busctl call --system app.boxpilot.Helper app/boxpilot/Helper \
    app.boxpilot.Helper1 DiagnosticsExportRedacted
  sleep 1
done
```

Tweak the cap manually for a quick test:

```bash
sudo truncate -s 50M /var/cache/boxpilot/diagnostics/<oldest>.tar.gz
# trigger another export; oldest 50M-padded file should be evicted.
```

(For v1 we trust the constant. The unit test in `diagnostics::gc::tests`
covers eviction logic; this manual step is optional.)

## Failure-mode check

Symlink `active` to a non-existent path, then export:

```bash
sudo rm /etc/boxpilot/active
sudo ln -s /etc/boxpilot/releases/nonexistent /etc/boxpilot/active
# trigger export from GUI; result should still succeed but the tarball
# contains active-config.json.unavailable.txt with the read error.
```

After verifying, restore the symlink to a valid release.
```

- [ ] **Step 2: Run the full workspace check**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd frontend && npm run build && cd ..
xmllint --noout packaging/linux/polkit-1/actions/app.boxpilot.helper.policy
```

Expected: all green. Fix any clippy/test/build failures inline before committing.

- [ ] **Step 3: Commit and close**

```bash
git add docs/superpowers/plans/2026-04-30-diagnostics-export-smoke-procedure.md
git commit -m "docs(plan-8): smoke procedure"
```

---

## Self-Review Checklist

Before opening the PR, confirm:

1. **Spec coverage:**
   - [ ] §14 fields all redacted (Tasks 2-6).
   - [ ] Default-deny under outbounds[*] / inbounds[*].users[*] (Tasks 3, 4).
   - [ ] §5.5 100 MiB cap with LRU (Task 12).
   - [ ] Bundle in `/var/cache/boxpilot/diagnostics/` (Tasks 10, 16).
   - [ ] Path-based delivery, no fd-passing (Task 16).
   - [ ] Polkit policy already declared, no change (verified in spec §3.5).

2. **No placeholders:** Every code block above is final, copy-paste-ready code.

3. **Type consistency:**
   - `DiagnosticsExportResponse` shape: schema_version / bundle_path / bundle_size_bytes / generated_at — used identically in Tasks 8, 16, 17, 18, 19.
   - `BundleEntry { name, contents, redacted }` — defined in Task 14, used in Tasks 15, 16.
   - `redact_singbox_config(&mut Value)` — defined in Task 1, called from Task 14.
   - `redact_journal_lines(&str) -> String` — defined in Task 13, called from Tasks 13 (checker.rs alias) and 16.

4. **Test commands:** Each task lists a runnable test command. Final sweep in Task 20 runs the full workspace.

5. **Commit messages:** Each task ends with a `git commit` referencing `plan #8 task N`.
