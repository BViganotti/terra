//! Icosphere geometry.
//!
//! An icosphere starts from the 20 faces of a regular icosahedron and splits
//! each triangle into four, projecting new midpoints onto the unit sphere. The
//! result has near-uniform cell areas and no polar singularity, which makes it a
//! good substrate for planet-scale simulation.
//!
//! Adjacency is stored in compressed sparse row form: a sorted neighbor list and
//! an offset table. Neighbor lists are built from the triangle soup, so the
//! graph is always reciprocal even after floating point rounding.

use std::collections::BTreeMap;

use crate::vec3::Vec3;

/// A subdivided icosphere.
#[derive(Clone, Debug)]
pub struct Sphere {
    subdiv: u32,
    verts: Vec<Vec3>,
    faces: Vec<[u32; 3]>,
    nbr_start: Vec<u32>,
    nbr_index: Vec<u32>,
    area: Vec<f64>,
    index: SpatialIndex,
}

const GOLDEN: f64 = 1.618_033_988_749_894_9;

/// The 20 base faces of an icosahedron in the vertex order produced by
/// [`base_vertices`]. Orientation is normalized by the builder.
const BASE_FACES: [[u32; 3]; 20] = [
    [0, 11, 5],
    [0, 5, 1],
    [0, 1, 7],
    [0, 7, 10],
    [0, 10, 11],
    [1, 5, 9],
    [5, 11, 4],
    [11, 10, 2],
    [10, 7, 6],
    [7, 1, 8],
    [3, 9, 4],
    [3, 4, 2],
    [3, 2, 6],
    [3, 6, 8],
    [3, 8, 9],
    [4, 9, 5],
    [2, 4, 11],
    [6, 2, 10],
    [8, 6, 7],
    [9, 8, 1],
];

fn base_vertices() -> Vec<Vec3> {
    let t = GOLDEN;
    let raw = [
        (-1.0, t, 0.0),
        (1.0, t, 0.0),
        (-1.0, -t, 0.0),
        (1.0, -t, 0.0),
        (0.0, -1.0, t),
        (0.0, 1.0, t),
        (0.0, -1.0, -t),
        (0.0, 1.0, -t),
        (t, 0.0, -1.0),
        (t, 0.0, 1.0),
        (-t, 0.0, -1.0),
        (-t, 0.0, 1.0),
    ];
    raw.iter()
        .map(|&(x, y, z)| Vec3::new(x, y, z).normalized())
        .collect()
}

/// Supported subdivision levels. The upper bound caps memory at 163,842 cells
/// (about 2.6 MB per `f64` layer) and rejects accidental huge allocations.
pub const MAX_SUBDIV: u32 = 7;

fn midpoint_key(a: u32, b: u32) -> (u32, u32) {
    if a < b {
        (a, b)
    } else {
        (b, a)
    }
}

impl Sphere {
    /// Builds a sphere at the requested subdivision level, clamped to
    /// [`MAX_SUBDIV`].
    pub fn new(subdiv: u32) -> Self {
        let subdiv = subdiv.min(MAX_SUBDIV);
        let mut verts = base_vertices();
        let mut faces: Vec<[u32; 3]> = BASE_FACES.to_vec();

        for _ in 0..subdiv {
            let mut cache: BTreeMap<(u32, u32), u32> = BTreeMap::new();
            let mut next: Vec<[u32; 3]> = Vec::with_capacity(faces.len() * 4);
            let mut push_mid = |verts: &mut Vec<Vec3>, a: u32, b: u32| -> u32 {
                let key = midpoint_key(a, b);
                if let Some(&idx) = cache.get(&key) {
                    return idx;
                }
                let mid = ((verts[a as usize] + verts[b as usize]) * 0.5).normalized();
                let idx = verts.len() as u32;
                verts.push(mid);
                cache.insert(key, idx);
                idx
            };
            for f in &faces {
                let [a, b, c] = *f;
                let ab = push_mid(&mut verts, a, b);
                let bc = push_mid(&mut verts, b, c);
                let ca = push_mid(&mut verts, c, a);
                next.push([a, ab, ca]);
                next.push([b, bc, ab]);
                next.push([c, ca, bc]);
                next.push([ab, bc, ca]);
            }
            faces = next;
        }

        // Ensure every face is counter-clockwise when viewed from outside.
        for f in &mut faces {
            let [a, b, c] = *f;
            let pa = verts[a as usize];
            let pb = verts[b as usize];
            let pc = verts[c as usize];
            if (pb - pa).cross(pc - pa).dot(pa) < 0.0 {
                *f = [a, c, b];
            }
        }

        let neighbor_sets = build_neighbors(verts.len(), &faces);
        let (nbr_start, nbr_index) = flatten_neighbors(&neighbor_sets);
        let area = cell_areas(&verts, &faces);
        let index = SpatialIndex::build(&verts);
        Self {
            subdiv,
            verts,
            faces,
            nbr_start,
            nbr_index,
            area,
            index,
        }
    }

