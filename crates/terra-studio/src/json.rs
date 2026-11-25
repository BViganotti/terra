//! JSON reading and writing with hard limits.
//!
//! The Studio accepts small configuration documents from the browser and emits
//! larger result documents. The writer serializes [`Json`] values with stable
//! key order (the order in which pairs were pushed) and shortest round-trip
//! numbers. The parser is deliberately boring: a byte cursor with explicit
//! depth, length and nesting limits. It never allocates based on an unchecked
//! prefix and never recurses deeper than [`MAX_DEPTH`].
//!
//! Malformed input is returned as [`JsonError`] with a byte offset. The fuzz
//! campaign feeds thousands of mutated documents through this parser.

use std::fmt;

/// Maximum input size in bytes.
pub const MAX_INPUT: usize = 256 * 1024;
/// Maximum nesting depth.
pub const MAX_DEPTH: usize = 32;
/// Maximum string length in bytes after unescaping.
pub const MAX_STRING: usize = 64 * 1024;
/// Maximum array elements.
pub const MAX_ARRAY: usize = 4_096;
/// Maximum object members.
pub const MAX_OBJECT: usize = 1_024;

/// A JSON value.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    /// JSON null.
    Null,
    /// Boolean.
    Bool(bool),
    /// Number.
    Number(f64),
    /// String.
    String(String),
    /// Array.
    Array(Vec<Json>),
    /// Object with insertion order preserved.
    Object(Vec<(String, Json)>),
}

impl Json {
    /// Builds an object from string keys.
    pub fn obj(pairs: Vec<(&str, Json)>) -> Json {
        Json::Object(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }

    /// Builds an array.
    pub fn array(items: Vec<Json>) -> Json {
        Json::Array(items)
    }

    /// Convenience for a string value.
    pub fn str(s: impl Into<String>) -> Json {
        Json::String(s.into())
    }

    /// Convenience for a number, coercing non-finite values to zero.
    pub fn num(v: f64) -> Json {
        Json::Number(if v.is_finite() { v } else { 0.0 })
    }

    /// Looks up an object member.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Indexes an array.
    pub fn index(&self, i: usize) -> Option<&Json> {
        match self {
            Json::Array(items) => items.get(i),
            _ => None,
        }
    }

    /// True for null.
    pub fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }

    /// Numeric view.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// Unsigned integer view, rejecting fractions and negatives.
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Json::Number(n) if n.is_finite() && *n >= 0.0 && n.fract() == 0.0 => {
                if *n <= 9_007_199_254_740_992.0 {
                    Some(*n as u64)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Signed integer view.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Json::Number(n)
                if n.is_finite() && n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_992.0 =>
            {
                Some(*n as i64)
            }
            _ => None,
        }
    }

    /// String view.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }

    /// Boolean view.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Array view.
    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }

    /// Object view.
    pub fn as_object(&self) -> Option<&[(String, Json)]> {
        match self {
            Json::Object(pairs) => Some(pairs),
            _ => None,
        }
    }

    /// Compact JSON rendering.
    pub fn to_string(&self) -> String {
        let mut out = String::new();
        self.write_into(&mut out);
        out
    }

    /// Appends the compact rendering to a buffer.
    pub fn write_into(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(true) => out.push_str("true"),
            Json::Bool(false) => out.push_str("false"),
            Json::Number(n) => {
                if !n.is_finite() {
                    out.push('0');
                } else if *n == n.trunc() && n.abs() < 1e15 {
                    // Avoid printing "-0".
                    if *n == 0.0 {
                        out.push('0');
                    } else {
                        out.push_str(&format!("{n:.0}"));
                    }
                } else {
                    out.push_str(&format!("{n}"));
                }
            }
            Json::String(s) => write_escaped(out, s),
            Json::Array(items) => {
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write_into(out);
                }
                out.push(']');
            }
            Json::Object(pairs) => {
                out.push('{');
                for (i, (k, v)) in pairs.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_escaped(out, k);
                    out.push(':');
                    v.write_into(out);
                }
                out.push('}');
            }
        }
    }
}

fn write_escaped(out: &mut String, s: &str) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c if (c as u32) >= 0x7f && (c as u32) < 0xa0 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// A parse failure with a byte offset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonError {
    /// Byte offset into the input.
    pub offset: usize,
    /// Description.
    pub message: String,
}

impl JsonError {
    fn new(offset: usize, message: impl Into<String>) -> Self {
        Self {
            offset,
            message: message.into(),
        }
    }
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "json at byte {}: {}", self.offset, self.message)
    }
}

impl std::error::Error for JsonError {}

