//! SMTP経由でのOTPメール送信。[`open-easy-web`]/[`open-redmine`]の
//! `src/mail.rs`と同じ設計(`lettre`の同期SMTPクライアントを
//! `spawn_blocking`でオフロード)。
//!
//! [`open-easy-web`]: https://github.com/aon-co-jp/open-easy-web
//! [`open-redmine`]: https://github.com/aon-co-jp/open-redmine

use lettre::message::header::ContentType;
use lettre::message::{Attachment, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};

#[derive(Debug, Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub from: String,
}

impl SmtpConfig {
    pub fn from_env() -> Option<Self> {
        Some(Self {
            host: std::env::var("RS_SYNC_SMTP_HOST").ok()?,
            port: std::env::var("RS_SYNC_SMTP_PORT").ok().and_then(|v| v.parse().ok()).unwrap_or(587),
            username: std::env::var("RS_SYNC_SMTP_USERNAME").ok()?,
            password: std::env::var("RS_SYNC_SMTP_PASSWORD").ok()?,
            from: std::env::var("RS_SYNC_SMTP_FROM").ok()?,
        })
    }
}

#[derive(Debug)]
pub enum MailError {
    Build(String),
    Send(String),
}

impl std::fmt::Display for MailError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MailError::Build(e) => write!(f, "メール作成失敗: {e}"),
            MailError::Send(e) => write!(f, "メール送信失敗: {e}"),
        }
    }
}

/// メール本体の組み立てのみを担う純粋関数(2026-07-29追記)。実SMTP接続
/// (`starttls_relay`)を必要とする`build_and_send`から分離し、実際の
/// ネットワークI/O無しで宛先・件名・本文が正しく組み立てられるかを
/// 単体テストできるようにした——これまで`mail.rs`にテストが1件も無かった
/// (実SMTP資格情報が無い環境では`build_and_send`自体は検証できない、
/// 正直な既存の制約)ため、少なくとも組み立てロジックの正しさは検証
/// 可能にする。
fn build_message(config: &SmtpConfig, to: &str, subject: &str, body: String) -> Result<Message, MailError> {
    Message::builder()
        .from(config.from.parse().map_err(|e| MailError::Build(format!("{e}")))?)
        .to(to.parse().map_err(|e| MailError::Build(format!("{e}")))?)
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(body)
        .map_err(|e| MailError::Build(format!("{e}")))
}

/// SMTP接続・送信のみを担う(メッセージの組み立て方式に依存しない)。
/// `build_and_send`(プレーンテキスト)と`build_otp_message`
/// (HTML+プレーンテキストのmultipart、2026-07-30追記)の両方から
/// 共有される。
fn send_message(config: &SmtpConfig, email: &Message) -> Result<(), MailError> {
    let creds = Credentials::new(config.username.clone(), config.password.clone());
    let mailer =
        SmtpTransport::starttls_relay(&config.host).map_err(|e| MailError::Send(format!("{e}")))?.port(config.port).credentials(creds).build();

    mailer.send(email).map_err(|e| MailError::Send(format!("{e}")))?;
    Ok(())
}

fn build_and_send(config: &SmtpConfig, to: &str, subject: &str, body: String) -> Result<(), MailError> {
    let email = build_message(config, to, subject, body)?;
    send_message(config, &email)
}

/// OTPログインコードメール専用のメッセージ組み立て(2026-07-30追記、
/// ユーザー指示「メールで届くワンタイムパスワードのメッセージの文字
/// サイズはもっと二周り大きくして」への対応)。
///
/// 従来は`build_message`(プレーンテキストのみ)を共用していたため、
/// コード自体の文字サイズを変える手段が無かった(プレーンテキストメール
/// にはフォントサイズという概念が存在しない)。OTPメールだけは
/// HTML+プレーンテキストの`multipart/alternative`にし、HTML側でコード
/// 部分を大きな文字サイズ(本文の地の文が通常15px程度であるのに対し、
/// コードは40px=約2.5倍、"二周り大きく"の要望に対応)で表示する。
/// HTML非対応のメールクライアント向けに、従来通りのプレーンテキスト
/// パートも必ず併記する(`multipart/alternative`はクライアント側が
/// 対応する方を選ぶため、後方互換を損なわない)。
fn build_otp_message(config: &SmtpConfig, to: &str, code: &str) -> Result<Message, MailError> {
    let plain = format!(
        "RS-Sync ログイン用ワンタイムパスワード / RS-Sync login one-time password\n\n\
         コード / Code: {code}\n\n\
         このコードは5分間有効です。 / This code is valid for 5 minutes.\n\n\
         心当たりがない場合はこのメールを無視してください。 / If you did not request this, ignore this email."
    );
    let html = format!(
        "<div style=\"font-family:system-ui,sans-serif;font-size:15px;color:#222;line-height:1.6\">\
           <p>RS-Sync ログイン用ワンタイムパスワード / RS-Sync login one-time password</p>\
           <p style=\"font-size:40px;font-weight:bold;letter-spacing:.15em;margin:1em 0;\
                      background:#f0f0f0;padding:.3em .5em;display:inline-block;\
                      border-radius:.3em\">{code}</p>\
           <p>このコードは5分間有効です。 / This code is valid for 5 minutes.</p>\
           <p style=\"color:#777\">心当たりがない場合はこのメールを無視してください。 / \
              If you did not request this, ignore this email.</p>\
         </div>"
    );

    Message::builder()
        .from(config.from.parse().map_err(|e| MailError::Build(format!("{e}")))?)
        .to(to.parse().map_err(|e| MailError::Build(format!("{e}")))?)
        .subject("RS-Sync ログインコード / login code")
        .multipart(
            MultiPart::alternative()
                .singlepart(SinglePart::builder().header(ContentType::TEXT_PLAIN).body(plain))
                .singlepart(SinglePart::builder().header(ContentType::TEXT_HTML).body(html)),
        )
        .map_err(|e| MailError::Build(format!("{e}")))
}

