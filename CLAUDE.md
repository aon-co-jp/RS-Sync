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

## HANDOFF追記(2026-08-12) トークン欄のtrim漏れによる401 Unauthorized実バグを発見・修正(cargo test 8件全green確認済み)

ユーザーが実際に本番相当の環境でクイック設定から同期ジョブを作成した
ところ、`aon-co-jp -> OpenRuno`ジョブが`auth_error`ステータスで
`repo discovery failed: GitHub list_repos failed: 401 Unauthorized`を
報告。原因調査・修正を行った。

1. **原因**: `static/index.html`のトークン入力欄(クイック設定フォーム・
   通常のアカウント登録フォーム`#accForm`・個別アカウントのトークン
   更新欄)がいずれも`.trim()`されずに`input.value`/`FormData`の値を
   そのままAPIへ送っていた。GitHub PATをブラウザへコピペする際に
   前後に紛れ込みやすい空白・改行がそのままGitHub APIへ送信されると、
   GitHub側はこれを有効なトークン形式として認識できず**401
   Unauthorized**で拒否する(スコープ不足による`403`とは異なり、
   資格情報そのものが無効という扱いになる)——`maybe_auth_header`
   (2026-07-29修正、空トークン時はAuthorizationヘッダー自体を送らない
   ロジック)自体は正しく動作していたが、「空でないが実質無効な
   トークン」というこのケースは想定していなかった。
2. **修正**:
   - クライアント側: `static/index.html`の4箇所(クイック設定の
     マスター/コピー先トークン欄・`#accForm`の`token`フィールド・
     個別アカウントのトークン更新`updateAccountToken`)全てに
     `.trim()`を追加。
   - **サーバー側でも防御的にtrim**(ユーザー指示「コピペ時に紛れ込む
     前後の空白/改行を自動削除する機能を搭載して」への対応、単一の
     UIパスだけでなくAndroid版アプリやAPI直接呼び出し等、他のクライアント
     経由でも確実に効くようにするため): `src/main.rs`の`create_account`・
     `update_account_token`の両ハンドラで`acc.token.trim().to_string()`/
     `body.token.trim().to_string()`を適用。
3. **回帰テスト追加**: `account_tokens_are_trimmed_of_surrounding_
   whitespace_on_create_and_update`(新規)——前後に空白・改行・タブを
   含むトークンを`POST /api/accounts`・`PUT /api/accounts/:id`双方へ
   送り、サーバー内部のストア(`state.store`を直接検査)に保存される
   トークンが実際にtrim済みであることを確認する。
4. **検証結果(コンテキストリミット接近のため中断していたビルドが
   完了、続報)**: `cargo test --bin rs-sync`**8件全green**
   (新規`account_tokens_are_trimmed_of_surrounding_whitespace_on_
   create_and_update`含む、既存7件も回帰無し)。ローカルの
   `RS_SYNC_REQUIRE_AUTH=false`検証時点(修正前バージョン)では
   クイック設定フォーム自体の表示・送信経路は正常に動作していたことも
   確認済み(スクリーンショットでマスター/コピー先URL・トークン・
   間隔選択の全フィールドの存在を確認済み)。
5. **ユーザーへの案内(このHANDOFF内に記録、次回参照用)**: GitHub
   Classic PATのスコープ——マスター(読み取り)用は公開リポジトリのみ
   なら無スコープ(空欄可)、非公開含むなら`repo`のみ。コピー先
   (書き込み)用は`repo`(または公開限定なら`public_repo`)のみ。
   「期間永久」は発行時のExpiration欄で「No expiration」を選択する。
- 次にすべきこと: (1) 本番(`https://easy-web.tokyo/rs-sync/`)への
  デプロイ(`git pull`→`cargo build --release`→
  `systemctl restart rs-sync.service`、`static/index.html`はランタイム
  配信のため再ビルド不要な可能性が高いが要確認)、(3) ユーザーが実際に
  `aon-co-jp -> OpenRuno`ジョブを再実行し、401エラーが解消することの
  実地確認(トークンが最初から空白付きで保存されている既存アカウントは、
  今回の修正では過去データまで自動修正しないため、ユーザーに一度
  トークンを再入力〈またはトークン更新欄から再送〉してもらう必要がある
  可能性がある)。

## HANDOFF追記(2026-08-07続き4) Android実機で「本体をこの端末で実行」機能の起動確認完了、実バグ2件を発見・修正

前回HANDOFF(続き3)の「次にすべきこと」を実施。`cargo ndk -t arm64-v8a
-t x86_64 build --release --bin rs-sync`が`ssh2`除去後**実際に成功**
することを確認(前回懸念していたAndroidクロスビルド阻害要因は解消済み)。
`librssync.so`を`jniLibs/{arm64-v8a,x86_64}/`へ配置し、実機
(`moto g53y 5G`、`ZY22J7RFND`)で「本体をこの端末で実行」機能を実際に
検証したところ、2件の実バグを発見・修正した。

1. **実バグ1: `build.gradle.kts`がネイティブバイナリを一切想定していない
   設定のままだった**(`defaultConfig`のコメントに「ABI別jniLibsは
   不要」と明記された、リモートクライアント専用だった頃の設定が残存)。
   `abiFilters`未指定・`packaging { jniLibs { useLegacyPackaging =
   true } }`未設定のため、AGP既定の「APK内から直接実行(run-from-apk)」
   のままとなり、`applicationInfo.nativeLibraryDir`配下にファイルが
   展開されず`binary exists: false`で起動が拒否されていた
   (open-easy-web/open-web-server側で過去に踏んだ実バグと同一パターン、
   横展開漏れ)。両設定を追加して解消。
2. **実バグ2: プロセス起動直後の`/healthz`疎通確認にリトライが無く、
   単発の接続失敗でそのまま「起動失敗」扱いになっていた**——poemサーバー
   がリスンソケットをbindし終える前に確認してしまうタイミング問題
   (open-easy-webの`pollHealthz()`は複数回リトライする設計だったのに
   対し、こちらは単発呼び出しのままだった)。1秒間隔で最大5回リトライ
   するよう`MainActivity.kt`を修正。
3. **実機検証(型チェックのみで完了と報告しない、既存運用ルール徹底)**:
   修正後、実機で「本体をこの端末で実行する設定」→有効化→
   「本体をこの端末で起動」の一連の操作を行い、画面上に
   **`RUNNING (local, port 18095)`、ログに`GET http://127.0.0.1:18095/
   healthz -> 200 body: ok`が実際に表示される**ことをスクリーンショットで
   確認した。検証後`am force-stop`でプロセスを終了。
