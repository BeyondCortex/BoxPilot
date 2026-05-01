# Plan #7 — GUI Shell Smoke Procedure

**Goal:** Validate every flow plan #7 ships against a real system that has
plans #1–#6 deployed.

## Prerequisites

1. `boxpilotd` installed via `make install-helper`.
2. `cargo tauri dev` run from `crates/boxpilot-tauri` (use `make run-gui`
   from the workspace root).
3. A system user with `sudo` available (polkit will prompt for admin auth on
   mutating actions).

## 1. Home page render

- Open BoxPilot. The "Home" tab loads automatically.
- Expect three cards: Service / Active profile / Core. The poll runs every
  5 s; values update without manual refresh.
- Click "Refresh" in Quick actions. Toast says "ok".
- Hide the window for >5 s (tab the desktop). When you bring it back, the
  Home page refreshes immediately on visibility change.

## 2. Service controls

- In Quick actions click Stop → Start → Restart. Each shows a toast and
  the service card updates.
- Open Logs panel → "Load tail (200)". Expect 200 lines or fewer.

## 3. Profile import + activate

- Profiles tab → enter a name + path to a known-good `config.json` →
  "File" button. Expect import success.
- Select the new profile. Overview tab shows inbounds/outbounds counts.
- Editor tab shows the source. "Save" is disabled because clean.
- Activate tab → "Best-effort check" → green OK → "Prepare bundle preview"
  → manifest renders → "Activate" → confirm modal. Outcome:
  - Happy path: green "active" panel, Home card updates within 5 s.
  - Bad config: yellow "rolled_back" panel.

## 4. Manual rollback

- Settings → Service → Manual rollback.
- Paste a previous activation_id (from the activate panel's "previous"
  field). Click "Roll back". Expect outcome panel.

## 5. Clash API toggle

- Profiles → select profile → Editor tab.
- Click "Enable Clash API on loopback". Editor reloads with
  `experimental.clash_api.external_controller = "127.0.0.1:9090"`.
- Button is now disabled because the field is set.
- Re-activate the profile to apply.

## 6. Legacy detection + migrate

- (Only on a system with a legacy `sing-box.service`.)
- Settings → Legacy → Scan. Card shows fragment_path / config_path / kind.
- For `system_path`: enter a profile name → "Import as profile (file)" or
  "(dir + assets)". Profile appears in Profiles tab.
- Activate the imported profile.
- Back to Settings → Legacy → "Stop + disable legacy unit". Confirm. Toast
  says ok; cutover button disables.

## 7. Drift / repair banners

- Force `controller_orphaned` (delete the user that owns the controller, or
  point `controller_uid` at a missing UID via root edit of
  `/etc/boxpilot/boxpilot.toml`). Reload Home — yellow banner.
- Force `active_corrupt` (as root, point `/etc/boxpilot/active` at a
  non-release path). Reload Home — red banner with "Open Profiles" action.

## 8. Build + lint

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
( cd frontend && npm ci && npm run build )
xmllint --noout packaging/linux/dbus/system.d/app.boxpilot.helper.conf
xmllint --noout packaging/linux/polkit-1/actions/app.boxpilot.helper.policy
```

All must pass.
