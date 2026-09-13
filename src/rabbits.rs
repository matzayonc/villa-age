//! Rabbits: small critters that hop about, seek each other out and multiply. They bump into
//! trees and people and get shoved aside, but nobody plans around them.
//!
//! A rabbit is born a kit, grows up, and from then on alternates between being ready to breed
//! (hopping toward the nearest other ready rabbit) and resting after a litter. Two ready rabbits
//! that meet produce one kit, up to a population cap.

use avian3d::prelude::*;
use bevy::prelude::*;
use rand::RngExt;

use crate::GameRng;
use crate::map::MapConfig;
use crate::physics::critter_layers;
use crate::sim::SimSet;

/// Marker for rabbit bodies. `size` is 0..=1: how grown the rabbit is, which scales its visual.
#[derive(Component)]
pub struct Rabbit {
    pub size: f32,
}

/// A piece of the rabbit's visual, a child of its body so it can arc through the air over a hop
/// without the physics body (which is locked to the ground plane) leaving the ground. Holds the
/// piece's height when a full-grown rabbit is sitting.
#[derive(Component)]
struct RabbitMesh {
    rest_y: f32,
}

/// What a rabbit is doing, in physics (fixed-clock) seconds.
#[derive(Component)]
enum Hopping {
    /// Sitting still until `until`, then it picks a new direction.
    Resting { until: f32 },
    /// Mid-hop: took off at `started`, lands at `started + HOP_DURATION`.
    Hop { started: f32 },
}

/// Where a rabbit is in its life, in physics (fixed-clock) seconds.
#[derive(Component, Clone, Copy, PartialEq)]
pub enum Breeding {
    /// A kit: full-grown at `until`.
    Growing { until: f32 },
    /// Looking for another ready rabbit.
    Ready,
    /// Had a litter (or just grew up); ready again at `until`.
    Resting { until: f32 },
}

/// Shared meshes and materials for spawning rabbits.
#[derive(Resource)]
struct RabbitAssets {
    body: Handle<Mesh>,
    ear: Handle<Mesh>,
    fur: Handle<StandardMaterial>,
}

/// Ground speed during a hop, and how long one lasts.
const HOP_SPEED: f32 = 2.5;
const HOP_DURATION: f32 = 0.3;
/// Peak height of the visual arc over a hop.
const HOP_HEIGHT: f32 = 0.35;
/// Time spent sitting between hops, rolled per rest.
const REST_RANGE: std::ops::RangeInclusive<f32> = 0.4..=2.0;
/// Largest change of direction between wandering hops, in radians either way.
const MAX_TURN: f32 = std::f32::consts::FRAC_PI_2;
/// A rabbit heading for a mate still veers a little, in radians either way.
const SEEK_WOBBLE: f32 = 0.3;
/// Rabbits won't hop closer than this to the map edge.
const EDGE_MARGIN: f32 = 1.5;

/// Seconds for a kit to grow up, and how big it is born.
const GROW_TIME: f32 = 40.0;
const KIT_SIZE: f32 = 0.45;
/// Seconds between litters (or after growing up), rolled per rest.
const BREED_REST: std::ops::RangeInclusive<f32> = 30.0..=60.0;
/// Rabbits placed by the map are staggered so they don't all pair off at once.
const FIRST_READY: std::ops::RangeInclusive<f32> = 5.0..=25.0;
/// How far a ready rabbit notices another one, and how close two must get to breed.
const SIGHT: f32 = 15.0;
const MEET_DISTANCE: f32 = 0.6;
/// The map won't hold more rabbits than this.
pub const MAX_RABBITS: usize = 40;

const RADIUS: f32 = 0.12;
const HEIGHT: f32 = 0.16;
/// Height of the capsule's center when it sits on the ground.
const GROUND_Y: f32 = HEIGHT / 2.0 + RADIUS;
const EAR_RADIUS: f32 = 0.03;
const EAR_HEIGHT: f32 = 0.16;

pub struct RabbitsPlugin;

impl Plugin for RabbitsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (load_rabbit_assets, spawn_rabbits).chain())
            .add_systems(
                FixedUpdate,
                (grow_up, hop, breed).chain().in_set(SimSet::Critters),
            )
            .add_systems(Update, animate_rabbit_meshes.in_set(SimSet::Critters));
    }
}

fn load_rabbit_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(RabbitAssets {
        body: meshes.add(Capsule3d::new(RADIUS, HEIGHT)),
        ear: meshes.add(Cylinder::new(EAR_RADIUS, EAR_HEIGHT)),
        fur: materials.add(StandardMaterial {
            base_color: Color::srgb(0.85, 0.8, 0.75),
            perceptual_roughness: 1.0,
            ..default()
        }),
    });
}