    /// Subdivision level actually used.
    pub fn subdiv(&self) -> u32 {
        self.subdiv
    }

    /// Number of cells.
    pub fn len(&self) -> usize {
        self.verts.len()
    }

    /// True when the sphere has no cells (never true in practice).
    pub fn is_empty(&self) -> bool {
        self.verts.is_empty()
    }

    /// Number of triangles.
    pub fn face_count(&self) -> usize {
        self.faces.len()
    }

    /// Cell direction on the unit sphere.
    pub fn pos(&self, cell: u32) -> Vec3 {
        self.verts[cell as usize]
    }

    /// Cell direction, interpreted as a slice for iteration.
    pub fn positions(&self) -> &[Vec3] {
        &self.verts
    }

    /// Triangle soup.
    pub fn faces(&self) -> &[[u32; 3]] {
        &self.faces
    }

    /// Neighbors of a cell. The slice is sorted by index, which keeps iteration
    /// deterministic across builds and platforms.
    pub fn neighbors(&self, cell: u32) -> &[u32] {
        let i = cell as usize;
        if i + 1 >= self.nbr_start.len() {
            return &[];
        }
        let start = self.nbr_start[i] as usize;
        let end = self.nbr_start[i + 1] as usize;
        &self.nbr_index[start..end]
    }

    /// Number of neighbors of a cell.
    pub fn degree(&self, cell: u32) -> usize {
        self.neighbors(cell).len()
    }

    /// Spherical area of a cell in steradians.
    pub fn cell_area(&self, cell: u32) -> f64 {
        self.area.get(cell as usize).copied().unwrap_or(0.0)
    }

    /// All cell areas.
    pub fn areas(&self) -> &[f64] {
        &self.area
    }

    /// Total sphere area, which must be `4*pi`.
    pub fn total_area(&self) -> f64 {
        self.area.iter().sum()
    }

    /// Great-circle angle in radians between two cells.
    pub fn arc(&self, a: u32, b: u32) -> f64 {
        self.pos(a).angle(self.pos(b))
    }

    /// Great-circle distance in planet radians (approximate kilometers when
    /// multiplied by the scenario radius).
    pub fn distance(&self, a: u32, b: u32) -> f64 {
        self.arc(a, b)
    }

    /// Latitude in degrees, positive north.
    pub fn lat(&self, cell: u32) -> f64 {
        self.pos(cell).latitude_deg()
    }

    /// Longitude in degrees, positive east.
    pub fn lon(&self, cell: u32) -> f64 {
        self.pos(cell).longitude_deg()
    }

    /// Nearest cell to an arbitrary direction.
    pub fn nearest(&self, dir: Vec3) -> u32 {
        self.index.nearest(&self.verts, dir)
    }

    /// Nearest cell to a latitude/longitude pair in degrees.
    pub fn nearest_lat_lon(&self, lat: f64, lon: f64) -> u32 {
        self.nearest(Vec3::from_lat_lon(lat, lon))
    }

    /// Neighbors of a cell that are closer than a given arc, sorted.
    pub fn neighbors_within(&self, cell: u32, max_arc: f64) -> Vec<u32> {
        let mut out: Vec<u32> = self
            .neighbors(cell)
            .iter()
            .copied()
            .filter(|&n| self.arc(cell, n) <= max_arc)
            .collect();
        out.sort_unstable();
        out
    }

