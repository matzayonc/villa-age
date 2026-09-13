//! Headless end-to-end runs of the simulation.

use avian3d::prelude::*;
use bevy::prelude::*;
use std::collections::HashMap;

use villa_age::characters::{Character, STAT_RANGE, Speed, Strength};
use villa_age::history::{Action, ActionLog, Entry};
use villa_age::trees::{MAX_TREES, Maturity, TRUNK_RADIUS, Tree, TreeState, tree_base};
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
    let step = app.world().resource::<RunConfig>().step as f32;
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
        .query_filtered::<(&Position, &Rotation, &Maturity), With<Tree>>()
        .iter(world)
        .map(|(p, r, &m)| tree_base(p, r, m).to_array())
        .collect();
    points.extend(
        world
            .query_filtered::<&Position, With<Character>>()
            .iter(world)
            .map(|p| p.to_array()),
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

/// Mature trees left alone drop saplings. No characters on this map, so regrowth can't lose a
/// race against the loggers.
#[test]
fn forest_regrows_within_cap() {
    let map = MapConfig::from_ron(
        "(size: 30.0, characters: [], trees: [
            (pos: (-6.0, -6.0), maturity: 1.0), (pos: (6.0, -6.0), maturity: 1.0),
            (pos: (-6.0, 6.0), maturity: 1.0), (pos: (6.0, 6.0), maturity: 1.0),
        ])",
    )
    .unwrap();
    let initial = map.trees.len();
    let mut app = build_app(&RunConfig {
        headless: true,
        seed: 2,
        map,
        ..RunConfig::default()
    });
    step(&mut app, 60.0);
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
        .query_filtered::<&Position, With<Character>>()
        .iter(world)
        .map(|p| p.xz().to_array())
        .collect();
    characters.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(characters, [[-3.0, -4.0], [1.0, 2.0]]);

    let mut trees: Vec<[f32; 2]> = world
        .query_filtered::<(&Position, &Rotation, &Maturity), With<Tree>>()
        .iter(world)
        .map(|(p, r, &m)| tree_base(p, r, m).xz().to_array())
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
    let (log, strength, speed) = world
        .query_filtered::<(&ActionLog, &Strength, &Speed), With<Character>>()
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

    // Chopping a fully grown tree takes ~4s of work, scaled by strength; hauling it home ~0.7s
    // of walking, scaled by speed.
    let at: Vec<f32> = log.iter().map(|e| e.at).collect();
    let chop_time = at[2] - at[1];
    let expected_chop = 4.0 / strength.0;
    assert!(
        (chop_time - expected_chop).abs() < 0.1,
        "chop took {chop_time}s, expected {expected_chop}s at {strength:?}"
    );
    let haul_time = at[4] - at[3];
    let expected_haul = 0.72 / speed.0;
    assert!(
        (haul_time - expected_haul).abs() < 0.2,
        "haul took {haul_time}s, expected {expected_haul}s at {speed:?}"
    );

    // The log ends up delivered, lying near home.
    let (state, position, rotation, &maturity) = world
        .query::<(&TreeState, &Position, &Rotation, &Maturity)>()
        .single(world)
        .unwrap();
    assert!(matches!(state, TreeState::Delivered));
    let base = tree_base(position, rotation, maturity);
    assert!(base.xz().length() < 3.0, "log left at {base}");
}

/// Runs one character toward one mature tree at `(8, 0)`, optionally with a delivered log lying
/// across the path at `x = 4`. Returns the seconds spent walking before the first chop and the
/// highest the character stood along the way.
fn walk_to_tree(seed: u64, with_log: bool) -> (f32, f32) {
    let map = MapConfig::from_ron(
        "(size: 20.0, characters: [(0.0, 0.0)], trees: [
            (pos: (8.0, 0.0), maturity: 1.0),
            (pos: (4.0, 0.0), maturity: 0.5),
        ])",
    )
    .unwrap();
    let mut app = build_app(&RunConfig {
        headless: true,
        seed,
        map,
        ..RunConfig::default()
    });
    app.update();

    // Turn the sapling at x = 4 (too young to be a target) into a full-size log lying along Z
    // across the character's path, or move it out of the way entirely.
    let world = app.world_mut();
    let (mut state, mut position, mut rotation, _) = world
        .query_filtered::<(&mut TreeState, &mut Position, &mut Rotation, &Maturity), With<Tree>>()
        .iter_mut(world)
        .find(|(_, p, r, m)| tree_base(p, r, **m).x < 6.0)
        .unwrap();
    if with_log {
        *state = TreeState::Delivered;
        *position = Position::from_xyz(4.0, TRUNK_RADIUS, 0.0);
        *rotation = Rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2));
    } else {
        *position = Position::from_xyz(-8.0, position.y, -8.0);
    }

    let step_secs = app.world().resource::<RunConfig>().step as f32;
    let mut max_y = f32::MIN;
    for _ in 0..(10.0 / step_secs) as u32 {
        app.update();
        let world = app.world_mut();
        let (position, children, log) = world
            .query_filtered::<(&Position, &Children, &ActionLog), With<Character>>()
            .single(world)
            .unwrap();
        let entries: Vec<Entry> = log.iter().copied().collect();
        if matches!(entries.last().unwrap().action, Action::Chop { .. }) {
            assert_eq!(entries.len(), 2, "unexpected detour: {entries:?}");
            return (entries[1].at - entries[0].at, max_y);
        }
        // The visual is a child of the body; it's what rises over a log.
        let body_y = position.y;
        let lift = children
            .iter()
            .filter_map(|child| world.get::<Transform>(child))
            .map(|t| t.translation.y)
            .fold(0.0, f32::max);
        max_y = max_y.max(body_y + lift);
    }
    panic!("character never reached the tree (with_log = {with_log})");
}

