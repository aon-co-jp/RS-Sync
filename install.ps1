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

$TaskName = "RS-Sync"
$answer = Read-Host "Register a Task Scheduler task to auto-start RS-Sync at logon? [y/N]"
if ($answer -match '^[Yy]') {
    # Windowsにはsystemd/launchdに相当する常駐サービス機構が無いため、
    # ログオン時トリガー+終了時の自動再起動(既定の再試行設定)で
    # Linux版のsystemd --user・Mac版のlaunchd(RunAtLoad+KeepAlive)に
    # おおよそ対応するTask Schedulerタスクとして登録する。
    $existing = Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    if ($existing) {
        Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false
    }
    $action = New-ScheduledTaskAction -Execute "powershell.exe" -Argument "-WindowStyle Hidden -ExecutionPolicy Bypass -File `"$LaunchScript`""
    $trigger = New-ScheduledTaskTrigger -AtLogOn
    $settings = New-ScheduledTaskSettingsSet -RestartCount 999 -RestartInterval (New-TimeSpan -Minutes 1) -ExecutionTimeLimit (New-TimeSpan -Days 0)
    Register-ScheduledTask -TaskName $TaskName -Action $action -Trigger $trigger -Settings $settings -Description "RS-Sync git backup/mirror sync tool" | Out-Null
    Start-ScheduledTask -TaskName $TaskName
    Write-Host "Registered and started Task Scheduler task '$TaskName' (auto-starts at logon, auto-restarts on failure)."
    Write-Host "Check status with: Get-ScheduledTask -TaskName '$TaskName' | Get-ScheduledTaskInfo"
    Write-Host "To uninstall: Unregister-ScheduledTask -TaskName '$TaskName' -Confirm:`$false"
} else {
    Write-Host "Skipped. For always-on use later, register $LaunchScript with Task Scheduler yourself,"
    Write-Host "or run it in a terminal window when needed."
}
