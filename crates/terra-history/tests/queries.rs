//! Chronicle query and export acceptance through the public history API.

use terra_core::world::{World, WorldConfig};
use terra_history::chronicle::{export, EventKind};
use terra_history::civ::sim::{simulate, HistoryConfig};

fn history() -> terra_history::civ::History {
    let world = World::generate(WorldConfig {
        seed: 61,
        subdiv: 2,
        years: 2_000,
        erosion_iterations: 3,
        ..WorldConfig::default()
    });
    simulate(
        &world,
        HistoryConfig {
            seed: 61,
            years: 2_000,
            initial_civs: 5,
            snapshot_interval: 100,
            ..HistoryConfig::default()
        },
    )
}

#[test]
fn filters_narrow_the_store_and_pages_are_consistent() {
    let h = history();
    let all = h.chronicle.query().count();
    assert!(all > 10);
    let expansions = h.chronicle.query().kind(EventKind::Expansion).count();
    assert!(expansions > 0);
    assert!(expansions <= all);
    let page = h.chronicle.query().kind(EventKind::Expansion).page(0, 5);
    assert!(page.len() <= 5);
    assert!(page.iter().all(|e| e.kind == EventKind::Expansion));

    let mid = h.chronicle.query().years(500, 1_000).count();
    assert!(mid <= all);
    let roots = h.chronicle.query().roots_only(true).count();
    assert!(roots >= 1);
    let text = h.chronicle.query().text("river").count();
    assert!(text <= all);
}

#[test]
fn civ_and_cell_filters_agree_with_helpers() {
    let h = history();
    for civ in h.civs.iter().take(4) {
        let count = h.chronicle.query().civ(civ.id).count();
        let helper = h.chronicle.events_for_civ(civ.id).len();
        assert!(count >= helper.min(count));
        if count > 0 {
            assert!(h
                .chronicle
                .events_for_civ(civ.id)
                .iter()
                .all(|e| e.civ == Some(civ.id)));
        }
    }
    if let Some(cell) = h.chronicle.events().iter().find_map(|e| e.cell) {
        assert!(h
            .chronicle
            .events_at_cell(cell)
            .iter()
            .all(|e| e.cell == Some(cell)));
    }
}

#[test]
fn chain_paths_and_explanations_are_available_for_every_event() {
    let h = history();
    for event in h.chronicle.events() {
        let chain = h.chronicle.cause_chain(event.id).unwrap();
        assert_eq!(chain.root.id, event.id);
        assert!(!chain.ids().is_empty());
        let paths = h.chronicle.trace_paths(event.id, 4);
        assert!(!paths.is_empty());
        assert!(h.chronicle.explain(event.id).is_some());
    }
    assert!(h.chronicle.cause_chain(u32::MAX).is_none());
    assert!(h.chronicle.explain(u32::MAX).is_none());
}

#[test]
fn exports_are_complete_and_deterministic() {
    let h = history();
    let md1 = export::markdown(&h.world_name, &h.chronicle);
    let md2 = export::markdown(&h.world_name, &h.chronicle);
    assert_eq!(md1, md2);
    assert!(md1.contains("# Chronicle of"));
    assert!(md1.contains("Causally sound: yes"));

    let json1 = export::json(&h.world_name, &h.chronicle);
    let json2 = export::json(&h.world_name, &h.chronicle);
    assert_eq!(json1, json2);
    assert!(json1.contains("\"entries\":["));
    assert!(json1.contains("\"sound\":true"));
    assert!(!json1.contains('\n'));

    let csv1 = export::csv(&h.world_name, &h.chronicle);
    let csv2 = export::csv(&h.world_name, &h.chronicle);
    assert_eq!(csv1, csv2);
    assert!(csv1.contains("id,year,kind"));
    assert_eq!(
        csv1.lines().filter(|l| !l.starts_with('#')).count(),
        h.chronicle.len() + 1
    );
}

#[test]
fn count_helpers_are_consistent() {
    let h = history();
    let counts = h.chronicle.counts_by_kind();
    let total: u32 = counts.values().sum();
    assert_eq!(total as usize, h.chronicle.len());
    let stats = h.chronicle.stats();
    assert_eq!(stats.events as usize, h.chronicle.len());
    assert!(stats.kinds as usize == counts.len());
    assert!(stats.peak_century_events >= 1);
    let density = h.chronicle.density_by_century();
    assert!(density.iter().sum::<u32>() as usize == total as usize);
}
