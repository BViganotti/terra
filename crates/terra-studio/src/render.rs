//! Map rendering.
//!
//! Layers are rasterized into an equirectangular image by nearest-cell lookup on
//! the icosphere. Elevation and biome layers use a hypsometric palette with a
//! hillshade derived from neighboring cells; rivers are polylines whose width
//! follows stream order; political maps tint territory and draw borders from the
//! ownership snapshot nearest the requested year.
//!
//! Everything here is deterministic and self-contained: no fonts are loaded, no
//! tiles are fetched. Labels use a tiny 5x7 bitmap font defined in this module.

use terra_core::biome::Biome;
use terra_core::sphere::Sphere;
use terra_core::tectonics::{BoundaryKind, Stress};
use terra_core::world::World;
use terra_history::civ::History;

use crate::png::RgbImage;

/// Selectable map layers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    /// Biome base with rivers and plate boundaries.
    Atlas,
    /// Hypsometric elevation with hillshade.
    Elevation,
    /// Plate partition with boundary types.
    Plates,
    /// Biome colors.
    Biomes,
    /// Mean annual temperature.
    Temperature,
    /// Annual precipitation.
    Precipitation,
    /// Rivers and lakes on a muted base.
    Rivers,
    /// Political control at a year.
    Political,
    /// Species richness.
    Biodiversity,
    /// Tectonic stress categories.
    Stress,
}

impl Layer {
    /// Every layer in menu order.
    pub const ALL: [Layer; 10] = [
        Layer::Atlas,
        Layer::Elevation,
        Layer::Plates,
        Layer::Biomes,
        Layer::Temperature,
        Layer::Precipitation,
        Layer::Rivers,
        Layer::Political,
        Layer::Biodiversity,
        Layer::Stress,
    ];

    /// Stable identifier used in URLs.
    pub fn code(self) -> &'static str {
        match self {
            Layer::Atlas => "atlas",
            Layer::Elevation => "elevation",
            Layer::Plates => "plates",
            Layer::Biomes => "biomes",
            Layer::Temperature => "temperature",
            Layer::Precipitation => "precipitation",
            Layer::Rivers => "rivers",
            Layer::Political => "political",
            Layer::Biodiversity => "biodiversity",
            Layer::Stress => "stress",
        }
    }

    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Layer::Atlas => "Atlas",
            Layer::Elevation => "Elevation",
            Layer::Plates => "Plates",
            Layer::Biomes => "Biomes",
            Layer::Temperature => "Temperature",
            Layer::Precipitation => "Precipitation",
            Layer::Rivers => "Rivers",
            Layer::Political => "Political",
            Layer::Biodiversity => "Biodiversity",
            Layer::Stress => "Stress",
        }
    }

    /// Looks up a layer by code.
    pub fn from_code(code: &str) -> Option<Layer> {
        Layer::ALL.iter().copied().find(|l| l.code() == code)
    }
}

/// Rendering parameters.
#[derive(Clone, Copy, Debug)]
pub struct RenderOptions {
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Layer to draw.
    pub layer: Layer,
    /// Year for political and civic overlays.
    pub year: Option<u32>,
    /// Draw rivers on top of the chosen layer.
    pub rivers: bool,
    /// Draw political borders and settlements.
    pub civ_overlay: bool,
    /// Apply hillshade to physical layers.
    pub hillshade: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            width: 960,
            height: 480,
            layer: Layer::Atlas,
            year: None,
            rivers: true,
            civ_overlay: false,
            hillshade: true,
        }
    }
}

impl RenderOptions {
    /// Clamps dimensions into a safe range.
    pub fn sanitized(mut self) -> Self {
        self.width = self.width.clamp(64, 2_048);
        self.height = self.height.clamp(32, 1_024);
        self
    }
}

/// A legend entry.
#[derive(Clone, Debug, PartialEq)]
pub struct LegendEntry {
    /// Short label.
    pub label: &'static str,
    /// Packed RGB.
    pub rgb: u32,
}