4. **正直な開示・未着手**: (1) VPS側`/healthz` URLを実際に入力して
   フェイルオーバー(VPS生存中はこの端末がジョブ実行をスキップする
   こと)を実地確認するところまでは未実施(今回は空欄のまま=常時この
   端末が実行する設定で起動確認のみ行った)。(2) VPS+スマホ実機2台での
   実際の同期ジョブ実行・フェイルオーバー動作の実地検証は引き続き
   未着手。(3) `RS_SYNC_REQUIRE_AUTH=false`(ループバックのみ到達可能
   という前提)での運用が実際に安全か、実機のファイアウォール/
   他アプリからのアクセス可能性は未検証。
- 次にすべきこと: (1) VPS `/healthz` URLを設定した状態でのフェイル
  オーバー実地検証、(2) 実際に同期ジョブ(git/フォルダ同期)をこの
  端末上のインスタンスで作成・実行するE2E確認、(3) タブレット実機/
  エミュレータでの同機能の確認。

## HANDOFF追記(2026-08-07続き3) `ssh2`(OpenSSL依存)→`russh`/`russh-sftp`(pure Rust)への置き換え完了、Androidクロスビルド阻害要因を解消

ユーザー指示「ssh2をpure-Rust実装(russh等)へ置き換える設計変更をやって」への
対応。前回HANDOFF(続き2)で「Android版の`cargo ndk`クロスビルドが
`ssh2`→`openssl-sys`のFFI依存で失敗する」と記録した根本原因を解消した。

1. **`SftpBackend`を`ssh2`から`russh`+`russh-keys`+`russh-sftp`
   (いずれもpure Rust、`open-web-server`側`sftp.rs`が既に実機検証済みの
   同一バージョン組み合わせ〈0.45系/2系〉を再利用)へ全面書き換え**
   (`src/folder_sync.rs`)。`SmbBackend`と同じ「使い捨てtokio
   ランタイム(`new_current_thread`)を保持しblock_onで同期traitへ
   変換する」パターンを踏襲。ホスト鍵検証は行わない
   (`TrustAnyServerKey`、旧`ssh2::Session::handshake()`も同様に検証
   していなかったため後退ではない、正直な開示としてコード内docに明記)。
2. **実装中に発見・修正した実バグ**: `russh_sftp::client::SftpSession::
   write()`は`OpenFlags::WRITE`のみでファイルを開くため、**新規ファイル
   の作成ができない**(既存ファイルの上書きのみ可能)——`write_file`で
   これを直接使うと新規アップロードが常に`No such file`で失敗する。
   `create()`(`CREATE|TRUNCATE|WRITE`)経由の`File`(`AsyncWrite`実装)へ
   `write_all`+`shutdown`する方式に修正して解消。
3. **デバッグで踏んだ環境固有の罠(教訓として明記)**: 実VPS
   (conoha、`160.251.237.162`)に対する`#[ignore]`統合テストを
   Git Bash経由で実行した際、`RS_SYNC_TEST_SFTP_REMOTE_PATH=/tmp/...`
   という環境変数値がMSYSの自動パス変換で
   `C:/Users/.../AppData/Local/Temp/...`のようなWindowsパスへ書き換え
   られ、一見コードのバグに見える別の失敗(`No such file`)を引き起こした
   ——`MSYS_NO_PATHCONV=1`を付けて解消(このエコシステムで過去にも
   繰り返し記録されている既知の罠パターンと同種)。
4. **検証(実測)**: `cargo test --lib`45件全green(回帰無し)。
   実VPS(conoha)に対する`local_to_sftp_and_back_round_trips_against_a_
   real_vps`(`--ignored`)を`MSYS_NO_PATHCONV=1`付きで実行し、
   **実際に新しいrusshバックエンド経由でlocal→VPS(SFTP新規アップ
   ロード)→別ローカルディレクトリ(SFTP読み取り)の往復が成功する
   ことを実機で確認**(修正前は新規アップロードが上記2.のバグで
   必ず失敗していた)。検証用の一時ファイル・ディレクトリは
   `ssh conoha "rm -rf /tmp/rs-sync-russh-test"`で後片付け済み。
5. **`cargo ndk`でのAndroidクロスビルド**: `ssh2`除去により
   `openssl-sys`がAndroidターゲットの依存グラフから消えたはずのため、
   `cargo ndk -t arm64-v8a -t x86_64 build --release --bin rs-sync`を
   再実行(結果は本エントリ執筆時点で実行中、次のHANDOFF追記で結果を
   記録する)。
- 次にすべきこと: (1) `cargo ndk`クロスビルドの成功確認、(2) 成功したら
  `librssync.so`を`jniLibs/{arm64-v8a,x86_64}/`へ配置しAPKビルド・
  実機インストール・起動確認、(3) VPS+スマホ実機2台でのフェイル
  オーバー実地検証。

## HANDOFF追記(2026-08-07続き2) Android版を「リモートクライアント」から「本体をこの端末で実行」方式へ拡張(open-easy-web方式のProcessBuilder起動)

前回HANDOFF(続き)の「次にすべきこと(1)」に対応。ユーザーから実際の
WireGuard固定IP用`.conf`ファイル(LOLIPOP!固定IPアクセス)を受け取り、
`PersistentKeepalive`が実際に欠落していることを確認・追記した
(ユーザー側でWireGuardアプリへの再インポートが必要、この修正だけでは
端末側には反映されない旨を案内済み)。これとは独立に、本項の本体
(Android版RS-Sync)の拡張を実施。

1. **新規`LocalServerConfig.kt`**: 「本体をこの端末で実行する」機能の
   有効/無効フラグ(既定無効、後方互換)・VPS側`/healthz`URL
   (`primaryHealthzUrl`、`RS_SYNC_PRIMARY_HEALTHZ_URL`環境変数として
   ネイティブプロセスへ渡す)・ローカルポート(既定`18095`)を
   `SharedPreferences`で保持。
