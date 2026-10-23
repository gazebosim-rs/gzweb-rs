//! The selection outline over each view (spec §6.4, §9.3): a node on the
//! view's node draws `selection_outline` around the silhouettes the view's
//! mask camera renders ([`gz_bevy::view::outline`]): 2px around the
//! selection, 1px at 50% around the entity hovered in the Scene tree.

use bevy::asset::embedded_asset;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::{Shader, ShaderRef};
use gz_bevy::camera::MainCamera;
use gz_bevy::view::outline::OutlineMask;
use gz_bevy::view::{ViewId, ViewPlacement};

use crate::shell::regions::pass_through;
use crate::shell::viewport::ViewCameras;
use crate::theme::{ColorRole, Theme};
use crate::widgets::layout::display;

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
    /// Where the overlay sits on the mask: its top left and size, in uv.
    pub(crate) rect: Vec4,
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
        .add_systems(
            PostUpdate,
            outline_overlay
                .after(gz_bevy::view::outline::OutlineRectSystems)
                .before(bevy::ui::UiSystems::Layout),
        );
}

/// A hidden overlay; [`outline_overlay`] gives it its material.
pub(crate) fn outline() -> impl Bundle {
    (
        OutlineOverlay,
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            ..default()
        },
        pass_through(),
    )
}

/// Each view's overlay shows while anything is outlined, over the
/// outlined meshes' part of the view (with room for the outline; all of it
/// until they have bounds), with the view's mask, the theme's colour and
/// the widths at the window's scale.
#[allow(clippy::type_complexity)]
pub(crate) fn outline_overlay(
    theme: Res<Theme>,
    views: ViewCameras,
    cameras: Query<(&ViewId, &ViewPlacement, &OutlineMask), With<MainCamera>>,
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
    let colour = theme.color(ColorRole::SelectionOutline).to_linear();
    for (e, mut node, material) in &mut overlays {
        let id = views.id_of(e);
        let Some((_, placement, mask)) = cameras.iter().find(|c| Some(*c.0) == id) else {
            continue;
        };
        let size = placement.physical_size().unwrap_or(UVec2::ONE).as_vec2();
        let scale = placement.scale.max(f32::EPSILON);
        let view = placement
            .rect
            .map_or(Vec2::ONE, |r| r.size().max(Vec2::ONE));
        let pad = SELECTED_WIDTH.max(HOVERED_WIDTH) + 2.0;
        let area = mask
            .rect
            .map(|r| {
                r.inflate(pad)
                    .intersect(Rect::from_corners(Vec2::ZERO, view))
            })
            .filter(|r| !r.is_empty())
            .unwrap_or(Rect::from_corners(Vec2::ZERO, view));
        let params = OutlineParams {
            selected: colour.to_vec4(),
            hovered: colour.with_alpha(colour.alpha * HOVERED_ALPHA).to_vec4(),
            texel_widths: Vec4::new(
                1.0 / size.x,
                1.0 / size.y,
                SELECTED_WIDTH * scale,
                HOVERED_WIDTH * scale,
            ),
            rect: (area.min / view).extend(0.0).extend(0.0)
                + Vec4::new(0.0, 0.0, area.width() / view.x, area.height() / view.y),
        };
        let want = display(mask.active && mask.image.is_some());
        if node.display != want {
            node.display = want;
        }
        let place = (
            px(area.min.x),
            px(area.min.y),
            px(area.width()),
            px(area.height()),
        );
        if (node.left, node.top, node.width, node.height) != place {
            (node.left, node.top, node.width, node.height) = place;
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
