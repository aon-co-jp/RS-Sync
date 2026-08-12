# PORTING.md — RS-Sync を他プロジェクト/他環境へお引越しする際のガイド

このファイル1枚で、RS-Syncを別のVPS/PC/開発環境へ導入・移設できるように
まとめる。対象バージョン: 2026-08-12時点(`main`ブランチ)。

## 0. このリポジトリのスコープ

GitHub / open-gitea / Gitea / GitBucket / GitLab / Bitbucket間でリポジトリを
一方向バックアップ同期・双方向同期するWebアプリ(Rust + [poem](https://github.com/poem-web/poem))。
gitホスティング非依存の単純なフォルダ間コピー(ローカル/SFTP/SMB)にも
対応する。詳細な機能一覧・プロバイダ対応状況は[README.md](README.md)、
開発経緯の詳細は[CLAUDE.md](CLAUDE.md)のHANDOFF節を参照。

他リポジトリへのpath依存・git依存は無い(単一クレート、`Cargo.toml`の
依存は全てcrates.io公開クレート)——このリポジトリ単体でclone・
ビルド・移設が完結する。

## 1. 持っていくもの(ファイル一覧)

```
rs-sync/
├── Cargo.toml / Cargo.lock
├── src/
│   ├── main.rs           # HTTPハンドラ・ルーティング・OTP認証ミドルウェア
│   ├── model.rs           # Account/SyncJob/Schedule/AppState、JSON永続化
│   ├── provider.rs        # GitHub/OpenGitea/Gitea/GitBucket/GitLab/Bitbucket
│   ├── sync_engine.rs      # git clone --mirror / push --mirrorの実行
│   ├── scheduler.rs        # 毎分ポーリング・自動間隔延長・排他ロック
│   ├── folder_sync.rs       # gitホスティング非依存のフォルダ同期(Local/Sftp/Smb)
│   ├── auth.rs / mail.rs     # OTPログイン・SMTP送信
│   └── rustjson.rs          # 緩い構文のJSONパーサー(store.json読み込み用)
├── static/index.html        # 日英2言語UI(SPA、`include_str!`でバイナリに埋め込み)
├── tests/sync_integration.rs  # 実git bareリポジトリでの一方向/双方向同期テスト
├── android/                 # Androidクライアント(リモート/ローカル実行両対応)
├── install.sh / install.ps1   # Linux/macOS/Windows向けインストーラー
├── .github/workflows/release.yml # タグpushでLinux/Windowsバイナリを自動リリース
├── PORTING.md(本ファイル)
└── CLAUDE.md
```

**`static/index.html`は`src/main.rs`の`include_str!("../static/
index.html")`でバイナリへコンパイル時埋め込みされる**——実行時に
ファイルを読みに行く方式ではないため、**HTMLだけを直接VPSへ上書きしても
反映されない。必ず`cargo build --release`で再ビルドしてからデプロイする
こと**(2026-08-12、この節を追記するきっかけになった実際の見落とし
パターン——修正がJSだけであっても再ビルドが必須)。

## 2. ビルド

```bash
cargo build --release
# 生成物: target/release/rs-sync(Windowsは.exe)
```

クロスコンパイル用の追加ツールチェーンは不要(pure-Rust依存のみ、
`ssh2`〈OpenSSL依存〉は2026-08-07に`russh`/`russh-sftp`へ置き換え済み)。
Android向けクロスビルドは`cargo ndk -t arm64-v8a -t x86_64 build
--release --bin rs-sync`(`android/app/src/main/jniLibs/`へ配置)。

## 3. 実行時の環境変数

| 変数 | 既定値 | 用途 |
|---|---|---|
| `RS_SYNC_DATA` | `data/store.json` | JSON永続化ファイルのパス |
| `RS_SYNC_PORT` | `8095` (本番運用では`8096`を使用) | listenポート |
| `RS_SYNC_REQUIRE_AUTH` | `true` | `false`でOTP認証を無効化(ループバック限定の検証用途のみ推奨) |
| `RS_SYNC_READ_ONLY` | `false` | `true`で`/api/*`のGET以外を403拒否(認証不要デモ環境向け) |
| `RS_SYNC_ADMIN_EMAIL` | なし | OTPログインを許可する唯一の管理者メールアドレス |
| `RS_SYNC_SMTP_*` | なし | OTPメール送信用SMTP設定一式(`mail.rs`参照) |
| `RS_SYNC_PRIMARY_HEALTHZ_URL` | なし | 副系(スマホ等)にのみ設定。主系(VPS)の`/healthz`を指すと、主系が生きている間は副系側の自動実行をスキップ |

## 4. VPSデプロイの実際の手順(本リポジトリでの実運用パターン)

```bash
ssh <vps>
cd /root/rs-sync
git pull
cargo build --release
systemctl restart rs-sync.service rs-sync-demo.service
curl -s -o /dev/null -w '%{http_code}\n' https://<domain>/rs-sync/
```

- `rs-sync.service`(本番、認証必須)・`rs-sync-demo.service`
  (デモ、`RS_SYNC_REQUIRE_AUTH=false`+`RS_SYNC_READ_ONLY=true`、
  本番とは別の`RS_SYNC_DATA`)は同一の`WorkingDirectory`・同一バイナリを
  共有するため、1回のビルドで両方に反映される。
- **デプロイのたびに必ず`cargo build --release`から行うこと**
  (上記1節の`include_str!`の罠を参照)。`systemctl restart`だけでは
  古いバイナリのまま(=古い`static/index.html`のまま)動き続ける。
- デプロイ後は`curl`でのステータスコード確認だけでなく、可能であれば
  実際にブラウザ(またはローカルでの`RS_SYNC_REQUIRE_AUTH=false`起動+
  Claude Browser pane等)で新機能の見た目・動作まで確認すること
  (このリポジトリで2026-08-12、間隔編集欄の入力保持バグ等、curl確認
  だけでは検出できない実バグを複数発見した実績あり)。

## 5. データ永続化・移行

- `data/store.json`(gitignore対象)——アカウント(トークン含む)・
  同期ジョブ・フォルダ同期ジョブ・実行ログを1ファイルに保持。
  読み込みは[RustJSON](src/rustjson.rs)(緩い構文のJSONスーパーセット、
  open-redmine/RPoemから移植)経由、書き込みは標準の
  pretty-printed JSON。
- 新フィールド追加時は`#[serde(default)]`を付けて既存データとの後方
  互換を維持する(このリポジトリの一貫した方針、`SyncJob`/
  `FolderSyncJob`の各追加フィールド参照)。
- `data/cache/<ジョブID>/<リポジトリ名>.git`——gitの永続キャッシュ
  (bareミラー)。初回のみ`git clone --mirror`、以降は`git fetch
  --prune`で差分のみ取得する設計のため、**このディレクトリを消すと
  次回同期が全リポジトリ分フルクローンし直しになる**(移設時は
  コピーを検討、必須ではないが初回同期が長くなる)。

## 6. 命名規約

- クレート名/バイナリ名: `rs-sync`
- systemd unit: `rs-sync.service` / `rs-sync-demo.service`
- 環境変数prefix: `RS_SYNC_*`
- Androidパッケージ名: `tokyo.runo.rssync`

## 7. 移植・拡張時の注意(このリポジトリで実際に踏んだ罠)

- **スケジューラのtickと手動「今すぐ同期」の競合**: 同じジョブに対して
  自動実行と手動実行が同時に走ると、同じgitキャッシュディレクトリへ
  同時に`git clone --mirror`しようとして競合する(`cannot copy
  '...hooks/pre-applypatch.sample': File exists`)。`AppState.
  running_jobs`(ジョブID単位の簡易排他ロック)で対策済み——同種の
  「スケジューラ+手動トリガー」構成を他リポジトリへ移植する際は、
  同じ排他制御パターンを検討すること。
- **Windows環境でのSFTPリモートパス組み立て**: `std::path::Path::join`は
  ホストOSのパスセパレータを使うため、Windows開発機で`/`区切りの
  リモートパスへ結合すると`\`が混入する。`join_unix_path()`
  (`folder_sync.rs`)のように、SFTP等常にUnix形式を期待する相手には
  文字列結合で明示的に`/`区切りにすること。
- **トークンをエラーメッセージ・URLにそのまま含めない**: `git`コマンドの
  引数(`https://<token>@host/...`)・標準エラー出力はいずれもトークンを
  含みうる。`redact_credentials()`(`sync_engine.rs`)のようなマスク処理を
  必ず経由してからログ・API応答・UIへ渡すこと(2026-07-30に実際に本番で
  トークンが平文で画面に漏れた事故の教訓)。
- **フロントエンドの背景自動更新が編集中の入力欄を上書きする罠**:
  定期的な`renderAll()`のような全体再描画は、ユーザーが入力中のフォーム
  要素ごと消し飛ばす。編集中の要素IDをクライアント側のSetで記憶し、
  再描画時にその状態・入力値を保持するパターン(`static/index.html`の
  `openScheduleEdits`)を、同種の「定期更新+インライン編集」を持つ他
  リポジトリへ移植する際も検討すること。

## 8. テスト

```bash
cargo test --lib                                   # 単体テスト
cargo test --bin rs-sync                            # main.rsの統合テスト(HTTPハンドラ)
cargo test --test sync_integration -- --test-threads=1  # 実git bareリポジトリでの同期テスト(cwd変更のためシングルスレッド必須)
```

## 9. 関連プロジェクト

- [open-gitea](https://github.com/aon-co-jp/open-gitea) — 自前Git forge、RS-Syncのプロバイダの1つ
- [open-raid-z](https://github.com/aon-co-jp/open-raid-z) — 開発ルールの正本
