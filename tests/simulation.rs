//! Headless end-to-end runs of the simulation.

use bevy::prelude::*;
use villa_age::characters::Character;
use villa_age::trees::{MAX_TREES, Tree, TreeState, tree_base};
use villa_age::{RunConfig, build_app};

/// Builds a headless app and steps it for `sim_seconds` of simulated time.
fn run_headless(seed: u64, sim_seconds: f32) -> App {
    let config = RunConfig {
        headless: true,
        seed,
        ..RunConfig::default()
    };
    let mut app = build_app(&config);
    let frames = (sim_seconds / config.step).ceil() as u32;
    for _ in 0..frames {
        app.update();
    }
    app
}

fn tree_count(app: &mut App) -> usize {
    app.world_mut()
        .query_filtered::<(), With<Tree>>()
        .iter(app.world())
        .count()
}

fn count_state(app: &mut App, pred: impl Fn(&TreeState) -> bool) -> usize {
    app.world_mut()
        .query::<&TreeState>()
        .iter(app.world())
        .filter(|s| pred(s))
        .count()
}

/// Snapshot of everything position-like, sorted so two worlds can be compared.
fn snapshot(app: &mut App) -> Vec<[f32; 3]> {
    let world = app.world_mut();
    let mut points: Vec<[f32; 3]> = world
        .query_filtered::<&Transform, With<Tree>>()
        .iter(world)
        .map(|t| tree_base(t).to_array())
        .collect();
    points.extend(
        world
            .query_filtered::<&Transform, With<Character>>()
            .iter(world)
            .map(|t| t.translation.to_array()),
    );
    points.sort_by(|a, b| a.partial_cmp(b).unwrap());
    points
}

#[test]
fn characters_deliver_logs() {
    let mut app = run_headless(1, 180.0);
    let delivered = count_state(&mut app, |s| matches!(s, TreeState::Delivered));
    assert!(
        delivered >= 1,
        "expected at least one delivered log, got {delivered}"
    );
}

#[test]
fn forest_regrows_within_cap() {
    let mut app = run_headless(2, 120.0);
    let trees = tree_count(&mut app);
    assert!(
        trees > 24,
        "expected new saplings beyond the initial 24, got {trees}"
    );
    assert!(trees <= MAX_TREES, "tree cap exceeded: {trees}");
}

#[test]
fn same_seed_same_world() {
    let mut a = run_headless(7, 60.0);
    let mut b = run_headless(7, 60.0);
    let (sa, sb) = (snapshot(&mut a), snapshot(&mut b));
    assert_eq!(sa.len(), sb.len(), "entity counts differ");
    for (pa, pb) in sa.iter().zip(&sb) {
        for i in 0..3 {
            assert!(
                (pa[i] - pb[i]).abs() < 1e-4,
                "worlds diverged: {pa:?} vs {pb:?}"
            );
        }
    }
}
