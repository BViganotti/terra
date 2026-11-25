//! Chronicle queries and causal tracing.
//!
//! Queries are read-only views over the store. They never allocate event copies
//! beyond the result vector, and every traversal has a hard node and depth
//! limit, so a pathological history cannot make the Studio unresponsive.

use std::collections::{BTreeSet, VecDeque};

use super::{Cause, Chronicle, Event, EventId, EventKind, StateFact};

/// Maximum number of results a single query returns.
pub const MAX_RESULT: usize = 500;
/// Maximum depth for cause traversal.
pub const MAX_CHAIN_DEPTH: u32 = 48;
/// Maximum nodes visited in one traversal.
pub const MAX_TRAVERSAL_NODES: usize = 5_000;

/// A compact event view used by lists and chains.
#[derive(Clone, Debug, PartialEq)]
pub struct EventSummary {
    /// Event id.
    pub id: EventId,
    /// Year.
    pub year: u32,
    /// Kind code.
    pub kind: &'static str,
    /// Title.
    pub title: String,
    /// Severity.
    pub severity: f64,
    /// Civilization, if any.
    pub civ: Option<u16>,
    /// Cell, if any.
    pub cell: Option<u32>,
}

impl EventSummary {
    /// Builds a summary from a stored event.
    pub fn from_event(e: &Event) -> Self {
        Self {
            id: e.id,
            year: e.year,
            kind: e.kind.code(),
            title: e.title.clone(),
            severity: e.severity,
            civ: e.civ,
            cell: e.cell,
        }
    }
}

/// One node in a cause chain.
#[derive(Clone, Debug, PartialEq)]
pub struct ChainNode {
    /// Distance from the queried event: 0 is the event itself.
    pub depth: u32,
    /// Which cause of the parent this node expands, for tree layouts.
    pub parent: Option<EventId>,
    /// Event data.
    pub event: EventSummary,
}

/// A state fact reached while tracing causes.
#[derive(Clone, Debug, PartialEq)]
pub struct ChainFact {
    /// Depth at which the fact appears.
    pub depth: u32,
    /// Event that cites the fact.
    pub from: EventId,
    /// The measured fact.
    pub fact: StateFact,
}

/// A trace from an event to its causes.
#[derive(Clone, Debug, PartialEq)]
pub struct CauseChain {
    /// The queried event.
    pub root: EventSummary,
    /// Events in breadth-first order.
    pub nodes: Vec<ChainNode>,
    /// State facts encountered.
    pub facts: Vec<ChainFact>,
    /// Maximum event depth reached.
    pub max_depth: u32,
    /// True when the traversal hit a limit.
    pub truncated: bool,
}

impl CauseChain {
    /// Number of distinct events in the chain.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// True when the chain has no nodes beyond the root.
    pub fn is_empty(&self) -> bool {
        self.nodes.len() <= 1
    }

    /// Event ids in breadth-first order.
    pub fn ids(&self) -> Vec<EventId> {
        self.nodes.iter().map(|n| n.event.id).collect()
    }
}

/// A step in an explicit root path.
#[derive(Clone, Debug, PartialEq)]
pub enum TraceStep {
    /// An event.
    Event(EventSummary),
    /// A measured fact.
    Fact(StateFact),
}

/// One path from an event back to state facts.
#[derive(Clone, Debug, PartialEq)]
pub struct TracePath {
    /// Steps ordered from the queried event to the root.
    pub steps: Vec<TraceStep>,
}

/// A filtered, paginated view over the chronicle.
#[derive(Clone, Debug)]
pub struct Query<'a> {
    chronicle: &'a Chronicle,
    from_year: Option<u32>,
    to_year: Option<u32>,
    kind: Option<EventKind>,
    civ: Option<u16>,
    cell: Option<u32>,
    text: Option<String>,
    roots_only: bool,
    min_severity: f64,
}

impl<'a> Query<'a> {
    /// Creates an unfiltered query.
    pub fn new(chronicle: &'a Chronicle) -> Self {
        Self {
            chronicle,
            from_year: None,
            to_year: None,
            kind: None,
            civ: None,
            cell: None,
            text: None,
            roots_only: false,
            min_severity: 0.0,
        }
    }

    /// Restricts to an inclusive year range. Reversed bounds are swapped.
    pub fn years(mut self, from: u32, to: u32) -> Self {
        let (a, b) = if from <= to { (from, to) } else { (to, from) };
        self.from_year = Some(a);
        self.to_year = Some(b);
        self
    }

