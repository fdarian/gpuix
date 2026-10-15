//! Numbers, lengths, percentages and `calc()` over them, shared by every value
//! parser that reads a dimension.
//!
//! Only absolute length units resolve: `em`, `rem` and the viewport units need
//! the element's font size or the window, which the parsers do not receive, so
//! they are invalid here rather than silently wrong.

/// The largest length a value may take. Browsers clamp an out-of-range length
/// instead of rejecting it (Chrome's `LayoutUnit` saturates here), and gpui
/// would otherwise cast a huge `f64` to an infinite `f32`.
pub(crate) const MAX_LENGTH: f64 = 33_554_431.0;

/// What a token or a `calc()` expression evaluates to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Quantity {
    Number(f64),
    /// In px.
    Length(f64),
    /// In percent, so `50%` is `Percent(50.0)`.
    Percent(f64),
}

impl Quantity {
    fn value(self) -> f64 {
        match self {
            Self::Number(value) | Self::Length(value) | Self::Percent(value) => value,
        }
    }

    fn with_value(self, value: f64) -> Self {
        match self {
            Self::Number(_) => Self::Number(value),
            Self::Length(_) => Self::Length(value),
            Self::Percent(_) => Self::Percent(value),
        }
    }

    fn same_kind(self, other: Self) -> bool {
        std::mem::discriminant(&self) == std::mem::discriminant(&other)
    }
}

/// Parses a number, an absolute length, a percentage or a `calc()` of them.
/// `None` for anything else, including units the parsers cannot resolve and
/// `calc()` that mixes lengths with percentages: no value can hold both.
pub(crate) fn parse_quantity(token: &str) -> Option<Quantity> {
    let token = token.trim();
    if let Some(args) = super::syntax::function_args(token, "calc") {
        return evaluate(args);
    }
    let (value, unit) = split_dimension(token)?;
    quantity(value, &unit)
}

fn quantity(value: f64, unit: &str) -> Option<Quantity> {
    Some(match unit {
        "" => Quantity::Number(value),
        "%" => Quantity::Percent(value),
        _ => Quantity::Length(value * px_per(unit)?),
    })
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

/// A length in px, clamped to [`MAX_LENGTH`]. A bare number is only a length
/// when it is zero.
pub(crate) fn parse_length(token: &str) -> Option<f64> {
    match parse_quantity(token)? {
        Quantity::Length(value) => Some(clamp_length(value)),
        _ if is_zero_literal(token) => Some(0.0),
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
        Quantity::Length(value) => Some(LengthPercentage::Length(clamp_length(value))),
        Quantity::Percent(value) => Some(LengthPercentage::Percent(value / 100.0)),
        Quantity::Number(_) if is_zero_literal(token) => Some(LengthPercentage::Length(0.0)),
        Quantity::Number(_) => None,
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
        Quantity::Percent(value) => Some(value / 100.0),
        Quantity::Length(_) => None,
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
    (parser.index == tokens.len() && result.value().is_finite()).then_some(result)
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
            if !left.same_kind(right) {
                return None;
            }
            let sign = if subtract { -1.0 } else { 1.0 };
            left = left.with_value(left.value() + sign * right.value());
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
                (_, Quantity::Number(divisor), true) if divisor != 0.0 => {
                    left.with_value(left.value() / divisor)
                }
                (Quantity::Number(factor), other, false) => other.with_value(factor * other.value()),
                (other, Quantity::Number(factor), false) => other.with_value(other.value() * factor),
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

    #[test]
    fn units_resolve_to_px_and_only_absolute_ones() {
        assert_eq!(parse_quantity("1in"), Some(Quantity::Length(96.0)));
        assert_eq!(parse_quantity("1PC"), Some(Quantity::Length(16.0)));
        assert_eq!(parse_quantity("3"), Some(Quantity::Number(3.0)));
        assert_eq!(parse_quantity("50%"), Some(Quantity::Percent(50.0)));
        for unit in ["em", "rem", "vw", "vh", "vmin", "vmax", "ch", "fr"] {
            assert_eq!(parse_quantity(&format!("1{unit}")), None, "{unit}");
        }
    }

    #[test]
    fn calc_follows_operator_precedence_and_nests() {
        let calc = |text: &str| parse_quantity(text);
        assert_eq!(calc("calc(1px + 2px * 3)"), Some(Quantity::Length(7.0)));
        assert_eq!(calc("calc((1px + 2px) * 3)"), Some(Quantity::Length(9.0)));
        assert_eq!(calc("calc(10px / 4)"), Some(Quantity::Length(2.5)));
        assert_eq!(calc("calc(1px - -2px)"), Some(Quantity::Length(3.0)));
        assert_eq!(calc("calc(calc(1px + 1px) * 2)"), Some(Quantity::Length(4.0)));
        assert_eq!(calc("calc(100% / 3)"), Some(Quantity::Percent(100.0 / 3.0)));
        assert_eq!(calc("calc(2 * 3)"), Some(Quantity::Number(6.0)));
        assert_eq!(calc("calc(1in + 24px)"), Some(Quantity::Length(120.0)));
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
            "calc(1px + 50%)",
            "calc(1px + 1em)",
            "calc((1px)",
            "calc(1px))",
            "calc(1px) 2px",
        ] {
            assert_eq!(parse_quantity(text), None, "{text}");
        }
    }

    #[test]
    fn lengths_clamp_instead_of_overflowing() {
        assert_eq!(parse_length("1e300px"), Some(MAX_LENGTH));
        assert_eq!(parse_length("-1e300px"), Some(-MAX_LENGTH));
        assert_eq!(parse_length("1e999px"), Some(MAX_LENGTH));
        assert_eq!(parse_length("calc(1e300px * 1e300)"), None);
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
