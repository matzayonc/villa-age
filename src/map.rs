//! The flat 2D map: a textured ground quad lying in the XZ plane, plus scene lighting. The map's
//! layout comes from a RON file (see `assets/maps/default.ron`).

use std::path::Path;

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageSampler};
use bevy::light::GlobalAmbientLight;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use serde::Deserialize;

use crate::RunConfig;

/// The built-in map, compiled in so no file is needed at runtime.
const DEFAULT_MAP: &str = include_str!("../assets/maps/default.ron");

/// Everything a map file defines: the ground and what initially stands on it.
#[derive(Resource, Deserialize, Clone, Debug)]
pub struct MapConfig {
    /// Side length of the square map in world units, centered on the origin.
    pub size: f32,
    /// Ground texture path under `assets/`; a placeholder checkerboard when `None`.
    #[serde(default)]
    pub texture: Option<String>,
    /// Character spawn points (x, z).
    pub characters: Vec<(f32, f32)>,
    /// Initial trees.
    pub trees: Vec<TreeSpec>,
    /// Rabbit spawn points (x, z).
    #[serde(default)]
    pub rabbits: Vec<(f32, f32)>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct TreeSpec {
    /// Base of the tree (x, z).
    pub pos: (f32, f32),
    /// 0 (fresh sapling) to 1 (fully grown).
    pub maturity: f32,
}

impl MapConfig {
    /// Parses and validates a map from RON text.
    pub fn from_ron(text: &str) -> Result<Self, String> {
        let map: Self = ron::from_str(text).map_err(|e| format!("invalid map file: {e}"))?;
        map.validate()?;
        Ok(map)
    }

    /// Reads and parses a map file.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("can't read map {}: {e}", path.display()))?;
        Self::from_ron(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Distance from the center to an edge.
    pub fn half_extent(&self) -> f32 {
        self.size / 2.0
    }

    fn validate(&self) -> Result<(), String> {
        if self.size.is_nan() || self.size <= 0.0 {
            return Err(format!("map size must be positive, got {}", self.size));
        }
        let half = self.half_extent();
        let on_map = |(x, z): (f32, f32)| x.abs() <= half && z.abs() <= half;

        for (i, &pos) in self.characters.iter().enumerate() {
            if !on_map(pos) {
                return Err(format!(
                    "character {i} at {pos:?} is outside the ±{half} map"
                ));
            }
        }
        for (i, &pos) in self.rabbits.iter().enumerate() {
            if !on_map(pos) {
                return Err(format!("rabbit {i} at {pos:?} is outside the ±{half} map"));
            }
        }
        for (i, tree) in self.trees.iter().enumerate() {
            if !on_map(tree.pos) {
                return Err(format!(
                    "tree {i} at {:?} is outside the ±{half} map",
                    tree.pos
                ));
            }
            if !(0.0..=1.0).contains(&tree.maturity) {
                return Err(format!(
                    "tree {i} maturity {} is not within 0..=1",
                    tree.maturity
                ));
            }
        }
        Ok(())
    }
}

impl Default for MapConfig {
    fn default() -> Self {
        Self::from_ron(DEFAULT_MAP).expect("built-in default map is valid")
    }
}

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
    map: Res<MapConfig>,
    config: Res<RunConfig>,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    // Headless has no image loader (nothing would draw the texture anyway).
    let texture = match &map.texture {
        Some(path) if !config.headless => asset_server.load(path.clone()),
        _ => images.add(checkerboard_image(512, 16)),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn map(body: &str) -> Result<MapConfig, String> {
        MapConfig::from_ron(&format!("(size: 20.0, {body})"))
    }

    #[test]
    fn default_map_is_valid_and_populated() {
        let map = MapConfig::default();
        assert_eq!(map.size, 40.0);
        assert_eq!(map.half_extent(), 20.0);
        assert!(map.texture.is_none());
        assert_eq!(map.characters.len(), 5);
        assert!(map.trees.len() >= 20, "{} trees", map.trees.len());
    }

    #[test]
    fn texture_is_optional_and_parsed_when_given() {
        let none = map("characters: [], trees: []").unwrap();
        assert_eq!(none.texture, None);

        let some = map(r#"texture: Some("maps/ground.png"), characters: [], trees: []"#).unwrap();
        assert_eq!(some.texture.as_deref(), Some("maps/ground.png"));
    }

    #[test]
    fn rejects_non_positive_size() {
        for size in ["0.0", "-5.0", "NaN"] {
            let err = MapConfig::from_ron(&format!("(size: {size}, characters: [], trees: [])"))
                .unwrap_err();
            assert!(err.contains("size must be positive"), "{size}: {err}");
        }
    }

    #[test]
    fn rejects_out_of_bounds_character() {
        let err = map("characters: [(0.0, 0.0), (0.0, -10.5)], trees: []").unwrap_err();
        assert!(err.contains("character 1"), "{err}");
    }

    #[test]
    fn accepts_positions_exactly_on_the_edge() {
        let ok = map("characters: [(10.0, -10.0)], trees: [(pos: (-10.0, 10.0), maturity: 0.5)]");
        assert!(ok.is_ok(), "{ok:?}");
    }

    #[test]
    fn rejects_maturity_outside_unit_range() {
        for maturity in ["-0.1", "1.5", "NaN"] {
            let err = map(&format!(
                "characters: [], trees: [(pos: (0.0, 0.0), maturity: {maturity})]"
            ))
            .unwrap_err();
            assert!(err.contains("tree 0 maturity"), "{maturity}: {err}");
        }
    }

    #[test]
    fn reports_syntax_errors() {
        let err = MapConfig::from_ron("(size: 20.0, characters: [").unwrap_err();
        assert!(err.starts_with("invalid map file:"), "{err}");
    }

    #[test]
    fn load_reads_a_file_and_prefixes_errors_with_its_path() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("villa-age-map-{}.ron", std::process::id()));
        std::fs::write(&path, "(size: 8.0, characters: [(1.0, 1.0)], trees: [])").unwrap();
        let map = MapConfig::load(&path).unwrap();
        assert_eq!(map.size, 8.0);
        assert_eq!(map.characters, [(1.0, 1.0)]);

        std::fs::write(&path, "(size: 8.0, characters: [(9.0, 0.0)], trees: [])").unwrap();
        let err = MapConfig::load(&path).unwrap_err();
        assert!(err.starts_with(&path.display().to_string()), "{err}");
        assert!(err.contains("character 0"), "{err}");
        std::fs::remove_file(&path).unwrap();

        let missing = dir.join("villa-age-does-not-exist.ron");
        let err = MapConfig::load(&missing).unwrap_err();
        assert!(err.starts_with("can't read map"), "{err}");
        assert!(err.contains(&missing.display().to_string()), "{err}");
    }
}
