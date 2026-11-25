//! Plate tectonics.
//!
//! The model is deliberately explicit and small enough to audit:
//!
//! 1. Plate seeds are chosen by farthest-point sampling on the cell graph.
//! 2. Cells are assigned to the nearest seed by breadth-first search, producing
//!    a contiguous partition of the sphere (a graph Voronoi diagram).
//! 3. Each plate receives an angular velocity derived from the world seed.
//! 4. Every cross-plate edge is classified as convergent, divergent or
//!    transform from the relative velocity projected onto the local boundary
//!    normal and tangent.
//! 5. Orogenic epochs accumulate uplift on overriding plates, trenches on
//!    subducting plates and volcanic arcs behind convergent margins.
//!
//! The model is qualitative, not geodynamic: it produces plausible belts and
//! converges, and the invariants (partition area, dense plate subducts, uplift
//! concentrated at convergence) are tested. It makes no claim of matching real
//! plate mechanics, which is documented in `LIMITATIONS.md`.

use std::collections::VecDeque;

use crate::noise::{cell_jitter, warped_fbm};
use crate::rng::{Rng, Stream};
use crate::sphere::Sphere;
use crate::vec3::Vec3;

/// Role of a plate's lithosphere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlateKind {
    /// Mostly submerged, dense, subducts readily.
    Oceanic,
    /// Mostly emergent, buoyant, overrides.
    Continental,
    /// Mixture of both; behaves according to its mean buoyancy.
    Mixed,
}

impl PlateKind {
    /// Short stable identifier used in JSON and the UI.
    pub fn code(self) -> &'static str {
        match self {
            PlateKind::Oceanic => "oceanic",
            PlateKind::Continental => "continental",
            PlateKind::Mixed => "mixed",
        }
    }
}

/// Relative motion across a plate boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoundaryKind {
    /// Plates approach; one subducts or they collide.
    Convergent,
    /// Plates separate; new crust forms.
    Divergent,
    /// Plates slide past each other.
    Transform,
}

impl BoundaryKind {
    /// Short stable identifier.
    pub fn code(self) -> &'static str {
        match self {
            BoundaryKind::Convergent => "convergent",
            BoundaryKind::Divergent => "divergent",
            BoundaryKind::Transform => "transform",
        }
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            BoundaryKind::Convergent => "convergent margin",
            BoundaryKind::Divergent => "divergent rift",
            BoundaryKind::Transform => "transform fault",
        }
    }
}

/// Dominant tectonic stress at a cell, used by maps and history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stress {
    /// Interior of a plate.
    Quiet,
    /// Compressive margin.
    Convergent,
    /// Extensional margin.
    Divergent,
    /// Shear margin.
    Transform,
    /// Volcanic arc behind a convergent margin.
    Volcanic,
}

impl Stress {
    /// Short stable identifier.
    pub fn code(self) -> &'static str {
        match self {
            Stress::Quiet => "quiet",
            Stress::Convergent => "convergent",
            Stress::Divergent => "divergent",
            Stress::Transform => "transform",
            Stress::Volcanic => "volcanic",
        }
    }
}

/// A lithospheric plate.
#[derive(Clone, Debug)]
pub struct Plate {
    /// Dense index, equal to the position in [`Tectonics::plates`].
    pub id: u16,
    /// Cell chosen as the plate nucleus.
    pub seed_cell: u32,
    /// Normalized mean direction of the plate's cells.
    pub centroid: Vec3,
    /// Mean continental mask value over the plate, in `[-1, 1]`.
    pub continental: f64,
    /// Lithospheric kind derived from the continental mask.
    pub kind: PlateKind,
    /// Relative density in `g/cm^3` units; higher subducts.
    pub density: f64,
    /// Angular velocity in radians per million years, magnitude and axis.
    pub motion: Vec3,
    /// Total spherical area in steradians.
    pub area: f64,
    /// Fraction of the sphere covered by the plate.
    pub area_share: f64,
    /// Palette slot for rendering.
    pub color_index: u8,
    /// Number of cells owned.
    pub cell_count: u32,
}

