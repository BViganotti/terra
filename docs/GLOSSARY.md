# Glossary

**Atlas** — the default map layer: biome color, relief, rivers and soft plate
boundaries.

**Carrying capacity** — estimated people a cell can support under simple
agriculture, from productivity, fertility, water and coastal access.

**Cause** — a structured reason for an event, either a measured state fact or an
earlier event id.

**Cell** — one vertex of the icosphere, with a direction, neighbors and a
spherical area. At subdivision 5 there are 10,242.

**Chain** — breadth-first traversal of an event's causes, including state facts.

**Chronicle** — the append-only event store for a run.

**Confluence** — a cell with two or more upstream tributaries of the highest
Strahler order; its order increases by one.

**Continentality** — graph distance from the coast divided by six, used to make
interiors colder and more seasonal.

**Convergent margin** — a plate boundary where relative motion closes the gap
faster than 1 km/Myr.

**Crust age** — a proxy from distance to the nearest divergent margin, capped at
190 Myr.

**Determinism** — the property that the same seed and scenario produce the same
world and chronicle hashes on the same build and platform.

**Discharge** — drainage area times precipitation, divided by seconds per year,
in cubic meters per second.

**Divergent margin** — a boundary opening faster than 1 km/Myr; produces rift
subsidence and shoulder uplift.

**Drainage area** — upstream contributing area, accumulated downstream on the
filled surface.

**Ecosystem** — the productivity, richness, fragility and capacity fields of a
cell.

**Ekman-like current** — wind rotated 0.6 rad toward the right (north) or left
(south), scaled to 2.8% of wind speed and smoothed over water.

**Filled surface** — the water surface after Priority-Flood raises closed
depressions to their spill level. It is never below the original ground.

**Fragility** — a measure in `[0, 1]` of how vulnerable a rich biome is when its
patch is small.

**Hillslope diffusion** — pairwise mass-conserving smoothing between neighboring
cells.

**History** — civilizations plus chronicle plus snapshots, produced from a
world and a history configuration.

**Hydrology** — the post-erosion elevation, filled surface, flow graph, lakes,
delta flags and sediment accounting.

**Icosphere** — a subdivided icosahedron projected to the unit sphere; Terra's
grid.

**Lake** — a connected set of land cells whose filled surface stands more than
0.4 m above the ground.

**Land fraction** — share of cells above sea level after erosion; targeted by
choosing the sea-level quantile.

**Leaf/root** — a root event has only state causes; every chain terminates at
state facts reachable through the world event.

**Miami model** — empirical NPP curves limited by temperature and precipitation.

**Moisture** — per-cell precipitable water proxy, advected synchronously along
the wind.

**NPP** — net primary productivity, grams of carbon per square meter per year.

**Orogeny** — tectonic uplift applied over epochs to overriding margins and
volcanic arcs.

**Plate** — a connected set of cells assigned by graph Voronoi from a seed, with
motion, density, kind and area.

**Plate area share** — plate spherical area divided by the sphere area; shares
sum to one within `1e-9`.

**Political snapshot** — owner id per cell plus civilization statistics at a
given year.

**Priority-Flood** — depression-filling algorithm using a min-heap of frontier
cells, with ε per step.

**Replay** — regenerating from seed and scenario and obtaining identical hashes.

**Rift** — a divergent boundary zone with subsidence and shoulder uplift.

**River** — a land cell whose drainage area is at least 2.2 cell areas and which
is not lake water.

**Runoff** — water depth per year. Before climate it is uniform; afterward it is
precipitation.

**Scenario** — bounded key/value configuration for a world.

**Sea level** — the elevation offset chosen to hit the land-fraction target; all
terrain elevations are relative to it.

**Severity** — narrative weight of an event in `[0, 1]`.

**Shelf** — shallow ocean near a coast or above −220 m.

**Strahler order** — stream order, starting at 1 for headwaters, increasing at
confluences of equal highest order.

**Subduction** — the denser plate at a convergent margin goes under the other;
produces a trench and a volcanic arc.

**Subdivision** — icosphere refinement level. Cells = `10·4^level + 2`.

**Terra** — the planet, its history, and this repository.

**Trait** — agrarian, martial or mercantile emphasis, normalized at founding.

**Transform margin** — a boundary dominated by shear rather than closing or
opening.

**World hash** — combined digest of configuration and all physical stage hashes.
