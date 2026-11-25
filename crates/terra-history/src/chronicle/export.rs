//! Chronicle exports.
//!
//! Two formats are supported: Markdown for reading and JSON for tooling. Both
//! are generated deterministically from the store. The JSON writer is small on
//! purpose and lives here so history can be exported without depending on the
//! Studio crate.

use std::fmt::Write as _;

use super::{Cause, Chronicle, Event, EventKind};

/// Escapes a string for a JSON document.
pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

fn json_number(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_string();
    }
    if v == v.trunc() && v.abs() < 1e15 {
        format!("{v:.0}")
    } else {
        format!("{v:.4}")
    }
}

/// Serializes a chronicle to a JSON document.
pub fn json(planet: &str, chronicle: &Chronicle) -> String {
    let stats = chronicle.stats();
    let mut out = String::with_capacity(chronicle.len() * 220 + 512);
    out.push('{');
    let _ = write!(
        out,
        "\"planet\":\"{}\",\"events\":{},\"firstYear\":{},\"lastYear\":{},\"sound\":{}",
        json_escape(planet),
        stats.events,
        stats.first_year,
        stats.last_year,
        chronicle.is_sound()
    );
    out.push_str(",\"counts\":{");
    let mut first = true;
    for (kind, count) in chronicle.counts_by_kind() {
        if !first {
            out.push(',');
        }
        first = false;
        let _ = write!(out, "\"{}\":{}", kind.code(), count);
    }
    out.push_str("},\"entries\":[");
    for (i, e) in chronicle.events().iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        write_event_json(&mut out, e);
    }
    out.push_str("]}");
    out
}

fn write_event_json(out: &mut String, e: &Event) {
    let _ = write!(
        out,
        "{{\"id\":{},\"year\":{},\"kind\":\"{}\",\"category\":\"{}\",\"title\":\"{}\",\"detail\":\"{}\",",
        e.id,
        e.year,
        e.kind.code(),
        e.kind.category(),
        json_escape(&e.title),
        json_escape(&e.detail)
    );
    match e.cell {
        Some(c) => {
            let _ = write!(out, "\"cell\":{c},");
        }
        None => out.push_str("\"cell\":null,"),
    }
    match e.civ {
        Some(c) => {
            let _ = write!(out, "\"civ\":{c},");
        }
        None => out.push_str("\"civ\":null,"),
    }
    let _ = write!(out, "\"severity\":{},\"tags\":[", json_number(e.severity));
    for (i, tag) in e.tags.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(out, "\"{}\"", json_escape(tag));
    }
    out.push_str("],\"causes\":[");
    for (i, cause) in e.causes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        match cause {
            Cause::Event(id) => {
                let _ = write!(out, "{{\"type\":\"event\",\"id\":{id}}}");
            }
            Cause::State(f) => {
                let _ = write!(
                    out,
                    "{{\"type\":\"state\",\"key\":\"{}\",\"value\":{},\"unit\":\"{}\",\"label\":\"{}\"",
                    json_escape(f.key),
                    json_number(f.value),
                    json_escape(f.unit),
                    json_escape(f.label)
                );
                if let Some(c) = f.cell {
                    let _ = write!(out, ",\"cell\":{c}");
                }
                if let Some(c) = f.civ {
                    let _ = write!(out, ",\"civ\":{c}");
                }
                out.push('}');
            }
        }
    }
    out.push_str("]}");
}

/// Serializes a chronicle to Markdown, grouped by century.
pub fn markdown(planet: &str, chronicle: &Chronicle) -> String {
    let stats = chronicle.stats();
    let mut out = String::with_capacity(chronicle.len() * 260 + 1024);
    let _ = writeln!(out, "# Chronicle of {planet}\n");
    let _ = writeln!(
        out,
        "{} events between year {} and {}. Causally sound: {}.\n",
        stats.events,
        stats.first_year,
        stats.last_year,
        if chronicle.is_sound() { "yes" } else { "no" }
    );
    if chronicle.is_empty() {
        out.push_str("_No events were recorded._\n");
        return out;
    }

    let mut current_century = u32::MAX;
    for e in chronicle.events() {
        let century = e.year / 100;
        if century != current_century {
            current_century = century;
            let _ = writeln!(
                out,
                "\n## Century {century} (years {}–{})\n",
                century * 100,
                century * 100 + 99
            );
        }
        let _ = writeln!(out, "### {} — {}\n", e.year, escape_markdown(&e.title));
        let _ = writeln!(out, "_{}_ · severity {:.2}\n", e.kind.label(), e.severity);
        let _ = writeln!(out, "{}\n", escape_markdown(&e.detail));
        if let Some(c) = e.cell {
            let _ = writeln!(out, "- Cell: {c}");
        }
        if let Some(c) = e.civ {
            let _ = writeln!(out, "- Civilization: {c}");
        }
        let _ = writeln!(out, "- Causes:");
        for cause in &e.causes {
            let _ = writeln!(out, "  - {}", escape_markdown(&cause.describe(chronicle)));
        }
        out.push('\n');
    }
    out
}

