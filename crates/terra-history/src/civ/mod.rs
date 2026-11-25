//! Civilizations and their material culture.
//!
//! A civilization is a small, explicit record: where it lives, how many people
//! it feeds, what it believes about its neighbors, and which technologies it
//! knows. There is no hidden agent logic. The simulation in [`sim`] reads and
//! writes these fields through documented rules, and every change that matters
//! is recorded in the chronicle with its causes.

pub mod sim;

use std::collections::BTreeMap;

use terra_core::world::World;

pub use sim::{simulate, History, HistoryConfig, HistoryHashes, HistoryStats, PoliticalSnapshot};

/// Maximum civilizations in one run.
pub const MAX_CIVS: usize = 32;
/// Maximum settlements tracked per civilization.
pub const MAX_CITIES: usize = 48;
/// Maximum technologies.
pub const MAX_TECH: u8 = 8;

/// A natural resource found in a cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Resource {
    /// Cereal and root crops.
    Grain,
    /// Construction and ship timber.
    Timber,
    /// Copper for tools and ornament.
    Copper,
    /// Iron for weapons and ploughs.
    Iron,
    /// Gold for trade and legitimacy.
    Gold,
    /// Building stone.
    Stone,
    /// Coastal and river fish.
    Fish,
    /// Horses and other draft animals.
    Horses,
    /// Aromatic spices.
    Spices,
}

impl Resource {
    /// Every resource.
    pub const ALL: [Resource; 9] = [
        Resource::Grain,
        Resource::Timber,
        Resource::Copper,
        Resource::Iron,
        Resource::Gold,
        Resource::Stone,
        Resource::Fish,
        Resource::Horses,
        Resource::Spices,
    ];

    /// Stable identifier.
    pub fn code(self) -> &'static str {
        match self {
            Resource::Grain => "grain",
            Resource::Timber => "timber",
            Resource::Copper => "copper",
            Resource::Iron => "iron",
            Resource::Gold => "gold",
            Resource::Stone => "stone",
            Resource::Fish => "fish",
            Resource::Horses => "horses",
            Resource::Spices => "spices",
        }
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Resource::Grain => "grain",
            Resource::Timber => "timber",
            Resource::Copper => "copper",
            Resource::Iron => "iron",
            Resource::Gold => "gold",
            Resource::Stone => "stone",
            Resource::Fish => "fish",
            Resource::Horses => "horses",
            Resource::Spices => "spices",
        }
    }

    /// Resource contribution to military strength.
    pub fn military_value(self) -> f64 {
        match self {
            Resource::Iron => 0.35,
            Resource::Horses => 0.25,
            Resource::Copper => 0.12,
            Resource::Gold => 0.1,
            _ => 0.0,
        }
    }

    /// Resource contribution to trade income.
    pub fn trade_value(self) -> f64 {
        match self {
            Resource::Gold => 0.3,
            Resource::Spices => 0.25,
            Resource::Copper | Resource::Iron => 0.12,
            Resource::Timber | Resource::Stone => 0.06,
            Resource::Fish | Resource::Grain => 0.04,
            Resource::Horses => 0.08,
        }
    }
}

/// Cultural tendencies generated at founding, each in `[0, 1]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Traits {
    /// Emphasis on farming and population growth.
    pub agrarian: f64,
    /// Emphasis on soldiers and conquest.
    pub martial: f64,
    /// Emphasis on trade and wealth.
    pub mercantile: f64,
}

impl Default for Traits {
    fn default() -> Self {
        Self {
            agrarian: 0.4,
            martial: 0.3,
            mercantile: 0.3,
        }
    }
}

impl Traits {
    /// Builds traits from three raw weights, normalizing them.
    pub fn normalized(a: f64, m: f64, t: f64) -> Self {
        let (a, m, t) = (a.max(0.0), m.max(0.0), t.max(0.0));
        let sum = a + m + t;
        if !(sum > 0.0) {
            return Traits::default();
        }
        Self {
            agrarian: (a / sum).clamp(0.05, 0.8),
            martial: (m / sum).clamp(0.05, 0.8),
            mercantile: (t / sum).clamp(0.05, 0.8),
        }
    }

