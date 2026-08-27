use rs_sync::auth::{AuthStore, RequestOtpOutcome};
use rs_sync::mail::{self, SmtpConfig};
use rs_sync::model::{self, Account, AppState, Direction, Schedule, SyncJob};
use rs_sync::scheduler;
use poem::{
    get, handler, http::Method, http::StatusCode, listener::TcpListener, post, web::{Data, Json, Path as PPath},
    Endpoint, EndpointExt, IntoResponse, Middleware, Request, Response, Result as PoemResult, Route, Server,
};
use serde::Serialize;
use std::sync::{Arc, Mutex};

/// OTPログイン・セッション状態。`require_auth=false`のときは
/// (デモ環境向け)全ての認証チェックを素通りさせる——本番/デモの分離は
/// プロセス単位(別ポート・別`RS_SYNC_DATA`)で行い、この1プロセス内で
/// 常時オンかオフかのどちらかに固定される設計とする。
pub struct AuthState {
    pub auth: AuthStore,
    /// 2026-08-13追記(ユーザー指示「ログインのe-mailアドレスも変更可能に
    /// して」): 従来は起動時に`RS_SYNC_ADMIN_EMAIL`環境変数から読んだ
    /// 固定値だったが、ログイン中の管理者自身が画面から変更できるように
    /// `Mutex`化した。変更内容は`AppState.store`(`admin_email_override`)
    /// にも永続化し、再起動後も新しいメールアドレスでログインできる
    /// ようにする(環境変数は初回起動時の初期値としてのみ機能する、
    /// `RS_SYNC_PRIMARY_HEALTHZ_URL`等の既存の「環境変数は初期値、
    /// 一度APIで変更されたら永続化した値が優先」パターンを踏襲)。
    pub admin_email: Mutex<String>,
    /// 2026-08-13追記(ユーザー指示「予備メールアドレスでメールアドレスを
    /// 二個登録する事も可能にして」): 主メールアドレスに加えて、OTP
    /// ログインを受け付けるもう1つの連絡先。`request_otp`は主/予備
    /// いずれのメールアドレスも受け付ける。
    pub backup_email: Mutex<Option<String>>,
    /// 2026-08-13追記: 万一メールアドレスを忘れた際の本人確認用連絡先
    /// (表示・案内のみ、SMS送信機能は持たない)。
    pub admin_phone: Mutex<Option<String>>,
    /// 2026-08-13追記(ユーザー指示「e-mail不明になっても2FAでもログイン
    /// 可能にして」): TOTP(認証アプリ型2段階認証)の秘密鍵と有効/無効。
    /// 有効な間は`POST /api/auth/totp-login`でメールOTPを経由せず
    /// ログインできる——メールアドレスへの依存を無くす代替経路。
    pub totp_secret: Mutex<Option<String>>,
    pub totp_enabled: Mutex<bool>,
    pub smtp: Option<SmtpConfig>,
    pub require_auth: bool,
    /// 2026-07-30追記(ユーザー指示「このデモでは実際にはログインも
    /// ログアウトも登録も保存も出来ないようにして」): デモ環境
    /// (`RS_SYNC_READ_ONLY=true`)では、閲覧(GET)以外の全ての
    /// `/api/*`リクエスト(ログイン/ログアウト自体を含む)を
    /// サーバー側で拒否する。UIの見た目だけを制限するのではなく、
    /// 直接APIを叩かれても実際に何も変更・保存できないことを保証する
    /// のが目的。
    pub read_only: bool,
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

// ---------------- Googleカレンダー連携(2026-08-13新設) ----------------
// ユーザー指示「設定したカレンダーから次の予定など1年分の月日と時間を
// 保存出来るようにして、そのDATAは...GoogleアカウントでログインしてGoogle
// カレンダーに保存して連動も可能」への対応。Googleドライブは対象外
// (ユーザー指示「Googleドライブは中止」)。

#[handler]
fn google_calendar_status(state: Data<&Arc<AppState>>) -> Json<serde_json::Value> {
    let configured = rs_sync::google_calendar::GoogleOAuthConfig::from_env().is_some();
    let connected = state.store.lock().unwrap().google_tokens.is_some();
    Json(serde_json::json!({"configured": configured, "connected": connected}))
}

/// 実バグ修正(実装中に発見): このエンドポイントはBearerセッション
/// トークン必須の保護下にあるため、単純な`<a href>`によるブラウザの
/// 素のナビゲーションではAuthorizationヘッダーが付与されず401になる。
/// JSON({"url": "..."})でGoogleの認可URLを返し、呼び出し側
/// (`api()`で認証済みfetch)が`window.location.href`へ代入して
/// 手動で遷移させる設計にした(HTTPリダイレクトそのものは使わない)。
#[handler]
fn google_calendar_login(state: Data<&Arc<AppState>>) -> Response {
    let Some(config) = rs_sync::google_calendar::GoogleOAuthConfig::from_env() else {
        return json_ok_with_status(
            StatusCode::SERVICE_UNAVAILABLE,
            &serde_json::json!({"error": "Google OAuth is not configured on this server (RS_SYNC_GOOGLE_CLIENT_ID/RS_SYNC_GOOGLE_CLIENT_SECRET)"}),
        );
    };
    let state_token = model::new_id();
    *state.google_oauth_state.lock().unwrap() = Some(state_token.clone());
    let url = rs_sync::google_calendar::build_auth_url(&config, &state_token);
    json_ok_with_status(StatusCode::OK, &serde_json::json!({"url": url}))
}

#[derive(serde::Deserialize)]
struct GoogleCallbackQuery {
    code: Option<String>,
    state: Option<String>,
}

/// GoogleのOAuthリダイレクト先。ブラウザが直接遷移してくるためこちらの
/// Bearerセッショントークンは付与されない——`state`パラメータの一致
/// (`google_calendar_login`で発行したワンタイム値)のみでCSRFを防ぐ。
#[handler]
fn google_calendar_callback(
    state: Data<&Arc<AppState>>,
    poem::web::Query(q): poem::web::Query<GoogleCallbackQuery>,
) -> Response {
    let expected = state.google_oauth_state.lock().unwrap().take();
    let (Some(code), Some(got_state)) = (q.code, q.state) else {
        return json_ok_with_status(StatusCode::BAD_REQUEST, &serde_json::json!({"error": "missing code/state"}));
    };
    if expected.as_deref() != Some(got_state.as_str()) {
        return json_ok_with_status(StatusCode::BAD_REQUEST, &serde_json::json!({"error": "state mismatch"}));
    }
    let Some(config) = rs_sync::google_calendar::GoogleOAuthConfig::from_env() else {
        return json_ok_with_status(StatusCode::SERVICE_UNAVAILABLE, &serde_json::json!({"error": "not configured"}));
    };
    match rs_sync::google_calendar::exchange_code_for_tokens(&config, &code) {
        Ok(tokens) => {
            let mut store = state.store.lock().unwrap();
            store.google_tokens = Some(tokens);
            drop(store);
            state.save();
            Response::builder().status(StatusCode::FOUND).header("Location", "../").body(())
        }
        Err(e) => {
            tracing::warn!("google token exchange failed: {e}");
            json_ok_with_status(StatusCode::BAD_GATEWAY, &serde_json::json!({"error": e.to_string()}))
        }
    }
}

/// 指定したジョブの`schedule.specific_dates`(特定日付スケジュール)を
/// Googleカレンダーへそれぞれ予定として作成する。同期成功後は
/// `google_synced=true`を立て、以後このジョブのスケジュールが特定日付
/// 方式でなくなったり削除されたりした際に、解除通知イベントを書き込む
/// 対象として扱われるようになる。
fn sync_schedule_to_google_calendar_blocking(
    state: &Arc<AppState>,
    job_name: &str,
    schedule: &Schedule,
) -> Result<usize, String> {
    let config = rs_sync::google_calendar::GoogleOAuthConfig::from_env()
        .ok_or_else(|| "not configured".to_string())?;
    let tokens = state
        .store
        .lock()
        .unwrap()
        .google_tokens
        .clone()
        .ok_or_else(|| "not connected".to_string())?;
    let refreshed = rs_sync::google_calendar::refresh_if_needed(&config, &tokens).map_err(|e| e.to_string())?;
    if refreshed.access_token != tokens.access_token {
        let mut store = state.store.lock().unwrap();
        store.google_tokens = Some(refreshed.clone());
        drop(store);
        state.save();
    }
    let (Some(h), Some(m)) = (schedule.specific_dates_hour, schedule.specific_dates_minute) else {
        return Err("no specific dates configured".to_string());
    };
    let mut created = 0;
    for date in &schedule.specific_dates {
        let summary = format!("RS-Sync: {job_name}");
        rs_sync::google_calendar::create_event_with_id(&refreshed.access_token, date, h, m, &summary)
            .map_err(|e| e.to_string())?;
        created += 1;
    }
    Ok(created)
}

#[handler]
async fn sync_job_to_google_calendar(state: Data<&Arc<AppState>>, PPath(id): PPath<String>) -> Json<serde_json::Value> {
    let state = state.clone();
    let job = { state.store.lock().unwrap().jobs.iter().find(|j| j.id == id).cloned() };
    let Some(job) = job else {
        return Json(serde_json::json!({"success": false, "message": "job not found"}));
    };
    let state2 = state.clone();
    let job_name = job.name.clone();
    let schedule = job.schedule.clone();
    let result = tokio::task::spawn_blocking(move || sync_schedule_to_google_calendar_blocking(&state2, &job_name, &schedule))
        .await
        .unwrap_or_else(|e| Err(e.to_string()));
    match result {
        Ok(count) => {
            let mut store = state.store.lock().unwrap();
            if let Some(j) = store.jobs.iter_mut().find(|j| j.id == id) {
                j.google_synced = true;
            }
            drop(store);
            state.save();
            Json(serde_json::json!({"success": true, "message": format!("synced {count} event(s)")}))
        }
        Err(e) => Json(serde_json::json!({"success": false, "message": e})),
    }
}

#[handler]
async fn sync_folder_job_to_google_calendar(state: Data<&Arc<AppState>>, PPath(id): PPath<String>) -> Json<serde_json::Value> {
    let state = state.clone();
    let job = { state.store.lock().unwrap().folder_jobs.iter().find(|j| j.id == id).cloned() };
    let Some(job) = job else {
        return Json(serde_json::json!({"success": false, "message": "job not found"}));
    };
    let state2 = state.clone();
    let job_name = job.name.clone();
    let schedule = job.schedule.clone();
    let result = tokio::task::spawn_blocking(move || sync_schedule_to_google_calendar_blocking(&state2, &job_name, &schedule))
        .await
        .unwrap_or_else(|e| Err(e.to_string()));
    match result {
        Ok(count) => {
            let mut store = state.store.lock().unwrap();
            if let Some(j) = store.folder_jobs.iter_mut().find(|j| j.id == id) {
                j.google_synced = true;
            }
            drop(store);
            state.save();
            Json(serde_json::json!({"success": true, "message": format!("synced {count} event(s)")}))
        }
        Err(e) => Json(serde_json::json!({"success": false, "message": e})),
    }
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
    token_expires_at: Option<chrono::DateTime<chrono::Utc>>,
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
            token_expires_at: a.token_expires_at,
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
    // 実バグ修正(2026-08-12、ユーザー報告「401 Unauthorized」への対応):
    // トークン欄はクライアント側(static/index.html)で.trim()するよう
    // 修正したが、Android版アプリやAPI直接呼び出し等、他のクライアント
    // 経由でも確実に効くよう、サーバー側でも防御的にtrimする。コピペ時に
    // 紛れ込む前後の空白/改行がそのままGitHub APIへ送られると、GitHub
    // 側は資格情報として無効と判断し401を返す(スコープ不足の403とは
    // 異なる)。
    acc.token = acc.token.trim().to_string();
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
    /// 2026-07-29追記: トークンの有効期限(任意、ISO8601)。GitHub等は
    /// トークン発行時にユーザー自身が期限を選ぶため、この値も自動取得は
    /// できず入力してもらう前提。
    #[serde(default)]
    token_expires_at: Option<chrono::DateTime<chrono::Utc>>,
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
    // 実バグ修正(2026-08-12): create_accountと同じ防御的trim(コピペ時の
    // 前後の空白/改行によるGitHub 401 Unauthorizedを防ぐ)。
    acc.token = body.token.trim().to_string();
    acc.token_expires_at = body.token_expires_at;
    // トークンを更新したら、古い期限に対する警告済みフラグをリセットする
    // (新しいトークン/新しい期限に対して、また期限が近づいたら通知すべきため)。
    acc.expiry_warning_sent = false;
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
    let initial_realtime_poll_minutes = req.schedule.realtime_poll_minutes;
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
        initial_realtime_poll_minutes,
        google_synced: false,
    };
    {
        let mut store = state.store.lock().unwrap();
        store.jobs.push(job.clone());
    }
    state.save();
    Json(job)
}

