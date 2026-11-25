//! Public JSON parser limits, escaping and malformed-input behavior.

use terra_studio::json::{self, Json, MAX_ARRAY, MAX_DEPTH, MAX_OBJECT, MAX_STRING};

#[test]
fn nesting_depth_is_enforced() {
    let ok = format!("{}{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
    assert!(json::parse(&ok).is_ok());
    let bad = format!("{}{}", "[".repeat(MAX_DEPTH + 1), "]".repeat(MAX_DEPTH + 1));
    assert!(json::parse(&bad).is_err());
}

#[test]
fn string_and_container_sizes_are_enforced() {
    let big_string = format!("\"{}\"", "a".repeat(MAX_STRING + 1));
    assert!(json::parse(&big_string).is_err());
    let big_array = format!("[{}]", vec!["0"; MAX_ARRAY + 1].join(","));
    assert!(json::parse(&big_array).is_err());
    let mut object = String::from("{");
    for i in 0..=MAX_OBJECT {
        if i > 0 {
            object.push(',');
        }
        object.push_str(&format!("\"k{i}\":0"));
    }
    object.push('}');
    assert!(json::parse(&object).is_err());
}

#[test]
fn malformed_documents_are_rejected() {
    for text in [
        "",
        "{",
        "[",
        "{\"a\":}",
        "{\"a\":1,}",
        "[1,]",
        "{\"a\":01}",
        "{\"a\":1e999}",
        "{\"a\":NaN}",
        "\"\\q\"",
        "\"unterminated",
        "{\"a\":1} trailing",
        "{\"a\":1,\"a\":2}",
        "{\"a\":\"\\ud800\"}",
        "// comment",
        "undefined",
    ] {
        assert!(json::parse(text).is_err(), "{text:?}");
    }
}

#[test]
fn valid_documents_round_trip_through_the_writer() {
    for i in 0..200u32 {
        let value = Json::obj(vec![
            ("i", Json::Number(f64::from(i))),
            ("name", Json::str(format!("world-{i}-\"quoted\"\n"))),
            (
                "list",
                Json::array(vec![Json::Bool(i % 2 == 0), Json::Null, Json::Number(-1.5)]),
            ),
            ("nested", Json::obj(vec![("x", Json::Number(0.1))])),
        ]);
        let text = value.to_string();
        let back = json::parse(&text).unwrap();
        assert_eq!(back, value);
    }
}

#[test]
fn one_thousand_truncated_requests_fail_without_panicking() {
    let mut rejected = 0;
    for i in 0..1_000usize {
        let request = format!("{{\"seed\":{i},\"scenario\":\"seed = {i}\\n\"");
        assert!(json::parse(&request).is_err());
        rejected += 1;
    }
    assert_eq!(rejected, 1_000);
}

#[test]
fn unicode_escapes_and_utf8_are_preserved() {
    let value = json::parse("\"\\u2603 \\ud83d\\ude00 café\"").unwrap();
    assert_eq!(value.as_str(), Some("☃ 😀 café"));
    let serialized = Json::str("☃ 😀 café").to_string();
    assert_eq!(json::parse(&serialized).unwrap(), Json::str("☃ 😀 café"));
}

#[test]
fn accessors_do_not_coerce_across_types() {
    let value = Json::obj(vec![("n", Json::Number(3.0)), ("s", Json::str("x"))]);
    assert_eq!(value.get("n").unwrap().as_u64(), Some(3));
    assert_eq!(value.get("n").unwrap().as_str(), None);
    assert_eq!(value.get("s").unwrap().as_f64(), None);
    assert_eq!(Json::Number(1.5).as_u64(), None);
    assert_eq!(Json::Number(-1.0).as_u64(), None);
    assert_eq!(Json::Null.as_bool(), None);
}