    /// Short description for event details.
    pub fn describe(&self) -> &'static str {
        if self.martial >= self.agrarian && self.martial >= self.mercantile {
            "martial"
        } else if self.mercantile >= self.agrarian {
            "mercantile"
        } else {
            "agrarian"
        }
    }
}

/// A settlement.
#[derive(Clone, Debug, PartialEq)]
pub struct City {
    /// Cell.
    pub cell: u32,
    /// Name.
    pub name: String,
    /// Year founded or captured.
    pub since: u32,
    /// Population.
    pub population: f64,
    /// True for the capital.
    pub capital: bool,
}

/// A civilization.
#[derive(Clone, Debug, PartialEq)]
pub struct Civ {
    /// Dense id.
    pub id: u16,
    /// Display name.
    pub name: String,
    /// Palette slot.
    pub color_index: u8,
    /// Capital cell.
    pub capital: u32,
    /// Capital name.
    pub capital_name: String,
    /// Owned cells, sorted ascending.
    pub cells: Vec<u32>,
    /// Settlements.
    pub cities: Vec<City>,
    /// Population in people.
    pub population: f64,
    /// Stored food in person-years.
    pub food: f64,
    /// Social cohesion in `[0, 1]`.
    pub cohesion: f64,
    /// Military strength in arbitrary units.
    pub military: f64,
    /// Treasury in gold units.
    pub treasury: f64,
    /// Technology level `0..=MAX_TECH`.
    pub tech: u8,
    /// Cultural traits.
    pub traits: Traits,
    /// Known resources and how many deposits are worked.
    pub resources: BTreeMap<Resource, u32>,
    /// Year founded.
    pub founded_year: u32,
    /// Alive at the end of the run.
    pub alive: bool,
    /// Year of collapse, when applicable.
    pub collapsed_year: Option<u32>,
    /// Last chronicle event involving this civilization.
    pub last_event: Option<u32>,
    /// Active war opponents.
    pub at_war_with: Vec<u16>,
    /// Whether a golden age has already been recorded.
    pub had_golden_age: bool,
}

impl Civ {
    /// Number of owned cells.
    pub fn size(&self) -> usize {
        self.cells.len()
    }

    /// True when the civilization owns a cell.
    pub fn owns(&self, cell: u32) -> bool {
        self.cells.binary_search(&cell).is_ok()
    }

    /// Adds a cell, keeping the list sorted and unique.
    pub fn claim(&mut self, cell: u32) -> bool {
        match self.cells.binary_search(&cell) {
            Ok(_) => false,
            Err(pos) => {
                self.cells.insert(pos, cell);
                true
            }
        }
    }

    /// Removes a cell, returning whether it was owned.
    pub fn release(&mut self, cell: u32) -> bool {
        match self.cells.binary_search(&cell) {
            Ok(pos) => {
                self.cells.remove(pos);
                true
            }
            Err(_) => false,
        }
    }

    /// Removes all cities at a cell, returning the removed city.
    pub fn take_city(&mut self, cell: u32) -> Option<City> {
        self.cities.iter().position(|c| c.cell == cell).map(|i| {
            let mut city = self.cities.remove(i);
            if city.population <= 0.0 {
                city.population = 1.0;
            }
            city
        })
    }

    /// Adds or refreshes a city.
    pub fn upsert_city(&mut self, city: City) {
        if let Some(existing) = self.cities.iter_mut().find(|c| c.cell == city.cell) {
            *existing = city;
            return;
        }
        if self.cities.len() < MAX_CITIES {
            self.cities.push(city);
        }
    }

    /// Find a city by cell.
    pub fn city_at(&self, cell: u32) -> Option<&City> {
        self.cities.iter().find(|c| c.cell == cell)
    }

