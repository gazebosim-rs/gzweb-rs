//! The keyboard map: `design/ui/shortcuts.json`, embedded at build time and
//! parsed into an [`InputMap`].
//!
//! The file is the spec's single source of truth (DESIGN.md §6.3). The crate
//! embeds its own copy (`src/view/shortcuts.json`, so `cargo package` works);
//! a test keeps it identical to the spec's. Each
//! binding has keys (`mod+shift+R`, `W A S D / arrows`, `Space (hold)`), a
//! scope that decides when it fires, and for the browser either the same
//! keys or `web: null` plus a `web_alt`. `mod` is ⌘ on macOS and Ctrl
//! elsewhere; in the browser it is either, since the page can't tell which
//! one the platform means.
//!
//! [`MINIMAL_JSON`] adds the bindings only minimal mode has (the fly keys),
//! in the same format, so they go through the same parser and conflict
//! check.

use bevy::input::keyboard::KeyCode;
use bevy::input::ButtonInput;
use bevy::prelude::Resource;
use serde::de::IgnoredAny;
use serde::Deserialize;

use super::{FlyDir, SnapView};
use crate::overlays::Overlay;

/// `design/ui/shortcuts.json`, from the crate's copy.
pub const SHORTCUTS_JSON: &str = include_str!("shortcuts.json");

/// Bindings that the spec file leaves out: minimal mode's camera fly keys.
/// Scope `minimal` fires only in minimal mode, which has no Teleop panel,
/// so they share W A S D and the arrows with `teleop.drive` without a
/// conflict (DECISIONS.md D-12).
pub const MINIMAL_JSON: &str = r#"{
  "bindings": [
    { "id": "view.fly_forward", "label": "Move forward", "keys": "W / Up", "scope": "minimal", "trigger": "held" },
    { "id": "view.fly_back", "label": "Move back", "keys": "S / Down", "scope": "minimal", "trigger": "held" },
    { "id": "view.fly_left", "label": "Move left", "keys": "A / Left", "scope": "minimal", "trigger": "held" },
    { "id": "view.fly_right", "label": "Move right", "keys": "D / Right", "scope": "minimal", "trigger": "held" },
    { "id": "view.fly_down", "label": "Move down", "keys": "Q", "scope": "minimal", "trigger": "held" },
    { "id": "view.fly_up", "label": "Move up", "keys": "E", "scope": "minimal", "trigger": "held" }
  ]
}"#;

/// Pairs of bindings that share keys and overlapping scopes but can never be
/// live at the same time, as the spec states.
pub const EXCLUSIVE: &[(&str, &str)] = &[
    // Decision Q5: an armed Teleop disables gizmo drags, and X only
    // constrains while a gizmo is being dragged.
    ("tool.constrain_x", "teleop.stop"),
];

/// When a binding fires (`scope` in the file).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    /// Anywhere, except inside text fields that consume the key.
    Global,
    /// The pointer is over a view, or a view has focus.
    Viewport,
    /// Never while a text field has focus.
    Text,
    /// Only while the Teleop panel is armed.
    Teleop,
    /// Only in minimal mode (gzweb-rs addition, see [`MINIMAL_JSON`]).
    Minimal,
}

impl Scope {
    fn parse(s: &str) -> Option<Scope> {
        Some(match s {
            "global" => Scope::Global,
            "viewport" => Scope::Viewport,
            "text" => Scope::Text,
            "teleop" => Scope::Teleop,
            "minimal" => Scope::Minimal,
            _ => return None,
        })
    }

    /// Whether both scopes can be live at once. Every pair can, except
    /// minimal mode and an armed Teleop panel, which only exists in the full
    /// UI.
    pub fn overlaps(self, other: Scope) -> bool {
        !matches!(
            (self, other),
            (Scope::Minimal, Scope::Teleop) | (Scope::Teleop, Scope::Minimal)
        )
    }
}

/// How the keys trigger the binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// Fires once when the chord goes down. If a [`Trigger::Hold`] binding
    /// shares the chord, it fires on a quick release instead (a tap).
    Press,
    /// Modifies pointer drags while held (`Space (hold)`).
    Hold,
    /// Acts every frame while held (the fly keys).
    Held,
}

/// Modifiers as the file writes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Mods {
    /// `mod`: ⌘ on macOS, Ctrl on Linux and Windows.
    pub cmd: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// Modifier keys as they are held on a keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct KeyMods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// ⌘ / Windows / Super.
    pub super_key: bool,
}

impl KeyMods {
    pub fn held(keys: &ButtonInput<KeyCode>) -> KeyMods {
        KeyMods {
            ctrl: keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]),
            shift: keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
            alt: keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]),
            super_key: keys.any_pressed([KeyCode::SuperLeft, KeyCode::SuperRight]),
        }
    }

    /// Ctrl, Alt or ⌘ (not Shift): a command chord is being typed.
    pub fn command(self) -> bool {
        self.ctrl || self.alt || self.super_key
    }

    /// The platform's `mod` key is held (⌘ on a Mac, Ctrl on Linux and
    /// Windows, either in a browser, as `mod` chords match there): it
    /// toggles in the selection (spec §6.2).
    pub fn mod_key(self, platform: Platform) -> bool {
        match platform {
            Platform::Mac => self.super_key,
            Platform::Web => self.ctrl || self.super_key,
            _ => self.ctrl,
        }
    }
}

/// Where the app runs, which decides what `mod` means and whether the
/// browser's reserved chords apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Mac,
    Linux,
    Windows,
    /// Any browser: `mod` is Ctrl or ⌘, and `web: null` bindings use their
    /// `web_alt`.
    Web,
}

impl Platform {
    pub const ALL: [Platform; 4] = [
        Platform::Mac,
        Platform::Linux,
        Platform::Windows,
        Platform::Web,
    ];

    /// The platform this build runs on.
    pub fn current() -> Platform {
        if cfg!(target_arch = "wasm32") {
            Platform::Web
        } else if cfg!(target_os = "macos") {
            Platform::Mac
        } else if cfg!(target_os = "windows") {
            Platform::Windows
        } else {
            Platform::Linux
        }
    }
}

/// One key with its modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord {
    pub key: KeyCode,
    pub mods: Mods,
}

