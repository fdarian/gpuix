//! CSS `white-space` (CSS Text 3 §4): what a text node's spaces, tabs and line
//! feeds turn into, one character at a time.
//!
//! [`super::inline`] feeds every character of a paragraph through [`step`], so
//! the collapsing works across text nodes and span edges: a space that follows a
//! collapsible space is dropped even when they belong to different runs.

use crate::retained_tree::{RetainedElement, RetainedTree};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WhiteSpace {
    #[default]
    Normal,
    Nowrap,
    Pre,
    PreWrap,
    PreLine,
}

impl WhiteSpace {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "normal" => Some(Self::Normal),
            "nowrap" => Some(Self::Nowrap),
            "pre" => Some(Self::Pre),
            "pre-wrap" => Some(Self::PreWrap),
            "pre-line" => Some(Self::PreLine),
            _ => None,
        }
    }

    /// Whether a run of spaces and tabs shrinks to one space.
    pub fn collapses_spaces(self) -> bool {
        matches!(self, Self::Normal | Self::Nowrap | Self::PreLine)
    }

    /// Whether a line feed ends the line. Elsewhere it is a space.
    pub fn keeps_line_feeds(self) -> bool {
        !matches!(self, Self::Normal | Self::Nowrap)
    }

    /// Whether the line may wrap at a soft wrap opportunity.
    pub fn wraps(self) -> bool {
        matches!(self, Self::Normal | Self::PreWrap | Self::PreLine)
    }
}

/// The `white-space` an element declares itself.
fn declared(element: &RetainedElement) -> Option<WhiteSpace> {
    element
        .style
        .as_deref()
        .and_then(|style| style.white_space.as_deref())
        .and_then(WhiteSpace::parse)
}

/// The `white-space` an element's own text has, inherited from the nearest
/// ancestor that declares one.
pub fn computed(tree: &RetainedTree, element: &RetainedElement) -> WhiteSpace {
    let mut current = Some(element);
    while let Some(element) = current {
        if let Some(value) = declared(element) {
            return value;
        }
        current = element.parent.and_then(|id| tree.elements.get(&id));
    }
    WhiteSpace::Normal
}

/// `value` for an inline box inside text that has `inherited`.
pub fn of_box(element: &RetainedElement, inherited: WhiteSpace) -> WhiteSpace {
    declared(element).unwrap_or(inherited)
}

/// What the last character that was kept tells the next one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tail {
    /// The start of a line: the paragraph's first, or just after a forced
    /// break. A collapsible space here is removed.
    LineStart,
    /// A collapsible space. The next one is removed.
    Space,
    Content,
}

pub enum Emit {
    Nothing,
    Char(char),
    /// A forced break. `trim` says the collapsible space before it, at the end
    /// of the line, is to be removed first.
    Break { trim: bool },
}

/// What `c`, a character of a text node with white-space `mode`, produces.
/// `\r\n` and a lone `\r` must already be `\n`.
pub fn step(tail: &mut Tail, c: char, mode: WhiteSpace) -> Emit {
    let space = |tail: &mut Tail| match *tail {
        Tail::Content => {
            *tail = Tail::Space;
            Emit::Char(' ')
        }
        Tail::LineStart | Tail::Space => Emit::Nothing,
    };
    match c {
        '\n' if mode.keeps_line_feeds() => {
            let trim = *tail == Tail::Space;
            *tail = Tail::LineStart;
            Emit::Break { trim }
        }
        '\n' => space(tail),
        ' ' | '\t' | '\u{000C}' if mode.collapses_spaces() => space(tail),
        _ => {
            *tail = Tail::Content;
            Emit::Char(c)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(mode: WhiteSpace, input: &str) -> String {
        let mut tail = Tail::LineStart;
        let mut out = String::new();
        for c in input.chars() {
            match step(&mut tail, c, mode) {
                Emit::Nothing => {}
                Emit::Char(c) => out.push(c),
                Emit::Break { trim } => {
                    if trim {
                        assert_eq!(out.pop(), Some(' '));
                    }
                    out.push('\n');
                }
            }
        }
        out
    }

    #[test]
    fn parses_the_five_values() {
        for (text, value) in [
            ("normal", WhiteSpace::Normal),
            ("nowrap", WhiteSpace::Nowrap),
            ("pre", WhiteSpace::Pre),
            ("pre-wrap", WhiteSpace::PreWrap),
            ("pre-line", WhiteSpace::PreLine),
        ] {
            assert_eq!(WhiteSpace::parse(text), Some(value));
        }
        assert_eq!(WhiteSpace::parse("break-spaces"), None);
    }

    #[test]
    fn normal_collapses_spaces_tabs_and_line_feeds_to_one_space() {
        assert_eq!(run(WhiteSpace::Normal, "a  \t b\n\n c"), "a b c");
        assert_eq!(run(WhiteSpace::Nowrap, "a \n b"), "a b");
    }

    #[test]
    fn a_leading_space_is_removed() {
        assert_eq!(run(WhiteSpace::Normal, "  a"), "a");
    }

    #[test]
    fn pre_keeps_everything() {
        assert_eq!(run(WhiteSpace::Pre, " a  b\t\n  c "), " a  b\t\n  c ");
        assert_eq!(run(WhiteSpace::PreWrap, "a  b\n c"), "a  b\n c");
    }

    #[test]
    fn pre_line_collapses_spaces_and_keeps_line_feeds_without_their_spaces() {
        assert_eq!(run(WhiteSpace::PreLine, "a   b \n  c"), "a b\nc");
    }

    #[test]
    fn wrapping_and_line_feeds_by_value() {
        assert!(WhiteSpace::Normal.wraps());
        assert!(WhiteSpace::PreWrap.wraps());
        assert!(WhiteSpace::PreLine.wraps());
        assert!(!WhiteSpace::Nowrap.wraps());
        assert!(!WhiteSpace::Pre.wraps());
        assert!(!WhiteSpace::Normal.keeps_line_feeds());
        assert!(WhiteSpace::PreLine.keeps_line_feeds());
    }
}