/// ユーザー指示(2026-08-12「チェックする間隔を何分に設定したかも
/// 表示する機能と変更可能な機能」)への対応: これまでジョブ作成後は
/// スケジュールを変更する手段が無く、間隔を変えたい場合は削除して
/// 作り直すしかなかった。スケジュールのみを差し替えるエンドポイントを
/// 新設(名前・アカウント・方向・対象リポジトリは変更不可のまま——
/// それらの変更は既存の「削除して作り直す」運用を維持し、今回は
/// スケジュールに限定してスコープを絞った)。
#[derive(serde::Deserialize)]
struct UpdateScheduleReq {
    schedule: Schedule,
}

/// 2026-08-13追記(ユーザー指示「rs-syncの設定を解除したら、設定を
/// 解除しましたと、その日時をGoogleカレンダーに自動で英語と日本語で
/// 書いて」): 特定日付スケジュールがGoogleカレンダーへ同期済み
/// (`google_synced`)だったジョブの、その設定が外れた(スケジュールが
/// 特定日付方式でなくなった、またはジョブ自体が削除された)ときに、
/// 「解除しました」という記録用イベントを"今"の日時でカレンダーへ
/// 書き込む。トークン未接続・資格情報未設定の場合は静かに何もしない
/// (この通知はベストエフォートの補助機能であり、本来の同期処理を
/// 妨げてはならないため)。
fn google_notify_cancelled_blocking(state: &Arc<AppState>, job_name: &str) {
    let Some(config) = rs_sync::google_calendar::GoogleOAuthConfig::from_env() else {
        return;
    };
    let tokens_opt = { state.store.lock().unwrap().google_tokens.clone() };
    let Some(tokens) = tokens_opt else {
        return;
    };
    let refreshed = match rs_sync::google_calendar::refresh_if_needed(&config, &tokens) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("google token refresh failed while notifying cancellation: {e}");
            return;
        }
    };
    if refreshed.access_token != tokens.access_token {
        let mut store = state.store.lock().unwrap();
        store.google_tokens = Some(refreshed.clone());
        drop(store);
        state.save();
    }
    if let Err(e) = rs_sync::google_calendar::create_cancellation_event(&refreshed.access_token, job_name) {
        tracing::warn!("failed to write google calendar cancellation event: {e}");
    }
}

