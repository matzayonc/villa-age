//! Rabbits: small critters that hop about at random. They bump into trees and people and get
//! shoved aside, but nobody plans around them. The body is a capsule locked to the ground plane;
//! what it looks like is `visuals::rabbits`' business.
//!
//! Rabbits get hungry now and then. A hungry rabbit looks for a carrot once per hop, as it sets
//! off: if one is in sight it hops toward it (landing on it if it's within a hop), otherwise it
//! hops wherever. Landing on a carrot eats it: the rabbit sits chewing and is full for a while.

use avian3d::prelude::*;
use bevy::prelude::*;
use rand::RngExt;

use crate::GameRng;
use crate::entities::carrots::Carrot;
use crate::map::MapConfig;
use crate::physics::critter_layers;
use crate::sim::SimSet;

/// Marker for rabbit bodies.
#[derive(Component)]
pub struct Rabbit;

/// What a rabbit is doing, in physics (fixed-clock) seconds.
#[derive(Component)]
pub enum Hopping {
    /// Sitting still until `until`, then it picks a new direction.
    Resting { until: f32 },
    /// Mid-hop: took off at `started`, lands at `started + HOP_DURATION`, on `carrot` if it was
    /// heading for one.
    Hop {
        started: f32,
        carrot: Option<Entity>,
    },
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
/// How far a full hop carries.
const HOP_LENGTH: f32 = HOP_SPEED * HOP_DURATION;
/// Time spent sitting between hops, rolled per rest.
const REST_RANGE: std::ops::RangeInclusive<f32> = 0.4..=2.0;
/// Largest change of direction between wandering hops, in radians either way.
const MAX_TURN: f32 = std::f32::consts::FRAC_PI_2;
/// A rabbit heading for a carrot still veers a little, in radians either way, until it's within
/// a hop of it.
const SEEK_WOBBLE: f32 = 0.3;
/// Rabbits won't hop closer than this to the map edge.
const EDGE_MARGIN: f32 = 1.5;
/// How far a hungry rabbit notices a carrot.
const SIGHT: f32 = 15.0;

/// Seconds a rabbit stays full after a carrot, rolled per meal.
const FULL_TIME: std::ops::RangeInclusive<f32> = 40.0..=80.0;
/// Rabbits placed by the map get hungry at their own times.
const FIRST_HUNGRY: std::ops::RangeInclusive<f32> = 5.0..=40.0;
/// How close a rabbit must land to a carrot to eat it, and how long it sits chewing.
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
            (get_hungry, hop).chain().in_set(SimSet::Critters),
        );
    }
}

