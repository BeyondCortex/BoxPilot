# Plan #8 Smoke Procedure — Diagnostics Export

**Pre-conditions:** `make install-helper` run on the dev VM, GUI launched
via `make run-gui`, at least one profile activated, `boxpilot-sing-box.service`
either running or in a known failed state.

## Manual flow

1. Open the GUI → **Settings** → **About** tab.
2. Verify the "Diagnostics bundles" line shows `/var/cache/boxpilot/diagnostics/`.
3. Click **Export diagnostics**.
4. Wait for the polkit prompt (admin auth, cached after first time).
5. Authenticate.
6. Expect the result line to show
   `Exported: /var/cache/boxpilot/diagnostics/boxpilot-diagnostics-<ts>.tar.gz (<size> MiB)`.
7. Open a terminal:
   ```bash
   sudo ls -la /var/cache/boxpilot/diagnostics/
   ```
   Expected: tarball exists, mode `0600`, owner `root:root`.

## Redaction sanity check

Use a profile whose `outbounds[0].password` is the canary string
`SMOKE_TEST_PASSWORD`:

```bash
sudo cp <bundle path> /tmp/diag.tar.gz
sudo chown $USER /tmp/diag.tar.gz
cd /tmp && tar -xzf diag.tar.gz
grep -r SMOKE_TEST_PASSWORD boxpilot-diagnostics-*/
```

Expected: zero matches.

```bash
grep -r '"password":' boxpilot-diagnostics-*/active-config.json
```

Expected: one match with value `"***"`.

## LRU cap check

Generate three bundles in succession (the controller's polkit cookie keeps
the second + third from prompting):

```bash
for i in 1 2 3; do
  busctl call --system app.boxpilot.Helper /app/boxpilot/Helper \
    app.boxpilot.Helper1 DiagnosticsExportRedacted
  sleep 1
done
```

Each call should return a `"path"` distinct from the previous one. The
GC trigger only fires when total size exceeds 100 MiB; the unit test in
`diagnostics::gc::tests` covers eviction logic deterministically.

## Failure-mode check

Symlink `active` to a non-existent path, then export:

```bash
sudo rm /etc/boxpilot/active
sudo ln -s /etc/boxpilot/releases/nonexistent /etc/boxpilot/active
# trigger export from GUI; result should still succeed but the tarball
# contains active-config.json.unavailable.txt with the read error.
```

After verifying, restore the symlink to a valid release.

## Workspace gates

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd frontend && npm run build && cd ..
xmllint --noout packaging/linux/polkit-1/actions/app.boxpilot.helper.policy
```

All must pass green.
