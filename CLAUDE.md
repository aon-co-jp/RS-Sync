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

## HANDOFF追記(2026-08-01) OpenGiteaプロバイダの実機再検証+空トークン時の実装統一(ユーザー指示「RSyncのVPSやレンタルサーバーのフォルダーやGithub、Gitbucket、Bitbucket、Gitlab、Gitea同士などの連携対応動作確認」)

同時並行タスクとして、各プロバイダの実際の疎通状態を再点検した。

1. **OpenGitea(実機で再検証)**: 本セッションで新規に本番稼働させた
   `https://easy-web.tokyo/open-gitea/`に対し、実際に`list_repos`
   (`GET /api/repos`)・`ensure_repo`(`PUT /repos/:name`)相当のHTTP
   リクエストを実行して確認した(モックではなく実サーバー)。
   - `GET /api/repos`(空Bearerトークン)→実際に`200 []`。
   - `PUT /repos/rs-sync-verification-test`(空Bearerトークン)→実際に
     `403 repository creation not permitted for this account`
     (`ensure_repo`の401/403判定ロジックが正しくエラーとして拾う
     ことを実際のレスポンスで確認)。
   - **発見**: `OpenGiteaProvider`は`GitHubProvider`が2026-07-29に
     修正済みの「空トークン時は`Authorization`ヘッダー自体を送らない」
     パターン(`maybe_auth_header`)を使っておらず、常に
     `Authorization: Bearer <token>`(空でも)を送っていた。open-gitea
     自身は現状これを許容する実装だが、GitHubで実際に踏んだのと同種の
     罠を将来再現するリスクがあるため、`list_repos`/`ensure_repo`とも
     `maybe_auth_header`経由に統一した。`cargo test --lib`36件全green
     (回帰無し)、VPS本番(`rs-sync`・`rs-sync-demo`)へデプロイ・
     再起動後も`/rs-sync/`・`/rs-sync/demo/`とも200のまま。
2. **Gitea/GitBucket/GitLab/Bitbucket(引き続き未検証、正直な開示)**:
   これら4プロバイダの実サーバーがこのセッションの環境には存在せず
   (実際にGitea/GitBucket/GitLab/Bitbucketサーバーを新規に構築する
   ことは本タスクのスコープを大きく超える別プロジェクト規模の
   インフラ整備が必要)、公開APIドキュメントに基づく実装のみで
   引き続き実機検証できていない。
3. **フォルダ同期(SFTP、VPS/レンタルサーバー間)**: 2026-07-29(続き5)の
   実装以降、実SFTPサーバーでの実機検証は依然として未実施のまま
   (ローカル↔ローカルの実I/O検証のみ済み、上記HANDOFF参照)。
- 次にすべきこと: (1) 実Gitea/GitBucket/GitLab/Bitbucketサーバーでの
  実機検証(インフラ整備が前提、別途優先度判断が必要)、(2) 実SFTP
  サーバーでのフォルダ同期実機検証。

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

## HANDOFF追記(2026-07-30続き) 【重大】トークン平文表示の実セキュリティ障害・デモの完全読み取り専用化・GitHubのhidden ref拒否エラー・OTPメールのフォントサイズ、計4件を対応

ユーザーが本番環境(`https://easy-web.tokyo/rs-sync/`)を実際に操作した
結果の画面出力から、**実際のGitHub PATが平文で本番UIに表示される
セキュリティインシデント**を含む複数の実障害が発覚した。

1. **【最優先・重大】同期ジョブ失敗時にトークンが平文表示される実障害**:
   `run_git()`が失敗時、gitコマンドの引数(`https://<token>@github.com/...`
   形式のリモートURL)と`git`の標準エラー出力(こちらもURLをそのまま
   エコーする)を無加工でエラーメッセージに含めており、ユーザーが実際に
   実行した同期ジョブの失敗ログに実GitHub PATがそのまま表示されていた
   (このセッションのチャットにも実トークンが貼り付けられる結果と
   なったため、**ユーザーには当該トークンをGitHub側で即座に失効させる
   よう案内済み**)。`redact_credentials()`(単体テスト5件)を新設し、
   `scheme://user:pass@`/`scheme://token@`形式の認証情報部分を`***`へ
   置換してから返すよう修正。本番・デモ両方に影響する修正のため、
   両サービスへ最優先でデプロイ・再起動した。
