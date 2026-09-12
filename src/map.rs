//! The flat 2D map: a textured ground quad lying in the XZ plane, plus scene lighting.

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageSampler};
use bevy::light::GlobalAmbientLight;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// Side length of the map in world units.
pub const MAP_SIZE: f32 = 40.0;

/// Marker for the ground plane entity.
#[derive(Component)]
pub struct Ground;

pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (spawn_map, spawn_lights));
    }
}

fn spawn_map(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    // Placeholder map texture. To use a real map image instead, drop it into `assets/`
    // and replace this with: `asset_server.load("map.png")` (add `asset_server: Res<AssetServer>`).
    let texture = images.add(checkerboard_image(512, 16));

    commands.spawn((
        Ground,
        Mesh3d(meshes.add(Plane3d::default().mesh().size(MAP_SIZE, MAP_SIZE))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(texture),
            perceptual_roughness: 1.0,
            ..default()
        })),
    ));
}

fn spawn_lights(mut commands: Commands, mut ambient: ResMut<GlobalAmbientLight>) {
    ambient.brightness = 300.0;

    commands.spawn((
        DirectionalLight {
            illuminance: 8_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, 0.6, -1.0, 0.0)),
    ));
}

/// Builds a `size`×`size` RGBA texture split into `cells`×`cells` alternating green squares.
fn checkerboard_image(size: u32, cells: u32) -> Image {
    const LIGHT: [u8; 4] = [116, 168, 84, 255];
    const DARK: [u8; 4] = [96, 144, 68, 255];

    let cell_px = size / cells;
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let odd = ((x / cell_px) + (y / cell_px)) % 2 == 1;
            data.extend_from_slice(if odd { &DARK } else { &LIGHT });
        }
    }

    let mut image = Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    // Nearest filtering keeps the tile edges crisp instead of blurring them.
    image.sampler = ImageSampler::nearest();
    image
}
