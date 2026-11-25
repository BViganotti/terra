//! The chronicle: an append-only, causally linked history.
//!
//! Every event carries at least one cause. A cause is either a measured fact
//! about the simulation state (elevation, rainfall, population, cohesion) or an
//! earlier event. Event ids are dense and increasing, and a cause must refer to
//! an event that already exists, so the cause graph is acyclic by construction.
//!
//! The store never mutates an event after it is recorded. Queries are pure
//! reads. This is what makes a run auditable: given a seed and a scenario, the
//! same events appear with the same causes and the same ids.

use std::collections::BTreeMap;
use std::fmt;

use crate::chronicle::query::Query;

pub mod export;
pub mod query;

/// Maximum number of events retained in one chronicle.
pub const MAX_EVENTS: usize = 200_000;
/// Maximum causes attached to a single event.
pub const MAX_CAUSES: usize = 12;
/// Maximum length of a title in characters.
pub const MAX_TITLE_CHARS: usize = 120;
/// Maximum length of a detail in characters.
pub const MAX_DETAIL_CHARS: usize = 600;

/// Event identifier, equal to the event's index in the store.
pub type EventId = u32;

/// What kind of thing happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EventKind {
    /// The planet itself came into being.
    WorldFormed,
    /// Plate motion organized into a new configuration.
    PlateShift,
    /// A major eruption.
    Eruption,
    /// A drought.
    Drought,
    /// A sea-level change.
    SeaLevelChange,
    /// A settlement was founded.
    SettlementFounded,
    /// A civilization claimed new land.
    Expansion,
    /// A resource was discovered.
    ResourceDiscovered,
    /// A trade route opened.
    TradeRouteOpened,
    /// Two civilizations went to war.
    WarDeclared,
    /// A battle was fought.
    Battle,
    /// A city changed hands.
    CityCaptured,
    /// A peace treaty was signed.
    PeaceTreaty,
    /// A technology was discovered.
    TechDiscovered,
    /// A harvest failed.
    Famine,
    /// A disease swept through a civilization.
    Epidemic,
    /// A civilization collapsed.
    Collapse,
    /// People migrated.
    Migration,
    /// A period of prosperity.
    GoldenAge,
    /// A species or lineage disappeared.
    Extinction,
}

impl EventKind {
    /// Every kind, in display order.
    pub const ALL: [EventKind; 20] = [
        EventKind::WorldFormed,
        EventKind::PlateShift,
        EventKind::Eruption,
        EventKind::Drought,
        EventKind::SeaLevelChange,
        EventKind::SettlementFounded,
        EventKind::Expansion,
        EventKind::ResourceDiscovered,
        EventKind::TradeRouteOpened,
        EventKind::WarDeclared,
        EventKind::Battle,
        EventKind::CityCaptured,
        EventKind::PeaceTreaty,
        EventKind::TechDiscovered,
        EventKind::Famine,
        EventKind::Epidemic,
        EventKind::Collapse,
        EventKind::Migration,
        EventKind::GoldenAge,
        EventKind::Extinction,
    ];