    /// Computes the graph distance in hops from a seed set using breadth-first
    /// search. Unreachable cells keep `u32::MAX`.
    pub fn hop_distances(&self, seeds: &[u32]) -> Vec<u32> {
        let n = self.len();
        let mut dist = vec![u32::MAX; n];
        let mut queue = std::collections::VecDeque::new();
        for &s in seeds {
            let i = s as usize;
            if i < n && dist[i] != 0 {
                dist[i] = 0;
                queue.push_back(s);
            }
        }
        while let Some(c) = queue.pop_front() {
            let d = dist[c as usize];
            for &n in self.neighbors(c) {
                let ni = n as usize;
                if dist[ni] == u32::MAX {
                    dist[ni] = d.saturating_add(1);
                    queue.push_back(n);
                }
            }
        }
        dist
    }

    /// Connected component label of each cell under a predicate, in ascending
    /// cell order. Components are numbered from zero; cells failing the
    /// predicate get `u32::MAX`.
    pub fn components<F: Fn(u32) -> bool>(&self, include: F) -> Vec<u32> {
        let n = self.len();
        let mut labels = vec![u32::MAX; n];
        let mut next = 0u32;
        let mut stack = Vec::new();
        for start in 0..n as u32 {
            if labels[start as usize] != u32::MAX || !include(start) {
                continue;
            }
            labels[start as usize] = next;
            stack.push(start);
            while let Some(c) = stack.pop() {
                for &nb in self.neighbors(c) {
                    let ni = nb as usize;
                    if labels[ni] == u32::MAX && include(nb) {
                        labels[ni] = next;
                        stack.push(nb);
                    }
                }
            }
            next += 1;
        }
        labels
    }

    /// Structural self-check used by tests and debug builds.
    pub fn validate(&self) -> Result<(), String> {
        if self.verts.len() != self.area.len() {
            return Err("area/vertex length mismatch".into());
        }
        if self.nbr_start.len() != self.verts.len() + 1 {
            return Err("neighbor offsets do not cover all cells".into());
        }
        for (i, v) in self.verts.iter().enumerate() {
            if !v.is_finite() || (v.length() - 1.0).abs() > 1e-9 {
                return Err(format!("cell {i} is not on the unit sphere"));
            }
            let nbs = self.neighbors(i as u32);
            if nbs.len() < 5 || nbs.len() > 6 {
                return Err(format!("cell {i} has {} neighbors", nbs.len()));
            }
            for w in nbs.windows(2) {
                if w[0] >= w[1] {
                    return Err(format!("cell {i} has an unsorted neighbor list"));
                }
            }
            for &n in nbs {
                if !self.neighbors(n).contains(&(i as u32)) {
                    return Err(format!("adjacency {i}->{n} is not reciprocal"));
                }
            }
        }
        let total = self.total_area();
        let expected = 4.0 * std::f64::consts::PI;
        if (total - expected).abs() > 1e-6 {
            return Err(format!("area sum {total} differs from 4*pi"));
        }
        Ok(())
    }
}

fn build_neighbors(n: usize, faces: &[[u32; 3]]) -> Vec<Vec<u32>> {
    let mut sets: Vec<Vec<u32>> = vec![Vec::new(); n];
    for f in faces {
        for k in 0..3 {
            let a = f[k];
            let b = f[(k + 1) % 3];
            if a == b {
                continue;
            }
            sets[a as usize].push(b);
            sets[b as usize].push(a);
        }
    }
    for list in &mut sets {
        list.sort_unstable();
        list.dedup();
    }
    sets
}

fn flatten_neighbors(sets: &[Vec<u32>]) -> (Vec<u32>, Vec<u32>) {
    let mut start = Vec::with_capacity(sets.len() + 1);
    let mut index = Vec::new();
    start.push(0);
    for list in sets {
        index.extend_from_slice(list);
        start.push(index.len() as u32);
    }
    (start, index)
}

/// Area of the spherical triangle spanned by three unit vectors.
pub fn triangle_area(a: Vec3, b: Vec3, c: Vec3) -> f64 {
    let numerator = a.dot(b.cross(c)).abs();
    let denominator = 1.0 + a.dot(b) + b.dot(c) + c.dot(a);
    2.0 * numerator.atan2(denominator)
}

