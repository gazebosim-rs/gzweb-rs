//! The Assets panel's state and rules, as plain data and functions: the
//! tab, what is armed to place, the Fuel search's query, and each insert
//! on its way (DECISIONS.md D-102).

use bevy::prelude::*;
use gz_core::insert::Insertable;

/// How long a search waits after the last keystroke.
pub(crate) const SEARCH_DELAY_SECS: f64 = 0.35;

/// How long the server has to answer a `create`.
pub(crate) const REPLY_SECS: f64 = 10.0;

/// How long an accepted insert has to show in the scene (a Fuel model
/// downloads on the server first).
pub(crate) const APPEAR_SECS: f64 = 120.0;

/// The panel's tabs (§3.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum AssetTab {
    #[default]
    Shapes,
    Lights,
    Fuel,
    Local,
}

impl AssetTab {
    pub(crate) const ALL: [AssetTab; 4] = [
        AssetTab::Shapes,
        AssetTab::Lights,
        AssetTab::Fuel,
        AssetTab::Local,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            AssetTab::Shapes => "Shapes",
            AssetTab::Lights => "Lights",
            AssetTab::Fuel => "Fuel",
            AssetTab::Local => "Local",
        }
    }
}

/// What the user set; the body is built from it.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct AssetsState {
    pub tab: AssetTab,
    /// Chosen to insert: clicks in the views place it.
    pub armed: Option<Insertable>,
    /// The Fuel search field's text, and when it last changed (real
    /// seconds), until it is searched.
    pub query: String,
    pub typed_at: Option<f64>,
    /// The Local tab's URI.
    pub uri: String,
}

/// Where an insert is.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum InsertStage {
    /// Sent; waiting for the server's answer.
    Sent,
    /// The server took it; waiting for it to show in the scene.
    Accepted,
    /// It failed: why.
    Failed(String),
}

/// One insert on its way, from the `create` to the scene.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Insert {
    pub id: u32,
    pub what: Insertable,
    pub stage: InsertStage,
    /// When it entered its stage (real seconds).
    pub since: f64,
    /// The scene's names that were already its own when it was sent, so
    /// only a new one counts as it showing up.
    pub taken: Vec<String>,
}

impl Insert {
    pub(crate) fn label(&self) -> String {
        self.what.label()
    }

    /// Its status line.
    pub(crate) fn status(&self) -> String {
        match &self.stage {
            InsertStage::Sent => "Inserting…".into(),
            InsertStage::Accepted => match self.what {
                Insertable::Model { .. } => "Loading on the server…".into(),
                _ => "Inserting…".into(),
            },
            InsertStage::Failed(why) => why.clone(),
        }
    }

    pub(crate) fn failed(&self) -> bool {
        matches!(self.stage, InsertStage::Failed(_))
    }
}

/// The inserts on their way, oldest first; a finished one goes, a failed
/// one stays until dismissed.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub(crate) struct Inserts {
    pub list: Vec<Insert>,
    next: u32,
    /// Answers still owed to inserts that timed out: the next ones that
    /// come are theirs, not a later insert's (replies carry no id).
    late: u32,
}

impl Inserts {
    /// Records `what` as sent at `now`, with the names already its own.
    pub(crate) fn sent(&mut self, what: Insertable, now: f64, taken: Vec<String>) -> u32 {
        self.next += 1;
        self.list.push(Insert {
            id: self.next,
            what,
            stage: InsertStage::Sent,
            since: now,
            taken,
        });
        self.next
    }

    /// The server answered the oldest `create` still waiting: `ok` or
    /// not.
    pub(crate) fn answered(&mut self, ok: bool, now: f64) {
        if self.late > 0 {
            self.late -= 1;
            return;
        }
        if let Some(i) = self.list.iter_mut().find(|i| i.stage == InsertStage::Sent) {
            i.stage = if ok {
                InsertStage::Accepted
            } else {
                InsertStage::Failed(format!("The server couldn't insert {}", i.what.label()))
            };
            i.since = now;
        }
    }

    /// Ends the accepted inserts that `names` (the scene's) now hold, the
    /// oldest claiming each new name (two boxes need two), and fails those
    /// that ran out of time.
    pub(crate) fn settle<'a>(&mut self, names: impl Iterator<Item = &'a str> + Clone, now: f64) {
        let mut claimed: Vec<&str> = vec![];
        self.list.retain(|i| {
            if i.stage != InsertStage::Accepted {
                return true;
            }
            let name = i.what.name();
            let new = names.clone().find(|n| {
                Insertable::named(&name, n)
                    && !i.taken.iter().any(|t| t == n)
                    && !claimed.contains(n)
            });
            match new {
                Some(n) => {
                    claimed.push(n);
                    false
                }
                None => true,
            }
        });
        for i in &mut self.list {
            i.taken.extend(claimed.iter().map(|n| (*n).to_string()));
        }
        for i in &mut self.list {
            let late = match i.stage {
                InsertStage::Sent if now - i.since > REPLY_SECS => {
                    self.late += 1;
                    Some("The server didn't answer".to_string())
                }
                InsertStage::Accepted if now - i.since > APPEAR_SECS => {
                    Some(format!("{} didn't show up in the world", i.what.label()))
                }
                _ => None,
            };
            if let Some(why) = late {
                i.stage = InsertStage::Failed(why);
                i.since = now;
            }
        }
    }

    /// Dismisses insert `id`.
    pub(crate) fn dismiss(&mut self, id: u32) {
        self.list.retain(|i| i.id != id);
    }

    /// Whether `uri`'s model is on its way (its Fuel row shows a bar).
    pub(crate) fn loading(&self, uri: &str) -> bool {
        self.list
            .iter()
            .any(|i| !i.failed() && matches!(&i.what, Insertable::Model { uri: u, .. } if u == uri))
    }
}
