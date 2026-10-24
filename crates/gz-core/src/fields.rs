//! A message's numeric fields, for the Plot panel: every number a message
//! holds, by its [`FieldPath`] ("twist.linear.x"), and one number read by
//! its path. Messages are decoded by type name against the `gz.msgs`
//! descriptors, as [`echo`](crate::echo) does.
//!
//! A path steps through fields by name. A repeated field's element is
//! named by its index (`ranges[3]`), or, for a list of messages that each
//! have a `name`, by that name (`joint[wheel_left].axis1.velocity`), so a
//! series follows a joint however the list is ordered.

use std::fmt;
use std::str::FromStr;

use prost_reflect::{DynamicMessage, FieldDescriptor, Kind, ReflectMessage, Value};

use crate::echo::{decode, EchoError};

/// How deep [`Message::numbers`] looks: a guard against types that nest
/// themselves.
const MAX_DEPTH: usize = 8;

/// How many elements of a repeated field [`Message::numbers`] lists (a
/// path reaches any of them).
pub const MAX_ELEMENTS: usize = 64;

/// How many numbers [`Message::numbers`] lists at most.
pub const MAX_NUMBERS: usize = 512;

/// One step of a [`FieldPath`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Step {
    /// A field, by name.
    Field(String),
    /// A repeated field's element, by index.
    Index(usize),
    /// A repeated message field's element, by its `name` field.
    Key(String),
}

/// Where a number is in a message: fields by name, a repeated field's
/// elements by index or by name (`joint[wheel_left].axis1.position`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct FieldPath(pub Vec<Step>);

impl fmt::Display for FieldPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, step) in self.0.iter().enumerate() {
            match step {
                Step::Field(name) if i == 0 => write!(f, "{name}")?,
                Step::Field(name) => write!(f, ".{name}")?,
                Step::Index(n) => write!(f, "[{n}]")?,
                Step::Key(key) if plain_key(key) => write!(f, "[{key}]")?,
                Step::Key(key) => write!(f, "[\"{}\"]", key.replace('"', "\\\""))?,
            }
        }
        Ok(())
    }
}

/// Whether a key reads back as itself unquoted: not a number, and no
/// brackets, dots or quotes.
fn plain_key(key: &str) -> bool {
    key.parse::<usize>().is_err() && !key.contains(['[', ']', '.', '"']) && !key.is_empty()
}

/// Why a path doesn't parse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadPath(pub String);

impl fmt::Display for BadPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not a field path: {}", self.0)
    }
}

impl std::error::Error for BadPath {}

impl FromStr for FieldPath {
    type Err = BadPath;

    /// `a.b[2].c` or `joint[wheel_left].axis1.position`: an element in
    /// brackets is an index when it's a number, a name otherwise; a name
    /// in quotes (`joint["3"]`, `link["a.b"]`) is a name whatever it
    /// holds, with `\"` for a quote.
    fn from_str(s: &str) -> Result<Self, BadPath> {
        let bad = || BadPath(s.to_string());
        let mut steps = Vec::new();
        let mut chars = s.chars().peekable();
        loop {
            let mut name = String::new();
            while let Some(&c) = chars.peek().filter(|c| c.is_alphanumeric() || **c == '_') {
                name.push(c);
                chars.next();
            }
            if name.is_empty() {
                return Err(bad());
            }
            steps.push(Step::Field(name));
            while chars.next_if_eq(&'[').is_some() {
                if chars.next_if_eq(&'"').is_some() {
                    let mut key = String::new();
                    loop {
                        match chars.next().ok_or_else(bad)? {
                            '\\' => key.push(chars.next().ok_or_else(bad)?),
                            '"' => break,
                            c => key.push(c),
                        }
                    }
                    if chars.next() != Some(']') {
                        return Err(bad());
                    }
                    steps.push(Step::Key(key));
                    continue;
                }
                let mut key = String::new();
                loop {
                    match chars.next().ok_or_else(bad)? {
                        ']' => break,
                        c => key.push(c),
                    }
                }
                if key.is_empty() {
                    return Err(bad());
                }
                steps.push(match key.parse() {
                    Ok(n) => Step::Index(n),
                    Err(_) => Step::Key(key),
                });
            }
            match chars.next() {
                None => return Ok(FieldPath(steps)),
                Some('.') => {}
                Some(_) => return Err(bad()),
            }
        }
    }
}

