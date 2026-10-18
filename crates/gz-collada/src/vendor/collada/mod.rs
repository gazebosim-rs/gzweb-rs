//! [COLLADA] (.dae) parser.
//!
//! [COLLADA]: https://en.wikipedia.org/wiki/COLLADA
//!
//! Modified for gzweb-rs; see `gz-collada:` markers. (gz-collada: the
//! document types are `pub(crate)` so `crate::assemble` can read them.)

#![allow(clippy::wildcard_imports)] // TODO
#![allow(clippy::many_single_char_names)]

mod effect;
mod error;
mod geometry;
mod image;
pub(crate) mod iter;
mod material;
mod scene;

use std::{
    cmp,
    collections::{BTreeMap, HashMap},
    fmt, io,
    marker::PhantomData,
    ops,
    path::Path,
    str::{self, FromStr},
};

pub(crate) use self::{effect::*, geometry::*, image::*, material::*, scene::*};
use crate::vendor::{
    common,
    utils::{
        float, hex,
        utf16::decode_string,
        xml::{self, XmlNodeExt},
    },
    Color4,
};

/// Parses COLLADA text into a node-tree scene. (gz-collada: replaces
/// mesh-loader's `from_slice`; the assembly is rewritten in
/// `crate::assemble`.)
pub(crate) fn parse(bytes: &[u8]) -> io::Result<crate::Scene> {
    let text = decode_string(bytes)?;
    // gz-collada: roxmltree recurses per element level.
    crate::validate::nesting(&text).map_err(crate::vendor::error::invalid_data)?;
    let xml = xml::Document::parse(&text).map_err(crate::vendor::error::invalid_data)?;
    let collada = Document::parse(&xml)?;
    crate::assemble::assemble(&collada).map_err(crate::vendor::error::invalid_data)
}

// Inspired by gltf-json's `Get` trait.
/// Helper trait for retrieving top-level objects by a universal identifier.
pub(crate) trait Get<T> {
    type Target;

    fn get(&self, uri: &T) -> Option<&Self::Target>;
}

macro_rules! impl_get_by_uri {
    ($ty:ty, $($field:ident).*) => {
        impl<'a> Get<Uri<'a, $ty>> for Document<'a> {
            type Target = $ty;

            fn get(&self, index: &Uri<'a, $ty>) -> Option<&Self::Target> {
                self.$($field).*.get(&*index.0)
            }
        }
    };
}

