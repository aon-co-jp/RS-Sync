use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use uuid::Uuid;

pub const DATA_DIR: &str = "data";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProviderKind {
    GitHub,
    OpenGitea,
    Gitea,
    /// GitBucket (the Scala-based self-hosted OSS git server), distinct
    /// from Atlassian's Bitbucket SaaS product (see [`Bitbucket`]).
    Gitbucket,
    /// GitLab (self-hosted or gitlab.com), REST API v4.
    GitLab,
    /// Atlassian Bitbucket (Cloud REST API v2). Distinct from the
    /// self-hosted "GitBucket" OSS project (see [`Gitbucket`]).
    Bitbucket,
    /// Local filesystem bare-repo "provider": not a real hosting service,
    /// used for integration tests and for ad-hoc local mirroring
    /// (git_base is a directory path, repos live at git_base/<repo>.git).
    Local,
}

/// A registered git-hosting account (source or destination).
/// SECURITY: token is stored only in the local JSON file under data/,
/// which is gitignored. Never log or echo the token value, never write
/// it into README/CLAUDE.md/commits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub provider: ProviderKind,
    /// API base URL, e.g. https://api.github.com or https://gitea.example.com
    pub api_base: String,
    /// git clone/push base URL, e.g. https://github.com or http://127.0.0.1:8090
    pub git_base: String,
    /// org/user namespace to operate under
    pub owner: String,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Direction {
    OneWay,
    TwoWay,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    /// run every N hours if Some
    pub every_hours: Option<u32>,
    /// 0=Sunday..6=Saturday, run at hour:minute JST-agnostic (server local) if days non-empty
    pub days_of_week: Vec<u8>,
    pub hour: Option<u8>,
    pub minute: Option<u8>,
    /// poll for source changes every N minutes and sync immediately (best-effort realtime)
    pub realtime_poll_minutes: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncJob {
    pub id: String,
    pub name: String,
    pub source_account_id: String,
    pub dest_account_id: String,
    pub direction: Direction,
    /// repo names (without owner prefix) to sync; empty = discover all from source
    pub repos: Vec<String>,
    pub schedule: Schedule,
    pub enabled: bool,
    pub last_run_at: Option<DateTime<Utc>>,
    pub last_run_status: Option<String>,
    /// repos currently flagged as conflicted in two-way mode (auto-run paused for them)
    pub conflicted_repos: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunLogEntry {
    pub id: String,
    pub job_id: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Store {
    pub accounts: Vec<Account>,
    pub jobs: Vec<SyncJob>,
    #[serde(default)]
    pub logs: Vec<RunLogEntry>,
    /// 2026-07-29追記: gitホスティングとは無関係の、レンタルサーバー/
    /// VPS/ローカルドライブ/スマホ/NAS間のフォルダコピー同期
    /// (`folder_sync`参照)。既存の`accounts`/`jobs`とは別の名前空間。
    #[serde(default)]
    pub folder_endpoints: Vec<crate::folder_sync::FolderEndpoint>,
    #[serde(default)]
    pub folder_jobs: Vec<crate::folder_sync::FolderSyncJob>,
}

pub struct AppState {
    pub store: Mutex<Store>,
    pub data_file: String,
    /// 2026-07-29追記(ユーザー指示「このようなBUGが起きないか定期的に
    /// 自動チェックする機能」): スケジューラのバックグラウンドループが
    /// 実際に「生きている」ことを外部から確認できるハートビート。
    /// `scheduler::run_scheduler_loop`が1分ごとのtickのたびに更新する。
    /// 今回のバグ(ブロッキングI/Oでループ全体が無言のまま永久停止)は、
    /// このタイムスタンプが更新され続けているかを見れば即座に検知できた
    /// はず——`GET /healthz`にこの値を含め、一定時間(3分)更新が無ければ
    /// 異常とみなす。
    pub scheduler_last_tick: Mutex<DateTime<Utc>>,
}

impl AppState {
    pub fn new(data_file: &str) -> Self {
        let store = if Path::new(data_file).exists() {
            let raw = fs::read(data_file).unwrap_or_default();
            crate::rustjson::parse_typed(&raw).unwrap_or_default()
        } else {
            Store::default()
        };
        Self {
            store: Mutex::new(store),
            data_file: data_file.to_string(),
            scheduler_last_tick: Mutex::new(Utc::now()),
        }
    }

    pub fn save(&self) {
        let store = self.store.lock().unwrap();
        if let Some(parent) = Path::new(&self.data_file).parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&*store) {
            let _ = fs::write(&self.data_file, json);
        }
    }
}

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-07-29追記: RustJSON移行の後方互換確認。既存の
    /// (標準JSONで書かれた)`store.json`が引き続き問題なく読み込める
    /// ことを確認する。
    #[test]
    fn loads_pre_existing_strict_json_store_without_modification() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store.json");
        fs::write(&path, r#"{"accounts":[],"jobs":[],"logs":[]}"#).unwrap();
        let state = AppState::new(path.to_str().unwrap());
        let store = state.store.lock().unwrap();
        assert!(store.accounts.is_empty());
        assert!(store.jobs.is_empty());
    }

    /// RustJSONの緩い構文(トレイリングカンマ・コメント)でも読み込める
    /// ことを確認する(手動編集した場合を想定)。
    #[test]
    fn loads_loosely_formatted_rustjson_store() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store.json");
        fs::write(&path, "{\n  // hand-edited\n  \"accounts\": [],\n  \"jobs\": [],\n  \"logs\": [],\n}").unwrap();
        let state = AppState::new(path.to_str().unwrap());
        let store = state.store.lock().unwrap();
        assert!(store.accounts.is_empty());
    }

    #[test]
    fn missing_file_falls_back_to_default_store() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does-not-exist.json");
        let state = AppState::new(path.to_str().unwrap());
        let store = state.store.lock().unwrap();
        assert!(store.accounts.is_empty());
        assert!(store.jobs.is_empty());
    }
}
