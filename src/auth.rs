//! メールアドレス宛のワンタイムパスワード(OTP)認証。
//! [`open-easy-web`](https://github.com/aon-co-jp/open-easy-web)/
//! [`open-redmine`](https://github.com/aon-co-jp/open-redmine)の
//! `src/auth.rs`と同じ設計(固定パスワード非保存、OTPはSHA-256ハッシュの
//! みを保持)をそのまま移植したもの。

use rand::Rng;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const OTP_TTL: Duration = Duration::from_secs(5 * 60);
const SESSION_TTL: Duration = Duration::from_secs(12 * 60 * 60);
const MAX_ATTEMPTS: u32 = 5;

fn hash(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn generate_otp() -> String {
    let mut rng = rand::thread_rng();
    format!("{:06}", rng.gen_range(0..1_000_000u32))
}

fn generate_token() -> String {
    let mut rng = rand::thread_rng();
    let bytes: [u8; 24] = rng.gen();
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
}
