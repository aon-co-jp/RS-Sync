//! RustJSON — 人間が手で編集しやすい、ゆるい構文のJSONスーパーセット
//! (トレイリングカンマ・コメント・裸キー・シングルクォート文字列を許容)。
//! [`open-redmine`](https://github.com/aon-co-jp/open-redmine)の
//! `src/rustjson.rs`(さらに遡ると`RPoem`の`open-runo-rustjson`)から
//! そのまま移植(2026-07-29、ユーザー指示「他リポジトリと同じRJSON方式に
//! 揃える」)。パース結果は標準の`serde_json::Value`そのものなので、
//! 既存の`serde_json`ベースの型(`Store`等)は変更不要——読み込み時の
//! パーサーだけをこちらに差し替える。書き込みは引き続き
//! `serde_json::to_string_pretty`(整形済み標準JSON、RustJSONの入力
//! としても有効)を使い、保存ファイルの可読性を保つ。
//!
//! クロスリポジトリのCargo依存(他リポジトリのcrateへの直接依存)は、
//! この エコシステムでビルド脆弱性の原因になってきた実績(open-web-server/
//! RPoemのリリースCIで判明したpath依存問題)があるため避け、小さな
//! モジュールとして直接コピーする既存パターンを踏襲する。

use serde_json::{Map, Number, Value};

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum RustJsonError {
    #[error("unexpected end of input at byte {0}")]
    UnexpectedEof(usize),
    #[error("unexpected character '{1}' at byte {0}")]
    UnexpectedChar(usize, char),
    #[error("invalid number literal at byte {0}")]
    InvalidNumber(usize),
    #[error("invalid escape sequence at byte {0}")]
    InvalidEscape(usize),
    #[error("unterminated string starting at byte {0}")]
    UnterminatedString(usize),
    #[error("unterminated comment starting at byte {0}")]
    UnterminatedComment(usize),
    #[error("trailing data after the top-level value, starting at byte {0}")]
    TrailingData(usize),
}

pub fn parse(input: &str) -> Result<Value, RustJsonError> {
    let bytes = input.as_bytes();
    let mut pos = 0;
    skip_whitespace_and_comments(bytes, &mut pos)?;
    let value = parse_value(bytes, &mut pos)?;
    skip_whitespace_and_comments(bytes, &mut pos)?;
    if pos != bytes.len() {
        return Err(RustJsonError::TrailingData(pos));
    }
    Ok(value)
}

fn skip_whitespace_and_comments(bytes: &[u8], pos: &mut usize) -> Result<(), RustJsonError> {
    loop {
        while *pos < bytes.len() && bytes[*pos].is_ascii_whitespace() {
            *pos += 1;
        }
        if *pos + 1 < bytes.len() && bytes[*pos] == b'/' && bytes[*pos + 1] == b'/' {
            *pos += 2;
            while *pos < bytes.len() && bytes[*pos] != b'\n' {
                *pos += 1;
            }
            continue;
        }
        if *pos + 1 < bytes.len() && bytes[*pos] == b'/' && bytes[*pos + 1] == b'*' {
            let start = *pos;
            *pos += 2;
            loop {
                if *pos + 1 >= bytes.len() {
                    if bytes.get(*pos..*pos + 2) == Some(b"*/") {
                        *pos += 2;
                        break;
                    }
                    return Err(RustJsonError::UnterminatedComment(start));
                }
                if bytes[*pos] == b'*' && bytes[*pos + 1] == b'/' {
                    *pos += 2;
                    break;
                }
                *pos += 1;
            }
            continue;
        }
        break;
    }
    Ok(())
}

fn peek(bytes: &[u8], pos: usize) -> Result<u8, RustJsonError> {
    bytes.get(pos).copied().ok_or(RustJsonError::UnexpectedEof(pos))
}

fn parse_value(bytes: &[u8], pos: &mut usize) -> Result<Value, RustJsonError> {
    skip_whitespace_and_comments(bytes, pos)?;
    match peek(bytes, *pos)? {
        b'{' => parse_object(bytes, pos),
        b'[' => parse_array(bytes, pos),
        b'"' => Ok(Value::String(parse_quoted_string(bytes, pos, b'"')?)),
        b'\'' => Ok(Value::String(parse_quoted_string(bytes, pos, b'\'')?)),
        b't' => parse_literal(bytes, pos, "true", Value::Bool(true)),
        b'f' => parse_literal(bytes, pos, "false", Value::Bool(false)),
        b'n' => parse_literal(bytes, pos, "null", Value::Null),
        b'-' | b'0'..=b'9' => parse_number(bytes, pos),
        c => Err(RustJsonError::UnexpectedChar(*pos, c as char)),
    }
}

