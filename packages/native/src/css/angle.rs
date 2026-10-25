//! CSS angles: `<angle>` dimensions and the `calc()` of them.

use super::dimension::scan_dimension;
use super::syntax::function_args;

/// An angle in degrees. A unitless number is only valid for zero, as a literal, and a `calc()`
/// has to come to an angle: `calc(0)` is a number.
pub(crate) fn parse_degrees(token: &str) -> Option<f64> {
    let token = token.trim();
    if let Some(args) = function_args(token, "calc") {
        return match evaluate(args)? {
            Quantity::Angle(degrees) => Some(degrees),
            Quantity::Number(_) => None,
        };
    }
    match dimension(token)? {
        Quantity::Angle(degrees) => Some(degrees),
        Quantity::Number(value) if value == 0.0 => Some(0.0),
        Quantity::Number(_) => None,
    }
}

#[derive(Debug, Clone, Copy)]
enum Quantity {
    Number(f64),
    /// In degrees.
    Angle(f64),
}

impl Quantity {
    fn is_finite(self) -> bool {
        match self {
            Self::Number(value) | Self::Angle(value) => value.is_finite(),
        }
    }

    fn map(self, apply: impl FnOnce(f64) -> f64) -> Self {
        match self {
            Self::Number(value) => Self::Number(apply(value)),
            Self::Angle(value) => Self::Angle(apply(value)),
        }
    }
}

/// The whole of `text` as one number or angle dimension.
fn dimension(text: &str) -> Option<Quantity> {
    let (value, unit, consumed) = scan_dimension(text)?;
    (consumed == text.len()).then_some(())?;
    angle_or_number(value, unit)
}

fn angle_or_number(value: f64, unit: &str) -> Option<Quantity> {
    Some(match unit.to_ascii_lowercase().as_str() {
        "" => Quantity::Number(value),
        "deg" => Quantity::Angle(value),
        "grad" => Quantity::Angle(value * 0.9),
        "rad" => Quantity::Angle(value.to_degrees()),
        "turn" => Quantity::Angle(value * 360.0),
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy)]
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
            tokens.push(Token::Quantity(angle_or_number(value, unit)?));
            rest = &rest[consumed..];
            continue;
        }
        if rest.get(..5).is_some_and(|name| name.eq_ignore_ascii_case("calc(")) {
            tokens.push(Token::Open);
            rest = &rest[5..];
            continue;
        }
        // A sign that does not start a number is an operator; `scan_dimension` takes one that
        // touches its digits into the number, which is how CSS requires the spaces around it.
        match first {
            '+' => tokens.push(Token::Plus),
            '-' => tokens.push(Token::Minus),
            _ => return None,
        }
        rest = &rest[1..];
    }
}

fn evaluate(args: &str) -> Option<Quantity> {
    let tokens = tokenize(args)?;
    let mut parser = Parser { tokens: &tokens, index: 0 };
    let result = parser.sum()?;
    (parser.index == tokens.len() && result.is_finite()).then_some(result)
}

struct Parser<'a> {
    tokens: &'a [Token],
    index: usize,
}

impl Parser<'_> {
    fn sum(&mut self) -> Option<Quantity> {
        let mut left = self.product()?;
        loop {
            let sign = match self.tokens.get(self.index) {
                Some(Token::Plus) => 1.0,
                Some(Token::Minus) => -1.0,
                _ => return Some(left),
            };
            self.index += 1;
            let right = self.product()?;
            left = match (left, right) {
                (Quantity::Number(left), Quantity::Number(right)) => Quantity::Number(left + sign * right),
                (Quantity::Angle(left), Quantity::Angle(right)) => Quantity::Angle(left + sign * right),
                _ => return None,
            };
        }
    }

    fn product(&mut self) -> Option<Quantity> {
        let mut left = self.factor()?;
        loop {
            let divide = match self.tokens.get(self.index) {
                Some(Token::Star) => false,
                Some(Token::Slash) => true,
                _ => return Some(left),
            };
            self.index += 1;
            let right = self.factor()?;
            left = match (left, right, divide) {
                (_, Quantity::Number(divisor), true) if divisor != 0.0 && divisor.is_finite() => {
                    left.map(|value| value / divisor)
                }
                (Quantity::Number(factor), other, false) | (other, Quantity::Number(factor), false) => {
                    other.map(|value| value * factor)
                }
                _ => return None,
            };
        }
    }

    fn factor(&mut self) -> Option<Quantity> {
        let token = *self.tokens.get(self.index)?;
        self.index += 1;
        match token {
            Token::Quantity(quantity) => Some(quantity),
            Token::Open => {
                let inner = self.sum()?;
                matches!(self.tokens.get(self.index), Some(Token::Close)).then_some(())?;
                self.index += 1;
                Some(inner)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(text: &str, expected: f64) {
        let degrees = parse_degrees(text).unwrap_or_else(|| panic!("{text} should be an angle"));
        assert!((degrees - expected).abs() < 1e-9, "{text}: {degrees} != {expected}");
    }

    #[test]
    fn angle_units_convert_to_degrees() {
        close("90deg", 90.0);
        close("100grad", 90.0);
        close("0.25TURN", 90.0);
        close("1rad", 1.0_f64.to_degrees());
        close("0", 0.0);
        assert_eq!(parse_degrees("90"), None);
        assert_eq!(parse_degrees("1px"), None);
    }

    #[test]
    fn calc_sums_angles_and_scales_them_by_numbers() {
        close("calc(10deg + 0.25turn)", 100.0);
        close("calc(1turn - 90deg)", 270.0);
        close("calc(90deg * 2)", 180.0);
        close("calc(2 * 90deg)", 180.0);
        close("calc(180deg / 4)", 45.0);
        close("calc((0.5 - 1) * 90deg)", -45.0);
        close("calc(-45deg * (1 + 1))", -90.0);
        close("calc(calc(10deg + 20deg) * 2)", 60.0);
        close("CALC(1turn / 2)", 180.0);
    }

    #[test]
    fn calc_that_is_not_an_angle_is_invalid() {
        for text in [
            "calc(0)",
            "calc(10deg + 5)",
            "calc(0 + 10deg)",
            "calc(10deg * 10deg)",
            "calc(10deg / 0)",
            "calc(10deg / 5deg)",
            "calc(10deg + 5px)",
            "calc(10deg +5deg)",
            "calc(10deg",
            "calc()",
            "calc(10deg 5deg)",
        ] {
            assert_eq!(parse_degrees(text), None, "{text}");
        }
    }
}
