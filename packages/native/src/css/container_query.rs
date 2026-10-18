//! The grammar and the evaluation of a CSS Containment 3 container query, the
//! text after `@container`: `sidebar (min-width: 400px) and (orientation: landscape)`,
//! `(400px <= width < 800px)`, `not (aspect-ratio > 1/1)`.
//!
//! A query is parsed once when the style is set. It is evaluated against the
//! containers above an element through [`Containers`], which picks the one container
//! that answers the whole query: the nearest one with the query's name that contains
//! every axis its size features read (for `style()` alone, the nearest element). That
//! container's content box answers the size features and its computed custom
//! properties the `style()` ones. A feature the grammar does not know
//! (`scroll-state()`, a feature name of a later level, a normal property in `style()`)
//! is *unknown*, which is false when it is the whole answer, as the spec says; text that
//! is not a query at all is an error.

use super::custom_properties::{self, CustomProperties};
use super::dimension::{parse_comparable, parse_definite_length, Comparable};
use super::length::{LengthContext, LinearLength};

/// A query: an optional container name and an optional condition. A query of
/// a name alone is true when a container of that name is above.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ContainerQuery {
    name: Option<String>,
    condition: Option<Condition>,
}

#[derive(Debug, Clone, PartialEq)]
enum Condition {
    Not(Box<Condition>),
    And(Vec<Condition>),
    Or(Vec<Condition>),
    Feature(Feature),
    /// One declaration of a `style()` query.
    Style(StyleFeature),
    /// `scroll-state(...)`, an unknown feature, a value that is not one: neither true nor false.
    Unknown,
}

/// `--name` or `--name: value` inside `style()`.
#[derive(Debug, Clone, PartialEq)]
struct StyleFeature {
    name: String,
    test: StyleTest,
}

#[derive(Debug, Clone, PartialEq)]
enum StyleTest {
    /// `style(--x)`: the property has a value.
    Present,
    /// `style(--x: initial)`: it has none.
    Initial,
    /// `style(--x: value)`: its computed value is this text.
    Equals(String),
    /// `style(--x > 5px)`: the property is a number, a length or a percentage, of the kind the
    /// operand is, and orders so against it.
    Compare(Op, Comparable),
}

#[derive(Debug, Clone, PartialEq)]
struct Feature {
    kind: Kind,
    test: Test,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// `width` and `inline-size`: gpuix has no vertical writing mode.
    Width,
    /// `height` and `block-size`.
    Height,
    AspectRatio,
    Orientation,
}

#[derive(Debug, Clone, PartialEq)]
enum Test {
    /// `(width)`: the feature is not zero.
    Exists,
    Compare(Op, Value),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
}

impl Op {
    /// The operator with its sides swapped: `400px <= width` is `width >= 400px`.
    fn flipped(self) -> Self {
        match self {
            Self::Lt => Self::Gt,
            Self::Le => Self::Ge,
            Self::Gt => Self::Lt,
            Self::Ge => Self::Le,
            Self::Eq => Self::Eq,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Length(LinearLength),
    /// `16 / 9`, as the numerator and the denominator.
    Ratio(f64, f64),
    Orientation(Orientation),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Orientation {
    Portrait,
    Landscape,
}

/// The axes of a container's content box a feature reads, and so the axes a
/// container must contain to answer it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Axes {
    pub width: bool,
    pub height: bool,
}

impl Axes {
    pub(crate) const NONE: Self = Self { width: false, height: false };
    pub(crate) const WIDTH: Self = Self { width: true, height: false };
    pub(crate) const HEIGHT: Self = Self { width: false, height: true };
    pub(crate) const BOTH: Self = Self { width: true, height: true };

    pub(crate) fn union(self, other: Self) -> Self {
        Self { width: self.width || other.width, height: self.height || other.height }
    }
}

/// What a query container answers with: its content box and how its units mean.
#[derive(Debug, Clone)]
pub(crate) struct Measured {
    /// `None` until a layout has produced the size, and for a container the size is not
    /// contained in (an element with no box of its own, or one the layout does not size
    /// from outside): a size feature is unknown for it.
    pub width: Option<f64>,
    pub height: Option<f64>,
    /// The context of the container itself: `em` in a query is its font size.
    pub lengths: LengthContext,
    /// The container's computed custom properties, which `style()` reads.
    pub custom: CustomProperties,
}

/// What a query asks of its container.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Needs {
    /// The axes of the content box its size features read.
    pub axes: Axes,
    /// Whether it has a `style()` feature.
    pub style: bool,
}

impl Needs {
    fn union(self, other: Self) -> Self {
        Self { axes: self.axes.union(other.axes), style: self.style || other.style }
    }
}

/// The containers above the element being styled.
pub(crate) trait Containers {
    /// The one container that answers a query with `name` (any, with `None`) and `needs`: the
    /// nearest that has the name and whose type contains the axes, and with no axes the nearest
    /// element, or with a name the nearest element called it. A call is a read: the caller treats
    /// the axes as depended on. The container answers whether or not its size is known or
    /// contained, and the lookup does not go past it.
    fn container(&self, name: Option<&str>, needs: Needs) -> Option<Measured>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Truth {
    True,
    False,
    Unknown,
}

impl Truth {
    fn not(self) -> Self {
        match self {
            Self::True => Self::False,
            Self::False => Self::True,
            Self::Unknown => Self::Unknown,
        }
    }

    fn of(value: bool) -> Self {
        if value {
            Self::True
        } else {
            Self::False
        }
    }
}

impl ContainerQuery {
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("an empty container query".to_owned());
        }
        let (name, rest) = split_name(text);
        let condition = if rest.trim().is_empty() {
            if name.is_none() {
                return Err(format!("invalid container query \"{text}\""));
            }
            None
        } else {
            Some(parse_condition(rest, Atom::Size).map_err(|reason| format!("invalid container query \"{text}\": {reason}"))?)
        };
        Ok(Self { name, condition })
    }

