//! Checks that turn a malformed document into an error instead of a stack
//! overflow, a panic in the vendored iterators, or silently missing
//! geometry: element nesting before the XML is parsed, and what
//! [`crate::assemble`] is about to read from a primitive. Like three.js,
//! only primitives the scene instantiates are checked.

use crate::vendor::collada::{Accessor, ArrayData, Document, Geometry, PrimitiveType, Uri};

/// Every source of primitive `index` of `geometry` (including its mesh's
/// `<vertices>`) resolves to an accessor that fits its `<float_array>`, and
/// `<p>` holds as many indices as `count`/`<vcount>` say.
pub(crate) fn primitive(
    doc: &Document<'_>,
    geometry: &Geometry<'_>,
    index: usize,
) -> Result<(), String> {
    let prim = &geometry.mesh.primitives[index];
    let element = || format!("<{:?}> of geometry #{}", prim.ty, geometry.id).to_lowercase();
    // Without a VERTEX input nothing is drawn.
    let Some(input) = &prim.input else {
        return Ok(());
    };
    // VERTEX always reads this mesh's own `<vertices>`, as in three.js.
    let vertices = &geometry.mesh.vertices.input;
    let sources = [
        Some(&vertices.position.source),
        vertices.normal.as_ref().map(|i| &i.source),
        vertices.texcoord.as_ref().map(|i| &i.source),
        vertices.color.as_ref().map(|i| &i.source),
        input.normal.as_ref().map(|i| &i.source),
        input.color.as_ref().map(|i| &i.source),
    ];
    let texcoords = input.texcoord.iter().map(|i| Some(&i.source));
    for source in sources.into_iter().chain(texcoords).flatten() {
        accessor(doc, source).map_err(|e| format!("{}: {e}", element()))?;
    }

    let stride = u64::from(prim.stride);
    let expected = match prim.ty {
        // Not drawn.
        PrimitiveType::Lines | PrimitiveType::LineStrips => return Ok(()),
        PrimitiveType::Triangles => u64::from(prim.count) * 3 * stride,
        // Polylists read `<vcount>`; the others count each `<p>`.
        PrimitiveType::Polylist
        | PrimitiveType::Polygons
        | PrimitiveType::TriFans
        | PrimitiveType::TriStrips => {
            prim.vcount.iter().map(|&n| u64::from(n)).sum::<u64>() * stride
        }
    };
    let found = prim.p.len() as u64;
    if found != expected {
        return Err(format!(
            "{}: <p> has {found} indices, expected {expected}",
            element()
        ));
    }
    Ok(())
}

fn accessor(doc: &Document<'_>, source: &Uri<'_, Accessor<'_>>) -> Result<(), String> {
    let id = source.as_str();
    let accessor = doc
        .get(source)
        .ok_or_else(|| format!("no <source> #{id} with an <accessor>"))?;
    let array = accessor.source.as_str();
    let data = doc
        .get(&accessor.source)
        .and_then(ArrayData::as_float)
        .ok_or_else(|| format!("source #{id}: #{array} is not a <float_array>"))?;
    let needed = u64::from(accessor.count) * u64::from(accessor.stride);
    if needed > data.len() as u64 {
        return Err(format!(
            "source #{id}: the accessor reads {needed} floats, #{array} has {}",
            data.len()
        ));
    }
    Ok(())
}

/// Deepest element nesting accepted. roxmltree's tokenizer recurses once
/// per level, so a deeply nested document would overflow the stack (which
/// aborts a wasm client). Measured: ~350 levels fit a 2 MB thread in a
/// debug build, ~1500 a 1 MB one in release. COLLADA needs a few levels
/// more than its `<node>` nesting, which the parser caps at 256.
pub(crate) const MAX_XML_DEPTH: usize = 300;

