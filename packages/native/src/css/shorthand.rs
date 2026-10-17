//! The box shorthands that take several values: `padding`, `margin`,
//! `border-width` and `inset` (one to four, for the sides) and `gap` (one or
//! two, for the axes).
//!
//! A shorthand is kept as its own style field holding one value per side or
//! axis, so a state style that sets it still replaces the longhands it covers
//! and an explicit longhand still beats it. [`parse_sides`] reads the text and
//! the consumers read the side they need from the [`Sides`].

use std::fmt;

use serde::de::{self, Visitor};
use serde::{Deserializer, Serialize, Serializer};

use super::dimension::MAX_LENGTH;
use super::length::LinearLength;
use super::syntax::{expand_sides, split_whitespace};

/// One value per side, in the order CSS lists them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sides<T> {
    pub top: T,
    pub right: T,
    pub bottom: T,
    pub left: T,
}

impl<T> Sides<T> {
    pub fn all(value: T) -> Self
    where
        T: Clone,
    {
        Self { top: value.clone(), right: value.clone(), bottom: value.clone(), left: value }
    }

    pub(crate) fn from_array([top, right, bottom, left]: [T; 4]) -> Self {
        Self { top, right, bottom, left }
    }

    /// `None` when `apply` gives none for any side.
    pub(crate) fn try_map<U>(self, mut apply: impl FnMut(T) -> Option<U>) -> Option<Sides<U>> {
        Some(Sides {
            top: apply(self.top)?,
            right: apply(self.right)?,
            bottom: apply(self.bottom)?,
            left: apply(self.left)?,
        })
    }

    pub(crate) fn values(&self) -> [&T; 4] {
        [&self.top, &self.right, &self.bottom, &self.left]
    }

    fn is_uniform(&self) -> bool
    where
        T: PartialEq,
    {
        self.top == self.right && self.right == self.bottom && self.bottom == self.left
    }
}

/// `gap`: the row gap (between rows) and the column gap (between columns).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gap {
    pub row: LinearLength,
    pub column: LinearLength,
}

impl Gap {
    pub fn all(length: LinearLength) -> Self {
        Self { row: length, column: length }
    }
}

/// A single value when every side agrees, which is what the shorthand was
/// usually written as; otherwise the four values, which parse back to the same.
impl<T: Serialize + fmt::Display + PartialEq> Serialize for Sides<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.is_uniform() {
            self.top.serialize(serializer)
        } else {
            serializer.collect_str(&format_args!("{} {} {} {}", self.top, self.right, self.bottom, self.left))
        }
    }
}

impl Serialize for Gap {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.row == self.column {
            self.row.serialize(serializer)
        } else {
            serializer.collect_str(&format_args!("{} {}", self.row, self.column))
        }
    }
}

/// `text` as one to `max` whitespace-separated values, each read by `parse_one`.
/// A `calc()` with spaces inside is one value.
fn parse_values<T>(
    text: &str,
    max: usize,
    mut parse_one: impl FnMut(&str) -> Result<T, String>,
) -> Result<Vec<T>, String> {
    let tokens = split_whitespace(text);
    if tokens.is_empty() || tokens.len() > max {
        let expected = if max == 2 { "one or two values" } else { "one to four values" };
        return Err(format!("invalid \"{text}\": expected {expected}, got {}", tokens.len()));
    }
    tokens.into_iter().map(&mut parse_one).collect()
}

/// A CSS one to four value string (`"1px"`, `"1px 2px"`, `"1px 2px 3px"`,
/// `"1px 2px 3px 4px"`) as one value per side, with the usual fill-in: the
/// missing sides copy the opposite one. `parse_one` reads each value and
/// names the value and the rule in its error.
pub(crate) fn parse_sides<T: Clone>(
    text: &str,
    parse_one: impl FnMut(&str) -> Result<T, String>,
) -> Result<Sides<T>, String> {
    let values = parse_values(text, 4, parse_one)?;
    Ok(Sides::from_array(expand_sides(&values).expect("parse_values returned one to four values")))
}

