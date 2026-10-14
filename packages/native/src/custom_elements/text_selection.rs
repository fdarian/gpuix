//! The DOM selection API of `<input>` and `<textarea>`: `select()`,
//! `setSelectionRange()`, `selectionStart`, `selectionEnd`,
//! `selectionDirection`. Offsets are UTF-16 code units, like the DOM.

use std::ops::Range;

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use napi_derive::napi;

use crate::renderer::GpuixView;

/// `selectionStart`, `selectionEnd` and `selectionDirection` of a text control.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(all(target_arch = "wasm32", target_os = "unknown")), napi(object))]
pub struct TextSelection {
    pub start: u32,
    pub end: u32,
    /// `"forward"`, `"backward"` or `"none"`.
    pub direction: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelectionDirection {
    Forward,
    Backward,
    None,
}

impl SelectionDirection {
    /// Anything but `forward` and `backward` is `none`, as in the DOM.
    pub(crate) fn parse(token: Option<&str>) -> Self {
        match token {
            Some("forward") => Self::Forward,
            Some("backward") => Self::Backward,
            _ => Self::None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Forward => "forward",
            Self::Backward => "backward",
            Self::None => "none",
        }
    }
}

/// A selection as the DOM states it, before it meets the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SelectionRequest {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) direction: SelectionDirection,
}

impl SelectionRequest {
    /// From the numbers and string a DOM call passes. A negative or NaN offset
    /// is 0, as an unsigned conversion of a clamped value would be.
    pub(crate) fn from_dom(start: f64, end: f64, direction: Option<&str>) -> Self {
        Self {
            start: start as usize,
            end: end as usize,
            direction: SelectionDirection::parse(direction),
        }
    }

    pub(crate) fn select_all() -> Self {
        Self {
            start: 0,
            end: usize::MAX,
            direction: SelectionDirection::None,
        }
    }
}

/// A request resolved against the content, in the editor's own terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedSelection {
    pub(crate) range: Range<usize>,
    pub(crate) reversed: bool,
    /// Programmatic selections without a direction report `"none"`.
    pub(crate) directionless: bool,
}

pub(crate) fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// Byte offset for a UTF-16 offset. One inside a surrogate pair rounds up to
/// the end of the character; one past the end is the end.
pub(crate) fn utf16_to_byte(text: &str, offset: usize) -> usize {
    let mut byte = 0;
    let mut units = 0;
    for character in text.chars() {
        if units >= offset {
            break;
        }
        units += character.len_utf16();
        byte += character.len_utf8();
    }
    byte
}

pub(crate) fn byte_to_utf16(text: &str, offset: usize) -> usize {
    let mut units = 0;
    let mut byte = 0;
    for character in text.chars() {
        if byte >= offset {
            break;
        }
        byte += character.len_utf8();
        units += character.len_utf16();
    }
    units
}

/// `setSelectionRange` semantics: both ends clamp to the text, and an `end`
/// at or before `start` collapses the selection to `end`.
pub(crate) fn resolve(content: &str, request: SelectionRequest) -> ResolvedSelection {
    let length = utf16_len(content);
    let end = request.end.min(length);
    let start = if end <= request.start {
        end
    } else {
        request.start
    };
    let range = utf16_to_byte(content, start)..utf16_to_byte(content, end);
    ResolvedSelection {
        reversed: request.direction == SelectionDirection::Backward && !range.is_empty(),
        directionless: request.direction == SelectionDirection::None,
        range,
    }
}

/// What the DOM reports for a selection the editor holds.
///
/// `directionless_range` is the range a programmatic selection set without a
/// direction. Comparing against it, instead of clearing a flag at every edit
/// site, means any later change of the selection reports `"forward"` again.
pub(crate) fn describe(
    content: &str,
    range: &Range<usize>,
    reversed: bool,
    directionless_range: Option<&Range<usize>>,
) -> TextSelection {
    let direction = if range.is_empty() || directionless_range == Some(range) && !reversed {
        SelectionDirection::None
    } else if reversed {
        SelectionDirection::Backward
    } else {
        SelectionDirection::Forward
    };
    TextSelection {
        start: byte_to_utf16(content, range.start) as u32,
        end: byte_to_utf16(content, range.end) as u32,
        direction: direction.name().to_string(),
    }
}

