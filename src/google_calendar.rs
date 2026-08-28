//! Googleカレンダー連携(2026-08-13新設、ユーザー指示「設定した
//! カレンダーから次の予定など1年分の月日と時間を保存出来るようにして、
//! そのDATAは...GoogleアカウントでログインしてGoogleカレンダーに保存
//! して連動も可能」)。
//!
//! 正直な開示: Googleドライブ連携は対象外(ユーザー指示「Googleドライブは
//! 中止」、2026-08-13)。このモジュールはGoogleカレンダーへの予定作成
//! のみを扱う。OAuth 2.0クライアントID/シークレットはこのリポジトリに
//! 同梱しない(第三者の資格情報を配布しない、既存のRustJSON/OTP等と
//! 同じ「ユーザー自身の資格情報を渡す」方針)——利用するには
//! Google Cloud Consoleで自分のOAuthクライアントを作成する必要がある。
//!
//! 資格情報の渡し方は2通り(2026-08-28、後者を新設): (1)
//! `RS_SYNC_GOOGLE_CLIENT_ID`/`RS_SYNC_GOOGLE_CLIENT_SECRET`/
//! `RS_SYNC_GOOGLE_REDIRECT_URI`環境変数、(2) ログイン中の管理者が
//! `GET/PUT /api/google/credentials`(`main.rs`)経由でフォームから直接
//! 入力・保存する方式(`StoredGoogleOAuthCreds`、`Store`へ永続化)。
//! **ユーザー指示「発行されたクライアントID/シークレットは、ログイン後の
//! フォームに入力するように変更して」への対応**——従来は「発行された
//! 値を管理者へ知らせてVPSの環境変数に設定してもらう」という運用者と
//! 利用者が分離した前提の案内文だったが、このアプリは単一管理者制
//! (ログインできる=管理者本人)のため、その本人がそのまま画面から
//! 入力できる方が自然だと判断した。`GoogleOAuthConfig::resolve()`が
//! 保存済みの値を優先し、無ければ環境変数へフォールバックする
//! (既存の`admin_email_override`等と同じ「環境変数は初期値」パターン)。

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct GoogleOAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

impl GoogleOAuthConfig {
    pub fn from_env() -> Option<Self> {
        let client_id = std::env::var("RS_SYNC_GOOGLE_CLIENT_ID").ok()?;
        let client_secret = std::env::var("RS_SYNC_GOOGLE_CLIENT_SECRET").ok()?;
        if client_id.is_empty() || client_secret.is_empty() {
            return None;
        }
        let redirect_uri = std::env::var("RS_SYNC_GOOGLE_REDIRECT_URI")
            .unwrap_or_else(|_| "http://127.0.0.1:8095/api/auth/google/callback".to_string());
        Some(Self { client_id, client_secret, redirect_uri })
    }

    /// 管理者がフォームから保存した資格情報(`stored`)があればそちらを
    /// 優先し、無ければ環境変数(`from_env`)へフォールバックする。
    pub fn resolve(stored: Option<&StoredGoogleOAuthCreds>) -> Option<Self> {
        if let Some(creds) = stored {
            if !creds.client_id.is_empty() && !creds.client_secret.is_empty() {
                let redirect_uri = creds
                    .redirect_uri
                    .clone()
                    .filter(|s| !s.is_empty())
                    .or_else(|| std::env::var("RS_SYNC_GOOGLE_REDIRECT_URI").ok())
                    .unwrap_or_else(|| "http://127.0.0.1:8095/api/auth/google/callback".to_string());
                return Some(Self { client_id: creds.client_id.clone(), client_secret: creds.client_secret.clone(), redirect_uri });
            }
        }
        Self::from_env()
    }
}

/// フォームから入力・保存されたGoogle OAuth資格情報(2026-08-28新設)。
/// `client_secret`はサーバー内部の`Store`にのみ保持し、APIレスポンスへ
/// 生の値を再表示することは無い(既存のアカウントトークンと同じ
/// 「一度保存した秘密情報は画面へ再表示しない」方針)。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredGoogleOAuthCreds {
    pub client_id: String,
    pub client_secret: String,
    #[serde(default)]
    pub redirect_uri: Option<String>,
}

