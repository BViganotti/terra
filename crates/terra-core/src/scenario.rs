//! Scenario files.
//!
//! A scenario is a small, line-oriented text document. It is intentionally not
//! JSON: it is easy to hand-edit, easy to diff, and easy to bound. Every limit
//! is enforced before allocation:
//!
//! - at most 8 KiB of input,
//! - at most 96 lines,
//! - at most 512 bytes per line,
//! - at most 32 key/value entries,
//! - at most 80 characters in a name or note.
//!
//! Unknown keys, duplicate keys, malformed numbers and out-of-range values are
//! errors with a line number. Parsing never panics, which the fuzz harness
//! checks with thousands of mutations.

use std::fmt;

use crate::world::WorldConfig;

/// Hard input limit in bytes.
pub const MAX_INPUT_BYTES: usize = 8 * 1024;
/// Hard line limit.
pub const MAX_LINES: usize = 96;
/// Hard line length limit in bytes.
pub const MAX_LINE_BYTES: usize = 512;
/// Hard entry limit.
pub const MAX_ENTRIES: usize = 32;
/// Hard limit for name and note strings in characters.
pub const MAX_TEXT_CHARS: usize = 80;

/// A parse or validation failure with its source line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioError {
    /// One-based line number, zero when the error is not tied to a line.
    pub line: usize,
    /// What went wrong.
    pub message: String,
}

impl ScenarioError {
    fn new(line: usize, message: impl Into<String>) -> Self {
        Self {
            line,
            message: message.into(),
        }
    }
}

impl fmt::Display for ScenarioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line > 0 {
            write!(f, "line {}: {}", self.line, self.message)
        } else {
            f.write_str(&self.message)
        }
    }
}

impl std::error::Error for ScenarioError {}

/// A validated scenario.
#[derive(Clone, Debug, PartialEq)]
pub struct Scenario {
    /// Display name.
    pub name: String,
    /// Optional note.
    pub note: String,
    /// World configuration.
    pub config: WorldConfig,
}

impl Scenario {
    /// Builds a scenario from a configuration, with a generated name.
    pub fn from_config(config: WorldConfig) -> Self {
        Self {
            name: format!("seed-{}", config.seed),
            note: String::new(),
            config,
        }
    }

    /// A named preset with a seed and span.
    pub fn preset(name: &str, seed: u64, years: u32) -> Option<Self> {
        let mut config = WorldConfig {
            seed,
            years,
            ..WorldConfig::default()
        };
        let mut scenario_name = String::new();
        match name {
            "temperate" => scenario_name.push_str("Temperate"),
            "arid" => {
                scenario_name.push_str("Arid");
                config.land_fraction = 0.42;
                config.precip_target_mm = 420.0;
                config.temperature_offset_c = 4.0;
            }
            "icehouse" => {
                scenario_name.push_str("Icehouse");
                config.temperature_offset_c = -8.0;
                config.land_fraction = 0.36;
                config.precip_target_mm = 600.0;
            }
            "archipelago" => {
                scenario_name.push_str("Archipelago");
                config.land_fraction = 0.18;
                config.plates = 14;
                config.erosion_iterations = 10;
            }
            "pangaea" => {
                scenario_name.push_str("Pangaea");
                config.land_fraction = 0.48;
                config.plates = 5;
                config.erosion_iterations = 10;
            }
            _ => return None,
        }
        Some(Self {
            name: format!("{scenario_name} {seed}"),
            note: String::new(),
            config,
        })
    }