impl_get_by_uri!(Accessor<'a>, library_geometries.accessors);
impl_get_by_uri!(ArrayData<'a>, library_geometries.array_data);
impl_get_by_uri!(Effect<'a>, library_effects.effects);
impl_get_by_uri!(Geometry<'a>, library_geometries.geometries);
impl_get_by_uri!(Image<'a>, library_images.images);
impl_get_by_uri!(Material<'a>, library_materials.materials);

pub(crate) struct Uri<'a, T>(&'a str, PhantomData<fn() -> T>);

impl<'a, T> Uri<'a, T> {
    fn parse(url: &'a str) -> io::Result<Self> {
        // skipping the leading #, hopefully the remaining text is the accessor ID only
        if let Some(id) = url.strip_prefix('#') {
            Ok(Self(id, PhantomData))
        } else {
            Err(format_err!("unknown reference format {:?}", url))
        }
    }

    fn from_id(id: &'a str) -> Self {
        Self(id, PhantomData)
    }

    fn cast<U>(self) -> Uri<'a, U> {
        Uri(self.0, PhantomData)
    }

    pub(crate) fn as_str(&self) -> &'a str {
        self.0
    }
}

impl<T> PartialEq for Uri<'_, T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<T> Eq for Uri<'_, T> {}

impl<T, S> PartialEq<S> for Uri<'_, T>
where
    S: ?Sized + AsRef<str>,
{
    fn eq(&self, other: &S) -> bool {
        self.0 == other.as_ref()
    }
}

impl<T> PartialEq<Uri<'_, T>> for str {
    #[inline]
    fn eq(&self, other: &Uri<'_, T>) -> bool {
        self == other.0
    }
}

impl<T> PartialEq<Uri<'_, T>> for &str {
    #[inline]
    fn eq(&self, other: &Uri<'_, T>) -> bool {
        *self == other.0
    }
}

trait ColladaXmlNodeExt<'a, 'input> {
    fn parse_url<T>(&self, name: &str) -> io::Result<Uri<'a, T>>;
    // fn parse_url_opt<T>(&self, name: &str) -> io::Result<Option<Uri<'a, T>>>;
}

impl<'a, 'input> ColladaXmlNodeExt<'a, 'input> for xml::Node<'a, 'input> {
    fn parse_url<T>(&self, name: &str) -> io::Result<Uri<'a, T>> {
        let url = self.required_attribute(name)?;
        Uri::parse(url).map_err(|e| {
            format_err!(
                "{} in {} attribute of <{}> element at {}",
                e,
                name,
                self.tag_name().name(),
                self.attr_value_location(name),
            )
        })
    }

    // fn parse_url_opt<T>(&self, name: &str) -> io::Result<Option<Uri<'a, T>>> {
    //     if let Some(url) = self.attribute(name) {
    //         Uri::parse(url).map(Some).map_err(|e| {
    //             format_err!(
    //                 "{} in {} attribute of <{}> element at {}",
    //                 e,
    //                 name,
    //                 self.tag_name().name(),
    //                 self.attr_value_location(name),
    //             )
    //         })
    //     } else {
    //         Ok(None)
    //     }
    // }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Version {
    minor: u32,
    patch: u32,
}

impl Version {
    const MIN: Self = Self::new(4, 0);

    const fn new(minor: u32, patch: u32) -> Self {
        Self { minor, patch }
    }
    fn is_1_4(self) -> bool {
        self >= Self::new(4, 0) && self < Self::new(5, 0)
    }
}

impl FromStr for Version {
    type Err = io::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        (|| {
            let mut digits = s.splitn(3, '.');
            let major = digits.next()?;
            if major != "1" {
                return None;
            }
            let minor = digits.next()?.parse().ok()?;
            let patch = digits.next()?.parse().ok()?;
            Some(Self::new(minor, patch))
        })()
        .ok_or_else(|| format_err!("unrecognized version format {:?}", s))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "1.{}.{}", self.minor, self.patch)
    }
}

pub(crate) struct Context<'a> {
    version: Version,
    asset: Asset,
    library_effects: LibraryEffects<'a>,
    library_geometries: LibraryGeometries<'a>,
    library_images: LibraryImages<'a>,
    library_materials: LibraryMaterials<'a>,
    library_visual_scenes: LibraryVisualScenes<'a>,
    scene: Scene<'a>,
}

pub(crate) struct Document<'a> {
    pub(crate) asset: Asset,
    pub(crate) library_effects: LibraryEffects<'a>,
    pub(crate) library_geometries: LibraryGeometries<'a>,
    pub(crate) library_images: LibraryImages<'a>,
    pub(crate) library_materials: LibraryMaterials<'a>,
    pub(crate) library_visual_scenes: LibraryVisualScenes<'a>,
    pub(crate) scene: Scene<'a>,
}