    /// Largest city.
    pub fn largest_city(&self) -> Option<&City> {
        self.cities.iter().max_by(|a, b| {
            a.population
                .partial_cmp(&b.population)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    }

    /// Adds a resource deposit, returning true when it is newly known.
    pub fn discover(&mut self, resource: Resource) -> bool {
        let entry = self.resources.entry(resource).or_insert(0);
        *entry = entry.saturating_add(1);
        *entry == 1
    }

    /// Technology bonus multiplier.
    pub fn tech_bonus(&self) -> f64 {
        1.0 + f64::from(self.tech) * 0.11
    }

    /// Resource-derived military multiplier.
    pub fn resource_military(&self) -> f64 {
        1.0 + self
            .resources
            .keys()
            .map(|r| r.military_value())
            .sum::<f64>()
    }

    /// Resource-derived trade multiplier.
    pub fn resource_trade(&self) -> f64 {
        1.0 + self.resources.keys().map(|r| r.trade_value()).sum::<f64>()
    }

    /// True when the civilization is at war.
    pub fn at_war(&self) -> bool {
        !self.at_war_with.is_empty()
    }

    /// Population density in people per cell.
    pub fn density(&self) -> f64 {
        if self.cells.is_empty() {
            0.0
        } else {
            self.population / self.cells.len() as f64
        }
    }

    /// Average carrying capacity of owned cells, from the world's ecosystems.
    pub fn capacity(&self, world: &World) -> f64 {
        self.cells
            .iter()
            .map(|&c| {
                world
                    .ecosystems
                    .carrying_capacity
                    .get(c as usize)
                    .copied()
                    .unwrap_or(0.0)
            })
            .sum()
    }

    /// Centre of the owned territory as a unit vector sum, helpful for labels.
    pub fn centroid(&self, world: &World) -> terra_core::vec3::Vec3 {
        let mut sum = terra_core::vec3::Vec3::ZERO;
        for &c in &self.cells {
            sum += world.sphere.pos(c);
        }
        sum.normalized()
    }

    /// Capital if still alive, otherwise the largest held city, otherwise the
    /// capital cell. Used for map labels.
    pub fn seat(&self) -> u32 {
        if self.owns(self.capital) || self.cities.is_empty() {
            self.capital
        } else {
            self.largest_city().map(|c| c.cell).unwrap_or(self.capital)
        }
    }
}

/// Technology names in discovery order.
pub const TECHNOLOGY_NAMES: [&str; 8] = [
    "Agriculture",
    "Pottery",
    "Bronze working",
    "Writing",
    "Iron working",
    "Navigation",
    "Astronomy",
    "Printing",
];

/// Population thresholds for each technology beyond Agriculture.
pub const TECHNOLOGY_THRESHOLDS: [f64; 8] = [
    0.0,
    25_000.0,
    90_000.0,
    260_000.0,
    700_000.0,
    1_800_000.0,
    4_500_000.0,
    11_000_000.0,
];

const NAME_ONSETS: [&str; 24] = [
    "ash", "bel", "cor", "dun", "el", "fen", "gal", "harn", "ith", "jor", "kel", "lor", "mor",
    "nor", "or", "pel", "quen", "rom", "sar", "tal", "vel", "wyr", "xan", "zel",
];
const NAME_MIDS: [&str; 12] = [
    "a", "e", "i", "o", "u", "ae", "ia", "io", "ea", "ei", "ou", "y",
];
const NAME_CODAS: [&str; 18] = [
    "dor", "mar", "nis", "tia", "ron", "via", "lan", "dis", "gar", "mon", "ra", "eth", "wyn",
    "sel", "tor", "vak", "und", "ion",
];

/// Generates a deterministic, pronounceable civilization name.
pub fn civilization_name(index: u32, seed: u64) -> String {
    use terra_core::rng::{mix3, unit_from_hash};
    let pick = |salt: u64, n: usize| -> usize {
        (unit_from_hash(mix3(seed, salt, u64::from(index))) * n as f64) as usize % n.max(1)
    };
    let onset = NAME_ONSETS[pick(0x01, NAME_ONSETS.len())];
    let mid = NAME_MIDS[pick(0x02, NAME_MIDS.len())];
    let coda = NAME_CODAS[pick(0x03, NAME_CODAS.len())];
    let mut name = String::with_capacity(12);
    name.push_str(onset);
    name.push_str(mid);
    name.push_str(coda);
    if pick(0x04, 100) < 35 {
        name.push_str(NAME_CODAS[pick(0x05, NAME_CODAS.len())]);
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => {
            let mut out = first.to_uppercase().collect::<String>();
            out.push_str(chars.as_str());
            out
        }
        None => format!("Civ {index}"),
    }
}

/// Generates a settlement name, generally shorter than a civilization name.
pub fn city_name(civ_index: u32, city_index: u32, seed: u64) -> String {
    use terra_core::rng::{mix3, unit_from_hash};
    let key = u64::from(civ_index) * 97 + u64::from(city_index);
    let pick = |salt: u64, n: usize| -> usize {
        (unit_from_hash(mix3(seed, salt, key)) * n as f64) as usize % n.max(1)
    };
    let onset = NAME_ONSETS[pick(0x10, NAME_ONSETS.len())];
    let mid = NAME_MIDS[pick(0x11, NAME_MIDS.len())];
    let coda = NAME_CODAS[pick(0x12, NAME_CODAS.len())];
    let mut name = String::with_capacity(10);
    name.push_str(onset);
    name.push_str(mid);
    name.push_str(coda);
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => {
            let mut out = first.to_uppercase().collect::<String>();
            out.push_str(chars.as_str());
            out
        }
        None => format!("Town {key}"),
    }
}

/// Chooses a resource for a newly claimed cell from its physical context.
pub fn resource_for_cell(world: &World, cell: u32, seed: u64) -> Option<Resource> {
    if world
        .terrain
        .elevation
        .get(cell as usize)
        .copied()
        .unwrap_or(0.0)
        <= 0.0
    {
        return None;
    }
    use terra_core::biome::Biome;
    use terra_core::rng::{mix3, unit_from_hash};
    let biome = world.biomes.at(cell);
    let coastal = world
        .terrain
        .coastal
        .get(cell as usize)
        .copied()
        .unwrap_or(false);
    let river = world.river.get(cell as usize).copied().unwrap_or(false);
    let roll = unit_from_hash(mix3(seed ^ 0x00c1_7e50, 31, u64::from(cell)));
    let mut candidates: Vec<(Resource, f64)> = Vec::new();
    match biome {
        Biome::Grassland | Biome::Savanna | Biome::Wetland => {
            candidates.push((Resource::Grain, 3.0));
            candidates.push((Resource::Horses, 1.2));
        }
        Biome::TemperateForest
        | Biome::TemperateRainforest
        | Biome::BorealForest
        | Biome::TropicalRainforest
        | Biome::TropicalSeasonalForest
        | Biome::Mangrove => {
            candidates.push((Resource::Timber, 3.0));
            candidates.push((Resource::Spices, 0.8));
        }
        Biome::Mediterranean => {
            candidates.push((Resource::Grain, 1.2));
            candidates.push((Resource::Spices, 0.6));
        }
        Biome::SubtropicalDesert | Biome::TemperateDesert => {
            candidates.push((Resource::Stone, 1.0));
            candidates.push((Resource::Gold, 0.35));
        }
        Biome::Tundra | Biome::Alpine | Biome::Ice => {
            candidates.push((Resource::Stone, 1.4));
        }
        Biome::Ocean | Biome::Lake => return None,
    }
    if coastal || river {
        candidates.push((Resource::Fish, 1.8));
    }
    if world.terrain.elevation[cell as usize] > 1_200.0 {
        candidates.push((Resource::Iron, 1.1));
        candidates.push((Resource::Copper, 0.9));
        candidates.push((Resource::Stone, 0.8));
    }
    if world.terrain.elevation[cell as usize] > 2_200.0 {
        candidates.push((Resource::Gold, 0.4));
    }
    if river {
        candidates.push((Resource::Copper, 0.4));
    }
    let total: f64 = candidates.iter().map(|(_, w)| *w).sum();
    if total <= 0.0 {
        return None;
    }
    let mut target = roll * total;
    for (resource, weight) in candidates {
        target -= weight;
        if target <= 0.0 {
            return Some(resource);
        }
    }
    Some(Resource::Stone)
}

#[cfg(test)]
mod tests {
    use super::*;
    use terra_core::world::{World, WorldConfig};

