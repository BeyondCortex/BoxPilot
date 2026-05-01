# Plan #9 — `.deb` Packaging Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship BoxPilot as a Debian package. After this plan, `sudo apt
install ./boxpilot_0.1.0_amd64.deb` installs the helper, the GUI, all
D-Bus / polkit / systemd / desktop / icon files, and `boxpilot` becomes a
launchable desktop application.

**Architecture:** A `debian/` source tree under the repo root drives
`dpkg-buildpackage` (`debhelper-compat = 13`, `dh` sequencer with
overrides for cargo + npm). New shipped files: `boxpilotd.service`,
`app.boxpilot.desktop`, hicolor icons, three maintainer scripts. The
existing dev `Makefile` keeps its `install-helper` / `uninstall-helper`
targets and gains a `deb` target.

**Tech Stack:** Rust 1.78 workspace, Vite + Vue 3 frontend, Tauri 2,
debhelper 13, dpkg-buildpackage, systemd, polkit, D-Bus.

**Spec parent:** `docs/superpowers/specs/2026-04-30-deb-packaging-design.md`

---

## File Structure

### New files (packaging tree)

| Path | Responsibility |
|------|----------------|
| `debian/changelog` | Debian-format changelog; v0.1.0 entry with the plan #9 release notes. |
| `debian/control` | Source + binary package metadata, build / runtime deps. |
| `debian/copyright` | DEP-5 machine-readable license header (GPL-3.0-or-later). |
| `debian/rules` | Executable Makefile; `dh` sequencer with cargo + npm overrides. |
| `debian/source/format` | `3.0 (native)`. |
| `debian/boxpilot.dirs` | Empty dirs to create at install (`/etc/boxpilot/`, `/var/lib/boxpilot/`, `/var/cache/boxpilot/`). |
| `debian/boxpilot.install` | File → install path mapping for everything except the cargo build outputs. |
| `debian/boxpilot.links` | `/usr/bin/boxpilot` → `/usr/lib/boxpilot/boxpilot` symlink. |
| `debian/postinst` | dbus / systemd / polkit reload, icon + desktop cache refresh. |
| `debian/prerm` | No-op aside from the `#DEBHELPER#` block — lifecycle decisions in postrm. |
| `debian/postrm` | On `purge`: remove `/etc/boxpilot/`, `/var/lib/boxpilot/`, `/var/cache/boxpilot/`, runtime-generated `boxpilot-sing-box.service`. On `remove`: refresh caches only. |
| `debian/README.Debian` | Packaging-side README (where the helper logs go, how to inspect activation, etc.). |
| `.gitignore` updates | Ignore `debian/.debhelper/`, `debian/boxpilot/`, `debian/boxpilot.substvars`, `debian/files`, `debian/debhelper-build-stamp`, `debian/cargo-home/`, `*.buildinfo`, `*.changes`, `../boxpilot_*.deb`, `../boxpilot_*.tar.*`, `../boxpilot_*.dsc`. |

### New files (assets shipped by the package)

| Path | Responsibility |
|------|----------------|
| `packaging/linux/desktop/app.boxpilot.desktop` | Desktop entry. |
| `packaging/linux/icons/hicolor/32x32/apps/boxpilot.png` | 32 px icon (copy of existing master). |
| `packaging/linux/icons/hicolor/128x128/apps/boxpilot.png` | 128 px upscale. |
| `packaging/linux/icons/hicolor/256x256/apps/boxpilot.png` | 256 px upscale. |
| `packaging/linux/systemd/boxpilotd.service` | D-Bus systemd activation unit for the helper. |

### Modified files

| Path | Change |
|------|--------|
| `packaging/linux/dbus/system-services/app.boxpilot.Helper.service` | Add `SystemdService=boxpilotd.service`; remove the "Plan #9 will reintroduce" TODO comment block. |
| `Makefile` | Add `deb` target. Extend `install-helper` to also install the desktop entry, icons, `boxpilotd.service`, and the GUI binary so dev installs match the package. Extend `uninstall-helper` symmetrically. Update the leading comment. |
| `README.md` | Replace dev quick start with `apt install ./*.deb`. Move existing `make install-helper` flow under "Building from source / dev". Mark plan #9 complete in the Status section. |

