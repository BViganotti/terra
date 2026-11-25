//! Example scenarios compiled into the Studio.
//!
//! The files under `examples/scenarios/` are the single source of truth. The
//! Studio serves them as text so the scenario editor can load them without
//! reading from disk or the network.

/// One bundled scenario.
#[derive(Clone, Copy, Debug)]
pub struct Example {
    /// Stable identifier used by the client.
    pub code: &'static str,
    /// Display name.
    pub name: &'static str,
    /// Short description.
    pub summary: &'static str,
    /// Scenario document.
    pub text: &'static str,
}

/// All bundled examples in menu order.
pub const EXAMPLES: [Example; 6] = [
    Example {
        code: "temperate",
        name: "Temperate Canopy",
        summary: "Balanced nine-plate world with ample rain.",
        text: include_str!("../../../examples/scenarios/temperate.terra"),
    },
    Example {
        code: "arid",
        name: "Amber Expanse",
        summary: "Hot, dry continents and river-bound cities.",
        text: include_str!("../../../examples/scenarios/arid.terra"),
    },
    Example {
        code: "icehouse",
        name: "Long Winter",
        summary: "Glacial coastlines and short growing seasons.",
        text: include_str!("../../../examples/scenarios/icehouse.terra"),
    },
    Example {
        code: "archipelago",
        name: "Thousand Sounds",
        summary: "Fragmented islands where trade and isolation matter.",
        text: include_str!("../../../examples/scenarios/archipelago.terra"),
    },
    Example {
        code: "pangaea",
        name: "One Land",
        summary: "A supercontinent with dry continental interiors.",
        text: include_str!("../../../examples/scenarios/pangaea.terra"),
    },
    Example {
        code: "micro",
        name: "Pebble",
        summary: "A small, fast smoke-test world.",
        text: include_str!("../../../examples/scenarios/micro.terra"),
    },
];

/// Looks up an example by code.
pub fn find(code: &str) -> Option<&'static Example> {
    EXAMPLES.iter().find(|e| e.code == code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_example_parses_and_validates() {
        for example in &EXAMPLES {
            let scenario = terra_core::scenario::parse(example.text)
                .unwrap_or_else(|e| panic!("{}: {e}", example.code));
            scenario
                .validate()
                .unwrap_or_else(|e| panic!("{}: {e}", example.code));
            assert!(!example.name.is_empty());
            assert!(!example.summary.is_empty());
        }
    }

    #[test]
    fn codes_are_unique_and_findable() {
        let mut codes = std::collections::BTreeSet::new();
        for example in &EXAMPLES {
            assert!(codes.insert(example.code));
            assert_eq!(find(example.code).map(|e| e.name), Some(example.name));
        }
        assert!(find("missing").is_none());
    }

    #[test]
    fn micro_is_small_and_fast() {
        let micro = find("micro").unwrap();
        let scenario = terra_core::scenario::parse(micro.text).unwrap();
        assert_eq!(scenario.config.subdiv, 2);
        assert_eq!(scenario.config.years, 1_000);
    }
}