impl Chord {
    /// The modifier sets that type this chord on `platform`.
    pub fn resolve(&self, platform: Platform) -> Vec<KeyMods> {
        let base = KeyMods {
            ctrl: self.mods.ctrl,
            shift: self.mods.shift,
            alt: self.mods.alt,
            super_key: false,
        };
        if !self.mods.cmd {
            return vec![base];
        }
        let ctrl = KeyMods { ctrl: true, ..base };
        let cmd = KeyMods {
            super_key: true,
            ..base
        };
        match platform {
            Platform::Mac => vec![cmd],
            Platform::Linux | Platform::Windows => vec![ctrl],
            Platform::Web => vec![ctrl, cmd],
        }
    }

    /// Whether `held` modifiers type this chord on `platform`.
    pub fn matches(&self, held: KeyMods, platform: Platform) -> bool {
        self.resolve(platform).contains(&held)
    }

    /// How the chord reads on `platform`: `Ctrl+Right`, `Cmd+Shift+R`, and
    /// `Ctrl/Cmd+K` in the browser, where `mod` is either.
    pub fn label(&self, platform: Platform) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.mods.cmd {
            parts.push(match platform {
                Platform::Mac => "Cmd",
                Platform::Linux | Platform::Windows => "Ctrl",
                Platform::Web => "Ctrl/Cmd",
            });
        }
        if self.mods.ctrl {
            parts.push("Ctrl");
        }
        if self.mods.alt {
            parts.push(if platform == Platform::Mac {
                "Option"
            } else {
                "Alt"
            });
        }
        if self.mods.shift {
            parts.push("Shift");
        }
        let key = key_name(self.key);
        parts.push(&key);
        parts.join("+")
    }

    /// Whether both chords can be typed with the same keys on `platform`.
    pub fn collides(&self, other: &Chord, platform: Platform) -> bool {
        self.key == other.key
            && self
                .resolve(platform)
                .iter()
                .any(|m| other.resolve(platform).contains(m))
    }
}

/// What a binding does, for the bindings gzweb-rs implements. The rest
/// (tools, panels, files, …) parse but have no action yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Focus,
    Home,
    ToggleProjection,
    Snap(SnapView),
    /// Space held: left drag pans.
    PanHold,
    /// ⇧F: follow the selection (or the model under the cursor), or release.
    Follow,
    /// ⇧T: track it, or release.
    Track,
    ToggleOverlay(Overlay),
    PlayPause,
    Step,
    Fly(FlyDir),
}

impl Action {
    fn from_id(id: &str) -> Option<Action> {
        Some(match id {
            "view.focus" => Action::Focus,
            "view.home" => Action::Home,
            "view.projection" => Action::ToggleProjection,
            "view.top" => Action::Snap(SnapView::Top),
            "view.bottom" => Action::Snap(SnapView::Bottom),
            "view.front" => Action::Snap(SnapView::Front),
            "view.back" => Action::Snap(SnapView::Back),
            "view.right" => Action::Snap(SnapView::Right),
            "view.left" => Action::Snap(SnapView::Left),
            "view.pan_hold" => Action::PanHold,
            "view.follow" => Action::Follow,
            "view.track" => Action::Track,
            "overlay.grid" => Action::ToggleOverlay(Overlay::Grid),
            "overlay.collisions" => Action::ToggleOverlay(Overlay::Collisions),
            "overlay.inertia" => Action::ToggleOverlay(Overlay::Inertia),
            "overlay.joints" => Action::ToggleOverlay(Overlay::Joints),
            "sim.play_pause" => Action::PlayPause,
            "sim.step" => Action::Step,
            "view.fly_forward" => Action::Fly(FlyDir::Forward),
            "view.fly_back" => Action::Fly(FlyDir::Back),
            "view.fly_left" => Action::Fly(FlyDir::Left),
            "view.fly_right" => Action::Fly(FlyDir::Right),
            "view.fly_down" => Action::Fly(FlyDir::Down),
            "view.fly_up" => Action::Fly(FlyDir::Up),
            _ => return None,
        })
    }
}

/// One entry of the map.
#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    pub id: String,
    pub label: String,
    pub scope: Scope,
    pub trigger: Trigger,
    /// Empty for palette-only commands (`keys: null`).
    pub keys: Vec<Chord>,
    /// False for `web: null`: the browser never delivers these keys.
    pub on_web: bool,
    /// The browser's alternative when `on_web` is false.
    pub web_alt: Option<Vec<Chord>>,
    pub action: Option<Action>,
    /// A [`Trigger::Press`] binding that shares its chord with a
    /// [`Trigger::Hold`] one, so it fires on a quick release (Space).
    pub tap: bool,
}

impl Binding {
    /// The chords that type this binding on `platform`.
    pub fn chords(&self, platform: Platform) -> &[Chord] {
        match (platform, self.on_web, &self.web_alt) {
            (Platform::Web, false, Some(alt)) => alt,
            (Platform::Web, false, None) => &[],
            _ => &self.keys,
        }
    }
}

/// Two bindings that the same keys would fire at once.
#[derive(Debug, Clone, PartialEq)]
pub struct Conflict {
    pub platform: Platform,
    pub a: String,
    pub b: String,
    pub key: KeyCode,
}

/// The parsed key map, for the platform the app runs on.
#[derive(Resource, Debug, Clone)]
pub struct InputMap {
    pub bindings: Vec<Binding>,
    /// Chords the browser keeps for itself (`web_reserved.keys`).
    pub web_reserved: Vec<Chord>,
    /// Chords the page swallows (`preventDefault`) while the canvas has
    /// focus (`web_reserved.prevent_default_when_canvas_focused`), so the
    /// browser's own shortcuts keep working everywhere else.
    pub web_prevent_default: Vec<Chord>,
    pub platform: Platform,
}

