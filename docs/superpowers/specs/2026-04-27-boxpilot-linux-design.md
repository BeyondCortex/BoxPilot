# BoxPilot Linux v1.0 Design

Date: 2026-04-27
Status: design draft for user review

## 1. Product Positioning

BoxPilot v1.0 is a Linux desktop control panel for system-level `sing-box` management.

It is not a generic subscription converter, not a private embedded-core proxy app, and not a multi-user server platform. Its job is to make the system-installed `sing-box` understandable and controllable from a small desktop UI.

The first release targets **systemd Linux desktop distributions**, with Debian/Ubuntu-family systems as the reference validation target. Windows and macOS are future ports that should reuse the same product model where possible, but they are not part of the v1.0 implementation target.

Confirmed stack:

- Desktop shell: Tauri 2
- Frontend: Vue 3 + TypeScript + Vite
- System layer: Rust
- Linux service layer: systemd
- Privileged helper: root-owned `boxpilotd` service exposed through a controlled local IPC/D-Bus interface and guarded by polkit

## 2. Scope

### In scope for Linux v1.0

- Manage system-level `sing-box` operation on systemd Linux.
- Reuse an existing system `sing-box` binary.
- Install and maintain a BoxPilot-managed `sing-box` core from official SagerNet GitHub releases.
- Manage a BoxPilot-owned systemd service: `boxpilot-sing-box.service`.
- Import, edit, validate, update, activate, back up, and delete sing-box-native JSON profiles.
- Support local JSON profiles and remote sing-box-native JSON URL profiles.
- Support profile bundles containing `config.json` plus local assets referenced by relative paths.
- Show runtime truth from systemd, active release metadata, service configuration, core version, and optional local sing-box API.
- Provide logs, diagnostics, drift detection, and rollback after failed activation.

### Out of scope for Linux v1.0

- Clash/Mihomo YAML subscription conversion.
- Treating AppImage as the complete privileged system-management installation path.
- Multi-user profile ownership. v1.0 uses a single controller user model.
- Default in-place takeover of an existing `sing-box.service`.
- Making desktop environment system-proxy integration part of the main control path.
- Running the GUI as root.

## 3. UI Model

BoxPilot uses three top-level pages.

### 3.1 Home

Home is the daily operation page.

It shows:

- Current service status: active, inactive, failed, enabling state, restart count.
- Current active profile: name, id, activation time, config hash.
- Current core path, version, and whether it is managed by BoxPilot.
- Network shape detected from the active config: TUN, local proxy, TUN plus local proxy, server, mixed, or unknown.
- Basic TUN indicators: TUN inbound exists, `auto_route`, `auto_redirect`, `strict_route`, `/dev/net/tun` availability.
- Recent journal tail for the current BoxPilot-managed service.
- Drift warnings when the runtime state no longer matches BoxPilot metadata.
- Actions: start, stop, restart, check active config, update current remote profile when applicable.

Home must prefer runtime truth over stored metadata.

### 3.2 Profiles

Profiles owns user configuration.

It supports:

- Create local profile from pasted JSON.
- Import local JSON file.
- Import local profile directory containing `config.json` and assets.
- Add remote sing-box-native JSON URL profile.
- Update remote profile.
- Edit profile JSON without losing unknown fields.
- Show structured overview of inbounds, outbounds, DNS, route, rule sets, and experimental settings.
- Patch common TUN fields from structured controls.
- Validate profile with the selected `sing-box` core before activation.
- Activate a profile through `boxpilotd`, producing a root-owned release bundle under `/etc/boxpilot`.
- Keep local profile history and last-known-valid snapshot.

Remote profiles support sing-box JSON only. YAML conversion is explicitly not implemented.

### 3.3 Settings

Settings groups system and advanced controls.

It includes:

- Core discovery and selection.
- Managed core install, upgrade, rollback, and version display.
- External core reuse without BoxPilot upgrades.
- `boxpilot-sing-box.service` install, enable, disable, start, stop, restart, and status.
- Existing `sing-box.service` observation and migration flow.
- Local API detection and optional patch to enable loopback-only `experimental.clash_api`.
- Diagnostics export with secret redaction.
- Storage path display.
- Controller user status.
- Advanced drift repair tools.

## 4. Runtime Architecture

The strong-consistency path is:

```text
BoxPilot GUI
  -> user-owned profile store
  -> user-side validation and bundle preparation
  -> boxpilotd privileged helper
  -> /etc/boxpilot/releases/<activation-id>/
  -> /etc/boxpilot/active
  -> boxpilot-sing-box.service
  -> sing-box run -c config.json
```

The systemd service never points to a file in the user's home directory.

## 5. File and Directory Layout

### 5.1 Application package files

For packaged Linux installs:

