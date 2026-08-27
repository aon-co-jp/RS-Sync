//! TOTP(RFC 6238、認証アプリ型2段階認証)の自前実装(2026-08-13新設、
//! ユーザー指示「初回にe-mailと携帯電話番号も登録して、e-mail不明に
//! なっても2FAでもログイン可能にして」)。
//!
//! 外部のtotp専用crateには依存せず、`hmac`+`sha1`(HOTP/TOTPの標準アルゴ
//! リズムはHMAC-SHA1を使う、RFC 6238準拠)のみで実装する——このエコ
//! システムの他リポジトリ(open-easy-web等)が採用している「HMAC-SHA1+
//! base32を自前実装」という既存方針をそのまま踏襲したもの。

use hmac::{Hmac, Mac};
use sha1::Sha1;

type HmacSha1 = Hmac<Sha1>;

const BASE32_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// 20バイト(160bit、RFC 4226推奨)のランダム秘密鍵を生成し、認証アプリへ
/// 手入力できるbase32文字列として返す。
pub fn generate_secret() -> String {
    let mut rng = rand::thread_rng();
    let bytes: [u8; 20] = rand::Rng::gen(&mut rng);
    base32_encode(&bytes)
}

fn base32_encode(data: &[u8]) -> String {
    let mut out = String::new();
    let mut bits = 0u32;
    let mut value = 0u32;
    for &b in data {
        value = (value << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            out.push(BASE32_ALPHABET[((value >> (bits - 5)) & 0x1F) as usize] as char);
            bits -= 5;
        }
    }
    if bits > 0 {
        out.push(BASE32_ALPHABET[((value << (5 - bits)) & 0x1F) as usize] as char);
    }
    out
}

fn base32_decode(s: &str) -> Option<Vec<u8>> {
    let mut bits = 0u32;
    let mut value = 0u32;
    let mut out = Vec::new();
    for c in s.to_ascii_uppercase().bytes() {
        if c == b'=' {
            continue;
        }
        let idx = BASE32_ALPHABET.iter().position(|&x| x == c)?;
        value = (value << 5) | idx as u32;
        bits += 5;
        if bits >= 8 {
            out.push(((value >> (bits - 8)) & 0xFF) as u8);
            bits -= 8;
        }
    }
    Some(out)
}

/// 指定したUNIX時刻(秒)における6桁のTOTPコードを直接計算する
/// (RFC 6238、30秒ステップ)。テストコードから「今の時刻に対する正解を
/// 総当たりせず直接計算する」ために公開している——open-easy-webの
/// CLAUDE.mdに記録されている教訓(総当たりだとdebugビルドでTOTPの時間窓を
/// 越えてflakyになる)を踏まえ、最初からこの設計にした。
pub fn code_at(secret: &str, unix_time: i64) -> Option<String> {
    let key = base32_decode(secret)?;
    let counter = (unix_time / 30) as u64;
    let counter_bytes = counter.to_be_bytes();
    let mut mac = HmacSha1::new_from_slice(&key).ok()?;
    mac.update(&counter_bytes);
    let hash = mac.finalize().into_bytes();
    let offset = (hash[hash.len() - 1] & 0x0F) as usize;
    let binary = ((hash[offset] as u32 & 0x7F) << 24)
        | ((hash[offset + 1] as u32) << 16)
        | ((hash[offset + 2] as u32) << 8)
        | (hash[offset + 3] as u32);
    Some(format!("{:06}", binary % 1_000_000))
}

/// 認証アプリ(Google Authenticator/Authy等)が読み取れる標準の
/// `otpauth://totp/...`URIを組み立てる(RFC準拠のkey URI形式)。
/// `account_label`・`issuer`はURLエンコードする(コロン・スペース等を
/// 含む可能性があるため)。
pub fn otpauth_uri(secret: &str, account_label: &str, issuer: &str) -> String {
    fn url_encode(s: &str) -> String {
        let mut out = String::new();
        for b in s.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(b as char);
                }
                _ => out.push_str(&format!("%{:02X}", b)),
            }
        }
        out
    }
    format!(
        "otpauth://totp/{}:{}?secret={}&issuer={}&algorithm=SHA1&digits=6&period=30",
        url_encode(issuer),
        url_encode(account_label),
        secret,
        url_encode(issuer)
    )
}

