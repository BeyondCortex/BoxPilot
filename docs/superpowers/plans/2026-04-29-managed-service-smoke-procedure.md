# Plan #3 manual smoke procedure

Run on a Debian/Ubuntu desktop after T22 passes, with a polkit
agent active and at least one managed core already installed (run the
plan #2 smoke procedure through step 2 first).

## Reinstall the helper from this branch

```bash
sudo make install-helper
```

## 1. Install the unit (claims controller if not yet claimed)

Run `make run-gui`, navigate to **Home**, click **Install unit**.

Expected:
- polkit prompt for admin auth (XML default `auth_admin_keep`).
- Panel updates to show `unit_state: { kind: "known", load_state: "loaded", … }`.
- `cat /etc/systemd/system/boxpilot-sing-box.service` matches the §7.1 template
  with `ExecStart=/var/lib/boxpilot/cores/current/sing-box run -c config.json`.
- `cat /etc/polkit-1/rules.d/48-boxpilot-controller.rules` shows
  `var BOXPILOT_CONTROLLER = "<your-username>";`.

## 2. Enable + Start (will fail to start until plan #5 adds a profile)

Click **Enable** → expect no prompt (controller cached after Task 1).
Click **Start** → expect the unit to enter `failed` because
`/etc/boxpilot/active/config.json` does not exist yet.

```bash
systemctl status boxpilot-sing-box.service
```

Expected: `Loaded`, but `Active: failed`. This confirms the unit is in
the right place and the sandbox loaded; it's just that activation (plan
#5) hasn't supplied a config yet.

## 3. Tail logs

Click **Tail last 200 lines**.

Expected: ~10–20 lines from journald showing the failed-start attempts
plus systemd's restart messages. No spawn errors.

## 4. Stop + Disable

Click **Stop** → unit goes to `inactive (dead)`.
Click **Disable** → `systemctl is-enabled boxpilot-sing-box.service`
prints `disabled`.

## 5. polkit perf check

```bash
time gdbus call --system --dest app.boxpilot.Helper \
  --object-path /app/boxpilot/Helper \
  --method app.boxpilot.Helper1.ServiceStatus
```

Expected: < 50 ms wall time (was ~150 ms with `polkit.spawn(cat)` per call).

## 6. Negative test: missing core_path

```bash
sudo cp /etc/boxpilot/boxpilot.toml /tmp/boxpilot.toml.bak
sudo sh -c 'sed -i "/^core_path/d; /^core_state/d" /etc/boxpilot/boxpilot.toml'
```

In the GUI click **Install unit** again. Expected: error toast naming
"no core configured — install or adopt a core first". `cat /etc/systemd/system/boxpilot-sing-box.service`
is unchanged (no half-write).

Restore:

```bash
sudo cp /tmp/boxpilot.toml.bak /etc/boxpilot/boxpilot.toml
```

## 7. Cleanup

```bash
sudo systemctl disable --now boxpilot-sing-box.service || true
sudo rm -f /etc/systemd/system/boxpilot-sing-box.service
sudo systemctl daemon-reload
```
