# このフォルダについて / About this folder

`rs-sync.iss` はこのインストーラーを**作るための** [Inno Setup](https://jrsoftware.org/isinfo.php)
ビルドスクリプトです。`rs-sync-installer.exe` はこのフォルダ内に実体として置いてあります
(ユーザー指示により、v0.1.1でビルドしたバイナリを直接コミット)。

**⬇ 今すぐダウンロード**: [rs-sync-installer.exe](rs-sync-installer.exe)

**正直な開示**: このファイルはビルド成果物であり、`rs-sync.iss`やソースコードを変更しても
自動的には更新されません(手動での再ビルド・再コミットが必要)。常に最新版が欲しい場合は
[GitHub Releases](https://github.com/aon-co-jp/RS-Sync/releases/latest)(`.github/workflows/release.yml`が
バージョンタグpush時に自動ビルド・公開)を参照してください。

---

`rs-sync.iss` is the [Inno Setup](https://jrsoftware.org/isinfo.php) script used to **build**
this installer. `rs-sync-installer.exe` is committed directly in this folder as the actual
binary (built from v0.1.1, per user request).

**⬇ Download now**: [rs-sync-installer.exe](rs-sync-installer.exe)

**Honest disclosure**: this file is a build artifact — it does not update automatically when
`rs-sync.iss` or the source code changes (it must be manually rebuilt and re-committed). For
the always-current version, see [GitHub Releases](https://github.com/aon-co-jp/RS-Sync/releases/latest)
(auto-built and published by `.github/workflows/release.yml` on every version tag push).
