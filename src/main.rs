use clap::Parser;
use villa_age::{RunConfig, build_app};

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
    /// Physics steps per simulated second (lower is faster, less accurate).
    #[arg(long, default_value_t = 64.0)]
    physics_hz: f64,
}

fn main() {
    let cli = Cli::parse();
    let config = RunConfig {
        headless: cli.headless,
        seed: cli.seed,
        speed: cli.speed,
        vsync: !cli.no_vsync,
        duration: cli.duration,
        step: cli.step,
        physics_hz: cli.physics_hz,
    };
    build_app(&config).run();
}
