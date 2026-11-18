//! メールアドレス宛のワンタイムパスワード(OTP)認証。
//! [`open-easy-web`](https://github.com/aon-co-jp/open-easy-web)/
//! [`open-redmine`](https://github.com/aon-co-jp/open-redmine)の
//! `src/auth.rs`と同じ設計(固定パスワード非保存、OTPはSHA-256ハッシュの
//! みを保持)をそのまま移植したもの。

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const OTP_TTL: Duration = Duration::from_secs(5 * 60);
const SESSION_TTL: Duration = Duration::from_secs(12 * 60 * 60);
const MAX_ATTEMPTS: u32 = 5;
/// QR確認ログインの有効期間(3分・1回限り)。
const QR_LOGIN_TTL: Duration = Duration::from_secs(3 * 60);

/// ログイン方式(2026-11-18、4択ログイン方式の横展開)。
/// - `"otp"`(既定): メールOTPのみ。
/// - `"qr"`: TOTP有効化済みのとき、登録メールを名乗るだけでQR撮影のみでログイン
///   (秘密を持たないため弱い方式。画面を見た人が確認URLを開ければ通る点を管理画面で開示)。
/// - `"otp_qr"`: メールOTP検証後、QR撮影による確認を第二要素にする2段階認証。
/// 「パスワード無し」は、同期先の資格情報を扱う管理アプリのため意図的に実装しない。
pub const LOGIN_MODE_OTP: &str = "otp";
pub const LOGIN_MODE_QR: &str = "qr";
pub const LOGIN_MODE_OTP_QR: &str = "otp_qr";

pub fn is_valid_login_mode(s: &str) -> bool {
    matches!(s, LOGIN_MODE_OTP | LOGIN_MODE_QR | LOGIN_MODE_OTP_QR)
}

/// メールアドレスの一部を伏せる(QR確認ページの表示用)。
fn mask_email(email: &str) -> String {
    match email.find('@') {
        Some(at) => {
            let (local, domain) = email.split_at(at);
            let visible: String = local.chars().take(2).collect();
            format!("{visible}***{domain}")
        }
        None => "***".to_string(),
    }
}

struct QrLoginEntry {
    account_email: String,
    confirmed: bool,
    expires_at: Instant,
}