/// One cross-plate edge in the cell graph.
#[derive(Clone, Debug)]
pub struct Boundary {
    /// Clockwise-first cell of the edge.
    pub a: u32,
    /// Clockwise-second cell of the edge.
    pub b: u32,
    /// Plate of `a`.
    pub plate_a: u16,
    /// Plate of `b`.
    pub plate_b: u16,
    /// Classification for this epoch.
    pub kind: BoundaryKind,
    /// Closing rate in meters per million years; positive means converging.
    pub convergence: f64,
    /// Shear rate along the boundary in meters per million years.
    pub shear: f64,
    /// Plate that subducts, present for convergent boundaries.
    pub subducting: Option<u16>,
    /// Plate that overrides, present for convergent boundaries.
    pub overriding: Option<u16>,
    /// Midpoint direction on the unit sphere.
    pub midpoint: Vec3,
    /// Unit tangent along the boundary.
    pub tangent: Vec3,
}

/// Tunable tectonic parameters.
#[derive(Clone, Copy, Debug)]
pub struct TectonicsConfig {
    /// Number of plates, clamped to `3..=24`.
    pub plates: u32,
    /// Number of orogenic epochs, clamped to `1..=64`.
    pub epochs: u32,
    /// Duration represented by one epoch, in million years.
    pub epoch_myr: f64,
    /// Orogenic efficiency; conversion of convergence to relief.
    pub uplift_efficiency: f64,
}

impl Default for TectonicsConfig {
    fn default() -> Self {
        Self {
            plates: 9,
            epochs: 24,
            epoch_myr: 6.0,
            uplift_efficiency: 0.0011,
        }
    }
}

impl TectonicsConfig {
    /// Clamps all fields into supported ranges.
    pub fn sanitized(mut self) -> Self {
        self.plates = self.plates.clamp(3, 24);
        self.epochs = self.epochs.clamp(1, 64);
        self.epoch_myr = if self.epoch_myr.is_finite() {
            self.epoch_myr.clamp(0.5, 50.0)
        } else {
            6.0
        };
        self.uplift_efficiency = if self.uplift_efficiency.is_finite() {
            self.uplift_efficiency.clamp(0.0001, 0.02)
        } else {
            0.0011
        };
        self
    }
}

/// The complete tectonic state of a world.
#[derive(Clone, Debug)]
pub struct Tectonics {
    /// Plate id per cell.
    pub plate_of: Vec<u16>,
    /// Plates in id order.
    pub plates: Vec<Plate>,
    /// Cross-plate edges after the final epoch.
    pub boundaries: Vec<Boundary>,
    /// Dominant stress per cell.
    pub stress: Vec<Stress>,
    /// Accumulated uplift in meters (non-negative).
    pub uplift: Vec<f64>,
    /// Accumulated subsidence in meters (non-negative).
    pub subsidence: Vec<f64>,
    /// Volcanic intensity in `[0, 1]`.
    pub volcanism: Vec<f64>,
    /// Broad continental mask in `[-1, 1]`; drives base elevation.
    pub continental: Vec<f64>,
    /// Number of orogenic epochs performed.
    pub epochs: u32,
}

impl Tectonics {
    /// Number of cells covered.
    pub fn len(&self) -> usize {
        self.plate_of.len()
    }

    /// True when no cells exist.
    pub fn is_empty(&self) -> bool {
        self.plate_of.is_empty()
    }

    /// Plate owning a cell.
    pub fn plate_at(&self, cell: u32) -> u16 {
        self.plate_of.get(cell as usize).copied().unwrap_or(0)
    }

    /// Sum of plate areas, for partition checks.
    pub fn total_area(&self) -> f64 {
        self.plates.iter().map(|p| p.area).sum()
    }

