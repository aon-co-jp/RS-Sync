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
