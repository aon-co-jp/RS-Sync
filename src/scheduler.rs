use crate::model::{AppState, RunLogEntry};
use crate::sync_engine::run_job;
use chrono::{Datelike, Timelike, Utc};
use std::sync::Arc;
use std::time::Duration;

/// Background loop: every minute, check each enabled job's schedule and run
/// it if due. Tracks `last_run_at` to avoid double-firing within the same
/// minute. This is intentionally simple (no external cron crate) per the
/// MVP scope; see CLAUDE.md for the fuller cron-expression idea deferred to
/// a later iteration.
pub async fn run_scheduler_loop(state: Arc<AppState>) {
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    loop {
        interval.tick().await;
        let now = Utc::now();
        // ハートビート更新(2026-07-29追記): このループが実際に生きている
        // ことの証跡。ジョブ処理は`spawn_blocking`経由(下記)なのでここは
        // 一切ブロックされず、ジョブがどれだけ時間がかかろうと・
        // ハングしようと、このtickだけは1分ごとに必ず更新され続ける。
        *state.scheduler_last_tick.lock().unwrap() = now;

        let due_job_ids: Vec<String> = {
            let store = state.store.lock().unwrap();
            store
                .jobs
                .iter()
                .filter(|j| j.enabled)
                .filter(|j| is_due(j, &now))
                .map(|j| j.id.clone())
                .collect()
        };

        for job_id in due_job_ids {
            // **Real bug fixed 2026-07-29**: `execute_job` does blocking
            // network I/O (reqwest::blocking) and shells out to `git`.
            // Calling it directly on this async loop's tokio worker thread
            // blocked that thread until the call returned — and with no
            // timeout on the HTTP client (also fixed today), a single slow
            // GitHub response froze the scheduler forever, silently, with
            // no further jobs ever running again. Route it through
            // `spawn_blocking` so it always runs on tokio's dedicated
            // blocking thread pool instead.
            let state = state.clone();
            tokio::task::spawn_blocking(move || execute_job(state, &job_id));
        }

        // 2026-07-29追記: フォルダ同期ジョブ(gitホスティング非依存、
        // `folder_sync`参照)も同じループ・同じ`spawn_blocking`方式で
        // 処理する。
        let due_folder_job_ids: Vec<String> = {
            let store = state.store.lock().unwrap();
            store
                .folder_jobs
                .iter()
                .filter(|j| j.enabled)
                .filter(|j| is_due_schedule(&j.schedule, j.last_run_at, &now))
                .map(|j| j.id.clone())
                .collect()
        };
        for job_id in due_folder_job_ids {
            let state = state.clone();
            tokio::task::spawn_blocking(move || execute_folder_job(state, &job_id));
        }

        // 2026-07-29追記(ユーザー指示「トークンはいつ切れますなども
        // サイト上でもe-mailにもお知らせ機能を搭載して」): 期限が
        // 7日以内に迫っているアカウントに、まだ警告していなければ
        // メールで知らせる。メール送信自体はブロッキングI/Oのため
        // `spawn_blocking`で行う。
        check_expiring_tokens(state.clone(), &now).await;
    }
}

async fn check_expiring_tokens(state: Arc<AppState>, now: &chrono::DateTime<Utc>) {
    const WARNING_WINDOW_DAYS: i64 = 7;
    let now = *now;
    let to_warn: Vec<(String, String, chrono::DateTime<Utc>)> = {
        let store = state.store.lock().unwrap();
        store
            .accounts
            .iter()
            .filter(|a| !a.token.is_empty() && !a.expiry_warning_sent)
            .filter_map(|a| a.token_expires_at.map(|exp| (a.id.clone(), a.name.clone(), exp)))
            .filter(|(_, _, exp)| (*exp - now).num_days() <= WARNING_WINDOW_DAYS)
            .collect()
    };
    for (id, name, expires_at) in to_warn {
        let state = state.clone();
        tokio::task::spawn_blocking(move || {
            let days_left = (expires_at - Utc::now()).num_days();
            notify(
                &state,
                &format!("RS-Sync: トークンの有効期限が近づいています / token expiring soon — {name}"),
                format!(
                    "アカウント「{name}」のトークンは{expires_at}(あと約{days_left}日)に期限切れになります。\
                     早めに新しいトークンを発行し、「トークンを更新」から設定してください。\n\n\
                     Account \"{name}\"'s token expires at {expires_at} (about {days_left} day(s) left). \
                     Please issue a new token soon and set it via \"Update token\"."
                ),
            );
            let mut store = state.store.lock().unwrap();
            if let Some(acc) = store.accounts.iter_mut().find(|a| a.id == id) {
                acc.expiry_warning_sent = true;
            }
            drop(store);
            state.save();
        });
    }
}

