//! Numbers, lengths, percentages and `calc()` over them, shared by every value
//! parser that reads a dimension.
//!
//! [`parse_linear_length`] reads one length of any unit into a
//! [`LinearLength`], which stays unresolved: `em`, `rem`, `%` and the viewport
//! units need the element or the window. The other parsers here have no such
//! context, so they accept only what is already known without one (an absolute
//! length, a number or a percentage) and treat everything else as invalid
//! rather than silently wrong.

use super::length::LinearLength;

/// The largest length a value may take. Browsers clamp an out-of-range length
/// instead of rejecting it (Chrome's `LayoutUnit` saturates here), and gpui
/// would otherwise cast a huge `f64` to an infinite `f32`.
pub(crate) const MAX_LENGTH: f64 = 33_554_431.0;

/// What a token or a `calc()` expression evaluates to.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Quantity {
    Number(f64),
    Linear(LinearLength, Units),
}

/// The kinds of unit a [`Quantity::Linear`] was written with, which `0px`,
/// `0%` and `0em` all collapse into the same all-zero [`LinearLength`] and the
/// context-free parsers must still tell apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Units {
    absolute: bool,
    percent: bool,
    relative: bool,
}

impl Units {
    const ABSOLUTE: Self = Self { absolute: true, percent: false, relative: false };
    const PERCENT: Self = Self { absolute: false, percent: true, relative: false };
    const RELATIVE: Self = Self { absolute: false, percent: false, relative: true };

    fn union(self, other: Self) -> Self {
        Self {
            absolute: self.absolute || other.absolute,
            percent: self.percent || other.percent,
            relative: self.relative || other.relative,
        }
    }
}

/// Parses a number, a length of any unit, a percentage or a `calc()` of them.
fn parse_quantity(token: &str) -> Option<Quantity> {
    let token = token.trim();
    if let Some(args) = super::syntax::function_args(token, "calc") {
        return evaluate(args);
    }
    let (value, unit) = split_dimension(token)?;
    quantity(value, &unit)
}

fn quantity(value: f64, unit: &str) -> Option<Quantity> {
    let linear = |length: LinearLength, units: Units| Some(Quantity::Linear(length, units));
    let fraction = value / 100.0;
    match unit {
        "" => Some(Quantity::Number(value)),
        "%" => linear(LinearLength::percent(fraction), Units::PERCENT),
        "em" => linear(LinearLength { em: value, ..LinearLength::default() }, Units::RELATIVE),
        "rem" => linear(LinearLength { rem: value, ..LinearLength::default() }, Units::RELATIVE),
        "vw" => linear(LinearLength { vw: fraction, ..LinearLength::default() }, Units::RELATIVE),
        "vh" => linear(LinearLength { vh: fraction, ..LinearLength::default() }, Units::RELATIVE),
        "vmin" => linear(LinearLength { vmin: fraction, ..LinearLength::default() }, Units::RELATIVE),
        "vmax" => linear(LinearLength { vmax: fraction, ..LinearLength::default() }, Units::RELATIVE),
        _ => linear(LinearLength::px(value * px_per(unit)?), Units::ABSOLUTE),
    }
}

fn px_per(unit: &str) -> Option<f64> {
    Some(match unit {
        "px" => 1.0,
        "in" => 96.0,
        "cm" => 96.0 / 2.54,
        "mm" => 96.0 / 25.4,
        "q" => 96.0 / 101.6,
        "pt" => 96.0 / 72.0,
        "pc" => 16.0,
        _ => return None,
    })
}

/// One CSS length: a dimension of any length unit or `%`, a `calc()` of them,
/// or a bare `0`. Each term is clamped to [`MAX_LENGTH`]. A bare number
/// that is not zero, and a `calc()` that comes to a plain number, are not
/// lengths.
pub(crate) fn parse_linear_length(token: &str) -> Option<LinearLength> {
    match parse_quantity(token)? {
        Quantity::Linear(length, _) => Some(length.clamped()),
        Quantity::Number(_) if is_zero_literal(token) => Some(LinearLength::default()),
        Quantity::Number(_) => None,
    }
}

/// A length in px, clamped to [`MAX_LENGTH`]. A bare number is only a length
/// when it is zero.
pub(crate) fn parse_length(token: &str) -> Option<f64> {
    match parse_quantity(token)? {
        Quantity::Linear(length, Units::ABSOLUTE) => length.as_px().map(clamp_length),
        Quantity::Number(_) if is_zero_literal(token) => Some(0.0),
        _ => None,
    }
}

