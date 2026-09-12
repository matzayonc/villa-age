use bevy::prelude::*;

mod camera;
mod characters;
mod map;
mod physics;
mod trees;

/// Seed for everything procedurally generated (tree placement, etc.).
#[derive(Resource)]
pub struct WorldSeed(pub u64);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Villa Age".into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(WorldSeed(0x5EED_1234))
        .add_plugins((
            physics::GamePhysicsPlugin,
            map::MapPlugin,
            trees::TreesPlugin,
            characters::CharactersPlugin,
            camera::CameraPlugin,
        ))
        .run();
}