2. **`MainActivity.kt`拡張**: 新規ボタン「⚙️ この端末で本体を実行する
   設定」(有効化+VPS `/healthz` URL入力ダイアログ)・「🖥️ 本体をこの
   端末で起動」を追加。`startLocalServerProcess()`
   (`open-easy-web/android::startServerProcess()`と同じ
   `ProcessBuilder`+`jniLibs`同梱パターン、バイナリ名`librssync.so`
   ——`cargo ndk`でのクロスビルド成果物をこの名前で`jniLibs/<abi>/`へ
   配置する想定)が、`RS_SYNC_DATA`(端末内`filesDir/rs-sync-data/
   store.json`)・`RS_SYNC_PORT`(既定18095)・`RS_SYNC_REQUIRE_AUTH=false`
   (ループバックのみ到達可能なため認証不要、既存の`rs-sync-demo`運用と
   同じ判断)・`RS_SYNC_PRIMARY_HEALTHZ_URL`(設定されていれば、
   前回実装した`scheduler.rs`のフェイルオーバーロジックがVPS生存中は
   この端末の同期ジョブ実行をスキップする)を環境変数として渡して起動する。
   起動後は自動で`/healthz`疎通確認→成功なら`serverUrlInput`をローカル
   URLへ切り替えて既存の`startPeriodicPolling()`をそのまま再利用。
   `onDestroy()`でプロセスを道連れに終了(フォアグラウンドサービス化は
   今回スコープ外、既存のバックグラウンド常駐化の課題と同じ)。
3. **既定は無効のまま**(後方互換): 有効化しない限り、従来通り
   「リモートクライアント」として動作し続ける(既存の`connectButton`/
   `openBrowserButton`のロジックは無変更)。
4. **検証**: `gradle :app:compileDebugKotlin`・`:app:assembleDebug`
   ともに**BUILD SUCCESSFUL**(`--offline`、キャッシュ済みGradle
   8.11.1使用)。`librssync.so`は未同梱のため`mergeDebugNativeLibs`は
   `NO-SOURCE`(想定通り)——`binaryPath.exists()`チェックにより、
   未ビルド状態で起動を試みても「ERROR: native binary not found」と
   ログへ明示し、黙って失敗しない設計にしてある。
5. **正直な開示・未着手**: (1) `cargo ndk -t arm64-v8a -t x86_64 build
   --release --bin rs-sync`での実クロスビルド・`jniLibs`への実配置・
   実機/エミュレータでの実際の起動確認はいずれも今回未実施
   (Kotlin側の実装・ビルド成功の確認までに留まる)。(2) VPS+スマホ
   実機2台での実際のフェイルオーバー動作(VPS停止時にスマホ側が
   代行開始すること)の実地検証も未実施。(3) `open-easy-web`と異なり
   RS-Syncは固定アカウント制の認証を持たないため`OPEN_EASYWEB_
   FIXED_ACCOUNT_EMAIL`相当の必須環境変数チェックは不要——代わりに
   `RS_SYNC_REQUIRE_AUTH=false`で認証自体を無効化する設計とした
   (ループバックのみ到達可能なため実害は低いと判断、正直な開示として
   明記)。
- 次にすべきこと: (1) `cargo ndk`での実クロスビルド+`librssync.so`を
  `jniLibs/{arm64-v8a,x86_64}/`へ配置、(2) 実機/エミュレータでの
  「本体をこの端末で起動」ボタンの実タップ確認、(3) VPS+スマホ実機
  2台でのフェイルオーバー実地検証、(4) WireGuard `.conf`修正後、
  実際に外部からアクセス可能になったかのユーザー確認待ち。

## HANDOFF追記(2026-08-07続き) VPS/スマホ二重稼働時の重複実行回避(フェイルオーバー方式)+テスト強化

ユーザー指示「RS-SyncはVPSとスマホの二箇所にインストール予定。どちらかが
生きていれば、重複しないで稼働させたい」+「Androidスマホにも本体を
インストールしたい(従来のリモートクライアント方式から変更)」+
「rs-syncのテスト強化をして」への対応。

1. **排他方式の確認(ユーザー選択)**: VPSを常に優先(primary)、スマホ側
   インスタンスはVPSの`/healthz`への死活監視で「VPSが不通の間だけ」
   同期ジョブを代行する片方向フェイルオーバー方式を採用(共有ロック
   ファイル方式ではなく、追加インフラ不要でシンプルな方を選択)。
2. **実装**(`src/model.rs`/`src/scheduler.rs`): `AppState`に
   `primary_healthz_url: Option<String>`
   (`RS_SYNC_PRIMARY_HEALTHZ_URL`環境変数、VPS側には設定しない・
   スマホ側インスタンスにのみ設定する運用)を追加。
   `run_scheduler_loop`の毎分tickの冒頭で、この値が設定されている
   場合のみ`check_primary_alive()`(3秒タイムアウトの`reqwest::blocking`
   GET、`spawn_blocking`経由)でVPS側`/healthz`への到達性を確認し、
   到達できれば`continue`でこのtickのジョブ実行(gitジョブ・フォルダ
   同期ジョブとも)をまるごとスキップする。VPS不通時のみ通常通り実行する
   ——ネットワークエラー・非200応答はいずれも「primary不通」(=実行すべき)
   として安全側(実行する側)に倒す設計。
3. **Android版の位置づけ変更(今回はサーバー側の土台のみ、Android実装は
   未着手)**: 従来の「リモートクライアント専用」設計(`open-easy-web`と
   同様の`ProcessBuilder`起動)から、`open-easy-web`方式(スマホ上で
   本体〈`cargo ndk`クロスビルド〉を`ProcessBuilder`起動、
   `RS_SYNC_PRIMARY_HEALTHZ_URL`にVPS側の公開URLを設定)へ変更する方針を
   ユーザーと確認した。**今回のパスではAndroidアプリ側(`android/`)の
   改修は未着手**——サーバー側の排他ロジック(上記2.)のみ実装した。
4. **テスト強化**: `scheduler.rs`に新規`failover_tests`モジュール
   (実TCPモックサーバーで200/非200/接続不可の3パターンを検証、
   `check_primary_alive`の回帰テスト3件)。`cargo test --lib`は
   42→**45件全green**(新規3件、既存42件は無回帰)。
5. **正直な開示・未着手**: (1) Android側`android/`の「リモート
   クライアント」から「本体インストール」への設計変更自体(UI・
   `ProcessBuilder`起動ロジック・`RS_SYNC_PRIMARY_HEALTHZ_URL`環境変数の
   設定導線)はまだ実装していない——次回セッションでの着手事項。
   (2) 実際にVPS+スマホの2台構成で、VPS停止時にスマホ側が実際に
   同期を代行し始めることの実機検証は未実施(ロジック単体のテストのみ)。