/// Legend for a layer, used by the Studio footer.
pub fn legend(layer: Layer) -> Vec<LegendEntry> {
    match layer {
        Layer::Biomes | Layer::Atlas => Biome::ALL
            .iter()
            .filter(|b| b.is_land())
            .map(|b| LegendEntry {
                label: b.label(),
                rgb: b.rgb(),
            })
            .collect(),
        Layer::Elevation => vec![
            LegendEntry {
                label: "Ocean",
                rgb: 0x1b3a5c,
            },
            LegendEntry {
                label: "Lowland",
                rgb: 0x6e8e50,
            },
            LegendEntry {
                label: "Upland",
                rgb: 0xb0a06e,
            },
            LegendEntry {
                label: "Summit",
                rgb: 0xf6f6f6,
            },
        ],
        Layer::Plates => vec![
            LegendEntry {
                label: "Convergent",
                rgb: 0xff6b5b,
            },
            LegendEntry {
                label: "Divergent",
                rgb: 0x5bc8ff,
            },
            LegendEntry {
                label: "Transform",
                rgb: 0xf0c75b,
            },
        ],
        Layer::Temperature => vec![
            LegendEntry {
                label: "Cold",
                rgb: 0x2c4f8a,
            },
            LegendEntry {
                label: "Mild",
                rgb: 0x7cc47f,
            },
            LegendEntry {
                label: "Hot",
                rgb: 0xe06a4b,
            },
        ],
        Layer::Precipitation => vec![
            LegendEntry {
                label: "Arid",
                rgb: 0xc9b17a,
            },
            LegendEntry {
                label: "Wet",
                rgb: 0x2f7f57,
            },
            LegendEntry {
                label: "Soaked",
                rgb: 0x2b4f8a,
            },
        ],
        Layer::Rivers => vec![
            LegendEntry {
                label: "River",
                rgb: 0x63b7e6,
            },
            LegendEntry {
                label: "Lake",
                rgb: 0x2f6f8f,
            },
        ],
        Layer::Political => vec![],
        Layer::Biodiversity => vec![
            LegendEntry {
                label: "Poor",
                rgb: 0x3a3a2a,
            },
            LegendEntry {
                label: "Rich",
                rgb: 0x4fd1a5,
            },
        ],
        Layer::Stress => vec![
            LegendEntry {
                label: "Quiet",
                rgb: 0x2b3b46,
            },
            LegendEntry {
                label: "Convergent",
                rgb: 0xc25a4a,
            },
            LegendEntry {
                label: "Divergent",
                rgb: 0x3f7fae,
            },
            LegendEntry {
                label: "Transform",
                rgb: 0xb99a4e,
            },
            LegendEntry {
                label: "Volcanic",
                rgb: 0x9b5bd1,
            },
        ],
    }
}

/// Renders a layer into an RGB image.
pub fn render(world: &World, history: Option<&History>, opts: RenderOptions) -> RgbImage {
    let opts = opts.sanitized();
    let sphere = &world.sphere;
    let shade = if opts.hillshade {
        hillshade(world)
    } else {
        vec![1.0; world.len()]
    };
    let owner: Option<Vec<u16>> =
        if (opts.layer == Layer::Political || opts.civ_overlay) && history.is_some() {
            history
                .and_then(|h| h.snapshot_at(opts.year.unwrap_or(h.config.years)))
                .map(|s| s.owner.clone())
        } else {
            None
        };
    let civ_colors: Vec<[u8; 3]> = history
        .map(|h| h.civs.iter().map(|c| civ_color(c.color_index)).collect())
        .unwrap_or_default();
    let cell_colors = layer_colors(
        world,
        opts.layer,
        &shade,
        owner.as_deref(),
        &civ_colors,
        opts.civ_overlay,
    );
    let mut image = RgbImage::new(opts.width, opts.height, [5, 8, 11]);

    // Base pass: one nearest-cell lookup per pixel.
    let w = opts.width as f64;
    let h = opts.height as f64;
    for py in 0..opts.height {
        let lat = 90.0 - (f64::from(py) + 0.5) / h * 180.0;
        for px in 0..opts.width {
            let lon = (f64::from(px) + 0.5) / w * 360.0 - 180.0;
            let cell = sphere.nearest_lat_lon(lat, lon);
            let mut color = cell_colors[cell as usize];
            // Subtle latitude shading in the deep ocean for depth.
            if opts.layer != Layer::Rivers && opts.layer != Layer::Political {
                let ocean_depth = world.terrain.elevation[cell as usize].min(0.0);
                if ocean_depth < 0.0 {
                    let k = (-ocean_depth / 6_000.0).clamp(0.0, 1.0);
                    color = mix(color, [8, 20, 34], k * 0.55);
                }
            }
            image.set(px as i32, py as i32, color);
        }
    }

    match opts.layer {
        Layer::Plates => draw_boundaries(&mut image, sphere, world),
        Layer::Elevation | Layer::Biomes | Layer::Atlas => {
            if opts.layer == Layer::Atlas {
                draw_boundaries_soft(&mut image, sphere, world);
            }
            if opts.rivers {
                draw_rivers(&mut image, world, 0.9);
            }
        }
        Layer::Rivers => draw_rivers(&mut image, world, 1.0),
        Layer::Political => {}
        _ => {
            if opts.rivers {
                draw_rivers(&mut image, world, 0.7);
            }
        }
    }

    if opts.civ_overlay || opts.layer == Layer::Political {
        draw_civs(&mut image, world, history, opts.year);
    }
    image
}

