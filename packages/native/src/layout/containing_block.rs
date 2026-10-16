//! Which ancestors reach an element: the ones whose `overflow` clips it and whose scroll offset
//! moves it. In CSS that is the chain of containing blocks, not of parents. An `absolute` box is
//! reached only by its containing block (the nearest positioned ancestor) and what reaches that
//! one, so a static `overflow: hidden` or scroller in between does nothing to it; a `fixed` box
//! is reached by nothing. zed paints, hit tests and places the box the same way
//! (`Element::escapes_to`).

use crate::retained_tree::{RetainedElement, RetainedTree};
use crate::style::StyleDesc;

/// A `position` as layout reads it: an unset or unknown value is `static`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Position {
    Static,
    Relative,
    Absolute,
    Fixed,
    Sticky,
}

impl Position {
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("relative") => Self::Relative,
            Some("absolute") => Self::Absolute,
            Some("fixed") => Self::Fixed,
            Some("sticky") => Self::Sticky,
            _ => Self::Static,
        }
    }

    fn declared(element: &RetainedElement) -> Self {
        Self::parse(element.style.as_deref().and_then(|style| style.position.as_deref()))
    }
}

/// The `position` an element was painted with, for a walk over the retained tree, which only
/// has the declared style. It differs when a `hover`, `active`, `focusVisible`, `focusWithin` or
/// group refinement sets `position`: `build_element` folds the state in and the painted box
/// records it (`ElementBounds::position`). `None` is no record, and the declared one stands.
pub(crate) type PaintedPosition<'a> = &'a dyn Fn(u64) -> Option<Position>;

fn position(element: &RetainedElement, painted: PaintedPosition) -> Position {
    painted(element.id).unwrap_or_else(|| Position::declared(element))
}

/// What the element escapes to, which is what `layout::style::apply_position` asked zed for.
pub(crate) fn escape(element: &RetainedElement, painted: PaintedPosition) -> Option<gpui::Escape> {
    match position(element, painted) {
        Position::Absolute => Some(gpui::Escape::ContainingBlock),
        Position::Fixed => Some(gpui::Escape::Viewport),
        _ => None,
    }
}

/// Whether the element can escape in some state: it does so declared, or a state refinement
/// sets an `absolute` or `fixed` position. A walk that prunes what a clip excludes needs to know
/// there is none of these first.
pub(crate) fn may_escape(element: &RetainedElement) -> bool {
    let Some(style) = element.style.as_deref() else {
        return false;
    };
    let escapes = |style: &StyleDesc| matches!(style.position.as_deref(), Some("absolute" | "fixed"));
    escapes(style)
        || [
            &style.hover,
            &style.active,
            &style.focus_visible,
            &style.focus_within,
            &style.group_hover,
            &style.group_active,
            &style.group_focus_within,
        ]
        .into_iter()
        .any(|state| state.as_deref().is_some_and(escapes))
}

/// A containing block for the boxes that escape to one. `<anchored>` counts: zed lays it out as
/// absolute whatever its style says. A `display: contents` element generates no box.
pub(crate) fn is_positioned(element: &RetainedElement, painted: PaintedPosition) -> bool {
    if super::contents::is_display_contents(element) {
        return false;
    }
    element.element_type == "anchored" || position(element, painted) != Position::Static
}

/// The nearest positioned ancestor of `element`, or the row of a `<virtual-list>` it sits in: a
/// row is laid out as a root of its own, which is the containing block when nothing inside it is
/// positioned. `None` is the window.
fn containing_block(tree: &RetainedTree, element: &RetainedElement, painted: PaintedPosition) -> Option<u64> {
    let mut current = element.parent;
    while let Some(id) = current {
        let ancestor = tree.elements.get(&id)?;
        let is_row = ancestor
            .parent
            .and_then(|parent| tree.elements.get(&parent))
            .is_some_and(|parent| parent.element_type == "virtual-list");
        if is_positioned(ancestor, painted) || is_row {
            return Some(id);
        }
        current = ancestor.parent;
    }
    None
}