    /// Stable identifier for JSON and filters.
    pub fn code(self) -> &'static str {
        match self {
            EventKind::WorldFormed => "world_formed",
            EventKind::PlateShift => "plate_shift",
            EventKind::Eruption => "eruption",
            EventKind::Drought => "drought",
            EventKind::SeaLevelChange => "sea_level_change",
            EventKind::SettlementFounded => "settlement_founded",
            EventKind::Expansion => "expansion",
            EventKind::ResourceDiscovered => "resource_discovered",
            EventKind::TradeRouteOpened => "trade_route_opened",
            EventKind::WarDeclared => "war_declared",
            EventKind::Battle => "battle",
            EventKind::CityCaptured => "city_captured",
            EventKind::PeaceTreaty => "peace_treaty",
            EventKind::TechDiscovered => "tech_discovered",
            EventKind::Famine => "famine",
            EventKind::Epidemic => "epidemic",
            EventKind::Collapse => "collapse",
            EventKind::Migration => "migration",
            EventKind::GoldenAge => "golden_age",
            EventKind::Extinction => "extinction",
        }
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            EventKind::WorldFormed => "World formed",
            EventKind::PlateShift => "Plate shift",
            EventKind::Eruption => "Eruption",
            EventKind::Drought => "Drought",
            EventKind::SeaLevelChange => "Sea level change",
            EventKind::SettlementFounded => "Settlement founded",
            EventKind::Expansion => "Expansion",
            EventKind::ResourceDiscovered => "Resource discovered",
            EventKind::TradeRouteOpened => "Trade route opened",
            EventKind::WarDeclared => "War declared",
            EventKind::Battle => "Battle",
            EventKind::CityCaptured => "City captured",
            EventKind::PeaceTreaty => "Peace treaty",
            EventKind::TechDiscovered => "Technology discovered",
            EventKind::Famine => "Famine",
            EventKind::Epidemic => "Epidemic",
            EventKind::Collapse => "Collapse",
            EventKind::Migration => "Migration",
            EventKind::GoldenAge => "Golden age",
            EventKind::Extinction => "Extinction",
        }
    }

    /// Coarse category used for timeline colors: `nature`, `politics`,
    /// `economy`, `culture`, `disaster`.
    pub fn category(self) -> &'static str {
        match self {
            EventKind::WorldFormed | EventKind::PlateShift | EventKind::SeaLevelChange => "nature",
            EventKind::Eruption | EventKind::Drought | EventKind::Famine | EventKind::Epidemic => {
                "disaster"
            }
            EventKind::SettlementFounded
            | EventKind::Expansion
            | EventKind::Collapse
            | EventKind::Migration => "society",
            EventKind::ResourceDiscovered | EventKind::TradeRouteOpened => "economy",
            EventKind::WarDeclared
            | EventKind::Battle
            | EventKind::CityCaptured
            | EventKind::PeaceTreaty => "conflict",
            EventKind::TechDiscovered | EventKind::GoldenAge => "culture",
            EventKind::Extinction => "nature",
        }
    }

    /// Palette color as packed RGB.
    pub fn rgb(self) -> u32 {
        match self.category() {
            "nature" => 0x7cc4ff,
            "disaster" => 0xf07a5b,
            "society" => 0x4fd1a5,
            "economy" => 0xe8c76a,
            "conflict" => 0xd96a8a,
            _ => 0xb8a0ff,
        }
    }

    /// Looks up a kind by its stable code.
    pub fn from_code(code: &str) -> Option<EventKind> {
        EventKind::ALL.iter().copied().find(|k| k.code() == code)
    }
}

/// A measured fact about the world or a society.
#[derive(Clone, Debug, PartialEq)]
pub struct StateFact {
    /// Stable key, e.g. `elevation` or `cohesion`.
    pub key: &'static str,
    /// Cell the fact refers to, when local.
    pub cell: Option<u32>,
    /// Civilization the fact refers to, when social.
    pub civ: Option<u16>,
    /// Measured value.
    pub value: f64,
    /// Unit symbol.
    pub unit: &'static str,
    /// Human-readable description used by the explanation panel.
    pub label: &'static str,
}

impl StateFact {
    /// A physical fact at a cell.
    pub fn physical(
        key: &'static str,
        cell: u32,
        value: f64,
        unit: &'static str,
        label: &'static str,
    ) -> Self {
        Self {
            key,
            cell: Some(cell),
            civ: None,
            value: if value.is_finite() { value } else { 0.0 },
            unit,
            label,
        }
    }

    /// A social fact about a civilization.
    pub fn social(
        key: &'static str,
        civ: u16,
        value: f64,
        unit: &'static str,
        label: &'static str,
    ) -> Self {
        Self {
            key,
            cell: None,
            civ: Some(civ),
            value: if value.is_finite() { value } else { 0.0 },
            unit,
            label,
        }
    }

    /// A global fact not tied to a cell or civilization.
    pub fn global(key: &'static str, value: f64, unit: &'static str, label: &'static str) -> Self {
        Self {
            key,
            cell: None,
            civ: None,
            value: if value.is_finite() { value } else { 0.0 },
            unit,
            label,
        }
    }

    /// Formatted value with a fixed number of decimals, chosen from magnitude.
    pub fn value_text(&self) -> String {
        let a = self.value.abs();
        if a >= 1_000_000.0 {
            format!("{:.0}", self.value)
        } else if a >= 1_000.0 {
            format!("{:.0}", self.value)
        } else if a >= 10.0 {
            format!("{:.1}", self.value)
        } else {
            format!("{:.2}", self.value)
        }
    }

    /// Rendering used by explanations and exports.
    pub fn describe(&self) -> String {
        let mut s = format!("{} = {} {}", self.label, self.value_text(), self.unit);
        if let Some(c) = self.cell {
            s.push_str(&format!(" at cell {c}"));
        }
        if let Some(c) = self.civ {
            s.push_str(&format!(" for civilization {c}"));
        }
        s
    }
}