/// A decoded message, read by field path.
#[derive(Clone, Debug)]
pub struct Message(DynamicMessage);

impl Message {
    /// `payload`, a serialized `msg_type` (Fortress's `ignition.msgs`
    /// names read as `gz.msgs`).
    pub fn decode(msg_type: &str, payload: &[u8]) -> Result<Self, EchoError> {
        decode(msg_type, payload).map(Message)
    }

    /// Every number in the message, in field order, with its path:
    /// unset fields too (as their defaults), so a field that is zero now
    /// can still be picked. Booleans read as 0 and 1; enums, strings and
    /// bytes aren't numbers. The header is left out (its stamp is the
    /// plot's time), as are maps. At most [`MAX_ELEMENTS`] of a repeated
    /// field and [`MAX_NUMBERS`] in all.
    pub fn numbers(&self) -> Vec<(FieldPath, f64)> {
        let mut out = Vec::new();
        let mut path = Vec::new();
        numbers_in(&self.0, &mut path, &mut out, 0);
        out
    }

    /// The number at `path`; `None` when the message has no such field or
    /// element, or it isn't a number.
    pub fn number(&self, path: &FieldPath) -> Option<f64> {
        number_in(&self.0, &path.0)
    }

    /// The header's stamp in seconds, if the message has one that is set.
    pub fn stamp(&self) -> Option<f64> {
        let header = self.0.get_field_by_name("header")?;
        let Value::Message(header) = &*header else {
            return None;
        };
        let stamp = header.get_field_by_name("stamp")?;
        let Value::Message(stamp) = &*stamp else {
            return None;
        };
        let sec = stamp.get_field_by_name("sec").and_then(|v| as_number(&v))?;
        let nsec = stamp
            .get_field_by_name("nsec")
            .and_then(|v| as_number(&v))?;
        let t = sec + nsec * 1e-9;
        (t > 0.0).then_some(t)
    }
}

/// The number `steps` lead to in `msg`, by reference (a field unset is
/// its default).
fn number_in(msg: &DynamicMessage, steps: &[Step]) -> Option<f64> {
    let (Step::Field(name), rest) = steps.split_first()? else {
        return None;
    };
    let field = msg.descriptor().get_field_by_name(name)?;
    let value = msg.get_field(&field);
    value_in(&value, rest)
}

/// The number `steps` lead to from `value`: an element of a list, a field
/// of a message, or the value itself when no steps are left.
fn value_in(value: &Value, steps: &[Step]) -> Option<f64> {
    match (value, steps.split_first()) {
        (Value::List(_), None) => None,
        (value, None) => as_number(value),
        (Value::List(items), Some((Step::Index(n), rest))) => value_in(items.get(*n)?, rest),
        (Value::List(items), Some((Step::Key(key), rest))) => {
            let item = items.iter().find(|v| element_name(v) == Some(key))?;
            value_in(item, rest)
        }
        (Value::Message(m), Some((Step::Field(_), _))) => number_in(m, steps),
        _ => None,
    }
}

/// A number's value, or `None` for anything else.
fn as_number(value: &Value) -> Option<f64> {
    match *value {
        Value::F64(v) => Some(v),
        Value::F32(v) => Some(f64::from(v)),
        Value::I32(v) => Some(f64::from(v)),
        Value::U32(v) => Some(f64::from(v)),
        // Precision beyond 2^53 doesn't matter on a chart.
        #[allow(clippy::cast_precision_loss)]
        Value::I64(v) => Some(v as f64),
        #[allow(clippy::cast_precision_loss)]
        Value::U64(v) => Some(v as f64),
        Value::Bool(v) => Some(f64::from(u8::from(v))),
        _ => None,
    }
}

/// A list element's `name`, if it is a message with a non-empty one.
fn element_name(value: &Value) -> Option<&str> {
    let Value::Message(m) = value else {
        return None;
    };
    let field = m.descriptor().get_field_by_name("name")?;
    match m.get_field(&field) {
        std::borrow::Cow::Borrowed(Value::String(s)) if !s.is_empty() => Some(s.as_str()),
        _ => None,
    }
}

