//! The Scene panel's tree (spec §3.3, §5.7) as data: the world's entities
//! in a flat preorder array ([`TreeModel`]), the panel-owned state a body
//! is built from ([`TreeState`]: expansion, filter text, type chip; the
//! scroll offset is [`TreeScroll`]), and the rows they make
//! ([`TreeModel::rows`]). No ECS here beyond the resource derives, so it is
//! tested and measured on its own (thousands of entities).
//!
//! What is in the tree (DECISIONS.md D-66): the world (W), its models (M,
//! nested models under theirs), links (K), lights (L, the scene's and the
//! links'), the links' sensors (S, from the scene message) and the models'
//! joints (J, from the world's SDFormat once it is known; under the model,
//! after its links, as Gazebo's entity tree has them). Visuals aren't
//! listed (§5.7 has no letter for them).

use std::collections::HashSet;

use bevy::prelude::*;

use crate::widgets::EntityKind;

/// A tree node's identity across rebuilds (the expansion and the keyboard
/// cursor key on it): entities by their unique scene path, sensors and
/// joints by their parent's path and name.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum NodeKey {
    World,
    Entity(String),
    Sensor(String, String),
    Joint(String, String),
}

/// One entity of the tree.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TreeNode {
    pub(crate) key: NodeKey,
    pub(crate) name: String,
    /// The name in lower case, for the filter.
    lower: String,
    pub(crate) kind: EntityKind,
    pub(crate) depth: u32,
    /// Nodes in its subtree, itself included; they follow it in the array.
    pub(crate) size: u32,
    /// The scene entity whose visibility the eye toggles (models, links,
    /// lights; not the world, sensors or joints).
    pub(crate) entity: Option<Entity>,
}

impl TreeNode {
    pub(crate) fn has_children(&self) -> bool {
        self.size > 1
    }
}

/// The world's entities in preorder (DECISIONS.md D-66), rebuilt when the
/// scene's structure changes.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct TreeModel {
    pub(crate) nodes: Vec<TreeNode>,
}

/// Builds a [`TreeModel`] depth first: [`open`](Self::open) a node, add
/// its children, [`close`](Self::close) it.
#[derive(Default)]
pub(crate) struct ModelBuilder {
    nodes: Vec<TreeNode>,
    open: Vec<usize>,
}

impl ModelBuilder {
    pub(crate) fn open(
        &mut self,
        key: NodeKey,
        name: &str,
        kind: EntityKind,
        entity: Option<Entity>,
    ) {
        self.open.push(self.nodes.len());
        self.nodes.push(TreeNode {
            key,
            name: name.to_string(),
            lower: name.to_lowercase(),
            kind,
            depth: self.open.len() as u32 - 1,
            size: 1,
            entity,
        });
    }

    pub(crate) fn close(&mut self) {
        if let Some(i) = self.open.pop() {
            self.nodes[i].size = (self.nodes.len() - i) as u32;
        }
    }

    /// A node without children.
    pub(crate) fn leaf(
        &mut self,
        key: NodeKey,
        name: &str,
        kind: EntityKind,
        entity: Option<Entity>,
    ) {
        self.open(key, name, kind, entity);
        self.close();
    }

    pub(crate) fn finish(mut self) -> TreeModel {
        while !self.open.is_empty() {
            self.close();
        }
        TreeModel { nodes: self.nodes }
    }
}

/// The type chips (§3.3: All, Models, Lights, Sensors, Joints).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum Chip {
    #[default]
    All,
    Models,
    Lights,
    Sensors,
    Joints,
}

impl Chip {
    pub(crate) const ALL: [Chip; 5] = [
        Chip::All,
        Chip::Models,
        Chip::Lights,
        Chip::Sensors,
        Chip::Joints,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Chip::All => "All",
            Chip::Models => "Models",
            Chip::Lights => "Lights",
            Chip::Sensors => "Sensors",
            Chip::Joints => "Joints",
        }
    }

    fn admits(self, kind: EntityKind) -> bool {
        match self {
            Chip::All => true,
            Chip::Models => kind == EntityKind::Model,
            Chip::Lights => kind == EntityKind::Light,
            Chip::Sensors => kind == EntityKind::Sensor,
            Chip::Joints => kind == EntityKind::Joint,
        }
    }
}

/// What the user set on the tree, which outlives its body (UI_PLAN §7,
/// DECISIONS.md D-58): expansion, the filter text and chip, the keyboard
/// cursor, and whether the chip row shows.
#[derive(Resource, Clone, Debug, PartialEq)]
pub(crate) struct TreeState {
    /// Expanded nodes (the world, at first).
    pub(crate) expanded: HashSet<NodeKey>,
    pub(crate) filter: String,
    pub(crate) chip: Chip,
    /// While filtering, the tree opens every path to a match; these are
    /// the ones the user closed again (forgotten when the filter changes).
    pub(crate) filter_collapsed: HashSet<NodeKey>,
    /// The keyboard's row (§6.5: arrow keys move within the tree).
    pub(crate) cursor: Option<NodeKey>,
    /// The chip row shows (the filter button toggles it).
    pub(crate) chips_shown: bool,
}

