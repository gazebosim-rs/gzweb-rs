//! Writing the draft into its model (spec §6.6; DECISIONS.md D-114): each
//! part's element goes into the model's SDFormat as the server wrote it
//! (`world_file::model_sdf`), and Done checks the result: every link has
//! inertia or the model is static, and every joint has both ends.

use std::ops::Range;

use roxmltree::Node;

use super::{sdf_link_name, Part};
use crate::insert::xml_escape;

/// `sdf` (a document holding model `model`, a scene path) with `parts`
/// written into it, in order: a link, joint or plugin last in the model; a
/// visual, collision or sensor last in its link (a link the draft adds
/// gets them in its own element); a reparent as its joint's `<parent>`
/// (the draft's own joint written with it, the model's rewritten). The
/// rest of the text stays as it was: each part is a text edit to it
/// (`edits`, applied from the end).
pub fn write_parts(sdf: &str, model: &str, parts: &[Part]) -> Result<String, String> {
    // A reparent of the draft's own joint goes into its element; the last
    // of a joint's reparents wins.
    let mut parts: Vec<Part> = parts.to_vec();
    let mut reparents: Vec<(String, String, String)> = Vec::new();
    for i in 0..parts.len() {
        let Part::Reparent {
            joint,
            model: owner,
            parent: to,
            ..
        } = parts[i].clone()
        else {
            continue;
        };
        let draft = parts[..i].iter_mut().find_map(|p| match p {
            Part::Joint { name, parent, .. } if *name == joint && owner == model => Some(parent),
            _ => None,
        });
        match draft {
            Some(parent) => *parent = to,
            None => {
                reparents.retain(|(j, o, _)| !(*j == joint && *o == owner));
                reparents.push((joint, owner, to));
            }
        }
    }
    let parts = parts.as_slice();
    let doc = roxmltree::Document::parse(sdf).map_err(|e| e.to_string())?;
    let top = doc
        .root_element()
        .children()
        .find(|n| n.has_tag_name("model"))
        .ok_or("no <model> in the SDFormat")?;
    // What goes on each link, by scene path, in the order they came.
    let mut on_links: Vec<(String, String)> = Vec::new();
    for p in parts {
        if let Part::Visual { link, .. }
        | Part::Collision { link, .. }
        | Part::Sensor { link, .. } = p
        {
            match on_links.iter_mut().find(|(l, _)| l == link) {
                Some((_, text)) => text.push_str(&p.sdf(model)),
                None => on_links.push((link.clone(), p.sdf(model))),
            }
        }
    }
    let mut in_model = String::new();
    for p in parts {
        match p {
            Part::Link { name, .. } => {
                let mut element = p.sdf(model);
                let path = format!("{model}/{name}");
                if let Some(i) = on_links.iter().position(|(l, _)| *l == path) {
                    let (_, text) = on_links.remove(i);
                    element.insert_str(element.len() - "</link>".len(), &text);
                }
                in_model.push_str(&element);
            }
            Part::Joint { .. } | Part::Plugin { .. } => in_model.push_str(&p.sdf(model)),
            Part::Visual { .. }
            | Part::Collision { .. }
            | Part::Sensor { .. }
            | Part::Reparent { .. } => {}
        }
    }
    let mut edits = Vec::new();
    for (joint, owner, to) in &reparents {
        let element = find_model(top, model, owner)
            .and_then(|m| named(m, "joint", joint))
            .ok_or_else(|| format!("no joint {joint} in the model"))?;
        let parent = element
            .children()
            .find(|c| c.has_tag_name("parent"))
            .ok_or_else(|| format!("{joint} has no parent link"))?;
        edits.push((
            parent.range(),
            format!("<parent>{}</parent>", xml_escape(&sdf_link_name(owner, to))),
        ));
    }
    for (path, text) in &on_links {
        let link = find_link(top, model, path)
            .ok_or_else(|| format!("no link {} in the model", sdf_link_name(model, path)))?;
        edits.push(last_in(sdf, link, text)?);
    }
    if !in_model.is_empty() {
        edits.push(last_in(sdf, top, &in_model)?);
    }
    edits.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
    let mut out = sdf.to_string();
    for (range, text) in edits {
        out.replace_range(range, &text);
    }
    Ok(out)
}