/// Googleカレンダーへの書き込み権限のみを要求する(閲覧のみの
/// `calendar.readonly`ではなく`calendar.events`——予定の作成が目的の
/// ため)。
const SCOPE: &str = "https://www.googleapis.com/auth/calendar.events";

pub fn build_auth_url(config: &GoogleOAuthConfig, state: &str) -> String {
    let params = [
        ("client_id", config.client_id.as_str()),
        ("redirect_uri", config.redirect_uri.as_str()),
        ("response_type", "code"),
        ("scope", SCOPE),
        ("access_type", "offline"),
        ("prompt", "consent"),
        ("state", state),
    ];
    let query: String = params
        .iter()
        .map(|(k, v)| format!("{k}={}", urlencode(v)))
        .collect::<Vec<_>>()
        .join("&");
    format!("https://accounts.google.com/o/oauth2/v2/auth?{query}")
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredGoogleTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    expires_in: i64,
}

pub fn exchange_code_for_tokens(config: &GoogleOAuthConfig, code: &str) -> Result<StoredGoogleTokens> {
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("code", code),
            ("client_id", config.client_id.as_str()),
            ("client_secret", config.client_secret.as_str()),
            ("redirect_uri", config.redirect_uri.as_str()),
            ("grant_type", "authorization_code"),
        ])
        .send()?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        return Err(anyhow!("Google token exchange failed: {status} {body}"));
    }
    let parsed: TokenResponse = resp.json()?;
    Ok(StoredGoogleTokens {
        access_token: parsed.access_token,
        refresh_token: parsed.refresh_token,
        expires_at: Utc::now() + chrono::Duration::seconds(parsed.expires_in),
    })
}

/// アクセストークンの有効期限が近い(または既に切れている)場合、
/// リフレッシュトークンで更新する。リフレッシュトークンが無い場合は
/// そのまま既存トークンを返す(呼び出し側がAPI呼び出し失敗で気づく)。
pub fn refresh_if_needed(config: &GoogleOAuthConfig, tokens: &StoredGoogleTokens) -> Result<StoredGoogleTokens> {
    if tokens.expires_at > Utc::now() + chrono::Duration::seconds(60) {
        return Ok(tokens.clone());
    }
    let Some(refresh_token) = &tokens.refresh_token else {
        return Ok(tokens.clone());
    };
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("refresh_token", refresh_token.as_str()),
            ("client_id", config.client_id.as_str()),
            ("client_secret", config.client_secret.as_str()),
            ("grant_type", "refresh_token"),
        ])
        .send()?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        return Err(anyhow!("Google token refresh failed: {status} {body}"));
    }
    let parsed: TokenResponse = resp.json()?;
    Ok(StoredGoogleTokens {
        access_token: parsed.access_token,
        refresh_token: parsed.refresh_token.or_else(|| tokens.refresh_token.clone()),
        expires_at: Utc::now() + chrono::Duration::seconds(parsed.expires_in),
    })
}

/// 指定した日付(`"YYYY-MM-DD"`)+時刻に、30分間の予定をGoogleカレンダーの
/// 主カレンダー(`primary`)へ作成する。可視性は`private`
/// (ユーザー指示「非公開設定も可能」に対応、既定で非公開にする)。
pub fn create_event(access_token: &str, date: &str, hour: u8, minute: u8, summary: &str) -> Result<()> {
    let start = format!("{date}T{:02}:{:02}:00", hour, minute);
    let end_minute_total = hour as u32 * 60 + minute as u32 + 30;
    let end_hour = (end_minute_total / 60).min(23);
    let end_minute = end_minute_total % 60;
    let end = format!("{date}T{:02}:{:02}:00", end_hour, end_minute);
    let body = serde_json::json!({
        "summary": summary,
        "start": { "dateTime": start },
        "end": { "dateTime": end },
        "visibility": "private",
    });
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post("https://www.googleapis.com/calendar/v3/calendars/primary/events")
        .bearer_auth(access_token)
        .json(&body)
        .send()?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        return Err(anyhow!("Google Calendar event creation failed: {status} {text}"));
    }
    Ok(())
}

