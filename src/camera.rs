//! Orbit camera driven by the mouse.
//!
//! - Right-drag: pan (grabs the map point under the cursor and drags it along)
//! - Middle-drag: orbit (yaw + pitch)
//! - Scroll: zoom (ignored over the UI)
//!
//! The left button is left free for interacting with objects and windows (see `history`).

use bevy::ecs::system::SystemParam;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerId;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::map::MapConfig;
use crate::sim::SimSet;

const PAN_BUTTON: MouseButton = MouseButton::Right;
const ORBIT_BUTTON: MouseButton = MouseButton::Middle;

/// Radians of rotation per pixel of mouse movement.
const ORBIT_SENSITIVITY: f32 = 0.005;
/// Fraction of the distance zoomed per scroll line / per scroll pixel.
const ZOOM_LINE_STEP: f32 = 0.1;
const ZOOM_PIXEL_STEP: f32 = 0.005;

const MIN_DISTANCE: f32 = 4.0;
/// Farthest the camera zooms out, as a multiple of the map's side.
const MAX_DISTANCE_PER_SIZE: f32 = 2.0;
/// Pitch limits keep the camera above the map and away from the straight-down gimbal lock.
const MIN_PITCH: f32 = 0.15;
const MAX_PITCH: f32 = 1.5;

/// Camera state; the actual `Transform` is derived from this every frame.
#[derive(Component)]
pub struct OrbitCamera {
    /// Point on the ground the camera looks at and orbits around.
    pub focus: Vec3,
    /// Rotation around the Y axis, in radians.
    pub yaw: f32,
    /// Angle above the ground plane, in radians.
    pub pitch: f32,
    /// Distance from `focus` to the camera.
    pub distance: f32,
    /// Ground point grabbed at the start of a pan drag.
    grab: Option<Vec3>,
}

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_camera).add_systems(
            Update,
            (zoom, orbit, pan, sync_transform)
                .chain()
                .in_set(SimSet::Camera),
        );
    }
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        OrbitCamera {
            focus: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.9,
            distance: 25.0,
            grab: None,
        },
    ));
}

fn zoom(
    scroll: Res<AccumulatedMouseScroll>,
    ui: UiHover,
    map: Res<MapConfig>,
    mut camera: Single<&mut OrbitCamera>,
) {
    if scroll.delta.y == 0.0 || ui.over_ui() {
        return;
    }
    let step = match scroll.unit {
        MouseScrollUnit::Line => ZOOM_LINE_STEP,
        MouseScrollUnit::Pixel => ZOOM_PIXEL_STEP,
    };
    let factor = (1.0 - scroll.delta.y * step).clamp(0.5, 2.0);
    let max_distance = map.size * MAX_DISTANCE_PER_SIZE;
    camera.distance = (camera.distance * factor).clamp(MIN_DISTANCE, max_distance);
}

fn orbit(
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    mut camera: Single<&mut OrbitCamera>,
) {
    if !buttons.pressed(ORBIT_BUTTON) || motion.delta == Vec2::ZERO {
        return;
    }
    camera.yaw -= motion.delta.x * ORBIT_SENSITIVITY;
    camera.pitch = (camera.pitch + motion.delta.y * ORBIT_SENSITIVITY).clamp(MIN_PITCH, MAX_PITCH);
}

fn pan(
    buttons: Res<ButtonInput<MouseButton>>,
    map: Res<MapConfig>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform, &mut OrbitCamera)>,
) {
    let (cam, cam_transform, mut orbit) = camera.into_inner();

    if buttons.just_released(PAN_BUTTON) {
        orbit.grab = None;
        return;
    }
    if !buttons.pressed(PAN_BUTTON) {
        return;
    }

    let Some(hit) = cursor_ground_point(&window, cam, cam_transform) else {
        return;
    };

    match orbit.grab {
        None => orbit.grab = Some(hit),
        Some(grab) => {
            // Shift the focus so the grabbed point lands back under the cursor.
            let half = map.half_extent();
            let focus = orbit.focus + (grab - hit);
            orbit.focus = Vec3::new(focus.x.clamp(-half, half), 0.0, focus.z.clamp(-half, half));
        }
    }
}

/// Whether the mouse is over a UI node.
#[derive(SystemParam)]
pub struct UiHover<'w, 's> {
    hover: Res<'w, HoverMap>,
    nodes: Query<'w, 's, (), With<Node>>,
}

impl UiHover<'_, '_> {
    pub fn over_ui(&self) -> bool {
        // The picking window backend always lists the window itself as a hit, so check for
        // actual UI nodes rather than for any hit.
        self.hover
            .get(&PointerId::Mouse)
            .is_some_and(|hits| hits.keys().any(|&entity| self.nodes.contains(entity)))
    }
}

/// The view ray under the cursor, if the cursor is inside the window.
pub fn cursor_ray(window: &Window, camera: &Camera, transform: &GlobalTransform) -> Option<Ray3d> {
    let cursor = window.cursor_position()?;
    camera.viewport_to_world(transform, cursor).ok()
}

/// Where the cursor's view ray hits the ground plane (y = 0), if it does.
fn cursor_ground_point(
    window: &Window,
    camera: &Camera,
    transform: &GlobalTransform,
) -> Option<Vec3> {
    cursor_ray(window, camera, transform)?
        .plane_intersection_point(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y))
}

fn sync_transform(camera: Single<(&OrbitCamera, &mut Transform)>) {
    let (orbit, mut transform) = camera.into_inner();
    let rotation = Quat::from_euler(EulerRot::YXZ, orbit.yaw, -orbit.pitch, 0.0);
    transform.translation = orbit.focus + rotation * (Vec3::Z * orbit.distance);
    transform.look_at(orbit.focus, Vec3::Y);
}
