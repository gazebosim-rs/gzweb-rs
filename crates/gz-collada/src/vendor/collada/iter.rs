// Modified for gzweb-rs; see `gz-collada:` markers.
//
// gz-collada: nothing here panics on a bad document. A source that doesn't
// resolve, isn't a `<float_array>` or overruns its array yields nothing,
// and an index past the end of `<p>` ends the iteration. `crate::validate`
// turns those cases into errors before these iterators run.

use std::{
    iter::{self, FusedIterator},
    ops::Range,
    slice,
};

use crate::vendor::{
    collada as ast,
    common::{Face, Vec2, Vec3},
};

#[derive(Clone)]
pub(crate) struct Mesh<'a> {
    pub(crate) doc: &'a ast::Document<'a>,
    pub(crate) xml: &'a ast::Geometry<'a>,
}

impl<'a> Mesh<'a> {
    pub(crate) fn primitives(&self) -> Primitives<'a> {
        Primitives {
            mesh: self.clone(),
            iter: self.xml.mesh.primitives.iter().enumerate(),
        }
    }
}

pub(crate) struct Primitives<'a> {
    mesh: Mesh<'a>,
    iter: iter::Enumerate<slice::Iter<'a, ast::Primitive<'a>>>,
}

impl<'a> Iterator for Primitives<'a> {
    type Item = Primitive<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|(_index, xml)| Primitive {
            mesh: self.mesh.clone(),
            xml,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl ExactSizeIterator for Primitives<'_> {}
impl FusedIterator for Primitives<'_> {}

#[derive(Clone)]
pub(crate) struct Primitive<'a> {
    pub(crate) mesh: Mesh<'a>,
    pub(crate) xml: &'a ast::Primitive<'a>,
}

/// gz-collada: the `stride`-sized chunks of the float array behind
/// `source`, or `None` when it doesn't resolve, isn't a `<float_array>`, has
/// fewer than `min` float params, or its accessor overruns the array. With
/// `counted`, only the accessor's `count` chunks (upstream ignored `count`
/// for texture coordinates, as three.js does).
fn floats<'a>(
    doc: &'a ast::Document<'a>,
    source: &ast::Uri<'a, ast::Accessor<'a>>,
    min: usize,
    counted: bool,
) -> Option<slice::ChunksExact<'a, f32>> {
    let acc = doc.get(source)?;
    let data = doc.get(&acc.source)?.as_float()?;
    let stride = acc.stride as usize;
    if stride < min || acc.params.len() < min || acc.params.iter().any(|p| p.ty != "float") {
        return None;
    }
    let len = (acc.count as usize).checked_mul(stride)?;
    if len > data.len() {
        return None;
    }
    Some(if counted { &data[..len] } else { data }.chunks_exact(stride))
}

impl<'a> Primitive<'a> {
    // gz-collada: VERTEX always means this mesh's own `<vertices>` (a mesh
    // has exactly one), as in three.js; upstream compared the input's id and
    // hit `todo!()` when it differed.
    fn vertices(&self) -> &'a ast::VerticesInputs<'a> {
        &self.mesh.xml.mesh.vertices.input
    }

