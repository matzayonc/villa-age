//! Rabbit visuals: a capsule body with two ears. The meshes are children of the physics body so
//! they can arc through the air over a hop while the body (locked to the ground plane) stays
//! put.

use bevy::prelude::*;

use crate::entities::rabbits::{self, HOP_DURATION, Hopping, Rabbit};

/// A piece of the rabbit's visual. Holds the piece's height when the rabbit is sitting.
#[derive(Component)]
struct RabbitMesh {
    rest_y: f32,
}

/// Shared meshes and materials for rabbit visuals.
#[derive(Resource)]
struct RabbitAssets {
    body: Handle<Mesh>,
    ear: Handle<Mesh>,
    fur: Handle<StandardMaterial>,
}

/// Peak height of the visual arc over a hop.
const HOP_HEIGHT: f32 = 0.35;
const EAR_RADIUS: f32 = 0.03;
const EAR_HEIGHT: f32 = 0.16;

pub struct RabbitVisualsPlugin;

impl Plugin for RabbitVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, load_rabbit_assets)
            .add_observer(attach_rabbit_meshes)
            .add_systems(Update, animate_rabbit_meshes);
    }
}

fn load_rabbit_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(RabbitAssets {
        body: meshes.add(Capsule3d::new(rabbits::RADIUS, rabbits::HEIGHT)),
        ear: meshes.add(Cylinder::new(EAR_RADIUS, EAR_HEIGHT)),
        fur: materials.add(StandardMaterial {
            base_color: Color::srgb(0.85, 0.8, 0.75),
            perceptual_roughness: 1.0,
            ..default()
        }),
    });
}

fn attach_rabbit_meshes(add: On<Add, Rabbit>, mut commands: Commands, assets: Res<RabbitAssets>) {
    commands
        .entity(add.entity)
        .insert(Visibility::default())
        .with_children(|parent| {
            parent.spawn((
                RabbitMesh { rest_y: 0.0 },
                Mesh3d(assets.body.clone()),
                MeshMaterial3d(assets.fur.clone()),
                // The capsule lies along the hop direction.
                Transform::from_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
            ));
            let ear_y = rabbits::RADIUS + EAR_HEIGHT / 2.0;
            for side in [-1.0, 1.0] {
                parent.spawn((
                    RabbitMesh { rest_y: ear_y },
                    Mesh3d(assets.ear.clone()),
                    MeshMaterial3d(assets.fur.clone()),
                    Transform::from_xyz(side * 0.05, ear_y, -rabbits::HEIGHT / 2.0),
                ));
            }
        });
}

/// Per-frame: the mesh rises and falls in an arc over a hop.
fn animate_rabbit_meshes(
    time: Res<Time<Fixed>>,
    rabbits: Query<(&Hopping, &Children), With<Rabbit>>,
    mut meshes: Query<(&mut Transform, &RabbitMesh)>,
) {
    // Where the fixed clock is between steps, so the arc is smooth at any frame rate.
    let now = time.elapsed_secs() + time.overstep().as_secs_f32();
    for (hopping, children) in &rabbits {
        let lift = match *hopping {
            Hopping::Hop { started, .. } => {
                let progress = ((now - started) / HOP_DURATION).clamp(0.0, 1.0);
                (progress * std::f32::consts::PI).sin() * HOP_HEIGHT
            }
            Hopping::Resting { .. } => 0.0,
        };
        for &child in children {
            if let Ok((mut transform, mesh)) = meshes.get_mut(child) {
                transform.translation.y = mesh.rest_y + lift;
            }
        }
    }
}
