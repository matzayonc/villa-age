//! Trees: placement, growth from sapling to mature, seed dispersal, felling animation and physics.

use avian3d::prelude::*;
use bevy::prelude::*;
use rand::RngExt;

use crate::GameRng;
use crate::map::MapConfig;
use crate::physics::{PLANE_LOCK, carried_layers, log_layers, obstacle_layers};
use crate::sim::SimSet;

/// Marker for tree entities. The entity's origin is the tree's center (halfway from base to tip),
/// so a single capsule collider covers it both standing and lying down; the whole entity is
/// uniformly scaled by maturity. Use [`tree_base`] for the point where it touches the ground.
#[derive(Component)]
#[require(TreeState, Maturity)]
pub struct Tree;

#[derive(Component)]
pub enum TreeState {
    /// Upright; `damage` is the chopping work done so far (see [`max_health`]).
    Standing { damage: f32 },
    /// Tipping over from `base` toward `dir` (unit vector on the ground); `progress` runs 0..=1.
    Falling {
        base: Vec3,
        dir: Vec3,
        progress: f32,
    },
    /// Lying on the ground, free for a villager to drag away. Villagers climb over it.
    Fallen,
    /// Being dragged by this villager.
    Carried(Entity),
    /// Dropped off at a villager's home; no longer interacted with. Villagers climb over it.
    Delivered,
}

impl Default for TreeState {
    fn default() -> Self {
        Self::Standing { damage: 0.0 }
    }
}

/// How grown a tree is, 0 (fresh sapling) to 1 (fully mature). Drives its size.
#[derive(Component, Default, Clone, Copy)]
pub struct Maturity(pub f32);

impl Maturity {
    /// Uniform scale of the tree entity. Stepped in `GROWTH_STEPS` increments: every change to a
    /// tree's scale makes the physics engine rescale its collider and recompute its mass, so a
    /// tree that grew a hair every frame would cost more than everything else in the sim.
    pub fn scale(self) -> f32 {
        let stepped = (self.0.clamp(0.0, 1.0) * GROWTH_STEPS).floor() / GROWTH_STEPS;
        SAPLING_SCALE + (1.0 - SAPLING_SCALE) * stepped
    }
}

/// Counts down to the next sapling a mature tree drops.
#[derive(Component)]
struct SeedTimer(Timer);

pub struct TreesPlugin;

impl Plugin for TreesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_trees)
            // Everything that moves a body runs at the physics rate, on the physics components.
            .add_systems(
                FixedUpdate,
                (
                    grow_trees,
                    disperse_seeds,
                    animate_falling_trees,
                    sync_tree_bodies,
                )
                    .chain()
                    .in_set(SimSet::Trees),
            );
    }
}

/// Chopping work (in seconds of one villager chopping) to fell a fully grown tree.
const TREE_HEALTH: f32 = 4.0;
/// Seconds it takes a felled tree to hit the ground.
const FALL_DURATION: f32 = 1.2;
pub const TRUNK_RADIUS: f32 = 0.25;
pub const TRUNK_HEIGHT: f32 = 1.2;
pub const CANOPY_RADIUS: f32 = 1.1;
pub const CANOPY_HEIGHT: f32 = 2.4;
/// Base-to-tip length of a fully grown tree.
pub const TREE_LENGTH: f32 = TRUNK_HEIGHT + CANOPY_HEIGHT;
/// Offset from the tree's origin (its center) to its base, in unscaled local space.
pub const BASE_OFFSET: Vec3 = Vec3::new(0.0, -TREE_LENGTH / 2.0, 0.0);

