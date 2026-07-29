use rs_sync::auth::{AuthStore, RequestOtpOutcome};
use rs_sync::mail::{self, SmtpConfig};
use rs_sync::model::{self, Account, AppState, Direction, Schedule, SyncJob};
use rs_sync::scheduler;
use poem::{
    get, handler, http::StatusCode, listener::TcpListener, post, web::{Data, Json, Path as PPath},
    Endpoint, EndpointExt, IntoResponse, Middleware, Request, Response, Result as PoemResult, Route, Server,
};
use serde::Serialize;
use std::sync::Arc;

/// OTPログイン・セッション状態。`require_auth=false`のときは
/// (デモ環境向け)全ての認証チェックを素通りさせる——本番/デモの分離は
/// プロセス単位(別ポート・別`RS_SYNC_DATA`)で行い、この1プロセス内で
/// 常時オンかオフかのどちらかに固定される設計とする。
pub struct AuthState {
    pub auth: AuthStore,
    pub admin_email: String,
    pub smtp: Option<SmtpConfig>,
    pub require_auth: bool,
}

#[handler]
fn healthz() -> &'static str {
    "ok"
}

/// 2026-07-29追記(ユーザー指示「このようなBUGが起きないか定期的に自動
/// チェックする機能」): スケジューラのバックグラウンドループが実際に
/// 生きているかを外部から確認できるエンドポイント。
/// `scheduler_last_tick`は1分ごとに更新されるはずなので、3分(180秒)
/// 以上更新が無ければ異常("healthy": false)とみなす——今回発生した
/// 「ループが無言のまま永久停止」というバグを、次回は数分以内に検知
/// できるようにするための仕組み。認証不要(`/healthz`と同じ扱い、外部の
/// 死活監視ツールから直接叩けるようにするため)。
#[handler]
fn healthz_scheduler(state: Data<&Arc<AppState>>) -> Response {
    let last_tick = *state.scheduler_last_tick.lock().unwrap();
    let seconds_since_tick = (chrono::Utc::now() - last_tick).num_seconds();
    let healthy = seconds_since_tick < 180;
    let status = if healthy { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
    json_ok_with_status(
        status,
        &serde_json::json!({
            "healthy": healthy,
            "scheduler_last_tick": last_tick.to_rfc3339(),
            "seconds_since_tick": seconds_since_tick,
        }),
    )
}

fn json_ok_with_status(status: StatusCode, value: &serde_json::Value) -> Response {
    Response::builder().status(status).content_type("application/json").body(value.to_string())
}

/// GET /api/accountsのレスポンス専用のトークン非公開ビュー。
/// **セキュリティ上重要**: `Account.token`(GitHub PAT等の実トークン)を
/// 一覧APIへそのままシリアライズすると、ログイン済みブラウザは元より、
/// 認証を素通りさせるデモ環境では誰でも生トークンを閲覧できてしまう
/// (2026-07-29発見、修正)。一覧では常にマスクし、実トークン値は
/// サーバー内部(同期実行時)でのみ使用する。
#[derive(Serialize)]
struct AccountView {
    id: String,
    name: String,
    provider: model::ProviderKind,
    api_base: String,
    git_base: String,
    owner: String,
    /// 実トークンの代わりに、設定済みかどうかのみを返す。
    token_set: bool,
}

impl From<&Account> for AccountView {
    fn from(a: &Account) -> Self {
        Self {
            id: a.id.clone(),
            name: a.name.clone(),
            provider: a.provider.clone(),
            api_base: a.api_base.clone(),
            git_base: a.git_base.clone(),
            owner: a.owner.clone(),
            token_set: !a.token.is_empty(),
        }
    }
}

#[handler]
fn list_accounts(state: Data<&Arc<AppState>>) -> Json<Vec<AccountView>> {
    let store = state.store.lock().unwrap();
    Json(store.accounts.iter().map(AccountView::from).collect())
}

#[handler]
fn create_account(state: Data<&Arc<AppState>>, Json(mut acc): Json<Account>) -> Json<AccountView> {
    acc.id = model::new_id();
    let view = AccountView::from(&acc);
    {
        let mut store = state.store.lock().unwrap();
        store.accounts.push(acc);
    }
    state.save();
    Json(view)
}

#[handler]
fn delete_account(state: Data<&Arc<AppState>>, PPath(id): PPath<String>) -> Json<serde_json::Value> {
    {
        let mut store = state.store.lock().unwrap();
        store.accounts.retain(|a| a.id != id);
    }
    state.save();
    Json(serde_json::json!({"ok": true}))
}

/// アカウントのトークンだけを後から更新するためのリクエスト。
/// 2026-07-29追記(ユーザー指示「この中でトークンを入れられるように
/// 改善して」): それまでアカウントの更新手段が無く、トークンを後から
/// 設定するには削除して作り直す必要があった(実際にユーザーがトークン
/// 未設定のままアカウントを作り、GitHubの匿名アクセス制限〈60
/// リクエスト/時間〉に達してしまう実障害が発生した)。他のフィールドは
/// 変更頻度が低いためスコープ外とし、最も必要とされるトークン更新のみを
/// 対象にした最小実装。
#[derive(serde::Deserialize)]
struct UpdateAccountTokenReq {
    token: String,
}

#[handler]
fn update_account_token(
    state: Data<&Arc<AppState>>,
    PPath(id): PPath<String>,
    Json(body): Json<UpdateAccountTokenReq>,
) -> Response {
    let mut store = state.store.lock().unwrap();
    let Some(acc) = store.accounts.iter_mut().find(|a| a.id == id) else {
        return error_response(StatusCode::NOT_FOUND, "account not found");
    };
    acc.token = body.token;
    let view = AccountView::from(&*acc);
    drop(store);
    state.save();
    json_ok(&serde_json::to_value(view).unwrap())
}

#[derive(serde::Deserialize)]
struct NewJobReq {
    name: String,
    source_account_id: String,
    dest_account_id: String,
    direction: Direction,
    repos: Vec<String>,
    schedule: Schedule,
}

#[handler]
fn list_jobs(state: Data<&Arc<AppState>>) -> Json<Vec<SyncJob>> {
    let store = state.store.lock().unwrap();
    Json(store.jobs.clone())
}

#[handler]
fn create_job(state: Data<&Arc<AppState>>, Json(req): Json<NewJobReq>) -> Json<SyncJob> {
    let job = SyncJob {
        id: model::new_id(),
        name: req.name,
        source_account_id: req.source_account_id,
        dest_account_id: req.dest_account_id,
        direction: req.direction,
        repos: req.repos,
        schedule: req.schedule,
        enabled: true,
        last_run_at: None,
        last_run_status: None,
        conflicted_repos: vec![],
    };
    {
        let mut store = state.store.lock().unwrap();
        store.jobs.push(job.clone());
    }
    state.save();
    Json(job)
}

#[handler]
fn delete_job(state: Data<&Arc<AppState>>, PPath(id): PPath<String>) -> Json<serde_json::Value> {
    {
        let mut store = state.store.lock().unwrap();
        store.jobs.retain(|j| j.id != id);
    }
    state.save();
    Json(serde_json::json!({"ok": true}))
}

#[handler]
async fn run_job_now(state: Data<&Arc<AppState>>, PPath(id): PPath<String>) -> Json<serde_json::Value> {
    // 2026-07-29追記: `execute_job`はブロッキングI/O(reqwest::blocking+
    // `git`サブプロセス)を行うため、`spawn_blocking`経由で呼ぶ
    // (scheduler.rs側の同種の修正と同じ理由——直接呼ぶとこのリクエストを
    // 処理しているtokioワーカースレッドを、応答が返るまで丸ごと塞いで
    // しまう)。
    let state = state.clone();
    let outcome = tokio::task::spawn_blocking(move || scheduler::execute_job(state, &id))
        .await
        .unwrap_or(None);
    match outcome {
        Some(outcome) => Json(serde_json::json!({
            "success": outcome.success,
            "message": outcome.message,
        })),
        None => Json(serde_json::json!({
            "success": false,
            "message": "job or accounts not found",
        })),
    }
}

#[handler]
fn resolve_conflict(
    state: Data<&Arc<AppState>>,
    PPath((id, repo)): PPath<(String, String)>,
) -> Json<serde_json::Value> {
    let mut store = state.store.lock().unwrap();
    if let Some(job) = store.jobs.iter_mut().find(|j| j.id == id) {
        job.conflicted_repos.retain(|r| r != &repo);
        drop(store);
        state.save();
        return Json(serde_json::json!({"ok": true}));
    }
    Json(serde_json::json!({"ok": false}))
}

#[handler]
fn list_logs(state: Data<&Arc<AppState>>) -> Json<Vec<model::RunLogEntry>> {
    let store = state.store.lock().unwrap();
    let mut logs = store.logs.clone();
    logs.reverse();
    logs.truncate(100);
    Json(logs)
}

// ---------------- フォルダ同期(gitホスティング非依存、SFTP/ローカル) ----------------

#[handler]
fn list_folder_endpoints(state: Data<&Arc<AppState>>) -> Json<Vec<rs_sync::folder_sync::FolderEndpointView>> {
    let store = state.store.lock().unwrap();
    Json(store.folder_endpoints.iter().map(rs_sync::folder_sync::FolderEndpointView::from).collect())
}

#[handler]
fn create_folder_endpoint(
    state: Data<&Arc<AppState>>,
    Json(mut ep): Json<rs_sync::folder_sync::FolderEndpoint>,
) -> Json<rs_sync::folder_sync::FolderEndpointView> {
    ep.id = model::new_id();
    let view = rs_sync::folder_sync::FolderEndpointView::from(&ep);
    {
        let mut store = state.store.lock().unwrap();
        store.folder_endpoints.push(ep);
    }
    state.save();
    Json(view)
}

#[handler]
fn delete_folder_endpoint(state: Data<&Arc<AppState>>, PPath(id): PPath<String>) -> Json<serde_json::Value> {
    {
        let mut store = state.store.lock().unwrap();
        store.folder_endpoints.retain(|e| e.id != id);
    }
    state.save();
    Json(serde_json::json!({"ok": true}))
}

#[derive(serde::Deserialize)]
struct UpdateFolderEndpointCredentialsReq {
    #[serde(default)]
    password: String,
    #[serde(default)]
    private_key_path: String,
}

#[handler]
fn update_folder_endpoint_credentials(
    state: Data<&Arc<AppState>>,
    PPath(id): PPath<String>,
    Json(body): Json<UpdateFolderEndpointCredentialsReq>,
) -> Response {
    let mut store = state.store.lock().unwrap();
    let Some(ep) = store.folder_endpoints.iter_mut().find(|e| e.id == id) else {
        return error_response(StatusCode::NOT_FOUND, "folder endpoint not found");
    };
    ep.password = body.password;
    ep.private_key_path = body.private_key_path;
    let view = rs_sync::folder_sync::FolderEndpointView::from(&*ep);
    drop(store);
    state.save();
    json_ok(&serde_json::to_value(view).unwrap())
}

#[handler]
fn list_folder_jobs(state: Data<&Arc<AppState>>) -> Json<Vec<rs_sync::folder_sync::FolderSyncJob>> {
    let store = state.store.lock().unwrap();
    Json(store.folder_jobs.clone())
}

#[derive(serde::Deserialize)]
struct NewFolderJobReq {
    name: String,
    source_endpoint_id: String,
    dest_endpoint_id: String,
    schedule: Schedule,
}

#[handler]
fn create_folder_job(state: Data<&Arc<AppState>>, Json(req): Json<NewFolderJobReq>) -> Json<rs_sync::folder_sync::FolderSyncJob> {
    let job = rs_sync::folder_sync::FolderSyncJob {
        id: model::new_id(),
        name: req.name,
        source_endpoint_id: req.source_endpoint_id,
        dest_endpoint_id: req.dest_endpoint_id,
        repos: vec![],
        schedule: req.schedule,
        enabled: true,
        last_run_at: None,
        last_run_status: None,
    };
    {
        let mut store = state.store.lock().unwrap();
        store.folder_jobs.push(job.clone());
    }
    state.save();
    Json(job)
}

#[handler]
fn delete_folder_job(state: Data<&Arc<AppState>>, PPath(id): PPath<String>) -> Json<serde_json::Value> {
    {
        let mut store = state.store.lock().unwrap();
        store.folder_jobs.retain(|j| j.id != id);
    }
    state.save();
    Json(serde_json::json!({"ok": true}))
}

#[handler]
async fn run_folder_job_now(state: Data<&Arc<AppState>>, PPath(id): PPath<String>) -> Json<serde_json::Value> {
    // フォルダ同期はSFTP接続を含むブロッキングI/Oのため、既存のgit同期
    // ジョブと同じ理由(2026-07-29のスケジューラ無言停止バグの教訓)で
    // `spawn_blocking`経由で呼ぶ。
    let state = state.clone();
    let outcome = tokio::task::spawn_blocking(move || scheduler::execute_folder_job(state, &id)).await.unwrap_or(None);
    match outcome {
        Some(outcome) => Json(serde_json::json!({"success": outcome.success, "message": outcome.message})),
        None => Json(serde_json::json!({"success": false, "message": "job or endpoints not found"})),
    }
}

#[handler]
fn index() -> poem::web::Html<&'static str> {
    poem::web::Html(include_str!("../static/index.html"))
}