---

## Task 1: Add `boxpilotd.service` and wire `SystemdService=`

**Files:**
- Create: `packaging/linux/systemd/boxpilotd.service`
- Modify: `packaging/linux/dbus/system-services/app.boxpilot.Helper.service`

- [ ] **Step 1: Create `boxpilotd.service`**

```ini
[Unit]
Description=BoxPilot privileged helper (D-Bus activated)
Documentation=https://github.com/BeyondCortex/BoxPilot
After=dbus.service
Requires=dbus.service

[Service]
Type=dbus
BusName=app.boxpilot.Helper
ExecStart=/usr/lib/boxpilot/boxpilotd
User=root
Restart=on-failure
RestartSec=2s
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
ReadWritePaths=/etc/boxpilot /var/lib/boxpilot /var/cache/boxpilot /etc/systemd/system /run/systemd /run/dbus
CapabilityBoundingSet=CAP_SYS_ADMIN CAP_NET_ADMIN CAP_DAC_OVERRIDE CAP_CHOWN CAP_FOWNER

[Install]
# No WantedBy — D-Bus activates this on demand.
```

- [ ] **Step 2: Update D-Bus service file**

Replace the existing `Plan #9 will reintroduce ...` comment block with a
single short comment, and append `SystemdService=boxpilotd.service`:

```ini
# D-Bus system-bus activation file for the BoxPilot privileged helper.
# Activation is delegated to systemd via SystemdService=, with Exec= as
# a fallback when systemd is not the dbus init path.
[D-BUS Service]
Name=app.boxpilot.Helper
Exec=/usr/lib/boxpilot/boxpilotd
User=root
SystemdService=boxpilotd.service
```

- [ ] **Step 3: Verify both files parse**

```bash
# Lint the service file: it is INI-ish; just check there is no obvious
# typo by sourcing the [Section]-like values.
awk '/^\[/ {section=$0} /=/ {print section, $0}' \
    packaging/linux/systemd/boxpilotd.service
```

Expected: every line shows `[Unit]`, `[Service]`, or `[Install]` with the
key=value pair.

---

## Task 2: Generate hicolor icon set

**Files:**
- Create: `packaging/linux/icons/hicolor/32x32/apps/boxpilot.png`
- Create: `packaging/linux/icons/hicolor/128x128/apps/boxpilot.png`
- Create: `packaging/linux/icons/hicolor/256x256/apps/boxpilot.png`

- [ ] **Step 1: Copy the 32 px master**

```bash
mkdir -p packaging/linux/icons/hicolor/32x32/apps
cp crates/boxpilot-tauri/icons/icon.png \
    packaging/linux/icons/hicolor/32x32/apps/boxpilot.png
```

- [ ] **Step 2: Generate 128 / 256 px upscales**

Try ImageMagick first; fall back to a pure-Cargo upscaler if not present.
The build host for the PR has neither — the agent runs the most portable
path:

```bash
mkdir -p packaging/linux/icons/hicolor/128x128/apps
mkdir -p packaging/linux/icons/hicolor/256x256/apps

if command -v convert >/dev/null 2>&1; then
    convert crates/boxpilot-tauri/icons/icon.png -resize 128x128 \
        packaging/linux/icons/hicolor/128x128/apps/boxpilot.png
    convert crates/boxpilot-tauri/icons/icon.png -resize 256x256 \
        packaging/linux/icons/hicolor/256x256/apps/boxpilot.png
else
    # Fall back: ship the same 32 px image at every advertised size.
    # GTK will cope by treating it as the "best available" for each
    # bucket; the visual hit is acceptable for v0.1.0 and the master
    # image is a follow-up.
    cp crates/boxpilot-tauri/icons/icon.png \
        packaging/linux/icons/hicolor/128x128/apps/boxpilot.png
    cp crates/boxpilot-tauri/icons/icon.png \
        packaging/linux/icons/hicolor/256x256/apps/boxpilot.png
fi
```

- [ ] **Step 3: Verify**

```bash
file packaging/linux/icons/hicolor/*/apps/boxpilot.png
```