    /// Restricts to one event kind.
    pub fn kind(mut self, kind: EventKind) -> Self {
        self.kind = Some(kind);
        self
    }

    /// Restricts to events involving a civilization.
    pub fn civ(mut self, civ: u16) -> Self {
        self.civ = Some(civ);
        self
    }

    /// Restricts to events at a cell.
    pub fn cell(mut self, cell: u32) -> Self {
        self.cell = Some(cell);
        self
    }

    /// Case-insensitive substring search over titles, details and tags.
    pub fn text(mut self, text: impl Into<String>) -> Self {
        let t = text.into();
        if !t.trim().is_empty() {
            self.text = Some(t.to_lowercase());
        }
        self
    }

    /// Restricts to events whose causes are all state facts.
    pub fn roots_only(mut self, yes: bool) -> Self {
        self.roots_only = yes;
        self
    }

    /// Drops events below a severity.
    pub fn min_severity(mut self, value: f64) -> Self {
        self.min_severity = if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self
    }

    fn matches(&self, e: &Event) -> bool {
        if let Some(from) = self.from_year {
            if e.year < from {
                return false;
            }
        }
        if let Some(to) = self.to_year {
            if e.year > to {
                return false;
            }
        }
        if let Some(k) = self.kind {
            if e.kind != k {
                return false;
            }
        }
        if let Some(c) = self.civ {
            if e.civ != Some(c) {
                return false;
            }
        }
        if let Some(c) = self.cell {
            if e.cell != Some(c) {
                return false;
            }
        }
        if self.roots_only && !e.is_root() {
            return false;
        }
        if e.severity < self.min_severity {
            return false;
        }
        if let Some(t) = &self.text {
            let hit = e.title.to_lowercase().contains(t)
                || e.detail.to_lowercase().contains(t)
                || e.tags.iter().any(|tag| tag.to_lowercase().contains(t));
            if !hit {
                return false;
            }
        }
        true
    }

    /// Counts matching events.
    pub fn count(&self) -> usize {
        self.chronicle
            .events()
            .iter()
            .filter(|e| self.matches(e))
            .count()
    }

    /// Returns a page of events. `limit` is clamped to [`MAX_RESULT`].
    pub fn page(&self, offset: usize, limit: usize) -> Vec<&'a Event> {
        let limit = limit.clamp(1, MAX_RESULT);
        self.chronicle
            .events()
            .iter()
            .filter(|e| self.matches(e))
            .skip(offset)
            .take(limit)
            .collect()
    }

    /// Returns up to [`MAX_RESULT`] matching events.
    pub fn all(&self) -> Vec<&'a Event> {
        self.page(0, MAX_RESULT)
    }

    /// First matching event.
    pub fn first(&self) -> Option<&'a Event> {
        self.chronicle.events().iter().find(|e| self.matches(e))
    }

    /// Last matching event.
    pub fn last(&self) -> Option<&'a Event> {
        self.chronicle
            .events()
            .iter()
            .rev()
            .find(|e| self.matches(e))
    }
}

impl Chronicle {
    /// Traces the event's causes breadth-first.
    pub fn cause_chain(&self, id: EventId) -> Option<CauseChain> {
        let root = self.get(id)?;
        let mut nodes = vec![ChainNode {
            depth: 0,
            parent: None,
            event: EventSummary::from_event(root),
        }];
        let mut facts = Vec::new();
        let mut seen: BTreeSet<EventId> = BTreeSet::new();
        seen.insert(id);
        let mut queue: VecDeque<(EventId, u32)> = VecDeque::new();
        queue.push_back((id, 0));
        let mut truncated = false;
        let mut max_depth = 0u32;
        let mut visited = 0usize;

        while let Some((current, depth)) = queue.pop_front() {
            visited += 1;
            if visited > MAX_TRAVERSAL_NODES || depth >= MAX_CHAIN_DEPTH {
                truncated = true;
                break;
            }
            let Some(event) = self.get(current) else {
                continue;
            };
            for cause in &event.causes {
                match cause {
                    Cause::State(fact) => {
                        facts.push(ChainFact {
                            depth: depth + 1,
                            from: current,
                            fact: fact.clone(),
                        });
                    }
                    Cause::Event(cid) => {
                        if !seen.insert(*cid) {
                            continue;
                        }
                        let Some(child) = self.get(*cid) else {
                            continue;
                        };
                        let d = depth + 1;
                        max_depth = max_depth.max(d);
                        nodes.push(ChainNode {
                            depth: d,
                            parent: Some(current),
                            event: EventSummary::from_event(child),
                        });
                        queue.push_back((*cid, d));
                    }
                }
            }
        }

        Some(CauseChain {
            root: EventSummary::from_event(root),
            nodes,
            facts,
            max_depth,
            truncated,
        })
    }