// ---------------- OTP認証エンドポイント ----------------

#[derive(serde::Deserialize)]
struct RequestOtpReq {
    email: String,
}

#[handler]
async fn request_otp(auth_state: Data<&Arc<AuthState>>, Json(body): Json<RequestOtpReq>) -> Response {
    let email = body.email.trim().to_string();
    if email != auth_state.admin_email {
        return error_response(StatusCode::FORBIDDEN, "登録されていない連絡先です。 / This contact is not registered.");
    }
    let Some(smtp) = auth_state.smtp.clone() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "SMTP未設定のため、このサーバーではOTP送信できません。 / SMTP is not configured on this server.",
        );
    };
    let RequestOtpOutcome::Issued(code) = auth_state.auth.request_otp(&email);
    match mail::send_otp(smtp, email, code).await {
        Ok(()) => json_ok(&serde_json::json!({"message_ja": "OTPを送信しました。", "message_en": "OTP sent."})),
        Err(e) => {
            tracing::warn!("failed to send OTP mail: {e}");
            error_response(StatusCode::BAD_GATEWAY, "メール送信に失敗しました。 / Failed to send mail.")
        }
    }
}

#[derive(serde::Deserialize)]
struct VerifyOtpReq {
    email: String,
    code: String,
}

#[handler]
fn verify_otp(auth_state: Data<&Arc<AuthState>>, Json(body): Json<VerifyOtpReq>) -> Response {
    match auth_state.auth.consume_otp(&body.email, &body.code) {
        Ok(()) => {
            let token = auth_state.auth.create_session(&body.email);
            json_ok(&serde_json::json!({"token": token}))
        }
        Err(e) => error_response(StatusCode::UNAUTHORIZED, e.message()),
    }
}

