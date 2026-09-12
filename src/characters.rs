//! Placeholder 3D characters: they chop the nearest tree, drag the log back home, repeat.

use avian3d::prelude::*;
use bevy::prelude::*;
use rand::RngExt;

use crate::GameRng;
use crate::history::{Action, ActionLog};
use crate::map::MapConfig;
use crate::physics::character_layers;
use crate::sim::SimSet;
use crate::trees::{Maturity, TRUNK_RADIUS, TreeState, max_health, tree_base};

/// Marker for character entities.
#[derive(Component)]
#[require(Task, Heading, ActionLog, Strength, Speed)]
pub struct Character;

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

#[derive(Component, Default)]
pub enum Task {
    /// Walk to the nearest usable tree; chop it if standing, pick it up if fallen.
    #[default]
    Gather,
    /// Drag `tree` back home; `rope` is the joint entity holding it.
    Haul { tree: Entity, rope: Entity },
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
/// Steering: how far ahead to look for obstacles, and how hard to swerve around them.
const LOOKAHEAD: f32 = 2.5;
const AVOID_STRENGTH: f32 = 1.5;

const RADIUS: f32 = 0.4;
const HEIGHT: f32 = 1.0;
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
        app.add_systems(Startup, spawn_characters)
            .add_systems(Update, (gather, haul).chain().in_set(SimSet::Characters));
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
    // Capsule stands on the ground when its center is at half-height + radius.
    let y = HEIGHT / 2.0 + RADIUS;
    let mesh = meshes.add(Capsule3d::new(RADIUS, HEIGHT));

    for (i, &(x, z)) in map.characters.iter().enumerate() {
        let position = Vec3::new(x, y, z);
        let color = COLORS[i % COLORS.len()];
        let strength = Strength(rng.0.random_range(STAT_RANGE));
        let speed = Speed(rng.0.random_range(STAT_RANGE));
        debug!("character {} spawned with {strength:?} {speed:?}", i + 1);
        // To use a real model instead of the capsule, replace `Mesh3d`/`MeshMaterial3d` with
        // `SceneRoot(asset_server.load("character.glb#Scene0"))`.
        commands.spawn((
            Character,
            Name::new(format!("Character {}", i + 1)),
            Home(position),
            strength,
            speed,
            Mesh3d(mesh.clone()),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                ..default()
            })),
            Transform::from_translation(position),
            RigidBody::Dynamic,
            Collider::capsule(RADIUS, HEIGHT),
            // Rotation is driven by `Heading`, not physics.
            LockedAxes::ROTATION_LOCKED.lock_translation_y(),
            character_layers(),
            Mass(80.0),
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
}

impl WalkerItem<'_, '_> {
    /// Turns toward `facing` (a direction on the ground) at `TURN_SPEED`.
    fn turn_toward(&mut self, facing: Vec3, dt: f32) {
        let target = Transform::default().looking_to(facing, Vec3::Y).rotation;
        self.heading.0 = self.heading.0.rotate_towards(target, TURN_SPEED * dt);
    }

    /// Sets the velocity to move toward `target` (in XZ) without overshooting, steering around
    /// anything in the way except `ignore`; returns the remaining distance. Stops when within `stop_at`.
    fn walk_toward(
        &mut self,
        spatial: &SpatialQuery,
        target: Vec2,
        stop_at: f32,
        ignore: Option<Entity>,
        dt: f32,
    ) -> f32 {
        let to_target = target - self.transform.translation.xz();
        let distance = to_target.length();
        if distance <= stop_at {
            self.velocity.0 = Vec3::ZERO;
            return distance;
        }
        let dir = to_target / distance;
        let desired = Vec3::new(dir.x, 0.0, dir.y);
        let steered = self.steer(spatial, desired, ignore);
        let speed = (MOVE_SPEED * self.speed.0).min((distance - stop_at) / dt);
        self.velocity.0 = steered * speed;
        self.turn_toward(steered, dt);
        distance
    }

    /// Local obstacle avoidance: casts this character's shape along `desired` and, if something is
    /// in the way, blends in a sideways push along the obstacle's surface.
    fn steer(&self, spatial: &SpatialQuery, desired: Vec3, ignore: Option<Entity>) -> Vec3 {
        let Ok(direction) = Dir3::new(desired) else {
            return desired;
        };
        let filter =
            SpatialQueryFilter::from_excluded_entities([self.entity].into_iter().chain(ignore));
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

/// Walks each gathering character to its chosen tree, chops it, and picks it up once fallen.
fn gather(
    mut commands: Commands,
    time: Res<Time>,
    spatial: SpatialQuery,
    mut trees: Query<(Entity, &Transform, &mut TreeState, &Maturity), Without<Character>>,
    mut characters: Query<(Walker, &Strength, &mut Task, &mut ActionLog), With<Character>>,
) {
    let now = time.elapsed_secs();
    for (mut walker, strength, mut task, mut log) in &mut characters {
        if !matches!(*task, Task::Gather) {
            continue;
        }
        let pos = walker.transform.translation.xz();
        let dt = time.delta_secs();
        // Chop swing angle applied on top of the heading this frame.
        let mut swing = 0.0;

        let candidates = trees.iter().map(|(entity, t, state, &maturity)| TreeInfo {
            entity,
            base: tree_base(t).xz(),
            state,
            maturity,
        });
        let Some(chosen) = choose_tree(pos, candidates) else {
            walker.velocity.0 = Vec3::ZERO;
            log.record(now, Action::Idle);
            continue;
        };
        let (tree, target) = (chosen.entity, chosen.base);
        let Ok((_, tree_transform, mut state, &maturity)) = trees.get_mut(tree) else {
            continue;
        };

        let distance = walker.walk_toward(&spatial, target, ARRIVE_DISTANCE, Some(tree), dt);
        if distance > ARRIVE_DISTANCE + ARRIVE_SLACK {
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
                    let slack = ROPE_LENGTH.max(distance);
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
            *task = Task::Gather;
            log.record(now, Action::LostLog { tree });
            continue;
        }

        let remaining = walker.walk_toward(
            &spatial,
            home.0.xz(),
            DROP_DISTANCE,
            Some(tree),
            time.delta_secs(),
        );
        walker.transform.rotation = walker.heading.0;

        if remaining <= DROP_DISTANCE {
            // Let go: the log freezes exactly where it was dragged to.
            commands.entity(rope).despawn();
            if let Ok(mut state) = trees.get_mut(tree) {
                *state = TreeState::Delivered;
            }
            *task = Task::Gather;
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