    /// Area share of a plate in `[0, 1]`.
    pub fn area_share(&self, plate: u16) -> f64 {
        self.plates
            .get(plate as usize)
            .map(|p| p.area_share)
            .unwrap_or(0.0)
    }

    /// Subset of boundary edges with a given classification.
    pub fn boundaries_of(&self, kind: BoundaryKind) -> impl Iterator<Item = &Boundary> {
        self.boundaries.iter().filter(move |b| b.kind == kind)
    }

    /// Counts of boundaries by kind: `(convergent, divergent, transform)`.
    pub fn boundary_counts(&self) -> (usize, usize, usize) {
        let mut c = (0, 0, 0);
        for b in &self.boundaries {
            match b.kind {
                BoundaryKind::Convergent => c.0 += 1,
                BoundaryKind::Divergent => c.1 += 1,
                BoundaryKind::Transform => c.2 += 1,
            }
        }
        c
    }

    /// Mean uplift over a set of cells.
    pub fn mean_uplift(&self, cells: impl Iterator<Item = u32>) -> f64 {
        let mut sum = 0.0;
        let mut n = 0u64;
        for c in cells {
            if let Some(&u) = self.uplift.get(c as usize) {
                sum += u;
                n += 1;
            }
        }
        if n == 0 {
            0.0
        } else {
            sum / n as f64
        }
    }

    /// Highest uplift cell, if any.
    pub fn highest_uplift(&self) -> Option<(u32, f64)> {
        let mut best: Option<(u32, f64)> = None;
        for (i, &u) in self.uplift.iter().enumerate() {
            if best.map_or(true, |(_, b)| u > b) {
                best = Some((i as u32, u));
            }
        }
        best
    }
}

/// Seeds plate nuclei by farthest-point sampling.
///
/// The first seed is uniform; each later seed is the cell whose minimum
/// great-circle distance to the existing seeds is largest, with ties broken by
/// cell index. This spreads plates over the sphere without clustering.
pub fn choose_seeds(sphere: &Sphere, seed: u64, count: u32) -> Vec<u32> {
    let n = sphere.len();
    let count = (count as usize).clamp(3, 24).min(n.max(1));
    if n == 0 {
        return Vec::new();
    }
    let mut rng = Rng::new(seed, Stream::PLATES);
    let first = (rng.below(n as u64) as u32).min(n as u32 - 1);
    let mut seeds = vec![first];
    let mut min_dist = vec![f64::MAX; n];
    for i in 0..n as u32 {
        min_dist[i as usize] = sphere.arc(first, i);
    }
    while seeds.len() < count {
        let mut best = 0u32;
        let mut best_d = -1.0;
        for (i, &d) in min_dist.iter().enumerate() {
            if d > best_d + 1e-12 {
                best_d = d;
                best = i as u32;
            }
        }
        if best_d <= 0.0 {
            break;
        }
        seeds.push(best);
        for i in 0..n as u32 {
            let d = sphere.arc(best, i);
            if d < min_dist[i as usize] {
                min_dist[i as usize] = d;
            }
        }
    }
    seeds
}