#[handler]
fn logout(auth_state: Data<&Arc<AuthState>>, req: &Request) -> Response {
    if let Some(token) = bearer_token(req) {
        auth_state.auth.logout(&token);
    }
    json_ok(&serde_json::json!({"ok": true}))
}

fn bearer_token(req: &Request) -> Option<String> {
    req.header("Authorization")?.strip_prefix("Bearer ").map(|s| s.to_string())
}

fn json_ok(value: &serde_json::Value) -> Response {
    Response::builder().status(StatusCode::OK).content_type("application/json").body(value.to_string())
}

fn error_response(status: StatusCode, message: &str) -> Response {
    Response::builder()
        .status(status)
        .content_type("application/json")
        .body(serde_json::json!({"error": message}).to_string())
}

/// `/api/`配下(認証系エンドポイント自身を除く)を保護するミドルウェア。
/// `AuthState::require_auth`が`false`(デモ環境)の場合は常に素通りする。
struct RequireAuth {
    auth_state: Arc<AuthState>,
}

impl<E: Endpoint> Middleware<E> for RequireAuth {
    type Output = RequireAuthEndpoint<E>;
    fn transform(&self, ep: E) -> Self::Output {
        RequireAuthEndpoint { ep, auth_state: self.auth_state.clone() }
    }
}

