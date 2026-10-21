//! `content-visibility` (CSS Containment 2): `hidden` skips the content of an element, `auto`
//! skips it while the element is off screen.
//!
//! An element with a value other than `visible` has style, layout and paint containment. In this
//! renderer that is a style: [`contained`] gives the element the `overflow: clip` that clips its
//! content at the padding box (unless the author's `overflow` already clips), makes it the
//! containing block of its `position: absolute` descendants and a stacking context. A skipped
//! element also has size containment, sized by `contain-intrinsic-size`, and builds none of its
//! children: their DOM, animation and transition state and resolved styles are kept, they are not
//! destroyed as under `display: none`.
//!
//! Only the host elements (`div`, `span`, `text`, `label`, `form`) skip their content. Any other
//! element has the containment and builds as it did.
//!
//! Not modelled: the element is not an independent formatting context (a margin of its first or
//! last child still collapses through it), and a `position: fixed` descendant is placed against
//! the viewport, where a contained element would be its containing block.

use serde::de;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::css::length::LengthContext;
use crate::layout::containing_block::Position;
use crate::retained_tree::{RetainedElement, RetainedTree};
use crate::scroll::{Axes, Overflow};
use crate::style::StyleDesc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContentVisibility {
    #[default]
    Visible,
    /// Skipped while the element is not relevant to the user.
    Auto,
    /// Always skipped.
    Hidden,
}

impl ContentVisibility {
    fn keyword(self) -> &'static str {
        match self {
            Self::Visible => "visible",
            Self::Auto => "auto",
            Self::Hidden => "hidden",
        }
    }

    pub(crate) fn contains(self) -> bool {
        self != Self::Visible
    }
}

impl<'de> Deserialize<'de> for ContentVisibility {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        match text.as_str() {
            "visible" => Ok(Self::Visible),
            "auto" => Ok(Self::Auto),
            "hidden" => Ok(Self::Hidden),
            _ => Err(de::Error::custom(format!(
                "invalid contentVisibility \"{text}\": expected \"visible\", \"auto\" or \"hidden\""
            ))),
        }
    }
}

impl Serialize for ContentVisibility {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.keyword())
    }
}

/// The style `style` has with the containment its `content-visibility` brings, or `None` when it
/// has none. Idempotent: the result is its own containment.
pub(crate) fn contained(style: &StyleDesc) -> Option<StyleDesc> {
    if !style.content_visibility.is_some_and(ContentVisibility::contains) {
        return None;
    }
    let mut contained = style.clone();
    // Paint containment clips at the padding box on both axes. A scroller or an `overflow: hidden`
    // already does, and `clip` beside `visible` becomes `clip` on both.
    let declared = Axes::declared(style);
    if !declared.x.has_offset() && !declared.y.has_offset() {
        let clip = |overflow| (overflow == Overflow::Visible).then(|| "clip".to_owned());
        if let Some(clip) = clip(declared.x) {
            contained.overflow_x = Some(clip);
        }
        if let Some(clip) = clip(declared.y) {
            contained.overflow_y = Some(clip);
        }
    }
    // Layout containment makes it the containing block of its absolute descendants, as a
    // positioned element is. A static one is laid out as `relative` with no offset: the insets
    // of a static element do nothing. Its `z-index` does nothing either, except for an item.
    if Position::parse(style.position.as_deref()) == Position::Static {
        contained.position = Some("relative".to_owned());
        contained.top = None;
        contained.right = None;
        contained.bottom = None;
        contained.left = None;
        contained.inset = None;
        contained.inset_block = None;
        contained.inset_inline = None;
        if !style.flex_or_grid_item {
            contained.z_index = None;
        }
    }
    Some(contained)
}

/// Whether an element whose `content-visibility` is `value` skips its content. The one place
/// that decides, so that the build and the walks over the tree cannot disagree.
pub(crate) fn skips(value: ContentVisibility, _id: u64) -> bool {
    match value {
        ContentVisibility::Hidden => true,
        // Relevance to the user decides it, and nothing says an element is not relevant yet.
        ContentVisibility::Auto | ContentVisibility::Visible => false,
    }
}

/// The content-box size `auto` in `contain-intrinsic-size` stands for: the size the element had
/// when it was last laid out while it was not skipped. Nothing records it yet, so `auto` is its
/// length.
pub(crate) fn remembered_size(_id: u64) -> Option<(f64, f64)> {
    None
}

/// Gives the box of a skipped element its size: size containment on both axes, and the content
/// box `contain-intrinsic-size` says.
pub(crate) fn contain_skipped<E: gpui::Styled>(
    element: E,
    id: u64,
    style: &StyleDesc,
    lengths: &LengthContext,
) -> E {
    let mut element = crate::layout::container::contain_size(element, crate::layout::container::ContainerType::Size);
    let axes = crate::css::contain_intrinsic::axes(style);
    let remembered = remembered_size(id);
    let size = |px: f64| gpui::px(px as f32);
    element.style().contain_intrinsic_width =
        axes.width.skipped_size(remembered.map(|size| size.0), lengths).map(size);
    element.style().contain_intrinsic_height =
        axes.height.skipped_size(remembered.map(|size| size.1), lengths).map(size);
    element
}

