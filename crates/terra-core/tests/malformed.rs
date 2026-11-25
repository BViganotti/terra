//! Malformed-input acceptance: one thousand bad scenarios must fail cleanly.
//!
//! The parser is the only way an attacker can influence configuration. Every
//! case below is guaranteed malformed by construction, and the parser must
//! return an error without panicking.

use terra_core::scenario::{self, MAX_INPUT_BYTES};

#[test]
fn one_thousand_malformed_scenarios_fail_cleanly() {
    let mut rejected = 0;
    for i in 0..1_000usize {
        let case = match i % 8 {
            0 => format!("seed = {i}\nunknown_key_{i} = 1\n"),
            1 => format!("seed = {i}\nseed = {}\n", i + 1),
            2 => format!("name = \"unterminated {i}\n"),
            3 => format!("years = {}\n", 20_001 + i),
            4 => format!("subdiv = {}.5\n", i % 10),
            5 => format!("seed = not_a_number_{i}\n"),
            6 => format!("name = \"{}\"\n", "x".repeat(81)),
            _ => format!("plate = {i}\n"),
        };
        let result = scenario::parse(&case);
        assert!(result.is_err(), "case {i} unexpectedly parsed: {case:?}");
        rejected += 1;
    }
    assert_eq!(rejected, 1_000);
}

#[test]
fn size_and_line_limits_are_enforced() {
    let huge = format!("seed = 1\n{}", "# pad\n".repeat(MAX_INPUT_BYTES));
    assert!(scenario::parse(&huge).is_err());
    let long_line = format!("seed = 1\n{}\n", "1".repeat(600));
    assert!(scenario::parse(&long_line).is_err());
    let many_lines = "\n".repeat(300);
    assert!(scenario::parse(&many_lines).is_err());
    let overlong_name = format!("name = \"{}\"\n", "é".repeat(81));
    assert!(scenario::parse(&overlong_name).is_err());
}

#[test]
fn mutation_sweep_never_panics() {
    let base = "name = \"Terra\"\nseed = 42\nsubdiv = 4\nyears = 2000\nplates = 9\nland = 0.31\n";
    let mut state = 0x5eed_1234_9abc_def0u64;
    for _ in 0..5_000 {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let mut bytes = base.as_bytes().to_vec();
        let at = (state as usize) % bytes.len();
        let value = ((state >> 33) & 0x7f) as u8;
        match state % 4 {
            0 => bytes[at] = value,
            1 => bytes.insert(at, value),
            2 => {
                bytes.remove(at);
            }
            _ => {
                let extra = (state >> 40) as usize % 12;
                bytes.extend(std::iter::repeat(value).take(extra));
            }
        }
        if let Ok(text) = String::from_utf8(bytes) {
            match scenario::parse(&text) {
                Ok(s) => {
                    assert!(s.validate().is_ok());
                    assert!(s.config.subdiv <= 6);
                    assert!(s.config.years <= 20_000);
                }
                Err(e) => {
                    assert!(!e.to_string().is_empty());
                }
            }
        }
    }
}

#[test]
fn valid_scenarios_still_parse_after_mutations() {
    // A small canary: the parser must not become "reject everything".
    let s = scenario::parse("name = \"Canary\"\nseed = 1\nsubdiv = 2\nyears = 100\n").unwrap();
    assert_eq!(s.config.subdiv, 2);
    assert_eq!(s.config.years, 100);
    assert!(s.validate().is_ok());
}
