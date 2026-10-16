//! A CSS length as a linear expression over the units a length can mix, and
//! the context that turns one into px.
//!
//! The expression is built once, when the style is set, and resolved when the
//! tree is applied, because `em`, `rem`, `%` and the viewport units only have a
//! value once the element's font size, its containing block and the window are
//! known.

use std::fmt;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::dimension::{parse_linear_length, MAX_LENGTH};

/// `px + percent * basis + em * font-size + ...`. The relative terms are
/// fractions or multipliers: `50%` and `50vw` are `0.5`, `2em` is `2.0`.
/// `vmin` and `vmax` stay separate because which viewport side they follow is
/// only known against a window.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LinearLength {
    pub(crate) px: f64,
    pub(crate) percent: f64,
    pub(crate) em: f64,
    pub(crate) rem: f64,
    pub(crate) vw: f64,
    pub(crate) vh: f64,
    pub(crate) vmin: f64,
    pub(crate) vmax: f64,
}

impl LinearLength {
    pub(crate) const fn px(px: f64) -> Self {
        Self { px, percent: 0.0, em: 0.0, rem: 0.0, vw: 0.0, vh: 0.0, vmin: 0.0, vmax: 0.0 }
    }

    /// `fraction` is `0.5` for `50%`.
    pub(crate) fn percent(fraction: f64) -> Self {
        Self { percent: fraction, ..Self::default() }
    }

    fn terms(&self) -> [f64; 8] {
        [self.px, self.percent, self.em, self.rem, self.vw, self.vh, self.vmin, self.vmax]
    }

    fn from_terms(terms: [f64; 8]) -> Self {
        Self {
            px: terms[0],
            percent: terms[1],
            em: terms[2],
            rem: terms[3],
            vw: terms[4],
            vh: terms[5],
            vmin: terms[6],
            vmax: terms[7],
        }
    }

    pub(crate) fn plus(self, other: Self) -> Self {
        let mut terms = self.terms();
        for (term, addend) in terms.iter_mut().zip(other.terms()) {
            *term += addend;
        }
        Self::from_terms(terms)
    }

    pub(crate) fn minus(self, other: Self) -> Self {
        self.plus(other.map(|term| -term))
    }

    pub(crate) fn map(self, apply: impl FnMut(f64) -> f64) -> Self {
        Self::from_terms(self.terms().map(apply))
    }

    /// Every term clamped to [`MAX_LENGTH`], the way a lone px length is.
    pub(crate) fn clamped(self) -> Self {
        self.map(|term| term.clamp(-MAX_LENGTH, MAX_LENGTH))
    }

    pub(crate) fn is_finite(&self) -> bool {
        self.terms().iter().all(|term| term.is_finite())
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.terms().iter().all(|term| *term == 0.0)
    }

    /// Negative in any context: every unit is a non-negative quantity, so a length
    /// with no positive term and a negative one cannot come out positive.
    pub(crate) fn is_negative(&self) -> bool {
        let terms = self.terms();
        terms.iter().all(|term| *term <= 0.0) && terms.iter().any(|term| *term < 0.0)
    }

    pub(crate) fn has_percent(&self) -> bool {
        self.percent != 0.0
    }

    /// The px, when no other term contributes.
    pub(crate) fn as_px(&self) -> Option<f64> {
        self.terms()[1..].iter().all(|term| *term == 0.0).then_some(self.px)
    }

    /// The percentage as a fraction, when no other term contributes.
    pub(crate) fn as_percent(&self) -> Option<f64> {
        (self.px == 0.0 && self.terms()[2..].iter().all(|term| *term == 0.0)).then_some(self.percent)
    }
}

/// The CSS initial `font-size` in px. gpui's default text style is `rems(1.)` and
/// `Window::rem_size` is 16px until set, which gpuix never does.
pub(crate) const INITIAL_FONT_SIZE: f64 = 16.0;

/// What a length needs besides itself to become px. One context describes one
/// element; `descend` makes the context its children inherit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LengthContext {
    /// This element's computed `font-size`, which `em` means.
    pub(crate) font_size: f64,
    /// The parent's computed `font-size`: the `em` and `%` of this element's own
    /// `font-size`, and of a state refinement of it.
    parent_font_size: f64,
    /// The computed `font-size` of the root element, which `rem` means.
    pub(crate) root_font_size: f64,
    pub(crate) viewport_width: f64,
    pub(crate) viewport_height: f64,
}

