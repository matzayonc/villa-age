//! Placeholder 3D characters: they chop the nearest tree, drag the log back home, repeat.

use avian3d::prelude::*;
use bevy::prelude::*;
use rand::RngExt;

use crate::GameRng;
use crate::history::{Action, ActionLog};
use crate::map::MapConfig;
use crate::physics::{Layer, character_layers, steer_mask};
use crate::sim::SimSet;
use crate::trees::{Maturity, TRUNK_RADIUS, TreeState, max_health, tree_base};

/// Marker for character entities.
#[derive(Component)]
#[require(Task, Heading, ActionLog, Strength, Speed, Climbing)]
pub struct Character;

/// Whether the character is on top of a log (its capsule overlaps one). Refreshed every frame
/// by [`climb_logs`].
#[derive(Component, Default, Clone, Copy, Debug, PartialEq)]
pub struct Climbing(pub bool);

/// The character's visual, a child of its body so it can rise over a log without moving the
/// physics body (which the rope to a hauled log is anchored to).
#[derive(Component)]
struct CharacterMesh;

/// How hard a character chops: a multiplier on the base chop rate, rolled at spawn.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Strength(pub f32);

/// How fast a character walks: a multiplier on the base move speed, rolled at spawn.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Speed(pub f32);

impl Default for Strength {
    fn default() -> Self {
        Self(1.0)
    }
}

impl Default for Speed {
    fn default() -> Self {
        Self(1.0)
    }
}

/// The direction the character is turned toward, smoothed over time. The transform's rotation is
/// derived from this every frame (plus any animation on top).
#[derive(Component, Default)]
pub struct Heading(Quat);

/// Where the character spawned and hauls logs back to.
#[derive(Component)]
pub struct Home(pub Vec3);

#[derive(Component)]
pub enum Task {
    /// Walk to `target`, chop it if standing, pick it up if fallen. The target is chosen (the
    /// nearest usable tree) when there is none and kept until it stops being usable, so the
    /// choice isn't recomputed over every tree each frame.
    Gather { target: Option<Entity> },
    /// Drag `tree` back home; `rope` is the joint entity holding it.
    Haul { tree: Entity, rope: Entity },
}

impl Default for Task {
    fn default() -> Self {
        Self::Gather { target: None }
    }
}

/// World units per second at `Speed(1.0)`.
const MOVE_SPEED: f32 = 3.0;
/// Spawn-time roll for each character's [`Strength`] and [`Speed`] multipliers.
pub const STAT_RANGE: std::ops::RangeInclusive<f32> = 0.7..=1.3;
/// Radians per second a character can turn.
const TURN_SPEED: f32 = 5.0;
/// How close (in XZ) a character gets to a tree's base before stopping.
const ARRIVE_DISTANCE: f32 = TRUNK_RADIUS + RADIUS + 0.2;
/// A character stopped at `ARRIVE_DISTANCE` still counts as arrived within this much extra, so
/// rounding in a moving tree's base position or a nudge from physics doesn't flicker it back to
/// walking.
const ARRIVE_SLACK: f32 = 0.05;
/// How close to home a character gets before dropping the log.
const DROP_DISTANCE: f32 = 0.3;
/// Tree health removed per second while chopping at `Strength(1.0)`.
const CHOP_RATE: f32 = 1.0;
/// Chop swing animation: swings per second and lean angle in radians.
const SWING_SPEED: f32 = 8.0;
const SWING_ANGLE: f32 = 0.25;
/// Maximum length of the rope between a hauling character and the base of its log. Equal to the
/// distance at which the character stopped to chop, so grabbing doesn't move the log.
const ROPE_LENGTH: f32 = ARRIVE_DISTANCE;
/// Standing trees below this maturity are left to grow.
const HARVEST_MATURITY: f32 = 0.6;
/// Walking speed multiplier while climbing over a log.
const CLIMB_SPEED_FACTOR: f32 = 0.35;
/// How much a character's visual rises while on top of a log (the log's radius).
const CLIMB_HEIGHT: f32 = TRUNK_RADIUS;
/// Steering: how far ahead to look for obstacles, and how hard to swerve around them.
const LOOKAHEAD: f32 = 2.5;
const AVOID_STRENGTH: f32 = 1.5;

