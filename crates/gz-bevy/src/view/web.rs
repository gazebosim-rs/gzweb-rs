//! The browser side of the viewer's input (spec §9.6): listeners on the
//! page's canvas for what winit leaves to the page.
//!
//! - **wheel** (`passive: false`): a wheel event with `ctrlKey` would zoom
//!   the page, so it is cancelled; that is also how a trackpad pinch
//!   arrives (Chrome, Firefox), with no Control key-down, so the frame is
//!   flagged in [`BrowserWheel`] and zooms to the cursor.
//! - **gesturestart/gesturechange** (Safari's pinch): cancelled, and the
//!   scale change is passed on as a pinch.
//! - **contextmenu**: cancelled on the canvas only, so right-drag pans.
//! - **keydown**: `preventDefault` for the chords in `shortcuts.json`'s
//!   `prevent_default_when_canvas_focused` ([`InputMap::prevents_default`]).
//!   The listener is on the canvas, so it only sees keys while the canvas
//!   has focus; text fields elsewhere on the page keep every browser
//!   shortcut, and so do the UI's own text fields inside the canvas while
//!   one has focus ([`set_text_focus`]), except Tab and Shift+Tab: those
//!   move focus between the UI's widgets, and the browser would also move
//!   its own focus off the canvas.
//!
//! Listeners live as long as the page.

use std::cell::Cell;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use super::controls::BrowserWheel;
use super::input::{InputMap, KeyMods};

thread_local! {
    /// What the listeners saw since the last frame: a `ctrlKey` wheel event,
    /// and a pinch's change in log scale.
    static SEEN: Cell<(bool, f32)> = const { Cell::new((false, 0.0)) };
    /// Safari's gesture scale at the last `gesturechange`.
    static GESTURE_SCALE: Cell<f64> = const { Cell::new(1.0) };
    /// A UI text field in the canvas has keyboard focus.
    static TEXT_FOCUS: Cell<bool> = const { Cell::new(false) };
}

/// The `localStorage` key of the last server the page connected to
/// (spec §7.10: the connect card's address is prefilled from it).
pub const LAST_URL_KEY: &str = "gzweb.last_url";

/// The last server the page connected to, if the browser kept it.
pub fn last_url() -> Option<String> {
    web_sys::window()?
        .local_storage()
        .ok()??
        .get_item(LAST_URL_KEY)
        .ok()?
        .filter(|u| !u.is_empty())
}

/// Remembers `url` as the last server connected to.
pub fn remember_url(url: &str) {
    if let Some(Ok(Some(storage))) = web_sys::window().map(|w| w.local_storage()) {
        let _ = storage.set_item(LAST_URL_KEY, url);
    }
}

/// Tells the canvas listener that a UI text field has (or lost) keyboard
/// focus. While one has, no key's default is prevented but Tab's: the
/// viewport doesn't have focus (spec §9.6), and Tab belongs to the UI.
pub fn set_text_focus(focused: bool) {
    TEXT_FOCUS.with(|f| f.set(focused));
}

/// Adds the listeners right away (they don't wait for the renderer, which
/// starts asynchronously) and the system that hands their findings to
/// [`BrowserWheel`] each frame. The widget gallery calls it too, for the
/// key handling.
pub fn install(app: &mut App) {
    app.init_resource::<BrowserWheel>();
    let world = app.world_mut();
    let selector = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .iter(world)
        .next()
        .and_then(|w| w.canvas.clone());
    let map = world
        .get_resource::<InputMap>()
        .cloned()
        .unwrap_or_default();
    listen(selector.as_deref().unwrap_or("canvas"), &map);
    app.add_systems(PreUpdate, pump);
}

/// Moves what the listeners saw into [`BrowserWheel`] for this frame.
fn pump(mut wheel: ResMut<BrowserWheel>) {
    let (ctrl, pinch) = SEEN.with(|s| s.replace((false, 0.0)));
    let next = BrowserWheel { ctrl, pinch };
    if *wheel != next {
        *wheel = next;
    }
}

fn listen(selector: &str, map: &InputMap) {
    let Some(canvas) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.query_selector(selector).ok().flatten())
    else {
        warn!("no canvas '{selector}': the browser keeps its wheel, menu and key defaults");
        return;
    };
    let options = web_sys::AddEventListenerOptions::new();
    options.set_passive(false);
    let on = |name: &str, f: Closure<dyn FnMut(web_sys::Event)>| {
        if canvas
            .add_event_listener_with_callback_and_add_event_listener_options(
                name,
                f.as_ref().unchecked_ref(),
                &options,
            )
            .is_err()
        {
            warn!("couldn't listen for '{name}' on the canvas");
        }
        f.forget();
    };

    on(
        "wheel",
        Closure::new(|e: web_sys::Event| {
            if e.dyn_ref::<web_sys::WheelEvent>()
                .is_some_and(|w| w.ctrl_key())
            {
                e.prevent_default();
                SEEN.with(|s| s.set((true, s.get().1)));
            }
        }),
    );
    on(
        "contextmenu",
        Closure::new(|e: web_sys::Event| e.prevent_default()),
    );
    on(
        "gesturestart",
        Closure::new(|e: web_sys::Event| {
            e.prevent_default();
            GESTURE_SCALE.with(|g| g.set(1.0));
        }),
    );
    on(
        "gesturechange",
        Closure::new(|e: web_sys::Event| {
            e.prevent_default();
            let scale = js_sys::Reflect::get(&e, &"scale".into())
                .ok()
                .and_then(|v| v.as_f64())
                .filter(|s| *s > 0.0);
            if let Some(scale) = scale {
                let last = GESTURE_SCALE.with(|g| g.replace(scale));
                let d = (scale / last).ln() as f32;
                SEEN.with(|s| s.set((s.get().0, s.get().1 + d)));
            }
        }),
    );
    let swallow: Vec<_> = map
        .web_prevent_default
        .iter()
        .map(|c| (super::input::dom_code(c.key), c.key))
        .collect();
    let map = map.clone();
    on(
        "keydown",
        Closure::new(move |e: web_sys::Event| {
            let Some(k) = e.dyn_ref::<web_sys::KeyboardEvent>() else {
                return;
            };
            let held = KeyMods {
                ctrl: k.ctrl_key(),
                shift: k.shift_key(),
                alt: k.alt_key(),
                super_key: k.meta_key(),
            };
            let code = k.code();
            let typing = TEXT_FOCUS.with(Cell::get);
            if swallow
                .iter()
                .any(|(c, key)| *c == code && map.web_cancels(*key, held, typing))
            {
                e.prevent_default();
            }
        }),
    );
}
