# Limitations

Terra is a qualitative simulator built for coherence, determinism and
traceability. It is not a research geodynamic model, a climate model or a
historical sociology. This file records the honest gaps.

## Geology

- **Fixed plate partition.** Plates have angular velocities and evolving
  boundary stress, but cells do not migrate between plates. Plate area is a
  partition by construction. The acceptance criterion is therefore stated as
  partition area within 0.1% of the sphere, not as a moving-plate area budget.
- **No mantle convection.** Plate motion changes are small random perturbations
  per orogenic epoch, not a solution of mantle flow.
- **Qualitative orogeny.** Uplift is applied to overriding margins within two
  hops. It produces plausible belts and a positive uplift/elevation
  correlation, but it is not isostasy or flexure.
- **No true seafloor spreading.** Divergent margins create rift shoulders and
  subsidence; crust age is a distance proxy, not a magnetic anomaly record.
- **Fixed sea level.** The climate declares sea-level-change events, but terrain
  is not re-flooded during history; doing so would require re-running hydrology
  and biomes.

## Hydrology

- **D8 flow on an icosphere.** Steepest descent on the primal graph is simple
  and testable; it is not a high-order hydrodynamic solver.
- **Priority-Flood epsilon.** Lakes are flat to within 0.01 m, not exactly flat.
  The surface is the spill level; interior filled values carry a tiny ε ramp.
- **Erosion is stream power.** There is no sediment size distribution, bedload
  separation, chemical weathering or glacial erosion.
- **Deltas are capped.** A delta may rise at most 4 m per pass and only in water
  shallower than 120 m. This avoids land bridges built by numerical enthusiasm.
- **No groundwater or aquifers.** Discharge is drainage area times
  precipitation.

## Climate

- **No general circulation model.** Winds are zonal bands with noise. Currents
  are an Ekman-like rotation of wind. There is no pressure field, Coriolis
  integration or seasonal cycle.
- **Annual means only.** Temperature has a seasonal amplitude for biome flavor,
  but the simulation never steps through seasons.
- **Precipitation is calibrated.** The relative pattern comes from advection,
  orographic lift, ITCZ boost, subtropical suppression and a foehn rule. The
  land mean is then scaled to the scenario target. Absolute totals are a
  modelling choice.
- **No clouds feedback.** Cloud cover is derived from precipitation and
  humidity; it does not feed back into radiation.
- **No ice-albedo feedback.** Ice is a biome, not a dynamic reservoir.

## Ecology

- **Species richness is a scalar.** There are no species lists, phylogenies or
  trophic networks. "Species" values estimate vascular plant richness from
  biome, patch area and productivity.
- **NPP uses the Miami model.** It is a classic empirical approximation, not a
  process model.
- **Extinction is local.** A lineage can disappear when a small, rich patch
  loses its protecting civilization. There is no global biodiversity debt.

## Civilizations

- **Rule-based, not agent-based.** There are no individual people, firms or
  institutions. Civilizations have population, food, cohesion, military,
  treasury, technology and traits.
- **Expansion is frontier scoring.** Territories grow contiguously; there is no
  migration into empty land beyond the frontier, no sea travel and no
  colonialism across oceans.
- **Diplomacy is a scalar.** Relations, trade, incidents and wars are explicit
  rules. There are no alliances, vassals, federations or non-state actors.
- **Economy is abstract.** One treasury number and nine resource flags. There
  are no prices, markets, currencies or supply chains.
- **Technology is a ladder.** Eight named technologies with population and
  treasury thresholds; no diffusion between neighbors.
- **Cities are mostly narrative.** Population is distributed approximately among
  settlements; city capture and capitals matter for events and maps.
- **No dynamic trade networks.** A trade route is opened and then persists. Its
  benefit is a resource multiplier and treasury income, not a routed graph.
- **War is attrition plus a border capture.** No fronts, logistics or sieges
  beyond the city-capture event.

## History and causality

- **Causes are structured but curated.** Rules choose which measured facts and
  prior events to cite. The citations are real and validated, but they are not a
  full counterfactual explanation.
- **Severity is authored.** Event severity is a narrative weight, not a
  physical quantity.
- **Event cap.** 200,000 events per run. Long runs with many civilizations stay
  far below this, but it is a hard stop rather than an endless log.
- **Natural events do not always change terrain.** Droughts, eruptions and
  sea-level changes affect civilizations and the record; eruptions do not
  reshape elevation after hydrology, and sea level does not re-flood coasts.

## Determinism

- **Same platform, same build.** Replay is bit-exact within a build and platform.
  Cross-platform bit equality is not promised because `sin`, `cos`, `exp` and
  `powf` come from the platform libm.
- **No cross-version promise.** Changing the model changes hashes. The version
  is part of the project's communication, not a compatibility guarantee.
- **Hashing is FNV-1a**, suitable for identity checks, not cryptography.

## Studio

- **Single user, loopback only.** No CORS, no TLS, no multi-user sessions, no
  persistence. Closing the process discards worlds.
- **Token in the URL.** It is a local convenience. It can appear in browser
  history; the server is bound to loopback and short-lived.
- **World cache is in memory.** At most ten finished worlds; older finished
  worlds are evicted. A long SSE stream pins nothing beyond the job record.
- **Rendering is nearest-cell.** At 1024×512 the 10,242-cell sphere is visible as
  facets at high zoom. There is no antialiasing of cell boundaries.
- **No external fonts.** Labels use a 5×7 bitmap font; longer names are clipped
  or scaled rather than typeset.
- **PNG encoder is minimal.** 8-bit RGB, no interlace, no ancillary chunks,
  fixed-Huffman DEFLATE. Files are larger than a general-purpose encoder would
  produce but valid and deterministic.
- **Headless flags are for tests and benchmarks.** The product is the Studio;
  the CLI does not expose the simulation as a scripting interface.

## Engineering

- **No CI.** By requirement. Every commit is tested locally; the final report
  records the exact commands.
- **Playwright is a development-only tool.** It is not a Rust dependency and is
  not needed to build or run Terra. A pure-Rust HTTP end-to-end test covers the
  same endpoints when Playwright is unavailable.
- **No coverage instrumentation.** Line coverage is not measured with external
  tooling. The test inventory and acceptance mapping are in
  `docs/TESTING.md`; the final report states this gap explicitly.

## Out of scope

Real Earth import, GIS, exact climatology, sentient agents, full economies,
multiplayer, databases, CRDTs, crash analysis and observability dashboards.