const RADIUS: f32 = 0.4;
const HEIGHT: f32 = 1.0;
/// Height of the capsule's center when it stands on the ground.
const GROUND_Y: f32 = HEIGHT / 2.0 + RADIUS;
/// Character colors, cycled by spawn index.
const COLORS: [Color; 5] = [
    Color::srgb(0.85, 0.25, 0.2),
    Color::srgb(0.2, 0.45, 0.9),
    Color::srgb(0.95, 0.8, 0.2),
    Color::srgb(0.6, 0.3, 0.8),
    Color::srgb(0.2, 0.8, 0.75),
];

pub struct CharactersPlugin;

impl Plugin for CharactersPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_characters).add_systems(
            Update,
            (climb_logs, gather, haul)
                .chain()
                .in_set(SimSet::Characters),
        );
    }
}

/// Spawns a character at each of the map's spawn points, each with its own rolled stats.
fn spawn_characters(
    mut commands: Commands,
    map: Res<MapConfig>,
    mut rng: ResMut<GameRng>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = meshes.add(Capsule3d::new(RADIUS, HEIGHT));

    for (i, &(x, z)) in map.characters.iter().enumerate() {
        let position = Vec3::new(x, GROUND_Y, z);
        let color = COLORS[i % COLORS.len()];
        let strength = Strength(rng.0.random_range(STAT_RANGE));
        let speed = Speed(rng.0.random_range(STAT_RANGE));
        debug!("character {} spawned with {strength:?} {speed:?}", i + 1);
        commands
            .spawn((
                Character,
                Name::new(format!("Character {}", i + 1)),
                Home(position),
                strength,
                speed,
                Transform::from_translation(position),
                // Bodies start with their physics position set explicitly (see `physics.rs`).
                Position(position),
                Visibility::default(),
                RigidBody::Dynamic,
                Collider::capsule(RADIUS, HEIGHT),
                // Rotation is driven by `Heading`, not physics.
                LockedAxes::ROTATION_LOCKED.lock_translation_y(),
                character_layers(),
                Mass(80.0),
            ))
            .with_child((
                CharacterMesh,
                // To use a real model instead of the capsule, replace `Mesh3d`/`MeshMaterial3d`
                // with `SceneRoot(asset_server.load("character.glb#Scene0"))`.
                Mesh3d(mesh.clone()),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: color,
                    ..default()
                })),
            ));
    }
}

/// The physics-facing parts of a character that walking needs.
#[derive(bevy::ecs::query::QueryData)]
#[query_data(mutable)]
struct Walker {
    entity: Entity,
    transform: &'static mut Transform,
    collider: &'static Collider,
    velocity: &'static mut LinearVelocity,
    heading: &'static mut Heading,
    speed: &'static Speed,
    climbing: &'static Climbing,
}

impl WalkerItem<'_, '_> {
    /// Turns toward `facing` (a direction on the ground) at `TURN_SPEED`.
    fn turn_toward(&mut self, facing: Vec3, dt: f32) {
        let target = Transform::default().looking_to(facing, Vec3::Y).rotation;
        self.heading.0 = self.heading.0.rotate_towards(target, TURN_SPEED * dt);
    }

    /// Sets the velocity to move toward `target` (in XZ), steering around anything in the way
    /// except `ignore`; logs aren't steered around but climbed over, slowly. Aims to stop at
    /// `stop_at` from the target and returns whether it has arrived, which allows `ARRIVE_SLACK`
    /// beyond `stop_at` so the physics step landing a hair short can't leave it creeping forever.
    ///
    /// `dt` is this frame's delta (turning happens per frame); `physics_step` is how long the
    /// velocity will be applied for, which sets how fast the last stretch can be taken.
    fn walk_toward(
        &mut self,
        spatial: &SpatialQuery,
        target: Vec2,
        stop_at: f32,
        ignore: Option<Entity>,
        dt: f32,
        physics_step: f32,
    ) -> bool {
        let to_target = target - self.transform.translation.xz();
        let distance = to_target.length();
        if distance <= stop_at + ARRIVE_SLACK {
            self.velocity.0 = Vec3::ZERO;
            return true;
        }
        let dir = to_target / distance;
        let desired = Vec3::new(dir.x, 0.0, dir.y);
        let steered = self.steer(spatial, desired, ignore);
        let mut max_speed = MOVE_SPEED * self.speed.0;
        if self.climbing.0 {
            max_speed *= CLIMB_SPEED_FACTOR;
        }
        let speed = max_speed.min((distance - stop_at) / physics_step);
        self.velocity.0 = steered * speed;
        self.turn_toward(steered, dt);
        false
    }

