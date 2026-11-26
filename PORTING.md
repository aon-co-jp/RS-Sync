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

## REST全廃→Connect 移行記録(2026-11-05)
- 移した: 44本(`src/connect.rs`の`METHODS`)。例外: healthz 2、静的画面、Google OAuthコールバック。旧REST経路は削除。
- 画面(`static/index.html`)は`connectCall`で旧表記をConnect呼び出しへ変換。Androidに旧API呼び出しは無し。
- 統合テストの失敗は`set_current_dir`の並列競合で、直列化して解消(全テスト通過)。
- 本番(VPS)反映済み: `/root/rs-sync/target/release/rs-sync`(両サービス共用、バックアップ`.bak-20261105`)。公開URL`easy-web.tokyo/rs-sync/`でConnect(本番401・デモ200)・旧REST404を確認。VPSのcloneは`d5b839e`へff。

## HANDOFF追記(2026-11-08) 依存を最新の主要版へ更新

- 更新: rand 0.10 / sha2 0.11 / hmac 0.13 / sha1 0.11 / thiserror 2 / reqwest 0.13 / russh 0.65 / russh-sftp 3 / smb2 0.43(`cargo update`でロックも更新)。
- 新APIへの修正: `rand::random`/`random_range`(thread_rng廃止)、sha2の出力は自前で16進化、hmacは`KeyInit`をimport、reqwestのTLS機能は`rustls`(aws-lc-rs)・`form`を明示。
- 確認済み: `cargo test --no-fail-fast`全通過(83+15+4)、`cargo ndk`でarm64-v8a/x86_64のreleaseビルド成功(aws-lc-rsでも通る)。Android実機の起動確認は未実施。
- 実バグではなくテスト側の食い違い: `open_gitea_list_repos_sends_expected_path_and_bearer_header`はConnect化(2aa17fb)に追従しておらず`POST /open_gitea.v1.Gitea/ListRepos`へ修正した。
- 既知: 双方向同期の結合テスト`two_way_sync_propagates_dest_only_change_back_to_source`が、全テストを一括実行した直後に稀に落ちることがあった(単独5回・全体複数回では再現せず、更新前のロックでも1回発生)。原因は未特定。

## 追記(2026-11-16) スケジュール拡充・Googleカレンダー連携・アカウント設定・認証アプリログイン

- **スケジュール**: 毎週(曜日ごとに時刻)、毎月カレンダー(今月/来月・24時間表記・「毎日」チェック)、1年分の特定日(`specific_dates`、後から変更可)に対応。判定は`is_due_schedule`、テスト追加済み。
- **Googleカレンダー連携**: OAuth2(scope `calendar.events`、非公開予定)。同期予定の登録と、スケジュール解除時の「解除しました」予定(日英・日時つき)を自動作成。Googleドライブは中止。**利用者自身のGoogle CloudのClient ID/Secretが必要**で、実OAuthの通し動作は資格情報が無く未検証(状態応答の形のみ確認)。
- **アカウント設定**: 管理者メールの変更、予備メール(OTPを受け取れる2つ目)、電話番号(参照用のみ。SMS・ログインには未使用)。電話番号未登録でメールログイン中の利用者には登録を促す案内(日英)。
- **認証アプリでのログイン**(TOTP、RFC 6238自前実装): 「2段階認証」ではなく、**メールOTPまたは認証アプリのコードの"どちらか"でログインできる別経路**。UI表記を訂正済み(旧「2段階認証/2FA」→「認証アプリでのログイン」)。
- **UI/キャッシュ**: `Cache-Control: no-cache, must-revalidate`+一度だけの強制再読込、ブラウザ自動入力の抑止、間隔の「変更を保存」ボタン、データ量に応じたプリセットの灰色表示、タイムゾーン/国表示。
- **既知の制限**: 一度も実行していないジョブは次回同期予定を推定できない、旧キャッシュの画面は一度だけ手動の強制再読込が要る、`monthly_days`はバックエンドに残るがUIは`specific_dates`を書く。
- **未了**: Google OAuthの実機確認(資格情報待ち)、本節の表記訂正のVPS反映(別途デプロイ)。


## 再開用メモ(2026-11-15、別アカウントでの再開用) / Resume notes (for resuming on another account)

### 日本語

**到達点**: 依存を最新の主要版へ更新済み(`3b144cd`)。open-giteaテストの期待値をConnectへ追従(`177d68c`)。いずれもpush済み。`cargo test --no-fail-fast`全通過(83+15+4)、`cargo ndk`でarm64-v8a/x86_64のreleaseビルド成功。