2. **デモ環境の完全読み取り専用化**(ユーザー指示「このデモでは実際には
   ログインもログアウトも登録も保存も出来ないようにして」): 新規
   `AuthState::read_only`+`ReadOnlyGuard`ミドルウェアを追加。
   `RS_SYNC_READ_ONLY=true`のとき、`/api/*`配下のGET以外の全リクエスト
   (ログイン/ログアウト自体を含む)をサーバー側で403拒否する——UIの
   見た目だけを制限するのではなく、直接APIを叩かれても実際に何も
   変更・保存できないことを保証する設計。`rs-sync-demo.service`に
   `Environment=RS_SYNC_READ_ONLY=true`を追加し、実際に
   `curl -X POST .../demo/api/accounts`が403で拒否されることを確認済み。
3. **`git push --mirror`がGitHubの「hidden ref」拒否で毎回失敗していた
   実障害**: `--mirror`は`mirror_fetch`が`+refs/*:refs/*`で取得した
   GitHub自身が管理する`refs/pull/*`(PR参照)まで送信先へpushしようと
   しており、GitHub側は`deny updating a hidden ref`で必ず拒否していた
   (ユーザーの本番実行結果で、多数のリポジトリがこのエラーで
   FAILEDになっているのを確認)。`refs/heads/*`・`refs/tags/*`のみを
   明示的に対象にする(`--prune`で宛先限定の古いブランチ/タグ削除は
   維持)よう`mirror_push()`を修正——`refs/pull/*`はそもそも外部から
   書き込めるものではなく、対象から除外するのが正しい設計。
4. **OTPログインメールのフォントサイズ拡大**(ユーザー指示「文字サイズは
   もっと二周り大きくして」): 従来プレーンテキストのみだった
   `send_otp`を、HTML+プレーンテキストの`multipart/alternative`
   (`build_otp_message`)へ変更。コード部分を地の文(15px想定)の
   約2.5倍(40px・太字・背景色付き)で表示、HTML非対応クライアント
   向けの従来通りのプレーンテキストも必ず併記。
- **検証**: `cargo test --lib`36件全green(新規7件: redact_credentials
  5件+build_otp_message 2件)、`cargo test --test sync_integration --
  --test-threads=1`4件全green(mirror_push変更後も回帰無し)。VPS本番
  (`rs-sync`・`rs-sync-demo`両サービス)へ`git pull`→
  `cargo build --release`→`systemctl restart`まで実施、デモの
  読み取り専用化・GET応答継続を実機確認済み。
- 次にすべきこと: 特になし(今回報告された4件はすべて対応・デプロイ・
  実機確認まで完了)。ユーザーには引き続き、チャットに貼り付けられた
  GitHub PATの失効を強く推奨する。

## HANDOFF追記(2026-07-30) デモ環境(認証不要)が実際には一切開けなかった実バグ2件を発見・修正・実機確認

ユーザー指示「easy-web.tokyo/rs-sync などで実際に確認したい」に対応して
実際にブラウザで開いたところ、`https://easy-web.tokyo/rs-sync/demo`
(デモ、`RS_SYNC_REQUIRE_AUTH=false`のはず)が**ログイン画面から一切
先に進めない**という実障害を発見した。

1. **実バグ1: UIがサーバーの認証要否を一切見ていなかった**:
   `syncAuthVisibility()`が`sessionToken()`(ローカルOTPトークン)の
   有無だけでログイン画面の表示/非表示を判定しており、サーバー側API
   (`GET api/accounts`)が実際にはトークン無しで200を返す(デモは
   認証不要)場合でも、UIは常にログインフォームのまま止まっていた。
   起動時にトークン無しで`api/accounts`を1回叩き、200が返れば
   ログイン画面をスキップする`probeAuthRequirement()`を追加。
2. **実バグ2(根本原因): 末尾スラッシュ無しパスでの相対API解決ミス**:
   バグ1を直した直後、`https://easy-web.tokyo/rs-sync/demo`
   (末尾スラッシュ**無し**)ではまだ直らず、`/rs-sync/demo/`
   (末尾スラッシュ**あり**)でのみ直ることが実際のブラウザ検証で判明。
   原因はブラウザの相対URL解決の標準仕様——`fetch("api/accounts")`が
   末尾スラッシュ無しの`/rs-sync/demo`から解決されると、"demo"が
   ファイル名扱いされ`/rs-sync/api/accounts`(本番、認証必須)に
   誤解決されていた。`<head>`の最初(他のスクリプトが動く前)で、
   末尾スラッシュが無ければ即座に補ってリダイレクトする処理を追加し
   解消。