fn schedule_has_specific_dates(s: &Schedule) -> bool {
    s.specific_dates_hour.is_some() && s.specific_dates_minute.is_some() && !s.specific_dates.is_empty()
}

#[handler]
async fn update_job_schedule(
    state: Data<&Arc<AppState>>,
    PPath(id): PPath<String>,
    Json(req): Json<UpdateScheduleReq>,
) -> Json<serde_json::Value> {
    let state = state.clone();
    let notify = {
        let mut store = state.store.lock().unwrap();
        let Some(job) = store.jobs.iter_mut().find(|j| j.id == id) else {
            return Json(serde_json::json!({"ok": false}));
        };
        let was_synced = job.google_synced;
        let had_specific_dates = schedule_has_specific_dates(&job.schedule);
        let name = job.name.clone();
        job.schedule = req.schedule;
        let still_has_specific_dates = schedule_has_specific_dates(&job.schedule);
        let should_notify = was_synced && had_specific_dates && !still_has_specific_dates;
        if should_notify {
            job.google_synced = false;
        }
        should_notify.then_some(name)
    };
    state.save();
    if let Some(name) = notify {
        let state2 = state.clone();
        tokio::task::spawn_blocking(move || google_notify_cancelled_blocking(&state2, &name))
            .await
            .ok();
    }
    Json(serde_json::json!({"ok": true}))
}

#[handler]
async fn delete_job(state: Data<&Arc<AppState>>, PPath(id): PPath<String>) -> Json<serde_json::Value> {
    let state = state.clone();
    let notify = {
        let mut store = state.store.lock().unwrap();
        let removed = store.jobs.iter().find(|j| j.id == id).cloned();
        store.jobs.retain(|j| j.id != id);
        removed.filter(|j| j.google_synced).map(|j| j.name)
    };
    state.save();
    if let Some(name) = notify {
        let state2 = state.clone();
        tokio::task::spawn_blocking(move || google_notify_cancelled_blocking(&state2, &name))
            .await
            .ok();
    }
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
    let initial_realtime_poll_minutes = req.schedule.realtime_poll_minutes;
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
        initial_realtime_poll_minutes,
        google_synced: false,
    };
    {
        let mut store = state.store.lock().unwrap();
        store.folder_jobs.push(job.clone());
    }
    state.save();
    Json(job)
}

#[handler]
async fn update_folder_job_schedule(
    state: Data<&Arc<AppState>>,
    PPath(id): PPath<String>,
    Json(req): Json<UpdateScheduleReq>,
) -> Json<serde_json::Value> {
    let state = state.clone();
    let notify = {
        let mut store = state.store.lock().unwrap();
        let Some(job) = store.folder_jobs.iter_mut().find(|j| j.id == id) else {
            return Json(serde_json::json!({"ok": false}));
        };
        let was_synced = job.google_synced;
        let had_specific_dates = schedule_has_specific_dates(&job.schedule);
        let name = job.name.clone();
        job.schedule = req.schedule;
        let still_has_specific_dates = schedule_has_specific_dates(&job.schedule);
        let should_notify = was_synced && had_specific_dates && !still_has_specific_dates;
        if should_notify {
            job.google_synced = false;
        }
        should_notify.then_some(name)
    };
    state.save();
    if let Some(name) = notify {
        let state2 = state.clone();
        tokio::task::spawn_blocking(move || google_notify_cancelled_blocking(&state2, &name))
            .await
            .ok();
    }
    Json(serde_json::json!({"ok": true}))
}

