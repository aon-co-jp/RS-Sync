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
    /// 2026-07-29追記(ユーザー指示「トークンはいつ切れますなども
    /// サイト上でもe-mailにもお知らせ機能を搭載して」): トークンの
    /// 有効期限(ユーザーが任意入力、GitHub等はトークン発行時に自分で
    /// 期限を設定するため、こちらから自動取得はできない——正直な開示、
    /// 入力してもらう前提の設計)。期限の7日前を切ると通知メールを送る。
    #[serde(default)]
    pub token_expires_at: Option<DateTime<Utc>>,
    /// 期限切れ間近の通知を送信済みかどうか(同じ期限に対して何度も
    /// メールを送らないためのフラグ、トークン更新時にリセットされる)。
    #[serde(default)]
    pub expiry_warning_sent: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Direction {
    OneWay,
    TwoWay,
}

/// 2026-08-13追記(ユーザー指示「一週間に1回から数回の場合は、曜日から
/// 選択して、後は24時間の中からその日ごとに選択して」): 週次スケジュール
/// で曜日ごとに異なる時刻を指定できるようにするための1エントリ。
/// 既存の`days_of_week`+`hour`+`minute`(全曜日で単一時刻を共有)とは別に
/// 併用可能——後方互換のため既存フィールドは残したまま、より柔軟な
/// 表現をこちらへ追加する形にした。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WeeklyDayTime {
    /// 0=Sunday..6=Saturday(既存`days_of_week`と同じ規約)
    pub weekday: u8,
    pub hour: u8,
    pub minute: u8,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Schedule {
    /// run every N hours if Some
    pub every_hours: Option<u32>,
    /// 0=Sunday..6=Saturday, run at hour:minute JST-agnostic (server local) if days non-empty
    pub days_of_week: Vec<u8>,
    pub hour: Option<u8>,
    pub minute: Option<u8>,
    /// poll for source changes every N minutes and sync immediately (best-effort realtime)
    pub realtime_poll_minutes: Option<u32>,
    /// 2026-08-13追記: 曜日ごとに異なる時刻を指定する週次スケジュール
    /// (`days_of_week`+`hour`+`minute`の「全曜日で同一時刻」制約を
    /// 緩和したもの)。`#[serde(default)]`で既存データとの後方互換を維持。
    #[serde(default)]
    pub weekly_day_times: Vec<WeeklyDayTime>,
    /// 2026-08-13追記(ユーザー指示「一ヶ月に一回から数回を選択したら
    /// カレンダーから...それが何日で...時間を指定してもらう」): 毎月の
    /// 実行日(1〜31)。月をまたいで同じ日番号で繰り返す(特定年月の
    /// 1回限りの日付ではない)。
    #[serde(default)]
    pub monthly_days: Vec<u8>,
    #[serde(default)]
    pub monthly_hour: Option<u8>,
    #[serde(default)]
    pub monthly_minute: Option<u8>,
    /// 2026-08-13追記(ユーザー指示「定時の時間を指定したらそれを毎日と
    /// 言うチェックボックスで指定も可能」): trueの場合`monthly_days`を
    /// 無視し、`monthly_hour`:`monthly_minute`に毎日実行する
    /// (月次UIから作った「毎日この時刻に実行」設定)。
    #[serde(default)]
    pub monthly_daily: bool,
    /// 2026-08-13追記(ユーザー指示「設定したカレンダーから次の予定など
    /// 1年分の月日と時間を保存出来るようにして」): `monthly_days`
    /// (日番号のみ、毎月繰り返す)とは別に、特定の年月日
    /// (`"YYYY-MM-DD"`形式、最大1年先まで画面から選択可能)を複数
    /// 保存できる一回限りの予定。同じ日付は年をまたいでも一度しか
    /// 一致しないため、真の「特定の日」の予定として機能する。
    #[serde(default)]
    pub specific_dates: Vec<String>,
    #[serde(default)]
    pub specific_dates_hour: Option<u8>,
    #[serde(default)]
    pub specific_dates_minute: Option<u8>,
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
    /// 2026-08-12追記(ユーザー指示「最初に何分間隔を指定したかと、今は
    /// 何分間隔かを両方保存して」): ジョブ作成時にユーザーが最初に
    /// 指定した`realtime_poll_minutes`を、以後`schedule`が手動編集や
    /// 自動延長(`auto_widen_schedule_if_too_slow`)で変わっても一切
    /// 上書きしない記録用フィールド。`#[serde(default)]`で既存データとの
    /// 後方互換を維持。
    #[serde(default)]
    pub initial_realtime_poll_minutes: Option<u32>,
    /// 2026-08-13追記(ユーザー指示「rs-syncの設定を解除したら、設定を
    /// 解除しましたと...Googleカレンダーに自動で書いて」): このジョブの
    /// 特定日付スケジュールをGoogleカレンダーへ同期したことがあるかの
    /// フラグ。trueのジョブの月次/特定日スケジュールが解除・削除された
    /// 際、解除通知イベントをカレンダーへ書き込む対象になる。
    #[serde(default)]
    pub google_synced: bool,
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
    /// 2026-08-13追記(ユーザー指示「GoogleアカウントでログインしてGoogle
    /// カレンダーに保存して連動も可能」): OAuthトークン(単一の管理者
    /// アカウントのみを想定、このアプリ自体が単一管理者制のため既存の
    /// `AuthState`と同じ設計)。
    #[serde(default)]
    pub google_tokens: Option<crate::google_calendar::StoredGoogleTokens>,
    /// 2026-08-13追記(ユーザー指示「ログインのe-mailアドレスも変更可能に
    /// して」): 管理者がAPI経由で変更した場合にここへ永続化する。
    /// `None`の間は`RS_SYNC_ADMIN_EMAIL`環境変数の値が使われる
    /// (初回起動時の初期値としてのみ機能、一度でも変更されたらこちらが
    /// 優先——他の`*_override`系フィールドと同じパターン)。
    #[serde(default)]
    pub admin_email_override: Option<String>,
    /// 2026-08-13追記(ユーザー指示「予備メールアドレスでメールアドレスを
    /// 二個登録する事も可能にして」): OTPログインを受け付ける2つ目の
    /// メールアドレス(任意)。
    #[serde(default)]
    pub backup_email: Option<String>,
    /// 2026-08-13追記(ユーザー指示「初回にe-mailと携帯電話番号も登録して、
    /// e-mail不明になっても2FAでもログイン可能にして」): 管理者の携帯
    /// 電話番号(表示・案内のみ、SMS送信機能は無い——万一メールアドレスを
    /// 忘れた際に本人確認の連絡先として管理者自身が控えておくための項目)。
    #[serde(default)]
    pub admin_phone_override: Option<String>,
    /// TOTP(認証アプリ型2段階認証)の秘密鍵。base32文字列。
    #[serde(default)]
    pub totp_secret: Option<String>,
    /// trueなら`totp_secret`でのログイン(`POST /api/auth/totp-login`、
    /// メールOTPを経由しない)が有効。メールアドレスを忘れた・受信でき
    /// なくなった場合の代替ログイン経路として機能する。
    #[serde(default)]
    pub totp_enabled: bool,
    /// 2026-08-26追記(ユーザー指示「この様なインストーラーを分かりやすい
    /// フォルダーに同期システムで自動で配置し直すRS-Syncとして、15分に
    /// 1回でも良いのでその様に機能するようにして」): 各リポジトリの
    /// Windowsインストーラーを定期的に再ビルドし、差分があればコミット・
    /// pushする機能の設定・実行結果(`installer_sync`モジュール参照)。
    #[serde(default)]
    pub installer_sync: crate::installer_sync::InstallerSyncSettings,
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
    /// 2026-07-29追記(ユーザー指示「同期を取り始める時とパスワードや
    /// トークン切れのタイミングでサイト上でもe-mailにもお知らせ機能を
    /// 搭載して」): 通知メールの送信先・SMTP設定。`AuthState`側の
    /// SMTP設定と同じ環境変数から読む(スケジューラは`AuthState`を
    /// 持たないため、`AppState`側にも同じ設定を複製して持つ)。
    pub notify_email: Option<String>,
    pub smtp: Option<crate::mail::SmtpConfig>,
    /// 2026-08-07追記(ユーザー指示「VPSとスマホの二箇所にインストール
    /// 予定。どちらかが生きていれば重複しないで稼働させたい」): VPSを
    /// 常に優先させ、スマホ側インスタンスはこのURL(VPS側の`/healthz`)へ
    /// 到達できる間はスケジューラのジョブ実行を自身では行わない
    /// (フェイルオーバー方式、`RS_SYNC_PRIMARY_HEALTHZ_URL`環境変数)。
    /// VPS側インスタンス自体にはこの変数を設定しない(常に自分が
    /// primaryとして動くため、`None`のままでよい)。
    pub primary_healthz_url: Option<String>,
    /// 2026-08-12追記(実バグ発見: スケジューラの自動実行と「今すぐ同期」
    /// ボタンの手動実行が同じジョブに対して同時に走ると、両者が同じ
    /// gitキャッシュディレクトリへ同時に`git clone --mirror`しようと
    /// 競合し、`cannot copy '.../hooks/pre-applypatch.sample': File
    /// exists`のようなエラーで片方が失敗する——本番で実際に
    /// `open-easy-web`リポジトリの同期がこのパターンで失敗しているのを
    /// 確認した)。ジョブID単位の簡易排他ロック。同じジョブが既に実行中
    /// なら新しい実行はスキップする(git側のキャッシュ競合を避けるのが
    /// 目的で、DB的な厳密なロックではない)。
    pub running_jobs: Mutex<std::collections::HashSet<String>>,
    /// 2026-08-13追記: GoogleカレンダーOAuth CSRF対策用のワンタイム
    /// state値。ログイン開始時に生成・保持し、コールバック時に一致
    /// するか検証する(Googleからのリダイレクトは独自のBearerトークンを
    /// 運べないため、この`state`パラメータが唯一のCSRF対策)。
    pub google_oauth_state: Mutex<Option<String>>,
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
            notify_email: std::env::var("RS_SYNC_ADMIN_EMAIL").ok().filter(|s| !s.is_empty()),
            smtp: crate::mail::SmtpConfig::from_env(),
            primary_healthz_url: std::env::var("RS_SYNC_PRIMARY_HEALTHZ_URL").ok().filter(|s| !s.is_empty()),
            running_jobs: Mutex::new(std::collections::HashSet::new()),
            google_oauth_state: Mutex::new(None),
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
