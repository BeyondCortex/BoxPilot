# Plan #6 — Legacy `sing-box.service` Handling — Smoke Procedure

Smoke target: a Linux desktop with a hand-rolled `sing-box.service` already installed (e.g. via the upstream `.deb` or a manual unit file) and an installed `boxpilotd` from this branch.

## Preconditions

- `boxpilotd` running, registered on the system bus.
- An existing `/etc/systemd/system/sing-box.service` unit with an `ExecStart=/usr/bin/sing-box run -c /etc/sing-box/config.json` line and a real config + at least one sibling asset (e.g. `geosite.db`) in `/etc/sing-box/`.
- The legacy unit is NOT named `boxpilot-sing-box.service`.
- `boxpilot.toml::target_service` is `boxpilot-sing-box.service` (default).

## 1. Observation — happy path

```sh
busctl --system call app.boxpilot.Helper /app/boxpilot/Helper app.boxpilot.Helper1 LegacyObserveService
```

Expected JSON (fields):

- `detected: true`
- `unit_name: "sing-box.service"`
- `fragment_path: "/etc/systemd/system/sing-box.service"`
- `config_path: "/etc/sing-box/config.json"`
- `config_path_kind: "system_path"`
- `unit_state.active_state` reflects current systemctl state.
- `conflicts_with_managed: false`

## 2. Observation — user-path config

Edit the legacy fragment to point at `/home/<you>/sb.json`, then `systemctl daemon-reload && systemctl restart sing-box.service`. Re-run LegacyObserveService.

Expected: `config_path_kind: "user_or_ephemeral"`. The GUI should refuse migration; the `LegacyConfigPathUnsafe` error appears only on `LegacyMigrateService prepare`, not on observe — observe is informational.

Restore the original fragment before continuing.

## 3. Migration prepare

```sh
busctl --system call app.boxpilot.Helper /app/boxpilot/Helper app.boxpilot.Helper1 LegacyMigrateService s '{"step":"prepare"}'
```

Expected:

- `step: "prepare"`
- `config_filename: "config.json"`
- `config_bytes` = the bytes of the legacy config (verify via `sha256sum /etc/sing-box/config.json` and compare to a manual hash of the response).
- `assets` contains exactly the regular files in `/etc/sing-box/` other than `config.json` and not symlinks.

## 4. User-side import

In the running GUI, exercise `profile_import_file` (or `profile_import_dir` if assets present) feeding the prepare response. Confirm the new profile appears in `~/.local/share/boxpilot/profiles/`.

## 5. Migration cutover

Before cutover, capture: `systemctl is-active sing-box.service` and `systemctl is-enabled sing-box.service`.

```sh
busctl --system call app.boxpilot.Helper /app/boxpilot/Helper app.boxpilot.Helper1 LegacyMigrateService s '{"step":"cutover"}'
```

Expected:

- After call, `systemctl is-active sing-box.service` reports `inactive` (or unit absent).
- `systemctl is-enabled sing-box.service` reports `disabled` (or `not-found`).
- `/var/lib/boxpilot/backups/units/sing-box.service-<timestamp>` exists, mode `0600`, root-owned, contents identical to the original fragment.

## 6. Activation

Click "Activate this profile" in the GUI for the imported profile. Confirm the standard plan-#5 activation pipeline runs and `boxpilot-sing-box.service` is enabled + started without error. The two services never run concurrently because cutover stopped the legacy unit before activation began.

## 7. Recovery if cutover fails

Failure modes by error code:

- `LegacyStopFailed` — fragment_path was read and the unit fragment was backed up to `/var/lib/boxpilot/backups/units/`; the legacy unit was NOT stopped or disabled. Safe to retry; the legacy `sing-box.service` is unchanged.
- `LegacyDisableFailed` — the legacy unit has been stopped and a backup was written, but `disable` did not complete. The unit is stopped now, but its symlinks under `multi-user.target.wants/` may still point at the fragment, so the legacy unit will return at next reboot. Recovery: re-run cutover, or manually `sudo systemctl disable sing-box.service`.

In neither case does BoxPilot start `boxpilot-sing-box.service`. Activation only proceeds when the user separately invokes `profile.activate_bundle` against the imported profile.

## 8. Authorization gating

As a non-controller user, run the bus call from §1. Expected: `LegacyObserveService` returns successfully (it's read-only with `allow_any: yes`).

As a non-controller user, run the bus call from §3. Expected: polkit prompt for admin auth (`auth_admin`, no caching).