impl<'a> Document<'a> {
    /*
    The `<COLLADA>` element.

    Attributes:
    - `version` (Required)
    - `xmlns` (xs:anyURI)
    - `base` (xs:anyURI)

    Child elements must appear in the following order if present:
    - `<asset>` (1)
    - library_element (0 or more)
        Any quantity and combination of any library elements can appear in any order:
        - `<library_animation_clips>`
        - `<library_animations>`
        - `<library_articulated_systems>` (in Kinematics)
        - `<library_cameras>`
        - `<library_controllers>`
        - `<library_effects>` (in FX)
        - `<library_force_fields>` (in Physics)
        - `<library_formulas>`
        - `<library_geometries>`
        - `<library_images>` (in FX)
        - `<library_joints>` (in Kinematics)
        - `<library_kinematics_models>` (in Kinematics)
        - `<library_kinematics_scenes>` (in Kinematics)
        - `<library_lights>`
        - `<library_materials>` (in FX)
        - `<library_nodes>`
        - `<library_physics_materials>` (in Physics)
        - `<library_physics_models>` (in Physics)
        - `<library_physics_scenes>` (in Physics)
        - `<library_visual_scenes>`
    - `<scene>` (0 or 1)
    - `<extra>` (0 or more)
    */
    fn parse(doc: &'a xml::Document<'_>) -> io::Result<Self> {
        let node = doc.root_element();
        if node.tag_name().name() != "COLLADA" {
            bail!("root element is not <COLLADA>");
        }

        let version: Version = node.required_attribute("version")?.parse()?;
        if version < Version::MIN {
            bail!("collada schema version {} is not supported", version);
        };
        // debug!("collada schema version is {}", version);

        let mut cx = Context {
            version,
            asset: Asset {
                unit: DEFAULT_UNIT_SIZE,
                up_axis: crate::UpAxis::Y,
            },
            library_effects: LibraryEffects::default(),
            library_geometries: LibraryGeometries::default(),
            library_images: LibraryImages::default(),
            library_materials: LibraryMaterials::default(),
            library_visual_scenes: LibraryVisualScenes::default(),
            scene: Scene::default(),
        };

        for node in node.element_children() {
            match node.tag_name().name() {
                "library_effects" => {
                    parse_library_effects(&mut cx, node)?;
                }
                "library_geometries" => {
                    parse_library_geometries(&mut cx, node)?;
                }
                "library_images" => {
                    parse_library_images(&mut cx, node)?;
                }
                "library_materials" => {
                    parse_library_materials(&mut cx, node)?;
                }
                "library_visual_scenes" => {
                    parse_library_visual_scenes(&mut cx, node)?;
                }
                // gz-collada
                "library_nodes" => {
                    parse_library_nodes(&mut cx, node)?;
                }
                "asset" => {
                    cx.asset = Asset::parse(node)?;
                }
                "scene" => {
                    cx.scene = parse_scene(&mut cx, node)?;
                }
                _name => {
                    // debug!("ignored <{}> element", name);
                }
            }
        }

        Ok(Self {
            asset: cx.asset,
            library_effects: cx.library_effects,
            library_geometries: cx.library_geometries,
            library_images: cx.library_images,
            library_materials: cx.library_materials,
            library_visual_scenes: cx.library_visual_scenes,
            scene: cx.scene,
        })
    }

    pub(crate) fn get<T>(&self, url: &T) -> Option<&<Self as Get<T>>::Target>
    where
        Self: Get<T>,
    {
        <Self as Get<T>>::get(self, url)
    }
}

// gz-collada: no `Index` impl; every lookup goes through `get`, which
// can fail, instead of panicking on a dangling reference.

const DEFAULT_UNIT_SIZE: f32 = 1.;

/// The `<asset>` element of the `<COLLADA>` element.
pub(crate) struct Asset {
    // <unit meter="<float>" name="..."/>
    pub(crate) unit: f32,
    /// gz-collada: `<up_axis>` (`Y_UP` when absent, per the spec).
    pub(crate) up_axis: crate::UpAxis,
}