**未了・未確認**:
1. 双方向同期の結合テスト`two_way_sync_propagates_dest_only_change_back_to_source`が稀に落ちる件の原因(未特定)。単独5回・全体複数回では再現せず。調査(日英のWeb検索+GitHub)で見つかった候補: (a) Windowsでウイルス対策/indexerが一時ファイルを掴み、git操作や削除が時々失敗する、(b) `DATA_DIR="data"`が相対パスで、動作が`set_current_dir`に左右される(結合テスト4件は`CWD_LOCK`で直列化済みだが、tempdir破棄後にカレントが消える形は残る)。次の手: `DATA_DIR`を注入可能にして`set_current_dir`を廃止、失敗時に`handle.exe`で掴んでいるプロセスを確認、失敗メッセージ(`second.message`)を必ず出力。
2. Android実機での起動確認(`cargo ndk`のビルドは成功、実機は未実施。`reqwest` 0.13の`rustls`はaws-lc-rs)。
3. `reqwest`のTLSをringに揃えるか(`rustls-no-provider`+手動設定)の判断は未実施。

**注意**: cargoはPATH前置きが要る(`/c/Users/noruk/.cargo/bin`)。全テストの初回は10分近くかかるので、前面で連続実行せず`run_in_background`を使う。Androidは`ANDROID_NDK_HOME=C:\Users\noruk\AppData\Local\Android\Sdk\ndk\27.1.12297006`。`git pull --rebase`で`Cargo.lock`が衝突したら、`git checkout --ours Cargo.lock`→`cargo update`→テスト→`git add`→`git rebase --continue`。モックのテストはAPI形式が変わると静かに古くなるので、形式変更(Connect化など)のたびに期待値を実サーバー呼び出しと合わせて確認する。

**そのまま貼り付ける再開用メッセージ**:
> `F:\RS-Sync`(aon-co-jp/RS-Sync、main)の`PORTING.md`末尾「再開用メモ(2026-11-15)」を読み、(1)不安定な結合テスト`two_way_sync_propagates_dest_only_change_back_to_source`の原因特定と修正(`DATA_DIR`の注入化、失敗時のログ出力)、(2)Android実機での起動確認、の順に進めてください。長いテストは`run_in_background`で。完了したらCLAUDE.md/PORTING.mdを日英で更新してpushしてください。

### English

**Where things stand**: dependencies were updated to the latest major versions (`3b144cd`) and the open-gitea test expectation was aligned with Connect (`177d68c`); both are pushed. `cargo test --no-fail-fast` passes (83+15+4) and `cargo ndk` release builds succeed for arm64-v8a and x86_64.

**Open / unverified**:
1. The cause of the occasional failure of the two-way sync integration test `two_way_sync_propagates_dest_only_change_back_to_source` is unknown. It did not reproduce in 5 single runs or several full runs. Candidates from the research (English/Japanese web search + GitHub): (a) on Windows, antivirus/indexers briefly hold temp files so git operations or deletions fail now and then; (b) `DATA_DIR="data"` is a relative path, so behaviour depends on `set_current_dir` (the four integration tests are serialised with `CWD_LOCK`, but the cwd can still point into a dropped tempdir). Next: make `DATA_DIR` injectable and drop `set_current_dir`; use `handle.exe` to find the process holding files at failure time; always print `second.message` on failure.
2. Launching on a real Android device is not checked (the `cargo ndk` build succeeds; `reqwest` 0.13 `rustls` uses aws-lc-rs).
3. Whether to align reqwest's TLS with ring (`rustls-no-provider` + manual setup) is undecided.

**Pitfalls**: cargo needs PATH (`/c/Users/noruk/.cargo/bin`). A cold full test run takes close to 10 minutes, so use `run_in_background` instead of looping in the foreground. Android: `ANDROID_NDK_HOME=C:\Users\noruk\AppData\Local\Android\Sdk\ndk\27.1.12297006`. If `git pull --rebase` conflicts on `Cargo.lock`: `git checkout --ours Cargo.lock`, `cargo update`, run the tests, `git add`, `git rebase --continue`. Mock-based tests go stale silently when an API shape changes (e.g. the Connect migration), so re-check expectations against a real server call whenever a shape changes.

**Paste-ready resume message**:
> Read "Resume notes (2026-11-15)" at the end of `PORTING.md` in `F:\RS-Sync` (aon-co-jp/RS-Sync, main). Then (1) find and fix the cause of the flaky integration test `two_way_sync_propagates_dest_only_change_back_to_source` (make `DATA_DIR` injectable, log failures), and (2) verify the app starts on a real Android device. Use `run_in_background` for long tests. When done, update CLAUDE.md / PORTING.md in Japanese and English and push.