impl Default for TreeState {
    fn default() -> Self {
        Self {
            expanded: HashSet::from([NodeKey::World]),
            filter: String::new(),
            chip: Chip::All,
            filter_collapsed: HashSet::new(),
            cursor: None,
            chips_shown: true,
        }
    }
}

impl TreeState {
    /// A filter text or a chip other than All is on.
    pub(crate) fn filtering(&self) -> bool {
        !self.filter.trim().is_empty() || self.chip != Chip::All
    }
}

/// The tree's scroll offset in UI pixels, kept across body rebuilds.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct TreeScroll(pub(crate) f32);

/// One shown row: its node, and its caret (`None` for a leaf).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    pub(crate) node: u32,
    pub(crate) expanded: Option<bool>,
}

impl TreeModel {
    pub(crate) fn node(&self, row: Row) -> &TreeNode {
        &self.nodes[row.node as usize]
    }

    /// Entities in the tree, the world not counted (the header's count).
    pub(crate) fn entities(&self) -> usize {
        self.nodes
            .iter()
            .filter(|n| n.kind != EntityKind::World)
            .count()
    }

    /// The rows `state` shows, top to bottom. Without a filter: every node
    /// whose ancestors are expanded. With one: the matches (name contains
    /// the text, case-insensitively, and the chip admits the kind) and the
    /// paths to them, open unless the user closed them; a match's other
    /// children stay out. Linear in the nodes.
    pub(crate) fn rows(&self, state: &TreeState) -> Vec<Row> {
        let n = self.nodes.len();
        let mut rows = Vec::new();
        if !state.filtering() {
            let mut i = 0;
            while i < n {
                let node = &self.nodes[i];
                let expanded = node
                    .has_children()
                    .then(|| state.expanded.contains(&node.key));
                rows.push(Row {
                    node: i as u32,
                    expanded,
                });
                i += if expanded == Some(false) {
                    node.size as usize
                } else {
                    1
                };
            }
            return rows;
        }
        let text = state.filter.trim().to_lowercase();
        // keep: a match or an ancestor of one; under: has a kept descendant.
        let mut keep = vec![false; n];
        let mut under = vec![false; n];
        let mut parents: Vec<usize> = Vec::new();
        let mut parent = vec![usize::MAX; n];
        for (i, node) in self.nodes.iter().enumerate() {
            while parents
                .last()
                .is_some_and(|&p| i >= p + self.nodes[p].size as usize)
            {
                parents.pop();
            }
            parent[i] = parents.last().copied().unwrap_or(usize::MAX);
            if node.has_children() {
                parents.push(i);
            }
        }
        for i in (0..n).rev() {
            let node = &self.nodes[i];
            if state.chip.admits(node.kind) && node.lower.contains(&text) {
                keep[i] = true;
            }
            if keep[i] && parent[i] != usize::MAX {
                keep[parent[i]] = true;
                under[parent[i]] = true;
            }
        }
        let mut i = 0;
        while i < n {
            let node = &self.nodes[i];
            if !keep[i] {
                i += node.size as usize;
                continue;
            }
            let expanded = under[i].then(|| !state.filter_collapsed.contains(&node.key));
            rows.push(Row {
                node: i as u32,
                expanded,
            });
            i += if expanded == Some(true) {
                1
            } else {
                node.size as usize
            };
        }
        rows
    }

    /// Every node with children (Expand all).
    pub(crate) fn branches(&self) -> impl Iterator<Item = &NodeKey> {
        self.nodes
            .iter()
            .filter(|n| n.has_children())
            .map(|n| &n.key)
    }

