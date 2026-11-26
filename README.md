> Latest translations / 翻訳の最新版: [0-LATEST-TRANSLATIONS.md](0-LATEST-TRANSLATIONS.md)

# RS-Sync

Git repository sync/backup tool across multiple git-hosting providers
(GitHub / RS-Git / Gitea / Gitbucket), built with Rust + [poem](https://github.com/poem-web/poem).
It also syncs plain folders (not just git repos) between your VPS/rental
server, local drive, and NAS — see [`src/folder_sync.rs`](src/folder_sync.rs).
日本語の説明は下にあります。

## ⬇ Download / ダウンロード

**Windows**: [rs-sync-installer.exe](https://github.com/aon-co-jp/RS-Sync/releases/latest/download/rs-sync-installer.exe)
— double-click to install, no admin rights or command line needed. /
ダブルクリックでインストールできます(管理者権限・コマンド操作不要)。

**Linux**: [rs-sync-linux-x86_64.tar.gz](https://github.com/aon-co-jp/RS-Sync/releases/latest/download/rs-sync-linux-x86_64.tar.gz)
(includes `install.sh`) — see [all releases](https://github.com/aon-co-jp/RS-Sync/releases).

## v0.2.0 (2026-11-20)
- **日本語**: ログイン方式を3つから選べます(メールのワンタイムコード `otp` / コード+スマホでQR確認 `otp_qr` / QR確認だけ `qr`)。選んだ方式は再起動しても残ります。open-gitea との連携は Connect に移りました(REST は使いません)。試験108件成功、本番VPSでも同じ機能が動いています。
- **English**: Choose one of three login modes (e-mail one-time code `otp`, code plus a phone QR check `otp_qr`, or QR check only `qr`); the choice survives a restart. The open-gitea link now uses Connect (no REST). 108 tests pass, and the same features run on the production VPS.
- **简体中文**:可从三种登录方式中选择(邮件一次性验证码 `otp`、验证码加手机扫码确认 `otp_qr`、仅扫码确认 `qr`),重启后仍会保留。与 open-gitea 的连接改用 Connect(不再使用 REST)。108 项测试通过。
- **繁體中文**:可從三種登入方式中選擇(郵件一次性驗證碼 `otp`、驗證碼加手機掃碼確認 `otp_qr`、僅掃碼確認 `qr`),重新啟動後仍會保留。與 open-gitea 的連線改用 Connect(不再使用 REST)。108 項測試通過。
- **한국어**: 로그인 방식을 세 가지 중에서 고를 수 있습니다(메일 일회용 코드 `otp`, 코드+휴대폰 QR 확인 `otp_qr`, QR 확인만 `qr`). 선택한 방식은 다시 시작해도 유지됩니다. open-gitea 연동은 Connect로 바뀌었습니다(REST 미사용). 테스트 108개 통과.
- **Deutsch**: Drei Anmeldearten stehen zur Wahl (Einmalcode per E-Mail `otp`, Code plus QR-Bestätigung am Handy `otp_qr`, nur QR-Bestätigung `qr`); die Wahl bleibt nach einem Neustart erhalten. Die Anbindung an open-gitea läuft jetzt über Connect (kein REST). 108 Tests bestanden.
- **Français** : trois modes de connexion au choix (code à usage unique par e-mail `otp`, code plus confirmation QR sur le téléphone `otp_qr`, confirmation QR seule `qr`) ; le choix est conservé après un redémarrage. Le lien avec open-gitea passe désormais par Connect (sans REST). 108 tests réussis.
- **Español**: puedes elegir entre tres modos de inicio de sesión (código de un solo uso por correo `otp`, código más confirmación QR en el móvil `otp_qr`, o solo confirmación QR `qr`); la elección se conserva tras reiniciar. La conexión con open-gitea usa ahora Connect (sin REST). 108 pruebas superadas.
- **Português**: escolha entre três modos de login (código único por e-mail `otp`, código mais confirmação por QR no celular `otp_qr`, ou só confirmação por QR `qr`); a escolha continua após reiniciar. A ligação com o open-gitea agora usa Connect (sem REST). 108 testes aprovados.
- **Русский**: можно выбрать один из трёх способов входа (одноразовый код по почте `otp`, код плюс подтверждение QR на телефоне `otp_qr`, только подтверждение QR `qr`); выбор сохраняется после перезапуска. Связь с open-gitea теперь идёт через Connect (без REST). 108 тестов пройдено.

## GitHub Actions だけで自動バックアップ(GitHub → GitLab) / Backup with GitHub Actions only

VPS やレンタルサーバー以外の GitHub / GitLab のデータは、GitHub Actions で自動バックアップできます。スクリプトと使い方(日本語・英語)は [`docs/github-actions-backup/`](docs/github-actions-backup/README.md)。
For data on GitHub / GitLab (not on your own server), GitHub Actions can back it up automatically. Scripts and instructions (Japanese and English): [`docs/github-actions-backup/`](docs/github-actions-backup/README.md).

## What it does (MVP)

- Register **accounts** (source/destination) for a git-hosting provider,
  storing name/API base/git base/owner/token in a local JSON file
  (`data/store.json`, gitignored — never committed). Read via
  [RustJSON](src/rustjson.rs) (a lenient JSON superset — trailing commas,
  comments, unquoted keys — ported from open-redmine/RPoem, matching the
  rest of this ecosystem); write is still standard pretty-printed JSON, so
  the file stays readable and hand-editable.
- Define **sync jobs**: source account -> destination account, one-way or
  two-way, for a chosen repo list (or "all repos" via provider discovery).
- **Schedule** each job by "every N hours", day-of-week + time-of-day, and/or
  a short realtime poll interval; a background loop checks every minute and
  fires due jobs.
- **Run any job on demand** ("sync now") from the web UI.
- Actual sync uses the standard git smart-HTTP protocol via the `git` CLI
  (`git clone --mirror` / `git push --mirror`), the same approach as the
  existing VPS cron script this project supersedes.
- **Two-way conflict safety**: if both sides changed since the last
  successful sync, the repo is flagged as conflicted and excluded from
  automatic runs until a human clears it from the UI — no blind overwrite.

## Provider support status

| Provider  | List repos | Create repo | git clone/push | Verified live |
|-----------|-----------|-------------|-----------------|----------------|
| GitHub    | Yes | Yes | Yes | Yes (used in this repo's own sync) |
| open-gitea | Yes (via `/api/repos`) | Yes, authenticated (`Bearer` token + Basic-auth git push) | Yes | Yes — verified live against `https://easy-web.tokyo/open-gitea/api/repos` (2026-07-28) |
| Gitea     | Implemented against documented API | Implemented | Yes | No — not tested against a live Gitea instance |
| GitBucket (Scala OSS) | Implemented against documented GitHub-compatible API | Implemented | Yes | No — not tested against a live GitBucket instance |
| GitLab (self-hosted or gitlab.com) | Implemented against documented REST v4 API | Implemented | Yes | No — not tested against a live GitLab instance |
| Bitbucket (Atlassian Cloud) | Implemented against documented REST v2 API (first page only, no pagination) | Implemented | Yes | No — not tested against a live Bitbucket workspace |

**Rented/VPS servers**: there is no separate "rented server" provider type —
any self-hosted git-hosting service (Gitea, GitBucket, open-gitea) that you
run on your own rented server or VPS works today by simply pointing the
account's API base / git base URLs at that server; the same holds for
GitLab/Bitbucket if self-hosted (GitLab CE, Bitbucket Data Center).

All git transport (clone/push/fetch) is provider-agnostic — it is plain git
over HTTP(S), so any provider that speaks git smart-HTTP will work for the
actual sync even where the REST adapter above is unverified; only repo
discovery/creation depends on the adapter.

## Recent updates (2026-08-12)

- **Scheduler/manual-run race fixed**: a per-job lock (`AppState.running_jobs`)
  now prevents the scheduler's tick and a manual "sync now" click from
  racing on the same job's git cache directory (was causing real
  `cannot copy '...hooks/pre-applypatch.sample': File exists` failures in
  production for a large repo set).
- **Interval display + inline editing + auto-widen**: each job now shows its
  current check interval, can have it changed without recreating the job
  (`PUT /api/jobs/:id`, `PUT /api/folder-jobs/:id`), and the server
  automatically raises the interval (never lowers it) if a run's actual
  duration exceeds the configured interval — tiers
  `[1,3,5,15,30,60,120,240,360,720,1440]` minutes, target ≥1.5× the measured
  duration. The originally-specified interval is kept separately
  (`initial_realtime_poll_minutes`) so it never gets silently overwritten by
  the auto-widen logic.
- **Quick-setup form no longer duplicates accounts/jobs on resubmit**: it now
  reuses an existing account by owner (skipping the token update if the
  field is left blank) and updates an existing job's schedule instead of
  creating a duplicate — and pre-fills from existing accounts on load, with
  token fields showing a masked "already set" placeholder rather than ever
  re-displaying the real token.
- **Live status/date UI**: per-job "syncing / complete / failed / waiting for
  next run / schedule canceled" pills (JA+EN), a live clock, a
  per-job "next sync estimate" column, and timezone/country labels on all
  timestamps (defaults to the viewer's own local time; Japan shows "日本時刻",
  others show an English + local-language country name, Iran shown as
  "Iran (Persia)").

## Recent updates (2026-08-07)

- **VPS/phone duplicate-run avoidance (failover)**: set
  `RS_SYNC_PRIMARY_HEALTHZ_URL` on a secondary instance (e.g. a phone) to
  the primary instance's (e.g. a VPS) `/healthz` URL. The scheduler skips
  its own job execution for a tick whenever the primary responds; it only
  runs jobs itself when the primary is unreachable. See `src/scheduler.rs`.
- **Android app: local-server mode (new, not yet cross-compiled)**: the
  Android client can now optionally run the RS-Sync binary directly on the
  device (`ProcessBuilder`, mirroring `open-easy-web`'s pattern) instead of
  only acting as a remote client. Disabled by default. See
  `android/.../LocalServerConfig.kt`. **Honest gap**: the native binary
  (`librssync.so`, built via `cargo ndk`) has not been cross-compiled or
  placed into `jniLibs/` in this pass — the Kotlin code builds
  successfully but on-device startup is unverified.

## What is NOT implemented yet (documented honestly)

- Webhook-based realtime detection — only polling is implemented.
- Full cron-expression scheduling — only "every N hours" / day-of-week+time /
  poll-interval combinations.
- Per-branch two-way conflict detection — only the default branch's HEAD sha
  is compared; other branches are mirrored blindly in whichever direction the
  default branch's comparison decides.
- Code-signed installers / OS package manager integration (msi/deb) — see
  `install.sh` / `install.ps1` for the current from-source installer scope.
  As of 2026-07-23, `.github/workflows/release.yml` also builds and attaches
  prebuilt Linux x86_64 / Windows x86_64 binaries to
  [GitHub Releases](https://github.com/aon-co-jp/RS-Sync/releases) on every
  `v*` tag push, as an alternative to the source-build flow.
- Cross-compiling the Linux binary from this Windows dev machine (no
  cross-linker toolchain installed); Linux binaries are built directly on
  the target Linux host instead (verified on the VPS, see HANDOFF below).

## Self-update (2026-08-19, opt-in)

`src/self_update.rs` polls the GitHub Releases API for a newer tag,
downloads the matching prebuilt asset (from the `release.yml` flow above),
and swaps the running binary with a health-check + automatic rollback
(`GET /healthz`). This is a **separate path from `install.sh`/`install.ps1`'s
`git pull --ff-only` + `cargo build --release` flow** — it only replaces a
copy that was installed from a prebuilt release asset, never a source
checkout, so the two update mechanisms cannot race on the same file.
Disabled by default; set `RS_SYNC_SELF_UPDATE=true` to opt in (VPS
production keeps relying on `git pull`, not this). A Windows installer
(`installer/windows/rs-sync.iss`, built with Inno Setup into
`installer/windows/dist/rs-sync-install.exe`, gitignored) has been built and
manually install/uninstall-tested on this dev machine; the Linux
self-update path is code-reviewed only (not exercised on real hardware yet),
and no GitHub Release tag has existed yet to exercise the full
detect→download→swap→rollback flow end-to-end. See CLAUDE.md HANDOFF
(2026-08-19) for details.

## Deployment

- **Production/admin**: <https://easy-web.tokyo/rs-sync/>
- **Demo**: <https://easy-web.tokyo/rs-sync/demo> (honest disclosure: currently
  just an alias to the same backend as production — no isolated demo dataset
  yet)
- The old `runo.tokyo/rs-sync` demo instance was fully decommissioned on
  2026-07-28 (service, tenant routing, and data all removed) in favor of the
  above.

## Running locally

```
cargo run
# then open http://127.0.0.1:8095/
```

## Testing

```
cargo test --test sync_integration -- --test-threads=1
```

This spins up real local bare git repositories, runs `run_job` against them
(no network), and asserts the destination actually received the source's
commit — for both one-way and two-way modes.

## Security

Account tokens are stored only in `data/store.json` (gitignored) or, on the
VPS, under the deployed app's data directory outside of any git repo. Tokens
and hostnames are never written into commits, README, or CLAUDE.md.

Token fields are automatically trimmed of leading/trailing whitespace and
newlines (both client-side and server-side, 2026-08-12) — this matters
because pasting a GitHub PAT often carries a stray trailing newline, which
GitHub's API rejects with a bare `401 Unauthorized` (not a scope error).

---

## 日本語

RS-Syncは、複数のGitホスティングサービス(GitHub / open-gitea / Gitea /
Gitbucket)間でリポジトリを同期・バックアップするWebアプリです
(Rust + poem)。

### 運用先

- **本番/管理者用**: <https://easy-web.tokyo/rs-sync/>
- **デモ用**: <https://easy-web.tokyo/rs-sync/demo>(正直な開示: 現状は
  本番と同一バックエンドを指すエイリアスで、独立したデモ専用データは
  まだ無い)
- 旧`runo.tokyo/rs-sync`のデモインスタンスは2026-07-28に完全に廃止
  (サービス・テナントルーティング・データを全て削除)し、上記へ一本化した。

### できること(MVP)

- アカウント登録(ソース/宛先、プロバイダ種別、API/gitのベースURL、
  owner、トークン)—— `data/store.json`(gitignore対象)にのみ保存。
  トークン欄はコピペ時に紛れ込む前後の空白・改行を自動的に除去する
  (クライアント側・サーバー側の両方、2026-08-12)——GitHub PATに
  余分な改行が付いたままだとGitHub側は資格情報として無効と判断し、
  スコープ不足の`403`ではなく`401 Unauthorized`を返す。
- 同期ジョブ設定(ソース→宛先、一方向/双方向、対象リポジトリ)。
- スケジュール(何時間ごと・曜日+時刻・リアルタイムポーリング間隔の
  組み合わせ)。バックグラウンドで毎分チェックし該当ジョブを実行。
- Web UIから「今すぐ同期」で手動実行。
- 実際の同期は`git`コマンドの`clone --mirror`/`push --mirror`(既存の
  VPS cronスクリプトと同じ方式)。
- 双方向同期での安全策: 前回同期以降に両側で変更があった場合は
  コンフリクトとして記録し、UIから手動解除するまで自動実行を止める
  (力任せの上書きはしない)。

### プロバイダ対応状況

上記の英語表を参照。GitHub・open-giteaとも実機(実HTTP・実git push)で
検証済み(2026-07-28、open-gitea側はリポジトリ作成・git pushの認証欠落
バグを修正した上で本番`https://easy-web.tokyo/open-gitea/`への疎通を
確認)。Gitea/GitBucket(Scala製OSS)/GitLab/Bitbucket(Atlassian Cloud)は
いずれも公開APIドキュメントに基づく実装のみで実機未検証(2026-07-29、
ユーザー要望「Bitbucket・Gitlabなどにも同期対応」を受けGitLab/Bitbucket
アダプタを追加)。**「レンタルサーバー/VPSレンタルサーバー」専用の
プロバイダ種別は存在しない**——自分のVPS/レンタルサーバー上で動かして
いるGitea/GitBucket/open-gitea(あるいはセルフホスト版GitLab/Bitbucket)
も、そのURLをAPI base/git baseに指定するだけで既存のプロバイダとして
そのまま登録できる。

### 最近の更新(2026-08-12)

- **スケジューラtickと手動実行の競合を修正**: ジョブID単位の排他ロック
  (`AppState.running_jobs`)を追加し、両者が同じgitキャッシュ
  ディレクトリへ同時に書き込んで失敗する実障害(本番で実際に発生)を解消。
- **間隔の表示・その場での変更・データ量に応じた自動延長**: 各ジョブに
  現在のチェック間隔を表示し、削除して作り直さず変更できる
  (`PUT /api/jobs/:id`・`PUT /api/folder-jobs/:id`)。実行時間が設定間隔を
  超えたらサーバー側で自動的に間隔を引き上げる(縮めることはしない、
  段階`[1,3,5,15,30,60,120,240,360,720,1440]`分、目安は実測の1.5倍以上)。
  最初に指定した間隔は`initial_realtime_poll_minutes`として別途保持し、
  自動延長で上書きされない。
- **クイック設定フォームの再送信で重複作成しないよう修正**: owner一致の
  既存アカウントを再利用(トークン欄が空欄なら変更しない)、既存ジョブは
  スケジュールのみ更新。フォームは既存アカウントから自動入力され、
  トークン欄は実トークンを再表示せず「設定済み」の伏字プレースホルダの
  み表示。
- **状態表示・日時UIの強化**: 同期中/完了/失敗/次回待機中/予約解除の
  ステータス表示(日英)、現在時刻のライブ表示、次回同期予定列、
  タイムゾーン・国名ラベル(閲覧者の現地時刻がデフォルト、日本なら
  「日本時刻」、他国は英語+現地語の国名、イランのみ"Iran (Persia)")。

### 最近の更新(2026-08-07)

- **VPS/スマホ二重稼働時の重複実行回避(フェイルオーバー方式)**: 副系
  (スマホ等)に`RS_SYNC_PRIMARY_HEALTHZ_URL`環境変数を設定し、主系
  (VPS等)の`/healthz` URLを指すと、主系が生きている間は副系側の
  スケジューラがそのtickでのジョブ実行をスキップする。主系が不通の
  ときのみ副系が実行を代行する(`src/scheduler.rs`参照)。
- **Android版に「本体をこの端末で実行」機能を新設(未クロスビルド)**:
  従来の「リモートクライアント専用」設計に加え、`open-easy-web`と同じ
  `ProcessBuilder`方式で本体(Rustバイナリ)を端末上で直接起動できる
  ようにした(既定は無効、`android/.../LocalServerConfig.kt`参照)。
  **正直な開示**: `cargo ndk`でのネイティブバイナリ(`librssync.so`)の
  実クロスビルド・`jniLibs`への配置は今回未実施——Kotlin側のビルド
  成功までの確認に留まる。

### 未実装(正直な開示)

- Webhookによるリアルタイム検知(ポーリングのみ実装)。
- 完全なcron式(「何時間ごと」「曜日+時刻」「ポーリング間隔」の組合せのみ)。
- ブランチ単位の双方向コンフリクト検知(デフォルトブランチのHEADのみ比較)。
- コード署名済みインストーラー・deb/msiパッケージング。2026-07-23、
  `.github/workflows/release.yml`を追加し、タグ(`v0.1.0`〜)push時に
  Linux x86_64・Windows x86_64向けのプリビルド済みバイナリを
  [GitHub Releases](https://github.com/aon-co-jp/RS-Sync/releases)へ
  自動添付するようにした(ソースビルド方式の代替として提供)。
- このWindows開発機からのLinuxバイナリのクロスコンパイル(クロス
  リンカー未導入のため未対応。Linux向けはVPS上で直接ビルドして検証済み)。

### 次回再開ポイント(2026-07-29時点)

詳細は[CLAUDE.md](CLAUDE.md)の「HANDOFF追記(2026-07-29続き5)」参照。
未着手: (1) 実SFTPサーバーでのフォルダ同期の実機検証、(2) 実トークン
期限切れシナリオでの通知メール実地確認。
(3)は解消済み——確認したところ`open-easy-web/uninstall.ps1`は
既に作成済み(構文チェック済み、data-portability.ps1経由のバックアップ
選択フロー込み)だった。この記載は古い情報だったため訂正(2026-07-29)。

## API 方針: REST 全廃・gRPC / Connect へ (2026-11-04)

### 日本語
- **REST は全廃する。新しい API は REST で作らない。** ブラウザや `curl` から呼ぶ API は **Connect の単純呼び出し**(`POST /<パッケージ>.<Service>/<Method>`、`content-type: application/json`、本文は JSON)、サービス間は **gRPC / gRPC-web**(RPoem の管理API `open_runo.admin.v1.Admin` と同じ流儀)。ストリーミングは、必要になってから。
- **既存の REST は、触るときに順に移す。** 移したら旧 REST の経路は消す。移行が終わるまで残るが、**新規には追加しない**。
- **JSON は RS-JSON に統一**する(読み込みは `from_str_secure` / `parse_secure` でサイズ・深さ・重複キーの上限つき、書き出しは `rust_json::fast`)。ブラウザの WASM に `prost` は入れない(サイズが増えるため)。
- **例外(自分たちの API ではなく、相手の規格が決めているもの)**: ACME、OAuth/OIDC のコールバック、第三者(GitHub など)からの Webhook、ヘルスチェック、静的ファイル配信、OTLP など。これらは規格どおりの HTTP を受ける。
- 参考実装: `RPoem`(`open_runo.admin.v1.Admin`)、`open-easy-web`(`server/src/connect.rs`、`proto/open_easy_web/admin/v1/admin.proto`)。
- ※ このリポジトリのコードが既に方針どおりかどうかは、各リポジトリの現状による。この節は方針の記録であり、コードの変更を意味しない。

### English
- **REST is being abolished. New APIs are not built as REST.** APIs called from browsers or `curl` use **Connect unary calls** (`POST /<package>.<Service>/<Method>`, `content-type: application/json`, JSON body); service-to-service calls use **gRPC / gRPC-web** (the same style as RPoem's admin API `open_runo.admin.v1.Admin`). Streaming only when it is actually needed.
- **Existing REST endpoints are migrated one by one, when they are touched.** The old REST route is removed once migrated. Until migration is finished they remain, but **no new ones are added**.
- **JSON is unified on RS-JSON**: reading with `from_str_secure` / `parse_secure` (size, depth and duplicate-key limits), writing with `rust_json::fast`. `prost` is not put into browser WASM (it would grow the binary).
- **Exceptions (shapes dictated by someone else's standard, not our own API)**: ACME, OAuth/OIDC callbacks, webhooks from third parties (e.g. GitHub), health checks, static file serving, OTLP, and so on. These accept plain HTTP as the standard requires.
- Reference implementations: `RPoem` (`open_runo.admin.v1.Admin`) and `open-easy-web` (`server/src/connect.rs`, `proto/open_easy_web/admin/v1/admin.proto`).
- Note: whether the code in a given repository already follows this policy depends on that repository. This section records the policy; it does not mean the code has been changed.

### Deutsch (KI-Übersetzung, redaktionell überarbeitet; noch nicht von Muttersprachlern geprüft)
- **REST wird vollständig abgeschafft; neue APIs entstehen nicht mehr als REST.** APIs, die aus dem Browser oder per `curl` aufgerufen werden, verwenden **unäre Connect-Aufrufe** (`POST /<Paket>.<Service>/<Methode>`, `content-type: application/json`, JSON-Body). Zwischen Diensten kommen **gRPC bzw. gRPC-web** zum Einsatz, im selben Stil wie die Admin-API von RPoem (`open_runo.admin.v1.Admin`). Streaming wird erst ergänzt, wenn es tatsächlich gebraucht wird.
- **Bestehende REST-Endpunkte werden schrittweise umgestellt, und zwar immer dann, wenn man ohnehin an ihnen arbeitet.** Sobald ein Endpunkt umgestellt ist, wird die alte REST-Route entfernt. Bis zum Abschluss der Migration bleiben die übrigen bestehen, es werden aber **keine neuen mehr hinzugefügt**.
- **JSON wird einheitlich über RS-JSON verarbeitet**: Gelesen wird mit `from_str_secure` bzw. `parse_secure` (mit Obergrenzen für Größe, Verschachtelungstiefe und doppelte Schlüssel), geschrieben mit `rust_json::fast`. `prost` wird nicht in das Browser-WASM aufgenommen, da es die Datei vergrößern würde.
- **Ausnahmen (Schnittstellen, deren Form ein fremder Standard vorgibt und die nicht zu unserer eigenen API gehören)**: ACME, OAuth/OIDC-Callbacks, Webhooks von Drittanbietern (z. B. GitHub), Health-Checks, die Auslieferung statischer Dateien, OTLP und Ähnliches. Diese nehmen, wie vom jeweiligen Standard verlangt, gewöhnliches HTTP entgegen.
- Referenzimplementierungen: `RPoem` (`open_runo.admin.v1.Admin`) und `open-easy-web` (`server/src/connect.rs`, `proto/open_easy_web/admin/v1/admin.proto`).
- Hinweis: Ob der Code eines einzelnen Repositorys diese Richtlinie bereits erfüllt, hängt vom jeweiligen Repository ab. Dieser Abschnitt dokumentiert lediglich die Richtlinie; er besagt nicht, dass der Code schon angepasst wurde.

### Русский (перевод выполнен с помощью ИИ и отредактирован; носителями языка пока не проверен)
- **От REST мы полностью отказываемся; новые API больше не создаются в стиле REST.** API, к которым обращаются из браузера или через `curl`, используют **унарные вызовы Connect** (`POST /<пакет>.<Service>/<Method>`, `content-type: application/json`, тело запроса — JSON). Для взаимодействия между сервисами применяются **gRPC и gRPC-web** — в том же стиле, что и административный API RPoem (`open_runo.admin.v1.Admin`). Потоковая передача данных добавляется лишь тогда, когда в ней действительно возникает необходимость.
- **Существующие REST-эндпоинты переводятся поэтапно — по мере того, как с ними приходится работать.** После перевода старый REST-маршрут удаляется. Пока миграция не завершена, оставшиеся эндпоинты сохраняются, но **новые не добавляются**.
- **Работа с JSON унифицирована на основе RS-JSON**: чтение — через `from_str_secure` или `parse_secure` (с ограничениями на размер, глубину вложенности и повторяющиеся ключи), запись — через `rust_json::fast`. `prost` в браузерный WASM не включается, поскольку это увеличило бы размер файла.
- **Исключения (интерфейсы, форму которых определяет чужой стандарт, а не наш собственный API)**: ACME, обратные вызовы OAuth/OIDC, вебхуки сторонних сервисов (например, GitHub), проверки работоспособности, раздача статических файлов, OTLP и тому подобное. Такие интерфейсы принимают обычный HTTP, как того требует соответствующий стандарт.
- Эталонные реализации: `RPoem` (`open_runo.admin.v1.Admin`) и `open-easy-web` (`server/src/connect.rs`, `proto/open_easy_web/admin/v1/admin.proto`).
- Примечание: соответствует ли код конкретного репозитория этой политике, зависит от самого репозитория. Данный раздел лишь фиксирует политику и не означает, что код уже изменён.

## API 方針の補足: 用語の整理と使い分け (2026-11-04)

### 日本語
**1つに統一するものではなく、別の層の話です。** 「REST の代わり」になるのは Connect・gRPC・GraphQL だけで、WASM は画面を作る技術です。

| 用語 | 何か | REST の代わりか |
|---|---|---|
| REST | URL と HTTP メソッドで操作を表す API の作り方 | 全廃する側 |
| **Connect** | `POST /サービス名/メソッド名` に JSON を送る形式。`curl` でもブラウザの `fetch` でも呼べる | はい(**採用**) |
| **gRPC** | 同じ「サービス/メソッド」の考え方を、バイナリ(protobuf)と HTTP/2 で行う形式。速く、ストリーミングも得意 | はい(**サービス同士に採用**) |
| gRPC-web | gRPC のブラウザ向けの変形版 | はい(必要なら) |
| GraphQL | 欲しい項目を問い合わせる形式 | 一部だけ(**問い合わせ(読み取り)専用**) |
| WASM | Rust をブラウザの中で動かす仕組み | いいえ(画面を作る技術) |

- **ブラウザの画面や `curl` から呼ぶ API は Connect**(POST、JSON を RS-JSON で読み書き)。ブラウザの WASM は素の gRPC を話せず、Connect なら `fetch` のままで、`prost` を足さずに済む(画面のサイズも増えない)。
- **サービス同士の通信(サーバー間、重い処理、ストリーミング)は gRPC。** 多くは Connect で足りるので、ストリーミングが必要になった所だけ gRPC に上げる。
- **GraphQL は、まとめて問い合わせたいときの読み取り専用。** 書き込み(操作)には使わない。
- **WASM は画面を作る技術として、そのまま使う。** 画面から呼ぶ API が Connect になっただけ。
- 外部の規格が形を決めるもの(ACME、OAuth のコールバック、第三者の Webhook、ヘルスチェック、静的ファイル、OTLP)は、規格どおりの HTTP のまま。
- **迷ったら Connect。**

### English
**These are not alternatives to unify; they sit at different layers.** Only Connect, gRPC and GraphQL can replace REST; WASM is a way to build the UI.

| Term | What it is | Replaces REST? |
|---|---|---|
| REST | API style that expresses operations through URLs and HTTP methods | The thing being abolished |
| **Connect** | `POST /Service/Method` with a JSON body; callable from `curl` and from a browser `fetch` | Yes (**adopted**) |
| **gRPC** | The same service/method idea over binary (protobuf) and HTTP/2; fast, good at streaming | Yes (**for service-to-service**) |
| gRPC-web | A browser-friendly variant of gRPC | Yes (if needed) |
| GraphQL | A query language: ask for exactly the fields you want | Partly (**read-only queries**) |
| WASM | Runs Rust inside the browser | No (a UI technology) |

- **APIs called from browser screens or `curl` use Connect** (POST, JSON read and written with RS-JSON). Browser WASM cannot speak plain gRPC; with Connect it stays a `fetch`, and `prost` is not needed (the binary does not grow).
- **Service-to-service calls (server to server, heavy work, streaming) use gRPC.** Connect is enough in most places; move a call to gRPC only where streaming is actually needed.
- **GraphQL is read-only**, for queries that gather several pieces of data. It is not used for writes (operations).
- **WASM stays as the way to build the UI.** The API it calls simply became Connect.
- Interfaces whose shape an outside standard dictates (ACME, OAuth callbacks, third-party webhooks, health checks, static files, OTLP) stay plain HTTP.
- **When in doubt, use Connect.**

### Deutsch (KI-Übersetzung, sprachlich überarbeitet; noch nicht von Muttersprachlern geprüft)
**Es geht nicht darum, eine einzige Technik auszuwählen – die Begriffe liegen auf unterschiedlichen Ebenen.** Als Ersatz für REST kommen nur Connect, gRPC und GraphQL in Frage; WASM dagegen ist eine Technik, mit der die Benutzeroberfläche gebaut wird.

| Begriff | Was ist das? | Ersatz für REST? |
|---|---|---|
| REST | Ein API-Stil, bei dem Operationen über URLs und HTTP-Methoden ausgedrückt werden | wird abgeschafft |
| **Connect** | `POST /Dienst/Methode` mit JSON im Body; aufrufbar mit `curl` ebenso wie mit `fetch` im Browser | ja (**gewählt**) |
| **gRPC** | Dasselbe Prinzip aus Dienst und Methode, aber über binäres protobuf und HTTP/2; schnell und gut für Streaming geeignet | ja (**zwischen Diensten**) |
| gRPC-web | Eine für den Browser angepasste Variante von gRPC | ja (bei Bedarf) |
| GraphQL | Abfragesprache, mit der man genau die benötigten Felder anfordert | teilweise (**nur für lesende Abfragen**) |
| WASM | Führt Rust-Code direkt im Browser aus | nein (Technik für die Oberfläche) |

- **APIs, die von Browseroberflächen oder per `curl` aufgerufen werden, verwenden Connect** (POST; JSON wird mit RS-JSON gelesen und geschrieben). WASM im Browser kann kein reines gRPC sprechen. Mit Connect bleibt es bei einem gewöhnlichen `fetch`, und `prost` ist nicht nötig – die WASM-Datei wird dadurch nicht größer.
- **Für die Kommunikation zwischen Diensten (Server zu Server, rechenintensive Aufgaben, Streaming) wird gRPC eingesetzt.** In den meisten Fällen reicht Connect aus; auf gRPC wird nur dort umgestellt, wo Streaming tatsächlich gebraucht wird.
- **GraphQL dient ausschließlich lesenden Abfragen**, wenn mehrere Informationen gebündelt abgefragt werden sollen. Für Schreibzugriffe (Operationen) wird es nicht verwendet.
- **WASM bleibt die Technik für die Oberfläche.** Geändert hat sich lediglich, dass die aufgerufene API nun auf Connect beruht.
- Schnittstellen, deren Form durch einen externen Standard festgelegt ist (ACME, OAuth-Callbacks, Webhooks von Drittanbietern, Health-Checks, statische Dateien, OTLP), bleiben gewöhnliches HTTP.
- **Im Zweifel: Connect.**

### Русский (перевод выполнен с помощью ИИ и литературно отредактирован; носителями языка пока не проверен)
**Это не варианты, из которых нужно выбрать один, а технологии разных уровней.** Заменить REST могут только Connect, gRPC и GraphQL; WASM же — это способ создания пользовательского интерфейса.

| Термин | Что это | Заменяет REST? |
|---|---|---|
| REST | Стиль построения API, при котором операции выражаются через URL и HTTP-методы | отменяется |
| **Connect** | `POST /Сервис/Метод` с телом в формате JSON; вызывается как из `curl`, так и из браузерного `fetch` | да (**выбран**) |
| **gRPC** | Тот же принцип «сервис — метод», но поверх двоичного формата protobuf и HTTP/2; работает быстро и хорошо подходит для потоковой передачи | да (**для связи между сервисами**) |
| gRPC-web | Адаптированный для браузера вариант gRPC | да (при необходимости) |
| GraphQL | Язык запросов: клиент запрашивает ровно те поля, которые ему нужны | частично (**только запросы на чтение**) |
| WASM | Позволяет выполнять код на Rust прямо в браузере | нет (технология для интерфейса) |

- **API, которые вызываются из браузерных интерфейсов или через `curl`, строятся на Connect** (POST; JSON читается и записывается через RS-JSON). Браузерный WASM не умеет работать с «чистым» gRPC. С Connect достаточно обычного `fetch`, а `prost` не требуется — размер WASM-файла не увеличивается.
- **Для обмена между сервисами (сервер — сервер, ресурсоёмкие задачи, потоковая передача) применяется gRPC.** В большинстве случаев достаточно Connect; на gRPC переходят только там, где потоковая передача действительно необходима.
- **GraphQL используется только для чтения**, когда нужно получить сразу несколько связанных данных. Для записи (операций) он не применяется.
- **WASM по-прежнему остаётся технологией для интерфейса.** Изменилось лишь то, что вызываемый им API теперь построен на Connect.
- Интерфейсы, форма которых задана внешним стандартом (ACME, обратные вызовы OAuth, вебхуки сторонних сервисов, проверки работоспособности, статические файлы, OTLP), остаются обычным HTTP.
- **Если есть сомнения — выбирайте Connect.**

## API(2026-11-05〜): REST全廃・Connect
公開APIは `POST /rs_sync.Sync/<Method>`(`content-type: application/json`、`id`/`repo`は本文、エラーは`{"code","message"}`)のみ。例外(規格どおり): `/healthz`・`/healthz/scheduler`・`/`・`/api/auth/google/callback`(GoogleのOAuth)。旧`/api/*`のRESTは削除済み。メソッド一覧は`src/connect.rs`。

## 追記(2026-11-16) スケジュール拡充・Googleカレンダー連携・アカウント設定・認証アプリログイン

- **スケジュール**: 毎週(曜日ごとに時刻)、毎月カレンダー(今月/来月・24時間表記・「毎日」チェック)、1年分の特定日(`specific_dates`、後から変更可)に対応。判定は`is_due_schedule`、テスト追加済み。
- **Googleカレンダー連携**: OAuth2(scope `calendar.events`、非公開予定)。同期予定の登録と、スケジュール解除時の「解除しました」予定(日英・日時つき)を自動作成。Googleドライブは中止。**利用者自身のGoogle CloudのClient ID/Secretが必要**で、実OAuthの通し動作は資格情報が無く未検証(状態応答の形のみ確認)。
- **アカウント設定**: 管理者メールの変更、予備メール(OTPを受け取れる2つ目)、電話番号(参照用のみ。SMS・ログインには未使用)。電話番号未登録でメールログイン中の利用者には登録を促す案内(日英)。
- **認証アプリでのログイン**(TOTP、RFC 6238自前実装): 「2段階認証」ではなく、**メールOTPまたは認証アプリのコードの"どちらか"でログインできる別経路**。UI表記を訂正済み(旧「2段階認証/2FA」→「認証アプリでのログイン」)。
- **UI/キャッシュ**: `Cache-Control: no-cache, must-revalidate`+一度だけの強制再読込、ブラウザ自動入力の抑止、間隔の「変更を保存」ボタン、データ量に応じたプリセットの灰色表示、タイムゾーン/国表示。
- **既知の制限**: 一度も実行していないジョブは次回同期予定を推定できない、旧キャッシュの画面は一度だけ手動の強制再読込が要る、`monthly_days`はバックエンドに残るがUIは`specific_dates`を書く。
- **未了**: Google OAuthの実機確認(資格情報待ち)、本節の表記訂正のVPS反映(別途デプロイ)。


## 開発状況・再開 / Status and resume (2026-11-15)

**日本語**: 依存は最新の主要版へ更新済み・テスト全通過・Androidのクロスビルド成功。未了は、稀に落ちる双方向同期の結合テストの原因特定とAndroid実機の起動確認。再開用メッセージは[`PORTING.md`](PORTING.md)末尾。

**English**: Dependencies are on the latest major versions, all tests pass and the Android cross-builds succeed. Open: root cause of the occasionally failing two-way sync integration test, and a real-device launch check. The resume message is at the end of [`PORTING.md`](PORTING.md).

## ログイン方式 / Login modes (2026-11-18)

**日本語**: 管理画面のログイン方式を設定パネルで選べます。`otp`(メールOTPのみ・既定)、`otp_qr`(メールOTP+QR撮影の2段階認証・推奨)、`qr`(QR撮影のみ・認証アプリ有効時のみ。秘密を持たないため弱い方式)。QRは3分・1回限りのURLで、撮影した端末でページが開くと自動で確認されます。「パスワード無し」は同期先の資格情報を扱うため実装していません。詳細は`PORTING.md`末尾。

**English**: Pick the admin login mode in the settings panel: `otp` (email OTP only, default), `otp_qr` (email OTP + QR scan, a real 2-step login; recommended), `qr` (QR scan only; needs the authenticator app and is weak because it holds no secret). The QR is a URL valid for 3 minutes and one use; the scanning device confirms automatically when the page opens. "No password" is not implemented because this app holds sync credentials. See the end of `PORTING.md`.

## 2026-11-20 同期状態の保存を確実に(ときどき落ちる試験への対策) / Reliable sync-state saving (fix for the occasionally failing test)

### 日本語
**現状(できたこと)**
- 双方向同期では、前回そろえたときのコミット番号を `data/state/<job>.json` に保存し、次の回は「どちらが変わったか」をこの番号と比べて決めます。
- これまでは、保存に失敗しても**何も言わずに無視**していました(`let _ = fs::write(..)`)。Windows ではウイルス対策などが一瞬ファイルをつかむことがあり、保存や読み込みに失敗すると、次の回は「前回の記録なし」と見なされます。すると、送信先だけを変えたのに「両方が変わった=衝突(CONFLICT)」と誤って判断していました。これが、ときどき落ちていた試験 `two_way_sync_propagates_dest_only_change_back_to_source` の、いちばん考えられる原因です。
- 直したこと: 保存は「一時ファイルに書く → 名前を変えて置き換える」方式にし、失敗したら最大10回まで少し待ってやり直します。それでも保存できなければ、同期の結果を**失敗として報告**します(黙って記録を失わない)。読み込みも、ファイルがあるのに一時的に読めないときはやり直します(ファイルが無いときは初回として扱う)。
- 試験: 新しい単体試験(保存→読み込みが一致し、一時ファイルが残らない)を追加。全109件(86+19+4)が成功。問題の試験は25回続けて実行して25回とも成功しました。
- **正直な注意**: この PC では、修正前のコードでも失敗を再現できませんでした。そのため「原因を確定した」とは言えません。いちばん疑わしい所を直した、という状態です。

**完成予定(これから)**
- 実スマホのカメラで QR を撮るログインの通し試験(人の操作が必要)。
- 実際の SMTP で `otp_qr` の通し試験。
- 本番 VPS への今回の修正の反映は、次のリリースで行います(本番は Linux なので、この不具合は起きにくい)。

### English
**Current state (done)**
- Two-way sync stores the commit id from the last successful sync in `data/state/<job>.json`. The next run compares against it to decide which side changed.
- Until now, a failed save was **silently ignored** (`let _ = fs::write(..)`). On Windows, antivirus or indexing can hold a file for a moment. When a save or read failed, the next run saw "no previous record" and wrongly reported a dest-only change as a CONFLICT (both sides changed). This is the most likely cause of the occasionally failing test `two_way_sync_propagates_dest_only_change_back_to_source`.
- The fix: the state is written to a temp file and then renamed into place, with up to 10 retries and short waits. If it still cannot be saved, the run is **reported as failed** instead of silently losing the baseline. Reads are retried too when the file exists but cannot be read for a moment (a missing file still means "first run").
- Tests: a new unit test checks that save and load round-trip and that no temp file is left behind. All 109 tests pass (86 + 19 + 4). The flaky test passed 25 out of 25 repeated runs.
- **Honest note**: on this PC the failure could not be reproduced even with the old code, so the cause is not proven. We fixed the most likely culprit.

**Planned (next)**
- An end-to-end QR login test with a real phone camera (needs a person).
- An `otp_qr` run with real SMTP.
- Deploying this fix to the production VPS with the next release (production runs on Linux, where this problem is unlikely).

## 2026-11-20 追記: RS-Sync の同期状態の修正を本番へ反映 / Update: RS-Sync state fix deployed to production

### 日本語
- `d73c2c1`(同期の記録を確実に保存する修正)を、VPS と同じ glibc 2.39 の WSL `Ubuntu-24.04` で作り(`vps-build.sh`、検査OK)、VPS の `rs-sync`(本番)と `rs-sync-demo`(デモ)を入れ替えました。2つは同じプログラムを使っています。
- 旧版は `/root/rs-sync/target/release/rs-sync.before-state-20261120` に残しました。失敗したら自動で元に戻す手順付きで、戻す必要はありませんでした。
- 確認したこと:
  - 本番: `GetLoginMode` 200、認証なしの `ListAccounts` 401。
  - デモ: `GetLoginMode` 200、`ListAccounts` 200。
  - 旧 `/api/accounts` は 404。公開URL 200、`/healthz` 200、エラーログ0、足りないライブラリ0。
- これで「本番への反映」は完了です。残りは、実スマホのカメラでの QR ログインと、実際のメールでの `otp_qr` の通し試験(人の操作が必要)です。

### English
- `d73c2c1` (reliable sync-state saving) was built in WSL `Ubuntu-24.04` with glibc 2.39, the same as the VPS (`vps-build.sh`, check OK). It was swapped into `rs-sync` (production) and `rs-sync-demo` (demo) on the VPS; both use the same binary.
- The old binary is kept at `/root/rs-sync/target/release/rs-sync.before-state-20261120`. The swap had automatic rollback on failure, and no rollback was needed.
- Verified:
  - Production: `GetLoginMode` 200; unauthenticated `ListAccounts` 401.
  - Demo: `GetLoginMode` 200; `ListAccounts` 200.
  - Old `/api/accounts` 404; public URL 200; `/healthz` 200; error log 0; no missing libraries.
- The production deploy is done. Still open: a QR login with a real phone camera and an `otp_qr` run with real mail (both need a person).


## 2026-11-26 `.v1` 即削除・版なし名へ統一 / `.v1` removed immediately, versionless names only

### 日本語
- ユーザー指示(2026-11-26)により、自前APIの版つき名(`rs_sync.v1.…`)は**猶予なしで削除**し、版なし名(`rs_sync.Sync/<Method>`)だけにした。旧名は404を返す(試験 `v1_names_are_gone_versionless_works` で確認)。画面・試験・文書・メール文面の呼び出しも版なし名へ書き換え済み。
- 第三者の規格名(`grpc.health.v1` 等)は対象外で触っていない。
- 本番(VPS)への反映・実測は、このコミットの後の記録(下の「本番反映」行)を参照。反映前は旧名が本番で動いている。
- **再開用メッセージ(別アカウントでそのまま貼り付け)**: 前の続きです。`RS-Sync` の README/CLAUDE/PORTING の「2026-11-26 `.v1` 即削除」を読み、(1)`vps-build.sh`(Ubuntu-24.04)でビルドし `check-glibc.sh` を通す、(2)VPSの該当サービスを `*.before-v1del-20261126` にバックアップして入れ替える、(3)新名200/401・旧名404・`/healthz` 200・エラーログ0を実測、(4)結果をこの節に追記してpush。VPSのcloneは `git pull` せず fetch+reset。関連リポジトリ: RS-EC・RS-Blog・RS-Sync(RS-Opsは必要なら)・`runo`。秘密は出さない、VPSでビルドしない、新しいRESTは作らない。

### English
- Per the user's instruction (2026-11-26), the versioned names of our own API (`rs_sync.v1.…`) were **deleted immediately with no grace period**; only the versionless names (`rs_sync.Sync/<Method>`) remain. Old names return 404 (test `v1_names_are_gone_versionless_works`). Callers in the UI, tests, docs and mail text were rewritten to the versionless names.
- Third-party standard names (`grpc.health.v1` etc.) are out of scope and untouched.
- For the production swap and measurements see the later "production" note below; before that, production still serves the old names.
- **Resume message (paste as is on another account)**: Continuing from before. Read "2026-11-26 `.v1` removed immediately" in the README/CLAUDE/PORTING of `RS-Sync`, then (1) build with `vps-build.sh` (Ubuntu-24.04) and pass `check-glibc.sh`, (2) back up the VPS service as `*.before-v1del-20261126` and swap it, (3) measure new names 200/401, old names 404, `/healthz` 200, error log 0, (4) append the result to this section and push. Never `git pull` VPS clones (fetch+reset). Related repos: RS-EC, RS-Blog, RS-Sync (RS-Ops if needed), `runo`. Never print secrets, never build on the VPS, no new REST.