#[test]
fn characters_climb_over_logs_slowly() {
    let (clear_time, clear_y) = walk_to_tree(5, false);
    let (log_time, log_y) = walk_to_tree(5, true);

    assert!(
        log_time > clear_time + 0.3,
        "crossing a log should cost time: {log_time}s with vs {clear_time}s without"
    );
    assert!(
        log_time < clear_time + 3.0,
        "climb took too long: {log_time}s"
    );
    assert!(
        log_y > clear_y + 0.1,
        "character never rose over the log: {log_y} vs {clear_y}"
    );

    // Back on the ground once past it.
    let ground = clear_y;
    assert!(ground > 0.0);
}

/// Seconds a character may sit still while it's supposed to be walking or hauling.
const STALL_LIMIT: f32 = 5.0;

/// Nobody should freeze mid-walk: logs pile up around homes and characters cross them, and a
/// hauled log must never hold its hauler in place.
#[test]
fn nobody_gets_stuck() {
    let mut app = build_app(&RunConfig {
        headless: true,
        seed: 1,
        ..RunConfig::default()
    });
    let step = app.world().resource::<RunConfig>().step as f32;
    // Where each character was last seen moving (or doing something stationary), and when.
    let mut last_moved: HashMap<Entity, (Vec2, f32)> = HashMap::new();

    for frame in 0..(120.0 / step) as u32 {
        app.update();
        let now = frame as f32 * step;
        let world = app.world_mut();
        for (entity, position, log) in world
            .query_filtered::<(Entity, &Position, &ActionLog), With<Character>>()
            .iter(world)
        {
            let pos = position.xz();
            // Gameplay runs with the physics step; the very first frame has none yet.
            let Some(entry) = log.current() else {
                continue;
            };
            let action = entry.action;
            let should_move = matches!(action, Action::WalkTo { .. } | Action::Haul { .. });
            let seen = last_moved.entry(entity).or_insert((pos, now));
            if !should_move || pos.distance(seen.0) > 0.05 {
                *seen = (pos, now);
            }
            let stalled = now - seen.1;
            assert!(
                stalled <= STALL_LIMIT,
                "{entity} sat at {pos:.2} for {stalled:.1}s while in {action:?} (t = {now:.1}s)"
            );
        }
    }
}

fn stats(app: &mut App) -> Vec<(f32, f32)> {
    let world = app.world_mut();
    let mut stats: Vec<(f32, f32)> = world
        .query_filtered::<(&Strength, &Speed), With<Character>>()
        .iter(world)
        .map(|(s, v)| (s.0, v.0))
        .collect();
    stats.sort_by(|a, b| a.partial_cmp(b).unwrap());
    stats
}

#[test]
fn character_stats_are_rolled_in_range_and_seeded() {
    let mut a = build_app(&RunConfig {
        headless: true,
        seed: 3,
        ..RunConfig::default()
    });
    a.update();
    let sa = stats(&mut a);
    assert_eq!(sa.len(), MapConfig::default().characters.len());
    for &(strength, speed) in &sa {
        assert!(STAT_RANGE.contains(&strength), "strength {strength}");
        assert!(STAT_RANGE.contains(&speed), "speed {speed}");
    }
    let strengths: Vec<f32> = sa.iter().map(|s| s.0).collect();
    assert!(
        strengths.windows(2).any(|w| w[0] != w[1]),
        "every character rolled the same strength: {strengths:?}"
    );

    let mut b = build_app(&RunConfig {
        headless: true,
        seed: 3,
        ..RunConfig::default()
    });
    b.update();
    assert_eq!(sa, stats(&mut b), "same seed must roll the same stats");

    let mut c = build_app(&RunConfig {
        headless: true,
        seed: 4,
        ..RunConfig::default()
    });
    c.update();
    assert_ne!(
        sa,
        stats(&mut c),
        "different seeds should roll different stats"
    );
}