    /// Preset names in menu order.
    pub fn preset_names() -> &'static [&'static str] {
        &["temperate", "arid", "icehouse", "archipelago", "pangaea"]
    }

    /// Validates a built scenario, in case fields were changed by hand.
    pub fn validate(&self) -> Result<(), ScenarioError> {
        validate_config(&self.config)
    }

    /// Serializes the scenario in the canonical key order.
    pub fn to_text(&self) -> String {
        let c = &self.config;
        let mut out = String::new();
        out.push_str("# Terra scenario\n");
        out.push_str(&format!("name = \"{}\"\n", escape(&self.name)));
        if !self.note.is_empty() {
            out.push_str(&format!("note = \"{}\"\n", escape(&self.note)));
        }
        out.push_str(&format!("seed = {}\n", c.seed));
        out.push_str(&format!("subdiv = {}\n", c.subdiv));
        out.push_str(&format!("years = {}\n", c.years));
        out.push_str(&format!("plates = {}\n", c.plates));
        out.push_str(&format!("land = {:.3}\n", c.land_fraction));
        out.push_str(&format!("erosion = {}\n", c.erosion_iterations));
        out.push_str(&format!("temperature = {:.2}\n", c.temperature_offset_c));
        out.push_str(&format!("precip = {:.1}\n", c.precip_target_mm));
        out
    }
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// Parses a scenario.
pub fn parse(input: &str) -> Result<Scenario, ScenarioError> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(ScenarioError::new(
            0,
            format!("input exceeds {MAX_INPUT_BYTES} bytes"),
        ));
    }
    if !input.is_ascii() {
        // Non-ASCII is allowed in names and notes, but the byte limits above
        // already ran. Nothing else to do here.
    }
    let mut entries: Vec<(usize, String, String)> = Vec::new();
    let mut name = String::new();
    let mut note = String::new();
    let mut preset: Option<String> = None;

    let mut line_no = 0usize;
    for raw in input.split('\n') {
        line_no += 1;
        if line_no > MAX_LINES {
            return Err(ScenarioError::new(
                line_no,
                format!("more than {MAX_LINES} lines"),
            ));
        }
        if raw.len() > MAX_LINE_BYTES {
            return Err(ScenarioError::new(
                line_no,
                format!("line exceeds {MAX_LINE_BYTES} bytes"),
            ));
        }
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(ScenarioError::new(line_no, "expected key = value"));
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        if key.is_empty() {
            return Err(ScenarioError::new(line_no, "empty key"));
        }
        if key.len() > 24 || !key.bytes().all(|b| b.is_ascii_alphabetic() || b == b'_') {
            return Err(ScenarioError::new(line_no, "invalid key"));
        }
        let value = unquote(value).map_err(|m| ScenarioError::new(line_no, m))?;
        match key.as_str() {
            "name" => {
                if entries.iter().any(|(_, k, _)| k == "name") {
                    return Err(ScenarioError::new(line_no, "duplicate key name"));
                }
                if value.chars().count() > MAX_TEXT_CHARS {
                    return Err(ScenarioError::new(
                        line_no,
                        format!("name exceeds {MAX_TEXT_CHARS} characters"),
                    ));
                }
                name = value.to_string();
            }
            "note" => {
                if entries.iter().any(|(_, k, _)| k == "note") {
                    return Err(ScenarioError::new(line_no, "duplicate key note"));
                }
                if value.chars().count() > MAX_TEXT_CHARS {
                    return Err(ScenarioError::new(
                        line_no,
                        format!("note exceeds {MAX_TEXT_CHARS} characters"),
                    ));
                }
                note = value.to_string();
            }
            "preset" => {
                if entries.iter().any(|(_, k, _)| k == "preset") {
                    return Err(ScenarioError::new(line_no, "duplicate key preset"));
                }
                if !Scenario::preset_names().contains(&value) {
                    return Err(ScenarioError::new(line_no, "unknown preset"));
                }
                preset = Some(value.to_string());
            }
            "seed" | "subdiv" | "years" | "plates" | "land" | "erosion" | "temperature"
            | "precip" => {
                if entries.iter().any(|(_, k, _)| *k == key) {
                    return Err(ScenarioError::new(line_no, format!("duplicate key {key}")));
                }
                if entries.len() >= MAX_ENTRIES {
                    return Err(ScenarioError::new(
                        line_no,
                        format!("more than {MAX_ENTRIES} entries"),
                    ));
                }
                entries.push((line_no, key.clone(), value.to_string()));
            }
            _ => return Err(ScenarioError::new(line_no, format!("unknown key {key}"))),
        }
    }

    let mut config = if let Some(p) = preset {
        Scenario::preset(&p, 0, WorldConfig::default().years)
            .map(|s| s.config)
            .unwrap_or_default()
    } else {
        WorldConfig::default()
    };
    for (line, key, value) in entries {
        apply(&mut config, &key, &value, line)?;
    }
    if name.is_empty() {
        name = format!("seed-{}", config.seed);
    }
    let scenario = Scenario { name, note, config };
    validate_config(&scenario.config)?;
    Ok(scenario)
}

fn unquote(value: &str) -> Result<&str, String> {
    if let Some(rest) = value.strip_prefix('"') {
        let Some(inner) = rest.strip_suffix('"') else {
            return Err("unterminated string".into());
        };
        if inner.contains('"') && !inner.contains("\\\"") {
            return Err("nested quote in string".into());
        }
        return Ok(inner);
    }
    if value.contains('"') {
        return Err("misplaced quote".into());
    }
    Ok(value)
}