/// Why an event happened.
#[derive(Clone, Debug, PartialEq)]
pub enum Cause {
    /// A measured fact.
    State(StateFact),
    /// An earlier event.
    Event(EventId),
}

impl Cause {
    /// The event id, when this is an event cause.
    pub fn event_id(&self) -> Option<EventId> {
        match self {
            Cause::Event(id) => Some(*id),
            Cause::State(_) => None,
        }
    }

    /// Short description used in the cause panel.
    pub fn describe(&self, chronicle: &Chronicle) -> String {
        match self {
            Cause::Event(id) => chronicle
                .get(*id)
                .map(|e| format!("year {}: {}", e.year, e.title))
                .unwrap_or_else(|| format!("missing event {id}")),
            Cause::State(fact) => fact.describe(),
        }
    }

    /// Stable short code for JSON.
    pub fn code(&self) -> &'static str {
        match self {
            Cause::Event(_) => "event",
            Cause::State(_) => "state",
        }
    }
}

/// One chronicle event.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    /// Dense id.
    pub id: EventId,
    /// Year of occurrence, monotonic with id.
    pub year: u32,
    /// Kind.
    pub kind: EventKind,
    /// Short headline.
    pub title: String,
    /// Longer description.
    pub detail: String,
    /// Cell where it happened, when local.
    pub cell: Option<u32>,
    /// Civilization involved, when social.
    pub civ: Option<u16>,
    /// Causes, at least one.
    pub causes: Vec<Cause>,
    /// Severity in `[0, 1]`.
    pub severity: f64,
    /// Extra searchable tags.
    pub tags: Vec<&'static str>,
}

impl Event {
    /// True when every cause is a state fact.
    pub fn is_root(&self) -> bool {
        self.causes.iter().all(|c| matches!(c, Cause::State(_)))
    }

    /// Event causes only.
    pub fn cause_events(&self) -> impl Iterator<Item = EventId> + '_ {
        self.causes.iter().filter_map(Cause::event_id)
    }

    /// State causes only.
    pub fn cause_facts(&self) -> impl Iterator<Item = &StateFact> + '_ {
        self.causes.iter().filter_map(|c| match c {
            Cause::State(f) => Some(f),
            Cause::Event(_) => None,
        })
    }

    /// One-line rendering for lists.
    pub fn line(&self) -> String {
        format!("[{}] {}", self.year, self.title)
    }

    /// Rough measure of causal depth: zero for state-only events.
    pub fn direct_depth(&self) -> u32 {
        if self.is_root() {
            0
        } else {
            1
        }
    }
}

/// A failure while recording an event.
#[derive(Clone, Debug, PartialEq)]
pub enum ChronicleError {
    /// The chronicle is full.
    Full,
    /// An event needs at least one cause.
    MissingCause,
    /// Too many causes.
    TooManyCauses(usize),
    /// A cause refers to an event that does not exist yet.
    UnknownCause(EventId),
    /// A cause refers to this event or a later one.
    ForwardCause(EventId),
    /// Years must not move backwards.
    TimeReversed(u32, u32),
    /// Title is too long.
    TitleTooLong(usize),
    /// Detail is too long.
    DetailTooLong(usize),
    /// Severity is not finite or outside `[0, 1]`.
    BadSeverity(f64),
    /// A title or detail is empty.
    EmptyText,
}

impl fmt::Display for ChronicleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChronicleError::Full => write!(f, "chronicle is full"),
            ChronicleError::MissingCause => write!(f, "event needs at least one cause"),
            ChronicleError::TooManyCauses(n) => write!(f, "too many causes: {n}"),
            ChronicleError::UnknownCause(id) => write!(f, "unknown cause event {id}"),
            ChronicleError::ForwardCause(id) => write!(f, "cause {id} is not in the past"),
            ChronicleError::TimeReversed(a, b) => write!(f, "year {b} precedes {a}"),
            ChronicleError::TitleTooLong(n) => write!(f, "title has {n} characters"),
            ChronicleError::DetailTooLong(n) => write!(f, "detail has {n} characters"),
            ChronicleError::BadSeverity(v) => write!(f, "bad severity {v}"),
            ChronicleError::EmptyText => write!(f, "empty title or detail"),
        }
    }
}

impl std::error::Error for ChronicleError {}

