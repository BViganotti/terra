//! Civilization simulation.
//!
//! The year loop is a state machine with explicit rules:
//!
//! 1. **Growth** — population approaches the carrying capacity of owned land,
//!    modified by technology and food stores. Food shortages cause famines.
//! 2. **Expansion** — civilizations claim adjacent cells scored by productivity,
//!    fresh water and coastal access.
//! 3. **Resources** — claimed cells may reveal a resource; first discovery is a
//!    chronicle event.
//! 4. **Technology** — population and treasury thresholds unlock technologies.
//! 5. **Diplomacy** — border friction lowers relations, trade raises them.
//! 6. **War** — poor relations can escalate; battles transfer border cells and
//!    cities until exhaustion forces peace.
//! 7. **Collapse** — starvation, low cohesion and lost wars can end a
//!    civilization; survivors migrate.
//!
//! Every transition that changes the map or a civilization's fate is recorded
//! with causes: the civilization's previous event and the measured state that
//! triggered the rule. This is what makes the chronicle traceable rather than
//! decorative.

use std::collections::{BTreeMap, BTreeSet};

use crate::chronicle::EventId;
use terra_core::hash::{hex, Hasher64};
use terra_core::rng::{Rng, Stream};
use terra_core::world::World;

use crate::chronicle::{Cause, Chronicle, ChronicleStats, Event, EventKind, StateFact};
use crate::civ::{
    city_name, civilization_name, resource_for_cell, City, Civ, Resource, Traits, MAX_CITIES,
    MAX_CIVS, MAX_TECH, TECHNOLOGY_NAMES, TECHNOLOGY_THRESHOLDS,
};

/// Simulation parameters.
#[derive(Clone, Copy, Debug)]
pub struct HistoryConfig {
    /// Master seed; usually equal to the world seed.
    pub seed: u64,
    /// Years to simulate, clamped to `100..=20000`.
    pub years: u32,
    /// Initial civilizations, clamped to `2..=16`.
    pub initial_civs: u32,
    /// Aggression multiplier, clamped to `0.2..=3`.
    pub aggression: f64,
    /// Expansion rate multiplier, clamped to `0.2..=3`.
    pub expansion: f64,
    /// Years per simulation step, clamped to `5..=50`.
    pub step_years: u32,
    /// Years between political snapshots, clamped to `50..=1000`.
    pub snapshot_interval: u32,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            years: 10_000,
            initial_civs: 8,
            aggression: 1.0,
            expansion: 1.0,
            step_years: 10,
            snapshot_interval: 100,
        }
    }
}

impl HistoryConfig {
    /// Clamps every field into a supported range.
    pub fn sanitized(mut self) -> Self {
        self.years = self.years.clamp(100, 20_000);
        self.initial_civs = self.initial_civs.clamp(2, 16);
        self.aggression = if self.aggression.is_finite() {
            self.aggression.clamp(0.2, 3.0)
        } else {
            1.0
        };
        self.expansion = if self.expansion.is_finite() {
            self.expansion.clamp(0.2, 3.0)
        } else {
            1.0
        };
        self.step_years = self.step_years.clamp(5, 50);
        self.snapshot_interval = self.snapshot_interval.clamp(50, 1_000);
        self
    }
}

/// A civilization's state at a snapshot year.
#[derive(Clone, Debug, PartialEq)]
pub struct CivSnapshot {
    /// Dense id.
    pub id: u16,
    /// Name.
    pub name: String,
    /// Population.
    pub population: f64,
    /// Owned cells.
    pub cells: u32,
    /// Technology level.
    pub tech: u8,
    /// Cohesion.
    pub cohesion: f64,
    /// Seat cell.
    pub seat: u32,
    /// Alive.
    pub alive: bool,
}

/// Ownership of every cell at a moment in time.
#[derive(Clone, Debug)]
pub struct PoliticalSnapshot {
    /// Year.
    pub year: u32,
    /// Owner id per cell, `u16::MAX` for unowned.
    pub owner: Vec<u16>,
    /// Civilization stats at this year.
    pub civs: Vec<CivSnapshot>,
}

impl PoliticalSnapshot {
    /// Owner of a cell, `None` when unowned.
    pub fn owner_of(&self, cell: u32) -> Option<u16> {
        match self.owner.get(cell as usize).copied() {
            Some(id) if id != u16::MAX => Some(id),
            _ => None,
        }
    }
}

/// One war, retained for reports.
#[derive(Clone, Debug, PartialEq)]
pub struct WarRecord {
    /// Aggressor.
    pub a: u16,
    /// Defender.
    pub b: u16,
    /// Start year.
    pub start: u32,
    /// End year, if concluded.
    pub end: Option<u32>,
    /// Battles fought.
    pub battles: u32,
    /// Cells captured by the aggressor.
    pub captures: u32,
    /// Estimated casualties.
    pub casualties: f64,
    /// Winner, if any.
    pub winner: Option<u16>,
    /// Year-war declaration event.
    pub declared_event: EventId,
}

/// Aggregate history statistics.
#[derive(Clone, Debug, Default)]
pub struct HistoryStats {
    /// Civilizations at the start.
    pub initial_civs: u32,
    /// Civilizations alive at the end.
    pub surviving_civs: u32,
    /// Collapses.
    pub collapses: u32,
    /// Wars declared.
    pub wars: u32,
    /// Battles fought.
    pub battles: u32,
    /// Trade routes opened.
    pub trade_routes: u32,
    /// Cities alive at the end.
    pub cities: u32,
    /// Largest city population.
    pub largest_city: f64,
    /// Migrations.
    pub migrations: u32,
    /// Highest technology reached.
    pub max_tech: u8,
    /// Total events.
    pub events: u32,
    /// Total population at the end.
    pub total_population: f64,
    /// Peak population of any civilization.
    pub peak_population: f64,
    /// Mean technology of surviving civilizations.
    pub mean_tech: f64,
    /// Number of famines.
    pub famines: u32,
    /// Number of natural disasters recorded.
    pub disasters: u32,
    /// Owner cells at the end.
    pub claimed_cells: u32,
}

/// Identifiers for reproduction checks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HistoryHashes {
    /// Hash of the chronicle content.
    pub chronicle: String,
    /// Hash of civilizations and territorial control.
    pub civs: String,
    /// Combined hash.
    pub combined: String,
}

/// The result of a history run.
#[derive(Clone, Debug)]
pub struct History {
    /// Parameters used.
    pub config: HistoryConfig,
    /// World name.
    pub world_name: String,
    /// World hash, for coupling history to physics.
    pub world_hash: String,
    /// Civilizations, including dead ones.
    pub civs: Vec<Civ>,
    /// The chronicle.
    pub chronicle: Chronicle,
    /// Political snapshots in year order.
    pub snapshots: Vec<PoliticalSnapshot>,
    /// Wars, including finished ones.
    pub wars: Vec<WarRecord>,
    /// Summary statistics.
    pub stats: HistoryStats,
    /// Hashes.
    pub hashes: HistoryHashes,
}

impl History {
    /// True when the chronicle passes structural validation.
    pub fn is_sound(&self) -> bool {
        self.chronicle.is_sound()
    }

    /// Problems found by validation.
    pub fn problems(&self) -> Vec<String> {
        self.chronicle.validate()
    }

    /// Chronicle summary.
    pub fn chronicle_stats(&self) -> ChronicleStats {
        self.chronicle.stats()
    }

    /// Snapshot nearest to a year (or the final one).
    pub fn snapshot_at(&self, year: u32) -> Option<&PoliticalSnapshot> {
        let mut best: Option<&PoliticalSnapshot> = None;
        for s in &self.snapshots {
            if s.year <= year {
                best = Some(s);
            } else {
                break;
            }
        }
        best.or_else(|| self.snapshots.first())
    }

    /// Owner of a cell at a year.
    pub fn owner_at(&self, cell: u32, year: u32) -> Option<u16> {
        self.snapshot_at(year).and_then(|s| s.owner_of(cell))
    }

    /// Living civilizations.
    pub fn living(&self) -> impl Iterator<Item = &Civ> {
        self.civs.iter().filter(|c| c.alive)
    }

    /// Folds the history into a stable hash.
    pub fn hash_into(&self, h: &mut Hasher64) {
        h.write_str("history");
        h.write_u64(self.config.seed);
        h.write_u32(self.config.years);
        for e in self.chronicle.events() {
            h.write_u32(e.id);
            h.write_u32(e.year);
            h.write_str_framed(e.kind.code());
            h.write_str_framed(&e.title);
            for c in &e.causes {
                match c {
                    Cause::Event(id) => {
                        h.write_u32(1);
                        h.write_u32(*id);
                    }
                    Cause::State(f) => {
                        h.write_u32(2);
                        h.write_str_framed(f.key);
                        h.write_f64(f.value);
                        h.write_u32(f.cell.unwrap_or(u32::MAX));
                        h.write_u32(u32::from(f.civ.unwrap_or(u16::MAX)));
                    }
                }
            }
        }
        for c in &self.civs {
            h.write_str_framed(&c.name);
            h.write_f64(c.population);
            h.write_u32(c.cells.len() as u32);
            h.write_u32(u32::from(c.tech));
        }
        if let Some(last) = self.snapshots.last() {
            for &o in &last.owner {
                h.write_u32(u32::from(o));
            }
        }
    }
}

