//! The value types shared by `background-*` longhands and gradients: lengths,
//! angles, `<position>`, `background-size`, `background-repeat`, boxes.

use super::dimension;
use gpui::{
    px, BackgroundBox, BackgroundPosition, BackgroundPositionAxis, BackgroundRepeat, BackgroundSize,
    LengthPercentage, RepeatStyle,
};

/// `12px`, `50%`, a `calc()` of one kind, or the unitless `0`.
pub(crate) fn parse_length_percentage(token: &str) -> Option<LengthPercentage> {
    Some(match dimension::parse_length_percentage(token)? {
        dimension::LengthPercentage::Length(length) => LengthPercentage::Length(px(length as f32)),
        dimension::LengthPercentage::Percent(fraction) => LengthPercentage::Percent(fraction as f32),
    })
}

/// A length or percentage that may not be negative: a radius or a size.
pub(crate) fn parse_non_negative(token: &str) -> Option<LengthPercentage> {
    let value = parse_length_percentage(token)?;
    let amount = match value {
        LengthPercentage::Length(length) => f32::from(length),
        LengthPercentage::Percent(fraction) => fraction,
    };
    (amount >= 0.0).then_some(value)
}

/// `90deg`, `0.25turn`, `1.5rad`, `100grad` or the unitless `0`, in degrees.
pub(crate) fn parse_angle(token: &str) -> Option<f32> {
    let lower = token.to_ascii_lowercase();
    for (unit, scale) in [("deg", 1.0), ("grad", 0.9), ("rad", 180.0 / std::f32::consts::PI), ("turn", 360.0)] {
        if let Some(number) = lower.strip_suffix(unit) {
            let value = number.parse::<f32>().ok().filter(|value| value.is_finite())?;
            return Some(value * scale);
        }
    }
    (lower == "0").then_some(0.0)
}

#[derive(Clone, Copy, PartialEq)]
enum Keyword {
    Left,
    Right,
    Top,
    Bottom,
    Center,
}

fn keyword(token: &str) -> Option<Keyword> {
    Some(match token.to_ascii_lowercase().as_str() {
        "left" => Keyword::Left,
        "right" => Keyword::Right,
        "top" => Keyword::Top,
        "bottom" => Keyword::Bottom,
        "center" => Keyword::Center,
        _ => return None,
    })
}

fn percent(fraction: f32) -> LengthPercentage {
    LengthPercentage::Percent(fraction)
}

/// A CSS `<position>`: one to four values, with keywords.
pub(crate) fn parse_position(tokens: &[&str]) -> Option<BackgroundPosition> {
    let center = BackgroundPositionAxis::from_start(percent(0.5));
    let along = |keyword: Keyword, offset: Option<LengthPercentage>| -> Option<BackgroundPositionAxis> {
        Some(match (keyword, offset) {
            (Keyword::Left | Keyword::Top, None) => BackgroundPositionAxis::from_start(percent(0.0)),
            (Keyword::Right | Keyword::Bottom, None) => BackgroundPositionAxis::from_start(percent(1.0)),
            (Keyword::Center, None) => center,
            (Keyword::Left | Keyword::Top, Some(offset)) => BackgroundPositionAxis::from_start(offset),
            (Keyword::Right | Keyword::Bottom, Some(offset)) => BackgroundPositionAxis::from_end(offset),
            // `center` takes no offset.
            (Keyword::Center, Some(_)) => return None,
        })
    };
    let is_vertical = |keyword: Keyword| matches!(keyword, Keyword::Top | Keyword::Bottom);
    let is_horizontal = |keyword: Keyword| matches!(keyword, Keyword::Left | Keyword::Right);

    match tokens {
        [one] => match keyword(one) {
            Some(word) if is_vertical(word) => Some(BackgroundPosition { x: center, y: along(word, None)? }),
            Some(word) => Some(BackgroundPosition { x: along(word, None)?, y: center }),
            None => Some(BackgroundPosition {
                x: BackgroundPositionAxis::from_start(parse_length_percentage(one)?),
                y: center,
            }),
        },
        [first, second] => {
            let axis = |token: &str, vertical: bool| -> Option<BackgroundPositionAxis> {
                match keyword(token) {
                    Some(word) if vertical != is_vertical(word) && word != Keyword::Center => None,
                    Some(word) => along(word, None),
                    None => Some(BackgroundPositionAxis::from_start(parse_length_percentage(token)?)),
                }
            };
            // `top left` is `left top`: a keyword that can only be vertical comes first.
            let swapped = match (keyword(first), keyword(second)) {
                (Some(first), Some(second)) => is_vertical(first) || is_horizontal(second),
                _ => false,
            };
            let (x, y) = if swapped { (second, first) } else { (first, second) };
            Some(BackgroundPosition { x: axis(x, false)?, y: axis(y, true)? })
        }
        [_, _, _] | [_, _, _, _] => {
            // `<keyword> [<offset>]` pairs, in either axis order.
            let mut x = None;
            let mut y = None;
            let mut index = 0;
            while index < tokens.len() {
                let word = keyword(tokens[index])?;
                let offset = tokens.get(index + 1).and_then(|token| parse_length_percentage(token));
                index += if offset.is_some() { 2 } else { 1 };
                let slot = if is_vertical(word) {
                    &mut y
                } else if is_horizontal(word) {
                    &mut x
                } else if x.is_none() {
                    &mut x
                } else {
                    &mut y
                };
                if slot.replace(along(word, offset)?).is_some() {
                    return None;
                }
            }
            Some(BackgroundPosition { x: x.unwrap_or(center), y: y.unwrap_or(center) })
        }
        _ => None,
    }
}

