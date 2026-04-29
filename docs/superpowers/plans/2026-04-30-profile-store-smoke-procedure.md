# Plan #4 manual smoke procedure

Run on a Debian/Ubuntu desktop after Task 22 passes. The helper does
**not** need to be reinstalled — plan #4 changes nothing in `boxpilotd`.
A managed core under `/var/lib/boxpilot/cores/current/sing-box` is
required only for step 4 (best-effort check) and step 5 (prepare bundle
preview). If you do not have one, complete plan #2's smoke procedure
through step 2 first.

## 1. Launch GUI on the new branch

```bash
make run-gui
```

Expected: the top nav now has three tabs: **Home**, **Profiles**,
**Settings → Cores**. Click **Profiles** — the panel renders with an
empty list.

## 2. Import a local JSON file

Save a minimal sing-box config to `/tmp/p1.json`:

```json
{"log":{"level":"info"},"inbounds":[],"outbounds":[{"type":"direct","tag":"direct"}]}
```

In the GUI: enter `My Local` as the name, paste `/tmp/p1.json`, click
**Import file**. Expected:
- Profile appears in the list with `local · <8 hex>` metadata.
- `ls -la ~/.local/share/boxpilot/profiles/my-local-*/` shows
  `0700` directory mode and `0600` on `source.json` / `metadata.json`.

## 3. Import a directory profile with one asset

```bash
mkdir -p /tmp/p2/rules
cat > /tmp/p2/config.json <<'EOF'
{"route":{"rule_set":[{"tag":"r","type":"local","format":"binary","path":"geosite.db"}]},
 "outbounds":[{"type":"direct","tag":"d"}]}
EOF
printf 'GEO' > /tmp/p2/geosite.db
```

In the GUI: name `My Dir`, dir `/tmp/p2`, click **Import directory**.
Expected:
- Profile appears with `local-dir` source kind.
- `ls ~/.local/share/boxpilot/profiles/my-dir-*/assets/` lists
  `geosite.db` (`0600`).

## 4. Best-effort check

Click `My Dir`, set **Core path** to your managed core
(`/var/lib/boxpilot/cores/current/sing-box`), click **Best-effort check**.
Expected: `check OK` plus the core's stdout. If you mutate the textarea
to a clearly invalid config (e.g. `{"inbounds": "not an array"}`) and
**Save**, then re-run **Best-effort check**, expect `check FAILED` and
stderr output naming the offending field.

## 5. Prepare bundle (preview)

With `My Dir` still selected, click **Prepare bundle (preview)**.
Expected:
- Status updates to `bundle ready @ /tmp/.tmpXXXXXX`.
- The `manifest.json` JSON renders below, with:
  - `schema_version: 1`
  - `source_kind: "local-dir"`
  - `assets: [{"path": "geosite.db", "size": 3, ...}]`
  - `core_path_at_activation: "/var/lib/boxpilot/cores/current/sing-box"`
- `ls /tmp/.tmpXXXXXX/` (in another terminal, before closing the GUI)
  lists `config.json`, `assets/geosite.db`, `manifest.json`.

## 6. Add a remote profile (URL split test)

Pick any URL serving sing-box JSON. If you do not have a real one, run
a one-shot local server:

```bash
python3 -m http.server 8765 --directory /tmp/p2 &
SERVER=$!
```

In the GUI: name `Sub`, URL `http://localhost:8765/config.json?token=SECRET-TEST`,
click **Add remote**. Expected:
- Profile appears with `remote · <8 hex>`.
- Below the name: `http://localhost:8765/config.json?token=***` (token
  redacted in the panel).
- `cat ~/.local/share/boxpilot/remotes.json | grep token`
  shows the **un-redacted** URL with `SECRET-TEST` (this is correct;
  per §14 the user-side store keeps the full URL with `0600`).
- Click **Prepare bundle (preview)** for `Sub`. The rendered manifest's
  `source_url_redacted` shows `token=***` — confirming the system-side
  manifest never carries the secret.

```bash
kill $SERVER
```

## 7. Editor preserves unknown fields

Click `My Local`, replace the textarea contents with:

```json
{
  "log": {"level": "info", "_unknown_x": 42},
  "inbounds": [],
  "outbounds": [{"type":"direct","tag":"direct","_secret":true}]
}
```

Click **Save**, then click another profile and back. Expected: the
`_unknown_x` and `_secret` fields are still present in the textarea.
`cat ~/.local/share/boxpilot/profiles/my-local-*/source.json` confirms
the bytes on disk match.

## 8. Cleanup

```bash
rm -rf ~/.local/share/boxpilot/profiles ~/.local/share/boxpilot/remotes.json
rm -rf /tmp/p1.json /tmp/p2
```