/// The file's layout. Unknown fields are errors, so a typo (`"triger"`)
/// can't silently drop a binding's meaning; the documentation fields are
/// listed and ignored.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFile {
    bindings: Vec<RawBinding>,
    web_reserved: Option<RawReserved>,
    #[serde(rename = "$note")]
    _note: Option<IgnoredAny>,
    #[serde(rename = "modifier_vocabulary")]
    _modifier_vocabulary: Option<IgnoredAny>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawReserved {
    keys: Vec<String>,
    prevent_default_when_canvas_focused: Option<Vec<String>>,
    #[serde(rename = "note")]
    _note: Option<IgnoredAny>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBinding {
    id: String,
    label: String,
    keys: Option<String>,
    scope: String,
    /// Absent: same keys on the web. `null`: not available there.
    #[serde(default, deserialize_with = "present")]
    web: Option<serde_json::Value>,
    web_alt: Option<String>,
    trigger: Option<String>,
    #[serde(rename = "note")]
    _note: Option<IgnoredAny>,
    #[serde(rename = "also")]
    _also: Option<IgnoredAny>,
}

/// Keeps an explicit `null` as `Some(Null)`, so it differs from absent.
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<serde_json::Value>, D::Error> {
    serde_json::Value::deserialize(d).map(Some)
}

impl Default for InputMap {
    fn default() -> Self {
        Self::builtin()
    }
}

impl InputMap {
    /// The spec's shortcuts plus [`MINIMAL_JSON`], for this platform. Both
    /// are embedded and covered by tests, so they always parse.
    pub fn builtin() -> InputMap {
        let mut map =
            InputMap::parse(SHORTCUTS_JSON, Platform::current()).expect("shortcuts.json parses");
        let extra =
            InputMap::parse(MINIMAL_JSON, Platform::current()).expect("MINIMAL_JSON parses");
        map.bindings.extend(extra.bindings);
        map.mark_taps();
        map
    }

    /// Parses a file in the `shortcuts.json` format.
    pub fn parse(json: &str, platform: Platform) -> Result<InputMap, String> {
        let raw: RawFile = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let bindings = raw
            .bindings
            .into_iter()
            .map(binding)
            .collect::<Result<Vec<_>, _>>()?;
        let chords = |list: &[String]| {
            list.iter()
                .map(|k| parse_keys(k).map(|(chords, _)| chords))
                .collect::<Result<Vec<_>, _>>()
                .map(|c| c.concat())
        };
        let (web_reserved, web_prevent_default) = match raw.web_reserved {
            Some(r) => (
                chords(&r.keys)?,
                chords(&r.prevent_default_when_canvas_focused.unwrap_or_default())?,
            ),
            None => (Vec::new(), Vec::new()),
        };
        let mut map = InputMap {
            bindings,
            web_reserved,
            web_prevent_default,
            platform,
        };
        map.mark_taps();
        Ok(map)
    }

    fn mark_taps(&mut self) {
        let holds: Vec<(Scope, Chord)> = self
            .bindings
            .iter()
            .filter(|b| b.trigger == Trigger::Hold)
            .flat_map(|b| b.keys.iter().map(move |c| (b.scope, *c)))
            .collect();
        for b in &mut self.bindings {
            b.tap = b.trigger == Trigger::Press
                && b.keys.iter().any(|c| {
                    holds
                        .iter()
                        .any(|(s, h)| s.overlaps(b.scope) && h.key == c.key && h.mods == c.mods)
                });
        }
    }

    pub fn binding(&self, id: &str) -> Option<&Binding> {
        self.bindings.iter().find(|b| b.id == id)
    }

    fn live<'a>(&'a self, scopes: &'a [Scope]) -> impl Iterator<Item = &'a Binding> + 'a {
        self.bindings
            .iter()
            .filter(move |b| scopes.contains(&b.scope))
    }

    /// [`Trigger::Press`] bindings (not taps) in `scopes` whose chord went
    /// down this frame with exactly its modifiers held.
    pub fn just_pressed<'a>(
        &'a self,
        keys: &'a ButtonInput<KeyCode>,
        scopes: &'a [Scope],
    ) -> impl Iterator<Item = &'a Binding> + 'a {
        let held = KeyMods::held(keys);
        self.live(scopes).filter(move |b| {
            b.trigger == Trigger::Press
                && !b.tap
                && b.chords(self.platform)
                    .iter()
                    .any(|c| key_event(keys, c.key, true) && c.matches(held, self.platform))
        })
    }

    /// Tap bindings in `scopes`, with the chord that types each.
    pub fn taps<'a>(
        &'a self,
        scopes: &'a [Scope],
    ) -> impl Iterator<Item = (&'a Binding, Chord)> + 'a {
        self.live(scopes).filter(|b| b.tap).flat_map(move |b| {
            b.chords(self.platform)
                .iter()
                .map(move |c| (b, *c))
                .collect::<Vec<_>>()
        })
    }

    /// [`Trigger::Held`] bindings in `scopes` with a key down. Shift is
    /// allowed; Ctrl, Alt and ⌘ mean a command chord, so nothing is held.
    /// Nor is a key the held modifiers make another binding's chord (⇧D,
    /// the web's Duplicate, isn't D's fly right).
    pub fn held<'a>(
        &'a self,
        keys: &'a ButtonInput<KeyCode>,
        scopes: &'a [Scope],
    ) -> impl Iterator<Item = &'a Binding> + 'a {
        let mods = KeyMods::held(keys);
        let command = mods.command();
        let chorded = move |key: KeyCode| {
            mods != KeyMods::default()
                && self.live(scopes).any(|o| {
                    o.trigger != Trigger::Held
                        && o.chords(self.platform)
                            .iter()
                            .any(|c| c.key == key && c.matches(mods, self.platform))
                })
        };
        self.live(scopes).filter(move |b| {
            !command
                && b.trigger == Trigger::Held
                && b.chords(self.platform)
                    .iter()
                    .any(|c| key_event(keys, c.key, false) && !chorded(c.key))
        })
    }

    /// Whether a [`Trigger::Hold`] binding for `action` in `scopes` is held.
    pub fn holding(&self, keys: &ButtonInput<KeyCode>, scopes: &[Scope], action: Action) -> bool {
        self.live(scopes).any(|b| {
            b.trigger == Trigger::Hold
                && b.action == Some(action)
                && b.chords(self.platform).iter().any(|c| keys.pressed(c.key))
        })
    }

    /// Whether the browser must not act on `key` with `held` modifiers while
    /// the canvas has focus (spec §9.6: only the listed chords).
    pub fn prevents_default(&self, key: KeyCode, held: KeyMods) -> bool {
        self.web_prevent_default
            .iter()
            .any(|c| c.key == key && c.matches(held, Platform::Web))
    }

    /// Whether the browser canvas cancels a keydown's default: the
    /// [`prevents_default`](Self::prevents_default) chords while no UI text
    /// field has focus; while one has, only Tab and Shift+Tab, which move
    /// focus between the UI's widgets (the browser would also move its own
    /// focus off the canvas).
    pub fn web_cancels(&self, key: KeyCode, held: KeyMods, text_focus: bool) -> bool {
        if text_focus {
            key == KeyCode::Tab && !(held.ctrl || held.alt || held.super_key)
        } else {
            self.prevents_default(key, held)
        }
    }

    /// Spec §13: bindings that the same keys fire at once on `platform`:
    /// overlapping scopes, not listed in [`EXCLUSIVE`], and not a tap with
    /// the hold that shares its chord (decision Q4: Space tap and Space
    /// hold).
    pub fn conflicts(&self, platform: Platform) -> Vec<Conflict> {
        let mut out = Vec::new();
        let tap_pair = |x: &Binding, y: &Binding| x.trigger == Trigger::Hold && y.tap;
        for (i, a) in self.bindings.iter().enumerate() {
            for b in &self.bindings[i + 1..] {
                let exclusive = EXCLUSIVE
                    .iter()
                    .any(|&(x, y)| (a.id == x && b.id == y) || (a.id == y && b.id == x));
                let tap_and_hold = tap_pair(a, b) || tap_pair(b, a);
                if exclusive || tap_and_hold || !a.scope.overlaps(b.scope) {
                    continue;
                }
                for ca in a.chords(platform) {
                    if let Some(cb) = b
                        .chords(platform)
                        .iter()
                        .find(|cb| ca.collides(cb, platform))
                    {
                        out.push(Conflict {
                            platform,
                            a: a.id.clone(),
                            b: b.id.clone(),
                            key: cb.key,
                        });
                    }
                }
            }
        }
        out
    }

    /// Bindings that break the web rules: a binding on a reserved chord
    /// must say `web: null`, and every `web: null` binding needs a
    /// `web_alt` that isn't reserved itself. "Reserved" means the browser
    /// would see the same keys: `ctrl+W` is `mod+W` there.
    pub fn web_violations(&self) -> Vec<String> {
        let reserved = |c: &Chord| {
            self.web_reserved
                .iter()
                .any(|r| r.collides(c, Platform::Web))
        };
        let mut out = Vec::new();
        for b in &self.bindings {
            if b.on_web && b.keys.iter().any(reserved) {
                out.push(format!("{}: uses a browser-reserved chord", b.id));
            }
            if !b.on_web {
                match &b.web_alt {
                    None => out.push(format!("{}: web: null without a web_alt", b.id)),
                    Some(alt) if alt.iter().any(reserved) => {
                        out.push(format!("{}: web_alt is browser-reserved", b.id));
                    }
                    Some(_) => {}
                }
            }
        }
        out
    }
}