struct RequireAuthEndpoint<E> {
    ep: E,
    auth_state: Arc<AuthState>,
}

impl<E: Endpoint> Endpoint for RequireAuthEndpoint<E> {
    type Output = Response;
    async fn call(&self, req: Request) -> PoemResult<Self::Output> {
        if !self.auth_state.require_auth {
            return self.ep.call(req).await.map(IntoResponse::into_response);
        }
        let authorized = bearer_token(&req).is_some_and(|token| self.auth_state.auth.session_email(&token).is_some());
        if !authorized {
            return Ok(error_response(
                StatusCode::UNAUTHORIZED,
                "ログインが必要です。 / Login required.",
            ));
        }
        self.ep.call(req).await.map(IntoResponse::into_response)
    }
}

fn build_app(state: Arc<AppState>, auth_state: Arc<AuthState>) -> impl poem::Endpoint {
    let protected = Route::new()
        .at("/accounts", get(list_accounts).post(create_account))
        .at("/accounts/:id", poem::delete(delete_account).put(update_account_token))
        .at("/jobs", get(list_jobs).post(create_job))
        .at("/jobs/:id", poem::delete(delete_job))
        .at("/jobs/:id/run", post(run_job_now))
        .at("/jobs/:id/resolve/:repo", post(resolve_conflict))
        .at("/logs", get(list_logs))
        .at("/folder-endpoints", get(list_folder_endpoints).post(create_folder_endpoint))
        .at("/folder-endpoints/:id", poem::delete(delete_folder_endpoint).put(update_folder_endpoint_credentials))
        .at("/folder-jobs", get(list_folder_jobs).post(create_folder_job))
        .at("/folder-jobs/:id", poem::delete(delete_folder_job))
        .at("/folder-jobs/:id/run", post(run_folder_job_now))
        .with(RequireAuth { auth_state: auth_state.clone() });

    Route::new()
        .at("/", get(index))
        .at("/healthz", get(healthz))
        .at("/healthz/scheduler", get(healthz_scheduler))
        .at("/api/auth/request-otp", post(request_otp))
        .at("/api/auth/verify-otp", post(verify_otp))
        .at("/api/auth/logout", post(logout))
        .nest("/api", protected)
        .data(state)
        .data(auth_state)
}