impl GpuixView {
    fn text_control_props(
        &self,
        id: u64,
        operation: &str,
    ) -> Result<(String, std::collections::HashMap<String, serde_json::Value>), String> {
        let tree = self.tree.lock().unwrap();
        let element = tree
            .elements
            .get(&id)
            .ok_or_else(|| format!("Unknown element id {id}"))?;
        if !matches!(element.element_type.as_str(), "input" | "textarea") {
            return Err(format!(
                "{operation} is only valid on <input> and <textarea>, got <{}>",
                element.element_type
            ));
        }
        Ok((element.element_type.clone(), element.custom_props().clone()))
    }

    pub(crate) fn text_selection(
        &mut self,
        id: u64,
        cx: &mut gpui::Context<Self>,
    ) -> Result<TextSelection, String> {
        let (element_type, props) = self.text_control_props(id, "the selection")?;
        self.custom_registry
            .with_element(id, &element_type, &props, |element| {
                element.text_selection(cx)
            })
            .flatten()
            .ok_or_else(|| format!("Element {id} has no text selection"))
    }

    pub(crate) fn set_text_selection(
        &mut self,
        id: u64,
        request: SelectionRequest,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> Result<(), String> {
        let (element_type, props) = self.text_control_props(id, "setSelectionRange")?;
        let applied = self
            .custom_registry
            .with_element(id, &element_type, &props, |element| {
                element.set_text_selection(request, cx)
            })
            .unwrap_or(false);
        if !applied {
            return Err(format!("Element {id} has no text selection"));
        }
        cx.notify();
        window.refresh();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(start: usize, end: usize, direction: SelectionDirection) -> SelectionRequest {
        SelectionRequest {
            start,
            end,
            direction,
        }
    }

    #[test]
    fn offsets_count_utf16_units() {
        let text = "a\u{1F600}b";
        assert_eq!(utf16_len(text), 4);
        assert_eq!(utf16_to_byte(text, 1), 1);
        assert_eq!(utf16_to_byte(text, 3), 5);
        assert_eq!(utf16_to_byte(text, 4), 6);
        assert_eq!(utf16_to_byte(text, 99), 6);
        assert_eq!(byte_to_utf16(text, 5), 3);
        // Inside the pair rounds to its end.
        assert_eq!(utf16_to_byte(text, 2), 5);
    }

    #[test]
    fn select_all_covers_the_text() {
        let resolved = resolve("h\u{e9}llo", SelectionRequest::select_all());
        assert_eq!(resolved.range, 0.."h\u{e9}llo".len());
        assert!(!resolved.reversed);
        assert!(resolved.directionless);
    }

    #[test]
    fn ranges_clamp_and_collapse_like_set_selection_range() {
        let text = "hello";
        let direction = SelectionDirection::Forward;
        assert_eq!(resolve(text, request(1, 3, direction)).range, 1..3);
        assert_eq!(resolve(text, request(2, 99, direction)).range, 2..5);
        assert_eq!(resolve(text, request(99, 100, direction)).range, 5..5);
        // end <= start collapses to end
        assert_eq!(resolve(text, request(4, 2, direction)).range, 2..2);
        assert_eq!(resolve(text, request(3, 3, direction)).range, 3..3);
    }

    #[test]
    fn backward_selection_is_reversed_unless_collapsed() {
        let backward = SelectionDirection::Backward;
        assert!(resolve("hello", request(1, 3, backward)).reversed);
        assert!(!resolve("hello", request(2, 2, backward)).reversed);
        assert!(!resolve("hello", request(1, 3, SelectionDirection::Forward)).reversed);
    }

    #[test]
    fn direction_tokens() {
        assert_eq!(SelectionDirection::parse(Some("forward")), SelectionDirection::Forward);
        assert_eq!(SelectionDirection::parse(Some("backward")), SelectionDirection::Backward);
        assert_eq!(SelectionDirection::parse(Some("sideways")), SelectionDirection::None);
        assert_eq!(SelectionDirection::parse(None), SelectionDirection::None);
    }

    #[test]
    fn describe_reports_utf16_and_direction() {
        let text = "a\u{1F600}b";
        let range = 1..5;
        let forward = describe(text, &range, false, None);
        assert_eq!((forward.start, forward.end), (1, 3));
        assert_eq!(forward.direction, "forward");
        assert_eq!(describe(text, &range, true, None).direction, "backward");
        assert_eq!(describe(text, &range, false, Some(&range)).direction, "none");
        // A different selection no longer matches the directionless one.
        assert_eq!(describe(text, &(1..6), false, Some(&range)).direction, "forward");
        assert_eq!(describe(text, &(1..1), false, None).direction, "none");
    }
}
