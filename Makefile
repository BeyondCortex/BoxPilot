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
	$(INSTALL) -d -m 0755 $(BIN_DIR)
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