    fn tiny_world() -> World {
        World::generate(WorldConfig {
            seed: 5,
            subdiv: 2,
            years: 300,
            erosion_iterations: 3,
            ..WorldConfig::default()
        })
    }

    #[test]
    fn traits_normalize() {
        let t = Traits::normalized(1.0, 1.0, 2.0);
        let sum = t.agrarian + t.martial + t.mercantile;
        assert!((sum - 1.0).abs() < 0.2);
        assert_eq!(Traits::normalized(0.0, 0.0, 0.0), Traits::default());
        assert_eq!(Traits::normalized(-1.0, -2.0, -3.0), Traits::default());
        assert!(!t.describe().is_empty());
    }

    #[test]
    fn claim_and_release_keep_the_list_sorted() {
        let mut c = Civ {
            id: 0,
            name: "Test".into(),
            color_index: 0,
            capital: 1,
            capital_name: "Test".into(),
            cells: Vec::new(),
            cities: Vec::new(),
            population: 1000.0,
            food: 2.0,
            cohesion: 0.5,
            military: 10.0,
            treasury: 5.0,
            tech: 0,
            traits: Traits::default(),
            resources: BTreeMap::new(),
            founded_year: 0,
            alive: true,
            collapsed_year: None,
            last_event: None,
            at_war_with: Vec::new(),
            had_golden_age: false,
        };
        assert!(c.claim(9));
        assert!(c.claim(3));
        assert!(!c.claim(9));
        assert!(c.claim(7));
        assert_eq!(c.cells, vec![3, 7, 9]);
        assert!(c.owns(7));
        assert!(!c.owns(8));
        assert!(c.release(7));
        assert!(!c.release(7));
        assert_eq!(c.cells, vec![3, 9]);
        assert_eq!(c.size(), 2);
        assert_eq!(c.density(), 500.0);
    }

