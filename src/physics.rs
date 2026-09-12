//! Physics setup: avian3d run as a 2D simulation on the XZ plane.
//!
//! Gravity is off and every body is locked to the plane, so physics only ever resolves
//! horizontal overlap. Heights are set by the gameplay code.

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::RunConfig;

#[derive(PhysicsLayer, Clone, Copy, Debug, Default)]
pub enum Layer {
    #[default]
    Default,
    Character,
    /// Standing trees and logs lying on the ground.
    Obstacle,
    /// A log being dragged: passes through everything, it's held by a joint instead.
    Carried,
}

pub fn character_layers() -> CollisionLayers {
    CollisionLayers::new(Layer::Character, [Layer::Character, Layer::Obstacle])
}

pub fn obstacle_layers() -> CollisionLayers {
    CollisionLayers::new(Layer::Obstacle, [Layer::Character])
}

pub fn carried_layers() -> CollisionLayers {
    CollisionLayers::new(Layer::Carried, LayerMask::NONE)
}

/// Keeps a body on the ground plane: it may move in XZ and spin around Y only.
pub const PLANE_LOCK: LockedAxes = LockedAxes::new()
    .lock_translation_y()
    .lock_rotation_x()
    .lock_rotation_z();

pub struct GamePhysicsPlugin;

impl Plugin for GamePhysicsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(PhysicsPlugins::default())
            .insert_resource(Gravity(Vec3::ZERO));

        // Debug rendering only makes sense with a window.
        let headless = app
            .world()
            .get_resource::<RunConfig>()
            .is_some_and(|c| c.headless);
        if !headless {
            app.add_plugins(PhysicsDebugPlugin)
                .add_systems(Startup, disable_debug_gizmos)
                .add_systems(Update, toggle_debug_gizmos);
        }
    }
}

fn disable_debug_gizmos(mut store: ResMut<GizmoConfigStore>) {
    store.config_mut::<PhysicsGizmos>().0.enabled = false;
}

/// F1 toggles collider/joint debug rendering.
fn toggle_debug_gizmos(keys: Res<ButtonInput<KeyCode>>, mut store: ResMut<GizmoConfigStore>) {
    if keys.just_pressed(KeyCode::F1) {
        let config = store.config_mut::<PhysicsGizmos>().0;
        config.enabled = !config.enabled;
    }
}
