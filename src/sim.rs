//! Simulation control: deterministic system ordering, fast-forward, run stats and timed exit.

use std::time::{Duration, Instant};

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::RunConfig;
use crate::trees::{Tree, TreeState};

/// Order of the gameplay systems within `Update`. Fixed so a seed reproduces a run.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SimSet {
    Trees,
    Characters,
    History,
    Camera,
}

/// Fast-forward factors cycled with `]` and `[`.
const SPEEDS: [f32; 4] = [1.0, 4.0, 16.0, 64.0];
/// Simulated seconds between stats lines.
const STATS_INTERVAL: f32 = 10.0;

/// Current fast-forward setting.
#[derive(Resource)]
struct FastForward {
    speed: f32,
    paused: bool,
}

pub struct SimPlugin;

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        // Gameplay that moves bodies runs in `FixedUpdate` (at the physics rate, on the physics
        // pose); frame-rate things (visuals, UI, camera) in `Update`. Same order in both.
        let order = || {
            (
                SimSet::Trees,
                SimSet::Characters,
                SimSet::History,
                SimSet::Camera,
            )
                .chain()
        };
        app.configure_sets(FixedUpdate, order())
            .configure_sets(Update, order())
            .init_resource::<Stats>()
            .add_systems(Startup, apply_initial_speed)
            .add_systems(Update, (report_stats, exit_when_done));

        // Keyboard control needs the input plugin, which headless runs don't have.
        if !is_headless(app) {
            app.add_systems(Update, (fast_forward_keys, update_title));
        }
    }
}

/// Whether the app being built runs without a window.
pub fn is_headless(app: &App) -> bool {
    app.world()
        .get_resource::<RunConfig>()
        .is_some_and(|c| c.headless)
}

fn apply_initial_speed(
    mut commands: Commands,
    config: Res<RunConfig>,
    mut time: ResMut<Time<Virtual>>,
) {
    let speed = config.speed.max(f32::EPSILON);
    apply_speed(&mut time, speed, config.step as f32);
    commands.insert_resource(FastForward {
        speed,
        paused: false,
    });
}

/// Sets the virtual clock's speed and raises its per-frame delta clamp so the speed isn't
/// silently capped at low frame rates.
fn apply_speed(time: &mut Time<Virtual>, speed: f32, step: f32) {
    time.set_relative_speed(speed);
    let max_delta = Duration::from_secs_f32(step * speed * 4.0);
    time.set_max_delta(max_delta.max(Duration::from_millis(250)));
}

/// `]` / `[` cycle the fast-forward factor, `Space` pauses.
fn fast_forward_keys(
    keys: Res<ButtonInput<KeyCode>>,
    config: Res<RunConfig>,
    mut ff: ResMut<FastForward>,
    mut time: ResMut<Time<Virtual>>,
) {
    let index = SPEEDS
        .iter()
        .position(|&s| s >= ff.speed)
        .unwrap_or(SPEEDS.len() - 1);
    let mut changed = false;
    if keys.just_pressed(KeyCode::BracketRight) && index + 1 < SPEEDS.len() {
        ff.speed = SPEEDS[index + 1];
        changed = true;
    }
    if keys.just_pressed(KeyCode::BracketLeft) && index > 0 {
        ff.speed = SPEEDS[index - 1];
        changed = true;
    }
    if keys.just_pressed(KeyCode::Space) {
        ff.paused = !ff.paused;
        if ff.paused {
            time.pause();
        } else {
            time.unpause();
        }
    }
    if changed {
        apply_speed(&mut time, ff.speed, config.step as f32);
    }
}

fn update_title(ff: Res<FastForward>, mut window: Single<&mut Window>) {
    if !ff.is_changed() {
        return;
    }
    let mut title = format!("Villa Age — {}×", ff.speed);
    if ff.paused {
        title.push_str(" (paused)");
    }
    window.title = title;
}

/// Wall-clock bookkeeping for the stats line. `Time<Real>` can't be used: in headless mode it is
/// the clock being stepped manually.
#[derive(Resource)]
struct Stats {
    started: Instant,
    next_report: f32,
    /// Frames and wall time since the last report, for the frame rate.
    frames: u32,
    last_report: Instant,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            next_report: 0.0,
            frames: 0,
            last_report: Instant::now(),
        }
    }
}

fn report_stats(
    time: Res<Time<Virtual>>,
    mut stats: ResMut<Stats>,
    trees: Query<&TreeState, With<Tree>>,
    bodies: Query<(), With<RigidBody>>,
) {
    let sim = time.elapsed_secs();
    stats.frames += 1;
    if sim < stats.next_report {
        return;
    }
    stats.next_report = sim + STATS_INTERVAL;
    let fps = stats.frames as f32 / stats.last_report.elapsed().as_secs_f32().max(1e-3);
    stats.frames = 0;
    stats.last_report = Instant::now();

    let (mut standing, mut delivered) = (0, 0);
    for state in &trees {
        match state {
            TreeState::Standing { .. } => standing += 1,
            TreeState::Delivered => delivered += 1,
            _ => {}
        }
    }
    let wall = stats.started.elapsed().as_secs_f32();
    info!(
        "sim {sim:.0}s | wall {wall:.1}s | {:.1}x | {fps:.0} fps | trees {} (standing {standing}, delivered {delivered}) | bodies {}",
        sim / wall.max(1e-3),
        trees.iter().len(),
        bodies.iter().len(),
    );
}

fn exit_when_done(
    config: Res<RunConfig>,
    time: Res<Time<Virtual>>,
    mut exit: MessageWriter<AppExit>,
) {
    if config.duration.is_some_and(|d| time.elapsed_secs() >= d) {
        exit.write(AppExit::Success);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_speed_sets_rate_and_keeps_a_sane_delta_clamp() {
        let mut time = Time::<Virtual>::default();
        let step = 1.0 / 60.0;

        apply_speed(&mut time, 1.0, step);
        assert_eq!(time.relative_speed(), 1.0);
        // 4 frames at 1× is well under the 250ms floor.
        assert_eq!(time.max_delta(), Duration::from_millis(250));

        apply_speed(&mut time, 64.0, step);
        assert_eq!(time.relative_speed(), 64.0);
        let expected = Duration::from_secs_f32(step * 64.0 * 4.0);
        assert_eq!(time.max_delta(), expected);
        assert!(expected > Duration::from_millis(250));
    }
}