Expected: every line reports `PNG image data`.

---

## Task 3: Add desktop entry

**Files:**
- Create: `packaging/linux/desktop/app.boxpilot.desktop`

- [ ] **Step 1: Write the file**

```ini
[Desktop Entry]
Type=Application
Name=BoxPilot
GenericName=Sing-box Control Panel
Comment=Manage system sing-box from the desktop
Exec=boxpilot
Icon=boxpilot
Terminal=false
Categories=Network;
Keywords=sing-box;proxy;vpn;tun;
StartupWMClass=BoxPilot
StartupNotify=true
```

- [ ] **Step 2: Validate**

```bash
desktop-file-validate packaging/linux/desktop/app.boxpilot.desktop
```

Expected: no output (validator silent on success). If
`desktop-file-validate` is not installed, skip — `lintian` during the
package build will catch any issues.

---

## Task 4: Author `debian/` tree (control / changelog / copyright / source)

**Files:**
- Create: `debian/changelog`, `debian/control`, `debian/copyright`,
  `debian/source/format`.

- [ ] **Step 1: `debian/changelog`**

```
boxpilot (0.1.0-1) unstable; urgency=medium

  * Initial Debian package.
  * Plans #1–#8 ship as a single binary at v0.1.0:
    - skeleton + helperd, managed core, managed service, profile store,
      activation pipeline, legacy sing-box.service handling, GUI shell,
      diagnostics export with schema-aware redaction.
  * Plan #9: install / upgrade / remove / purge wired through dpkg
    maintainer scripts; D-Bus helper now activated via systemd.

 -- Conn Johnson <johnson.connor.97815@gmail.com>  Thu, 30 Apr 2026 23:00:00 +0000
```

- [ ] **Step 2: `debian/source/format`**

```
3.0 (native)
```

- [ ] **Step 3: `debian/control`**

```
Source: boxpilot
Section: net
Priority: optional
Maintainer: Conn Johnson <johnson.connor.97815@gmail.com>
Build-Depends:
 debhelper-compat (= 13),
 cargo (>= 1.78) | rustc (>= 1.78),
 rustc (>= 1.78),
 nodejs (>= 20),
 npm,
 pkg-config,
 libssl-dev,
 libgtk-3-dev,
 libwebkit2gtk-4.1-dev,
 libayatana-appindicator3-dev,
 librsvg2-dev,
 libsoup-3.0-dev
Standards-Version: 4.6.2
Homepage: https://github.com/BeyondCortex/BoxPilot
Vcs-Git: https://github.com/BeyondCortex/BoxPilot.git
Vcs-Browser: https://github.com/BeyondCortex/BoxPilot
Rules-Requires-Root: binary-targets

Package: boxpilot
Architecture: any
Depends:
 ${shlibs:Depends},
 ${misc:Depends},
 dbus,
 libpolkit-gobject-1-0,
 policykit-1 | polkitd,
 systemd,
 libwebkit2gtk-4.1-0,
 libgtk-3-0,
 libayatana-appindicator3-1
Description: Linux desktop control panel for system sing-box
 BoxPilot is a Tauri-based desktop application for managing the
 system-installed sing-box service. It imports, validates, activates and
 rolls back sing-box-native JSON profiles, and wraps every privileged
 operation behind a polkit-guarded D-Bus helper running as root.
 .
 BoxPilot does not embed sing-box; it manages either a system-provided
 binary or a release fetched from upstream and installed under
 /var/lib/boxpilot/cores.
```

Rationale on the `cargo | rustc` alternation: Debian ships rust through
the `cargo` package, which depends on `rustc`. Listing both is belt and
suspenders for derivative distros that split the toolchain differently.

- [ ] **Step 4: `debian/copyright`**

```
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: boxpilot
Upstream-Contact: Conn Johnson <johnson.connor.97815@gmail.com>
Source: https://github.com/BeyondCortex/BoxPilot

Files: *
Copyright: 2026 Conn Johnson
License: GPL-3.0-or-later

License: GPL-3.0-or-later
 This program is free software: you can redistribute it and/or modify
 it under the terms of the GNU General Public License as published by
 the Free Software Foundation, either version 3 of the License, or
 (at your option) any later version.
 .
 This program is distributed in the hope that it will be useful,
 but WITHOUT ANY WARRANTY; without even the implied warranty of
 MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 GNU General Public License for more details.
 .
 On Debian systems, the complete text of the GNU General Public License
 version 3 can be found in /usr/share/common-licenses/GPL-3.
```