- 次にすべきこと: (1) `android/`をopen-easy-web方式(本体埋め込み・
  `ProcessBuilder`起動)へ改修、(2) `RS_SYNC_PRIMARY_HEALTHZ_URL`を
  Android側の設定画面から入力できるようにする、(3) 実機2台
  (VPS+スマホ)でのフェイルオーバー実地検証。

## HANDOFF追記(2026-08-07) タブレットエミュレータでsw600dpレイアウト実機検証+実バグ1件発見・修正

前回HANDOFF(2026-08-05)の「次にすべきこと(1) タブレット実機/エミュレータ
での`sw600dp`レイアウトの見た目確認」に対応。

1. **タブレットAVD新規作成**: この開発機にAndroid SDK
   (`%LOCALAPPDATA%\Android\Sdk`)・emulator・system-image
   (`android-37.0/google_apis_playstore_ps16k/x86_64`)が導入済み
   だったが、`avdmanager`コマンド自体は同梱されておらず(cmdline-tools
   未導入)、AVD定義が1つ(`Pixel_9_Pro`、電話サイズ)しか無かった。
   `avdmanager`を使わず`~/.android/avd/rs_sync_tablet.avd/config.ini`を
   直接作成する方式で、10.1インチ相当タブレット
   (`hw.lcd.width=2560`/`height=1600`/`density=213`、
   smallest-width計算で約1922dp相当、sw600dp/sw720dpどちらの
   リソースバケットにも該当)のAVDを新規作成し、実際に起動できる
   ことを確認した。
2. **実機(エミュレータ)起動+APKインストール+画面遷移確認**:
   `%USERPROFILE%\.gradle\wrapper\dists`に既存のGradle 8.11.1
   配布物が見つかったため(`gradle`コマンド自体はPATH上に無い環境
   だった)、そのパスから直接`gradle.bat :app:assembleDebug`を実行し
   `BUILD SUCCESSFUL`。生成された`app-debug.apk`をタブレットAVD
   (`emulator-5554`)へ`adb install`、`ProfileSelectActivity`を起動して
   実際にスクリーンショットで画面を確認した(起動直後にSwiftShader
   ソフトウェアレンダリングの影響とみられるSystemUIの一時的なANR
   が発生したが、待機で自然に回復・実害なし)。
3. **発見した実バグ**: `layout-sw600dp/activity_main.xml`・
   `layout-sw600dp/activity_profile_select.xml`の両方で、内側の
   `LinearLayout`が`android:layout_width="match_parent"`のまま
   `android:maxWidth="720dp"`を指定していた。Android仕様上
   `android:maxWidth`は`layout_width="wrap_content"`の場合のみ有効
   ("幅720dp上限+中央寄せ"というレイアウト意図が実際には機能せず)、
   実機スクリーンショットで実際に画面全幅(2560px)へ左寄せで
   伸びきって表示されることを確認した(前回HANDOFFで「ビルド成功の
   確認まで」と書いた際は、この見た目の破綻を見逃していたことになる)。
   `layout_width`を`wrap_content`に変更して再ビルド・再インストール・
   再スクリーンショットし、中央寄せ+720dp上限が実際に機能する
   ことを確認した(`ProfileSelectActivity`・`MainActivity`の両画面で
   確認済み)。open-easy-web/open-web-server側の同一パターンを踏襲した
   実装だったため、同種のバグが他リポジトリにも存在する可能性がある
   (今回はrs-sync内の修正のみ実施、横展開の要否は未判断)。
4. **正直な開示・未着手事項**:
   - 今回検証したのはタブレット**エミュレータ**であり、実機タブレット
     (物理デバイス)での確認はまだ行っていない。
   - macOS版launchd・Linux版systemd --user実機検証は、依然として
     この開発環境(Windows)では実施不可能(未着手のまま)。
   - Gitea/Gitbucket/GitLab/Bitbucketの実サーバー確保時の実機検証は
     引き続き未着手(実インフラが無い)。
   - VPSデプロイ手順の独立ドキュメント化は今回も未着手。
   - `cargo build --release`を今回のセッションでも実行しようとしたが、
     タイムアウトのため完走確認まではできなかった(Androidエミュレータ
     関連の作業に時間を要したため。コード変更はAndroidの`.xml`のみで
     Rust側に変更は無いため、直近のHANDOFF(2026-08-05)で確認済みの
     `cargo test --lib`green状態からの回帰は無いと考えられるが、
     今回のセッションでの再実行確認はできていない)。
   - 他プロジェクト(open-easy-web/open-web-server等)の同一パターンへの
     横展開要否は未判断。
  - 次にすべきこと: (1) 実機タブレット(物理デバイス)での見た目確認、
    (2) open-easy-web/open-web-server等、同一の`maxWidth`+
    `layout_width="match_parent"`パターンを使っている他リポジトリに
    同種のバグが無いか横展開確認、(3) 実macOS/Linux環境でのlaunchd/
    systemd --user実機検証、(4) Gitea/Gitbucket/GitLab/Bitbucketの
    実サーバー確保時の実機検証、(5) VPSデプロイ手順の独立ドキュメント化
    の要否判断、(6) 次回セッションで`cargo build --release`/
    `cargo test --lib`の完走を再確認。

## HANDOFF追記(2026-08-05) PC/VPS/スマホ/タブレット全対応の棚卸し+実機検証+実バグ1件発見・修正、プロバイダ対応は既に完備と確認

ユーザー指示「RS-SyncをPCにもVPSにもスマホにも(追ってタブレットも)適用
可能にしたい」+「同期先プロバイダをGitHub・Gitbucket・Bitbucket・Gitea・
open-giteaの全てに対応させて」への対応。

