//! Carrots: food for rabbits. They sprout at random spots on the map at random times, up to a
//! cap, and sit there until a rabbit eats one. Nothing bumps into them: a carrot is just a
//! marker at a spot on the ground.

use avian3d::prelude::*;
use bevy::prelude::*;
use rand::RngExt;

use crate::GameRng;
use crate::entities::trees::{Maturity, Tree, tree_base};
use crate::map::MapConfig;
use crate::sim::SimSet;

/// Marker for carrots. The entity sits on the ground at the carrot's spot.
#[derive(Component)]
pub struct Carrot;

/// When the next carrot sprouts, in physics (fixed-clock) seconds.
#[derive(Resource)]
struct NextCarrot(f32);

/// Seconds between carrots sprouting, rolled per carrot, per [`crate::map::TUNING_AREA`]: a
/// bigger map sprouts proportionally more often.
const SPROUT_INTERVAL: std::ops::RangeInclusive<f32> = 6.0..=14.0;
/// The map won't hold more carrots than this, per [`crate::map::TUNING_AREA`] (scaled by area).
pub const MAX_CARROTS: usize = 30;
/// Random spots tried per carrot before giving up until the next interval.
const SPROUT_ATTEMPTS: usize = 6;
/// Carrots don't sprout closer than this to the map edge, or to a tree's base.
const EDGE_MARGIN: f32 = 1.5;
const TREE_CLEARANCE: f32 = 0.8;

pub struct CarrotsPlugin;

impl Plugin for CarrotsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, schedule_first_carrot)
            .add_systems(FixedUpdate, sprout_carrots.in_set(SimSet::Carrots));
    }
}

fn schedule_first_carrot(mut commands: Commands, map: Res<MapConfig>, mut rng: ResMut<GameRng>) {
    let interval = map.scale_interval(rng.0.random_range(SPROUT_INTERVAL));
    commands.insert_resource(NextCarrot(interval));
}

fn spawn_carrot(commands: &mut Commands, spot: Vec2) {
    commands.spawn((
        Carrot,
        Name::new("Carrot"),
        Transform::from_xyz(spot.x, 0.0, spot.y),
    ));
}

/// Whether a carrot can sprout at `spot`: on the map and clear of every tree base in `trees`.
fn spot_is_free(map: &MapConfig, spot: Vec2, trees: impl IntoIterator<Item = Vec2>) -> bool {
    let half = map.half_extent() - EDGE_MARGIN;
    spot.abs().max_element() <= half
        && trees
            .into_iter()
            .all(|p| p.distance_squared(spot) >= TREE_CLEARANCE * TREE_CLEARANCE)
}

/// Every so often a carrot sprouts at a random free spot, unless the map is full of them.
fn sprout_carrots(
    mut commands: Commands,
    time: Res<Time>,
    map: Res<MapConfig>,
    mut rng: ResMut<GameRng>,
    mut next: ResMut<NextCarrot>,
    carrots: Query<(), With<Carrot>>,
    trees: Query<(&Position, &Rotation, &Maturity), With<Tree>>,
) {
    let now = time.elapsed_secs();
    if now < next.0 {
        return;
    }
    let rng = &mut rng.0;
    next.0 = now + map.scale_interval(rng.random_range(SPROUT_INTERVAL));
    let count = carrots.iter().len();
    if count >= map.scale_count(MAX_CARROTS) {
        return;
    }

    let half = map.half_extent() - EDGE_MARGIN;
    let bases: Vec<Vec2> = trees
        .iter()
        .map(|(p, r, &m)| tree_base(p, r, m).xz())
        .collect();
    for _ in 0..SPROUT_ATTEMPTS {
        let spot = Vec2::new(
            rng.random_range(-half..=half),
            rng.random_range(-half..=half),
        );
        if spot_is_free(&map, spot, bases.iter().copied()) {
            debug!("carrot sprouted at {spot} ({} carrots)", count + 1);
            spawn_carrot(&mut commands, spot);
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spot_is_free_respects_edge_and_trees() {
        let map = MapConfig::from_ron("(size: 20.0, villagers: [], trees: [])").unwrap();
        let tree = Vec2::new(2.0, 2.0);
        assert!(spot_is_free(&map, Vec2::ZERO, [tree]));
        assert!(!spot_is_free(&map, tree + Vec2::X * 0.5, [tree]));
        assert!(!spot_is_free(&map, Vec2::new(9.0, 0.0), [tree]));
        assert!(spot_is_free(&map, Vec2::new(8.5, 0.0), [tree]));
    }
}