    pub(crate) fn matches(&self, containers: &dyn Containers) -> bool {
        let name = self.name.as_deref();
        let Some(condition) = &self.condition else {
            return containers.container(name, Needs::default()).is_some();
        };
        let needs = condition.needs();
        let container = (needs != Needs::default()).then(|| containers.container(name, needs)).flatten();
        condition.evaluate(container.as_ref()) == Truth::True
    }
}

impl Condition {
    fn needs(&self) -> Needs {
        match self {
            Self::Not(inner) => inner.needs(),
            Self::And(parts) | Self::Or(parts) => parts.iter().fold(Needs::default(), |needs, part| needs.union(part.needs())),
            Self::Feature(feature) => Needs { axes: feature.kind.axes(), style: false },
            Self::Style(_) => Needs { axes: Axes::NONE, style: true },
            Self::Unknown => Needs::default(),
        }
    }

    fn evaluate(&self, container: Option<&Measured>) -> Truth {
        match self {
            Self::Not(inner) => inner.evaluate(container).not(),
            // False beats unknown beats true; the dual for `or`.
            Self::And(parts) => {
                let mut result = Truth::True;
                for part in parts {
                    match part.evaluate(container) {
                        Truth::False => return Truth::False,
                        Truth::Unknown => result = Truth::Unknown,
                        Truth::True => {}
                    }
                }
                result
            }
            Self::Or(parts) => {
                let mut result = Truth::False;
                for part in parts {
                    match part.evaluate(container) {
                        Truth::True => return Truth::True,
                        Truth::Unknown => result = Truth::Unknown,
                        Truth::False => {}
                    }
                }
                result
            }
            Self::Feature(feature) => container.map_or(Truth::Unknown, |container| feature.evaluate(container)),
            Self::Style(feature) => container.map_or(Truth::Unknown, |container| feature.evaluate(container)),
            Self::Unknown => Truth::Unknown,
        }
    }
}

impl StyleFeature {
    fn evaluate(&self, container: &Measured) -> Truth {
        let value = container.custom.get(&self.name);
        match &self.test {
            StyleTest::Present => Truth::of(value.is_some()),
            StyleTest::Initial => Truth::of(value.is_none()),
            StyleTest::Equals(text) => Truth::of(value == Some(text.as_str())),
            StyleTest::Compare(op, operand) => {
                // A property that is not there, or not a value of the operand's kind, orders as unknown.
                let Some(Some(value)) = value.map(parse_comparable) else { return Truth::Unknown };
                let resolve = |length: &LinearLength| container.lengths.resolve(length, 0.0);
                match (value, operand) {
                    (Comparable::Number(left), Comparable::Number(right)) => Truth::of(order(left, *op, *right)),
                    (Comparable::Percentage(left), Comparable::Percentage(right)) => {
                        Truth::of(order(left, *op, *right))
                    }
                    (Comparable::Length(left), Comparable::Length(right)) => {
                        Truth::of(order(resolve(&left), *op, resolve(right)))
                    }
                    _ => Truth::Unknown,
                }
            }
        }
    }
}

/// Numbers and lengths in px compare exactly: a custom property is not a layout size.
fn order(left: f64, op: Op, right: f64) -> bool {
    match op {
        Op::Lt => left < right,
        Op::Le => left <= right,
        Op::Gt => left > right,
        Op::Ge => left >= right,
        Op::Eq => left == right,
    }
}

impl Kind {
    fn axes(self) -> Axes {
        match self {
            Self::Width => Axes::WIDTH,
            Self::Height => Axes::HEIGHT,
            Self::AspectRatio | Self::Orientation => Axes::BOTH,
        }
    }
}

impl Feature {
    fn evaluate(&self, container: &Measured) -> Truth {
        match self.kind {
            Kind::Width | Kind::Height => {
                let size = if self.kind == Kind::Width { container.width } else { container.height };
                let Some(size) = size else { return Truth::Unknown };
                match &self.test {
                    Test::Exists => Truth::of(size != 0.0),
                    Test::Compare(op, Value::Length(length)) => {
                        Truth::of(compare(size, *op, container.lengths.resolve(length, 0.0)))
                    }
                    Test::Compare(..) => Truth::Unknown,
                }
            }
            Kind::AspectRatio => {
                let (Some(width), Some(height)) = (container.width, container.height) else {
                    return Truth::Unknown;
                };
                match &self.test {
                    Test::Exists => Truth::of(width > 0.0 && height > 0.0),
                    // `width / height` against `n / d`, without dividing: a zero height is the
                    // infinite ratio rather than an error.
                    Test::Compare(op, Value::Ratio(numerator, denominator)) => {
                        Truth::of(compare(width * denominator, *op, numerator * height))
                    }
                    Test::Compare(..) => Truth::Unknown,
                }
            }
            Kind::Orientation => {
                let (Some(width), Some(height)) = (container.width, container.height) else {
                    return Truth::Unknown;
                };
                match &self.test {
                    Test::Exists => Truth::True,
                    Test::Compare(Op::Eq, Value::Orientation(wanted)) => {
                        let actual = if height >= width { Orientation::Portrait } else { Orientation::Landscape };
                        Truth::of(actual == *wanted)
                    }
                    Test::Compare(..) => Truth::Unknown,
                }
            }
        }
    }
}

