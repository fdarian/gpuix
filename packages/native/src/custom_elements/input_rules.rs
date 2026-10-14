//! Pure rules for `<input>`: `type`, password masking, `maxLength` and the
//! number stepper. No gpui here, so everything is unit-testable.

use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum InputType {
    #[default]
    Text,
    Password,
    Number,
    Email,
    Search,
    Tel,
    Url,
}

impl InputType {
    /// An unknown token falls back to `text`, as the DOM does.
    pub(crate) fn parse(token: &str) -> Self {
        match token.trim().to_ascii_lowercase().as_str() {
            "password" => Self::Password,
            "number" => Self::Number,
            "email" => Self::Email,
            "search" => Self::Search,
            "tel" => Self::Tel,
            "url" => Self::Url,
            _ => Self::Text,
        }
    }
}

const MASK: &str = "\u{2022}";

/// One bullet per grapheme, like a browser's password field.
pub(crate) fn masked(text: &str) -> String {
    MASK.repeat(text.graphemes(true).count())
}

/// Byte offset in `masked(text)` for a byte offset in `text`. An offset inside
/// a grapheme maps past it, so a caret never lands between the halves.
pub(crate) fn content_to_masked(text: &str, offset: usize) -> usize {
    text.grapheme_indices(true)
        .take_while(|(index, _)| *index < offset)
        .count()
        * MASK.len()
}

/// Byte offset in `text` for a byte offset in `masked(text)`.
pub(crate) fn masked_to_content(text: &str, masked_offset: usize) -> usize {
    text.grapheme_indices(true)
        .nth(masked_offset / MASK.len())
        .map(|(index, _)| index)
        .unwrap_or(text.len())
}

