//! Per-character timed action history, and windows that show it: a tooltip follows the cursor
//! while a character is hovered; clicking a character opens a pinned window for it (any number
//! can be open at once), draggable by its title bar and closed with its `×`.

use std::collections::VecDeque;
use std::fmt::Write as _;

use avian3d::prelude::*;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::camera::{UiHover, cursor_ray};
use crate::characters::{Speed, Strength};
use crate::physics::Layer;
use crate::sim::SimSet;

/// What a character is doing. Each variant names the tree involved, if any.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    /// No usable tree left.
    Idle,
    /// Heading for a tree to chop or pick up.
    WalkTo {
        tree: Entity,
    },
    Chop {
        tree: Entity,
    },
    /// Felled it, waiting for it to hit the ground.
    AwaitFall {
        tree: Entity,
    },
    /// Dragging the log home.
    Haul {
        tree: Entity,
    },
    /// Dropped the log at home (instantaneous marker).
    Deliver {
        tree: Entity,
    },
    /// The log vanished or someone else ended up with it (instantaneous marker).
    LostLog {
        tree: Entity,
    },
}

/// The start of an action; it lasts until the next entry in the log.
#[derive(Clone, Copy, Debug)]
pub struct Entry {
    /// `Time::elapsed_secs()` when the action started.
    pub at: f32,
    pub action: Action,
}

/// Entries kept per character; the oldest are dropped past this.
const HISTORY_CAPACITY: usize = 64;
/// Entries shown in the history window.
const WINDOW_LINES: usize = 10;
/// How far from the camera a character can still be picked.
const PICK_DISTANCE: f32 = 200.0;
/// Pixels between the cursor and the window's top-left corner while it follows the cursor.
const TOOLTIP_OFFSET: Vec2 = Vec2::new(16.0, 16.0);
/// The tooltip stays above every pinned window.
const TOOLTIP_Z: i32 = i32::MAX;

const PANEL_COLOR: Color = Color::srgba_u8(26, 26, 26, 184);
const BORDER_COLOR: Color = Color::srgba_u8(255, 255, 255, 51);
const TITLE_BAR_COLOR: Color = Color::srgba_u8(255, 255, 255, 20);
const TITLE_COLOR: Color = Color::srgb_u8(230, 230, 230);
const BODY_COLOR: Color = Color::srgb_u8(204, 204, 204);
const CLOSE_COLOR: Color = Color::srgb_u8(179, 179, 179);
const CLOSE_HOVER_COLOR: Color = Color::srgba_u8(255, 255, 255, 38);
const CLOSE_PRESSED_COLOR: Color = Color::srgba_u8(255, 92, 92, 102);

/// Bounded, timestamped log of a character's actions, oldest first. Preallocated: recording never
/// allocates after spawn.
#[derive(Component)]
pub struct ActionLog {
    entries: VecDeque<Entry>,
}

impl Default for ActionLog {
    fn default() -> Self {
        Self {
            entries: VecDeque::with_capacity(HISTORY_CAPACITY),
        }
    }
}

impl ActionLog {
    /// Starts `action` at time `now`, unless it's already the current action. Cheap enough to call
    /// every frame.
    pub fn record(&mut self, now: f32, action: Action) {
        if self.current().is_some_and(|entry| entry.action == action) {
            return;
        }
        if self.entries.len() == HISTORY_CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back(Entry { at: now, action });
    }

    /// The action in progress and when it started.
    pub fn current(&self) -> Option<Entry> {
        self.entries.back().copied()
    }

    /// Entries started at or after `since`, newest first.
    #[allow(dead_code)] // For gameplay/AI systems; nothing queries the log yet.
    pub fn since(&self, since: f32) -> impl Iterator<Item = &Entry> {
        self.entries
            .iter()
            .rev()
            .take_while(move |entry| entry.at >= since)
    }

    /// All entries, oldest first.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Entry> + ExactSizeIterator {
        self.entries.iter()
    }
}

pub struct HistoryPlugin;

impl Plugin for HistoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WindowStack>()
            .add_systems(Startup, spawn_tooltip)
            .add_systems(
                Update,
                (
                    (hover_character, open_window, render_windows).chain(),
                    style_close_button,
                )
                    .in_set(SimSet::History),
            );
    }
}