fn parse_literal(bytes: &[u8], pos: &mut usize, literal: &str, value: Value) -> Result<Value, RustJsonError> {
    let end = *pos + literal.len();
    if bytes.get(*pos..end) == Some(literal.as_bytes()) {
        *pos = end;
        Ok(value)
    } else {
        Err(RustJsonError::UnexpectedChar(*pos, peek(bytes, *pos)? as char))
    }
}

fn parse_object(bytes: &[u8], pos: &mut usize) -> Result<Value, RustJsonError> {
    *pos += 1;
    let mut map = Map::new();
    loop {
        skip_whitespace_and_comments(bytes, pos)?;
        if peek(bytes, *pos)? == b'}' {
            *pos += 1;
            break;
        }
        let key = parse_key(bytes, pos)?;
        skip_whitespace_and_comments(bytes, pos)?;
        if peek(bytes, *pos)? != b':' {
            return Err(RustJsonError::UnexpectedChar(*pos, peek(bytes, *pos)? as char));
        }
        *pos += 1;
        let value = parse_value(bytes, pos)?;
        map.insert(key, value);
        skip_whitespace_and_comments(bytes, pos)?;
        match peek(bytes, *pos)? {
            b',' => {
                *pos += 1;
                skip_whitespace_and_comments(bytes, pos)?;
                if peek(bytes, *pos)? == b'}' {
                    *pos += 1;
                    break;
                }
            }
            b'}' => {
                *pos += 1;
                break;
            }
            c => return Err(RustJsonError::UnexpectedChar(*pos, c as char)),
        }
    }
    Ok(Value::Object(map))
}

fn parse_key(bytes: &[u8], pos: &mut usize) -> Result<String, RustJsonError> {
    match peek(bytes, *pos)? {
        b'"' => parse_quoted_string(bytes, pos, b'"'),
        b'\'' => parse_quoted_string(bytes, pos, b'\''),
        c if c == b'_' || c.is_ascii_alphabetic() => {
            let start = *pos;
            *pos += 1;
            while *pos < bytes.len() && (bytes[*pos] == b'_' || bytes[*pos].is_ascii_alphanumeric()) {
                *pos += 1;
            }
            Ok(String::from_utf8_lossy(&bytes[start..*pos]).into_owned())
        }
        c => Err(RustJsonError::UnexpectedChar(*pos, c as char)),
    }
}

fn parse_array(bytes: &[u8], pos: &mut usize) -> Result<Value, RustJsonError> {
    *pos += 1;
    let mut items = Vec::new();
    loop {
        skip_whitespace_and_comments(bytes, pos)?;
        if peek(bytes, *pos)? == b']' {
            *pos += 1;
            break;
        }
        items.push(parse_value(bytes, pos)?);
        skip_whitespace_and_comments(bytes, pos)?;
        match peek(bytes, *pos)? {
            b',' => {
                *pos += 1;
                skip_whitespace_and_comments(bytes, pos)?;
                if peek(bytes, *pos)? == b']' {
                    *pos += 1;
                    break;
                }
            }
            b']' => {
                *pos += 1;
                break;
            }
            c => return Err(RustJsonError::UnexpectedChar(*pos, c as char)),
        }
    }
    Ok(Value::Array(items))
}

fn parse_quoted_string(bytes: &[u8], pos: &mut usize, quote: u8) -> Result<String, RustJsonError> {
    let start = *pos;
    *pos += 1;
    let mut out = String::new();
    loop {
        let c = *bytes.get(*pos).ok_or(RustJsonError::UnterminatedString(start))?;
        if c == quote {
            *pos += 1;
            return Ok(out);
        }
        if c == b'\\' {
            *pos += 1;
            let escaped = *bytes.get(*pos).ok_or(RustJsonError::UnterminatedString(start))?;
            match escaped {
                b'"' => out.push('"'),
                b'\'' => out.push('\''),
                b'\\' => out.push('\\'),
                b'/' => out.push('/'),
                b'n' => out.push('\n'),
                b't' => out.push('\t'),
                b'r' => out.push('\r'),
                b'b' => out.push('\u{8}'),
                b'f' => out.push('\u{c}'),
                b'u' => {
                    let hex_start = *pos + 1;
                    let hex = bytes.get(hex_start..hex_start + 4).and_then(|h| std::str::from_utf8(h).ok()).ok_or(RustJsonError::InvalidEscape(*pos))?;
                    let code = u32::from_str_radix(hex, 16).map_err(|_| RustJsonError::InvalidEscape(*pos))?;
                    let ch = char::from_u32(code).ok_or(RustJsonError::InvalidEscape(*pos))?;
                    out.push(ch);
                    *pos += 4;
                }
                _ => return Err(RustJsonError::InvalidEscape(*pos)),
            }
            *pos += 1;
            continue;
        }
        let ch_len = utf8_char_len(c);
        let ch_bytes = bytes.get(*pos..*pos + ch_len).ok_or(RustJsonError::UnterminatedString(start))?;
        let ch_str = std::str::from_utf8(ch_bytes).map_err(|_| RustJsonError::UnterminatedString(start))?;
        out.push_str(ch_str);
        *pos += ch_len;
    }
}

