# PORTING.md — RS-Sync を他プロジェクト/他環境へお引越しする際のガイド

このファイル1枚で、RS-Syncを別のVPS/PC/開発環境へ導入・移設できるように
まとめる。対象バージョン: 2026-08-12時点(`main`ブランチ)。

## 0. このリポジトリのスコープ

GitHub / open-gitea / Gitea / GitBucket / GitLab / Bitbucket間でリポジトリを
一方向バックアップ同期・双方向同期するWebアプリ(Rust + [poem](https://github.com/poem-web/poem))。
gitホスティング非依存の単純なフォルダ間コピー(ローカル/SFTP/SMB)にも
対応する。詳細な機能一覧・プロバイダ対応状況は[README.md](README.md)、
開発経緯の詳細は[CLAUDE.md](CLAUDE.md)のHANDOFF節を参照。

他リポジトリへのpath依存・git依存は無い(単一クレート、`Cargo.toml`の
依存は全てcrates.io公開クレート)——このリポジトリ単体でclone・
ビルド・移設が完結する。

## 1. 持っていくもの(ファイル一覧)

```
rs-sync/
├── Cargo.toml / Cargo.lock
├── src/
│   ├── main.rs           # HTTPハンドラ・ルーティング・OTP認証ミドルウェア
│   ├── model.rs           # Account/SyncJob/Schedule/AppState、JSON永続化
│   ├── provider.rs        # GitHub/OpenGitea/Gitea/GitBucket/GitLab/Bitbucket
│   ├── sync_engine.rs      # git clone --mirror / push --mirrorの実行
│   ├── scheduler.rs        # 毎分ポーリング・自動間隔延長・排他ロック
│   ├── folder_sync.rs       # gitホスティング非依存のフォルダ同期(Local/Sftp/Smb)
│   ├── auth.rs / mail.rs     # OTPログイン・SMTP送信
│   └── rustjson.rs          # 緩い構文のJSONパーサー(store.json読み込み用)
├── static/index.html        # 日英2言語UI(SPA、`include_str!`でバイナリに埋め込み)
├── tests/sync_integration.rs  # 実git bareリポジトリでの一方向/双方向同期テスト
├── android/                 # Androidクライアント(リモート/ローカル実行両対応)
├── install.sh / install.ps1   # Linux/macOS/Windows向けインストーラー
├── .github/workflows/release.yml # タグpushでLinux/Windowsバイナリを自動リリース
├── PORTING.md(本ファイル)
└── CLAUDE.md
```

**`static/index.html`は`src/main.rs`の`include_str!("../static/
index.html")`でバイナリへコンパイル時埋め込みされる**——実行時に
ファイルを読みに行く方式ではないため、**HTMLだけを直接VPSへ上書きしても
反映されない。必ず`cargo build --release`で再ビルドしてからデプロイする
こと**(2026-08-12、この節を追記するきっかけになった実際の見落とし
パターン——修正がJSだけであっても再ビルドが必須)。

## 2. ビルド

```bash
cargo build --release
# 生成物: target/release/rs-sync(Windowsは.exe)
```

クロスコンパイル用の追加ツールチェーンは不要(pure-Rust依存のみ、
`ssh2`〈OpenSSL依存〉は2026-08-07に`russh`/`russh-sftp`へ置き換え済み)。
Android向けクロスビルドは`cargo ndk -t arm64-v8a -t x86_64 build
--release --bin rs-sync`(`android/app/src/main/jniLibs/`へ配置)。

## 3. 実行時の環境変数

| 変数 | 既定値 | 用途 |
|---|---|---|
| `RS_SYNC_DATA` | `data/store.json` | JSON永続化ファイルのパス |
| `RS_SYNC_PORT` | `8095` (本番運用では`8096`を使用) | listenポート |
| `RS_SYNC_REQUIRE_AUTH` | `true` | `false`でOTP認証を無効化(ループバック限定の検証用途のみ推奨) |
| `RS_SYNC_READ_ONLY` | `false` | `true`で`/api/*`のGET以外を403拒否(認証不要デモ環境向け) |
| `RS_SYNC_ADMIN_EMAIL` | なし | OTPログインを許可する唯一の管理者メールアドレス |
| `RS_SYNC_SMTP_*` | なし | OTPメール送信用SMTP設定一式(`mail.rs`参照) |
| `RS_SYNC_PRIMARY_HEALTHZ_URL` | なし | 副系(スマホ等)にのみ設定。主系(VPS)の`/healthz`を指すと、主系が生きている間は副系側の自動実行をスキップ |

## 4. VPSデプロイの実際の手順(本リポジトリでの実運用パターン)

```bash
ssh <vps>
cd /root/rs-sync
git pull
cargo build --release
systemctl restart rs-sync.service rs-sync-demo.service
curl -s -o /dev/null -w '%{http_code}\n' https://<domain>/rs-sync/
```

- `rs-sync.service`(本番、認証必須)・`rs-sync-demo.service`
  (デモ、`RS_SYNC_REQUIRE_AUTH=false`+`RS_SYNC_READ_ONLY=true`、
  本番とは別の`RS_SYNC_DATA`)は同一の`WorkingDirectory`・同一バイナリを
  共有するため、1回のビルドで両方に反映される。
- **デプロイのたびに必ず`cargo build --release`から行うこと**
  (上記1節の`include_str!`の罠を参照)。`systemctl restart`だけでは
  古いバイナリのまま(=古い`static/index.html`のまま)動き続ける。
- デプロイ後は`curl`でのステータスコード確認だけでなく、可能であれば
  実際にブラウザ(またはローカルでの`RS_SYNC_REQUIRE_AUTH=false`起動+
  Claude Browser pane等)で新機能の見た目・動作まで確認すること
  (このリポジトリで2026-08-12、間隔編集欄の入力保持バグ等、curl確認
  だけでは検出できない実バグを複数発見した実績あり)。

## 5. データ永続化・移行

- `data/store.json`(gitignore対象)——アカウント(トークン含む)・
  同期ジョブ・フォルダ同期ジョブ・実行ログを1ファイルに保持。
  読み込みは[RustJSON](src/rustjson.rs)(緩い構文のJSONスーパーセット、
  open-redmine/RPoemから移植)経由、書き込みは標準の
  pretty-printed JSON。
- 新フィールド追加時は`#[serde(default)]`を付けて既存データとの後方
  互換を維持する(このリポジトリの一貫した方針、`SyncJob`/
  `FolderSyncJob`の各追加フィールド参照)。
- `data/cache/<ジョブID>/<リポジトリ名>.git`——gitの永続キャッシュ
  (bareミラー)。初回のみ`git clone --mirror`、以降は`git fetch
  --prune`で差分のみ取得する設計のため、**このディレクトリを消すと
  次回同期が全リポジトリ分フルクローンし直しになる**(移設時は
  コピーを検討、必須ではないが初回同期が長くなる)。

## 6. 命名規約

- クレート名/バイナリ名: `rs-sync`
- systemd unit: `rs-sync.service` / `rs-sync-demo.service`
- 環境変数prefix: `RS_SYNC_*`
- Androidパッケージ名: `tokyo.runo.rssync`

## 7. 移植・拡張時の注意(このリポジトリで実際に踏んだ罠)

- **スケジューラのtickと手動「今すぐ同期」の競合**: 同じジョブに対して
  自動実行と手動実行が同時に走ると、同じgitキャッシュディレクトリへ
  同時に`git clone --mirror`しようとして競合する(`cannot copy
  '...hooks/pre-applypatch.sample': File exists`)。`AppState.
  running_jobs`(ジョブID単位の簡易排他ロック)で対策済み——同種の
  「スケジューラ+手動トリガー」構成を他リポジトリへ移植する際は、
  同じ排他制御パターンを検討すること。
- **Windows環境でのSFTPリモートパス組み立て**: `std::path::Path::join`は
  ホストOSのパスセパレータを使うため、Windows開発機で`/`区切りの
  リモートパスへ結合すると`\`が混入する。`join_unix_path()`
  (`folder_sync.rs`)のように、SFTP等常にUnix形式を期待する相手には
  文字列結合で明示的に`/`区切りにすること。
- **トークンをエラーメッセージ・URLにそのまま含めない**: `git`コマンドの
  引数(`https://<token>@host/...`)・標準エラー出力はいずれもトークンを
  含みうる。`redact_credentials()`(`sync_engine.rs`)のようなマスク処理を
  必ず経由してからログ・API応答・UIへ渡すこと(2026-07-30に実際に本番で
  トークンが平文で画面に漏れた事故の教訓)。
- **フロントエンドの背景自動更新が編集中の入力欄を上書きする罠**:
  定期的な`renderAll()`のような全体再描画は、ユーザーが入力中のフォーム
  要素ごと消し飛ばす。編集中の要素IDをクライアント側のSetで記憶し、
  再描画時にその状態・入力値を保持するパターン(`static/index.html`の
  `openScheduleEdits`)を、同種の「定期更新+インライン編集」を持つ他
  リポジトリへ移植する際も検討すること。

## 8. テスト

```bash
cargo test --lib                                   # 単体テスト
cargo test --bin rs-sync                            # main.rsの統合テスト(HTTPハンドラ)
cargo test --test sync_integration -- --test-threads=1  # 実git bareリポジトリでの同期テスト(cwd変更のためシングルスレッド必須)
```

## 9. 関連プロジェクト

- [open-gitea](https://github.com/aon-co-jp/open-gitea) — 自前Git forge、RS-Syncのプロバイダの1つ
- [open-raid-z](https://github.com/aon-co-jp/open-raid-z) — 開発ルールの正本

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