/// `background-size`: `auto`, `cover`, `contain`, or one or two sizes.
pub(crate) fn parse_size(tokens: &[&str]) -> Option<BackgroundSize> {
    let dimension = |token: &str| -> Option<Option<LengthPercentage>> {
        if token.eq_ignore_ascii_case("auto") {
            Some(None)
        } else {
            parse_non_negative(token).map(Some)
        }
    };
    match tokens {
        [one] if one.eq_ignore_ascii_case("cover") => Some(BackgroundSize::Cover),
        [one] if one.eq_ignore_ascii_case("contain") => Some(BackgroundSize::Contain),
        [one] => Some(BackgroundSize::Explicit { width: dimension(one)?, height: None }),
        [width, height] => Some(BackgroundSize::Explicit { width: dimension(width)?, height: dimension(height)? }),
        _ => None,
    }
}

/// `background-repeat`: `repeat-x`, `repeat-y`, or one or two of
/// `repeat | space | round | no-repeat`.
pub(crate) fn parse_repeat(tokens: &[&str]) -> Option<BackgroundRepeat> {
    let style = |token: &str| match token.to_ascii_lowercase().as_str() {
        "repeat" => Some(RepeatStyle::Repeat),
        "space" => Some(RepeatStyle::Space),
        "round" => Some(RepeatStyle::Round),
        "no-repeat" => Some(RepeatStyle::NoRepeat),
        _ => None,
    };
    match tokens {
        [one] if one.eq_ignore_ascii_case("repeat-x") => {
            Some(BackgroundRepeat { x: RepeatStyle::Repeat, y: RepeatStyle::NoRepeat })
        }
        [one] if one.eq_ignore_ascii_case("repeat-y") => {
            Some(BackgroundRepeat { x: RepeatStyle::NoRepeat, y: RepeatStyle::Repeat })
        }
        [one] => {
            let both = style(one)?;
            Some(BackgroundRepeat { x: both, y: both })
        }
        [x, y] => Some(BackgroundRepeat { x: style(x)?, y: style(y)? }),
        _ => None,
    }
}