/// `gap`'s `<row-gap> <column-gap>?` string: one value is both.
pub(crate) fn parse_gap(
    text: &str,
    parse_one: impl FnMut(&str) -> Result<LinearLength, String>,
) -> Result<Gap, String> {
    match parse_values(text, 2, parse_one)?[..] {
        [both] => Ok(Gap::all(both)),
        [row, column] => Ok(Gap { row, column }),
        _ => unreachable!("one or two values were checked"),
    }
}

/// A length parser that reports itself as `"invalid {what} \"{token}\": {rule}"`.
pub(crate) fn length_token(
    what: &'static str,
    rule: &'static str,
    parse: fn(&str) -> Option<LinearLength>,
) -> impl Fn(&str) -> Result<LinearLength, String> {
    move |token| parse(token).ok_or_else(|| format!("invalid {what} \"{token}\": {rule}"))
}

/// A number of px as a length, clamped like a lone px length.
pub(crate) fn px_length(value: f64, what: &str) -> Result<LinearLength, String> {
    if !value.is_finite() {
        return Err(format!("invalid {what} {value}: not a finite number"));
    }
    Ok(LinearLength::px(value.clamp(-MAX_LENGTH, MAX_LENGTH)))
}

/// A style field that is a number of px or a CSS string, or `null`. `from_number`
/// and `from_text` build the field's value, and their errors reach the caller.
pub(crate) fn deserialize_optional<'de, D, T>(
    deserializer: D,
    expecting: &'static str,
    from_number: impl Fn(f64) -> Result<T, String>,
    from_text: impl Fn(&str) -> Result<T, String>,
) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
{
    struct Value<'a, N, S> {
        expecting: &'static str,
        from_number: &'a N,
        from_text: &'a S,
    }

    impl<'de, T, N: Fn(f64) -> Result<T, String>, S: Fn(&str) -> Result<T, String>> Visitor<'de> for Value<'_, N, S> {
        type Value = T;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(self.expecting)
        }

        fn visit_f64<E: de::Error>(self, value: f64) -> Result<T, E> {
            (self.from_number)(value).map_err(E::custom)
        }

        fn visit_i64<E: de::Error>(self, value: i64) -> Result<T, E> {
            self.visit_f64(value as f64)
        }

        fn visit_u64<E: de::Error>(self, value: u64) -> Result<T, E> {
            self.visit_f64(value as f64)
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<T, E> {
            (self.from_text)(value).map_err(E::custom)
        }
    }

    struct Optional<'a, N, S> {
        expecting: &'static str,
        from_number: &'a N,
        from_text: &'a S,
    }

    impl<'de, T, N: Fn(f64) -> Result<T, String>, S: Fn(&str) -> Result<T, String>> Visitor<'de> for Optional<'_, N, S> {
        type Value = Option<T>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str(self.expecting)
        }

        fn visit_none<E: de::Error>(self) -> Result<Option<T>, E> {
            Ok(None)
        }

        fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Option<T>, D::Error> {
            deserializer
                .deserialize_any(Value {
                    expecting: self.expecting,
                    from_number: self.from_number,
                    from_text: self.from_text,
                })
                .map(Some)
        }
    }

    deserializer.deserialize_option(Optional { expecting, from_number: &from_number, from_text: &from_text })
}

/// `padding` and `border-width`: one to four lengths, or a number of px.
pub(crate) fn deserialize_length_sides<'de, D: Deserializer<'de>>(
    deserializer: D,
    what: &'static str,
    rule: &'static str,
    parse: fn(&str) -> Option<LinearLength>,
) -> Result<Option<Sides<LinearLength>>, D::Error> {
    let token = length_token(what, rule, parse);
    deserialize_optional(
        deserializer,
        "a number of px or a string of one to four CSS lengths",
        |value| px_length(value, what).map(Sides::all),
        |text| parse_sides(text, &token),
    )
}