/// The model scene path `path` names: `top` (model `model`) or one nested
/// in it.
fn find_model<'a, 'i>(top: Node<'a, 'i>, model: &str, path: &str) -> Option<Node<'a, 'i>> {
    if path == model {
        return Some(top);
    }
    let rest = path.strip_prefix(model)?.strip_prefix('/')?;
    rest.split('/')
        .try_fold(top, |at, name| named(at, "model", name))
}

/// The link scene path `path` names under `top` (model `model`), through
/// nested models.
fn find_link<'a, 'i>(top: Node<'a, 'i>, model: &str, path: &str) -> Option<Node<'a, 'i>> {
    let rest = path.strip_prefix(model)?.strip_prefix('/')?;
    let mut names: Vec<&str> = rest.split('/').collect();
    let link = names.pop()?;
    let mut at = top;
    for name in names {
        at = named(at, "model", name)?;
    }
    named(at, "link", link)
}

/// `n`'s child element `tag` named `name`.
fn named<'a, 'i>(n: Node<'a, 'i>, tag: &str, name: &str) -> Option<Node<'a, 'i>> {
    n.children()
        .find(|c| c.has_tag_name(tag) && c.attribute("name") == Some(name))
}

/// The edit to `sdf` that puts `text` last inside element `n` (opening it
/// out if it is empty, `<link name="l"/>`).
fn last_in(sdf: &str, n: Node, text: &str) -> Result<(Range<usize>, String), String> {
    let range = n.range();
    let element = &sdf[range.clone()];
    if element.ends_with("/>") {
        let tag = n.tag_name().name();
        return Ok((range.end - 2..range.end, format!(">{text}</{tag}>")));
    }
    let close = element.rfind("</").ok_or("an element doesn't close")?;
    let at = range.start + close;
    Ok((at..at, text.to_string()))
}

/// Whether the model `sdf` holds can be written as Done does it (§6.6):
/// every link has inertia, unless its model is static; every joint has both
/// ends, each `world` or a link, nested model or frame of the model. Why
/// not, if it can't.
pub fn check_model(sdf: &str) -> Result<(), String> {
    let doc = roxmltree::Document::parse(sdf).map_err(|e| e.to_string())?;
    let top = doc
        .root_element()
        .children()
        .find(|n| n.has_tag_name("model"))
        .ok_or("no <model> in the SDFormat")?;
    check(top, false, "")
}

/// [`check_model`] for model `m`, inside a static one or not; `prefix`
/// names its nested models' parts as the top model does (`inner::`).
fn check(m: Node, in_static: bool, prefix: &str) -> Result<(), String> {
    let is_static = in_static
        || m.children()
            .find(|c| c.has_tag_name("static"))
            .and_then(|s| s.text())
            .is_some_and(|t| matches!(t.trim(), "true" | "1"));
    let elements = || m.children().filter(Node::is_element);
    for link in elements().filter(|c| c.has_tag_name("link")) {
        let name = link.attribute("name").unwrap_or_default();
        if !is_static && !link.children().any(|c| c.has_tag_name("inertial")) {
            return Err(format!(
                "{prefix}{name} has no inertia (a link needs it unless its model is static)"
            ));
        }
    }
    for joint in elements().filter(|c| c.has_tag_name("joint")) {
        let name = joint.attribute("name").unwrap_or_default();
        for end in ["parent", "child"] {
            let to = joint
                .children()
                .find(|c| c.has_tag_name(end))
                .and_then(|c| c.text())
                .map(str::trim)
                .unwrap_or_default();
            if to.is_empty() {
                return Err(format!("{prefix}{name} has no {end} link"));
            }
            if !(to == "world" || to == "__model__" || resolves(m, to)) {
                return Err(format!("{prefix}{name}'s {end} {to} isn't in the model"));
            }
        }
    }
    for inner in elements().filter(|c| c.has_tag_name("model")) {
        let name = inner.attribute("name").unwrap_or_default();
        check(inner, is_static, &format!("{prefix}{name}::"))?;
    }
    Ok(())
}

/// Whether `name` (`link`, `inner::link`) is a link, nested model or frame
/// of model `m`.
fn resolves(m: Node, name: &str) -> bool {
    let mut parts: Vec<&str> = name.split("::").collect();
    let Some(last) = parts.pop() else {
        return false;
    };
    let mut at = m;
    for p in parts {
        match named(at, "model", p) {
            Some(inner) => at = inner,
            None => return false,
        }
    }
    ["link", "model", "frame"]
        .iter()
        .any(|tag| named(at, tag, last).is_some())
}
