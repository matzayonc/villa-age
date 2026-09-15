//! The map's layout: a square of ground and what initially stands on it, from a RON file (see
//! `assets/maps/default.ron`, made by the `genmap` binary). Drawing the ground is
//! `visuals::ground`'s job.

use std::path::Path;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// The built-in map, compiled in so no file is needed at runtime.
const DEFAULT_MAP: &str = include_str!("../assets/maps/default.ron");

/// Area of the 40×40 map the gameplay numbers were tuned on. Population caps and spawn rates are
/// given for a map this big and scaled by area for others (see [`MapConfig::scale_count`]).
pub const TUNING_AREA: f32 = 40.0 * 40.0;

/// Everything a map file defines: the ground and what initially stands on it.
#[derive(Resource, Serialize, Deserialize, Clone, Debug)]
pub struct MapConfig {
    /// Side length of the square map in world units, centered on the origin.
    pub size: f32,
    /// Ground texture path under `assets/`; a placeholder checkerboard when `None`.
    #[serde(default)]
    pub texture: Option<String>,
    /// Villager spawn points (x, z).
    pub villagers: Vec<(f32, f32)>,
    /// Initial trees.
    pub trees: Vec<TreeSpec>,
    /// Rabbit spawn points (x, z).
    #[serde(default)]
    pub rabbits: Vec<(f32, f32)>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
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

    /// Scales a count or rate given per [`TUNING_AREA`] to this map's area, never below 1.
    pub fn scale_count(&self, per_tuning_area: usize) -> usize {
        let scaled = per_tuning_area as f32 * self.size * self.size / TUNING_AREA;
        (scaled.round() as usize).max(1)
    }

    /// Scales a time between events given per [`TUNING_AREA`] to this map's area: a bigger map
    /// has proportionally more of them, so they come sooner.
    pub fn scale_interval(&self, per_tuning_area: f32) -> f32 {
        per_tuning_area * TUNING_AREA / (self.size * self.size)
    }

    fn validate(&self) -> Result<(), String> {
        if self.size.is_nan() || self.size <= 0.0 {
            return Err(format!("map size must be positive, got {}", self.size));
        }
        let half = self.half_extent();
        let on_map = |(x, z): (f32, f32)| x.abs() <= half && z.abs() <= half;

        for (i, &pos) in self.villagers.iter().enumerate() {
            if !on_map(pos) {
                return Err(format!(
                    "villager {i} at {pos:?} is outside the ±{half} map"
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

#[cfg(test)]
mod tests {
    use super::*;

    fn map(body: &str) -> Result<MapConfig, String> {
        MapConfig::from_ron(&format!("(size: 20.0, {body})"))
    }

    #[test]
    fn default_map_is_valid_and_populated() {
        let map = MapConfig::default();
        assert_eq!(map.size, 400.0);
        assert_eq!(map.half_extent(), 200.0);
        assert!(map.texture.is_none());
        assert!(
            map.villagers.len() >= 100,
            "{} villagers",
            map.villagers.len()
        );
        assert!(map.trees.len() >= 1000, "{} trees", map.trees.len());
        assert!(map.rabbits.len() >= 100, "{} rabbits", map.rabbits.len());
    }

    #[test]
    fn counts_and_intervals_scale_with_area() {
        let tuning = map("villagers: [], trees: []").unwrap();
        // A 20×20 map is a quarter of the tuning area.
        assert_eq!(tuning.scale_count(40), 10);
        assert_eq!(tuning.scale_count(1), 1);
        assert_eq!(tuning.scale_interval(10.0), 40.0);
        let big = MapConfig::from_ron("(size: 400.0, villagers: [], trees: [])").unwrap();
        assert_eq!(big.scale_count(150), 15_000);
        assert_eq!(big.scale_interval(10.0), 0.1);
    }

    #[test]
    fn texture_is_optional_and_parsed_when_given() {
        let none = map("villagers: [], trees: []").unwrap();
        assert_eq!(none.texture, None);

        let some = map(r#"texture: Some("maps/ground.png"), villagers: [], trees: []"#).unwrap();
        assert_eq!(some.texture.as_deref(), Some("maps/ground.png"));
    }

    #[test]
    fn rejects_non_positive_size() {
        for size in ["0.0", "-5.0", "NaN"] {
            let err = MapConfig::from_ron(&format!("(size: {size}, villagers: [], trees: [])"))
                .unwrap_err();
            assert!(err.contains("size must be positive"), "{size}: {err}");
        }
    }

    #[test]
    fn rejects_out_of_bounds_villager() {
        let err = map("villagers: [(0.0, 0.0), (0.0, -10.5)], trees: []").unwrap_err();
        assert!(err.contains("villager 1"), "{err}");
    }

    #[test]
    fn accepts_positions_exactly_on_the_edge() {
        let ok = map("villagers: [(10.0, -10.0)], trees: [(pos: (-10.0, 10.0), maturity: 0.5)]");
        assert!(ok.is_ok(), "{ok:?}");
    }

    #[test]
    fn rejects_maturity_outside_unit_range() {
        for maturity in ["-0.1", "1.5", "NaN"] {
            let err = map(&format!(
                "villagers: [], trees: [(pos: (0.0, 0.0), maturity: {maturity})]"
            ))
            .unwrap_err();
            assert!(err.contains("tree 0 maturity"), "{maturity}: {err}");
        }
    }

    #[test]
    fn reports_syntax_errors() {
        let err = MapConfig::from_ron("(size: 20.0, villagers: [").unwrap_err();
        assert!(err.starts_with("invalid map file:"), "{err}");
    }

    #[test]
    fn load_reads_a_file_and_prefixes_errors_with_its_path() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("villa-age-map-{}.ron", std::process::id()));
        std::fs::write(&path, "(size: 8.0, villagers: [(1.0, 1.0)], trees: [])").unwrap();
        let map = MapConfig::load(&path).unwrap();
        assert_eq!(map.size, 8.0);
        assert_eq!(map.villagers, [(1.0, 1.0)]);

        std::fs::write(&path, "(size: 8.0, villagers: [(9.0, 0.0)], trees: [])").unwrap();
        let err = MapConfig::load(&path).unwrap_err();
        assert!(err.starts_with(&path.display().to_string()), "{err}");
        assert!(err.contains("villager 0"), "{err}");
        std::fs::remove_file(&path).unwrap();

        let missing = dir.join("villa-age-does-not-exist.ron");
        let err = MapConfig::load(&missing).unwrap_err();
        assert!(err.starts_with("can't read map"), "{err}");
        assert!(err.contains(&missing.display().to_string()), "{err}");
    }
}
