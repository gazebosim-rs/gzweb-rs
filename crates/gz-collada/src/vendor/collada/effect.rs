// Modified for gzweb-rs; see `gz-collada:` markers.

use super::*;

/// The `<library_effects>` element.
///
/// See the [specification][1.4] for details.
///
/// [1.4]: https://www.khronos.org/files/collada_spec_1_4.pdf#page=277
#[derive(Default)]
pub(crate) struct LibraryEffects<'a> {
    // /// The unique identifier of this element.
    // pub(crate) id: Option<&'a str>,
    // /// The name of this element.
    // pub(crate) name: Option<&'a str>,
    pub(crate) effects: HashMap<&'a str, Effect<'a>>,
}

/// The `<effect>` element.
///
/// See the [specification][1.4] for details.
///
/// [1.4]: https://www.khronos.org/files/collada_spec_1_4.pdf#page=265
pub(crate) struct Effect<'a> {
    /// The unique identifier of this element.
    pub(crate) id: &'a str,
    // /// The name of this element.
    // pub(crate) name: Option<&'a str>,
    pub(crate) profile: ProfileCommon<'a>,
}

/// The `<profile_COMMON>` element.
///
/// See the [specification][1.4] for details.
///
/// [1.4]: https://www.khronos.org/files/collada_spec_1_4.pdf#page=301
pub(crate) struct ProfileCommon<'a> {
    // /// The unique identifier of this element.
    // pub(crate) id: Option<&'a str>,
    pub(crate) surfaces: HashMap<&'a str, Surface<'a>>,
    pub(crate) samplers: HashMap<&'a str, Sampler<'a>>,

    pub(crate) technique: Technique<'a>,
}

pub(crate) struct Technique<'a> {
    #[allow(dead_code)] // TODO
    pub(crate) ty: ShadeType,

    // Colors/Textures
    pub(crate) emission: ColorAndTexture<'a>,
    pub(crate) ambient: ColorAndTexture<'a>,
    pub(crate) diffuse: ColorAndTexture<'a>,
    pub(crate) specular: ColorAndTexture<'a>,
    pub(crate) reflective: ColorAndTexture<'a>,
    pub(crate) transparent: ColorAndTexture<'a>,
    pub(crate) has_transparency: bool,
    pub(crate) rgb_transparency: bool,
    pub(crate) invert_transparency: bool,

    pub(crate) shininess: f32,
    pub(crate) reflectivity: f32,
    pub(crate) transparency: f32,
    pub(crate) index_of_refraction: f32,

    // GOOGLEEARTH/OKINO extensions
    pub(crate) double_sided: bool,

    // FCOLLADA extensions
    pub(crate) bump: Texture<'a>,

    // MAX3D extensions
    pub(crate) wireframe: bool,
    pub(crate) faceted: bool,
}

/// The `<surface>` element.
///
/// See the [specification][1.4] for details.
///
/// [1.4]: https://www.khronos.org/files/collada_spec_1_4.pdf#page=332
pub(crate) struct Surface<'a> {
    pub(crate) init_from: Uri<'a, Image<'a>>,
}

/// The `<sampler2D>` element.
///
/// See the [specification][1.4] for details.
///
/// [1.4] https://www.khronos.org/files/collada_spec_1_4.pdf#page=312
pub(crate) struct Sampler<'a> {
    // An xs:NCName, which is the sid of a <surface>. A
    // <sampler*> is a definition of how a shader will resolve a
    // color out of a <surface>. <source> identifies the
    // <surface> to read.
    pub(crate) source: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ShadeType {
    Constant,
    Lambert,
    Phong,
    Blinn,
}

pub(crate) struct ColorAndTexture<'a> {
    pub(crate) color: Color4,
    pub(crate) texture: Texture<'a>,
}

impl ColorAndTexture<'_> {
    fn new(color: Color4) -> Self {
        Self {
            color,
            texture: Texture::default(),
        }
    }
}

#[derive(Default)]
pub(crate) struct Texture<'a> {
    pub(crate) texture: &'a str,
    // pub(crate) texcoord: &'a str,
    /// gz-collada: whether U and V repeat, from `wrapU`/`wrapV` in the
    /// texture's `<extra><technique>`; `None` (repeat) without one.
    pub(crate) wrap: Option<[bool; 2]>,
}