    /// Local obstacle avoidance: casts this character's shape along `desired` and, if something is
    /// in the way, blends in a sideways push along the obstacle's surface.
    fn steer(&self, spatial: &SpatialQuery, desired: Vec3, ignore: Option<Entity>) -> Vec3 {
        let Ok(direction) = Dir3::new(desired) else {
            return desired;
        };
        let filter = SpatialQueryFilter::from_mask(steer_mask())
            .with_excluded_entities([self.entity].into_iter().chain(ignore));
        let Some(hit) = spatial.cast_shape(
            self.collider,
            self.transform.translation,
            Quat::IDENTITY,
            direction,
            &ShapeCastConfig::from_max_distance(LOOKAHEAD),
            &filter,
        ) else {
            return desired;
        };

        let normal = Vec3::new(hit.normal1.x, 0.0, hit.normal1.z).normalize_or_zero();
        let mut tangent = desired - normal * desired.dot(normal);
        if tangent.length_squared() < 1e-4 {
            // Head-on: pick a side.
            tangent = normal.cross(Vec3::Y);
        }
        let closeness = 1.0 - (hit.distance / LOOKAHEAD).clamp(0.0, 1.0);
        (desired + tangent.normalize() * (AVOID_STRENGTH * closeness)).normalize_or(desired)
    }
}

/// What a character knows about a tree when deciding whether to go for it.
struct TreeInfo<'a> {
    entity: Entity,
    /// Where it touches the ground, in XZ.
    base: Vec2,
    state: &'a TreeState,
    maturity: Maturity,
}

impl<'a> From<(Entity, &Transform, &'a TreeState, &Maturity)> for TreeInfo<'a> {
    fn from(
        (entity, transform, state, &maturity): (Entity, &Transform, &'a TreeState, &Maturity),
    ) -> Self {
        Self {
            entity,
            base: tree_base(transform).xz(),
            state,
            maturity,
        }
    }
}

/// How attractive a tree is to a character standing at `pos`: lower is better, `None` means it is
/// not a valid target. This is the place to add smarter rules (yield, competition, distance from home...).
fn tree_priority(pos: Vec2, tree: &TreeInfo) -> Option<f32> {
    let distance = tree.base.distance_squared(pos);
    match tree.state {
        TreeState::Standing { .. } if tree.maturity.0 >= HARVEST_MATURITY => Some(distance),
        TreeState::Falling { .. } | TreeState::Fallen => Some(distance),
        TreeState::Standing { .. } | TreeState::Carried(_) | TreeState::Delivered => None,
    }
}

/// The tree a character at `pos` should go for, if any.
fn choose_tree<'a>(
    pos: Vec2,
    trees: impl IntoIterator<Item = TreeInfo<'a>>,
) -> Option<TreeInfo<'a>> {
    trees
        .into_iter()
        .filter_map(|tree| tree_priority(pos, &tree).map(|priority| (priority, tree)))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, tree)| tree)
}

/// Notes which characters are standing on a log and lifts their visuals onto it.
fn climb_logs(
    spatial: SpatialQuery,
    mut characters: Query<(&Transform, &Collider, &mut Climbing, &Children), With<Character>>,
    mut meshes: Query<&mut Transform, (With<CharacterMesh>, Without<Character>)>,
) {
    for (transform, collider, mut climbing, children) in &mut characters {
        let on_log = !spatial
            .shape_intersections(
                collider,
                transform.translation,
                Quat::IDENTITY,
                &SpatialQueryFilter::from_mask(Layer::Log),
            )
            .is_empty();
        if climbing.0 != on_log {
            climbing.0 = on_log;
        }
        let lift = if on_log { CLIMB_HEIGHT } else { 0.0 };
        for &child in children {
            if let Ok(mut mesh) = meshes.get_mut(child) {
                mesh.translation.y = lift;
            }
        }
    }
}