/// A window showing `character`'s history. Pinned windows are spawned per click; the one with
/// [`Tooltip`] follows the cursor and switches character with the hover.
#[derive(Component)]
struct HistoryWindow {
    character: Option<Entity>,
    /// Text entities inside the window.
    title: Entity,
    body: Entity,
}

/// Marks the single unpinned window that follows the cursor.
#[derive(Component)]
struct Tooltip;

#[derive(Component)]
struct CloseButton;

/// Hands out increasing z-indices so the last window touched is on top.
#[derive(Resource, Default)]
struct WindowStack(i32);

impl WindowStack {
    fn raise(&mut self) -> GlobalZIndex {
        self.0 += 1;
        GlobalZIndex(self.0)
    }
}

/// The primary window's cursor and the camera looking through it.
#[derive(SystemParam)]
struct Cursor<'w, 's> {
    window: Single<'w, 's, &'static Window, With<PrimaryWindow>>,
    camera: Single<'w, 's, (&'static Camera, &'static GlobalTransform)>,
}

impl Cursor<'_, '_> {
    fn position(&self) -> Option<Vec2> {
        self.window.cursor_position()
    }

    fn ray(&self) -> Option<Ray3d> {
        cursor_ray(&self.window, self.camera.0, self.camera.1)
    }
}

fn spawn_tooltip(mut commands: Commands) {
    let tooltip = spawn_window(&mut commands, None, Vec2::ZERO, GlobalZIndex(TOOLTIP_Z));
    commands.entity(tooltip).insert(Tooltip);
}

/// Spawns a hidden history window at `corner` (top-left, in pixels). It gets a close button
/// only when pinned to a character.
fn spawn_window(
    commands: &mut Commands,
    character: Option<Entity>,
    corner: Vec2,
    z: GlobalZIndex,
) -> Entity {
    let window = commands.spawn_empty().id();

    let title = commands
        .spawn((
            Text::default(),
            TextFont::from_font_size(12.0),
            TextColor(TITLE_COLOR),
        ))
        .id();
    let body = commands
        .spawn((
            Text::default(),
            TextFont::from_font_size(11.0),
            TextColor(BODY_COLOR),
            Node {
                padding: UiRect::axes(Val::Px(8.0), Val::Px(6.0)),
                ..default()
            },
        ))
        .id();

    let mut title_bar = commands.spawn((
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            column_gap: Val::Px(12.0),
            padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
            ..default()
        },
        BackgroundColor(TITLE_BAR_COLOR),
    ));
    title_bar.add_child(title);
    // Dragging the bar moves the whole window.
    title_bar.observe(
        move |drag: On<Pointer<Drag>>, mut nodes: Query<&mut Node>| {
            if drag.button != PointerButton::Primary {
                return;
            }
            if let Ok(mut node) = nodes.get_mut(window) {
                node.left = Val::Px(px(node.left) + drag.delta.x);
                node.top = Val::Px(px(node.top) + drag.delta.y);
            }
        },
    );
    let title_bar = title_bar.id();
    if character.is_some() {
        let close_button = commands
            .spawn((
                CloseButton,
                Interaction::None,
                Node {
                    width: Val::Px(16.0),
                    height: Val::Px(16.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border_radius: BorderRadius::all(Val::Px(2.0)),
                    ..default()
                },
                children![(
                    Text::new("×"),
                    TextFont::from_font_size(12.0),
                    TextColor(CLOSE_COLOR),
                )],
            ))
            .observe(move |_click: On<Pointer<Click>>, mut commands: Commands| {
                commands.entity(window).despawn();
            })
            .id();
        commands.entity(title_bar).add_child(close_button);
    }

    commands
        .entity(window)
        .insert((
            HistoryWindow {
                character,
                title,
                body,
            },
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(corner.x),
                top: Val::Px(corner.y),
                flex_direction: FlexDirection::Column,
                min_width: Val::Px(220.0),
                border: UiRect::all(Val::Px(1.0)),
                overflow: Overflow::clip(),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(PANEL_COLOR),
            BorderColor::all(BORDER_COLOR),
            z,
            Visibility::Hidden,
        ))
        .add_children(&[title_bar, body])
        // Any press brings the window to the front.
        .observe(
            move |_press: On<Pointer<Press>>,
                  mut stack: ResMut<WindowStack>,
                  mut commands: Commands| {
                commands.entity(window).insert(stack.raise());
            },
        );
    window
}