impl LengthContext {
    /// The context above the root element: every font size is the initial one.
    pub(crate) fn initial(viewport_width: f64, viewport_height: f64) -> Self {
        Self {
            font_size: INITIAL_FONT_SIZE,
            parent_font_size: INITIAL_FONT_SIZE,
            root_font_size: INITIAL_FONT_SIZE,
            viewport_width,
            viewport_height,
        }
    }

    pub(crate) fn with_viewport(self, viewport_width: f64, viewport_height: f64) -> Self {
        Self { viewport_width, viewport_height, ..self }
    }

    /// The context of an element declaring `font_size`, whose parent has `self`.
    /// A font size is relative to the parent's: `em` is its size and `%` a
    /// fraction of it. `rem` is the root's, except for the root element itself,
    /// whose `rem` is still the initial size. No declaration inherits.
    pub(crate) fn descend(&self, font_size: Option<&LinearLength>, is_root: bool) -> Self {
        let computed = self.computed_font_size(font_size);
        Self {
            font_size: computed,
            parent_font_size: self.font_size,
            root_font_size: if is_root { computed } else { self.root_font_size },
            ..*self
        }
    }

    /// The context of a state refinement (`focusVisible`) of the element that has
    /// `self`: its `font-size` still resolves against the parent's, and a state
    /// that sets none keeps the element's own.
    pub(crate) fn for_state(&self, font_size: Option<&LinearLength>) -> Self {
        let Some(font_size) = font_size else { return *self };
        let parent = Self { font_size: self.parent_font_size, ..*self };
        Self { font_size: parent.computed_font_size(Some(font_size)), ..*self }
    }

    /// `self` is the parent's context. A negative result is clamped to 0, as CSS
    /// clamps `calc()` in a non-negative range.
    fn computed_font_size(&self, declared: Option<&LinearLength>) -> f64 {
        match declared {
            Some(length) => self.resolve(length, self.font_size).max(0.0),
            None => self.font_size,
        }
    }

    /// `percent_basis` is the px a `100%` stands for.
    pub(crate) fn resolve(&self, length: &LinearLength, percent_basis: f64) -> f64 {
        let (min_side, max_side) = if self.viewport_width < self.viewport_height {
            (self.viewport_width, self.viewport_height)
        } else {
            (self.viewport_height, self.viewport_width)
        };
        length.px
            + length.percent * percent_basis
            + length.em * self.font_size
            + length.rem * self.root_font_size
            + length.vw * self.viewport_width
            + length.vh * self.viewport_height
            + length.vmin * min_side
            + length.vmax * max_side
    }

    /// The px of a length that does not depend on a containing block.
    pub(crate) fn resolve_definite(&self, length: &LinearLength) -> Option<f64> {
        (!length.has_percent()).then(|| self.resolve(length, 0.0))
    }

    /// Everything that is px once the context is known, and the percent
    /// fraction that still needs a containing block.
    pub(crate) fn split(&self, length: &LinearLength) -> (f64, f64) {
        (self.resolve(&LinearLength { percent: 0.0, ..*length }, 0.0), length.percent)
    }
}

#[cfg(test)]
impl LengthContext {
    /// `1em` is 20px, `1rem` 10px and the viewport 800x600, so no unit
    /// coincides with another or with the initial context.
    pub(crate) fn for_tests() -> Self {
        Self::initial(800.0, 600.0)
            .descend(Some(&LinearLength::px(10.0)), true)
            .descend(Some(&LinearLength::px(20.0)), false)
    }
}

impl fmt::Display for LinearLength {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let terms: Vec<(f64, &str)> = [
            (self.percent * 100.0, "%"),
            (self.em, "em"),
            (self.rem, "rem"),
            (self.vw * 100.0, "vw"),
            (self.vh * 100.0, "vh"),
            (self.vmin * 100.0, "vmin"),
            (self.vmax * 100.0, "vmax"),
            (self.px, "px"),
        ]
        .into_iter()
        .filter(|(value, _)| *value != 0.0)
        .collect();
        match terms.as_slice() {
            [] => write!(formatter, "0px"),
            [(value, unit)] => write!(formatter, "{}{unit}", tidy(*value)),
            [(first, first_unit), rest @ ..] => {
                write!(formatter, "calc({}{first_unit}", tidy(*first))?;
                for (value, unit) in rest {
                    let sign = if *value < 0.0 { '-' } else { '+' };
                    write!(formatter, " {sign} {}{unit}", tidy(value.abs()))?;
                }
                write!(formatter, ")")
            }
        }
    }
}

