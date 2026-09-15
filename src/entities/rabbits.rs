//! Rabbits: small critters that hop about, seek each other out and multiply. They bump into
//! trees and people and get shoved aside, but nobody plans around them. The body is a capsule
//! locked to the ground plane; what it looks like is `visuals::rabbits`' business.
//!
//! A rabbit is born a kit, grows up, and from then on alternates between being ready to breed
//! (hopping toward the nearest other ready rabbit) and resting after a litter. Two ready rabbits
//! that meet produce one kit, up to a population cap.
//!
//! Rabbits also get hungry now and then. A hungry rabbit hops for the nearest carrot it can see
//! (before any mate), sits chewing when it reaches one, and is full for a while after.

use avian3d::prelude::*;
use bevy::prelude::*;
use rand::RngExt;

use crate::GameRng;
use crate::entities::carrots::Carrot;
use crate::map::MapConfig;
use crate::physics::critter_layers;
use crate::sim::SimSet;

/// Marker for rabbit bodies. `size` is 0..=1: how grown the rabbit is, which scales its visual.
#[derive(Component)]
pub struct Rabbit {
    pub size: f32,
}

/// What a rabbit is doing, in physics (fixed-clock) seconds.
#[derive(Component)]
pub enum Hopping {
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

/// Whether a rabbit wants a carrot, in physics (fixed-clock) seconds.
#[derive(Component, Clone, Copy, PartialEq)]
pub enum Appetite {
    /// Ate recently; hungry again at `until`.
    Full { until: f32 },
    /// Looking for a carrot.
    Hungry,
}

/// Ground speed during a hop, and how long one lasts.
const HOP_SPEED: f32 = 2.5;
pub const HOP_DURATION: f32 = 0.3;
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
/// Seconds a rabbit stays full after a carrot (or from birth), rolled per meal.
const FULL_TIME: std::ops::RangeInclusive<f32> = 40.0..=80.0;
/// Rabbits placed by the map get hungry at their own times.
const FIRST_HUNGRY: std::ops::RangeInclusive<f32> = 5.0..=40.0;
/// How close a rabbit must get to a carrot to eat it, and how long it sits chewing.
const EAT_DISTANCE: f32 = 0.4;
const EAT_TIME: f32 = 2.0;

/// The body capsule.
pub const RADIUS: f32 = 0.12;
pub const HEIGHT: f32 = 0.16;
/// Height of the capsule's center when it sits on the ground.
const GROUND_Y: f32 = HEIGHT / 2.0 + RADIUS;

pub struct RabbitsPlugin;

impl Plugin for RabbitsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_rabbits).add_systems(
            FixedUpdate,
            (grow_up, eat, hop, breed).chain().in_set(SimSet::Critters),
        );
    }
}

/// The map's rabbits: adults, each facing its own way and coming into season at its own time.
fn spawn_rabbits(mut commands: Commands, map: Res<MapConfig>, mut rng: ResMut<GameRng>) {
    for &(x, z) in &map.rabbits {
        let heading = Quat::from_rotation_y(rng.0.random_range(0.0..std::f32::consts::TAU));
        let breeding = Breeding::Resting {
            until: rng.0.random_range(FIRST_READY),
        };
        let appetite = Appetite::Full {
            until: rng.0.random_range(FIRST_HUNGRY),
        };
        spawn_rabbit(
            &mut commands,
            Vec2::new(x, z),
            heading,
            1.0,
            breeding,
            appetite,
        );
    }
}

fn spawn_rabbit(
    commands: &mut Commands,
    spot: Vec2,
    heading: Quat,
    size: f32,
    breeding: Breeding,
    appetite: Appetite,
) {
    let position = Vec3::new(spot.x, GROUND_Y, spot.y);
    commands.spawn((
        Rabbit { size },
        Name::new("Rabbit"),
        Hopping::Resting { until: 0.0 },
        breeding,
        appetite,
        Transform::from_translation(position).with_rotation(heading),
        Position(position),
        Rotation(heading),
        TransformInterpolation,
        RigidBody::Dynamic,
        Collider::capsule(RADIUS, HEIGHT),
        // Faces the way it hops; physics never turns it.
        LockedAxes::ROTATION_LOCKED.lock_translation_y(),
        critter_layers(),
        Mass(1.0),
    ));
}

