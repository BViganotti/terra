//! Terra timing harness.
//!
//! No external benchmarking crates. Each workload is run several times and the
//! median, minimum and maximum wall-clock times are printed. Numbers are
//! measured, never estimated; the final JSON line is copied into
//! `FINAL_REPORT.md`.
//!
//! Run with `cargo run --release -p terra-bench`.

use std::hint::black_box;
use std::time::{Duration, Instant};

use terra_core::scenario;
use terra_core::world::{World, WorldConfig};
use terra_history::civ::sim::{simulate, HistoryConfig};
use terra_studio::json;
use terra_studio::png::RgbImage;
use terra_studio::render::{self, Layer, RenderOptions};

struct Sample {
    median: Duration,
    min: Duration,
    max: Duration,
}

fn measure<F, T>(runs: usize, mut f: F) -> Sample
where
    F: FnMut() -> T,
{
    let mut times = Vec::with_capacity(runs.max(1));
    for _ in 0..runs.max(1) {
        let started = Instant::now();
        black_box(f());
        times.push(started.elapsed());
    }
    times.sort();
    let median = times[times.len() / 2];
    Sample {
        median,
        min: times[0],
        max: times[times.len() - 1],
    }
}

fn row(name: &str, s: &Sample) {
    println!(
        "{:<34} {:>9.2} {:>9.2} {:>9.2}",
        name,
        s.median.as_secs_f64() * 1_000.0,
        s.min.as_secs_f64() * 1_000.0,
        s.max.as_secs_f64() * 1_000.0
    );
}

fn world_config(seed: u64, subdiv: u32) -> WorldConfig {
    WorldConfig {
        seed,
        subdiv,
        years: 1_000,
        ..WorldConfig::default()
    }
}

fn main() {
    println!("Terra benchmark (release build)");
    println!(
        "{:<34} {:>9} {:>9} {:>9}",
        "workload", "median ms", "min ms", "max ms"
    );

    // Geology, climate and biosphere at each supported resolution.
    let mut world5 = None;
    let mut world5_sample = None;
    for subdiv in 2..=5u32 {
        let cfg = world_config(42, subdiv);
        let sample = measure(3, || World::generate(cfg.clone()));
        let cells = terra_core::cells_for_subdiv(subdiv);
        row(&format!("world subdiv={subdiv} ({cells} cells)"), &sample);
        if subdiv == 5 {
            world5 = Some(World::generate(cfg));
            world5_sample = Some(sample);
        }
    }
    let world5 = world5.expect("subdivision 5 world");
    let world5_sample = world5_sample.expect("world timing");

    // Full simulation at four and five subdivisions.
    let history_cfg = HistoryConfig {
        seed: 42,
        years: 10_000,
        initial_civs: 8,
        ..HistoryConfig::default()
    };
    let world4 = World::generate(world_config(42, 4));
    let history4 = simulate(&world4, history_cfg);
    let history_sample = measure(3, || simulate(&world4, history_cfg));
    row("history 10k years subdiv=4", &history_sample);

    let history5 = simulate(&world5, history_cfg);
    let history5_sample = measure(2, || simulate(&world5, history_cfg));
    row("history 10k years subdiv=5", &history5_sample);

    // Query and export workloads use the four-subdivision history.
    let query = measure(10, || {
        let mut n = 0usize;
        for event in history4.chronicle.events().iter().step_by(7) {
            n += history4
                .chronicle
                .cause_chain(event.id)
                .map(|c| c.len())
                .unwrap_or(0);
        }
        n
    });
    row("cause chains (every 7th event)", &query);
    let filtered = measure(100, || {
        history4
            .chronicle
            .query()
            .years(1_000, 6_000)
            .kind(terra_history::chronicle::EventKind::Expansion)
            .page(0, 100)
            .len()
    });
    row("filtered chronicle page", &filtered);
    let markdown = measure(5, || {
        terra_history::chronicle::export::markdown(&history4.world_name, &history4.chronicle)
    });
    row("chronicle export markdown", &markdown);
    let chronicle_json = measure(5, || {
        terra_history::chronicle::export::json(&history4.world_name, &history4.chronicle)
    });
    row("chronicle export json", &chronicle_json);

    // Map rendering at Studio resolution.
    let render_history = simulate(
        &world5,
        HistoryConfig {
            seed: 42,
            years: 1_000,
            initial_civs: 6,
            ..HistoryConfig::default()
        },
    );
    let mut atlas_image = None;
    let mut atlas_sample = None;
    for layer in Layer::ALL {
        let opts = RenderOptions {
            width: 1_024,
            height: 512,
            layer,
            ..RenderOptions::default()
        };
        let sample = measure(3, || render::render(&world5, Some(&render_history), opts));
        row(&format!("render {}", layer.code()), &sample);
        if layer == Layer::Atlas {
            atlas_image = Some(render::render(&world5, Some(&render_history), opts));
            atlas_sample = Some(sample);
        }
    }
    let image: RgbImage = atlas_image.expect("atlas image");
    let png = measure(5, || image.to_png());
    row("PNG encode 1024x512", &png);

    // Parser throughput.
    let valid_json =
        "{\"seed\":42,\"years\":10000,\"subdiv\":5,\"civs\":8,\"preset\":\"temperate\"}";
    let json_parse = measure(10, || {
        let mut n = 0usize;
        for _ in 0..1_000 {
            if let Ok(v) = json::parse(valid_json) {
                n += v.get("seed").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
            }
        }
        n
    });
    row("JSON parse 1000 valid docs", &json_parse);
    let bad_json = "{\"seed\":42,\"years\":}";
    let json_reject = measure(10, || {
        let mut n = 0usize;
        for _ in 0..1_000 {
            if json::parse(bad_json).is_err() {
                n += 1;
            }
        }
        n
    });
    row("JSON reject 1000 malformed docs", &json_reject);

    let scenario_text = "name = \"Bench\"\nseed = 42\nsubdiv = 4\nyears = 5000\nplates = 9\n";
    let scenario_parse = measure(10, || {
        let mut n = 0usize;
        for _ in 0..1_000 {
            if scenario::parse(scenario_text).is_ok() {
                n += 1;
            }
        }
        n
    });
    row("scenario parse 1000 docs", &scenario_parse);

    println!();
    println!(
        "{{\"world5_ms\":{:.2},\"history5_ms\":{:.2},\"render_atlas_ms\":{:.2},\"png_ms\":{:.2},\"events4\":{},\"events5\":{},\"cells5\":{},\"civs5\":{},\"surviving5\":{}}}",
        world5_sample.median.as_secs_f64() * 1_000.0,
        history5_sample.median.as_secs_f64() * 1_000.0,
        atlas_sample.expect("atlas timing").median.as_secs_f64() * 1_000.0,
        png.median.as_secs_f64() * 1_000.0,
        history4.chronicle.len(),
        history5.chronicle.len(),
        world5.len(),
        history5.civs.len(),
        history5.stats.surviving_civs
    );
}
