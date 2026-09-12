use bevy::prelude::*;

mod camera;
mod characters;
mod map;
mod trees;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Villa Age".into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins((
            map::MapPlugin,
            trees::TreesPlugin,
            characters::CharactersPlugin,
            camera::CameraPlugin,
        ))
        .run();
}