## 2026-11-18 追記: ログイン方式3択(otp / otp_qr / qr)とQR確認ログイン / Login modes and QR-confirm login

**日本語**: 4択ログイン方式の横展開(RS-Sync分)を実装した。`otp`(既定、メールOTPのみ)・`otp_qr`(メールOTP+QR撮影=2段階認証)・`qr`(QR撮影のみ)。「パスワード無し」は、同期先の資格情報を扱う管理アプリのため**意図的に実装しない**。QRは秘密鍵・公開鍵を使わず、3分・1回限りのランダムトークンを含むURLだけを埋め込む(撮影端末でページが開くと自動で確認され、ボタン操作は不要)。
- 変更: `src/auth.rs`(QRセッション・`login_mode`、既定は再起動でotpへ戻る安全側)、`src/main.rs`(`VerifyOtp`は`otp_qr`でトークンでなくQRを返す、`QrLoginStart/Confirm/Status/Whoami/Finish`、`GetLoginMode`、`SetLoginMode`、`/qr-confirm.html`)、`src/connect.rs`(メソッド表に7件追加)、`src/totp.rs`(`text_qr_svg`)、`static/index.html`(ログイン画面のQR表示・ポーリング、設定パネルの方式選択)、`static/qr-confirm.html`(新規)。公開は引き続きConnect(`POST /rs_sync.Sync/<Method>`)のみで、RESTは追加していない。
- 締め出し防止: `qr`はメールOTPを無効にするため、認証アプリ(TOTP)が有効なときだけ選べ、`QrLoginStart`も登録メールを名乗り、かつTOTP有効のときだけQRを出す。`SetLoginMode`はログイン必須。
- **正直な開示**: `qr`単独は秘密を持たないため弱い(画面を見た人が確認URLを開ければ通る)。通常は`otp_qr`を推奨(画面にも明記)。
- 確認済み: `cargo test --locked`全通過(auth 2件・結合4件を追加、既存含め失敗0)。実ブラウザで`qr`モードを通し(QR表示→別タブで確認URLを開く→元のタブが自動でログイン状態)。
- **未了**: 実スマホのカメラで撮影した通し試験、実SMTPでの`otp_qr`通し(メールOTPは本人が受信)、本番VPSへの反映(rs-sync・rs-sync-demoの再ビルドと再起動、旧バイナリ退避)、`login_mode`の永続化(現状は再起動でotpへ戻る)。
- 注意: ローカルの`F:\runo\repository\RS-Sync`は`origin/main`と履歴が分岐している(ahead 57 / behind 78)ため、`git pull`せず、`F:\runo\repository\_work\RS-Sync`(`origin/main`の新規clone)で作業した。

**English**: The login-mode rollout reached RS-Sync: `otp` (default, email OTP only), `otp_qr` (email OTP + QR scan = real 2-step) and `qr` (QR scan only). "No password" is **intentionally not implemented** because this admin app holds sync credentials. The QR uses no public/private keys; it only encodes a URL with a random token that lives 3 minutes and works once (the scanning device confirms automatically when the page opens, no button press).
- Changed: `src/auth.rs` (QR sessions, `login_mode`; resets to `otp` on restart, the safe side), `src/main.rs` (`VerifyOtp` returns a QR instead of a token in `otp_qr`; `QrLoginStart/Confirm/Status/Whoami/Finish`, `GetLoginMode`, `SetLoginMode`, `/qr-confirm.html`), `src/connect.rs` (7 methods added), `src/totp.rs` (`text_qr_svg`), `static/index.html` (QR display and polling on the login screen, mode selector in settings), `static/qr-confirm.html` (new). Still Connect-only (`POST /rs_sync.Sync/<Method>`); no REST added.
- Lock-out guard: `qr` disables email OTP, so it can be selected only while the authenticator app (TOTP) is enabled, and `QrLoginStart` shows a QR only for a registered email with TOTP enabled. `SetLoginMode` requires login.
- **Honest disclosure**: `qr` alone holds no secret and is weak (anyone who sees the screen and opens the URL gets in). `otp_qr` is recommended (also stated in the UI).
- Verified: `cargo test --locked` all green (2 auth + 4 integration tests added, 0 failures). In a real browser the `qr` mode worked end to end (QR shown, confirm URL opened in another tab, the first tab logged in by itself).
- **Open**: a real phone-camera scan, `otp_qr` with real SMTP (the owner receives the OTP), deployment to the production VPS (rebuild and restart rs-sync / rs-sync-demo, keep the old binary), persisting `login_mode` (it currently resets to `otp` on restart).
- Note: the local `F:\runo\repository\RS-Sync` has diverged from `origin/main` (ahead 57 / behind 78), so do not `git pull` there; the work was done in `F:\runo\repository\_work\RS-Sync` (a fresh clone of `origin/main`).