fn utf8_char_len(first_byte: u8) -> usize {
    if first_byte & 0x80 == 0 {
        1
    } else if first_byte & 0xE0 == 0xC0 {
        2
    } else if first_byte & 0xF0 == 0xE0 {
        3
    } else {
        4
    }
}

fn parse_number(bytes: &[u8], pos: &mut usize) -> Result<Value, RustJsonError> {
    let start = *pos;
    if peek(bytes, *pos)? == b'-' {
        *pos += 1;
    }
    let digits_start = *pos;
    while *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
        *pos += 1;
    }
    if *pos == digits_start {
        return Err(RustJsonError::InvalidNumber(start));
    }
    let mut is_integer = true;
    if *pos < bytes.len() && bytes[*pos] == b'.' {
        is_integer = false;
        *pos += 1;
        let frac_start = *pos;
        while *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
            *pos += 1;
        }
        if *pos == frac_start {
            return Err(RustJsonError::InvalidNumber(start));
        }
    }
    if *pos < bytes.len() && (bytes[*pos] == b'e' || bytes[*pos] == b'E') {
        is_integer = false;
        *pos += 1;
        if *pos < bytes.len() && (bytes[*pos] == b'+' || bytes[*pos] == b'-') {
            *pos += 1;
        }
        let exp_start = *pos;
        while *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
            *pos += 1;
        }
        if *pos == exp_start {
            return Err(RustJsonError::InvalidNumber(start));
        }
    }
    let text = std::str::from_utf8(&bytes[start..*pos]).map_err(|_| RustJsonError::InvalidNumber(start))?;
    let number: Number = if is_integer {
        if let Ok(i) = text.parse::<i64>() {
            Number::from(i)
        } else if let Ok(u) = text.parse::<u64>() {
            Number::from(u)
        } else {
            text.parse::<f64>().ok().and_then(Number::from_f64).ok_or(RustJsonError::InvalidNumber(start))?
        }
    } else {
        text.parse::<f64>().ok().and_then(Number::from_f64).ok_or(RustJsonError::InvalidNumber(start))?
    };
    Ok(Value::Number(number))
}

/// バイト列(ファイルから読み込んだ生データ)をRustJSONの緩い構文で
/// パースし、型`T`へデシリアライズする。パース・デシリアライズどちらかが
/// 失敗した場合は`None`(呼び出し側は既存の`unwrap_or_default()`と同じ
/// 「壊れていたら既定値」という寛容な挙動を維持する)。
pub fn parse_typed<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Option<T> {
    let text = String::from_utf8_lossy(bytes);
    let value = parse(&text).ok()?;
    serde_json::from_value(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_strict_json_unchanged() {
        assert_eq!(parse(r#"{"a": 1}"#).unwrap(), json!({"a": 1}));
    }

    #[test]
    fn trailing_comma_and_comments_are_accepted() {
        let input = "{\n  // note\n  \"a\": 1,\n}";
        assert_eq!(parse(input).unwrap(), json!({"a": 1}));
    }

    #[test]
    fn unquoted_keys_and_single_quotes_are_accepted() {
        assert_eq!(parse("{name: 'sword'}").unwrap(), json!({"name": "sword"}));
    }

    #[derive(Debug, PartialEq, serde::Deserialize)]
    struct Sample {
        id: u64,
        name: String,
    }

    #[test]
    fn parse_typed_round_trips_lenient_input_into_a_struct() {
        let bytes = b"{ id: 1, name: 'hello', }";
        let sample: Sample = parse_typed(bytes).unwrap();
        assert_eq!(sample, Sample { id: 1, name: "hello".to_string() });
    }

    #[test]
    fn parse_typed_returns_none_on_malformed_input() {
        let result: Option<Sample> = parse_typed(b"not json at all {{{");
        assert!(result.is_none());
    }
}