/// Apparent brightness of an event for timeline density, in `[0, 1]`.
fn clamp_severity(v: f64) -> f64 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Append-only event store.
#[derive(Clone, Debug, Default)]
pub struct Chronicle {
    events: Vec<Event>,
    last_year: u32,
}

impl Chronicle {
    /// Creates an empty chronicle.
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            last_year: 0,
        }
    }

    /// Number of events.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// True when no events have been recorded.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// All events in id order.
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// Fetches an event by id.
    pub fn get(&self, id: EventId) -> Option<&Event> {
        self.events.get(id as usize)
    }

    /// The most recent year recorded.
    pub fn last_year(&self) -> u32 {
        self.last_year
    }

    /// Records a validated event and returns its id.
    ///
    /// This is the only mutating operation. It enforces the invariants that
    /// make the chronicle traceable: at least one cause, causes must already
    /// exist, and time never runs backwards.
    pub fn record(&mut self, mut event: Event) -> Result<EventId, ChronicleError> {
        if self.events.len() >= MAX_EVENTS {
            return Err(ChronicleError::Full);
        }
        if event.causes.is_empty() {
            return Err(ChronicleError::MissingCause);
        }
        if event.causes.len() > MAX_CAUSES {
            return Err(ChronicleError::TooManyCauses(event.causes.len()));
        }
        // A cause may be cited twice by different rules (for example the war
        // declaration and the latest battle can be the same event). Keep the
        // first occurrence of each event cause so validation stays meaningful.
        let mut seen_events = std::collections::BTreeSet::new();
        event.causes.retain(|c| match c {
            Cause::Event(id) => seen_events.insert(*id),
            Cause::State(_) => true,
        });
        if event.year < self.last_year {
            return Err(ChronicleError::TimeReversed(self.last_year, event.year));
        }
        if event.title.trim().is_empty() || event.detail.trim().is_empty() {
            return Err(ChronicleError::EmptyText);
        }
        if event.title.chars().count() > MAX_TITLE_CHARS {
            return Err(ChronicleError::TitleTooLong(event.title.chars().count()));
        }
        if event.detail.chars().count() > MAX_DETAIL_CHARS {
            return Err(ChronicleError::DetailTooLong(event.detail.chars().count()));
        }
        if !event.severity.is_finite() || !(0.0..=1.0).contains(&event.severity) {
            return Err(ChronicleError::BadSeverity(event.severity));
        }
        let id = self.events.len() as EventId;
        for cause in &event.causes {
            if let Cause::Event(c) = cause {
                if *c >= id {
                    return Err(if *c == id {
                        ChronicleError::ForwardCause(*c)
                    } else {
                        ChronicleError::UnknownCause(*c)
                    });
                }
            }
        }
        event.id = id;
        event.severity = clamp_severity(event.severity);
        self.last_year = event.year;
        self.events.push(event);
        Ok(id)
    }

    /// Convenience constructor for an event with a single cause.
    pub fn record_simple(
        &mut self,
        year: u32,
        kind: EventKind,
        title: impl Into<String>,
        detail: impl Into<String>,
        cause: Cause,
        severity: f64,
    ) -> Result<EventId, ChronicleError> {
        self.record(Event {
            id: 0,
            year,
            kind,
            title: title.into(),
            detail: detail.into(),
            cell: None,
            civ: None,
            causes: vec![cause],
            severity,
            tags: Vec::new(),
        })
    }

    /// Builds a query handle over the store.
    pub fn query(&self) -> Query<'_> {
        Query::new(self)
    }

    /// Counts events per kind.
    pub fn counts_by_kind(&self) -> BTreeMap<EventKind, u32> {
        let mut map = BTreeMap::new();
        for e in &self.events {
            *map.entry(e.kind).or_insert(0) += 1;
        }
        map
    }

    /// Events per century, indexed by `year / 100`.
    pub fn density_by_century(&self) -> Vec<u32> {
        if self.events.is_empty() {
            return Vec::new();
        }
        let buckets = (self.last_year / 100) as usize + 1;
        let mut out = vec![0u32; buckets.min(512)];
        for e in &self.events {
            let i = (e.year / 100) as usize;
            if i < out.len() {
                out[i] += 1;
            }
        }
        out
    }

    /// Descriptive statistics used by reports and the timeline.
    pub fn stats(&self) -> ChronicleStats {
        let mut stats = ChronicleStats {
            events: self.events.len() as u32,
            ..ChronicleStats::default()
        };
        if self.events.is_empty() {
            return stats;
        }
        stats.first_year = self.events[0].year;
        stats.last_year = self.events[self.events.len() - 1].year;
        let mut severity = 0.0;
        let mut roots = 0u32;
        let mut civ_events = 0u32;
        for e in &self.events {
            severity += e.severity;
            if e.is_root() {
                roots += 1;
            }
            if e.civ.is_some() {
                civ_events += 1;
            }
        }
        stats.mean_severity = severity / self.events.len() as f64;
        stats.root_events = roots;
        stats.civ_events = civ_events;
        stats.kinds = self.counts_by_kind().len() as u32;
        stats.peak_century_events = self.density_by_century().into_iter().max().unwrap_or(0);
        stats
    }

    /// Full structural validation, returning one message per problem found.
    ///
    /// A healthy chronicle returns an empty vector.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let mut last_year = 0u32;
        for (i, e) in self.events.iter().enumerate() {
            if e.id as usize != i {
                problems.push(format!("event {i} has id {}", e.id));
            }
            if e.year < last_year {
                problems.push(format!("event {i} goes back in time"));
            }
            last_year = e.year;
            if e.causes.is_empty() {
                problems.push(format!("event {i} has no cause"));
            }
            let mut seen = std::collections::BTreeSet::new();
            for cause in &e.causes {
                if let Cause::Event(c) = cause {
                    if *c as usize >= i {
                        problems.push(format!("event {i} cites future event {c}"));
                    }
                    if !seen.insert(*c) {
                        problems.push(format!("event {i} repeats cause {c}"));
                    }
                }
            }
            if e.title.trim().is_empty() {
                problems.push(format!("event {i} has an empty title"));
            }
            if !(0.0..=1.0).contains(&e.severity) || !e.severity.is_finite() {
                problems.push(format!("event {i} has bad severity"));
            }
        }
        problems
    }

    /// True when the chronicle passes validation.
    pub fn is_sound(&self) -> bool {
        self.validate().is_empty()
    }
}