3. **検証**: Claude Browser paneで実際に本番URL・デモURL(末尾スラッシュ
   あり/なし両方)を開き、(a) 本番はOTPログイン画面のまま(想定通り)、
   (b) デモは末尾スラッシュの有無に関わらず自動的にアプリ本体(クイック
   設定・アカウント一覧・ジョブ一覧・フォルダ同期セクション+「これは
   デモ環境で実トークンを入力しないでください」の警告文)まで到達する
   ことを実際に確認した。VPS本番(`/root/rs-sync`、`rs-sync`/
   `rs-sync-demo`両サービスが同じ`WorkingDirectory`を共有するため
   1回のデプロイで両方に反映)へ`git pull`→`cargo build --release`→
   両サービス`systemctl restart`まで実施済み。
- 次にすべきこと: 特になし(この2件は完全に解消——ユーザーからの
  「実際に確認したい」という要望が、修正後の実地検証まで含めて
  達成された)。

## HANDOFF追記(2026-07-29続き7) `sync_integration.rs`完成、双方向同期の実バグ2件を発見・修正・VPS本番反映

ユーザー指示「sync_integration.rsを完成させて」への対応。ビルド破損の
修正(続き6)だけでは不十分だったため、既存テストが実際にはカバーして
いなかった双方向同期の2シナリオ(dest→source方向の変更伝播、両側変更時の
コンフリクト検知)のテストを新規に書いたところ、**いずれも実際に失敗し、
2件の本物のバグを発見した**——テストを書いたこと自体が実際に価値を
生んだ実例。

1. **`head_sha()`の実バグ(コンフリクト検知が実質機能していなかった)**:
   `git rev-parse HEAD`をそのまま使っていたが、bareリポジトリの`HEAD`
   symrefは`git init --bare`実行環境の`init.defaultBranch`(伝統的には
   `master`)を指したままになりやすく、このコードベースが実際に
   push/pullするブランチ(`main`)と一致しない場合、`git rev-parse HEAD`は
   **エラーにならず(exit 0)、未解決の文字列`"HEAD"`をそのまま返す**
   というgitの既知の挙動を踏んでいた。この結果、`src_sha`が常に
   同じ固定文字列`"HEAD"`になり、`last_synced`との比較(変更検知・
   コンフリクト検知の根幹)が事実上無意味になっていた——**2回以上
   実行される双方向ジョブでは、コンフリクト検知機能そのものが
   ずっと動いていなかった可能性がある**。dest側の比較では既に
   `refs/heads/main`→`refs/heads/master`のフォールバックが実装
   されていたが、src側の`head_sha()`には同じ配慮が無かったという
   非対称なバグ。dest側と同じフォールバックパターンをsrc側にも適用し
   修正。
2. **dest→source方向の伝播が無言のno-opだった実バグ**: `dst_changed`
   分岐が、`mirror_push(&cache, &src_url)`を呼ぶ前に**dest側の実際の
   変更を`cache`へ取り込んでいなかった**。`cache`はループ冒頭の
   `mirror_fetch(&cache, &src_url)`によって常にsource側の状態を
   ミラーしたままであり、dest比較用の`refs/dest/*`名前空間への
   フェッチは比較にしか使われず、実際にpushされる`cache`の通常の
   refには一切反映されていなかった。そのため「distだけを変更→
   同期実行」は**エラーも出さずに何も起きない**(sourceの古い状態を
   source自身へpushし直すだけの実質no-op)という気づきにくい実障害
   だった。`mirror_push`の直前に`mirror_fetch(&cache, &dst_url)`
   (destを本当の意味でミラー取得し直す)を追加して修正。
3. **新規テスト2件**(実git bareリポジトリでの実I/O、モック無し):
   `two_way_sync_propagates_dest_only_change_back_to_source`・
   `two_way_sync_detects_conflict_when_both_sides_diverge_and_then_skips_it`
   (両側変更→`conflicted_repos`へ記録・force pushで上書きしない・
   以後のrunでスキップされる、という安全性の核心を実証)。
