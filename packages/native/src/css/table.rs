//! The CSS properties of tables (CSS Tables 3): `table-layout`, `border-spacing`,
//! `border-collapse`, `caption-side` and `empty-cells`.
//!
//! Only `table-layout` is not inherited. A nested table takes the spacing of the
//! table around it, and a caption takes its side from its table.

use std::fmt;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::dimension::parse_definite_length;
use super::length::{LengthContext, LinearLength};

macro_rules! keyword {
    ($(#[$meta:meta])* $name:ident, $property:literal, $($variant:ident => $text:literal),+ $(,)?) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "kebab-case", try_from = "String")]
        pub enum $name {
            $($variant),+
        }

        impl TryFrom<String> for $name {
            type Error = String;

            fn try_from(value: String) -> Result<Self, String> {
                match value.as_str() {
                    $($text => Ok(Self::$variant),)+
                    _ => Err(format!(concat!("invalid ", $property, ": {:?}"), value)),
                }
            }
        }
    };
}

keyword!(
    /// `table-layout`: whether the first row alone sizes the columns.
    TableLayout, "tableLayout",
    Auto => "auto",
    Fixed => "fixed",
);

keyword!(
    /// `caption-side`: which side of the rows a caption goes on.
    CaptionSide, "captionSide",
    Top => "top",
    Bottom => "bottom",
);

keyword!(
    /// `border-collapse`.
    BorderCollapse, "borderCollapse",
    Separate => "separate",
    Collapse => "collapse",
);

keyword!(
    /// `empty-cells`: whether a cell with nothing in it paints its background and border.
    EmptyCells, "emptyCells",
    Show => "show",
    Hide => "hide",
);

/// `border-spacing`: the gap between cells and between the cells and the table's
/// edge. One length is both, two are the horizontal and then the vertical one.
/// A percentage is not a length here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BorderSpacing {
    pub horizontal: LinearLength,
    pub vertical: LinearLength,
}

/// `border-spacing` in px.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ResolvedSpacing {
    pub horizontal: f64,
    pub vertical: f64,
}

const RULE: &str = "one or two non-negative CSS lengths";

impl BorderSpacing {
    /// Resolved in the context of the element that declares it: `em` is that element's
    /// font size, and a descendant inherits the px.
    pub(crate) fn resolve(&self, lengths: &LengthContext) -> ResolvedSpacing {
        let px = |length: &LinearLength| lengths.resolve(length, 0.0).max(0.0);
        ResolvedSpacing { horizontal: px(&self.horizontal), vertical: px(&self.vertical) }
    }

    fn parse(text: &str) -> Option<Self> {
        let mut parts = split_top_level(text);
        let first = parts.next()?;
        let second = parts.next();
        if parts.next().is_some() {
            return None;
        }
        let length = |token: &str| parse_definite_length(token).filter(|length| !length.is_negative());
        let horizontal = length(first)?;
        let vertical = match second {
            Some(token) => length(token)?,
            None => horizontal,
        };
        Some(Self { horizontal, vertical })
    }
}

/// The whitespace-separated parts of `text` that are not inside parentheses, so a
/// `calc(1px + 2px)` stays one.
fn split_top_level(text: &str) -> impl Iterator<Item = &str> {
    let mut depth = 0usize;
    let mut start = None;
    let mut parts = Vec::new();
    for (index, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ => {}
        }
        match (c.is_whitespace() && depth == 0, start) {
            (true, Some(from)) => {
                parts.push(&text[from..index]);
                start = None;
            }
            (false, None) => start = Some(index),
            _ => {}
        }
    }
    if let Some(from) = start {
        parts.push(&text[from..]);
    }
    parts.into_iter()
}

impl Serialize for BorderSpacing {
    /// A number when both are the same plain px, otherwise the CSS text.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match (self.horizontal.as_px(), self.vertical.as_px()) {
            (Some(horizontal), Some(vertical)) if horizontal == vertical => serializer.serialize_f64(horizontal),
            _ if self.horizontal == self.vertical => serializer.collect_str(&self.horizontal),
            _ => serializer.collect_str(&format_args!("{} {}", self.horizontal, self.vertical)),
        }
    }
}

impl<'de> Deserialize<'de> for BorderSpacing {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SpacingVisitor;

        impl Visitor<'_> for SpacingVisitor {
            type Value = BorderSpacing;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "a number of px or {RULE}")
            }

            fn visit_f64<E: de::Error>(self, value: f64) -> Result<BorderSpacing, E> {
                if !value.is_finite() || value < 0.0 {
                    return Err(E::custom(format!("invalid borderSpacing {value}: {RULE}")));
                }
                let length = LinearLength::px(value).clamped();
                Ok(BorderSpacing { horizontal: length, vertical: length })
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> Result<BorderSpacing, E> {
                self.visit_f64(value as f64)
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<BorderSpacing, E> {
                self.visit_f64(value as f64)
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<BorderSpacing, E> {
                BorderSpacing::parse(value)
                    .ok_or_else(|| E::custom(format!("invalid borderSpacing \"{value}\": {RULE}")))
            }
        }

        deserializer.deserialize_any(SpacingVisitor)
    }
}

