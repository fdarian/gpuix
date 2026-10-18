//! The style an element was last built with, reduced to what a walk over the retained tree reads.
//!
//! `RetainedElement::style` is the declared style. `build_element` resolves the one that is
//! painted from it: `@container` blocks, pointer and focus states, CSS transitions, `motion`,
//! motion values and CSS animations all change it, and the result is dropped once the element is
//! built. Hit testing (`getElementsAt`), the stacking contexts it models, the clip chain of an
//! `IntersectionObserver`, key scrolling and `scrollIntoView` walk the retained tree instead of
//! the painted one, so they would miss everything those stages do. `build_element` leaves a
//! [`Resolved`] per element here, and those walks read it.
//!
//! An element that was never built (a virtual-list row outside the window, a subtree under
//! `display: none`) has no record, and [`RetainedTree::resolved_of`] answers with its declared
//! style, which is all there is to know about it.

use rustc_hash::FxHashMap;

use crate::color::ColorContext;
use crate::css::length::LengthContext;
use crate::layout::containing_block::Position;
use crate::retained_tree::{RetainedElement, RetainedTree};
use crate::scroll::{Axes, Overflow};
use crate::style::StyleDesc;

/// `visibility`, where an unset value inherits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Visibility {
    Inherit,
    Visible,
    Hidden,
}

/// `pointer-events`, where an unset value inherits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PointerEvents {
    Inherit,
    None,
    Auto,
}

/// The fields of the final style that the retained-tree walks read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Resolved {
    pub position: Position,
    /// The `z-index`, rounded and clamped as paint reads it. `None` is `auto`.
    pub z_index: Option<i32>,
    /// The composite is applied as a whole (`opacity` below 1, a filter, a mask, a clip path),
    /// which CSS paints as a stacking context.
    pub effect_group: bool,
    pub visibility: Visibility,
    pub pointer_events: PointerEvents,
    pub overflow: Axes,
    /// `display: contents`: the element makes no box.
    pub contents: bool,
    /// `display` is `flex` or `grid` (or the inline-level spelling): the children are items.
    pub lays_out_items: bool,
}

impl Resolved {
    pub(crate) const UNSTYLED: Self = Self {
        position: Position::Static,
        z_index: None,
        effect_group: false,
        visibility: Visibility::Inherit,
        pointer_events: PointerEvents::Inherit,
        overflow: Axes {
            x: Overflow::Visible,
            y: Overflow::Visible,
        },
        contents: false,
        lays_out_items: false,
    };

    /// The record for `element`, whose resolved `style` is given. `colors` and `lengths` decide
    /// whether a `filter` or `mask` value is valid, so they are the element's own.
    pub(crate) fn of(
        element: &RetainedElement,
        style: Option<&StyleDesc>,
        colors: &ColorContext,
        lengths: &LengthContext,
    ) -> Self {
        let resolved = style.map_or(Self::UNSTYLED, |style| Self::of_style(style, colors, lengths));
        Self::with_display(element, style.and_then(|style| style.display.as_deref()), resolved)
    }

    /// Everything but the `display` flags, which depend on the element.
    pub(crate) fn of_style(style: &StyleDesc, colors: &ColorContext, lengths: &LengthContext) -> Self {
        Self {
            position: Position::parse(style.position.as_deref()),
            z_index: style.z_index.map(crate::layout::stacking::round_z),
            effect_group: crate::layout::stacking::has_effect_group(style, colors, lengths),
            visibility: match style.visibility.as_deref() {
                Some("hidden") => Visibility::Hidden,
                Some("visible") => Visibility::Visible,
                _ => Visibility::Inherit,
            },
            pointer_events: match style.pointer_events.as_deref() {
                Some("none") => PointerEvents::None,
                Some("auto") => PointerEvents::Auto,
                _ => PointerEvents::Inherit,
            },
            overflow: Axes::of(style),
            ..Self::UNSTYLED
        }
    }

    /// What the retained element declares, `@container` folds included.
    fn declared(element: &RetainedElement, colors: &ColorContext, lengths: &LengthContext) -> Self {
        let resolved = Self::of(element, element.style.as_deref(), colors, lengths);
        Self::with_display(element, element.display().as_deref(), resolved)
    }