/// Whether a list's elements can be named by `name`: messages, each with
/// a different, non-empty one.
fn keyed(items: &[Value]) -> bool {
    let names: Vec<&str> = items.iter().filter_map(element_name).collect();
    names.len() == items.len()
        && names
            .iter()
            .enumerate()
            .all(|(i, n)| !names[..i].contains(n))
}

fn numbers_in(
    msg: &DynamicMessage,
    path: &mut Vec<Step>,
    out: &mut Vec<(FieldPath, f64)>,
    depth: usize,
) {
    if depth >= MAX_DEPTH {
        return;
    }
    for field in msg.descriptor().fields() {
        if field.is_map() || (depth == 0 && field.name() == "header") {
            continue;
        }
        path.push(Step::Field(field.name().to_string()));
        let value = msg.get_field(&field);
        match &*value {
            Value::List(items) => {
                let by_name = keyed(items);
                for (i, item) in items.iter().take(MAX_ELEMENTS).enumerate() {
                    let step = match element_name(item) {
                        Some(name) if by_name => Step::Key(name.to_string()),
                        _ => Step::Index(i),
                    };
                    path.push(step);
                    value_numbers(&field, item, path, out, depth);
                    path.pop();
                }
            }
            v => value_numbers(&field, v, path, out, depth),
        }
        path.pop();
        if out.len() >= MAX_NUMBERS {
            out.truncate(MAX_NUMBERS);
            return;
        }
    }
}