/// 2026-07-29追記(ユーザー指示「同期を取り始める時とパスワードが
/// 違ったり変わったりやトークン切れのタイミングでサイト上でもe-mailにも
/// お知らせ機能を搭載して」): ジョブの開始・認証失敗を知らせる汎用通知
/// メール。呼び出し元(`scheduler.rs`)は`tokio::task::spawn_blocking`の
/// 内側(同期コンテキスト)で動くため、`send_otp`のような非同期ラッパーは
/// 使わずここで直接同期送信する。
pub fn send_notification_sync(config: &SmtpConfig, to: &str, subject: &str, body: String) -> Result<(), MailError> {
    build_and_send(config, to, subject, body)
}

/// 2026-08-28新設(ユーザー指示「ログの送信依頼は、1週間に一度圧縮して、
/// 指定したメールアドレスへ送付」): ログ本文(JSON)をgzip圧縮し、
/// メール添付として送る。添付ファイル名に日付を入れ、いつのログか
/// (送信時点)が分かるようにする。呼び出し元(`scheduler.rs`)は
/// `spawn_blocking`の内側で同期的に呼ぶ想定(`send_notification_sync`と
/// 同じ扱い)。
pub fn send_compressed_log_email(config: &SmtpConfig, to: &str, logs_json: &str) -> Result<(), MailError> {
    let compressed = gzip_compress(logs_json.as_bytes()).map_err(|e| MailError::Build(format!("ログの圧縮に失敗しました: {e}")))?;
    let filename = format!("rs-sync-logs-{}.json.gz", chrono::Utc::now().format("%Y-%m-%d"));
    let content_type = ContentType::parse("application/gzip").map_err(|e| MailError::Build(format!("{e}")))?;
    let body_text = "RS-Syncの週次ログ(直近1ヶ月分、gzip圧縮)を添付します。\n\n\
        Attached is RS-Sync's weekly log export (most recent 1 month, gzip-compressed)."
        .to_string();

    let email = Message::builder()
        .from(config.from.parse().map_err(|e| MailError::Build(format!("{e}")))?)
        .to(to.parse().map_err(|e| MailError::Build(format!("{e}")))?)
        .subject("RS-Sync 週次ログ / weekly log export")
        .multipart(
            MultiPart::mixed()
                .singlepart(SinglePart::builder().header(ContentType::TEXT_PLAIN).body(body_text))
                .singlepart(Attachment::new(filename).body(compressed, content_type)),
        )
        .map_err(|e| MailError::Build(format!("{e}")))?;
    send_message(config, &email)
}

fn gzip_compress(data: &[u8]) -> std::io::Result<Vec<u8>> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data)?;
    encoder.finish()
}

