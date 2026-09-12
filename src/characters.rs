//! Placeholder 3D characters: they chop the nearest tree, drag the log back home, repeat.

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::physics::character_layers;
use crate::trees::{BASE_OFFSET, TRUNK_RADIUS, TreeState, tree_base};

/// Marker for character entities.
#[derive(Component)]
#[require(Task, Heading)]
pub struct Character;

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

/// World units per second.
const MOVE_SPEED: f32 = 3.0;
/// Radians per second a character can turn.
const TURN_SPEED: f32 = 5.0;
/// How close (in XZ) a character gets to a tree's base before stopping.
const ARRIVE_DISTANCE: f32 = TRUNK_RADIUS + RADIUS + 0.2;
/// How close to home a character gets before dropping the log.
const DROP_DISTANCE: f32 = 0.3;
/// Tree health removed per second while chopping.
const CHOP_RATE: f32 = 1.0;
/// Chop swing animation: swings per second and lean angle in radians.
const SWING_SPEED: f32 = 8.0;
const SWING_ANGLE: f32 = 0.25;
/// Maximum length of the rope between a hauling character and the base of its log. Equal to the
/// distance at which the character stopped to chop, so grabbing doesn't move the log.
const ROPE_LENGTH: f32 = ARRIVE_DISTANCE;
/// Steering: how far ahead to look for obstacles, and how hard to swerve around them.
const LOOKAHEAD: f32 = 2.5;
const AVOID_STRENGTH: f32 = 1.5;

const RADIUS: f32 = 0.4;
const HEIGHT: f32 = 1.0;

pub struct CharactersPlugin;

impl Plugin for CharactersPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_characters)
            .add_systems(Update, (gather, haul).chain());
    }
}

fn spawn_characters(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Capsule stands on the ground when its center is at half-height + radius.
    let y = HEIGHT / 2.0 + RADIUS;
    let mesh = meshes.add(Capsule3d::new(RADIUS, HEIGHT));

    let placements = [
        (Vec2::new(0.0, 0.0), Color::srgb(0.85, 0.25, 0.2)),
        (Vec2::new(4.0, -3.0), Color::srgb(0.2, 0.45, 0.9)),
        (Vec2::new(-5.0, 2.0), Color::srgb(0.95, 0.8, 0.2)),
        (Vec2::new(7.0, 6.0), Color::srgb(0.6, 0.3, 0.8)),
        (Vec2::new(-8.0, -7.0), Color::srgb(0.2, 0.8, 0.75)),
    ];

    for (pos, color) in placements {
        let position = Vec3::new(pos.x, y, pos.y);
        // To use a real model instead of the capsule, replace `Mesh3d`/`MeshMaterial3d` with
        // `SceneRoot(asset_server.load("character.glb#Scene0"))`.
        commands.spawn((
            Character,
            Home(position),
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
        let speed = MOVE_SPEED.min((distance - stop_at) / dt);
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

/// Walks each gathering character to the nearest usable tree, chops it, and picks it up once fallen.
fn gather(
    mut commands: Commands,
    time: Res<Time>,
    spatial: SpatialQuery,
    mut trees: Query<(Entity, &Transform, &mut TreeState), Without<Character>>,
    mut characters: Query<(Walker, &mut Task), With<Character>>,
) {
    for (mut walker, mut task) in &mut characters {
        if !matches!(*task, Task::Gather) {
            continue;
        }
        let pos = walker.transform.translation.xz();
        let dt = time.delta_secs();
        // Chop swing angle applied on top of the heading this frame.
        let mut swing = 0.0;

        let Some((tree, target, mut state)) = trees
            .iter_mut()
            .filter(|(_, _, state)| {
                matches!(
                    **state,
                    TreeState::Standing { .. } | TreeState::Falling { .. } | TreeState::Fallen
                )
            })
            .map(|(entity, t, state)| (entity, tree_base(t).xz(), state))
            .min_by(|a, b| {
                a.1.distance_squared(pos)
                    .total_cmp(&b.1.distance_squared(pos))
            })
        else {
            walker.velocity.0 = Vec3::ZERO;
            continue;
        };

        let distance = walker.walk_toward(&spatial, target, ARRIVE_DISTANCE, Some(tree), dt);
        if distance <= ARRIVE_DISTANCE {
            let to_tree = (target - pos).normalize_or(Vec2::X);
            let facing = Vec3::new(to_tree.x, 0.0, to_tree.y);
            walker.turn_toward(facing, dt);

            match *state {
                TreeState::Standing { health } => {
                    let health = health - CHOP_RATE * dt;
                    if health <= 0.0 {
                        // The tree falls away from whoever felled it.
                        *state = TreeState::Falling {
                            base: Vec3::new(target.x, 0.0, target.y),
                            dir: facing,
                            progress: 0.0,
                        };
                    } else {
                        *state = TreeState::Standing { health };
                        // Lean toward the tree in a swinging motion while chopping.
                        swing = (time.elapsed_secs() * SWING_SPEED).sin().max(0.0) * SWING_ANGLE;
                    }
                }
                // Wait for it to hit the ground.
                TreeState::Falling { .. } => {}
                TreeState::Fallen => {
                    *state = TreeState::Carried(walker.entity);
                    // A slack rope from the character (held at log height) to the log's base: the
                    // log is only pulled once the rope is taut, so grabbing doesn't move it.
                    let hand = Vec3::Y * (TRUNK_RADIUS - walker.transform.translation.y);
                    let slack = ROPE_LENGTH.max(distance);
                    let rope = commands
                        .spawn(
                            DistanceJoint::new(walker.entity, tree)
                                .with_local_anchor1(hand)
                                .with_local_anchor2(BASE_OFFSET)
                                .with_limits(0.0, slack),
                        )
                        .id();
                    *task = Task::Haul { tree, rope };
                }
                TreeState::Carried(_) | TreeState::Delivered => unreachable!("filtered out above"),
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
    mut characters: Query<(Walker, &mut Task, &Home), With<Character>>,
) {
    for (mut walker, mut task, home) in &mut characters {
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
        }
    }
}