    /// The node's parent, if any.
    pub(crate) fn parent(&self, node: u32) -> Option<u32> {
        let depth = self.nodes[node as usize].depth;
        (0..node)
            .rev()
            .find(|&i| self.nodes[i as usize].depth + 1 == depth)
            .filter(|_| depth > 0)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A world of `models` models, each with `links` links holding a
    /// sensor, and a joint, plus a sun: 1 + models × (2 + 2 × links) + 1
    /// nodes.
    pub(crate) fn synthetic(models: usize, links: usize) -> TreeModel {
        let mut b = ModelBuilder::default();
        b.open(NodeKey::World, "world", EntityKind::World, None);
        for m in 0..models {
            let path = format!("model_{m}");
            b.open(
                NodeKey::Entity(path.clone()),
                &path,
                EntityKind::Model,
                None,
            );
            for l in 0..links {
                let link = format!("{path}/link_{l}");
                b.open(
                    NodeKey::Entity(link.clone()),
                    &format!("link_{l}"),
                    EntityKind::Link,
                    None,
                );
                b.leaf(
                    NodeKey::Sensor(link, "cam".into()),
                    "cam",
                    EntityKind::Sensor,
                    None,
                );
                b.close();
            }
            b.leaf(
                NodeKey::Joint(path, "joint".into()),
                "joint",
                EntityKind::Joint,
                None,
            );
            b.close();
        }
        b.leaf(
            NodeKey::Entity("sun".into()),
            "Sun",
            EntityKind::Light,
            None,
        );
        b.finish()
    }

    fn names(model: &TreeModel, rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|r| {
                let n = model.node(*r);
                let caret = match r.expanded {
                    Some(true) => "v ",
                    Some(false) => "> ",
                    None => "",
                };
                format!("{}{caret}{}", "  ".repeat(n.depth as usize), n.name)
            })
            .collect()
    }

    #[test]
    fn rows_follow_the_expansion() {
        let model = synthetic(2, 1);
        assert_eq!(model.nodes.len(), 10);
        assert_eq!(model.entities(), 9);
        let mut state = TreeState::default();
        assert_eq!(
            names(&model, &model.rows(&state)),
            ["v world", "  > model_0", "  > model_1", "  Sun"]
        );
        state.expanded.insert(NodeKey::Entity("model_1".into()));
        state
            .expanded
            .insert(NodeKey::Entity("model_1/link_0".into()));
        assert_eq!(
            names(&model, &model.rows(&state)),
            [
                "v world",
                "  > model_0",
                "  v model_1",
                "    v link_0",
                "      cam",
                "    joint",
                "  Sun"
            ]
        );
        // Expand all opens every branch; collapsing the world hides all.
        state.expanded = model.branches().cloned().collect();
        assert_eq!(model.rows(&state).len(), model.nodes.len());
        state.expanded.remove(&NodeKey::World);
        assert_eq!(names(&model, &model.rows(&state)), ["> world"]);
        assert_eq!(model.parent(3), Some(2));
        assert_eq!(model.parent(0), None);
    }

    #[test]
    fn a_filter_shows_matches_and_their_paths() {
        let model = synthetic(3, 2);
        let mut state = TreeState {
            filter: "  MODEL_1 ".into(),
            ..TreeState::default()
        };
        // The match without its (unmatched) children, the world open to it.
        assert_eq!(names(&model, &model.rows(&state)), ["v world", "  model_1"]);
        // The Sensors chip: every camera, the paths open.
        state.filter.clear();
        state.chip = Chip::Sensors;
        let rows = model.rows(&state);
        assert_eq!(rows.len(), 1 + 3 * (1 + 2 * 2));
        assert_eq!(
            names(&model, &rows[..4]),
            ["v world", "  v model_0", "    v link_0", "      cam"]
        );
        // Text and chip together; a path closed by the user stays closed.
        state.filter = "link_1".into();
        state.chip = Chip::All;
        state
            .filter_collapsed
            .insert(NodeKey::Entity("model_2".into()));
        assert_eq!(
            names(&model, &model.rows(&state)),
            [
                "v world",
                "  v model_0",
                "    link_1",
                "  v model_1",
                "    link_1",
                "  > model_2"
            ]
        );
        // Nothing matches: no rows.
        state.filter = "nothing".into();
        assert!(model.rows(&state).is_empty());
        for chip in Chip::ALL {
            assert!(!chip.label().is_empty());
        }
        assert!(Chip::Lights.admits(EntityKind::Light) && !Chip::Joints.admits(EntityKind::Model));
    }

    /// Thousands of entities (UI_PLAN M10: a 5,000-entity tree at 60 fps):
    /// the rows of a 20,000-node tree, all expanded or filtered, take well
    /// under a frame even in a debug build.
    #[test]
    fn rows_scale_to_thousands_of_entities() {
        let model = synthetic(2_000, 4);
        assert_eq!(model.nodes.len(), 2 + 2_000 * 10);
        let mut state = TreeState {
            expanded: model.branches().cloned().collect(),
            ..TreeState::default()
        };
        let start = std::time::Instant::now();
        let all = model.rows(&state);
        state.filter = "link_3".into();
        let filtered = model.rows(&state);
        let took = start.elapsed();
        assert_eq!(all.len(), model.nodes.len());
        assert_eq!(filtered.len(), 1 + 2_000 * 2);
        assert!(took.as_millis() < 100, "{took:?}");
    }
}