/// `gap`: a number of px or one or two CSS lengths.
pub(crate) fn deserialize_gap<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Gap>, D::Error> {
    let token = length_token("gap", "a CSS length with a unit, or calc()", super::dimension::parse_linear_length);
    deserialize_optional(
        deserializer,
        "a number of px or a string of one or two CSS lengths",
        |value| px_length(value, "gap").map(Gap::all),
        |text| parse_gap(text, &token),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::dimension::parse_linear_length;

    fn length(text: &str) -> LinearLength {
        parse_linear_length(text).unwrap_or_else(|| panic!("{text} should parse"))
    }

    fn sides(text: &str) -> Result<Sides<LinearLength>, String> {
        parse_sides(text, length_token("padding", "a length", parse_linear_length))
    }

    #[test]
    fn one_to_four_values_fill_in_like_css() {
        let (a, b, c, d) = (length("1px"), length("2px"), length("3px"), length("4px"));
        assert_eq!(sides("1px"), Ok(Sides::all(a)));
        assert_eq!(sides("1px 2px"), Ok(Sides { top: a, right: b, bottom: a, left: b }));
        assert_eq!(sides("1px 2px 3px"), Ok(Sides { top: a, right: b, bottom: c, left: b }));
        assert_eq!(sides("1px 2px 3px 4px"), Ok(Sides { top: a, right: b, bottom: c, left: d }));
        assert_eq!(sides("  1px \t 2px\n"), sides("1px 2px"));
    }

    #[test]
    fn a_calc_with_spaces_is_one_value() {
        let sides = sides("calc(1px + 1em) 2px").unwrap();
        assert_eq!(sides.top, length("calc(1px + 1em)"));
        assert_eq!(sides.right, length("2px"));
        assert_eq!(sides.bottom, length("calc(1px + 1em)"));
    }

    #[test]
    fn no_values_too_many_or_a_bad_one_is_an_error() {
        for text in ["", "   ", "1px 2px 3px 4px 5px"] {
            let error = sides(text).unwrap_err();
            assert!(error.contains("one to four values"), "{text}: {error}");
        }
        let error = sides("1px 12").unwrap_err();
        assert!(error.contains("invalid padding \"12\""), "{error}");
        assert!(sides("1px, 2px").is_err());
        assert!(sides("calc(1px + 2) 1px").is_err());
    }

    #[test]
    fn a_gap_is_row_then_column() {
        let gap = |text: &str| parse_gap(text, length_token("gap", "a length", parse_linear_length));
        assert_eq!(gap("4px"), Ok(Gap::all(length("4px"))));
        assert_eq!(gap("4px 8px"), Ok(Gap { row: length("4px"), column: length("8px") }));
        assert!(gap("4px 8px 1px").unwrap_err().contains("one or two values"));
        assert!(gap("").is_err());
    }

    #[test]
    fn serializes_as_one_value_when_uniform_and_four_otherwise() {
        let json = |sides: &Sides<LinearLength>| serde_json::to_string(sides).unwrap();
        assert_eq!(json(&Sides::all(length("4px"))), "4.0");
        assert_eq!(json(&Sides::all(length("2em"))), "\"2em\"");
        let mixed = sides("1px 2em").unwrap();
        assert_eq!(json(&mixed), "\"1px 2em 1px 2em\"");
        assert_eq!(sides(&format!("{} {} {} {}", mixed.top, mixed.right, mixed.bottom, mixed.left)), Ok(mixed));
        assert_eq!(serde_json::to_string(&Gap { row: length("4px"), column: length("8px") }).unwrap(), "\"4px 8px\"");
        assert_eq!(serde_json::to_string(&Gap::all(length("4px"))).unwrap(), "4.0");
    }
}
