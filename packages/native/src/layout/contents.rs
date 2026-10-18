//! `display: contents`: taffy has no such value, so the element is removed from
//! the box tree when its parent builds its children, and its children are
//! spliced into the parent's child list in its place.

use std::borrow::Cow;

use crate::retained_tree::{RetainedElement, RetainedTree};
use crate::style::StyleDesc;

pub(crate) fn is_display_contents(element: &RetainedElement) -> bool {
    element
        .style
        .as_deref()
        .and_then(|style| style.display.as_deref())
        == Some("contents")
}

/// `children` with every `display: contents` element replaced, recursively, by its
/// own children. Borrowed when there is nothing to splice.
pub(crate) fn flatten<'a>(tree: &RetainedTree, children: &'a [u64]) -> Cow<'a, [u64]> {
    let has_contents = children
        .iter()
        .any(|id| tree.elements.get(id).is_some_and(is_display_contents));
    if !has_contents {
        return Cow::Borrowed(children);
    }
    let mut flat = Vec::with_capacity(children.len());
    splice(tree, children, &mut flat);
    Cow::Owned(flat)
}

fn splice(tree: &RetainedTree, children: &[u64], into: &mut Vec<u64>) {
    for id in children {
        match tree.elements.get(id) {
            Some(element) if is_display_contents(element) => splice(tree, &element.children, into),
            _ => into.push(*id),
        }
    }
}

/// The `display: contents` ancestors between `parent` and a spliced child,
/// outermost first. Empty for a direct child.
pub(crate) fn contents_between<'a>(
    tree: &'a RetainedTree,
    parent: &RetainedElement,
    child_id: u64,
) -> Option<Vec<&'a RetainedElement>> {
    let mut chain = Vec::new();
    let mut current = tree.elements.get(&child_id)?.parent?;
    while current != parent.id {
        chain.push(tree.elements.get(&current)?);
        current = tree.elements.get(&current)?.parent?;
    }
    chain.reverse();
    Some(chain)
}

/// The `display: contents` elements that `child_id` was spliced out of, for the build of a child
/// that is not built by the parent it was spliced into: the nearest ancestor that is not one, or
/// the top element when every ancestor is one (a `display: contents` root builds its own children).
pub(crate) fn splice_parent(tree: &RetainedTree, child_id: u64) -> Option<&RetainedElement> {
    let mut current = tree.elements.get(&child_id)?.parent?;
    loop {
        let ancestor = tree.elements.get(&current)?;
        match ancestor.parent {
            Some(above) if is_display_contents(ancestor) => current = above,
            _ => return Some(ancestor),
        }
    }
}

/// What a frame that skips the spliced child needs to tell that `inherited_overlay` would give
/// it the same style: the `display: contents` ancestors between `parent` and the child, and the
/// style each had when it last changed. 0 for a direct child.
pub(crate) fn overlay_key(tree: &RetainedTree, parent: &RetainedElement, child_id: u64) -> u64 {
    let mut fp = crate::retain::Fingerprint::new();
    let mut current = tree.elements.get(&child_id).and_then(|child| child.parent);
    while let Some(id) = current.filter(|id| *id != parent.id) {
        let Some(wrapper) = tree.elements.get(&id) else { break };
        fp.word(id);
        fp.word(wrapper.style_serial);
        current = wrapper.parent;
    }
    fp.finish()
}

/// The inheritable style the `display: contents` ancestors between `parent` and a
/// spliced `child` would have passed down, for `build_element` to fill into the
/// child's own style. `None` for a direct child.
pub(crate) fn inherited_overlay(
    tree: &RetainedTree,
    parent: &RetainedElement,
    child_id: u64,
) -> Option<Box<StyleDesc>> {
    let chain = contents_between(tree, parent, child_id)?;
    if chain.is_empty() {
        return None;
    }

    let mut overlay = StyleDesc::default();
    let mut levels = Vec::new();
    // Outermost first, so a nearer ancestor overrides a farther one.
    for ancestor in chain {
        if let Some(style) = ancestor.style.as_deref() {
            overlay.take_inheritable_from(style);
            let declaration = crate::layout::container::declared(style).map(|declared| (ancestor.id, declared));
            if declaration.is_some() || style.custom_properties.is_some() {
                levels.push(ContentsLevel { declaration, custom: style.custom_properties.clone() });
            }
        }
    }
    overlay.contents_levels = (!levels.is_empty()).then(|| levels.into());
    Some(Box::new(overlay))
}

