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
