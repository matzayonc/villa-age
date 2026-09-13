//! Villa Age: a village sim. The library builds the app; `main.rs` only parses flags.

use std::time::Duration;

use bevy::app::{PluginsState, ScheduleRunnerPlugin};
use bevy::ecs::schedule::{Schedules, SingleThreadedExecutor};
use bevy::light::GlobalAmbientLight;
use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::window::PresentMode;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

pub use map::MapConfig;

pub mod camera;
pub mod characters;
pub mod history;
pub mod map;
pub mod physics;
pub mod rabbits;
pub mod sim;
pub mod trees;

/// Default physics rate. Contacts here are simple push-outs plus a slack rope, and the gameplay
/// tolerates 50 ms decisions; lower means faster headless runs.
const PHYSICS_HZ: f64 = 20.0;

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
    /// Simulated seconds per frame in headless mode. Gameplay runs at the physics rate, so a
    /// frame is only overhead: one physics step per frame (`1 / physics_hz`) is the fastest
    /// setting that changes nothing; finer steps only add per-frame work.
    pub step: f64,
    /// Physics (and gameplay) steps per simulated second. The same in every mode, so a seed
    /// reproduces the same run windowed and headless; windowed motion between steps is smoothed
    /// by transform interpolation.
    pub physics_hz: f64,
    /// The map to play on.
    pub map: MapConfig,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            headless: false,
            seed: 0x5EED_1234,
            speed: 1.0,
            vsync: true,
            duration: None,
            step: 1.0 / PHYSICS_HZ,
            physics_hz: PHYSICS_HZ,
            map: MapConfig::default(),
        }
    }
}

/// Builds the full game app for the given configuration. Call `.run()` on it, or drive it
/// manually with `App::update` (headless only).
pub fn build_app(config: &RunConfig) -> App {
    let mut app = App::new();

    if config.headless {
        // Only what the sim needs: no rendering, windowing, input or UI plugins. Their systems
        // would run every frame doing nothing, and that per-frame cost is what bounds how fast
        // a headless run can go. Spawners still attach meshes and materials, so the asset
        // stores exist even though nothing draws them.
        app.add_plugins((
            MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::ZERO)),
            LogPlugin::default(),
            TransformPlugin,
            AssetPlugin::default(),
        ))
        .init_asset::<Mesh>()
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>()
        .init_resource::<GlobalAmbientLight>()
        // Every frame advances the sim by exactly one step, however long it took in wall time.
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
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
        .insert_resource(config.map.clone())
        .add_plugins((
            sim::SimPlugin,
            physics::GamePhysicsPlugin,
            map::MapPlugin,
            trees::TreesPlugin,
            characters::CharactersPlugin,
            rabbits::RabbitsPlugin,
            history::HistoryPlugin,
        ));

    if !config.headless {
        app.add_plugins(camera::CameraPlugin);
    } else {
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