impl Asset {
    fn parse(node: xml::Node<'_, '_>) -> io::Result<Self> {
        debug_assert_eq!(node.tag_name().name(), "asset");

        let mut unit = None;
        let mut up_axis = crate::UpAxis::Y;
        for child in node.element_children() {
            match child.tag_name().name() {
                "unit" => {
                    // gz-collada: a zero, negative, NaN or unreadable unit
                    // falls back to 1 instead of collapsing the model.
                    unit = child
                        .attribute("meter")
                        .and_then(|v| xml::comma_to_period(v).parse::<f32>().ok())
                        .filter(|u| u.is_finite() && *u > 0.0);
                }
                // gz-collada
                "up_axis" => {
                    up_axis = match child.trimmed_text() {
                        "X_UP" => crate::UpAxis::X,
                        "Z_UP" => crate::UpAxis::Z,
                        _ => crate::UpAxis::Y,
                    };
                }
                _ => { /* ignore */ }
            }
        }

        Ok(Self {
            unit: unit.unwrap_or(DEFAULT_UNIT_SIZE),
            up_axis,
        })
    }
}

pub(crate) struct Source<'a> {
    // Required
    id: &'a str,
    // // Optional
    // name: Option<&'a str>,

    // 0 or 1
    array_element: Option<ArrayElement<'a>>,
    // 0 or 1
    accessor: Option<Accessor<'a>>,
}

impl<'a> Source<'a> {
    /*
    The `<source>` element (core)

    Attributes:
    - `id` (xs:ID, Required)
    - `name` (xs:token, Optional)

    Child elements must appear in the following order if present:
    - `<asset>` (0 or 1)
    - array_element (0 or 1)
        Can be one of:
        - `<bool_array>`
        - `<float_array>`
        - `<IDREF_array>`
        - `<int_array>`
        - `<Name_array>`
        - `<SIDREF_array>`
        - `<token_array>`
    - `<technique_common>` (0 or 1)
    - `<technique>` (core) (0 or more)
    */
    fn parse(node: xml::Node<'a, '_>) -> io::Result<Self> {
        debug_assert_eq!(node.tag_name().name(), "source");
        let id = node.required_attribute("id")?;
        let mut array_element = None;
        let mut accessor = None;

        for child in node.element_children() {
            match child.tag_name().name() {
                "float_array" | "IDREF_array" | "Name_array" => {
                    array_element = Some(parse_array_element(child)?);
                }
                "technique_common" => {
                    for technique in child.element_children() {
                        match technique.tag_name().name() {
                            "accessor" => {
                                accessor = Some(Accessor::parse(technique)?);
                            }
                            _ => return Err(error::unexpected_child_elem(technique)),
                        }
                    }
                }
                "bool_array" | "int_array" | "SIDREF_array" | "token_array" => {
                    // warn!(
                    //     "ignored array element {} ({})",
                    //     child.tag_name().name(),
                    //     child.node_location()
                    // );
                }
                "asset" | "technique" => { /* skip */ }
                _ => return Err(error::unexpected_child_elem(child)),
            }
        }

        Ok(Self {
            id,
            // name: node.attribute("name"),
            array_element,
            accessor,
        })
    }
}

pub(crate) struct ArrayElement<'a> {
    // Required
    id: &'a str,
    // // Required
    // count: u32,
    data: ArrayData<'a>,
}