/// The ancestors that clip and scroll `id`, nearest first.
pub(crate) fn reaching_ancestors(tree: &RetainedTree, id: u64, painted: PaintedPosition) -> Vec<u64> {
    let mut found = Vec::new();
    let mut current = id;
    while let Some(element) = tree.elements.get(&current) {
        let next = match escape(element, painted) {
            None => element.parent,
            Some(gpui::Escape::ContainingBlock) => containing_block(tree, element, painted),
            Some(gpui::Escape::Viewport) => None,
        };
        let Some(next) = next else {
            break;
        };
        found.push(next);
        current = next;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn declared(_: u64) -> Option<Position> {
        None
    }

    fn tree(nodes: &[(u64, &str, Option<&str>)]) -> RetainedTree {
        let mut tree = RetainedTree::new();
        for (id, kind, position) in nodes {
            tree.create_element(*id, kind.to_string());
            let style = crate::style::StyleDesc {
                position: position.map(str::to_string),
                ..Default::default()
            };
            tree.set_style(*id, Arc::new(style));
        }
        for window in nodes.windows(2) {
            tree.append_child(window[0].0, window[1].0);
        }
        tree
    }

    #[test]
    fn an_in_flow_element_is_reached_by_every_ancestor() {
        let tree = tree(&[(1, "div", None), (2, "div", Some("relative")), (3, "div", None)]);
        assert_eq!(reaching_ancestors(&tree, 3, &declared), [2, 1]);
    }

    #[test]
    fn an_absolute_element_skips_the_static_ancestors_before_its_containing_block() {
        let tree = tree(&[
            (1, "div", None),
            (2, "div", Some("relative")),
            (3, "div", Some("static")),
            (4, "div", None),
            (5, "div", Some("absolute")),
        ]);
        assert_eq!(reaching_ancestors(&tree, 5, &declared), [2, 1]);
    }

    #[test]
    fn the_containing_block_is_reached_through_what_reaches_it() {
        // 3 escapes to 2, which is itself absolute in 1: the static 4 between them is skipped
        // for 2 and so for 3.
        let tree = tree(&[
            (1, "div", Some("relative")),
            (4, "div", None),
            (2, "div", Some("absolute")),
            (3, "div", Some("absolute")),
        ]);
        assert_eq!(reaching_ancestors(&tree, 3, &declared), [2, 1]);
    }

    #[test]
    fn a_fixed_element_is_reached_by_nothing_and_an_unanchored_absolute_by_the_window() {
        let tree = tree(&[(1, "div", Some("relative")), (2, "div", Some("fixed"))]);
        assert!(reaching_ancestors(&tree, 2, &declared).is_empty());
        let tree = self::tree(&[(1, "div", None), (2, "div", Some("absolute"))]);
        assert!(reaching_ancestors(&tree, 2, &declared).is_empty());
    }

    #[test]
    fn a_virtual_list_row_is_the_containing_block_when_nothing_in_it_is_positioned() {
        let tree = tree(&[
            (1, "div", Some("relative")),
            (2, "virtual-list", None),
            (3, "div", None),
            (4, "div", None),
            (5, "div", Some("absolute")),
        ]);
        assert_eq!(reaching_ancestors(&tree, 5, &declared), [3, 2, 1]);
    }

    #[test]
    fn display_contents_and_anchored_are_decided_by_whether_they_make_a_box() {
        let mut tree = tree(&[
            (1, "div", None),
            (2, "anchored", None),
            (3, "div", Some("relative")),
            (4, "div", None),
        ]);
        let contents = crate::style::StyleDesc {
            display: Some("contents".into()),
            position: Some("relative".into()),
            ..Default::default()
        };
        tree.set_style(3, Arc::new(contents));
        let positioned = |id| is_positioned(&tree.elements[&id], &declared);
        assert!(!positioned(1));
        assert!(positioned(2));
        assert!(!positioned(3));
    }

    #[test]
    fn a_painted_position_replaces_the_declared_one() {
        let tree = tree(&[
            (1, "div", Some("relative")),
            (2, "div", None),
            (3, "div", None),
            (4, "div", None),
        ]);
        let painted = |id| (id == 4).then_some(Position::Absolute);
        assert_eq!(reaching_ancestors(&tree, 4, &declared), [3, 2, 1]);
        assert_eq!(reaching_ancestors(&tree, 4, &painted), [1]);
        // A box that was declared absolute and painted static is in flow again.
        let tree = self::tree(&[(1, "div", Some("relative")), (2, "div", None), (3, "div", Some("absolute"))]);
        let painted = |id| (id == 3).then_some(Position::Static);
        assert_eq!(reaching_ancestors(&tree, 3, &painted), [2, 1]);
        // And a painted containing block is one.
        let painted = |id| (id == 2).then_some(Position::Relative);
        let tree = self::tree(&[(1, "div", Some("relative")), (2, "div", None), (3, "div", Some("absolute"))]);
        assert_eq!(reaching_ancestors(&tree, 3, &painted), [2, 1]);
    }

    #[test]
    fn an_element_may_escape_when_a_state_refinement_positions_it() {
        let style = |json: &str| -> RetainedElement {
            let mut tree = RetainedTree::new();
            tree.create_element(1, "div".to_owned());
            tree.set_style(1, Arc::new(serde_json::from_str(json).unwrap()));
            tree.elements.remove(&1).unwrap()
        };
        assert!(!may_escape(&style(r#"{"position": "relative"}"#)));
        assert!(may_escape(&style(r#"{"position": "fixed"}"#)));
        assert!(may_escape(&style(r#"{"hover": {"position": "absolute"}}"#)));
        assert!(may_escape(&style(r#"{"groupFocusWithin": {"position": "fixed"}}"#)));
        assert!(!may_escape(&style(r#"{"hover": {"position": "relative"}}"#)));
    }
}
