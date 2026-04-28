# BoxPilot

Linux desktop control panel for system-installed `sing-box`.

- **Design spec:** [`docs/superpowers/specs/2026-04-27-boxpilot-linux-design.md`](docs/superpowers/specs/2026-04-27-boxpilot-linux-design.md)
- **Plan #1 — skeleton + helperd:** [`docs/superpowers/plans/2026-04-27-boxpilot-skeleton-and-helperd.md`](docs/superpowers/plans/2026-04-27-boxpilot-skeleton-and-helperd.md)

## Status

Pre-1.0. Plan #1 establishes the workspace, the unprivileged Tauri GUI
shell, and the root D-Bus helper `boxpilotd` with its `service.status`
round-trip. All other privileged actions are stubbed; later plans (#2–#9)
fill them in.

## Layout

- `crates/boxpilot-ipc/` — shared serde types and config schema
- `crates/boxpilotd/` — root D-Bus helper (system-bus activated)
- `crates/boxpilot-tauri/` — Tauri 2 app (Rust side)
- `frontend/` — Vue 3 + TS + Vite (web side)
- `packaging/linux/` — D-Bus + polkit files (installed by `make install-helper`)

## Quick start (dev)

```bash
cargo build --release -p boxpilotd
sudo make install-helper
make run-gui
```

After clicking **Check service.status**, the panel shows the JSON returned
by `app.boxpilot.Helper1.ServiceStatus` — `unit_state.kind: not_found`
until plan #3 generates the unit.

## Building from source

Requires: Rust 1.78+, Node 20+, `polkit-daemon`, `dbus-daemon`, a polkit
authentication agent (any modern desktop ships one).

## License

GPL-3.0-or-later.
