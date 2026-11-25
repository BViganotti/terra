//! Political map ownership must agree with civilization records in every
//! snapshot, not only at the end of the run.

use terra_core::world::{World, WorldConfig};
use terra_history::civ::sim::{simulate, HistoryConfig};

fn history() -> terra_history::civ::History {
    let world = World::generate(WorldConfig {
        seed: 23,
        subdiv: 3,
        years: 2_000,
        erosion_iterations: 5,
        ..WorldConfig::default()
    });
    simulate(
        &world,
        HistoryConfig {
            seed: 23,
            years: 2_000,
            initial_civs: 5,
            snapshot_interval: 100,
            ..HistoryConfig::default()
        },
    )
}

#[test]
fn snapshots_are_sorted_and_end_at_the_run_year() {
    let h = history();
    assert!(!h.snapshots.is_empty());
    let mut last = 0;
    for snapshot in &h.snapshots {
        assert!(snapshot.year >= last);
        assert_eq!(snapshot.owner.len(), h.snapshots[0].owner.len());
        last = snapshot.year;
    }
    assert_eq!(h.snapshots.last().unwrap().year, h.config.years);
}

#[test]
fn ownership_never_references_unknown_civilizations() {
    let h = history();
    for snapshot in &h.snapshots {
        for &owner in &snapshot.owner {
            if owner != u16::MAX {
                assert!((owner as usize) < h.civs.len());
            }
        }
    }
}

#[test]
fn civ_cell_counts_match_the_owner_grid() {
    let h = history();
    for snapshot in &h.snapshots {
        for civ in &snapshot.civs {
            let counted = snapshot.owner.iter().filter(|&&o| o == civ.id).count() as u32;
            assert_eq!(
                counted, civ.cells,
                "year {} civ {} records {} cells but owns {}",
                snapshot.year, civ.id, civ.cells, counted
            );
        }
    }
}

#[test]
fn final_grid_agrees_with_live_records() {
    let h = history();
    let last = h.snapshots.last().unwrap();
    for civ in &h.civs {
        for &cell in &civ.cells {
            assert_eq!(last.owner[cell as usize], civ.id);
        }
    }
    let live_ids: std::collections::BTreeSet<u16> = last
        .owner
        .iter()
        .copied()
        .filter(|&o| o != u16::MAX)
        .collect();
    for id in live_ids {
        assert!(h.civs[id as usize].alive || !h.civs[id as usize].cells.is_empty());
    }
}

#[test]
fn owner_at_year_is_stable_and_safe() {
    let h = history();
    let mid = h.config.years / 2;
    assert!(h.snapshot_at(0).is_some());
    assert!(h.snapshot_at(mid).is_some());
    assert!(h.snapshot_at(u32::MAX).is_some());
    assert!(h.owner_at(u32::MAX, mid).is_none());
    assert!(h.snapshot_at(mid).unwrap().year <= mid);
}