/// `just` = went down this frame; otherwise held. Digits also answer to the
/// numeric keypad, as in other 3D tools.
fn key_event(keys: &ButtonInput<KeyCode>, key: KeyCode, just: bool) -> bool {
    let test = |k: KeyCode| {
        if just {
            keys.just_pressed(k)
        } else {
            keys.pressed(k)
        }
    };
    test(key) || numpad(key).is_some_and(test)
}

fn numpad(key: KeyCode) -> Option<KeyCode> {
    Some(match key {
        KeyCode::Digit0 => KeyCode::Numpad0,
        KeyCode::Digit1 => KeyCode::Numpad1,
        KeyCode::Digit2 => KeyCode::Numpad2,
        KeyCode::Digit3 => KeyCode::Numpad3,
        KeyCode::Digit4 => KeyCode::Numpad4,
        KeyCode::Digit5 => KeyCode::Numpad5,
        KeyCode::Digit6 => KeyCode::Numpad6,
        KeyCode::Digit7 => KeyCode::Numpad7,
        KeyCode::Digit8 => KeyCode::Numpad8,
        KeyCode::Digit9 => KeyCode::Numpad9,
        _ => return None,
    })
}

fn binding(raw: RawBinding) -> Result<Binding, String> {
    let err = |e: String| format!("{}: {e}", raw.id);
    let scope = Scope::parse(&raw.scope).ok_or_else(|| err(format!("scope '{}'", raw.scope)))?;
    let (keys, hold) = match &raw.keys {
        Some(k) => parse_keys(k).map_err(err)?,
        None => (Vec::new(), false),
    };
    let trigger = match raw.trigger.as_deref() {
        None if hold => Trigger::Hold,
        None => Trigger::Press,
        Some("held") => Trigger::Held,
        Some(t) => return Err(err(format!("trigger '{t}'"))),
    };
    let on_web = !matches!(raw.web, Some(serde_json::Value::Null));
    let web_alt = match &raw.web_alt {
        Some(k) => Some(parse_keys(k).map_err(err)?.0),
        None => None,
    };
    Ok(Binding {
        action: Action::from_id(&raw.id),
        id: raw.id,
        label: raw.label,
        scope,
        trigger,
        keys,
        on_web,
        web_alt,
        tap: false,
    })
}

/// Parses a `keys` string: alternatives split by ` / `, a space-separated
/// list of single keys (`W A S D`), `arrows`, chords joined by `+`, a key
/// range (`mod+1..9`) and a trailing note in parentheses. Returns the chords
/// and whether the note is `(hold)`.
pub fn parse_keys(s: &str) -> Result<(Vec<Chord>, bool), String> {
    let (keys, note) = match s.find('(') {
        Some(i) => (&s[..i], s[i + 1..].trim_end_matches(')').trim()),
        None => (s, ""),
    };
    let mut chords = Vec::new();
    for alt in keys.split(" / ").map(str::trim) {
        if alt.contains('+') {
            chords.extend(parse_chord(alt)?);
        } else {
            for token in alt.split_whitespace() {
                chords.extend(key_range(token)?.into_iter().map(|key| Chord {
                    key,
                    mods: Mods::default(),
                }));
            }
        }
    }
    if chords.is_empty() {
        return Err(format!("no keys in '{s}'"));
    }
    Ok((chords, note == "hold"))
}

