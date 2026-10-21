//! The selection outline over the view (spec §6.4, §9.3): a node on the
//! view's node draws `selection_outline` around the silhouettes gz-bevy's
//! mask camera renders ([`gz_bevy::view::outline`]): 2px around the
//! selection, 1px at 50% around the entity hovered in the Scene tree.

use bevy::asset::embedded_asset;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::{Shader, ShaderRef};
use gz_bevy::view::outline::OutlineMask;
use gz_bevy::view::ViewPlacement;

use crate::shell::regions::pass_through;
use crate::theme::{ColorRole, Theme};
use crate::widgets::layout::{display, overlay};

const SHADER: &str = "embedded://gz_ui/shell/viewport/outline/outline.wesl";

/// The selection outline's width in logical pixels.
pub(crate) const SELECTED_WIDTH: f32 = 2.0;
/// The hover outline's (spec §6.4: 1px at 50% alpha).
pub(crate) const HOVERED_WIDTH: f32 = 1.0;
pub(crate) const HOVERED_ALPHA: f32 = 0.5;

/// The node the outline draws on, over the view's image.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct OutlineOverlay;

#[derive(Asset, AsBindGroup, TypePath, Debug, Clone, Default)]
pub(crate) struct OutlineMaterial {
    #[uniform(0)]
    pub(crate) params: OutlineParams,
    #[texture(1)]
    #[sampler(2)]
    pub(crate) mask: Option<Handle<Image>>,
}

#[derive(ShaderType, Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct OutlineParams {
    pub(crate) selected: Vec4,
    pub(crate) hovered: Vec4,
    /// One mask texel in uv; the two widths in texels.
    pub(crate) texel_widths: Vec4,
}

impl UiMaterial for OutlineMaterial {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
}

pub(crate) fn build(app: &mut App) {
    // Without a renderer (unit tests) there are no shaders to draw it with.
    if app.world().contains_resource::<Assets<Shader>>() {
        embedded_asset!(app, "outline.wesl");
        app.add_plugins(UiMaterialPlugin::<OutlineMaterial>::default());
    }
    app.add_plugins(gz_bevy::view::outline::OutlinePlugin)
        .add_systems(PostUpdate, outline_overlay);
}

/// A hidden overlay; [`outline_overlay`] gives it its material.
pub(crate) fn outline() -> impl Bundle {
    (
        OutlineOverlay,
        Node {
            display: Display::None,
            ..overlay()
        },
        pass_through(),
    )
}

/// The overlay shows while anything is outlined, with the mask, the
/// theme's colour and the widths at the window's scale.
#[allow(clippy::type_complexity)]
pub(crate) fn outline_overlay(
    mask: Res<OutlineMask>,
    theme: Res<Theme>,
    placement: Res<ViewPlacement>,
    materials: Option<ResMut<Assets<OutlineMaterial>>>,
    mut overlays: Query<
        (Entity, &mut Node, Option<&MaterialNode<OutlineMaterial>>),
        With<OutlineOverlay>,
    >,
    mut commands: Commands,
) {
    let Some(mut materials) = materials else {
        return;
    };
    let size = placement.physical_size().unwrap_or(UVec2::ONE).as_vec2();
    let scale = placement.scale.max(f32::EPSILON);
    let colour = theme.color(ColorRole::SelectionOutline).to_linear();
    let params = OutlineParams {
        selected: colour.to_vec4(),
        hovered: colour.with_alpha(colour.alpha * HOVERED_ALPHA).to_vec4(),
        texel_widths: Vec4::new(
            1.0 / size.x,
            1.0 / size.y,
            SELECTED_WIDTH * scale,
            HOVERED_WIDTH * scale,
        ),
    };
    for (e, mut node, material) in &mut overlays {
        let want = display(mask.active && mask.image.is_some());
        if node.display != want {
            node.display = want;
        }
        let stale = material
            .and_then(|m| materials.get(&m.0))
            .map(|m| m.params != params || m.mask != mask.image);
        match (stale, material) {
            (Some(true), Some(m)) => {
                if let Some(mut m) = materials.get_mut(&m.0) {
                    m.params = params;
                    m.mask = mask.image.clone();
                }
            }
            (Some(false), _) => {}
            _ => {
                let handle = materials.add(OutlineMaterial {
                    params,
                    mask: mask.image.clone(),
                });
                commands.entity(e).insert(MaterialNode(handle));
            }
        }
    }
}