#[derive(Clone, Debug)]
struct War {
    a: u16,
    b: u16,
    start: u32,
    battles: u32,
    captures: u32,
    casualties: f64,
    declared_event: EventId,
    last_event: EventId,
    last_battle_year: u32,
}

struct Sim<'a> {
    world: &'a World,
    cfg: HistoryConfig,
    civs: Vec<Civ>,
    owner: Vec<u16>,
    relations: Vec<Vec<i8>>,
    chronicle: Chronicle,
    wars: Vec<War>,
    war_records: Vec<WarRecord>,
    snapshots: Vec<PoliticalSnapshot>,
    world_event: EventId,
    natural_event: EventId,
    trade_pairs: BTreeSet<(u16, u16)>,
    last_expansion_year: Vec<u32>,
    last_famine_year: Vec<u32>,
    last_disaster_year: u32,
    peak_population: Vec<f64>,
    volcanic_cells: Vec<u32>,
    arid_cells: Vec<u32>,
    stats: HistoryStats,
}

/// Runs a history over a world.
pub fn simulate(world: &World, config: HistoryConfig) -> History {
    let mut cfg = config.sanitized();
    // The world's own span is authoritative for reproducibility.
    cfg.years = cfg.years.clamp(100, 20_000);
    if world.config.years != cfg.years {
        cfg.years = world.config.years;
    }
    let capitals = choose_capitals(world, cfg.seed, cfg.initial_civs);
    let mut chronicle = Chronicle::new();
    let world_event = record_world_event(&mut chronicle, world, cfg.seed);
    let mut sim = Sim {
        world,
        cfg,
        civs: Vec::new(),
        owner: vec![u16::MAX; world.len()],
        relations: Vec::new(),
        chronicle,
        wars: Vec::new(),
        war_records: Vec::new(),
        snapshots: Vec::new(),
        world_event,
        natural_event: world_event,
        trade_pairs: BTreeSet::new(),
        last_expansion_year: Vec::new(),
        last_famine_year: Vec::new(),
        last_disaster_year: 0,
        peak_population: Vec::new(),
        volcanic_cells: (0..world.len() as u32)
            .filter(|&c| world.tectonics.volcanism[c as usize] > 0.08)
            .collect(),
        arid_cells: (0..world.len() as u32)
            .filter(|&c| {
                world.terrain.elevation[c as usize] > 0.0
                    && world.climate.moisture.precip_mm[c as usize] < 250.0
            })
            .collect(),
        stats: HistoryStats::default(),
    };

    sim.found_civs(&capitals);
    sim.snapshot(0);
    let mut year = 0u32;
    while year < sim.cfg.years {
        year = (year + sim.cfg.step_years).min(sim.cfg.years);
        sim.step(year);
        if year % sim.cfg.snapshot_interval == 0 || year == sim.cfg.years {
            sim.snapshot(year);
        }
    }

    let stats = sim.finish_stats();
    let mut history = History {
        config: sim.cfg,
        world_name: world.name.clone(),
        world_hash: world.hashes.combined.clone(),
        civs: sim.civs,
        chronicle: sim.chronicle,
        snapshots: sim.snapshots,
        wars: sim
            .war_records
            .into_iter()
            .chain(sim.wars.into_iter().map(|w| WarRecord {
                a: w.a,
                b: w.b,
                start: w.start,
                end: None,
                battles: w.battles,
                captures: w.captures,
                casualties: w.casualties,
                winner: None,
                declared_event: w.declared_event,
            }))
            .collect(),
        stats,
        hashes: HistoryHashes::default(),
    };
    let (chron, civs, combined) = history_hashes(&history);
    history.hashes = HistoryHashes {
        chronicle: chron,
        civs,
        combined,
    };
    history
}

fn record_world_event(chronicle: &mut Chronicle, world: &World, seed: u64) -> EventId {
    let detail = format!(
        "A planet of {} cells forms from seed {}. {} plates drift beneath {} percent ocean. Mean surface temperature is {:.1} C and land receives {:.0} mm of rain per year.",
        world.len(),
        seed,
        world.stats.plates,
        (100.0 - world.stats.land_fraction * 100.0).round(),
        world.stats.mean_temperature_c,
        world.stats.mean_precip_mm
    );
    let causes = vec![
        Cause::State(StateFact::global("seed", seed as f64, "", "world seed")),
        Cause::State(StateFact::global(
            "cells",
            world.len() as f64,
            "cells",
            "grid size",
        )),
        Cause::State(StateFact::global(
            "land_fraction",
            world.stats.land_fraction * 100.0,
            "%",
            "land fraction",
        )),
        Cause::State(StateFact::global(
            "mean_temperature",
            world.stats.mean_temperature_c,
            "C",
            "mean temperature",
        )),
        Cause::State(StateFact::global(
            "mean_precip",
            world.stats.mean_precip_mm,
            "mm/yr",
            "mean precipitation",
        )),
    ];
    chronicle
        .record(Event {
            id: 0,
            year: 0,
            kind: EventKind::WorldFormed,
            title: format!("{} takes shape", world.name),
            detail,
            cell: None,
            civ: None,
            causes,
            severity: 1.0,
            tags: vec!["planet", "origin"],
        })
        .unwrap_or(0)
}

/// Chooses well-separated, highly habitable starting cells.
fn choose_capitals(world: &World, seed: u64, count: u32) -> Vec<u32> {
    let mut candidates: Vec<(u32, f64)> = Vec::new();
    for cell in 0..world.len() as u32 {
        let i = cell as usize;
        if world.terrain.elevation[i] <= 0.0
            || world.lake[i]
            || world.ecosystems.carrying_capacity[i] <= 1.0
        {
            continue;
        }
        let kc = world.ecosystems.carrying_capacity[i];
        let river = if world.river[i] { 60_000.0 } else { 0.0 };
        let coast = if world.terrain.coastal[i] {
            25_000.0
        } else {
            0.0
        };
        let fertility = world.biomes.at(cell).fertility() * 30_000.0;
        let score = kc + river + coast + fertility;
        if score > 0.0 {
            candidates.push((cell, score));
        }
    }
    if candidates.is_empty() {
        return Vec::new();
    }
    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let pool_size = candidates.len().min(320).max(1);
    let pool: Vec<(u32, f64)> = candidates[..pool_size].to_vec();
    let want = (count as usize).min(pool.len()).clamp(2, MAX_CIVS);
    let max_score = pool.iter().map(|p| p.1).fold(0.0f64, f64::max).max(1.0);

    let mut selected: Vec<u32> = Vec::with_capacity(want);
    // Start from the best site, then greedily maximize separation with a small
    // quality bonus. Ties break on cell index.
    selected.push(pool[0].0);
    while selected.len() < want {
        let mut best: Option<(u32, f64)> = None;
        for &(cell, score) in &pool {
            if selected.contains(&cell) {
                continue;
            }
            let min_dist = selected
                .iter()
                .map(|&s| world.sphere.arc(cell, s))
                .fold(f64::INFINITY, f64::min);
            let value = min_dist + (score / max_score) * 0.18;
            if best.map_or(true, |(bc, bv)| {
                value > bv + 1e-12 || (value - bv).abs() < 1e-12 && cell < bc
            }) {
                best = Some((cell, value));
            }
        }
        match best {
            Some((cell, _)) => selected.push(cell),
            None => break,
        }
    }
    let _ = seed;
    selected
}

