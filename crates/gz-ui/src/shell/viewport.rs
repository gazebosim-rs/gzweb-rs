//! The viewport area (spec §3.4, §9.3, §7.10): the 3D view's node, the
//! camera viewport synced to it, pointer and key routing, and the
//! no-world card, the disconnected banner and the dimming over the last
//! frame.

use bevy::ecs::spawn::SpawnWith;
use bevy::input_focus::InputFocus;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerId;
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy::ui_widgets::Activate;
use bevy::window::PrimaryWindow;
use gz_bevy::connection::{GzReconnect, GzStatus};
use gz_bevy::view::controls::ViewRouting;
use gz_bevy::view::ViewPlacement;
use gz_core::transport::Status;

use super::connection::{banner, ConnectionView, CurrentPhase, Phase};
use super::{format, pass_through, Region};
use crate::brand::logo;
use crate::fonts::text;
use crate::icons::{icon, IconId};
use crate::theme::{
    elevation, motion, radius, ColorRole, Fill, Ink, Opacity, Shadow, Stroke, Theme, TypeStyle,
    SPACE,
};
use crate::widgets::text_field::TextFieldInput;
use crate::widgets::{button, rounded, spacer, text_field, ButtonVariant, TextSubmit};

/// The 3D view's placeholder node: the main camera renders into its rect.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct ViewNode;

/// The view had the last pointer press (its keys apply while the pointer
/// is elsewhere). True at start, so keys work before the first click.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewFocus(pub bool);

impl Default for ViewFocus {
    fn default() -> Self {
        ViewFocus(true)
    }
}

/// The last frame shows at 70% while disconnected (§7.10).
pub const DIM_OPACITY: f32 = 0.3;
/// The banner's height (§7.10).
pub const BANNER_H: f32 = 36.0;
/// The connect card's width (§7.10).
pub const CARD_W: f32 = 420.0;

/// The dimming over the last frame.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct DimOverlay;

/// The disconnected banner and its parts.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct DisconnectBanner;

#[derive(Component, Clone, Copy, Debug, Default)]
pub struct BannerText;

#[derive(Component, Clone, Copy, Debug, Default)]
pub struct BannerIcon;

/// Banner and card buttons.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectButton {
    /// "Retry now".
    Retry,
    /// "Connect to another server…": shows the card.
    Elsewhere,
    /// The card's Connect.
    Connect,
}

/// The no-world card.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct ConnectCard;

/// The card's address field.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct AddressField;

/// The card's status line.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct CardStatus;

/// A fade-out in progress (§7.10: 160 ms), seconds left.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Fading(pub f32);

/// The fade's length: `panel_slide_ms` (160).
pub fn fade_secs() -> f32 {
    motion::PANEL_SLIDE_MS / 1000.0
}

/// The viewport area.
pub fn viewport_area(theme: &Theme) -> impl Bundle {
    let t = theme.clone();
    (
        Region::ViewportArea,
        Node {
            flex_grow: 1.0,
            min_width: px(0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        // `bg` (§2.1) shows while no world is loaded; the view covers it.
        Fill(ColorRole::Bg),
        pass_through(),
        Children::spawn(SpawnWith(move |a: &mut ChildSpawner| {
            a.spawn((
                ViewNode,
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    right: px(0),
                    top: px(0),
                    bottom: px(0),
                    ..default()
                },
                pass_through(),
            ));
            a.spawn((
                DimOverlay,
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    right: px(0),
                    top: px(0),
                    bottom: px(0),
                    display: Display::None,
                    ..default()
                },
                Fill(ColorRole::Bg),
                Opacity(DIM_OPACITY),
                pass_through(),
            ));
            a.spawn(disconnect_banner(&t));
            a.spawn(connect_card(&t));
        })),
    )
}

fn disconnect_banner(theme: &Theme) -> impl Bundle {
    let t = theme.clone();
    (
        DisconnectBanner,
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            height: px(BANNER_H),
            padding: UiRect::horizontal(px(SPACE[6])),
            column_gap: px(SPACE[4]),
            align_items: AlignItems::Center,
            display: Display::None,
            ..default()
        },
        Fill(ColorRole::WarnSoft),
        Children::spawn(SpawnWith(move |b: &mut ChildSpawner| {
            b.spawn((icon(IconId::Warning, 15.0, ColorRole::WarnText), BannerIcon));
            b.spawn((
                text(
                    TypeStyle::new(crate::theme::FontFamily::Ui, 12.0, 500),
                    ColorRole::WarnText,
                    "",
                ),
                BannerText,
                TextLayout::no_wrap(),
            ));
            b.spawn(spacer());
            b.spawn((
                button(&t, "Connect to another server…", ButtonVariant::Normal),
                ConnectButton::Elsewhere,
            ));
            b.spawn((
                button(&t, "Retry now", ButtonVariant::Normal),
                ConnectButton::Retry,
            ));
        })),
    )
}