/// Sizes are laid out in sixty-fourths of a px, so equality is to half of one.
fn compare(left: f64, op: Op, right: f64) -> bool {
    const EPSILON: f64 = 1.0 / 128.0;
    match op {
        Op::Lt => left < right - EPSILON,
        Op::Le => left <= right + EPSILON,
        Op::Gt => left > right + EPSILON,
        Op::Ge => left >= right - EPSILON,
        Op::Eq => (left - right).abs() <= EPSILON,
    }
}

// ── Parsing ───────────────────────────────────────────────────────────────

/// The container name a query starts with and the rest of it. A name is an
/// identifier that is not `not`, `and`, `or` or `none` and does not open a
/// function.
fn split_name(text: &str) -> (Option<String>, &str) {
    let end = text
        .find(|character: char| character.is_whitespace() || character == '(')
        .unwrap_or(text.len());
    let word = &text[..end];
    let opens_function = text[end..].starts_with('(');
    let reserved = ["not", "and", "or", "none"].iter().any(|keyword| word.eq_ignore_ascii_case(keyword));
    if word.is_empty() || opens_function || reserved || !is_ident(word) {
        return (None, text);
    }
    (Some(word.to_owned()), &text[end..])
}

pub(crate) fn is_ident(word: &str) -> bool {
    let mut characters = word.chars();
    let starts = characters
        .next()
        .is_some_and(|first| first.is_alphabetic() || first == '_' || first == '-' || !first.is_ascii());
    starts && word.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || !c.is_ascii())
}

/// What the innermost parentheses of a condition hold: a size feature at the top of a query, a
/// declaration inside `style()`.
#[derive(Debug, Clone, Copy)]
enum Atom {
    Size,
    Style,
}

/// `<query-in-parens> [ and <query-in-parens> ]* | [ or <query-in-parens> ]*`
/// or `not <query-in-parens>`.
fn parse_condition(text: &str, atom: Atom) -> Result<Condition, String> {
    let text = text.trim();
    if let Some(rest) = strip_keyword(text, "not") {
        let (inner, remainder) = parse_in_parens(rest, atom)?;
        if !remainder.trim().is_empty() {
            return Err("`not` takes one condition in parentheses; join more with parentheses".to_owned());
        }
        return Ok(Condition::Not(Box::new(inner)));
    }
    let (first, mut remainder) = parse_in_parens(text, atom)?;
    let mut parts = vec![first];
    let mut joiner: Option<&str> = None;
    while !remainder.trim().is_empty() {
        let keyword = if let Some(rest) = strip_keyword(remainder, "and") {
            ("and", rest)
        } else if let Some(rest) = strip_keyword(remainder, "or") {
            ("or", rest)
        } else {
            return Err(format!("unexpected \"{}\"", remainder.trim()));
        };
        match joiner {
            Some(previous) if previous != keyword.0 => {
                return Err("`and` and `or` cannot be mixed without parentheses".to_owned());
            }
            _ => joiner = Some(keyword.0),
        }
        let (next, rest) = parse_in_parens(keyword.1, atom)?;
        parts.push(next);
        remainder = rest;
    }
    Ok(match joiner {
        None => parts.pop().expect("one part was parsed"),
        Some("and") => Condition::And(parts),
        Some(_) => Condition::Or(parts),
    })
}

/// `keyword` at the start of `text`, followed by whitespace or `(`, case-insensitively;
/// the text after it.
fn strip_keyword<'a>(text: &'a str, keyword: &str) -> Option<&'a str> {
    let text = text.trim_start();
    let head = text.get(..keyword.len())?;
    let rest = &text[keyword.len()..];
    (head.eq_ignore_ascii_case(keyword) && rest.starts_with(|c: char| c.is_whitespace() || c == '(')).then_some(rest)
}

/// One `( ... )` or one function `name( ... )`, and what follows it.
fn parse_in_parens(text: &str, atom: Atom) -> Result<(Condition, &str), String> {
    let text = text.trim_start();
    if text.starts_with('(') {
        let close = matching_paren(text, 0)?;
        let inner = &text[1..close];
        return Ok((parse_paren_contents(inner, atom), &text[close + 1..]));
    }
    // `style(...)` and `scroll-state(...)`: well formed, but only `style()` is answered.
    let name_end = text.find('(').ok_or_else(|| format!("expected a condition in parentheses, found \"{}\"", text.trim()))?;
    let name = &text[..name_end];
    if name_end == 0 || !is_ident(name) {
        return Err(format!("expected a condition in parentheses, found \"{}\"", text.trim()));
    }
    let close = matching_paren(text, name_end)?;
    let condition = match atom {
        Atom::Size if name.eq_ignore_ascii_case("style") => parse_style_query(&text[name_end + 1..close]),
        _ => Condition::Unknown,
    };
    Ok((condition, &text[close + 1..]))
}

/// What is inside `style( ... )`: a declaration, or a condition of declarations in parentheses.
/// Anything else (a normal property, a range, bad text) is unknown, not an error.
fn parse_style_query(inner: &str) -> Condition {
    let inner = inner.trim();
    if inner.starts_with('(') || strip_keyword(inner, "not").is_some() {
        return parse_condition(inner, Atom::Style).unwrap_or(Condition::Unknown);
    }
    parse_style_feature(inner).unwrap_or(Condition::Unknown)
}

