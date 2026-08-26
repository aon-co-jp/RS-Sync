# このフォルダについて / About this folder

`rs-sync.iss` はインストーラーを**作るための** [Inno Setup](https://jrsoftware.org/isinfo.php)
ビルドスクリプトです。コンパイル済みの `rs-sync-installer.exe` 本体はこのリポジトリには
コミットしていません(ビルド成果物のため、`dist/` は `.gitignore` 対象)。

**実際にインストーラーをダウンロードしたい方はこちら:**

➡ https://github.com/aon-co-jp/RS-Sync/releases/latest

直接リンク: https://github.com/aon-co-jp/RS-Sync/releases/latest/download/rs-sync-installer.exe

`.github/workflows/release.yml` がバージョンタグ(`v*.*.*`)のpush時にこのスクリプトを
`ISCC.exe`でコンパイルし、生成された `rs-sync-installer.exe` を上記GitHub Releasesへ
自動的に添付しています。

---

This folder only contains `rs-sync.iss`, the [Inno Setup](https://jrsoftware.org/isinfo.php)
script used to **build** the installer. The compiled `rs-sync-installer.exe` itself is not
committed to this repository (it's a build artifact; `dist/` is gitignored).

**To download the actual installer, go here:**

➡ https://github.com/aon-co-jp/RS-Sync/releases/latest

Direct link: https://github.com/aon-co-jp/RS-Sync/releases/latest/download/rs-sync-installer.exe

`.github/workflows/release.yml` compiles this script with `ISCC.exe` whenever a version tag
(`v*.*.*`) is pushed, and automatically attaches the resulting `rs-sync-installer.exe` to the
GitHub Release above.
