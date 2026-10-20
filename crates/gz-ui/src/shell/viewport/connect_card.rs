//! The no-world card (spec §7.10; DECISIONS.md D-33): "Connect to a
//! Gazebo server" with the address field, Connect and a status line, over
//! the viewport area until a world loads, then faded out over 160 ms. The
//! banner's and the card's buttons connect from here.

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui_widgets::Activate;
use gz_bevy::connection::{GzReconnect, GzStatus};
use gz_core::transport::Status;

use crate::assets::brand::logo;
use crate::assets::fonts::text;
use crate::shell::connection::{ConnectionView, CurrentPhase, Phase};
use crate::shell::format;
use crate::theme::{
    elevation, motion, radius, ColorRole, Fill, Ink, Opacity, Shadow, Stroke, Theme, TypeStyle,
    SPACE,
};
use crate::widgets::focus::Hiding;
use crate::widgets::layout::{column, row, set_shown};
use crate::widgets::text::set_text;
use crate::widgets::text_field::TextFieldInput;
use crate::widgets::{button, rounded, text_field, ButtonVariant, TextSubmit};

/// The connect card's width (§7.10).
pub(crate) const CARD_W: f32 = 420.0;

/// Banner and card buttons.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConnectButton {
    /// "Retry now".
    Retry,
    /// "Connect to another server…": shows the card.
    Elsewhere,
    /// The card's Connect.
    Connect,
}

/// The no-world card.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct ConnectCard;

/// The card's address field.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct AddressField;

/// The card's status line.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct CardStatus;

/// A fade-out in progress (§7.10: 160 ms), seconds left. The card also
/// gets [`Hiding`], so focus in it goes at once (DECISIONS.md D-52).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct Fading(pub f32);

/// An entity's own opacity before the card's fade, restored when the fade
/// ends or is interrupted (a world that drops again while the card fades).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct PreFade(pub f32);

/// The fade's length: `panel_slide_ms` (160).
pub(crate) fn fade_secs() -> f32 {
    motion::PANEL_SLIDE_MS / 1000.0
}

pub(super) fn connect_card(theme: &Theme) -> impl Bundle {
    let t = theme.clone();
    (
        ConnectCard,
        Node {
            width: px(CARD_W),
            align_items: AlignItems::Stretch,
            padding: UiRect::all(px(SPACE[10])),
            border: UiRect::all(px(crate::theme::border::HAIRLINE)),
            border_radius: rounded(radius::XL),
            ..column(SPACE[6])
        },
        Fill(ColorRole::Surface),
        Stroke(ColorRole::Line),
        elevation::MENU.box_shadow(),
        Shadow(ColorRole::Shadow),
        Children::spawn(SpawnWith(move |c: &mut ChildSpawner| {
            c.spawn(row(SPACE[6])).with_children(|r| {
                r.spawn(logo(40.0));
                r.spawn(text(
                    TypeStyle::new(crate::theme::FontFamily::Ui, 17.0, 600),
                    ColorRole::Text,
                    "Connect to a Gazebo server",
                ));
            });
            c.spawn(text(
                TypeStyle::BODY,
                ColorRole::Text2,
                "Address of a running Gazebo websocket server",
            ));
            c.spawn(row(SPACE[4])).with_children(|r| {
                r.spawn((text_field(&t, "Server address", ""), AddressField));
                r.spawn((
                    button(&t, "Connect", ButtonVariant::Normal),
                    ConnectButton::Connect,
                ));
            });
            c.spawn((text(TypeStyle::SMALL, ColorRole::Text2, ""), CardStatus));
        })),
    )
}

/// The card shows while no world has loaded, or when asked for while
/// lost; it fades out once a world loads. Its status line follows the
/// connection. Runs when the connection changes.
pub(crate) fn card_state(
    phase: Res<CurrentPhase>,
    status: Option<Res<GzStatus>>,
    view: Res<ConnectionView>,
    mut cards: Query<(Entity, &mut Node, Option<&Fading>), With<ConnectCard>>,
    mut lines: Query<&mut Text, With<CardStatus>>,
    mut commands: Commands,
) {
    let changed =
        phase.is_changed() || view.is_changed() || status.as_ref().is_some_and(Res::is_changed);
    if !changed {
        return;
    }
    let lost = matches!(phase.0, Phase::Lost { .. });
    let card = phase.0.no_world() || (lost && view.card_requested);
    for (e, mut node, fading) in &mut cards {
        let shown = node.display != Display::None;
        if card {
            if fading.is_some() {
                // `fade_card` puts the opacities back.
                commands.entity(e).remove::<(Fading, Hiding)>();
            }
            set_shown(&mut node, true);
        } else if shown && fading.is_none() {
            commands.entity(e).insert((Fading(fade_secs()), Hiding));
        }
    }
    let line = card_status(phase.0, status.as_deref());
    for mut t in &mut lines {
        set_text(&mut t, &line);
    }
}

