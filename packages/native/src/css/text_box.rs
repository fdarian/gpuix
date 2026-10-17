//! CSS `text-box-trim`, `text-box-edge` and the `text-box` shorthand: cutting the
//! space above the first line and below the last line of a block container down to
//! the glyph metrics the edge names.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::length::{LengthContext, LinearLength};
use super::syntax::split_whitespace;
use crate::color::ColorContext;
use crate::style::StyleDesc;

/// `text-box-trim`. Not inherited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", try_from = "String")]
pub enum Trim {
    None,
    TrimStart,
    TrimEnd,
    TrimBoth,
}

impl Trim {
    fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "trim-start" => Some(Self::TrimStart),
            "trim-end" => Some(Self::TrimEnd),
            "trim-both" => Some(Self::TrimBoth),
            _ => None,
        }
    }

    fn to_gpui(self) -> gpui::TextBoxTrim {
        gpui::TextBoxTrim {
            start: matches!(self, Self::TrimStart | Self::TrimBoth),
            end: matches!(self, Self::TrimEnd | Self::TrimBoth),
        }
    }
}

impl TryFrom<String> for Trim {
    type Error = String;

    fn try_from(value: String) -> Result<Self, String> {
        Self::parse(&value).ok_or_else(|| format!("invalid textBoxTrim: {value:?}"))
    }
}

/// `text-box-edge`, inherited. `auto` is `text`, as in Chrome, so both are the
/// same value here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Edge {
    over: gpui::TextBoxOver,
    under: gpui::TextBoxUnder,
}

impl Edge {
    const AUTO: Self = Self { over: gpui::TextBoxOver::Text, under: gpui::TextBoxUnder::Text };

    /// `auto | [ text | cap | ex ] [ text | alphabetic ]?`. One value sets the under edge
    /// to the same keyword where it can, and to `text` where it cannot.
    fn parse(value: &str) -> Option<Self> {
        let tokens = split_whitespace(value);
        let first = tokens.first()?.to_ascii_lowercase();
        if first == "auto" {
            return (tokens.len() == 1).then_some(Self::AUTO);
        }
        let over = match first.as_str() {
            "text" => gpui::TextBoxOver::Text,
            "cap" => gpui::TextBoxOver::Cap,
            "ex" => gpui::TextBoxOver::Ex,
            _ => return None,
        };
        let under = match tokens.get(1).map(|token| token.to_ascii_lowercase()).as_deref() {
            None => gpui::TextBoxUnder::Text,
            Some("text") => gpui::TextBoxUnder::Text,
            Some("alphabetic") => gpui::TextBoxUnder::Alphabetic,
            Some(_) => return None,
        };
        (tokens.len() <= 2).then_some(Self { over, under })
    }

    pub fn to_gpui(self) -> gpui::TextBoxEdge {
        gpui::TextBoxEdge { over: self.over, under: self.under }
    }
}

impl fmt::Display for Edge {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        let over = match self.over {
            gpui::TextBoxOver::Text => "text",
            gpui::TextBoxOver::Cap => "cap",
            gpui::TextBoxOver::Ex => "ex",
        };
        let under = match self.under {
            gpui::TextBoxUnder::Text => "text",
            gpui::TextBoxUnder::Alphabetic => "alphabetic",
        };
        write!(formatter, "{over} {under}")
    }
}

impl From<Edge> for String {
    fn from(edge: Edge) -> String {
        edge.to_string()
    }
}

impl TryFrom<String> for Edge {
    type Error = String;

    fn try_from(value: String) -> Result<Self, String> {
        Self::parse(&value).ok_or_else(|| format!("invalid textBoxEdge: {value:?}"))
    }
}

/// What `style` declares of the two properties: the `text-box` shorthand, then each
/// longhand laid over it. An invalid shorthand is ignored.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Declared {
    pub trim: Option<Trim>,
    pub edge: Option<Edge>,
}

pub fn of(style: &StyleDesc) -> Declared {
    let mut declared = style.text_box.as_deref().and_then(parse_shorthand).unwrap_or_default();
    if style.text_box_trim.is_some() {
        declared.trim = style.text_box_trim;
    }
    if style.text_box_edge.is_some() {
        declared.edge = style.text_box_edge;
    }
    declared
}