/// Growth: seconds from sapling to fully mature, and how big a fresh sapling is.
const GROW_TIME: f32 = 90.0;
const SAPLING_SCALE: f32 = 0.2;
/// Number of distinct sizes a tree passes through while growing (see [`Maturity::scale`]).
const GROWTH_STEPS: f32 = 100.0;
/// Trees at or above this maturity drop seeds.
const SEED_MATURITY: f32 = 0.9;
/// Seconds between seeds from one tree (random per tree within this range).
const SEED_INTERVAL: std::ops::RangeInclusive<f32> = 25.0..=45.0;
/// Distance from the parent at which a seed can land.
const SEED_DISTANCE: std::ops::RangeInclusive<f32> = 2.5..=6.0;
/// Minimum distance between a new sapling and any tree base or villager.
const TREE_SPACING: f32 = 2.0;
/// Random spots tried per seed before giving up.
const SEED_ATTEMPTS: usize = 6;
/// The map won't hold more trees than this, per [`crate::map::TUNING_AREA`] (scaled by area).
pub const MAX_TREES: usize = 150;
/// Trees can't be placed closer than this to the map edge.
const EDGE_MARGIN: f32 = 2.0;

/// Chopping work needed to fell a tree of this maturity.
pub fn max_health(maturity: Maturity) -> f32 {
    TREE_HEALTH * maturity.scale()
}

/// Where the tree touches the ground, in world space, from its physics pose.
pub fn tree_base(position: &Position, rotation: &Rotation, maturity: Maturity) -> Vec3 {
    position.0 + rotation.0 * (BASE_OFFSET * maturity.scale())
}

/// Spawns a standing tree with its base at `base` on the ground. The transform's scale is part
/// of gameplay: the collider follows it.
fn spawn_tree(commands: &mut Commands, base: Vec2, maturity: Maturity) {
    let scale = maturity.scale();
    let center = Vec3::new(base.x, scale * TREE_LENGTH / 2.0, base.y);
    commands.spawn((
        Tree,
        maturity,
        Transform {
            translation: center,
            scale: Vec3::splat(scale),
            ..default()
        },
        // Bodies start with their physics pose set explicitly (see `physics.rs`).
        Position(center),
        Rotation::IDENTITY,
        RigidBody::Static,
        Collider::capsule(TRUNK_RADIUS, TREE_LENGTH - 2.0 * TRUNK_RADIUS),
        obstacle_layers(),
    ));
}

/// Whether a sapling can go at `spot`: on the map and clear of every point in `occupied`.
fn spot_is_free(map: &MapConfig, spot: Vec2, occupied: impl IntoIterator<Item = Vec2>) -> bool {
    let half = map.half_extent() - EDGE_MARGIN;
    spot.abs().max_element() <= half
        && occupied
            .into_iter()
            .all(|p| p.distance_squared(spot) >= TREE_SPACING * TREE_SPACING)
}

/// Spawns the trees the map lists.
fn spawn_trees(mut commands: Commands, map: Res<MapConfig>) {
    for tree in &map.trees {
        spawn_tree(&mut commands, Vec2::from(tree.pos), Maturity(tree.maturity));
    }
}

/// Standing trees grow toward full size; once mature enough they start dropping seeds.
fn grow_trees(
    mut commands: Commands,
    time: Res<Time>,
    mut rng: ResMut<GameRng>,
    mut trees: Query<(
        Entity,
        &TreeState,
        &mut Maturity,
        &mut Position,
        &mut Transform,
        Has<SeedTimer>,
    )>,
) {
    for (entity, state, mut maturity, mut position, mut transform, has_timer) in &mut trees {
        if !matches!(state, TreeState::Standing { .. }) {
            continue;
        }
        if maturity.0 < 1.0 {
            maturity.0 = (maturity.0 + time.delta_secs() / GROW_TIME).min(1.0);
            let scale = maturity.scale();
            // Only touch the body when the stepped size actually changes. Scale isn't part of
            // the physics pose, so it goes on the transform, which the collider follows.
            if transform.scale.x != scale {
                transform.scale = Vec3::splat(scale);
                position.y = scale * TREE_LENGTH / 2.0;
            }
        }
        if maturity.0 >= SEED_MATURITY && !has_timer {
            let interval = rng.0.random_range(SEED_INTERVAL);
            commands
                .entity(entity)
                .insert(SeedTimer(Timer::from_seconds(
                    interval,
                    TimerMode::Repeating,
                )));
        }
    }
}