fn parse_array_element<'a>(node: xml::Node<'a, '_>) -> io::Result<ArrayElement<'a>> {
    let name = node.tag_name().name();
    let is_string_array = name == "IDREF_array" || name == "Name_array";

    let id = node.required_attribute("id")?;
    let count: u32 = node.parse_required_attribute("count")?;
    let mut content = node.trimmed_text();

    // some exporters write empty data arrays, but we need to conserve them anyways because others might reference them
    if content.is_empty() {
        let data = if is_string_array {
            ArrayData::String(vec![])
        } else {
            ArrayData::Float(vec![])
        };
        return Ok(ArrayElement {
            id,
            // count,
            data,
        });
    }

    // gz-collada: `count` is untrusted; every value takes at least two bytes.
    let capacity = (count as usize).min(content.len() / 2 + 1);
    if is_string_array {
        let mut values = Vec::with_capacity(capacity);
        for _ in 0..count {
            if content.is_empty() {
                bail!(
                    "expected more values while reading <{}> contents at {}",
                    node.tag_name().name(),
                    node.node_location()
                );
            }

            // gz-collada: upstream never advanced `n` here and hung.
            let n = content.find(xml::is_whitespace).unwrap_or(content.len());
            values.push(&content[..n]);

            content = xml::trim_start(content.get(n..).unwrap_or_default());
        }

        Ok(ArrayElement {
            id,
            // count,
            data: ArrayData::String(values),
        })
    } else {
        let mut values = Vec::with_capacity(capacity);
        let content = xml::comma_to_period(content);
        // TODO: include in parse_float_array_exact?
        let map_err = |e| {
            format_err!(
                "{e} in <{}> element ({})",
                node.tag_name().name(),
                node.text_location(),
            )
        };
        for res in xml::parse_float_array_exact(&content, count as usize) {
            let value = res.map_err(map_err)?;
            values.push(value);
        }

        Ok(ArrayElement {
            id,
            // count,
            data: ArrayData::Float(values),
        })
    }
}

/// Data source array.
pub(crate) enum ArrayData<'a> {
    /// <float_array>
    Float(Vec<f32>),
    /// <IDREF_array> or <Name_array>
    String(
        #[allow(dead_code)] // TODO
        Vec<&'a str>,
    ),
    // /// <int_array>
    // Int(Vec<i32>),
    // /// <bool_array>
    // Bool(Vec<bool>),
}

impl ArrayData<'_> {
    // fn is_float(&self) -> bool {
    //     matches!(self, Self::Float(..))
    // }
    // fn is_string(&self) -> bool {
    //     matches!(self, Self::String(..))
    // }

    pub(crate) fn as_float(&self) -> Option<&[f32]> {
        match self {
            Self::Float(v) => Some(v),
            Self::String(..) => None,
        }
    }
    // fn as_string(&self) -> Option<&[&'a str]> {
    //     match self {
    //         Self::String(v) => Some(v),
    //         _ => None,
    //     }
    // }

    // fn len(&self) -> usize {
    //     match self {
    //         Self::Float(v) => v.len(),
    //         Self::String(v) => v.len(),
    //         Self::Int(v) => v.len(),
    //         Self::Bool(v) => v.len(),
    //     }
    // }
    // fn is_empty(&self) -> bool {
    //     match self {
    //         Self::Float(v) => v.is_empty(),
    //         Self::String(v) => v.is_empty(),
    //         Self::Int(v) => v.is_empty(),
    //         Self::Bool(v) => v.is_empty(),
    //     }
    // }
}

pub(crate) struct Accessor<'a> {
    // Required
    pub(crate) count: u32,
    // // Optional
    // offset: u32,
    // Required
    pub(crate) source: Uri<'a, ArrayData<'a>>,
    // Optional
    pub(crate) stride: u32,

    // 0 or more
    pub(crate) params: Vec<Param<'a>>,
}

impl<'a> Accessor<'a> {
    /*
    The `<accessor>` element

    Attributes:
    - `count` (uint_type, Required)
    - `offset` (uint_type, Optional, default: 0)
    - `source` (xs:anyURI, Required)
    - `stride` (uint_type, Optional, default: 1)

    Child elements:
    - `<param>` (0 or more)
    */
    fn parse(node: xml::Node<'a, '_>) -> io::Result<Self> {
        debug_assert_eq!(node.tag_name().name(), "accessor");
        let count: u32 = node.parse_required_attribute("count")?;
        let source = node.parse_url("source")?;
        let _offset: u32 = node.parse_attribute("offset")?.unwrap_or(0);
        let stride: u32 = node.parse_attribute("stride")?.unwrap_or(1);
        let mut params = vec![];

        for child in node.element_children() {
            match child.tag_name().name() {
                "param" => {
                    params.push(Param::parse(child)?);
                }
                _ => return Err(error::unexpected_child_elem(child)),
            }
        }

        Ok(Self {
            count,
            // offset,
            source,
            stride,
            params,
        })
    }
}