fn escape_markdown(s: &str) -> String {
    // Only characters that would break headings and lists need escaping; the
    // rest is left readable.
    s.replace('|', "\\|").replace('\n', " ").replace('\r', "")
}

/// A compact list of the most recent events, for status panels.
pub fn recent(chronicle: &Chronicle, count: usize) -> String {
    let count = count.clamp(1, 200);
    let mut out = String::new();
    for e in chronicle
        .events()
        .iter()
        .rev()
        .take(count)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        let _ = writeln!(
            out,
            "- {} — {} ({})",
            e.year,
            escape_markdown(&e.title),
            e.kind.code()
        );
    }
    out
}

/// Counts events of a kind for quick checks.
pub fn kind_count(chronicle: &Chronicle, kind: EventKind) -> usize {
    chronicle.events().iter().filter(|e| e.kind == kind).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chronicle::{Cause, Event, StateFact};

    fn sample() -> Chronicle {
        let mut c = Chronicle::new();
        let a = c
            .record(Event {
                id: 0,
                year: 0,
                kind: EventKind::WorldFormed,
                title: "A \"new\" world".into(),
                detail: "Line one\nline two\twith tab".into(),
                cell: Some(3),
                civ: None,
                causes: vec![Cause::State(StateFact::global("seed", 42.0, "", "seed"))],
                severity: 0.9,
                tags: vec!["origin"],
            })
            .unwrap();
        c.record(Event {
            id: 0,
            year: 120,
            kind: EventKind::SettlementFounded,
            title: "First city".into(),
            detail: "People settled by a river.".into(),
            cell: Some(4),
            civ: Some(1),
            causes: vec![
                Cause::Event(a),
                Cause::State(StateFact::physical(
                    "discharge",
                    4,
                    120.5,
                    "m3/s",
                    "river discharge",
                )),
            ],
            severity: 0.4,
            tags: vec!["city", "river"],
        })
        .unwrap();
        c
    }

    #[test]
    fn json_structure_is_balanced() {
        let c = sample();
        let json = json("Aster", &c);
        assert!(json.starts_with('{') && json.ends_with('}'));
        assert_eq!(
            json.matches('{').count(),
            json.matches('}').count(),
            "{json}"
        );
        assert_eq!(json.matches('[').count(), json.matches(']').count());
        assert!(json.contains("\"entries\":["));
        assert!(json.contains("\"kind\":\"world_formed\""));
        assert!(json.contains("\"type\":\"state\""));
        assert!(json.contains("\"type\":\"event\""));
    }

    #[test]
    fn json_escapes_control_characters() {
        let c = sample();
        let json = json("Aster", &c);
        assert!(!json.contains('\n'));
        assert!(!json.contains('\t'));
        assert!(json.contains("\\n"));
        assert!(json.contains("\\\"new\\\""));
        assert!(json.contains("\\t"));
    }

    #[test]
    fn markdown_contains_events_and_causes() {
        let c = sample();
        let md = markdown("Aster", &c);
        assert!(md.contains("# Chronicle of Aster"));
        assert!(md.contains("## Century 0"));
        assert!(md.contains("## Century 1"));
        assert!(md.contains("First city"));
        assert!(md.contains("Causes:"));
        assert!(md.contains("river discharge"));
        assert!(md.contains("seed"));
        assert!(md.contains("Causally sound: yes"));
    }

    #[test]
    fn empty_chronicle_exports_cleanly() {
        let c = Chronicle::new();
        let md = markdown("Void", &c);
        assert!(md.contains("No events"));
        let json = json("Void", &c);
        assert!(json.contains("\"events\":0"));
        assert!(json.contains("\"entries\":[]"));
        assert!(json.contains("\"sound\":true"));
    }

    #[test]
    fn escape_handles_unicode_and_controls() {
        assert_eq!(json_escape("é"), "é");
        assert_eq!(json_escape("\u{1}"), "\\u0001");
        assert_eq!(json_escape("a\\b"), "a\\\\b");
    }

    #[test]
    fn recent_and_counts() {
        let c = sample();
        let text = recent(&c, 1);
        assert!(text.contains("First city"));
        assert!(!text.contains("new world"));
        assert_eq!(kind_count(&c, EventKind::SettlementFounded), 1);
        assert_eq!(kind_count(&c, EventKind::Battle), 0);
    }
}
