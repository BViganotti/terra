//! Command line and browser launcher.
//!
//! The product is the Studio, so the default action is to start the loopback
//! server and open the user's browser. There is no interactive terminal flow.
//! Headless flags exist only for tests, benchmarks and deterministic seed runs;
//! they print a machine-readable report and exit without serving.

use std::io;
use std::time::Instant;

use terra_core::scenario::Scenario;
use terra_core::world::{World, WorldConfig};
use terra_history::civ::sim::{simulate, HistoryConfig};

use crate::json::Json;
use crate::server::{self, ServerConfig};

/// Parsed launcher arguments.
#[derive(Clone, Debug, Default)]
pub struct Args {
    /// Print a world report and exit.
    pub report: bool,
    /// Do not open a browser.
    pub no_open: bool,
    /// Explicit port; zero picks a free port.
    pub port: u16,
    /// World seed.
    pub seed: Option<u64>,
    /// Historical span.
    pub years: Option<u32>,
    /// Icosphere subdivision.
    pub subdiv: Option<u32>,
    /// Preset name.
    pub preset: Option<String>,
    /// Scenario text file.
    pub scenario: Option<String>,
    /// Initial civilizations for the report.
    pub civs: Option<u32>,
    /// Print help text.
    pub help: bool,
}

/// Parses launcher arguments. Unknown flags are reported as errors.
pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Args, String> {
    let mut out = Args::default();
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--report" | "--headless" => out.report = true,
            "--no-open" => out.no_open = true,
            "--help" | "-h" => out.help = true,
            "--port" => {
                let v = iter.next().ok_or("--port needs a value")?;
                out.port = v.parse().map_err(|_| "--port must be a number")?;
            }
            "--seed" => {
                let v = iter.next().ok_or("--seed needs a value")?;
                out.seed = Some(v.parse().map_err(|_| "--seed must be an integer")?);
            }
            "--years" => {
                let v = iter.next().ok_or("--years needs a value")?;
                out.years = Some(v.parse().map_err(|_| "--years must be an integer")?);
            }
            "--subdiv" => {
                let v = iter.next().ok_or("--subdiv needs a value")?;
                out.subdiv = Some(v.parse().map_err(|_| "--subdiv must be an integer")?);
            }
            "--civs" => {
                let v = iter.next().ok_or("--civs needs a value")?;
                out.civs = Some(v.parse().map_err(|_| "--civs must be an integer")?);
            }
            "--preset" => {
                out.preset = Some(iter.next().ok_or("--preset needs a value")?);
            }
            "--scenario" => {
                out.scenario = Some(iter.next().ok_or("--scenario needs a path")?);
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(out)
}

/// Runs the launcher.
pub fn launch() -> io::Result<()> {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("terra: {message}");
            eprintln!("{}", help_text());
            std::process::exit(2);
        }
    };
    if args.help {
        println!("{}", help_text());
        return Ok(());
    }
    if args.report {
        return report(&args);
    }
    let server = server::start(ServerConfig {
        port: args.port,
        token: String::new(),
    })?;
    let url = server::local_url(server.addr, &server.token);
    println!("Terra Studio is running.");
    println!("  {url}");
    if !args.no_open {
        open_browser(&url);
    }
    loop {
        std::thread::park();
    }
}

fn report(args: &Args) -> io::Result<()> {
    let (world_cfg, history_cfg) =
        configs(args).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let started = Instant::now();
    let world = World::generate(world_cfg.clone());
    let world_ms = started.elapsed().as_millis();
    let started = Instant::now();
    let history = simulate(&world, history_cfg);
    let history_ms = started.elapsed().as_millis();

    let stats = &world.stats;
    let hs = &history.stats;
    let report = Json::obj(vec![
        ("seed", Json::num(world.config.seed as f64)),
        ("planet", Json::str(world.name.clone())),
        ("cells", Json::num(world.len() as f64)),
        ("subdiv", Json::num(f64::from(world.config.subdiv))),
        ("years", Json::num(f64::from(history.config.years))),
        ("worldMs", Json::num(world_ms as f64)),
        ("historyMs", Json::num(history_ms as f64)),
        ("worldHash", Json::str(world.hashes.combined.clone())),
        ("chronicleHash", Json::str(history.hashes.chronicle.clone())),
        ("historyHash", Json::str(history.hashes.combined.clone())),
        ("sound", Json::Bool(history.chronicle.is_sound())),
        (
            "world",
            Json::obj(vec![
                ("landFraction", Json::num(stats.land_fraction)),
                ("plates", Json::num(f64::from(stats.plates))),
                ("lakes", Json::num(f64::from(stats.lakes))),
                ("rivers", Json::num(f64::from(stats.river_cells))),
                ("deltas", Json::num(f64::from(stats.delta_cells))),
                ("continents", Json::num(f64::from(stats.continents))),
                ("maxElevationM", Json::num(stats.max_elevation)),
                ("meanTemperatureC", Json::num(stats.mean_temperature_c)),
                ("meanPrecipMm", Json::num(stats.mean_precip_mm)),
                ("meanSpecies", Json::num(stats.mean_species)),
                ("carryingCapacity", Json::num(stats.carrying_capacity)),
            ]),
        ),
        (
            "history",
            Json::obj(vec![
                ("civs", Json::num(history.civs.len() as f64)),
                ("surviving", Json::num(f64::from(hs.surviving_civs))),
                ("collapses", Json::num(f64::from(hs.collapses))),
                ("wars", Json::num(f64::from(hs.wars))),
                ("battles", Json::num(f64::from(hs.battles))),
                ("tradeRoutes", Json::num(f64::from(hs.trade_routes))),
                ("events", Json::num(history.chronicle.len() as f64)),
                ("totalPopulation", Json::num(hs.total_population)),
            ]),
        ),
    ]);
    println!("{}", report.to_string());
    Ok(())
}