#[handler]
async fn delete_folder_job(state: Data<&Arc<AppState>>, PPath(id): PPath<String>) -> Json<serde_json::Value> {
    let state = state.clone();
    let notify = {
        let mut store = state.store.lock().unwrap();
        let removed = store.folder_jobs.iter().find(|j| j.id == id).cloned();
        store.folder_jobs.retain(|j| j.id != id);
        removed.filter(|j| j.google_synced).map(|j| j.name)
    };
    state.save();
    if let Some(name) = notify {
        let state2 = state.clone();
        tokio::task::spawn_blocking(move || google_notify_cancelled_blocking(&state2, &name))
            .await
            .ok();
    }
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

/// 2026-08-26追記(installer_sync機能): 現在の設定・直近の実行結果・
/// このホストで実際に動作可能かどうか(環境変数`RS_SYNC_INSTALLER_SYNC_
/// ENABLED`・`RS_SYNC_INSTALLER_ROOT`が揃っているか)を返す。設定APIとは
/// 別にしているのは、環境変数は`Store`(JSON)には保存されないため
/// (VPS/Windowsそれぞれのホスト側で設定するもの)、設定画面が
/// 「Storeの設定はONだが、このホストの環境変数が未設定なので実際には
/// 動いていません」という食い違いを正直に表示できるようにするため。
#[handler]
fn installer_sync_status(state: Data<&Arc<AppState>>) -> Json<serde_json::Value> {
    let store = state.store.lock().unwrap();
    let s = &store.installer_sync;
    Json(serde_json::json!({
        "settings": {
            "enabled": s.enabled,
            "interval_minutes": s.interval_minutes,
            "last_run_at": s.last_run_at,
        },
        "last_outcomes": s.last_outcomes,
        "host_env_ready": rs_sync::installer_sync::env_enabled() && rs_sync::installer_sync::configured_root().is_some(),
        "host_master_switch_env": rs_sync::installer_sync::env_enabled(),
        "host_root_configured": rs_sync::installer_sync::configured_root().map(|p| p.display().to_string()),
        "repo_count": rs_sync::installer_sync::installer_sync_repos().len(),
    }))
}

#[derive(serde::Deserialize)]
struct UpdateInstallerSyncReq {
    enabled: bool,
    #[serde(default)]
    interval_minutes: Option<u32>,
}

#[handler]
fn update_installer_sync_config(state: Data<&Arc<AppState>>, Json(req): Json<UpdateInstallerSyncReq>) -> Json<serde_json::Value> {
    {
        let mut store = state.store.lock().unwrap();
        store.installer_sync.enabled = req.enabled;
        if let Some(mins) = req.interval_minutes {
            store.installer_sync.interval_minutes = mins.max(1);
        }
    }
    state.save();
    Json(serde_json::json!({"ok": true}))
}

/// 手動での即時実行(スケジューラの15分待ちを待たずに動作確認したい
/// 場合用)。ブロッキングI/O(cargo build・ISCC・git)のため他ジョブと
/// 同じく`spawn_blocking`経由。環境変数が未設定なら「このホストでは
/// 利用不可」という正直なメッセージを返すのみで、Store側の設定を
/// 勝手に有効化したりはしない。
#[handler]
async fn run_installer_sync_now(state: Data<&Arc<AppState>>) -> Json<serde_json::Value> {
    if !rs_sync::installer_sync::env_enabled() {
        return Json(serde_json::json!({
            "ok": false,
            "message": "RS_SYNC_INSTALLER_SYNC_ENABLED=1 がこのホストの環境変数に設定されていません。 / RS_SYNC_INSTALLER_SYNC_ENABLED=1 is not set on this host."
        }));
    }
    let Some(root) = rs_sync::installer_sync::configured_root() else {
        return Json(serde_json::json!({
            "ok": false,
            "message": "RS_SYNC_INSTALLER_ROOT がこのホストの環境変数に設定されていません。 / RS_SYNC_INSTALLER_ROOT is not set on this host."
        }));
    };
    let state = state.clone();
    let prev_heads = {
        let store = state.store.lock().unwrap();
        store.installer_sync.last_synced_heads.clone()
    };
    let outcomes = tokio::task::spawn_blocking(move || {
        let (outcomes, new_heads) = rs_sync::installer_sync::run_installer_sync_pass(&root, &prev_heads);
        let mut store = state.store.lock().unwrap();
        store.installer_sync.last_run_at = Some(chrono::Utc::now());
        store.installer_sync.last_outcomes = outcomes.clone();
        store.installer_sync.last_synced_heads = new_heads;
        drop(store);
        state.save();
        outcomes
    })
    .await
    .unwrap_or_default();
    Json(serde_json::json!({"ok": true, "outcomes": outcomes}))
}

#[handler]
fn index() -> Response {
    // 実バグ修正(2026-08-13、ユーザー報告「F5キーを押したら正常になった」):
    // このHTMLはRustバイナリに`include_str!`でコンパイル時埋め込みされて
    // おり、レスポンスに`Cache-Control`ヘッダーが一切無かったため、
    // ブラウザが古いHTML/JS(修正前のフォーム自動入力バグを含むバージョン)
    // を独自の判断でキャッシュし続け、手動リロード(F5)しない限り修正が
    // 反映されなかった。デプロイのたびに毎回ユーザーへ「F5を押してくだ
    // さい」と案内しなくて済むよう、常に再検証させるヘッダーを付与する。
    poem::web::Html(include_str!("../static/index.html"))
        .with_header("Cache-Control", "no-cache, must-revalidate")
        .into_response()
}

// ---------------- OTP認証エンドポイント ----------------

#[derive(serde::Deserialize)]
struct RequestOtpReq {
    email: String,
}

#[handler]
async fn request_otp(auth_state: Data<&Arc<AuthState>>, Json(body): Json<RequestOtpReq>) -> Response {
    let email = body.email.trim().to_string();
    // 2026-08-13追記(ユーザー指示「予備メールアドレスでメールアドレスを
    // 二個登録する事も可能にして」): 主メールアドレスに加え、予備
    // メールアドレスもOTP要求を受け付ける。
    let is_registered = email == *auth_state.admin_email.lock().unwrap()
        || auth_state.backup_email.lock().unwrap().as_deref() == Some(email.as_str());
    if !is_registered {
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

/// 2026-08-13追記(ユーザー指示「ログインのe-mailアドレスも変更可能に
/// して」): ログイン中の管理者が自身のメールアドレスを画面上で変更
/// できるようにする。既存のセッショントークンは有効なまま(ログイン中に
/// 突然セッションを失わせない)——次回以降のログイン(OTP要求)から
/// 新しいメールアドレス宛にのみコードが送られるようになる。
#[handler]
fn get_admin_email(auth_state: Data<&Arc<AuthState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({"admin_email": *auth_state.admin_email.lock().unwrap()}))
}

#[derive(serde::Deserialize)]
struct UpdateAdminEmailReq {
    admin_email: String,
}

#[handler]
fn update_admin_email(
    state: Data<&Arc<AppState>>,
    auth_state: Data<&Arc<AuthState>>,
    Json(req): Json<UpdateAdminEmailReq>,
) -> Response {
    let new_email = req.admin_email.trim().to_string();
    if new_email.is_empty() || !new_email.contains('@') {
        return error_response(StatusCode::BAD_REQUEST, "無効なメールアドレスです。 / Invalid email address.");
    }
    *auth_state.admin_email.lock().unwrap() = new_email.clone();
    {
        let mut store = state.store.lock().unwrap();
        store.admin_email_override = Some(new_email.clone());
    }
    state.save();
    json_ok(&serde_json::json!({"ok": true, "admin_email": new_email}))
}

/// 2026-08-13追記(ユーザー指示「初回にe-mailと携帯電話番号も登録して、
/// e-mail不明になっても2FAでもログイン可能にして」): 携帯電話番号は
/// SMS送信を行わない(表示・保管のみ)——万一メールアドレスを忘れた際、
/// 管理者自身の本人確認の記録として使う想定。
#[handler]
fn get_admin_phone(auth_state: Data<&Arc<AuthState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({"admin_phone": *auth_state.admin_phone.lock().unwrap()}))
}

#[derive(serde::Deserialize)]
struct UpdateAdminPhoneReq {
    admin_phone: String,
}

#[handler]
fn update_admin_phone(
    state: Data<&Arc<AppState>>,
    auth_state: Data<&Arc<AuthState>>,
    Json(req): Json<UpdateAdminPhoneReq>,
) -> Response {
    let phone = req.admin_phone.trim().to_string();
    let value = if phone.is_empty() { None } else { Some(phone) };
    *auth_state.admin_phone.lock().unwrap() = value.clone();
    {
        let mut store = state.store.lock().unwrap();
        store.admin_phone_override = value.clone();
    }
    state.save();
    json_ok(&serde_json::json!({"ok": true, "admin_phone": value}))
}

/// 2026-08-13追記(ユーザー指示「予備メールアドレスでメールアドレスを
/// 二個登録する事も可能にして」): 主メールアドレスとは別に、OTP要求を
/// 受け付けるもう1つの連絡先を登録する。
#[handler]
fn get_backup_email(auth_state: Data<&Arc<AuthState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({"backup_email": *auth_state.backup_email.lock().unwrap()}))
}

