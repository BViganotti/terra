//! Lakes.
//!
//! A lake is a connected set of land cells whose filled water surface stands
//! above the ground. The spill point is the lowest filled neighbor outside the
//! lake; if the lake has no spill it is endorheic (a salt flat in practice).
//!
//! Lake geometry is approximate: the surface is treated as horizontal at the
//! maximum filled level of the component, and volume integrates depth over cell
//! areas. That is consistent with the priority flood, which raises each basin to
//! its spill level.

use std::collections::VecDeque;

use crate::sphere::Sphere;

/// Minimum depth for a cell to count as lake water rather than a numerical
/// artifact of the epsilon fill.
pub const MIN_LAKE_DEPTH: f64 = 0.4;

/// One lake.
#[derive(Clone, Debug)]
pub struct Lake {
    /// Dense lake id in `[0, lakes.len())`.
    pub id: u32,
    /// Cells in the lake, sorted ascending.
    pub cells: Vec<u32>,
    /// Water surface elevation in meters.
    pub surface: f64,
    /// Lake cell adjacent to the spill point.
    pub spill_cell: Option<u32>,
    /// Cell just outside the lake at the spill point.
    pub spill_target: Option<u32>,
    /// Filled level of the spill target, in meters.
    pub spill_level: f64,
    /// Surface area in square meters.
    pub area_m2: f64,
    /// Volume in cubic meters.
    pub volume_m3: f64,
    /// Maximum depth in meters.
    pub max_depth: f64,
}

impl Lake {
    /// Mean depth in meters (zero for an empty lake).
    pub fn mean_depth(&self) -> f64 {
        if self.area_m2 > 0.0 {
            self.volume_m3 / self.area_m2
        } else {
            0.0
        }
    }

    /// True when the lake has no surface outflow.
    pub fn is_endorheic(&self) -> bool {
        self.spill_target.is_none()
    }
}

/// All lakes of a world.
#[derive(Clone, Debug, Default)]
pub struct Lakes {
    /// Lakes in id order.
    pub lakes: Vec<Lake>,
    /// Lake id per cell, `u32::MAX` when the cell is not lake water.
    pub lake_of: Vec<u32>,
}

impl Lakes {
    /// Number of lakes.
    pub fn len(&self) -> usize {
        self.lakes.len()
    }

    /// True when there are no lakes.
    pub fn is_empty(&self) -> bool {
        self.lakes.is_empty()
    }