**Paste-ready resume message / 再開用メッセージ**:
> RS-Syncの「2026-11-18 追記: ログイン方式3択」を読んでください(`PORTING.md`末尾、正本はRUNO `PORTING.md`の「2026-11-18 ログイン方式の横展開」)。(1) 実スマホでQR撮影の通し試験、(2) 本番VPSへ反映(rs-sync・rs-sync-demoを再ビルド・再起動、旧バイナリ退避)、(3) 残りの横展開: RS-Ops(全面REST、先にConnect化が必要)・open-gitea・open-redmine。作業は`origin/main`の新規cloneで行い、分岐した手元クローンで`git pull`しない。/ Read "2026-11-18 Addendum: login modes" at the end of `PORTING.md`. (1) Real phone-camera QR test, (2) deploy to the production VPS (rebuild/restart rs-sync and rs-sync-demo, keep the old binary), (3) remaining rollout: RS-Ops (all REST, needs Connect first), open-gitea, open-redmine. Work in a fresh clone of `origin/main`; never `git pull` in the diverged local clone.

## 2026-11-18 本番反映: ログイン方式(otp/otp_qr/qr)とQR確認ログイン / Production deploy: login modes and QR confirm login
**日本語**: `88a90b6` を WSL でリリースビルド(glibc 2.39、VPS と同じ上限)し、VPS の `rs-sync` と `rs-sync-demo` を入れ替えた(旧は `/root/rs-sync/target/release/rs-sync.before-login-20261118`、失敗時の自動ロールバック付き)。実測: 本番・デモ・公開URLの `GetLoginMode` が 200 `{"login_mode":"otp"}`、本番の認証なし `ListAccounts` は 401、デモは 200、旧 `/api/accounts` は 404、`/healthz` 200、`/qr-confirm.html` 200(公開URL経由も)、エラーログ 0。VPS の clone も `88a90b6`。未了: 実スマホのカメラでの QR 撮影、実 SMTP での `otp_qr` 通し、`login_mode` の永続化(再起動で `otp` に戻る)。
**English**: Release-built `88a90b6` in WSL (glibc 2.39, the VPS's ceiling) and swapped it into `rs-sync` and `rs-sync-demo` on the VPS (old binary `rs-sync.before-login-20261118`, with automatic rollback on failure). Verified: `GetLoginMode` returns 200 `{"login_mode":"otp"}` on production, demo and the public URL; unauthenticated `ListAccounts` is 401 on production and 200 on the demo; old `/api/accounts` is 404; `/healthz` 200; `/qr-confirm.html` 200 (also via the public URL); no error logs. The VPS clone is at `88a90b6`. Open: a real phone-camera QR test, an `otp_qr` run with real SMTP, and persisting `login_mode` (it resets to `otp` on restart).

## 2026-11-20 v0.2.0 リリース / Release v0.2.0
- **日本語**: v0.1.1 以降の33コミット(ログイン方式3択・QR確認ログイン・`login_mode` の永続化・open-gitea連携のConnect化・依存更新)を v0.2.0 として公開。版は `Cargo.toml` の `version`。タグ `v0.2.0` の push で `release.yml` が Linux/Windows の添付を作る。試験108件成功。本番VPSは `b204fdb` 相当で稼働中(バイナリの入れ替えは不要)。
- **English**: The 33 commits since v0.1.1 (three login modes, QR confirm login, persisted `login_mode`, open-gitea over Connect, dependency updates) are released as v0.2.0. The version lives in `Cargo.toml`; pushing tag `v0.2.0` makes `release.yml` build the Linux/Windows assets. 108 tests pass. Production already runs the `b204fdb` build (no swap needed).

## 2026-11-20 依存の更新(version-control通知) / Dependency update
- **日本語**: lettre 0.11.23→0.11.24(RUSTSEC-2026-0560〜0565: STARTTLS応答の注入・SMTPコマンド注入・EHLOでのpanic・宛先の取り違え・表示名のCR/LFでのpanic・応答の無制限バッファ)、qrcode 0.14→0.15。コードの変更は不要、`cargo test` 108件成功。
- **English**: lettre 0.11.23 -> 0.11.24 (RUSTSEC-2026-0560..0565: STARTTLS reply injection, SMTP command injection, EHLO panic, wrong recipient for quoted local parts, CR/LF display-name panic, unbounded reply buffering), qrcode 0.14 -> 0.15. No code changes needed; `cargo test` 108 passed.

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