    fn with_display(element: &RetainedElement, display: Option<&str>, resolved: Self) -> Self {
        let (contents, lays_out_items) = display_flags(element, display);
        Self {
            contents,
            lays_out_items,
            ..resolved
        }
    }

    fn escapes(&self) -> bool {
        matches!(self.position, Position::Absolute | Position::Fixed)
    }
}

/// Whether an element with this `display` makes no box and whether it lays its children out as
/// flex or grid items. `<anchored>` does the latter unless the author gives it a `display`, as it
/// is a flex column by default.
fn display_flags(element: &RetainedElement, display: Option<&str>) -> (bool, bool) {
    (
        display == Some("contents"),
        crate::layout::display::lays_out_items(display) || (display.is_none() && element.element_type == "anchored"),
    )
}

/// One [`Resolved`] per element that has been built, kept in the tree so that the walks over it
/// need nothing else. An element's entry is rewritten by every build of it and dropped when it is
/// destroyed. A subtree that a frame replays instead of rebuilding keeps its last entry, which is
/// right as nothing in it changed.
#[derive(Default)]
pub(crate) struct ResolvedStyles {
    by_element: FxHashMap<u64, Resolved>,
    /// The borders a part of a table with collapsed borders was painted with, which the table reads
    /// to share the edges between its parts. See [`Self::record_borders`].
    borders: FxHashMap<u64, [crate::css::border::SideBorder; 4]>,
    /// How many entries are `absolute` or `fixed`: a clip above them may not reach them.
    escaping: usize,
}

impl ResolvedStyles {
    pub(crate) fn record(&mut self, id: u64, resolved: Resolved) {
        let before = self.by_element.insert(id, resolved);
        self.escaping += usize::from(resolved.escapes());
        self.escaping -= usize::from(before.is_some_and(|before| before.escapes()));
    }

    pub(crate) fn forget(&mut self, id: u64) {
        self.borders.remove(&id);
        if let Some(before) = self.by_element.remove(&id) {
            self.escaping -= usize::from(before.escapes());
        }
    }

    /// The borders `id`, a part of a table with collapsed borders, declares as painted this pass:
    /// transitions, animations and `motion` included. The table is built first, so it reads
    /// the last pass's and builds again when a part has moved since.
    pub(crate) fn record_borders(&mut self, id: u64, borders: [crate::css::border::SideBorder; 4]) {
        self.borders.insert(id, borders);
    }

    pub(crate) fn borders_of(&self, id: u64) -> Option<[crate::css::border::SideBorder; 4]> {
        self.borders.get(&id).copied()
    }

    fn get(&self, id: u64) -> Option<Resolved> {
        self.by_element.get(&id).copied()
    }

    /// Whether some built element is `absolute` or `fixed` as painted.
    pub(crate) fn any_escaping(&self) -> bool {
        self.escaping > 0
    }
}

impl RetainedTree {
    /// The style `element` is painted with, as its last build resolved it, else as it is declared.
    /// `colors` and `lengths` only matter for the declared answer.
    pub(crate) fn resolved_of(
        &self,
        element: &RetainedElement,
        colors: &ColorContext,
        lengths: &LengthContext,
    ) -> Resolved {
        self.resolved
            .borrow()
            .get(element.id)
            .unwrap_or_else(|| Resolved::declared(element, colors, lengths))
    }

    /// The `position` `element` is painted with.
    pub(crate) fn position_of(&self, element: &RetainedElement) -> Position {
        match self.resolved.borrow().get(element.id) {
            Some(resolved) => resolved.position,
            None => Position::parse(element.style.as_deref().and_then(|style| style.position.as_deref())),
        }
    }

    /// Whether `element` makes no box (`display: contents`) and whether it lays its children out
    /// as flex or grid items, as painted.
    pub(crate) fn display_of(&self, element: &RetainedElement) -> (bool, bool) {
        match self.resolved.borrow().get(element.id) {
            Some(resolved) => (resolved.contents, resolved.lays_out_items),
            None => display_flags(element, element.display().as_deref()),
        }
    }