/// Summary statistics for a chronicle.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChronicleStats {
    /// Number of events.
    pub events: u32,
    /// First year with an event.
    pub first_year: u32,
    /// Last year with an event.
    pub last_year: u32,
    /// Mean severity.
    pub mean_severity: f64,
    /// Events with only state causes.
    pub root_events: u32,
    /// Events involving a civilization.
    pub civ_events: u32,
    /// Distinct event kinds present.
    pub kinds: u32,
    /// Maximum event count in a single century.
    pub peak_century_events: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fact() -> Cause {
        Cause::State(StateFact::global("seed", 42.0, "", "world seed"))
    }

    fn event(year: u32, kind: EventKind, cause: Cause) -> Event {
        Event {
            id: 0,
            year,
            kind,
            title: format!("{} event", kind.label()),
            detail: "Something happened for a documented reason.".into(),
            cell: None,
            civ: None,
            causes: vec![cause],
            severity: 0.5,
            tags: Vec::new(),
        }
    }

    #[test]
    fn recording_assigns_dense_ids() {
        let mut c = Chronicle::new();
        let a = c.record(event(0, EventKind::WorldFormed, fact())).unwrap();
        let b = c
            .record(event(10, EventKind::SettlementFounded, Cause::Event(a)))
            .unwrap();
        assert_eq!(a, 0);
        assert_eq!(b, 1);
        assert_eq!(c.len(), 2);
        assert_eq!(c.get(0).unwrap().year, 0);
        assert_eq!(c.last_year(), 10);
        assert!(c.is_sound());
    }

    #[test]
    fn causes_are_mandatory() {
        let mut c = Chronicle::new();
        let mut e = event(0, EventKind::WorldFormed, fact());
        e.causes.clear();
        assert_eq!(c.record(e), Err(ChronicleError::MissingCause));
        assert!(c.is_empty());
    }

    #[test]
    fn forward_causes_are_rejected() {
        let mut c = Chronicle::new();
        let a = c.record(event(0, EventKind::WorldFormed, fact())).unwrap();
        let mut e = event(5, EventKind::Battle, Cause::Event(a + 1));
        e.causes = vec![Cause::Event(a + 1)];
        assert!(matches!(
            c.record(e),
            Err(ChronicleError::UnknownCause(_)) | Err(ChronicleError::ForwardCause(_))
        ));
    }

    #[test]
    fn time_cannot_run_backwards() {
        let mut c = Chronicle::new();
        let a = c
            .record(event(100, EventKind::WorldFormed, fact()))
            .unwrap();
        let err = c
            .record(event(99, EventKind::Expansion, Cause::Event(a)))
            .unwrap_err();
        assert!(matches!(err, ChronicleError::TimeReversed(100, 99)));
    }

    #[test]
    fn text_limits_are_enforced() {
        let mut c = Chronicle::new();
        let mut e = event(0, EventKind::WorldFormed, fact());
        e.title = "x".repeat(MAX_TITLE_CHARS + 1);
        assert!(matches!(c.record(e), Err(ChronicleError::TitleTooLong(_))));

        let mut e = event(0, EventKind::WorldFormed, fact());
        e.detail = "x".repeat(MAX_DETAIL_CHARS + 1);
        assert!(matches!(c.record(e), Err(ChronicleError::DetailTooLong(_))));

        let mut e = event(0, EventKind::WorldFormed, fact());
        e.title = "  ".into();
        assert_eq!(c.record(e), Err(ChronicleError::EmptyText));

        let mut e = event(0, EventKind::WorldFormed, fact());
        e.severity = f64::NAN;
        assert!(matches!(c.record(e), Err(ChronicleError::BadSeverity(_))));
    }

    #[test]
    fn too_many_causes_are_rejected() {
        let mut c = Chronicle::new();
        let mut e = event(0, EventKind::WorldFormed, fact());
        e.causes = (0..=MAX_CAUSES).map(|_| fact()).collect();
        assert!(matches!(c.record(e), Err(ChronicleError::TooManyCauses(_))));
    }

    #[test]
    fn duplicate_event_causes_are_collapsed() {
        let mut c = Chronicle::new();
        let a = c.record(event(0, EventKind::WorldFormed, fact())).unwrap();
        let mut e = event(1, EventKind::Expansion, Cause::Event(a));
        e.causes = vec![Cause::Event(a), Cause::Event(a)];
        c.record(e).unwrap();
        assert_eq!(c.get(1).unwrap().causes.len(), 1);
        assert!(c.is_sound());
    }

    #[test]
    fn kind_metadata_is_complete() {
        for k in EventKind::ALL {
            assert_eq!(EventKind::from_code(k.code()), Some(k));
            assert!(!k.label().is_empty());
            assert!(!k.category().is_empty());
            assert!((0..=0xffffff).contains(&k.rgb()));
        }
        assert_eq!(EventKind::from_code("nope"), None);
    }

    #[test]
    fn counts_and_density() {
        let mut c = Chronicle::new();
        let a = c.record(event(0, EventKind::WorldFormed, fact())).unwrap();
        c.record(event(50, EventKind::Drought, Cause::Event(a)))
            .unwrap();
        c.record(event(60, EventKind::Drought, Cause::Event(a)))
            .unwrap();
        c.record(event(150, EventKind::Battle, Cause::Event(a)))
            .unwrap();
        let counts = c.counts_by_kind();
        assert_eq!(counts.get(&EventKind::Drought), Some(&2));
        assert_eq!(counts.get(&EventKind::Battle), Some(&1));
        let density = c.density_by_century();
        assert_eq!(density.len(), 2);
        assert_eq!(density[0], 3);
        assert_eq!(density[1], 1);
    }

    #[test]
    fn stats_are_consistent() {
        let mut c = Chronicle::new();
        let a = c.record(event(0, EventKind::WorldFormed, fact())).unwrap();
        c.record(event(100, EventKind::Collapse, Cause::Event(a)))
            .unwrap();
        let s = c.stats();
        assert_eq!(s.events, 2);
        assert_eq!(s.first_year, 0);
        assert_eq!(s.last_year, 100);
        assert_eq!(s.root_events, 1);
        assert_eq!(s.kinds, 2);
        assert!(s.mean_severity > 0.0);
    }

    #[test]
    fn state_fact_formatting() {
        let f = StateFact::physical("elevation", 100, 1234.5, "m", "elevation");
        assert!(f.describe().contains("elevation"));
        assert!(f.describe().contains("cell 100"));
        assert_eq!(f.value_text(), "1234");
        let neg = StateFact::social("cohesion", 2, -0.5, "", "cohesion");
        assert!(neg.value_text().contains('-'));
        let nan = StateFact::global("x", f64::NAN, "", "x");
        assert_eq!(nan.value, 0.0);
    }

    #[test]
    fn empty_chronicle_is_sound() {
        let c = Chronicle::new();
        assert!(c.is_empty());
        assert!(c.is_sound());
        assert_eq!(c.stats(), ChronicleStats::default());
        assert!(c.density_by_century().is_empty());
    }
}
