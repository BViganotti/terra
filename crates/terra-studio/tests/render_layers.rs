//! Renderer and PNG integration tests.
//!
//! The renderer is tested through its public API and the PNG output is decoded
//! independently, so a regression in chunk layout or filtering is visible.

use terra_core::world::{World, WorldConfig};
use terra_history::civ::sim::{simulate, HistoryConfig};
use terra_studio::png::{crc32, zlib_decompress, RgbImage, PNG_SIGNATURE};
use terra_studio::render::{self, Layer, RenderOptions};

fn sample() -> (World, terra_history::civ::History) {
    let world = World::generate(WorldConfig {
        seed: 5,
        subdiv: 2,
        years: 600,
        erosion_iterations: 3,
        ..WorldConfig::default()
    });
    let history = simulate(
        &world,
        HistoryConfig {
            seed: 5,
            years: 600,
            initial_civs: 4,
            snapshot_interval: 100,
            ..HistoryConfig::default()
        },
    );
    (world, history)
}

fn png_chunks(png: &[u8]) -> Vec<(String, Vec<u8>, u32)> {
    let mut out = Vec::new();
    let mut pos = 8;
    while pos + 12 <= png.len() {
        let len = u32::from_be_bytes([png[pos], png[pos + 1], png[pos + 2], png[pos + 3]]) as usize;
        let kind = String::from_utf8_lossy(&png[pos + 4..pos + 8]).to_string();
        let data = png[pos + 8..pos + 8 + len].to_vec();
        let crc = u32::from_be_bytes([
            png[pos + 8 + len],
            png[pos + 9 + len],
            png[pos + 10 + len],
            png[pos + 11 + len],
        ]);
        out.push((kind, data, crc));
        pos += 12 + len;
    }
    assert_eq!(pos, png.len());
    out
}

#[test]
fn every_layer_encodes_a_valid_png() {
    let (world, history) = sample();
    for layer in Layer::ALL {
        let image = render::render(
            &world,
            Some(&history),
            RenderOptions {
                width: 320,
                height: 160,
                layer,
                ..RenderOptions::default()
            },
        );
        assert_eq!(image.width(), 320);
        assert_eq!(image.height(), 160);
        let png = image.to_png();
        assert_eq!(&png[..8], &PNG_SIGNATURE);
        let chunks = png_chunks(&png);
        let kinds: Vec<&str> = chunks.iter().map(|(k, _, _)| k.as_str()).collect();
        assert_eq!(kinds, vec!["IHDR", "IDAT", "IEND"]);
        for (kind, data, crc) in &chunks {
            let mut input = kind.as_bytes().to_vec();
            input.extend_from_slice(data);
            assert_eq!(crc32(&input), *crc, "bad CRC in {kind}");
        }
        let ihdr = &chunks[0].1;
        assert_eq!(
            u32::from_be_bytes([ihdr[0], ihdr[1], ihdr[2], ihdr[3]]),
            320
        );
        assert_eq!(
            u32::from_be_bytes([ihdr[4], ihdr[5], ihdr[6], ihdr[7]]),
            160
        );
        assert_eq!(ihdr[8], 8);
        assert_eq!(ihdr[9], 2);

        let raw = zlib_decompress(&chunks[1].1).unwrap();
        let stride = 320 * 3 + 1;
        assert_eq!(raw.len(), stride * 160);
        for row in raw.chunks_exact(stride) {
            assert_eq!(row[0], 0, "unexpected filter type");
        }
        let unique: std::collections::BTreeSet<u8> = raw.iter().copied().collect();
        assert!(unique.len() > 3, "{:?} looks uniform", layer);
    }
}

#[test]
fn png_pixels_round_trip_through_the_decoder() {
    let mut image = RgbImage::new(64, 32, [0, 0, 0]);
    for y in 0..32 {
        for x in 0..64 {
            image.set(x, y, [(x * 3) as u8, (y * 7) as u8, ((x + y) * 2) as u8]);
        }
    }
    let png = image.to_png();
    let chunks = png_chunks(&png);
    let raw = zlib_decompress(&chunks[1].1).unwrap();
    let stride = 64 * 3 + 1;
    for y in 0..32usize {
        assert_eq!(raw[y * stride], 0);
        for x in 0..64usize {
            let offset = y * stride + 1 + x * 3;
            assert_eq!(
                [raw[offset], raw[offset + 1], raw[offset + 2]],
                image.get(x as i32, y as i32)
            );
        }
    }
}

#[test]
fn political_maps_depend_on_the_requested_year() {
    let (world, history) = sample();
    let render_year = |year: u32| {
        render::render(
            &world,
            Some(&history),
            RenderOptions {
                width: 240,
                height: 120,
                layer: Layer::Political,
                year: Some(year),
                ..RenderOptions::default()
            },
        )
        .to_png()
    };
    assert_ne!(render_year(0), render_year(600));
    assert_eq!(render_year(600), render_year(600));
}

#[test]
fn rivers_and_biomes_use_expected_palettes() {
    let (world, history) = sample();
    let rivers = render::render(
        &world,
        Some(&history),
        RenderOptions {
            width: 320,
            height: 160,
            layer: Layer::Rivers,
            ..RenderOptions::default()
        },
    );
    let blue = rivers
        .data()
        .chunks_exact(3)
        .filter(|c| i16::from(c[2]) > i16::from(c[0]) + 20)
        .count();
    assert!(blue > 100, "no blue river pixels: {blue}");

    let biomes = render::render(
        &world,
        Some(&history),
        RenderOptions {
            width: 320,
            height: 160,
            layer: Layer::Biomes,
            ..RenderOptions::default()
        },
    );
    let unique: std::collections::BTreeSet<[u8; 3]> = biomes
        .data()
        .chunks_exact(3)
        .map(|c| [c[0], c[1], c[2]])
        .collect();
    assert!(unique.len() > 10, "biome palette collapsed");
}