/// gz-collada: what `<extra>`s add to a technique.
#[derive(Default)]
struct Extra<'a> {
    bump: Texture<'a>,
    double_sided: bool,
}

// -----------------------------------------------------------------------------
// Parsing

pub(crate) fn parse_library_effects<'a>(
    cx: &mut Context<'a>,
    node: xml::Node<'a, '_>,
) -> io::Result<()> {
    debug_assert_eq!(node.tag_name().name(), "library_effects");
    // cx.library_effects.id = node.attribute("id");
    // cx.library_effects.name = node.attribute("name");

    for child in node.element_children() {
        match child.tag_name().name() {
            "effect" => {
                let effect = parse_effect(cx, child)?;
                cx.library_effects.effects.insert(effect.id, effect);
            }
            "asset" | "extra" => { /* skip */ }
            _ => return Err(error::unexpected_child_elem(child)),
        }
    }

    // The specification says <library_effects> has 1 or more <effect> elements,
    // but some exporters write empty <library_effects/> tags.

    Ok(())
}

/*
The `<effect>` element

Attributes:
- `id` (xs:ID, Required)
- `name` (xs:token, Optional)

Child elements must appear in the following order if present:
- `<asset>` (0 or 1)
- `<annotate>` (0 or more)
- `<newparam>` (0 or more)
- profile (1 or more)
    At least one profile must appear, but any number of any of
    the following profiles can be included:
    - <profile_BRIDGE>
    - <profile_CG>
    - <profile_GLES>
    - <profile_GLES2>
    - <profile_GLSL>
    - <profile_COMMON>
- `<extra>` (0 or more)
*/
fn parse_effect<'a>(cx: &mut Context<'a>, node: xml::Node<'a, '_>) -> io::Result<Effect<'a>> {
    debug_assert_eq!(node.tag_name().name(), "effect");
    let id = node.required_attribute("id")?;
    let mut profile = None;
    let mut extra = Extra::default();

    for child in node.element_children() {
        match child.tag_name().name() {
            "profile_COMMON" => profile = Some(parse_profile_common(cx, child)?),
            // gz-collada: MAX3D puts `<double_sided>` here.
            "extra" => parse_extra(child, &mut extra),
            _ => {}
        }
    }

    let mut profile = match profile {
        Some(profile) => profile,
        None => return Err(error::exactly_one_elem(node, "profile_COMMON")),
    };
    apply_extra(&mut profile.technique, extra);

    Ok(Effect {
        id,
        // name: node.attribute("name"),
        profile,
    })
}

/*
The `<profile_COMMON>` element

Attributes:
- `id` (xs:ID, Optional)

Child elements must appear in the following order if present:
- `<asset>` (0 or 1)
- `<newparam>` (0 or more)
- `<technique>` (1)
- `<extra>` (0 or more)

Child Elements for `<profile_COMMON>` / `<technique>`
Child elements must appear in the following order if present:
- `<asset>` (0 or 1)
- shader_element (0 or more)
    One of `<constant>` (FX), `<lambert>`, `<phong>`, or `<blinn>`.
- `<extra>` (0 or more)
*/
fn parse_profile_common<'a>(
    cx: &mut Context<'a>,
    node: xml::Node<'a, '_>,
) -> io::Result<ProfileCommon<'a>> {
    debug_assert_eq!(node.tag_name().name(), "profile_COMMON");
    let mut surfaces = HashMap::default();
    let mut samplers = HashMap::default();
    let mut technique = None;

    let mut extra = Extra::default();
    for child in node.element_children() {
        match child.tag_name().name() {
            "newparam" => {
                parse_newparam(cx, child, &mut surfaces, &mut samplers)?;
            }
            "technique" => {
                for t in child.element_children() {
                    let name = t.tag_name().name();
                    match name {
                        "constant" | "lambert" | "phong" | "blinn" => {
                            technique = Some(parse_technique(t, name.parse().unwrap())?);
                        }
                        "extra" => parse_extra(t, &mut extra),
                        "asset" => { /* skip */ }
                        _ => {}
                    }
                }
            }
            "extra" => parse_extra(child, &mut extra),
            "asset" => { /* skip */ }
            _ => return Err(error::unexpected_child_elem(child)),
        }
    }

    let mut technique = match technique {
        Some(technique) => technique,
        // TODO: technique maybe flatten?
        None => return Err(error::exactly_one_elem(node, "technique")),
    };
    apply_extra(&mut technique, extra);

    Ok(ProfileCommon {
        // id: node.attribute("id"),
        surfaces,
        samplers,
        technique,
    })
}

