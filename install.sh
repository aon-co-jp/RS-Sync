#!/usr/bin/env bash
# RS-Sync installer for Linux and macOS.
# Honest scope: builds from source with cargo (no prebuilt binary signing/
# packaging pipeline exists yet), installs to ~/.local/rs-sync, and offers to
# set up an auto-start service (systemd --user on Linux, launchd on macOS).
# Requires: git, curl, a Rust toolchain (installed automatically via rustup
# if missing).
set -euo pipefail

INSTALL_DIR="${RS_SYNC_HOME:-$HOME/.local/rs-sync}"
REPO_URL="https://github.com/aon-co-jp/RS-Sync.git"

OS_NAME="$(uname -s)"
case "$OS_NAME" in
  Linux) PLATFORM="linux" ;;
  Darwin) PLATFORM="macos" ;;
  *)
    echo "Unsupported OS: $OS_NAME (this script supports Linux and macOS only; use install.ps1 on Windows)"
    exit 1
    ;;
esac

echo "== RS-Sync installer ($PLATFORM) =="

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

if [ "$PLATFORM" = "linux" ]; then
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
elif [ "$PLATFORM" = "macos" ]; then
  # macOSにはsystemdが無いため、同等の役割をlaunchd(ユーザーagent、
  # ~/Library/LaunchAgents配下のplist)で代替する。ログイン時自動起動+
  # 異常終了時の自動再起動(KeepAlive)がsystemdの`enable --now`+
  # `Restart=on-failure`とおおよそ対応する。
  read -r -p "Install as a launchd user agent (auto-start on login)? [y/N] " ans
  if [[ "$ans" =~ ^[Yy]$ ]]; then
    mkdir -p "$HOME/Library/LaunchAgents"
    mkdir -p "$HOME/Library/Logs/rs-sync"
    PLIST="$HOME/Library/LaunchAgents/tokyo.aon.rs-sync.plist"
    cat > "$PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>tokyo.aon.rs-sync</string>
  <key>ProgramArguments</key>
  <array>
    <string>$INSTALL_DIR/target/release/rs-sync</string>
  </array>
  <key>WorkingDirectory</key>
  <string>$INSTALL_DIR</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>RS_SYNC_PORT</key>
    <string>8095</string>
    <key>RS_SYNC_DATA</key>
    <string>$INSTALL_DIR/data/store.json</string>
  </dict>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <dict>
    <key>SuccessfulExit</key>
    <false/>
  </dict>
  <key>StandardOutPath</key>
  <string>$HOME/Library/Logs/rs-sync/stdout.log</string>
  <key>StandardErrorPath</key>
  <string>$HOME/Library/Logs/rs-sync/stderr.log</string>
</dict>
</plist>
EOF
    launchctl unload "$PLIST" 2>/dev/null || true
    launchctl load -w "$PLIST"
    echo "Started tokyo.aon.rs-sync (launchctl list | grep rs-sync to check status)"
    echo "Logs: $HOME/Library/Logs/rs-sync/{stdout,stderr}.log"
    echo "To uninstall: launchctl unload $PLIST && rm $PLIST"
  fi
fi

echo "Done. Open http://127.0.0.1:8095/ once running."