/// Assigns every cell to the nearest seed by BFS hops, ties broken by lower
/// plate id. Every cell receives exactly one plate.
pub fn assign_plates(sphere: &Sphere, seeds: &[u32]) -> Vec<u16> {
    let n = sphere.len();
    let mut owner = vec![u16::MAX; n];
    let mut dist = vec![u32::MAX; n];
    let mut queue = VecDeque::new();
    for (id, &s) in seeds.iter().enumerate() {
        let si = s as usize;
        if si >= n {
            continue;
        }
        let id = id as u16;
        if dist[si] == u32::MAX {
            dist[si] = 0;
            owner[si] = id;
            queue.push_back(s);
        } else if id < owner[si] {
            owner[si] = id;
        }
    }
    while let Some(c) = queue.pop_front() {
        let d = dist[c as usize];
        for &nb in sphere.neighbors(c) {
            let ni = nb as usize;
            let nd = d.saturating_add(1);
            if nd < dist[ni] {
                dist[ni] = nd;
                owner[ni] = owner[c as usize];
                queue.push_back(nb);
            } else if nd == dist[ni] && owner[c as usize] < owner[ni] {
                // Equal hop distance: deterministic tie-break by plate id.
                owner[ni] = owner[c as usize];
            }
        }
    }
    // Any unreachable cell (impossible on a connected mesh) falls back to the
    // nearest seed by angle instead of panicking.
    for i in 0..n {
        if owner[i] == u16::MAX {
            owner[i] = seeds
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    let da = sphere.arc(**a, i as u32);
                    let db = sphere.arc(**b, i as u32);
                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(idx, _)| idx as u16)
                .unwrap_or(0);
        }
    }
    owner
}