/// Whether `value` is a valid `text-box` shorthand, so a state style's invalid
/// one does not reset the longhands it would cover.
pub(crate) fn is_valid_shorthand(value: &str) -> bool {
    parse_shorthand(value).is_some()
}

/// `normal | <text-box-trim> || <text-box-edge>`. A part that is left out is not the
/// initial value for the trim: it is `trim-both`.
fn parse_shorthand(value: &str) -> Option<Declared> {
    let tokens = split_whitespace(value);
    let first = *tokens.first()?;
    if tokens.len() == 1 && first.eq_ignore_ascii_case("normal") {
        return Some(Declared { trim: Some(Trim::None), edge: Some(Edge::AUTO) });
    }
    let (trim, edge_tokens) = if let Some(trim) = Trim::parse(first) {
        (Some(trim), &tokens[1..])
    } else if let Some(trim) = tokens.last().and_then(|last| Trim::parse(last)) {
        (Some(trim), &tokens[..tokens.len() - 1])
    } else {
        (None, &tokens[..])
    };
    let edge = if edge_tokens.is_empty() {
        None
    } else {
        Some(Edge::parse(&edge_tokens.join(" "))?)
    };
    Some(Declared {
        trim: Some(trim.unwrap_or(Trim::TrimBoth)),
        edge: Some(edge.unwrap_or(Edge::AUTO)),
    })
}

/// The ends of the lines of the element being built that are trimmed, given the ends
/// its parent passes down.
///
/// A passed-down end stops at an element with padding or a border on that side, and
/// an element that lays out flex or grid items passes nothing: its items are boxes
/// of their own. What the element declares itself is not blocked by its own padding
/// or border, the trim is inside them.
pub(crate) fn requested(
    style: Option<&StyleDesc>,
    passed: gpui::TextBoxTrim,
    colors: &ColorContext,
    lengths: &LengthContext,
) -> gpui::TextBoxTrim {
    let Some(style) = style else { return passed };
    if crate::text::inline::lays_out_items(Some(style)) {
        return gpui::TextBoxTrim::default();
    }
    let mut trim = passed;
    if !trim.is_none() {
        let [top, _, bottom, _] = crate::css::border::used_widths(style, colors, lengths);
        trim.start &= f32::from(top) == 0.0 && padding_is_zero(style, Side::Top, lengths);
        trim.end &= f32::from(bottom) == 0.0 && padding_is_zero(style, Side::Bottom, lengths);
    }
    if let Some(own) = of(style).trim {
        let own = own.to_gpui();
        trim.start |= own.start;
        trim.end |= own.end;
    }
    trim
}

#[derive(Clone, Copy)]
enum Side {
    Top,
    Bottom,
}