/// Drops the float noise `fraction * 100` leaves (`0.07 * 100` is
/// `7.000000000000001`) so the text reads as it was written.
fn tidy(value: f64) -> f64 {
    (value * 1e9).round() / 1e9
}

impl Serialize for LinearLength {
    /// A number when the length is plain px, which is what the style accepted
    /// before lengths could mix units; otherwise its CSS text.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.as_px() {
            Some(px) => serializer.serialize_f64(px),
            None => serializer.collect_str(self),
        }
    }
}

impl<'de> Deserialize<'de> for LinearLength {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserialize_with(deserializer, "length", "a CSS length with a unit, or calc()", parse_linear_length)
    }
}

/// A number of px or a string `parse` accepts, for the properties whose grammar
/// is narrower than a plain length (no `%`, extra keywords). `what` names the
/// value in the error and `rule` says what it takes.
pub(crate) fn deserialize_with<'de, D: Deserializer<'de>>(
    deserializer: D,
    what: &'static str,
    rule: &'static str,
    parse: fn(&str) -> Option<LinearLength>,
) -> Result<LinearLength, D::Error> {
    struct LengthVisitor {
        what: &'static str,
        rule: &'static str,
        parse: fn(&str) -> Option<LinearLength>,
    }

    impl Visitor<'_> for LengthVisitor {
        type Value = LinearLength;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "a number of px or {}", self.rule)
        }

        fn visit_f64<E: de::Error>(self, value: f64) -> Result<LinearLength, E> {
            if !value.is_finite() {
                return Err(E::custom(format!("invalid {} {value}: not a finite number", self.what)));
            }
            Ok(LinearLength::px(value).clamped())
        }

        fn visit_i64<E: de::Error>(self, value: i64) -> Result<LinearLength, E> {
            self.visit_f64(value as f64)
        }

        fn visit_u64<E: de::Error>(self, value: u64) -> Result<LinearLength, E> {
            self.visit_f64(value as f64)
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<LinearLength, E> {
            (self.parse)(value).ok_or_else(|| E::custom(format!("invalid {} \"{value}\": {}", self.what, self.rule)))
        }
    }

    deserializer.deserialize_any(LengthVisitor { what, rule, parse })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> LinearLength {
        parse_linear_length(text).unwrap_or_else(|| panic!("{text} should parse"))
    }

    #[test]
    fn as_px_and_as_percent_need_every_other_term_to_be_zero() {
        assert_eq!(LinearLength::px(12.0).as_px(), Some(12.0));
        assert_eq!(LinearLength::px(12.0).as_percent(), None);
        assert_eq!(LinearLength::percent(0.5).as_percent(), Some(0.5));
        assert_eq!(LinearLength::percent(0.5).as_px(), None);
        assert_eq!(parse("calc(100% - 20px)").as_px(), None);
        assert_eq!(parse("calc(100% - 20px)").as_percent(), None);
        assert_eq!(parse("2em").as_px(), None);
        assert_eq!(LinearLength::default().as_px(), Some(0.0));
        assert!(LinearLength::default().is_zero());
        assert!(!LinearLength::px(1.0).is_zero());
        assert!(parse("calc(100% - 20px)").has_percent());
        assert!(parse("-1px").is_negative() && parse("-0.5em").is_negative() && parse("calc(-1px - 1rem)").is_negative());
        assert!(!parse("calc(1px - 1em)").is_negative() && !parse("0").is_negative() && !parse("1vw").is_negative());
        assert!(!parse("20px").has_percent());
    }

    #[test]
    fn displays_as_css_that_parses_back_to_the_same_length() {
        assert_eq!(LinearLength::px(12.0).to_string(), "12px");
        assert_eq!(LinearLength::percent(0.5).to_string(), "50%");
        assert_eq!(LinearLength::default().to_string(), "0px");
        assert_eq!(parse("calc(100% - 20px)").to_string(), "calc(100% - 20px)");
        assert_eq!(parse("calc(1em + 2rem - 3vw)").to_string(), "calc(1em + 2rem - 3vw)");
        assert_eq!(parse("-20px").to_string(), "-20px");
        for text in ["calc(100% - 20px)", "calc(2 * (1em + 4px))", "calc(7% + 0.5vmin - 3vmax)", "-1.5rem", "calc(1vh - 1em)"] {
            let length = parse(text);
            assert_eq!(parse(&length.to_string()), length, "{text}");
        }
    }

    #[test]
    fn serializes_plain_px_as_a_number_and_anything_else_as_css() {
        assert_eq!(serde_json::to_string(&LinearLength::px(12.5)).unwrap(), "12.5");
        assert_eq!(serde_json::to_string(&parse("calc(100% - 20px)")).unwrap(), "\"calc(100% - 20px)\"");
        assert_eq!(serde_json::to_string(&parse("50%")).unwrap(), "\"50%\"");
    }

    #[test]
    fn deserializes_a_number_as_px_and_a_string_as_css() {
        let read = |json: &str| serde_json::from_str::<LinearLength>(json);
        assert_eq!(read("12").unwrap(), LinearLength::px(12.0));
        assert_eq!(read("-3.5").unwrap(), LinearLength::px(-3.5));
        assert_eq!(read("\"2in\"").unwrap(), LinearLength::px(192.0));
        assert_eq!(read("\"0\"").unwrap(), LinearLength::px(0.0));
        assert_eq!(read("\"calc(100% - 20px)\"").unwrap(), parse("calc(100% - 20px)"));
        assert_eq!(read("1e300").unwrap(), LinearLength::px(MAX_LENGTH));
    }

    #[test]
    fn deserialize_errors_name_the_bad_value() {
        for json in ["\"12\"", "\"12 px\"", "\"auto\"", "\"calc(1px + 2)\"", "true", "null", "[1]"] {
            assert!(serde_json::from_str::<LinearLength>(json).is_err(), "{json}");
        }
        let error = serde_json::from_str::<LinearLength>("\"12\"").unwrap_err().to_string();
        assert!(error.contains("invalid length \"12\""), "{error}");
    }

    #[test]
    fn round_trips_through_serde() {
        for length in [LinearLength::px(8.0), LinearLength::percent(0.25), parse("calc(50vmin + 1em)"), parse("calc(100% / 3 + 2px)")] {
            let json = serde_json::to_string(&length).unwrap();
            let back: LinearLength = serde_json::from_str(&json).unwrap();
            assert_eq!(back.to_string(), length.to_string(), "{json}");
            assert!((back.percent - length.percent).abs() < 1e-9, "{json}");
            assert!((back.vmin - length.vmin).abs() < 1e-9, "{json}");
        }
    }

    const CONTEXT: LengthContext = LengthContext {
        font_size: 20.0,
        parent_font_size: 10.0,
        root_font_size: 16.0,
        viewport_width: 800.0,
        viewport_height: 600.0,
    };

    #[test]
    fn resolve_adds_every_term_in_px() {
        assert_eq!(CONTEXT.resolve(&parse("calc(50vmin + 1em)"), 0.0), 320.0);
        assert_eq!(CONTEXT.resolve(&parse("calc(50vmax + 1rem)"), 0.0), 416.0);
        assert_eq!(CONTEXT.resolve(&parse("calc(10vw + 10vh)"), 0.0), 140.0);
        assert_eq!(CONTEXT.resolve(&parse("calc(50% + 2em)"), 200.0), 140.0);
        assert_eq!(CONTEXT.resolve(&parse("10px"), 999.0), 10.0);
    }

    #[test]
    fn vmin_and_vmax_follow_the_smaller_and_larger_side() {
        let portrait = LengthContext { viewport_width: 300.0, viewport_height: 500.0, ..CONTEXT };
        assert_eq!(portrait.resolve(&parse("10vmin"), 0.0), 30.0);
        assert_eq!(portrait.resolve(&parse("10vmax"), 0.0), 50.0);
        assert_eq!(CONTEXT.resolve(&parse("10vmin"), 0.0), 60.0);
        assert_eq!(CONTEXT.resolve(&parse("10vmax"), 0.0), 80.0);
    }

    #[test]
    fn resolve_definite_is_none_while_a_percent_remains() {
        assert_eq!(CONTEXT.resolve_definite(&parse("calc(50vmin + 1em)")), Some(320.0));
        assert_eq!(CONTEXT.resolve_definite(&parse("calc(100% - 20px)")), None);
        assert_eq!(CONTEXT.resolve_definite(&parse("calc(0% + 20px)")), Some(20.0));
    }

    #[test]
    fn split_separates_the_percent_from_everything_that_is_px() {
        assert_eq!(CONTEXT.split(&parse("calc(100% - 20px)")), (-20.0, 1.0));
        assert_eq!(CONTEXT.split(&parse("calc(50% + 1em + 10vw)")), (100.0, 0.5));
        assert_eq!(CONTEXT.split(&parse("12px")), (12.0, 0.0));
    }

    fn context(width: f64, height: f64) -> LengthContext {
        LengthContext::initial(width, height)
    }

    #[test]
    fn the_initial_context_is_the_default_size_everywhere() {
        let initial = context(800.0, 600.0);
        assert_eq!(initial.font_size, INITIAL_FONT_SIZE);
        assert_eq!(initial.root_font_size, INITIAL_FONT_SIZE);
        assert_eq!((initial.viewport_width, initial.viewport_height), (800.0, 600.0));
        let resized = initial.with_viewport(300.0, 200.0);
        assert_eq!((resized.viewport_width, resized.viewport_height), (300.0, 200.0));
        assert_eq!(resized.font_size, INITIAL_FONT_SIZE);
    }

    #[test]
    fn a_font_size_without_a_declaration_is_the_parents() {
        let parent = context(800.0, 600.0).descend(Some(&parse("20px")), true);
        let child = parent.descend(None, false);
        assert_eq!(child.font_size, 20.0);
        assert_eq!(child.root_font_size, 20.0);
    }

    #[test]
    fn em_and_percent_in_a_font_size_are_the_parents_size() {
        let parent = context(800.0, 600.0).descend(Some(&parse("20px")), true);
        assert_eq!(parent.descend(Some(&parse("1.5em")), false).font_size, 30.0);
        assert_eq!(parent.descend(Some(&parse("50%")), false).font_size, 10.0);
        assert_eq!(parent.descend(Some(&parse("calc(100% + 2px)")), false).font_size, 22.0);
        assert_eq!(parent.descend(Some(&parse("calc(1em - 5px)")), false).font_size, 15.0);
    }

    #[test]
    fn nested_em_compounds() {
        let root = context(800.0, 600.0).descend(Some(&parse("10px")), true);
        let child = root.descend(Some(&parse("2em")), false);
        let grandchild = child.descend(Some(&parse("2em")), false);
        assert_eq!((child.font_size, grandchild.font_size), (20.0, 40.0));
    }

    #[test]
    fn rem_is_the_roots_size_and_the_roots_own_rem_is_the_initial_one() {
        let root = context(800.0, 600.0).descend(Some(&parse("2rem")), true);
        assert_eq!(root.font_size, 2.0 * INITIAL_FONT_SIZE);
        assert_eq!(root.root_font_size, 2.0 * INITIAL_FONT_SIZE);
        let child = root.descend(Some(&parse("1.5rem")), false);
        assert_eq!(child.font_size, 3.0 * INITIAL_FONT_SIZE);
        assert_eq!(child.root_font_size, 2.0 * INITIAL_FONT_SIZE);
    }

    #[test]
    fn viewport_units_in_a_font_size_follow_the_viewport() {
        let size = parse("calc(1rem + 2vw)");
        let wide = context(800.0, 600.0).descend(Some(&size), false);
        assert_eq!(wide.font_size, 32.0);
        let narrow = context(400.0, 600.0).descend(Some(&size), false);
        assert_eq!(narrow.font_size, 24.0);
    }

    #[test]
    fn a_negative_font_size_is_zero() {
        let size = parse("calc(1em - 40px)");
        assert_eq!(context(800.0, 600.0).descend(Some(&size), false).font_size, 0.0);
    }

    #[test]
    fn a_state_font_size_resolves_against_the_parent_and_a_state_without_one_keeps_the_elements() {
        let parent = context(800.0, 600.0).descend(Some(&parse("20px")), true);
        let element = parent.descend(Some(&parse("2em")), false);
        assert_eq!(element.font_size, 40.0);
        assert_eq!(element.for_state(Some(&parse("1.5em"))).font_size, 30.0);
        assert_eq!(element.for_state(None), element);
        assert_eq!(element.for_state(Some(&parse("1.5em"))).root_font_size, 20.0);
    }
}