4. **検証・デプロイ**: `cargo test --lib`29件+
   `cargo test --test sync_integration -- --test-threads=1`4件、
   全green。VPS本番(`/root/rs-sync`)・デモ(`/root/rs-sync-demo`、
   同一バイナリを共有)の両方へ`git pull`→`cargo build --release`→
   `systemctl restart`まで実施し、`/healthz`が`ok`を返すことを確認。
   この修正は既存の双方向同期ジョブの挙動に直接影響するため、本番
   反映まで完了させた。
- 次にすべきこと: 本番で実際に稼働中の双方向ジョブがあれば、次回の
  実行タイミングで(コンフリクト検知が正しく働くようになったことで)
  これまで見過ごされていた本物のコンフリクトが新たに検出される
  可能性がある——挙動が変わったことをユーザーに周知しておくとよい。

## HANDOFF追記(2026-07-29続き6) mail.rsへの単体テスト追加+`tests/sync_integration.rs`のビルド破損を発見・修正

未着手・未完成項目の巡回開発の一環。

1. **`mail.rs`にテストが1件も無かった問題に対応**: 実SMTP資格情報が
   無い環境では`build_and_send`(実送信)自体は検証できないが、
   メッセージ組み立てロジックを`build_message`として純粋関数に切り出し、
   単体テスト5件を追加(正常系・`from`/`to`不正値での`MailError::Build`・
   `MailError`のDisplay文言・`SmtpConfig::from_env`が必須環境変数欠如時
   に`None`を返すこと)。
2. **`tests/sync_integration.rs`が実際にビルドできなくなっていた実バグを
   発見・修正**: 過去のパスで`Account`構造体に`token_expires_at`/
   `expiry_warning_sent`フィールドが追加された際、この統合テストの
   フィクスチャ(`make_account`)が追従しておらず、`cargo test --test
   sync_integration`自体が`E0063`(フィールド不足)でコンパイルエラーに
   なっていた(今回の変更とは無関係な既存バグ、この巡回で発見)。
   フィールドを追加して修正、`cargo test --test sync_integration --
   --test-threads=1`2件green(実git bareリポジトリでの一方向/双方向
   同期を実際に検証)に復帰。
3. **検証**: `cargo test --lib`29件全green(新規5件含む)、統合テスト
   2件green、リグレッション無し。
- 次にすべきこと: 前回HANDOFF(続き5)記載の(1)実SFTPサーバーでの実機
  検証、(2)実トークン期限切れシナリオでの通知メール実地確認、は
  いずれもユーザー操作(実サーバー・実受信確認)を要するため未着手の
  まま。

## HANDOFF追記(2026-07-29続き5) フォルダ同期(SFTP)+通知機能(同期開始・認証エラー・トークン期限)+ハートビート横展開

リミット接近のため簡潔に記録。

1. **フォルダ同期を新規実装**(`src/folder_sync.rs`、ユーザー指示
   「レンタルサーバーやVPSやローカルドライブやスマホやNASドライブなど
   同士でCOPYして同期を取れるシステムも搭載して」): gitリポジトリ非依存の
   一方向フォルダコピー。接続方式はSFTP(ユーザー確認済み)+ローカル
   ファイルシステム。既存のスケジューラ・認証・トークン非公開パターンを
   踏襲。SFTP自体はこの環境に実サーバーが無く未検証(ローカル↔ローカルは
   実I/Oで検証済み)。
2. **通知機能を新規実装**(`scheduler.rs`、ユーザー指示「同期を取り始める
   時とパスワードが違ったり変わったりやトークン切れのタイミングで
   サイト上でもe-mailにもお知らせ機能」): 初回同期開始・認証エラー
   (401/403等のヒューリスティック検出)・トークン期限7日前(ユーザーが
   任意入力する期限日、GitHub等はAPIで期限を返さないため自動取得不可と
   正直に開示)でメール通知。サイト上は既存のエラー表示(`auth_error`
   ステータスを新設し`error`と区別)がそのまま通知として機能する。
3. **open-web-server/open-easy-webへのハートビート横展開**(RS-Sync発の
   スケジューラ無言停止バグの教訓): 両リポジトリの主要バックグラウンド
   ループにタイムアウト+ハートビート+`/healthz/*`エンドポイントを追加
   済み(詳細は各リポジトリのCLAUDE.md参照)。