```text
/usr/bin/boxpilot
/usr/lib/boxpilot/boxpilotd
/usr/share/applications/boxpilot.desktop
/usr/share/dbus-1/system-services/app.boxpilot.Helper.service
/usr/share/polkit-1/actions/app.boxpilot.helper.policy
```

If a distribution uses `/usr/libexec`, `boxpilotd` may be installed at:

```text
/usr/libexec/boxpilot/boxpilotd
```

The package layout must be consistent per distribution target.

### 5.2 BoxPilot-managed sing-box core

```text
/usr/local/bin/sing-box
```

This path is only managed by BoxPilot after BoxPilot has an installation record or the controller user explicitly adopts the binary.

### 5.3 System runtime state

```text
/etc/boxpilot/
  boxpilot.toml
  active -> /etc/boxpilot/releases/<activation-id>/
  releases/
    <activation-id>/
      config.json
      assets/
      manifest.json
```

`/etc/boxpilot` is root-owned. Release directories are root-owned and not user-writable.

`boxpilot.toml` stores non-secret runtime mapping:

```toml
target_service = "boxpilot-sing-box.service"
core_path = "/usr/local/bin/sing-box"
core_managed = true
core_installed_by_boxpilot = true
controller_uid = 1000
active_profile_id = "profile-id"
active_profile_name = "Daily"
active_profile_sha256 = "sha256-hex"
active_release_id = "activation-id"
activated_at = "2026-04-27T00:00:00-07:00"
```

It must not store proxy passwords, full subscription URLs, private keys, or full profile JSON.

### 5.4 System backup and install state

```text
/var/lib/boxpilot/
  backups/
    cores/
    units/
    releases/
  install-state.json
```

Backups containing configuration content must be root-owned and not world-readable.

### 5.5 Cache and diagnostics

```text
/var/cache/boxpilot/
  downloads/
  diagnostics/
```

Downloads and diagnostics are subject to size limits and cleanup.

### 5.6 User profile store

```text
~/.local/share/boxpilot/
  profiles/
    <profile-id>/
      source.json
      assets/
      metadata.json
      last-valid/
        config.json
        assets/
  remotes.json
  ui-state.json
```

User profile directories should be `0700`. Secret-bearing files should be `0600`.

Remote URLs may contain tokens. UI display, logs, and diagnostics must redact them.

## 6. Privileged Helper Model

### 6.1 Helper form

Linux v1.0 uses a root helper service named `boxpilotd`, not a generic shell wrapper.

`boxpilotd` may be D-Bus activated and guarded by polkit. It should not require the GUI to run as root. Large profile bundles must not be sent as one unbounded D-Bus payload; the helper API should use Unix file-descriptor passing, a bounded local stream, or chunked transfer with explicit total-size limits after authorization.

### 6.2 Controller user model

v1.0 uses a single controller user per machine.

`boxpilotd` must identify the caller through the IPC credentials and compare the caller UID with `controller_uid` in `/etc/boxpilot/boxpilot.toml`.

- Matching controller user: read and write actions may be authorized by polkit.
- Non-controller local user: read-only status actions may be allowed; write actions require an explicit controller-transfer flow.

### 6.3 Allowed actions

`boxpilotd` only supports a whitelist of typed actions, such as:

- `service.status`
- `service.start`
- `service.stop`
- `service.restart`
- `service.enable`
- `service.disable`
- `service.install_managed`
- `profile.activate_bundle`
- `profile.rollback_release`
- `core.discover`
- `core.install_managed`
- `core.upgrade_managed`
- `core.rollback_managed`
- `legacy.observe_service`
- `legacy.migrate_service`
- `diagnostics.export_redacted`

It must not accept arbitrary shell commands, arbitrary filesystem paths, arbitrary systemd unit names, arbitrary executable paths, or arbitrary download URLs.

### 6.4 Locking

All privileged mutating operations must acquire a global lock:

```text
/run/boxpilot/lock
```

The lock prevents concurrent service installation, core upgrades, profile activation, rollback, and garbage collection.


### 6.5 Trusted executable paths

Any core path used in a generated systemd unit must pass trust checks inside `boxpilotd`:

- The file exists and is executable.
- The file is owned by root.
- The file is not writable by group or others.
- Every parent directory up to `/` is not writable by untrusted users.
- The path is under an allowed system prefix such as `/usr/bin`, `/usr/local/bin`, or an explicitly adopted root-owned installation directory.
- The binary reports a valid `sing-box version`.

This prevents a user-controlled binary under a home directory or writable path from being executed by root through `boxpilot-sing-box.service`.

### 6.6 Controller initialization and transfer

On a fresh install, the first local user who completes an authorized system write action becomes `controller_uid`. A controller transfer requires either the current controller's authorization or an administrator-authorized transfer action. Non-controller users do not get silent write access merely because they can launch the GUI.

## 7. Managed systemd Service

### 7.1 Main service

