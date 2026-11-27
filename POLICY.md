# RS-Sync POLICY / 方針

## 日本語
- 同期の記録(前回そろえたコミット番号)は、失ったまま黙って進めない。保存・読み込みに失敗したら、やり直し、それでもだめなら失敗として報告する。
- 衝突(CONFLICT)と判断したリポジトリは自動で上書きしない。人が確認して解決する。
- 秘密(トークン・パスワード)は画面・ログ・エラーメッセージ・コミットに出さない(URL中の資格情報は `***` に置き換える)。
- 新しい REST は作らない。API は Connect(ブラウザ/`curl`)と gRPC(サービス間)。
- 開発・試験・デバッグは手元の PC で行い、VPS は配備と確認だけに使う。
- 完成と書くのは、試験と実機での確認が済んでから。再現できなかった不具合は「原因未確定」と正直に書く。

## English
- Never silently lose the sync record (the commit id from the last sync). Retry failed saves and reads, and report the run as failed if they still fail.
- Never overwrite a repository flagged as CONFLICT automatically; a person resolves it.
- Never show secrets (tokens, passwords) on screen, in logs, in error messages or in commits (credentials in URLs become `***`).
- No new REST. APIs use Connect (browser/`curl`) and gRPC (service to service).
- Develop, test and debug on the local PC; use the VPS only for deployment and checks.
- Say "done" only after tests and real-system checks. When a bug could not be reproduced, say honestly that its cause is not proven.

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

### 本番反映(2026-11-26) / Production (2026-11-26)
- **日本語**: `vps-build.sh`(Ubuntu-24.04、glibc検査OK)でビルドし、`*.before-v1del-20261126` にバックアップして入れ替え・再起動。実測: 新名 本番 ListAccounts 401(認証なし)・GetLoginMode 200、デモ 200、旧名(`.v1`) 404、`/healthz` 200、エラーログ0。ロールバックはバックアップを元の名前へ戻して再起動。上の「反映前は旧名が本番で動いている」は反映済みのため無効。
- **English**: Built with `vps-build.sh` (Ubuntu-24.04, glibc check OK), backed up as `*.before-v1del-20261126`, swapped and restarted. Measured: new names production ListAccounts 401 without auth and GetLoginMode 200, demo 200, old (`.v1`) names 404, `/healthz` 200, error log 0. Roll back by restoring the backup under the original name and restarting. The "production still serves the old names" remark above is obsolete now.

## Versionless API の方針 / Versionless API policy (2026-11-25)
**日本語**: このリポジトリのAPIは **Versionless**(URL・パッケージ名に `v1` 等を付けず、互換性を壊さず足すだけで進化させる)。欄は足すだけ・足す欄は省略可能・知らない欄は無視・名前変更は新名を足して旧を非推奨に(期限は既定で6か月以上、使用0を確認してから削除)・enumは未知の値を許容・安全上の緊急修正は例外。既存の `.v1` は呼び出し側を壊さないよう当面残し、版なしの別名を追加して段階的に畳む。詳細・出典は [`runo/API-POLICY.md`](https://github.com/aon-co-jp/runo/blob/main/API-POLICY.md) 末尾。適用はこれから順に行う。
**English**: This repository's APIs are **Versionless**: no `v1` in URLs or package names; evolve by adding only, without breaking compatibility. Only add fields, new fields are optional, ignore unknown fields, rename by adding the new name and deprecating the old (default deadline 6 months or more, remove only after confirming zero use), tolerate unknown enum values, emergency security fixes are the exception. Existing `.v1` names stay for now so callers do not break; a versionless alias is added and `.v1` is retired in stages. Details and sources: the end of [`runo/API-POLICY.md`](https://github.com/aon-co-jp/runo/blob/main/API-POLICY.md). Rollout is step by step.

## API 方針: REST 全廃・gRPC / Connect へ
**日本語**: このリポジトリでは **REST API を新規に作らない**。ブラウザ・`curl` から呼ぶ API は **Connect の単純呼び出し**(`POST /<パッケージ>.<Service>/<Method>`、`content-type: application/json`)、サービス間は **gRPC / gRPC-web**。GraphQL は読み取り専用。既存の REST は触るときに移し、移したら旧経路は消す。JSON は RS-JSON(`from_str_secure` / `parse_secure`)で読む。例外は相手の規格が形を決めるもの(ACME、OAuth/OIDC のコールバック、第三者の Webhook、ヘルスチェック、静的ファイル、OTLP)のみ。迷ったら Connect。正本は [`runo/API-POLICY.md`](https://github.com/aon-co-jp/runo/blob/main/API-POLICY.md)。
**English**: This repository **does not add new REST APIs**. APIs called from browsers or `curl` use **Connect unary calls** (`POST /<package>.<Service>/<Method>`, `content-type: application/json`); service-to-service calls use **gRPC / gRPC-web**. GraphQL is read-only. Existing REST is migrated when touched and the old route is removed. Read JSON with RS-JSON (`from_str_secure` / `parse_secure`). Exceptions are only shapes dictated by someone else's standard (ACME, OAuth/OIDC callbacks, third-party webhooks, health checks, static files, OTLP). When in doubt, use Connect. Source of truth: the link above.

## 完成まで作り切る方針 / Finish everything policy (2026-11-27)
**日本語**: 全リポジトリで、**未確認は自動で確認し、未実施は実施し、未了は完成させる**(確認を求めず進める)。使用頻度が低いと判断される機能や、**1から作り直す大規模開発になる内容でも、やめずに最後まで開発・TEST・DEBUGする**(別の方式で済ませない、後回しにしない)。進め方: 調査(英語・日本語の検索とGitHub) → 設計 → 実装 → 実機/実データで確認 → 失敗は原因を直して再試験 → README・CLAUDE・PORTING・POLICY を日英で更新 → push。「できた」と書くのは実測した範囲だけ、確認できなかった所は「未確認」と正直に書く。例外(確認を取る): 元に戻せない削除、秘密の公開、所有者の認証・決済・実機操作が要るもの、利用者のシステム設定の変更。
**English**: In every repository, **verify what is unverified, do what is not yet done, and finish what is unfinished** automatically, without asking. Even features that look rarely used, or work that means **a large rewrite from scratch, are developed, tested and debugged to the end** (no substituting another approach, no postponing). Method: research (English and Japanese search plus GitHub) -> design -> implement -> verify on real devices or data -> fix causes and retest on failure -> update README, CLAUDE, PORTING and POLICY in Japanese and English -> push. Write "done" only for what was measured, and say "unverified" honestly for the rest. Exceptions that still need confirmation: irreversible deletions, publishing secrets, anything needing the owner's authentication, payment or real-device actions, and changes to the user's system settings.