/// The map's rabbits: adults, each facing its own way and coming into season at its own time.
fn spawn_rabbits(
    mut commands: Commands,
    assets: Res<RabbitAssets>,
    map: Res<MapConfig>,
    mut rng: ResMut<GameRng>,
) {
    for &(x, z) in &map.rabbits {
        let heading = Quat::from_rotation_y(rng.0.random_range(0.0..std::f32::consts::TAU));
        let breeding = Breeding::Resting {
            until: rng.0.random_range(FIRST_READY),
        };
        spawn_rabbit(
            &mut commands,
            &assets,
            Vec2::new(x, z),
            heading,
            1.0,
            breeding,
        );
    }
}

fn spawn_rabbit(
    commands: &mut Commands,
    assets: &RabbitAssets,
    spot: Vec2,
    heading: Quat,
    size: f32,
    breeding: Breeding,
) {
    let position = Vec3::new(spot.x, GROUND_Y, spot.y);
    commands
        .spawn((
            Rabbit { size },
            Name::new("Rabbit"),
            Hopping::Resting { until: 0.0 },
            breeding,
            Transform::from_translation(position).with_rotation(heading),
            Position(position),
            Rotation(heading),
            TransformInterpolation,
            Visibility::default(),
            RigidBody::Dynamic,
            Collider::capsule(RADIUS, HEIGHT),
            // Faces the way it hops; physics never turns it.
            LockedAxes::ROTATION_LOCKED.lock_translation_y(),
            critter_layers(),
            Mass(1.0),
        ))
        .with_children(|parent| {
            parent.spawn((
                RabbitMesh { rest_y: 0.0 },
                Mesh3d(assets.body.clone()),
                MeshMaterial3d(assets.fur.clone()),
                // The capsule lies along the hop direction.
                Transform::from_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
            ));
            let ear_y = RADIUS + EAR_HEIGHT / 2.0;
            for side in [-1.0, 1.0] {
                parent.spawn((
                    RabbitMesh { rest_y: ear_y },
                    Mesh3d(assets.ear.clone()),
                    MeshMaterial3d(assets.fur.clone()),
                    Transform::from_xyz(side * 0.05, ear_y, -HEIGHT / 2.0),
                ));
            }
        });
}

/// Kits grow toward full size; grown rabbits come into season once their rest is over.
fn grow_up(
    time: Res<Time>,
    mut rng: ResMut<GameRng>,
    mut rabbits: Query<(&mut Rabbit, &mut Breeding)>,
) {
    let now = time.elapsed_secs();
    for (mut rabbit, mut breeding) in &mut rabbits {
        match *breeding {
            Breeding::Growing { until } => {
                rabbit.size = 1.0 - (1.0 - KIT_SIZE) * ((until - now) / GROW_TIME).clamp(0.0, 1.0);
                if now >= until {
                    *breeding = Breeding::Resting {
                        until: now + rng.0.random_range(BREED_REST),
                    };
                }
            }
            Breeding::Resting { until } if now >= until => *breeding = Breeding::Ready,
            _ => {}
        }
    }
}

/// Alternates rest and hop. A hop is a burst of velocity in the facing direction. A rabbit
/// looking for a mate faces the nearest other one that is looking too; otherwise the direction
/// changes by a random amount before each hop. Near the edge it turns back toward the middle.
fn hop(
    time: Res<Time>,
    map: Res<MapConfig>,
    mut rng: ResMut<GameRng>,
    mut rabbits: Query<
        (
            Entity,
            &Position,
            &mut Rotation,
            &mut LinearVelocity,
            &mut Hopping,
            &Breeding,
        ),
        With<Rabbit>,
    >,
) {
    let now = time.elapsed_secs();
    let inner = map.half_extent() - EDGE_MARGIN;
    let ready: Vec<(Entity, Vec2)> = rabbits
        .iter()
        .filter(|(_, _, _, _, _, breeding)| **breeding == Breeding::Ready)
        .map(|(entity, position, ..)| (entity, position.xz()))
        .collect();

    for (entity, position, mut rotation, mut velocity, mut hopping, breeding) in &mut rabbits {
        match *hopping {
            Hopping::Resting { until } if now >= until => {
                let here = position.xz();
                let mate = (*breeding == Breeding::Ready)
                    .then(|| nearest_other(&ready, entity, here))
                    .flatten();
                let mut heading = match mate {
                    Some(there) => {
                        let wobble = rng.0.random_range(-SEEK_WOBBLE..=SEEK_WOBBLE);
                        facing(there - here) * Quat::from_rotation_y(wobble)
                    }
                    None => {
                        rotation.0 * Quat::from_rotation_y(rng.0.random_range(-MAX_TURN..=MAX_TURN))
                    }
                };
                let landing = here + (heading * Vec3::NEG_Z).xz() * (HOP_SPEED * HOP_DURATION);
                if landing.abs().max_element() > inner {
                    heading = facing(-here);
                }
                rotation.0 = heading;
                velocity.0 = heading * Vec3::NEG_Z * HOP_SPEED;
                *hopping = Hopping::Hop { started: now };
            }
            Hopping::Hop { started } if now >= started + HOP_DURATION => {
                velocity.0 = Vec3::ZERO;
                *hopping = Hopping::Resting {
                    until: now + rng.0.random_range(REST_RANGE),
                };
            }
            _ => {}
        }
    }
}