    /// Enumerates up to `limit` explicit paths from an event to state facts.
    pub fn trace_paths(&self, id: EventId, limit: usize) -> Vec<TracePath> {
        let limit = limit.clamp(1, 32);
        let mut out = Vec::new();
        let mut prefix = Vec::new();
        self.walk_paths(id, &mut prefix, &mut out, limit, 0);
        out
    }

    fn walk_paths(
        &self,
        id: EventId,
        prefix: &mut Vec<TraceStep>,
        out: &mut Vec<TracePath>,
        limit: usize,
        depth: u32,
    ) {
        if out.len() >= limit || depth > MAX_CHAIN_DEPTH {
            return;
        }
        let Some(event) = self.get(id) else {
            return;
        };
        prefix.push(TraceStep::Event(EventSummary::from_event(event)));
        if event.causes.is_empty() {
            out.push(TracePath {
                steps: prefix.clone(),
            });
            prefix.pop();
            return;
        }
        for cause in &event.causes {
            if out.len() >= limit {
                break;
            }
            match cause {
                Cause::State(fact) => {
                    prefix.push(TraceStep::Fact(fact.clone()));
                    out.push(TracePath {
                        steps: prefix.clone(),
                    });
                    prefix.pop();
                }
                Cause::Event(cid) => self.walk_paths(*cid, prefix, out, limit, depth + 1),
            }
        }
        prefix.pop();
    }

    /// Events that cite this event, directly or transitively, up to a limit.
    pub fn descendants(&self, id: EventId) -> Vec<EventId> {
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        seen.insert(id);
        let mut queue = VecDeque::new();
        queue.push_back(id);
        while let Some(current) = queue.pop_front() {
            for e in self.events() {
                if e.id <= current {
                    continue;
                }
                if e.cause_events().any(|c| c == current) && seen.insert(e.id) {
                    out.push(e.id);
                    queue.push_back(e.id);
                    if out.len() >= MAX_RESULT {
                        return out;
                    }
                }
            }
        }
        out
    }

    /// Builds a deterministic human-readable explanation.
    pub fn explain(&self, id: EventId) -> Option<String> {
        let event = self.get(id)?;
        let mut out = String::new();
        out.push_str(&format!(
            "{} (year {}, {}). ",
            event.title,
            event.year,
            event.kind.label()
        ));
        out.push_str(&event.detail);
        out.push(' ');
        if event.causes.is_empty() {
            out.push_str("No cause is recorded.");
            return Some(out);
        }
        out.push_str("Causes: ");
        let mut first = true;
        for cause in &event.causes {
            if !first {
                out.push_str("; ");
            }
            first = false;
            out.push_str(&cause.describe(self));
        }
        out.push('.');
        // One level of ancestry, so the explanation is useful without being a
        // full dump of the graph.
        let parents: Vec<EventId> = event.cause_events().collect();
        if !parents.is_empty() && parents.len() <= 3 {
            out.push_str(" In turn: ");
            let mut first = true;
            for p in parents {
                if let Some(pe) = self.get(p) {
                    if !first {
                        out.push_str("; ");
                    }
                    first = false;
                    out.push_str(&format!("{} (year {})", pe.title, pe.year));
                    if let Some(gp) = pe.cause_events().next() {
                        if let Some(g) = self.get(gp) {
                            out.push_str(&format!(" because {}", g.title));
                        }
                    }
                }
            }
            out.push('.');
        }
        Some(out)
    }

    /// Events at a cell, most recent first.
    pub fn events_at_cell(&self, cell: u32) -> Vec<&Event> {
        self.events()
            .iter()
            .rev()
            .filter(|e| e.cell == Some(cell))
            .take(MAX_RESULT)
            .collect()
    }

