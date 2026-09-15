//! Tree visuals: a cylinder trunk with a cone canopy, placeholder for a real model.

use bevy::prelude::*;

use crate::entities::trees::{
    BASE_OFFSET, CANOPY_HEIGHT, CANOPY_RADIUS, TRUNK_HEIGHT, TRUNK_RADIUS, Tree,
};

/// Shared meshes and materials for tree visuals.
#[derive(Resource)]
struct TreeAssets {
    trunk_mesh: Handle<Mesh>,
    canopy_mesh: Handle<Mesh>,
    trunk_material: Handle<StandardMaterial>,
    canopy_material: Handle<StandardMaterial>,
}

pub struct TreeVisualsPlugin;

impl Plugin for TreeVisualsPlugin {
    fn build(&self, app: &mut App) {
        // Assets are loaded before `Startup` so the observer finds them when the map's trees
        // are spawned.
        app.add_systems(PreStartup, load_tree_assets)
            .add_observer(attach_tree_meshes);
    }
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

/// Hangs trunk and canopy meshes under a newly spawned tree. The tree entity is uniformly scaled
/// by maturity, so the children are laid out at full size.
fn attach_tree_meshes(add: On<Add, Tree>, mut commands: Commands, assets: Res<TreeAssets>) {
    // To use a real model, replace the children with `SceneRoot(asset_server.load("tree.glb#Scene0"))`.
    commands
        .entity(add.entity)
        .insert(Visibility::default())
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
