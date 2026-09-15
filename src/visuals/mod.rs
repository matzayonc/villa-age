//! What everything looks like. Windowed-only: gameplay spawns bare entities (a marker, a
//! physics body, a transform) and this layer attaches meshes to them, via observers on their
//! marker components, and animates those meshes per frame from gameplay state.
//!
//! The dependency runs one way: visuals read `entities`, never the reverse. Nothing here may
//! draw from `GameRng` or touch a physics pose, so a seed reproduces the same run with or
//! without a window.

use bevy::prelude::*;

pub mod carrots;
pub mod ground;
pub mod rabbits;
pub mod trees;
pub mod villagers;

pub struct VisualsPlugin;

impl Plugin for VisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            ground::GroundPlugin,
            trees::TreeVisualsPlugin,
            villagers::VillagerVisualsPlugin,
            rabbits::RabbitVisualsPlugin,
            carrots::CarrotVisualsPlugin,
        ));
    }
}