---

## Task 5: Author `debian/rules`, `.install`, `.dirs`, `.links`

**Files:**
- Create: `debian/rules` (executable), `debian/boxpilot.install`,
  `debian/boxpilot.dirs`, `debian/boxpilot.links`.

- [ ] **Step 1: `debian/rules`**

```makefile
#!/usr/bin/make -f
export DH_VERBOSE = 1
export CARGO_HOME = $(CURDIR)/debian/cargo-home
export CARGO_TARGET_DIR = $(CURDIR)/target

%:
	dh $@

override_dh_auto_clean:
	cargo clean || true
	rm -rf debian/cargo-home frontend/dist frontend/node_modules

override_dh_auto_build:
	cargo build --release --locked -p boxpilotd
	cd frontend && npm install --no-audit --no-fund && npm run build
	cd crates/boxpilot-tauri && cargo build --release --locked

override_dh_auto_test:
	# Tests run pre-PR via cargo test --workspace; not at package build.

override_dh_auto_install:
	install -D -m 0755 target/release/boxpilotd \
	    debian/boxpilot/usr/lib/boxpilot/boxpilotd
	install -D -m 0755 target/release/boxpilot \
	    debian/boxpilot/usr/lib/boxpilot/boxpilot
```

Set executable: `chmod +x debian/rules`.

- [ ] **Step 2: `debian/boxpilot.install`**

```
packaging/linux/dbus/system-services/app.boxpilot.Helper.service usr/share/dbus-1/system-services/
packaging/linux/dbus/system.d/app.boxpilot.helper.conf            usr/share/dbus-1/system.d/
packaging/linux/polkit-1/actions/app.boxpilot.helper.policy       usr/share/polkit-1/actions/
packaging/linux/polkit-1/rules.d/49-boxpilot.rules                usr/share/polkit-1/rules.d/
packaging/linux/systemd/boxpilotd.service                         usr/lib/systemd/system/
packaging/linux/desktop/app.boxpilot.desktop                      usr/share/applications/
packaging/linux/icons/hicolor/32x32/apps/boxpilot.png             usr/share/icons/hicolor/32x32/apps/
packaging/linux/icons/hicolor/128x128/apps/boxpilot.png           usr/share/icons/hicolor/128x128/apps/
packaging/linux/icons/hicolor/256x256/apps/boxpilot.png           usr/share/icons/hicolor/256x256/apps/
README.md                                                          usr/share/doc/boxpilot/
```

- [ ] **Step 3: `debian/boxpilot.dirs`**

```
etc/boxpilot
var/lib/boxpilot
var/cache/boxpilot
```

- [ ] **Step 4: `debian/boxpilot.links`**

```
usr/lib/boxpilot/boxpilot usr/bin/boxpilot
```

---

## Task 6: Author maintainer scripts

**Files:**
- Create: `debian/postinst`, `debian/prerm`, `debian/postrm`,
  `debian/README.Debian` (each executable where applicable).

- [ ] **Step 1: `debian/postinst`**

```sh
#!/bin/sh
set -e

case "$1" in
    configure)
        if [ -d /run/systemd/system ]; then
            systemctl daemon-reload || true
            systemctl reload dbus.service 2>/dev/null \
                || systemctl restart dbus.service 2>/dev/null \
                || true
            if systemctl is-enabled polkit.service >/dev/null 2>&1; then
                systemctl reload polkit.service 2>/dev/null \
                    || systemctl restart polkit.service 2>/dev/null \
                    || true
            fi
        fi

        if command -v gtk-update-icon-cache >/dev/null 2>&1; then
            gtk-update-icon-cache -f -t /usr/share/icons/hicolor || true
        fi
        if command -v update-desktop-database >/dev/null 2>&1; then
            update-desktop-database /usr/share/applications || true
        fi
        ;;
esac

#DEBHELPER#

exit 0
```