4. **open-easy-webの`dist_sync`がVPS専用ではなく元々PC/タブレット/
   スマホ/NAS等の汎用SFTP接続先に対応済みと判明**——UI文言をそのように
   修正(新規実装ではなく既存機能の正確な説明への修正)。
5. **open-easy-webにインストール/アンインストール時のデータ移行機能を
   追加**(`scripts/data-portability.sh`+`install.sh`/新設`uninstall.sh`):
   ローカルtar.gz・GitHubリポジトリ・rclone(Googleドライブ等、OAuth代行
   はしない既存方針通り)の3方式。
  - 次にすべきこと: (1) 実SFTPサーバーでのフォルダ同期の実機検証、
    (2) 実トークン期限切れシナリオでの通知メール実地確認、(3) open-easy-web
    のuninstall.ps1(Windows版)は未作成(install.ps1のみ既存、今回は
    .sh版のみ新設)。

## HANDOFF追記(2026-07-29続き4) 実バグ修正: 空トークンアカウントで401 + RustJSONへの移行

ユーザーが実際にクイック設定GUIで`aon-co-jp`(公開リポジトリのみ、トークン
未入力)→`OpenRuno`のジョブを作成したところ`error`ステータスになり、
本番の`data/store.json`を直接確認した上での質問「RJSONを使って開発した
のではないのですか?」への対応(RustJSON移行+ちょうど見つかった実バグの
修正を同じパスで対応)。

1. **実バグの原因**: `store.json`を直接読んだところ`master-aon-co-jp`
   アカウントの`token`が空文字列(UIの「公開リポジトリのみなら空欄可」
   ヒント通りの入力)。しかし`GitHubProvider`は空トークンでも
   `Authorization: token `(空値)ヘッダーを常に送信しており、GitHub API
   はこれを匿名アクセスへフォールバックさせず`401 Unauthorized`で
   拒否していた(ヘッダー自体を送らなければ匿名アクセスとして扱われる
   ことを`curl`で実際に確認: `GET /users/aon-co-jp/repos`は無認証で
   `200`)。
2. **修正**: `maybe_auth_header()`ヘルパーを新設し、トークンが空の場合は
   `Authorization`ヘッダー自体を付与しないよう`GitHubProvider`の
   `list_repos`/`ensure_repo`全呼び出し箇所を修正。`remote_url()`も
   空トークン時は`https://<token>@`のプレフィックス自体を省略するよう
   修正(空ユーザー名+`@`が一部のgit/HTTPクライアントの組み合わせで
   混乱を招くため)。新規テスト4件(`provider::tests`)で回帰確認。
3. **RustJSONへの移行**(ユーザー指示、他リポジトリとの一貫性のため):
   `open-redmine`の`src/rustjson.rs`(緩い構文のJSONパーサー、パース
   結果は標準`serde_json::Value`)をそのまま移植。`model::AppState::new`
   の読み込みを`serde_json::from_str`から`rustjson::parse_typed`へ変更
   (書き込みは引き続き`serde_json::to_string_pretty`、可読性維持)。
   `thiserror`依存を追加。新規テスト3件(`model::tests`、既存の標準JSON
   ファイルの後方互換読み込み・緩い構文での読み込み・ファイル未存在時の
   既定値フォールバックを確認)。
4. **検証(実測)**: `cargo test --lib`17件全green(新規7件)、
   `cargo test --test sync_integration -- --test-threads=1`2件green
   (回帰無し)。ネットワークレベルでも`curl`で
   `https://api.github.com/orgs/aon-co-jp/repos`→`404`(aon-co-jpは
   Organizationではなく通常のUserアカウント、既知の事実の再確認)・
   `https://api.github.com/users/aon-co-jp/repos`→`200`(無認証)を確認。
   VPS本番・デモ両方へデプロイ済み。
5. **正直な開示**: 実際に本番で作成された`aon-co-jp -> OpenRuno`
   ジョブを、この修正後に再実行して成功することの確認はユーザー自身の
   操作(「今すぐ同期」クリック)が必要——本番はログイン必須のため
   エージェント側からは実行できない。
  - 次にすべきこと: ユーザーが本番で該当ジョブを再実行し、成功する
    ことを確認すること。