1. **棚卸し結果**:
   - **デスクトップ(PC)**: `install.sh`(Linux/macOS、cargoビルド+
     systemd --user/launchd登録)・`install.ps1`(Windows、cargoビルド+
     Task Scheduler登録)とも既存。前回HANDOFFの「Windows Task Scheduler
     登録は管理者PCで実機成功済み」の記載通り。
   - **VPS**: `https://easy-web.tokyo/rs-sync/`で本番稼働中
     (`rs-sync.service`、systemd)。デプロイ手順自体は各HANDOFFに
     散在する形で記録済みだが、まとまった「VPSデプロイ手順書」は
     無い(今回はドキュメント整備までは着手せず、既存の運用実績で
     代替可能と判断——次回まとまった手順化の要否を再検討)。
   - **Android(スマホ)**: `ProfileSelectActivity`→`MainActivity`は
     設計通り**リモートクライアント**(別ホストで動くRS-Syncインスタンスの
     `/healthz`へポーリング疎通確認+「ブラウザで開く」で既存WebUIへ
     委ねる)であることをソース(`MainActivity.kt`/`PowerProfile.kt`)を
     読んで確認した。RS-Sync本体(gitクローン・poemサーバー)をAndroid上で
     直接実行する経路は無い。
   - **タブレット**: `layout-sw600dp/activity_main.xml`は既存だったが、
     初期画面`activity_profile_select.xml`にはsw600dp版が無かった
     (見落とし)。`layout-sw600dp/activity_profile_select.xml`を新規追加
     (幅720dp上限+中央寄せ、他リポジトリ〈open-easy-web/
     open-web-server〉と同じ簡素なパターン)。
   - **同期先プロバイダ**: `src/provider.rs`を確認したところ、
     **GitHub・OpenGitea・Gitea・Gitbucket・GitLab・Bitbucketの6種類が
     全て既に実装済み**(`provider_for()`の`match`に全バリアント有り、
     `static/index.html`の`<select>`にも全選択肢が既に配線済み)。
     過去のHANDOFF通り、GitHub・OpenGiteaは実機検証済み、
     Gitea/Gitbucket/GitLab/Bitbucketは公開API仕様に基づく実装のみで
     実サーバーが無く未検証、という状態は変わっていない——**「対応させて
     ほしい」という要望自体は既に満たされていた**(新規プロバイダの
     追加実装は不要だった)。
2. **新規実施: モックHTTPサーバーでのプロバイダ単体テスト追加**:
   実アカウント/実サーバーが無いGitea/Gitbucket/GitLab/Bitbucket/
   open-giteaについて、外部crate(wiremock等)を追加せず`std::net::
   TcpListener`のみで最小限のモックサーバーを実装し、`list_repos`が
   実際に期待するパス・認証ヘッダー形式(`token`/`Bearer`/
   `PRIVATE-TOKEN`)でリクエストを送っていることを検証する回帰テストを
   6件追加(`src/provider.rs`)。**実サーバーとの互換性までは保証しない
   (正直な開示は変えていない)**が、実装が意図通りのリクエストを
   組み立てていることは実際のTCP通信で固定化された。
3. **Windows実機での実インストール検証**: `RS_SYNC_HOME`をスクラッチ
   ディレクトリに向けて`install.ps1`を実際に実行し、git clone→
   `cargo build --release`→起動スクリプト生成まで完走することを確認
   (Task Schedulerの対話質問には`N`で応答、既に前回HANDOFFで登録自体は
   実機確認済みのため今回はスキップ)。ビルド中、並行して走らせていた
   別のcargoプロセスとの資源競合とみられる`rustc`の一過性クラッシュ
   (`STATUS_STACK_BUFFER_OVERRUN`)が発生したが、cargoの再試行で最終的に
   `rs-sync.exe`の生成まで成功した(コード側の不具合ではなく、この
   検証セッション固有の並行実行の影響と判断)。
4. **`cargo build --release`/`cargo test --lib`実施**: 本体リポジトリ
   (`F:\runo\rs-sync`)で単独実行し、いずれも成功(`cargo test --lib`は
   44件中42件green・2件`#[ignore]`〈実VPS/実ルーター用〉、新規6件含む)。
5. **Android実機ビルド+インストール+起動+疎通確認まで完全実施**
   (今回、初めてこの制約〈ネットワーク分離〉を回避できた): 実機
   `moto g53y 5G`(`ZY22J7RFND`)がこのセッションでは`adb devices`に
   正常に出現し(この開発機にAndroid SDK platform-toolsが既に導入済み
   だったため、前回セッションで詰まっていたUSBドライバ/ネットワーク
   分離の問題を今回は踏まずに済んだ——環境差によるものと考えられる)、
   `gradle :app:assembleDebug`→**BUILD SUCCESSFUL**→
   `adb install`→実機で起動→スクリーンショットで画面遷移
   (プロファイル選択→通常モード→メイン画面)を実際に確認、「接続/
   疎通確認」ボタンをタップして**実際に本番VPS
   (`https://easy-web.tokyo/rs-sync/healthz`)への実HTTPリクエストが
   200 okを返すところまで実機で確認できた**(モックではない、実際の
   スマホ→実際のインターネット→実際のVPSの往復)。
6. **上記の過程で発見・修正した実バグ**: `PowerProfile.kt`の
   `loadServerUrl()`のデフォルトURLが`https://easy-web.tokyo/RS-Sync`
   (大文字)になっており、`strings.xml`の`server_url_hint`
   (`https://easy-web.tokyo/rs-sync`、小文字)と食い違っていた。
   nginxの`location`マッチは大文字小文字を区別するため、初回起動時に
   ユーザーがヒントを見ずにそのまま「接続/疎通確認」を押すと**大文字
   パスは実際には存在せず404になっていたはず**(実機で修正前の
   デフォルト値のまま接続を試して初めて気づいた)。小文字
   `https://easy-web.tokyo/rs-sync/`へ修正し、修正後に実機で200 okを
   確認済み。地味だが、まさに「PCにもVPSにもスマホにも」という
   今回の目的に直結する導線上の実障害だった。
7. **正直な開示・未検証事項**:
   - macOS版launchd登録・Linux版systemd --user登録は、依然としてこの
     開発環境(Windows)では実機検証できていない(コードレビューのみ)。
   - タブレット実機/タブレットエミュレータでの`layout-sw600dp`の
     見た目確認は今回未実施(スマホ実機での確認のみ、sw600dp
     リソースが正しくビルドに含まれることは`assembleDebug`成功で
     確認済みだが、実際の大画面での表示崩れの有無は未確認)。
   - Gitea/Gitbucket/GitLab/Bitbucketの4プロバイダは、今回追加した
     モックサーバーテストによりリクエスト形式の正しさは固定化された
     ものの、引き続き実サーバーでの応答互換性は未検証(実インフラ
     構築は本タスクのスコープ外と判断、従来の判断を維持)。
   - VPSへのデプロイ手順を独立したドキュメントとしてはまとめていない
     (各HANDOFFに散在したまま)。
  - 次にすべきこと: (1) タブレット実機/エミュレータでの`sw600dp`
    レイアウトの見た目確認、(2) 実macOS/Linux環境でのlaunchd/
    systemd --user実機検証、(3) Gitea/Gitbucket/GitLab/Bitbucketの
    実サーバー確保時の実機検証、(4) VPSデプロイ手順の独立ドキュメント化
    の要否判断。

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

