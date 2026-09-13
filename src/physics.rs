//! Physics setup: avian3d run as a 2D simulation on the XZ plane.
//!
//! Gravity is off and every body is locked to the plane, so physics only ever resolves
//! horizontal overlap. Heights are set by the gameplay code.

use avian3d::dynamics::solver::joint_graph::JointGraphPlugin;
use avian3d::physics_transform::{PhysicsTransformConfig, PhysicsTransformSystems};
use avian3d::prelude::*;
use avian3d::schedule::LastPhysicsTick;
use bevy::ecs::change_detection::Tick;
use bevy::ecs::system::SystemChangeTick;
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
}

pub fn character_layers() -> CollisionLayers {
    CollisionLayers::new(Layer::Character, [Layer::Character, Layer::Obstacle])
}

pub fn obstacle_layers() -> CollisionLayers {
    CollisionLayers::new(Layer::Obstacle, [Layer::Character])
}

pub fn log_layers() -> CollisionLayers {
    CollisionLayers::new(Layer::Log, LayerMask::NONE)
}

pub fn carried_layers() -> CollisionLayers {
    CollisionLayers::new(Layer::Carried, LayerMask::NONE)
}

/// What a walking character looks ahead for and swerves around: everything it can't walk over.
pub fn steer_mask() -> LayerMask {
    let mut mask = LayerMask::ALL;
    mask.remove(Layer::Log);
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
            // Nothing is rendered between physics steps; gameplay measures distances on
            // `Transform`, so interpolating it would also break arrival checks.
            .disable::<PhysicsInterpolationPlugin>()
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
            plugins = plugins.disable::<IslandPlugin>();
        }
        app.add_plugins(plugins)
            .insert_resource(Gravity(Vec3::ZERO))
            // Contacts here are simple push-outs plus a slack rope; the default 6 substeps are
            // sized for stacks and stiff joints. Part of what a run means: changing it changes runs.
            .insert_resource(SubstepCount(2))
            .insert_resource(PhysicsTransformConfig {
                // No colliders are children of other entities, so physics doesn't need its own
                // transform propagation pass before each step.
                propagate_before_physics: false,
                // Replaced by `transform_to_position` below.
                transform_to_position: false,
                ..default()
            })
            .add_systems(
                FixedPostUpdate,
                transform_to_position.in_set(PhysicsTransformSystems::TransformToPosition),
            );

        // Debug rendering only makes sense with a window.
        if !is_headless(app) {
            app.add_plugins(PhysicsDebugPlugin)
                .add_systems(Startup, disable_debug_gizmos)
                .add_systems(Update, toggle_debug_gizmos);
        }
    }
}

/// Copies `Transform` changes into the physics `Position`/`Rotation` before each step, so
/// gameplay can move bodies through their transform.
///
/// This is avian's own `transform_to_position` with one difference: it only visits bodies whose
/// `GlobalTransform` changed. Avian's version walks every body every step to find the ones that
/// moved, which with thousands of static trees is the single most expensive thing physics does.
///
/// With avian's version switched off, avian no longer derives a new body's `Position` from its
/// `Transform` at spawn (it zeroes it instead), so spawn code must include an explicit `Position`.
/// A spawn-time sync here wouldn't do: the first frame runs no physics step, yet its gameplay
/// already queries the physics world.
fn transform_to_position(
    mut bodies: Query<(&GlobalTransform, &mut Position, &mut Rotation), Changed<GlobalTransform>>,
    length_unit: Res<PhysicsLengthUnit>,
    last_physics_tick: Res<LastPhysicsTick>,
    system_tick: SystemChangeTick,
) {
    // Before the first physics step the last tick is 0; keep the system tick above it so change
    // detection stays well-formed.
    let this_run = if last_physics_tick.0.get() == 0 {
        Tick::new(1)
    } else {
        system_tick.this_run()
    };
    // Differences below 0.01 mm / 0.1° are noise from the round trip, not a move.
    let distance_tolerance = length_unit.0 * 1e-5;
    let rotation_tolerance = 0.1f32.to_radians();

    // A body whose `Position`/`Rotation` was written directly since the last step keeps that
    // value; the transform is stale in that case.
    let written_since_last_step = |changed: bool, last_changed: Tick| {
        changed && last_changed.is_newer_than(last_physics_tick.0, this_run)
    };

    for (global_transform, mut position, mut rotation) in &mut bodies {
        let transform = global_transform.compute_transform();

        let position_written = !position.is_added()
            && written_since_last_step(position.is_changed(), position.last_changed());
        if !position_written
            && (position.0 - transform.translation).abs().max_element() > distance_tolerance
        {
            position.0 = transform.translation;
        }

        let rotation_written = !rotation.is_added()
            && written_since_last_step(rotation.is_changed(), rotation.last_changed());
        if !rotation_written
            && rotation
                .angle_between(Rotation::from(transform.rotation))
                .abs()
                > rotation_tolerance
        {
            *rotation = Rotation::from(transform.rotation);
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
    fn steering_ignores_logs_only() {
        let mask = steer_mask();
        assert!(!mask.has_all(Layer::Log));
        assert!(mask.has_all(Layer::Obstacle));
        assert!(mask.has_all(Layer::Character));
        assert!(mask.has_all(Layer::Carried));
    }
}