/// `0` written out, the one bare number that is also a length. A `calc()` that
/// comes to zero is still a number.
fn is_zero_literal(token: &str) -> bool {
    split_dimension(token.trim()).is_some_and(|(value, unit)| unit.is_empty() && value == 0.0)
}

/// A length in px or a percentage as a fraction, for the places that take either.
pub(crate) fn parse_length_percentage(token: &str) -> Option<LengthPercentage> {
    match parse_quantity(token)? {
        Quantity::Linear(length, Units::ABSOLUTE) => length.as_px().map(|px| LengthPercentage::Length(clamp_length(px))),
        Quantity::Linear(length, Units::PERCENT) => length.as_percent().map(LengthPercentage::Percent),
        Quantity::Number(_) if is_zero_literal(token) => Some(LengthPercentage::Length(0.0)),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum LengthPercentage {
    Length(f64),
    /// A fraction: `50%` is `0.5`.
    Percent(f64),
}

/// A number, or a percentage as a fraction.
pub(crate) fn parse_number_or_percentage(token: &str) -> Option<f64> {
    match parse_quantity(token)? {
        Quantity::Number(value) => Some(value),
        Quantity::Linear(length, Units::PERCENT) => length.as_percent(),
        Quantity::Linear(..) => None,
    }
}

/// Whether the token is written as a number, a dimension or a `calc()`, valid
/// or not. No colour starts that way.
pub(crate) fn is_numeric(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    lower.starts_with("calc(")
        || lower.starts_with(|first: char| first.is_ascii_digit() || matches!(first, '+' | '-' | '.'))
}

fn clamp_length(value: f64) -> f64 {
    value.clamp(-MAX_LENGTH, MAX_LENGTH)
}

/// A number and its lowercased unit, which is empty for a bare number and `%`
/// for a percentage. The whole token must be the dimension.
pub(crate) fn split_dimension(token: &str) -> Option<(f64, String)> {
    let (value, unit, consumed) = scan_dimension(token)?;
    (consumed == token.len()).then(|| (value, unit.to_ascii_lowercase()))
}

/// The dimension at the start of `text`: its number, its unit as written and
/// the bytes it covers. A number that overflows `f64` is infinite, which the
/// callers clamp.
fn scan_dimension(text: &str) -> Option<(f64, &str, usize)> {
    let bytes = text.as_bytes();
    let digits = |from: usize| bytes[from..].iter().take_while(|byte| byte.is_ascii_digit()).count();
    let mut end = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let integer = digits(end);
    end += integer;
    let mut fraction = 0;
    if bytes.get(end) == Some(&b'.') {
        fraction = digits(end + 1);
        if fraction > 0 {
            end += 1 + fraction;
        }
    }
    if integer + fraction == 0 {
        return None;
    }
    // An exponent needs digits, or the `e` starts a unit such as `em`.
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(bytes.get(end + 1), Some(b'+' | b'-')));
        let exponent = digits(end + 1 + sign);
        if exponent > 0 {
            end += 1 + sign + exponent;
        }
    }
    let value = text[..end].parse::<f64>().ok()?;
    let unit_length = if bytes.get(end) == Some(&b'%') {
        1
    } else {
        bytes[end..].iter().take_while(|byte| byte.is_ascii_alphabetic()).count()
    };
    Some((value, &text[end..end + unit_length], end + unit_length))
}

#[derive(Debug, PartialEq)]
enum Token {
    Quantity(Quantity),
    Plus,
    Minus,
    Star,
    Slash,
    Open,
    Close,
}

fn tokenize(text: &str) -> Option<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut rest = text;
    loop {
        rest = rest.trim_start();
        let Some(first) = rest.chars().next() else {
            return Some(tokens);
        };
        let simple = match first {
            '(' => Some(Token::Open),
            ')' => Some(Token::Close),
            '*' => Some(Token::Star),
            '/' => Some(Token::Slash),
            _ => None,
        };
        if let Some(token) = simple {
            tokens.push(token);
            rest = &rest[1..];
            continue;
        }
        if let Some((value, unit, consumed)) = scan_dimension(rest) {
            tokens.push(Token::Quantity(quantity(value, &unit.to_ascii_lowercase())?));
            rest = &rest[consumed..];
            continue;
        }
        // A nested `calc(` groups like a bare parenthesis.
        if rest.get(..5).is_some_and(|name| name.eq_ignore_ascii_case("calc(")) {
            tokens.push(Token::Open);
            rest = &rest[5..];
            continue;
        }
        // A sign that does not start a number is an operator. CSS requires
        // whitespace around it, which `scan_dimension` enforces by taking a sign
        // that touches its digits as part of the number.
        match first {
            '+' => tokens.push(Token::Plus),
            '-' => tokens.push(Token::Minus),
            _ => return None,
        }
        rest = &rest[1..];
    }
}