/// Mature trees periodically drop a sapling at a free spot nearby.
fn disperse_seeds(
    mut commands: Commands,
    time: Res<Time>,
    map: Res<MapConfig>,
    mut rng: ResMut<GameRng>,
    mut parents: Query<(&Position, &Rotation, &Maturity, &TreeState, &mut SeedTimer)>,
    trees: Query<(&Position, &Rotation, &Maturity), With<Tree>>,
    villagers: Query<&Position, With<crate::entities::villagers::Villager>>,
) {
    let mut tree_count = trees.iter().len();
    let max_trees = map.scale_count(MAX_TREES);
    let mut occupied: Vec<Vec2> = trees
        .iter()
        .map(|(p, r, &m)| tree_base(p, r, m).xz())
        .chain(villagers.iter().map(|p| p.xz()))
        .collect();

    for (position, rotation, &maturity, state, mut timer) in &mut parents {
        if !timer.0.tick(time.delta()).just_finished()
            || !matches!(state, TreeState::Standing { .. })
            || tree_count >= max_trees
        {
            continue;
        }
        let parent = tree_base(position, rotation, maturity).xz();
        let rng = &mut rng.0;

        for _ in 0..SEED_ATTEMPTS {
            let angle = rng.random_range(0.0..std::f32::consts::TAU);
            let spot = parent + Vec2::from_angle(angle) * rng.random_range(SEED_DISTANCE);
            if spot_is_free(&map, spot, occupied.iter().copied()) {
                debug!("sapling dropped at {spot} ({} trees)", tree_count + 1);
                spawn_tree(&mut commands, spot, Maturity(0.0));
                occupied.push(spot);
                tree_count += 1;
                break;
            }
        }
    }
}

/// Tips felled trees over around their base until they lie flat, then marks them `Fallen`.
fn animate_falling_trees(
    time: Res<Time>,
    mut trees: Query<(&mut TreeState, &mut Position, &mut Rotation, &Maturity)>,
) {
    for (mut state, mut position, mut rotation, &maturity) in &mut trees {
        let TreeState::Falling {
            base,
            dir,
            progress,
        } = *state
        else {
            continue;
        };
        let progress = (progress + time.delta_secs() / FALL_DURATION).min(1.0);
        // Ease in: a tree starts tipping slowly and accelerates.
        let eased = progress * progress;
        let scale = maturity.scale();

        let axis = Vec3::Y.cross(dir).normalize();
        let tilt = Quat::from_axis_angle(axis, eased * std::f32::consts::FRAC_PI_2);
        // Pivot around the base, lifting it by the trunk radius as it comes to rest on its side.
        rotation.0 = tilt;
        position.0 = base + Vec3::Y * (TRUNK_RADIUS * scale * eased) - tilt * (BASE_OFFSET * scale);

        *state = if progress >= 1.0 {
            TreeState::Fallen
        } else {
            TreeState::Falling {
                base,
                dir,
                progress,
            }
        };
    }
}