/// Walks each gathering character to its chosen tree, chops it, and picks it up once fallen.
fn gather(
    mut commands: Commands,
    time: Res<Time>,
    physics_time: Res<Time<Fixed>>,
    spatial: SpatialQuery,
    mut trees: Query<(Entity, &Transform, &mut TreeState, &Maturity), Without<Character>>,
    mut characters: Query<(Walker, &Strength, &mut Task, &mut ActionLog), With<Character>>,
) {
    let now = time.elapsed_secs();
    for (mut walker, strength, mut task, mut log) in &mut characters {
        let Task::Gather { target: current } = &mut *task else {
            continue;
        };
        let pos = walker.transform.translation.xz();
        let dt = time.delta_secs();
        // Chop swing angle applied on top of the heading this frame.
        let mut swing = 0.0;

        // Stick with the current target while it's still usable; only scan the whole forest
        // when there is none or someone else got to it first.
        let still_usable = current
            .and_then(|tree| trees.get(tree).ok())
            .is_some_and(|tree| tree_priority(pos, &TreeInfo::from(tree)).is_some());
        if !still_usable {
            *current = choose_tree(pos, trees.iter().map(TreeInfo::from)).map(|tree| tree.entity);
        }
        let Some(tree) = *current else {
            walker.velocity.0 = Vec3::ZERO;
            log.record(now, Action::Idle);
            continue;
        };
        let Ok((_, tree_transform, mut state, &maturity)) = trees.get_mut(tree) else {
            continue;
        };
        let target = tree_base(tree_transform).xz();

        let arrived = walker.walk_toward(
            &spatial,
            target,
            ARRIVE_DISTANCE,
            Some(tree),
            dt,
            physics_time.delta_secs(),
        );
        if !arrived {
            log.record(now, Action::WalkTo { tree });
        } else {
            let to_tree = (target - pos).normalize_or(Vec2::X);
            let facing = Vec3::new(to_tree.x, 0.0, to_tree.y);
            walker.turn_toward(facing, dt);

            match *state {
                TreeState::Standing { damage } => {
                    let damage = damage + CHOP_RATE * strength.0 * dt;
                    if damage >= max_health(maturity) {
                        log.record(now, Action::AwaitFall { tree });
                        // The tree falls away from whoever felled it.
                        *state = TreeState::Falling {
                            base: Vec3::new(target.x, 0.0, target.y),
                            dir: facing,
                            progress: 0.0,
                        };
                    } else {
                        *state = TreeState::Standing { damage };
                        log.record(now, Action::Chop { tree });
                        // Lean toward the tree in a swinging motion while chopping.
                        swing = (time.elapsed_secs() * SWING_SPEED).sin().max(0.0) * SWING_ANGLE;
                    }
                }
                // Wait for it to hit the ground.
                TreeState::Falling { .. } => log.record(now, Action::AwaitFall { tree }),
                TreeState::Fallen => {
                    *state = TreeState::Carried(walker.entity);
                    log.record(now, Action::Haul { tree });
                    // A slack rope from the character (held at log height) to the log's base: the
                    // log is only pulled once the rope is taut, so grabbing doesn't move it.
                    let log_base = tree_base(tree_transform);
                    let hand = Vec3::Y * (log_base.y - walker.transform.translation.y);
                    let slack = ROPE_LENGTH.max(target.distance(pos));
                    let mut joint = DistanceJoint::new(walker.entity, tree)
                        .with_local_anchor1(hand)
                        .with_limits(0.0, slack);
                    joint.anchor2 = JointAnchor::FromGlobal(log_base);
                    let rope = commands.spawn(joint).id();
                    *task = Task::Haul { tree, rope };
                }
                TreeState::Carried(_) | TreeState::Delivered => {
                    unreachable!("rejected by tree_priority")
                }
            }
        }

        walker.transform.rotation = walker.heading.0 * Quat::from_rotation_x(-swing);
    }
}

