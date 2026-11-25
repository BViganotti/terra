# Simulation model

Terra is a qualitative planet simulator. It is built to produce coherent,
reproducible worlds and causally traceable histories, not to match Earth
observations. This document describes every stage, its inputs and outputs, the
approximations, and the invariants the tests enforce.

## Pipeline

```
seed + scenario
  → icosphere
  → plates and boundary stress
  → elevation and crust
  → depression fill → erosion → sediment → lakes/deltas
  → wind → currents → temperature → moisture
  → biomes → ecosystems
  → civilizations → chronicle
```

Each stage is a pure function of the previous layers, the configuration and
explicit random streams. The Studio renders the resulting layers; it never
mutates them.

## Grid

The sphere is a subdivided icosahedron (`terra_core::sphere`). Level 5 is the
Studio default: 10,242 cells, 20,480 triangles. Cells have a unit direction,
5 or 6 neighbors stored in a sorted CSR adjacency list, and a spherical area
computed from incident triangles. The area sum is `4π` within `1e-9`.
Nearest-cell lookup uses a latitude/longitude bin index with a 7×7 neighborhood
and a full-scan fallback.

The graph is connected, adjacency is reciprocal, and neighbor lists are sorted.
These properties are checked at every level during tests.

## Plates and tectonics

1. **Seeds** are chosen by farthest-point sampling: the first is uniform, each
   later seed maximizes the minimum great-circle distance to previous seeds.
2. **Assignment** is a multi-source breadth-first search over the cell graph,
   with ties broken by lower plate id. The result is a contiguous graph Voronoi
   partition: every plate is one connected component. Plate areas sum to the
   sphere area within 0.1%, which is the plate-area acceptance criterion.
3. **Buoyancy** comes from a broad continental mask (`warped_fbm`). Plates with
   a mean mask above 0.06 are continental (density ≈ 2.78), below −0.06 oceanic
   (≈ 3.05), otherwise mixed. Each cell contributes a small density anomaly, so
   the denser side of a convergent margin subducts.
4. **Motion** is an angular velocity vector per plate, magnitude 0.00022–0.00135
   rad/Myr. Every orogenic epoch perturbs the axis and speed slightly.
5. **Boundaries** are cross-plate edges. The relative velocity is projected on
   the local normal and tangent:
   - closing faster than 1 km/Myr → convergent,
   - opening faster than 1 km/Myr → divergent,
   - otherwise transform.
6. **Orogeny** runs for 1–64 epochs. Each epoch applies uplift to the overriding
   plate within two hops of a convergent edge, a trench to the subducting side,
   a volcanic arc behind the margin, shoulder uplift beside rifts, and minor
   shear uplift on transforms. Uplift is smoothed once to avoid single-cell
   spikes.

The model is not geodynamic. Plates do not actually move through the grid;
their stress field evolves while the partition stays fixed. This keeps the area
accounting exact and is documented in `LIMITATIONS.md`.

## Elevation and crust

Base elevation is a smooth function of the continental mask: up to about 4,900 m
for continental crust and down to about −6,250 m for ocean floor. Ridged noise
roughens uplifted belts. Sea level is chosen as the quantile that matches the
scenario land fraction, so the coastline is a consequence of the crust rather
than a random per-cell choice.

Crustal thickness is 26–44 km for continental and 7–10 km for oceanic crust,
plus orogenic thickening. Crust age grows with distance from the nearest
divergent margin, capped at 190 Myr.

## Hydrology

Hydrology is iterative because erosion changes the surface that defines flow.

### Depression filling

Priority-Flood (Barnes et al., 2014): all ocean cells enter a min-heap, and the
lowest frontier cell expands next. Neighbors are raised to at least the current
level plus ε = 0.001 m. The result is a strictly descending filled surface and a
processing order from low to high.

### Drainage

Each non-ocean cell flows to the neighbor with the steepest descent on the
filled surface. Slopes use the great-circle distance between cell centers.
Accumulation is a single reverse-order pass; Strahler order is computed from the
two highest-order upstream tributaries. Flow is a forest rooted at ocean cells,
so rivers cannot run uphill.

### Erosion and sediment

For each pass:

- Stream power: `E = K · A^m · S^n`, with `K = 9e-5`, `m = 0.5`, `n = 1.0`.
  `A` is upstream area and `S` the slope. Removal is capped at 30% of the local
  drop per pass.
- Sediment routing: eroded volume travels downstream. Deposition is stronger on
  gentle reaches and at river mouths; lakes trap 85%. Deltas may emerge, but
  only in water shallower than 120 m and by at most 4 m per pass.
- Hillslope diffusion: pairwise, mass-conserving volume exchange between
  neighbors, capped so the lower cell cannot pass the higher one.