/// The `<param>` element (data flow).
///
/// See the specifications ([1.4], [1.5]) for details.
///
/// [1.4]: https://www.khronos.org/files/collada_spec_1_4.pdf#page=125
/// [1.5]: https://www.khronos.org/files/collada_spec_1_5.pdf#page=144
pub(crate) struct Param<'a> {
    // /// The name of this element.
    // name: Option<&'a str>,
    // /// The scoped identifier of this element.
    // sid: Option<&'a str>,
    // Required
    pub(crate) ty: &'a str,
    // // Optional
    // semantic: Option<&'a str>,
}

impl<'a> Param<'a> {
    /*
    The `<param>` element (data flow)
    Attributes:
    - `name` (xs:token, Optional)
    - `sid` (sid_type, Optional)
    - `type` (xs:NMTOKEN, Required)
    - `semantic` (xs:NMTOKEN, Optional)

    Child elements: None
    */
    fn parse(node: xml::Node<'a, '_>) -> io::Result<Self> {
        let ty = node.required_attribute("type")?;
        // let name = node.attribute("name");
        // let sid = node.attribute("sid");
        // let semantic = node.attribute("semantic");
        if let Some(child) = node.element_children().next() {
            return Err(error::unexpected_child_elem(child));
        }
        Ok(Self {
            // name,
            // sid,
            ty,
            // semantic,
        })
    }
}

/// The `<input>` element (shared).
///
/// See the specifications ([1.4], [1.5]) for details.
///
/// [1.4]: https://www.khronos.org/files/collada_spec_1_4.pdf#page=73
/// [1.5]: https://www.khronos.org/files/collada_spec_1_5.pdf#page=87
pub(crate) struct SharedInput<'a, T = Accessor<'a>> {
    // Required
    pub(crate) offset: u32,
    // Required
    pub(crate) semantic: InputSemantic,
    // Required
    pub(crate) source: Uri<'a, T>,
    // Optional
    pub(crate) set: u32,
}

impl<'a, T> SharedInput<'a, T> {
    /*
    The `<input>` element (shared)

    Attributes:
    - `offset` (uint_type, Required)
    - `semantic` (xs:NMTOKEN, Required)
    - `source` (uri_fragment_type, Required)
    - `set` (uint_type, Optional)

    Child elements: None
    */
    fn parse(node: xml::Node<'a, '_>) -> io::Result<Self> {
        debug_assert_eq!(node.tag_name().name(), "input");
        let semantic = node.parse_required_attribute("semantic")?;
        let source = node.parse_url("source")?;
        let offset: u32 = node.parse_required_attribute("offset")?;
        let set: u32 = node.parse_attribute("set")?.unwrap_or(0);
        if let Some(child) = node.element_children().next() {
            return Err(error::unexpected_child_elem(child));
        }
        Ok(Self {
            offset,
            semantic,
            source,
            set,
        })
    }

    fn cast<U>(self) -> SharedInput<'a, U> {
        SharedInput {
            offset: self.offset,
            semantic: self.semantic,
            source: self.source.cast(),
            set: self.set,
        }
    }
}

/// The `<input>` element (unshared).
///
/// See the specifications ([1.4], [1.5]) for details.
///
/// [1.4]: https://www.khronos.org/files/collada_spec_1_4.pdf#page=76
/// [1.5]: https://www.khronos.org/files/collada_spec_1_5.pdf#page=90
pub(crate) struct UnsharedInput<'a> {
    // Required
    pub(crate) semantic: InputSemantic,
    // Required
    pub(crate) source: Uri<'a, Accessor<'a>>,
}