/// `--name`, `--name: value`, or a range: `--name > 5px`, `5px < --name`, `1 < --name <= 5`.
fn parse_style_feature(text: &str) -> Option<Condition> {
    let feature = |name: &str, test| Condition::Style(StyleFeature { name: name.to_owned(), test });
    if text.contains(':') {
        let (name, value) = text.split_once(':')?;
        let name = name.trim();
        if !custom_properties::is_name(name) {
            return None;
        }
        let value = custom_properties::normalise(value);
        if value.is_empty() || !is_balanced(&value) {
            return None;
        }
        if custom_properties::is_initial(&value) {
            return Some(feature(name, StyleTest::Initial));
        }
        // `inherit`, `unset` and `revert` need the value the container's parent has.
        if ["inherit", "unset", "revert", "revert-layer"].iter().any(|keyword| value.eq_ignore_ascii_case(keyword)) {
            return None;
        }
        return Some(feature(name, StyleTest::Equals(value)));
    }
    let operators = find_operators(text);
    let compare = |name: &str, op, operand: &str| {
        Some(feature(name, StyleTest::Compare(op, parse_comparable(operand.trim())?)))
    };
    match operators.as_slice() {
        [] => {
            let name = text.trim();
            custom_properties::is_name(name).then(|| feature(name, StyleTest::Present))
        }
        [(index, Operator::Compare(op, width))] => {
            let (left, right) = (text[..*index].trim(), text[*index + width..].trim());
            if custom_properties::is_name(left) {
                compare(left, *op, right)
            } else if custom_properties::is_name(right) {
                compare(right, op.flipped(), left)
            } else {
                None
            }
        }
        [(first, Operator::Compare(low, low_width)), (second, Operator::Compare(high, high_width))] => {
            let (low_value, name, high_value) = (
                text[..*first].trim(),
                text[*first + low_width..*second].trim(),
                text[*second + high_width..].trim(),
            );
            if !custom_properties::is_name(name) {
                return None;
            }
            // `a < --x < b` and `a > --x > b`; `=` has no range form.
            let ascending = matches!((low, high), (Op::Lt | Op::Le, Op::Lt | Op::Le));
            let descending = matches!((low, high), (Op::Gt | Op::Ge, Op::Gt | Op::Ge));
            if !ascending && !descending {
                return None;
            }
            Some(Condition::And(vec![compare(name, low.flipped(), low_value)?, compare(name, *high, high_value)?]))
        }
        _ => None,
    }
}

/// Whether `value` could be the value of a custom property: its brackets close, and no `;` or
/// `!` sits outside them.
fn is_balanced(value: &str) -> bool {
    let mut open = Vec::new();
    for character in value.chars() {
        match character {
            '(' | '[' | '{' => open.push(character),
            ')' | ']' | '}' => {
                let wanted = match character {
                    ')' => '(',
                    ']' => '[',
                    _ => '{',
                };
                if open.pop() != Some(wanted) {
                    return false;
                }
            }
            ';' | '!' if open.is_empty() => return false,
            _ => {}
        }
    }
    open.is_empty()
}

/// The index of the `)` closing the `(` at `open`.
fn matching_paren(text: &str, open: usize) -> Result<usize, String> {
    let mut depth = 0usize;
    for (index, character) in text[open..].char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Ok(open + index);
                }
            }
            _ => {}
        }
    }
    Err("unbalanced parentheses".to_owned())
}

/// What is inside one pair of parentheses: a nested condition, or a size feature (a
/// declaration inside `style()`). Text that is neither is *unknown*, not an error, so a later
/// level's syntax does not invalidate the rule.
fn parse_paren_contents(inner: &str, atom: Atom) -> Condition {
    let inner = inner.trim();
    if inner.starts_with('(') || strip_keyword(inner, "not").is_some() {
        return parse_condition(inner, atom).unwrap_or(Condition::Unknown);
    }
    match atom {
        Atom::Size => parse_feature(inner),
        Atom::Style => parse_style_feature(inner),
    }
    .unwrap_or(Condition::Unknown)
}

fn parse_feature(text: &str) -> Option<Condition> {
    let operators = find_operators(text);
    let feature = |kind, test| Condition::Feature(Feature { kind, test });
    match operators.as_slice() {
        [] => {
            let (kind, _) = feature_kind(text.trim())?;
            Some(feature(kind, Test::Exists))
        }
        [(index, Operator::Colon)] => {
            let (name, value) = (text[..*index].trim(), text[*index + 1..].trim());
            let (kind, prefix) = feature_kind(name)?;
            let op = match prefix {
                Prefix::None => Op::Eq,
                Prefix::Min => Op::Ge,
                Prefix::Max => Op::Le,
            };
            if kind == Kind::Orientation && prefix != Prefix::None {
                return None;
            }
            Some(feature(kind, Test::Compare(op, parse_value(kind, value)?)))
        }
        [(index, Operator::Compare(op, width))] => {
            let (left, right) = (text[..*index].trim(), text[*index + width..].trim());
            if let Some((kind, Prefix::None)) = feature_kind(left) {
                Some(feature(kind, Test::Compare(*op, parse_value(kind, right)?)))
            } else {
                let (kind, Prefix::None) = feature_kind(right)? else { return None };
                Some(feature(kind, Test::Compare(op.flipped(), parse_value(kind, left)?)))
            }
        }
        [(first, Operator::Compare(low, low_width)), (second, Operator::Compare(high, high_width))] => {
            let (low_value, name, high_value) = (
                text[..*first].trim(),
                text[*first + low_width..*second].trim(),
                text[*second + high_width..].trim(),
            );
            let (kind, Prefix::None) = feature_kind(name)? else { return None };
            // `a < width < b` and `a > width > b`; `=` has no range form.
            let ascending = matches!((low, high), (Op::Lt | Op::Le, Op::Lt | Op::Le));
            let descending = matches!((low, high), (Op::Gt | Op::Ge, Op::Gt | Op::Ge));
            if !ascending && !descending {
                return None;
            }
            Some(Condition::And(vec![
                feature(kind, Test::Compare(low.flipped(), parse_value(kind, low_value)?)),
                feature(kind, Test::Compare(*high, parse_value(kind, high_value)?)),
            ]))
        }
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Prefix {
    None,
    Min,
    Max,
}

fn feature_kind(name: &str) -> Option<(Kind, Prefix)> {
    let lower = name.to_ascii_lowercase();
    let (prefix, bare) = if let Some(rest) = lower.strip_prefix("min-") {
        (Prefix::Min, rest)
    } else if let Some(rest) = lower.strip_prefix("max-") {
        (Prefix::Max, rest)
    } else {
        (Prefix::None, lower.as_str())
    };
    let kind = match bare {
        "width" | "inline-size" => Kind::Width,
        "height" | "block-size" => Kind::Height,
        "aspect-ratio" => Kind::AspectRatio,
        "orientation" => Kind::Orientation,
        _ => return None,
    };
    Some((kind, prefix))
}

#[derive(Debug, Clone, Copy)]
enum Operator {
    Colon,
    /// The operator and its width in bytes.
    Compare(Op, usize),
}

/// The `:`, `<`, `<=`, `>`, `>=` and `=` outside parentheses, with their positions.
fn find_operators(text: &str) -> Vec<(usize, Operator)> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut depth = 0usize;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b':' if depth == 0 => found.push((index, Operator::Colon)),
            b'=' if depth == 0 => found.push((index, Operator::Compare(Op::Eq, 1))),
            b'<' | b'>' if depth == 0 => {
                let less = bytes[index] == b'<';
                if bytes.get(index + 1) == Some(&b'=') {
                    found.push((index, Operator::Compare(if less { Op::Le } else { Op::Ge }, 2)));
                    index += 1;
                } else {
                    found.push((index, Operator::Compare(if less { Op::Lt } else { Op::Gt }, 1)));
                }
            }
            _ => {}
        }
        index += 1;
    }
    found
}