/// Parses a JSON document.
pub fn parse(input: &str) -> Result<Json, JsonError> {
    if input.len() > MAX_INPUT {
        return Err(JsonError::new(
            0,
            format!("input exceeds {MAX_INPUT} bytes"),
        ));
    }
    let mut p = Parser {
        bytes: input.as_bytes(),
        pos: 0,
    };
    p.skip_ws();
    let value = p.value(0)?;
    p.skip_ws();
    if p.pos != p.bytes.len() {
        return Err(JsonError::new(p.pos, "trailing data"));
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.pos += 1;
        Some(b)
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if matches!(b, b' ' | b'\t' | b'\n' | b'\r') {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn expect(&mut self, want: u8) -> Result<(), JsonError> {
        match self.bump() {
            Some(b) if b == want => Ok(()),
            Some(b) => Err(JsonError::new(
                self.pos - 1,
                format!("expected '{}', found '{}'", want as char, b as char),
            )),
            None => Err(JsonError::new(self.pos, "unexpected end of input")),
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, JsonError> {
        if depth > MAX_DEPTH {
            return Err(JsonError::new(self.pos, "nesting limit exceeded"));
        }
        self.skip_ws();
        let at = self.pos;
        match self.peek() {
            None => Err(JsonError::new(at, "unexpected end of input")),
            Some(b'n') => {
                self.literal(b"null")?;
                Ok(Json::Null)
            }
            Some(b't') => {
                self.literal(b"true")?;
                Ok(Json::Bool(true))
            }
            Some(b'f') => {
                self.literal(b"false")?;
                Ok(Json::Bool(false))
            }
            Some(b'"') => Ok(Json::String(self.string()?)),
            Some(b'[') => self.array(depth),
            Some(b'{') => self.object(depth),
            Some(b'-') | Some(b'0'..=b'9') => self.number(),
            Some(b) => Err(JsonError::new(
                at,
                format!("unexpected character '{}'", b as char),
            )),
        }
    }

    fn literal(&mut self, lit: &[u8]) -> Result<(), JsonError> {
        let start = self.pos;
        if self.bytes.len() < self.pos + lit.len() {
            self.pos = self.bytes.len();
            return Err(JsonError::new(start, "truncated literal"));
        }
        if &self.bytes[self.pos..self.pos + lit.len()] == lit {
            self.pos += lit.len();
            Ok(())
        } else {
            Err(JsonError::new(start, "invalid literal"))
        }
    }

    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        let mut digits = 0;
        let first_digit = self.peek();
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
            digits += 1;
        }
        if digits == 0 {
            return Err(JsonError::new(start, "number needs an integer part"));
        }
        if first_digit == Some(b'0') && digits > 1 {
            return Err(JsonError::new(start, "leading zero in number"));
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            let mut frac = 0;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
                frac += 1;
            }
            if frac == 0 {
                return Err(JsonError::new(self.pos, "number needs a fraction"));
            }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.pos += 1;
            }
            let mut exp = 0;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
                exp += 1;
            }
            if exp == 0 {
                return Err(JsonError::new(self.pos, "number needs an exponent"));
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| JsonError::new(start, "invalid utf-8 in number"))?;
        let value: f64 = text
            .parse()
            .map_err(|_| JsonError::new(start, "number out of range"))?;
        if !value.is_finite() {
            return Err(JsonError::new(start, "number is not finite"));
        }
        Ok(Json::Number(value))
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let Some(b) = self.bump() else {
                return Err(JsonError::new(self.pos, "unterminated string"));
            };
            match b {
                b'"' => break,
                b'\\' => {
                    let Some(esc) = self.bump() else {
                        return Err(JsonError::new(self.pos, "unterminated escape"));
                    };
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{08}'),
                        b'f' => out.push('\u{0c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            let code = if (0xd800..=0xdbff).contains(&hi) {
                                if self.bump() != Some(b'\\') || self.bump() != Some(b'u') {
                                    return Err(JsonError::new(
                                        self.pos,
                                        "high surrogate without low surrogate",
                                    ));
                                }
                                let lo = self.hex4()?;
                                if !(0xdc00..=0xdfff).contains(&lo) {
                                    return Err(JsonError::new(self.pos, "invalid low surrogate"));
                                }
                                0x1_0000
                                    + (((u32::from(hi) - 0xd800) << 10) | (u32::from(lo) - 0xdc00))
                            } else if (0xdc00..=0xdfff).contains(&hi) {
                                return Err(JsonError::new(self.pos, "lone low surrogate"));
                            } else {
                                u32::from(hi)
                            };
                            let ch = char::from_u32(code).ok_or_else(|| {
                                JsonError::new(self.pos, "invalid unicode scalar")
                            })?;
                            out.push(ch);
                        }
                        other => {
                            return Err(JsonError::new(
                                self.pos,
                                format!("invalid escape '\\{}'", other as char),
                            ))
                        }
                    }
                }
                b if b < 0x20 => {
                    return Err(JsonError::new(self.pos - 1, "control character in string"))
                }
                b if b < 0x80 => out.push(b as char),
                _ => {
                    // Decode a UTF-8 sequence starting from this byte.
                    self.pos -= 1;
                    let rest = &self.bytes[self.pos..];
                    let text = std::str::from_utf8(rest).map_err(|e| {
                        JsonError::new(self.pos, format!("invalid utf-8: {}", e.valid_up_to()))
                    })?;
                    let ch = text
                        .chars()
                        .next()
                        .ok_or_else(|| JsonError::new(self.pos, "empty utf-8 sequence"))?;
                    out.push(ch);
                    self.pos += ch.len_utf8();
                }
            }
            if out.len() > MAX_STRING {
                return Err(JsonError::new(self.pos, "string too long"));
            }
        }
        Ok(out)
    }

    fn hex4(&mut self) -> Result<u16, JsonError> {
        let mut value = 0u16;
        for _ in 0..4 {
            let Some(b) = self.bump() else {
                return Err(JsonError::new(self.pos, "truncated unicode escape"));
            };
            let digit = match b {
                b'0'..=b'9' => b - b'0',
                b'a'..=b'f' => b - b'a' + 10,
                b'A'..=b'F' => b - b'A' + 10,
                _ => return Err(JsonError::new(self.pos - 1, "invalid hex digit")),
            };
            value = (value << 4) | u16::from(digit);
        }
        Ok(value)
    }

    fn array(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.expect(b'[')?;
        self.skip_ws();
        let mut items = Vec::new();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Json::Array(items));
        }
        loop {
            if items.len() >= MAX_ARRAY {
                return Err(JsonError::new(self.pos, "array too long"));
            }
            items.push(self.value(depth + 1)?);
            self.skip_ws();
            match self.bump() {
                Some(b',') => {
                    self.skip_ws();
                }
                Some(b']') => break,
                Some(_) => return Err(JsonError::new(self.pos - 1, "expected , or ]")),
                None => return Err(JsonError::new(self.pos, "unterminated array")),
            }
        }
        Ok(Json::Array(items))
    }

    fn object(&mut self, depth: usize) -> Result<Json, JsonError> {
        self.expect(b'{')?;
        self.skip_ws();
        let mut pairs: Vec<(String, Json)> = Vec::new();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Json::Object(pairs));
        }
        loop {
            if pairs.len() >= MAX_OBJECT {
                return Err(JsonError::new(self.pos, "object too large"));
            }
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(JsonError::new(self.pos, "object key must be a string"));
            }
            let key = self.string()?;
            if pairs.iter().any(|(k, _)| *k == key) {
                return Err(JsonError::new(self.pos, "duplicate object key"));
            }
            self.skip_ws();
            self.expect(b':')?;
            let value = self.value(depth + 1)?;
            pairs.push((key, value));
            self.skip_ws();
            match self.bump() {
                Some(b',') => {
                    self.skip_ws();
                }
                Some(b'}') => break,
                Some(_) => return Err(JsonError::new(self.pos - 1, "expected , or }")),
                None => return Err(JsonError::new(self.pos, "unterminated object")),
            }
        }
        Ok(Json::Object(pairs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_scalars() {
        for value in [
            Json::Null,
            Json::Bool(true),
            Json::Bool(false),
            Json::Number(0.0),
            Json::Number(-12.5),
            Json::Number(1e20),
            Json::String("hello \"world\"\n".into()),
            Json::String("héllo ☃".into()),
        ] {
            let text = value.to_string();
            let back = parse(&text).unwrap_or_else(|e| panic!("{text}: {e}"));
            if let (Json::Number(a), Json::Number(b)) = (&value, &back) {
                assert_eq!(a.to_bits(), b.to_bits(), "{text}");
            } else {
                assert_eq!(back, value, "{text}");
            }
        }
    }

    #[test]
    fn round_trips_structures() {
        let v = Json::obj(vec![
            ("seed", Json::Number(42.0)),
            (
                "biomes",
                Json::array(vec![Json::str("ocean"), Json::str("forest")]),
            ),
            ("nested", Json::obj(vec![("ok", Json::Bool(true))])),
        ]);
        let text = v.to_string();
        let back = parse(&text).unwrap();
        assert_eq!(back, v);
        assert_eq!(back.get("seed").unwrap().as_u64(), Some(42));
        assert_eq!(back.get("biomes").unwrap().as_array().unwrap().len(), 2);
        assert_eq!(
            back.get("nested").unwrap().get("ok").unwrap().as_bool(),
            Some(true)
        );
        assert!(back.index(0).is_none());
        assert!(back.get("missing").is_none());
    }

    #[test]
    fn escapes_are_safe() {
        let v = Json::str("a\u{0}b\u{1f}\u{7f}");
        let text = v.to_string();
        assert!(!text.contains('\u{0}'));
        let back = parse(&text).unwrap();
        assert_eq!(back.as_str(), Some("a\u{0}b\u{1f}\u{7f}"));
    }

    #[test]
    fn unicode_escapes_work() {
        let back = parse(r#""\u00e9 \u2603 \ud83d\ude00""#).unwrap();
        assert_eq!(back.as_str(), Some("é ☃ 😀"));
        assert!(parse(r#""\ud800""#).is_err());
        assert!(parse(r#""\udc00""#).is_err());
        assert!(parse(r#""\ud800\u0041""#).is_err());
        assert!(parse(r#""\uZZZZ""#).is_err());
        assert!(parse("\"\\u12\"").is_err());
    }

    #[test]
    fn rejects_malformed_documents() {
        for text in [
            "",
            " ",
            "{",
            "[",
            "{]",
            "[}",
            "{\"a\"}",
            "{\"a\":}",
            "{a:1}",
            "{\"a\":1,}",
            "[1,]",
            "[1 2]",
            "\"unterminated",
            "nul",
            "truex",
            "01",
            "-01",
            "-",
            "1.",
            ".5",
            "+1",
            "1e",
            "1e+",
            "\"\\q\"",
            "\"a\u{0}b\"",
            "NaN",
            "Infinity",
            "1e999",
        ] {
            assert!(parse(text).is_err(), "{text:?} should fail");
        }
    }

    #[test]
    fn depth_is_bounded() {
        let deep = format!("{}{}", "[".repeat(MAX_DEPTH + 5), "]".repeat(MAX_DEPTH + 5));
        assert!(parse(&deep).is_err());
        let ok = format!("{}{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
        assert!(parse(&ok).is_ok());
    }

    #[test]
    fn sizes_are_bounded() {
        let long_string = format!("\"{}\"", "a".repeat(MAX_STRING + 1));
        assert!(parse(&long_string).is_err());
        let long_array = format!("[{}]", vec!["0"; MAX_ARRAY + 1].join(","));
        assert!(parse(&long_array).is_err());
        let long_input = " ".repeat(MAX_INPUT + 1);
        assert!(parse(&long_input).is_err());
    }

    #[test]
    fn duplicate_keys_are_rejected() {
        assert!(parse(r#"{"a":1,"a":2}"#).is_err());
    }

    #[test]
    fn trailing_data_is_rejected() {
        assert!(parse("1 2").is_err());
        assert!(parse("{} {}").is_err());
        assert!(parse("[] \n").is_ok());
    }

    #[test]
    fn accessors_reject_wrong_types() {
        let v = Json::obj(vec![("n", Json::Number(4.0)), ("s", Json::str("x"))]);
        assert_eq!(v.get("n").unwrap().as_u64(), Some(4));
        assert_eq!(v.get("n").unwrap().as_str(), None);
        assert_eq!(v.get("s").unwrap().as_f64(), None);
        assert_eq!(Json::Number(-1.0).as_u64(), None);
        assert_eq!(Json::Number(1.5).as_u64(), None);
        assert!(Json::Number(f64::NAN).to_string() == "0");
        assert_eq!(Json::Null.as_bool(), None);
        assert_eq!(Json::Null.as_object(), None);
    }

    #[test]
    fn parser_survives_structural_mutations() {
        let base = br#"{"seed":42,"years":1000,"preset":"temperate","layers":["plates","biomes"]}"#;
        let mut state = 0x9e37_79b9u64;
        for _ in 0..800 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let mut bytes = base.to_vec();
            let at = (state as usize) % bytes.len();
            let b = ((state >> 33) & 0xff) as u8;
            match state % 3 {
                0 => bytes[at] = b,
                1 => bytes.insert(at, b),
                _ => {
                    bytes.remove(at);
                }
            }
            if let Ok(text) = std::str::from_utf8(&bytes) {
                let _ = parse(text);
            }
        }
    }
}
