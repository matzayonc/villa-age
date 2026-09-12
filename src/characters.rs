//! Placeholder 3D characters standing on the map.

use bevy::prelude::*;

use crate::trees::{TRUNK_RADIUS, Tree};

/// Marker for character entities.
#[derive(Component)]
pub struct Character;

/// World units per second.
const MOVE_SPEED: f32 = 3.0;
/// How close (in XZ) a character gets to a tree's base before stopping.
const ARRIVE_DISTANCE: f32 = TRUNK_RADIUS + RADIUS + 0.2;

pub struct CharactersPlugin;

impl Plugin for CharactersPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_characters)
            .add_systems(Update, seek_nearest_tree);
    }
}

const RADIUS: f32 = 0.4;
const HEIGHT: f32 = 1.0;

fn spawn_characters(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Capsule stands on the ground when its center is at half-height + radius.
    let y = HEIGHT / 2.0 + RADIUS;
    let mesh = meshes.add(Capsule3d::new(RADIUS, HEIGHT));

    let placements = [
        (Vec2::new(0.0, 0.0), Color::srgb(0.85, 0.25, 0.2)),
        (Vec2::new(4.0, -3.0), Color::srgb(0.2, 0.45, 0.9)),
        (Vec2::new(-5.0, 2.0), Color::srgb(0.95, 0.8, 0.2)),
        (Vec2::new(7.0, 6.0), Color::srgb(0.6, 0.3, 0.8)),
        (Vec2::new(-8.0, -7.0), Color::srgb(0.2, 0.8, 0.75)),
    ];

    for (pos, color) in placements {
        // To use a real model instead of the capsule, replace `Mesh3d`/`MeshMaterial3d` with
        // `SceneRoot(asset_server.load("character.glb#Scene0"))`.
        commands.spawn((
            Character,
            Mesh3d(mesh.clone()),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                ..default()
            })),
            Transform::from_xyz(pos.x, y, pos.y),
        ));
    }
}

/// Walks every character toward the tree closest to it, stopping just short of the trunk.
fn seek_nearest_tree(
    time: Res<Time>,
    trees: Query<&Transform, (With<Tree>, Without<Character>)>,
    mut characters: Query<&mut Transform, With<Character>>,
) {
    for mut transform in &mut characters {
        let pos = transform.translation.xz();
        let Some(target) = trees
            .iter()
            .map(|t| t.translation.xz())
            .min_by(|a, b| a.distance_squared(pos).total_cmp(&b.distance_squared(pos)))
        else {
            continue;
        };

        let to_target = target - pos;
        let distance = to_target.length();
        if distance <= ARRIVE_DISTANCE {
            continue;
        }

        let step = (MOVE_SPEED * time.delta_secs()).min(distance - ARRIVE_DISTANCE);
        let dir = to_target / distance;
        transform.translation += Vec3::new(dir.x, 0.0, dir.y) * step;
        transform.look_to(Vec3::new(dir.x, 0.0, dir.y), Vec3::Y);
    }
}
