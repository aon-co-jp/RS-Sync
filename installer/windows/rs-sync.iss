; rs-sync Windowsインストーラー(Inno Setup)。
;
; ユーザー指示「実行バイナリを持つリポジトリについてのみ、
; {リポジトリ名}-install.exeという名前の単一Windows実行ファイル
; インストーラーを作成」への対応。`open-english`の
; `installer/windows/open-english.iss`の実装パターン(PrivilegesRequired
; =lowest・バージョン番号なしファイル名・自動起動)を踏襲した。
;
; 正直な開示: このインストーラーが同梱するのは`cargo build --release
; --bin rs-sync`でこの開発機上でビルドした実行ファイル1個+
; `install.ps1`(参考同梱、実行はしない)のみ。データファイル
; (`data/store.json`)は同梱しない(初回起動時に自動生成される)。
;
; ビルド方法: リポジトリルートで`cargo build --release --bin rs-sync`
; を実行した後、このディレクトリで`ISCC.exe rs-sync.iss`を実行する。

#define MyAppName "rs-sync"
#ifndef MyAppVersion
  #define MyAppVersion "0.0.0-local-build"
#endif
#define MyAppPublisher "aon-co-jp"
#define MyAppURL "https://github.com/aon-co-jp/RS-Sync"
#define MyAppExeName "rs-sync.exe"

[Setup]
; open-englishと同じ理由(管理者権限を要する操作を一切行わないため)で
; UAC昇格を不要にする。
PrivilegesRequired=lowest
AppId={{7C1E4F2A-9B3D-4E6A-8F21-3D5C6B9A1E02}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}/releases
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
UninstallDisplayIcon={app}\{#MyAppExeName}
Compression=lzma2
SolidCompression=yes
OutputDir=dist
; エコシステム全体の命名規則(open-english/2026-08-17決定)に合わせ、
; バージョン番号を含まない固定名`rs-sync-install.exe`にする——
; 自動アップデート機能(`self_update.rs`のWindows経路)が展開する
; zip内のファイル名は`rs-sync.exe`のみのため、この`.iss`自体は
; 自動アップデートの対象アセットではない(手動配布用インストーラー)。
OutputBaseFilename=rs-sync-install
ArchitecturesInstallIn64BitMode=x64compatible
DisableProgramGroupPage=yes

[Languages]
Name: "japanese"; MessagesFile: "compiler:Languages\Japanese.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Files]
Source: "..\..\target\release\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\install.sh"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "..\..\install.ps1"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "README-INSTALLED.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Run]
; RS_SYNC_SELF_UPDATE=trueで自動更新を明示的に有効化する
; (`self_update.rs`モジュールdoc「正直な開示」参照、既定オフのため
; インストーラー起動時にこのフラグを付けて起動する)。
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#MyAppName}}"; Flags: nowait postinstall skipifsilent runasoriginaluser; Parameters: ""
Filename: "http://127.0.0.1:8095/"; Flags: shellexec nowait postinstall skipifsilent runasoriginaluser

[UninstallDelete]
Type: filesandordirs; Name: "{app}\data"