/// What a `display: contents` ancestor passes to the children spliced out of it that is not a
/// field of the style: the query container it declares, which has no box, and the custom
/// properties it sets, which a `style()` query of the child reads before the child's own build
/// has inherited anything.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ContentsLevel {
    pub declaration: Option<(u64, crate::layout::container::Declared)>,
    pub custom: Option<crate::css::custom_properties::CustomProperties>,
}

impl StyleDesc {
    /// Copies the properties CSS inherits that this style sets.
    fn take_inheritable_from(&mut self, other: &StyleDesc) {
        macro_rules! take {
            ($($field:ident),*) => {
                $(if other.$field.is_some() { self.$field = other.$field.clone(); })*
            };
        }
        take!(
            color,
            font_size,
            font_family,
            font_weight,
            text_align,
            line_height,
            white_space,
            tab_size,
            text_underline_offset,
            word_break,
            overflow_wrap,
            border_spacing,
            border_collapse,
            caption_side,
            empty_cells,
            text_wrap,
            text_wrap_mode,
            text_wrap_style,
            cursor,
            visibility,
            user_select,
            selection_color
        );
        // Either `textBoxEdge` or the edge of the `textBox` shorthand.
        if let Some(edge) = crate::css::text_box::of(other).edge {
            self.text_box_edge = Some(edge);
        }
    }

    /// Only the properties CSS inherits: what a box-less element passes to the
    /// anonymous block that stands in for it.
    pub(crate) fn inheritable_part(&self) -> StyleDesc {
        let mut part = StyleDesc::default();
        part.take_inheritable_from(self);
        part
    }