#[derive(serde::Deserialize)]
struct UpdateBackupEmailReq {
    backup_email: String,
}

#[handler]
fn update_backup_email(
    state: Data<&Arc<AppState>>,
    auth_state: Data<&Arc<AuthState>>,
    Json(req): Json<UpdateBackupEmailReq>,
) -> Response {
    let email = req.backup_email.trim().to_string();
    if !email.is_empty() && !email.contains('@') {
        return error_response(StatusCode::BAD_REQUEST, "無効なメールアドレスです。 / Invalid email address.");
    }
    let value = if email.is_empty() { None } else { Some(email) };
    *auth_state.backup_email.lock().unwrap() = value.clone();
    {
        let mut store = state.store.lock().unwrap();
        store.backup_email = value.clone();
    }
    state.save();
    json_ok(&serde_json::json!({"ok": true, "backup_email": value}))
}

/// TOTPセットアップ: 新しい秘密鍵を生成して返す(まだ有効化しない
/// ——ユーザーが認証アプリへ登録し、実際に生成されたコードで
/// `totp/enable`を呼んで初めて有効になる、という「動くことを確認して
/// から切り替える」流れ)。
///
/// 2026-08-27追記(ユーザー指示「携帯電話番号でQRコード撮影の2FA」への
/// 対応): レスポンスへ`qr_svg`(認証アプリのカメラで読み取れるQRコード、
/// SVG文字列)を追加した。**従来のテキスト`secret`はそのまま維持**
/// (QRを読み取れないガラケー等の端末では引き続き手入力で登録できる、
/// open-easy-web側と同じ「テキストとQRの両方を提供する」設計方針)。
/// QR生成に失敗した場合でも`secret`自体は返す(致命的エラーにしない
/// ——テキスト手入力の経路は生かしたまま、`qr_svg`のみ`null`にする)。
#[handler]
fn totp_setup(auth_state: Data<&Arc<AuthState>>) -> Json<serde_json::Value> {
    let secret = rs_sync::totp::generate_secret();
    *auth_state.totp_secret.lock().unwrap() = Some(secret.clone());
    let account_label = auth_state.admin_email.lock().unwrap().clone();
    let qr_svg = rs_sync::totp::totp_qr_svg(&secret, &account_label, "RS-Sync").ok();
    Json(serde_json::json!({"secret": secret, "qr_svg": qr_svg}))
}

#[derive(serde::Deserialize)]
struct TotpCodeReq {
    code: String,
}

#[handler]
fn totp_enable(
    state: Data<&Arc<AppState>>,
    auth_state: Data<&Arc<AuthState>>,
    Json(req): Json<TotpCodeReq>,
) -> Response {
    let Some(secret) = auth_state.totp_secret.lock().unwrap().clone() else {
        return error_response(StatusCode::BAD_REQUEST, "先に2段階認証のセットアップを開始してください。 / Start TOTP setup first.");
    };
    let now = chrono::Utc::now().timestamp();
    if !rs_sync::totp::verify_code(&secret, &req.code, now) {
        return error_response(StatusCode::UNAUTHORIZED, "コードが一致しません。 / Code does not match.");
    }
    *auth_state.totp_enabled.lock().unwrap() = true;
    {
        let mut store = state.store.lock().unwrap();
        store.totp_secret = Some(secret);
        store.totp_enabled = true;
    }
    state.save();
    json_ok(&serde_json::json!({"ok": true}))
}