fn parse_newparam<'a>(
    _cx: &mut Context<'a>,
    node: xml::Node<'a, '_>,
    surfaces: &mut HashMap<&'a str, Surface<'a>>,
    samplers: &mut HashMap<&'a str, Sampler<'a>>,
) -> io::Result<()> {
    debug_assert_eq!(node.tag_name().name(), "newparam");
    let sid = node.required_attribute("sid")?;

    for child in node.element_children() {
        match child.tag_name().name() {
            "surface" => {
                // image ID given inside <init_from> tags
                if let Some(init) = child.child("init_from") {
                    surfaces.insert(
                        sid,
                        Surface {
                            init_from: Uri::from_id(init.trimmed_text()),
                        },
                    );
                }
            }
            "sampler2D" => {
                // surface ID is given inside <source> tags
                if let Some(source) = child.child("source") {
                    samplers.insert(
                        sid,
                        Sampler {
                            source: source.trimmed_text(),
                        },
                    );
                }
            }
            _ => return Err(error::unexpected_child_elem(child)),
        }
    }

    Ok(())
}

impl FromStr for ShadeType {
    type Err = io::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "constant" => Self::Constant,
            "lambert" => Self::Lambert,
            "phong" => Self::Phong,
            "blinn" => Self::Blinn,
            _ => bail!("unknown shade type {:?}", s),
        })
    }
}

/*
Child elements must appear in the following order if present:
- <emission> (0 or 1, fx_common_color_or_texture_type)
- <ambient> (FX) (0 or 1, fx_common_color_or_texture_type)
- <diffuse> (0 or 1, fx_common_color_or_texture_type)
- <specular> (0 or 1, fx_common_color_or_texture_type)
- <shininess> (0 or 1, fx_common_float_or_param_type)
- <reflective> (0 or 1, fx_common_color_or_texture_type)
- <reflectivity> (0 or 1, fx_common_float_or_param_type 0.0 ..= 1.0)
- <transparent> (0 or 1, fx_common_color_or_texture_type)
- <transparency> (0 or 1, fx_common_float_or_param_type 0.0 ..= 1.0)
- <index_of_refraction> (0 or 1, fx_common_float_or_param_type)
*/
fn parse_technique<'a>(node: xml::Node<'a, '_>, ty: ShadeType) -> io::Result<Technique<'a>> {
    debug_assert_eq!(node.tag_name().name().parse::<ShadeType>().unwrap(), ty);
    let mut effect = Technique::new(ty);

    for child in node.element_children() {
        let name = child.tag_name().name();
        match name {
            // fx_common_color_or_texture_type
            "emission" => {
                parse_effect_color(
                    child,
                    &mut effect.emission.color,
                    &mut effect.emission.texture,
                )?;
            }
            "ambient" => {
                parse_effect_color(
                    child,
                    &mut effect.ambient.color,
                    &mut effect.ambient.texture,
                )?;
            }
            "diffuse" => {
                parse_effect_color(
                    child,
                    &mut effect.diffuse.color,
                    &mut effect.diffuse.texture,
                )?;
            }
            "specular" => {
                parse_effect_color(
                    child,
                    &mut effect.specular.color,
                    &mut effect.specular.texture,
                )?;
            }
            "reflective" => {
                parse_effect_color(
                    child,
                    &mut effect.reflective.color,
                    &mut effect.reflective.texture,
                )?;
            }
            "transparent" => {
                effect.has_transparency = true;
                if let Some(opaque) = child.parse_attribute::<Opaque>("opaque")? {
                    effect.rgb_transparency = opaque.rgb_transparency();
                    effect.invert_transparency = opaque.invert_transparency();
                }
                parse_effect_color(
                    child,
                    &mut effect.transparent.color,
                    &mut effect.transparent.texture,
                )?;
            }

            // fx_common_float_or_param_type
            "shininess" => {
                if let Some(n) = parse_effect_float(child)? {
                    effect.shininess = n;
                }
            }
            "reflectivity" => {
                if let Some(n) = parse_effect_float(child)? {
                    effect.reflectivity = n;
                }
            }
            "transparency" => {
                if let Some(n) = parse_effect_float(child)? {
                    effect.transparency = n;
                }
            }
            "index_of_refraction" => {
                if let Some(n) = parse_effect_float(child)? {
                    effect.index_of_refraction = n;
                }
            }

            // GOOGLEEARTH/OKINO extensions
            // gz-collada: these are elements with text content; upstream read
            // a (missing) attribute of the parent and failed the parse.
            "double_sided" => {
                effect.double_sided = parse_flag(child);
            }

            // FCOLLADA extensions
            "bump" => {
                let mut dummy = [0.; 4];
                parse_effect_color(child, &mut dummy, &mut effect.bump)?;
            }

            // MAX3D extensions
            "wireframe" => {
                effect.wireframe = parse_flag(child);
            }
            "faceted" => {
                effect.faceted = parse_flag(child);
            }

            _ => {}
        }
    }

    Ok(effect)
}

