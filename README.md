# RS-Sync

Git repository sync/backup tool across multiple git-hosting providers
(GitHub / RS-Git / Gitea / Gitbucket), built with Rust + [poem](https://github.com/poem-web/poem).
日本語の説明は下にあります。

## What it does (MVP)

- Register **accounts** (source/destination) for a git-hosting provider,
  storing name/API base/git base/owner/token in a local JSON file
  (`data/store.json`, gitignored — never committed).
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
| Gitbucket | Implemented against documented GitHub-compatible API | Implemented | Yes | No — not tested against a live Gitbucket instance |

All git transport (clone/push/fetch) is provider-agnostic — it is plain git
over HTTP(S), so any provider that speaks git smart-HTTP will work for the
actual sync even where the REST adapter above is unverified; only repo
discovery/creation depends on the adapter.

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
確認)。Gitea/Gitbucketは公開APIドキュメントに基づく実装のみで実機未検証。

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