fn apply(
    config: &mut WorldConfig,
    key: &str,
    value: &str,
    line: usize,
) -> Result<(), ScenarioError> {
    let number = |v: &str| -> Result<f64, ScenarioError> {
        let parsed: f64 = v
            .parse()
            .map_err(|_| ScenarioError::new(line, format!("invalid number {v:?}")))?;
        if !parsed.is_finite() {
            return Err(ScenarioError::new(line, "number must be finite"));
        }
        Ok(parsed)
    };
    match key {
        "seed" => {
            let v: u64 = value
                .parse()
                .map_err(|_| ScenarioError::new(line, "seed must be an unsigned integer"))?;
            config.seed = v;
        }
        "subdiv" => {
            let v = number(value)?;
            if !(2.0..=6.0).contains(&v) || v.fract() != 0.0 {
                return Err(ScenarioError::new(line, "subdiv must be an integer 2..=6"));
            }
            config.subdiv = v as u32;
        }
        "years" => {
            let v = number(value)?;
            if !(100.0..=20_000.0).contains(&v) || v.fract() != 0.0 {
                return Err(ScenarioError::new(
                    line,
                    "years must be an integer 100..=20000",
                ));
            }
            config.years = v as u32;
        }
        "plates" => {
            let v = number(value)?;
            if !(3.0..=20.0).contains(&v) || v.fract() != 0.0 {
                return Err(ScenarioError::new(line, "plates must be an integer 3..=20"));
            }
            config.plates = v as u32;
        }
        "erosion" => {
            let v = number(value)?;
            if !(1.0..=24.0).contains(&v) || v.fract() != 0.0 {
                return Err(ScenarioError::new(
                    line,
                    "erosion must be an integer 1..=24",
                ));
            }
            config.erosion_iterations = v as u32;
        }
        "land" => {
            let v = number(value)?;
            if !(0.1..=0.75).contains(&v) {
                return Err(ScenarioError::new(line, "land must be in 0.1..=0.75"));
            }
            config.land_fraction = v;
        }
        "temperature" => {
            let v = number(value)?;
            if !(-12.0..=12.0).contains(&v) {
                return Err(ScenarioError::new(line, "temperature must be in -12..=12"));
            }
            config.temperature_offset_c = v;
        }
        "precip" => {
            let v = number(value)?;
            if !(200.0..=2_500.0).contains(&v) {
                return Err(ScenarioError::new(line, "precip must be in 200..=2500"));
            }
            config.precip_target_mm = v;
        }
        _ => return Err(ScenarioError::new(line, "unknown key")),
    }
    Ok(())
}