#[handler]
fn totp_disable(state: Data<&Arc<AppState>>, auth_state: Data<&Arc<AuthState>>) -> Response {
    *auth_state.totp_enabled.lock().unwrap() = false;
    *auth_state.totp_secret.lock().unwrap() = None;
    {
        let mut store = state.store.lock().unwrap();
        store.totp_enabled = false;
        store.totp_secret = None;
    }
    state.save();
    json_ok(&serde_json::json!({"ok": true}))
}

#[handler]
fn totp_status(auth_state: Data<&Arc<AuthState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({"enabled": *auth_state.totp_enabled.lock().unwrap()}))
}

/// 2026-08-13追記(ユーザー指示「e-mail不明になっても2FAでもログイン
/// 可能にして」): メールOTPを一切経由しない代替ログイン経路。TOTPが
/// 有効化されていなければ拒否する(2要素目が存在しないアカウントに
/// 対してこの経路自体を無意味に開けておかない)。
#[handler]
fn totp_login(auth_state: Data<&Arc<AuthState>>, Json(req): Json<TotpCodeReq>) -> Response {
    if !*auth_state.totp_enabled.lock().unwrap() {
        return error_response(StatusCode::FORBIDDEN, "2段階認証が有効になっていません。 / TOTP is not enabled.");
    }
    let Some(secret) = auth_state.totp_secret.lock().unwrap().clone() else {
        return error_response(StatusCode::FORBIDDEN, "2段階認証が有効になっていません。 / TOTP is not enabled.");
    };
    let now = chrono::Utc::now().timestamp();
    if !rs_sync::totp::verify_code(&secret, &req.code, now) {
        return error_response(StatusCode::UNAUTHORIZED, "コードが一致しません。 / Code does not match.");
    }
    let email = auth_state.admin_email.lock().unwrap().clone();
    let token = auth_state.auth.create_session(&email);
    json_ok(&serde_json::json!({"token": token}))
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

/// `/api/`配下全体(認証系エンドポイント自身も含む)を対象に、
/// `AuthState::read_only`が`true`のときGET以外の全メソッドを拒否する
/// (2026-07-30追記)。`RequireAuth`より外側(先)に効かせることで、
/// 「読み取り専用デモではログイン試行自体もサーバー側で意味を持たない」
/// ことを保証する——`RequireAuth`は`require_auth=false`なら素通りする
/// 設計のため、それだけでは書き込み系エンドポイントを止められない。
struct ReadOnlyGuard {
    read_only: bool,
}

impl<E: Endpoint> Middleware<E> for ReadOnlyGuard {
    type Output = ReadOnlyGuardEndpoint<E>;
    fn transform(&self, ep: E) -> Self::Output {
        ReadOnlyGuardEndpoint { ep, read_only: self.read_only }
    }
}

struct ReadOnlyGuardEndpoint<E> {
    ep: E,
    read_only: bool,
}

impl<E: Endpoint> Endpoint for ReadOnlyGuardEndpoint<E> {
    type Output = Response;
    async fn call(&self, req: Request) -> PoemResult<Self::Output> {
        if self.read_only && req.method() != Method::GET {
            return Ok(error_response(
                StatusCode::FORBIDDEN,
                "これは閲覧専用のデモ環境です。ログイン・登録・保存・実行はできません。 / \
                 This is a read-only demo environment; login, registration, saving, and running jobs are all disabled.",
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
        .at("/jobs/:id", poem::delete(delete_job).put(update_job_schedule))
        .at("/jobs/:id/run", post(run_job_now))
        .at("/jobs/:id/resolve/:repo", post(resolve_conflict))
        .at("/logs", get(list_logs))
        .at("/folder-endpoints", get(list_folder_endpoints).post(create_folder_endpoint))
        .at("/folder-endpoints/:id", poem::delete(delete_folder_endpoint).put(update_folder_endpoint_credentials))
        .at("/folder-jobs", get(list_folder_jobs).post(create_folder_job))
        .at("/folder-jobs/:id", poem::delete(delete_folder_job).put(update_folder_job_schedule))
        .at("/folder-jobs/:id/run", post(run_folder_job_now))
        .at("/folder-jobs/:id/sync-google-calendar", post(sync_folder_job_to_google_calendar))
        .at("/jobs/:id/sync-google-calendar", post(sync_job_to_google_calendar))
        .at("/google/status", get(google_calendar_status))
        .at("/google/login", get(google_calendar_login))
        .at("/auth/admin-email", get(get_admin_email).put(update_admin_email))
        .at("/auth/backup-email", get(get_backup_email).put(update_backup_email))
        .at("/auth/admin-phone", get(get_admin_phone).put(update_admin_phone))
        .at("/auth/totp/setup", post(totp_setup))
        .at("/auth/totp/enable", post(totp_enable))
        .at("/auth/totp/disable", post(totp_disable))
        .at("/auth/totp/status", get(totp_status))
        .at("/installer-sync/status", get(installer_sync_status))
        .at("/installer-sync/config", poem::put(update_installer_sync_config))
        .at("/installer-sync/run", post(run_installer_sync_now))
        .with(RequireAuth { auth_state: auth_state.clone() });

    let api = Route::new()
        .at("/auth/request-otp", post(request_otp))
        .at("/auth/verify-otp", post(verify_otp))
        .at("/auth/logout", post(logout))
        // ユーザー指示「e-mail不明になっても2FAでもログイン可能にして」:
        // メールOTPを経由しない代替ログイン経路のため、セッション未取得の
        // 状態から呼べるよう未認証のまま公開する(TOTPコード自体が
        // 認証情報のため、`RequireAuth`は不要かつ本末転倒になる)。
        .at("/auth/totp-login", post(totp_login))
        // GoogleのOAuthリダイレクトはこちらのBearerトークンを運べないため
        // 未認証のまま公開する(CSRF対策は`state`パラメータの一致のみ)。
        .at("/auth/google/callback", get(google_calendar_callback))
        .nest("/", protected)
        .with(ReadOnlyGuard { read_only: auth_state.read_only });

    Route::new()
        .at("/", get(index))
        .at("/healthz", get(healthz))
        .at("/healthz/scheduler", get(healthz_scheduler))
        .nest("/api", api)
        .data(state)
        .data(auth_state)
}

#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
    tracing_subscriber::fmt::init();
    let data_file = std::env::var("RS_SYNC_DATA").unwrap_or_else(|_| "data/store.json".to_string());
    let state = Arc::new(AppState::new(&data_file));

    let require_auth = std::env::var("RS_SYNC_REQUIRE_AUTH").map(|v| v != "false" && v != "0").unwrap_or(true);
    let read_only = std::env::var("RS_SYNC_READ_ONLY").map(|v| v == "true" || v == "1").unwrap_or(false);
    let (initial_admin_email, initial_backup_email, initial_admin_phone, initial_totp_secret, initial_totp_enabled) = {
        let store = state.store.lock().unwrap();
        (
            store.admin_email_override.clone().unwrap_or_else(|| std::env::var("RS_SYNC_ADMIN_EMAIL").unwrap_or_default()),
            store.backup_email.clone(),
            store.admin_phone_override.clone(),
            store.totp_secret.clone(),
            store.totp_enabled,
        )
    };
    let auth_state = Arc::new(AuthState {
        auth: AuthStore::default(),
        admin_email: Mutex::new(initial_admin_email),
        backup_email: Mutex::new(initial_backup_email),
        admin_phone: Mutex::new(initial_admin_phone),
        totp_secret: Mutex::new(initial_totp_secret),
        totp_enabled: Mutex::new(initial_totp_enabled),
        smtp: SmtpConfig::from_env(),
        require_auth,
        read_only,
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

    // 2026-08-19新設(自動アップデート機能): サーバー自体の起動を
    // ブロックしない非同期タスクとして起動する。既定は無効
    // (`RS_SYNC_SELF_UPDATE=true`が必要、詳細は`self_update.rs`
    // モジュールdoc「正直な開示」参照)。
    tokio::spawn(async move {
        rs_sync::self_update::check_and_apply_update().await;
    });

    let port: u16 = std::env::var("RS_SYNC_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8095);
    let app = build_app(state, auth_state);
    tracing::info!("rs-sync listening on 0.0.0.0:{port} (auth required: {require_auth}, read only: {read_only})");
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
            admin_email: Mutex::new("admin@example.com".to_string()),
            backup_email: Mutex::new(None),
            admin_phone: Mutex::new(None),
            totp_secret: Mutex::new(None),
            totp_enabled: Mutex::new(false),
            smtp: None,
            require_auth,
            read_only: false,
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

    /// 2026-08-12追記(ユーザー報告「同期ジョブでERROR: 401
    /// Unauthorized」への対応): コピペ時に紛れ込む前後の空白/改行付きの
    /// トークンを送っても、サーバー側で自動的にtrimされ、GitHub APIへは
    /// 素のトークンのみが渡ることの回帰確認。`create_account`・
    /// `update_account_token`の両経路を検証する。
    #[tokio::test]
    async fn account_tokens_are_trimmed_of_surrounding_whitespace_on_create_and_update() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, auth_state) = make_state(&tmp, true);
        let token = auth_state.auth.create_session("admin@example.com");
        let state_for_inspection = state.clone();
        let app = build_app(state, auth_state);
        let client = TestClient::new(app);

        let create_resp = client
            .post("/api/accounts")
            .header("Authorization", format!("Bearer {token}"))
            .body_json(&serde_json::json!({
                "id": "", "name": "master", "provider": "GitHub", "api_base": "https://api.github.com",
                "git_base": "https://github.com", "owner": "aon-co-jp", "token": "  ghp_pastedwithwhitespace\n"
            }))
            .send()
            .await;
        create_resp.assert_status_is_ok();
        let created: serde_json::Value = create_resp.0.into_body().into_json().await.unwrap();
        let id = created["id"].as_str().unwrap().to_string();

        {
            let store = state_for_inspection.store.lock().unwrap();
            let acc = store.accounts.iter().find(|a| a.id == id).unwrap();
            assert_eq!(acc.token, "ghp_pastedwithwhitespace", "token was not trimmed on create: {:?}", acc.token);
        }

        let update_resp = client
            .put(format!("/api/accounts/{id}"))
            .header("Authorization", format!("Bearer {token}"))
            .body_json(&serde_json::json!({"token": "\tghp_anothertoken  "}))
            .send()
            .await;
        update_resp.assert_status_is_ok();

        let store = state_for_inspection.store.lock().unwrap();
        let acc = store.accounts.iter().find(|a| a.id == id).unwrap();
        assert_eq!(acc.token, "ghp_anothertoken", "token was not trimmed on update: {:?}", acc.token);
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

    /// 2026-08-12追記(ユーザー指示「チェックする間隔を何分に設定したかも
    /// 表示する機能と変更可能な機能」)への対応: `PUT /api/folder-jobs/:id`で
    /// ジョブを削除・再作成せずスケジュールだけを更新できることの回帰確認。
    #[tokio::test]
    async fn folder_job_schedule_can_be_updated_without_recreating_the_job() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, auth_state) = make_state(&tmp, true);
        let token = auth_state.auth.create_session("admin@example.com");
        let app = build_app(state, auth_state);
        let client = TestClient::new(app);
        let auth = format!("Bearer {token}");

        let src_resp = client
            .post("/api/folder-endpoints")
            .header("Authorization", &auth)
            .body_json(&serde_json::json!({
                "id": "", "name": "src", "kind": "Local", "path": "/tmp/a",
                "host": "", "port": 22, "username": "", "password": "", "private_key_path": "", "share": ""
            }))
            .send()
            .await;
        let src: serde_json::Value = src_resp.0.into_body().into_json().await.unwrap();
        let dst_resp = client
            .post("/api/folder-endpoints")
            .header("Authorization", &auth)
            .body_json(&serde_json::json!({
                "id": "", "name": "dst", "kind": "Local", "path": "/tmp/b",
                "host": "", "port": 22, "username": "", "password": "", "private_key_path": "", "share": ""
            }))
            .send()
            .await;
        let dst: serde_json::Value = dst_resp.0.into_body().into_json().await.unwrap();

        let job_resp = client
            .post("/api/folder-jobs")
            .header("Authorization", &auth)
            .body_json(&serde_json::json!({
                "id": "", "name": "j", "source_endpoint_id": src["id"], "dest_endpoint_id": dst["id"],
                "schedule": {"every_hours": 1, "days_of_week": [], "hour": null, "minute": null, "realtime_poll_minutes": null}
            }))
            .send()
            .await;
        let job: serde_json::Value = job_resp.0.into_body().into_json().await.unwrap();
        let job_id = job["id"].as_str().unwrap().to_string();
        assert_eq!(job["schedule"]["every_hours"], serde_json::json!(1));

        let update_resp = client
            .put(format!("/api/folder-jobs/{job_id}"))
            .header("Authorization", &auth)
            .body_json(&serde_json::json!({
                "schedule": {"every_hours": null, "days_of_week": [], "hour": null, "minute": null, "realtime_poll_minutes": 15}
            }))
            .send()
            .await;
        update_resp.assert_status_is_ok();

        let list_resp = client.get("/api/folder-jobs").header("Authorization", &auth).send().await;
        let list: serde_json::Value = list_resp.0.into_body().into_json().await.unwrap();
        assert_eq!(list[0]["schedule"]["every_hours"], serde_json::json!(null));
        assert_eq!(list[0]["schedule"]["realtime_poll_minutes"], serde_json::json!(15));

        let missing_resp = client
            .put("/api/folder-jobs/does-not-exist")
            .header("Authorization", &auth)
            .body_json(&serde_json::json!({
                "schedule": {"every_hours": 1, "days_of_week": [], "hour": null, "minute": null, "realtime_poll_minutes": null}
            }))
            .send()
            .await;
        let missing_body: serde_json::Value = missing_resp.0.into_body().into_json().await.unwrap();
        assert_eq!(missing_body["ok"], serde_json::json!(false));
    }

    /// 2026-08-13追記(ユーザー指示「予備メールアドレスでメールアドレスを
    /// 二個登録する事も可能にして」)の回帰確認: 主メールアドレスに加え、
    /// 登録した予備メールアドレスでもOTP要求が通ること、未登録のメール
    /// アドレスは引き続き拒否されることを実HTTPで検証する。
    #[tokio::test]
    async fn backup_email_can_also_request_otp() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, auth_state) = make_state(&tmp, true);
        let token = auth_state.auth.create_session("admin@example.com");
        let app = build_app(state, auth_state);
        let client = TestClient::new(app);
        let auth = format!("Bearer {token}");

        let resp = client
            .put("/api/auth/backup-email")
            .header("Authorization", &auth)
            .body_json(&serde_json::json!({"backup_email": "backup@example.com"}))
            .send()
            .await;
        resp.assert_status_is_ok();

        // 予備メールアドレスでもOTP要求が(SMTP未設定のため503にはなるが
        // "登録されていない連絡先"の403にはならない、という形で)通る。
        let resp = client
            .post("/api/auth/request-otp")
            .body_json(&serde_json::json!({"email": "backup@example.com"}))
            .send()
            .await;
        assert_eq!(resp.0.status(), StatusCode::SERVICE_UNAVAILABLE);

        // 未登録のメールアドレスは引き続き拒否される。
        let resp = client
            .post("/api/auth/request-otp")
            .body_json(&serde_json::json!({"email": "someone-else@example.com"}))
            .send()
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);
    }

    /// 2026-08-13追記(ユーザー指示「e-mail不明になっても2FAでもログイン
    /// 可能にして」)の回帰確認: セットアップ→有効化→(メールを一切
    /// 経由しない)TOTPログイン、という一連のフローを実HTTPで検証する。
    #[tokio::test]
    async fn totp_setup_enable_and_login_without_email() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, auth_state) = make_state(&tmp, true);
        let token = auth_state.auth.create_session("admin@example.com");
        let app = build_app(state, auth_state);
        let client = TestClient::new(app);
        let auth = format!("Bearer {token}");

        // 有効化前はtotp-loginで拒否される。
        let resp = client
            .post("/api/auth/totp-login")
            .body_json(&serde_json::json!({"code": "000000"}))
            .send()
            .await;
        resp.assert_status(StatusCode::FORBIDDEN);

        let setup_resp = client.post("/api/auth/totp/setup").header("Authorization", &auth).send().await;
        setup_resp.assert_status_is_ok();
        let setup_body: serde_json::Value = setup_resp.0.into_body().into_json().await.unwrap();
        let secret = setup_body["secret"].as_str().unwrap().to_string();

        let now = chrono::Utc::now().timestamp();
        let code = rs_sync::totp::code_at(&secret, now).unwrap();

        let enable_resp = client
            .post("/api/auth/totp/enable")
            .header("Authorization", &auth)
            .body_json(&serde_json::json!({"code": code}))
            .send()
            .await;
        enable_resp.assert_status_is_ok();

        let status_resp = client.get("/api/auth/totp/status").header("Authorization", &auth).send().await;
        let status_body: serde_json::Value = status_resp.0.into_body().into_json().await.unwrap();
        assert_eq!(status_body["enabled"], serde_json::json!(true));

        // メールを一切経由せず、TOTPコードのみでログインできる。
        let now2 = chrono::Utc::now().timestamp();
        let code2 = rs_sync::totp::code_at(&secret, now2).unwrap();
        let login_resp = client
            .post("/api/auth/totp-login")
            .body_json(&serde_json::json!({"code": code2}))
            .send()
            .await;
        login_resp.assert_status_is_ok();
        let login_body: serde_json::Value = login_resp.0.into_body().into_json().await.unwrap();
        assert!(login_body["token"].as_str().is_some());

        // 無効化後は拒否される。
        client.post("/api/auth/totp/disable").header("Authorization", &auth).send().await;
        let login_resp2 = client
            .post("/api/auth/totp-login")
            .body_json(&serde_json::json!({"code": code2}))
            .send()
            .await;
        login_resp2.assert_status(StatusCode::FORBIDDEN);
    }
}
