use rs_sync::model::{self, Account, AppState, Direction, Schedule, SyncJob};
use rs_sync::scheduler;
use poem::{
    get, handler, listener::TcpListener, post, web::{Data, Json, Path as PPath}, EndpointExt, Route, Server,
};
use std::sync::Arc;

#[handler]
fn healthz() -> &'static str {
    "ok"
}

#[handler]
fn list_accounts(state: Data<&Arc<AppState>>) -> Json<Vec<Account>> {
    let store = state.store.lock().unwrap();
    Json(store.accounts.clone())
}

#[handler]
fn create_account(state: Data<&Arc<AppState>>, Json(mut acc): Json<Account>) -> Json<Account> {
    acc.id = model::new_id();
    {
        let mut store = state.store.lock().unwrap();
        store.accounts.push(acc.clone());
    }
    state.save();
    Json(acc)
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

fn build_app(state: Arc<AppState>) -> impl poem::Endpoint {
    Route::new()
        .at("/", get(index))
        .at("/healthz", get(healthz))
        .at("/api/accounts", get(list_accounts).post(create_account))
        .at("/api/accounts/:id", poem::delete(delete_account))
        .at("/api/jobs", get(list_jobs).post(create_job))
        .at("/api/jobs/:id", poem::delete(delete_job))
        .at("/api/jobs/:id/run", post(run_job_now))
        .at("/api/jobs/:id/resolve/:repo", post(resolve_conflict))
        .at("/api/logs", get(list_logs))
        .data(state)
}

#[tokio::main]
async fn main() -> Result<(), std::io::Error> {
    tracing_subscriber::fmt::init();
    let data_file = std::env::var("RS_SYNC_DATA").unwrap_or_else(|_| "data/store.json".to_string());
    let state = Arc::new(AppState::new(&data_file));

    let scheduler_state = state.clone();
    tokio::spawn(async move {
        scheduler::run_scheduler_loop(scheduler_state).await;
    });

    let port: u16 = std::env::var("RS_SYNC_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8095);
    let app = build_app(state);
    tracing::info!("rs-sync listening on 0.0.0.0:{port}");
    Server::new(TcpListener::bind(("0.0.0.0", port)))
        .run(app)
        .await
}
