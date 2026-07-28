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
fn run_job_now(state: Data<&Arc<AppState>>, PPath(id): PPath<String>) -> Json<serde_json::Value> {
    match scheduler::execute_job(state.clone(), &id) {
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
        .at("/accounts/:id", poem::delete(delete_account))
        .at("/jobs", get(list_jobs).post(create_job))
        .at("/jobs/:id", poem::delete(delete_job))
        .at("/jobs/:id/run", post(run_job_now))
        .at("/jobs/:id/resolve/:repo", post(resolve_conflict))
        .at("/logs", get(list_logs))
        .with(RequireAuth { auth_state: auth_state.clone() });

    Route::new()
        .at("/", get(index))
        .at("/healthz", get(healthz))
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
