# BoxPilot Linux v1.0 Design

Date: 2026-04-27
Status: design draft for user review (revision r1)

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
- Target sing-box: SagerNet sing-box ≥ 1.10 (required for `auto_redirect` and the v1.0 TUN control set; older versions are detected and surfaced as compatibility warnings rather than silently downgrading features)

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
- Run a best-effort `sing-box check` against the selected core before activation as a fast UX preflight; the authoritative validation always runs inside `boxpilotd` against the staged release (see §10 step 7).
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
/usr/share/dbus-1/system.d/app.boxpilot.helper.conf
/usr/share/polkit-1/actions/app.boxpilot.helper.policy
```

The `system.d/*.conf` file is the system bus access policy. Without it the GUI cannot own or call the `app.boxpilot.Helper` bus name even with polkit authorization. The policy must allow `send_destination="app.boxpilot.Helper"` from any local user; method-level authorization is enforced by polkit at call time, not by D-Bus policy.

If a distribution uses `/usr/libexec`, `boxpilotd` may be installed at:

```text
/usr/libexec/boxpilot/boxpilotd
```

The package layout must be consistent per distribution target.

### 5.2 BoxPilot-managed sing-box core

Managed cores are versioned and isolated from system-administrator paths:

```text
/var/lib/boxpilot/cores/
  <version>/
    sing-box
    sha256
    install-source.json
  current -> <version>/
```

`current` is a root-owned symlink that BoxPilot atomically swings between installed versions. The systemd unit references `/var/lib/boxpilot/cores/current/sing-box`, so version upgrades do not require unit regeneration.

`/usr/local/bin/sing-box` is treated as an external core. BoxPilot never silently writes to `/usr/local/bin`. The controller user may explicitly adopt `/usr/local/bin/sing-box`, but adoption copies the binary into a new `/var/lib/boxpilot/cores/adopted-<timestamp>/` directory rather than reclassifying the original path — so adoption never changes the meaning of `/usr/local/bin/sing-box` for other tools or admins.

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
schema_version = 1
target_service = "boxpilot-sing-box.service"
core_path = "/var/lib/boxpilot/cores/current/sing-box"
core_state = "managed-installed"   # one of: "external" | "managed-installed" | "managed-adopted"
controller_uid = 1000
active_profile_id = "profile-id"
active_profile_name = "Daily"
active_profile_sha256 = "sha256-hex"
active_release_id = "activation-id"
activated_at = "2026-04-27T00:00:00-07:00"
```

It must not store proxy passwords, full subscription URLs, private keys, or full profile JSON. On every read, `boxpilotd` rejects unknown `schema_version` values rather than guessing — future migrations are explicit and atomic.

### 5.4 System backup and install state

```text
/var/lib/boxpilot/
  backups/
    units/    # previous unit content captured before service-unit regeneration
  install-state.json
  cores/      # managed core install tree, see §5.2
```

Neither `releases/` nor `cores/` is duplicated under `backups/`: release bundles are content-addressed under `/etc/boxpilot/releases/<activation-id>/` and rolled back by swinging `/etc/boxpilot/active`; managed core versions are kept in `/var/lib/boxpilot/cores/<version>/` and rolled back by swinging `current`. Only the on-disk `service-unit` text needs a separate backup trail, since unit content is otherwise generated rather than versioned.

`install-state.json` schema (v1.0):

```json
{
  "schema_version": 1,
  "managed_cores": [
    { "version": "1.10.0",
      "path": "/var/lib/boxpilot/cores/1.10.0/sing-box",
      "sha256": "<hex>",
      "installed_at": "2026-04-27T00:00:00-07:00",
      "source": "github-sagernet" }
  ],
  "adopted_cores": [
    { "label": "adopted-2026-04-27T00-00-00Z",
      "path": "/var/lib/boxpilot/cores/adopted-2026-04-27T00-00-00Z/sing-box",
      "sha256": "<hex>",
      "adopted_from": "/usr/local/bin/sing-box",
      "adopted_at": "2026-04-27T00:00:00-07:00" }
  ],
  "current_managed_core": "1.10.0"
}
```

Backups containing configuration content must be root-owned and not world-readable.

### 5.5 Cache and diagnostics

```text
/var/cache/boxpilot/
  downloads/
  diagnostics/
```

Default caps (overridable via the same `[limits]` section as §9.2): `downloads/` ≤ 1 GiB total, `diagnostics/` ≤ 100 MiB total. Both directories use LRU eviction beyond the cap. Diagnostics export bundles are written here before the user is given a path to share.

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

`last-valid/` is updated only after a successful activation completes step 12 of §10 — i.e. after `sing-box check` passes inside the staging directory **and** runtime verification confirms the service started cleanly. It is the snapshot the editor falls back to on user-initiated "revert" and the source of truth for "last known good" comparisons.

User profile directories should be `0700`. Secret-bearing files should be `0600`.

Remote URLs may contain tokens. UI display, logs, and diagnostics must redact them.

## 6. Privileged Helper Model

### 6.1 Helper form

Linux v1.0 uses a root helper service named `boxpilotd`, not a generic shell wrapper.

`boxpilotd` is D-Bus activated on the system bus and guarded by polkit. It must not require the GUI to run as root. Caller identity is established from D-Bus connection credentials via `org.freedesktop.DBus.GetConnectionUnixUser`, and on any auxiliary Unix socket via `SO_PEERCRED`. Identity is **never** taken from values supplied in the request body.

Large profile bundles must not be sent as one unbounded D-Bus payload. The helper API uses Unix file-descriptor passing for bundle uploads (the GUI hands the daemon a sealed `memfd` or a chunked stream), with explicit per-file and total-size limits enforced before unpacking and after polkit authorization.

### 6.2 Controller user model

v1.0 uses a single controller user per machine.

`boxpilotd` must identify the caller through the IPC credentials and compare the caller UID with `controller_uid` in `/etc/boxpilot/boxpilot.toml`.

- Matching controller user: read and write actions may be authorized by polkit.
- Non-controller local user: read-only status actions may be allowed; write actions require an explicit controller-transfer flow.

### 6.3 Allowed actions

`boxpilotd` only supports a whitelist of typed actions:

- `service.status`
- `service.start`
- `service.stop`
- `service.restart`
- `service.enable`
- `service.disable`
- `service.install_managed`
- `service.logs`            # bounded journal tail for boxpilot-sing-box.service
- `profile.activate_bundle`
- `profile.rollback_release`
- `core.discover`
- `core.install_managed`
- `core.upgrade_managed`
- `core.rollback_managed`
- `core.adopt`
- `legacy.observe_service`
- `legacy.migrate_service`
- `controller.transfer`
- `diagnostics.export_redacted`

Each action maps 1:1 to a polkit action under the `app.boxpilot.helper.` namespace, e.g. `service.start` → `app.boxpilot.helper.service.start`, `profile.activate_bundle` → `app.boxpilot.helper.profile.activate-bundle`. The polkit policy file at `/usr/share/polkit-1/actions/app.boxpilot.helper.policy` declares each action with an `auth_*` mode appropriate to its class:

- Read-only status (e.g. `service.status`, `service.logs`, `core.discover`) → `auth_self_keep` for the controller, `yes` for non-controllers (no auth prompt needed).
- Mutating actions (`service.start`, `profile.activate_bundle`, …) → `auth_admin_keep` for non-controllers, `auth_self_keep` for the controller.
- High-risk actions (`controller.transfer`, `legacy.migrate_service`) → `auth_admin` (always re-prompt; no caching).

`boxpilotd` rejects any incoming method whose name is not in this whitelist before consulting polkit. It must not accept arbitrary shell commands, arbitrary filesystem paths, arbitrary systemd unit names, arbitrary executable paths, or arbitrary download URLs.

### 6.4 Locking

All privileged mutating operations must acquire a global lock:

```text
/run/boxpilot/lock
```

The lock is held using `flock(2)` (advisory, exclusive). `/run` is tmpfs and is cleared on reboot, so stale lock files cannot survive a crash-restart. The lock prevents concurrent service installation, core upgrades, profile activation, rollback, garbage collection, and controller-UID assignment.


### 6.5 Trusted executable paths

Any core path used in a generated systemd unit must pass trust checks inside `boxpilotd`:

- The file exists and is executable.
- The file is owned by root.
- The file is not writable by group or others.
- Every parent directory up to `/` is not writable by untrusted users.
- The path is under an allowed system prefix: `/usr/bin`, `/usr/local/bin`, `/var/lib/boxpilot/cores/<version>/`, or an explicitly adopted root-owned installation directory recorded in `install-state.json`.
- The binary's setuid/setgid/sticky mode bits are not set. A normal `sing-box` distribution binary does not need them, and their presence is treated as suspicious.
- After symlink resolution, every component of the resolved path passes the same trust checks (no half-trusted chain where the symlink is in `/usr/local/bin` but the target is under `/home`).
- The binary reports a valid `sing-box version`.

This prevents a user-controlled binary under a home directory or writable path from being executed by root through `boxpilot-sing-box.service`.

### 6.6 Controller initialization and transfer

On a fresh install, `controller_uid` is unset. The first local user to acquire `/run/boxpilot/lock` and complete an authorized system write action becomes `controller_uid`. The lock serializes this assignment, so simultaneous first-time prompts cannot race into a split-controller state.

A controller transfer requires either the current controller's authorization or an administrator-authorized transfer action. Non-controller users do not get silent write access merely because they can launch the GUI.

If `controller_uid` resolves to a UID that no longer exists (for example, the user was deleted with `userdel`), `boxpilotd` reports `controller_orphaned` on every status query. Mutating actions are blocked until the next admin-authorized `controller.transfer` succeeds; read-only actions remain available.

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
StartLimitIntervalSec=300
StartLimitBurst=5
LimitNOFILE=1048576

# Sandboxing — keep what TUN / auto_redirect need, drop everything else
NoNewPrivileges=true
CapabilityBoundingSet=CAP_NET_ADMIN CAP_NET_BIND_SERVICE CAP_NET_RAW
AmbientCapabilities=CAP_NET_ADMIN CAP_NET_BIND_SERVICE CAP_NET_RAW
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
ProtectControlGroups=true
RestrictNamespaces=true
RestrictRealtime=true
LockPersonality=true
RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6 AF_NETLINK AF_PACKET
ReadWritePaths=/etc/boxpilot/active

[Install]
WantedBy=multi-user.target
```

`/actual/core/path` is generated from the selected core path after `boxpilotd` trust checks. For managed cores it resolves to `/var/lib/boxpilot/cores/current/sing-box`, so version upgrades do not require unit regeneration. It is not hardcoded unless the configured path is hardcoded.

The sandbox keeps the capabilities sing-box needs for TUN (`CAP_NET_ADMIN` for `auto_route` and netlink, `CAP_NET_BIND_SERVICE` for low-port inbounds, `CAP_NET_RAW` for ICMP) and drops everything else. `ProtectKernelTunables` is intentionally **not** set because `auto_redirect` writes to `/proc/sys/net/...` sysctls; if a future feature stops needing sysctl writes the option should be added. If a user's config requires additional writable paths (e.g. a custom cache file outside the active release), BoxPilot exposes a one-click drop-in editor under Settings → Service rather than rewriting the main unit. `StartLimitBurst=5` over `IntervalSec=300` is intentionally looser than systemd defaults to tolerate slow DNS, ModemManager, and late `network-online.target`.

### 7.2 Runtime verification

After service changes, BoxPilot checks:

- `systemctl show boxpilot-sing-box.service` fields such as `ActiveState`, `SubState`, `ExecMainStatus`, `NRestarts`.
- The generated unit still points to `/etc/boxpilot/active` and the configured core path.
- `/etc/boxpilot/active` resolves under `/etc/boxpilot/releases`.
- Active manifest and active config hash match `boxpilot.toml`.
- If loopback `experimental.clash_api` is enabled, the local API responds.

The verification window is **5 seconds by default**, capped at 30 s, and configurable per activation. Within the window BoxPilot polls service state and (if applicable) the local API; success requires `ActiveState=active` and `SubState=running` with `NRestarts` unchanged from pre-activation. Journal text is diagnostic evidence, not the sole success criterion.

## 8. Existing sing-box.service Handling

Existing `sing-box.service` is not modified by default.

BoxPilot supports three modes:

1. **Observation mode**
   - Show service status, start, stop, restart, and logs.
   - No BoxPilot Profile activation into that service.

2. **Migration mode**
   - Read the existing unit and config path.
   - **Copy** the existing config (and any locally-referenced assets it points at) into the user's profile store as a new BoxPilot Profile. The new Profile must not retain references to the old config's filesystem path — system services must never depend on `/home/<user>/...`.
   - Create `boxpilot-sing-box.service` but do **not** enable or start it yet.
   - Require explicit confirmation, then atomically: stop and disable the old `sing-box.service`, then enable and start `boxpilot-sing-box.service`. The two services must never run concurrently — they would race for the same TUN device name and the second to start would fail. If the stop/disable step fails, BoxPilot aborts and does not start the new service.
   - If the old unit's config path is under `/home`, `/tmp`, `/run/user/`, or another non-system path, BoxPilot warns before migration and refuses to keep that path as a runtime reference.

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

`manifest.json` schema (v1.0):

```json
{
  "schema_version": 1,
  "activation_id": "2026-04-27T00-00-00Z-abc123",
  "profile_id": "profile-id",
  "profile_sha256": "<hex>",
  "config_sha256": "<hex>",
  "source_kind": "local | local-dir | remote",
  "source_url_redacted": "https://host/path?token=***",
  "core_path_at_activation": "/var/lib/boxpilot/cores/current/sing-box",
  "core_version_at_activation": "1.10.x",
  "created_at": "2026-04-27T00:00:00-07:00",
  "assets": [
    { "path": "geosite.db", "sha256": "<hex>", "size": 12345 }
  ]
}
```

The bundle is transferred to `boxpilotd` through the controlled IPC channel using a sealed `memfd` or a chunked stream over a passed file descriptor. `boxpilotd` does not read arbitrary user-supplied paths as root, and it enforces size limits before unpacking.

Default size limits (overridable via a future `[limits]` section of `boxpilot.toml`):

| Limit | Default |
|-------|---------|
| Maximum file size in bundle | 16 MiB |
| Maximum total bundle size | 64 MiB |
| Maximum file count in bundle | 1024 |
| Maximum nesting depth under `assets/` | 8 |

The user-side backend must also verify that every asset referenced by `config.json` (e.g. `geosite.db`, rule-set files) is present in the bundle's `assets/` directory before submission. Missing references abort activation with a structured error rather than producing a release that fails at runtime.

During unpacking, `boxpilotd` rejects:

- Absolute paths.
- `../` path traversal, including paths that escape the staging root after symlink expansion at every step of the walk (defense against tar-bomb-style mid-extraction symlinks).
- Symlinks.
- Hardlinks.
- Device files, FIFOs, sockets.
- Files over the per-file size limit.
- Bundles over the total size limit, file count, or nesting depth.
- Filenames containing NUL bytes, alternate Unicode encodings of path separators, or control characters.

### 9.3 Absolute path dependencies

BoxPilot supports relative paths inside the activation bundle.

If config analysis detects absolute paths, especially under `/home`, `/tmp`, `/run/user/`, or other mutable user locations, BoxPilot marks them as external dependency risk. It does not automatically copy arbitrary absolute paths.

**Default behavior at activation time: refuse.** A profile with absolute path references to non-system locations cannot be activated until either:

- the user re-imports the profile as a directory bundle so the assets land under `assets/` and the config is rewritten to use relative paths, or
- the user explicitly opts in via Settings → Advanced → "Allow absolute external paths in active profiles", an opt-in that is logged and surfaced in the Home drift panel.

This is a conscious safety choice: a system service running as root must not silently read configuration from a path that any local user can replace.

## 10. Activation, Rollback, and Garbage Collection

Activation flow:

```text
 1. User edits or selects a Profile.
 2. User-side backend validates JSON syntax.
 3. User-side backend runs selected sing-box check when a usable core is reachable as the current uid; this is best-effort and never authoritative.
 4. User-side backend builds constrained activation bundle.
 5. boxpilotd acquires /run/boxpilot/lock (flock(2)).
 6. boxpilotd unpacks into /etc/boxpilot/.staging/<activation-id>/.
 7. boxpilotd runs: cd staging && <core_path> check -c config.json.
 8. boxpilotd renames staging to /etc/boxpilot/releases/<activation-id>/ (rename(2), same filesystem, atomic).
 9. boxpilotd creates /etc/boxpilot/active.new pointing at the new release, then rename(2)-replaces active with active.new — single inode swap. Never `ln -sfn`, which is unlink-then-symlink and leaves a crash window where active does not exist.
10. boxpilotd restarts boxpilot-sing-box.service.
11. boxpilotd waits the verification window (default 5 s, see §7.2).
12. boxpilotd verifies systemd state and optional local API.
13. On success, boxpilotd writes boxpilot.toml.new, fsyncs, then rename(2)-replaces boxpilot.toml.
14. On failure, boxpilotd creates active.new pointing at the previous release directory and rename(2)-replaces active (same atomic pattern as step 9), then restarts the service.
15. boxpilotd runs a second verification pass over the rolled-back release. On success, the transaction closes with "activation failed; rolled back". On failure (the previous release also will not start), boxpilotd stops boxpilot-sing-box.service to prevent a systemd Restart-loop, and surfaces `rollback_unstartable` to the GUI — a distinct, explicit terminal state from `rollback_target_missing` described below.
```

Crash recovery: on `boxpilotd` startup, before serving requests, the daemon walks `/etc/boxpilot/.staging/` and removes every subdirectory there — staging is only valid mid-call, and any leftover represents a crashed transaction. It also verifies that `/etc/boxpilot/active` resolves under `/etc/boxpilot/releases/`; if it does not (corrupted symlink, deleted target), the daemon refuses to (re)start the managed service and surfaces a repair prompt in the GUI.

If the previous release referenced for rollback in step 14 no longer exists on disk (manual `rm`, prior aggressive GC, disk corruption), boxpilotd does **not** attempt to fabricate one. It stops `boxpilot-sing-box.service`, leaves `active` pointing at the failed-but-checked release, and reports `rollback_target_missing` to the GUI. The user is then prompted to pick another release from history or to import a known-good profile manually. Reaching this state should never happen in normal operation; it is an explicit, visible failure rather than a silent service flap.

Release retention:

- Always keep active release.
- Always keep previous release.
- Keep the most recent 10 releases **and** total `releases/` directory ≤ 2 GiB by default; whichever bound is hit first wins.
- Garbage collection only deletes releases not referenced by active or previous, oldest-first.
- Garbage collection requires the same global lock.

## 11. Core Management

BoxPilot distinguishes external and managed cores.

### 11.1 External core

Examples:

```text
/usr/bin/sing-box                  (distribution-packaged)
/usr/local/bin/sing-box            (admin-installed; never written by BoxPilot)
~/.local/bin/sing-box              (rejected — fails §6.5 trust check)
```

External core behavior:

- Can be discovered.
- Can be selected for checks and service execution if it passes the §6.5 trust check.
- Can display version.
- Is never upgraded, overwritten, or deleted by BoxPilot.

### 11.2 Managed core

Managed cores live in their own versioned tree, isolated from `/usr/local/bin`:

```text
/var/lib/boxpilot/cores/
  <version>/
    sing-box
    sha256
    install-source.json
  current -> <version>/
```

A core is managed only if BoxPilot installed it (`core_state = "managed-installed"`) or the controller user explicitly adopted it (`core_state = "managed-adopted"`). Adoption copies the source binary into a new `/var/lib/boxpilot/cores/adopted-<timestamp>/` directory; it does not reclassify the original path.

Managed core operations:

- Install from official SagerNet/sing-box release source into a new versioned directory.
- Upgrade by installing a new versioned directory and atomically swinging `current` (rename(2) on the symlink).
- Roll back by swinging `current` to a previous versioned directory still on disk.
- Validate with `sing-box version` after each install.
- The service unit always references `/var/lib/boxpilot/cores/current/sing-box`; version upgrades do not regenerate the unit. Unit regeneration is only required when the *kind* of core changes (managed → external, or vice versa).

### 11.3 Architecture and source

`boxpilotd` accepts version and architecture choices from trusted typed requests. It does not accept arbitrary download URLs; the upstream source is hardcoded to `https://github.com/SagerNet/sing-box/releases`.

Linux v1.0 supported architectures: `x86_64`, `aarch64`. Asset naming follows upstream's `sing-box-<version>-linux-<arch>.tar.gz` convention. `armv7` and other architectures may be added post-1.0 once a tested install path exists.

**Linux v1.0**: SagerNet does not publish per-release `checksums.txt` files alongside `sing-box` GitHub assets. BoxPilot therefore does not verify upstream checksums; the `install-source.json::upstream_sha256_match` field is reserved and remains `null` for every install. Trust rests on TLS to `github.com`, GitHub asset immutability, and post-install audit by the user via the SHA-8 prefix shown in the cores list (full SHA256 is recorded in `install-source.json` for the determined).

The verification code path is preserved: should SagerNet (or a future fork BoxPilot tracks) publish a `checksums.txt` again, BoxPilot will fetch and verify it without a daemon update — the `upstream_sha256_match` field will start populating with `Some(true)` for matching installs.

A future revision may migrate to GitHub Artifact Attestations for cryptographic per-asset provenance once upstream enables them; that would be a stronger primitive than `checksums.txt` and would supersede the field semantics here.

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
- `controller_uid` resolves to a live local user (`controller_orphaned` from §6.6 surfaces here as a drift signal, not a silent state).
- Whether the "allow absolute external paths" opt-in from §9.3 is engaged. When it is, the drift panel lists the active config references that triggered it so the user can re-import as a directory bundle.

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

Redaction must be **sing-box-schema-aware**, not regex-on-text. The exporter walks the JSON tree and zeros known sensitive fields by JSON path — at minimum: `outbounds[*].password`, `outbounds[*].uuid`, `outbounds[*].private_key`, `outbounds[*].server`, `outbounds[*].server_port`, `inbounds[*].users[*].password`, `inbounds[*].users[*].uuid`, `dns.servers[*].address` host portion, `experimental.clash_api.secret`, and remote-URL `?token=` / `?key=` / equivalent auth query components. A pure regex over JSON text both misses structured secrets (binary-encoded keys, etc.) and false-positives on unrelated hex strings. Unknown fields under known sensitive containers are redacted by default rather than passed through.

**Subscription URL split.** The full remote-profile URL (with tokens) is stored *only* in the user-side `~/.local/share/boxpilot/remotes.json` (`0600`). The system-side activation manifest under `/etc/boxpilot/releases/<id>/manifest.json` records only `source_url_redacted`. This split is intentional: a system administrator auditing `/etc` and `/var/lib/boxpilot` must not be able to recover subscription tokens from any controller user's profile. The "update remote" action reads the full URL from the user-side store and never writes it system-side.

## 15. Packaging Strategy

The full Linux v1.0 experience is delivered through system packages. The reference v1.0 package target is `.deb` for Debian/Ubuntu-family systems. The same filesystem, helper, systemd, and polkit design is intended to support `.rpm` packaging without changing the runtime architecture.

The package owns the GUI binary, helper daemon, D-Bus service file, polkit policy, desktop file, and default resources.

AppImage may be provided later as a UI-only or limited-management package, but it is not the primary full-feature Linux v1.0 delivery format.

## 16. Acceptance Criteria

Linux v1.0 is acceptable when all of these are true on a supported systemd desktop distribution:

1. BoxPilot installs as a normal desktop app package without running the GUI as root.
2. `boxpilotd` can perform authorized privileged operations without accepting arbitrary commands or paths.
3. BoxPilot can discover external `sing-box` and show that it is external.
4. BoxPilot can install a managed `sing-box` core under `/var/lib/boxpilot/cores/<version>/`, swing `current` to it, and record it in `install-state.json`. `/usr/local/bin/sing-box` is never written by BoxPilot during this flow.
5. BoxPilot can create `boxpilot-sing-box.service` using the selected core path.
6. BoxPilot can import a local JSON profile.
7. BoxPilot can import a directory profile with relative assets.
8. BoxPilot can add and update a remote sing-box-native JSON URL profile.
9. Profile activation creates a root-owned release bundle under `/etc/boxpilot/releases`.
10. Activation runs `sing-box check` from the release working directory before switching active.
11. Failed activation rolls back to the previous release and reports the failure. Two distinct terminal failure states are surfaced explicitly rather than as silent service flaps: `rollback_target_missing` (no previous release on disk; `active` left at the failed release, service stopped) and `rollback_unstartable` (previous release exists but its second verification pass fails; service stopped to prevent a systemd Restart-loop).
12. Home displays runtime truth from systemd and active release state.
13. Drift caused by manual system changes is detected and displayed.
14. Existing `sing-box.service` can be observed without being modified.
15. Migration from existing `sing-box.service` imports its config and moves runtime to `boxpilot-sing-box.service` only after explicit confirmation.
16. Logs and diagnostics redact sensitive values by default.
17. JSON editing preserves unknown sing-box fields.
18. Activation is atomic from the user's perspective: either the new release is fully active and verified, or the previous release is restored and the failure is surfaced. Partial states (active points to an unchecked release, or to a non-existent target) are never visible to consumers under normal operation.
19. The managed sing-box service runs with a reduced capability set (`CAP_NET_ADMIN`, `CAP_NET_BIND_SERVICE`, `CAP_NET_RAW`) and the systemd hardening directives in §7.1, not as unrestricted root.

## 17. References

- Tauri frontend model: https://v2.tauri.app/start/frontend/
- sing-box documentation: https://sing-box.sagernet.org/
- sing-box GitHub releases: https://github.com/SagerNet/sing-box
- Clash Verge Rev as UI inspiration: https://github.com/clash-verge-rev/clash-verge-rev
- systemd service hardening (`systemd.exec`): https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html
- polkit reference: https://www.freedesktop.org/software/polkit/docs/latest/
- Filesystem Hierarchy Standard 3.0: https://refspecs.linuxfoundation.org/FHS_3.0/

## 18. Open Questions for Subsequent Revisions

These are tracked but not committed in v1.0:

- **Remote profile auto-update policy.** Today: manual only. Future: opt-in scheduled refresh per remote profile, exposed via `remotes.json` and surfaced in Settings. Needs a story for failure handling (stale-but-valid vs refuse-to-stay-active-when-stale).
- **`.deb` distribution channel.** Sideloaded `.deb` vs hosted apt repository (signing key trust, Release file rotation, mirror story). Until decided, v1.0 ships as a sideloaded `.deb` with a published checksum and a clear "no auto-update" disclosure.
- **Additional architectures.** `armv7`, `riscv64`, and `mips64` are not in v1.0 but the install-source code path is structured to add them without schema changes.
- **Multi-controller / fleet mode.** Out of scope for v1.0 by design; the file layout already accommodates adding it later (`controller_uid` becomes `controllers = [...]`).
- **In-process patch flow for enabling local API.** §3.3 / §12 describe enabling `experimental.clash_api`; this is a profile mutation that triggers a re-activation. The exact UX (inline toggle vs guided edit) is not finalized.
- **System tray and desktop-environment system-proxy hooks.** §2 explicitly excludes them from v1.0; design left open.