fn validate_config(c: &WorldConfig) -> Result<(), ScenarioError> {
    if !(2..=6).contains(&c.subdiv) {
        return Err(ScenarioError::new(0, "subdiv out of range"));
    }
    if !(100..=20_000).contains(&c.years) {
        return Err(ScenarioError::new(0, "years out of range"));
    }
    if !(3..=20).contains(&c.plates) {
        return Err(ScenarioError::new(0, "plates out of range"));
    }
    if !(1..=24).contains(&c.erosion_iterations) {
        return Err(ScenarioError::new(0, "erosion out of range"));
    }
    if !c.land_fraction.is_finite() || !(0.1..=0.75).contains(&c.land_fraction) {
        return Err(ScenarioError::new(0, "land fraction out of range"));
    }
    if !c.temperature_offset_c.is_finite() || !(-12.0..=12.0).contains(&c.temperature_offset_c) {
        return Err(ScenarioError::new(0, "temperature offset out of range"));
    }
    if !c.precip_target_mm.is_finite() || !(200.0..=2_500.0).contains(&c.precip_target_mm) {
        return Err(ScenarioError::new(0, "precipitation target out of range"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_minimal_scenario() {
        let s = parse("seed = 7\n").unwrap();
        assert_eq!(s.config.seed, 7);
        assert_eq!(s.name, "seed-7");
    }

    #[test]
    fn parses_all_keys() {
        let text = r#"
# a comment
name = "Wet World"
note = "test case"
seed = 99
subdiv = 3
years = 2500
plates = 12
land = 0.44
erosion = 9
temperature = 2.5
precip = 1200
"#;
        let s = parse(text).unwrap();
        assert_eq!(s.name, "Wet World");
        assert_eq!(s.note, "test case");
        assert_eq!(s.config.seed, 99);
        assert_eq!(s.config.subdiv, 3);
        assert_eq!(s.config.years, 2500);
        assert_eq!(s.config.plates, 12);
        assert!((s.config.land_fraction - 0.44).abs() < 1e-12);
        assert_eq!(s.config.erosion_iterations, 9);
        assert_eq!(s.config.temperature_offset_c, 2.5);
        assert_eq!(s.config.precip_target_mm, 1200.0);
    }

    #[test]
    fn presets_exist() {
        for name in Scenario::preset_names() {
            let s = Scenario::preset(name, 5, 1000).unwrap();
            assert!(s.validate().is_ok());
            assert!(s.name.contains("5"));
        }
        assert!(Scenario::preset("nope", 1, 100).is_none());
    }

    #[test]
    fn preset_can_be_overridden() {
        let s = parse("preset = arid\nprecip = 900\n").unwrap();
        assert_eq!(s.config.land_fraction, 0.42);
        assert_eq!(s.config.precip_target_mm, 900.0);
    }

    #[test]
    fn unknown_keys_fail() {
        let err = parse("seed = 1\nvolcanoes = 3\n").unwrap_err();
        assert_eq!(err.line, 2);
        assert!(err.message.contains("unknown"));
    }

    #[test]
    fn duplicates_fail() {
        let err = parse("seed = 1\nseed = 2\n").unwrap_err();
        assert!(err.message.contains("duplicate"));
    }

    #[test]
    fn malformed_values_fail() {
        for text in [
            "seed = -1\n",
            "seed = 1.5\n",
            "seed = abc\n",
            "subdiv = 9\n",
            "subdiv = 3.5\n",
            "years = 10\n",
            "plates = 0\n",
            "land = 1.5\n",
            "land = nan\n",
            "erosion = 0\n",
            "temperature = 99\n",
            "precip = 0\n",
            "= 4\n",
            "seed 4\n",
        ] {
            assert!(parse(text).is_err(), "{text:?} should fail");
        }
    }

    #[test]
    fn strings_are_quoted_and_bounded() {
        let long = "a".repeat(MAX_TEXT_CHARS + 1);
        assert!(parse(&format!("name = \"{long}\"\n")).is_err());
        assert!(parse("name = \"unterminated\n").is_err());
        assert!(parse("name = a\"b\n").is_err());
        let ok = parse(&format!("note = \"{}\"\n", "b".repeat(MAX_TEXT_CHARS))).unwrap();
        assert_eq!(ok.note.len(), MAX_TEXT_CHARS);
    }

    #[test]
    fn input_size_is_bounded() {
        let huge = format!("seed = 1\n{}", "# ignore\n".repeat(2_000));
        assert!(huge.len() > MAX_INPUT_BYTES);
        assert!(parse(&huge).is_err());
        let many_lines = "\n".repeat(MAX_LINES + 5);
        assert!(parse(&many_lines).is_err());
    }

    #[test]
    fn round_trip() {
        let mut s = Scenario::preset("archipelago", 21, 1500).unwrap();
        s.note = "islands".into();
        let text = s.to_text();
        let back = parse(&text).unwrap();
        assert_eq!(back.name, s.name);
        assert_eq!(back.note, s.note);
        assert_eq!(back.config, s.config);
    }

    #[test]
    fn non_finite_numbers_are_rejected() {
        for token in ["inf", "-inf", "NaN", "1e999"] {
            assert!(parse(&format!("land = {token}\n")).is_err(), "{token}");
        }
    }

    #[test]
    fn malformed_inputs_never_panic() {
        // A deterministic structural mutation sweep; the full campaign lives in
        // the fuzz crate but every module keeps a smoke test.
        let base = "name = \"Terra\"\nseed = 42\nsubdiv = 4\n";
        let mut seed = 0x1234_5678u64;
        for _ in 0..500 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let mut bytes = base.as_bytes().to_vec();
            let value = ((seed >> 32) & 0xff) as u8;
            let at = (seed as usize) % bytes.len().max(1) % bytes.len().max(1);
            match seed % 4 {
                0 => {
                    if !bytes.is_empty() {
                        let n = bytes.len();
                        bytes[at % n] = value;
                    }
                }
                1 => bytes.insert(at, value),
                2 => {
                    bytes.pop();
                }
                _ => bytes.extend_from_slice(&vec![value; (seed % 9) as usize]),
            }
            if let Ok(text) = String::from_utf8(bytes) {
                let _ = parse(&text);
            }
        }
    }
}