/// The value of the arguments of a `calc()`.
fn evaluate(args: &str) -> Option<Quantity> {
    let tokens = tokenize(args)?;
    let mut parser = Parser { tokens: &tokens, index: 0 };
    let result = parser.sum()?;
    let finite = match result {
        Quantity::Number(value) => value.is_finite(),
        Quantity::Linear(length, _) => length.is_finite(),
    };
    (parser.index == tokens.len() && finite).then_some(result)
}

struct Parser<'a> {
    tokens: &'a [Token],
    index: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.index)
    }

    fn sum(&mut self) -> Option<Quantity> {
        let mut left = self.product()?;
        loop {
            let subtract = match self.peek() {
                Some(Token::Plus) => false,
                Some(Token::Minus) => true,
                _ => return Some(left),
            };
            self.index += 1;
            let right = self.product()?;
            left = match (left, right) {
                (Quantity::Number(left), Quantity::Number(right)) => {
                    Quantity::Number(if subtract { left - right } else { left + right })
                }
                (Quantity::Linear(left, left_units), Quantity::Linear(right, right_units)) => {
                    let length = if subtract { left.minus(right) } else { left.plus(right) };
                    Quantity::Linear(length, left_units.union(right_units))
                }
                _ => return None,
            };
        }
    }

    fn product(&mut self) -> Option<Quantity> {
        let mut left = self.factor()?;
        loop {
            let divide = match self.peek() {
                Some(Token::Star) => false,
                Some(Token::Slash) => true,
                _ => return Some(left),
            };
            self.index += 1;
            let right = self.factor()?;
            left = match (left, right, divide) {
                (_, Quantity::Number(divisor), true) if divisor != 0.0 && divisor.is_finite() => {
                    map_quantity(left, |value| value / divisor)
                }
                (Quantity::Number(factor), other, false) => map_quantity(other, |value| value * factor),
                (other, Quantity::Number(factor), false) => map_quantity(other, |value| value * factor),
                _ => return None,
            };
        }
    }

    fn factor(&mut self) -> Option<Quantity> {
        let token = self.tokens.get(self.index)?;
        self.index += 1;
        match token {
            Token::Quantity(quantity) => Some(*quantity),
            Token::Open => self.group(),
            _ => None,
        }
    }

    fn group(&mut self) -> Option<Quantity> {
        let inner = self.sum()?;
        if self.tokens.get(self.index) != Some(&Token::Close) {
            return None;
        }
        self.index += 1;
        Some(inner)
    }
}