    #[test]
    fn cities_and_resources() {
        let mut c = Civ {
            id: 1,
            name: "Cities".into(),
            color_index: 1,
            capital: 0,
            capital_name: "Cap".into(),
            cells: vec![0, 1],
            cities: Vec::new(),
            population: 1000.0,
            food: 1.0,
            cohesion: 0.5,
            military: 5.0,
            treasury: 0.0,
            tech: 2,
            traits: Traits::default(),
            resources: BTreeMap::new(),
            founded_year: 0,
            alive: true,
            collapsed_year: None,
            last_event: None,
            at_war_with: Vec::new(),
            had_golden_age: false,
        };
        c.upsert_city(City {
            cell: 0,
            name: "Cap".into(),
            since: 0,
            population: 100.0,
            capital: true,
        });
        c.upsert_city(City {
            cell: 1,
            name: "Two".into(),
            since: 5,
            population: 300.0,
            capital: false,
        });
        assert_eq!(c.cities.len(), 2);
        assert_eq!(c.largest_city().unwrap().name, "Two");
        assert_eq!(c.seat(), 0);
        c.release(0);
        assert_eq!(c.seat(), 1);
        assert_eq!(c.city_at(1).unwrap().name, "Two");
        assert!(c.take_city(1).is_some());
        assert!(c.take_city(1).is_none());
        assert!(c.discover(Resource::Iron));
        assert!(!c.discover(Resource::Iron));
        assert!(c.resource_military() > 1.0);
        assert!(c.tech_bonus() > 1.0);
    }

    #[test]
    fn names_are_stable_and_varied() {
        assert_eq!(civilization_name(3, 7), civilization_name(3, 7));
        let names: std::collections::BTreeSet<String> =
            (0..24).map(|i| civilization_name(i, 7)).collect();
        assert!(names.len() > 15);
        assert!(!civilization_name(0, 0).is_empty());
        assert!(!city_name(1, 2, 3).is_empty());
        assert_eq!(city_name(1, 2, 3), city_name(1, 2, 3));
    }

    #[test]
    fn resources_depend_on_terrain() {
        let w = tiny_world();
        let mut found = 0;
        for i in 0..w.len() as u32 {
            if w.terrain.elevation[i as usize] > 0.0 {
                if let Some(r) = resource_for_cell(&w, i, 3) {
                    assert!(r.label().len() > 0);
                    found += 1;
                }
            } else {
                assert!(resource_for_cell(&w, i, 3).is_none());
            }
        }
        assert!(found > 0);
        assert_eq!(resource_for_cell(&w, 99_999, 3), None);
    }

    #[test]
    fn resource_metadata() {
        for r in Resource::ALL {
            assert!(!r.code().is_empty());
            assert!((0.0..=1.0).contains(&r.military_value()));
            assert!((0.0..=1.0).contains(&r.trade_value()));
        }
        assert_eq!(TECHNOLOGY_NAMES.len(), MAX_TECH as usize);
        assert_eq!(TECHNOLOGY_THRESHOLDS.len(), MAX_TECH as usize);
    }
}
