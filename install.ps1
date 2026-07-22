# RS-Sync installer for Windows.
# Honest scope: builds from source with cargo (no code-signed prebuilt binary
# yet), installs to $env:LOCALAPPDATA\rs-sync, and creates a Start-menu-free
# desktop shortcut plus a convenience launch script. Requires git and a Rust
# toolchain (installs rustup automatically if missing, via winget if
# available, otherwise prompts the user to install manually).

$ErrorActionPreference = "Stop"

$InstallDir = if ($env:RS_SYNC_HOME) { $env:RS_SYNC_HOME } else { Join-Path $env:LOCALAPPDATA "rs-sync" }
$RepoUrl = "https://github.com/aon-co-jp/RS-Sync.git"

Write-Host "== RS-Sync installer (Windows) =="

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host "Rust toolchain not found."
    if (Get-Command winget -ErrorAction SilentlyContinue) {
        Write-Host "Installing via winget (Rustlang.Rustup)..."
        winget install --id Rustlang.Rustup -e --source winget
    } else {
        Write-Host "Please install Rust manually from https://rustup.rs and re-run this script."
        exit 1
    }
}

if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    Write-Host "git is required but was not found on PATH. Install Git for Windows first: https://git-scm.com/download/win"
    exit 1
}

if (-not (Test-Path $InstallDir)) {
    git clone $RepoUrl $InstallDir
} else {
    Write-Host "Existing install found at $InstallDir, pulling latest..."
    git -C $InstallDir pull --ff-only
}

Set-Location $InstallDir
cargo build --release

$ExePath = Join-Path $InstallDir "target\release\rs-sync.exe"
Write-Host ""
Write-Host "Build complete: $ExePath"

$LaunchScript = Join-Path $InstallDir "run-rs-sync.ps1"
@"
`$env:RS_SYNC_PORT = "8095"
`$env:RS_SYNC_DATA = "$InstallDir\data\store.json"
& "$ExePath"
"@ | Set-Content -Encoding utf8 $LaunchScript

Write-Host "Launch script written to $LaunchScript"
Write-Host "Run it with: powershell -ExecutionPolicy Bypass -File `"$LaunchScript`""
Write-Host "Then open http://127.0.0.1:8095/ in a browser."
Write-Host ""
Write-Host "Note: this does not register a Windows service. For always-on use,"
Write-Host "register $LaunchScript with Task Scheduler (run at log on) yourself,"
Write-Host "or run it in a terminal window when needed."