/// The map's rabbits, each facing its own way and getting hungry at its own time.
fn spawn_rabbits(mut commands: Commands, map: Res<MapConfig>, mut rng: ResMut<GameRng>) {
    for &(x, z) in &map.rabbits {
        let heading = Quat::from_rotation_y(rng.0.random_range(0.0..std::f32::consts::TAU));
        let position = Vec3::new(x, GROUND_Y, z);
        commands.spawn((
            Rabbit,
            Name::new("Rabbit"),
            Hopping::Resting { until: 0.0 },
            Appetite::Full {
                until: rng.0.random_range(FIRST_HUNGRY),
            },
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
}

/// A rabbit gets hungry once its last meal wears off.
fn get_hungry(time: Res<Time>, mut rabbits: Query<&mut Appetite, With<Rabbit>>) {
    let now = time.elapsed_secs();
    for mut appetite in &mut rabbits {
        if matches!(*appetite, Appetite::Full { until } if now >= until) {
            *appetite = Appetite::Hungry;
        }
    }
}

/// Alternates rest and hop. A hop is a burst of velocity in the facing direction. Setting off, a
/// hungry rabbit looks for the nearest carrot in sight and heads for it, landing right on it once
/// it's within a hop; otherwise the direction changes by a random amount. Near the edge it turns
/// back toward the middle. Landing on the carrot it was after eats it.
fn hop(
    mut commands: Commands,
    time: Res<Time>,
    map: Res<MapConfig>,
    mut rng: ResMut<GameRng>,
    carrots: Query<(Entity, &Transform), With<Carrot>>,
    mut rabbits: Query<
        (
            &Position,
            &mut Rotation,
            &mut LinearVelocity,
            &mut Hopping,
            &mut Appetite,
        ),
        With<Rabbit>,
    >,
) {
    let now = time.elapsed_secs();
    let inner = map.half_extent() - EDGE_MARGIN;
    // Carrots already eaten this step: a despawn only lands after the system.
    let mut eaten: Vec<Entity> = Vec::new();

    for (position, mut rotation, mut velocity, mut hopping, mut appetite) in &mut rabbits {
        match *hopping {
            Hopping::Resting { until } if now >= until => {
                let here = position.xz();
                let target = (*appetite == Appetite::Hungry)
                    .then(|| nearest_carrot(&carrots, here))
                    .flatten();
                let (mut heading, speed) = match target {
                    Some((_, there)) if there.distance(here) <= HOP_LENGTH => {
                        // Within reach: a shorter hop that lands on it.
                        (facing(there - here), there.distance(here) / HOP_DURATION)
                    }
                    Some((_, there)) => {
                        let wobble = rng.0.random_range(-SEEK_WOBBLE..=SEEK_WOBBLE);
                        (
                            facing(there - here) * Quat::from_rotation_y(wobble),
                            HOP_SPEED,
                        )
                    }
                    None => (
                        rotation.0
                            * Quat::from_rotation_y(rng.0.random_range(-MAX_TURN..=MAX_TURN)),
                        HOP_SPEED,
                    ),
                };
                let landing = here + (heading * Vec3::NEG_Z).xz() * (speed * HOP_DURATION);
                if landing.abs().max_element() > inner {
                    heading = facing(-here);
                }
                rotation.0 = heading;
                velocity.0 = heading * Vec3::NEG_Z * speed;
                *hopping = Hopping::Hop {
                    started: now,
                    carrot: target.map(|(carrot, _)| carrot),
                };
            }
            Hopping::Hop { started, carrot } if now >= started + HOP_DURATION => {
                velocity.0 = Vec3::ZERO;
                let reached = carrot
                    .filter(|carrot| !eaten.contains(carrot))
                    .and_then(|carrot| carrots.get(carrot).ok())
                    .is_some_and(|(_, transform)| {
                        transform.translation.xz().distance(position.xz()) <= EAT_DISTANCE
                    });
                if reached {
                    let carrot = carrot.unwrap();
                    debug!(
                        "carrot at {} eaten",
                        carrots.get(carrot).unwrap().1.translation.xz()
                    );
                    commands.entity(carrot).despawn();
                    eaten.push(carrot);
                    *appetite = Appetite::Full {
                        until: now + EAT_TIME + rng.0.random_range(FULL_TIME),
                    };
                }
                *hopping = Hopping::Resting {
                    until: now
                        + if reached {
                            EAT_TIME
                        } else {
                            rng.0.random_range(REST_RANGE)
                        },
                };
            }
            _ => {}
        }
    }
}

/// The closest carrot to `here` within [`SIGHT`], and where it is.
fn nearest_carrot(
    carrots: &Query<(Entity, &Transform), With<Carrot>>,
    here: Vec2,
) -> Option<(Entity, Vec2)> {
    carrots
        .iter()
        .map(|(entity, transform)| (entity, transform.translation.xz()))
        .map(|(entity, there)| (there.distance_squared(here), entity, there))
        .filter(|(d2, ..)| *d2 <= SIGHT * SIGHT)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, entity, there)| (entity, there))
}

/// The rotation that faces along `direction` on the ground.
fn facing(direction: Vec2) -> Quat {
    Transform::default()
        .looking_to(Vec3::new(direction.x, 0.0, direction.y), Vec3::Y)
        .rotation
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facing_points_along_the_direction() {
        let forward = facing(Vec2::new(0.0, -1.0)) * Vec3::NEG_Z;
        assert!(forward.abs_diff_eq(Vec3::NEG_Z, 1e-5), "{forward}");
        let right = facing(Vec2::X) * Vec3::NEG_Z;
        assert!(right.abs_diff_eq(Vec3::X, 1e-5), "{right}");
    }
}
