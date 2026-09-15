//! Carrot visuals: an orange root mostly buried, with a tuft of green on top.

use bevy::prelude::*;

use crate::entities::carrots::Carrot;

/// Shared meshes and materials for carrot visuals.
#[derive(Resource)]
struct CarrotAssets {
    root: Handle<Mesh>,
    leaves: Handle<Mesh>,
    orange: Handle<StandardMaterial>,
    green: Handle<StandardMaterial>,
}

const ROOT_RADIUS: f32 = 0.07;
const ROOT_LENGTH: f32 = 0.3;
/// How much of the root shows above the ground.
const ROOT_SHOWING: f32 = 0.06;
const LEAVES_RADIUS: f32 = 0.09;

pub struct CarrotVisualsPlugin;

impl Plugin for CarrotVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, load_carrot_assets)
            .add_observer(attach_carrot_meshes);
    }
}

fn load_carrot_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(CarrotAssets {
        root: meshes.add(Cone::new(ROOT_RADIUS, ROOT_LENGTH)),
        leaves: meshes.add(Sphere::new(LEAVES_RADIUS)),
        orange: materials.add(StandardMaterial {
            base_color: Color::srgb(0.95, 0.5, 0.1),
            perceptual_roughness: 0.9,
            ..default()
        }),
        green: materials.add(StandardMaterial {
            base_color: Color::srgb(0.3, 0.65, 0.2),
            perceptual_roughness: 1.0,
            ..default()
        }),
    });
}

fn attach_carrot_meshes(add: On<Add, Carrot>, mut commands: Commands, assets: Res<CarrotAssets>) {
    commands
        .entity(add.entity)
        .insert(Visibility::default())
        .with_children(|parent| {
            // The cone's apex points up; flipped, it's a root with only its top showing.
            parent.spawn((
                Mesh3d(assets.root.clone()),
                MeshMaterial3d(assets.orange.clone()),
                Transform::from_xyz(0.0, ROOT_SHOWING - ROOT_LENGTH / 2.0, 0.0)
                    .with_rotation(Quat::from_rotation_x(std::f32::consts::PI)),
            ));
            parent.spawn((
                Mesh3d(assets.leaves.clone()),
                MeshMaterial3d(assets.green.clone()),
                Transform::from_xyz(0.0, ROOT_SHOWING + LEAVES_RADIUS * 0.6, 0.0)
                    .with_scale(Vec3::new(1.0, 0.7, 1.0)),
            ));
        });
}