## HANDOFF追記(2026-07-29続き2) クイック設定GUI+ダークテーマの可読性修正(ユーザー指摘対応)

1. **クイック設定パネルを新規追加**(ユーザー指示「aon-co-jpをマスターとして
   OpenRunoにCOPYしながら同期、などの指定を複数でも可能にする簡単なGUI」):
   マスター組織のURL/名前+任意の読み取りトークン、コピー先を複数行
   追加可能(各行URL/名前+書き込みトークン)、チェック間隔(15/30/60分/
   手動のみ)を1つのフォームで指定すると、既存の`/api/accounts`・
   `/api/jobs`をそのまま使って(バックエンド変更無し)マスター用アカウント
   1件・コピー先ごとのアカウント・一方向/全リポジトリの同期ジョブを
   まとめて作成する。「マスターに変化があれば自動でコピー」という
   ユーザー要件通り、既定でチェック間隔30分の自動実行付きで作成される。
2. **ダークテーマの可読性修正**(ユーザー指摘「文字が灰色/紺色で読みにくい」):
   `<a>`タグに明るい青(`#5fb3ff`)の明示色を追加(従来はブラウザ既定色
   〈暗い青〉のまま、暗い背景色`#0b0e14`との組み合わせで読みにくかった)。
   デモ環境案内文・ログインフォームの説明文/ラベルは`.bright`
   (白色)クラスへ変更。**同種の問題がopen-easy-web側にも実在すると気づき
   横展開で修正**(`index.html`に`a { color: var(--accent); }`を追加、
   テーマ変数`--accent`は元々light/dark両対応で定義済みだったが`<a>`タグに
   一度も適用されていなかった)。
3. **検証**: `cargo build`/`cargo test`回帰無し(静的HTML/CSSのみの変更)。
   実ブラウザ(デモ環境、ログイン不要)でクイック設定フォームのDOM存在・
   コピー先行追加ボタン・スケジュール選択肢を確認、色修正後は
   `getComputedStyle`で実際に`rgb(255,255,255)`(白)・
   `rgb(95,179,255)`(明るい青)になっていることを確認済み。
  - 次にすべきこと: クイック設定パネルを実際にユーザー自身のGitHub
    トークンで使い、`aon-co-jp`→`OpenRuno`のような実際のorg間コピー同期を
    フルE2Eで確認すること(ユーザー操作が必要)。

## HANDOFF追記(2026-07-29) 実バグ発見・修正: RS-Syncに認証が一切無く、トークンがAPI応答に生で漏れていた

ユーザー報告「https://easy-web.tokyo/rs-sync/ この画面は管理者用ではありませんか?
本来e-mailのワンタイムパスワードでログインしてからの画面ではありませんか?」
への対応。指摘は完全に正しく、実際に本番相当のセキュリティ欠陥だった。

1. **発見した実際の欠陥**: `src/main.rs`には認証機構が一切存在せず、
   `https://easy-web.tokyo/rs-sync/`へ到達できる誰でもGitHubアカウントの
   閲覧・追加、同期ジョブの作成・実行ができた。さらに`GET
   /api/accounts`は`Account`構造体をそのまま`Json`シリアライズしており、
   `token`フィールド(実際のGitHub PAT等)が平文でレスポンスに含まれて
   いた——実トークンを登録した瞬間、誰でもそれを読み取れる状態だった。
2. **修正(open-gitea/open-redmine/open-easy-webと同じOTPパターンを移植)**:
   `src/auth.rs`(OTP発行・検証・セッション)・`src/mail.rs`(Gmail SMTP)
   を新規追加。`/api/*`(`/api/auth/*`自身を除く)を`RequireAuth`
   ミドルウェアで保護し、有効な`Authorization: Bearer <session-token>`
   が無ければ`401`。`AccountView`(トークンを含まない、`token_set: bool`
   のみのビュー)を新設し、`GET`/`POST /api/accounts`のレスポンスは
   常にこちらを返すよう変更(生トークンはサーバー内部の同期実行時にしか
   触れない)。
