use std::path::PathBuf;

use clap::Parser;
use villa_age::{MapConfig, RunConfig, WINDOWED_PHYSICS_HZ, build_app};

/// Villa Age.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Run without a window or GPU, stepping the simulation as fast as possible.
    #[arg(long)]
    headless: bool,
    /// World seed; the same seed reproduces the same run.
    #[arg(long, default_value_t = 0x5EED_1234)]
    seed: u64,
    /// Initial fast-forward factor (windowed runs). `]` / `[` change it at runtime.
    #[arg(long, default_value_t = 1.0)]
    speed: f32,
    /// Don't wait for the display's refresh between frames.
    #[arg(long)]
    no_vsync: bool,
    /// Stop after this many simulated seconds.
    #[arg(long)]
    duration: Option<f32>,
    /// Headless: simulated seconds per frame (coarser is faster).
    #[arg(long, default_value_t = 1.0 / 60.0)]
    step: f32,
    /// Physics steps per simulated second (lower is faster, less accurate). Defaults to 20 headless
    /// and 64 windowed.
    #[arg(long)]
    physics_hz: Option<f64>,
    /// Map definition (.ron); the built-in default map when omitted.
    #[arg(long)]
    map: Option<PathBuf>,
}

fn main() {
    let cli = Cli::parse();
    let map = match cli.map {
        Some(path) => MapConfig::load(&path).unwrap_or_else(|err| {
            eprintln!("{err}");
            std::process::exit(1);
        }),
        None => MapConfig::default(),
    };
    let config = RunConfig {
        headless: cli.headless,
        seed: cli.seed,
        speed: cli.speed,
        vsync: !cli.no_vsync,
        duration: cli.duration,
        step: cli.step,
        physics_hz: cli.physics_hz.unwrap_or(if cli.headless {
            RunConfig::default().physics_hz
        } else {
            WINDOWED_PHYSICS_HZ
        }),
        map,
    };
    build_app(&config).run();
}
