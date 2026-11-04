//! Connect の単純呼び出し層(2026-11-04、REST全廃方針)。
//!
//! 公開される入口は `POST /rs_sync.v1.Sync/<Method>`(`content-type: application/json`、
//! 本文はJSONオブジェクト)のみ。各メソッドは内部ルーター(従来のハンドラ群、外部には
//! 公開しない)の呼び出しへ変換する。`id`・`repo` は本文のフィールドで渡す。
//! エラーは `{"code","message"}`。本文は上限(サイズ・深さ・重複キー)つきで検査する。

use poem::http::{header, Method, StatusCode};
use poem::{Body, Endpoint, IntoResponse, Request, Response, Result as PoemResult};
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use std::fmt;

pub const SERVICE: &str = "rs_sync.v1.Sync";
const MAX_BODY: usize = 1024 * 1024;
const MAX_DEPTH: usize = 32;

/// (Connectメソッド名, 内部HTTPメソッド, 内部パス。`:id` `:repo` は本文から埋める)
pub const METHODS: &[(&str, &str, &str)] = &[
    ("ListAccounts", "GET", "/accounts"),
    ("CreateAccount", "POST", "/accounts"),
    ("DeleteAccount", "DELETE", "/accounts/:id"),
    ("UpdateAccountToken", "PUT", "/accounts/:id"),
    ("ListJobs", "GET", "/jobs"),
    ("CreateJob", "POST", "/jobs"),
    ("DeleteJob", "DELETE", "/jobs/:id"),
    ("UpdateJobSchedule", "PUT", "/jobs/:id"),
    ("RunJob", "POST", "/jobs/:id/run"),
    ("ResolveConflict", "POST", "/jobs/:id/resolve/:repo"),
    ("SyncJobToGoogleCalendar", "POST", "/jobs/:id/sync-google-calendar"),
    ("ListLogs", "GET", "/logs"),
    ("GetLogEmailSettings", "GET", "/log-email"),
    ("UpdateLogEmailSettings", "PUT", "/log-email"),
    ("ListFolderEndpoints", "GET", "/folder-endpoints"),
    ("CreateFolderEndpoint", "POST", "/folder-endpoints"),
    ("DeleteFolderEndpoint", "DELETE", "/folder-endpoints/:id"),
    ("UpdateFolderEndpointCredentials", "PUT", "/folder-endpoints/:id"),
    ("ListFolderJobs", "GET", "/folder-jobs"),
    ("CreateFolderJob", "POST", "/folder-jobs"),
    ("DeleteFolderJob", "DELETE", "/folder-jobs/:id"),
    ("UpdateFolderJobSchedule", "PUT", "/folder-jobs/:id"),
    ("RunFolderJob", "POST", "/folder-jobs/:id/run"),
    ("SyncFolderJobToGoogleCalendar", "POST", "/folder-jobs/:id/sync-google-calendar"),
    ("GetGoogleStatus", "GET", "/google/status"),
    ("GetGoogleLoginUrl", "GET", "/google/login"),
    ("GetGoogleCredentials", "GET", "/google/credentials"),
    ("UpdateGoogleCredentials", "PUT", "/google/credentials"),
    ("GetAdminEmail", "GET", "/auth/admin-email"),
    ("UpdateAdminEmail", "PUT", "/auth/admin-email"),
    ("GetBackupEmail", "GET", "/auth/backup-email"),
    ("UpdateBackupEmail", "PUT", "/auth/backup-email"),
    ("GetAdminPhone", "GET", "/auth/admin-phone"),
    ("UpdateAdminPhone", "PUT", "/auth/admin-phone"),
    ("SetupTotp", "POST", "/auth/totp/setup"),
    ("EnableTotp", "POST", "/auth/totp/enable"),
    ("DisableTotp", "POST", "/auth/totp/disable"),
    ("GetTotpStatus", "GET", "/auth/totp/status"),
    ("GetInstallerSyncStatus", "GET", "/installer-sync/status"),
    ("UpdateInstallerSyncConfig", "PUT", "/installer-sync/config"),
    ("RunInstallerSync", "POST", "/installer-sync/run"),
    ("RequestOtp", "POST", "/auth/request-otp"),
    ("VerifyOtp", "POST", "/auth/verify-otp"),
    ("Logout", "POST", "/auth/logout"),
    ("TotpLogin", "POST", "/auth/totp-login"),
];

fn code_for(status: StatusCode) -> &'static str {
    match status.as_u16() {
        400 | 422 => "invalid_argument",
        401 => "unauthenticated",
        403 => "permission_denied",
        404 => "not_found",
        405 => "unimplemented",
        409 => "already_exists",
        413 | 429 => "resource_exhausted",
        503 => "unavailable",
        s if s >= 500 => "internal",
        _ => "unknown",
    }
}

pub fn connect_error(status: StatusCode, message: &str) -> Response {
    Response::builder()
        .status(status)
        .content_type("application/json")
        .body(serde_json::json!({"code": code_for(status), "message": message}).to_string())
}