- [ ] **Step 2: `debian/prerm`**

```sh
#!/bin/sh
set -e

# Lifecycle is handled in postrm. boxpilot-sing-box.service (if
# present) is intentionally left running across upgrade/remove and
# only cleaned on purge, per spec §11 of the plan #9 design.

#DEBHELPER#

exit 0
```

- [ ] **Step 3: `debian/postrm`**

```sh
#!/bin/sh
set -e

case "$1" in
    purge)
        rm -f  /etc/systemd/system/boxpilot-sing-box.service
        rm -rf /etc/boxpilot
        rm -rf /var/lib/boxpilot
        rm -rf /var/cache/boxpilot
        if [ -d /run/systemd/system ]; then
            systemctl daemon-reload || true
        fi
        ;;
    remove)
        if command -v gtk-update-icon-cache >/dev/null 2>&1; then
            gtk-update-icon-cache -f -t /usr/share/icons/hicolor || true
        fi
        if command -v update-desktop-database >/dev/null 2>&1; then
            update-desktop-database /usr/share/applications || true
        fi
        if [ -d /run/systemd/system ]; then
            systemctl daemon-reload || true
            systemctl reload dbus.service 2>/dev/null \
                || systemctl restart dbus.service 2>/dev/null \
                || true
        fi
        ;;
esac

#DEBHELPER#

exit 0
```

- [ ] **Step 4: `debian/README.Debian`**

```
boxpilot for Debian
-------------------

Helper logs:
    journalctl -u boxpilotd.service

Managed sing-box logs (when a profile is active):
    journalctl -u boxpilot-sing-box.service

Runtime trees (cleared on `apt purge`, preserved on `apt remove`):
    /etc/boxpilot/        — controller name, active profile symlink
    /var/lib/boxpilot/    — managed core releases
    /var/cache/boxpilot/  — diagnostics bundles

The package never modifies an external /etc/systemd/system/sing-box.service.
That unit is observed only; see plan #6 for migration semantics.

 -- Conn Johnson <johnson.connor.97815@gmail.com>
```

- [ ] **Step 5: Make scripts executable**

```bash
chmod +x debian/rules debian/postinst debian/prerm debian/postrm
```

---

## Task 7: Update `Makefile`

**Files:**
- Modify: `Makefile`.

- [ ] **Step 1: Replace contents**

Keep `install-helper` as the dev-mode flow but extend it to install the
desktop entry, icons, `boxpilotd.service`, and the GUI binary, so dev
installs match what the `.deb` produces. Add a `deb` target.

