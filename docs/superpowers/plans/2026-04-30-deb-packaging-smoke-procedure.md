# Plan #9 — `.deb` Packaging Smoke Procedure

**Pre-conditions:** Clean Debian 13 / Ubuntu 24.04 VM with the build
deps from `debian/control` installed:

```bash
sudo apt install -y \
    cargo rustc nodejs npm \
    pkg-config libssl-dev libgtk-3-dev libwebkit2gtk-4.1-dev \
    libayatana-appindicator3-dev librsvg2-dev libsoup-3.0-dev \
    debhelper devscripts dbus polkitd systemd
```

## 1. Build

```bash
cd /path/to/boxpilot
make deb
ls -la ../boxpilot_*.deb
```

Expected: a single `boxpilot_0.1.0-1_<arch>.deb` next to the repo.

## 2. Inspect the package

```bash
dpkg-deb -I ../boxpilot_0.1.0-1_*.deb            # control metadata
dpkg-deb -c  ../boxpilot_0.1.0-1_*.deb           # contents
lintian      ../boxpilot_0.1.0-1_*.deb || true   # advisory only
```

Expected file list (must include all of):

```
./usr/bin/boxpilot                                                # symlink
./usr/lib/boxpilot/boxpilot
./usr/lib/boxpilot/boxpilotd
./usr/lib/systemd/system/boxpilotd.service
./usr/share/dbus-1/system-services/app.boxpilot.Helper.service
./usr/share/dbus-1/system.d/app.boxpilot.helper.conf
./usr/share/polkit-1/actions/app.boxpilot.helper.policy
./usr/share/polkit-1/rules.d/49-boxpilot.rules
./usr/share/applications/app.boxpilot.desktop
./usr/share/icons/hicolor/32x32/apps/boxpilot.png
./usr/share/icons/hicolor/128x128/apps/boxpilot.png
./usr/share/icons/hicolor/256x256/apps/boxpilot.png
./etc/boxpilot/                                                   # empty dir
./var/lib/boxpilot/                                               # empty dir
./var/cache/boxpilot/                                             # empty dir
```

The `Depends:` line in `dpkg-deb -I` output must list `dbus`,
`policykit-1 | polkitd`, `systemd`, `libwebkit2gtk-4.1-0`, `libgtk-3-0`,
`libayatana-appindicator3-1`.

## 3. Install

```bash
sudo apt install ./boxpilot_0.1.0-1_*.deb
```

Expected: install completes without prompts. dpkg picks up the
maintainer scripts; no errors logged about systemd / dbus reload.

```bash
ls /usr/lib/boxpilot/                                       # boxpilot, boxpilotd
ls -la /usr/bin/boxpilot                                    # symlink → /usr/lib/boxpilot/boxpilot
systemctl status boxpilotd.service 2>&1 | head -5           # `loaded` (inactive until first call)
busctl introspect app.boxpilot.Helper /app/boxpilot/Helper  # auto-activates the helper
systemctl status boxpilotd.service 2>&1 | head -5           # now `active`
```

Desktop entry visible:

```bash
grep -l BoxPilot /usr/share/applications/*.desktop
gtk-launch app.boxpilot 2>/dev/null &                       # opens the GUI
```

## 4. End-to-end GUI smoke

Re-run the plan #7 smoke procedure against the **installed package**
(not the dev `make run-gui` flow):

```bash
boxpilot &
```

Then walk the smoke procedure at
`docs/superpowers/plans/2026-04-30-gui-shell-smoke-procedure.md`:

- Home tab loads, `service.status` populates within a few seconds.
- Profiles tab: import a known-good local profile JSON; activation
  succeeds; `boxpilot-sing-box.service` becomes active.
- Settings → About: shows the BoxPilot version (`0.1.0`).

## 5. Upgrade test

Build a `0.1.0-2` package by bumping `debian/changelog`, rebuild:

```bash
make deb
sudo apt install ./boxpilot_0.1.0-2_*.deb
```

Expected:
- `boxpilot-sing-box.service` keeps running across the upgrade.
- The next `service.status` call still works.
- No prompts about modified conffiles.

## 6. Remove (preserve user data)

```bash
sudo apt remove boxpilot
```

Expected:
- `/usr/bin/boxpilot`, `/usr/lib/boxpilot/`, `/usr/lib/systemd/system/boxpilotd.service`, desktop entry, icons all gone.
- `/etc/boxpilot/`, `/var/lib/boxpilot/`, `/var/cache/boxpilot/` retained.
- `/etc/systemd/system/boxpilot-sing-box.service` (runtime-generated) retained.
- `boxpilot-sing-box.service` keeps running.

```bash
ls /etc/boxpilot/ /var/lib/boxpilot/ /var/cache/boxpilot/
systemctl status boxpilot-sing-box.service | head -3
```

## 7. Purge (clean up everything)

```bash
sudo apt purge boxpilot
```

Expected:
- All three runtime dirs removed.
- `/etc/systemd/system/boxpilot-sing-box.service` removed.
- An external `/etc/systemd/system/sing-box.service` (if any) is **not**
  touched.

```bash
ls /etc/boxpilot/ 2>/dev/null              # No such file or directory
ls /var/lib/boxpilot/ 2>/dev/null          # No such file or directory
ls /var/cache/boxpilot/ 2>/dev/null        # No such file or directory
ls /etc/systemd/system/boxpilot-sing-box.service 2>/dev/null   # No such file
ls /etc/systemd/system/sing-box.service 2>/dev/null            # If user had one, still here
```

## 8. Workspace gates (host repo)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd frontend && npm install --no-audit --no-fund && npm run build && cd ..
xmllint --noout packaging/linux/polkit-1/actions/app.boxpilot.helper.policy
xmllint --noout packaging/linux/dbus/system.d/app.boxpilot.helper.conf
file packaging/linux/icons/hicolor/*/apps/boxpilot.png
```

All must pass green.