BoxPilot's primary service is:

```text
boxpilot-sing-box.service
```

It is the only default service that supports Profile activation.

Unit content is generated from `core_path` and should resemble:

```ini
[Unit]
Description=BoxPilot managed sing-box service
Documentation=https://sing-box.sagernet.org/
After=network-online.target nss-lookup.target
Wants=network-online.target

[Service]
Type=simple
User=root
UMask=0077
WorkingDirectory=/etc/boxpilot/active
ExecStartPre=/actual/core/path check -c config.json
ExecStart=/actual/core/path run -c config.json
Restart=on-failure
RestartSec=5s
StartLimitIntervalSec=60
StartLimitBurst=3
LimitNOFILE=infinity

[Install]
WantedBy=multi-user.target
```

`/actual/core/path` is generated from the selected core path after `boxpilotd` trust checks. It is not hardcoded unless the configured path is hardcoded.

### 7.2 Runtime verification

After service changes, BoxPilot checks:

- `systemctl show boxpilot-sing-box.service` fields such as `ActiveState`, `SubState`, `ExecMainStatus`, `NRestarts`.
- The generated unit still points to `/etc/boxpilot/active` and the configured core path.
- `/etc/boxpilot/active` resolves under `/etc/boxpilot/releases`.
- Active manifest and active config hash match `boxpilot.toml`.
- If loopback `experimental.clash_api` is enabled, the local API responds.

Journal text is diagnostic evidence, not the sole success criterion.

## 8. Existing sing-box.service Handling

Existing `sing-box.service` is not modified by default.

BoxPilot supports three modes:

1. **Observation mode**
   - Show service status, start, stop, restart, and logs.
   - No BoxPilot Profile activation into that service.

2. **Migration mode**
   - Read the existing unit and config path.
   - Import the existing config as a BoxPilot Profile.
   - Create and enable `boxpilot-sing-box.service`.
   - Disable the old service only after explicit confirmation.

3. **Advanced in-place takeover**
   - Hidden under advanced settings.
   - Show a complete diff.
   - Use drop-ins or controlled replacement only after explicit confirmation.
   - Not part of the default path.

The recommended path is migration to `boxpilot-sing-box.service`.

## 9. Profile Bundle Model

### 9.1 User profile bundle

A Profile is either:

- A single sing-box JSON file.
- A directory containing `source.json` or `config.json` and local assets.
- A remote sing-box-native JSON URL cached into a local profile.

The editor treats JSON as `serde_json::Value`. Structured editing is implemented as patch operations against the JSON value. Unknown fields must be preserved.

### 9.2 Activation bundle

When activating, the user-side backend prepares a constrained bundle containing:

```text
config.json
assets/
manifest.json
```

The bundle is transferred to `boxpilotd` through the controlled IPC channel using a bounded stream or passed file descriptor. `boxpilotd` does not read arbitrary user-supplied paths as root, and it must enforce request and bundle size limits before unpacking.

During unpacking, `boxpilotd` rejects:

- Absolute paths.
- `../` path traversal.
- Symlinks.
- Hardlinks.
- Device files.
- Files over configured size limits.
- Bundles over configured total size limits.

### 9.3 Absolute path dependencies

BoxPilot supports relative paths inside the activation bundle.

If config analysis detects absolute paths, especially under `/home`, `/tmp`, or other mutable user locations, BoxPilot marks them as external dependency risk. It does not automatically copy arbitrary absolute paths. The user can either import a directory bundle or explicitly accept the external dependency risk.

## 10. Activation, Rollback, and Garbage Collection

Activation flow:

```text
1. User edits or selects a Profile.
2. User-side backend validates JSON syntax.
3. User-side backend runs selected sing-box check when possible.
4. User-side backend builds constrained activation bundle.
5. boxpilotd acquires /run/boxpilot/lock.
6. boxpilotd unpacks into /etc/boxpilot/.staging/<activation-id>/.
7. boxpilotd runs: cd staging && <core_path> check -c config.json.
8. boxpilotd moves staging to /etc/boxpilot/releases/<activation-id>/.
9. boxpilotd atomically switches /etc/boxpilot/active to the new release.
10. boxpilotd restarts boxpilot-sing-box.service.
11. boxpilotd waits a short verification window.
12. boxpilotd verifies systemd state and optional local API.
13. On success, boxpilotd updates boxpilot.toml.
14. On failure, boxpilotd switches active back to previous release and restarts again.
```

Release retention:

- Always keep active release.
- Always keep previous release.
- Keep the most recent 10 releases by default.
- Garbage collection only deletes releases not referenced by active or previous.
- Garbage collection requires the same global lock.

## 11. Core Management

BoxPilot distinguishes external and managed cores.

### 11.1 External core

Examples:

```text
/usr/bin/sing-box
/usr/local/bin/sing-box without BoxPilot install record
```

