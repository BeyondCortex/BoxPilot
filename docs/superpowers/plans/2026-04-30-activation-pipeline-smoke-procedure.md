# Plan #5 — Activation Pipeline Smoke Procedure

Run on a Debian/Ubuntu desktop (or VM) with systemd. Requires
`sudo`, `gdbus`, `python3`, `tar`, and a built `boxpilotd`.

## 1. Pre-flight (one time)

- Build: `cargo build --workspace --release`
- Install daemon: `sudo install -m 0755 target/release/boxpilotd /usr/local/libexec/boxpilotd`
- Install the D-Bus service file + polkit policy + helper rules from `packaging/linux/`.
- Reload polkit (or reboot): `sudo systemctl restart polkit`.
- Start daemon manually for the smoke run: in another terminal,
  `sudo /usr/local/libexec/boxpilotd` (foreground, watching logs).

## 2. Plan #2 / #3 prerequisites

These verbs were exercised in plan #2 / #3 smoke runs; they must be
green before plan #5 can succeed:

- Adopt or install a sing-box core via `core.install_managed` or
  `core.adopt`.
- Install the managed unit via `service.install_managed`.

After these, `/var/lib/boxpilot/cores/current/sing-box` exists and
`/etc/systemd/system/boxpilot-sing-box.service` is in place.

## 3. Build a test bundle

Either:

a) Use the new Tauri command from the GUI: `profile_activate` builds
   the bundle and calls the daemon in one shot. (Plan #7 will surface
   it in the UI; until then run the GUI in dev mode and call from
   the JS console: `await invoke('profile_activate', { request: { profile_id, core_path, core_version } })`.)

b) Build by hand with a Python helper. Sketch:

```python
import os, tarfile, json, ctypes, ctypes.util
libc = ctypes.CDLL(ctypes.util.find_library('c'), use_errno=True)
MFD_CLOEXEC, MFD_ALLOW_SEALING = 0x0001, 0x0002
F_ADD_SEALS = 1033
F_SEAL_SEAL = 0x0001; F_SEAL_SHRINK = 0x0002
F_SEAL_GROW = 0x0004; F_SEAL_WRITE = 0x0008
fd = libc.memfd_create(b"smoke", MFD_CLOEXEC | MFD_ALLOW_SEALING)
# tar config.json + manifest.json (assets optional) into fd
# (build manifest with activation_id, profile_*, config_sha256, etc.)
libc.fcntl(fd, F_ADD_SEALS, F_SEAL_WRITE | F_SEAL_GROW | F_SEAL_SHRINK | F_SEAL_SEAL)
# Then invoke gdbus with the fd; gdbus does NOT directly pass fds, so
# use python-dbus or zbus-from-CLI: simplest is to call from Rust or
# busctl. For ad-hoc smoke, the GUI route (a) is the easier path.
```

## 4. Verify happy path

After a successful `profile_activate` call:

- `readlink /etc/boxpilot/active` → resolves under `/etc/boxpilot/releases/<activation_id>/`.
- `systemctl is-active boxpilot-sing-box.service` → `active`.
- `cat /etc/boxpilot/boxpilot.toml` shows updated:
  - `active_release_id = "<new>"`
  - `active_profile_id = "..."`
  - `active_profile_sha256 = "..."`
  - `activated_at = "<rfc3339>"`
- The previous fields (if a prior activation ran) populated under `previous_*`.

## 5. Verify rollback paths

a. **`rolled_back`** — Pre-activate a working profile so previous
   exists. Build a bundle with an intentionally broken `config.json`
   (e.g. `outbounds: 0` instead of an array). Activate it. Daemon
   runs sing-box check, fails, returns `SingboxCheckFailed` (BEFORE
   rollback path). For the verify-window failure mode (passes check
   but service won't run), construct a bundle that passes check but
   has an inbound port that conflicts on the host (e.g. listen on
   :22). Expect `outcome=rolled_back`. `active` symlink restored to
   the previous release.

b. **`rollback_target_missing`** — On a fresh install with no prior
   activation, force a verify-window failure (port conflict). Daemon
   has no previous release, leaves `active` at the failed-but-checked
   release, stops service. Expect `outcome=rollback_target_missing`,
   `systemctl is-active … = inactive`.

c. **`rollback_unstartable`** — Activate a working profile A, then
   another working profile B (so previous=A, active=B). Then corrupt
   profile A's `releases/<A-id>/config.json` (e.g. truncate to 0
   bytes). Now activate a verify-failing profile C. Daemon runs check
   on C (succeeds), swaps active to C, restart fails verification,
   rollback path takes over: swaps active to A, restart, second
   verify also fails (A is corrupt). Outcome=`rollback_unstartable`,
   service stopped.

## 6. Verify GC

Run 12 successful activations in a row (each tweaks `log.level` so
all SHAs differ). After the 12th:

- `ls /etc/boxpilot/releases/` → at most 11 directories
  (≤10 keepable + active + previous, with overlap).
- `cat /etc/boxpilot/boxpilot.toml` still tracks the right pair.
- `du -sh /etc/boxpilot/releases/` ≤ 2 GiB (default cap).

## 7. Crash recovery

Kill the daemon (SIGKILL) during step 6's middle activation, ideally
between unpack and rename. Restart `boxpilotd`. Confirm:

- `journalctl -u boxpilot-helper.service` (or stderr if running
  manually) logs "swept stale activation .staging entries".
- `/etc/boxpilot/.staging/` is empty.
- The next activation succeeds normally.

## 8. Report results

Capture: outcomes table (which case → which outcome + which boxpilot.toml
state), output of `readlink /etc/boxpilot/active` after each, daemon
log tail. Diff against expected, file issues for any mismatch.