```makefile
# Dev install flow. The release path is `make deb` (see below).

PREFIX        ?= /usr
DBUS_SYS_DIR  ?= $(PREFIX)/share/dbus-1
POLKIT_DIR    ?= $(PREFIX)/share/polkit-1
LIB_DIR       ?= $(PREFIX)/lib/boxpilot
BIN_DIR       ?= $(PREFIX)/bin
SYSTEMD_DIR   ?= $(PREFIX)/lib/systemd/system
APP_DIR       ?= $(PREFIX)/share/applications
ICON_DIR      ?= $(PREFIX)/share/icons/hicolor
ETC_DIR       ?= /etc/boxpilot

CARGO         ?= cargo
INSTALL       ?= install
NPM           ?= npm

.PHONY: build-helper build-gui build install-helper uninstall-helper run-gui deb

build-helper:
	$(CARGO) build --release -p boxpilotd

build-gui:
	cd frontend && $(NPM) install --no-audit --no-fund && $(NPM) run build
	$(CARGO) build --release -p boxpilot

build: build-helper build-gui

install-helper: build
	$(INSTALL) -d -m 0755 $(LIB_DIR)
	$(INSTALL) -D -m 0755 target/release/boxpilotd $(LIB_DIR)/boxpilotd
	$(INSTALL) -D -m 0755 target/release/boxpilot  $(LIB_DIR)/boxpilot
	ln -sf $(LIB_DIR)/boxpilot $(BIN_DIR)/boxpilot
	$(INSTALL) -D -m 0644 packaging/linux/dbus/system-services/app.boxpilot.Helper.service \
	    $(DBUS_SYS_DIR)/system-services/app.boxpilot.Helper.service
	$(INSTALL) -D -m 0644 packaging/linux/dbus/system.d/app.boxpilot.helper.conf \
	    $(DBUS_SYS_DIR)/system.d/app.boxpilot.helper.conf
	$(INSTALL) -D -m 0644 packaging/linux/polkit-1/actions/app.boxpilot.helper.policy \
	    $(POLKIT_DIR)/actions/app.boxpilot.helper.policy
	$(INSTALL) -D -m 0644 packaging/linux/polkit-1/rules.d/49-boxpilot.rules \
	    $(POLKIT_DIR)/rules.d/49-boxpilot.rules
	$(INSTALL) -D -m 0644 packaging/linux/systemd/boxpilotd.service \
	    $(SYSTEMD_DIR)/boxpilotd.service
	$(INSTALL) -D -m 0644 packaging/linux/desktop/app.boxpilot.desktop \
	    $(APP_DIR)/app.boxpilot.desktop
	$(INSTALL) -D -m 0644 packaging/linux/icons/hicolor/32x32/apps/boxpilot.png \
	    $(ICON_DIR)/32x32/apps/boxpilot.png
	$(INSTALL) -D -m 0644 packaging/linux/icons/hicolor/128x128/apps/boxpilot.png \
	    $(ICON_DIR)/128x128/apps/boxpilot.png
	$(INSTALL) -D -m 0644 packaging/linux/icons/hicolor/256x256/apps/boxpilot.png \
	    $(ICON_DIR)/256x256/apps/boxpilot.png
	$(INSTALL) -d -m 0755 $(ETC_DIR)
	systemctl daemon-reload || true
	systemctl reload dbus.service || systemctl restart dbus.service
	command -v update-desktop-database >/dev/null && update-desktop-database $(APP_DIR) || true
	command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -f -t $(ICON_DIR) || true

uninstall-helper:
	rm -f $(LIB_DIR)/boxpilotd
	rm -f $(LIB_DIR)/boxpilot
	rm -f $(BIN_DIR)/boxpilot
	rm -f $(DBUS_SYS_DIR)/system-services/app.boxpilot.Helper.service
	rm -f $(DBUS_SYS_DIR)/system.d/app.boxpilot.helper.conf
	rm -f $(POLKIT_DIR)/actions/app.boxpilot.helper.policy
	rm -f $(POLKIT_DIR)/rules.d/49-boxpilot.rules
	rm -f $(SYSTEMD_DIR)/boxpilotd.service
	rm -f $(APP_DIR)/app.boxpilot.desktop
	rm -f $(ICON_DIR)/32x32/apps/boxpilot.png
	rm -f $(ICON_DIR)/128x128/apps/boxpilot.png
	rm -f $(ICON_DIR)/256x256/apps/boxpilot.png
	systemctl daemon-reload || true
	systemctl reload dbus.service || systemctl restart dbus.service
	command -v update-desktop-database >/dev/null && update-desktop-database $(APP_DIR) || true
	command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -f -t $(ICON_DIR) || true

run-gui:
	cd crates/boxpilot-tauri && cargo tauri dev

deb:
	dpkg-buildpackage -b -uc -us
```

---

## Task 8: Update README

**Files:**
- Modify: `README.md`.

- [ ] **Step 1: Replace contents**

```markdown
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

## Install (Debian / Ubuntu)

Download the latest `.deb` from the GitHub Releases page, then:

```bash
sudo apt install ./boxpilot_0.1.0_amd64.deb
```

After install, launch **BoxPilot** from the desktop application menu, or
run `boxpilot` from a terminal.

If you previously ran the dev `make install-helper`, run `sudo make
uninstall-helper` first so `dpkg` does not flag file conflicts.

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
make install-helper      # build + install to system paths
make run-gui             # cargo tauri dev with hot reload
```

`make uninstall-helper` reverses the dev install.

## License

