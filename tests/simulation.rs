//! Headless end-to-end runs of the simulation.

use avian3d::prelude::*;
use bevy::prelude::*;
use std::collections::HashMap;

use villa_age::entities::carrots::{Carrot, MAX_CARROTS};
use villa_age::entities::rabbits::{Appetite, Breeding, MAX_RABBITS, Rabbit};
use villa_age::entities::trees::{MAX_TREES, Maturity, TRUNK_RADIUS, Tree, TreeState, tree_base};
use villa_age::entities::villagers::{Climbing, STAT_RANGE, Speed, Strength, Villager};
use villa_age::history::{Action, ActionLog, Entry};
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
            .query_filtered::<&Position, With<Villager>>()
            .iter(world)
            .map(|p| p.to_array()),
    );
    points.sort_by(|a, b| a.partial_cmp(b).unwrap());
    points
}

#[test]
fn villagers_deliver_logs() {
    let mut app = run_headless(1, 45.0);
    let delivered = count_state(&mut app, |s| matches!(s, TreeState::Delivered));
    assert!(
        delivered >= 1,
        "expected at least one delivered log, got {delivered}"
    );
}

/// Mature trees left alone drop saplings. No villagers on this map, so regrowth can't lose a
/// race against the loggers.
#[test]
fn forest_regrows_within_cap() {
    let map = MapConfig::from_ron(
        "(size: 30.0, villagers: [], trees: [
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
            villagers: [(1.0, 2.0), (-3.0, -4.0)],
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
    let mut villagers: Vec<[f32; 2]> = world
        .query_filtered::<&Position, With<Villager>>()
        .iter(world)
        .map(|p| p.xz().to_array())
        .collect();
    villagers.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(villagers, [[-3.0, -4.0], [1.0, 2.0]]);

    let mut trees: Vec<[f32; 2]> = world
        .query_filtered::<(&Position, &Rotation, &Maturity), With<Tree>>()
        .iter(world)
        .map(|(p, r, &m)| tree_base(p, r, m).xz().to_array())
        .collect();
    trees.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(trees, [[-6.0, 7.0], [5.0, 5.0], [8.0, -8.0]]);
}

/// One villager, one mature tree next to it: the villager should walk over, chop it down, wait
/// for it to fall, drag it home and drop it — in that order, with nothing else in between.
#[test]
fn villager_runs_through_the_gathering_cycle() {
    let map = MapConfig::from_ron(
        "(size: 20.0, villagers: [(0.0, 0.0)], trees: [(pos: (3.0, 0.0), maturity: 1.0)])",
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
        .query_filtered::<(&ActionLog, &Strength, &Speed), With<Villager>>()
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

/// Runs one villager toward one mature tree at `(8, 0)`, optionally with a delivered log lying
/// across the path at `x = 4`. Returns the seconds spent walking before the first chop and
/// whether the villager was ever climbing a log along the way.
fn walk_to_tree(seed: u64, with_log: bool) -> (f32, bool) {
    let map = MapConfig::from_ron(
        "(size: 20.0, villagers: [(0.0, 0.0)], trees: [
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
    // across the villager's path, or move it out of the way entirely.
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
    let mut climbed = false;
    for _ in 0..(10.0 / step_secs) as u32 {
        app.update();
        let world = app.world_mut();
        let (climbing, log) = world
            .query_filtered::<(&Climbing, &ActionLog), With<Villager>>()
            .single(world)
            .unwrap();
        let entries: Vec<Entry> = log.iter().copied().collect();
        if matches!(entries.last().unwrap().action, Action::Chop { .. }) {
            assert_eq!(entries.len(), 2, "unexpected detour: {entries:?}");
            return (entries[1].at - entries[0].at, climbed);
        }
        climbed |= climbing.0;
    }
    panic!("villager never reached the tree (with_log = {with_log})");
}

#[test]
fn villagers_climb_over_logs_slowly() {
    let (clear_time, climbed_clear) = walk_to_tree(5, false);
    let (log_time, climbed_log) = walk_to_tree(5, true);

    assert!(
        log_time > clear_time + 0.3,
        "crossing a log should cost time: {log_time}s with vs {clear_time}s without"
    );
    assert!(
        log_time < clear_time + 3.0,
        "climb took too long: {log_time}s"
    );
    assert!(climbed_log, "villager never climbed the log");
    assert!(!climbed_clear, "villager climbed with no log in the way");
}

/// Seconds a villager may sit still while it's supposed to be walking or hauling.
const STALL_LIMIT: f32 = 5.0;

/// Nobody should freeze mid-walk: logs pile up around homes and villagers cross them, and a
/// hauled log must never hold its hauler in place.
#[test]
fn nobody_gets_stuck() {
    let mut app = build_app(&RunConfig {
        headless: true,
        seed: 1,
        ..RunConfig::default()
    });
    let step = app.world().resource::<RunConfig>().step as f32;
    // Where each villager was last seen moving (or doing something stationary), and when.
    let mut last_moved: HashMap<Entity, (Vec2, f32)> = HashMap::new();

    for frame in 0..(120.0 / step) as u32 {
        app.update();
        let now = frame as f32 * step;
        let world = app.world_mut();
        for (entity, position, log) in world
            .query_filtered::<(Entity, &Position, &ActionLog), With<Villager>>()
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
        .query_filtered::<(&Strength, &Speed), With<Villager>>()
        .iter(world)
        .map(|(s, v)| (s.0, v.0))
        .collect();
    stats.sort_by(|a, b| a.partial_cmp(b).unwrap());
    stats
}

#[test]
fn villager_stats_are_rolled_in_range_and_seeded() {
    let mut a = build_app(&RunConfig {
        headless: true,
        seed: 3,
        ..RunConfig::default()
    });
    a.update();
    let sa = stats(&mut a);
    assert_eq!(sa.len(), MapConfig::default().villagers.len());
    for &(strength, speed) in &sa {
        assert!(STAT_RANGE.contains(&strength), "strength {strength}");
        assert!(STAT_RANGE.contains(&speed), "speed {speed}");
    }
    let strengths: Vec<f32> = sa.iter().map(|s| s.0).collect();
    assert!(
        strengths.windows(2).any(|w| w[0] != w[1]),
        "every villager rolled the same strength: {strengths:?}"
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
        "(size: 20.0, villagers: [], trees: [(pos: (50.0, 0.0), maturity: 1.0)])",
    )
    .unwrap_err();
    assert!(err.contains("tree 0"), "unexpected error: {err}");
}

/// Rabbits wander in short hops with rests in between: over a minute each one covers plenty of
/// ground, but on any given step most of them are sitting still, and none leaves the map.
#[test]
fn rabbits_hop_about_and_stay_on_the_map() {
    let mut app = build_app(&RunConfig {
        headless: true,
        seed: 3,
        ..RunConfig::default()
    });
    app.update();
    let world = app.world_mut();
    let start: Vec<(Entity, Vec3)> = world
        .query_filtered::<(Entity, &Position), With<Rabbit>>()
        .iter(world)
        .map(|(e, p)| (e, p.0))
        .collect();
    assert_eq!(start.len(), MapConfig::default().rabbits.len());

    let half = MapConfig::default().half_extent();
    let step_secs = app.world().resource::<RunConfig>().step as f32;
    let mut resting_steps = 0;
    let mut total_steps = 0;
    // Distance covered per rabbit, and where it was last step.
    let mut travelled: HashMap<Entity, (f32, Vec2)> = start
        .iter()
        .map(|&(entity, origin)| (entity, (0.0, origin.xz())))
        .collect();
    for _ in 0..(60.0 / step_secs) as u32 {
        app.update();
        let world = app.world_mut();
        for (entity, position, velocity) in world
            .query_filtered::<(Entity, &Position, &LinearVelocity), With<Rabbit>>()
            .iter(world)
        {
            assert!(
                position.x.abs() <= half && position.z.abs() <= half,
                "rabbit left the map at {}",
                position.0
            );
            assert!(
                (position.y - start[0].1.y).abs() < 1e-3,
                "rabbit left the ground: y = {}",
                position.y
            );
            total_steps += 1;
            if velocity.0 == Vec3::ZERO {
                resting_steps += 1;
            }
            // Kits born along the way haven't had the whole minute to cover ground.
            if let Some((distance, last)) = travelled.get_mut(&entity) {
                *distance += last.distance(position.xz());
                *last = position.xz();
            }
        }
    }
    // Hops are short bursts: most of the time is spent sitting.
    assert!(
        resting_steps * 2 > total_steps,
        "rabbits rested only {resting_steps} of {total_steps} rabbit-steps"
    );

    for (entity, (distance, _)) in travelled {
        assert!(
            distance > 10.0,
            "rabbit {entity} covered only {distance:.1}m in a minute"
        );
    }
}

/// Ready rabbits find each other and have kits, which grow up; the population is capped.
#[test]
fn rabbits_breed_up_to_the_cap() {
    let map = MapConfig::from_ron(
        "(size: 20.0, villagers: [], trees: [], rabbits: [(-3.0, 0.0), (3.0, 0.0), (0.0, 3.0), (0.0, -3.0)])",
    )
    .unwrap();
    let initial = map.rabbits.len();
    let mut app = build_app(&RunConfig {
        headless: true,
        seed: 4,
        map,
        ..RunConfig::default()
    });
    step(&mut app, 120.0);

    let world = app.world_mut();
    let rabbits: Vec<(&Rabbit, &Breeding)> =
        world.query::<(&Rabbit, &Breeding)>().iter(world).collect();
    let kits = rabbits
        .iter()
        .filter(|(_, b)| matches!(b, Breeding::Growing { .. }))
        .count();
    assert!(
        rabbits.len() > initial,
        "expected kits beyond the initial {initial}, got {}",
        rabbits.len()
    );
    assert!(kits > 0, "no kit still growing after 2 minutes");
    for (rabbit, breeding) in &rabbits {
        let grown = !matches!(breeding, Breeding::Growing { .. });
        assert!(
            grown == (rabbit.size >= 1.0),
            "size {} doesn't match growth state",
            rabbit.size
        );
    }

    // Left running, the warren fills up but never goes past the cap.
    step(&mut app, 600.0);
    let world = app.world_mut();
    let count = world.query::<&Rabbit>().iter(world).count();
    assert!(count > initial * 2, "warren stayed small: {count}");
    assert!(count <= MAX_RABBITS, "rabbit cap exceeded: {count}");
}

fn carrot_count(app: &mut App) -> usize {
    app.world_mut()
        .query_filtered::<(), With<Carrot>>()
        .iter(app.world())
        .count()
}

/// Carrots sprout at random spots over time, clear of the edge, and never past the cap.
#[test]
fn carrots_sprout_within_cap() {
    let map = MapConfig::from_ron("(size: 20.0, villagers: [], trees: [])").unwrap();
    let mut app = build_app(&RunConfig {
        headless: true,
        seed: 5,
        map,
        ..RunConfig::default()
    });
    app.update();
    assert_eq!(carrot_count(&mut app), 0, "carrots should sprout over time");

    step(&mut app, 60.0);
    let world = app.world_mut();
    let spots: Vec<Vec2> = world
        .query_filtered::<&Transform, With<Carrot>>()
        .iter(world)
        .map(|t| t.translation.xz())
        .collect();
    assert!(
        spots.len() >= 3,
        "only {} carrots after a minute",
        spots.len()
    );
    for spot in &spots {
        assert!(
            spot.abs().max_element() < 10.0,
            "carrot off the map at {spot}"
        );
    }
    assert!(
        spots
            .iter()
            .any(|a| spots.iter().any(|b| a.distance(*b) > 5.0)),
        "carrots all sprouted in one place: {spots:?}"
    );

    step(&mut app, 600.0);
    let count = carrot_count(&mut app);
    assert_eq!(
        count, MAX_CARROTS,
        "with nobody eating, carrots should fill up to the cap"
    );
}

/// A hungry rabbit hops to a carrot in sight and eats it.
#[test]
fn rabbits_eat_carrots() {
    let map = MapConfig::from_ron(
        "(size: 20.0, villagers: [], trees: [], rabbits: [(0.0, 0.0), (5.0, 5.0)])",
    )
    .unwrap();
    let mut app = build_app(&RunConfig {
        headless: true,
        seed: 6,
        map,
        ..RunConfig::default()
    });
    let step_secs = app.world().resource::<RunConfig>().step as f32;
    let mut sprouted = 0;
    let mut eaten = 0;
    let mut last = 0;
    for _ in 0..(300.0 / step_secs) as u32 {
        app.update();
        let count = carrot_count(&mut app);
        if count > last {
            sprouted += count - last;
        } else {
            eaten += last - count;
        }
        last = count;
    }
    assert!(sprouted > eaten, "sprouted {sprouted}");
    assert!(
        eaten >= 2,
        "rabbits ate only {eaten} of {sprouted} carrots in 5 minutes"
    );

    // A rabbit that has eaten is full for a while.
    let world = app.world_mut();
    let appetites: Vec<Appetite> = world.query::<&Appetite>().iter(world).copied().collect();
    assert!(
        appetites.iter().any(|a| matches!(a, Appetite::Full { .. })),
        "no rabbit is full after eating"
    );
}

/// Load test: 1 000 villagers among 10 000 trees. Ignored by default because it takes seconds
/// even in release; run with `cargo test --release --test simulation heavy -- --ignored --nocapture`
/// to get the timing report.
#[test]
#[ignore = "benchmark: run explicitly with --ignored --nocapture"]
fn heavy_world_keeps_stepping() {
    use std::time::Instant;
    use villa_age::map::TreeSpec;

    const TREES_PER_SIDE: usize = 100;
    const TREE_SPACING: f32 = 3.0;
    const VILLAGERS: usize = 1_000;
    const SIM_SECONDS: f32 = 30.0;

    // Trees on a square grid; villagers at the centres of a subset of the cells, so each sits
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
    let chars_per_side = (VILLAGERS as f32).sqrt().ceil() as usize;
    let cell_centre = |k: usize| {
        let cell = k * (TREES_PER_SIDE - 1) / chars_per_side;
        origin + (cell as f32 + 0.5) * TREE_SPACING
    };
    let villagers: Vec<(f32, f32)> = (0..VILLAGERS)
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
        villagers,
        trees,
        rabbits: vec![],
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
        .query_filtered::<(), With<Villager>>()
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
        "heavy world: {chars} villagers, {trees} trees\n  \
         build app        {build:>9.2?}\n  \
         first frame      {first_frame:>9.2?}  (startup systems + spawning)\n  \
         {frames} frames      {run:>9.2?}  ({per_frame:.2?}/frame, {:.1}x realtime)\n  \
         delivered logs   {delivered}  (lost mid-haul: {lost})",
        SIM_SECONDS / run.as_secs_f32(),
    );

    assert_eq!(chars, VILLAGERS);
    assert_eq!(trees, TREES_PER_SIDE * TREES_PER_SIDE);
    assert!(delivered > 0, "nobody delivered a log in {SIM_SECONDS}s");
}