/// The closest of `candidates` to `here` within [`SIGHT`], other than `me`.
fn nearest_other(candidates: &[(Entity, Vec2)], me: Entity, here: Vec2) -> Option<Vec2> {
    candidates
        .iter()
        .filter(|(entity, _)| *entity != me)
        .map(|(_, there)| (there.distance_squared(here), *there))
        .filter(|(d2, _)| *d2 <= SIGHT * SIGHT)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, there)| there)
}

/// The rotation that faces along `direction` on the ground.
fn facing(direction: Vec2) -> Quat {
    Transform::default()
        .looking_to(Vec3::new(direction.x, 0.0, direction.y), Vec3::Y)
        .rotation
}

/// Two ready rabbits that meet have a kit, born between them, and both rest before the next.
fn breed(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<RabbitAssets>,
    mut rng: ResMut<GameRng>,
    mut rabbits: Query<(Entity, &Position, &Rotation, &mut Breeding), With<Rabbit>>,
) {
    let now = time.elapsed_secs();
    let mut count = rabbits.iter().len();
    let ready: Vec<(Entity, Vec2, Quat)> = rabbits
        .iter()
        .filter(|(_, _, _, breeding)| **breeding == Breeding::Ready)
        .map(|(entity, position, rotation, _)| (entity, position.xz(), rotation.0))
        .collect();

    // Pair each ready rabbit with the first still-ready one close enough, in a stable order.
    let mut paired: Vec<Entity> = Vec::new();
    for (i, &(a, at_a, heading)) in ready.iter().enumerate() {
        if count >= MAX_RABBITS {
            break;
        }
        if paired.contains(&a) {
            continue;
        }
        let Some(&(b, at_b, _)) = ready[i + 1..]
            .iter()
            .find(|(b, at_b, _)| !paired.contains(b) && at_a.distance(*at_b) <= MEET_DISTANCE)
        else {
            continue;
        };
        paired.extend([a, b]);
        count += 1;

        let kit_spot = (at_a + at_b) / 2.0;
        debug!("kit born at {kit_spot} ({count} rabbits)");
        spawn_rabbit(
            &mut commands,
            &assets,
            kit_spot,
            heading,
            KIT_SIZE,
            Breeding::Growing {
                until: now + GROW_TIME,
            },
        );
        for parent in [a, b] {
            let (_, _, _, mut breeding) = rabbits.get_mut(parent).unwrap();
            *breeding = Breeding::Resting {
                until: now + rng.0.random_range(BREED_REST),
            };
        }
    }
}

/// Per-frame visual: the mesh rises and falls in an arc over a hop, and is scaled to how grown
/// the rabbit is.
fn animate_rabbit_meshes(
    time: Res<Time<Fixed>>,
    rabbits: Query<(&Rabbit, &Hopping, &Children)>,
    mut meshes: Query<(&mut Transform, &RabbitMesh)>,
) {
    // Where the fixed clock is between steps, so the arc is smooth at any frame rate.
    let now = time.elapsed_secs() + time.overstep().as_secs_f32();
    for (rabbit, hopping, children) in &rabbits {
        let lift = match *hopping {
            Hopping::Hop { started } => {
                let progress = ((now - started) / HOP_DURATION).clamp(0.0, 1.0);
                (progress * std::f32::consts::PI).sin() * HOP_HEIGHT
            }
            Hopping::Resting { .. } => 0.0,
        };
        for &child in children {
            if let Ok((mut transform, mesh)) = meshes.get_mut(child) {
                transform.scale = Vec3::splat(rabbit.size);
                transform.translation.y = mesh.rest_y * rabbit.size + lift;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_other_skips_itself_and_far_rabbits() {
        let mut world = World::new();
        let (me, near, far) = (
            world.spawn_empty().id(),
            world.spawn_empty().id(),
            world.spawn_empty().id(),
        );
        let here = Vec2::ZERO;
        let candidates = [
            (me, here),
            (far, Vec2::new(SIGHT + 1.0, 0.0)),
            (near, Vec2::new(3.0, 4.0)),
        ];
        assert_eq!(
            nearest_other(&candidates, me, here),
            Some(Vec2::new(3.0, 4.0))
        );
        assert_eq!(nearest_other(&candidates[..2], me, here), None);
    }

    #[test]
    fn facing_points_along_the_direction() {
        let forward = facing(Vec2::new(0.0, -1.0)) * Vec3::NEG_Z;
        assert!(forward.abs_diff_eq(Vec3::NEG_Z, 1e-5), "{forward}");
        let right = facing(Vec2::X) * Vec3::NEG_Z;
        assert!(right.abs_diff_eq(Vec3::X, 1e-5), "{right}");
    }
}
