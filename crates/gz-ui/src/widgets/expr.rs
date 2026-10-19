//! The arithmetic a NumberField accepts: `1.5*2`, `-(0.25 + 1)/2`, `90deg`,
//! `pi/2 rad`, `50cm`, `12.5%`. `+ - * /`, parentheses, unary signs,
//! decimals with exponents, the constant `pi`, and units. A unit applies to
//! the whole product or quotient before it (`pi/2 rad` is `(pi/2) rad`,
//! `45deg*2` is 90°); `+` and `-` end it (`3m + 20cm` is 3.2 m). Units
//! convert into the field's [`Unit`]: angles into degrees, lengths into
//! metres; a unit of the wrong kind is an error. Parentheses nest at most
//! [`MAX_DEPTH`] deep.

use std::fmt;

/// What a field's value measures, and so which unit suffixes it takes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Unit {
    /// A plain number (`%` still means ÷100).
    #[default]
    None,
    /// Metres; takes `m`, `cm`, `mm`, `km`.
    Length,
    /// Degrees; takes `deg`, `°`, `rad`.
    Angle,
}

impl Unit {
    /// The suffix shown after the value (`text_3`), if any.
    pub fn suffix(self) -> Option<&'static str> {
        match self {
            Unit::None => None,
            Unit::Length => Some("m"),
            Unit::Angle => Some("°"),
        }
    }

    /// The factor that turns a value in `name` into this unit.
    fn factor(self, name: &str) -> Result<f64, ExprError> {
        let f = match (self, name) {
            (_, "%") | (Unit::Length, "cm") => 0.01,
            (Unit::Angle, "deg" | "°") | (Unit::Length, "m") => 1.0,
            (Unit::Angle, "rad") => 180.0 / std::f64::consts::PI,
            (Unit::Length, "mm") => 0.001,
            (Unit::Length, "km") => 1000.0,
            (_, "deg" | "°" | "rad" | "m" | "cm" | "mm" | "km") => {
                return Err(ExprError::WrongUnit(name.to_string()))
            }
            _ => return Err(ExprError::UnknownUnit(name.to_string())),
        };
        Ok(f)
    }
}

/// Why an expression didn't evaluate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExprError {
    Empty,
    /// Something unexpected at this byte offset.
    Unexpected(usize),
    /// Input ended mid-expression.
    UnexpectedEnd,
    UnknownUnit(String),
    /// A unit of another kind (`cm` in an angle field).
    WrongUnit(String),
    /// Division by zero, overflow or similar.
    NotFinite,
    /// Parentheses nested deeper than [`MAX_DEPTH`].
    TooDeep,
}

/// How deeply parentheses may nest. The parser recurses once per level, so
/// this bounds its stack use (wasm's stack is 1 MB).
pub const MAX_DEPTH: usize = 64;

impl fmt::Display for ExprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExprError::Empty => write!(f, "empty"),
            ExprError::Unexpected(at) => write!(f, "unexpected input at {at}"),
            ExprError::UnexpectedEnd => write!(f, "incomplete expression"),
            ExprError::UnknownUnit(u) => write!(f, "unknown unit '{u}'"),
            ExprError::WrongUnit(u) => write!(f, "'{u}' doesn't fit this field"),
            ExprError::NotFinite => write!(f, "not a finite number"),
            ExprError::TooDeep => write!(f, "too many nested parentheses"),
        }
    }
}

/// Evaluates `input` for a field measured in `unit`.
pub fn eval(input: &str, unit: Unit) -> Result<f64, ExprError> {
    let mut p = Parser {
        s: input.as_bytes(),
        text: input,
        i: 0,
        unit,
        depth: 0,
    };
    p.skip_ws();
    if p.i == p.s.len() {
        return Err(ExprError::Empty);
    }
    let v = p.expr()?;
    p.skip_ws();
    if p.i != p.s.len() {
        return Err(ExprError::Unexpected(p.i));
    }
    if v.is_finite() {
        Ok(v)
    } else {
        Err(ExprError::NotFinite)
    }
}

struct Parser<'a> {
    s: &'a [u8],
    text: &'a str,
    i: usize,
    unit: Unit,
    /// Open parentheses around the current position.
    depth: usize,
}