## HANDOFF追記(2026-08-03続き2) デスクトップ向けインストーラーmacOS/Windows対応+Android版アプリシェル新規実装

ユーザー指示「デスクトップ向けインストーラー(Windows/Linux/Mac)の整備して
Androidスマホとタブレットも一緒に整備して、iPhoneは後回しで良いです」。

1. **`install.sh`にmacOS対応を追加**: 従来Linux専用(systemd --user
   決め打ち)だったのを`uname -s`でOS判定し、macOSでは`launchd`の
   ユーザーagent(`~/Library/LaunchAgents/tokyo.aon.rs-sync.plist`、
   `RunAtLoad`+`KeepAlive`でsystemdの`enable --now`+`Restart=on-failure`
   相当を実現)を登録するようにした。
2. **`install.ps1`にWindows常駐化を追加**: 従来は手動起動スクリプトの
   生成のみで「サービス登録はしない」と明記されていたが、Task Scheduler
   タスク(ログオントリガー+`RestartCount 999`で自動再起動)を対話的に
   登録できるようにし、Linux/macOSの自動起動機能と揃えた。
3. **Android版アプリシェルを新規実装**(`android/`、
   `open-redmine/android`のリモートクライアント設計をそのまま移植・
   RS-Sync向けに文言調整): RS-Sync本体をAndroid上で実行するのではなく、
   リモートインスタンスの`GET /healthz`へ疎通確認し、「ブラウザで開く」で
   既存のWebUIへ委ねる設計。3電源プロファイル(省電力/通常/常時電源接続)・
   スマホ/タブレット両レイアウト(`layout`/`layout-sw600dp`)は参照実装と
   同一パターン。
4. **検証(実測)**: `cargo test --lib`36件全green(回帰無し)。
   `gradle :app:assembleDebug`→**BUILD SUCCESSFUL**、
   `app-debug.apk`(約3.24MB)を実際に生成することを確認。
   `bash -n install.sh`・PowerShellの`Parser::ParseInput`でinstall.ps1の
   構文検証済み。
5. **正直な開示**: (1) macOS版launchd・Windows版Task Scheduler登録は
   いずれもこの開発環境(Windows)では実機検証できていない(スクリプトの
   コードレビュー+構文検証のみ)。(2) Android実機/エミュレータでの
   起動・`/healthz`実疎通確認は今回未実施(ビルド成功の確認まで)。
   (3) iPhone/iPad対応はユーザー指示により後回し
   (`open-easy-web/IOS_FEASIBILITY_MEMO.md`に実現可能性調査を記録済み)。
  - 次にすべきこと: (1) 実macOS環境でのlaunchd登録実地確認、
    (2) 実Windows環境でのTask Scheduler登録実地確認、(3) Android実機/
    エミュレータでの起動確認、(4) これらのインストーラー整備を
    他リポジトリ(open-easy-web等)へも横展開するか判断。

## HANDOFF追記(2026-08-03続き5) 別のWiFiルーター(TP-Link)でSMB実機検証成功+実バグ1件を発見・修正

直前エントリでASUS RT-AC65U(SMB1専用)への接続に失敗した後、ユーザーが
別のルーター(TP-Link AX6600 Wi-Fi 6、USBストレージ+Samba共有
「TP-Share」)を用意してくれたため、そちらで再検証した。

1. **接続・認証・共有一覧取得に成功**: `smb2::connect`→
   `list_shares()`で実際にSMB 2.0.2でネゴシエートし、共有`G`を検出。
   ASUS機がSMB1専用だったのとは異なり、TP-Link機はSMB2/3に対応して
   おり、`smb2`crateと問題なく通信できることを確認した(=`SmbBackend`
   実装自体は正しく、前回の失敗は特定ルーターのSMB1限定という個体差
   だったことが裏付けられた)。
2. **発見・修正した実バグ**: 共有ルート(既存の`.minidlna`/`.TPDLNA`
   フォルダ含む)を一覧・読み取りしたところ、`.minidlna/files.db`
   (77824バイト)の読み取りで
   `file is 77824 bytes, larger than the server's 65536-byte
   single-read limit; use read_file_pipelined for files this size`
   というエラーが発生した。原因は`SmbBackend::read_file`が単発
   (non-pipelined)の`read_file`を使っており、サーバーがネゴシエート
   した最大読み取りサイズ(このルーターでは64KB)を超えるファイルを
   一切読めなかったこと(`write_file`側は元々`write_file_pipelined`を
   使用済みで、`read_file`側だけ対称性が崩れていた)。
   `client.read_file_pipelined`へ変更して解消。
3. **`SmbBackend`経由の実機テストを新規追加**: `run_folder_sync()`
   (プロダクションコードと同じ経路)でlocal→remote(実SMB書き込み)→
   remote→local(共有ルート全体、大きい既存ファイル含む一覧+読み取り)を
   実施し、アップロードしたテストファイルの内容が正しく往復することを
   確認した(`local_to_smb_and_back_round_trips_against_a_real_router`、
   `#[ignore]`付き)。修正後、実際にこのテストがpassすることを確認
   (修正前は`.minidlna/files.db`の読み取りで失敗していた)。
4. **検証(実測)**: `cargo test --lib`36件全green(回帰無し)+実機
   テスト2件(SFTP・SMB)を`--ignored`で個別実行しいずれもpass。
   実機上に残ったテストファイル(`rs-sync-smb-realdevice-test.txt`・
   `smb-roundtrip-<pid>.txt`)は検証用の一時スクリプトで削除済み
   (実機を汚さないよう後片付け)。VPS本番(`rs-sync`・
   `rs-sync-demo`)へも反映・再起動、両サービスとも`active`を確認。
5. **正直な開示・結論**: 「WiFiルーターのUSB共有」と一括りにしても
   機種によってSMB1専用/SMB2対応が分かれる(ASUS RT-AC65U vs TP-Link
   AX6600の実例)。今回の`SmbBackend`(SMB2/3専用の`smb2`crateベース)は
   **SMB2/3対応ルーター/NASでは実際に正しく動作する**ことを実機で
   確認できたが、SMB1専用の旧いルーターは引き続きサポート対象外
   (直前エントリの「次にすべきこと」に記載の設計判断は保留のまま)。
  - 次にすべきこと: (1) SMB1対応の要否再検討(引き続き保留、直前
    エントリ参照)、(2) Android実機検証(ネットワーク分離により保留中、
    別環境での再挑戦を検討)、(3) 他のNAS/ルーター機種でのさらなる
    実機検証(任意)。