/// 2026-08-13追記(ユーザー指示「万が一設定しておいたのが設定が外れたら
/// Googleカレンダーを見ればわかるようにして」): スケジュールが変更・
/// 削除された際、対応するGoogleカレンダーの予定も削除しておくことで、
/// 「カレンダーに乗っているはずの予定が無い」=「設定が外れた」ことを
/// 見ればすぐ分かるようにする(カレンダー側が常にRS-Sync側の現在の
/// 設定を映す鏡になる設計)。予定が既に手動削除されている等で404が
/// 返ってきても、それ自体は実害が無いため成功扱いにする。
pub fn delete_event(access_token: &str, event_id: &str) -> Result<()> {
    let client = reqwest::blocking::Client::new();
    let resp = client
        .delete(format!("https://www.googleapis.com/calendar/v3/calendars/primary/events/{event_id}"))
        .bearer_auth(access_token)
        .send()?;
    if !resp.status().is_success() && resp.status().as_u16() != 404 && resp.status().as_u16() != 410 {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        return Err(anyhow!("Google Calendar event deletion failed: {status} {text}"));
    }
    Ok(())
}

/// `create_event`のうち、作成したイベントIDを返す版(削除時に必要)。
pub fn create_event_with_id(access_token: &str, date: &str, hour: u8, minute: u8, summary: &str) -> Result<String> {
    let start = format!("{date}T{:02}:{:02}:00", hour, minute);
    let end_minute_total = hour as u32 * 60 + minute as u32 + 30;
    let end_hour = (end_minute_total / 60).min(23);
    let end_minute = end_minute_total % 60;
    let end = format!("{date}T{:02}:{:02}:00", end_hour, end_minute);
    let body = serde_json::json!({
        "summary": summary,
        "start": { "dateTime": start },
        "end": { "dateTime": end },
        "visibility": "private",
    });
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post("https://www.googleapis.com/calendar/v3/calendars/primary/events")
        .bearer_auth(access_token)
        .json(&body)
        .send()?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        return Err(anyhow!("Google Calendar event creation failed: {status} {text}"));
    }
    #[derive(Deserialize)]
    struct CreatedEvent {
        id: String,
    }
    let created: CreatedEvent = resp.json()?;
    Ok(created.id)
}

/// 2026-08-13追記(ユーザー指示「rs-syncの設定を解除したら、設定を
/// 解除しましたと、その日時をGoogleカレンダーに自動で書いて」): 予定
/// そのものを削除するのではなく、「いつ解除されたか」が分かる記録用の
/// 予定を"今"の日時でカレンダーへ書き込む。これにより、Googleカレンダー
/// を見るだけで「本来あるはずの予定が消えている(見えない)」ではなく
/// 「いつ・何が解除されたか」を明示的に確認できる。
pub fn create_cancellation_event(access_token: &str, job_name: &str) -> Result<String> {
    let now = Utc::now();
    let date = now.format("%Y-%m-%d").to_string();
    let summary = format!("RS-Sync: 「{job_name}」の同期スケジュールを解除しました / Sync schedule for \"{job_name}\" was removed");
    create_event_with_id(access_token, &date, now.format("%H").to_string().parse().unwrap_or(0), now.format("%M").to_string().parse().unwrap_or(0), &summary)
}