fn is_due(job: &crate::model::SyncJob, now: &chrono::DateTime<Utc>) -> bool {
    is_due_schedule(&job.schedule, job.last_run_at, now)
}

fn is_due_schedule(sched: &crate::model::Schedule, last: Option<chrono::DateTime<Utc>>, now: &chrono::DateTime<Utc>) -> bool {
    if let Some(hours) = sched.every_hours {
        let due = match last {
            None => true,
            Some(l) => (*now - l).num_minutes() >= (hours as i64) * 60,
        };
        if due {
            return true;
        }
    }
    if let Some(mins) = sched.realtime_poll_minutes {
        let due = match last {
            None => true,
            Some(l) => (*now - l).num_minutes() >= mins as i64,
        };
        if due {
            return true;
        }
    }
    if !sched.days_of_week.is_empty() {
        let weekday = now.weekday().num_days_from_sunday() as u8;
        if sched.days_of_week.contains(&weekday) {
            if let (Some(h), Some(m)) = (sched.hour, sched.minute) {
                let same_slot = now.hour() as u8 == h && now.minute() as u8 == m;
                let already_ran_this_minute = last
                    .map(|l| l.hour() as u8 == h && l.minute() as u8 == m && l.date_naive() == now.date_naive())
                    .unwrap_or(false);
                if same_slot && !already_ran_this_minute {
                    return true;
                }
            }
        }
    }
    false
}

/// 2026-07-29追記(ユーザー指示「同期を取り始める時とパスワードが
/// 違ったり変わったりやトークン切れのタイミングでサイト上でもe-mailにも
/// お知らせ機能を搭載して」): エラーメッセージから認証失敗(トークン
/// 失効・パスワード誤り)らしさを判定する簡易ヒューリスティック。
/// GitHub/SFTP/open-gitea等いずれも401/403、または"unauthorized"/
/// "forbidden"/"authentication"という語を返すのが一般的なため、これで
/// 概ね検出できる(正直な開示: 完全な網羅ではない簡易判定)。
fn is_auth_failure(message: &str) -> bool {
    let m = message.to_lowercase();
    m.contains("401") || m.contains("403") || m.contains("unauthorized") || m.contains("forbidden") || m.contains("authentication")
}

fn notify(state: &AppState, subject: &str, body: String) {
    let (Some(to), Some(smtp)) = (state.notify_email.clone(), state.smtp.clone()) else {
        return;
    };
    if let Err(e) = crate::mail::send_notification_sync(&smtp, &to, subject, body) {
        tracing::warn!("notification email failed: {e}");
    }
}

