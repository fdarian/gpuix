//! The grammar and the evaluation of a CSS Containment 3 container query, the
//! text after `@container`: `sidebar (min-width: 400px) and (orientation: landscape)`,
//! `(400px <= width < 800px)`, `not (aspect-ratio > 1/1)`.
//!
//! A query is parsed once when the style is set. It is evaluated against the
//! containers above an element through [`Containers`], which says which
//! container answers (the nearest one with the query's name that contains the
//! axes the feature reads) and what its content box is. A feature the grammar
//! does not know (`style()`, `scroll-state()`, a feature name of a later
//! level) is *unknown*, which is false when it is the whole answer, as the
//! spec says; text that is not a query at all is an error.

use super::dimension::parse_definite_length;
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
    /// `style(...)`, an unknown feature, a value that is not one: neither true nor false.
    Unknown,
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
#[derive(Debug, Clone, Copy)]
pub(crate) struct Measured {
    /// `None` until a layout has produced the size.
    pub width: Option<f64>,
    pub height: Option<f64>,
    /// The context of the container itself: `em` in a query is its font size.
    pub lengths: LengthContext,
}

/// The containers above the element being styled.
pub(crate) trait Containers {
    /// The nearest container that has `name` (any, with `None`) and contains `axes`,
    /// which is the one whose size answers a feature reading those axes. A call
    /// is a read: the caller treats the axes as depended on.
    fn container(&self, name: Option<&str>, axes: Axes) -> Option<Measured>;
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
            Some(parse_condition(rest).map_err(|reason| format!("invalid container query \"{text}\": {reason}"))?)
        };
        Ok(Self { name, condition })
    }

    pub(crate) fn matches(&self, containers: &dyn Containers) -> bool {
        let name = self.name.as_deref();
        match &self.condition {
            None => containers.container(name, Axes::NONE).is_some(),
            Some(condition) => condition.evaluate(name, containers) == Truth::True,
        }
    }
}

impl Condition {
    fn evaluate(&self, name: Option<&str>, containers: &dyn Containers) -> Truth {
        match self {
            Self::Not(inner) => inner.evaluate(name, containers).not(),
            // False beats unknown beats true; the dual for `or`.
            Self::And(parts) => {
                let mut result = Truth::True;
                for part in parts {
                    match part.evaluate(name, containers) {
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
                    match part.evaluate(name, containers) {
                        Truth::True => return Truth::True,
                        Truth::Unknown => result = Truth::Unknown,
                        Truth::False => {}
                    }
                }
                result
            }
            Self::Feature(feature) => feature.evaluate(name, containers),
            Self::Unknown => Truth::Unknown,
        }
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
    fn evaluate(&self, name: Option<&str>, containers: &dyn Containers) -> Truth {
        let Some(container) = containers.container(name, self.kind.axes()) else {
            return Truth::Unknown;
        };
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

/// `<query-in-parens> [ and <query-in-parens> ]* | [ or <query-in-parens> ]*`
/// or `not <query-in-parens>`.
fn parse_condition(text: &str) -> Result<Condition, String> {
    let text = text.trim();
    if let Some(rest) = strip_keyword(text, "not") {
        let (inner, remainder) = parse_in_parens(rest)?;
        if !remainder.trim().is_empty() {
            return Err("`not` takes one condition in parentheses; join more with parentheses".to_owned());
        }
        return Ok(Condition::Not(Box::new(inner)));
    }
    let (first, mut remainder) = parse_in_parens(text)?;
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
        let (next, rest) = parse_in_parens(keyword.1)?;
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
fn parse_in_parens(text: &str) -> Result<(Condition, &str), String> {
    let text = text.trim_start();
    if text.starts_with('(') {
        let close = matching_paren(text, 0)?;
        let inner = &text[1..close];
        return Ok((parse_paren_contents(inner), &text[close + 1..]));
    }
    // `style(...)` and `scroll-state(...)`: well formed, but nothing here answers them.
    let name_end = text.find('(').ok_or_else(|| format!("expected a condition in parentheses, found \"{}\"", text.trim()))?;
    if name_end == 0 || !is_ident(&text[..name_end]) {
        return Err(format!("expected a condition in parentheses, found \"{}\"", text.trim()));
    }
    let close = matching_paren(text, name_end)?;
    Ok((Condition::Unknown, &text[close + 1..]))
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

/// What is inside one pair of parentheses: a nested condition, or a size feature.
/// Text that is neither is *unknown*, not an error, so a later level's syntax does not
/// invalidate the rule.
fn parse_paren_contents(inner: &str) -> Condition {
    let inner = inner.trim();
    if inner.starts_with('(') || strip_keyword(inner, "not").is_some() {
        return parse_condition(inner).unwrap_or(Condition::Unknown);
    }
    parse_feature(inner).unwrap_or(Condition::Unknown)
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

    /// One container per name, the way a chain of them answers.
    struct Chain(Vec<Container>);

    struct Container {
        names: Vec<&'static str>,
        width: Option<f64>,
        height: Option<f64>,
        contains_height: bool,
        font_size: f64,
    }

    fn container(names: &[&'static str], width: f64, height: f64) -> Container {
        Container { names: names.to_vec(), width: Some(width), height: Some(height), contains_height: true, font_size: 16.0 }
    }

    fn inline_only(names: &[&'static str], width: f64) -> Container {
        Container { names: names.to_vec(), width: Some(width), height: None, contains_height: false, font_size: 16.0 }
    }

    impl Containers for Chain {
        fn container(&self, name: Option<&str>, axes: Axes) -> Option<Measured> {
            // Nearest first.
            self.0
                .iter()
                .find(|candidate| {
                    name.is_none_or(|name| candidate.names.contains(&name)) && (!axes.height || candidate.contains_height)
                })
                .map(|candidate| {
                    let lengths = LengthContext::initial(1000.0, 800.0)
                        .descend(Some(&LinearLength::px(candidate.font_size)), false);
                    Measured { width: candidate.width, height: candidate.height, lengths }
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
        for text in ["style(--theme: dark)", "scroll-state(stuck: top)", "(foo: bar)", "(width: wide)", "(color)", "(min-orientation: portrait)", "(width > )", "(400px < width > 100px)"] {
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
}
