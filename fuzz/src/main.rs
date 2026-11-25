//! Terra malformed-input campaign.
//!
//! This is a deterministic, standard-library-only fuzz harness rather than a
//! `cargo-fuzz` target, because Terra takes no external crates. It feeds
//! thousands of malformed scenario files and JSON request bodies through the
//! real parsers and asserts that every one is rejected (or, for the mutation
//! sweep, either rejected or accepted with a valid configuration) without a
//! panic.
//!
//! Run with `cargo run --release -p terra-fuzz -- [iterations]`.

use std::process::ExitCode;

use terra_core::scenario::{self, MAX_INPUT_BYTES};
use terra_studio::json::{self, Json};

const DEFAULT_ITERATIONS: usize = 1_000;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let iterations = args
        .first()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(DEFAULT_ITERATIONS)
        .clamp(1, 200_000);
    let seed = args
        .get(1)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0x7e44_a11c_0f0d_0001);

    let mut report = Report::default();
    scenario_family(iterations, &mut report);
    json_family(iterations, &mut report);
    json_valid_family(iterations, &mut report);
    mutation_family(iterations, seed, &mut report);

    println!(
        "{{\"iterations\":{},\"scenarioRejected\":{},\"jsonRejected\":{},\"mutationsParsed\":{},\"mutationsRejected\":{},\"validRoundTrips\":{},\"status\":\"{}\"}}",
        iterations,
        report.scenario_rejected,
        report.json_rejected,
        report.mutations_parsed,
        report.mutations_rejected,
        report.valid_roundtrips,
        if report.failures.is_empty() { "pass" } else { "fail" }
    );
    if report.failures.is_empty() {
        ExitCode::SUCCESS
    } else {
        for failure in &report.failures {
            eprintln!("FAIL: {failure}");
        }
        ExitCode::FAILURE
    }
}

#[derive(Default)]
struct Report {
    scenario_rejected: usize,
    json_rejected: usize,
    mutations_parsed: usize,
    mutations_rejected: usize,
    valid_roundtrips: usize,
    failures: Vec<String>,
}

fn scenario_family(n: usize, report: &mut Report) {
    for i in 0..n {
        let case = match i % 10 {
            0 => format!("seed = {i}\nrogue_key_{i} = 1\n"),
            1 => format!("seed = 1\nseed = {i}\n"),
            2 => format!("name = \"unterminated {i}\n"),
            3 => format!("years = {}\n", 20_000 + i),
            4 => format!("subdiv = {}.5\n", i % 10),
            5 => format!("plates = not_a_number_{i}\n"),
            6 => format!("name = \"{}\"\n", "x".repeat(81)),
            7 => format!("plate = {i}\n"),
            8 => format!("land = {}\n", 5.0 + i as f64),
            _ => format!("{}", "z".repeat(MAX_INPUT_BYTES + 1)),
        };
        match scenario::parse(&case) {
            Err(_) => report.scenario_rejected += 1,
            Ok(_) => report
                .failures
                .push(format!("malformed scenario {i} was accepted")),
        }
    }
}

fn json_family(n: usize, report: &mut Report) {
    for i in 0..n {
        let deep = format!("{}{}", "[".repeat(40), "]".repeat(40));
        let case = match i % 10 {
            0 => format!("{{\"seed\":{i},\"years\":}}"),
            1 => format!("{{\"seed\":{i},\"seed\":2}}"),
            2 => format!("{{\"seed\":{i},\"name\":\"unterminated}}"),
            3 => format!("[{}]", vec!["0"; 5_000].join(",")),
            4 => deep,
            5 => format!("{{\"seed\":01{i}}}"),
            6 => format!("{{\"seed\":{i}}} trailing"),
            7 => format!("{{\"scenario\":\"\\ud800\"}}"),
            8 => format!("{{\"value\":1e999}}"),
            _ => format!("{{\"seed\":{i},\"x\":[1,]}}"),
        };
        match json::parse(&case) {
            Err(_) => report.json_rejected += 1,
            Ok(_) => report
                .failures
                .push(format!("malformed json {i} was accepted")),
        }
    }
}

fn json_valid_family(n: usize, report: &mut Report) {
    for i in 0..n {
        let value = Json::obj(vec![
            ("seed", Json::Number(i as f64)),
            ("years", Json::Number(1_000.0)),
            ("name", Json::str(format!("world-{i} \"quoted\""))),
            (
                "layers",
                Json::array(vec![Json::str("atlas"), Json::str("biomes")]),
            ),
            ("sound", Json::Bool(true)),
            ("missing", Json::Null),
            ("nested", Json::obj(vec![("n", Json::Number(-1.25))])),
        ]);
        let text = value.to_string();
        match json::parse(&text) {
            Ok(back) => {
                if back == value {
                    report.valid_roundtrips += 1;
                } else {
                    report
                        .failures
                        .push(format!("json round trip lost data for {i}"));
                }
            }
            Err(e) => report
                .failures
                .push(format!("valid json rejected for {i}: {e}")),
        }
    }
}

fn mutation_family(n: usize, seed: u64, report: &mut Report) {
    let scenarios = [
        "name = \"Terra\"\nseed = 42\nsubdiv = 3\nyears = 2000\nplates = 9\nland = 0.31\n",
        "preset = arid\nseed = 7\nland = 0.5\n",
        "preset = archipelago\nyears = 3000\nnote = \"islands\"\n",
    ];
    let jsons = [
        "{\"seed\":42,\"years\":1000,\"subdiv\":3,\"civs\":6}",
        "{\"preset\":\"icehouse\",\"seed\":7}",
        "{\"scenario\":\"seed = 5\\nsubdiv = 2\\n\"}",
    ];
    let mut state = seed | 1;
    for i in 0..n {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let base = if i % 2 == 0 {
            scenarios[(state as usize >> 8) % scenarios.len()]
        } else {
            jsons[(state as usize >> 8) % jsons.len()]
        };
        let mut bytes = base.as_bytes().to_vec();
        let at = (state as usize) % bytes.len();
        let value = ((state >> 33) & 0x7f) as u8;
        match state % 3 {
            0 => bytes[at] = value,
            1 => bytes.insert(at, value),
            _ => {
                bytes.remove(at);
            }
        }
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        if i % 2 == 0 {
            match scenario::parse(&text) {
                Ok(s) => {
                    report.mutations_parsed += 1;
                    if s.validate().is_err() {
                        report
                            .failures
                            .push(format!("mutation {i} produced an unsound scenario"));
                    }
                }
                Err(e) => {
                    report.mutations_rejected += 1;
                    if e.to_string().is_empty() {
                        report
                            .failures
                            .push(format!("mutation {i} had an empty error"));
                    }
                }
            }
        } else {
            match json::parse(&text) {
                Ok(_) => report.mutations_parsed += 1,
                Err(_) => report.mutations_rejected += 1,
            }
        }
    }
}
