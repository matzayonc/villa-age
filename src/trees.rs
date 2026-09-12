//! Placeholder trees scattered over the map.

use bevy::prelude::*;

use crate::map::MAP_SIZE;

/// Marker for tree entities. The entity's translation is the tree's base on the ground.
#[derive(Component)]
pub struct Tree;

pub struct TreesPlugin;

impl Plugin for TreesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_trees);
    }
}

const TREE_COUNT: u32 = 24;
pub const TRUNK_RADIUS: f32 = 0.25;
const TRUNK_HEIGHT: f32 = 1.2;
const CANOPY_RADIUS: f32 = 1.1;
const CANOPY_HEIGHT: f32 = 2.4;

fn spawn_trees(
    mut commands: Commands,
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

    let mut rng = Lcg(0x5EED_1234);
    let half = MAP_SIZE / 2.0 - 2.0;

    for _ in 0..TREE_COUNT {
        let x = rng.next_f32() * 2.0 * half - half;
        let z = rng.next_f32() * 2.0 * half - half;
        // Keep the middle clear so characters don't start inside a tree.
        if x.abs() < 3.0 && z.abs() < 3.0 {
            continue;
        }

        // To use a real model, replace the children with `SceneRoot(asset_server.load("tree.glb#Scene0"))`.
        commands.spawn((Tree, Transform::from_xyz(x, 0.0, z), Visibility::default())).with_children(|parent| {
            parent.spawn((
                Mesh3d(trunk_mesh.clone()),
                MeshMaterial3d(trunk_material.clone()),
                Transform::from_xyz(0.0, TRUNK_HEIGHT / 2.0, 0.0),
            ));
            parent.spawn((
                Mesh3d(canopy_mesh.clone()),
                MeshMaterial3d(canopy_material.clone()),
                Transform::from_xyz(0.0, TRUNK_HEIGHT + CANOPY_HEIGHT / 2.0, 0.0),
            ));
        });
    }
}

/// Tiny deterministic PRNG so tree placement is stable across runs without pulling in `rand`.
struct Lcg(u64);

impl Lcg {
    fn next_f32(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 40) as f32) / ((1u64 << 24) as f32)
    }
}