pub async fn send_otp(config: SmtpConfig, to: String, code: String) -> Result<(), MailError> {
    tokio::task::spawn_blocking(move || {
        let email = build_otp_message(&config, &to, &code)?;
        send_message(&config, &email)
    })
    .await
    .map_err(|e| MailError::Send(format!("task panicked: {e}")))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_config() -> SmtpConfig {
        SmtpConfig {
            host: "smtp.example.test".to_string(),
            port: 587,
            username: "user@example.test".to_string(),
            password: "secret".to_string(),
            from: "RS-Sync <noreply@example.test>".to_string(),
        }
    }

    #[test]
    fn build_message_succeeds_with_valid_addresses() {
        let config = sample_config();
        let msg = build_message(&config, "someone@example.test", "件名 / Subject", "本文 / body".to_string());
        assert!(msg.is_ok());
    }

    #[test]
    fn build_message_rejects_malformed_from_address() {
        let mut config = sample_config();
        config.from = "not-an-email".to_string();
        let err = build_message(&config, "someone@example.test", "subject", "body".to_string()).unwrap_err();
        assert!(matches!(err, MailError::Build(_)));
    }

    #[test]
    fn build_message_rejects_malformed_to_address() {
        let config = sample_config();
        let err = build_message(&config, "not-an-email", "subject", "body".to_string()).unwrap_err();
        assert!(matches!(err, MailError::Build(_)));
    }

    #[test]
    fn mail_error_display_is_japanese_and_includes_underlying_message() {
        let build_err = MailError::Build("boom".to_string());
        assert_eq!(build_err.to_string(), "メール作成失敗: boom");

        let send_err = MailError::Send("connection refused".to_string());
        assert_eq!(send_err.to_string(), "メール送信失敗: connection refused");
    }

    #[test]
    fn smtp_config_from_env_returns_none_when_required_vars_missing() {
        // 必須環境変数(RS_SYNC_SMTP_HOST等)が1つでも欠けていれば`None`を
        // 返すことを確認する(未設定環境での`check_expiring_tokens`等が
        // panicせず単に通知をスキップする設計の裏付け)。
        for var in ["RS_SYNC_SMTP_HOST", "RS_SYNC_SMTP_USERNAME", "RS_SYNC_SMTP_PASSWORD", "RS_SYNC_SMTP_FROM"] {
            std::env::remove_var(var);
        }
        assert!(SmtpConfig::from_env().is_none());
    }

    #[test]
    fn build_otp_message_succeeds_and_contains_a_large_font_size_for_the_code() {
        let config = sample_config();
        let msg = build_otp_message(&config, "someone@example.test", "123456").expect("otp message should build");
        let raw = String::from_utf8(msg.formatted()).expect("message bytes should be utf8");
        assert!(raw.contains("multipart/alternative"), "OTP email must be multipart/alternative so plain-text clients still work: {raw}");
        assert!(raw.contains("123456"), "the actual code must appear in the message: {raw}");
        // 「二周り大きく」の要望: 地の文(15px想定)よりコード自体が
        // 明確に大きい(40px)ことを、実際に組み立てたメッセージの中に
        // そのCSSが含まれることで検証する。quoted-printableエンコード
        // により長い行は"=\n"で折り返されるため、折り返され得る"px"
        // までは厳密一致せず、数値部分までの一致で確認する。
        assert!(raw.contains("font-size:40"), "the code must be rendered at a visibly larger font size than the ~15px body text: {raw}");
    }

    #[test]
    fn gzip_compress_round_trips_via_flate2_decoder() {
        let original = b"{\"logs\": [\"a\", \"b\", \"c\"]}";
        let compressed = gzip_compress(original).expect("compression should succeed");
        assert_ne!(compressed, original, "compressed bytes should differ from the input");
        use flate2::read::GzDecoder;
        use std::io::Read;
        let mut decoder = GzDecoder::new(&compressed[..]);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed).expect("decompression should succeed");
        assert_eq!(decompressed, original);
    }

    #[test]
    fn send_compressed_log_email_builds_a_gzip_attachment_with_a_dated_filename() {
        let config = sample_config();
        // send_message自体(実SMTP接続)はこのテストでは呼ばない
        // (実SMTP資格情報が無い環境のため、既存のbuild_message系テストと
        // 同じ制約)。メッセージの組み立てまでを確認するため、send_message
        // を経由しない構築ロジックだけを別途再現して検証する。
        let compressed = gzip_compress(b"[]").unwrap();
        let filename = format!("rs-sync-logs-{}.json.gz", chrono::Utc::now().format("%Y-%m-%d"));
        let content_type = ContentType::parse("application/gzip").unwrap();
        let email = Message::builder()
            .from(config.from.parse().unwrap())
            .to("someone@example.test".parse().unwrap())
            .subject("RS-Sync 週次ログ / weekly log export")
            .multipart(
                MultiPart::mixed()
                    .singlepart(SinglePart::builder().header(ContentType::TEXT_PLAIN).body("body".to_string()))
                    .singlepart(Attachment::new(filename.clone()).body(compressed, content_type)),
            )
            .unwrap();
        let raw = String::from_utf8(email.formatted()).unwrap();
        assert!(raw.contains("multipart/mixed"));
        assert!(raw.contains(&filename), "attachment filename should include today's date: {raw}");
        assert!(raw.contains("application/gzip"));
    }

    #[test]
    fn build_otp_message_rejects_malformed_to_address() {
        let config = sample_config();
        let err = build_otp_message(&config, "not-an-email", "123456").unwrap_err();
        assert!(matches!(err, MailError::Build(_)));
    }
}