/// Keeps each tree's rigid body in step with its state: static while standing (an obstacle) or
/// lying down (a log, walked over), kinematic while the fall animation drives it, dynamic (held
/// by a joint) while being dragged.
fn sync_tree_bodies(
    mut commands: Commands,
    trees: Query<(Entity, &TreeState, &RigidBody, &CollisionLayers), Changed<TreeState>>,
) {
    for (entity, state, body, layers) in &trees {
        let (wanted_body, wanted_layers) = match state {
            TreeState::Standing { .. } => (RigidBody::Static, obstacle_layers()),
            TreeState::Falling { .. } => (RigidBody::Kinematic, obstacle_layers()),
            TreeState::Fallen | TreeState::Delivered => (RigidBody::Static, log_layers()),
            TreeState::Carried(_) => (RigidBody::Dynamic, carried_layers()),
        };
        if *body == wanted_body && *layers == wanted_layers {
            continue;
        }

        let mut tree = commands.entity(entity);
        tree.insert((
            wanted_body,
            wanted_layers,
            LinearVelocity::ZERO,
            AngularVelocity::ZERO,
        ));
        // A felled tree moves (the fall, then being dragged): smooth its transform between
        // physics steps from here on. Standing trees never move, so they don't pay for it.
        if matches!(state, TreeState::Falling { .. }) {
            tree.insert(TransformInterpolation);
        }
        if wanted_body == RigidBody::Dynamic {
            tree.insert((
                PLANE_LOCK,
                Mass(5.0),
                LinearDamping(2.0),
                AngularDamping(4.0),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maturity_scales_from_sapling_to_full_size() {
        assert_eq!(Maturity(0.0).scale(), SAPLING_SCALE);
        assert_eq!(Maturity(1.0).scale(), 1.0);
        let half = Maturity(0.5).scale();
        assert!(half > SAPLING_SCALE && half < 1.0);
        // Growth is stepped: a sliver of maturity doesn't change the size.
        assert_eq!(
            Maturity(0.5).scale(),
            Maturity(0.5 + 0.4 / GROWTH_STEPS).scale()
        );
        assert!(Maturity(0.5 + 1.0 / GROWTH_STEPS).scale() > half);
        // Out-of-range values clamp rather than extrapolate.
        assert_eq!(Maturity(-1.0).scale(), SAPLING_SCALE);
        assert_eq!(Maturity(3.0).scale(), 1.0);
    }

    #[test]
    fn health_grows_with_maturity() {
        assert_eq!(max_health(Maturity(1.0)), TREE_HEALTH);
        assert!(max_health(Maturity(0.0)) < max_health(Maturity(1.0)));
        assert!(max_health(Maturity(0.0)) > 0.0);
    }

    #[test]
    fn tree_base_is_below_the_center_when_upright() {
        let maturity = Maturity(0.5);
        let position = Position::from_xyz(3.0, maturity.scale() * TREE_LENGTH / 2.0, -4.0);
        let base = tree_base(&position, &Rotation::IDENTITY, maturity);
        assert!(base.abs_diff_eq(Vec3::new(3.0, 0.0, -4.0), 1e-5), "{base}");
    }

    #[test]
    fn tree_base_follows_rotation_when_lying_down() {
        // Tipped 90° around Z: the base is now beside the center along +X.
        let rotation = Rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2));
        let base = tree_base(&Position::default(), &rotation, Maturity(1.0));
        assert!(
            base.abs_diff_eq(Vec3::new(TREE_LENGTH / 2.0, 0.0, 0.0), 1e-5),
            "{base}"
        );
    }

    #[test]
    fn spot_is_free_respects_edge_margin_and_spacing() {
        let map = MapConfig::from_ron("(size: 20.0, villagers: [], trees: [])").unwrap();
        let inner = map.half_extent() - EDGE_MARGIN;

        assert!(spot_is_free(&map, Vec2::ZERO, []));
        assert!(spot_is_free(&map, Vec2::new(inner, -inner), []));
        assert!(!spot_is_free(&map, Vec2::new(inner + 0.01, 0.0), []));
        assert!(!spot_is_free(&map, Vec2::new(0.0, -inner - 0.01), []));

        let occupied = [Vec2::new(5.0, 5.0)];
        assert!(!spot_is_free(
            &map,
            Vec2::new(5.0, 5.0 + TREE_SPACING - 0.01),
            occupied
        ));
        assert!(spot_is_free(
            &map,
            Vec2::new(5.0, 5.0 + TREE_SPACING),
            occupied
        ));
        assert!(!spot_is_free(&map, Vec2::new(6.0, 6.0), occupied));
    }
}