/// Generates the tectonic state for a world.
pub fn generate(sphere: &Sphere, seed: u64, config: TectonicsConfig) -> Tectonics {
    let config = config.sanitized();
    let cfg = config;
    let n = sphere.len();
    let seeds = choose_seeds(sphere, seed, cfg.plates);
    let plate_count = seeds.len().max(1);
    let plate_of = assign_plates(sphere, &seeds);

    // Broad continental mask. Plate buoyancy is derived from this before any
    // relief exists, keeping the causal order explicit: composition first,
    // motion second, orogeny third.
    let continental: Vec<f64> = (0..n)
        .map(|i| warped_fbm(seed ^ 0x00c0_4e01, sphere.pos(i as u32), 1.55, 5, 0.42))
        .collect();

    let mut rng = Rng::new(seed, Stream::PLATES).fork(Stream(0x00a1_1000));
    let mut plates: Vec<Plate> = Vec::with_capacity(plate_count);
    for (id, &seed_cell) in seeds.iter().enumerate() {
        let mut centroid = Vec3::ZERO;
        let mut area = 0.0;
        let mut cells = 0u32;
        let mut cont_sum = 0.0;
        for cell in 0..n {
            if plate_of[cell] as usize == id {
                centroid += sphere.pos(cell as u32);
                area += sphere.cell_area(cell as u32);
                cont_sum += continental[cell];
                cells += 1;
            }
        }
        let mean_cont = if cells > 0 {
            cont_sum / f64::from(cells)
        } else {
            0.0
        };
        let kind = if mean_cont > 0.06 {
            PlateKind::Continental
        } else if mean_cont < -0.06 {
            PlateKind::Oceanic
        } else {
            PlateKind::Mixed
        };
        let density = match kind {
            PlateKind::Oceanic => 3.05,
            PlateKind::Mixed => 2.95,
            PlateKind::Continental => 2.78,
        } + rng.range(-0.04, 0.04);
        let axis = random_unit(&mut rng);
        let rate = rng.range(0.00022, 0.00135);
        plates.push(Plate {
            id: id as u16,
            seed_cell,
            centroid: if centroid.length_sq() > 0.0 {
                centroid.normalized()
            } else {
                sphere.pos(seed_cell)
            },
            continental: mean_cont,
            kind,
            density,
            motion: axis * rate,
            area,
            area_share: 0.0,
            color_index: id as u8,
            cell_count: cells,
        });
    }

    // Deterministic palette slots so adjacent plates look distinct.
    let mut slots: Vec<u8> = (0..plates.len() as u8).collect();
    rng.shuffle(&mut slots);
    for (p, slot) in plates.iter_mut().zip(slots) {
        p.color_index = slot;
    }

    let total: f64 = plates.iter().map(|p| p.area).sum();
    for p in &mut plates {
        p.area_share = if total > 0.0 { p.area / total } else { 0.0 };
    }

    // Per-epoch orogeny. Motions are perturbed slightly each epoch to model
    // plate reorganization; assignments stay fixed so the partition is stable.
    let mut uplift = vec![0.0f64; n];
    let mut subsidence = vec![0.0f64; n];
    let mut volcanism = vec![0.0f64; n];
    let mut stress = vec![Stress::Quiet; n];
    let mut boundaries: Vec<Boundary> = Vec::new();

    for epoch in 0..cfg.epochs {
        perturb_motions(&mut plates, seed, epoch);
        boundaries = classify_boundaries(sphere, &plate_of, &plates, &continental);
        stress.fill(Stress::Quiet);
        for b in &boundaries {
            let s = match b.kind {
                BoundaryKind::Convergent => Stress::Convergent,
                BoundaryKind::Divergent => Stress::Divergent,
                BoundaryKind::Transform => Stress::Transform,
            };
            stress[b.a as usize] = s;
            stress[b.b as usize] = s;
        }

        for b in &boundaries {
            let falloff = 1.0 / (1.0 + epoch as f64 * 0.04);
            match b.kind {
                BoundaryKind::Convergent => {
                    let Some(sub) = b.subducting else {
                        continue;
                    };
                    let over = b.overriding.unwrap_or(b.plate_a);
                    let rate = b.convergence.max(0.0);
                    let base = rate * cfg.epoch_myr * cfg.uplift_efficiency * falloff;
                    apply_relief(
                        sphere,
                        &plate_of,
                        &mut uplift,
                        cell_on_side(b, over),
                        2,
                        over,
                        base,
                    );
                    // Trench on the subducting side.
                    let trench_cell = cell_on_side(b, sub);
                    subsidence[trench_cell as usize] += base * 0.45;
                    // Volcanic arc one hop behind the overriding margin.
                    for &nb in sphere.neighbors(cell_on_side(b, over)) {
                        if plate_of[nb as usize] == over && nb != trench_cell {
                            volcanism[nb as usize] = (volcanism[nb as usize] + 0.12).min(1.0);
                            stress[nb as usize] = Stress::Volcanic;
                        }
                    }
                }
                BoundaryKind::Divergent => {
                    let rate = (-b.convergence).max(0.0);
                    let base = rate * cfg.epoch_myr * cfg.uplift_efficiency * 0.35 * falloff;
                    subsidence[b.a as usize] += base;
                    subsidence[b.b as usize] += base;
                    // Shoulder uplift just off the rift axis.
                    for cell in [b.a, b.b] {
                        for &nb in sphere.neighbors(cell) {
                            if plate_of[nb as usize] == plate_of[cell as usize] {
                                uplift[nb as usize] += base * 0.18;
                            }
                        }
                    }
                }
                BoundaryKind::Transform => {
                    let base =
                        b.shear.abs() * cfg.epoch_myr * cfg.uplift_efficiency * 0.08 * falloff;
                    uplift[b.a as usize] += base;
                    uplift[b.b as usize] += base;
                }
            }
        }
    }

    // Isostatic smoothing of the uplift field keeps single-cell spikes from
    // surviving; relief is a crustal property, not a per-cell lottery.
    smooth(&sphere, &mut uplift, 1);
    smooth(&sphere, &mut volcanism, 1);
    for u in &mut uplift {
        *u = u.max(0.0);
        if !u.is_finite() {
            *u = 0.0;
        }
    }
    for s in &mut subsidence {
        *s = s.max(0.0);
        if !s.is_finite() {
            *s = 0.0;
        }
    }
    for v in &mut volcanism {
        *v = v.clamp(0.0, 1.0);
    }

    Tectonics {
        plate_of,
        plates,
        boundaries,
        stress,
        uplift,
        subsidence,
        volcanism,
        continental,
        epochs: cfg.epochs,
    }
}

fn cell_on_side(b: &Boundary, plate: u16) -> u32 {
    if b.plate_a == plate {
        b.a
    } else {
        b.b
    }
}