/// The card's status line (§7.10).
pub(crate) fn card_status(phase: Phase, status: Option<&GzStatus>) -> String {
    let Some(s) = status else {
        return "Not connected".into();
    };
    let addr = format::address(&s.url);
    let error = match &s.status {
        Status::Error(e) => Some(e.as_str()),
        _ => None,
    };
    match phase {
        Phase::Live => format!("Connected to {addr}"),
        Phase::Loading => format!("Connected to {addr} · loading the world…"),
        Phase::Connecting if s.attempting => format!("Connecting to {addr}…"),
        Phase::Connecting => format!(
            "Couldn't connect{} · retrying (attempt {})",
            error.map_or_else(String::new, |e| format!(": {e}")),
            s.attempts + 1
        ),
        Phase::Lost { .. } => format!("Lost connection to {addr}"),
        Phase::Idle => match error {
            Some(e) => format!("Couldn't connect: {e}"),
            None => "Not connected".into(),
        },
    }
}

/// Anything with a colour role (what the fade's opacity applies to).
type WithRole = Or<(With<Fill>, With<Stroke>, With<Ink>, With<Shadow>)>;

/// The card's node, its fade, and whether it has opacities to put back.
type CardFade = (
    Entity,
    &'static mut Node,
    Option<&'static mut Fading>,
    Has<PreFade>,
);

/// Fades the card out (after the widgets' own looks, so the fade wins),
/// then hides it. Each faded entity keeps its own opacity in [`PreFade`]
/// (a disabled widget's 40%), which comes back when the fade ends or is
/// interrupted.
pub(crate) fn fade_card(
    time: Res<Time<Real>>,
    mut cards: Query<CardFade, With<ConnectCard>>,
    children: Query<&Children>,
    roles: Query<(), WithRole>,
    mut opacities: Query<(Option<&mut Opacity>, Option<&PreFade>)>,
    mut commands: Commands,
) {
    for (card, mut node, fading, faded) in &mut cards {
        let (alpha, done) = match fading {
            Some(mut fading) => {
                fading.0 -= time.delta_secs();
                let done = fading.0 <= 0.0;
                let alpha = (!done).then(|| (fading.0 / fade_secs()).clamp(0.0, 1.0));
                (alpha, done)
            }
            // Not fading: put back what an interrupted fade left.
            None if faded => (None, false),
            None => continue,
        };
        let mut stack = vec![card];
        while let Some(e) = stack.pop() {
            if roles.contains(e) {
                if let Ok((opacity, pre)) = opacities.get_mut(e) {
                    fade_one(&mut commands, e, opacity, pre, alpha);
                }
            }
            if let Ok(c) = children.get(e) {
                stack.extend(c.iter());
            }
        }
        if done {
            set_shown(&mut node, false);
            commands.entity(card).remove::<(Fading, Hiding)>();
        }
    }
}

/// One entity's opacity at `alpha` of its own (`None`: its own again).
fn fade_one(
    commands: &mut Commands,
    e: Entity,
    opacity: Option<Mut<Opacity>>,
    pre: Option<&PreFade>,
    alpha: Option<f32>,
) {
    let base = pre.map_or_else(|| opacity.as_deref().map_or(1.0, |o| o.0), |p| p.0);
    let want = Opacity(alpha.map_or(base, |a| base * a));
    match opacity {
        Some(mut o) => {
            o.set_if_neq(want);
        }
        None => {
            commands.entity(e).insert(want);
        }
    }
    match (alpha, pre) {
        (Some(_), None) => {
            commands.entity(e).insert(PreFade(base));
        }
        (None, Some(_)) => {
            commands.entity(e).remove::<PreFade>();
        }
        _ => {}
    }
}

