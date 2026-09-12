//! Trees: placement, growth from sapling to mature, seed dispersal, felling animation and physics.

use avian3d::prelude::*;
use bevy::prelude::*;
use rand::RngExt;

use crate::GameRng;
use crate::map::MAP_SIZE;
use crate::physics::{PLANE_LOCK, carried_layers, obstacle_layers};
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
    /// Lying on the ground, free for a character to drag away.
    Fallen,
    /// Being dragged by this character.
    Carried(Entity),
    /// Dropped off at a character's home; no longer interacted with.
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
    /// Uniform scale of the tree entity.
    pub fn scale(self) -> f32 {
        SAPLING_SCALE + (1.0 - SAPLING_SCALE) * self.0.clamp(0.0, 1.0)
    }
}

/// Counts down to the next sapling a mature tree drops.
#[derive(Component)]
struct SeedTimer(Timer);

/// Shared meshes and materials for spawning trees.
#[derive(Resource)]
struct TreeAssets {
    trunk_mesh: Handle<Mesh>,
    canopy_mesh: Handle<Mesh>,
    trunk_material: Handle<StandardMaterial>,
    canopy_material: Handle<StandardMaterial>,
}

pub struct TreesPlugin;

impl Plugin for TreesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (load_tree_assets, spawn_trees).chain())
            .add_systems(
                Update,
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

const TREE_COUNT: u32 = 24;
/// Chopping work (in seconds of one character chopping) to fell a fully grown tree.
const TREE_HEALTH: f32 = 4.0;
/// Seconds it takes a felled tree to hit the ground.
const FALL_DURATION: f32 = 1.2;
pub const TRUNK_RADIUS: f32 = 0.25;
const TRUNK_HEIGHT: f32 = 1.2;
const CANOPY_RADIUS: f32 = 1.1;
const CANOPY_HEIGHT: f32 = 2.4;
/// Base-to-tip length of a fully grown tree.
pub const TREE_LENGTH: f32 = TRUNK_HEIGHT + CANOPY_HEIGHT;
/// Offset from the tree's origin (its center) to its base, in unscaled local space.
pub const BASE_OFFSET: Vec3 = Vec3::new(0.0, -TREE_LENGTH / 2.0, 0.0);

/// Growth: seconds from sapling to fully mature, and how big a fresh sapling is.
const GROW_TIME: f32 = 90.0;
const SAPLING_SCALE: f32 = 0.2;
/// Trees at or above this maturity drop seeds.
const SEED_MATURITY: f32 = 0.9;
/// Seconds between seeds from one tree (random per tree within this range).
const SEED_INTERVAL: std::ops::RangeInclusive<f32> = 25.0..=45.0;
/// Distance from the parent at which a seed can land.
const SEED_DISTANCE: std::ops::RangeInclusive<f32> = 2.5..=6.0;
/// Minimum distance between a new sapling and any tree base or character.
const TREE_SPACING: f32 = 2.0;
/// Random spots tried per seed before giving up.
const SEED_ATTEMPTS: usize = 6;
pub const MAX_TREES: usize = 150;
/// Trees can't be placed closer than this to the map edge.
const EDGE_MARGIN: f32 = 2.0;

/// Chopping work needed to fell a tree of this maturity.
pub fn max_health(maturity: Maturity) -> f32 {
    TREE_HEALTH * maturity.scale()
}

/// Where the tree touches the ground, in world space.
pub fn tree_base(transform: &Transform) -> Vec3 {
    transform.translation + transform.rotation * (BASE_OFFSET * transform.scale.y)
}

fn load_tree_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(TreeAssets {
        trunk_mesh: meshes.add(Cylinder::new(TRUNK_RADIUS, TRUNK_HEIGHT)),
        canopy_mesh: meshes.add(Cone::new(CANOPY_RADIUS, CANOPY_HEIGHT)),
        trunk_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.45, 0.3, 0.15),
            perceptual_roughness: 1.0,
            ..default()
        }),
        canopy_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.15, 0.5, 0.2),
            perceptual_roughness: 0.9,
            ..default()
        }),
    });
}

/// Spawns a standing tree with its base at `base` on the ground.
fn spawn_tree(commands: &mut Commands, assets: &TreeAssets, base: Vec2, maturity: Maturity) {
    let scale = maturity.scale();
    // To use a real model, replace the children with `SceneRoot(asset_server.load("tree.glb#Scene0"))`.
    commands
        .spawn((
            Tree,
            maturity,
            Transform {
                translation: Vec3::new(base.x, scale * TREE_LENGTH / 2.0, base.y),
                scale: Vec3::splat(scale),
                ..default()
            },
            Visibility::default(),
            RigidBody::Static,
            Collider::capsule(TRUNK_RADIUS, TREE_LENGTH - 2.0 * TRUNK_RADIUS),
            obstacle_layers(),
        ))
        .with_children(|parent| {
            parent.spawn((
                Mesh3d(assets.trunk_mesh.clone()),
                MeshMaterial3d(assets.trunk_material.clone()),
                Transform::from_translation(BASE_OFFSET + Vec3::Y * (TRUNK_HEIGHT / 2.0)),
            ));
            parent.spawn((
                Mesh3d(assets.canopy_mesh.clone()),
                MeshMaterial3d(assets.canopy_material.clone()),
                Transform::from_translation(
                    BASE_OFFSET + Vec3::Y * (TRUNK_HEIGHT + CANOPY_HEIGHT / 2.0),
                ),
            ));
        });
}