fn random_unit(rng: &mut Rng) -> Vec3 {
    for _ in 0..8 {
        let v = Vec3::new(
            rng.range(-1.0, 1.0),
            rng.range(-1.0, 1.0),
            rng.range(-1.0, 1.0),
        );
        if let Some(u) = v.try_normalize() {
            return u;
        }
    }
    Vec3::Z
}

fn perturb_motions(plates: &mut [Plate], seed: u64, epoch: u32) {
    let mut rng = Rng::new(seed, Stream::PLATES)
        .fork(Stream(0x00e0_0000 + u64::from(epoch)))
        .fork(Stream(0x00e1_0000));
    for p in plates.iter_mut() {
        let drift = Vec3::new(
            rng.range(-1.0, 1.0),
            rng.range(-1.0, 1.0),
            rng.range(-1.0, 1.0),
        ) * 0.000_08;
        let speed = p.motion.length();
        let dir = p.motion.normalized();
        let merged = (dir + drift).normalized();
        let factor = rng.range(0.94, 1.06);
        p.motion = merged * (speed * factor).clamp(0.0001, 0.0025);
    }
}

/// Classifies every cross-plate edge.
fn classify_boundaries(
    sphere: &Sphere,
    plate_of: &[u16],
    plates: &[Plate],
    continental: &[f64],
) -> Vec<Boundary> {
    let n = sphere.len();
    let mut out = Vec::new();
    // Relative motion threshold: 0.1 cm/yr is 1 km/Myr = 1000 m/Myr.
    let shear_threshold = 1_000.0;
    for a in 0..n as u32 {
        let pa = plate_of[a as usize];
        let va = plates
            .get(pa as usize)
            .map(|p| p.motion.cross(sphere.pos(a)))
            .unwrap_or(Vec3::ZERO);
        for &b in sphere.neighbors(a) {
            if b <= a {
                continue;
            }
            let pb = plate_of[b as usize];
            if pb == pa {
                continue;
            }
            let va = va * 6_371_000.0;
            let vb = plates
                .get(pb as usize)
                .map(|p| p.motion.cross(sphere.pos(b)) * 6_371_000.0)
                .unwrap_or(Vec3::ZERO);
            let mid = ((sphere.pos(a) + sphere.pos(b)) * 0.5).normalized();
            let tangent = sphere
                .pos(a)
                .cross(sphere.pos(b))
                .try_normalize()
                .unwrap_or_else(|| {
                    let (e, _) = mid.tangent_basis();
                    e
                });
            let toward_b = (sphere.pos(b) - sphere.pos(a))
                .project_tangent(mid)
                .unwrap_or(tangent);
            let rel = va - vb;
            let convergence = rel.dot(toward_b);
            let shear = rel.dot(tangent);
            let kind = if convergence > shear_threshold {
                BoundaryKind::Convergent
            } else if convergence < -shear_threshold {
                BoundaryKind::Divergent
            } else {
                BoundaryKind::Transform
            };
            let (subducting, overriding) = if kind == BoundaryKind::Convergent {
                let da = plates.get(pa as usize).map(|p| p.density).unwrap_or(2.9)
                    + continental[a as usize] * -0.05;
                let db = plates.get(pb as usize).map(|p| p.density).unwrap_or(2.9)
                    + continental[b as usize] * -0.05;
                if da >= db {
                    (Some(pa), Some(pb))
                } else {
                    (Some(pb), Some(pa))
                }
            } else {
                (None, None)
            };
            out.push(Boundary {
                a,
                b,
                plate_a: pa,
                plate_b: pb,
                kind,
                convergence,
                shear,
                subducting,
                overriding,
                midpoint: mid,
                tangent,
            });
        }
    }
    // Deterministic order regardless of neighbor iteration order.
    out.sort_by(|x, y| (x.a, x.b).cmp(&(y.a, y.b)));
    out
}