/// Fills the card's address with the server's address once.
pub(crate) fn prefill_address(
    status: Option<Res<GzStatus>>,
    fields: Query<&Children, With<AddressField>>,
    mut inputs: Query<&mut EditableText, With<TextFieldInput>>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let Some(status) = status else { return };
    for children in &fields {
        for &c in children {
            if let Ok(mut text) = inputs.get_mut(c) {
                text.editor.set_text(&status.url);
                *done = true;
            }
        }
    }
}

fn address(
    fields: &Query<&Children, With<AddressField>>,
    inputs: &Query<&mut EditableText, With<TextFieldInput>>,
) -> Option<String> {
    fields
        .iter()
        .flat_map(|c| c.iter())
        .find_map(|c| inputs.get(c).ok())
        .map(|t| t.value().to_string().trim().to_string())
        .filter(|s| !s.is_empty())
}

/// The address to connect to (DECISIONS.md D-55). `http://` and
/// `https://` become `ws://` and `wss://` (the websocket server shares
/// the page's scheme); a bare host gets `ws://`, or `wss://` from a
/// `secure` (https) page, which can't open `ws://` sockets, and the
/// default port when it names none. A URL with its own scheme keeps its
/// port, or lack of one.
pub(crate) fn normalize_address(s: &str, secure: bool) -> String {
    let s = s.trim();
    if let Some((scheme, rest)) = s.split_once("://") {
        let scheme = match scheme.to_ascii_lowercase().as_str() {
            "http" => "ws".to_string(),
            "https" => "wss".to_string(),
            other => other.to_string(),
        };
        return format!("{scheme}://{rest}");
    }
    let scheme = if secure { "wss" } else { "ws" };
    let (host, path) = s.split_at(s.find('/').unwrap_or(s.len()));
    // An IPv6 literal's colons are inside its brackets.
    let has_port = match host.rfind(']') {
        Some(end) => host[end..].contains(':'),
        None => host.contains(':'),
    };
    if has_port {
        format!("{scheme}://{s}")
    } else {
        format!("{scheme}://{host}:{}{path}", gz_bevy::DEFAULT_PORT)
    }
}

/// The page was served over https (only ever in the browser).
fn page_is_secure() -> bool {
    #[cfg(target_arch = "wasm32")]
    return gz_bevy::view::web::page_is_secure();
    #[cfg(not(target_arch = "wasm32"))]
    false
}

/// Banner and card buttons.
pub(crate) fn on_connect_button(
    ev: On<Activate>,
    buttons: Query<&ConnectButton>,
    fields: Query<&Children, With<AddressField>>,
    inputs: Query<&mut EditableText, With<TextFieldInput>>,
    status: Option<Res<GzStatus>>,
    mut view: ResMut<ConnectionView>,
    mut out: MessageWriter<GzReconnect>,
) {
    let Ok(which) = buttons.get(ev.entity) else {
        return;
    };
    match which {
        ConnectButton::Retry => crate::shell::connection::retry(&mut out),
        ConnectButton::Elsewhere => view.card_requested = true,
        ConnectButton::Connect => {
            let url = address(&fields, &inputs).map(|a| normalize_address(&a, page_is_secure()));
            connect_to(url, status.as_deref(), &mut view, &mut out);
        }
    }
}

fn connect_to(
    url: Option<String>,
    status: Option<&GzStatus>,
    view: &mut ConnectionView,
    out: &mut MessageWriter<GzReconnect>,
) {
    let elsewhere = url
        .as_ref()
        .is_some_and(|u| status.is_none_or(|s| s.url != *u));
    if elsewhere {
        view.connect_elsewhere();
    }
    out.write(GzReconnect { url });
}

/// Enter in the address field connects.
pub(crate) fn on_address_submit(
    ev: On<TextSubmit>,
    fields: Query<(), With<AddressField>>,
    status: Option<Res<GzStatus>>,
    mut view: ResMut<ConnectionView>,
    mut out: MessageWriter<GzReconnect>,
) {
    if fields.contains(ev.entity) {
        let url = Some(ev.value.trim())
            .filter(|s| !s.is_empty())
            .map(|a| normalize_address(a, page_is_secure()));
        connect_to(url, status.as_deref(), &mut view, &mut out);
    }
}
