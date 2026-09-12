//! Headless end-to-end runs of the simulation.

use bevy::prelude::*;
use villa_age::characters::Character;
use villa_age::history::{Action, ActionLog};
use villa_age::trees::{MAX_TREES, Tree, TreeState, tree_base};
use villa_age::{MapConfig, RunConfig, build_app};

/// Builds a headless app on the default map and steps it for `sim_seconds` of simulated time.
fn run_headless(seed: u64, sim_seconds: f32) -> App {
    let mut app = build_app(&RunConfig {
        headless: true,
        seed,
        ..RunConfig::default()
    });
    step(&mut app, sim_seconds);
    app
}

/// Advances a headless app by `sim_seconds` of simulated time.
fn step(app: &mut App, sim_seconds: f32) {
    let step = app.world().resource::<RunConfig>().step;
    let frames = (sim_seconds / step).ceil() as u32;
    for _ in 0..frames {
        app.update();
    }
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
    let mut app = run_headless(1, 45.0);
    let delivered = count_state(&mut app, |s| matches!(s, TreeState::Delivered));
    assert!(
        delivered >= 1,
        "expected at least one delivered log, got {delivered}"
    );
}

#[test]
fn forest_regrows_within_cap() {
    let initial = MapConfig::default().trees.len();
    let mut app = run_headless(2, 60.0);
    let trees = tree_count(&mut app);
    assert!(
        trees > initial,
        "expected new saplings beyond the initial {initial}, got {trees}"
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

#[test]
fn custom_map_spawns_what_it_lists() {
    let map = MapConfig::from_ron(
        r#"(
            size: 20.0,
            characters: [(1.0, 2.0), (-3.0, -4.0)],
            trees: [
                (pos: (5.0, 5.0), maturity: 1.0),
                (pos: (-6.0, 7.0), maturity: 0.5),
                (pos: (8.0, -8.0), maturity: 0.0),
            ],
        )"#,
    )
    .unwrap();
    let config = RunConfig {
        headless: true,
        map,
        ..RunConfig::default()
    };
    let mut app = build_app(&config);
    app.update();

    assert_eq!(tree_count(&mut app), 3);
    let world = app.world_mut();
    let mut characters: Vec<[f32; 2]> = world
        .query_filtered::<&Transform, With<Character>>()
        .iter(world)
        .map(|t| t.translation.xz().to_array())
        .collect();
    characters.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(characters, [[-3.0, -4.0], [1.0, 2.0]]);

    let mut trees: Vec<[f32; 2]> = world
        .query_filtered::<&Transform, With<Tree>>()
        .iter(world)
        .map(|t| tree_base(t).xz().to_array())
        .collect();
    trees.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(trees, [[-6.0, 7.0], [5.0, 5.0], [8.0, -8.0]]);
}

/// One character, one mature tree next to it: the character should walk over, chop it down, wait
/// for it to fall, drag it home and drop it — in that order, with nothing else in between.
#[test]
fn character_runs_through_the_gathering_cycle() {
    let map = MapConfig::from_ron(
        "(size: 20.0, characters: [(0.0, 0.0)], trees: [(pos: (3.0, 0.0), maturity: 1.0)])",
    )
    .unwrap();
    let mut app = build_app(&RunConfig {
        headless: true,
        map,
        ..RunConfig::default()
    });
    step(&mut app, 20.0);

    let world = app.world_mut();
    let tree = world
        .query_filtered::<Entity, With<Tree>>()
        .single(world)
        .unwrap();
    let log = world
        .query_filtered::<&ActionLog, With<Character>>()
        .single(world)
        .unwrap();
    let actions: Vec<Action> = log.iter().map(|e| e.action).collect();
    assert_eq!(
        actions,
        [
            Action::WalkTo { tree },
            Action::Chop { tree },
            Action::AwaitFall { tree },
            Action::Haul { tree },
            Action::Deliver { tree },
            Action::Idle,
        ]
    );

    // Chopping a fully grown tree takes a few seconds; hauling it home a few more.
    let at: Vec<f32> = log.iter().map(|e| e.at).collect();
    let chop_time = at[2] - at[1];
    assert!((3.0..6.0).contains(&chop_time), "chop took {chop_time}s");
    let haul_time = at[4] - at[3];
    assert!((0.3..5.0).contains(&haul_time), "haul took {haul_time}s");

    // The log ends up delivered, lying near home.
    let (state, transform) = world
        .query::<(&TreeState, &Transform)>()
        .single(world)
        .unwrap();
    assert!(matches!(state, TreeState::Delivered));
    let base = tree_base(transform);
    assert!(base.xz().length() < 3.0, "log left at {base}");
}

#[test]
fn map_rejects_out_of_bounds_tree() {
    let err = MapConfig::from_ron(
        "(size: 20.0, characters: [], trees: [(pos: (50.0, 0.0), maturity: 1.0)])",
    )
    .unwrap_err();
    assert!(err.contains("tree 0"), "unexpected error: {err}");
}
