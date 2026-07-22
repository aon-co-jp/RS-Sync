#!/usr/bin/env bash
# RS-Sync installer for Linux.
# Honest scope: builds from source with cargo (no prebuilt binary signing/
# packaging pipeline exists yet), installs to ~/.local/rs-sync, and offers to
# set up a systemd --user service. Requires: git, curl, a Rust toolchain
# (installed automatically via rustup if missing).
set -euo pipefail

INSTALL_DIR="${RS_SYNC_HOME:-$HOME/.local/rs-sync}"
REPO_URL="https://github.com/aon-co-jp/RS-Sync.git"

echo "== RS-Sync installer (Linux) =="

if ! command -v cargo >/dev/null 2>&1; then
  echo "Rust toolchain not found; installing via rustup..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  # shellcheck disable=SC1090
  source "$HOME/.cargo/env"
fi

if [ ! -d "$INSTALL_DIR" ]; then
  git clone "$REPO_URL" "$INSTALL_DIR"
else
  echo "Existing install found at $INSTALL_DIR, pulling latest..."
  git -C "$INSTALL_DIR" pull --ff-only
fi

cd "$INSTALL_DIR"
cargo build --release

echo
echo "Build complete: $INSTALL_DIR/target/release/rs-sync"
echo "Run manually with:"
echo "  RS_SYNC_PORT=8095 RS_SYNC_DATA=$INSTALL_DIR/data/store.json $INSTALL_DIR/target/release/rs-sync"
echo
read -r -p "Install as a systemd --user service (auto-start on login)? [y/N] " ans
if [[ "$ans" =~ ^[Yy]$ ]]; then
  mkdir -p "$HOME/.config/systemd/user"
  cat > "$HOME/.config/systemd/user/rs-sync.service" <<EOF
[Unit]
Description=RS-Sync git backup/mirror sync tool

[Service]
Environment=RS_SYNC_PORT=8095
Environment=RS_SYNC_DATA=$INSTALL_DIR/data/store.json
WorkingDirectory=$INSTALL_DIR
ExecStart=$INSTALL_DIR/target/release/rs-sync
Restart=on-failure

[Install]
WantedBy=default.target
EOF
  systemctl --user daemon-reload
  systemctl --user enable --now rs-sync.service
  echo "Started rs-sync.service (systemctl --user status rs-sync)"
fi

echo "Done. Open http://127.0.0.1:8095/ once running."
