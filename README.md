# BoxPilot

Linux desktop control panel for system-installed `sing-box`.

- **Design spec:** [`docs/superpowers/specs/2026-04-27-boxpilot-linux-design.md`](docs/superpowers/specs/2026-04-27-boxpilot-linux-design.md)
- **Plans:** [`docs/superpowers/plans/`](docs/superpowers/plans/)

## Status

v0.1.0 — first packaged release. Plans #1–#9 complete:

| # | Plan | Status |
|---|------|--------|
| 1 | skeleton + helperd | ✅ |
| 2 | managed core | ✅ |
| 3 | managed service | ✅ |
| 4 | profile store | ✅ |
| 5 | activation pipeline | ✅ |
| 6 | legacy `sing-box.service` handling | ✅ |
| 7 | GUI shell | ✅ |
| 8 | diagnostics export with redaction | ✅ |
| 9 | `.deb` packaging + GUI auto-launch | ✅ |

**Windows port:** in progress on `feat/windows-support`. Sub-project #1
(platform abstraction) lands the trait surface and a Windows minimum-boot
helper service. Real Windows verbs and installer arrive in Sub-projects
#2 and #3.

## Install (Debian / Ubuntu)

Download the latest `.deb` from the GitHub Releases page, then:

```bash
sudo apt install ./boxpilot_0.1.0-1_amd64.deb
```

After install, launch **BoxPilot** from the desktop application menu, or
run `boxpilot` from a terminal.

If you previously ran the dev `make install-helper`, run
`sudo make uninstall-helper` first so `dpkg` does not flag file conflicts.

To remove (preserves user data under `/etc/boxpilot/`,
`/var/lib/boxpilot/`, `/var/cache/boxpilot/`):

```bash
sudo apt remove boxpilot
```

To purge user data as well:

```bash
sudo apt purge boxpilot
```

## Layout

- `crates/boxpilot-ipc/` — shared serde types and config schema
- `crates/boxpilotd/` — root D-Bus helper (system-bus / systemd activated)
- `crates/boxpilot-tauri/` — Tauri 2 GUI (Rust side)
- `crates/boxpilot-profile/` — profile store, validation, redaction
- `frontend/` — Vue 3 + TS + Vite (web side)
- `packaging/linux/` — D-Bus, polkit, systemd, desktop, icons
- `debian/` — Debian source package definition

## Building from source

Requirements (Debian / Ubuntu):

```bash
sudo apt install \
    cargo rustc nodejs npm \
    pkg-config libssl-dev libgtk-3-dev libwebkit2gtk-4.1-dev \
    libayatana-appindicator3-dev librsvg2-dev libsoup-3.0-dev \
    debhelper devscripts dbus polkitd systemd
```

Build the `.deb` (recommended):

```bash
make deb
sudo apt install ../boxpilot_*.deb
```

Or do a dev install without packaging:

```bash
sudo make install-helper      # build + install to system paths
make run-gui                  # cargo tauri dev with hot reload
```

`sudo make uninstall-helper` reverses the dev install.

## License

GPL-3.0-or-later.