#[test]
fn map_rejects_out_of_bounds_tree() {
    let err = MapConfig::from_ron(
        "(size: 20.0, characters: [], trees: [(pos: (50.0, 0.0), maturity: 1.0)])",
    )
    .unwrap_err();
    assert!(err.contains("tree 0"), "unexpected error: {err}");
}

/// Load test: 1 000 characters among 10 000 trees. Ignored by default because it takes seconds
/// even in release; run with `cargo test --release --test simulation heavy -- --ignored --nocapture`
/// to get the timing report.
#[test]
#[ignore = "benchmark: run explicitly with --ignored --nocapture"]
fn heavy_world_keeps_stepping() {
    use std::time::Instant;
    use villa_age::map::TreeSpec;

    const TREES_PER_SIDE: usize = 100;
    const TREE_SPACING: f32 = 3.0;
    const CHARACTERS: usize = 1_000;
    const SIM_SECONDS: f32 = 30.0;

    // Trees on a square grid; characters at the centres of a subset of the cells, so each sits
    // on the diagonal between four trees and never inside a trunk.
    let origin = -(TREES_PER_SIDE as f32 - 1.0) * TREE_SPACING / 2.0;
    let trees: Vec<TreeSpec> = (0..TREES_PER_SIDE * TREES_PER_SIDE)
        .map(|i| {
            let (x, z) = ((i % TREES_PER_SIDE) as f32, (i / TREES_PER_SIDE) as f32);
            TreeSpec {
                pos: (origin + x * TREE_SPACING, origin + z * TREE_SPACING),
                maturity: 0.5 + 0.5 * ((i * 7919) % 100) as f32 / 100.0,
            }
        })
        .collect();
    let chars_per_side = (CHARACTERS as f32).sqrt().ceil() as usize;
    let cell_centre = |k: usize| {
        let cell = k * (TREES_PER_SIDE - 1) / chars_per_side;
        origin + (cell as f32 + 0.5) * TREE_SPACING
    };
    let characters: Vec<(f32, f32)> = (0..CHARACTERS)
        .map(|i| {
            (
                cell_centre(i % chars_per_side),
                cell_centre(i / chars_per_side),
            )
        })
        .collect();
    let map = MapConfig {
        size: TREES_PER_SIDE as f32 * TREE_SPACING + 8.0,
        texture: None,
        characters,
        trees,
    };

    let build_start = Instant::now();
    let mut app = build_app(&RunConfig {
        headless: true,
        seed: 42,
        map,
        ..RunConfig::default()
    });
    let build = build_start.elapsed();

    let first_frame_start = Instant::now();
    app.update();
    let first_frame = first_frame_start.elapsed();

    let step_secs = app.world().resource::<RunConfig>().step as f32;
    let frames = (SIM_SECONDS / step_secs).ceil() as u32;
    let run_start = Instant::now();
    for _ in 0..frames {
        app.update();
    }
    let run = run_start.elapsed();

    let trees = tree_count(&mut app);
    let chars = app
        .world_mut()
        .query_filtered::<(), With<Character>>()
        .iter(app.world())
        .count();
    let delivered = count_state(&mut app, |s| matches!(s, TreeState::Delivered));
    // Logs that slipped away mid-haul: a sign the rope or arrival logic is struggling.
    let lost: usize = app
        .world_mut()
        .query::<&ActionLog>()
        .iter(app.world())
        .map(|log| {
            log.iter()
                .filter(|e| matches!(e.action, Action::LostLog { .. }))
                .count()
        })
        .sum();

    let per_frame = run / frames;
    eprintln!(
        "heavy world: {chars} characters, {trees} trees\n  \
         build app        {build:>9.2?}\n  \
         first frame      {first_frame:>9.2?}  (startup systems + spawning)\n  \
         {frames} frames      {run:>9.2?}  ({per_frame:.2?}/frame, {:.1}x realtime)\n  \
         delivered logs   {delivered}  (lost mid-haul: {lost})",
        SIM_SECONDS / run.as_secs_f32(),
    );

    assert_eq!(chars, CHARACTERS);
    assert_eq!(trees, TREES_PER_SIDE * TREES_PER_SIDE);
    assert!(delivered > 0, "nobody delivered a log in {SIM_SECONDS}s");
}