/// `border-box`, `padding-box` or `content-box`.
pub(crate) fn parse_box(token: &str) -> Option<BackgroundBox> {
    match token.to_ascii_lowercase().as_str() {
        "border-box" => Some(BackgroundBox::BorderBox),
        "padding-box" => Some(BackgroundBox::PaddingBox),
        "content-box" => Some(BackgroundBox::ContentBox),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(value: &str) -> Option<BackgroundPosition> {
        parse_position(&value.split_whitespace().collect::<Vec<_>>())
    }

    fn at(x: LengthPercentage, y: LengthPercentage) -> BackgroundPosition {
        BackgroundPosition::new(x, y)
    }

    #[test]
    fn lengths_percentages_and_angles() {
        assert_eq!(parse_length_percentage("12px"), Some(LengthPercentage::Length(px(12.0))));
        assert_eq!(parse_length_percentage("50%"), Some(LengthPercentage::Percent(0.5)));
        assert_eq!(parse_length_percentage("0"), Some(LengthPercentage::Length(px(0.0))));
        assert_eq!(parse_length_percentage("12"), None);
        assert_eq!(parse_length_percentage("12em"), None);
        assert_eq!(parse_angle("90deg"), Some(90.0));
        assert_eq!(parse_angle("0.25turn"), Some(90.0));
        assert_eq!(parse_angle("200grad"), Some(180.0));
        assert!((parse_angle("3.14159265rad").unwrap() - 180.0).abs() < 1e-3);
        assert_eq!(parse_angle("0"), Some(0.0));
        assert_eq!(parse_angle("90"), None);
    }

    #[test]
    fn one_and_two_value_positions() {
        let half = LengthPercentage::Percent(0.5);
        let zero = LengthPercentage::Percent(0.0);
        let full = LengthPercentage::Percent(1.0);
        assert_eq!(pos("center"), Some(at(half, half)));
        assert_eq!(pos("left"), Some(at(zero, half)));
        assert_eq!(pos("bottom"), Some(at(half, full)));
        assert_eq!(pos("10px"), Some(at(LengthPercentage::Length(px(10.0)), half)));
        assert_eq!(pos("right bottom"), Some(at(full, full)));
        assert_eq!(pos("bottom right"), Some(at(full, full)));
        assert_eq!(pos("top left"), Some(at(zero, zero)));
        assert_eq!(pos("20% 30px"), Some(at(LengthPercentage::Percent(0.2), LengthPercentage::Length(px(30.0)))));
        assert_eq!(pos("left right"), None);
        assert_eq!(pos("top 10px"), None);
    }

    #[test]
    fn three_and_four_value_positions_measure_from_an_edge() {
        let parsed = pos("right 10px bottom 20%").unwrap();
        assert_eq!(parsed.x, BackgroundPositionAxis::from_end(LengthPercentage::Length(px(10.0))));
        assert_eq!(parsed.y, BackgroundPositionAxis::from_end(LengthPercentage::Percent(0.2)));
        let parsed = pos("bottom 5px right 8px").unwrap();
        assert_eq!(parsed.x, BackgroundPositionAxis::from_end(LengthPercentage::Length(px(8.0))));
        let parsed = pos("left 4px top").unwrap();
        assert_eq!(parsed.x, BackgroundPositionAxis::from_start(LengthPercentage::Length(px(4.0))));
        assert_eq!(parsed.y, BackgroundPositionAxis::from_start(LengthPercentage::Percent(0.0)));
        assert_eq!(pos("left 4px left 5px"), None);
    }

    #[test]
    fn sizes() {
        let tokens = |value: &'static str| value.split_whitespace().collect::<Vec<_>>();
        assert_eq!(parse_size(&tokens("cover")), Some(BackgroundSize::Cover));
        assert_eq!(parse_size(&tokens("contain")), Some(BackgroundSize::Contain));
        assert_eq!(
            parse_size(&tokens("20px auto")),
            Some(BackgroundSize::Explicit { width: Some(LengthPercentage::Length(px(20.0))), height: None })
        );
        assert_eq!(
            parse_size(&tokens("50%")),
            Some(BackgroundSize::Explicit { width: Some(LengthPercentage::Percent(0.5)), height: None })
        );
        assert_eq!(parse_size(&tokens("cover cover")), None);
        assert_eq!(parse_size(&tokens("1 2")), None);
    }

    #[test]
    fn repeats_and_boxes() {
        let tokens = |value: &'static str| value.split_whitespace().collect::<Vec<_>>();
        assert_eq!(
            parse_repeat(&tokens("repeat-x")),
            Some(BackgroundRepeat { x: RepeatStyle::Repeat, y: RepeatStyle::NoRepeat })
        );
        assert_eq!(
            parse_repeat(&tokens("space round")),
            Some(BackgroundRepeat { x: RepeatStyle::Space, y: RepeatStyle::Round })
        );
        assert_eq!(
            parse_repeat(&tokens("no-repeat")),
            Some(BackgroundRepeat { x: RepeatStyle::NoRepeat, y: RepeatStyle::NoRepeat })
        );
        assert_eq!(parse_repeat(&tokens("repeat-x repeat")), None);
        assert_eq!(parse_box("Content-Box"), Some(BackgroundBox::ContentBox));
        assert_eq!(parse_box("text"), None);
    }

    #[test]
    fn lengths_are_case_insensitive_signed_and_may_be_calc() {
        assert_eq!(parse_length_percentage("12PX"), Some(LengthPercentage::Length(px(12.0))));
        assert_eq!(parse_length_percentage("+12Px"), Some(LengthPercentage::Length(px(12.0))));
        assert_eq!(parse_length_percentage("-0.5in"), Some(LengthPercentage::Length(px(-48.0))));
        assert_eq!(parse_length_percentage("+50%"), Some(LengthPercentage::Percent(0.5)));
        assert_eq!(parse_length_percentage("calc(10px + 5px)"), Some(LengthPercentage::Length(px(15.0))));
        assert_eq!(parse_length_percentage("calc(50% + 25%)"), Some(LengthPercentage::Percent(0.75)));
        assert_eq!(parse_length_percentage("calc(2 * 10%)"), Some(LengthPercentage::Percent(0.2)));
        // gpui has no length-plus-percentage value.
        assert_eq!(parse_length_percentage("calc(50% - 10px)"), None);
        assert_eq!(parse_length_percentage("12vw"), None);
    }
}