pub fn execute_job(state: Arc<AppState>, job_id: &str) -> Option<crate::sync_engine::RunOutcome> {
    let (mut job, source, dest) = {
        let store = state.store.lock().unwrap();
        let job = store.jobs.iter().find(|j| j.id == job_id)?.clone();
        let source = store
            .accounts
            .iter()
            .find(|a| a.id == job.source_account_id)?
            .clone();
        let dest = store
            .accounts
            .iter()
            .find(|a| a.id == job.dest_account_id)?
            .clone();
        (job, source, dest)
    };

    let is_first_run = job.last_run_at.is_none();
    let was_auth_failure = job.last_run_status.as_deref() == Some("auth_error");
    if is_first_run {
        notify(
            &state,
            &format!("RS-Sync: 同期を開始しました / sync started — {}", job.name),
            format!(
                "ジョブ「{}」の同期を開始しました。 / Started syncing job \"{}\".",
                job.name, job.name
            ),
        );
    }

    let started_at = Utc::now();
    let outcome = run_job(&mut job, &source, &dest);
    let now_auth_failure = !outcome.success && is_auth_failure(&outcome.message);
    job.last_run_at = Some(Utc::now());
    job.last_run_status = Some(if outcome.success {
        "ok".to_string()
    } else if now_auth_failure {
        "auth_error".to_string()
    } else {
        "error".to_string()
    });
    if now_auth_failure && !was_auth_failure {
        notify(
            &state,
            &format!("RS-Sync: 認証エラー(トークン失効・パスワード誤りの可能性) / auth error — {}", job.name),
            format!(
                "ジョブ「{}」で認証エラーが発生しました。トークンの有効期限切れ、またはパスワード変更の可能性があります。\n詳細: {}\n\n\
                 Job \"{}\" hit an authentication error — the token may have expired, or the password may have changed.\nDetails: {}",
                job.name, outcome.message, job.name, outcome.message
            ),
        );
    }

    {
        let mut store = state.store.lock().unwrap();
        if let Some(slot) = store.jobs.iter_mut().find(|j| j.id == job_id) {
            *slot = job.clone();
        }
        store.logs.push(RunLogEntry {
            id: crate::model::new_id(),
            job_id: job_id.to_string(),
            started_at,
            finished_at: Some(Utc::now()),
            success: outcome.success,
            message: outcome.message.clone(),
        });
        // keep log growth bounded
        let len = store.logs.len();
        if len > 500 {
            store.logs.drain(0..len - 500);
        }
    }
    state.save();
    Some(outcome)
}

/// フォルダ同期ジョブ(gitホスティング非依存、`folder_sync`参照)の
/// 実行。`execute_job`と同じ構造(取得→実行→結果を反映→ログ追記→保存)を
/// 踏襲する。
pub fn execute_folder_job(state: Arc<AppState>, job_id: &str) -> Option<crate::folder_sync::FolderRunOutcome> {
    let (mut job, source, dest) = {
        let store = state.store.lock().unwrap();
        let job = store.folder_jobs.iter().find(|j| j.id == job_id)?.clone();
        let source = store.folder_endpoints.iter().find(|e| e.id == job.source_endpoint_id)?.clone();
        let dest = store.folder_endpoints.iter().find(|e| e.id == job.dest_endpoint_id)?.clone();
        (job, source, dest)
    };

    let started_at = Utc::now();
    let outcome = crate::folder_sync::run_folder_sync(&source, &dest);
    job.last_run_at = Some(Utc::now());
    job.last_run_status = Some(if outcome.success { "ok".to_string() } else { "error".to_string() });

    {
        let mut store = state.store.lock().unwrap();
        if let Some(slot) = store.folder_jobs.iter_mut().find(|j| j.id == job_id) {
            *slot = job.clone();
        }
        store.logs.push(RunLogEntry {
            id: crate::model::new_id(),
            job_id: job_id.to_string(),
            started_at,
            finished_at: Some(Utc::now()),
            success: outcome.success,
            message: outcome.message.clone(),
        });
        let len = store.logs.len();
        if len > 500 {
            store.logs.drain(0..len - 500);
        }
    }
    state.save();
    Some(outcome)
}

#[cfg(test)]
mod notification_tests {
    use super::*;

    /// 2026-07-29追記: 認証失敗らしいエラーメッセージの判定ロジックの
    /// 回帰確認。
    #[test]
    fn is_auth_failure_detects_common_patterns() {
        assert!(is_auth_failure("GitHub list_repos failed: 401 Unauthorized"));
        assert!(is_auth_failure("GitHub ensure_repo failed: 403"));
        assert!(is_auth_failure("SFTP authentication failed for user@host"));
        assert!(!is_auth_failure("repo discovery failed: connection timed out"));
        assert!(!is_auth_failure("2 file(s) copied, 0 unchanged"));
    }
}