/// The table properties an element has, its own over what it inherits.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct TableInherited {
    pub spacing: ResolvedSpacing,
    pub collapse: bool,
    pub caption_bottom: bool,
    pub hide_empty_cells: bool,
}

impl TableInherited {
    /// What the element's children inherit: `style` over `self`. `lengths` is the element's own
    /// context, in which a declared spacing resolves.
    pub(crate) fn descend(mut self, style: &crate::style::StyleDesc, lengths: &LengthContext) -> Self {
        if let Some(spacing) = &style.border_spacing {
            self.spacing = spacing.resolve(lengths);
        }
        if let Some(collapse) = style.border_collapse {
            self.collapse = collapse == BorderCollapse::Collapse;
        }
        if let Some(side) = style.caption_side {
            self.caption_bottom = side == CaptionSide::Bottom;
        }
        if let Some(empty) = style.empty_cells {
            self.hide_empty_cells = empty == EmptyCells::Hide;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spacing(json: &str) -> Result<BorderSpacing, serde_json::Error> {
        serde_json::from_str(json)
    }

    fn resolved(json: &str) -> (f64, f64) {
        // 1em is 20px, 1rem 10px.
        let resolved = spacing(json).unwrap().resolve(&LengthContext::for_tests());
        (resolved.horizontal, resolved.vertical)
    }

    #[test]
    fn one_length_is_both_axes_and_two_are_horizontal_then_vertical() {
        assert_eq!(resolved("4"), (4.0, 4.0));
        assert_eq!(resolved("\"4px\""), (4.0, 4.0));
        assert_eq!(resolved("\"4px 8px\""), (4.0, 8.0));
        assert_eq!(resolved("\"  1em   0.5rem \""), (20.0, 5.0));
        assert_eq!(resolved("\"calc(1em + 2px) 1in\""), (22.0, 96.0));
        assert_eq!(resolved("\"0 3px\""), (0.0, 3.0));
    }

    #[test]
    fn what_css_rejects_is_an_error_naming_the_value() {
        for json in ["\"-1px\"", "-2", "\"1px 2px 3px\"", "\"10%\"", "\"auto\"", "\"\"", "\"1\"", "true", "null"] {
            assert!(spacing(json).is_err(), "{json} is accepted");
        }
        let error = spacing("\"1px 2px 3px\"").unwrap_err().to_string();
        assert!(error.contains("invalid borderSpacing"), "{error}");
    }

    #[test]
    fn a_calc_that_only_comes_out_negative_is_zero() {
        assert_eq!(resolved("\"calc(1em - 30px)\""), (0.0, 0.0));
    }

    #[test]
    fn it_survives_a_round_trip() {
        for json in ["4", "\"4px 8px\"", "\"calc(1em + 2px)\"", "\"1em 0\"", "0"] {
            let parsed = spacing(json).unwrap();
            let again = spacing(&serde_json::to_string(&parsed).unwrap()).unwrap();
            assert_eq!(again, parsed, "{json}");
        }
    }

    #[test]
    fn a_keyword_is_validated_where_the_style_is_read() {
        let style: crate::style::StyleDesc = serde_json::from_str(
            r#"{"tableLayout": "fixed", "captionSide": "bottom", "borderCollapse": "collapse", "emptyCells": "hide", "borderSpacing": "2px 3px"}"#,
        )
        .unwrap();
        assert_eq!(style.table_layout, Some(TableLayout::Fixed));
        assert_eq!(style.caption_side, Some(CaptionSide::Bottom));
        assert_eq!(style.border_collapse, Some(BorderCollapse::Collapse));
        assert_eq!(style.empty_cells, Some(EmptyCells::Hide));
        let mut table = crate::retained_tree::StyleTable::default();
        for (json, name) in [
            (r#"{"tableLayout": "balanced"}"#, "tableLayout"),
            (r#"{"captionSide": "left"}"#, "captionSide"),
            (r#"{"borderCollapse": "yes"}"#, "borderCollapse"),
            (r#"{"emptyCells": "none"}"#, "emptyCells"),
            (r#"{"borderSpacing": "1px 2px 3px"}"#, "borderSpacing"),
        ] {
            let error = table.intern(json.as_bytes()).unwrap_err();
            assert!(error.contains(&format!("invalid {name}")), "{error}");
        }
    }

    #[test]
    fn a_descendant_inherits_the_spacing_resolved_where_it_was_declared() {
        let declared: crate::style::StyleDesc =
            serde_json::from_str(r#"{"borderSpacing": "1em", "captionSide": "bottom"}"#).unwrap();
        let inherited = TableInherited::default().descend(&declared, &LengthContext::for_tests());
        assert_eq!((inherited.spacing.horizontal, inherited.spacing.vertical), (20.0, 20.0));
        assert!(inherited.caption_bottom);
        // A child with a different font size and no declaration keeps the 20px.
        let child = TableInherited::descend(inherited, &Default::default(), &LengthContext::initial(0.0, 0.0));
        assert_eq!(child.spacing.horizontal, 20.0);
        let own: crate::style::StyleDesc = serde_json::from_str(r#"{"borderCollapse": "collapse"}"#).unwrap();
        assert!(child.descend(&own, &LengthContext::for_tests()).collapse);
    }
}