fn configs(args: &Args) -> Result<(WorldConfig, HistoryConfig), String> {
    let base = if let Some(path) = &args.scenario {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        terra_core::scenario::parse(&text).map_err(|e| format!("{path}: {e}"))?
    } else if let Some(preset) = &args.preset {
        Scenario::preset(
            preset,
            args.seed.unwrap_or(42),
            args.years.unwrap_or(10_000),
        )
        .ok_or_else(|| format!("unknown preset {preset}"))?
    } else {
        Scenario::from_config(WorldConfig::default())
    };
    let mut world = base.config;
    if let Some(seed) = args.seed {
        world.seed = seed.min(u64::from(u32::MAX));
    }
    if let Some(years) = args.years {
        world.years = years.clamp(100, 20_000);
    }
    if let Some(subdiv) = args.subdiv {
        world.subdiv = subdiv.clamp(2, 5);
    }
    let world = world.sanitized();
    let history = HistoryConfig {
        seed: world.seed,
        years: world.years,
        initial_civs: args.civs.unwrap_or(8).clamp(2, 12),
        step_years: (world.years / 2_000).clamp(5, 25),
        snapshot_interval: (world.years / 100).clamp(50, 1_000),
        ..HistoryConfig::default()
    }
    .sanitized();
    Ok((world, history))
}

fn help_text() -> String {
    "Terra Studio launcher\n\n\
     Usage: terra [options]\n\n\
     Default: start the loopback server and open the browser.\n\n\
     Options:\n\
       --report            generate once, print a JSON report, exit\n\
       --seed N            world seed (default 42)\n\
       --years N           historical span, 100..20000 (default 10000)\n\
       --subdiv N          icosphere level, 2..5 (default 5)\n\
       --civs N            initial civilizations, 2..12 (default 8)\n\
       --preset NAME       temperate | arid | icehouse | archipelago | pangaea\n\
       --scenario PATH     scenario file\n\
       --port N            server port, 0 chooses a free one\n\
       --no-open           do not launch a browser\n\
       --help              show this text\n"
        .to_string()
}

/// Opens a URL with the platform handler. Failures are silent: the URL is also
/// printed so the user can open it manually.
pub fn open_browser(url: &str) {
    let candidates: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("open", &[url])]
    } else if cfg!(target_os = "windows") {
        &[("cmd", &["/C", "start", "", url])]
    } else {
        &[("xdg-open", &[url]), ("gio", &["open", url])]
    };
    for (program, args) in candidates {
        if std::process::Command::new(program)
            .args(*args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_ok()
        {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Result<Args, String> {
        parse_args(items.iter().map(|s| (*s).to_string()))
    }

    #[test]
    fn parses_flags() {
        let a = args(&[
            "--seed",
            "7",
            "--years",
            "500",
            "--subdiv",
            "3",
            "--civs",
            "4",
            "--preset",
            "arid",
            "--report",
            "--no-open",
        ])
        .unwrap();
        assert_eq!(a.seed, Some(7));
        assert_eq!(a.years, Some(500));
        assert_eq!(a.subdiv, Some(3));
        assert_eq!(a.civs, Some(4));
        assert_eq!(a.preset.as_deref(), Some("arid"));
        assert!(a.report);
        assert!(a.no_open);
    }

    #[test]
    fn rejects_unknown_and_missing_values() {
        assert!(args(&["--nope"]).is_err());
        assert!(args(&["--seed"]).is_err());
        assert!(args(&["--seed", "x"]).is_err());
        assert!(args(&["--port", "-1"]).is_err());
    }

    #[test]
    fn configs_are_clamped() {
        let a = args(&[
            "--seed", "9", "--years", "1", "--subdiv", "99", "--civs", "99",
        ])
        .unwrap();
        let (w, h) = configs(&a).unwrap();
        assert_eq!(w.years, 100);
        assert_eq!(w.subdiv, 5);
        assert_eq!(h.initial_civs, 12);
        assert_eq!(h.years, 100);
    }

    #[test]
    fn help_mentions_the_product() {
        assert!(help_text().contains("Terra Studio"));
        assert!(help_text().contains("--report"));
    }
}