impl<'a> Sim<'a> {
    fn found_civs(&mut self, capitals: &[u32]) {
        let n = capitals.len();
        self.relations = vec![vec![0i8; n]; n];
        let mut rng = Rng::new(self.cfg.seed, Stream::HISTORY).fork(Stream(0x00f0_1111));
        let mut colors: Vec<u8> = (0..n as u8).collect();
        rng.shuffle(&mut colors);
        for (idx, &capital) in capitals.iter().enumerate() {
            let traits = Traits::normalized(
                rng.range(0.2, 1.0),
                rng.range(0.1, 1.0) * self.cfg.aggression,
                rng.range(0.2, 1.0),
            );
            let capacity = self
                .world
                .ecosystems
                .carrying_capacity
                .get(capital as usize)
                .copied()
                .unwrap_or(10_000.0);
            let population = (capacity * 0.18).clamp(600.0, 60_000.0);
            let name = self.unique_civ_name(idx as u32, self.cfg.seed);
            let mut civ = Civ {
                id: idx as u16,
                name,
                color_index: colors[idx],
                capital,
                capital_name: city_name(idx as u32, 0, self.cfg.seed),
                cells: Vec::new(),
                cities: Vec::new(),
                population,
                food: 3.0,
                cohesion: 0.65,
                military: population * 0.004 + 20.0,
                treasury: 40.0,
                tech: 1,
                traits,
                resources: BTreeMap::new(),
                founded_year: 0,
                alive: true,
                collapsed_year: None,
                last_event: None,
                at_war_with: Vec::new(),
                had_golden_age: false,
            };
            civ.claim(capital);
            self.owner[capital as usize] = idx as u16;
            // Claim a few immediate neighbours to avoid one-cell states.
            let mut neighbours: Vec<(u32, f64)> = self
                .world
                .sphere
                .neighbors(capital)
                .iter()
                .filter(|&&nb| {
                    self.owner[nb as usize] == u16::MAX
                        && self.world.terrain.elevation[nb as usize] > 0.0
                        && !self.world.lake[nb as usize]
                })
                .map(|&nb| {
                    (
                        nb,
                        self.world.ecosystems.carrying_capacity[nb as usize]
                            + if self.world.river[nb as usize] {
                                30_000.0
                            } else {
                                0.0
                            },
                    )
                })
                .collect();
            neighbours.sort_by(|a, b| {
                b.1.partial_cmp(&a.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.0.cmp(&b.0))
            });
            for &(nb, _) in neighbours.iter().take(2) {
                civ.claim(nb);
                self.owner[nb as usize] = idx as u16;
            }
            let pop_share = population * 0.35;
            civ.cities.push(City {
                cell: capital,
                name: civ.capital_name.clone(),
                since: 0,
                population: pop_share,
                capital: true,
            });
            for &(nb, _) in neighbours.iter().take(1) {
                let cname = city_name(idx as u32, 1, self.cfg.seed);
                civ.cities.push(City {
                    cell: nb,
                    name: cname,
                    since: 0,
                    population: population * 0.12,
                    capital: false,
                });
            }
            self.civs.push(civ);
            self.peak_population.push(population);
            self.last_expansion_year.push(0);
            self.last_famine_year.push(0);
        }

        // Establish initial rivalries from cultural distance.
        for i in 0..n {
            for j in 0..n {
                if i == j {
                    continue;
                }
                let mi = self.civs[i].traits.martial;
                let mj = self.civs[j].traits.martial;
                let merchant =
                    (self.civs[i].traits.mercantile + self.civs[j].traits.mercantile) * 0.5;
                let rel = (5.0 - 25.0 * (mi + mj) + 20.0 * merchant).round() as i8;
                self.relations[i][j] = rel.clamp(-55, 30);
            }
        }

        let capitals: Vec<(usize, u32)> = self
            .civs
            .iter()
            .enumerate()
            .map(|(i, c)| (i, c.capital))
            .collect();
        for (idx, capital) in capitals {
            self.record_settlement(idx, capital, 0);
        }
        self.stats.initial_civs = n as u32;
    }

    fn unique_civ_name(&self, index: u32, seed: u64) -> String {
        let base = civilization_name(index, seed);
        if self.civs.iter().all(|c| c.name != base) {
            return base;
        }
        for suffix in 2..50 {
            let candidate = format!("{base} {suffix}");
            if self.civs.iter().all(|c| c.name != candidate) {
                return candidate;
            }
        }
        format!("{base} of the {index}")
    }

    fn record_settlement(&mut self, ci: usize, cell: u32, year: u32) {
        let (civ_name, traits_desc, capital_name, pop) = {
            let civ = &self.civs[ci];
            (
                civ.name.clone(),
                civ.traits.describe(),
                civ.capital_name.clone(),
                civ.population,
            )
        };
        let biome = self.world.biomes.at(cell);
        let river = self
            .world
            .river
            .get(cell as usize)
            .copied()
            .unwrap_or(false);
        let coastal = self
            .world
            .terrain
            .coastal
            .get(cell as usize)
            .copied()
            .unwrap_or(false);
        let causes = vec![
            Cause::Event(self.world_event),
            Cause::State(StateFact::physical(
                "carrying_capacity",
                cell,
                self.world.ecosystems.carrying_capacity[cell as usize],
                "people",
                "carrying capacity",
            )),
            Cause::State(StateFact::physical(
                "precip",
                cell,
                self.world.climate.moisture.precip_mm[cell as usize],
                "mm/yr",
                "precipitation",
            )),
        ];
        let water = if river {
            "beside a river"
        } else if coastal {
            "on the coast"
        } else {
            "in the interior"
        };
        let detail = format!(
            "The {traits_desc} people of {civ_name} found {capital_name} {water}, {}, in {}. Population is about {pop:.0}.",
            if biome.is_forest() { "in forest" } else { "in open country" },
            biome.label().to_lowercase()
        );
        let _ = self.record(
            Some(ci),
            year,
            EventKind::SettlementFounded,
            format!("{capital_name} is founded"),
            detail,
            Some(cell),
            causes,
            0.55,
        );
    }

    fn record(
        &mut self,
        civ: Option<usize>,
        year: u32,
        kind: EventKind,
        title: String,
        detail: String,
        cell: Option<u32>,
        causes: Vec<Cause>,
        severity: f64,
    ) -> Option<EventId> {
        let civ_id = civ.map(|i| i as u16);
        let event = Event {
            id: 0,
            year,
            kind,
            title,
            detail,
            cell,
            civ: civ_id,
            causes,
            severity,
            tags: Vec::new(),
        };
        match self.chronicle.record(event) {
            Ok(id) => {
                if let Some(i) = civ {
                    if let Some(c) = self.civs.get_mut(i) {
                        c.last_event = Some(id);
                    }
                }
                Some(id)
            }
            Err(_) => None,
        }
    }

    fn last_cause(&self, ci: usize) -> EventId {
        self.civs
            .get(ci)
            .and_then(|c| c.last_event)
            .unwrap_or(self.world_event)
    }

    fn step(&mut self, year: u32) {
        let alive: Vec<usize> = self
            .civs
            .iter()
            .enumerate()
            .filter(|(_, c)| c.alive)
            .map(|(i, _)| i)
            .collect();
        let mut rng =
            Rng::new(self.cfg.seed, Stream::HISTORY).fork(Stream(0x7100_0000 + u64::from(year)));

        for &ci in &alive {
            self.grow(ci, year, &mut rng);
        }
        for &ci in &alive {
            if self.civs[ci].alive && year >= self.last_expansion_year[ci] + 20 {
                self.expand(ci, year, &mut rng);
                self.last_expansion_year[ci] = year;
            }
        }
        for &ci in &alive {
            if self.civs[ci].alive {
                self.tech(ci, year, &mut rng);
                self.famine(ci, year, &mut rng);
                self.golden_age(ci, year);
                self.maybe_collapse(ci, year, &mut rng);
            }
        }
        self.diplomacy(year, &mut rng);
        self.wars_step(year, &mut rng);
        self.epidemic(year, &mut rng);
        self.natural(year, &mut rng);
        for &ci in &alive {
            if self.civs[ci].alive && self.civs[ci].population < 1.0 {
                self.collapse(ci, year, "the last inhabitants scatter");
            }
        }
    }

    fn grow(&mut self, ci: usize, _year: u32, rng: &mut Rng) {
        let capacity = self.civs[ci].capacity(self.world) * self.civs[ci].tech_bonus();
        let pop = self.civs[ci].population;
        let traits = self.civs[ci].traits;
        let war_penalty = if self.civs[ci].at_war() { 0.6 } else { 1.0 };
        let rate = 0.018 * (0.7 + traits.agrarian) * war_penalty;
        let dt = f64::from(self.cfg.step_years);
        let growth = pop * rate * (1.0 - (pop / capacity.max(1.0)).min(1.5)) * dt;
        let noise = rng.range(0.9, 1.1);
        let new_pop = (pop + growth * noise).clamp(0.0, capacity.max(0.0));
        self.civs[ci].population = new_pop.max(0.0);

        let production = capacity * 0.05 * (0.6 + 0.8 * traits.agrarian);
        let consumption = new_pop * 0.043;
        let war_drain = if self.civs[ci].at_war() {
            0.04 * dt / 10.0
        } else {
            0.0
        };
        let food_delta = (production - consumption) * dt / 1_000.0 - war_drain;
        self.civs[ci].food = (self.civs[ci].food + food_delta).clamp(0.0, 24.0);

        let target = (0.5 + if self.civs[ci].food > 2.0 { 0.12 } else { 0.0 }
            - if self.civs[ci].at_war() { 0.25 } else { 0.0 }
            - (self.civs[ci].cells.len() as f64 / 600.0).min(0.3))
        .clamp(0.02, 0.95);
        let c = self.civs[ci].cohesion;
        self.civs[ci].cohesion = (c + (target - c) * 0.08).clamp(0.0, 1.0);

        let income = new_pop
            * 0.000_45
            * (1.0 + traits.mercantile * 2.0)
            * dt
            * self.civs[ci].resource_trade();
        self.civs[ci].treasury = (self.civs[ci].treasury + income).min(1e7);

        let military_target = new_pop
            * 0.004
            * (1.0 + traits.martial)
            * self.civs[ci].tech_bonus()
            * self.civs[ci].resource_military();
        let m = self.civs[ci].military;
        self.civs[ci].military = (m + (military_target - m) * 0.06).max(1.0);

        if new_pop > self.peak_population[ci] {
            self.peak_population[ci] = new_pop;
        }
        // Keep city populations roughly proportional.
        let share = if self.civs[ci].cities.is_empty() {
            0.0
        } else {
            0.6 / self.civs[ci].cities.len() as f64
        };
        for city in self.civs[ci].cities.iter_mut() {
            city.population = (city.population * 0.7 + new_pop * share * 0.3).max(1.0);
        }
    }