fn parse_chord(s: &str) -> Result<Vec<Chord>, String> {
    let mut tokens: Vec<&str> = s.split('+').map(str::trim).collect();
    let key = tokens.pop().unwrap_or_default();
    let mut mods = Mods::default();
    for m in tokens {
        match m.to_ascii_lowercase().as_str() {
            "mod" => mods.cmd = true,
            "ctrl" => mods.ctrl = true,
            "shift" => mods.shift = true,
            "alt" => mods.alt = true,
            _ => return Err(format!("modifier '{m}' in '{s}'")),
        }
    }
    Ok(key_range(key)?
        .into_iter()
        .map(|key| Chord { key, mods })
        .collect())
}

/// One key, `arrows`, or a digit range like `1..9`.
fn key_range(token: &str) -> Result<Vec<KeyCode>, String> {
    if token.eq_ignore_ascii_case("arrows") {
        return Ok(vec![
            KeyCode::ArrowUp,
            KeyCode::ArrowLeft,
            KeyCode::ArrowDown,
            KeyCode::ArrowRight,
        ]);
    }
    if let Some((a, b)) = token.split_once("..") {
        let digit = |t: &str| t.parse::<u8>().ok().filter(|d| *d <= 9);
        let (Some(a), Some(b)) = (digit(a), digit(b)) else {
            return Err(format!("range '{token}'"));
        };
        if a > b {
            return Err(format!("reversed range '{token}'"));
        }
        return (a..=b).map(|d| key(&d.to_string())).collect();
    }
    key(token).map(|k| vec![k])
}

/// A key name as the file writes it (`W`, `7`, `Escape`, `Right`, `F2`).
pub fn key(name: &str) -> Result<KeyCode, String> {
    const LETTERS: [KeyCode; 26] = [
        KeyCode::KeyA,
        KeyCode::KeyB,
        KeyCode::KeyC,
        KeyCode::KeyD,
        KeyCode::KeyE,
        KeyCode::KeyF,
        KeyCode::KeyG,
        KeyCode::KeyH,
        KeyCode::KeyI,
        KeyCode::KeyJ,
        KeyCode::KeyK,
        KeyCode::KeyL,
        KeyCode::KeyM,
        KeyCode::KeyN,
        KeyCode::KeyO,
        KeyCode::KeyP,
        KeyCode::KeyQ,
        KeyCode::KeyR,
        KeyCode::KeyS,
        KeyCode::KeyT,
        KeyCode::KeyU,
        KeyCode::KeyV,
        KeyCode::KeyW,
        KeyCode::KeyX,
        KeyCode::KeyY,
        KeyCode::KeyZ,
    ];
    const DIGITS: [KeyCode; 10] = [
        KeyCode::Digit0,
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    const FKEYS: [KeyCode; 12] = [
        KeyCode::F1,
        KeyCode::F2,
        KeyCode::F3,
        KeyCode::F4,
        KeyCode::F5,
        KeyCode::F6,
        KeyCode::F7,
        KeyCode::F8,
        KeyCode::F9,
        KeyCode::F10,
        KeyCode::F11,
        KeyCode::F12,
    ];
    let lower = name.to_ascii_lowercase();
    let mut chars = lower.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_lowercase() {
            return Ok(LETTERS[(c as u8 - b'a') as usize]);
        }
        if c.is_ascii_digit() {
            return Ok(DIGITS[(c as u8 - b'0') as usize]);
        }
    }
    if let Some(n) = lower
        .strip_prefix('f')
        .and_then(|n| n.parse::<usize>().ok())
    {
        if (1..=12).contains(&n) {
            return Ok(FKEYS[n - 1]);
        }
    }
    Ok(match lower.as_str() {
        "escape" | "esc" => KeyCode::Escape,
        "space" => KeyCode::Space,
        "tab" => KeyCode::Tab,
        "enter" => KeyCode::Enter,
        "delete" => KeyCode::Delete,
        "backspace" => KeyCode::Backspace,
        "left" => KeyCode::ArrowLeft,
        "right" => KeyCode::ArrowRight,
        "up" => KeyCode::ArrowUp,
        "down" => KeyCode::ArrowDown,
        _ => return Err(format!("key '{name}'")),
    })
}

/// A key's name as the file writes it (the inverse of [`key`]); the
/// keypad's digits read as digits.
pub fn key_name(key: KeyCode) -> String {
    let name = format!("{key:?}");
    if let Some(rest) = name
        .strip_prefix("Key")
        .or_else(|| name.strip_prefix("Digit"))
        .or_else(|| name.strip_prefix("Numpad"))
        .or_else(|| name.strip_prefix("Arrow"))
    {
        return rest.to_string();
    }
    name
}