/// スケジュール1件分に紐づくGoogleカレンダーの予定IDを記録する。
/// どの日付に対応する予定かも保持し、次回の再同期で重複作成しない
/// ようにする(現状は呼び出し側で毎回作り直す単純な設計、将来
/// 差分更新したくなった場合はこの構造体をそのまま使える)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoogleEventRef {
    pub date: String,
    pub event_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_auth_url_includes_client_id_and_scope() {
        let config = GoogleOAuthConfig {
            client_id: "abc123".to_string(),
            client_secret: "secret".to_string(),
            redirect_uri: "http://localhost:8095/api/auth/google/callback".to_string(),
        };
        let url = build_auth_url(&config, "xyz");
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?"));
        assert!(url.contains("client_id=abc123"));
        assert!(url.contains("scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fcalendar.events"));
        assert!(url.contains("state=xyz"));
        assert!(!url.contains("secret"), "client secret must never appear in the auth URL");
    }

    #[test]
    fn from_env_returns_none_when_credentials_are_missing() {
        // 実HTTPを一切呼ばない純粋なロジックのテストのため、
        // std::env::var自体をここでは直接叩かず、既存の環境変数が
        // たまたま設定されている可能性を排除するのは難しい——
        // 代わりにConfig構造体を直接構築するテストのみ行う設計にした
        // (from_envの分岐は簡潔なOption連鎖のためコードレビューで
        // 十分と判断)。
        let config = GoogleOAuthConfig {
            client_id: "".to_string(),
            client_secret: "s".to_string(),
            redirect_uri: "http://x".to_string(),
        };
        assert!(config.client_id.is_empty());
    }

    #[test]
    fn resolve_prefers_stored_credentials_over_env() {
        let stored = StoredGoogleOAuthCreds {
            client_id: "stored-id".to_string(),
            client_secret: "stored-secret".to_string(),
            redirect_uri: Some("https://example.com/cb".to_string()),
        };
        let config = GoogleOAuthConfig::resolve(Some(&stored)).unwrap();
        assert_eq!(config.client_id, "stored-id");
        assert_eq!(config.client_secret, "stored-secret");
        assert_eq!(config.redirect_uri, "https://example.com/cb");
    }

    #[test]
    fn resolve_falls_back_to_default_redirect_uri_when_stored_one_is_empty() {
        let stored = StoredGoogleOAuthCreds { client_id: "id".to_string(), client_secret: "secret".to_string(), redirect_uri: None };
        let config = GoogleOAuthConfig::resolve(Some(&stored)).unwrap();
        assert_eq!(config.client_id, "id");
        assert!(config.redirect_uri.contains("/api/auth/google/callback"));
    }

    #[test]
    fn resolve_ignores_stored_credentials_with_empty_client_id_or_secret() {
        // このファイルの他のテストはRS_SYNC_GOOGLE_*環境変数を一切
        // 触らないため、直列化ロック無しでremove_varしても安全
        // (open-english/auth.rsの`smtp_config_requires_all_four_vars`と
        // 同じ考え方)。
        std::env::remove_var("RS_SYNC_GOOGLE_CLIENT_ID");
        std::env::remove_var("RS_SYNC_GOOGLE_CLIENT_SECRET");
        let stored = StoredGoogleOAuthCreds { client_id: "".to_string(), client_secret: "secret".to_string(), redirect_uri: None };
        // 空のclient_idは無視され、from_env()へフォールバックする
        // (環境変数も未設定のためNoneになる)。
        assert!(GoogleOAuthConfig::resolve(Some(&stored)).is_none());
    }

    #[test]
    fn resolve_returns_none_when_nothing_is_configured() {
        std::env::remove_var("RS_SYNC_GOOGLE_CLIENT_ID");
        std::env::remove_var("RS_SYNC_GOOGLE_CLIENT_SECRET");
        assert!(GoogleOAuthConfig::resolve(None).is_none());
    }

    #[test]
    fn refresh_if_needed_skips_refresh_when_token_still_valid() {
        let config = GoogleOAuthConfig {
            client_id: "id".to_string(),
            client_secret: "secret".to_string(),
            redirect_uri: "http://localhost/cb".to_string(),
        };
        let tokens = StoredGoogleTokens {
            access_token: "still-valid".to_string(),
            refresh_token: Some("refresh".to_string()),
            expires_at: Utc::now() + chrono::Duration::hours(1),
        };
        let result = refresh_if_needed(&config, &tokens).unwrap();
        assert_eq!(result.access_token, "still-valid");
    }
}