fn layer_colors(
    world: &World,
    layer: Layer,
    shade: &[f64],
    owner: Option<&[u16]>,
    civ_colors: &[[u8; 3]],
    civ_overlay: bool,
) -> Vec<[u8; 3]> {
    let n = world.len();
    let mut colors = vec![[0u8; 3]; n];
    for i in 0..n {
        let cell = i as u32;
        let elev = world.terrain.elevation[i];
        let s = shade[i];
        colors[i] = match layer {
            Layer::Elevation | Layer::Atlas | Layer::Rivers | Layer::Political => {
                if layer == Layer::Atlas && world.terrain.elevation[i] > 0.0 {
                    let base = rgb(world.biomes.at(cell).rgb());
                    apply_shade(base, s, 0.85 + s * 0.2)
                } else if layer == Layer::Political {
                    // Desaturate the physical base so political colors read
                    // clearly, while retaining enough relief to locate cities.
                    let base = hypsometric(elev, s);
                    if elev > 0.0 {
                        mix(base, [86, 92, 96], 0.52)
                    } else {
                        mix(base, [10, 24, 38], 0.35)
                    }
                } else {
                    hypsometric(elev, s)
                }
            }
            Layer::Plates => plate_color(world, i),
            Layer::Biomes => {
                if elev > 0.0 {
                    apply_shade(rgb(world.biomes.at(cell).rgb()), s, 0.85 + s * 0.2)
                } else {
                    rgb(Biome::Ocean.rgb())
                }
            }
            Layer::Temperature => {
                let t = world.climate.temperature.mean_c[i];
                let t = ramp(t, -35.0, 38.0, &TEMPERATURE_RAMP);
                if elev > 0.0 {
                    apply_shade(t, s, 1.0)
                } else {
                    mix(t, [10, 30, 55], 0.35)
                }
            }
            Layer::Precipitation => {
                let p = world.climate.moisture.precip_mm[i];
                let c = ramp(p, 0.0, 3_500.0, &PRECIP_RAMP);
                if elev > 0.0 {
                    apply_shade(c, s, 1.0)
                } else {
                    mix(c, [10, 30, 55], 0.4)
                }
            }
            Layer::Biodiversity => {
                if elev > 0.0 {
                    let r = f64::from(world.ecosystems.species_richness[i]) / 400.0;
                    ramp(r, 0.0, 1.0, &BIODIVERSITY_RAMP)
                } else {
                    [12, 24, 38]
                }
            }
            Layer::Stress => stress_color(world.tectonics.stress[i]),
        };
        if let Some(owners) = owner {
            if let Some(&id) = owners.get(i) {
                if id != u16::MAX {
                    if let Some(&civ) = civ_colors.get(id as usize) {
                        let strength = if layer == Layer::Political {
                            0.52
                        } else if civ_overlay {
                            0.3
                        } else {
                            0.0
                        };
                        colors[i] = mix(colors[i], civ, strength);
                    }
                }
            }
        }
        // Coastline darkening.
        if elev > 0.0
            && world
                .sphere
                .neighbors(cell)
                .iter()
                .any(|&nb| world.terrain.elevation[nb as usize] <= 0.0)
        {
            colors[i] = mix(colors[i], [12, 20, 26], 0.45);
        }
    }
    colors
}

