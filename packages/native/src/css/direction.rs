//! CSS `direction`: which way the inline axis runs. It is inherited.
//!
//! It reaches layout as the `direction` of the element's GPUI style, which mirrors the main
//! axis of a row flex container, the columns of a grid and of a table, the side a vertical
//! scrollbar is on and where a narrow block child sits. It also decides what the `start` and `end`
//! of `text-align` are.

use serde::{Deserialize, Serialize};

/// `direction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", try_from = "String")]
pub enum Direction {
    #[default]
    Ltr,
    Rtl,
}

impl TryFrom<String> for Direction {
    type Error = String;

    fn try_from(value: String) -> Result<Self, String> {
        match value.as_str() {
            "ltr" => Ok(Self::Ltr),
            "rtl" => Ok(Self::Rtl),
            _ => Err(format!("invalid direction: {value:?}")),
        }
    }
}

impl Direction {
    pub(crate) fn is_rtl(self) -> bool {
        self == Self::Rtl
    }

    pub(crate) fn to_gpui(self) -> gpui::Direction {
        match self {
            Self::Ltr => gpui::Direction::Ltr,
            Self::Rtl => gpui::Direction::Rtl,
        }
    }
}

/// `text-align` as far as inheritance and direction are concerned. `start` and `end` are kept
/// as written, because they mean a different side below an element with another `direction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) enum Align {
    #[default]
    Start,
    End,
    Left,
    Right,
    Center,
}

impl Align {
    /// `None` for a value gpuix does not lay out (`justify`, `match-parent`) or that is no keyword.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "start" => Self::Start,
            "end" => Self::End,
            "left" => Self::Left,
            "right" => Self::Right,
            "center" => Self::Center,
            _ => return None,
        })
    }

    pub(crate) fn resolve(self, direction: Direction) -> gpui::TextAlign {
        match (self, direction) {
            (Self::Left, _) | (Self::Start, Direction::Ltr) | (Self::End, Direction::Rtl) => gpui::TextAlign::Left,
            (Self::Right, _) | (Self::End, Direction::Ltr) | (Self::Start, Direction::Rtl) => gpui::TextAlign::Right,
            (Self::Center, _) => gpui::TextAlign::Center,
        }
    }
}

/// The inherited `direction` and `text-align` of an element, and the alignment its parent's
/// text style gives it (which GPUI inherits as resolved, not as `start`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Inherited {
    pub direction: Direction,
    pub align: Align,
    parent_resolved: gpui::TextAlign,
}

impl Inherited {
    pub(crate) fn fingerprint(&self, fp: &mut crate::retain::Fingerprint) {
        fp.hash(&self.direction);
        fp.hash(&self.align);
        fp.word(self.parent_resolved as u64);
    }

    /// What the element's own style makes of what it inherits.
    pub(crate) fn descend(mut self, style: &crate::style::StyleDesc) -> Self {
        self.parent_resolved = self.align.resolve(self.direction);
        if let Some(direction) = style.direction {
            self.direction = direction;
        }
        if let Some(align) = style.text_align.as_deref().and_then(Align::parse) {
            self.align = align;
        }
        self
    }

    /// Sets what an element with this context needs beyond its style: its `direction`, and the
    /// text alignment when it is not the one the parent's text style hands down.
    pub(crate) fn apply<E: gpui::Styled>(&self, el: &mut E) {
        if self.direction.is_rtl() {
            el.style().direction = Some(self.direction.to_gpui());
        }
        let resolved = self.align.resolve(self.direction);
        if resolved != self.parent_resolved {
            el.text_style().text_align = Some(resolved);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::StyleDesc;

    fn style(json: &str) -> StyleDesc {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn start_and_end_follow_the_direction_of_the_element() {
        let root = Inherited::default().descend(&style(r#"{"direction": "rtl"}"#));
        assert_eq!(root.align.resolve(root.direction), gpui::TextAlign::Right);
        let ltr_child = root.descend(&style(r#"{"direction": "ltr"}"#));
        assert_eq!(ltr_child.align.resolve(ltr_child.direction), gpui::TextAlign::Left);
        let end = ltr_child.descend(&style(r#"{"textAlign": "end"}"#));
        assert_eq!(end.align.resolve(end.direction), gpui::TextAlign::Right);
        // A child that goes back to rtl inherits `end` as `end`, which is the left in rtl.
        let back = end.descend(&style(r#"{"direction": "rtl"}"#));
        assert_eq!(back.align.resolve(back.direction), gpui::TextAlign::Left);
    }

    #[test]
    fn an_invalid_direction_is_an_error_naming_it() {
        let mut table = crate::retained_tree::StyleTable::default();
        let error = table.intern(br#"{"direction": "sideways"}"#).unwrap_err();
        assert!(error.contains("invalid direction"), "{error}");
    }
}
