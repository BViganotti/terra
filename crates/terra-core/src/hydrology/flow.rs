//! Drainage networks on the cell graph.
//!
//! Given a filled water surface, each land cell drains to the neighbor with the
//! steepest descent. Because the filled surface is strictly descending along
//! that edge, the flow graph is a forest rooted at ocean cells. That property is
//! what makes accumulation a single reverse-order pass and what lets tests
//! assert that rivers never run uphill.
//!
//! Accumulation is measured in arbitrary upstream weight units. The caller
//! supplies per-cell weights (usually runoff times cell area), so the same code
//! works for erosion before climate exists and for discharge afterward.

use crate::sphere::Sphere;

/// Flow directions and network statistics.
#[derive(Clone, Debug)]
pub struct Drainage {
    /// Downstream cell, or `None` for ocean cells and cells with no lower
    /// neighbor.
    pub downhill: Vec<Option<u32>>,
    /// Upstream weight accumulated at each cell, including its own weight.
    pub accumulation: Vec<f64>,
    /// Strahler stream order, starting at 1 for headwater cells.
    pub strahler: Vec<u8>,
    /// True when the cell's flow reaches the ocean.
    pub ocean_reach: Vec<bool>,
    /// Land cells that empty directly into the ocean, sorted ascending.
    pub outlets: Vec<u32>,
    /// Number of cells that drain into each cell (unweighted).
    pub upstream_cells: Vec<u32>,
}

impl Drainage {
    /// Number of cells.
    pub fn len(&self) -> usize {
        self.downhill.len()
    }

    /// True when no cells exist.
    pub fn is_empty(&self) -> bool {
        self.downhill.is_empty()
    }

    /// Follows the flow path from a cell, with a hard step limit.
    pub fn path(&self, start: u32, max_steps: usize) -> Vec<u32> {
        let mut out = Vec::new();
        let mut cur = start;
        for _ in 0..max_steps {
            out.push(cur);
            match self.downhill.get(cur as usize).copied().flatten() {
                Some(next) if next != cur => cur = next,
                _ => break,
            }
        }
        out
    }

    /// Largest accumulation value.
    pub fn max_accumulation(&self) -> f64 {
        self.accumulation.iter().copied().fold(0.0, f64::max)
    }

    /// Cells with an accumulation at or above a threshold, in ascending order.
    pub fn channels_above(&self, threshold: f64) -> Vec<u32> {
        let mut out: Vec<u32> = self
            .accumulation
            .iter()
            .enumerate()
            .filter(|(_, &a)| a >= threshold)
            .map(|(i, _)| i as u32)
            .collect();
        out.sort_unstable();
        out
    }
}

