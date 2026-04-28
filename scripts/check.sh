#!/usr/bin/env bash
# Local CI gate. Run before every PR.
set -euo pipefail

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
( cd frontend && npm ci && npm run build )
xmllint --noout packaging/linux/dbus/system.d/app.boxpilot.helper.conf
xmllint --noout packaging/linux/polkit-1/actions/app.boxpilot.helper.policy
echo "All checks passed."