External core behavior:

- Can be discovered.
- Can be selected for checks and service execution.
- Can display version.
- Is not upgraded or overwritten by BoxPilot.

### 11.2 Managed core

Managed core path:

```text
/usr/local/bin/sing-box
```

A core is managed only if BoxPilot installed it or the controller user explicitly adopts it.

Managed core operations:

- Install from official SagerNet/sing-box release source.
- Upgrade.
- Roll back to a backed-up binary.
- Validate with `sing-box version`.
- Regenerate service unit when `core_path` changes.

`boxpilotd` accepts version and architecture choices from trusted typed requests. It does not accept arbitrary download URLs.

If upstream provides checksums or digests for release assets, BoxPilot verifies them. If not, BoxPilot records the computed SHA256 of the downloaded asset and installed binary and shows the source and digest to the user.

## 12. Runtime and Clash-like API

Base runtime information comes from:

- systemd state;
- active release manifest;
- active config parsing;
- core version;
- journal tail for diagnostics.

Advanced runtime information can come from sing-box `experimental.clash_api` when configured to listen only on loopback by default.

BoxPilot can:

- Detect whether local API is enabled.
- Offer a patch to enable local API on `127.0.0.1`.
- Use it for connections, traffic, mode-like views, and outbound status when available.

The local API is optional for core service management. It is required only for advanced runtime panels.

## 13. Drift Detection

Home must detect drift between metadata and runtime.

Checks include:

- `boxpilot-sing-box.service` exists and is the target service.
- Unit `ExecStart` and `ExecStartPre` match configured core path and `config.json` under `/etc/boxpilot/active`.
- `/etc/boxpilot/active` resolves to `/etc/boxpilot/releases/<id>`.
- Release manifest hash matches active config.
- `boxpilot.toml` active profile hash matches active config hash.
- Core binary path still exists and version matches recorded state when known.
- Current service state matches expected running/stopped state.

When drift is detected, BoxPilot shows a warning and offers repair actions instead of silently overwriting runtime state.

## 14. Security and Privacy

Sensitive materials include:

- Proxy passwords.
- UUIDs.
- Private keys.
- Remote profile URLs and URL query tokens.
- Server addresses when exporting diagnostics.
- Full profile JSON.

Rules:

- User profile files: `0600` where possible.
- User profile directories: `0700` where possible.
- System release directories: root-owned and not user-writable.
- `boxpilotd` must not log request bodies or full configs.
- Error responses must avoid echoing full configs.
- Diagnostics export must redact tokens, passwords, UUIDs, private keys, server addresses, and remote URL query strings by default.
- GUI must avoid showing full remote URLs unless the user explicitly reveals them.

## 15. Packaging Strategy

The full Linux v1.0 experience is delivered through system packages. The reference v1.0 package target is `.deb` for Debian/Ubuntu-family systems. The same filesystem, helper, systemd, and polkit design is intended to support `.rpm` packaging without changing the runtime architecture.

The package owns the GUI binary, helper daemon, D-Bus service file, polkit policy, desktop file, and default resources.

AppImage may be provided later as a UI-only or limited-management package, but it is not the primary full-feature Linux v1.0 delivery format.

## 16. Acceptance Criteria

Linux v1.0 is acceptable when all of these are true on a supported systemd desktop distribution:

1. BoxPilot installs as a normal desktop app package without running the GUI as root.
2. `boxpilotd` can perform authorized privileged operations without accepting arbitrary commands or paths.
3. BoxPilot can discover external `sing-box` and show that it is external.
4. BoxPilot can install a managed `sing-box` core to `/usr/local/bin/sing-box` and record it as managed.
5. BoxPilot can create `boxpilot-sing-box.service` using the selected core path.
6. BoxPilot can import a local JSON profile.
7. BoxPilot can import a directory profile with relative assets.
8. BoxPilot can add and update a remote sing-box-native JSON URL profile.
9. Profile activation creates a root-owned release bundle under `/etc/boxpilot/releases`.
10. Activation runs `sing-box check` from the release working directory before switching active.
11. Failed activation rolls back to the previous release and reports the failure.
12. Home displays runtime truth from systemd and active release state.
13. Drift caused by manual system changes is detected and displayed.
14. Existing `sing-box.service` can be observed without being modified.
15. Migration from existing `sing-box.service` imports its config and moves runtime to `boxpilot-sing-box.service` only after explicit confirmation.
16. Logs and diagnostics redact sensitive values by default.
17. JSON editing preserves unknown sing-box fields.

## 17. References

- Tauri frontend model: https://v2.tauri.app/start/frontend/
- sing-box documentation: https://sing-box.sagernet.org/
- sing-box GitHub releases: https://github.com/SagerNet/sing-box
- Clash Verge Rev as UI inspiration: https://github.com/clash-verge-rev/clash-verge-rev