The mass balance is exact in volume: the net rock lost equals the volume
exported past the shelf within 1%. Gross erosion and gross deposition are also
reported.

### Lakes

A lake is a connected run of land cells whose filled surface is more than 0.4 m
above the ground. The surface is the lowest filled neighbor outside the
component (the spill point). Volume integrates depth over spherical cell areas.
Endorheic lakes have no spill target.

## Climate

### Wind

Zonal profiles:

- trades: westward, strongest at the equator, fading by 30°,
- westerlies: eastward, peaking at 45°,
- polar easterlies: westward above 60°.

A deterministic value-noise field perturbs speed by ±25% and direction by ±18°.
Profiles are continuous across band edges (tested at 0.1° resolution).

### Currents

Surface currents are the wind rotated 0.6 rad to the right in the northern
hemisphere and to the left in the south, at 2.8% of wind speed, then smoothed
across water three times. Land currents are zero. This is an Ekman-like
approximation, not a shallow-water solution.

### Temperature

Annual mean:

```
T = 30·cos(lat)^1.3 − 30·(1−cos(lat))
    − 6.2·(elevation_km)          (land)
    + 1.5                          (ocean)
    − 1 − 2·continentality         (land)
    + 4.5·poleward_current·|lat|/55 (ocean)
    + noise
```

clamped to `[-65, 42] °C`. Seasonality is 1.5–5.5 °C over ocean and 4–33 °C over
land, increasing with latitude and distance from water. Continentality is graph
distance to water divided by six.

### Moisture

Moisture is a per-cell scalar with saturation capacity from temperature
(Clausius–Clapeyron-like, 2–26 mm). Each of 32 synchronous passes:

1. rains a fraction of the moisture out; the fraction increases with orographic
   lift, is boosted near the equator and suppressed near 25° latitude,
2. advects 55% of the remainder to downwind neighbors weighted by alignment²,
3. evaporates from water toward capacity.

A post-pass foehn rule reduces rain sharply downwind of a ridge more than 700 m
higher than the cell. Land precipitation is then normalized to the scenario
target mean (default 860 mm/yr); the relative pattern is physical, the absolute
scale is a documented choice.

Potential evapotranspiration is `180 + 38·T + 1.2·T²` mm/yr for positive T.
Aridity is precipitation divided by PET.

## Biomes

Biomes follow a Whittaker-like table over annual mean temperature,
precipitation, elevation, coastal context and river presence. Ice and alpine
override moisture. Wetlands and mangroves are azonal: flat, wet, coastal or
river-adjacent cells can override the climate table. Every cell gets exactly one
biome; connected patches are labelled, and patch area feeds ecosystem richness.

## Ecosystems

Net primary productivity uses the Miami model:

```
NPP_T = 3000 / (1 + exp(1.315 − 0.119·T))
NPP_P = 3000 · (1 − exp(−0.000664·P))
NPP   = min(NPP_T, NPP_P) · biome_factor
```

Species richness combines biome base richness, log patch area, productivity and
a river bonus, scaled to 5–400. Fragility rises for rich biomes that survive
only in small patches. Human carrying capacity combines productivity, biome
fertility, fresh water and coastal access, capped at 2 million people per cell.

## Civilization simulation

The history crate runs a year loop over 2–12 civilizations:

- **Growth** — logistic population growth toward carrying capacity times a
  technology bonus. Food stores track production minus consumption and war
  drain. Cohesion drifts toward a target lowered by war and overextension.
- **Expansion** — every 20 years a civilization scores frontier cells by
  productivity, river, coast and adjacency, then claims a bounded number.
- **Resources** — claimed cells may reveal one of nine resources based on biome,
  elevation, coast and rivers. First discovery is a chronicle event.
- **Technology** — eight technologies unlock at population and treasury
  thresholds.
- **Diplomacy** — border friction, trade, gifts and incidents move a relation
  score. Trade routes open above a threshold; low relations plus pressure can
  escalate to war.
- **War** — battles compare military strength with technology, resources and
  noise. The loser may lose a border cell, a city, or its last stronghold.
  Peace follows exhaustion or collapse.
- **Collapse** — starvation, lost cohesion, overstretch or conquest ends a
  civilization. Survivors may migrate to a neighbour; fragile habitats can lose
  a lineage.
- **Natural events** — eruptions, droughts, sea-level change and plate shifts
  occur on deterministic schedules with causes in physical state.

Every state change that matters records an event with its causes. The history
stone never mutates recorded events.

## Determinism

All random decisions use xoshiro256** seeded through SplitMix64 from the world
seed and an explicit stream constant. Grids are dense vectors. There is no
hash-map iteration in a simulation path. See `docs/DETERMINISM.md`.