/// Fails if elements nest deeper than [`MAX_XML_DEPTH`]. A scan of the
/// text, skipping comments, CDATA, processing instructions and the
/// doctype; anything malformed is left to the XML parser to report.
pub(crate) fn nesting(text: &str) -> Result<(), String> {
    let b = text.as_bytes();
    // Index just past the first `pattern` at or after `from`.
    let after = |from: usize, pattern: &[u8]| {
        b.get(from..)?
            .windows(pattern.len())
            .position(|w| w == pattern)
            .map(|p| from + p + pattern.len())
    };
    let mut depth = 0usize;
    let mut i = 0;
    while let Some(at) = b[i..].iter().position(|&c| c == b'<').map(|p| i + p) {
        let rest = &b[at..];
        let end = if rest.starts_with(b"<!--") {
            after(at, b"-->")
        } else if rest.starts_with(b"<![CDATA[") {
            after(at, b"]]>")
        } else if rest.starts_with(b"<?") {
            after(at, b"?>")
        } else if rest.starts_with(b"<!") {
            // A doctype, possibly with an internal subset in brackets.
            let mut brackets = 0usize;
            rest.iter()
                .position(|&c| {
                    match c {
                        b'[' => brackets += 1,
                        b']' => brackets = brackets.saturating_sub(1),
                        _ => {}
                    }
                    c == b'>' && brackets == 0
                })
                .map(|p| at + p + 1)
        } else if rest.starts_with(b"</") {
            depth = depth.saturating_sub(1);
            after(at, b">")
        } else {
            // A start tag; `>` may appear in quoted attribute values.
            let mut quote = None;
            let close = rest.iter().position(|&c| {
                match (quote, c) {
                    (None, b'"' | b'\'') => quote = Some(c),
                    (Some(q), _) if q == c => quote = None,
                    _ => {}
                }
                quote.is_none() && c == b'>'
            });
            if let Some(close) = close {
                if rest[close - 1] != b'/' {
                    depth += 1;
                    if depth > MAX_XML_DEPTH {
                        return Err(format!("elements nested more than {MAX_XML_DEPTH} deep"));
                    }
                }
            }
            close.map(|p| at + p + 1)
        };
        match end {
            Some(end) => i = end,
            None => return Ok(()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nested(depth: usize, open: &str, close: &str) -> String {
        open.repeat(depth) + &close.repeat(depth)
    }

    #[test]
    fn nesting_counts_elements_only() {
        let max = MAX_XML_DEPTH;
        assert!(nesting(&nested(max, "<a>", "</a>")).is_ok());
        let e = nesting(&nested(max + 1, "<a>", "</a>")).unwrap_err();
        assert!(e.contains("more than 300"), "{e}");
        // Siblings, empty elements, quoted `>`, comments, CDATA, processing
        // instructions and doctypes don't nest.
        assert!(nesting(&"<a></a>".repeat(max * 2)).is_ok());
        assert!(nesting(&nested(max, "<a/><b x='>'>", "</b>")).is_ok());
        let skipped = r#"<?xml version="1.0"?><!DOCTYPE c [<!ENTITY e "<x>">]><!-- <x> --><c><![CDATA[<x>]]></c>"#;
        assert!(nesting(&skipped.repeat(max * 2)).is_ok());
        // Unterminated markup is the XML parser's to report.
        for text in ["<a", "<!-- <a>", "<![CDATA[", "<?x", "<!DOCTYPE", "</a"] {
            assert!(nesting(text).is_ok(), "{text}");
        }
    }

    #[test]
    fn nesting_at_the_limit_parses() {
        let doc = format!(
            r#"<COLLADA version="1.4.1"><asset>{}</asset></COLLADA>"#,
            nested(MAX_XML_DEPTH - 2, "<extra>", "</extra>")
        );
        // A coverage build's frames are larger than a plain debug build's.
        let parse = move || crate::parse(doc.as_bytes()).is_ok();
        let thread = std::thread::Builder::new().stack_size(8 << 20);
        assert!(thread.spawn(parse).unwrap().join().unwrap());
    }
}