impl<'a> UnsharedInput<'a> {
    /*
    The `<input>` element (unshared)

    Attributes:
    - `semantic` (xs:NMTOKEN, Required)
    - `source` (uri_fragment_type, Required)

    Child elements: None
    */
    fn parse(node: xml::Node<'a, '_>) -> io::Result<Self> {
        debug_assert_eq!(node.tag_name().name(), "input");
        let semantic = node.parse_required_attribute("semantic")?;
        let source = node.parse_url("source")?;
        if let Some(child) = node.element_children().next() {
            return Err(error::unexpected_child_elem(child));
        }
        Ok(Self { semantic, source })
    }
}

/// The value of the `semantic` attribute in the `<input>` element.
///
/// See the specifications ([1.4], [1.5]) for details.
///
/// [1.4]: https://www.khronos.org/files/collada_spec_1_4.pdf#page=74
/// [1.5]: https://www.khronos.org/files/collada_spec_1_5.pdf#page=88
#[allow(non_camel_case_types, clippy::upper_case_acronyms)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum InputSemantic {
    /// Geometric binormal (bitangent) vector.
    BINORMAL,
    /// Color coordinate vector. Color inputs are RGB (float3_type).
    COLOR,
    /// Continuity constraint at the control vertex (CV).
    CONTINUITY,
    /// Raster or MIP-level input.
    IMAGE,
    /// Sampler input.
    INPUT,
    /// Tangent vector for preceding control point.
    IN_TANGENT,
    /// Sampler interpolation type.
    INTERPOLATION,
    /// Inverse of local-to-world matrix.
    INV_BIND_MATRIX,
    /// Skin influence identifier.
    JOINT,
    /// Number of piece-wise linear approximation steps to use for the spline segment that follows this CV.
    LINEAR_STEPS,
    /// Morph targets for mesh morphing.
    MORPH_TARGET,
    /// Weights for mesh morphing
    MORPH_WEIGHT,
    /// Normal vector
    NORMAL,
    /// Sampler output.
    OUTPUT,
    /// Tangent vector for succeeding control point.
    OUT_TANGENT,
    /// Geometric coordinate vector.
    POSITION,
    /// Geometric tangent vector.
    TANGENT,
    /// Texture binormal (bitangent) vector.
    TEXBINORMAL,
    /// Texture coordinate vector.
    TEXCOORD,
    /// Texture tangent vector.
    TEXTANGENT,
    /// Generic parameter vector.
    UV,
    /// Mesh vertex.
    VERTEX,
    /// Skin influence weighting value.
    WEIGHT,
}

impl FromStr for InputSemantic {
    type Err = io::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "BINORMAL" => Self::BINORMAL,
            "COLOR" => Self::COLOR,
            "CONTINUITY" => Self::CONTINUITY,
            "IMAGE" => Self::IMAGE,
            "INPUT" => Self::INPUT,
            "IN_TANGENT" => Self::IN_TANGENT,
            "INTERPOLATION" => Self::INTERPOLATION,
            "INV_BIND_MATRIX" => Self::INV_BIND_MATRIX,
            "JOINT" => Self::JOINT,
            "LINEAR_STEPS" => Self::LINEAR_STEPS,
            "MORPH_TARGET" => Self::MORPH_TARGET,
            "MORPH_WEIGHT" => Self::MORPH_WEIGHT,
            "NORMAL" => Self::NORMAL,
            "OUTPUT" => Self::OUTPUT,
            "OUT_TANGENT" => Self::OUT_TANGENT,
            "POSITION" => Self::POSITION,
            "TANGENT" => Self::TANGENT,
            "TEXBINORMAL" => Self::TEXBINORMAL,
            "TEXCOORD" => Self::TEXCOORD,
            "TEXTANGENT" => Self::TEXTANGENT,
            "UV" => Self::UV,
            "VERTEX" => Self::VERTEX,
            "WEIGHT" => Self::WEIGHT,
            _ => bail!("unknown input semantic {:?}", s),
        })
    }
}