    fn expand(&mut self, ci: usize, year: u32, rng: &mut Rng) {
        let civ_id = ci as u16;
        let mut frontier: BTreeSet<u32> = BTreeSet::new();
        for &cell in &self.civs[ci].cells {
            for &nb in self.world.sphere.neighbors(cell) {
                if self.owner[nb as usize] == u16::MAX
                    && self.world.terrain.elevation[nb as usize] > 0.0
                    && !self.world.lake[nb as usize]
                {
                    frontier.insert(nb);
                }
            }
        }
        if frontier.is_empty() {
            return;
        }
        let mut scored: Vec<(u32, f64)> = frontier
            .into_iter()
            .map(|cell| {
                let kc = self.world.ecosystems.carrying_capacity[cell as usize];
                let river = if self.world.river[cell as usize] {
                    40_000.0
                } else {
                    0.0
                };
                let coast = if self.world.terrain.coastal[cell as usize] {
                    15_000.0
                } else {
                    0.0
                };
                let adjacent = self
                    .world
                    .sphere
                    .neighbors(cell)
                    .iter()
                    .filter(|&&nb| self.owner[nb as usize] == civ_id)
                    .count() as f64;
                let jitter = rng.range(0.85, 1.15);
                (cell, (kc + river + coast) * (1.0 + adjacent * 0.2) * jitter)
            })
            .collect();
        scored.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.0.cmp(&b.0))
        });
        let size = self.civs[ci].cells.len() as f64;
        let pop = self.civs[ci].population;
        let allowed =
            ((1.0 + self.cfg.expansion * 2.0 + pop / 120_000.0).min(5.0 + size / 500.0)) as usize;
        let mut claimed = 0usize;
        let mut first_cell = None;
        let mut new_resources: Vec<Resource> = Vec::new();
        for (cell, _) in scored.into_iter().take(allowed.max(1) * 3) {
            if claimed >= allowed.max(1) {
                break;
            }
            if self.owner[cell as usize] != u16::MAX {
                continue;
            }
            self.owner[cell as usize] = civ_id;
            let newly_claimed = self.civs[ci].claim(cell);
            if !newly_claimed {
                continue;
            }
            if first_cell.is_none() {
                first_cell = Some(cell);
            }
            claimed += 1;
            if let Some(resource) = resource_for_cell(self.world, cell, self.cfg.seed) {
                if self.civs[ci].discover(resource) {
                    new_resources.push(resource);
                }
            }
            // Occasional new town.
            if claimed % 4 == 0 && self.civs[ci].cities.len() < MAX_CITIES {
                let idx = self.civs[ci].cities.len() as u32;
                let name = city_name(ci as u32, idx, self.cfg.seed);
                self.civs[ci].cities.push(City {
                    cell,
                    name,
                    since: year,
                    population: pop * 0.01 + 20.0,
                    capital: false,
                });
            }
        }
        if claimed == 0 {
            return;
        }
        let Some(cell) = first_cell else {
            return;
        };
        let cause = self.last_cause(ci);
        let civ_name = self.civs[ci].name.clone();
        let biome = self.world.biomes.at(cell);
        let detail = format!(
            "{} claims {} new cells, including {} in {}. Population {} supports the expansion.",
            civ_name,
            claimed,
            biome.label().to_lowercase(),
            biome.label().to_lowercase(),
            pop
        );
        let id = self.record(
            Some(ci),
            year,
            EventKind::Expansion,
            format!("{civ_name} expands along its frontier"),
            detail,
            Some(cell),
            vec![
                Cause::Event(cause),
                Cause::State(StateFact::physical(
                    "carrying_capacity",
                    cell,
                    self.world.ecosystems.carrying_capacity[cell as usize],
                    "people",
                    "carrying capacity",
                )),
                Cause::State(StateFact::social(
                    "population",
                    civ_id,
                    pop,
                    "people",
                    "population",
                )),
            ],
            0.3,
        );
        if let Some(id) = id {
            for resource in new_resources {
                let res_name = resource.label().to_string();
                let _ = self.record(
                    Some(ci),
                    year,
                    EventKind::ResourceDiscovered,
                    format!("{civ_name} finds {res_name}"),
                    format!(
                        "Surveyors working the newly claimed frontier discover {} deposits. The resource enters the {} economy.",
                        res_name,
                        self.civs[ci].traits.describe()
                    ),
                    Some(cell),
                    vec![
                        Cause::Event(id),
                        Cause::State(StateFact::physical(
                            "resource_abundance",
                            cell,
                            1.0,
                            "deposits",
                            "deposits",
                        )),
                    ],
                    0.25,
                );
            }
        }
    }

    fn tech(&mut self, ci: usize, year: u32, rng: &mut Rng) {
        let tech = self.civs[ci].tech;
        if tech >= MAX_TECH {
            return;
        }
        let threshold = TECHNOLOGY_THRESHOLDS[tech as usize];
        if self.civs[ci].population < threshold || self.civs[ci].treasury < f64::from(tech) * 60.0 {
            return;
        }
        if !rng.chance(0.22) {
            return;
        }
        self.civs[ci].tech = tech + 1;
        let name = TECHNOLOGY_NAMES[tech as usize];
        let civ_name = self.civs[ci].name.clone();
        let cause = self.last_cause(ci);
        let pop = self.civs[ci].population;
        let treasury = self.civs[ci].treasury;
        let _ = self.record(
            Some(ci),
            year,
            EventKind::TechDiscovered,
            format!("{civ_name} discovers {name}"),
            format!(
                "Scholars and craftspeople of {civ_name} master {name}. A population of {pop:.0} and a treasury of {treasury:.0} could support the work.",
            ),
            None,
            vec![
                Cause::Event(cause),
                Cause::State(StateFact::social(
                    "population",
                    ci as u16,
                    pop,
                    "people",
                    "population",
                )),
                Cause::State(StateFact::social(
                    "treasury",
                    ci as u16,
                    treasury,
                    "gold",
                    "treasury",
                )),
            ],
            0.4,
        );
    }

    fn famine(&mut self, ci: usize, year: u32, rng: &mut Rng) {
        if year < self.last_famine_year[ci] + 200 {
            return;
        }
        let food = self.civs[ci].food;
        let cohesion = self.civs[ci].cohesion;
        if food > 0.4 || cohesion > 0.48 {
            return;
        }
        if !rng.chance(0.05) {
            return;
        }
        self.last_famine_year[ci] = year;
        let civ_name = self.civs[ci].name.clone();
        let pop = self.civs[ci].population;
        let cause = self.last_cause(ci);
        self.civs[ci].population = (pop * 0.8).max(0.0);
        self.civs[ci].cohesion = (cohesion - 0.12).max(0.0);
        self.civs[ci].food = 0.0;
        self.stats.famines += 1;
        let cell = self.civs[ci].capital;
        let _ = self.record(
            Some(ci),
            year,
            EventKind::Famine,
            format!("Famine strikes {civ_name}"),
            format!(
                "Stored food falls to {food:.2} years and cohesion to {cohesion:.2}. Roughly twelve percent of the population is lost before the harvest recovers."
            ),
            Some(cell),
            vec![
                Cause::Event(cause),
                Cause::State(StateFact::social(
                    "food",
                    ci as u16,
                    food,
                    "years",
                    "stored food",
                )),
                Cause::State(StateFact::physical(
                    "precip",
                    cell,
                    self.world.climate.moisture.precip_mm[cell as usize],
                    "mm/yr",
                    "precipitation",
                )),
            ],
            0.65,
        );
    }

    fn golden_age(&mut self, ci: usize, year: u32) {
        if self.civs[ci].had_golden_age || self.civs[ci].tech < 4 || self.civs[ci].cohesion < 0.78 {
            return;
        }
        self.civs[ci].had_golden_age = true;
        let civ_name = self.civs[ci].name.clone();
        let tech = self.civs[ci].tech;
        let cohesion = self.civs[ci].cohesion;
        let pop = self.civs[ci].population;
        let cause = self.last_cause(ci);
        let _ = self.record(
            Some(ci),
            year,
            EventKind::GoldenAge,
            format!("A golden age dawns in {civ_name}"),
            format!(
                "{civ_name} enters a period of confidence: cohesion {cohesion:.2}, technology level {tech}, population {pop:.0}. Monuments and scholarship flourish."
            ),
            None,
            vec![
                Cause::Event(cause),
                Cause::State(StateFact::social(
                    "cohesion",
                    ci as u16,
                    cohesion,
                    "",
                    "cohesion",
                )),
                Cause::State(StateFact::social(
                    "technology",
                    ci as u16,
                    f64::from(tech),
                    "level",
                    "technology",
                )),
            ],
            0.5,
        );
    }

    fn maybe_collapse(&mut self, ci: usize, year: u32, rng: &mut Rng) {
        let civ = &self.civs[ci];
        if !civ.alive {
            return;
        }
        let peak = self.peak_population[ci].max(1.0);
        let starving = civ.population < peak * 0.25 && civ.food < 0.35;
        let broken = civ.cohesion < 0.2 && civ.population < peak * 0.5;
        let overrun = civ.cells.is_empty()
            || (civ.cells.len() < 3 && !civ.owns(civ.capital) && peak > 5_000.0);
        let overstretched = civ.cells.len() > 120 && civ.cohesion < 0.3;
        let revolt = civ.cohesion < 0.12 && civ.cells.len() > 8;
        if !(starving || broken || overrun || overstretched || revolt) {
            return;
        }
        let chance = if overstretched {
            0.05
        } else if revolt {
            0.15
        } else {
            0.4
        };
        if !rng.chance(chance) {
            return;
        }
        let reason = if overrun {
            "its last territory is lost"
        } else if revolt {
            "its provinces reject the capital"
        } else if overstretched {
            "distance and factionalism tear it apart"
        } else if starving {
            "repeated harvest failures hollow it out"
        } else {
            "its society loses the will to continue"
        };
        self.collapse(ci, year, reason);
    }

    fn collapse(&mut self, ci: usize, year: u32, reason: &str) {
        if !self.civs[ci].alive {
            return;
        }
        let civ_name = self.civs[ci].name.clone();
        let pop = self.civs[ci].population;
        let cohesion = self.civs[ci].cohesion;
        let cause = self.last_cause(ci);
        let cell = self.civs[ci].seat();
        let cells: Vec<u32> = self.civs[ci].cells.clone();
        let fragility = cells
            .iter()
            .map(|&c| self.world.ecosystems.fragility[c as usize])
            .fold(0.0f64, f64::max);
        let id = self.record(
            Some(ci),
            year,
            EventKind::Collapse,
            format!("{civ_name} collapses"),
            format!(
                "{civ_name} ends after {reason}. Population was {pop:.0}; cohesion {cohesion:.2}. Its lands fall out of central control."
            ),
            Some(cell),
            vec![
                Cause::Event(cause),
                Cause::State(StateFact::social(
                    "population",
                    ci as u16,
                    pop,
                    "people",
                    "population",
                )),
                Cause::State(StateFact::social(
                    "cohesion",
                    ci as u16,
                    cohesion,
                    "",
                    "cohesion",
                )),
            ],
            0.9,
        );
        self.civs[ci].alive = false;
        self.civs[ci].collapsed_year = Some(year);
        self.stats.collapses += 1;
        // Wars end when a participant ceases to exist.
        self.civs[ci].at_war_with.clear();
        for w in &mut self.wars {
            if w.a == ci as u16 || w.b == ci as u16 {
                w.last_event = id.unwrap_or(w.last_event);
            }
        }

        // Neighbours absorb survivors.
        let mut target: Option<usize> = None;
        let mut best = f64::MIN;
        for (ti, t) in self.civs.iter().enumerate() {
            if ti == ci || !t.alive {
                continue;
            }
            let border = cells.iter().any(|&c| {
                self.world
                    .sphere
                    .neighbors(c)
                    .iter()
                    .any(|&nb| self.owner[nb as usize] == ti as u16)
            });
            if border {
                let score = t.capacity(self.world) + t.population;
                if score > best {
                    best = score;
                    target = Some(ti);
                }
            }
        }
        if let Some(ti) = target {
            let survivors = pop * 0.08;
            self.civs[ti].population += survivors;
            self.stats.migrations += 1;
            let target_cell = self.civs[ti].capital;
            let cause = id.unwrap_or(self.world_event);
            let target_name = self.civs[ti].name.clone();
            let _ = self.record(
                Some(ti),
                year,
                EventKind::Migration,
                format!("Refugees reach {target_name}"),
                format!(
                    "About {survivors:.0} survivors of {civ_name} settle among {target_name}. Their skills and grievances travel with them."
                ),
                Some(target_cell),
                vec![
                    Cause::Event(cause),
                    Cause::State(StateFact::physical(
                        "carrying_capacity",
                        target_cell,
                        self.world.ecosystems.carrying_capacity[target_cell as usize],
                        "people",
                        "carrying capacity",
                    )),
                ],
                0.5,
            );
        }
        if fragility > 0.72 {
            let fragile_cell = cells
                .iter()
                .copied()
                .max_by(|&a, &b| {
                    self.world.ecosystems.fragility[a as usize]
                        .partial_cmp(&self.world.ecosystems.fragility[b as usize])
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .unwrap_or(cell);
            let species = self.world.ecosystems.species_richness[fragile_cell as usize];
            let cause = id.unwrap_or(self.world_event);
            let _ = self.record(
                None,
                year,
                EventKind::Extinction,
                "A fragile lineage disappears".to_string(),
                format!(
                    "With the fall of {civ_name}, a small endemic population of about {species} species in an isolated patch loses its protection."
                ),
                Some(fragile_cell),
                vec![
                    Cause::Event(cause),
                    Cause::State(StateFact::physical(
                        "species_richness",
                        fragile_cell,
                        f64::from(species),
                        "species",
                        "species richness",
                    )),
                    Cause::State(StateFact::physical(
                        "fragility",
                        fragile_cell,
                        self.world.ecosystems.fragility[fragile_cell as usize],
                        "",
                        "habitat fragility",
                    )),
                ],
                0.6,
            );
        }
        // Release territory.
        for cell in cells {
            if self.owner[cell as usize] == ci as u16 {
                self.owner[cell as usize] = u16::MAX;
            }
        }
        self.civs[ci].cells.clear();
    }

    fn diplomacy(&mut self, year: u32, rng: &mut Rng) {
        let alive: Vec<usize> = self
            .civs
            .iter()
            .enumerate()
            .filter(|(_, c)| c.alive)
            .map(|(i, _)| i)
            .collect();
        if alive.len() < 2 {
            return;
        }
        // Collect adjacent pairs once from the ownership grid.
        let mut borders: BTreeSet<(u16, u16)> = BTreeSet::new();
        for cell in 0..self.owner.len() as u32 {
            let a = self.owner[cell as usize];
            if a == u16::MAX {
                continue;
            }
            for &nb in self.world.sphere.neighbors(cell) {
                let b = self.owner[nb as usize];
                if b != u16::MAX && b != a {
                    let pair = if a < b { (a, b) } else { (b, a) };
                    borders.insert(pair);
                }
            }
        }
        for &(a, b) in &borders {
            let ai = a as usize;
            let bi = b as usize;
            if !self.civs[ai].alive || !self.civs[bi].alive {
                continue;
            }
            let at_war = self.civs[ai].at_war_with.contains(&b);
            let traded = self.trade_pairs.contains(&(a, b));
            let mut delta: i16 = if at_war {
                -2
            } else if traded {
                1
            } else if self.civs[ai].traits.martial + self.civs[bi].traits.martial > 0.9 {
                -1
            } else {
                0
            };
            if !at_war && rng.chance(0.015) {
                // Border incident.
                delta -= 24;
            } else if !at_war && rng.chance(0.012) {
                delta += 14;
            }
            let rel = i16::from(self.relations[ai][bi]) + delta;
            self.relations[ai][bi] = rel.clamp(-100, 100) as i8;
            self.relations[bi][ai] = self.relations[ai][bi];

            if !at_war
                && !traded
                && self.relations[ai][bi] >= 5
                && rng.chance(0.03 * self.cfg.expansion.min(2.0))
            {
                self.open_trade(ai, bi, year);
            }
            // Resource competition and crowding add pressure beyond the
            // diplomatic score, so rivalries can escalate before relations
            // reach their floor.
            let pressure =
                ((self.civs[ai].cells.len() + self.civs[bi].cells.len()) as f64 / 300.0).min(3.0);
            if !at_war
                && self.relations[ai][bi] <= -18
                && self.wars.len() < 8
                && rng.chance(0.008 * self.cfg.aggression * (1.0 + pressure))
            {
                self.declare_war(ai, bi, year);
            }
        }
    }

    fn open_trade(&mut self, ai: usize, bi: usize, year: u32) {
        let pair = (ai as u16, bi as u16);
        if !self.trade_pairs.insert(pair) {
            return;
        }
        self.relations[ai][bi] = (i16::from(self.relations[ai][bi]) + 20).clamp(-100, 100) as i8;
        self.relations[bi][ai] = self.relations[ai][bi];
        self.stats.trade_routes += 1;
        let a_name = self.civs[ai].name.clone();
        let b_name = self.civs[bi].name.clone();
        let a_cause = self.last_cause(ai);
        let b_event = self.civs[bi].last_event.unwrap_or(self.world_event);
        let a_trade = self.civs[ai].resource_trade();
        let b_trade = self.civs[bi].resource_trade();
        let cell = self.civs[ai].capital;
        let _ = self.record(
            Some(ai),
            year,
            EventKind::TradeRouteOpened,
            format!("{a_name} and {b_name} open a trade route"),
            format!(
                "Merchants link {a_name} and {b_name}. Resource endowments ({a_trade:.2} and {b_trade:.2}) make the exchange worthwhile, and relations improve."
            ),
            Some(cell),
            vec![
                Cause::Event(a_cause),
                Cause::Event(b_event),
                Cause::State(StateFact::social(
                    "relation",
                    ai as u16,
                    f64::from(self.relations[ai][bi]),
                    "",
                    "relations",
                )),
            ],
            0.35,
        );
        self.civs[ai].treasury += 120.0;
        self.civs[bi].treasury += 120.0;
    }

    fn declare_war(&mut self, ai: usize, bi: usize, year: u32) {
        let pair = (ai as u16, bi as u16);
        let event = self.last_cause(ai);
        let relation = self.relations[ai][bi];
        let a_name = self.civs[ai].name.clone();
        let b_name = self.civs[bi].name.clone();
        let a_mil = self.civs[ai].military;
        let b_mil = self.civs[bi].military;
        let cell = self.civs[ai].capital;
        let id = self.record(
            Some(ai),
            year,
            EventKind::WarDeclared,
            format!("{a_name} declares war on {b_name}"),
            format!(
                "Relations between {a_name} and {b_name} collapse to {relation}. With {a_mil:.0} soldiers against {b_mil:.0}, the frontier becomes a battlefield."
            ),
            Some(cell),
            vec![
                Cause::Event(event),
                Cause::State(StateFact::social(
                    "relation",
                    ai as u16,
                    f64::from(relation),
                    "",
                    "relations",
                )),
                Cause::State(StateFact::social(
                    "military",
                    ai as u16,
                    a_mil,
                    "soldiers",
                    "military strength",
                )),
            ],
            0.75,
        );
        let Some(declared) = id else {
            return;
        };
        self.civs[ai].at_war_with.push(bi as u16);
        self.civs[bi].at_war_with.push(ai as u16);
        self.relations[ai][bi] = (i16::from(self.relations[ai][bi]) - 25).clamp(-100, 100) as i8;
        self.relations[bi][ai] = self.relations[ai][bi];
        self.wars.push(War {
            a: ai as u16,
            b: bi as u16,
            start: year,
            battles: 0,
            captures: 0,
            casualties: 0.0,
            declared_event: declared,
            last_event: declared,
            last_battle_year: year,
        });
        self.stats.wars += 1;
        let _ = pair;
    }

    fn wars_step(&mut self, year: u32, rng: &mut Rng) {
        let mut ended: Vec<usize> = Vec::new();
        for wi in 0..self.wars.len() {
            let (a, b) = (self.wars[wi].a as usize, self.wars[wi].b as usize);
            if !self.civs[a].alive || !self.civs[b].alive {
                ended.push(wi);
                continue;
            }
            let war_age = year.saturating_sub(self.wars[wi].start);
            if war_age >= 10 && year >= self.wars[wi].last_battle_year + 10 && rng.chance(0.5) {
                self.battle(wi, year, rng);
            }
            let exhausted = war_age > 60
                || self.civs[a].treasury < 5.0
                || self.civs[b].treasury < 5.0
                || self.civs[a].population < self.peak_population[a] * 0.45
                || self.civs[b].population < self.peak_population[b] * 0.45;
            if exhausted && war_age >= 10 {
                ended.push(wi);
            }
        }
        ended.sort_unstable();
        ended.dedup();
        for wi in ended.into_iter().rev() {
            if wi < self.wars.len() {
                self.end_war(wi, year);
            }
        }
    }

    fn battle(&mut self, wi: usize, year: u32, rng: &mut Rng) {
        let (a, b) = (self.wars[wi].a as usize, self.wars[wi].b as usize);
        let a_str = self.civs[a].military
            * self.civs[a].tech_bonus()
            * self.civs[a].resource_military()
            * rng.range(0.75, 1.25);
        let b_str = self.civs[b].military
            * self.civs[b].tech_bonus()
            * self.civs[b].resource_military()
            * rng.range(0.75, 1.25);
        let (winner, loser) = if a_str >= b_str { (a, b) } else { (b, a) };
        let casualties = self.civs[loser].population * rng.range(0.01, 0.04)
            + self.civs[winner].population * rng.range(0.002, 0.012);
        self.civs[loser].population = (self.civs[loser].population - casualties * 0.75).max(0.0);
        self.civs[winner].population = (self.civs[winner].population - casualties * 0.25).max(0.0);
        self.civs[loser].military *= 0.94;
        self.civs[winner].military *= 1.02;
        self.civs[loser].cohesion = (self.civs[loser].cohesion - 0.06).max(0.0);
        self.civs[winner].cohesion = (self.civs[winner].cohesion + 0.03).min(1.0);
        self.wars[wi].battles += 1;
        self.wars[wi].casualties += casualties;
        self.wars[wi].last_battle_year = year;
        self.stats.battles += 1;

        // Choose a border cell of the loser adjacent to the winner.
        let mut target: Option<u32> = None;
        let mut target_score = f64::MIN;
        for &cell in &self.civs[loser].cells {
            let adjacent = self
                .world
                .sphere
                .neighbors(cell)
                .iter()
                .filter(|&&nb| self.owner[nb as usize] == winner as u16)
                .count();
            if adjacent == 0 {
                continue;
            }
            let city_bonus = if self.civs[loser].city_at(cell).is_some() {
                5.0
            } else {
                0.0
            };
            let score = city_bonus
                + adjacent as f64
                + self.world.ecosystems.carrying_capacity[cell as usize] / 100_000.0;
            if score > target_score {
                target_score = score;
                target = Some(cell);
            }
        }
        let winner_name = self.civs[winner].name.clone();
        let loser_name = self.civs[loser].name.clone();
        let cause = self.wars[wi].last_event;
        let war_declared = self.wars[wi].declared_event;
        let cell = target.unwrap_or(self.civs[loser].capital);
        let battle_id = self.record(
            None,
            year,
            EventKind::Battle,
            format!("{winner_name} prevails in battle against {loser_name}"),
            format!(
                "An engagement leaves about {casualties:.0} dead. {winner_name} holds the field; {loser_name} falls back."
            ),
            Some(cell),
            vec![
                Cause::Event(cause),
                Cause::Event(war_declared),
                Cause::State(StateFact::social(
                    "military",
                    winner as u16,
                    self.civs[winner].military,
                    "soldiers",
                    "military strength",
                )),
            ],
            0.6,
        );
        if let Some(id) = battle_id {
            self.wars[wi].last_event = id;
            if let Some(c) = self.civs.get_mut(winner) {
                c.last_event = Some(id);
            }
        }

        if let Some(cell) = target {
            let captured_city = self.civs[loser].take_city(cell);
            self.civs[loser].release(cell);
            if self.owner[cell as usize] == loser as u16 {
                self.owner[cell as usize] = winner as u16;
            }
            self.civs[winner].claim(cell);
            self.wars[wi].captures += 1;
            if let Some(mut city) = captured_city {
                city.since = year;
                let city_name = city.name.clone();
                let capital = city.capital;
                city.capital = false;
                self.civs[winner].upsert_city(city);
                let cause = self.wars[wi].last_event;
                let _ = self.record(
                    Some(winner),
                    year,
                    EventKind::CityCaptured,
                    format!("{winner_name} captures {city_name}"),
                    format!(
                        "{winner_name} takes {city_name} from {loser_name}. {}",
                        if capital {
                            "It was the enemy capital, a blow to legitimacy as much as to trade."
                        } else {
                            "The town changes hands with its stores and workshops intact."
                        }
                    ),
                    Some(cell),
                    vec![
                        Cause::Event(cause),
                        Cause::State(StateFact::physical(
                            "carrying_capacity",
                            cell,
                            self.world.ecosystems.carrying_capacity[cell as usize],
                            "people",
                            "carrying capacity",
                        )),
                    ],
                    0.7,
                );
            }
        }
        if self.civs[loser].cells.is_empty() {
            self.collapse(loser, year, "its last stronghold is taken");
        }
    }

    fn end_war(&mut self, wi: usize, year: u32) {
        let war = self.wars.remove(wi);
        let (a, b) = (war.a as usize, war.b as usize);
        self.civs[a].at_war_with.retain(|&x| x != war.b);
        self.civs[b].at_war_with.retain(|&x| x != war.a);
        self.relations[a][b] = -20;
        self.relations[b][a] = -20;
        let a_name = self.civs[a].name.clone();
        let b_name = self.civs[b].name.clone();
        let winner = if self.civs[a].cells.len() > self.civs[b].cells.len() {
            Some(a)
        } else if self.civs[b].cells.len() > self.civs[a].cells.len() {
            Some(b)
        } else {
            None
        };
        let winner_name = winner.map(|w| self.civs[w].name.clone());
        let casualties = war.casualties;
        let cause = war.last_event;
        let cell = self.civs[a].capital;
        let id = self.record(
            Some(a),
            year,
            EventKind::PeaceTreaty,
            format!("{a_name} and {b_name} make peace"),
            format!(
                "After {} battles and about {casualties:.0} dead, the war between {a_name} and {b_name} ends. {}",
                war.battles,
                winner_name
                    .map(|w| format!("{w} holds the stronger position."))
                    .unwrap_or_else(|| "Neither side can impose terms.".to_string())
            ),
            Some(cell),
            vec![
                Cause::Event(war.declared_event),
                Cause::Event(cause),
                Cause::State(StateFact::social(
                    "casualties",
                    a as u16,
                    casualties,
                    "people",
                    "war dead",
                )),
            ],
            0.45,
        );
        self.war_records.push(WarRecord {
            a: war.a,
            b: war.b,
            start: war.start,
            end: Some(year),
            battles: war.battles,
            captures: war.captures,
            casualties,
            winner: winner.map(|w| w as u16),
            declared_event: war.declared_event,
        });
        let _ = id;
    }

    fn epidemic(&mut self, year: u32, rng: &mut Rng) {
        // Crowded, connected societies suffer outbreaks; this is a separate
        // rule from famine so its cause chain can cite trade and density.
        if self.trade_pairs.is_empty() || !rng.chance(0.006) {
            return;
        }
        let mut worst: Option<(usize, f64)> = None;
        for (i, c) in self.civs.iter().enumerate() {
            if c.alive && c.population > 1_500_000.0 {
                let density = c.density();
                if worst.map_or(true, |(_, d)| density > d) {
                    worst = Some((i, density));
                }
            }
        }
        let Some((ci, density)) = worst else {
            return;
        };
        if year < self.last_famine_year[ci] + 80 {
            return;
        }
        self.last_famine_year[ci] = year;
        let civ_name = self.civs[ci].name.clone();
        let pop = self.civs[ci].population;
        let cause = self.last_cause(ci);
        self.civs[ci].population = (pop * 0.88).max(0.0);
        self.civs[ci].cohesion = (self.civs[ci].cohesion - 0.08).max(0.0);
        self.stats.disasters += 1;
        let _ = self.record(
            Some(ci),
            year,
            EventKind::Epidemic,
            format!("Plague spreads through {civ_name}"),
            format!(
                "A disease moves along the trade routes. Density of {density:.0} people per cell and the size of the population accelerate its spread."
            ),
            None,
            vec![
                Cause::Event(cause),
                Cause::State(StateFact::social(
                    "population",
                    ci as u16,
                    pop,
                    "people",
                    "population",
                )),
                Cause::State(StateFact::social(
                    "density",
                    ci as u16,
                    density,
                    "people/cell",
                    "population density",
                )),
            ],
            0.7,
        );
    }

    fn natural(&mut self, year: u32, rng: &mut Rng) {
        if year < self.last_disaster_year + 120 {
            return;
        }
        if !rng.chance(0.5) {
            return;
        }
        self.last_disaster_year = year;
        let roll = rng.below(100);
        let cause = self.natural_event;
        if roll < 28 {
            let cell = if self.volcanic_cells.is_empty() {
                self.civs
                    .iter()
                    .find(|c| c.alive)
                    .map(|c| c.capital)
                    .unwrap_or(0)
            } else {
                self.volcanic_cells[(rng.below(self.volcanic_cells.len() as u64)) as usize]
            };
            let volcanism = self
                .world
                .tectonics
                .volcanism
                .get(cell as usize)
                .copied()
                .unwrap_or(0.0);
            let elev = self.world.terrain.elevation[cell as usize];
            let id = self.record(
                None,
                year,
                EventKind::Eruption,
                "A volcano awakens".to_string(),
                format!(
                    "Ash falls over {} as a volcanic arc erupts. Volcanic intensity was {volcanism:.2}; elevation {elev:.0} m.",
                    self.place(cell)
                ),
                Some(cell),
                vec![
                    Cause::Event(cause),
                    Cause::State(StateFact::physical(
                        "volcanism",
                        cell,
                        volcanism,
                        "",
                        "volcanic intensity",
                    )),
                    Cause::State(StateFact::physical(
                        "elevation",
                        cell,
                        elev,
                        "m",
                        "elevation",
                    )),
                ],
                0.55,
            );
            if let Some(id) = id {
                self.natural_event = id;
            }
            self.stats.disasters += 1;
        } else if roll < 60 {
            let cell = if self.arid_cells.is_empty() {
                self.civs
                    .iter()
                    .find(|c| c.alive)
                    .map(|c| c.capital)
                    .unwrap_or(0)
            } else {
                self.arid_cells[(rng.below(self.arid_cells.len() as u64)) as usize]
            };
            let precip = self.world.climate.moisture.precip_mm[cell as usize];
            let id = self.record(
                None,
                year,
                EventKind::Drought,
                "A drought grips the drylands".to_string(),
                format!(
                    "Rainfall in {} falls to {precip:.0} mm for the year. Harvests fail across the surrounding country.",
                    self.place(cell)
                ),
                Some(cell),
                vec![
                    Cause::Event(cause),
                    Cause::State(StateFact::physical(
                        "precip",
                        cell,
                        precip,
                        "mm/yr",
                        "precipitation",
                    )),
                    Cause::State(StateFact::physical(
                        "aridity",
                        cell,
                        self.world.climate.moisture.aridity[cell as usize],
                        "",
                        "aridity",
                    )),
                ],
                0.45,
            );
            if let Some(id) = id {
                self.natural_event = id;
            }
            self.stats.disasters += 1;
            // Food stress for owners of the cell.
            let owner = self.owner[cell as usize];
            if owner != u16::MAX {
                if let Some(c) = self.civs.get_mut(owner as usize) {
                    c.food = (c.food - 1.2).max(0.0);
                }
            }
        } else if roll < 82 {
            let id = self.record(
                None,
                year,
                EventKind::SeaLevelChange,
                "The sea shifts".to_string(),
                format!(
                    "A slow eustatic adjustment changes coastlines by a few meters; the mean planetary temperature is {:.1} C.",
                    self.world.stats.mean_temperature_c
                ),
                None,
                vec![
                    Cause::Event(cause),
                    Cause::State(StateFact::global(
                        "mean_temperature",
                        self.world.stats.mean_temperature_c,
                        "C",
                        "mean temperature",
                    )),
                ],
                0.4,
            );
            if let Some(id) = id {
                self.natural_event = id;
            }
            self.stats.disasters += 1;
        } else {
            let id = self.record(
                None,
                year,
                EventKind::PlateShift,
                "The mantle rearranges the plates".to_string(),
                format!(
                    "A reorganization of mantle flow shifts the motion of {} plates. Boundary stresses change on a timescale no city can perceive.",
                    self.world.stats.plates
                ),
                None,
                vec![
                    Cause::Event(cause),
                    Cause::State(StateFact::global(
                        "plates",
                        f64::from(self.world.stats.plates),
                        "plates",
                        "plate count",
                    )),
                ],
                0.5,
            );
            if let Some(id) = id {
                self.natural_event = id;
            }
            self.stats.disasters += 1;
        }
    }

    fn place(&self, cell: u32) -> String {
        let biome = self.world.biomes.at(cell);
        let lat = self.world.sphere.lat(cell);
        let lon = self.world.sphere.lon(cell);
        format!(
            "the {} at {:.1} {}, {:.1} {}",
            biome.label().to_lowercase(),
            lat.abs(),
            if lat >= 0.0 { "N" } else { "S" },
            lon.abs(),
            if lon >= 0.0 { "E" } else { "W" }
        )
    }

    fn snapshot(&mut self, year: u32) {
        let civs = self
            .civs
            .iter()
            .map(|c| CivSnapshot {
                id: c.id,
                name: c.name.clone(),
                population: c.population,
                cells: c.cells.len() as u32,
                tech: c.tech,
                cohesion: c.cohesion,
                seat: c.seat(),
                alive: c.alive,
            })
            .collect();
        self.snapshots.push(PoliticalSnapshot {
            year,
            owner: self.owner.clone(),
            civs,
        });
    }

    fn finish_stats(&mut self) -> HistoryStats {
        let mut stats = std::mem::take(&mut self.stats);
        stats.surviving_civs = self.civs.iter().filter(|c| c.alive).count() as u32;
        stats.cities = self.civs.iter().map(|c| c.cities.len() as u32).sum();
        stats.largest_city = self
            .civs
            .iter()
            .filter_map(|c| c.largest_city().map(|city| city.population))
            .fold(0.0f64, f64::max);
        stats.max_tech = self.civs.iter().map(|c| c.tech).max().unwrap_or(0);
        stats.events = self.chronicle.len() as u32;
        stats.total_population = self.civs.iter().map(|c| c.population).sum();
        stats.peak_population = self.peak_population.iter().copied().fold(0.0f64, f64::max);
        let alive: Vec<&Civ> = self.civs.iter().filter(|c| c.alive).collect();
        stats.mean_tech = if alive.is_empty() {
            0.0
        } else {
            alive.iter().map(|c| f64::from(c.tech)).sum::<f64>() / alive.len() as f64
        };
        stats.claimed_cells = self.owner.iter().filter(|&&o| o != u16::MAX).count() as u32;
        stats
    }
}

fn history_hashes(history: &History) -> (String, String, String) {
    let mut hc = Hasher64::with_domain("chronicle");
    for e in history.chronicle.events() {
        hc.write_u32(e.id);
        hc.write_u32(e.year);
        hc.write_str_framed(e.kind.code());
        hc.write_str_framed(&e.title);
        hc.write_str_framed(&e.detail);
        for c in &e.causes {
            match c {
                Cause::Event(id) => {
                    hc.write_u32(0);
                    hc.write_u32(*id);
                }
                Cause::State(f) => {
                    hc.write_u32(1);
                    hc.write_str_framed(f.key);
                    hc.write_f64(f.value);
                    hc.write_u32(f.cell.unwrap_or(u32::MAX));
                }
            }
        }
    }
    let mut hv = Hasher64::with_domain("civs");
    for c in &history.civs {
        hv.write_str_framed(&c.name);
        hv.write_f64(c.population);
        hv.write_u32(u32::from(c.tech));
        hv.write_u32(c.cells.len() as u32);
        for &cell in &c.cells {
            hv.write_u32(cell);
        }
    }
    if let Some(last) = history.snapshots.last() {
        for &o in &last.owner {
            hv.write_u32(u32::from(o));
        }
    }
    let mut h = Hasher64::with_domain("combined");
    h.write_u64(hc.finish());
    h.write_u64(hv.finish());
    h.write_u64(history.config.seed);
    h.write_u32(history.config.years);
    (hex(hc.finish()), hex(hv.finish()), hex(h.finish()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use terra_core::world::WorldConfig;

    fn run(seed: u64) -> History {
        let w = World::generate(WorldConfig {
            seed,
            subdiv: 2,
            years: 600,
            erosion_iterations: 3,
            ..WorldConfig::default()
        });
        simulate(
            &w,
            HistoryConfig {
                seed,
                years: 600,
                initial_civs: 4,
                snapshot_interval: 100,
                ..HistoryConfig::default()
            },
        )
    }

    #[test]
    fn history_is_sound_and_populated() {
        let h = run(3);
        assert!(h.is_sound(), "{:?}", h.problems());
        assert!(!h.chronicle.is_empty());
        assert!(h.civs.len() >= 2);
        assert!(h.stats.events > 4);
        assert!(h.stats.claimed_cells > 0);
        assert!(h.snapshots.len() >= 2);
        assert_eq!(h.snapshots.last().unwrap().year, 600);
    }

    #[test]
    fn chronicle_starts_with_a_state_only_world_event() {
        let h = run(5);
        let first = h.chronicle.get(0).unwrap();
        assert_eq!(first.kind, EventKind::WorldFormed);
        assert!(first.is_root());
        assert!(first.causes.len() >= 3);
        assert!(h.chronicle.validate().is_empty());
    }

    #[test]
    fn civilizations_are_founded_with_causal_events() {
        let h = run(7);
        let found: Vec<_> = h
            .chronicle
            .events()
            .iter()
            .filter(|e| e.kind == EventKind::SettlementFounded)
            .collect();
        assert_eq!(found.len(), h.civs.len());
        for e in found {
            assert!(e.civ.is_some());
            assert!(e.cell.is_some());
            assert!(e.cause_events().any(|c| c == 0));
            assert!(e.cause_facts().count() >= 2);
        }
        for e in h.chronicle.events() {
            for c in e.cause_events() {
                assert!((c as usize) < e.id as usize);
            }
        }
    }

    #[test]
    fn expansion_claims_new_cells_over_time() {
        let h = run(9);
        let initial: u32 = h
            .snapshots
            .first()
            .unwrap()
            .owner
            .iter()
            .filter(|&&o| o != u16::MAX)
            .count() as u32;
        let final_owned: u32 = h
            .snapshots
            .last()
            .unwrap()
            .owner
            .iter()
            .filter(|&&o| o != u16::MAX)
            .count() as u32;
        assert!(final_owned >= initial, "{initial} -> {final_owned}");
        assert!(h
            .chronicle
            .events()
            .iter()
            .any(|e| e.kind == EventKind::Expansion));
    }

    #[test]
    fn same_seed_same_history() {
        let a = run(13);
        let b = run(13);
        assert_eq!(a.hashes, b.hashes);
        assert_eq!(a.chronicle.len(), b.chronicle.len());
        assert_eq!(a.stats.events, b.stats.events);
        for (x, y) in a.civs.iter().zip(&b.civs) {
            assert_eq!(x.name, y.name);
            assert_eq!(x.cells, y.cells);
            assert_eq!(x.population.to_bits(), y.population.to_bits());
        }
    }

    #[test]
    fn different_seeds_differ() {
        let a = run(15);
        let b = run(16);
        assert_ne!(a.hashes.combined, b.hashes.combined);
    }

    #[test]
    fn owners_and_civs_agree() {
        let h = run(17);
        let last = h.snapshots.last().unwrap();
        for (cell, &owner) in last.owner.iter().enumerate() {
            if owner != u16::MAX {
                let civ = &h.civs[owner as usize];
                assert!(civ.owns(cell as u32), "cell {cell} owner mismatch");
            }
        }
        for civ in &h.civs {
            for &cell in &civ.cells {
                assert_eq!(last.owner[cell as usize], civ.id);
            }
        }
    }

    #[test]
    fn config_is_sanitized() {
        let c = HistoryConfig {
            seed: 1,
            years: 5,
            initial_civs: 100,
            aggression: f64::NAN,
            expansion: -5.0,
            step_years: 0,
            snapshot_interval: 1,
        }
        .sanitized();
        assert_eq!(c.years, 100);
        assert_eq!(c.initial_civs, 16);
        assert_eq!(c.aggression, 1.0);
        assert!(c.step_years >= 5);
        assert!(c.snapshot_interval >= 50);
    }

    #[test]
    fn empty_world_is_safe() {
        let w = World::generate(WorldConfig {
            seed: 21,
            subdiv: 2,
            years: 100,
            land_fraction: 0.1,
            erosion_iterations: 1,
            ..WorldConfig::default()
        });
        let h = simulate(
            &w,
            HistoryConfig {
                seed: 21,
                years: 100,
                initial_civs: 2,
                ..HistoryConfig::default()
            },
        );
        assert!(h.is_sound());
    }

    #[test]
    fn rise_and_fall_appear_across_seeds() {
        let mut kinds = std::collections::BTreeSet::new();
        for seed in 0..3u64 {
            let w = World::generate(WorldConfig {
                seed,
                subdiv: 2,
                years: 10_000,
                erosion_iterations: 3,
                ..WorldConfig::default()
            });
            let h = simulate(
                &w,
                HistoryConfig {
                    seed,
                    years: 10_000,
                    initial_civs: 6,
                    snapshot_interval: 1_000,
                    ..HistoryConfig::default()
                },
            );
            assert!(h.is_sound(), "{:?}", h.problems());
            for e in h.chronicle.events() {
                kinds.insert(e.kind.code());
            }
        }
        for required in [
            "settlement_founded",
            "expansion",
            "trade_route_opened",
            "war_declared",
            "battle",
            "collapse",
            "eruption",
        ] {
            assert!(kinds.contains(required), "missing {required}: {kinds:?}");
        }
    }

    #[test]
    fn history_snapshot_lookup() {
        let h = run(23);
        assert!(h.snapshot_at(0).is_some());
        assert!(h.snapshot_at(10_000).is_some());
        let last = h.snapshots.last().unwrap();
        let cell = (0..last.owner.len() as u32).find(|&c| last.owner[c as usize] != u16::MAX);
        if let Some(cell) = cell {
            assert!(h.owner_at(cell, h.config.years).is_some());
        }
    }
}
