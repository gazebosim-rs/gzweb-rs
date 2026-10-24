//! The command palette's items and how a query ranks them (spec §7.7;
//! DECISIONS.md D-107): what each item is, what running it does, the
//! Gazebo GUI plugin names that find it, and the fuzzy match.

use bevy::prelude::*;

use crate::assets::icons::IconId;
use crate::panels::{ActionId, PanelId};
use crate::shell::UiCommand;

/// The result groups, in the order they show (§7.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Group {
    Commands,
    Entities,
    Topics,
    Panels,
    Settings,
}

impl Group {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Group::Commands => "Commands",
            Group::Entities => "Entities",
            Group::Topics => "Topics",
            Group::Panels => "Panels",
            Group::Settings => "Settings",
        }
    }

    /// The most a group lists for a query (the world's entities and the
    /// server's topics can be many).
    pub(crate) fn cap(self) -> usize {
        match self {
            Group::Entities | Group::Topics => 8,
            Group::Commands | Group::Panels | Group::Settings => 50,
        }
    }
}

/// What running an item does.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Run {
    /// A `shortcuts.json` binding, as its key would
    /// ([`RunBinding`](gz_bevy::view::controls::RunBinding)).
    Binding(String),
    Ui(UiCommand),
    /// A shown panel's header or footer action.
    PanelAction {
        panel: PanelId,
        action: ActionId,
    },
    /// Select an entity and bring it into view.
    Select(Entity),
    /// Echo a topic in Topics.
    Echo(String),
}

/// One result.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Item {
    pub group: Group,
    pub icon: IconId,
    pub name: String,
    /// A path, a type or a note, in `text_3`.
    pub hint: String,
    /// Its keys ("⌘K"), at the right.
    pub shortcut: Option<String>,
    /// Other words that find it: the Gazebo GUI plugin it stands for.
    pub keywords: Vec<&'static str>,
    pub run: Run,
    /// Unavailable now (shown dimmed; Enter does nothing): why.
    pub disabled: Option<&'static str>,
}

/// The Gazebo GUI plugins each command or panel stands for (spec §11),
/// so a Gazebo user finds it by the name they know.
pub(crate) fn gazebo_names(id: &str) -> &'static [&'static str] {
    match id {
        "sim.play_pause" | "sim.step" | "sim.reset" => &["WorldControl"],
        "view.focus" | "view.home" | "view.projection" => &["InteractiveViewControl"],
        "view.top" | "view.bottom" | "view.front" | "view.back" | "view.right" | "view.left" => {
            &["ViewAngle"]
        }
        "view.follow" | "view.track" => &["CameraTracking"],
        "overlay.grid" => &["GridConfig", "VisualizationCapabilities"],
        "overlay.collisions" | "overlay.inertia" | "overlay.joints" => {
            &["VisualizationCapabilities"]
        }
        "tool.navigate" | "tool.box_select" | "tool.translate" | "tool.rotate" => {
            &["TransformControl"]
        }
        "ui.inspector" => &["ComponentInspector", "ComponentInspectorEditor"],
        "scene" => &["EntityTree"],
        "assets" => &["Shapes", "Lights", "ResourceSpawner", "Fuel"],
        "topics" => &["TopicViewer", "TopicEcho", "Publisher"],
        "plot" => &["Plot"],
        "sensors" => &["ImageDisplay", "PointCloud"],
        "teleop" => &["Teleop", "KeyPublisher", "JointPositionController"],
        "console" => &["Log", "Messages"],
        _ => &[],
    }
}