fn cell_areas(verts: &[Vec3], faces: &[[u32; 3]]) -> Vec<f64> {
    let mut area = vec![0.0f64; verts.len()];
    for f in faces {
        let a = verts[f[0] as usize];
        let b = verts[f[1] as usize];
        let c = verts[f[2] as usize];
        let t = triangle_area(a, b, c) / 3.0;
        area[f[0] as usize] += t;
        area[f[1] as usize] += t;
        area[f[2] as usize] += t;
    }
    area
}

/// Latitude/longitude binning for nearest-cell lookups.
///
/// Rendering asks for the cell under millions of pixels; a linear scan over ten
/// thousand cells per pixel would dominate generation. The index maps a
/// direction to a bin of roughly one cell width and scans the 5x5 neighborhood.
#[derive(Clone, Debug)]
struct SpatialIndex {
    lat_bins: usize,
    lon_bins: usize,
    bins: Vec<Vec<u32>>,
}

impl SpatialIndex {
    fn build(verts: &[Vec3]) -> Self {
        // Bins are about half the mean cell spacing wide, and the query scans a
        // small neighborhood around its own bin. That guarantees the nearest
        // cell is considered while keeping per-lookup work constant.
        let n = verts.len().max(1);
        let lat_bins = (n as f64).sqrt().floor().clamp(8.0, 180.0) as usize;
        let lon_bins = lat_bins * 2;
        let mut bins = vec![Vec::new(); lat_bins * lon_bins];
        for (i, v) in verts.iter().enumerate() {
            let (la, lo) = Self::bin_of(v, lat_bins, lon_bins);
            bins[la * lon_bins + lo].push(i as u32);
        }
        Self {
            lat_bins,
            lon_bins,
            bins,
        }
    }

    fn bin_of(v: &Vec3, lat_bins: usize, lon_bins: usize) -> (usize, usize) {
        let lat = v.latitude_deg();
        let lon = v.longitude_deg();
        let la = (((lat + 90.0) / 180.0) * lat_bins as f64).floor() as isize;
        let lo = (((lon + 180.0) / 360.0) * lon_bins as f64).floor() as isize;
        let la = la.clamp(0, lat_bins as isize - 1) as usize;
        let lo = lo.rem_euclid(lon_bins as isize) as usize;
        (la, lo)
    }