fn map_quantity(quantity: Quantity, mut apply: impl FnMut(f64) -> f64) -> Quantity {
    match quantity {
        Quantity::Number(value) => Quantity::Number(apply(value)),
        Quantity::Linear(length, units) => Quantity::Linear(length.map(apply), units),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_numbers_signs_exponents_and_units() {
        assert_eq!(split_dimension("12px"), Some((12.0, "px".to_owned())));
        assert_eq!(split_dimension("+.5Em"), Some((0.5, "em".to_owned())));
        assert_eq!(split_dimension("1e1"), Some((10.0, String::new())));
        assert_eq!(split_dimension("1e1px"), Some((10.0, "px".to_owned())));
        assert_eq!(split_dimension("2em"), Some((2.0, "em".to_owned())));
        assert_eq!(split_dimension("50%"), Some((50.0, "%".to_owned())));
        assert_eq!(split_dimension("px"), None);
        assert_eq!(split_dimension("1px2"), None);
        assert_eq!(split_dimension("1%%"), None);
    }

    fn linear(text: &str) -> LinearLength {
        parse_linear_length(text).unwrap_or_else(|| panic!("{text} should parse"))
    }

    #[test]
    fn absolute_units_resolve_to_px() {
        assert_eq!(parse_length("1in"), Some(96.0));
        assert_eq!(parse_length("1PC"), Some(16.0));
        assert_eq!(parse_length("2.54cm"), Some(96.0));
        assert_eq!(parse_length("25.4MM"), Some(96.0));
        assert_eq!(parse_length("101.6q"), Some(96.0));
        assert_eq!(parse_length("72pt"), Some(96.0));
        assert_eq!(parse_length("3PX"), Some(3.0));
        assert_eq!(parse_number_or_percentage("3"), Some(3.0));
        assert_eq!(parse_number_or_percentage("50%"), Some(0.5));
    }

    #[test]
    fn the_context_free_parsers_reject_units_that_need_a_context() {
        for unit in ["em", "rem", "vw", "vh", "vmin", "vmax", "ch", "fr"] {
            let text = format!("1{unit}");
            assert_eq!(parse_length(&text), None, "{unit}");
            assert_eq!(parse_length_percentage(&text), None, "{unit}");
            assert_eq!(parse_number_or_percentage(&text), None, "{unit}");
        }
        for text in ["calc(1px + 50%)", "calc(50% + 1px)", "calc(1px + 0%)", "calc(1em - 1em)", "0em", "0%", "0vw"] {
            assert_eq!(parse_length(text), None, "{text}");
        }
        assert_eq!(parse_length_percentage("calc(1px + 50%)"), None);
        assert_eq!(parse_length_percentage("calc(1px + 0%)"), None);
        assert_eq!(parse_length_percentage("0em"), None);
        assert_eq!(parse_number_or_percentage("0px"), None);
        assert_eq!(parse_number_or_percentage("calc(50% + 1px)"), None);
    }

    #[test]
    fn length_percentage_tells_a_length_from_a_percentage() {
        assert_eq!(parse_length_percentage("12px"), Some(LengthPercentage::Length(12.0)));
        assert_eq!(parse_length_percentage("50%"), Some(LengthPercentage::Percent(0.5)));
        assert_eq!(parse_length_percentage("0"), Some(LengthPercentage::Length(0.0)));
        assert_eq!(parse_length_percentage("0%"), Some(LengthPercentage::Percent(0.0)));
        assert_eq!(parse_length_percentage("calc(100% / 4)"), Some(LengthPercentage::Percent(0.25)));
        assert_eq!(parse_length_percentage("calc(1in + 24px)"), Some(LengthPercentage::Length(120.0)));
        assert_eq!(parse_length_percentage("3"), None);
    }

    #[test]
    fn calc_follows_operator_precedence_and_nests() {
        let calc = |text: &str| parse_length(text);
        assert_eq!(calc("calc(1px + 2px * 3)"), Some(7.0));
        assert_eq!(calc("calc((1px + 2px) * 3)"), Some(9.0));
        assert_eq!(calc("calc(10px / 4)"), Some(2.5));
        assert_eq!(calc("calc(1px - -2px)"), Some(3.0));
        assert_eq!(calc("calc(calc(1px + 1px) * 2)"), Some(4.0));
        assert_eq!(calc("calc(1in + 24px)"), Some(120.0));
        assert_eq!(parse_number_or_percentage("calc(100% / 4)"), Some(0.25));
        assert_eq!(parse_number_or_percentage("calc(2 * 3)"), Some(6.0));
    }

    #[test]
    fn calc_rejects_what_css_rejects() {
        for text in [
            "calc()",
            "calc(1px +)",
            "calc(1px+2px)",
            "calc(1px -2px)",
            "calc(1px 2px)",
            "calc(1px * 2px)",
            "calc(1px / 2px)",
            "calc(1px / 0)",
            "calc(2 / 1px)",
            "calc(1px + 2)",
            "calc(2 + 1px)",
            "calc(1px + 1em * 2em)",
            "calc((1px)",
            "calc(1px))",
            "calc(1px) 2px",
        ] {
            assert_eq!(parse_linear_length(text), None, "{text}");
        }
    }

    #[test]
    fn every_unit_parses_into_its_own_term() {
        assert_eq!(linear("12px"), LinearLength::px(12.0));
        assert_eq!(linear("1in"), LinearLength::px(96.0));
        assert_eq!(linear("2.54cm"), LinearLength::px(96.0));
        assert_eq!(linear("25.4mm"), LinearLength::px(96.0));
        assert_eq!(linear("101.6q"), LinearLength::px(96.0));
        assert_eq!(linear("72pt"), LinearLength::px(96.0));
        assert_eq!(linear("6pc"), LinearLength::px(96.0));
        assert_eq!(linear("50%"), LinearLength::percent(0.5));
        assert_eq!(linear("1.5em"), LinearLength { em: 1.5, ..LinearLength::default() });
        assert_eq!(linear("2rem"), LinearLength { rem: 2.0, ..LinearLength::default() });
        assert_eq!(linear("50vw"), LinearLength { vw: 0.5, ..LinearLength::default() });
        assert_eq!(linear("25vh"), LinearLength { vh: 0.25, ..LinearLength::default() });
        assert_eq!(linear("10vmin"), LinearLength { vmin: 0.1, ..LinearLength::default() });
        assert_eq!(linear("75vmax"), LinearLength { vmax: 0.75, ..LinearLength::default() });
    }

    #[test]
    fn units_signs_and_whitespace_are_forgiving_where_css_is() {
        assert_eq!(linear("1EM"), linear("1em"));
        assert_eq!(linear("5VMIN"), linear("5vmin"));
        assert_eq!(linear("  12Px "), LinearLength::px(12.0));
        assert_eq!(linear("-3rem"), LinearLength { rem: -3.0, ..LinearLength::default() });
        assert_eq!(linear("+.5em"), LinearLength { em: 0.5, ..LinearLength::default() });
        assert_eq!(linear("CALC( 1PX  +  2Px )"), LinearLength::px(3.0));
        assert_eq!(linear("0"), LinearLength::px(0.0));
        assert_eq!(linear("-0.0"), LinearLength::px(0.0));
        assert_eq!(linear("0%"), LinearLength::percent(0.0));
    }

    #[test]
    fn vmin_and_vmax_stay_separate_from_vw_and_vh() {
        let length = linear("calc(10vmin + 20vmax + 30vw + 40vh)");
        assert_eq!(length.vmin, 0.1);
        assert_eq!(length.vmax, 0.2);
        assert_eq!(length.vw, 0.3);
        assert_eq!(length.vh, 0.4);
        assert_eq!(linear("calc(10vmin - 10vmin)").vmin, 0.0);
    }

    #[test]
    fn calc_mixes_every_unit_with_plus_and_minus() {
        let length = linear("calc(100% - 20px)");
        assert_eq!((length.percent, length.px), (1.0, -20.0));
        let length = linear("calc(1em + 2rem - 3vw)");
        assert_eq!((length.em, length.rem, length.vw, length.px), (1.0, 2.0, -0.03, 0.0));
        assert_eq!(linear("calc(1px + 1em - 1em)").as_px(), Some(1.0));
    }

    #[test]
    fn calc_scales_a_mixed_length_by_numbers() {
        let length = linear("calc(2 * (1em + 4px))");
        assert_eq!((length.em, length.px), (2.0, 8.0));
        let length = linear("calc((1em + 4px) * 2)");
        assert_eq!((length.em, length.px), (2.0, 8.0));
        assert_eq!(linear("calc(100% / 3)").percent, 1.0 / 3.0);
        assert_eq!(linear("calc((2em + 6px) / 2)"), linear("calc(1em + 3px)"));
        assert_eq!(linear("calc(2 * 3 * 1px)"), LinearLength::px(6.0));
        assert_eq!(linear("calc(calc(1em + calc(2px * 2)) * 2)"), linear("calc(2em + 8px)"));
        assert_eq!(linear("calc((1 + 2) * 1rem)").rem, 3.0);
    }

    #[test]
    fn linear_length_rejects_what_is_not_one_length() {
        for text in [
            "",
            "12",
            "1.5",
            "calc(2 * 3)",
            "calc(0)",
            "12 px",
            "12px 4px",
            "12pxx",
            "12foo",
            "1ch",
            "1fr",
            "px",
            "auto",
            "calc(1 + 2px)",
            "calc(1px * 2em)",
            "calc(1px / 0)",
            "calc(1px / 0px)",
            "calc(1px) junk",
            "1px junk",
        ] {
            assert_eq!(parse_linear_length(text), None, "{text}");
        }
    }

    #[test]
    fn lengths_clamp_instead_of_overflowing() {
        assert_eq!(parse_length("1e300px"), Some(MAX_LENGTH));
        assert_eq!(parse_length("-1e300px"), Some(-MAX_LENGTH));
        assert_eq!(parse_length("1e999px"), Some(MAX_LENGTH));
        assert_eq!(parse_length("calc(1e300px * 1e300)"), None);
        assert_eq!(parse_linear_length("calc(1e300px * 1e300)"), None);
        let huge = linear("calc(1e300% + 1e300em - 1e300vw)");
        assert_eq!((huge.percent, huge.em, huge.vw), (MAX_LENGTH, MAX_LENGTH, -MAX_LENGTH));
        assert_eq!(linear("1e999rem").rem, MAX_LENGTH);
        assert!(f64::from(parse_length("1e300px").unwrap() as f32).is_finite());
    }

    #[test]
    fn a_bare_zero_is_a_length_but_a_calc_of_numbers_is_not() {
        assert_eq!(parse_length("0"), Some(0.0));
        assert_eq!(parse_length("-0.0"), Some(0.0));
        assert_eq!(parse_length("4"), None);
        assert_eq!(parse_length("calc(0)"), None);
        assert_eq!(parse_length("calc(1 - 1)"), None);
    }
}