/// 2026-08-27新設(ユーザー指示「携帯電話番号でQRコード撮影の2FA」への
/// 対応)。上記`otpauth_uri`をQRコードとしてSVG文字列へレンダリングする。
/// **正直な開示**: このQRコードは認証アプリでの読み取りを想定した補助
/// 手段であり、既存のテキストシークレット表示(ガラケー等QRを読み取れない
/// 端末向け)を置き換えるものではない——両方を併記する設計(呼び出し側の
/// `main.rs`参照)。エラー訂正レベルは`Medium`(標準的な選択、印刷物や
/// 画面表示どちらでも安定して読み取れるバランス)。
pub fn totp_qr_svg(secret: &str, account_label: &str, issuer: &str) -> Result<String, String> {
    let uri = otpauth_uri(secret, account_label, issuer);
    let code = qrcode::QrCode::with_error_correction_level(&uri, qrcode::EcLevel::M)
        .map_err(|e| format!("QRコード生成に失敗しました / QR code generation failed: {e}"))?;
    let svg = code
        .render()
        .min_dimensions(200, 200)
        .dark_color(qrcode::render::svg::Color("#000000"))
        .light_color(qrcode::render::svg::Color("#ffffff"))
        .build();
    Ok(svg)
}

/// 時計のずれを許容するため、前後1ステップ(合計90秒分)も許容して照合
/// する。
pub fn verify_code(secret: &str, code: &str, now_unix: i64) -> bool {
    let code = code.trim();
    for step in [-30, 0, 30] {
        if let Some(expected) = code_at(secret, now_unix + step) {
            if expected == code {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base32_round_trips() {
        let data = b"hello totp secret!!!";
        let encoded = base32_encode(data);
        let decoded = base32_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn code_at_produces_six_digits() {
        let secret = generate_secret();
        let code = code_at(&secret, 1_723_000_000).unwrap();
        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn verify_code_accepts_correct_code_and_rejects_wrong_one() {
        let secret = generate_secret();
        let now = 1_723_000_000i64;
        let correct = code_at(&secret, now).unwrap();
        assert!(verify_code(&secret, &correct, now));
        assert!(!verify_code(&secret, "000000", now));
    }

    #[test]
    fn otpauth_uri_url_encodes_and_embeds_the_secret() {
        let uri = otpauth_uri("ABCD1234", "user@example.com", "RS-Sync 本番");
        assert!(uri.starts_with("otpauth://totp/"));
        assert!(uri.contains("secret=ABCD1234"));
        assert!(uri.contains("user%40example.com"));
        // スペース・非ASCII文字がパーセントエンコードされ、生の空白/
        // マルチバイト文字がそのままURIへ混入していないことを確認。
        assert!(!uri.contains(' '));
    }

    #[test]
    fn totp_qr_svg_produces_a_non_empty_svg_document() {
        let secret = generate_secret();
        let svg = totp_qr_svg(&secret, "user@example.com", "RS-Sync").expect("QR generation should succeed");
        assert!(svg.starts_with("<?xml") || svg.trim_start().starts_with("<svg"));
        assert!(svg.contains("<svg"));
        assert!(svg.len() > 100, "SVG should contain actual QR module data, not just a stub");
    }

    #[test]
    fn verify_code_tolerates_one_step_of_clock_skew() {
        let secret = generate_secret();
        let now = 1_723_000_000i64;
        let code_30s_ago = code_at(&secret, now - 30).unwrap();
        assert!(verify_code(&secret, &code_30s_ago, now));
        let code_60s_ago = code_at(&secret, now - 60).unwrap();
        // 2ステップ以上前のコードは許容範囲外。
        assert!(!verify_code(&secret, &code_60s_ago, now));
    }
}