/// gz-collada: `1`/`true` text content.
fn parse_flag(node: xml::Node<'_, '_>) -> bool {
    matches!(node.trimmed_text(), "1" | "true")
}

/// gz-collada: a `<bump>` (FCOLLADA, OpenCOLLADA and Blender put normal maps
/// there) and `<double_sided>` (GOOGLEEARTH/SketchUp, MAX3D) anywhere under
/// an `<extra>`. Best effort: what can't be read is ignored, because
/// `<extra>` is optional and other tools skip it.
fn parse_extra<'a>(extra: xml::Node<'a, '_>, out: &mut Extra<'a>) {
    for node in extra.descendants() {
        match node.tag_name().name() {
            "bump" if out.bump.texture.is_empty() => {
                let mut dummy = [0.; 4];
                let mut bump = Texture::default();
                if parse_effect_color(node, &mut dummy, &mut bump).is_ok() {
                    out.bump = bump;
                }
            }
            "double_sided" => out.double_sided |= parse_flag(node),
            _ => {}
        }
    }
}

/// gz-collada: the technique's own `<bump>` wins over one in an `<extra>`.
fn apply_extra<'a>(technique: &mut Technique<'a>, extra: Extra<'a>) {
    if technique.bump.texture.is_empty() {
        technique.bump = extra.bump;
    }
    technique.double_sided |= extra.double_sided;
}

/// gz-collada: three.js's texture wrapping: repeat, unless the texture's
/// `<extra><technique>` sets any of `wrapU`/`wrapV`/`repeatU`/`repeatV`/
/// `offsetU`/`offsetV`; then each axis repeats only if its `wrap` is true.
fn parse_texture_wrap(texture: xml::Node<'_, '_>) -> Option<[bool; 2]> {
    let mut wrap = None;
    let techniques = texture
        .element_children()
        .filter(|n| n.tag_name().name() == "extra")
        .flat_map(|extra| extra.element_children())
        .filter(|n| n.tag_name().name() == "technique");
    for child in techniques.flat_map(|t| t.element_children()) {
        let axis = match child.tag_name().name() {
            "wrapU" => 0,
            "wrapV" => 1,
            "repeatU" | "repeatV" | "offsetU" | "offsetV" => {
                wrap.get_or_insert([false; 2]);
                continue;
            }
            _ => continue,
        };
        wrap.get_or_insert([false; 2])[axis] = parse_wrap(child.trimmed_text());
    }
    wrap
}

/// gz-collada: `TRUE`, or a nonzero integer (three.js's `parseInt`).
fn parse_wrap(text: &str) -> bool {
    if text.eq_ignore_ascii_case("true") {
        return true;
    }
    let digits = text.strip_prefix(['-', '+']).unwrap_or(text);
    digits
        .bytes()
        .take_while(u8::is_ascii_digit)
        .any(|b| b != b'0')
}