    /// The `overflow` `element` is painted with.
    pub(crate) fn overflow_of(&self, element: &RetainedElement) -> Axes {
        match self.resolved.borrow().get(element.id) {
            Some(resolved) => resolved.overflow,
            None => element.style.as_deref().map_or(Resolved::UNSTYLED.overflow, Axes::of),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::color::Scheme;

    fn contexts() -> (ColorContext, LengthContext) {
        (ColorContext::root(Scheme::Light), LengthContext::for_tests())
    }

    fn tree(nodes: &[(u64, &str, StyleDesc)]) -> RetainedTree {
        let mut tree = RetainedTree::new();
        for (id, kind, style) in nodes {
            tree.create_element(*id, kind.to_string());
            tree.set_style(*id, Arc::new(style.clone()));
        }
        tree.set_root(nodes[0].0);
        for pair in nodes.windows(2) {
            tree.append_child(pair[0].0, pair[1].0);
        }
        tree
    }

    fn style(json: &str) -> StyleDesc {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_style_is_reduced_to_what_the_walks_read() {
        let (colors, lengths) = contexts();
        let tree = tree(&[(1, "div", StyleDesc::default())]);
        let element = &tree.elements[&1];
        let resolved = Resolved::of(
            element,
            Some(&style(
                r#"{"position":"sticky","zIndex":2.6,"opacity":0.5,"visibility":"hidden","pointerEvents":"none","overflowX":"hidden","display":"grid"}"#,
            )),
            &colors,
            &lengths,
        );
        assert_eq!(resolved.position, Position::Sticky);
        assert_eq!(resolved.z_index, Some(3));
        assert!(resolved.effect_group);
        assert_eq!(resolved.visibility, Visibility::Hidden);
        assert_eq!(resolved.pointer_events, PointerEvents::None);
        assert_eq!((resolved.overflow.x, resolved.overflow.y), (Overflow::Hidden, Overflow::Auto));
        assert!(resolved.lays_out_items && !resolved.contents);
        assert_eq!(Resolved::of(element, None, &colors, &lengths), Resolved::UNSTYLED);
    }

    #[test]
    fn an_anchored_layer_is_a_flex_column_unless_it_has_a_display() {
        let (colors, lengths) = contexts();
        let tree = tree(&[(1, "anchored", StyleDesc::default()), (2, "div", StyleDesc::default())]);
        let items = |element: &RetainedElement, json: &str| Resolved::of(element, Some(&style(json)), &colors, &lengths).lays_out_items;
        assert!(items(&tree.elements[&1], "{}"));
        assert!(items(&tree.elements[&1], r#"{"display":"grid"}"#));
        assert!(!items(&tree.elements[&1], r#"{"display":"block"}"#));
        assert!(!items(&tree.elements[&2], "{}"));
    }

    #[test]
    fn an_element_that_was_never_built_answers_with_its_declared_style() {
        let (colors, lengths) = contexts();
        let tree = tree(&[(1, "div", style(r#"{"position":"absolute","opacity":0.5}"#))]);
        let element = &tree.elements[&1];
        assert_eq!(tree.position_of(element), Position::Absolute);
        assert!(tree.resolved_of(element, &colors, &lengths).effect_group);

        tree.resolved.borrow_mut().record(1, Resolved::UNSTYLED);
        assert_eq!(tree.position_of(element), Position::Static, "what it was built with wins");
        assert!(!tree.resolved_of(element, &colors, &lengths).effect_group);
    }

    #[test]
    fn a_destroyed_element_leaves_no_record() {
        let mut tree = tree(&[(1, "div", StyleDesc::default()), (2, "div", StyleDesc::default())]);
        let absolute = Resolved { position: Position::Absolute, ..Resolved::UNSTYLED };
        tree.resolved.borrow_mut().record(2, absolute);
        assert!(tree.resolved.borrow().any_escaping());
        // A second build of the same element replaces its record, not adds to the count.
        tree.resolved.borrow_mut().record(2, absolute);
        tree.resolved.borrow_mut().record(2, Resolved::UNSTYLED);
        assert!(!tree.resolved.borrow().any_escaping());
        tree.resolved.borrow_mut().record(2, absolute);
        tree.destroy_element(2);
        assert!(tree.resolved.borrow().get(2).is_none());
        assert!(!tree.resolved.borrow().any_escaping());
    }
}