fn padding_is_zero(style: &StyleDesc, side: Side, lengths: &LengthContext) -> bool {
    let length: Option<LinearLength> = match side {
        Side::Top => style.padding_top.or(style.padding.as_ref().map(|sides| sides.top)),
        Side::Bottom => style.padding_bottom.or(style.padding.as_ref().map(|sides| sides.bottom)),
    };
    // A percentage resolves against a width that is not known here; it is zero only
    // when it is written as zero.
    length.is_none_or(|length| lengths.resolve(&length, 100.0) == 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(value: &str) -> Option<String> {
        Edge::parse(value).map(|edge| edge.to_string())
    }

    #[test]
    fn a_two_value_edge_names_its_over_and_under() {
        assert_eq!(edge("cap alphabetic").as_deref(), Some("cap alphabetic"));
        assert_eq!(edge("ex alphabetic").as_deref(), Some("ex alphabetic"));
        assert_eq!(edge("text alphabetic").as_deref(), Some("text alphabetic"));
        assert_eq!(edge("cap text").as_deref(), Some("cap text"));
        assert_eq!(edge("  EX   Text ").as_deref(), Some("ex text"));
    }

    #[test]
    fn one_value_sets_the_under_edge_to_the_same_keyword_where_it_can_and_to_text_where_not() {
        assert_eq!(edge("text").as_deref(), Some("text text"));
        assert_eq!(edge("cap").as_deref(), Some("cap text"));
        assert_eq!(edge("ex").as_deref(), Some("ex text"));
        assert_eq!(edge("auto").as_deref(), Some("text text"));
    }

    #[test]
    fn an_edge_that_is_not_one_of_the_keywords_is_invalid() {
        for value in ["", "alphabetic", "alphabetic text", "cap cap", "text ex", "auto text", "cap alphabetic text", "ideographic", "1px"] {
            assert_eq!(edge(value), None, "{value:?}");
        }
    }

    #[test]
    fn the_shorthand_leaves_out_a_trim_as_trim_both_and_an_edge_as_auto() {
        let declared = |value: &str| parse_shorthand(value).map(|d| (d.trim, d.edge.map(|e| e.to_string())));
        assert_eq!(declared("normal"), Some((Some(Trim::None), Some("text text".into()))));
        assert_eq!(declared("trim-both cap alphabetic"), Some((Some(Trim::TrimBoth), Some("cap alphabetic".into()))));
        assert_eq!(declared("cap alphabetic"), Some((Some(Trim::TrimBoth), Some("cap alphabetic".into()))));
        assert_eq!(declared("trim-start"), Some((Some(Trim::TrimStart), Some("text text".into()))));
        assert_eq!(declared("ex alphabetic trim-end"), Some((Some(Trim::TrimEnd), Some("ex alphabetic".into()))));
        assert_eq!(declared("cap"), Some((Some(Trim::TrimBoth), Some("cap text".into()))));
        assert_eq!(declared("none"), Some((Some(Trim::None), Some("text text".into()))));
    }

    #[test]
    fn an_invalid_shorthand_declares_nothing() {
        for value in ["", "normal trim-both", "trim-both trim-end", "sideways", "trim-both cap cap"] {
            assert_eq!(parse_shorthand(value), None, "{value:?}");
        }
    }

    #[test]
    fn a_longhand_beats_the_shorthand() {
        let style = StyleDesc {
            text_box: Some("trim-start cap alphabetic".into()),
            text_box_trim: Some(Trim::TrimEnd),
            ..Default::default()
        };
        let declared = of(&style);
        assert_eq!(declared.trim, Some(Trim::TrimEnd));
        assert_eq!(declared.edge.map(|edge| edge.to_string()).as_deref(), Some("cap alphabetic"));
    }

    fn requested_of(style: StyleDesc, passed: (bool, bool)) -> (bool, bool) {
        let trim = requested(
            Some(&style),
            gpui::TextBoxTrim { start: passed.0, end: passed.1 },
            &ColorContext::root(crate::color::Scheme::Light),
            &LengthContext::initial(0.0, 0.0),
        );
        (trim.start, trim.end)
    }

    fn style(json: serde_json::Value) -> StyleDesc {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn padding_or_a_border_on_a_side_stops_the_trim_passed_down_to_that_side() {
        assert_eq!(requested_of(style(serde_json::json!({})), (true, true)), (true, true));
        assert_eq!(requested_of(style(serde_json::json!({"paddingTop": 4})), (true, true)), (false, true));
        assert_eq!(requested_of(style(serde_json::json!({"paddingBottom": "1em"})), (true, true)), (true, false));
        assert_eq!(requested_of(style(serde_json::json!({"padding": "0 8px"})), (true, true)), (true, true));
        assert_eq!(requested_of(style(serde_json::json!({"borderTop": "1px solid red"})), (true, true)), (false, true));
        assert_eq!(requested_of(style(serde_json::json!({"borderBottom": "2px solid red"})), (true, true)), (true, false));
    }

    #[test]
    fn the_padding_of_the_element_that_declares_the_trim_does_not_stop_it() {
        let own = style(serde_json::json!({"textBoxTrim": "trim-both", "padding": 10, "border": "1px solid red"}));
        assert_eq!(requested_of(own, (false, false)), (true, true));
    }

    #[test]
    fn a_flex_container_passes_down_and_declares_no_trim() {
        let flex = style(serde_json::json!({"display": "flex", "textBoxTrim": "trim-both"}));
        assert_eq!(requested_of(flex, (true, true)), (false, false));
    }

    #[test]
    fn trim_none_on_a_descendant_does_not_take_off_the_trim_of_an_ancestor() {
        let none = style(serde_json::json!({"textBoxTrim": "none"}));
        assert_eq!(requested_of(none, (true, true)), (true, true));
    }
}