// ---- 深さ・重複キーの検査 ----
struct Check {
    depth: usize,
}
impl<'de> DeserializeSeed<'de> for Check {
    type Value = ();
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        if self.depth > MAX_DEPTH {
            return Err(de::Error::custom("nesting too deep"));
        }
        d.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Check {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("json")
    }
    fn visit_bool<E>(self, _: bool) -> Result<(), E> {
        Ok(())
    }
    fn visit_i64<E>(self, _: i64) -> Result<(), E> {
        Ok(())
    }
    fn visit_u64<E>(self, _: u64) -> Result<(), E> {
        Ok(())
    }
    fn visit_f64<E>(self, _: f64) -> Result<(), E> {
        Ok(())
    }
    fn visit_str<E>(self, _: &str) -> Result<(), E> {
        Ok(())
    }
    fn visit_unit<E>(self) -> Result<(), E> {
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<(), A::Error> {
        while a.next_element_seed(Check { depth: self.depth + 1 })?.is_some() {}
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<(), A::Error> {
        let mut seen = std::collections::HashSet::new();
        while let Some(k) = a.next_key::<String>()? {
            if !seen.insert(k.clone()) {
                return Err(de::Error::custom(format!("duplicate key: {k}")));
            }
            a.next_value_seed(Check { depth: self.depth + 1 })?;
        }
        Ok(())
    }
}

/// 本文を検査し、JSONオブジェクトとして返す(空本文は `{}` とみなす)。
pub fn parse_body(bytes: &[u8]) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    if bytes.iter().all(|b| b.is_ascii_whitespace()) {
        return Ok(Default::default());
    }
    let mut de = serde_json::Deserializer::from_slice(bytes);
    DeserializeSeed::deserialize(Check { depth: 0 }, &mut de).map_err(|e| e.to_string())?;
    de.end().map_err(|e| e.to_string())?;
    match serde_json::from_slice::<serde_json::Value>(bytes) {
        Ok(serde_json::Value::Object(m)) => Ok(m),
        Ok(_) => Err("body must be a JSON object".into()),
        Err(e) => Err(e.to_string()),
    }
}

fn pct(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub struct ConnectEndpoint<E> {
    pub inner: E,
}

impl<E: Endpoint> Endpoint for ConnectEndpoint<E> {
    type Output = Response;
    async fn call(&self, mut req: Request) -> PoemResult<Response> {
        let name = req.uri().path().trim_start_matches('/').to_string();
        let Some(method) = name.strip_prefix(&format!("{SERVICE}/")) else {
            return Ok(connect_error(StatusCode::NOT_FOUND, "unknown service"));
        };
        let Some(&(_, http, tmpl)) = METHODS.iter().find(|(n, _, _)| *n == method) else {
            return Ok(connect_error(StatusCode::NOT_FOUND, "unknown method"));
        };
        if req.method() != Method::POST {
            return Ok(connect_error(StatusCode::METHOD_NOT_ALLOWED, "POST only"));
        }
        let body = match req.take_body().into_bytes_limit(MAX_BODY).await {
            Ok(b) => b,
            Err(_) => return Ok(connect_error(StatusCode::PAYLOAD_TOO_LARGE, "body too large")),
        };
        let is_json = req
            .header(header::CONTENT_TYPE)
            .is_some_and(|v| v.starts_with("application/json"));
        if !is_json && !body.iter().all(|b| b.is_ascii_whitespace()) {
            return Ok(connect_error(StatusCode::BAD_REQUEST, "content-type must be application/json"));
        }
        let obj = match parse_body(&body) {
            Ok(o) => o,
            Err(e) => return Ok(connect_error(StatusCode::BAD_REQUEST, &e)),
        };
        let mut path = String::new();
        for seg in tmpl.split('/').filter(|s| !s.is_empty()) {
            path.push('/');
            if let Some(key) = seg.strip_prefix(':') {
                match obj.get(key).and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                    Some(v) => path.push_str(&pct(v)),
                    None => {
                        return Ok(connect_error(StatusCode::BAD_REQUEST, &format!("`{key}` is required")));
                    }
                }
            } else {
                path.push_str(seg);
            }
        }
        let m: Method = http.parse().unwrap();
        let mut b = Request::builder().method(m.clone()).uri_str(&path);
        for (k, v) in req.headers() {
            if k != header::CONTENT_LENGTH && k != header::CONTENT_TYPE {
                b = b.header(k.clone(), v.clone());
            }
        }
        let inner_req = if m == Method::GET || m == Method::DELETE {
            b.body(Body::empty())
        } else {
            b.header(header::CONTENT_TYPE, "application/json").body(Body::from(body))
        };
        let resp = match self.inner.call(inner_req).await {
            Ok(r) => r.into_response(),
            Err(e) => e.into_response(),
        };
        if resp.status().as_u16() < 400 {
            return Ok(resp);
        }
        let status = resp.status();
        let text = resp.into_body().into_string().await.unwrap_or_default();
        let msg = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
            .unwrap_or(text);
        Ok(connect_error(status, &msg))
    }
}