fn hash(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn generate_otp() -> String {
    format!("{:06}", rand::random_range(0..1_000_000u32))
}

fn generate_token() -> String {
    let bytes: [u8; 24] = rand::random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

struct PendingOtp {
    code_hash: String,
    expires_at: Instant,
    attempts: u32,
}

struct Session {
    email: String,
    expires_at: Instant,
}

#[derive(Default)]
pub struct AuthStore {
    pending: Mutex<HashMap<String, PendingOtp>>,
    sessions: Mutex<HashMap<String, Session>>,
    qr_logins: Mutex<HashMap<String, QrLoginEntry>>,
    /// 空のときは`"otp"`として扱う(再起動で既定のotpへ戻る安全側の設計)。
    login_mode: Mutex<String>,
}

pub enum RequestOtpOutcome {
    Issued(String),
}

impl AuthStore {
    pub fn request_otp(&self, email: &str) -> RequestOtpOutcome {
        let code = generate_otp();
        let mut pending = self.pending.lock().unwrap();
        pending.insert(email.to_string(), PendingOtp { code_hash: hash(&code), expires_at: Instant::now() + OTP_TTL, attempts: 0 });
        RequestOtpOutcome::Issued(code)
    }

    pub fn consume_otp(&self, email: &str, code: &str) -> Result<(), VerifyError> {
        let mut pending = self.pending.lock().unwrap();
        let Some(entry) = pending.get_mut(email) else {
            return Err(VerifyError::NotRequested);
        };
        if Instant::now() > entry.expires_at {
            pending.remove(email);
            return Err(VerifyError::Expired);
        }
        if entry.attempts >= MAX_ATTEMPTS {
            pending.remove(email);
            return Err(VerifyError::TooManyAttempts);
        }
        entry.attempts += 1;
        if entry.code_hash != hash(code) {
            return Err(VerifyError::Mismatch);
        }
        pending.remove(email);
        Ok(())
    }

    pub fn create_session(&self, email: &str) -> String {
        let token = generate_token();
        self.sessions.lock().unwrap().insert(token.clone(), Session { email: email.to_string(), expires_at: Instant::now() + SESSION_TTL });
        token
    }

    pub fn session_email(&self, token: &str) -> Option<String> {
        let mut sessions = self.sessions.lock().unwrap();
        let session = sessions.get(token)?;
        if Instant::now() > session.expires_at {
            sessions.remove(token);
            return None;
        }
        Some(session.email.clone())
    }

    pub fn login_mode(&self) -> String {
        let m = self.login_mode.lock().unwrap().clone();
        if m.is_empty() { LOGIN_MODE_OTP.to_string() } else { m }
    }

    /// `is_valid_login_mode`で検証済みの値のみ渡すこと。
    pub fn set_login_mode(&self, mode: &str) {
        *self.login_mode.lock().unwrap() = mode.to_string();
    }

    /// QR確認ログインを開始し、QRに埋め込む短命ID(3分・1回限り)を返す。
    pub fn start_qr_login(&self, account_email: &str) -> String {
        let mut logins = self.qr_logins.lock().unwrap();
        logins.retain(|_, v| v.expires_at > Instant::now());
        let id = generate_token();
        logins.insert(
            id.clone(),
            QrLoginEntry { account_email: account_email.to_string(), confirmed: false, expires_at: Instant::now() + QR_LOGIN_TTL },
        );
        id
    }

    pub fn qr_login_masked_email(&self, id: &str) -> Option<String> {
        let logins = self.qr_logins.lock().unwrap();
        let entry = logins.get(id).filter(|e| e.expires_at > Instant::now())?;
        Some(mask_email(&entry.account_email))
    }

    /// 確認用ページが開かれたときに自動で呼ばれる(ボタン操作は不要)。
    /// セッショントークンはここでは発行しない(別端末のため、プライマリ端末の`finish_qr_login`が発行)。
    pub fn confirm_qr_login(&self, id: &str) -> Result<(), VerifyError> {
        let mut logins = self.qr_logins.lock().unwrap();
        let Some(entry) = logins.get_mut(id) else {
            return Err(VerifyError::NotRequested);
        };
        if Instant::now() > entry.expires_at {
            logins.remove(id);
            return Err(VerifyError::Expired);
        }
        entry.confirmed = true;
        Ok(())
    }

    pub fn qr_login_status(&self, id: &str) -> Option<bool> {
        let logins = self.qr_logins.lock().unwrap();
        logins.get(id).filter(|e| e.expires_at > Instant::now()).map(|e| e.confirmed)
    }

    /// 確認済みのQRセッションを消費してセッショントークンを発行する(1回限り)。
    pub fn finish_qr_login(&self, id: &str) -> Result<String, VerifyError> {
        let mut logins = self.qr_logins.lock().unwrap();
        let Some(entry) = logins.get(id) else {
            return Err(VerifyError::NotRequested);
        };
        if Instant::now() > entry.expires_at {
            logins.remove(id);
            return Err(VerifyError::Expired);
        }
        if !entry.confirmed {
            return Err(VerifyError::Mismatch);
        }
        let account_email = entry.account_email.clone();
        logins.remove(id);
        drop(logins);
        Ok(self.create_session(&account_email))
    }

    pub fn logout(&self, token: &str) {
        self.sessions.lock().unwrap().remove(token);
    }
}

#[derive(Debug)]
pub enum VerifyError {
    NotRequested,
    Expired,
    TooManyAttempts,
    Mismatch,
}

impl VerifyError {
    pub fn message(&self) -> &'static str {
        match self {
            VerifyError::NotRequested => "この連絡先宛にOTPは発行されていません。 / No OTP was requested for this contact.",
            VerifyError::Expired => "OTPの有効期限が切れました。再度リクエストしてください。 / The OTP has expired, please request a new one.",
            VerifyError::TooManyAttempts => "試行回数の上限を超えました。再度リクエストしてください。 / Too many attempts, please request a new one.",
            VerifyError::Mismatch => "コードが正しくありません。 / Incorrect code.",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn otp_roundtrip_succeeds_with_correct_code() {
        let store = AuthStore::default();
        let RequestOtpOutcome::Issued(code) = store.request_otp("admin@example.com");
        store.consume_otp("admin@example.com", &code).unwrap();
        let token = store.create_session("admin@example.com");
        assert_eq!(store.session_email(&token).as_deref(), Some("admin@example.com"));
    }

    #[test]
    fn wrong_code_is_rejected_and_does_not_consume_the_otp() {
        let store = AuthStore::default();
        let RequestOtpOutcome::Issued(code) = store.request_otp("admin@example.com");
        assert!(matches!(store.consume_otp("admin@example.com", "000000"), Err(VerifyError::Mismatch)));
        assert!(store.consume_otp("admin@example.com", &code).is_ok());
    }

    #[test]
    fn exceeding_max_attempts_invalidates_the_otp() {
        let store = AuthStore::default();
        let RequestOtpOutcome::Issued(_code) = store.request_otp("admin@example.com");
        for _ in 0..MAX_ATTEMPTS {
            let _ = store.consume_otp("admin@example.com", "000000");
        }
        assert!(matches!(
            store.consume_otp("admin@example.com", "000000"),
            Err(VerifyError::TooManyAttempts) | Err(VerifyError::NotRequested)
        ));
    }

    #[test]
    fn logout_invalidates_the_session_token() {
        let store = AuthStore::default();
        let RequestOtpOutcome::Issued(code) = store.request_otp("admin@example.com");
        store.consume_otp("admin@example.com", &code).unwrap();
        let token = store.create_session("admin@example.com");
        store.logout(&token);
        assert!(store.session_email(&token).is_none());
    }

    #[test]
    fn unknown_token_is_invalid() {
        let store = AuthStore::default();
        assert!(store.session_email("does-not-exist").is_none());
    }

    #[test]
    fn qr_login_requires_confirmation_and_is_single_use() {
        let store = AuthStore::default();
        let id = store.start_qr_login("admin@example.com");
        assert_eq!(store.qr_login_masked_email(&id).as_deref(), Some("ad***@example.com"));
        assert_eq!(store.qr_login_status(&id), Some(false));
        assert!(store.finish_qr_login(&id).is_err(), "not confirmed yet");
        store.confirm_qr_login(&id).unwrap();
        assert_eq!(store.qr_login_status(&id), Some(true));
        let token = store.finish_qr_login(&id).unwrap();
        assert_eq!(store.session_email(&token).as_deref(), Some("admin@example.com"));
        assert!(store.finish_qr_login(&id).is_err(), "single use");
        assert!(store.confirm_qr_login("nope").is_err());
    }

    #[test]
    fn login_mode_defaults_to_otp_and_validates() {
        let store = AuthStore::default();
        assert_eq!(store.login_mode(), "otp");
        store.set_login_mode("otp_qr");
        assert_eq!(store.login_mode(), "otp_qr");
        assert!(is_valid_login_mode("qr") && !is_valid_login_mode("none"));
    }
}