/// The DOM `KeyboardEvent.code` of a key: Bevy names [`KeyCode`]s after
/// the same W3C list (`KeyK`, `ArrowRight`, `Space`, `Tab`).
pub fn dom_code(key: KeyCode) -> String {
    format!("{key:?}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> InputMap {
        let mut map = InputMap::parse(SHORTCUTS_JSON, Platform::Linux).unwrap();
        map.bindings.extend(
            InputMap::parse(MINIMAL_JSON, Platform::Linux)
                .unwrap()
                .bindings,
        );
        map.mark_taps();
        map
    }

    fn chord(s: &str) -> Chord {
        let (c, _) = parse_keys(s).unwrap();
        assert_eq!(c.len(), 1, "{s}");
        c[0]
    }

    #[test]
    fn the_spec_file_parses_completely() {
        let map = spec();
        assert!(map.bindings.len() > 60);
        let ids: std::collections::HashSet<_> = map.bindings.iter().map(|b| &b.id).collect();
        assert_eq!(ids.len(), map.bindings.len(), "ids are unique");
        // Every action gzweb-rs implements is bound in the file or the
        // minimal table.
        assert_eq!(map.bindings.iter().filter_map(|b| b.action).count(), 24);

        let step = map.binding("sim.step").unwrap();
        assert_eq!(step.scope, Scope::Global);
        assert_eq!(step.keys, [chord("mod+Right")]);
        assert!(step.keys[0].mods.cmd);
        let pan = map.binding("view.pan_hold").unwrap();
        assert_eq!(
            (pan.trigger, pan.keys[0].key),
            (Trigger::Hold, KeyCode::Space)
        );
        assert!(map.binding("sim.play_pause").unwrap().tap);
        assert!(!map.binding("view.maximize").unwrap().tap);
        let drive = map.binding("teleop.drive").unwrap();
        assert_eq!(drive.keys.len(), 8);
        let delete = map.binding("edit.delete").unwrap();
        assert_eq!(delete.keys.len(), 2);
        let theme = map.binding("ui.theme_dark").unwrap();
        assert!(theme.keys.is_empty() && theme.on_web);
        let close = map.binding("view.close").unwrap();
        assert!(!close.on_web);
        assert_eq!(close.chords(Platform::Web), [chord("alt+W")]);
        assert_eq!(close.chords(Platform::Mac), [chord("mod+W")]);
        let fly = map.binding("view.fly_forward").unwrap();
        assert_eq!(fly.trigger, Trigger::Held);
        assert_eq!(fly.scope, Scope::Minimal);
        // `mod+1..9` expands to nine reserved chords.
        assert!(map.web_reserved.len() > 15);
        assert!(map.web_reserved.contains(&chord("mod+5")));
        assert!(map.web_reserved.contains(&chord("mod+D")));
    }

    /// Spec §13: no two bindings share keys within overlapping scopes on any
    /// platform, and every browser-reserved binding has a `web_alt`.
    #[test]
    fn shortcuts_have_no_conflicts_and_web_alternatives() {
        let map = spec();
        for platform in Platform::ALL {
            assert_eq!(map.conflicts(platform), [], "{platform:?}");
        }
        assert_eq!(map.web_violations(), Vec::<String>::new());
    }

    #[test]
    fn the_conflict_check_catches_clashes() {
        let json = r#"{
          "bindings": [
            { "id": "a", "label": "A", "keys": "mod+K", "scope": "global" },
            { "id": "b", "label": "B", "keys": "ctrl+K", "scope": "viewport" },
            { "id": "c", "label": "C", "keys": "W", "scope": "teleop" },
            { "id": "d", "label": "D", "keys": "W", "scope": "minimal", "trigger": "held" },
            { "id": "e", "label": "E", "keys": "mod+T", "scope": "global" },
            { "id": "f", "label": "F", "keys": "mod+N", "scope": "global", "web": null },
            { "id": "g", "label": "G", "keys": "mod+Q", "scope": "global", "web": null, "web_alt": "mod+T" }
          ],
          "web_reserved": { "keys": ["mod+T", "mod+N", "mod+1..3"] }
        }"#;
        let map = InputMap::parse(json, Platform::Linux).unwrap();
        // ⌘K and Ctrl+K are the same keys off macOS and in the browser.
        let pairs = |p| {
            map.conflicts(p)
                .into_iter()
                .map(|c| (c.a, c.b, c.key))
                .collect::<Vec<_>>()
        };
        assert_eq!(pairs(Platform::Mac), []);
        let ab = ("a".to_string(), "b".to_string(), KeyCode::KeyK);
        assert_eq!(pairs(Platform::Linux), std::slice::from_ref(&ab));
        assert_eq!(pairs(Platform::Windows), std::slice::from_ref(&ab));
        // In the browser, g's web_alt is e's key.
        assert_eq!(
            pairs(Platform::Web),
            [ab, ("e".into(), "g".into(), KeyCode::KeyT)]
        );
        assert_eq!(
            map.web_violations(),
            [
                "e: uses a browser-reserved chord",
                "f: web: null without a web_alt",
                "g: web_alt is browser-reserved",
            ]
        );
        assert_eq!(map.binding("f").unwrap().chords(Platform::Web), []);
    }

    #[test]
    fn the_embedded_copy_is_the_spec_file() {
        let spec = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../design/ui/shortcuts.json"
        ))
        .unwrap();
        assert_eq!(
            SHORTCUTS_JSON, spec,
            "copy design/ui/shortcuts.json to crates/gz-bevy/src/view/"
        );
    }

    /// Only a hold and the tap that shares its chord are exempt from the
    /// conflict rule, and in the browser `ctrl+W` is the reserved `mod+W`.
    #[test]
    fn the_tap_exemption_and_reserved_chords_are_narrow() {
        let json = r#"{ "bindings": [
            { "id": "hold", "label": "H", "keys": "mod+X (hold)", "scope": "viewport" },
            { "id": "press", "label": "P", "keys": "ctrl+X", "scope": "viewport" },
            { "id": "pan", "label": "Pan", "keys": "Space (hold)", "scope": "viewport" },
            { "id": "fly", "label": "Fly", "keys": "Space", "scope": "minimal", "trigger": "held" },
            { "id": "tap", "label": "Tap", "keys": "Space", "scope": "global" },
            { "id": "w", "label": "W", "keys": "ctrl+W", "scope": "global" }
          ], "web_reserved": { "keys": ["mod+W"] } }"#;
        let map = InputMap::parse(json, Platform::Linux).unwrap();
        assert!(!map.binding("press").unwrap().tap);
        assert!(map.binding("tap").unwrap().tap);
        let pairs: Vec<_> = map
            .conflicts(Platform::Linux)
            .into_iter()
            .map(|c| (c.a, c.b))
            .collect();
        // mod+X held is Ctrl+X off macOS: the press isn't its tap. A held
        // fly key isn't a tap either. The tap and its hold are fine.
        assert_eq!(
            pairs,
            [
                ("hold".to_string(), "press".to_string()),
                ("pan".into(), "fly".into()),
                ("fly".into(), "tap".into()),
            ]
        );
        assert_eq!(map.web_violations(), ["w: uses a browser-reserved chord"]);
    }

    #[test]
    fn typos_and_reversed_ranges_are_errors() {
        let typo = r#"{ "bindings": [ { "id": "a", "label": "A", "keys": "K", "scope": "global", "triger": "held" } ] }"#;
        let e = InputMap::parse(typo, Platform::Linux).unwrap_err();
        assert!(e.contains("unknown field `triger`"), "{e}");
        let top = r#"{ "bindings": [], "web_reservd": { "keys": [] } }"#;
        assert!(InputMap::parse(top, Platform::Linux).is_err());
        let reserved = r#"{ "bindings": [], "web_reserved": { "keys": [], "kees": [] } }"#;
        assert!(InputMap::parse(reserved, Platform::Linux).is_err());
        // The documentation fields are allowed.
        let docs = r#"{ "$note": "n", "modifier_vocabulary": { "shift": "s" },
            "bindings": [ { "id": "a", "label": "A", "keys": "K", "scope": "global", "note": "n", "also": "x" } ],
            "web_reserved": { "note": "n", "keys": [], "prevent_default_when_canvas_focused": ["mod+K"] } }"#;
        let map = InputMap::parse(docs, Platform::Linux).unwrap();
        assert_eq!(map.web_prevent_default, [chord("mod+K")]);
        assert_eq!(parse_keys("A / 9..1").unwrap_err(), "reversed range '9..1'");
        assert_eq!(parse_keys("1..1").unwrap().0, [chord("1")]);
    }

    #[test]
    fn the_canvas_swallows_only_the_listed_chords() {
        let map = spec();
        assert!(map.web_prevent_default.contains(&chord("Space")));
        assert!(map.web_prevent_default.contains(&chord("Tab")));
        assert!(map.web_prevent_default.contains(&chord("mod+K")));
        // Save as…: Firefox would also take its screenshot.
        assert!(map.web_prevent_default.contains(&chord("mod+shift+S")));
        // Step: Cmd+Right is the browser's Forward on macOS (D-17).
        assert!(map.web_prevent_default.contains(&chord("mod+Right")));
        let mods = |ctrl, super_key, shift| KeyMods {
            ctrl,
            super_key,
            shift,
            alt: false,
        };
        assert!(map.prevents_default(KeyCode::ArrowRight, mods(false, true, false)));
        assert!(map.prevents_default(KeyCode::ArrowRight, mods(true, false, false)));
        assert!(map.prevents_default(KeyCode::Space, mods(false, false, false)));
        // Not listed: the browser keeps them.
        assert!(!map.prevents_default(KeyCode::ArrowRight, mods(false, false, false)));
        assert!(!map.prevents_default(KeyCode::KeyR, mods(false, true, false)));
        // In a UI text field: only Tab, with or without Shift.
        assert!(map.web_cancels(KeyCode::Space, mods(false, false, false), false));
        assert!(!map.web_cancels(KeyCode::Space, mods(false, false, false), true));
        assert!(!map.web_cancels(KeyCode::KeyK, mods(true, false, false), true));
        assert!(map.web_cancels(KeyCode::Tab, mods(false, false, false), true));
        assert!(map.web_cancels(KeyCode::Tab, mods(false, false, true), true));
        assert!(!map.web_cancels(KeyCode::Tab, mods(true, false, false), true));
        assert!(!map.prevents_default(KeyCode::Space, mods(false, false, true)));
        assert!(!map.prevents_default(KeyCode::KeyW, mods(true, false, false)));
    }

    #[test]
    fn chord_labels_and_dom_codes() {
        assert_eq!(chord("mod+Right").label(Platform::Mac), "Cmd+Right");
        assert_eq!(chord("mod+Right").label(Platform::Linux), "Ctrl+Right");
        assert_eq!(chord("mod+K").label(Platform::Web), "Ctrl/Cmd+K");
        assert_eq!(
            chord("ctrl+alt+shift+7").label(Platform::Windows),
            "Ctrl+Alt+Shift+7"
        );
        assert_eq!(chord("alt+W").label(Platform::Mac), "Option+W");
        assert_eq!(chord("Space").label(Platform::Linux), "Space");
        assert_eq!(key_name(KeyCode::Numpad3), "3");
        assert_eq!(key_name(KeyCode::F2), "F2");
        assert_eq!(key_name(KeyCode::Escape), "Escape");
        for (k, code) in [
            (KeyCode::KeyK, "KeyK"),
            (KeyCode::ArrowRight, "ArrowRight"),
            (KeyCode::Space, "Space"),
            (KeyCode::Tab, "Tab"),
            (KeyCode::Digit5, "Digit5"),
        ] {
            assert_eq!(dom_code(k), code);
        }
        // Every name the file uses reads back to the same key.
        for b in spec().bindings {
            for c in b.keys {
                assert_eq!(key(&key_name(c.key)), Ok(c.key), "{}", b.id);
            }
        }
    }

    #[test]
    fn scopes_overlap_except_minimal_and_teleop() {
        use Scope::*;
        for a in [Global, Viewport, Text, Teleop, Minimal] {
            for b in [Global, Viewport, Text, Teleop, Minimal] {
                let apart = matches!((a, b), (Minimal, Teleop) | (Teleop, Minimal));
                assert_eq!(a.overlaps(b), !apart, "{a:?} {b:?}");
            }
        }
    }

    #[test]
    fn mod_is_cmd_on_mac_ctrl_elsewhere_and_either_on_the_web() {
        let c = chord("mod+shift+R");
        let ctrl = KeyMods {
            ctrl: true,
            shift: true,
            ..default_mods()
        };
        let cmd = KeyMods {
            super_key: true,
            shift: true,
            ..default_mods()
        };
        assert!(c.matches(cmd, Platform::Mac) && !c.matches(ctrl, Platform::Mac));
        assert!(c.matches(ctrl, Platform::Linux) && !c.matches(cmd, Platform::Linux));
        assert!(c.matches(ctrl, Platform::Windows));
        assert!(c.matches(ctrl, Platform::Web) && c.matches(cmd, Platform::Web));
        // Exact modifiers: Shift alone isn't the chord.
        let shift = KeyMods {
            shift: true,
            ..default_mods()
        };
        assert!(!c.matches(shift, Platform::Linux));
        assert!(!chord("R").matches(shift, Platform::Linux));
        assert_eq!(chord("ctrl+alt+X").resolve(Platform::Mac).len(), 1);
    }

    fn default_mods() -> KeyMods {
        KeyMods::default()
    }

    #[test]
    fn key_strings() {
        assert_eq!(
            parse_keys("Space (hold)").unwrap(),
            (vec![chord("Space")], true)
        );
        assert!(!parse_keys("mod+D (bookmarks)").unwrap().1);
        assert_eq!(
            parse_keys("W A S D / arrows").unwrap().0.len(),
            8,
            "four letters and four arrows"
        );
        assert_eq!(parse_keys("alt+1..4").unwrap().0.len(), 4);
        for (name, code) in [
            ("Escape", KeyCode::Escape),
            ("esc", KeyCode::Escape),
            ("Tab", KeyCode::Tab),
            ("Enter", KeyCode::Enter),
            ("Delete", KeyCode::Delete),
            ("Backspace", KeyCode::Backspace),
            ("Left", KeyCode::ArrowLeft),
            ("Up", KeyCode::ArrowUp),
            ("Down", KeyCode::ArrowDown),
            ("F2", KeyCode::F2),
            ("F12", KeyCode::F12),
            ("z", KeyCode::KeyZ),
            ("0", KeyCode::Digit0),
        ] {
            assert_eq!(key(name), Ok(code), "{name}");
        }
        for bad in ["F13", "F0", "Hyper", "", "1..x", "!"] {
            assert!(key_range(bad).is_err(), "{bad}");
        }
        assert!(parse_keys("meta+K").is_err());
        assert!(parse_keys("  ").is_err());
        assert!(parse_keys("(hold)").is_err());
    }

    #[test]
    fn malformed_files_are_rejected_with_the_binding_id() {
        let file = |b: &str| format!(r#"{{ "bindings": [ {b} ] }}"#);
        let parse = |b: &str| InputMap::parse(&file(b), Platform::Linux).unwrap_err();
        assert!(parse("nonsense").contains("expected"));
        assert_eq!(
            parse(r#"{ "id": "x", "label": "X", "keys": "K", "scope": "nowhere" }"#),
            "x: scope 'nowhere'"
        );
        assert_eq!(
            parse(r#"{ "id": "x", "label": "X", "keys": "Hyper", "scope": "global" }"#),
            "x: key 'Hyper'"
        );
        assert_eq!(
            parse(
                r#"{ "id": "x", "label": "X", "keys": "K", "scope": "global", "trigger": "twice" }"#
            ),
            "x: trigger 'twice'"
        );
        assert_eq!(
            parse(
                r#"{ "id": "x", "label": "X", "keys": "K", "scope": "global", "web": null, "web_alt": "?" }"#
            ),
            "x: key '?'"
        );
        let reserved = r#"{ "bindings": [], "web_reserved": { "keys": ["mod+Hyper"] } }"#;
        assert!(InputMap::parse(reserved, Platform::Linux).is_err());
    }

    #[test]
    fn runtime_matching() {
        let map = spec();
        let scopes = [Scope::Global, Scope::Viewport, Scope::Minimal];
        let mut keys = ButtonInput::<KeyCode>::default();
        let ids = |map: &InputMap, keys: &ButtonInput<KeyCode>| {
            map.just_pressed(keys, &scopes)
                .map(|b| b.id.clone())
                .collect::<Vec<_>>()
        };

        keys.press(KeyCode::KeyF);
        assert_eq!(ids(&map, &keys), ["view.focus"]);
        // Shift+F is follow, not focus.
        keys.press(KeyCode::ShiftLeft);
        assert_eq!(ids(&map, &keys), ["view.follow"]);
        keys.clear();
        // Held keys don't fire again.
        assert!(ids(&map, &keys).is_empty());
        keys.reset_all();

        // The keypad's 7 is the top view too.
        keys.press(KeyCode::Numpad7);
        assert_eq!(ids(&map, &keys), ["view.top"]);
        keys.reset_all();

        // Ctrl+Right steps on Linux; ⌘+Right on macOS.
        keys.press(KeyCode::ControlLeft);
        keys.press(KeyCode::ArrowRight);
        assert_eq!(ids(&map, &keys), ["sim.step"]);
        let mac = InputMap {
            platform: Platform::Mac,
            ..map.clone()
        };
        assert!(ids(&mac, &keys).is_empty());
        // A command chord holds no fly key.
        assert_eq!(map.held(&keys, &scopes).count(), 0);
        keys.reset_all();

        // Space is a tap, never a plain press.
        keys.press(KeyCode::Space);
        assert!(ids(&map, &keys).is_empty());
        let taps: Vec<_> = map.taps(&scopes).map(|(b, c)| (b.id.clone(), c)).collect();
        assert_eq!(taps, [("sim.play_pause".to_string(), chord("Space"))]);
        assert!(map.holding(&keys, &scopes, Action::PanHold));
        assert!(!map.holding(&keys, &[Scope::Global], Action::PanHold));
        keys.reset_all();

        // Fly keys are held, and Shift doesn't stop them.
        keys.press(KeyCode::KeyW);
        keys.press(KeyCode::ArrowLeft);
        keys.press(KeyCode::ShiftRight);
        let held: Vec<_> = map.held(&keys, &scopes).filter_map(|b| b.action).collect();
        assert_eq!(
            held,
            [Action::Fly(FlyDir::Forward), Action::Fly(FlyDir::Left)]
        );
        // Out of scope: nothing.
        assert_eq!(map.held(&keys, &[Scope::Viewport]).count(), 0);
        keys.reset_all();

        // On the web ⇧D is Duplicate (in the full UI's text scope), not
        // D's fly right; ⇧A still flies.
        let web = InputMap {
            platform: Platform::Web,
            ..map
        };
        let full = [Scope::Global, Scope::Viewport, Scope::Minimal, Scope::Text];
        keys.press(KeyCode::ShiftLeft);
        keys.press(KeyCode::KeyD);
        assert_eq!(web.held(&keys, &full).count(), 0);
        keys.press(KeyCode::KeyA);
        assert_eq!(web.held(&keys, &full).filter_map(|b| b.action).count(), 1);
    }

    #[test]
    fn the_builtin_map_is_the_spec_plus_the_fly_keys() {
        let map = InputMap::default();
        assert_eq!(map.platform, Platform::current());
        assert_eq!(map.bindings.len(), spec().bindings.len());
        assert!(map.binding("sim.play_pause").unwrap().tap);
        if cfg!(target_os = "linux") {
            assert_eq!(Platform::current(), Platform::Linux);
        }
    }
}
