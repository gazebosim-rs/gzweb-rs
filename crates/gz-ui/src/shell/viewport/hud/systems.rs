//! The HUD pills' state: the scale label follows the view's orthographic
//! scale, the target chip the camera's mode.

use bevy::prelude::*;
use bevy::ui::ComputedNode;
use bevy::ui_widgets::Activate;
use gz_bevy::spawn::GzEntity;
use gz_bevy::view::input::InputMap;
use gz_bevy::view::{ortho_height, CameraMode, ViewCommand, ViewOp};

use super::{scale_bar, scale_text, ReleaseTarget, ScaleBar, ScaleLabel, TargetChip, TargetIcon};
use crate::assets::icons::{Icon, IconId};
use crate::shell::viewport::ViewCameras;
use crate::widgets::layout::set_shown;
use crate::widgets::shortcut::shortcut_label;
use crate::widgets::text::set_text;
use crate::widgets::Tooltip;

/// The scale label: shown in an orthographic view, with a 1-2-5 bar and
/// its length.
pub(crate) fn scale_label_state(
    views: ViewCameras,
    nodes: Query<&ComputedNode>,
    mut labels: Query<(Entity, &mut Node, &Children), With<ScaleLabel>>,
    mut bars: Query<&mut Node, (With<ScaleBar>, Without<ScaleLabel>)>,
    mut texts: Query<&mut Text>,
) {
    for (label, mut node, children) in &mut labels {
        let height = views.of(label).and_then(|(_, _, _, p)| ortho_height(p));
        // The view's height in UI pixels, which the bar is drawn in.
        let ui_h = views
            .node_of(label)
            .and_then(|n| nodes.get(n).ok())
            .map_or(0.0, |n| n.size().y * n.inverse_scale_factor());
        let bar = height.and_then(|h| scale_bar(h / ui_h.max(1.0)));
        set_shown(&mut node, bar.is_some());
        let Some((metres, length)) = bar else {
            continue;
        };
        for &c in children {
            if let Ok(mut b) = bars.get_mut(c) {
                let w = px(length.round());
                if b.width != w {
                    b.width = w;
                }
            }
            if let Ok(mut t) = texts.get_mut(c) {
                set_text(&mut t, &scale_text(metres));
            }
        }
    }
}

/// What the chip shows for a mode: its icon, close tooltip and binding.
fn chip_look(mode: CameraMode) -> (IconId, &'static str, &'static str) {
    match mode {
        CameraMode::Track(_) => (IconId::Track, "Stop tracking", "view.track"),
        _ => (IconId::Follow, "Stop following", "view.follow"),
    }
}

/// An entity's name for the chip and the Camera menu.
pub(crate) fn entity_name(names: &Query<(Option<&GzEntity>, Option<&Name>)>, e: Entity) -> String {
    match names.get(e) {
        Ok((Some(g), _)) => g.name.clone(),
        Ok((None, Some(n))) => n.to_string(),
        _ => "target".into(),
    }
}

/// The target chip: shown while following or tracking, with the mode's
/// icon and the target's name.
#[allow(clippy::too_many_arguments)]
pub(crate) fn target_chip_state(
    views: ViewCameras,
    map: Option<Res<InputMap>>,
    names: Query<(Option<&GzEntity>, Option<&Name>)>,
    mut chips: Query<(Entity, &mut Node, &Children), With<TargetChip>>,
    mut icons: Query<&mut Icon, With<TargetIcon>>,
    mut texts: Query<&mut Text>,
    mut tips: Query<&mut Tooltip, With<ReleaseTarget>>,
) {
    for (chip, mut node, children) in &mut chips {
        let mode = views.of(chip).map_or(CameraMode::Free, |(_, v, ..)| v.mode);
        let Some(target) = mode.target() else {
            set_shown(&mut node, false);
            continue;
        };
        set_shown(&mut node, true);
        let (id, title, binding) = chip_look(mode);
        let name = entity_name(&names, target);
        for &c in children {
            if let Ok(mut icon) = icons.get_mut(c) {
                if icon.id != id {
                    icon.id = id;
                }
            }
            if let Ok(mut t) = texts.get_mut(c) {
                set_text(&mut t, &name);
            }
            if let Ok(mut tip) = tips.get_mut(c) {
                let keys = map.as_deref().and_then(|m| shortcut_label(m, binding));
                if tip.title != title || tip.shortcut != keys {
                    tip.title = title.into();
                    tip.shortcut = keys;
                }
            }
        }
    }
}

/// The chip's × frees the camera.
pub(crate) fn on_release(
    ev: On<Activate>,
    buttons: Query<(), With<ReleaseTarget>>,
    views: ViewCameras,
    mut out: MessageWriter<ViewCommand>,
) {
    if !buttons.contains(ev.entity) {
        return;
    }
    if let Some((view, ..)) = views.of(ev.entity) {
        out.write(ViewCommand {
            view,
            op: ViewOp::SetMode(CameraMode::Free),
        });
    }
}