fn parse_value(kind: Kind, text: &str) -> Option<Value> {
    match kind {
        Kind::Width | Kind::Height => parse_definite_length(text).map(Value::Length),
        Kind::AspectRatio => parse_ratio(text),
        Kind::Orientation => match text.to_ascii_lowercase().as_str() {
            "portrait" => Some(Value::Orientation(Orientation::Portrait)),
            "landscape" => Some(Value::Orientation(Orientation::Landscape)),
            _ => None,
        },
    }
}

/// `<number> [ / <number> ]?`, both positive.
fn parse_ratio(text: &str) -> Option<Value> {
    let number = |part: &str| part.trim().parse::<f64>().ok().filter(|value| value.is_finite() && *value >= 0.0);
    let (numerator, denominator) = match text.split_once('/') {
        Some((numerator, denominator)) => (number(numerator)?, number(denominator)?),
        None => (number(text)?, 1.0),
    };
    Some(Value::Ratio(numerator, denominator))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The containers above an element, nearest first, the way a chain of them answers.
    struct Chain(Vec<Container>);

    struct Container {
        names: Vec<&'static str>,
        width: Option<f64>,
        height: Option<f64>,
        contains_height: bool,
        /// Whether a size feature can be answered by this container at all.
        contains_width: bool,
        font_size: f64,
        custom: CustomProperties,
    }

    fn container(names: &[&'static str], width: f64, height: f64) -> Container {
        Container {
            names: names.to_vec(),
            width: Some(width),
            height: Some(height),
            contains_height: true,
            contains_width: true,
            font_size: 16.0,
            custom: CustomProperties::default(),
        }
    }

    fn inline_only(names: &[&'static str], width: f64) -> Container {
        Container { height: None, contains_height: false, ..container(names, width, 0.0) }
    }

    /// An element with a name or a custom property and no size container type.
    fn plain(names: &[&'static str], custom: &[(&str, &str)]) -> Container {
        Container {
            width: None,
            height: None,
            contains_height: false,
            contains_width: false,
            custom: CustomProperties::declared(custom.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())))
                .unwrap_or_default(),
            ..container(names, 0.0, 0.0)
        }
    }

    impl Containers for Chain {
        fn container(&self, name: Option<&str>, needs: Needs) -> Option<Measured> {
            // Nearest first.
            self.0
                .iter()
                .find(|candidate| {
                    name.is_none_or(|name| candidate.names.contains(&name))
                        && (!needs.axes.width || candidate.contains_width)
                        && (!needs.axes.height || candidate.contains_height)
                        // A query with no axis asks the nearest element (a name picks among them).
                        && (name.is_some() || needs.axes != Axes::NONE || needs.style)
                })
                .map(|candidate| {
                    let lengths = LengthContext::initial(1000.0, 800.0)
                        .descend(Some(&LinearLength::px(candidate.font_size)), false);
                    Measured {
                        width: candidate.width,
                        height: candidate.height,
                        lengths,
                        custom: candidate.custom.clone(),
                    }
                })
        }
    }

    fn query(text: &str) -> ContainerQuery {
        ContainerQuery::parse(text).unwrap_or_else(|error| panic!("{text}: {error}"))
    }

    fn matches(text: &str, chain: &Chain) -> bool {
        query(text).matches(chain)
    }

    fn one(width: f64, height: f64) -> Chain {
        Chain(vec![container(&[], width, height)])
    }

    #[test]
    fn min_and_max_width_compare_the_content_box() {
        let chain = one(400.0, 300.0);
        assert!(matches("(min-width: 400px)", &chain));
        assert!(!matches("(min-width: 401px)", &chain));
        assert!(matches("(max-width: 400px)", &chain));
        assert!(!matches("(max-width: 399px)", &chain));
        assert!(matches("(width: 400px)", &chain));
        assert!(!matches("(width: 399px)", &chain));
        assert!(matches("(min-height: 300px)", &chain));
        assert!(matches("(MIN-WIDTH: 400PX)", &chain));
        assert!(matches("(inline-size: 400px)", &chain));
        assert!(matches("(min-block-size: 300px)", &chain));
    }

    #[test]
    fn the_range_syntax_takes_either_order_and_two_bounds() {
        let chain = one(500.0, 300.0);
        assert!(matches("(width >= 500px)", &chain));
        assert!(matches("(width > 499px)", &chain));
        assert!(!matches("(width > 500px)", &chain));
        assert!(matches("(width <= 500px)", &chain));
        assert!(!matches("(width < 500px)", &chain));
        assert!(matches("(width = 500px)", &chain));
        assert!(matches("(400px <= width)", &chain));
        assert!(!matches("(500px < width)", &chain));
        assert!(matches("(500px >= width)", &chain));
        assert!(matches("(400px <= width < 800px)", &chain));
        assert!(matches("(400px < width <= 500px)", &chain));
        assert!(!matches("(500px < width < 800px)", &chain));
        assert!(matches("(800px > width >= 500px)", &chain));
        assert!(!matches("(800px > width > 500px)", &chain));
        assert!(matches("(height < 400px)", &chain));
    }

    #[test]
    fn a_boolean_feature_is_true_when_it_is_not_zero() {
        assert!(matches("(width)", &one(10.0, 0.0)));
        assert!(!matches("(height)", &one(10.0, 0.0)));
        assert!(!matches("(width)", &one(0.0, 10.0)));
        assert!(matches("(orientation)", &one(0.0, 0.0)));
    }

    #[test]
    fn aspect_ratio_compares_width_over_height() {
        let chain = one(400.0, 200.0);
        assert!(matches("(aspect-ratio: 2/1)", &chain));
        assert!(matches("(aspect-ratio: 2)", &chain));
        assert!(matches("(aspect-ratio: 4 / 2)", &chain));
        assert!(matches("(min-aspect-ratio: 16/9)", &chain));
        assert!(!matches("(max-aspect-ratio: 16/9)", &chain));
        assert!(matches("(aspect-ratio > 1/1)", &chain));
        assert!(matches("(1/1 < aspect-ratio <= 2/1)", &chain));
        assert!(!matches("(aspect-ratio < 1)", &chain));
        assert!(matches("(aspect-ratio)", &chain));
        // A flat container is infinitely wide, not undefined.
        assert!(matches("(min-aspect-ratio: 100/1)", &one(10.0, 0.0)));
    }

    #[test]
    fn orientation_is_portrait_when_the_height_is_at_least_the_width() {
        assert!(matches("(orientation: landscape)", &one(400.0, 300.0)));
        assert!(!matches("(orientation: portrait)", &one(400.0, 300.0)));
        assert!(matches("(orientation: portrait)", &one(300.0, 300.0)));
        assert!(matches("(orientation: portrait)", &one(300.0, 400.0)));
        assert!(matches("(ORIENTATION: Portrait)", &one(300.0, 400.0)));
    }

    #[test]
    fn and_or_and_not_combine_with_parentheses() {
        let chain = one(500.0, 300.0);
        assert!(matches("(min-width: 400px) and (max-width: 600px)", &chain));
        assert!(!matches("(min-width: 400px) and (max-width: 450px)", &chain));
        assert!(matches("(max-width: 450px) or (min-height: 300px)", &chain));
        assert!(!matches("(max-width: 450px) or (min-height: 400px)", &chain));
        assert!(matches("not (max-width: 450px)", &chain));
        assert!(!matches("not (min-width: 450px)", &chain));
        assert!(matches("((min-width: 400px) and (min-height: 200px)) or (max-width: 10px)", &chain));
        assert!(matches("(not (max-width: 450px))", &chain));
        assert!(matches("(min-width: 1px) AND (min-height: 1px)", &chain));
        assert!(matches("NOT (min-width: 900px)", &chain));
    }

    #[test]
    fn and_with_or_unparenthesised_and_a_bare_not_chain_are_errors() {
        for text in [
            "(width > 1px) and (width < 9px) or (height > 1px)",
            "not (width > 1px) and (width < 9px)",
            "not not (width > 1px)",
            "(width > 1px",
            "width > 1px",
            "",
            "   ",
            "(width > 1px) (height > 1px)",
            "(width > 1px) and",
        ] {
            assert!(ContainerQuery::parse(text).is_err(), "{text:?} should not parse");
        }
    }

    #[test]
    fn lengths_take_every_unit_a_length_takes() {
        let chain = one(320.0, 100.0);
        // The container's font size is 16px; 1vw is 10px (a 1000px viewport) and 1rem is 16px.
        assert!(matches("(min-width: 20em)", &chain));
        assert!(!matches("(min-width: 21em)", &chain));
        assert!(matches("(width = 20rem)", &chain));
        assert!(matches("(width = 32vw)", &chain));
        assert!(matches("(width = calc(10em + 10rem))", &chain));
        assert!(matches("(width = 240pt)", &chain));
        assert!(matches("(min-height: 10vh)", &chain));
    }

    #[test]
    fn em_is_the_font_size_of_the_container() {
        let mut big = container(&[], 320.0, 100.0);
        big.font_size = 32.0;
        let chain = Chain(vec![big]);
        assert!(matches("(width = 10em)", &chain));
        assert!(!matches("(width = 20em)", &chain));
    }

    #[test]
    fn a_name_picks_the_nearest_container_with_that_name() {
        let chain = Chain(vec![container(&["inner"], 200.0, 100.0), container(&["outer", "layout"], 800.0, 600.0)]);
        assert!(matches("(min-width: 700px)", &Chain(vec![container(&[], 800.0, 600.0)])));
        assert!(!matches("(min-width: 700px)", &chain), "unnamed asks the nearest");
        assert!(matches("outer (min-width: 700px)", &chain));
        assert!(matches("layout (min-width: 700px)", &chain));
        assert!(matches("inner (max-width: 200px)", &chain));
        assert!(!matches("inner (min-width: 700px)", &chain));
        assert!(!matches("missing (min-width: 1px)", &chain), "no such container: unknown, so false");
        assert!(matches("outer not (max-width: 700px)", &chain));
        assert!(matches("outer", &chain), "a name alone is true when a container has it");
        assert!(!matches("missing", &chain));
    }

    #[test]
    fn a_feature_skips_the_containers_that_do_not_contain_its_axis() {
        let chain = Chain(vec![inline_only(&[], 300.0), container(&[], 900.0, 700.0)]);
        // Width is the nearest container's; height and the ratio come from the one with a block axis.
        assert!(matches("(max-width: 300px)", &chain));
        assert!(matches("(min-height: 700px)", &chain));
        assert!(matches("(min-aspect-ratio: 1/1)", &chain));
        let only_inline = Chain(vec![inline_only(&[], 300.0)]);
        assert!(!matches("(min-height: 1px)", &only_inline));
        assert!(!matches("not (min-height: 1px)", &only_inline), "unknown stays unknown under not");
        assert!(!matches("(orientation: landscape)", &only_inline));
    }

    #[test]
    fn a_container_with_no_size_yet_answers_unknown() {
        let mut waiting = container(&[], 0.0, 0.0);
        waiting.width = None;
        waiting.height = None;
        let chain = Chain(vec![waiting]);
        assert!(!matches("(min-width: 0px)", &chain));
        assert!(!matches("not (min-width: 0px)", &chain));
        assert!(!matches("(min-width: 0px) or (min-height: 0px)", &chain));
    }

    #[test]
    fn unknown_features_are_unknown_not_errors() {
        let chain = one(500.0, 300.0);
        for text in ["scroll-state(stuck: top)", "style(color: red)", "style(--x > 0)", "style()", "style(--x: inherit)", "(foo: bar)", "(width: wide)", "(color)", "(min-orientation: portrait)", "(width > )", "(400px < width > 100px)"] {
            assert!(!matches(text, &chain), "{text}");
            assert!(!matches(&format!("not {text}"), &chain), "not {text}");
        }
        // Unknown makes `and` unknown unless something is false, and `or` unknown unless something is true.
        assert!(!matches("(foo: bar) and (min-width: 1px)", &chain));
        assert!(!matches("(foo: bar) and (min-width: 9999px)", &chain));
        assert!(matches("(foo: bar) or (min-width: 1px)", &chain));
        assert!(!matches("(foo: bar) or (min-width: 9999px)", &chain));
        assert!(!matches("not ((foo: bar) and (min-width: 1px))", &chain));
        assert!(matches("not ((foo: bar) and (min-width: 9999px))", &chain));
    }

    #[test]
    fn a_container_name_cannot_be_a_keyword() {
        assert_eq!(query("not (width > 1px)").name, None);
        assert_eq!(query("sidebar (width > 1px)").name.as_deref(), Some("sidebar"));
        assert_eq!(query("side-bar_2 (width > 1px)").name.as_deref(), Some("side-bar_2"));
        assert!(ContainerQuery::parse("none (width > 1px)").is_err());
        assert!(ContainerQuery::parse("and").is_err());
        assert!(ContainerQuery::parse("4chan (width > 1px)").is_err());
    }

    fn with_custom(mut container: Container, custom: &[(&str, &str)]) -> Container {
        container.custom = CustomProperties::declared(custom.iter().map(|(n, v)| ((*n).to_owned(), (*v).to_owned())))
            .unwrap_or_default();
        container
    }

    fn styled(custom: &[(&str, &str)]) -> Chain {
        Chain(vec![with_custom(plain(&[], &[]), custom)])
    }

    #[test]
    fn style_compares_a_computed_custom_property_as_text() {
        let chain = styled(&[("--theme", "dark"), ("--space", "a   b"), ("--len", "10px")]);
        assert!(matches("style(--theme: dark)", &chain));
        assert!(matches("style(  --theme  :  dark  )", &chain));
        assert!(matches("style(--theme: dark !important)", &chain));
        assert!(matches("style(--theme: dark /* c */)", &chain));
        assert!(!matches("style(--theme: Dark)", &chain), "case matters");
        assert!(!matches("style(--Theme: dark)", &chain), "and so does the name");
        assert!(!matches("style(--theme: light)", &chain));
        assert!(!matches("style(--missing: dark)", &chain));
        assert!(matches("style(--space: a   b)", &chain));
        assert!(!matches("style(--space: a b)", &chain), "the text inside is compared as it is");
        assert!(matches("style(--len: 10px)", &chain));
        assert!(!matches("style(--len: 10PX)", &chain), "no value is parsed");
    }

    #[test]
    fn style_with_no_value_asks_whether_the_property_has_one_and_initial_whether_it_has_none() {
        let chain = styled(&[("--on", "0")]);
        assert!(matches("style(--on)", &chain), "0 is a value");
        assert!(!matches("style(--off)", &chain));
        assert!(matches("style(--off: initial)", &chain));
        assert!(matches("style(--off: INITIAL)", &chain));
        assert!(!matches("style(--on: initial)", &chain));
    }

    #[test]
    fn style_combines_with_not_and_or_inside_and_outside() {
        let chain = styled(&[("--a", "1"), ("--b", "2")]);
        assert!(matches("style((--a: 1) and (--b: 2))", &chain));
        assert!(!matches("style((--a: 1) and (--b: 3))", &chain));
        assert!(matches("style((--a: 9) or (--b: 2))", &chain));
        assert!(matches("style(not (--a: 9))", &chain));
        assert!(!matches("style(not (--a: 1))", &chain));
        assert!(matches("style(--a: 1) and style(--b: 2)", &chain));
        assert!(matches("style(--a: 9) or style(--b: 2)", &chain));
        assert!(matches("not style(--a: 9)", &chain));
        assert!(!matches("not style(--a: 1)", &chain));
        assert!(matches("style(((--a: 1)) and (not (--b: 3)))", &chain));
        // Not a list of declarations: a value that has `and` in it, which is simply not equal.
        assert!(!matches("style(--a: 1 and --b: 2)", &chain));
        assert!(matches("not style(--a: 1 and --b: 2)", &chain));
        assert!(matches("not style(--a: 1, --b: 2)", &chain));
        // A value that no custom property could have, and `not` of a condition without parentheses.
        for text in ["style(--a: 1; --b: 2)", "style(not --a: 1)", "style(--a: {)", "style(--a: )", "style(--a: 1 ! 2)"] {
            assert!(!matches(text, &chain), "{text}");
            assert!(!matches(&format!("not {text}"), &chain), "not {text} stays unknown");
        }
    }

    #[test]
    fn a_normal_property_in_style_is_unknown() {
        let chain = styled(&[("--a", "1")]);
        for text in ["style(color: red)", "style(display: block)", "style(--a != 0)", "style(--a == 1)", "style()", "style(--a: inherit)", "style(--a: unset)"] {
            assert!(!matches(text, &chain), "{text}");
            assert!(!matches(&format!("not {text}"), &chain), "not {text}");
            assert!(matches(&format!("{text} or style(--a: 1)"), &chain), "{text} or true");
        }
    }

    #[test]
    fn one_container_answers_the_whole_query() {
        // The nearest container has only the inline size, the one above has both.
        let chain = Chain(vec![inline_only(&[], 50.0), container(&[], 100.0, 100.0)]);
        assert!(matches("(max-width: 60px)", &chain));
        assert!(!matches("(max-width: 60px) and (min-height: 0px)", &chain), "both come from the outer one, 100 wide");
        assert!(!matches("(max-width: 60px) or (min-height: 99999px)", &chain));
        assert!(matches("not ((max-width: 60px) and (min-height: 0px))", &chain));
        assert!(matches("(max-width: 60px) and (max-width: 70px)", &chain), "no height read: the nearest answers");
    }

    #[test]
    fn style_and_size_features_in_one_query_are_answered_by_the_size_container() {
        // An element in between sets --x; the size container above it does not.
        let chain = Chain(vec![with_custom(plain(&[], &[]), &[("--x", "1")]), container(&[], 100.0, 100.0)]);
        assert!(!matches("(min-width: 1px) and style(--x: 1)", &chain));
        assert!(matches("style(--x: 1)", &chain), "a style() alone asks the nearest element");
    }

    #[test]
    fn a_container_without_a_size_answers_unknown_to_size_but_style_still_reads_it() {
        let span = Chain(vec![with_custom(plain(&["nm"], &[]), &[("--x", "1")]), container(&[], 500.0, 500.0)]);
        // `plain` has no size type: a size query goes past it, a named one is found by it.
        assert!(matches("(min-width: 100px)", &span));
        assert!(matches("nm style(--x: 1)", &span));
        assert!(matches("nm", &span));
        assert!(!matches("nm (min-width: 100px)", &span), "named: no container with a size is called nm");
    }

    #[test]
    fn style_orders_a_numeric_custom_property_against_a_value_of_the_same_kind() {
        let chain = styled(&[("--n", "1.5"), ("--w", "10px"), ("--em", "1em"), ("--p", "50%"), ("--t", "abc")]);
        assert!(matches("style(--n > 1)", &chain));
        assert!(matches("style(--n >= 1.5)", &chain));
        assert!(!matches("style(--n < 1.5)", &chain));
        assert!(matches("style(--n = 1.5)", &chain));
        assert!(matches("style(1 < --n)", &chain));
        assert!(matches("style(0 < --n <= 1.5)", &chain));
        assert!(!matches("style(0 < --n < 1.5)", &chain));
        assert!(matches("style(2 > --n >= 1)", &chain));
        assert!(matches("style(--w > 5px)", &chain));
        assert!(matches("style(--w < 1em)", &chain), "10px is less than the container's 16px em");
        assert!(matches("style(--w = 10px)", &chain));
        assert!(matches("style(--em = 16px)", &chain), "and 1em is 16px");
        assert!(matches("style(--em > 5px)", &chain));
        assert!(matches("style(--w = calc(5px * 2))", &chain));
        assert!(matches("style(--p > 10%)", &chain));
        // Kinds do not mix, and text is not a number: unknown, so `not` is false too.
        for text in ["style(--w > 5)", "style(--n > 5px)", "style(--p > 10)", "style(--t > 0)", "style(--missing > 0)", "style(--p > 5px)"] {
            assert!(!matches(text, &chain), "{text}");
            assert!(!matches(&format!("not {text}"), &chain), "not {text}");
        }
        // A range that does not parse is unknown too.
        for text in ["style(--n > )", "style(1 < 2)", "style(1 < --n > 0)", "style(--n < --w)"] {
            assert!(!matches(text, &chain), "{text}");
            assert!(!matches(&format!("not {text}"), &chain), "not {text}");
        }
    }
}