    /// What the boxes of `chain` pass down to a box inside them all, outermost first.
    pub(crate) fn inherited_through<'a>(chain: impl IntoIterator<Item = &'a StyleDesc>) -> StyleDesc {
        let mut inherited = StyleDesc::default();
        for style in chain {
            inherited.take_inheritable_from(style);
        }
        inherited
    }

    /// Fills the inheritable properties this style leaves unset from `overlay`. Borrowed when
    /// there is nothing to fill.
    pub(crate) fn with_inherited_overlay(&self, overlay: &StyleDesc) -> Cow<'_, StyleDesc> {
        let mut result = Cow::Borrowed(self);
        macro_rules! fill {
            ($($field:ident),*) => {
                $(if self.$field.is_none() && overlay.$field.is_some() { result.to_mut().$field = overlay.$field.clone(); })*
            };
        }
        fill!(
            color,
            font_size,
            font_family,
            font_weight,
            text_align,
            line_height,
            white_space,
            tab_size,
            text_underline_offset,
            word_break,
            overflow_wrap,
            border_spacing,
            border_collapse,
            caption_side,
            empty_cells,
            text_wrap,
            text_wrap_mode,
            text_wrap_style,
            cursor,
            visibility,
            user_select,
            selection_color
        );
        if overlay.text_box_edge.is_some() && crate::css::text_box::of(&result).edge.is_none() {
            result.to_mut().text_box_edge = overlay.text_box_edge;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn element(id: u64, parent: Option<u64>, display: Option<&str>, color: Option<&str>) -> RetainedElement {
        let mut element = RetainedElement::new(id, "div".to_string(), 0);
        element.parent = parent;
        element.style = Some(Arc::new(StyleDesc {
            display: display.map(str::to_string),
            color: color.map(str::to_string),
            ..Default::default()
        }));
        element
    }

    /// 1 > [2 (contents, red) > [3 (contents) > [4, 5], 6], 7]
    fn tree() -> RetainedTree {
        let mut tree = RetainedTree::new();
        let mut one = element(1, None, None, None);
        one.children = vec![2, 7];
        let mut two = element(2, Some(1), Some("contents"), Some("red"));
        two.children = vec![3, 6];
        let mut three = element(3, Some(2), Some("contents"), None);
        three.children = vec![4, 5];
        for (id, parent) in [(4, 3), (5, 3), (6, 2), (7, 1)] {
            tree.elements.insert(id, element(id, Some(parent), None, None));
        }
        tree.elements.insert(1, one);
        tree.elements.insert(2, two);
        tree.elements.insert(3, three);
        tree
    }

    #[test]
    fn splices_nested_contents_in_place() {
        let tree = tree();
        let flat = flatten(&tree, &tree.elements.get(&1).unwrap().children);
        assert_eq!(&*flat, &[4, 5, 6, 7]);
    }

    #[test]
    fn leaves_a_list_without_contents_borrowed() {
        let tree = tree();
        let flat = flatten(&tree, &[4, 5]);
        assert!(matches!(flat, Cow::Borrowed(_)));
    }

    #[test]
    fn tab_size_passes_through_a_display_contents_element() {
        let declared = StyleDesc {
            tab_size: Some(crate::css::tab_size::TabSize::Spaces(2.0)),
            ..Default::default()
        };
        let mut overlay = StyleDesc::default();
        overlay.take_inheritable_from(&declared);
        let merged = StyleDesc::default().with_inherited_overlay(&overlay).into_owned();
        assert_eq!(merged.tab_size, declared.tab_size);
    }

    #[test]
    fn text_underline_offset_passes_through_a_display_contents_element() {
        let declared = StyleDesc {
            text_underline_offset: Some(crate::css::text_underline_offset::TextUnderlineOffset::Auto),
            ..Default::default()
        };
        let mut overlay = StyleDesc::default();
        overlay.take_inheritable_from(&declared);
        let bare = StyleDesc::default();
        let merged = bare.with_inherited_overlay(&overlay);
        assert_eq!(merged.text_underline_offset, declared.text_underline_offset);
    }

    #[test]
    fn word_break_and_overflow_wrap_pass_through_a_display_contents_element() {
        let declared = StyleDesc {
            word_break: Some(crate::css::word_break::WordBreak::KeepAll),
            overflow_wrap: Some(crate::css::word_break::OverflowWrap::Anywhere),
            ..Default::default()
        };
        let mut overlay = StyleDesc::default();
        overlay.take_inheritable_from(&declared);
        let merged = StyleDesc::default().with_inherited_overlay(&overlay).into_owned();
        assert_eq!(merged.word_break, declared.word_break);
        assert_eq!(merged.overflow_wrap, declared.overflow_wrap);
    }

    #[test]
    fn text_wrap_passes_through_a_display_contents_element() {
        let declared = StyleDesc {
            text_wrap: Some(crate::css::text_wrap::TextWrap {
                mode: crate::css::text_wrap::TextWrapMode::Nowrap,
                style: crate::css::text_wrap::TextWrapStyle::Balance,
            }),
            text_wrap_mode: Some(crate::css::text_wrap::TextWrapMode::Wrap),
            text_wrap_style: Some(crate::css::text_wrap::TextWrapStyle::Pretty),
            ..Default::default()
        };
        let mut overlay = StyleDesc::default();
        overlay.take_inheritable_from(&declared);
        let bare = StyleDesc::default();
        let merged = bare.with_inherited_overlay(&overlay);
        assert_eq!(merged.text_wrap, declared.text_wrap);
        assert_eq!(merged.text_wrap_mode, declared.text_wrap_mode);
        assert_eq!(merged.text_wrap_style, declared.text_wrap_style);
    }

    #[test]
    fn spliced_children_inherit_from_the_removed_ancestors() {
        let tree = tree();
        let parent = tree.elements.get(&1).unwrap();
        assert_eq!(inherited_overlay(&tree, parent, 7), None);
        let overlay = inherited_overlay(&tree, parent, 4).unwrap();
        assert_eq!(overlay.color.as_deref(), Some("red"));

        let own = StyleDesc {
            font_size: Some(crate::css::length::LinearLength::px(12.0)),
            ..Default::default()
        };
        let merged = own.with_inherited_overlay(&overlay);
        assert_eq!(merged.color.as_deref(), Some("red"));
        assert_eq!(merged.font_size, Some(crate::css::length::LinearLength::px(12.0)));

        let own_color = StyleDesc {
            color: Some("blue".to_string()),
            ..Default::default()
        };
        assert_eq!(
            own_color.with_inherited_overlay(&overlay).color.as_deref(),
            Some("blue")
        );
    }

    #[test]
    fn a_box_less_element_hands_its_stand_in_only_the_inherited_properties() {
        let style = StyleDesc {
            color: Some("red".to_string()),
            font_size: Some(crate::css::length::LinearLength::px(12.0)),
            width: Some(crate::style::DimensionValue::Pixels(40.0)),
            background_color: Some("blue".to_string()),
            ..Default::default()
        };
        let part = style.inheritable_part();
        assert_eq!(part.color.as_deref(), Some("red"));
        assert_eq!(part.font_size, Some(crate::css::length::LinearLength::px(12.0)));
        assert!(part.width.is_none());
        assert!(part.background_color.is_none());
    }
}
