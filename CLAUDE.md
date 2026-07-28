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
  配下にマウントされても壊れないようにするため(open-gitea(現RS-Git)の`/rgit`デプロイ
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

- **2026-07-23(続き) `.github/workflows/release.yml`を新規追加、
  3点セット完備・v0.1.0タグでCI成功・GitHub Release実在確認まで完了**:
  エコシステム全体インストーラー整備計画(正本: `open-raid-z/CLAUDE.md`
  「エコシステム全体インストーラー整備計画」節)の一環。`install.sh`/
  `install.ps1`は既存(ソースからのcargoビルド方式、変更無し)だった
  ため、`release.yml`のみ新規追加した。Linux x86_64・Windows x86_64
  向けにプリビルド済みバイナリをビルドし、`install.sh`/`install.ps1`と
  一緒にtar.gz/zipへ同梱、GitHub Releasesへ`softprops/action-gh-release@v2`
  で添付する構成(sibling path依存は無し、確認済み)。`v0.1.0`タグを
  実際にpushし、`gh run list`で2ジョブ(Linux/Windows)とも
  `completed success`、`gh release view v0.1.0`で
  `rs-sync-linux-x86_64.tar.gz`/`rs-sync-windows-x86_64.zip`の両方が
  実在することを確認した(型チェックのみでの完了報告ではない、
  上記「未実装」節のクロスコンパイル制約とは別に、CI環境
  〈ubuntu-latest/windows-latest ネイティブ〉では両OSとも問題なく
  ビルドできることも確認できた)。READMEにもプリビルド済みバイナリの
  入手先を追記(日英両方)。
  - 次にすべきこと: Android版インストーラー(未着手、他リポジトリと
    共通のバックログ)。

- **2026-07-22 初版実装・実機検証・VPSデプロイ完了**:
  1. **実装**: プロバイダ抽象化(`Provider`トレイト)+GitHub実装フル、
     open-gitea/Gitea/Gitbucketは公開API仕様に基づく実装(未検証)。アカウント
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

  - 次にすべきこと: (1) open-gitea/Gitea/Gitbucketプロバイダの実機検証
    (特にRGitは自前サーバーがあるため優先度高)、(2) ブラウザでの
    実UI確認(Claude Browser paneでのクリック操作検証は未実施、
    curlでのHTML/JSON確認のみ)、(3) Webhookベースのリアルタイム検知、
    (4) ブランチ単位の双方向コンフリクト検知、(5) コード署名済み
    インストーラー・deb/msiパッケージング、(6)
    `/root/sync-repos.sh`のcronをRS-Syncへ最終的に一本化するかの判断。

## HANDOFF追記(2026-07-27) `RGit`プロバイダを`OpenGitea`へリネーム

ユーザー指示「複数のGITHUBアカウントとopen-giteaとの同期をopen-easy-webに
登録のopen-giteaで簡単同期管理」への対応。前提として`RS-Git`(旧`RGit`)
リポジトリ自体が同日`open-gitea`へ改名されたため、RS-Sync側のプロバイダ
実装・UIも追従させた。

1. **`src/model.rs`**: `ProviderKind::RGit`→`ProviderKind::OpenGitea`。
2. **`src/provider.rs`**: `RGitProvider`/`RGitRepoEntry`→
   `OpenGiteaProvider`/`OpenGiteaRepoEntry`。実装自体(`GET /api/repos`・
   `PUT /repos/:name`を叩く)は無変更——**open-gitea側のAPIエンドポイントは
   改名の前後で変わっていないことをソースで確認済み**
   (`open-gitea/src/main.rs`の`.at("/api/repos", get(list_repos))`・
   `.at("/repos/:name", put(create_repo))`)。
3. **`static/index.html`**: プロバイダ選択`<select>`の`<option>RGit`を
   `<option>OpenGitea</option>`へ変更(このフォームは選択テキストを
   そのまま`provider`フィールドとしてPOSTし、serdeが`ProviderKind`の
   バリアント名文字列と照合するため、HTML側も必ず追従させる必要が
   あった——ここを見落とすと「表示は直ったがフォーム送信時に
   デシリアライズ失敗」という静かな不具合になる)。
4. **検証**: `cargo build`/`cargo test`成功(既存の2件の実git統合テスト
   green、回帰なし)。実際に本番稼働中の
   `https://easy-web.tokyo/open-gitea/api/repos`へ`curl`し、200・
   有効なJSON(`[]`)が返ることを確認——open-gitea側のAPIが実際に
   到達可能でRS-Syncが期待する形式と一致することを実機で確認した。
5. **正直な開示・未着手**: (1) RS-Sync UI上で実際にopen-giteaアカウントを
   登録→同期ジョブを作成→実際に1リポジトリをミラー同期するE2Eは
   今回未実施(API疎通確認止まり)。(2) open-easy-webの「外部ツール」
   セクションにはRS-Sync・open-gitea双方へのリンクは既に登録済みだが、
   「open-giteaへのGitHub複数アカウント同期をワンクリックで設定する」
   専用の統合UI(RS-Syncを起動せず、open-easy-web上で直接アカウント
   登録できるような)は存在しない——現状は「open-easy-webからRS-Syncを
   起動し、RS-Sync自身のUIでアカウント登録・同期ジョブ作成を行う」
   という2段階の運用フローのまま。
  - 次にすべきこと: (1) RS-Sync UIでの実アカウント登録→同期ジョブ作成→
    実ミラー同期のE2E確認、(2) 「open-easy-webから直接open-giteaの
    同期設定ができる」というユーザー要望により深く応える場合、
    open-easy-web側に专用の埋め込みUI/リダイレクト付きディープリンクを
    追加するかの設計検討(現状の「別タブでRS-Syncを開く」体験で
    十分かの確認を含む)。

## 関連プロジェクト

- [open-gitea](https://github.com/aon-co-jp/open-gitea) — 自前Git forge
  (旧名RGit→2026-07-22にRS-Gitへ改名→2026-07-27にopen-giteaへ再改名)、
  RS-Syncのプロバイダの1つ
- [open-raid-z](https://github.com/aon-co-jp/open-raid-z) — 開発ルールの正本

## HANDOFF追記(2026-07-27続き) VPS上でRS-Syncが削除済みディレクトリから実行され続けていた問題を発見・修正

`OpenGitea`リネーム反映のため`systemctl cat rs-sync.service`で
`WorkingDirectory=/root/runo.tokyo/rs-sync`を確認したところ、
実際にはそのディレクトリが**存在しなかった**(`ls`で見えない)。
`/proc/<pid>/exe`・`/proc/<pid>/cwd`を確認したところ、稼働中の
プロセスは`(deleted)`マーク付きのファイルハンドルを保持したまま
動き続けているゾンビ状態だったと判明(何らかの理由で作業ディレクトリ
自体が削除されたが、既に起動していたプロセスはメモリ上のバイナリ
イメージで動き続けていたため、外形上は`systemctl status`が
`active (running)`のまま正常に見えていた)。

**修正**: `git clone https://github.com/aon-co-jp/RS-Sync.git`で
`/root/runo.tokyo/rs-sync`を再作成し、`cargo build --release`で
再ビルド、`systemctl restart rs-sync.service`で新しいバイナリへ
切り替え。`curl https://runo.tokyo/rs-sync/`で`OpenGitea`が
選択肢に含まれることを実際に確認済み。

**正直な開示**: このディレクトリがいつ・なぜ削除されたかは不明
(このセッションで削除した記憶はなく、別セッションの作業や
手動オペレーションが原因の可能性がある)。今後、同様の「ゾンビ
プロセス」状態(`ls`にディレクトリが見えないのに`systemctl status`は
正常)を早期発見するため、デプロイ前に`/proc/<pid>/exe`の
`(deleted)`マークを確認する習慣を推奨する。