fn apply_relief(
    sphere: &Sphere,
    plate_of: &[u16],
    uplift: &mut [f64],
    start: u32,
    radius: u32,
    plate: u16,
    base: f64,
) {
    if !base.is_finite() || base <= 0.0 {
        return;
    }
    let mut seen = vec![false; uplift.len()];
    let mut queue = VecDeque::new();
    seen[start as usize] = true;
    queue.push_back((start, 0u32));
    while let Some((cell, hop)) = queue.pop_front() {
        if plate_of[cell as usize] == plate {
            let weight = 1.0 / (1.0 + f64::from(hop) * 1.6);
            uplift[cell as usize] += base * weight;
        }
        if hop >= radius {
            continue;
        }
        for &nb in sphere.neighbors(cell) {
            let ni = nb as usize;
            if !seen[ni] && plate_of[ni] == plate {
                seen[ni] = true;
                queue.push_back((nb, hop + 1));
            }
        }
    }
}

fn smooth(sphere: &Sphere, field: &mut [f64], passes: u32) {
    let n = field.len();
    let mut tmp = vec![0.0; n];
    for _ in 0..passes {
        for i in 0..n {
            let mut sum = field[i] * 2.0;
            let mut w = 2.0;
            for &nb in sphere.neighbors(i as u32) {
                sum += field[nb as usize];
                w += 1.0;
            }
            tmp[i] = sum / w;
        }
        field.copy_from_slice(&tmp);
    }
}

