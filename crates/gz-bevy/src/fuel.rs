//! Searching Gazebo Fuel (DECISIONS.md D-102): a [`FuelSearch`] asks for
//! a page of models matching a query, fetched off the main thread (the
//! browser's `fetch` on the web), and [`GzFuel`] holds what came back. A
//! newer search replaces one still under way; an empty query clears.

use bevy::prelude::*;
use bevy::tasks::{futures::check_ready, IoTaskPool, Task};
use gz_core::fuel::{parse_models, search_url, FuelModel, PAGE_SIZE};

use crate::assets::fetch_bytes;

/// Ask Fuel for `query`'s `page` (from 1; a later page adds to the
/// results).
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub struct FuelSearch {
    pub query: String,
    pub page: u32,
}

/// Where a search is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum FuelState {
    #[default]
    Idle,
    Searching,
    /// Fuel couldn't be reached (`true`), or answered with something else.
    Failed {
        unreachable: bool,
        message: String,
    },
}

/// The last search's results.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct GzFuel {
    pub query: String,
    /// The pages fetched so far.
    pub page: u32,
    pub models: Vec<FuelModel>,
    /// The last page was full: there may be more.
    pub more: bool,
    pub state: FuelState,
}

type Fetch = Task<Result<Vec<u8>, (bool, String)>>;

/// How a search URL is fetched: over HTTP, or a test's canned replies.
#[derive(Resource, Clone, Copy)]
pub struct FuelFetch(pub fn(String) -> Fetch);

impl Default for FuelFetch {
    fn default() -> Self {
        FuelFetch(|url| {
            IoTaskPool::get().spawn(async move {
                fetch_bytes(&url)
                    .await
                    .map_err(|e| (e.unreachable, e.message))
            })
        })
    }
}

/// The search under way: its query, page and fetch.
#[derive(Resource, Default)]
struct Searching(Option<(String, u32, Fetch)>);

/// Starts the newest search, dropping one under way.
fn start_searches(
    mut searches: MessageReader<FuelSearch>,
    fetch: Res<FuelFetch>,
    mut searching: ResMut<Searching>,
    mut fuel: ResMut<GzFuel>,
) {
    let Some(s) = searches.read().last().cloned() else {
        return;
    };
    let query = s.query.trim().to_string();
    if query.is_empty() {
        searching.0 = None;
        *fuel = GzFuel::default();
        return;
    }
    let task = (fetch.0)(search_url(&query, s.page.max(1)));
    if s.page <= 1 || fuel.query != query {
        fuel.models.clear();
        fuel.page = 0;
        fuel.more = false;
    }
    fuel.query.clone_from(&query);
    fuel.state = FuelState::Searching;
    searching.0 = Some((query, s.page.max(1), task));
}

/// Takes a finished search's models.
fn finish_searches(mut searching: ResMut<Searching>, mut fuel: ResMut<GzFuel>) {
    let Some((_, page, task)) = searching.0.as_mut() else {
        return;
    };
    let Some(result) = check_ready(task) else {
        return;
    };
    let page = *page;
    searching.0 = None;
    let fetched = result.map_err(|(unreachable, message)| FuelState::Failed {
        unreachable,
        message,
    });
    match fetched.and_then(|body| {
        parse_models(&body).map_err(|message| FuelState::Failed {
            unreachable: false,
            message,
        })
    }) {
        Ok(models) => {
            fuel.more = models.len() as u32 >= PAGE_SIZE;
            fuel.models.extend(models);
            fuel.page = page;
            fuel.state = FuelState::Idle;
        }
        Err(state) => fuel.state = state,
    }
}

/// Fuel search: [`FuelSearch`] in, [`GzFuel`] out.
pub struct FuelPlugin;

impl Plugin for FuelPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<FuelSearch>()
            .init_resource::<GzFuel>()
            .init_resource::<FuelFetch>()
            .init_resource::<Searching>()
            .add_systems(Update, (start_searches, finish_searches).chain());
    }
}

impl FuelPlugin {
    /// Adds the plugin unless it is already there.
    pub fn ensure(app: &mut App) {
        if !app.is_plugin_added::<Self>() {
            app.add_plugins(Self);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Canned replies by page: page 1 full, page 2 short; "down" can't
    /// reach Fuel; "bad" answers with something else.
    fn canned(url: String) -> Fetch {
        IoTaskPool::get().spawn(async move {
            if url.contains("q=down") {
                return Err((true, "connection refused".to_string()));
            }
            if url.contains("q=bad") {
                return Ok(b"<html>".to_vec());
            }
            let n = if url.contains("page=1&") {
                PAGE_SIZE
            } else {
                3
            };
            let models: Vec<String> = (0..n)
                .map(|i| format!(r#"{{"name":"m{i}","owner":"o"}}"#))
                .collect();
            Ok(format!("[{}]", models.join(",")).into_bytes())
        })
    }

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((TaskPoolPlugin::default(), FuelPlugin))
            .insert_resource(FuelFetch(canned));
        app
    }

    fn search(app: &mut App, query: &str, page: u32) -> GzFuel {
        app.world_mut().write_message(FuelSearch {
            query: query.into(),
            page,
        });
        for _ in 0..100 {
            app.update();
            if app.world().resource::<GzFuel>().state != FuelState::Searching {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        app.world().resource::<GzFuel>().clone()
    }

    /// A search fills the results; the next page adds to them and says
    /// when there are no more; a new query starts over; an empty one
    /// clears; failures say whether Fuel was reached.
    #[test]
    fn searches_fill_pages_and_fail_with_a_reason() {
        let mut app = app();
        let f = search(&mut app, "pallet", 1);
        assert_eq!(
            (f.models.len(), f.page, f.more),
            (PAGE_SIZE as usize, 1, true)
        );
        let f = search(&mut app, "pallet", 2);
        assert_eq!(
            (f.models.len(), f.page, f.more),
            (PAGE_SIZE as usize + 3, 2, false)
        );
        let f = search(&mut app, "box", 1);
        assert_eq!(
            (f.query.as_str(), f.models.len()),
            ("box", PAGE_SIZE as usize)
        );
        let f = search(&mut app, "down", 1);
        assert!(
            matches!(
                f.state,
                FuelState::Failed {
                    unreachable: true,
                    ..
                }
            ),
            "{f:?}"
        );
        let f = search(&mut app, "bad", 1);
        assert!(
            matches!(
                f.state,
                FuelState::Failed {
                    unreachable: false,
                    ..
                }
            ),
            "{f:?}"
        );
        app.world_mut().write_message(FuelSearch {
            query: "  ".into(),
            page: 1,
        });
        app.update();
        assert_eq!(*app.world().resource::<GzFuel>(), GzFuel::default());
    }
}