3. **本番/デモの真の分離(直前まで正直な開示として残っていた「デモは
   本番と同一バックエンドのエイリアス」を解消)**: OTP認証を本番に
   必須化したことで、デモ環境(ログイン不要で誰でも触れる想定)を
   同じバックエンド・同じデータのまま共存させることは不可能になった
   (本番の実データがデモ経由でログイン無しに見えてしまう)。そのため
   **`rs-sync-demo.service`を別プロセスとして新設**(port 8099、
   `RS_SYNC_DATA=/root/rs-sync-demo/data/store.json`〈本番とは別ファイル〉、
   `RS_SYNC_REQUIRE_AUTH=false`)、`easy-web.tokyo/rs-sync/demo`テナントを
   本番と同じ8096ポートから8099ポートへ張り替えた。これでデモは正真正銘
   独立したデータセット・認証不要のインスタンスになった。
4. **UI刷新**: `static/index.html`にOTPログインフォーム(未ログイン時は
   アカウント/ジョブ/ログ管理パネル自体を非表示)を追加。ユーザー指摘
   「これも分かりづらく使いづらいです」(アカウント登録フォームが
   placeholderのみで意味が読み取りにくい)に対応し、全フィールドに
   日英併記の`<label>`+補足説明(hint)を追加(例:「API base
   URL(プロバイダのREST API入口)/ GitHubなら通常そのまま
   https://api.github.com でOK」)。
5. **検証(実測)**: `cargo test`(ワークスペース全体)9件全green
   (新規: `auth`モジュール5件、`main.rs`の`tests`モジュール4件——
   未ログイン時401・有効セッションでの200・**作成/一覧いずれのレスポンス
   本文にも生トークン文字列が含まれないことを直接アサート**・
   `RS_SYNC_REQUIRE_AUTH=false`時は認証チェックを素通りすることを
   確認)。実機: `https://easy-web.tokyo/rs-sync/api/accounts`→`401`
   (本番)、`https://easy-web.tokyo/rs-sync/demo/api/accounts`→`200`
   `[]`(デモ、独立データ)を確認済み。
6. **正直な開示**: (1) 実際のOTPログイン→アカウント登録→同期ジョブ実行
   というフルE2Eは、ユーザー自身の管理者メール受信操作が必要なため
   今回も未実施。(2) デモ環境は認証不要のため、デモ上で登録した
   アカウント情報(実トークンを間違って入力してしまった場合含む)は
   誰でも閲覧できる——デモ環境には**絶対に実トークンを入力しないこと**
   をUI上でも明示すべきか、次回の検討課題として残す。
  - 次にすべきこと: (1) デモ環境のUIに「実トークンを入力しないでください」
    という明示的な警告を追加、(2) 実OTPログインでのフルE2E(ユーザー
    操作が必要)。

## 関連作業・横串メモ(2026-07-28、どのリポジトリから再開しても迷わないための相互参照)

2026-07-28のセッションでは、RS-Sync・open-redmine・runo.tokyo・
open-easy-webの4リポジトリにまたがる作業を行った。**このセッションの
続きをどこから再開しても良いように**、各リポジトリのCLAUDE.mdに同じ日付の
HANDOFFエントリを置いてある: [open-redmine](https://github.com/aon-co-jp/open-redmine/blob/main/CLAUDE.md)
(runo.tokyoテナント削除+デモリンク追加)・
[runo.tokyo](https://github.com/aon-co-jp/runo.tokyo/blob/main/CLAUDE.md)
(rs-syncデモ廃止に伴う参照削除)・
[open-easy-web](https://github.com/aon-co-jp/open-easy-web/blob/main/CLAUDE.md)
(外部ツールリンクのURL更新、VPS側WASM未反映が既知の残課題)。
未着手事項の全体像は下の各HANDOFFエントリの「次にすべきこと」を参照。

## HANDOFF追記(2026-07-28) OpenGiteaプロバイダの認証欠落バグ修正 + 本番をeasy-web.tokyoへ移設

ユーザー指示「複数のGITHUBアカウントとopen-giteaとの同期をopen-easy-webに
登録のopen-giteaで簡単同期管理したい」のRS-Sync実E2E検証への対応
(前回のプロバイダ改名・API疎通確認の続き)。

1. **発見・修正した実バグ**: `src/provider.rs`の`OpenGiteaProvider`が
   `list_repos`/`ensure_repo`/`remote_url`のいずれでも一切認証情報を
   送っていなかった——open-gitea側は`PUT /repos/:name`(リポジトリ作成)に
   `Authorization: Bearer <session-token>`を必須化済みのため、このままでは
   実際のミラー同期(リポジトリ作成・git push)が確実に401で失敗する状態
   だった。`account.owner`をopen-giteaのログインメール、`account.token`を
   OTPセッショントークンとして扱い、JSON APIへは`Bearer`ヘッダ、git
   push URLへは`https://<email>:<token>@<host>/<repo>.git`形式のBasic認証
   埋め込みを追加した(open-gitea側CLAUDE.mdが文書化する認証方式と一致)。
2. **runo.tokyo/rs-syncデモインスタンスを完全廃止**(ユーザー指示
   「デモ用はやめて削除して」): systemdサービス停止・unit削除・
   `domains.toml`からテナントエントリ削除・`/root/runo.tokyo/rs-sync`
   ディレクトリ(ソース・`data/store.json`含む)を削除。`https://runo.tokyo/
   rs-sync/`は404を返すことを確認済み。
3. **本番を`https://easy-web.tokyo/rs-sync/`へ新規デプロイ**:
   `/root/rs-sync`にクリーンclone(上記1の修正込み)→`cargo build
   --release`→`rs-sync.service`新設(`RestartSec=2`・
   `StartLimitIntervalSec=0`)→`easy-web.tokyo`の`/rs-sync`
   (本番/管理者用)・`/rs-sync/demo`(デモ、現状は同一バックエンドへの
   エイリアス)の2テナントを`POST /admin/tenants`で登録。両方200を確認。
4. **本番ページ自体にデモへの案内リンクを追加**(ユーザー指示
   「その中に以下のデモ用にリンクも貼って」): `static/index.html`の
   イントロ直下に、日本語・英語併記で`/rs-sync/demo`へのリンクを追加。
5. **正直な開示・未着手**: (1) 実GitHubアカウント登録→open-giteaへの
   実ミラー同期という完全なE2Eは、実GitHub PAT(トークン)の入力が必要な
   ためこのセッションでは未実施——ユーザー自身がRS-SyncのフォームにPATを
   入力する必要がある(エージェントが認証情報をフォームへ入力することは
   ポリシー上行わない)。次回、ユーザーが本番`https://easy-web.tokyo/
   rs-sync/`でGitHubアカウント+open-giteaアカウント(ログインメール+
   OTPセッショントークン)を登録し、同期ジョブを作成・実行するところ
   まで確認すること。(2) `/rs-sync/demo`は本番と同一バックエンド・
   同一データを指すエイリアスに過ぎず、独立したデモ専用データセットでは
   ない——将来的に本当の意味でのデモ分離が必要なら別プロセス/別
   `RS_SYNC_DATA`が必要。
  - 次にすべきこと: (1) 実GitHub PAT+open-giteaセッショントークンでの
    フルE2E(ユーザー操作が必要)、(2) `/rs-sync/demo`の真のデータ分離
    (現状はエイリアスのみ)、(3) 上記のsync実行後、実際にopen-gitea側に
    リポジトリが作成されミラーされることの実機確認。

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

## HANDOFF追記(2026-07-31) インストーラーの電源プロファイル選択機能(未実装、エコシステム標準方針として記録)

`open-raid-z`のCLAUDE.md(全リポジトリ共通の設計思想セクション)に、
インストーラー(`install.sh`/`install.ps1`等)実行時に以下3つの電源
プロファイルを選択させる標準方針を追記した(ユーザー指示、2026-07-31):

1. **省電力(Power-saving)**: CPU使用率・ポーリング間隔を抑えた低負荷設定。
2. **省メモリ(Low-memory)**: メモリ確保量・キャッシュサイズを抑えた設定。
3. **常時電源接続(Always-on)**: 上記の抑制を行わないフル性能設定。
   **この場合のみ**ハードウェアアクセラレータ(NPU/GPU)のサポートを
   自動検出・自動有効化する(`open-cuda`の`GpuDevice`抽象化を利用)。

**正直な開示**: このリポジトリのインストーラーへの実装はまだ未着手。
実装時は`open-raid-z/CLAUDE.md`の該当節、および先行実装予定の
`open-redmine/CLAUDE.md`を参照し、`open-cuda`側のGPU/NPUベンダー検出
ロジックを再利用すること(車輪の再発明を避ける)。
- 次にすべきこと: このリポジトリの`install.sh`/`install.ps1`に上記3
  プロファイルの選択機能を追加する。
