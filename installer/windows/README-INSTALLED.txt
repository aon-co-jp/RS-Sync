rs-sync (RS-Sync) — インストール後のご案内 / Post-install notes
================================================================

日本語
------
このフォルダには rs-sync.exe (単体実行ファイル) が配置されています。
起動すると既定で http://127.0.0.1:8095/ で待ち受けます。

- データは初回起動時に data\store.json へ自動生成されます。
- 自動アップデート機能は既定で無効です。有効化するには環境変数
  RS_SYNC_SELF_UPDATE=true を設定してから起動してください
  (src/self_update.rs のモジュールコメントに詳細な正直な開示あり —
  VPS本番のgit pull運用と衝突しないよう、明示的なオプトイン方式に
  しています)。
- 認証(OTPログイン)は既定で有効です。無効化するには
  RS_SYNC_REQUIRE_AUTH=false を設定してください(信頼できる環境専用)。

English
-------
This folder contains rs-sync.exe (a single self-contained executable).
By default it listens on http://127.0.0.1:8095/.

- Data is auto-created at data\store.json on first run.
- Automatic self-update is OFF by default. Set the environment variable
  RS_SYNC_SELF_UPDATE=true before launching to opt in (see the module
  doc in src/self_update.rs for the full honest disclosure — this is
  deliberately opt-in so it never races with the VPS's git-pull based
  deployment).
- OTP login is required by default. Set RS_SYNC_REQUIRE_AUTH=false to
  disable it (trusted/local environments only).
