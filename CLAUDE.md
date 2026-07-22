# 開発方針＆開発環境ルール(RS-Sync)

作業ドライブは`F:\runo`。この節は[`open-raid-z`](https://github.com/aon-co-jp/open-raid-z)の
`CLAUDE.md`を正本とし、各プロジェクトへコピーして同期する方針に準じる。
GitHubリポジトリ: [aon-co-jp/RS-Sync](https://github.com/aon-co-jp/RS-Sync)。

> ⚠️ **正直な開示(2026-07-22、初版)**: GitHubプロバイダのみ実機
> (実HTTP・実git push)で検証済み。RS-Git/Gitea/Gitbucketはドキュメント
> ベースの実装のみで実機未検証(README参照)。双方向同期のコンフリクト
> 検知はデフォルトブランチのHEAD sha比較のみ(ブランチ単位ではない)。
> Webhookによるリアルタイム検知は未実装(ポーリングのみ)。

## このプロジェクトの役割

VPS上の既存`/root/sync-repos.sh`(cron、aon-co-jp組織の全リポジトリを
バックアップ用GitHubアカウント+自前RS-Gitサーバーへ一方向ミラーする
固定スクリプト)を土台に、汎用のGitリポジトリ同期WEBアプリとして
作り直したもの。GitHub / RS-Git / Gitea / Gitbucket間で、アカウント登録・
同期ジョブ設定(一方向/双方向)・スケジュール設定・手動実行ができる。

## 技術スタック

RS-Git/RPoem等と同じ方針: `poem`クレートを直接利用する単純なHTTP
サービス。DB非依存、JSON永続化(`data/store.json`、gitignore対象)。
実際のgit転送は`git`コマンドをシェルアウトする方式(`clone --mirror`/
`push --mirror`)——既存の`sync-repos.sh`と同じアプローチ。

## 主要モジュール

- `src/model.rs` — `Account`/`SyncJob`/`Schedule`/`RunLogEntry`、
  `AppState`(JSON永続化)。
- `src/provider.rs` — `Provider`トレイトとGitHub/RS-Git/Gitea/Gitbucket/
  Local(テスト用)の各実装。git転送用のURL生成とREST API(一覧・作成)
  のみを担当。
- `src/sync_engine.rs` — 実際の`git clone --mirror`/`push --mirror`
  実行、一方向/双方向の分岐、双方向時のコンフリクト検知
  (`last_synced` shaとの比較)。
- `src/scheduler.rs` — 毎分ポーリングして「何時間ごと」「曜日+時刻」
  「リアルタイムポーリング間隔」のいずれかに該当するジョブを実行。
- `static/index.html` — 日英2言語UI(`data-i18n`属性+JS辞書切替)。
  APIパスは相対パス(`api/...`)——`/rs-sync/`のようなプレフィックス
  配下にマウントされても壊れないようにするため(RGit(現RS-Git)の`/rgit`デプロイ
  で絶対パスfetchが壊れた教訓を踏襲)。

## テスト

`tests/sync_integration.rs`——ローカルのbareリポジトリ2つ(モックではなく
実際の`git init --bare`/`clone`/`push`)を使い、一方向・双方向それぞれの
同期が実際にコミットを反映することを検証。`ProviderKind::Local`という
テスト/開発専用の擬似プロバイダ(実在のホスティングサービスではない、
ファイルパスを直接扱うだけ)を使用。

```
cargo test --test sync_integration -- --test-threads=1
```

(cwdをテスト内で変更するため`--test-threads=1`必須。)

## HANDOFF

- **2026-07-22 初版実装・実機検証・VPSデプロイ完了**:
  1. **実装**: プロバイダ抽象化(`Provider`トレイト)+GitHub実装フル、
     RGit/Gitea/Gitbucketは公開API仕様に基づく実装(未検証)。アカウント
     登録・同期ジョブ設定(一方向/双方向)・スケジュール(何時間ごと/
     曜日+時刻/リアルタイムポーリング)・手動実行(「今すぐ同期」)・
     日英UIを実装。双方向同期はデフォルトブランチHEAD shaの前回同期時
     との比較で「両側変更=コンフリクト」を検知し、該当リポジトリの
     自動実行を止める(force pushでの上書きはしない)。
  2. **実機検証**: `cargo test --test sync_integration -- --test-threads=1`
     で一方向・双方向とも green(2 passed)。ローカルのbareリポジトリへ
     実際に`git`コマンドでcommit→同期実行→destに反映されることを確認。
  3. **クロスコンパイル確認**: Windows開発機での`cargo build --release`
     (ネイティブmsvc)は成功。Linux(`x86_64-unknown-linux-gnu`)への
     クロスコンパイルはこのWindows機にクロスリンカーが無く失敗
     (`x86_64-linux-gnu-gcc`が見つからない)——正直に未対応と記録。
     代わりにVPS(`ssh conoha`)上で直接`cargo build --release`し成功
     (rustc/cargoは`/root/.cargo/bin/`に既存)。
  4. **インストーラー**: `install.sh`(Linux、rustup自動導入+
     `cargo build --release`+`systemctl --user`サービス登録の対話式)、
     `install.ps1`(Windows、winget経由のrustup導入+`cargo build
     --release`+起動用PowerShellスクリプト生成、常駐サービス化は
     Task Schedulerへの手動登録が必要と明記)。コード署名・msi/deb
     パッケージングは未実施と正直に開示。
  5. **VPSデプロイ**: `/root/runo.tokyo/rs-sync`にclone→
     `cargo build --release`成功→`rs-sync.service`(systemd、
     ポート8096)を作成・起動。`curl http://127.0.0.1:8096/healthz`→`ok`
     を確認。
  6. **nginx**: `/etc/nginx/conf.d/runo-tokyo-tls.conf`の443番SSL
     server blockに`location /rs-sync/ { proxy_pass
     http://127.0.0.1:8096/; ... }`+`/rs-sync`→`/rs-sync/`の301
     リダイレクトを追加(変更前の設定は`.bak-before-rs-sync`として
     退避済み)。UIのAPIフェッチを絶対パスから相対パス(`api/...`)へ
     変更済みなので、プレフィックス配下でも壊れない。
  7. **実機到達確認**: `curl https://runo.tokyo/rs-sync`→`301`、
     `https://runo.tokyo/rs-sync/`→`200`、
     `https://runo.tokyo/rs-sync/api/accounts`→`200 []`を確認。
  8. **runo.tokyo TOPページ**: `SUB_SERVICES`に`/rs-sync`エントリを
     追加(`F:\runo\runo.tokyo\src\lib.rs`)、`cargo test`20件green、
     VPS本番反映・`curl https://runo.tokyo/`のHTML内に"RS-Sync"文字列
     が含まれることを確認。
  9. **RGitのCLAUDE.md更新**: 「保留中の外部バックアップ同期スクリプト
     へのRGit組み込み」の記述を、RS-Syncが実際にRGitをプロバイダとして
     (未検証ながら)サポートしている旨に更新。
  10. **既存資産との関係**: VPS上の`/root/sync-repos.sh`(cron)は今回
      停止・削除せず、RS-Syncと並行稼働のままにしている。

  - 次にすべきこと: (1) RGit/Gitea/Gitbucketプロバイダの実機検証
    (特にRGitは自前サーバーがあるため優先度高)、(2) ブラウザでの
    実UI確認(Claude Browser paneでのクリック操作検証は未実施、
    curlでのHTML/JSON確認のみ)、(3) Webhookベースのリアルタイム検知、
    (4) ブランチ単位の双方向コンフリクト検知、(5) コード署名済み
    インストーラー・deb/msiパッケージング、(6)
    `/root/sync-repos.sh`のcronをRS-Syncへ最終的に一本化するかの判断。

## 関連プロジェクト

- [RS-Git](https://github.com/aon-co-jp/RS-Git) — 自前Git forge(旧RGit、
  2026-07-22リネーム)、RS-Syncのプロバイダの1つ
- [open-raid-z](https://github.com/aon-co-jp/open-raid-z) — 開発ルールの正本