/// The longest prefix of `text` that fits in `allowed` UTF-16 code units
/// without splitting a character.
pub(crate) fn truncate_to_utf16(text: &str, allowed: usize) -> &str {
    let mut units = 0;
    for (index, character) in text.char_indices() {
        units += character.len_utf16();
        if units > allowed {
            return &text[..index];
        }
    }
    text
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NumberRules {
    pub(crate) min: Option<f64>,
    pub(crate) max: Option<f64>,
    pub(crate) step: f64,
}

/// Accepts a JSON number or a numeric string, like the DOM attribute.
pub(crate) fn number_from_json(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
    .filter(|number| number.is_finite())
}

/// The step in effect: `step` when it is a positive number, else 1.
/// `step="any"` also lands on 1, which is what browsers use for the arrow keys.
pub(crate) fn step_from_json(value: Option<&serde_json::Value>) -> f64 {
    value
        .and_then(number_from_json)
        .filter(|step| *step > 0.0)
        .unwrap_or(1.0)
}

fn decimals(value: f64) -> usize {
    format!("{value}")
        .split_once('.')
        .map(|(_, fraction)| fraction.len())
        .unwrap_or(0)
        .min(15)
}

/// The text after one ArrowUp (`direction` 1) or ArrowDown (-1), or `None`
/// when it would not change. Text that is not a number steps from 0 and
/// replaces the text, as `stepUp()` does.
pub(crate) fn step_number(current: &str, direction: f64, rules: NumberRules) -> Option<String> {
    let base = current
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite());
    let mut next = base.unwrap_or(0.0) + direction * rules.step;
    if let Some(max) = rules.max {
        next = next.min(max);
    }
    if let Some(min) = rules.min {
        next = next.max(min);
    }
    let places = decimals(rules.step)
        .max(base.map(decimals).unwrap_or(0))
        .max(decimals(next));
    let scale = 10f64.powi(places as i32);
    let next = (next * scale).round() / scale;
    (base != Some(next)).then(|| format!("{next}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNIT: NumberRules = NumberRules {
        min: None,
        max: None,
        step: 1.0,
    };

    #[test]
    fn unknown_types_are_text() {
        assert_eq!(InputType::parse("password"), InputType::Password);
        assert_eq!(InputType::parse("Search"), InputType::Search);
        assert_eq!(InputType::parse("date"), InputType::Text);
        assert_eq!(InputType::parse(""), InputType::Text);
    }

    #[test]
    fn masks_one_bullet_per_grapheme() {
        assert_eq!(masked("ab"), "\u{2022}\u{2022}");
        assert_eq!(masked("e\u{301}"), "\u{2022}");
        assert_eq!(masked("\u{1F600}x"), "\u{2022}\u{2022}");
        assert_eq!(masked(""), "");
    }

    #[test]
    fn mask_offsets_round_trip_on_grapheme_boundaries() {
        let text = "a\u{1F600}b";
        assert_eq!(content_to_masked(text, 0), 0);
        assert_eq!(content_to_masked(text, 1), 3);
        assert_eq!(content_to_masked(text, 5), 6);
        assert_eq!(content_to_masked(text, text.len()), 9);
        assert_eq!(masked_to_content(text, 0), 0);
        assert_eq!(masked_to_content(text, 3), 1);
        assert_eq!(masked_to_content(text, 6), 5);
        assert_eq!(masked_to_content(text, 9), text.len());
        // Inside the emoji: rounds up to its end.
        assert_eq!(content_to_masked(text, 3), 6);
    }

    #[test]
    fn truncates_in_utf16_units_without_splitting_a_character() {
        assert_eq!(truncate_to_utf16("hello", 3), "hel");
        assert_eq!(truncate_to_utf16("hello", 9), "hello");
        assert_eq!(truncate_to_utf16("hello", 0), "");
        // The emoji is two UTF-16 units: one unit left cannot hold it.
        assert_eq!(truncate_to_utf16("a\u{1F600}", 2), "a");
        assert_eq!(truncate_to_utf16("a\u{1F600}", 3), "a\u{1F600}");
    }

    #[test]
    fn steps_by_one_and_drops_the_trailing_zero() {
        assert_eq!(step_number("4", 1.0, UNIT).as_deref(), Some("5"));
        assert_eq!(step_number("4", -1.0, UNIT).as_deref(), Some("3"));
        assert_eq!(step_number("-1", 1.0, UNIT).as_deref(), Some("0"));
    }

    #[test]
    fn steps_by_a_decimal_without_float_noise() {
        let rules = NumberRules {
            step: 0.1,
            ..UNIT
        };
        assert_eq!(step_number("0.2", 1.0, rules).as_deref(), Some("0.3"));
        assert_eq!(step_number("0.3", -1.0, rules).as_deref(), Some("0.2"));
        let half = NumberRules {
            step: 0.5,
            ..UNIT
        };
        assert_eq!(step_number("1", 1.0, half).as_deref(), Some("1.5"));
        assert_eq!(step_number("1.5", 1.0, half).as_deref(), Some("2"));
    }

    #[test]
    fn clamps_to_min_and_max() {
        let rules = NumberRules {
            min: Some(0.0),
            max: Some(10.0),
            step: 1.0,
        };
        assert_eq!(step_number("10", 1.0, rules), None);
        assert_eq!(step_number("0", -1.0, rules), None);
        assert_eq!(step_number("9.5", 1.0, rules).as_deref(), Some("10"));
        assert_eq!(step_number("50", -1.0, rules).as_deref(), Some("10"));
    }

    #[test]
    fn text_that_is_not_a_number_steps_from_zero() {
        assert_eq!(step_number("", 1.0, UNIT).as_deref(), Some("1"));
        assert_eq!(step_number("abc", -1.0, UNIT).as_deref(), Some("-1"));
        let floor = NumberRules {
            min: Some(5.0),
            ..UNIT
        };
        assert_eq!(step_number("", 1.0, floor).as_deref(), Some("5"));
    }

    #[test]
    fn step_attribute_accepts_numbers_and_numeric_strings() {
        assert_eq!(step_from_json(Some(&serde_json::json!(0.25))), 0.25);
        assert_eq!(step_from_json(Some(&serde_json::json!("2"))), 2.0);
        assert_eq!(step_from_json(Some(&serde_json::json!("any"))), 1.0);
        assert_eq!(step_from_json(Some(&serde_json::json!(-3))), 1.0);
        assert_eq!(step_from_json(None), 1.0);
    }
}