fn connect_card(theme: &Theme) -> impl Bundle {
    let t = theme.clone();
    (
        ConnectCard,
        Node {
            width: px(CARD_W),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            row_gap: px(SPACE[6]),
            padding: UiRect::all(px(SPACE[10])),
            border: UiRect::all(px(crate::theme::border::HAIRLINE)),
            border_radius: rounded(radius::XL),
            ..default()
        },
        Fill(ColorRole::Surface),
        Stroke(ColorRole::Line),
        elevation::MENU.box_shadow(),
        Shadow(ColorRole::Shadow),
        Children::spawn(SpawnWith(move |c: &mut ChildSpawner| {
            c.spawn(Node {
                column_gap: px(SPACE[6]),
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|r| {
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
            c.spawn(Node {
                column_gap: px(SPACE[4]),
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|r| {
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

/// The card's status line (§7.10).
pub fn card_status(phase: Phase, status: Option<&GzStatus>) -> String {
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

/// The physical rect of a laid-out node.
pub fn node_rect(node: &ComputedNode, transform: &UiGlobalTransform) -> Rect {
    Rect::from_center_size(transform.translation, node.size())
}

/// Places the 3D view on the view node (spec §9.3): its rect, in window
/// logical pixels, goes to `gz-bevy`, which renders the main camera into a
/// texture of the node's physical size in the same frame (after UI layout,
/// before culling); the node shows that texture.
pub fn place_view(
    views: Query<(&ComputedNode, &UiGlobalTransform), With<ViewNode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut placement: ResMut<ViewPlacement>,
) {
    let Ok((node, transform)) = views.single() else {
        return;
    };
    let scale = windows.single().map_or(1.0, Window::scale_factor);
    let r = node_rect(node, transform);
    let rect = (r.width() >= 1.0 && r.height() >= 1.0)
        .then(|| Rect::from_corners(r.min / scale, r.max / scale));
    if placement.rect != rect || placement.scale != scale {
        placement.rect = rect;
        placement.scale = scale;
    }
}

/// The view node shows the camera's texture once it exists, and nothing
/// (the area's `bg`) while no world is loaded.
pub fn show_view_image(
    placement: Res<ViewPlacement>,
    phase: Res<CurrentPhase>,
    mut views: Query<(Entity, Option<&mut ImageNode>, &mut Visibility), With<ViewNode>>,
    mut commands: Commands,
) {
    let shown = if phase.0.no_world() {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    for (_, _, mut v) in &mut views {
        v.set_if_neq(shown);
    }
    let Some(handle) = &placement.image else {
        return;
    };
    for (e, image, _) in &mut views {
        match image {
            Some(mut image) => {
                if image.image != *handle {
                    image.image = handle.clone();
                }
            }
            None => {
                commands.entity(e).insert(ImageNode::new(handle.clone()));
            }
        }
    }
}

/// Pointer and key routing (spec §3.4, §9.3): the camera gets the pointer
/// only over the view's node and not over any UI node; the view keeps the
/// keys after a press in it, and loses them to a press elsewhere.
#[allow(clippy::too_many_arguments)]
pub fn route_pointer(
    windows: Query<&Window, With<PrimaryWindow>>,
    views: Query<(&ComputedNode, &UiGlobalTransform), With<ViewNode>>,
    nodes: Query<(), (With<ComputedNode>, Without<ViewNode>)>,
    hover: Option<Res<HoverMap>>,
    buttons: Res<ButtonInput<MouseButton>>,
    focus: Option<Res<InputFocus>>,
    mut view_focus: ResMut<ViewFocus>,
    mut routing: ResMut<ViewRouting>,
) {
    let cursor = windows
        .single()
        .ok()
        .and_then(|w| w.physical_cursor_position());
    let in_view = match (cursor, views.single()) {
        (Some(c), Ok((node, t))) => node_rect(node, t).contains(c),
        _ => false,
    };
    let over_ui = hover
        .as_ref()
        .and_then(|h| h.get(&PointerId::Mouse))
        .is_some_and(|hits| hits.keys().any(|e| nodes.contains(*e)));
    let pointer = in_view && !over_ui;
    if buttons.get_just_pressed().next().is_some() {
        view_focus.0 = pointer;
    }
    // A widget that takes keyboard focus takes the keys too (the window
    // itself holds the focus at start).
    if focus.is_some_and(|f| f.is_changed() && f.get().is_some_and(|e| nodes.contains(e))) {
        view_focus.0 = false;
    }
    routing.set_if_neq(ViewRouting {
        pointer,
        keys: pointer || view_focus.0,
    });
}

fn set_display(node: &mut Node, show: bool) {
    let d = if show { Display::Flex } else { Display::None };
    if node.display != d {
        node.display = d;
    }
}

/// The banner, the dimming and the card, from the connection phase.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn overlays(
    phase: Res<CurrentPhase>,
    status: Option<Res<GzStatus>>,
    view: Res<ConnectionView>,
    time: Res<Time<Real>>,
    mut banners: Query<(&mut Node, &mut Fill), (With<DisconnectBanner>, Without<ConnectCard>)>,
    mut dims: Query<
        &mut Node,
        (
            With<DimOverlay>,
            Without<DisconnectBanner>,
            Without<ConnectCard>,
        ),
    >,
    mut banner_texts: Query<(&mut Text, &mut Ink), With<BannerText>>,
    mut banner_icons: Query<&mut Ink, (With<BannerIcon>, Without<BannerText>)>,
    mut elsewhere: Query<
        (&ConnectButton, &mut Node),
        (
            Without<DisconnectBanner>,
            Without<DimOverlay>,
            Without<ConnectCard>,
        ),
    >,
    mut cards: Query<(Entity, &mut Node, Option<&Fading>), With<ConnectCard>>,
    mut card_status_q: Query<&mut Text, (With<CardStatus>, Without<BannerText>)>,
    mut commands: Commands,
) {
    let status = status.as_deref();
    let now = time.elapsed_secs_f64();
    let b = banner(phase.0, status, now);
    for (mut node, mut fill) in &mut banners {
        set_display(&mut node, b.is_some());
        if let Some(b) = &b {
            fill.set_if_neq(Fill(b.roles().0));
        }
    }
    if let Some(b) = &b {
        let ink = b.roles().1;
        for (mut t, mut i) in &mut banner_texts {
            if t.0 != b.text {
                t.0.clone_from(&b.text);
            }
            i.set_if_neq(Ink(ink));
        }
        for mut i in &mut banner_icons {
            i.set_if_neq(Ink(ink));
        }
    }
    let long = b.as_ref().is_some_and(|b| b.long);
    for (which, mut node) in &mut elsewhere {
        if *which == ConnectButton::Elsewhere {
            set_display(&mut node, long);
        }
    }
    let lost = matches!(phase.0, Phase::Lost { .. });
    for mut node in &mut dims {
        set_display(&mut node, lost);
    }
    let card = phase.0.no_world() || (lost && view.card_requested);
    for (e, mut node, fading) in &mut cards {
        let shown = node.display != Display::None;
        if card {
            if fading.is_some() {
                commands.entity(e).remove::<Fading>();
            }
            set_display(&mut node, true);
        } else if shown && fading.is_none() {
            commands.entity(e).insert(Fading(fade_secs()));
        }
    }
    let line = card_status(phase.0, status);
    for mut t in &mut card_status_q {
        if t.0 != line {
            t.0.clone_from(&line);
        }
    }
}

/// Fades the card out (after the widgets' own looks, so the fade wins),
/// then hides it.
#[allow(clippy::type_complexity)]
pub fn fade_card(
    time: Res<Time<Real>>,
    mut cards: Query<(Entity, &mut Node, &mut Fading), With<ConnectCard>>,
    children: Query<&Children>,
    roles: Query<(), Or<(With<Fill>, With<Stroke>, With<Ink>, With<Shadow>)>>,
    mut opacities: Query<&mut Opacity>,
    mut commands: Commands,
) {
    for (card, mut node, mut fading) in &mut cards {
        fading.0 -= time.delta_secs();
        let done = fading.0 <= 0.0;
        let alpha = if done {
            1.0
        } else {
            (fading.0 / fade_secs()).clamp(0.0, 1.0)
        };
        let mut stack = vec![card];
        while let Some(e) = stack.pop() {
            if roles.contains(e) {
                match opacities.get_mut(e) {
                    Ok(mut o) => {
                        o.set_if_neq(Opacity(alpha));
                    }
                    Err(_) => {
                        commands.entity(e).insert(Opacity(alpha));
                    }
                }
            }
            if let Ok(c) = children.get(e) {
                stack.extend(c.iter());
            }
        }
        if done {
            set_display(&mut node, false);
            commands.entity(card).remove::<Fading>();
        }
    }
}

/// Fills the card's address with the server's address once.
pub fn prefill_address(
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

/// The address to connect to: `ws://` added when no scheme is given.
pub fn normalize_address(s: &str) -> String {
    let s = s.trim();
    if s.contains("://") {
        s.to_string()
    } else {
        format!("ws://{s}")
    }
}

/// Banner and card buttons.
#[allow(clippy::too_many_arguments)]
pub fn on_connect_button(
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
        ConnectButton::Retry => super::connection::retry(&mut out),
        ConnectButton::Elsewhere => view.card_requested = true,
        ConnectButton::Connect => {
            let url = address(&fields, &inputs).map(|a| normalize_address(&a));
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
pub fn on_address_submit(
    ev: On<TextSubmit>,
    fields: Query<(), With<AddressField>>,
    status: Option<Res<GzStatus>>,
    mut view: ResMut<ConnectionView>,
    mut out: MessageWriter<GzReconnect>,
) {
    if fields.contains(ev.entity) {
        let url = Some(ev.value.trim())
            .filter(|s| !s.is_empty())
            .map(normalize_address);
        connect_to(url, status.as_deref(), &mut view, &mut out);
    }
}

pub(crate) fn build(app: &mut App) {
    app.add_systems(
        Update,
        (prefill_address, show_view_image).in_set(crate::GzUiSystems::Behaviour),
    )
    .add_observer(on_connect_button)
    .add_observer(on_address_submit);
}