fn value_numbers(
    field: &FieldDescriptor,
    value: &Value,
    path: &mut Vec<Step>,
    out: &mut Vec<(FieldPath, f64)>,
    depth: usize,
) {
    if out.len() >= MAX_NUMBERS {
        return;
    }
    match value {
        Value::Message(m) => numbers_in(m, path, out, depth + 1),
        _ if matches!(field.kind(), Kind::Enum(_)) => {}
        v => {
            if let Some(n) = as_number(v) {
                out.push((FieldPath(path.clone()), n));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use prost::Message as _;

    use super::*;
    use crate::msgs;

    fn path(s: &str) -> FieldPath {
        s.parse().unwrap()
    }

    /// A path prints as it parses: fields with dots, elements in
    /// brackets, numbers as indices and anything else as a name.
    #[test]
    fn a_path_parses_and_prints() {
        for s in [
            "x",
            "twist.linear.x",
            "ranges[3]",
            "joint[wheel_left].axis1.position",
            "a[1][2].b",
        ] {
            assert_eq!(path(s).to_string(), s);
        }
        assert_eq!(
            path("joint[wheel_left].axis1").0,
            vec![
                Step::Field("joint".into()),
                Step::Key("wheel_left".into()),
                Step::Field("axis1".into())
            ]
        );
        assert_eq!(path("r[0]").0[1], Step::Index(0));
        // Names that look like numbers or hold dots or quotes go in quotes.
        for (step, shown) in [
            ("3", "j[\"3\"]"),
            ("a.b", "j[\"a.b\"]"),
            ("say \"hi\"", "j[\"say \\\"hi\\\"\"]"),
            ("v::chassis", "j[v::chassis]"),
        ] {
            let p = FieldPath(vec![Step::Field("j".into()), Step::Key(step.into())]);
            assert_eq!(p.to_string(), shown);
            assert_eq!(path(shown), p, "{shown}");
        }
        for bad in [
            "",
            "a.",
            ".a",
            "a[",
            "a[]",
            "a[1]x",
            "a b",
            "[1]",
            "a[\"x",
            "a[\"x\"y]",
        ] {
            assert_eq!(bad.parse::<FieldPath>(), Err(BadPath(bad.into())), "{bad}");
        }
        assert_eq!(BadPath("q".into()).to_string(), "not a field path: q");
    }

    fn odometry() -> Vec<u8> {
        msgs::Odometry {
            header: Some(msgs::Header {
                stamp: Some(msgs::Time {
                    sec: 12,
                    nsec: 500_000_000,
                }),
                ..Default::default()
            }),
            twist: Some(msgs::Twist {
                linear: Some(msgs::Vector3d {
                    x: 0.352,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
        .encode_to_vec()
    }

    /// Every number is listed, unset ones as zero, the header left out;
    /// one reads by its path; the header's stamp is the time.
    #[test]
    fn an_odometry_lists_and_reads_its_numbers() {
        let msg = Message::decode("gz.msgs.Odometry", &odometry()).unwrap();
        let numbers = msg.numbers();
        let names: Vec<String> = numbers.iter().map(|(p, _)| p.to_string()).collect();
        assert!(names.contains(&"twist.linear.x".to_string()), "{names:?}");
        assert!(names.contains(&"twist.angular.z".to_string()), "{names:?}");
        assert!(names.contains(&"pose.position.x".to_string()), "{names:?}");
        assert!(!names.iter().any(|n| n.starts_with("header")), "{names:?}");
        let x = numbers.iter().find(|(p, _)| p == &path("twist.linear.x"));
        assert_eq!(x.map(|(_, v)| *v), Some(0.352));

        assert_eq!(msg.number(&path("twist.linear.x")), Some(0.352));
        assert_eq!(msg.number(&path("twist.angular.z")), Some(0.0));
        assert_eq!(msg.number(&path("twist.linear")), None);
        assert_eq!(msg.number(&path("twist.nope")), None);
        assert_eq!(msg.number(&path("twist.linear.x.y")), None);
        assert_eq!(msg.stamp(), Some(12.5));

        let none = Message::decode("gz.msgs.Odometry", &[]).unwrap();
        assert_eq!(none.stamp(), None);
        assert!(Message::decode("gz.msgs.Nope", &[]).is_err());
    }

    /// Joints are named by their names, so a series follows its joint; a
    /// list without names goes by index; booleans are numbers, strings
    /// aren't.
    #[test]
    fn repeated_fields_go_by_name_or_index() {
        let joint = |name: &str, v: f64| msgs::Joint {
            name: name.into(),
            axis1: Some(msgs::Axis {
                velocity: v,
                ..Default::default()
            }),
            ..Default::default()
        };
        let model = msgs::Model {
            name: "vehicle".into(),
            joint: vec![joint("wheel_left", 3.52), joint("wheel_right", 4.71)],
            ..Default::default()
        }
        .encode_to_vec();
        let msg = Message::decode("gz.msgs.Model", &model).unwrap();
        let names: Vec<String> = msg.numbers().iter().map(|(p, _)| p.to_string()).collect();
        assert!(
            names.contains(&"joint[wheel_right].axis1.velocity".to_string()),
            "{names:?}"
        );
        assert_eq!(
            msg.number(&path("joint[wheel_right].axis1.velocity")),
            Some(4.71)
        );
        assert_eq!(msg.number(&path("joint[1].axis1.velocity")), Some(4.71));
        assert_eq!(
            msg.number(&path("joint[wheel_middle].axis1.velocity")),
            None
        );
        assert_eq!(msg.number(&path("joint[9].axis1.velocity")), None);
        assert_eq!(msg.number(&path("joint.axis1.velocity")), None);
        assert_eq!(msg.number(&path("name")), None);
        assert_eq!(msg.number(&path("is_static")), Some(0.0));

        let scan = msgs::LaserScan {
            ranges: (0..100).map(f64::from).collect(),
            ..Default::default()
        }
        .encode_to_vec();
        let msg = Message::decode("gz.msgs.LaserScan", &scan).unwrap();
        let ranges = msg
            .numbers()
            .into_iter()
            .filter(|(p, _)| p.0.first() == Some(&Step::Field("ranges".into())))
            .count();
        assert_eq!(ranges, MAX_ELEMENTS);
        assert_eq!(msg.number(&path("ranges[99]")), Some(99.0));
    }

    /// Two elements with one name, or one without, go by index.
    #[test]
    fn a_list_with_unnamed_or_shared_names_goes_by_index() {
        let joints = |a: &str, b: &str| {
            let j = |n: &str| msgs::Joint {
                name: n.into(),
                ..Default::default()
            };
            msgs::Model {
                joint: vec![j(a), j(b)],
                ..Default::default()
            }
            .encode_to_vec()
        };
        for (a, b) in [("j", "j"), ("j", "")] {
            let msg = Message::decode("gz.msgs.Model", &joints(a, b)).unwrap();
            let names: Vec<String> = msg.numbers().iter().map(|(p, _)| p.to_string()).collect();
            assert!(
                names.iter().any(|n| n.starts_with("joint[1].")),
                "{names:?}"
            );
        }
    }
}
