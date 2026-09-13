//! Physics setup: avian3d run as a 2D simulation on the XZ plane.
//!
//! Gravity is off and every body is locked to the plane, so physics only ever resolves
//! horizontal overlap. Heights are set by the gameplay code.

use avian3d::dynamics::solver::joint_graph::JointGraphPlugin;
use avian3d::physics_transform::PhysicsTransformConfig;
use avian3d::prelude::*;
use bevy::prelude::*;

use crate::sim::is_headless;

#[derive(PhysicsLayer, Clone, Copy, Debug, Default)]
pub enum Layer {
    #[default]
    Default,
    Character,
    /// Standing and falling trees: solid, characters steer around them.
    Obstacle,
    /// Logs lying on the ground: characters climb over them rather than around, so they
    /// collide with nothing and are only ever found by spatial queries.
    Log,
    /// A log being dragged: passes through everything, it's held by a joint instead.
    Carried,
    /// Small animals: bump into trees and people (and get shoved aside), but nobody plans
    /// around them.
    Critter,
}

pub fn character_layers() -> CollisionLayers {
    CollisionLayers::new(
        Layer::Character,
        [Layer::Character, Layer::Obstacle, Layer::Critter],
    )
}

pub fn obstacle_layers() -> CollisionLayers {
    CollisionLayers::new(Layer::Obstacle, [Layer::Character, Layer::Critter])
}

pub fn critter_layers() -> CollisionLayers {
    CollisionLayers::new(
        Layer::Critter,
        [Layer::Critter, Layer::Character, Layer::Obstacle],
    )
}

pub fn log_layers() -> CollisionLayers {
    CollisionLayers::new(Layer::Log, LayerMask::NONE)
}

pub fn carried_layers() -> CollisionLayers {
    CollisionLayers::new(Layer::Carried, LayerMask::NONE)
}

/// What a walking character looks ahead for and swerves around: everything it can't walk over
/// or simply push out of the way.
pub fn steer_mask() -> LayerMask {
    let mut mask = LayerMask::ALL;
    mask.remove(Layer::Log);
    mask.remove(Layer::Critter);
    mask
}

/// Keeps a body on the ground plane: it may move in XZ and spin around Y only.
pub const PLANE_LOCK: LockedAxes = LockedAxes::new()
    .lock_translation_y()
    .lock_rotation_x()
    .lock_rotation_z();

pub struct GamePhysicsPlugin;

impl Plugin for GamePhysicsPlugin {
    fn build(&self, app: &mut App) {
        // Only the parts of avian this game uses. Every plugin left in still runs its systems
        // each step whether or not there is anything for them to do, and that per-step cost is
        // what bounds how fast a headless run can go.
        let mut plugins = PhysicsPlugins::default()
            .build()
            // Nothing moves fast enough to tunnel.
            .disable::<CcdPlugin>()
            // Characters never stand still long enough to sleep.
            .disable::<IslandSleepingPlugin>()
            // Bodies are driven by velocity, never by forces or accelerations.
            .disable::<ForcePlugin>()
            // Every collider sits directly on its body; none are child entities.
            .disable::<ColliderTransformPlugin>()
            // Colliders are primitives, never generated from meshes.
            .disable::<ColliderCachePlugin>()
            // Spatial queries go through the `SpatialQuery` param, not caster components.
            .disable::<SpatialQueryPlugin>()
            // The only joint is the rope (a `DistanceJoint`).
            .disable::<JointGraphPlugin<FixedJoint>>()
            .disable::<JointGraphPlugin<RevoluteJoint>>()
            .disable::<JointGraphPlugin<PrismaticJoint>>()
            .disable::<JointGraphPlugin<SphericalJoint>>();
        if is_headless(app) {
            // Islands only serve sleeping (off) and the debug renderer (windowed only).
            // Interpolation only smooths what is drawn; gameplay never reads the transform.
            plugins = plugins
                .disable::<IslandPlugin>()
                .disable::<PhysicsInterpolationPlugin>();
        }
        app.add_plugins(plugins)
            .insert_resource(Gravity(Vec3::ZERO))
            // Contacts here are simple push-outs plus a slack rope; the default 6 substeps are
            // sized for stacks and stiff joints. Part of what a run means: changing it changes runs.
            .insert_resource(SubstepCount(2))
            .insert_resource(PhysicsTransformConfig {
                // Gameplay reads and writes the physics pose (`Position`/`Rotation`) directly and
                // runs in `FixedUpdate`; the `Transform` is render-only, written from the pose
                // after each step (and smoothed between steps when windowed). So physics never
                // needs to read transforms back, nor propagate them before a step. One
                // consequence: avian no longer derives a spawned body's pose from its transform,
                // so spawn code sets `Position` (and `Rotation`) explicitly.
                propagate_before_physics: false,
                transform_to_position: false,
                ..default()
            });

        // Debug rendering only makes sense with a window.
        if !is_headless(app) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_are_walked_over_and_trees_are_not() {
        assert!(character_layers().interacts_with(obstacle_layers()));
        assert!(character_layers().interacts_with(character_layers()));
        assert!(!character_layers().interacts_with(log_layers()));
        assert!(!character_layers().interacts_with(carried_layers()));
    }

    #[test]
    fn critters_bump_into_trees_and_people() {
        assert!(critter_layers().interacts_with(obstacle_layers()));
        assert!(critter_layers().interacts_with(character_layers()));
        assert!(critter_layers().interacts_with(critter_layers()));
        assert!(!critter_layers().interacts_with(log_layers()));
        assert!(!steer_mask().has_all(Layer::Critter));
    }

    #[test]
    fn steering_ignores_logs_only() {
        let mask = steer_mask();
        assert!(!mask.has_all(Layer::Log));
        assert!(mask.has_all(Layer::Obstacle));
        assert!(mask.has_all(Layer::Character));
        assert!(mask.has_all(Layer::Carried));
    }
}