    pub(crate) fn positions(&self) -> Positions<'a> {
        if self.xml.input.is_none() {
            return Positions(None);
        }
        // ["X", "Y", "Z"]
        Positions(floats(
            self.mesh.doc,
            &self.vertices().position.source,
            3,
            true,
        ))
    }

    pub(crate) fn normals(&self) -> Normals<'a> {
        let Some(input) = &self.xml.input else {
            return Positions(None);
        };
        let source = match (&input.normal, &self.vertices().normal) {
            (Some(normal), _) => &normal.source,
            (None, Some(normal)) => &normal.source,
            (None, None) => return Positions(None),
        };
        // ["X", "Y", "Z"]
        Positions(floats(self.mesh.doc, source, 3, true))
    }

    pub(crate) fn texcoords(&self, set: usize) -> Texcoords<'a> {
        let Some(input) = &self.xml.input else {
            return Texcoords(None);
        };
        let source = match (input.texcoord.get(set), &self.vertices().texcoord) {
            (Some(texcoord), _) => &texcoord.source,
            (None, Some(texcoord)) if set == 0 => &texcoord.source,
            _ => return Texcoords(None),
        };
        // ["S", "T"] or ["S", "T", "P"]
        Texcoords(floats(self.mesh.doc, source, 2, false))
    }

    pub(crate) fn colors(&self) -> Colors<'a> {
        let Some(input) = &self.xml.input else {
            return Positions(None);
        };
        let source = match (&input.color, &self.vertices().color) {
            (Some(color), _) => &color.source,
            (None, Some(color)) => &color.source,
            (None, None) => return Positions(None),
        };
        // ["R", "G", "B"] or ["R", "G", "B", "A"]
        Positions(floats(self.mesh.doc, source, 3, true))
    }

    fn vertex_indices_inner(&self, offset: u32) -> IndicesInner<'a> {
        let polygons = |strip| IndicesInner::Polygons {
            offset,
            indices: &self.xml.p,
            stride: self.xml.stride,
            vcount: self.xml.vcount.iter(),
            index: 0,
            current: None,
            strip,
        };
        match self.xml.ty {
            ast::PrimitiveType::Lines | ast::PrimitiveType::LineStrips => IndicesInner::Skip,
            ast::PrimitiveType::Polylist
            | ast::PrimitiveType::Polygons
            | ast::PrimitiveType::TriFans => polygons(false),
            // gz-collada: strips used to share the fan code.
            ast::PrimitiveType::TriStrips => polygons(true),
            ast::PrimitiveType::Triangles => IndicesInner::Triangles {
                offset,
                indices: self.xml.p.chunks(self.xml.stride.max(1) as usize),
            },
        }
    }

    fn vertex_indices_size(&self) -> u32 {
        match self.xml.ty {
            ast::PrimitiveType::Polylist
            | ast::PrimitiveType::Polygons
            | ast::PrimitiveType::TriStrips
            | ast::PrimitiveType::TriFans => self
                .xml
                .vcount
                .iter()
                .map(|count| count.saturating_sub(2))
                .sum(),
            ast::PrimitiveType::Triangles => self.xml.count,
            ast::PrimitiveType::Lines => 0,
            ast::PrimitiveType::LineStrips => 0,
        }
    }

    fn indices(&self, offset: Option<u32>) -> VertexIndices<'a> {
        match offset {
            Some(offset) => VertexIndices {
                remaining: self.vertex_indices_size(),
                inner: self.vertex_indices_inner(offset),
            },
            None => VertexIndices::none(),
        }
    }

    pub(crate) fn vertex_indices(&self) -> VertexIndices<'a> {
        self.indices(self.xml.input.as_ref().map(|input| input.vertex.offset))
    }

    pub(crate) fn normal_indices(&self) -> VertexIndices<'a> {
        let offset = self
            .xml
            .input
            .as_ref()
            .and_then(|input| match &input.normal {
                Some(normal) => Some(normal.offset),
                None => self.vertices().normal.as_ref().map(|_| input.vertex.offset),
            });
        self.indices(offset)
    }

    pub(crate) fn texcoord_indices(&self, set: usize) -> VertexIndices<'a> {
        let offset = self
            .xml
            .input
            .as_ref()
            .and_then(|input| match input.texcoord.get(set) {
                Some(texcoord) => Some(texcoord.offset),
                None if set == 0 => self
                    .vertices()
                    .texcoord
                    .as_ref()
                    .map(|_| input.vertex.offset),
                None => None,
            });
        self.indices(offset)
    }

    pub(crate) fn color_indices(&self) -> VertexIndices<'a> {
        let offset = self
            .xml
            .input
            .as_ref()
            .and_then(|input| match &input.color {
                Some(color) => Some(color.offset),
                None => self.vertices().color.as_ref().map(|_| input.vertex.offset),
            });
        self.indices(offset)
    }
}

