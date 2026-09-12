//! Placeholder trees scattered over the map, their fall animation, and their physics bodies.

use avian3d::prelude::*;
use bevy::prelude::*;
use rand::{RngExt, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::WorldSeed;
use crate::map::MAP_SIZE;
use crate::physics::{PLANE_LOCK, carried_layers, obstacle_layers};

/// Marker for tree entities. The entity's origin is the tree's center (halfway from base to tip),
/// so a single capsule collider covers it both standing and lying down. Use [`tree_base`] for the
/// point where it touches the ground.
#[derive(Component)]
#[require(TreeState)]
pub struct Tree;

#[derive(Component)]
pub enum TreeState {
    /// Upright; `health` is the remaining chopping work in seconds of one character chopping.
    Standing { health: f32 },
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
        Self::Standing {
            health: TREE_HEALTH,
        }
    }
}

pub struct TreesPlugin;

impl Plugin for TreesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_trees)
            .add_systems(Update, (animate_falling_trees, sync_tree_bodies).chain());
    }
}

const TREE_COUNT: u32 = 24;
const TREE_HEALTH: f32 = 4.0;
/// Seconds it takes a felled tree to hit the ground.
const FALL_DURATION: f32 = 1.2;
pub const TRUNK_RADIUS: f32 = 0.25;
const TRUNK_HEIGHT: f32 = 1.2;
const CANOPY_RADIUS: f32 = 1.1;
const CANOPY_HEIGHT: f32 = 2.4;
/// Base-to-tip length of a tree.
pub const TREE_LENGTH: f32 = TRUNK_HEIGHT + CANOPY_HEIGHT;
/// Offset from the tree's origin (its center) to its base, in local space.
pub const BASE_OFFSET: Vec3 = Vec3::new(0.0, -TREE_LENGTH / 2.0, 0.0);

/// Where the tree touches the ground, in world space.
pub fn tree_base(transform: &Transform) -> Vec3 {
    transform.translation + transform.rotation * BASE_OFFSET
}

fn spawn_trees(
    mut commands: Commands,
    seed: Res<WorldSeed>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let trunk_mesh = meshes.add(Cylinder::new(TRUNK_RADIUS, TRUNK_HEIGHT));
    let canopy_mesh = meshes.add(Cone::new(CANOPY_RADIUS, CANOPY_HEIGHT));
    let trunk_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.45, 0.3, 0.15),
        perceptual_roughness: 1.0,
        ..default()
    });
    let canopy_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.15, 0.5, 0.2),
        perceptual_roughness: 0.9,
        ..default()
    });

    let mut rng = ChaCha8Rng::seed_from_u64(seed.0);
    let half = MAP_SIZE / 2.0 - 2.0;

    for _ in 0..TREE_COUNT {
        let x = rng.random_range(-half..=half);
        let z = rng.random_range(-half..=half);
        // Keep the middle clear so characters don't start inside a tree.
        if x.abs() < 3.0 && z.abs() < 3.0 {
            continue;
        }

        // To use a real model, replace the children with `SceneRoot(asset_server.load("tree.glb#Scene0"))`.
        commands
            .spawn((
                Tree,
                Transform::from_xyz(x, TREE_LENGTH / 2.0, z),
                Visibility::default(),
                RigidBody::Static,
                Collider::capsule(TRUNK_RADIUS, TREE_LENGTH - 2.0 * TRUNK_RADIUS),
                obstacle_layers(),
            ))
            .with_children(|parent| {
                parent.spawn((
                    Mesh3d(trunk_mesh.clone()),
                    MeshMaterial3d(trunk_material.clone()),
                    Transform::from_translation(BASE_OFFSET + Vec3::Y * (TRUNK_HEIGHT / 2.0)),
                ));
                parent.spawn((
                    Mesh3d(canopy_mesh.clone()),
                    MeshMaterial3d(canopy_material.clone()),
                    Transform::from_translation(
                        BASE_OFFSET + Vec3::Y * (TRUNK_HEIGHT + CANOPY_HEIGHT / 2.0),
                    ),
                ));
            });
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

        let axis = Vec3::Y.cross(dir).normalize();
        let rotation = Quat::from_axis_angle(axis, eased * std::f32::consts::FRAC_PI_2);
        // Pivot around the base, lifting it by the trunk radius as it comes to rest on its side.
        transform.rotation = rotation;
        transform.translation = base + Vec3::Y * (TRUNK_RADIUS * eased) - rotation * BASE_OFFSET;

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