fn px(val: Val) -> f32 {
    match val {
        Val::Px(px) => px,
        _ => 0.0,
    }
}

fn style_close_button(mut buttons: Query<(&Interaction, &mut BackgroundColor), With<CloseButton>>) {
    for (interaction, mut background) in &mut buttons {
        let wanted = match interaction {
            Interaction::Pressed => CLOSE_PRESSED_COLOR,
            Interaction::Hovered => CLOSE_HOVER_COLOR,
            Interaction::None => Color::NONE,
        };
        if background.0 != wanted {
            background.0 = wanted;
        }
    }
}

/// Points the tooltip at the character under the cursor, if any.
fn hover_character(
    ui: UiHover,
    spatial: SpatialQuery,
    cursor: Cursor,
    characters: Query<(), With<ActionLog>>,
    mut tooltip: Single<&mut HistoryWindow, With<Tooltip>>,
) {
    // Nothing is hovered while the cursor is on the UI.
    tooltip.character = cursor.ray().filter(|_| !ui.over_ui()).and_then(|ray| {
        let hit = spatial.cast_ray(
            ray.origin,
            ray.direction,
            PICK_DISTANCE,
            true,
            &SpatialQueryFilter::from_mask(Layer::Character),
        )?;
        characters.contains(hit.entity).then_some(hit.entity)
    });
}

/// Clicking a hovered character opens a pinned window for it, or raises the one it already has.
/// Escape closes every pinned window.
fn open_window(
    mut commands: Commands,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    cursor: Cursor,
    mut stack: ResMut<WindowStack>,
    tooltip: Single<&HistoryWindow, With<Tooltip>>,
    pinned: Query<(Entity, &HistoryWindow), Without<Tooltip>>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        for (window, _) in &pinned {
            commands.entity(window).despawn();
        }
    }

    let Some(character) = tooltip
        .character
        .filter(|_| buttons.just_pressed(MouseButton::Left))
    else {
        return;
    };
    let existing = pinned
        .iter()
        .find(|(_, window)| window.character == Some(character))
        .map(|(entity, _)| entity);
    match existing {
        Some(window) => {
            commands.entity(window).insert(stack.raise());
        }
        None => {
            // Opens where the tooltip is, so it looks like the tooltip stuck in place.
            let corner = cursor.position().unwrap_or_default() + TOOLTIP_OFFSET;
            spawn_window(&mut commands, Some(character), corner, stack.raise());
        }
    }
}

/// Fills every window with its character's history. The tooltip follows the cursor and hides
/// when nothing is hovered; a pinned window closes when its character is gone.
fn render_windows(
    mut commands: Commands,
    time: Res<Time>,
    cursor: Cursor,
    logs: Query<(&ActionLog, Option<&Name>, &Strength, &Speed)>,
    mut windows: Query<(
        Entity,
        &HistoryWindow,
        &mut Node,
        &mut Visibility,
        Has<Tooltip>,
    )>,
    mut texts: Query<&mut Text>,
) {
    let now = time.elapsed_secs();
    for (entity, window, mut node, mut visibility, is_tooltip) in &mut windows {
        let Some((character, (log, name, strength, speed))) = window
            .character
            .and_then(|character| Some((character, logs.get(character).ok()?)))
        else {
            if is_tooltip {
                *visibility = Visibility::Hidden;
            } else {
                commands.entity(entity).despawn();
            }
            continue;
        };
        if is_tooltip {
            let corner = cursor.position().unwrap_or_default() + TOOLTIP_OFFSET;
            node.left = Val::Px(corner.x);
            node.top = Val::Px(corner.y);
        }
        *visibility = Visibility::Inherited;

        if let Ok(mut title) = texts.get_mut(window.title) {
            title.0.clear();
            match name {
                Some(name) => title.0.push_str(name),
                None => {
                    let _ = write!(title.0, "{character}");
                }
            }
            let _ = write!(title.0, "   str {:.2}  spd {:.2}", strength.0, speed.0);
        }
        if let Ok(mut body) = texts.get_mut(window.body) {
            write_history(&mut body.0, log, now);
        }
    }
}