## HANDOFF追記(2026-08-03続き4) 実機検証まとめ: Task Scheduler成功、Android実機はネットワーク分離で保留、SMBは実ルーターがSMB1のみ対応と判明

ユーザーが実際に管理者権限のWindows PC・Android実機(FCNT WE2 Plus)・
WiFiルーター(ASUS RT-AC65U、USBストレージ+Samba共有)を用意し、
順に実機検証を行った(2026-08-03、同日中の続きセッション)。

1. **Windows Task Scheduler登録: 実機で成功を確認**。ユーザーに管理者
   PowerShellで実際に`Register-ScheduledTask`→`Start-ScheduledTask`を
   実行してもらい、`http://127.0.0.1:18095/healthz`が実際に`200 ok`を
   返すことを確認した(=タスクが実際にログオントリガーで起動し、
   `rs-sync.exe`が正しく立ち上がった証拠)。**`install.ps1`のWindows
   常駐化機能は実環境で動作確認済みとなった**。
2. **Android実機(USB経由)**: FCNT製「WE2 Plus」を用意してもらい
   USB接続を試みたが、PC側では単純な「USB大容量記憶装置」としてのみ
   認識され、MTPモード切替後も`adb devices`に一切出現しなかった。
   Google公式USBドライバの手動インストールも試したが変化なし
   ——キャリア/メーカー側でADB自体を制限している可能性が高いと判断。
3. **Android実機(WiFiワイヤレスデバッグ経由)**: ペアリングコード+
   IP:ポートを複数回取得し`adb pair`を試したが
   `protocol fault(couldn't read status message)`で失敗。
   `Test-NetConnection`で生TCP接続自体が`False`だったことから、
   **アプリ層以前のネットワークレベルでの遮断**と判明——このPCが
   参加する企業ネットワーク(`aon`ドメイン、SSID`aon.tokyo AP AS 5G 2`)の
   クライアント分離(AP アイソレーション)が原因である可能性が高い。
   Android実機検証はこの制約により保留とした。
4. **SMB(WiFiルーターのUSBストレージ共有)**: ASUS RT-AC65Uの管理画面で
   Samba共有を確認・有効化(「Samba 共有」タブ、共有名
   `RT-AC65U-3FD8`)し、ポート445への実TCP到達を確認した上で、
   `smb2`crateによる実接続を試みた——**SMB2 Negotiateリクエスト送信
   直後にサーバー側から切断される**という結果になった。念のため
   Windows標準のSMBクライアント(`net use`)でも同じ共有への接続を
   試したところ、こちらも`System error 64`(接続不可)で失敗——
   **同じ症状が異なる2つのSMB2/3クライアント実装で再現した**ため、
   コード側の不具合ではなく、**このルーターのSamba実装がSMB1
   (旧世代プロトコル)のみに対応しており、SMB2/3専用の`smb2`crateとは
   そもそもプロトコルレベルで互換性が無い**と判断した(Windows 11は
   既定でSMB1クライアントが無効化されているため、Windows自身の
   `net use`も同じ理由で失敗したと推測される、こちらは管理者権限が
   必要な`Get-WindowsOptionalFeature`で最終確認までは至っていない)。
5. **正直な開示・今後の対応方針**: SMB1のみ対応の家庭用ルーターは
   相当数現存すると考えられるため、`SmbBackend`をSMB1(CIFS)にも
   対応させるか(`smb2`crateはSMB2/3専用のため、SMB1対応には別crateまたは
   `pavao`〈libsmbclient FFIバインディング、SMB1/2/3全対応〉への切り替えが
   必要)、あるいは「SMB2/3対応ルーター/NASのみサポート」と明確に
   スコープを絞るかは次回判断すること。今回検証した実機(ASUS
   RT-AC65U)はSMB1専用だったため、少なくとも現状の実装ではこの
   ルーターを使えないという実害が確認された。
  - 次にすべきこと: (1) `SmbBackend`のSMB1対応要否の判断(`pavao`crateへの
    切り替え検討、ただしlibsmbclientネイティブ依存が入るためクロス
    プラットフォームビルドへの影響を要確認)、(2) SMB2/3対応の別の
    ルーター/NAS実機があれば再検証、(3) Android実機検証はネットワーク
    分離の制約が無い環境(モバイルルーター等)で再挑戦、(4) Windows
    SMB1クライアント状態の管理者権限での最終確認。

## HANDOFF追記(2026-08-03続き3) Windows Task Scheduler登録を実際に試行、この開発環境固有の権限制約を発見(正直な開示)

この開発環境自体がWindowsであるため、`install.ps1`のTask Scheduler
登録部分を実際に動かして検証を試みた。

1. **実施**: `whoami`で現在のユーザーが`aon\norukia`(ドメインアカウント、
   非管理者、`IsInRole(Administrator)`が`False`)であることを確認した
   上で、`Register-ScheduledTask`を実際に呼び出した。
2. **結果**: `Access is denied`(HRESULT 0x80070005)で失敗。ログオン
   トリガーのみを使う per-user タスク登録は通常、管理者権限を要求しない
   Windows標準機能だが、このドメイン参加済みマシンにはグループポリシー等で
   Task Scheduler書き込みが追加制限されている可能性が高い(スクリプト側の
   実装不備によるものではなく、この特定の開発環境固有の権限制約と判断)。
3. **代替検証**: 生成されるlaunchd plist(macOS向け)のXML構文が正しい
   ことは`python -c "xml.dom.minidom.parse(...)"`で別途確認した(整形式
   であることの裏付け、実際のlaunchd登録自体はmacOS実機が必要)。
4. **正直な開示**: 上記により、`install.ps1`のTask Scheduler登録機能・
   `install.sh`のlaunchd登録機能とも、**このセッションでは実環境での
   最終確認(実際にタスクが起動しrs-syncプロセスが立ち上がること)が
   できていない**——コードレビュー+部分的な構文検証までに留まる。
   次回、管理者権限のある個人所有Windows機・実macOS環境で改めて確認
   すること。
  - 次にすべきこと: (1) 管理者権限のあるWindows環境でのTask Scheduler
    実地確認、(2) 実macOS環境でのlaunchd実地確認、(3) Android実機/
    エミュレータでの起動確認(未着手のまま継続)。

