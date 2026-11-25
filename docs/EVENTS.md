# Event catalogue

Every chronicle event has a kind, a title, a detail, a severity and structured
causes. This catalogue lists what each kind means, the causes Terra usually
cites, and the effects on the simulation.

## Nature

| Kind | Meaning | Typical causes | Effects |
|---|---|---|---|
| `world_formed` | The planet exists | seed, cell count, land fraction, mean climate | Root event for the run |
| `plate_shift` | Mantle reorganization | prior natural event, plate count | Narrative only |
| `sea_level_change` | Eustatic adjustment | prior natural event, mean temperature | Narrative only |
| `extinction` | A lineage disappears | collapse event, species richness, fragility | Narrative only |

## Disasters

| Kind | Meaning | Typical causes | Effects |
|---|---|---|---|
| `eruption` | Volcanic arc erupts | prior natural event, volcanism, elevation | Narrative; local color |
| `drought` | Dryland rainfall failure | prior natural event, precipitation, aridity | Food store of the cell owner falls |
| `famine` | Harvest failure | last civ event, food, precipitation | Population −20%, cohesion −0.12, food zeroed |
| `epidemic` | Disease outbreak | last civ event, population, density | Population −12%, cohesion −0.08 |

## Society

| Kind | Meaning | Typical causes | Effects |
|---|---|---|---|
| `settlement_founded` | Capital founded | world formation, carrying capacity, precipitation | Civ appears on the map |
| `expansion` | Frontier claims land | previous civ event, carrying capacity, population | Cells, cities and resources grow |
| `collapse` | Civilization ends | last event, population, cohesion | Alive=false, territory released, survivors may migrate |
| `migration` | Refugees join a neighbor | collapse, target carrying capacity | Target population rises |

## Economy

| Kind | Meaning | Typical causes | Effects |
|---|---|---|---|
| `resource_discovered` | First deposit of a resource | expansion event, abundance fact | Resource enters the civ's economy |
| `trade_route_opened` | Two peoples exchange goods | both last events, relation | Relations +20, treasury +120 each |

## Conflict

| Kind | Meaning | Typical causes | Effects |
|---|---|---|---|
| `war_declared` | Hostilities begin | last civ event, relation, military | War record, at-war flags |
| `battle` | Field engagement | prior war/battle, war declaration, military | Casualties, cohesion shift, one border cell captured |
| `city_captured` | Settlement changes hands | battle event, carrying capacity | City moves, capital marker removed |
| `peace_treaty` | War ends | declaration, latest battle, casualties | War closes, relations reset to −20 |

## Culture

| Kind | Meaning | Typical causes | Effects |
|---|---|---|---|
| `tech_discovered` | Technology mastered | last civ event, population, treasury | Tech level +1, bonuses rise |
| `golden_age` | Prosperity recorded | tech event, cohesion, technology | One-time event per civ |

## Severity

Severity is a narrative weight in `[0, 1]`, used by the timeline and lists. It
is not a physical quantity. Root events are severe by definition; resource
discoveries are light; collapses and world formation are near the top.

## Causal shape

- Founding, expansion and war events cite the civilization's previous event.
- Battles cite both the war declaration and the latest engagement.
- Famines cite the previous civ event plus food and rainfall.
- Collapses cite the trigger (famine, battle, or overstretch) plus population
  and cohesion.
- Natural events cite the previous natural event and physical measurements.

The chain is a DAG, not a tree: a root event can be cited by many later events.
`cause_chain` deduplicates repeated ancestors; `trace_paths` keeps explicit
routes so the same root can appear on multiple paths.