impl Technique<'_> {
    fn new(ty: ShadeType) -> Self {
        Self {
            ty,
            emission: ColorAndTexture::new([0.0, 0.0, 0.0, 1.0]),
            ambient: ColorAndTexture::new([0.1, 0.1, 0.1, 1.0]),
            diffuse: ColorAndTexture::new([0.6, 0.6, 0.6, 1.0]),
            specular: ColorAndTexture::new([0.4, 0.4, 0.4, 1.0]),
            // refs: https://www.khronos.org/files/collada_spec_1_5.pdf#page=250
            transparent: ColorAndTexture::new([1.0, 1.0, 1.0, 1.0]),
            reflective: ColorAndTexture::new([0.0, 0.0, 0.0, 1.0]),
            shininess: 10.0,
            index_of_refraction: 1.0,
            reflectivity: 0.0,
            // refs: https://www.khronos.org/files/collada_spec_1_5.pdf#page=250
            transparency: 1.0,
            has_transparency: false,
            rgb_transparency: false,
            invert_transparency: false,
            double_sided: false,
            bump: Texture::default(),
            wireframe: false,
            faceted: false,
        }
    }
}

// Attributes:
// Only <transparent> has an attribute
// - `opaque` (Enumeration, Optional)
//
// Child Elements:
// Note: Exactly one of the child elements `<color>`, `<param>`, or
// `<texture>` must appear. They are mutually exclusive.
// - `<color>`
// - `<param>` (reference)
// - `<texture>`
//
// See also fx_common_color_or_texture_type in specification.
fn parse_effect_color<'a>(
    node: xml::Node<'a, '_>,
    color: &mut Color4,
    texture: &mut Texture<'a>,
) -> io::Result<()> {
    for child in node.element_children() {
        match child.tag_name().name() {
            "color" => {
                // gz-collada: RGB or RGBA (alpha 1), like three.js.
                let content = xml::comma_to_period(child.trimmed_text());
                let mut rgba = [0.0, 0.0, 0.0, 1.0];
                let mut n = 0;
                for word in content.split(xml::is_whitespace).filter(|w| !w.is_empty()) {
                    let value = rgba.get_mut(n).and_then(|v| {
                        *v = float::parse(word.as_bytes())?;
                        Some(())
                    });
                    if value.is_none() {
                        n = 0;
                        break;
                    }
                    n += 1;
                }
                if n < 3 {
                    bail!(
                        "expected 3 or 4 floats in <{}> element ({})",
                        child.tag_name().name(),
                        child.text_location(),
                    );
                }
                *color = rgba;
            }
            "texture" => {
                // gz-collada: `texcoord` is optional here, as in three.js.
                *texture = Texture {
                    texture: child.required_attribute("texture")?,
                    // texcoord,
                    wrap: parse_texture_wrap(child),
                };
            }
            "param" => {
                // warn!(
                //     "<{}> child element in <{}> element is unsupported ({})",
                //     child.tag_name().name(),
                //     child.parent_element().unwrap().tag_name().name(),
                //     child.node_location()
                // );
            }
            _ => {}
        }
    }
    Ok(())
}

fn parse_effect_float(node: xml::Node<'_, '_>) -> io::Result<Option<f32>> {
    let mut float = None;

    for child in node.element_children() {
        match child.tag_name().name() {
            "float" => {
                let content = xml::comma_to_period(child.trimmed_text());
                float = Some(
                    float::parse(content.as_bytes())
                        .ok_or_else(|| format_err!("error while parsing a float"))?,
                );
            }
            "param" => {
                // warn!(
                //     "<{}> child element in <{}> element is unsupported ({})",
                //     child.tag_name().name(),
                //     child.parent_element().unwrap().tag_name().name(),
                //     child.node_location()
                // );
            }
            _ => return Err(error::unexpected_child_elem(child)),
        }
    }

    Ok(float)
}

#[allow(non_camel_case_types)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum Opaque {
    A_ZERO,
    A_ONE,
    RGB_ZERO,
    RGB_ONE,
}

impl Opaque {
    fn rgb_transparency(self) -> bool {
        matches!(self, Self::RGB_ZERO | Self::RGB_ONE)
    }
    fn invert_transparency(self) -> bool {
        matches!(self, Self::RGB_ZERO | Self::A_ZERO)
    }
}

impl FromStr for Opaque {
    type Err = io::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "A_ZERO" => Self::A_ZERO,
            "A_ONE" => Self::A_ONE,
            "RGB_ZERO" => Self::RGB_ZERO,
            "RGB_ONE" => Self::RGB_ONE,
            _ => bail!("unknown opaque type {:?}", s),
        })
    }
}