fn plate_color(world: &World, i: usize) -> [u8; 3] {
    let plate = world.tectonics.plate_of[i] as usize;
    let palette = PLATE_PALETTE;
    let base = palette[plate % palette.len()];
    let uplift = (world.tectonics.uplift[i] / 6_000.0).clamp(0.0, 1.0);
    let mut c = mix(base, [235, 240, 245], uplift * 0.35);
    if world.terrain.elevation[i] <= 0.0 {
        c = mix(c, [8, 20, 32], 0.35);
    }
    c
}

fn stress_color(stress: Stress) -> [u8; 3] {
    match stress {
        Stress::Quiet => [43, 59, 70],
        Stress::Convergent => [194, 90, 74],
        Stress::Divergent => [63, 127, 174],
        Stress::Transform => [185, 154, 78],
        Stress::Volcanic => [155, 91, 209],
    }
}

const PLATE_PALETTE: [[u8; 3]; 12] = [
    [104, 152, 186],
    [186, 141, 104],
    [128, 168, 112],
    [168, 112, 154],
    [112, 130, 184],
    [186, 172, 100],
    [100, 168, 164],
    [184, 116, 118],
    [140, 140, 180],
    [120, 170, 136],
    [180, 148, 120],
    [136, 116, 168],
];

const TEMPERATURE_RAMP: [[u8; 3]; 6] = [
    [38, 54, 120],
    [64, 118, 174],
    [124, 196, 198],
    [180, 214, 140],
    [230, 190, 96],
    [224, 106, 75],
];
const PRECIP_RAMP: [[u8; 3]; 6] = [
    [201, 177, 122],
    [172, 179, 106],
    [120, 163, 90],
    [47, 127, 87],
    [43, 79, 138],
    [70, 60, 150],
];
const BIODIVERSITY_RAMP: [[u8; 3]; 6] = [
    [40, 42, 34],
    [58, 84, 52],
    [64, 120, 78],
    [79, 168, 116],
    [126, 209, 165],
    [200, 245, 220],
];

fn hypsometric(elev: f64, shade: f64) -> [u8; 3] {
    if elev <= 0.0 {
        let depth = (-elev / 6_000.0).clamp(0.0, 1.0);
        mix([52, 108, 126], [8, 20, 40], depth * 0.9)
    } else {
        let e = (elev / 5_000.0).clamp(0.0, 1.0);
        let mut c = ramp(e, 0.0, 1.0, &HYPSOMETRIC_RAMP);
        c = apply_shade(c, shade, 1.0);
        c
    }
}

const HYPSOMETRIC_RAMP: [[u8; 3]; 7] = [
    [72, 110, 72],
    [116, 142, 80],
    [156, 158, 96],
    [176, 154, 110],
    [190, 172, 142],
    [216, 208, 194],
    [246, 246, 246],
];

fn rgb(packed: u32) -> [u8; 3] {
    [
        ((packed >> 16) & 0xff) as u8,
        ((packed >> 8) & 0xff) as u8,
        (packed & 0xff) as u8,
    ]
}

