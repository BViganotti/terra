//! Tectonic acceptance tests: area, subduction, boundary classification and
//! the relationship between convergence and relief.

mod common;

use terra_core::tectonics::BoundaryKind;

#[test]
fn plate_areas_cover_the_sphere_within_a_tenth_of_a_percent() {
    for seed in 0..6u64 {
        let w = common::world(seed, 3);
        let expected = 4.0 * std::f64::consts::PI;
        let total = w.tectonics.total_area();
        assert!(
            common::rel_err(total, expected) < 1e-3,
            "seed {seed}: area {total} expected {expected}"
        );
        let share: f64 = w.tectonics.plates.iter().map(|p| p.area_share).sum();
        assert!((share - 1.0).abs() < 1e-9, "seed {seed} share {share}");
        let counted: u32 = w.tectonics.plates.iter().map(|p| p.cell_count).sum();
        assert_eq!(counted as usize, w.len(), "seed {seed} partition");
        for p in &w.tectonics.plates {
            assert!(p.area > 0.0 && p.cell_count > 0);
            assert_eq!(
                p.kind.code(),
                match p.kind {
                    terra_core::tectonics::PlateKind::Oceanic => "oceanic",
                    terra_core::tectonics::PlateKind::Continental => "continental",
                    terra_core::tectonics::PlateKind::Mixed => "mixed",
                }
            );
        }
    }
}

#[test]
fn plates_are_contiguous_regions() {
    let w = common::world(13, 3);
    let plate_of = &w.tectonics.plate_of;
    for plate in 0..w.tectonics.plates.len() as u16 {
        let cells: Vec<u32> = (0..plate_of.len() as u32)
            .filter(|&c| plate_of[c as usize] == plate)
            .collect();
        assert!(!cells.is_empty());
        // BFS from the first cell must reach every other cell of the plate.
        let mut seen = vec![false; w.len()];
        let mut stack = vec![cells[0]];
        seen[cells[0] as usize] = true;
        while let Some(c) = stack.pop() {
            for &nb in w.sphere.neighbors(c) {
                if !seen[nb as usize] && plate_of[nb as usize] == plate {
                    seen[nb as usize] = true;
                    stack.push(nb);
                }
            }
        }
        for c in cells {
            assert!(seen[c as usize], "plate {plate} is not contiguous at {c}");
        }
    }
}

#[test]
fn relief_concentrates_at_convergent_margins() {
    let mut checked = 0;
    for seed in 0..5u64 {
        let w = common::world(seed, 3);
        let tectonic = &w.tectonics;
        let convergent: Vec<u32> = tectonic
            .boundaries_of(BoundaryKind::Convergent)
            .flat_map(|b| [b.a, b.b])
            .collect();
        if convergent.is_empty() {
            continue;
        }
        checked += 1;
        let margin = common::mean(
            &convergent
                .iter()
                .map(|&c| tectonic.uplift[c as usize])
                .collect::<Vec<_>>(),
        );
        let quiet: Vec<f64> = (0..w.len())
            .filter(|&i| tectonic.stress[i] == terra_core::tectonics::Stress::Quiet)
            .map(|i| tectonic.uplift[i])
            .collect();
        let interior = common::mean(&quiet);
        assert!(
            margin > interior * 2.0,
            "seed {seed}: convergent uplift {margin} vs interior {interior}"
        );
        // The tallest land on the planet should sit near a convergent margin:
        // count how many of the top five percent of land cells are within two
        // hops of one. Subducting trenches are excluded by looking at land only.
        let mut land: Vec<(u32, f64)> = (0..w.len() as u32)
            .filter(|&c| w.terrain.elevation[c as usize] > 0.0)
            .map(|c| (c, w.terrain.elevation[c as usize]))
            .collect();
        land.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let top = land.len() / 20 + 1;
        let margin: std::collections::BTreeSet<u32> = convergent.iter().copied().collect();
        let mut near = 0;
        for &(cell, _) in land.iter().take(top) {
            let mut around = vec![cell];
            for _ in 0..2 {
                let mut next = Vec::new();
                for c in &around {
                    for &nb in w.sphere.neighbors(*c) {
                        next.push(nb);
                    }
                }
                around.extend(next);
            }
            if around.iter().any(|c| margin.contains(c)) {
                near += 1;
            }
        }
        assert!(
            near * 2 > top,
            "seed {seed}: only {near} of top {top} peaks are near convergent margins"
        );
    }
    assert!(checked >= 3, "not enough seeds produced convergent margins");
}

#[test]
fn dense_plates_subduct() {
    for seed in 0..5u64 {
        let w = common::world(seed, 3);
        let tectonic = &w.tectonics;
        for b in tectonic.boundaries_of(BoundaryKind::Convergent) {
            let sub = b
                .subducting
                .expect("convergent boundary without a subducting plate");
            let over = b
                .overriding
                .expect("convergent boundary without an overriding plate");
            assert_ne!(sub, over);
            assert!(
                tectonic.plates[sub as usize].density
                    >= tectonic.plates[over as usize].density - 0.2
            );
        }
    }
}

#[test]
fn boundary_catalogue_is_complete_across_seeds() {
    let mut kinds = [false; 3];
    for seed in 0..8u64 {
        let w = common::world(seed, 3);
        let mut edges = std::collections::BTreeSet::new();
        for b in &w.tectonics.boundaries {
            assert!(b.a < b.b);
            assert!(edges.insert((b.a, b.b)));
            assert_ne!(w.tectonics.plate_at(b.a), w.tectonics.plate_at(b.b));
            match b.kind {
                BoundaryKind::Convergent => kinds[0] = true,
                BoundaryKind::Divergent => kinds[1] = true,
                BoundaryKind::Transform => kinds[2] = true,
            }
        }
        assert!(!w.tectonics.boundaries.is_empty());
    }
    assert!(kinds[0], "no convergent boundaries in eight seeds");
    assert!(kinds[1], "no divergent boundaries in eight seeds");
    assert!(kinds[2], "no transform boundaries in eight seeds");
}

#[test]
fn uplift_and_subsidence_are_finite_and_non_negative() {
    let w = common::world(21, 3);
    for (cell, &u) in w.tectonics.uplift.iter().enumerate() {
        assert!(u.is_finite() && u >= 0.0, "uplift at {cell}");
    }
    for (cell, &s) in w.tectonics.subsidence.iter().enumerate() {
        assert!(s.is_finite() && s >= 0.0, "subsidence at {cell}");
    }
    for (cell, &v) in w.tectonics.volcanism.iter().enumerate() {
        assert!((0.0..=1.0).contains(&v), "volcanism at {cell}");
    }
}