    fn nearest(&self, verts: &[Vec3], dir: Vec3) -> u32 {
        if verts.is_empty() {
            return 0;
        }
        let d = dir.normalized();
        let (la, lo) = Self::bin_of(&d, self.lat_bins, self.lon_bins);
        let mut best = u32::MAX;
        let mut best_dot = f64::NEG_INFINITY;
        for dla in -3isize..=3 {
            let row = la as isize + dla;
            if !(0..self.lat_bins as isize).contains(&row) {
                continue;
            }
            for dlo in -3isize..=3 {
                let col = (lo as isize + dlo).rem_euclid(self.lon_bins as isize) as usize;
                for &idx in &self.bins[row as usize * self.lon_bins + col] {
                    let dot = verts[idx as usize].dot(d);
                    if dot > best_dot {
                        best_dot = dot;
                        best = idx;
                    }
                }
            }
        }
        if best == u32::MAX || best_dot < 0.5 {
            // Degenerate index (should not happen); fall back to a full scan.
            for (i, v) in verts.iter().enumerate() {
                let dot = v.dot(d);
                if dot > best_dot {
                    best_dot = dot;
                    best = i as u32;
                }
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_have_expected_sizes() {
        assert_eq!(Sphere::new(0).len(), 12);
        assert_eq!(Sphere::new(1).len(), 42);
        assert_eq!(Sphere::new(2).len(), 162);
        assert_eq!(Sphere::new(3).len(), 642);
        assert_eq!(Sphere::new(4).len(), 2562);
        assert_eq!(Sphere::new(5).len(), 10_242);
        assert_eq!(Sphere::new(0).face_count(), 20);
        assert_eq!(Sphere::new(1).face_count(), 80);
    }

    #[test]
    fn subdivision_is_clamped() {
        let s = Sphere::new(99);
        assert_eq!(s.subdiv(), MAX_SUBDIV);
        assert_eq!(s.len(), 163_842);
    }

    #[test]
    fn structure_validates_at_every_level() {
        for level in 0..=5 {
            let s = Sphere::new(level);
            s.validate()
                .unwrap_or_else(|e| panic!("level {level}: {e}"));
        }
    }

    #[test]
    fn neighbors_are_sorted_and_reciprocal() {
        let s = Sphere::new(3);
        for cell in 0..s.len() as u32 {
            let nbs = s.neighbors(cell);
            assert!((5..=6).contains(&nbs.len()));
            for &n in nbs {
                assert!(n != cell);
                assert!(s.neighbors(n).contains(&cell));
            }
        }
    }

    #[test]
    fn total_area_is_four_pi() {
        for level in 0..=4 {
            let s = Sphere::new(level);
            let expected = 4.0 * std::f64::consts::PI;
            assert!((s.total_area() - expected).abs() < 1e-9);
            assert!(s.areas().iter().all(|&a| a > 0.0));
        }
    }

    #[test]
    fn cell_areas_are_balanced() {
        let s = Sphere::new(4);
        let mean = s.total_area() / s.len() as f64;
        for &a in s.areas() {
            assert!(a > mean * 0.7 && a < mean * 1.3, "area {a} vs mean {mean}");
        }
    }

    #[test]
    fn nearest_returns_the_cell_itself() {
        let s = Sphere::new(4);
        for cell in (0..s.len() as u32).step_by(7) {
            assert_eq!(s.nearest(s.pos(cell)), cell);
            assert_eq!(s.nearest_lat_lon(s.lat(cell), s.lon(cell)), cell);
        }
    }

    #[test]
    fn nearest_is_close_to_brute_force() {
        let s = Sphere::new(4);
        for lat in (-80..=80).step_by(13) {
            for lon in (-180..=180).step_by(17) {
                let dir = Vec3::from_lat_lon(f64::from(lat), f64::from(lon));
                let best = s.nearest(dir);
                let best_dot = s.pos(best).dot(dir);
                for i in 0..s.len() as u32 {
                    assert!(s.pos(i).dot(dir) <= best_dot + 1e-12);
                }
            }
        }
    }

    #[test]
    fn hop_distances_are_zero_at_seeds() {
        let s = Sphere::new(4);
        let seeds = [0u32, 100, 2000];
        let d = s.hop_distances(&seeds);
        for &seed in &seeds {
            assert_eq!(d[seed as usize], 0);
        }
        assert!(d.iter().all(|&v| v != u32::MAX));
        // Neighbors of a seed are one hop away unless they are also seeds.
        for &seed in &seeds {
            for &n in s.neighbors(seed) {
                if !seeds.contains(&n) {
                    assert_eq!(d[n as usize], 1);
                }
            }
        }
    }

    #[test]
    fn components_split_a_band() {
        let s = Sphere::new(4);
        let labels = s.components(|c| s.pos(c).z > 0.0);
        let north: Vec<u32> = labels.iter().copied().filter(|&l| l != u32::MAX).collect();
        assert!(!north.is_empty());
        // All included cells form one northern cap.
        assert_eq!(
            labels.iter().filter(|&&l| l != u32::MAX).count(),
            north.len()
        );
        assert!(north.iter().all(|&l| l == north[0]));
        assert!(labels.iter().any(|&l| l == u32::MAX));
    }

    #[test]
    fn triangle_area_matches_octant() {
        let a = Vec3::X;
        let b = Vec3::Y;
        let c = Vec3::Z;
        let expected = std::f64::consts::FRAC_PI_2;
        assert!((triangle_area(a, b, c) - expected).abs() < 1e-12);
    }

    #[test]
    fn latitude_range_is_covered() {
        let s = Sphere::new(3);
        let mut min = f64::MAX;
        let mut max = f64::MIN;
        for i in 0..s.len() as u32 {
            min = min.min(s.lat(i));
            max = max.max(s.lat(i));
        }
        assert!(min < -80.0);
        assert!(max > 80.0);
    }

    #[test]
    fn neighbors_within_filters_by_arc() {
        let s = Sphere::new(4);
        let all = s.neighbors(0);
        let none = s.neighbors_within(0, 0.0);
        assert!(none.is_empty());
        let plenty = s.neighbors_within(0, std::f64::consts::PI);
        assert_eq!(plenty.len(), all.len());
    }
}