    /// Events involving a civilization, in order.
    pub fn events_for_civ(&self, civ: u16) -> Vec<&Event> {
        self.events()
            .iter()
            .filter(|e| e.civ == Some(civ))
            .take(MAX_RESULT)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chronicle::{Event, StateFact};

    fn cause(year: u32) -> Cause {
        Cause::State(StateFact::global("seed", f64::from(year), "", "seed"))
    }

    fn push(
        c: &mut Chronicle,
        year: u32,
        kind: EventKind,
        title: &str,
        causes: Vec<Cause>,
    ) -> EventId {
        c.record(Event {
            id: 0,
            year,
            kind,
            title: title.into(),
            detail: format!("detail for {title}"),
            cell: Some(1 + year / 10),
            civ: if year % 3 == 0 { Some(1) } else { None },
            causes,
            severity: 0.5,
            tags: vec!["test"],
        })
        .unwrap()
    }

    fn sample() -> Chronicle {
        let mut c = Chronicle::new();
        let root = push(&mut c, 0, EventKind::WorldFormed, "world", vec![cause(0)]);
        let a = push(
            &mut c,
            10,
            EventKind::SettlementFounded,
            "settlement",
            vec![Cause::Event(root), cause(1)],
        );
        let b = push(
            &mut c,
            20,
            EventKind::Expansion,
            "expansion",
            vec![Cause::Event(a)],
        );
        let _c = push(
            &mut c,
            30,
            EventKind::WarDeclared,
            "war",
            vec![Cause::Event(b), Cause::Event(root)],
        );
        c
    }

    #[test]
    fn filters_compose() {
        let c = sample();
        assert_eq!(c.query().count(), 4);
        assert_eq!(c.query().years(10, 20).count(), 2);
        assert_eq!(c.query().kind(EventKind::WarDeclared).count(), 1);
        assert_eq!(c.query().civ(1).count(), 2);
        assert_eq!(c.query().cell(4).count(), 1);
        assert_eq!(c.query().text("EXPANSION").count(), 1);
        assert_eq!(c.query().roots_only(true).count(), 1);
        assert_eq!(c.query().min_severity(0.6).count(), 0);
        assert!(c.query().first().is_some());
        assert!(c.query().last().is_some());
    }

    #[test]
    fn pagination_is_bounded() {
        let c = sample();
        let page = c.query().page(1, 2);
        assert_eq!(page.len(), 2);
        assert_eq!(page[0].year, 10);
        assert!(c.query().page(0, 10_000).len() <= MAX_RESULT);
        assert!(c.query().page(100, 10).is_empty());
    }

    #[test]
    fn chain_is_breadth_first_and_acyclic() {
        let c = sample();
        let chain = c.cause_chain(3).unwrap();
        assert_eq!(chain.root.id, 3);
        assert_eq!(chain.len(), 4);
        assert_eq!(chain.max_depth, 2);
        assert!(!chain.truncated);
        assert!(chain.ids().contains(&0));
        assert!(!chain.facts.is_empty());
        // Deduplication: event 0 is cited twice but appears once.
        assert_eq!(chain.ids().iter().filter(|&&id| id == 0).count(), 1);
    }

    #[test]
    fn chain_for_root_has_only_facts() {
        let c = sample();
        let chain = c.cause_chain(0).unwrap();
        assert_eq!(chain.len(), 1);
        assert_eq!(chain.max_depth, 0);
        assert!(!chain.facts.is_empty());
        assert!(chain.is_empty());
    }

    #[test]
    fn missing_event_has_no_chain() {
        let c = sample();
        assert!(c.cause_chain(99).is_none());
        assert!(c.explain(99).is_none());
    }

    #[test]
    fn paths_terminate_in_facts() {
        let c = sample();
        let paths = c.trace_paths(3, 10);
        assert!(!paths.is_empty());
        for p in &paths {
            assert!(!p.steps.is_empty());
            match p.steps.last().unwrap() {
                TraceStep::Fact(_) => {}
                TraceStep::Event(e) => assert!(e.id == 0),
            }
        }
    }

    #[test]
    fn descendants_follow_citations() {
        let c = sample();
        let d = c.descendants(0);
        assert!(d.contains(&1));
        assert!(d.contains(&2));
        assert!(d.contains(&3));
        assert!(c.descendants(3).is_empty());
        assert!(c.descendants(999).is_empty());
    }

    #[test]
    fn explain_mentions_causes() {
        let c = sample();
        let text = c.explain(3).unwrap();
        assert!(text.contains("war"));
        assert!(text.contains("Causes:"));
        assert!(text.contains("expansion"));
        assert!(text.contains("year 30"));
    }

    #[test]
    fn cell_and_civ_helpers() {
        let c = sample();
        assert_eq!(c.events_at_cell(4).len(), 1);
        assert_eq!(c.events_for_civ(1).len(), 2);
        assert!(c.events_for_civ(9).is_empty());
    }

    #[test]
    fn text_query_ignores_blank() {
        let c = sample();
        assert_eq!(c.query().text("   ").count(), 4);
    }
}
