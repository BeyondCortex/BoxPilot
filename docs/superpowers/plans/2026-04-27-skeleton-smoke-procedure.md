# Plan #1 manual smoke procedure

This isn't an automated test — system-bus + polkit testing requires a real
desktop session. Run this after task 26 completes, on a Debian/Ubuntu system
with a graphical login active.

## 1. Install the privileged side

```bash
cargo build --release -p boxpilotd
sudo make install-helper
```

Verify the files landed:

```bash
ls /usr/lib/boxpilot/boxpilotd
ls /usr/share/dbus-1/system-services/app.boxpilot.Helper.service
ls /usr/share/dbus-1/system.d/app.boxpilot.helper.conf
ls /usr/share/polkit-1/actions/app.boxpilot.helper.policy
ls /usr/share/polkit-1/rules.d/49-boxpilot.rules
```

## 2. Verify the bus picks up the service file

```bash
gdbus introspect --system --dest app.boxpilot.Helper --object-path /app/boxpilot/Helper
```

Expected: D-Bus auto-activates `boxpilotd`, the introspection reply includes
the `app.boxpilot.Helper1` interface with `ServiceStatus`, `ServiceStart`, …
(19 methods total).

## 3. Verify polkit actions are registered

```bash
pkaction --action-id app.boxpilot.helper.service.status
pkaction --action-id app.boxpilot.helper.profile.activate-bundle
pkaction --action-id app.boxpilot.helper.controller.transfer
```

Expected: each command returns the action description, vendor, and defaults
matching the XML.

## 4. Call ServiceStatus directly via D-Bus

```bash
gdbus call --system --dest app.boxpilot.Helper \
  --object-path /app/boxpilot/Helper \
  --method app.boxpilot.Helper1.ServiceStatus
```

Expected (clean machine, no `boxpilot-sing-box.service` installed yet):
returns a JSON string whose decoded payload is

```json
{
  "unit_name": "boxpilot-sing-box.service",
  "unit_state": { "kind": "not_found" },
  "controller": { "kind": "unset" }
}
```

No polkit prompt because `service.status` is `yes/yes/yes`.

## 5. Call a stubbed mutating method

```bash
gdbus call --system --dest app.boxpilot.Helper \
  --object-path /app/boxpilot/Helper \
  --method app.boxpilot.Helper1.ServiceStart
```

Expected: polkit admin prompt appears (because no controller is set so XML
defaults apply). After authenticating, the call still returns
`app.boxpilot.Helper1.ControllerNotSet: no controller has been initialized`
because the controller-set path doesn't exist in plan #1.

## 6. Run the GUI

```bash
make run-gui
```

Expected: a Tauri window opens. Click **Check service.status**. The status
panel populates with the same JSON as in step 4.

## 7. Tear down

```bash
sudo make uninstall-helper
```
