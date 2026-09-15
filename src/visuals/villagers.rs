//! Villager visuals: a colored capsule, placeholder for a real model. It's a child of the
//! physics body so it can rise over a log and swing while chopping without moving the body
//! (which the rope to a hauled log is anchored to).

use bevy::prelude::*;

use crate::entities::trees::TRUNK_RADIUS;
use crate::entities::villagers::{self, Climbing, Villager};
use crate::history::{Action, ActionLog};

/// Marker for the villager's mesh.
#[derive(Component)]
struct VillagerMesh;

/// Shared mesh and the color palette, cycled by spawn order.
#[derive(Resource)]
struct VillagerAssets {
    mesh: Handle<Mesh>,
    materials: [Handle<StandardMaterial>; COLORS.len()],
}

/// Villager colors, cycled by spawn order.
const COLORS: [Color; 5] = [
    Color::srgb(0.85, 0.25, 0.2),
    Color::srgb(0.2, 0.45, 0.9),
    Color::srgb(0.95, 0.8, 0.2),
    Color::srgb(0.6, 0.3, 0.8),
    Color::srgb(0.2, 0.8, 0.75),
];
/// Chop swing animation: swings per second and lean angle in radians.
const SWING_SPEED: f32 = 8.0;
const SWING_ANGLE: f32 = 0.25;
/// How much the visual rises while on top of a log (the log's radius).
const CLIMB_HEIGHT: f32 = TRUNK_RADIUS;

pub struct VillagerVisualsPlugin;

impl Plugin for VillagerVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, load_villager_assets)
            .add_observer(attach_villager_mesh)
            .add_systems(Update, animate_villager_meshes);
    }
}

fn load_villager_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(VillagerAssets {
        mesh: meshes.add(Capsule3d::new(villagers::RADIUS, villagers::HEIGHT)),
        materials: COLORS.map(|color| {
            materials.add(StandardMaterial {
                base_color: color,
                ..default()
            })
        }),
    });
}

/// Hangs a capsule under a newly spawned villager, in the next color of the palette.
fn attach_villager_mesh(
    add: On<Add, Villager>,
    mut commands: Commands,
    assets: Res<VillagerAssets>,
    mut spawned: Local<usize>,
) {
    let material = assets.materials[*spawned % COLORS.len()].clone();
    *spawned += 1;
    commands
        .entity(add.entity)
        .insert(Visibility::default())
        .with_child((
            VillagerMesh,
            // To use a real model instead of the capsule, replace `Mesh3d`/`MeshMaterial3d`
            // with `SceneRoot(asset_server.load("villager.glb#Scene0"))`.
            Mesh3d(assets.mesh.clone()),
            MeshMaterial3d(material),
        ));
}

/// Per-frame: the mesh rises onto a log while climbing and swings in a chopping motion while
/// chopping.
fn animate_villager_meshes(
    time: Res<Time>,
    villagers: Query<(&Climbing, &ActionLog, &Children), With<Villager>>,
    mut meshes: Query<&mut Transform, (With<VillagerMesh>, Without<Villager>)>,
) {
    let swing = (time.elapsed_secs() * SWING_SPEED).sin().max(0.0) * SWING_ANGLE;
    for (climbing, log, children) in &villagers {
        let lift = if climbing.0 { CLIMB_HEIGHT } else { 0.0 };
        let chopping = log
            .current()
            .is_some_and(|entry| matches!(entry.action, Action::Chop { .. }));
        let rotation = if chopping {
            Quat::from_rotation_x(-swing)
        } else {
            Quat::IDENTITY
        };
        for &child in children {
            if let Ok(mut mesh) = meshes.get_mut(child) {
                mesh.translation.y = lift;
                mesh.rotation = rotation;
            }
        }
    }
}