GPL-3.0-or-later.
```

---

## Task 9: Update `.gitignore`

**Files:**
- Modify: `.gitignore`.

- [ ] **Step 1: Append packaging artefacts**

If the file does not yet exist, create it; otherwise append:

```
# Debian packaging build artefacts (debhelper temp dirs + outputs)
debian/.debhelper/
debian/boxpilot/
debian/boxpilot.substvars
debian/boxpilot.debhelper.log
debian/cargo-home/
debian/files
debian/debhelper-build-stamp

# dpkg-buildpackage output goes to the parent dir; ignore from repo root
../boxpilot_*.deb
../boxpilot_*.tar.*
../boxpilot_*.dsc
../boxpilot_*.changes
../boxpilot_*.buildinfo
*.buildinfo
*.changes
```

---

## Task 10: Smoke-test build artefacts

**Files:** none modified.

- [ ] **Step 1: Workspace gates**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

All must pass.

- [ ] **Step 2: Frontend build**

```bash
cd frontend && npm install --no-audit --no-fund && npm run build
```

Must produce `frontend/dist/`.

- [ ] **Step 3: Validate packaging files (no full deb build required)**

```bash
xmllint --noout packaging/linux/polkit-1/actions/app.boxpilot.helper.policy
xmllint --noout packaging/linux/dbus/system.d/app.boxpilot.helper.conf
desktop-file-validate packaging/linux/desktop/app.boxpilot.desktop || true
file packaging/linux/icons/hicolor/*/apps/boxpilot.png
ls -la debian/
```

- [ ] **Step 4: Optional: full `dpkg-buildpackage`**

This step requires `debhelper` and the GUI build-deps to be installed.
On the build host:

```bash
sudo apt install -y debhelper devscripts \
    pkg-config libssl-dev libgtk-3-dev libwebkit2gtk-4.1-dev \
    libayatana-appindicator3-dev librsvg2-dev libsoup-3.0-dev
make deb
ls -la ../boxpilot_*.deb
dpkg-deb -c ../boxpilot_*.deb | head -50
```

The PR records whether step 4 was exercised on the build host or
deferred to release-build CI.

---

## Task 11: Commit + open PR

**Files:** none modified beyond what previous tasks produced.

- [ ] **Step 1: Stage everything**

```bash
git add debian/ packaging/linux/ Makefile README.md .gitignore \
    docs/superpowers/specs/2026-04-30-deb-packaging-design.md \
    docs/superpowers/plans/2026-04-30-deb-packaging.md \
    docs/superpowers/plans/2026-04-30-deb-packaging-smoke-procedure.md
```

- [ ] **Step 2: Single commit**

```
feat: .deb packaging + GUI desktop entry (plan #9)

- Debian source tree (debian/) with debhelper-13 dh sequencer overrides
  for cargo + npm.
- New boxpilotd.service for D-Bus systemd activation; D-Bus service file
  gains SystemdService=.
- app.boxpilot.desktop + hicolor 32/128/256 icon set.
- postinst / prerm / postrm: idempotent reloads of dbus / systemd /
  polkit / icon + desktop caches; purge-only cleanup of /etc/boxpilot,
  /var/lib/boxpilot, /var/cache/boxpilot, and runtime-generated
  boxpilot-sing-box.service.
- Makefile: install-helper now installs the full app (GUI + helper +
  desktop + icons + boxpilotd.service); new `make deb` target.
- README rewritten around `apt install ./boxpilot_*.deb`.
```

- [ ] **Step 3: Push and open the PR**

PR title: `feat: .deb packaging + GUI desktop entry (plan #9)`

PR body:

```
## Summary
- Ship BoxPilot as a Debian package; `apt install ./boxpilot_*.deb` is the new install path.
- D-Bus helper now activated via systemd through a shipped `boxpilotd.service`.
- Adds the desktop entry + hicolor icons so the GUI shows up in the application menu.

## Test plan
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `cd frontend && npm install && npm run build`
- [ ] `make install-helper` on a dev host, GUI launches from menu, `service.status` round-trip works
- [ ] (Optional, on build host with debhelper) `make deb` produces `../boxpilot_0.1.0_*.deb`
- [ ] (Optional, fresh VM) `apt install ./*.deb` → menu entry → GUI works → `apt purge` cleans `/etc/boxpilot`, `/var/lib/boxpilot`, `/var/cache/boxpilot`
```