/// Kits grow toward full size; grown rabbits come into season once their rest is over; every
/// rabbit gets hungry once its last meal wears off.
fn grow_up(
    time: Res<Time>,
    mut rng: ResMut<GameRng>,
    mut rabbits: Query<(&mut Rabbit, &mut Breeding, &mut Appetite)>,
) {
    let now = time.elapsed_secs();
    for (mut rabbit, mut breeding, mut appetite) in &mut rabbits {
        if matches!(*appetite, Appetite::Full { until } if now >= until) {
            *appetite = Appetite::Hungry;
        }
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

/// Alternates rest and hop. A hop is a burst of velocity in the facing direction. A hungry
/// rabbit faces the nearest carrot in sight; failing that, one looking for a mate faces the
/// nearest other one that is looking too; otherwise the direction changes by a random amount
/// before each hop. Near the edge it turns back toward the middle.
fn hop(
    time: Res<Time>,
    map: Res<MapConfig>,
    mut rng: ResMut<GameRng>,
    carrots: Query<&Transform, With<Carrot>>,
    mut rabbits: Query<
        (
            Entity,
            &Position,
            &mut Rotation,
            &mut LinearVelocity,
            &mut Hopping,
            &Breeding,
            &Appetite,
        ),
        With<Rabbit>,
    >,
) {
    let now = time.elapsed_secs();
    let inner = map.half_extent() - EDGE_MARGIN;
    let ready: Vec<(Entity, Vec2)> = rabbits
        .iter()
        .filter(|(_, _, _, _, _, breeding, _)| **breeding == Breeding::Ready)
        .map(|(entity, position, ..)| (entity, position.xz()))
        .collect();
    let food: Vec<Vec2> = carrots.iter().map(|t| t.translation.xz()).collect();

    for (entity, position, mut rotation, mut velocity, mut hopping, breeding, appetite) in
        &mut rabbits
    {
        match *hopping {
            Hopping::Resting { until } if now >= until => {
                let here = position.xz();
                let carrot = (*appetite == Appetite::Hungry)
                    .then(|| nearest(food.iter().copied(), here))
                    .flatten();
                let mate = (carrot.is_none() && *breeding == Breeding::Ready)
                    .then(|| nearest_other(&ready, entity, here))
                    .flatten();
                let mut heading = match carrot.or(mate) {
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
    let others = candidates
        .iter()
        .filter(|(entity, _)| *entity != me)
        .map(|(_, there)| *there);
    nearest(others, here)
}

/// The closest of `spots` to `here` within [`SIGHT`].
fn nearest(spots: impl IntoIterator<Item = Vec2>, here: Vec2) -> Option<Vec2> {
    spots
        .into_iter()
        .map(|there| (there.distance_squared(here), there))
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

/// A hungry rabbit that reaches a carrot eats it: the carrot goes, the rabbit sits chewing and
/// is full for a while. One carrot feeds one rabbit; whoever is first in query order gets it.
fn eat(
    mut commands: Commands,
    time: Res<Time>,
    mut rng: ResMut<GameRng>,
    carrots: Query<(Entity, &Transform), With<Carrot>>,
    mut rabbits: Query<(&Position, &mut Hopping, &mut LinearVelocity, &mut Appetite), With<Rabbit>>,
) {
    let now = time.elapsed_secs();
    let mut left: Vec<(Entity, Vec2)> = carrots
        .iter()
        .map(|(entity, t)| (entity, t.translation.xz()))
        .collect();
    for (position, mut hopping, mut velocity, mut appetite) in &mut rabbits {
        if *appetite != Appetite::Hungry {
            continue;
        }
        let here = position.xz();
        let Some(i) = left
            .iter()
            .position(|(_, spot)| spot.distance_squared(here) <= EAT_DISTANCE * EAT_DISTANCE)
        else {
            continue;
        };
        let (carrot, spot) = left.swap_remove(i);
        debug!("carrot at {spot} eaten");
        commands.entity(carrot).despawn();
        velocity.0 = Vec3::ZERO;
        *hopping = Hopping::Resting {
            until: now + EAT_TIME,
        };
        *appetite = Appetite::Full {
            until: now + EAT_TIME + rng.0.random_range(FULL_TIME),
        };
    }
}

/// Two ready rabbits that meet have a kit, born between them, and both rest before the next.
fn breed(
    mut commands: Commands,
    time: Res<Time>,
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
            kit_spot,
            heading,
            KIT_SIZE,
            Breeding::Growing {
                until: now + GROW_TIME,
            },
            Appetite::Full {
                until: now + rng.0.random_range(FULL_TIME),
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