/// What `element` was last built with, the declared value for one that never was.
fn value_of(tree: &RetainedTree, element: &RetainedElement) -> ContentVisibility {
    tree.content_visibility_of(element)
}

/// Whether `element` is a host element that skips its content, as it was last built.
pub(crate) fn skips_content(tree: &RetainedTree, element: &RetainedElement) -> bool {
    crate::text::inline::is_host_container(element)
        && !crate::layout::contents::is_display_contents(element)
        && skips(value_of(tree, element), element.id)
}

/// Whether a strict ancestor of `element` skips its content, so nothing is built for it.
pub(crate) fn is_skipped_by_ancestor(tree: &RetainedTree, element: &RetainedElement) -> bool {
    let mut current = element.parent.and_then(|parent| tree.elements.get(&parent));
    while let Some(ancestor) = current {
        if skips_content(tree, ancestor) {
            return true;
        }
        current = ancestor.parent.and_then(|parent| tree.elements.get(&parent));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(json: &str) -> StyleDesc {
        serde_json::from_str(json).unwrap_or_else(|error| panic!("{json}: {error}"))
    }

    fn contained_of(json: &str) -> StyleDesc {
        contained(&style(json)).expect("an element with content-visibility is contained")
    }

    #[test]
    fn visible_contains_nothing() {
        assert!(contained(&style("{}")).is_none());
        assert!(contained(&style(r#"{"contentVisibility":"visible"}"#)).is_none());
        assert!(serde_json::from_str::<StyleDesc>(r#"{"contentVisibility":"maybe"}"#).is_err());
    }

    #[test]
    fn the_content_is_clipped_unless_the_author_already_clips_it() {
        let both = contained_of(r#"{"contentVisibility":"auto"}"#);
        assert_eq!((both.overflow_x.as_deref(), both.overflow_y.as_deref()), (Some("clip"), Some("clip")));
        let scroller = contained_of(r#"{"contentVisibility":"auto","overflowY":"scroll"}"#);
        assert_eq!((scroller.overflow_x, scroller.overflow_y.as_deref()), (None, Some("scroll")));
        let hidden = contained_of(r#"{"contentVisibility":"hidden","overflow":"hidden"}"#);
        assert_eq!(hidden.overflow.as_deref(), Some("hidden"));
        assert!(hidden.overflow_x.is_none() && hidden.overflow_y.is_none());
        let beside_clip = contained_of(r#"{"contentVisibility":"auto","overflowX":"clip"}"#);
        assert_eq!(
            (beside_clip.overflow_x.as_deref(), beside_clip.overflow_y.as_deref()),
            (Some("clip"), Some("clip"))
        );
    }

    #[test]
    fn a_static_element_becomes_a_containing_block_without_an_offset() {
        let static_box = contained_of(r#"{"contentVisibility":"hidden","top":5,"inset":3,"zIndex":4}"#);
        assert_eq!(static_box.position.as_deref(), Some("relative"));
        assert!(static_box.top.is_none() && static_box.inset.is_none() && static_box.z_index.is_none());
        let positioned = contained_of(r#"{"contentVisibility":"hidden","position":"absolute","top":5,"zIndex":4}"#);
        assert_eq!(positioned.position.as_deref(), Some("absolute"));
        assert!(positioned.top.is_some() && positioned.z_index == Some(4.0));
        let item = StyleDesc { flex_or_grid_item: true, ..style(r#"{"contentVisibility":"auto","zIndex":2}"#) };
        assert_eq!(contained(&item).unwrap().z_index, Some(2.0));
    }

    #[test]
    fn containing_twice_changes_nothing() {
        let once = contained_of(r#"{"contentVisibility":"auto","top":1}"#);
        assert_eq!(contained(&once).unwrap(), once);
    }

    #[test]
    fn the_walks_over_the_tree_see_a_stacking_context_that_is_a_containing_block() {
        use crate::color::{ColorContext, Scheme};
        use crate::layout::{containing_block, stacking};

        let (colors, lengths) = (ColorContext::root(Scheme::Light), LengthContext::for_tests());
        let mut tree = RetainedTree::new();
        for (id, json) in [(1, r#"{"contentVisibility":"auto"}"#), (2, r#"{"position":"absolute"}"#), (3, "{}")] {
            tree.create_element(id, "div".to_owned());
            tree.set_style(id, std::sync::Arc::new(style(json)));
        }
        tree.set_root(1);
        tree.append_child(1, 2);
        tree.append_child(1, 3);
        let contained = &tree.elements[&1];
        let resolved = tree.resolved_of(contained, &colors, &lengths);
        assert!(stacking::creates_context(&resolved, false));
        assert_eq!(stacking::lift_z(&resolved, false), Some(0));
        assert!(containing_block::is_positioned(&tree, contained));
        assert_eq!(containing_block::reaching_ancestors(&tree, 2), vec![1]);
        assert!(!containing_block::is_positioned(&tree, &tree.elements[&3]));
        assert!(resolved.overflow.x.clips() && resolved.overflow.y.clips());
    }

    #[test]
    fn only_hidden_skips_for_now() {
        assert!(skips(ContentVisibility::Hidden, 1));
        assert!(!skips(ContentVisibility::Auto, 1));
        assert!(!skips(ContentVisibility::Visible, 1));
    }
}