## HANDOFF追記(2026-08-03続き) SMBバックエンドを追加(WiFiルーターUSB共有・市販NAS対応)

ユーザー指示「端末間ドライブ同期+自動バックアップに、WiFiルーターに
接続のUSBメモリーや外付けHDDやNASなども対応...に着手して」への対応
(デスクトップ優先+WiFiルーターUSB/HDD対応を同時に進める方針、ユーザー
選択済み)。

1. **背景**: 家庭用WiFiルーター(バッファロー・NECアテルモ等)のUSB
   ストレージ共有機能・市販NASの大半はSSH/SFTPではなく**SMB
   (Windowsファイル共有、CIFS)**でのみアクセス可能。既存の
   `FolderEndpointKind`(`Local`/`Sftp`)では対応できなかった。
2. **実装**: `smb2`crate(Pure-Rust SMB2/3クライアント、`libsmbclient`等の
   ネイティブ依存無し)を追加、`FolderEndpointKind::Smb`+`SmbBackend`
   (既存の`FolderBackend`トレイトへ統一実装)を新設。`smb2`crateは
   tokio非同期APIのみのため、接続ごとに専用の使い捨てtokioランタイムを
   保持し`block_on`する設計(`open-easy-web`の`dist_sync.rs`が
   `SftpBackupTarget`で採用している「使い捨てランタイム」パターンと同じ)。
   `FolderEndpoint`に`share: String`フィールド(`#[serde(default)]`で
   既存データとの後方互換維持)を追加。UI(`static/index.html`)にも
   種類選択肢・共有名入力欄・日英ラベルを追加。
3. **検証(実測)**: `cargo build`/`cargo test --lib`36件全green
   (回帰無し)。VPS本番(`rs-sync`・`rs-sync-demo`両サービス)へも
   `git pull`→`cargo build --release`→`systemctl restart`まで実施、
   両サービスとも`active`、`https://easy-web.tokyo/rs-sync/`・
   `/rs-sync/demo/`とも200を確認。
4. **正直な開示**: この開発環境には実際のWiFiルーターUSB共有・市販NAS
   実機が無く、**SMB接続自体の実機検証はできていない**(`smb2`crateの
   公開APIどおりに実装したのみ、SFTPの時と違い実機での往復確認が
   まだ済んでいない)。次回、実際のルーター/NAS機器で接続確認を行うこと。
  - 次にすべきこと: (1) 実ルーター/NAS実機でのSMB接続確認、
    (2) デスクトップ(Windows/Linux/Mac)向けのインストーラー付き
    同期クライアント本体の設計・実装(今回はバックエンドのプロトコル
    対応のみ、常駐型クライアントアプリ自体は別途)、(3) iOS/iPadOS対応は
    `open-easy-web/IOS_FEASIBILITY_MEMO.md`に実現可能性調査を記録済み
    (今回は実装未着手、ユーザー指示により次回持ち越し)。

## HANDOFF追記(2026-08-03) フォルダ同期(SFTP)を実VPSで初検証、実バグ1件を発見・修正

ユーザー指示「RSyncのVPSやレンタルサーバーのフォルダーやGithub...
などの連携対応動作確認」の続き。これまで「実SFTPサーバーが無く未検証」
と記載していたフォルダ同期(`folder_sync.rs`)を、実際にconoha VPS
(SSH鍵認証、`~/.ssh/conoha-vps.pem`)へ接続して検証した。

1. **発見した実バグ**: `SftpBackend`の`read_file`/`write_file`/`stat`が
   `self.base.join(rel_path)`(`std::path::Path::join`)でリモートパスを
   組み立てていたが、**Windows開発機では`Path::join`/`push`がホストOSの
   セパレータ(バックスラッシュ)を挿入する**ため、既存の文字列が
   `/`区切りでも、結合点だけ`\`が混入したパス(例:
   `/root/dest\file.txt`)を生成していた。SFTPは常にUnix系のリモート
   パスを期待するため、これがそのままリモートサーバーへ送られると
   `no such file`(SFTP(2))で失敗する。ローカル↔ローカル同期
   (`LocalBackend`)はネイティブOSパスのままでよいため影響を受けず、
   これまでのテストで検出されなかった。
2. **修正**: `join_unix_path()`ヘルパーを新設し、`SftpBackend`側の全ての
   リモートパス組み立て(`read_file`/`write_file`/`stat`+親ディレクトリ
   作成ループ)をこの関数経由に統一。ホストOSに関わらず常に`/`区切りの
   文字列として結合してから`Path`化する。
3. **実機検証(実測、モックではない)**: 新規`#[ignore]`テスト
   `local_to_sftp_and_back_round_trips_against_a_real_vps`
   (open-redmineのGitLab/Bitbucket実ネットワークテストと同じパターン、
   `RS_SYNC_TEST_SFTP_*`環境変数で実VPSの接続情報を渡す)を追加し、
   実際に(a) ローカル→VPS(SFTP書き込み)、(b) VPS→別のローカル
   ディレクトリ(SFTP読み取り)の往復で内容が一致することを確認した。
   **ハマった点(正直な開示)**: Git Bash(MSYS)環境で`/root/...`形式の
   環境変数をそのまま渡すと、MSYSのパス自動変換機能が
   `C:/Program Files/Git/root/...`のようなWindowsパスへ勝手に書き換えて
   しまい、一見コードのバグのように見える別の失敗を引き起こした
   (`MSYS_NO_PATHCONV=1`で回避、今回の本当のバグ〈上記1〉とは別の
   テスト環境固有の罠として切り分けて記録)。`cargo test --lib`
   36件全green(回帰無し)。
4. **正直な開示・残る範囲**: (1) Gitea(OSS)・GitBucket・GitLab・
   Bitbucketの各プロバイダは引き続き実サーバーが無く未検証のまま
   (このリポジトリのスコープでは新規インフラ構築は行わない判断)。
   (2) フォルダ同期の双方向マージ・差分転送(rsync的なブロック単位
   転送)は引き続き対象外(モジュールdocの既存の正直な開示通り)。
  - 次にすべきこと: (1) 実際のSFTP folder syncジョブをUI経由で作成し、
    スケジューラ実行での実地確認(今回は`run_folder_sync()`を直接
    呼ぶ形の検証、UI/スケジューラ経由のE2Eは未実施)、(2) 他の
    プロバイダ(Gitea/GitBucket/GitLab/Bitbucket)の実サーバー確保。

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