#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
    tracing_subscriber::fmt::init();
    let data_file = std::env::var("RS_SYNC_DATA").unwrap_or_else(|_| "data/store.json".to_string());
    let state = Arc::new(AppState::new(&data_file));

    let require_auth = std::env::var("RS_SYNC_REQUIRE_AUTH").map(|v| v != "false" && v != "0").unwrap_or(true);
    let auth_state = Arc::new(AuthState {
        auth: AuthStore::default(),
        admin_email: std::env::var("RS_SYNC_ADMIN_EMAIL").unwrap_or_default(),
        smtp: SmtpConfig::from_env(),
        require_auth,
    });
    if !require_auth {
        tracing::warn!(
            "RS_SYNC_REQUIRE_AUTH=false: this instance has NO login protection on /api/* \
             (intended only for an isolated, no-real-tokens demo deployment)"
        );
    }

    let scheduler_state = state.clone();
    tokio::spawn(async move {
        scheduler::run_scheduler_loop(scheduler_state).await;
    });

    let port: u16 = std::env::var("RS_SYNC_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8095);
    let app = build_app(state, auth_state);
    tracing::info!("rs-sync listening on 0.0.0.0:{port} (auth required: {require_auth})");
    Server::new(TcpListener::bind(("0.0.0.0", port)))
        .run(app)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use poem::test::TestClient;

    fn make_state(tempdir: &tempfile::TempDir, require_auth: bool) -> (Arc<AppState>, Arc<AuthState>) {
        let data_file = tempdir.path().join("store.json");
        let state = Arc::new(AppState::new(data_file.to_str().unwrap()));
        let auth_state = Arc::new(AuthState {
            auth: AuthStore::default(),
            admin_email: "admin@example.com".to_string(),
            smtp: None,
            require_auth,
        });
        (state, auth_state)
    }

    #[tokio::test]
    async fn api_requires_login_by_default() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, auth_state) = make_state(&tmp, true);
        let app = build_app(state, auth_state);
        let client = TestClient::new(app);

        let resp = client.get("/api/accounts").send().await;
        resp.assert_status(StatusCode::UNAUTHORIZED);

        let resp = client.post("/api/accounts").body_json(&serde_json::json!({
            "id": "", "name": "x", "provider": "GitHub", "api_base": "https://api.github.com",
            "git_base": "https://github.com", "owner": "me", "token": "secret-token"
        })).send().await;
        resp.assert_status(StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn valid_session_can_reach_protected_api_and_account_list_never_exposes_the_raw_token() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, auth_state) = make_state(&tmp, true);
        let token = auth_state.auth.create_session("admin@example.com");
        let app = build_app(state, auth_state);
        let client = TestClient::new(app);

        let resp = client
            .post("/api/accounts")
            .header("Authorization", format!("Bearer {token}"))
            .body_json(&serde_json::json!({
                "id": "", "name": "x", "provider": "GitHub", "api_base": "https://api.github.com",
                "git_base": "https://github.com", "owner": "me", "token": "super-secret-pat"
            }))
            .send()
            .await;
        resp.assert_status_is_ok();
        let body_text = resp.0.into_body().into_string().await.unwrap();
        assert!(!body_text.contains("super-secret-pat"), "raw token leaked in create response: {body_text}");

        let resp = client.get("/api/accounts").header("Authorization", format!("Bearer {token}")).send().await;
        resp.assert_status_is_ok();
        let body_text = resp.0.into_body().into_string().await.unwrap();
        assert!(!body_text.contains("super-secret-pat"), "raw token leaked in list response: {body_text}");
        assert!(body_text.contains("token_set"));
    }

    /// 2026-07-29追記: `PUT /api/accounts/:id`でトークンだけを後から
    /// 更新できることの回帰確認(削除して作り直す必要をなくす)。
    #[tokio::test]
    async fn account_token_can_be_updated_without_recreating_the_account() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, auth_state) = make_state(&tmp, true);
        let token = auth_state.auth.create_session("admin@example.com");
        let app = build_app(state, auth_state);
        let client = TestClient::new(app);

        let create_resp = client
            .post("/api/accounts")
            .header("Authorization", format!("Bearer {token}"))
            .body_json(&serde_json::json!({
                "id": "", "name": "master", "provider": "GitHub", "api_base": "https://api.github.com",
                "git_base": "https://github.com", "owner": "aon-co-jp", "token": ""
            }))
            .send()
            .await;
        create_resp.assert_status_is_ok();
        let created: serde_json::Value = create_resp.0.into_body().into_json().await.unwrap();
        let id = created["id"].as_str().unwrap().to_string();

        // 更新前はtoken_set=false(空トークンのまま)。
        assert_eq!(created["token_set"], serde_json::json!(false));

        let update_resp = client
            .put(format!("/api/accounts/{id}"))
            .header("Authorization", format!("Bearer {token}"))
            .body_json(&serde_json::json!({"token": "ghp_newtoken"}))
            .send()
            .await;
        update_resp.assert_status_is_ok();
        let updated_body = update_resp.0.into_body().into_string().await.unwrap();
        assert!(!updated_body.contains("ghp_newtoken"), "raw token leaked in update response: {updated_body}");

        let list_resp = client.get("/api/accounts").header("Authorization", format!("Bearer {token}")).send().await;
        let list: serde_json::Value = list_resp.0.into_body().into_json().await.unwrap();
        assert_eq!(list[0]["token_set"], serde_json::json!(true));

        // 存在しないIDへの更新は404。
        let missing_resp = client
            .put("/api/accounts/does-not-exist")
            .header("Authorization", format!("Bearer {token}"))
            .body_json(&serde_json::json!({"token": "x"}))
            .send()
            .await;
        missing_resp.assert_status(StatusCode::NOT_FOUND);
    }

    /// 2026-07-29追記(ユーザー指示「レンタルサーバーやVPSやローカル
    /// ドライブやスマホやNASドライブなど同士でCOPYして同期を取れる
    /// システムも搭載して」): フォルダ同期の接続先(パスワード含む)を
    /// 作成すると、レスポンスに生パスワードが含まれず`credentials_set`
    /// のみが返ることを確認する(既存のトークン非公開パターンを踏襲)。
    #[tokio::test]
    async fn folder_endpoint_create_and_list_never_expose_raw_password() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, auth_state) = make_state(&tmp, true);
        let token = auth_state.auth.create_session("admin@example.com");
        let app = build_app(state, auth_state);
        let client = TestClient::new(app);

        let create_resp = client
            .post("/api/folder-endpoints")
            .header("Authorization", format!("Bearer {token}"))
            .body_json(&serde_json::json!({
                "id":"", "name":"nas", "kind":"Sftp", "path":"/backups",
                "host":"203.0.113.10", "port":22, "username":"root",
                "password":"super-secret-password", "private_key_path":""
            }))
            .send()
            .await;
        create_resp.assert_status_is_ok();
        let created_body = create_resp.0.into_body().into_string().await.unwrap();
        assert!(!created_body.contains("super-secret-password"), "raw password leaked: {created_body}");

        let list_resp = client.get("/api/folder-endpoints").header("Authorization", format!("Bearer {token}")).send().await;
        list_resp.assert_status_is_ok();
        let list_body = list_resp.0.into_body().into_string().await.unwrap();
        assert!(!list_body.contains("super-secret-password"), "raw password leaked in list: {list_body}");
        assert!(list_body.contains("\"credentials_set\":true"));

        // 未ログインでは401。
        let unauth = client.get("/api/folder-endpoints").send().await;
        unauth.assert_status(StatusCode::UNAUTHORIZED);
    }

    /// 2026-07-29追記: `GET /healthz/scheduler`が、直近のtickが新しければ
    /// healthy=true・503ではなく200を返すことを確認する(回帰確認)。
    /// 実際に「ハートビートが更新されなくなる」異常系は`scheduler.rs`側の
    /// ロジック(180秒しきい値)そのものに対するユニットテストではなく、
    /// ここでは「正常時に200かつhealthy:trueを返す」という基本契約のみを
    /// 検証する(スケジューラループ自体の統合テストは60秒間隔のtickを
    /// 待つ必要がありコストが高いため、意図的にスコープ外とした)。
    #[tokio::test]
    async fn healthz_scheduler_reports_healthy_when_tick_is_recent() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, auth_state) = make_state(&tmp, true);
        let app = build_app(state, auth_state);
        let client = TestClient::new(app);

        let resp = client.get("/healthz/scheduler").send().await;
        resp.assert_status_is_ok();
        let body: serde_json::Value = resp.0.into_body().into_json().await.unwrap();
        assert_eq!(body["healthy"], serde_json::json!(true));
    }

    #[tokio::test]
    async fn demo_mode_with_require_auth_false_skips_the_login_check() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, auth_state) = make_state(&tmp, false);
        let app = build_app(state, auth_state);
        let client = TestClient::new(app);

        let resp = client.get("/api/accounts").send().await;
        resp.assert_status_is_ok();
    }

    #[tokio::test]
    async fn otp_login_roundtrip_over_real_http() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, auth_state) = make_state(&tmp, true);
        // SMTP未設定なので実送信はできないが、request_otpを直接呼んで
        // コードを取得し、verify-otpエンドポイント自体の検証は行える。
        let RequestOtpOutcome::Issued(code) = auth_state.auth.request_otp("admin@example.com");
        let app = build_app(state, auth_state);
        let client = TestClient::new(app);

        let resp = client
            .post("/api/auth/verify-otp")
            .body_json(&serde_json::json!({"email": "admin@example.com", "code": code}))
            .send()
            .await;
        resp.assert_status_is_ok();

        let resp = client
            .post("/api/auth/verify-otp")
            .body_json(&serde_json::json!({"email": "admin@example.com", "code": "000000"}))
            .send()
            .await;
        resp.assert_status(StatusCode::UNAUTHORIZED);
    }
}