/// How well `query` matches `text`, higher better; `None` when it
/// doesn't: its letters in order, any case. The whole text, then its
/// start, then a word's start, then a run of letters scores most.
pub(crate) fn score(query: &str, text: &str) -> Option<i32> {
    let q: Vec<char> = query.trim().to_lowercase().chars().collect();
    if q.is_empty() {
        return Some(0);
    }
    let orig: Vec<char> = text.chars().collect();
    let t: Vec<char> = orig
        .iter()
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();
    let whole: String = q.iter().collect();
    let lower: String = t.iter().collect();
    if lower == whole {
        return Some(1000);
    }
    if lower.starts_with(&whole) {
        return Some(800 - i32::try_from(t.len()).unwrap_or(i32::MAX).min(200));
    }
    let word_start = |i: usize| {
        i == 0
            || !orig[i - 1].is_alphanumeric()
            || (orig[i].is_uppercase() && orig[i - 1].is_lowercase())
    };
    if let Some(at) = lower.find(&whole) {
        let i = lower[..at].chars().count();
        return Some(if word_start(i) { 600 } else { 400 });
    }
    // Letters in order: a point each, more for a word's start or a run.
    let mut points = 0;
    let mut qi = 0;
    let mut last: Option<usize> = None;
    for (i, c) in t.iter().enumerate() {
        if qi < q.len() && *c == q[qi] {
            points += 1;
            if word_start(i) {
                points += 3;
            }
            if last.is_some_and(|l| l + 1 == i) {
                points += 2;
            }
            last = Some(i);
            qi += 1;
        }
    }
    (qi == q.len()).then_some(points)
}

/// `item`'s best score for `query`, over its name and its keywords (a
/// keyword's counts a little less).
pub(crate) fn item_score(query: &str, item: &Item) -> Option<i32> {
    let name = score(query, &item.name);
    let words = item
        .keywords
        .iter()
        .filter_map(|k| score(query, k))
        .map(|s| s - 50)
        .max();
    name.into_iter().chain(words).max()
}

/// A score this high has the query in one piece (not only its letters in
/// order).
const WHOLE: i32 = 400;

/// The results for `query`: by group in their order, each group's best
/// first (up to its [cap](Group::cap)). When some result has the query
/// in one piece, the ones with only its letters scattered go (so "amr"
/// lists `amr_01`, not "Maximize view, right"). With no query, the
/// world's entities and the server's topics stay out (they can be many);
/// the rest come in their own order.
pub(crate) fn rank(items: Vec<Item>, query: &str) -> Vec<Item> {
    let empty = query.trim().is_empty();
    let mut scored: Vec<(i32, usize, Item)> = items
        .into_iter()
        .enumerate()
        .filter(|(_, it)| !(empty && matches!(it.group, Group::Entities | Group::Topics)))
        .filter_map(|(i, it)| item_score(query, &it).map(|s| (s, i, it)))
        .collect();
    // (A keyword's whole match scores 50 under its name's.)
    if scored.iter().any(|(s, ..)| *s >= WHOLE - 50) {
        scored.retain(|(s, ..)| *s >= WHOLE - 50);
    }
    scored.sort_by(|a, b| {
        (a.2.group, std::cmp::Reverse(a.0), a.1).cmp(&(b.2.group, std::cmp::Reverse(b.0), b.1))
    });
    let mut out: Vec<Item> = vec![];
    for (_, _, item) in scored {
        let n = out.iter().filter(|i| i.group == item.group).count();
        if n < item.group.cap() {
            out.push(item);
        }
    }
    out
}

/// The next enabled result from `from` by `step` (1 down, -1 up),
/// wrapping; `from` itself when no other is.
pub(crate) fn step(items: &[Item], from: Option<usize>, step: i32) -> Option<usize> {
    let n = items.len();
    if n == 0 {
        return None;
    }
    let n_i = i64::try_from(n).ok()?;
    let start = from.map_or(if step > 0 { -1 } else { n_i }, |f| {
        i64::try_from(f).unwrap_or(0)
    });
    for k in 1..=n_i {
        let i = (start + k * i64::from(step)).rem_euclid(n_i);
        let i = usize::try_from(i).ok()?;
        if items[i].disabled.is_none() {
            return Some(i);
        }
    }
    from
}
