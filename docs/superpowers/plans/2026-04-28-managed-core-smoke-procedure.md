# Plan #2 manual smoke procedure

Run after task 26 completes, on a Debian/Ubuntu desktop with a polkit
agent active and network access to github.com.

## Reinstall the helper from this branch

```bash
sudo make install-helper
```

## 1. Discover (no controller yet)

```bash
gdbus call --system --dest app.boxpilot.Helper \
  --object-path /app/boxpilot/Helper \
  --method app.boxpilot.Helper1.CoreDiscover
```

Expected: empty `cores: []` (or only externals if `/usr/bin/sing-box` is
installed). `current: null`.

## 2. Install latest (claims controller)

Run `make run-gui`, navigate to **Settings → Cores**, click
**Install latest sing-box**.

Expected: polkit prompt for admin auth (XML default `auth_admin_keep`).
After auth, panel shows the new managed-installed entry as active.

Verify side effects:

```bash
ls /var/lib/boxpilot/cores/
cat /var/lib/boxpilot/install-state.json
cat /etc/boxpilot/boxpilot.toml
cat /etc/boxpilot/controller-name
```

Expect: `cores/<version>/`, `cores/current` symlink, `controller_uid`
populated, `controller-name` contains your username.

## 3. Discover again

The Cores panel updates; `gdbus call ... CoreDiscover` shows the managed
entry plus current label. The polkit JS rule should now relax for
controller calls — read-only `service.status` should be silent.

## 4. Adopt an external (if `/usr/bin/sing-box` exists)

In the panel, type `/usr/bin/sing-box` into the adopt field, click
**Adopt**. Expect: a new `adopted-<ts>` row, `current` unchanged.

## 5. Rollback

In the panel, click **Make active** next to a non-current managed
version. Expect: `current` symlink swings; panel updates.

## 6. Negative test: bad path

Try adopting `/home/$USER/sing-box` (touch a stub file there first).
Expect: error toast / panel error message naming the trust-check
violation.
