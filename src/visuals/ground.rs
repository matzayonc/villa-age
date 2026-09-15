//! The ground: a textured quad the size of the map, lying in the XZ plane, plus scene lighting.

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageSampler};
use bevy::light::GlobalAmbientLight;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::map::MapConfig;

/// Marker for the ground plane entity.
#[derive(Component)]
pub struct Ground;

pub struct GroundPlugin;

impl Plugin for GroundPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (spawn_ground, spawn_lights));
    }
}

fn spawn_ground(
    mut commands: Commands,
    map: Res<MapConfig>,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let texture = match &map.texture {
        Some(path) => asset_server.load(path.clone()),
        None => images.add(checkerboard_image(512, 16)),
    };

    commands.spawn((
        Ground,
        Mesh3d(meshes.add(Plane3d::default().mesh().size(map.size, map.size))),
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