fn mix(a: [u8; 3], b: [u8; 3], t: f64) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| -> u8 {
        (f64::from(x) * (1.0 - t) + f64::from(y) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    [f(a[0], b[0]), f(a[1], b[1]), f(a[2], b[2])]
}

fn apply_shade(c: [u8; 3], shade: f64, strength: f64) -> [u8; 3] {
    let k = (1.0 + (shade - 1.0) * strength).clamp(0.45, 1.65);
    [
        (f64::from(c[0]) * k).round().clamp(0.0, 255.0) as u8,
        (f64::from(c[1]) * k).round().clamp(0.0, 255.0) as u8,
        (f64::from(c[2]) * k).round().clamp(0.0, 255.0) as u8,
    ]
}

fn ramp(value: f64, lo: f64, hi: f64, stops: &[[u8; 3]]) -> [u8; 3] {
    if stops.is_empty() {
        return [0, 0, 0];
    }
    let t = ((value - lo) / (hi - lo)).clamp(0.0, 1.0);
    let pos = t * (stops.len() - 1) as f64;
    let i = pos.floor() as usize;
    if i >= stops.len() - 1 {
        return stops[stops.len() - 1];
    }
    mix(stops[i], stops[i + 1], pos - i as f64)
}

/// Hillshade in `[0.55, 1.4]` from the local elevation gradient.
fn hillshade(world: &World) -> Vec<f64> {
    let sphere = &world.sphere;
    let n = world.len();
    let mut out = vec![1.0f64; n];
    for i in 0..n {
        let cell = i as u32;
        let pos = sphere.pos(cell);
        let (east, north) = pos.tangent_basis();
        let mut east_de: Option<f64> = None;
        let mut north_de: Option<f64> = None;
        for &nb in sphere.neighbors(cell) {
            let toward = (sphere.pos(nb) - pos).normalized();
            if toward.dot(east) > 0.6 {
                east_de = Some(world.terrain.elevation[nb as usize] - world.terrain.elevation[i]);
            }
            if toward.dot(north) > 0.6 {
                north_de = Some(world.terrain.elevation[nb as usize] - world.terrain.elevation[i]);
            }
        }
        let dx = east_de.unwrap_or(0.0);
        let dy = north_de.unwrap_or(0.0);
        // Light from the northwest.
        let slope = (-dx * 0.7 + dy * 0.7) / 900.0;
        out[i] = (1.0 + slope.clamp(-0.8, 0.8) * 0.55).clamp(0.45, 1.5);
    }
    out
}

fn to_pixel(sphere: &Sphere, cell: u32, w: u32, h: u32) -> (f64, f64) {
    let lat = sphere.lat(cell);
    let lon = sphere.lon(cell);
    let x = (lon + 180.0) / 360.0 * f64::from(w);
    let y = (90.0 - lat) / 180.0 * f64::from(h);
    (x, y)
}

fn draw_boundaries(image: &mut RgbImage, sphere: &Sphere, world: &World) {
    let w = image.width();
    let h = image.height();
    for b in &world.tectonics.boundaries {
        let (x0, y0) = to_pixel(sphere, b.a, w, h);
        let (x1, y1) = to_pixel(sphere, b.b, w, h);
        let color = match b.kind {
            BoundaryKind::Convergent => [255, 107, 91],
            BoundaryKind::Divergent => [91, 200, 255],
            BoundaryKind::Transform => [240, 199, 91],
        };
        let radius = if b.kind == BoundaryKind::Convergent {
            1
        } else {
            0
        };
        line_wrapped(image, x0, y0, x1, y1, radius, color);
    }
}

fn draw_boundaries_soft(image: &mut RgbImage, sphere: &Sphere, world: &World) {
    let w = image.width();
    let h = image.height();
    for b in &world.tectonics.boundaries {
        let (x0, y0) = to_pixel(sphere, b.a, w, h);
        let (x1, y1) = to_pixel(sphere, b.b, w, h);
        let color = match b.kind {
            BoundaryKind::Convergent => [210, 120, 100],
            BoundaryKind::Divergent => [100, 160, 190],
            BoundaryKind::Transform => [190, 175, 120],
        };
        line_wrapped_alpha(image, x0, y0, x1, y1, color, 0.22);
    }
}

#[allow(clippy::too_many_arguments)]
fn line_wrapped(
    image: &mut RgbImage,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    radius: i32,
    color: [u8; 3],
) {
    let w = image.width() as f64;
    let mut dx = x1 - x0;
    if dx.abs() > w * 0.5 {
        dx -= w * dx.signum();
    }
    image.line_thick(
        x0.round() as i32,
        y0.round() as i32,
        (x0 + dx).round() as i32,
        y1.round() as i32,
        radius,
        color,
    );
}

fn line_wrapped_alpha(
    image: &mut RgbImage,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    color: [u8; 3],
    alpha: f64,
) {
    let w = image.width() as f64;
    let mut dx = x1 - x0;
    if dx.abs() > w * 0.5 {
        dx -= w * dx.signum();
    }
    let steps = dx.abs().max((y1 - y0).abs()).ceil() as i32 + 1;
    let steps = steps.clamp(1, 4_096);
    for s in 0..=steps {
        let t = f64::from(s) / f64::from(steps);
        let x = (x0 + dx * t).round() as i32;
        let y = (y0 + (y1 - y0) * t).round() as i32;
        image.blend(x, y, color, alpha);
    }
}

fn draw_rivers(image: &mut RgbImage, world: &World, strength: f64) {
    let w = image.width();
    let h = image.height();
    // Lakes first.
    for i in 0..world.len() {
        if world.lake[i] {
            let (x, y) = to_pixel(&world.sphere, i as u32, w, h);
            image.blend(
                x.round() as i32,
                y.round() as i32,
                [47, 111, 143],
                0.85 * strength,
            );
        }
    }
    let river_color = [99, 183, 230];
    for i in 0..world.len() {
        if !world.river[i] {
            continue;
        }
        let cell = i as u32;
        let Some(ds) = world.hydrology.downhill[i] else {
            continue;
        };
        if world.lake[ds as usize] {
            continue;
        }
        let order = world.hydrology.strahler[i];
        let radius = match order {
            0..=1 => 0,
            2..=3 => 1,
            4..=5 => 2,
            _ => 3,
        };
        let (x0, y0) = to_pixel(&world.sphere, cell, w, h);
        let (x1, y1) = to_pixel(&world.sphere, ds, w, h);
        let dx = {
            let mut d = x1 - x0;
            if d.abs() > f64::from(w) * 0.5 {
                d -= f64::from(w) * d.signum();
            }
            d
        };
        let color = mix(river_color, [190, 230, 250], strength * 0.25);
        let x0i = x0.round() as i32;
        let y0i = y0.round() as i32;
        if radius == 0 {
            image.blend(x0i, y0i, color, strength);
        } else {
            image.line_thick(
                x0i,
                y0i,
                (x0 + dx).round() as i32,
                y1.round() as i32,
                radius,
                color,
            );
        }
    }
    // Deltas.
    for i in 0..world.len() {
        if world.hydrology.delta[i] {
            let (x, y) = to_pixel(&world.sphere, i as u32, w, h);
            image.blend(
                x.round() as i32,
                y.round() as i32,
                [126, 186, 150],
                0.7 * strength,
            );
        }
    }
}

const CIV_PALETTE: [[u8; 3]; 16] = [
    [232, 96, 84],
    [90, 170, 220],
    [240, 190, 88],
    [140, 200, 120],
    [190, 120, 210],
    [235, 140, 180],
    [110, 210, 190],
    [220, 160, 110],
    [150, 150, 230],
    [200, 210, 100],
    [120, 190, 140],
    [225, 120, 140],
    [130, 170, 210],
    [210, 180, 150],
    [170, 130, 190],
    [160, 200, 190],
];

/// Civilization color for a palette slot.
pub fn civ_color(index: u8) -> [u8; 3] {
    CIV_PALETTE[index as usize % CIV_PALETTE.len()]
}

fn draw_civs(image: &mut RgbImage, world: &World, history: Option<&History>, year: Option<u32>) {
    let Some(history) = history else {
        return;
    };
    let Some(snapshot) = history.snapshot_at(year.unwrap_or(history.config.years)) else {
        return;
    };
    let w = image.width();
    let h = image.height();
    // Borders between owners and along the edge of claimed territory.
    for cell in 0..world.len() as u32 {
        let owner = snapshot.owner_of(cell);
        let (x0, y0) = to_pixel(&world.sphere, cell, w, h);
        for &nb in world.sphere.neighbors(cell) {
            if nb <= cell {
                continue;
            }
            let other = snapshot.owner_of(nb);
            if other == owner {
                continue;
            }
            let (x1, y1) = to_pixel(&world.sphere, nb, w, h);
            if owner.is_some() || other.is_some() {
                let color = if owner.is_some() && other.is_some() {
                    [8, 11, 14]
                } else {
                    [24, 34, 40]
                };
                line_wrapped(image, x0, y0, x1, y1, 0, color);
            }
        }
    }
    // Cities.
    for civ in &history.civs {
        let color = civ_color(civ.color_index);
        if !civ.alive {
            continue;
        }
        for city in &civ.cities {
            if snapshot.owner_of(city.cell) != Some(civ.id) {
                continue;
            }
            let (x, y) = to_pixel(&world.sphere, city.cell, w, h);
            let xi = x.round() as i32;
            let yi = y.round() as i32;
            if city.capital {
                image.fill_rect(xi - 2, yi - 2, 5, 5, [245, 245, 245]);
                image.fill_rect(xi - 1, yi - 1, 3, 3, color);
            } else {
                image.fill_rect(xi, yi, 2, 2, color);
            }
        }
    }
    // Labels at civ seats.
    let scale = if w >= 700 { 2 } else { 1 };
    for civ in history.civs.iter().filter(|c| c.alive).take(16) {
        let seat = civ.seat();
        let (x, y) = to_pixel(&world.sphere, seat, w, h);
        let label = civ.name.to_uppercase();
        let label_width = (label.chars().count().min(24) as i32) * 6 * scale;
        let start_x = (x.round() as i32 + 4).clamp(2, (w as i32 - label_width).max(2));
        draw_text(
            image,
            start_x,
            (y.round() as i32).clamp(8, h as i32 - 1),
            &label,
            scale,
            [245, 248, 250],
        );
    }
}

const FONT: [(char, [u8; 7]); 39] = [
    (
        'A',
        [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
    ),
    (
        'B',
        [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
    ),
    (
        'C',
        [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
    ),
    (
        'D',
        [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
    ),
    (
        'E',
        [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
    ),
    (
        'F',
        [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
    ),
    (
        'G',
        [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111,
        ],
    ),
    (
        'H',
        [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
    ),
    (
        'I',
        [
            0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
    ),
    (
        'J',
        [
            0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100,
        ],
    ),
    (
        'K',
        [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
    ),
    (
        'L',
        [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
    ),
    (
        'M',
        [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
    ),
    (
        'N',
        [
            0b10001, 0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001,
        ],
    ),
    (
        'O',
        [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        'P',
        [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
    ),
    (
        'Q',
        [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
    ),
    (
        'R',
        [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
    ),
    (
        'S',
        [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
    ),
    (
        'T',
        [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
    ),
    (
        'U',
        [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        'V',
        [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
    ),
    (
        'W',
        [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001,
        ],
    ),
    (
        'X',
        [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
    ),
    (
        'Y',
        [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
    ),
    (
        'Z',
        [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
    ),
    (
        '0',
        [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
    ),
    (
        '1',
        [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
    ),
    (
        '2',
        [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
    ),
    (
        '3',
        [
            0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110,
        ],
    ),
    (
        '4',
        [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
    ),
    (
        '5',
        [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
    ),
    (
        '6',
        [
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        '7',
        [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
    ),
    (
        '8',
        [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
    ),
    (
        '9',
        [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
        ],
    ),
    ('-', [0, 0, 0, 0b11111, 0, 0, 0]),
    ('.', [0, 0, 0, 0, 0, 0b01100, 0b01100]),
    (' ', [0, 0, 0, 0, 0, 0, 0]),
];

fn glyph(ch: char) -> Option<[u8; 7]> {
    FONT.iter()
        .find(|(c, _)| *c == ch.to_ascii_uppercase())
        .map(|(_, g)| *g)
}

/// Draws text with the built-in 5x7 font, with a one-pixel drop shadow.
pub fn draw_text(image: &mut RgbImage, x: i32, y: i32, text: &str, scale: i32, color: [u8; 3]) {
    let scale = scale.clamp(1, 4);
    draw_glyphs(image, x + 1, y + 1, text, scale, [8, 10, 12]);
    draw_glyphs(image, x, y, text, scale, color);
}

fn draw_glyphs(image: &mut RgbImage, x: i32, y: i32, text: &str, scale: i32, color: [u8; 3]) {
    let mut cursor = x;
    let max_x = image.width() as i32 - 6 * scale;
    for ch in text.chars().take(24) {
        if cursor > max_x {
            break;
        }
        if let Some(g) = glyph(ch) {
            for (row, bits) in g.iter().enumerate() {
                for col in 0..5 {
                    if bits & (1 << (4 - col)) != 0 {
                        for sy in 0..scale {
                            for sx in 0..scale {
                                image.set(
                                    cursor + col * scale + sx,
                                    y + row as i32 * scale + sy,
                                    color,
                                );
                            }
                        }
                    }
                }
            }
        }
        cursor += 6 * scale;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use terra_core::world::WorldConfig;
    use terra_history::civ::sim::{simulate, HistoryConfig};

    fn sample() -> (World, History) {
        let w = World::generate(WorldConfig {
            seed: 5,
            subdiv: 2,
            years: 400,
            erosion_iterations: 3,
            ..WorldConfig::default()
        });
        let h = simulate(
            &w,
            HistoryConfig {
                seed: 5,
                years: 400,
                initial_civs: 4,
                snapshot_interval: 100,
                ..HistoryConfig::default()
            },
        );
        (w, h)
    }

    #[test]
    fn every_layer_renders() {
        let (w, h) = sample();
        for layer in Layer::ALL {
            let img = render(
                &w,
                Some(&h),
                RenderOptions {
                    width: 160,
                    height: 80,
                    layer,
                    ..RenderOptions::default()
                },
            );
            assert_eq!(img.width(), 160);
            assert_eq!(img.height(), 80);
            let png = img.to_png();
            assert!(png.len() > 100);
            let uniq: std::collections::BTreeSet<[u8; 3]> = img
                .data()
                .chunks_exact(3)
                .map(|c| [c[0], c[1], c[2]])
                .collect();
            assert!(uniq.len() > 2, "{:?} is uniform", layer);
        }
    }

    #[test]
    fn dimensions_are_clamped() {
        let (w, _) = sample();
        let img = render(
            &w,
            None,
            RenderOptions {
                width: 0,
                height: 99_999,
                ..RenderOptions::default()
            },
        );
        assert_eq!(img.width(), 64);
        assert_eq!(img.height(), 1_024);
    }

    #[test]
    fn rendering_is_deterministic() {
        let (w, h) = sample();
        let a = render(&w, Some(&h), RenderOptions::default());
        let b = render(&w, Some(&h), RenderOptions::default());
        assert_eq!(a.to_png(), b.to_png());
    }

    #[test]
    fn layer_codes_round_trip() {
        for l in Layer::ALL {
            assert_eq!(Layer::from_code(l.code()), Some(l));
            assert!(!l.label().is_empty());
        }
        assert_eq!(Layer::from_code("nope"), None);
    }

    #[test]
    fn legends_are_sensible() {
        for l in Layer::ALL {
            let entries = legend(l);
            for e in &entries {
                assert!(!e.label.is_empty());
                assert!(e.rgb <= 0xff_ffff);
            }
        }
        assert!(!legend(Layer::Biomes).is_empty());
    }

    #[test]
    fn rivers_appear_blue() {
        let (w, _) = sample();
        let img = render(
            &w,
            None,
            RenderOptions {
                layer: Layer::Rivers,
                width: 240,
                height: 120,
                ..RenderOptions::default()
            },
        );
        let blue = img
            .data()
            .chunks_exact(3)
            .filter(|c| i16::from(c[2]) > i16::from(c[0]) + 25 && c[2] > 90)
            .count();
        assert!(blue > 20, "blue pixels {blue}");
    }

    #[test]
    fn political_maps_change_with_year() {
        let (w, h) = sample();
        let a = render(
            &w,
            Some(&h),
            RenderOptions {
                layer: Layer::Political,
                year: Some(0),
                width: 160,
                height: 80,
                ..RenderOptions::default()
            },
        );
        let b = render(
            &w,
            Some(&h),
            RenderOptions {
                layer: Layer::Political,
                year: Some(400),
                width: 160,
                height: 80,
                ..RenderOptions::default()
            },
        );
        assert_ne!(a.to_png(), b.to_png());
    }

    #[test]
    fn text_draw_is_bounded() {
        let mut img = RgbImage::new(32, 16, [0, 0, 0]);
        draw_text(&mut img, 0, 0, "TERRA-42", 1, [255, 255, 255]);
        assert!(img.data().iter().any(|&b| b > 0));
        draw_text(&mut img, 30, 14, "LONG LABEL THAT CLIPS", 4, [255, 0, 0]);
        draw_text(&mut img, -10, -10, "OFFSCREEN", 2, [0, 255, 0]);
    }

    #[test]
    fn ramps_are_bounded() {
        assert_eq!(
            ramp(-100.0, 0.0, 1.0, &BIODIVERSITY_RAMP),
            BIODIVERSITY_RAMP[0]
        );
        assert_eq!(
            ramp(100.0, 0.0, 1.0, &BIODIVERSITY_RAMP),
            BIODIVERSITY_RAMP[5]
        );
        for c in [rgb(0x123456), [1, 2, 3], [200, 100, 50]] {
            let m = mix(c, [255, 255, 255], 0.5);
            assert_eq!(m.len(), 3);
        }
    }

    #[test]
    fn civ_palette_is_distinct() {
        let colors: std::collections::BTreeSet<[u8; 3]> = (0..32).map(civ_color).collect();
        assert_eq!(colors.len(), 16);
    }
}