    /// Largest lake by surface area.
    pub fn largest(&self) -> Option<&Lake> {
        self.lakes.iter().max_by(|a, b| {
            a.area_m2
                .partial_cmp(&b.area_m2)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    }

    /// Total water surface area in square meters.
    pub fn total_area_m2(&self) -> f64 {
        self.lakes.iter().map(|l| l.area_m2).sum()
    }

    /// True when a cell is lake water.
    pub fn is_lake(&self, cell: u32) -> bool {
        self.lake_of
            .get(cell as usize)
            .is_some_and(|&id| id != u32::MAX)
    }

    /// Lake containing a cell.
    pub fn lake_at(&self, cell: u32) -> Option<&Lake> {
        let id = *self.lake_of.get(cell as usize)?;
        if id == u32::MAX {
            None
        } else {
            self.lakes.get(id as usize)
        }
    }
}

/// Extracts lakes from a filled surface.
pub fn extract(sphere: &Sphere, elevation: &[f64], filled: &[f64], radius_km: f64) -> Lakes {
    let n = sphere.len().min(elevation.len()).min(filled.len());
    let radius_m = if radius_km.is_finite() && radius_km > 0.0 {
        radius_km * 1_000.0
    } else {
        6_371_000.0
    };
    let mut lake_of = vec![u32::MAX; n];
    let mut lakes = Vec::new();
    let mut visited = vec![false; n];

    let is_water =
        |i: usize| -> bool { elevation[i] > 0.0 && filled[i] - elevation[i] > MIN_LAKE_DEPTH };

    let mut queue = VecDeque::new();
    for start in 0..n {
        if visited[start] || !is_water(start) {
            continue;
        }
        let id = lakes.len() as u32;
        visited[start] = true;
        queue.push_back(start as u32);
        let mut cells = Vec::new();
        let mut surface = f64::NEG_INFINITY;
        let mut volume = 0.0;
        let mut area = 0.0;
        let mut max_depth = 0.0f64;
        while let Some(cell) = queue.pop_front() {
            let i = cell as usize;
            cells.push(cell);
            lake_of[i] = id;
            surface = surface.max(filled[i]);
            let a = sphere.cell_area(cell) * radius_m * radius_m;
            let depth = (filled[i] - elevation[i]).max(0.0);
            volume += depth * a;
            area += a;
            max_depth = max_depth.max(depth);
            for &nb in sphere.neighbors(cell) {
                let ni = nb as usize;
                if ni < n && !visited[ni] && is_water(ni) {
                    visited[ni] = true;
                    queue.push_back(nb);
                }
            }
        }
        cells.sort_unstable();
        if cells.is_empty() {
            continue;
        }
        // Find the lowest filled neighbor outside the lake.
        let mut spill_cell = None;
        let mut spill_target = None;
        let mut spill_level = f64::INFINITY;
        for &cell in &cells {
            for &nb in sphere.neighbors(cell) {
                let ni = nb as usize;
                if ni >= n || lake_of[ni] == id {
                    continue;
                }
                if filled[ni] < spill_level
                    || (filled[ni] == spill_level && spill_target.is_none_or(|t: u32| nb < t))
                {
                    spill_level = filled[ni];
                    spill_target = Some(nb);
                    spill_cell = Some(cell);
                }
            }
        }
        // The lake surface is the spill elevation; the maximum filled value is
        // epsilon-inflated by the priority flood and would overstate the lake.
        let surface = if spill_level.is_finite() {
            spill_level
        } else {
            surface
        };
        let spill_level = surface;
        lakes.push(Lake {
            id,
            cells,
            surface,
            spill_cell,
            spill_target,
            spill_level,
            area_m2: area,
            volume_m3: volume,
            max_depth,
        });
    }
    Lakes { lakes, lake_of }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hydrology::fill::{priority_flood, DEFAULT_EPSILON};

    fn pit_world() -> (Sphere, Vec<f64>, Vec<f64>) {
        let s = Sphere::new(3);
        let mut e = vec![600.0; s.len()];
        let pit = 17u32;
        e[pit as usize] = 100.0;
        // Ocean at the south pole so the flood has a seed.
        e[0] = -500.0;
        let f = priority_flood(&s, &e, DEFAULT_EPSILON);
        let level = f.level;
        (s, e, level)
    }

    #[test]
    fn a_pit_becomes_a_lake() {
        let (s, e, f) = pit_world();
        let lakes = extract(&s, &e, &f, 6_371.0);
        assert_eq!(lakes.len(), 1);
        let l = &lakes.lakes[0];
        assert_eq!(l.cells, vec![17]);
        assert!(l.surface > 600.0);
        assert!(l.max_depth > 400.0);
        assert!(l.area_m2 > 0.0);
        assert!(l.volume_m3 > 0.0);
        assert!(l.spill_target.is_some());
        assert!(lakes.is_lake(17));
        assert!(!lakes.is_lake(0));
    }

    #[test]
    fn lakes_are_basins() {
        let (s, e, f) = pit_world();
        let lakes = extract(&s, &e, &f, 6_371.0);
        for l in &lakes.lakes {
            for &c in &l.cells {
                assert!(e[c as usize] < l.surface + 1e-6);
                assert!(f[c as usize] <= l.surface + 0.01);
            }
            if let (Some(sc), Some(st)) = (l.spill_cell, l.spill_target) {
                assert!(l.cells.contains(&sc));
                assert!(!l.cells.contains(&st));
                assert!(s.neighbors(sc).contains(&st));
                assert!(f[st as usize] >= l.surface - 0.01);
                assert!((l.spill_level - f[sc as usize]).abs() < 0.01);
            }
        }
    }

    #[test]
    fn flat_terrain_has_no_lakes() {
        let s = Sphere::new(3);
        let e = vec![120.0; s.len()];
        let f = priority_flood(&s, &e, DEFAULT_EPSILON);
        let lakes = extract(&s, &e, &f.level, 6_371.0);
        assert!(lakes.is_empty());
        assert!(lakes.largest().is_none());
        assert_eq!(lakes.total_area_m2(), 0.0);
    }

    #[test]
    fn ocean_cells_are_not_lakes() {
        let s = Sphere::new(2);
        let e = vec![-10.0; s.len()];
        let f = priority_flood(&s, &e, 0.001);
        let lakes = extract(&s, &e, &f.level, 6_371.0);
        assert!(lakes.is_empty());
    }

    #[test]
    fn lake_ids_are_dense_and_unique() {
        let (s, e, f) = pit_world();
        let lakes = extract(&s, &e, &f, 6_371.0);
        for (i, l) in lakes.lakes.iter().enumerate() {
            assert_eq!(l.id as usize, i);
            for &c in &l.cells {
                assert_eq!(lakes.lake_of[c as usize], l.id);
                assert_eq!(lakes.lake_at(c).map(|x| x.id), Some(l.id));
            }
        }
    }

    #[test]
    fn mean_depth_is_positive() {
        let (s, e, f) = pit_world();
        let lakes = extract(&s, &e, &f, 6_371.0);
        let l = lakes.largest().unwrap();
        assert!(l.mean_depth() > 0.0);
        assert!(l.mean_depth() <= l.max_depth + 1e-9);
    }

    #[test]
    fn determinism() {
        let (s, e, f) = pit_world();
        let a = extract(&s, &e, &f, 6_371.0);
        let b = extract(&s, &e, &f, 6_371.0);
        assert_eq!(a.lakes.len(), b.lakes.len());
        for (x, y) in a.lakes.iter().zip(&b.lakes) {
            assert_eq!(x.cells, y.cells);
            assert_eq!(x.volume_m3.to_bits(), y.volume_m3.to_bits());
        }
    }
}