/// Writes the newest entries, one per line; each action lasts until the one logged after it.
fn write_history(out: &mut String, log: &ActionLog, now: f32) {
    out.clear();
    let mut end = now;
    for entry in log.iter().rev().take(WINDOW_LINES) {
        let duration = end - entry.at;
        let _ = writeln!(
            out,
            "{:>6.1}s ago  {:?}  ({duration:.1}s)",
            now - entry.at,
            entry.action
        );
        end = entry.at;
    }
    if out.is_empty() {
        out.push_str("(no history)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(world: &mut World) -> Entity {
        world.spawn_empty().id()
    }

    #[test]
    fn record_skips_repeats_of_the_current_action() {
        let mut world = World::new();
        let tree = entity(&mut world);
        let mut log = ActionLog::default();

        log.record(1.0, Action::WalkTo { tree });
        log.record(2.0, Action::WalkTo { tree });
        log.record(3.0, Action::Chop { tree });
        log.record(4.0, Action::WalkTo { tree });

        let actions: Vec<_> = log.iter().map(|e| (e.at, e.action)).collect();
        assert_eq!(
            actions,
            [
                (1.0, Action::WalkTo { tree }),
                (3.0, Action::Chop { tree }),
                (4.0, Action::WalkTo { tree }),
            ]
        );
        assert_eq!(log.current().unwrap().at, 4.0);
    }

    #[test]
    fn empty_log_has_no_current_action() {
        let log = ActionLog::default();
        assert!(log.current().is_none());
        assert_eq!(log.iter().len(), 0);
        assert_eq!(log.since(0.0).count(), 0);
    }

    #[test]
    fn log_drops_oldest_past_capacity() {
        let mut world = World::new();
        let (a, b) = (entity(&mut world), entity(&mut world));
        let mut log = ActionLog::default();

        // Alternate so no entry is deduplicated away.
        for i in 0..(HISTORY_CAPACITY + 5) {
            let tree = if i % 2 == 0 { a } else { b };
            log.record(i as f32, Action::Chop { tree });
        }

        assert_eq!(log.iter().len(), HISTORY_CAPACITY);
        assert_eq!(log.iter().next().unwrap().at, 5.0, "oldest five evicted");
        assert_eq!(log.current().unwrap().at, (HISTORY_CAPACITY + 4) as f32);
    }

    #[test]
    fn since_returns_recent_entries_newest_first() {
        let mut world = World::new();
        let tree = entity(&mut world);
        let mut log = ActionLog::default();
        log.record(0.0, Action::Idle);
        log.record(5.0, Action::WalkTo { tree });
        log.record(10.0, Action::Chop { tree });
        log.record(15.0, Action::Haul { tree });

        let at: Vec<f32> = log.since(5.0).map(|e| e.at).collect();
        assert_eq!(at, [15.0, 10.0, 5.0]);
        assert_eq!(log.since(16.0).count(), 0);
    }

    #[test]
    fn write_history_formats_newest_first_with_durations() {
        let mut log = ActionLog::default();
        let mut out = String::new();

        write_history(&mut out, &log, 10.0);
        assert_eq!(out, "(no history)");

        let mut world = World::new();
        let tree = entity(&mut world);
        log.record(2.0, Action::Idle);
        log.record(5.0, Action::Chop { tree });
        write_history(&mut out, &log, 10.0);

        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2);
        // Current action: started 5s ago, still going (5s so far).
        assert!(lines[0].starts_with("   5.0s ago  Chop"), "{:?}", lines[0]);
        assert!(lines[0].ends_with("(5.0s)"), "{:?}", lines[0]);
        // Previous action lasted from 2s to 5s.
        assert_eq!(lines[1], "   8.0s ago  Idle  (3.0s)");
    }

    #[test]
    fn write_history_shows_at_most_window_lines() {
        let mut world = World::new();
        let (a, b) = (entity(&mut world), entity(&mut world));
        let mut log = ActionLog::default();
        for i in 0..(WINDOW_LINES * 2) {
            let tree = if i % 2 == 0 { a } else { b };
            log.record(i as f32, Action::Chop { tree });
        }
        let mut out = String::new();
        write_history(&mut out, &log, 100.0);
        assert_eq!(out.lines().count(), WINDOW_LINES);
    }
}
