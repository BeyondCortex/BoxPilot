# Dev-install BoxPilot's privileged side onto the local machine. Plan #9
# replaces this with proper .deb postinst/prerm scripts.

PREFIX        ?= /usr
DBUS_SYS_DIR  ?= $(PREFIX)/share/dbus-1
POLKIT_DIR    ?= $(PREFIX)/share/polkit-1
LIB_DIR       ?= $(PREFIX)/lib/boxpilot
BIN_DIR       ?= $(PREFIX)/bin
ETC_DIR       ?= /etc/boxpilot

CARGO         ?= cargo
INSTALL       ?= install

.PHONY: build-helper install-helper uninstall-helper run-gui

build-helper:
	$(CARGO) build --release -p boxpilotd

install-helper: build-helper
	$(INSTALL) -d -m 0755 $(LIB_DIR)
	$(INSTALL) -D -m 0755 target/release/boxpilotd $(LIB_DIR)/boxpilotd
	$(INSTALL) -D -m 0644 packaging/linux/dbus/system-services/app.boxpilot.Helper.service \
	    $(DBUS_SYS_DIR)/system-services/app.boxpilot.Helper.service
	$(INSTALL) -D -m 0644 packaging/linux/dbus/system.d/app.boxpilot.helper.conf \
	    $(DBUS_SYS_DIR)/system.d/app.boxpilot.helper.conf
	$(INSTALL) -D -m 0644 packaging/linux/polkit-1/actions/app.boxpilot.helper.policy \
	    $(POLKIT_DIR)/actions/app.boxpilot.helper.policy
	$(INSTALL) -D -m 0644 packaging/linux/polkit-1/rules.d/49-boxpilot.rules \
	    $(POLKIT_DIR)/rules.d/49-boxpilot.rules
	$(INSTALL) -d -m 0755 $(ETC_DIR)
	# /etc/boxpilot/controller-name is left absent on dev installs; the polkit
	# JS rule treats absence as "no controller, fall through to defaults".
	systemctl reload dbus.service || systemctl restart dbus.service

uninstall-helper:
	rm -f $(LIB_DIR)/boxpilotd
	rm -f $(DBUS_SYS_DIR)/system-services/app.boxpilot.Helper.service
	rm -f $(DBUS_SYS_DIR)/system.d/app.boxpilot.helper.conf
	rm -f $(POLKIT_DIR)/actions/app.boxpilot.helper.policy
	rm -f $(POLKIT_DIR)/rules.d/49-boxpilot.rules
	systemctl reload dbus.service || systemctl restart dbus.service

run-gui:
	cd crates/boxpilot-tauri && cargo tauri dev