/// A small deterministic normal used for per-cell variation that should not
/// shift when tectonics changes.
pub fn plate_jitter(seed: u64, cell: u32) -> f64 {
    cell_jitter(seed, 0x7ec7_0001, cell) - 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(plates: u32) -> TectonicsConfig {
        TectonicsConfig {
            plates,
            epochs: 8,
            ..TectonicsConfig::default()
        }
    }

    #[test]
    fn seeds_are_far_apart() {
        let s = Sphere::new(3);
        let seeds = choose_seeds(&s, 5, 8);
        assert_eq!(seeds.len(), 8);
        for (i, &a) in seeds.iter().enumerate() {
            for &b in &seeds[i + 1..] {
                assert!(s.arc(a, b) > 0.05, "seeds {a} and {b} are too close");
            }
        }
    }

    #[test]
    fn seeds_are_deterministic() {
        let s = Sphere::new(4);
        assert_eq!(choose_seeds(&s, 9, 7), choose_seeds(&s, 9, 7));
        assert_ne!(choose_seeds(&s, 9, 7), choose_seeds(&s, 10, 7));
    }

    #[test]
    fn assignment_is_a_partition() {
        let s = Sphere::new(3);
        for count in [3, 5, 9] {
            let seeds = choose_seeds(&s, 3, count);
            let owner = assign_plates(&s, &seeds);
            assert_eq!(owner.len(), s.len());
            for (i, &o) in owner.iter().enumerate() {
                assert!((o as usize) < seeds.len(), "cell {i} owned by {o}");
            }
            for id in 0..seeds.len() as u16 {
                assert!(owner.iter().any(|&o| o == id), "plate {id} is empty");
            }
        }
    }

    #[test]
    fn plate_areas_sum_to_the_sphere() {
        let s = Sphere::new(4);
        let t = generate(&s, 42, cfg(9));
        assert_eq!(t.plates.len(), 9);
        let total = t.total_area();
        let expected = 4.0 * std::f64::consts::PI;
        assert!((total - expected).abs() / expected < 1e-3);
        let share: f64 = t.plates.iter().map(|p| p.area_share).sum();
        assert!((share - 1.0).abs() < 1e-9);
        for p in &t.plates {
            assert!(p.area > 0.0);
            assert!(p.cell_count > 0);
        }
        let cells: u32 = t.plates.iter().map(|p| p.cell_count).sum();
        assert_eq!(cells as usize, s.len());
    }

    #[test]
    fn plate_kinds_match_continental_means() {
        let s = Sphere::new(3);
        let t = generate(&s, 7, cfg(7));
        for p in &t.plates {
            match p.kind {
                PlateKind::Continental => assert!(p.continental > 0.06),
                PlateKind::Oceanic => assert!(p.continental < -0.06),
                PlateKind::Mixed => {
                    assert!(p.continental <= 0.06 && p.continental >= -0.06)
                }
            }
            assert!(p.density > 2.5 && p.density < 3.3);
        }
    }

    #[test]
    fn dense_plate_subducts() {
        let s = Sphere::new(3);
        let t = generate(&s, 11, cfg(8));
        let mut checked = 0;
        for b in t.boundaries_of(BoundaryKind::Convergent) {
            let sub = b.subducting.unwrap();
            let over = b.overriding.unwrap();
            let sub_p = &t.plates[sub as usize];
            let over_p = &t.plates[over as usize];
            assert!(sub_p.density >= over_p.density - 0.2);
            assert_eq!(sub_p.id, sub);
            checked += 1;
        }
        assert!(checked > 0, "no convergent boundaries generated");
    }

    #[test]
    fn all_boundary_kinds_appear() {
        let s = Sphere::new(3);
        let mut found = (false, false, false);
        for seed in 0..12 {
            let t = generate(&s, seed, cfg(9));
            let (c, d, tr) = t.boundary_counts();
            found.0 |= c > 0;
            found.1 |= d > 0;
            found.2 |= tr > 0;
        }
        assert!(found.0 && found.1 && found.2, "{found:?}");
    }

    #[test]
    fn boundaries_are_unique_per_edge() {
        let s = Sphere::new(3);
        let t = generate(&s, 13, cfg(6));
        let mut seen = std::collections::BTreeSet::new();
        for b in &t.boundaries {
            assert!(seen.insert((b.a, b.b)));
            assert!(b.a < b.b);
            assert_ne!(t.plate_at(b.a), t.plate_at(b.b));
        }
    }

    #[test]
    fn uplift_concentrates_at_convergent_margins() {
        let s = Sphere::new(4);
        let t = generate(&s, 21, cfg(10));
        let convergent: Vec<u32> = t
            .boundaries_of(BoundaryKind::Convergent)
            .flat_map(|b| [b.a, b.b])
            .collect();
        assert!(!convergent.is_empty());
        let at_margin = t.mean_uplift(convergent.iter().copied());
        let all = t.mean_uplift(0..s.len() as u32);
        assert!(at_margin > all * 3.0, "margin {at_margin} vs mean {all}");
        assert!(t.uplift.iter().all(|u| u.is_finite() && *u >= 0.0));
        assert!(t.uplift.iter().any(|&u| u > 100.0));
    }

    #[test]
    fn generation_is_deterministic() {
        let s = Sphere::new(3);
        let a = generate(&s, 99, cfg(8));
        let b = generate(&s, 99, cfg(8));
        assert_eq!(a.plate_of, b.plate_of);
        assert_eq!(a.boundaries.len(), b.boundaries.len());
        for (x, y) in a.uplift.iter().zip(&b.uplift) {
            assert_eq!(x.to_bits(), y.to_bits());
        }
        for (x, y) in a.plates.iter().zip(&b.plates) {
            assert_eq!(x.motion.x.to_bits(), y.motion.x.to_bits());
        }
    }

    #[test]
    fn config_is_clamped() {
        let s = Sphere::new(2);
        let t = generate(
            &s,
            1,
            TectonicsConfig {
                plates: 500,
                epochs: 500,
                epoch_myr: f64::NAN,
                uplift_efficiency: -9.0,
            },
        );
        assert!(t.plates.len() <= 24);
        assert!(t.epochs <= 64);
    }

    #[test]
    fn highest_uplift_is_positive() {
        let s = Sphere::new(2);
        let t = generate(&s, 4, cfg(5));
        let (_, h) = t.highest_uplift().unwrap();
        assert!(h > 0.0);
    }
}
