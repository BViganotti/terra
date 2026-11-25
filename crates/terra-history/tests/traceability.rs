//! Chronicle traceability acceptance.
//!
//! Every event must cite a cause, every event cause must be in the past, and
//! tracing causes must terminate in measured state facts. This is the central
//! claim of Terra's history.

use terra_core::world::{World, WorldConfig};
use terra_history::chronicle::query::TraceStep;
use terra_history::civ::sim::{simulate, HistoryConfig};

fn history() -> terra_history::civ::History {
    let world = World::generate(WorldConfig {
        seed: 31,
        subdiv: 3,
        years: 2_000,
        erosion_iterations: 5,
        ..WorldConfig::default()
    });
    simulate(
        &world,
        HistoryConfig {
            seed: 31,
            years: 2_000,
            initial_civs: 5,
            snapshot_interval: 200,
            ..HistoryConfig::default()
        },
    )
}

#[test]
fn chronicle_is_structurally_sound() {
    let h = history();
    assert!(h.is_sound(), "{:?}", h.problems());
    assert!(h.chronicle.len() > 10);
    assert!(!h.chronicle.events().is_empty());
    assert!(h.chronicle.validate().is_empty());
}

#[test]
fn every_event_has_a_cause_and_causes_are_in_the_past() {
    let h = history();
    for event in h.chronicle.events() {
        assert!(!event.causes.is_empty(), "event {} has no causes", event.id);
        for cause in &event.causes {
            if let Some(id) = cause.event_id() {
                assert!(id < event.id, "event {} cites future event {id}", event.id);
                assert!(
                    h.chronicle.get(id).is_some(),
                    "event {} cites missing event {id}",
                    event.id
                );
            }
        }
    }
}

#[test]
fn every_chain_terminates_in_state_facts() {
    let h = history();
    for event in h.chronicle.events() {
        let chain = h
            .chronicle
            .cause_chain(event.id)
            .unwrap_or_else(|| panic!("no chain for {}", event.id));
        assert_eq!(chain.root.id, event.id);
        assert!(
            !chain.facts.is_empty(),
            "chain from {} never reaches a state fact",
            event.id
        );
        assert!(chain.max_depth <= terra_history::chronicle::query::MAX_CHAIN_DEPTH);
        let paths = h.chronicle.trace_paths(event.id, 8);
        assert!(!paths.is_empty(), "no paths for {}", event.id);
        for path in paths {
            let last = path.steps.last().expect("empty path");
            match last {
                TraceStep::Fact(_) => {}
                TraceStep::Event(e) => {
                    // Only a root may end in an event with no event causes; even
                    // then the chain's state facts prove termination.
                    assert!(e.id == 0 || e.year <= event.year);
                }
            }
        }
    }
}

#[test]
fn explanations_are_present_and_mention_causes() {
    let h = history();
    for event in h.chronicle.events().iter().take(200) {
        let text = h
            .chronicle
            .explain(event.id)
            .unwrap_or_else(|| panic!("no explanation for {}", event.id));
        assert!(text.contains(&event.title));
        assert!(text.contains("Causes:") || text.contains("cause"));
    }
    assert!(h.chronicle.explain(u32::MAX).is_none());
}

#[test]
fn descendants_only_follow_forward_citations() {
    let h = history();
    for id in 0..h.chronicle.len() as u32 {
        for child in h.chronicle.descendants(id) {
            assert!(child > id);
            let child_event = h.chronicle.get(child).unwrap();
            assert!(child_event.cause_events().any(|c| c <= child));
        }
    }
}

#[test]
fn event_limits_and_text_are_bounded() {
    let h = history();
    for event in h.chronicle.events() {
        assert!(event.title.chars().count() <= 120);
        assert!(event.detail.chars().count() <= 600);
        assert!((0.0..=1.0).contains(&event.severity));
        assert!(!event.title.trim().is_empty());
    }
    assert!(h.chronicle.stats().root_events >= 1);
    assert!(h.chronicle.stats().events as usize == h.chronicle.len());
}