/// Computes steepest-descent flow, Strahler order and accumulation.
///
/// `elevation` is the real surface (used for the ocean test), `filled` is the
/// depression-filled surface (used for descent), and `weights` supplies the
/// per-cell contribution. Missing or non-finite weights count as zero.
pub fn build(
    sphere: &Sphere,
    elevation: &[f64],
    filled: &[f64],
    order: &[u32],
    weights: &[f64],
) -> Drainage {
    let n = sphere.len().min(filled.len()).min(elevation.len());
    let mut downhill = vec![None; n];
    for i in 0..n {
        if elevation[i] <= 0.0 {
            continue;
        }
        let cell = i as u32;
        let mut best: Option<(f64, u32)> = None;
        for &nb in sphere.neighbors(cell) {
            let ni = nb as usize;
            if ni >= n {
                continue;
            }
            let drop = filled[i] - filled[ni];
            if drop <= 1e-12 {
                continue;
            }
            let dist = sphere.arc(cell, nb).max(1e-9);
            let slope = drop / dist;
            let replace = match best {
                None => true,
                Some((s, b)) => slope > s + 1e-15 || ((slope - s).abs() <= 1e-15 && nb < b),
            };
            if replace {
                best = Some((slope, nb));
            }
        }
        downhill[i] = best.map(|(_, b)| b);
    }

    // Ascending filled order is downstream-first. Use it to propagate ocean
    // reachability; use the reverse for accumulation and Strahler ordering.
    let mut ocean_reach = vec![false; n];
    for i in 0..n {
        if elevation[i] <= 0.0 {
            ocean_reach[i] = true;
        }
    }
    let mut sorted_order: Vec<u32> = order
        .iter()
        .copied()
        .filter(|&c| (c as usize) < n)
        .collect();
    if sorted_order.len() < n {
        for i in 0..n as u32 {
            if !sorted_order.contains(&i) {
                sorted_order.push(i);
            }
        }
    }
    for &cell in &sorted_order {
        let i = cell as usize;
        if elevation[i] > 0.0 {
            if let Some(ds) = downhill[i] {
                ocean_reach[i] = ocean_reach[ds as usize];
            }
        }
    }

    let mut accumulation = vec![0.0f64; n];
    let mut upstream_cells = vec![0u32; n];
    let mut strahler = vec![1u8; n];
    let mut max_upstream_order = vec![1u8; n];
    let mut max_upstream_count = vec![0u8; n];
    for i in 0..n {
        let w = weights.get(i).copied().unwrap_or(0.0);
        accumulation[i] = if w.is_finite() && w > 0.0 { w } else { 0.0 };
    }
    for &cell in sorted_order.iter().rev() {
        let i = cell as usize;
        // Order is final now: all upstream cells were processed earlier in this
        // reversed sequence because they sit higher on the filled surface.
        if max_upstream_count[i] >= 2 {
            strahler[i] = max_upstream_order[i].saturating_add(1).min(9);
        } else if max_upstream_count[i] == 1 {
            strahler[i] = max_upstream_order[i];
        } else {
            strahler[i] = 1;
        }
        if let Some(ds) = downhill[i] {
            let d = ds as usize;
            if d < n {
                accumulation[d] += accumulation[i];
                upstream_cells[d] = upstream_cells[d].saturating_add(1);
                if strahler[i] > max_upstream_order[d] {
                    max_upstream_order[d] = strahler[i];
                    max_upstream_count[d] = 1;
                } else if strahler[i] == max_upstream_order[d] {
                    max_upstream_count[d] = max_upstream_count[d].saturating_add(1);
                }
            }
        }
    }

    let mut outlets: Vec<u32> = (0..n as u32)
        .filter(|&c| {
            elevation[c as usize] > 0.0
                && downhill[c as usize].is_some_and(|d| elevation[d as usize] <= 0.0)
        })
        .collect();
    outlets.sort_unstable();

    Drainage {
        downhill,
        accumulation,
        strahler,
        ocean_reach,
        outlets,
        upstream_cells,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hydrology::fill::{priority_flood, DEFAULT_EPSILON};

    fn synthetic_sphere() -> (Sphere, Vec<f64>) {
        // Latitude-driven ramp: south is ocean, north is a smooth upland. This
        // guarantees a well-defined drainage network for structural tests.
        let s = Sphere::new(3);
        let e: Vec<f64> = (0..s.len())
            .map(|i| {
                let lat = s.lat(i as u32);
                (lat + 30.0) * 40.0
            })
            .collect();
        (s, e)
    }

    fn network(s: &Sphere, e: &[f64]) -> (Vec<f64>, Drainage) {
        let f = priority_flood(s, e, DEFAULT_EPSILON);
        let w = vec![1.0; s.len()];
        let d = build(s, e, &f.level, &f.order, &w);
        (f.level, d)
    }

    #[test]
    fn every_land_cell_flows_downhill_or_to_ocean() {
        let (s, e) = synthetic_sphere();
        let (level, d) = network(&s, &e);
        for i in 0..s.len() {
            if e[i] > 0.0 {
                let ds = d.downhill[i].expect("land cell without a downstream cell");
                assert!(level[ds as usize] < level[i]);
            } else {
                assert!(d.downhill[i].is_none());
            }
        }
    }

    #[test]
    fn flow_only_ever_descends() {
        let (s, e) = synthetic_sphere();
        let (level, d) = network(&s, &e);
        for (i, ds) in d.downhill.iter().enumerate() {
            if let Some(ds) = ds {
                assert!(level[*ds as usize] < level[i] + 1e-9);
                assert!(*ds != i as u32);
            }
        }
    }

    #[test]
    fn accumulation_is_conserved_at_outlets() {
        let (s, e) = synthetic_sphere();
        let (_, d) = network(&s, &e);
        // Every cell contributes one unit. Ocean cells do not flow onward, so
        // summing their accumulation counts every contribution exactly once.
        let at_ocean: f64 = (0..s.len())
            .filter(|&i| e[i] <= 0.0)
            .map(|i| d.accumulation[i])
            .sum();
        assert!((at_ocean - s.len() as f64).abs() < 1e-9, "{at_ocean}");
        assert!(d.accumulation.iter().all(|a| a.is_finite() && *a >= 0.0));
    }

    #[test]
    fn accumulation_increases_downstream() {
        let (s, e) = synthetic_sphere();
        let (_, d) = network(&s, &e);
        for (i, ds) in d.downhill.iter().enumerate() {
            if let Some(ds) = ds {
                assert!(d.accumulation[*ds as usize] >= d.accumulation[i]);
            }
        }
    }

    #[test]
    fn strahler_order_respects_confluence() {
        let (s, e) = synthetic_sphere();
        let (_, d) = network(&s, &e);
        assert!(d.strahler.iter().all(|&o| (1..=9).contains(&o)));
        for (i, ds) in d.downhill.iter().enumerate() {
            if let Some(ds) = ds {
                let dsi = *ds as usize;
                assert!(d.strahler[dsi] >= 1);
                // A cell with two order-k parents must be at least k+1.
                if d.upstream_cells[dsi] >= 2 {
                    let parents: Vec<u8> = (0..s.len())
                        .filter(|&j| d.downhill[j] == Some(*ds))
                        .map(|j| d.strahler[j])
                        .collect();
                    let max = parents.iter().copied().max().unwrap_or(1);
                    let count = parents.iter().filter(|&&o| o == max).count();
                    if count >= 2 {
                        assert_eq!(d.strahler[dsi], max.saturating_add(1).min(9));
                    }
                    let _ = i;
                }
            }
        }
    }

    #[test]
    fn all_land_reaches_the_ocean() {
        let (s, e) = synthetic_sphere();
        let (_, d) = network(&s, &e);
        for i in 0..s.len() {
            if e[i] > 0.0 {
                assert!(d.ocean_reach[i], "cell {i} is endorheic");
            }
        }
        assert!(!d.outlets.is_empty());
        for &o in &d.outlets {
            assert!(e[o as usize] > 0.0);
            assert!(d.downhill[o as usize].is_some_and(|c| e[c as usize] <= 0.0));
        }
    }

    #[test]
    fn paths_terminate() {
        let (s, e) = synthetic_sphere();
        let (_, d) = network(&s, &e);
        for start in (0..s.len() as u32).step_by(37) {
            let p = d.path(start, s.len());
            assert!(!p.is_empty());
            assert!(p.len() <= s.len());
        }
    }

    #[test]
    fn weights_are_respected() {
        let (s, e) = synthetic_sphere();
        let f = priority_flood(&s, &e, DEFAULT_EPSILON);
        let w = vec![0.0; s.len()];
        let d = build(&s, &e, &f.level, &f.order, &w);
        assert!(d.accumulation.iter().all(|&a| a == 0.0));

        let mut w2 = vec![0.0; s.len()];
        let source = (0..s.len() as u32)
            .find(|&c| e[c as usize] > 0.0 && d.ocean_reach[c as usize])
            .expect("synthetic world has land");
        w2[source as usize] = 7.5;
        let d2 = build(&s, &e, &f.level, &f.order, &w2);
        let path = d2.path(source, s.len());
        for &c in &path {
            assert!((d2.accumulation[c as usize] - 7.5).abs() < 1e-9);
        }
        let sink = *path.last().unwrap();
        assert!(e[sink as usize] <= 0.0 || d2.downhill[sink as usize].is_none());
    }

    #[test]
    fn deterministic() {
        let (s, e) = synthetic_sphere();
        let (_, a) = network(&s, &e);
        let (_, b) = network(&s, &e);
        assert_eq!(a.downhill, b.downhill);
        assert_eq!(a.accumulation, b.accumulation);
        assert_eq!(a.strahler, b.strahler);
        assert_eq!(a.outlets, b.outlets);
    }

    #[test]
    fn channels_above_threshold() {
        let (s, e) = synthetic_sphere();
        let (_, d) = network(&s, &e);
        let max = d.max_accumulation();
        let ch = d.channels_above(max * 0.5);
        assert!(!ch.is_empty());
        assert!(ch.windows(2).all(|w| w[0] < w[1]));
    }
}