pub(crate) struct Positions<'a>(Option<slice::ChunksExact<'a, f32>>);

impl Iterator for Positions<'_> {
    type Item = Vec3;

    fn next(&mut self) -> Option<Self::Item> {
        let v = self.0.as_mut()?.next()?;
        Some([v[0], v[1], v[2]])
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match &self.0 {
            Some(iter) => iter.size_hint(),
            None => (0, Some(0)),
        }
    }
}

impl ExactSizeIterator for Positions<'_> {}
impl FusedIterator for Positions<'_> {}

pub(crate) type Normals<'a> = Positions<'a>;
pub(crate) type Colors<'a> = Positions<'a>;

pub(crate) struct Texcoords<'a>(Option<slice::ChunksExact<'a, f32>>);

impl Iterator for Texcoords<'_> {
    type Item = Vec2;

    fn next(&mut self) -> Option<Self::Item> {
        let v = self.0.as_mut()?.next()?;
        Some([v[0], v[1]])
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match &self.0 {
            Some(iter) => iter.size_hint(),
            None => (0, Some(0)),
        }
    }
}

impl ExactSizeIterator for Texcoords<'_> {}
impl FusedIterator for Texcoords<'_> {}

pub(crate) struct VertexIndices<'a> {
    remaining: u32,
    inner: IndicesInner<'a>,
}

enum IndicesInner<'a> {
    /// gz-collada: polylists, polygons, fans and strips, one `vcount` entry
    /// per polygon (or fan, or strip).
    Polygons {
        offset: u32,
        indices: &'a [u32],
        stride: u32,
        vcount: slice::Iter<'a, u32>,
        /// Index into `indices` of the current polygon's first vertex.
        index: usize,
        /// The current polygon's remaining triangles, and its vertex count.
        current: Option<(Range<usize>, usize)>,
        /// Triangulate as a strip, `(k, k+1, k+2)` with alternating
        /// winding, instead of as a fan, `(0, k, k+1)`.
        strip: bool,
    },
    Triangles {
        offset: u32,
        indices: slice::Chunks<'a, u32>,
    },
    Skip,
    None,
}

impl VertexIndices<'_> {
    const fn none() -> Self {
        Self {
            remaining: 0,
            inner: IndicesInner::None,
        }
    }

    fn next_face(&mut self) -> Option<Face> {
        match &mut self.inner {
            IndicesInner::Polygons {
                offset,
                indices,
                stride,
                vcount,
                index,
                current,
                strip,
            } => {
                let stride = *stride as usize;
                let corners = loop {
                    if let Some((triangles, vc)) = current {
                        if let Some(k) = triangles.next() {
                            break if !*strip {
                                [0, k, k + 1]
                            } else if k % 2 == 0 {
                                [k, k + 1, k + 2]
                            } else {
                                [k + 1, k, k + 2]
                            };
                        }
                        *index += stride * *vc;
                        *current = None;
                    }
                    // gz-collada: a polygon with fewer than three vertices
                    // is skipped without using up a triangle (upstream
                    // recursed without advancing `index`).
                    let vc = *vcount.next()? as usize;
                    *current = Some(match (vc < 3, *strip) {
                        (true, _) => (0..0, vc),
                        (false, false) => (1..vc - 1, vc),
                        (false, true) => (0..vc - 2, vc),
                    });
                };
                let base = *index + *offset as usize;
                let at = |c: usize| indices.get(base + stride * c).copied();
                Some([at(corners[0])?, at(corners[1])?, at(corners[2])?])
            }
            IndicesInner::Triangles { offset, indices } => {
                let offset = *offset as usize;
                let mut corner = || indices.next()?.get(offset).copied();
                Some([corner()?, corner()?, corner()?])
            }
            IndicesInner::Skip | IndicesInner::None => None,
        }
    }
}

impl Iterator for VertexIndices<'_> {
    type Item = Face;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        let face = self.next_face();
        if face.is_none() {
            self.remaining = 0;
        }
        face
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.remaining as usize))
    }
}

impl FusedIterator for VertexIndices<'_> {}
