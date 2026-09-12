//! Villa Age: a village sim. The library builds the app; `main.rs` only parses flags.

use std::time::Duration;

use bevy::app::{PluginsState, ScheduleRunnerPlugin};
use bevy::ecs::schedule::{Schedules, SingleThreadedExecutor};
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::settings::WgpuSettings;
use bevy::time::TimeUpdateStrategy;
use bevy::window::{ExitCondition, PresentMode};
use bevy::winit::WinitPlugin;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

pub mod camera;
pub mod characters;
pub mod history;
pub mod map;
pub mod physics;
pub mod sim;
pub mod trees;

/// Seed for everything procedurally generated (tree placement, seeding, etc.).
#[derive(Resource)]
pub struct WorldSeed(pub u64);

/// The single RNG stream all gameplay randomness draws from, so a seed reproduces a whole run.
#[derive(Resource)]
pub struct GameRng(pub ChaCha8Rng);

/// How to run the simulation.
#[derive(Resource, Clone, Debug)]
pub struct RunConfig {
    /// No window, no GPU: step the sim as fast as the CPU allows.
    pub headless: bool,
    pub seed: u64,
    /// Initial fast-forward factor for windowed runs.
    pub speed: f32,
    pub vsync: bool,
    /// Stop after this many simulated seconds.
    pub duration: Option<f32>,
    /// Simulated seconds per frame in headless mode. Coarser steps run faster; gameplay systems
    /// see larger deltas, physics keeps its own rate.
    pub step: f32,
    /// Physics steps per simulated second.
    pub physics_hz: f64,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            headless: false,
            seed: 0x5EED_1234,
            speed: 1.0,
            vsync: true,
            duration: None,
            step: 1.0 / 60.0,
            physics_hz: 64.0,
        }
    }
}

/// Builds the full game app for the given configuration. Call `.run()` on it, or drive it
/// manually with `App::update` (headless only).
pub fn build_app(config: &RunConfig) -> App {
    let mut app = App::new();

    if config.headless {
        app.add_plugins((
            DefaultPlugins
                .set(RenderPlugin {
                    render_creation: WgpuSettings {
                        backends: None,
                        ..default()
                    }
                    .into(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .disable::<WinitPlugin>(),
            ScheduleRunnerPlugin::run_loop(Duration::ZERO),
        ))
        // Every frame advances the sim by exactly one step, however long it took in wall time.
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
            config.step,
        )));
    } else {
        app.add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Villa Age".into(),
                present_mode: if config.vsync {
                    PresentMode::AutoVsync
                } else {
                    PresentMode::AutoNoVsync
                },
                ..default()
            }),
            ..default()
        }));
    }

    app.insert_resource(Time::<Fixed>::from_hz(config.physics_hz))
        .insert_resource(WorldSeed(config.seed))
        .insert_resource(GameRng(ChaCha8Rng::seed_from_u64(config.seed)))
        .insert_resource(config.clone())
        .add_plugins((
            sim::SimPlugin,
            physics::GamePhysicsPlugin,
            map::MapPlugin,
            trees::TreesPlugin,
            characters::CharactersPlugin,
            history::HistoryPlugin,
            camera::CameraPlugin,
        ));

    if config.headless {
        // The sim's systems are tiny; the multithreaded executor's sync overhead roughly halves
        // headless throughput compared to running everything on one thread.
        let mut schedules = app.world_mut().resource_mut::<Schedules>();
        for (_, schedule) in schedules.iter_mut() {
            schedule.set_executor(SingleThreadedExecutor::new());
        }

        // Finalize plugins now so the app can be driven with `App::update` (tests) as well as
        // `App::run`; the runner skips this step when it has already happened.
        while app.plugins_state() == PluginsState::Adding {
            bevy::tasks::tick_global_task_pools_on_main_thread();
        }
        app.finish();
        app.cleanup();
    }
    app
}