/// Drags the carried tree behind the character back home, then lets go of it there.
fn haul(
    mut commands: Commands,
    time: Res<Time>,
    physics_time: Res<Time<Fixed>>,
    spatial: SpatialQuery,
    mut trees: Query<&mut TreeState, Without<Character>>,
    mut characters: Query<(Walker, &mut Task, &Home, &mut ActionLog), With<Character>>,
) {
    let now = time.elapsed_secs();
    for (mut walker, mut task, home, mut log) in &mut characters {
        let Task::Haul { tree, rope } = *task else {
            continue;
        };
        // Drop the task if the log vanished or someone else ended up with it.
        let owned = trees.get(tree).is_ok_and(
            |state| matches!(*state, TreeState::Carried(owner) if owner == walker.entity),
        );
        if !owned {
            commands.entity(rope).despawn();
            *task = Task::default();
            log.record(now, Action::LostLog { tree });
            continue;
        }

        let arrived = walker.walk_toward(
            &spatial,
            home.0.xz(),
            DROP_DISTANCE,
            Some(tree),
            time.delta_secs(),
            physics_time.delta_secs(),
        );
        walker.transform.rotation = walker.heading.0;

        if arrived {
            // Let go: the log freezes exactly where it was dragged to.
            commands.entity(rope).despawn();
            if let Ok(mut state) = trees.get_mut(tree) {
                *state = TreeState::Delivered;
            }
            *task = Task::default();
            log.record(now, Action::Deliver { tree });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(entity: Entity, base: Vec2, state: &TreeState, maturity: f32) -> TreeInfo<'_> {
        TreeInfo {
            entity,
            base,
            state,
            maturity: Maturity(maturity),
        }
    }

    #[test]
    fn only_harvestable_trees_are_targets() {
        let mut world = World::new();
        let (tree, other) = (world.spawn_empty().id(), world.spawn_empty().id());
        let base = Vec2::new(3.0, 4.0);
        let pos = Vec2::ZERO;

        let standing = TreeState::Standing { damage: 0.0 };
        assert_eq!(
            tree_priority(pos, &info(tree, base, &standing, HARVEST_MATURITY)),
            Some(25.0)
        );
        assert_eq!(
            tree_priority(pos, &info(tree, base, &standing, HARVEST_MATURITY - 0.01)),
            None,
            "immature trees are left to grow"
        );
        let falling = TreeState::Falling {
            base: Vec3::ZERO,
            dir: Vec3::X,
            progress: 0.5,
        };
        assert_eq!(
            tree_priority(pos, &info(tree, base, &falling, 0.0)),
            Some(25.0)
        );
        assert_eq!(
            tree_priority(pos, &info(tree, base, &TreeState::Fallen, 0.0)),
            Some(25.0)
        );
        assert_eq!(
            tree_priority(pos, &info(tree, base, &TreeState::Carried(other), 1.0)),
            None
        );
        assert_eq!(
            tree_priority(pos, &info(tree, base, &TreeState::Delivered, 1.0)),
            None
        );
    }

    #[test]
    fn choose_tree_picks_the_nearest_valid_one() {
        let mut world = World::new();
        let ids: Vec<Entity> = (0..3).map(|_| world.spawn_empty().id()).collect();
        let standing = TreeState::Standing { damage: 0.0 };
        let pos = Vec2::ZERO;

        let candidates = [
            info(ids[0], Vec2::new(1.0, 0.0), &standing, 0.1), // nearest, but a sapling
            info(ids[1], Vec2::new(0.0, 5.0), &TreeState::Fallen, 0.0),
            info(ids[2], Vec2::new(4.0, 0.0), &standing, 1.0),
        ];
        let chosen = choose_tree(pos, candidates).unwrap();
        assert_eq!(chosen.entity, ids[2]);

        let none = choose_tree(pos, [info(ids[0], Vec2::X, &standing, 0.1)]);
        assert!(none.is_none());
    }
}