impl Parser<'_> {
    fn skip_ws(&mut self) {
        while self.s.get(self.i).is_some_and(u8::is_ascii_whitespace) {
            self.i += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_ws();
        self.s.get(self.i).copied()
    }

    fn expr(&mut self) -> Result<f64, ExprError> {
        let mut v = self.term()?;
        while let Some(op @ (b'+' | b'-')) = self.peek() {
            self.i += 1;
            let r = self.term()?;
            v = if op == b'+' { v + r } else { v - r };
        }
        Ok(v)
    }

    /// Factors joined by `*` and `/`; a unit after any of them scales the
    /// term so far. One unit per term (`90deg*2rad` is an error).
    fn term(&mut self) -> Result<f64, ExprError> {
        let mut v = self.factor()?;
        let mut united = false;
        loop {
            self.skip_ws();
            let at = self.i;
            if let Some(f) = self.unit()? {
                if united {
                    return Err(ExprError::Unexpected(at));
                }
                united = true;
                v *= f;
            }
            let Some(op @ (b'*' | b'/')) = self.peek() else {
                return Ok(v);
            };
            self.i += 1;
            let r = self.factor()?;
            v = if op == b'*' { v * r } else { v / r };
        }
    }

    /// Unary signs (in a loop, so a run of them doesn't recurse) and a
    /// primary.
    fn factor(&mut self) -> Result<f64, ExprError> {
        let mut negative = false;
        while let Some(sign @ (b'-' | b'+')) = self.peek() {
            self.i += 1;
            negative ^= sign == b'-';
        }
        let v = self.primary()?;
        Ok(if negative { -v } else { v })
    }

    fn primary(&mut self) -> Result<f64, ExprError> {
        match self.peek() {
            None => Err(ExprError::UnexpectedEnd),
            Some(b'(') => {
                if self.depth == MAX_DEPTH {
                    return Err(ExprError::TooDeep);
                }
                self.i += 1;
                self.depth += 1;
                let v = self.expr()?;
                self.depth -= 1;
                match self.peek() {
                    Some(b')') => {
                        self.i += 1;
                        Ok(v)
                    }
                    Some(_) => Err(ExprError::Unexpected(self.i)),
                    None => Err(ExprError::UnexpectedEnd),
                }
            }
            Some(c) if c.is_ascii_digit() || c == b'.' => self.number(),
            Some(_) => {
                if self.text[self.i..].to_ascii_lowercase().starts_with("pi") {
                    self.i += 2;
                    Ok(std::f64::consts::PI)
                } else {
                    Err(ExprError::Unexpected(self.i))
                }
            }
        }
    }

    fn number(&mut self) -> Result<f64, ExprError> {
        let start = self.i;
        let digits = |p: &mut Self| {
            while p.s.get(p.i).is_some_and(u8::is_ascii_digit) {
                p.i += 1;
            }
        };
        digits(self);
        if self.s.get(self.i) == Some(&b'.') {
            self.i += 1;
            digits(self);
        }
        // An exponent only if digits follow (so `2e` isn't half a number).
        if matches!(self.s.get(self.i), Some(b'e' | b'E')) {
            let save = self.i;
            self.i += 1;
            if matches!(self.s.get(self.i), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if self.s.get(self.i).is_some_and(u8::is_ascii_digit) {
                digits(self);
            } else {
                self.i = save;
            }
        }
        self.text[start..self.i]
            .parse()
            .map_err(|_| ExprError::Unexpected(start))
    }

    /// The factor of a unit at the current position, if there is one.
    fn unit(&mut self) -> Result<Option<f64>, ExprError> {
        let rest = &self.text[self.i..];
        let len = if rest.starts_with('°') {
            '°'.len_utf8()
        } else if rest.starts_with('%') {
            1
        } else {
            rest.bytes().take_while(u8::is_ascii_alphabetic).count()
        };
        if len == 0 {
            return Ok(None);
        }
        let name = rest[..len].to_ascii_lowercase();
        if name == "pi" || name == "e" {
            return Err(ExprError::Unexpected(self.i));
        }
        let f = self.unit.factor(&name)?;
        self.i += len;
        Ok(Some(f))
    }
}