/// Whether a sapling can go at `spot`: on the map and clear of every point in `occupied`.
fn spot_is_free(spot: Vec2, occupied: impl IntoIterator<Item = Vec2>) -> bool {
    let half = MAP_SIZE / 2.0 - EDGE_MARGIN;
    spot.abs().max_element() <= half
        && occupied
            .into_iter()
            .all(|p| p.distance_squared(spot) >= TREE_SPACING * TREE_SPACING)
}

fn spawn_trees(mut commands: Commands, assets: Res<TreeAssets>, mut rng: ResMut<GameRng>) {
    let rng = &mut rng.0;
    let half = MAP_SIZE / 2.0 - EDGE_MARGIN;
    let mut placed: Vec<Vec2> = Vec::new();

    for _ in 0..TREE_COUNT {
        for _ in 0..SEED_ATTEMPTS {
            let spot = Vec2::new(
                rng.random_range(-half..=half),
                rng.random_range(-half..=half),
            );
            // Keep the middle clear so characters don't start inside a tree.
            if spot.abs().max_element() < 3.0 || !spot_is_free(spot, placed.iter().copied()) {
                continue;
            }
            let maturity = Maturity(rng.random_range(0.2..=1.0));
            spawn_tree(&mut commands, &assets, spot, maturity);
            placed.push(spot);
            break;
        }
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
        &mut Transform,
        Has<SeedTimer>,
    )>,
) {
    for (entity, state, mut maturity, mut transform, has_timer) in &mut trees {
        if !matches!(state, TreeState::Standing { .. }) {
            continue;
        }
        if maturity.0 < 1.0 {
            maturity.0 = (maturity.0 + time.delta_secs() / GROW_TIME).min(1.0);
            let scale = maturity.scale();
            transform.scale = Vec3::splat(scale);
            transform.translation.y = scale * TREE_LENGTH / 2.0;
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
    assets: Res<TreeAssets>,
    mut rng: ResMut<GameRng>,
    mut parents: Query<(&Transform, &TreeState, &mut SeedTimer)>,
    trees: Query<&Transform, With<Tree>>,
    characters: Query<&Transform, With<crate::characters::Character>>,
) {
    let mut tree_count = trees.iter().len();
    let mut occupied: Vec<Vec2> = trees
        .iter()
        .map(|t| tree_base(t).xz())
        .chain(characters.iter().map(|t| t.translation.xz()))
        .collect();

    for (transform, state, mut timer) in &mut parents {
        if !timer.0.tick(time.delta()).just_finished()
            || !matches!(state, TreeState::Standing { .. })
            || tree_count >= MAX_TREES
        {
            continue;
        }
        let parent = tree_base(transform).xz();
        let rng = &mut rng.0;

        for _ in 0..SEED_ATTEMPTS {
            let angle = rng.random_range(0.0..std::f32::consts::TAU);
            let spot = parent + Vec2::from_angle(angle) * rng.random_range(SEED_DISTANCE);
            if spot_is_free(spot, occupied.iter().copied()) {
                debug!("sapling dropped at {spot} ({} trees)", tree_count + 1);
                spawn_tree(&mut commands, &assets, spot, Maturity(0.0));
                occupied.push(spot);
                tree_count += 1;
                break;
            }
        }
    }
}

/// Tips felled trees over around their base until they lie flat, then marks them `Fallen`.
fn animate_falling_trees(time: Res<Time>, mut trees: Query<(&mut TreeState, &mut Transform)>) {
    for (mut state, mut transform) in &mut trees {
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
        let scale = transform.scale.y;

        let axis = Vec3::Y.cross(dir).normalize();
        let rotation = Quat::from_axis_angle(axis, eased * std::f32::consts::FRAC_PI_2);
        // Pivot around the base, lifting it by the trunk radius as it comes to rest on its side.
        transform.rotation = rotation;
        transform.translation =
            base + Vec3::Y * (TRUNK_RADIUS * scale * eased) - rotation * (BASE_OFFSET * scale);

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

/// Keeps each tree's rigid body in step with its state: static while it's an obstacle, kinematic
/// while the fall animation drives it, dynamic (held by a joint) while being dragged.
fn sync_tree_bodies(
    mut commands: Commands,
    trees: Query<(Entity, &TreeState, &RigidBody), Changed<TreeState>>,
) {
    for (entity, state, body) in &trees {
        let wanted = match state {
            TreeState::Falling { .. } => RigidBody::Kinematic,
            TreeState::Carried(_) => RigidBody::Dynamic,
            TreeState::Standing { .. } | TreeState::Fallen | TreeState::Delivered => {
                RigidBody::Static
            }
        };
        if *body == wanted {
            continue;
        }

        let mut tree = commands.entity(entity);
        tree.insert((wanted, LinearVelocity::ZERO, AngularVelocity::ZERO));
        match wanted {
            RigidBody::Dynamic => {
                tree.insert((
                    carried_layers(),
                    PLANE_LOCK,
                    Mass(5.0),
                    LinearDamping(2.0),
                    AngularDamping(4.0),
                ));
            }
            _ => {
                tree.insert(obstacle_layers());
            }
        }
    }
}
