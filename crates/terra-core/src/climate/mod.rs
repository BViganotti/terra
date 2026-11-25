//! Climate: temperature, wind, ocean currents and precipitation.
//!
//! The stages run in a fixed order because each depends on the previous:
//!
//! 1. Winds are derived from latitude and perturbed by terrain-independent
//!    noise.
//! 2. Surface currents are an Ekman-like rotation of the wind over water.
//! 3. Temperature combines insolation, lapse rate, continentality and poleward
//!    heat transport by those currents.
//! 4. Moisture evaporates from water, advects with the wind and rains out with
//!    orographic and convective controls.
//!
//! Nothing here reads wall-clock time or random state outside the seed.

pub mod moisture;
pub mod temperature;
pub mod wind;

pub use moisture::{Moisture, MoistureConfig};
pub use temperature::Temperature;
pub use wind::Winds;

use crate::hash::Hasher64;
use crate::sphere::Sphere;

/// Climate configuration.
#[derive(Clone, Copy, Debug, Default)]
pub struct ClimateConfig {
    /// Moisture transport settings.
    pub moisture: MoistureConfig,
    /// Optional global temperature offset in degrees Celsius.
    pub temperature_offset_c: f64,
}

impl ClimateConfig {
    /// Clamps fields into supported ranges.
    pub fn sanitized(mut self) -> Self {
        self.moisture = self.moisture.sanitized();
        self.temperature_offset_c = if self.temperature_offset_c.is_finite() {
            self.temperature_offset_c.clamp(-20.0, 20.0)
        } else {
            0.0
        };
        self
    }
}

/// The complete climate of a world.
#[derive(Clone, Debug)]
pub struct Climate {
    /// Temperature fields.
    pub temperature: Temperature,
    /// Wind and current fields.
    pub winds: Winds,
    /// Precipitation and humidity.
    pub moisture: Moisture,
    /// Water mask used to build the climate (ocean and large lakes).
    pub ocean: Vec<bool>,
}

impl Climate {
    /// Builds all climate fields.
    pub fn build(
        sphere: &Sphere,
        elevation: &[f64],
        ocean: &[bool],
        seed: u64,
        config: ClimateConfig,
    ) -> Self {
        let config = config.sanitized();
        let winds = wind::compute(sphere, ocean, seed);
        let mut temperature =
            temperature::compute(sphere, elevation, ocean, &winds.current_poleward, seed);
        if config.temperature_offset_c != 0.0 {
            let d = config.temperature_offset_c;
            for t in &mut temperature.mean_c {
                *t = (*t + d).clamp(-70.0, 50.0);
            }
            for t in &mut temperature.max_c {
                *t = (*t + d).clamp(-80.0, 62.0);
            }
            for t in &mut temperature.min_c {
                *t = (*t + d).clamp(-92.0, 52.0);
            }
        }
        let moisture = moisture::compute(
            sphere,
            elevation,
            ocean,
            &temperature,
            &winds,
            seed,
            config.moisture,
        );
        Self {
            temperature,
            winds,
            moisture,
            ocean: ocean.to_vec(),
        }
    }

    /// Number of cells.
    pub fn len(&self) -> usize {
        self.temperature.len()
    }

    /// True when there are no cells.
    pub fn is_empty(&self) -> bool {
        self.temperature.is_empty()
    }

    /// Mean annual temperature at a cell.
    pub fn temperature_at(&self, cell: u32) -> f64 {
        self.temperature
            .mean_c
            .get(cell as usize)
            .copied()
            .unwrap_or(0.0)
    }

    /// Annual precipitation at a cell in millimeters.
    pub fn precip_at(&self, cell: u32) -> f64 {
        self.moisture
            .precip_mm
            .get(cell as usize)
            .copied()
            .unwrap_or(0.0)
    }

    /// True when a cell is water in the climate sense.
    pub fn is_ocean(&self, cell: u32) -> bool {
        self.ocean.get(cell as usize).copied().unwrap_or(false)
    }

    /// Aridity at a cell.
    pub fn aridity_at(&self, cell: u32) -> f64 {
        self.moisture
            .aridity
            .get(cell as usize)
            .copied()
            .unwrap_or(0.0)
    }

    /// Folds every climate layer into a stable hash.
    pub fn hash_into(&self, h: &mut Hasher64) {
        self.temperature.hash_into(h);
        self.winds.hash_into(h);
        self.moisture.hash_into(h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tectonics::{self, TectonicsConfig};
    use crate::terrain::{self, TerrainConfig};

    fn world(seed: u64) -> (Sphere, Climate) {
        let s = Sphere::new(3);
        let t = tectonics::generate(&s, seed, TectonicsConfig::default());
        let ter = terrain::build(&s, &t, seed, TerrainConfig::default());
        let ocean: Vec<bool> = ter.elevation.iter().map(|&e| e <= 0.0).collect();
        let c = Climate::build(&s, &ter.elevation, &ocean, seed, ClimateConfig::default());
        (s, c)
    }

    #[test]
    fn layers_have_matching_lengths() {
        let (s, c) = world(3);
        assert_eq!(c.len(), s.len());
        assert_eq!(c.ocean.len(), s.len());
        assert_eq!(c.winds.len(), s.len());
        assert_eq!(c.moisture.len(), s.len());
        assert!(!c.is_empty());
    }

    #[test]
    fn temperature_offset_applies() {
        let s = Sphere::new(2);
        let t = tectonics::generate(&s, 4, TectonicsConfig::default());
        let ter = terrain::build(&s, &t, 4, TerrainConfig::default());
        let ocean: Vec<bool> = ter.elevation.iter().map(|&e| e <= 0.0).collect();
        let base = Climate::build(&s, &ter.elevation, &ocean, 4, ClimateConfig::default());
        let warm = Climate::build(
            &s,
            &ter.elevation,
            &ocean,
            4,
            ClimateConfig {
                temperature_offset_c: 5.0,
                ..ClimateConfig::default()
            },
        );
        for i in 0..s.len() {
            assert!(warm.temperature.mean_c[i] >= base.temperature.mean_c[i] + 4.9);
        }
    }

    #[test]
    fn accessors_are_safe_for_bad_cells() {
        let (_, c) = world(5);
        assert_eq!(c.temperature_at(999_999), 0.0);
        assert_eq!(c.precip_at(999_999), 0.0);
        assert!(!c.is_ocean(999_999));
        assert_eq!(c.aridity_at(999_999), 0.0);
    }

    #[test]
    fn climate_is_deterministic() {
        let (_, a) = world(7);
        let (_, b) = world(7);
        let mut ha = Hasher64::new();
        let mut hb = Hasher64::new();
        a.hash_into(&mut ha);
        b.hash_into(&mut hb);
        assert_eq!(ha.finish(), hb.finish());
    }

    #[test]
    fn config_is_sanitized() {
        let c = ClimateConfig {
            moisture: MoistureConfig::default(),
            temperature_offset_c: f64::NAN,
        }
        .sanitized();
        assert_eq!(c.temperature_offset_c, 0.0);
    }
}
