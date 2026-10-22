//! Tab into the content of an `auto` element that skips it.
//!
//! gpui moves focus along the tab stops that were painted, and nothing below a skipped element is
//! built. Chrome lets Tab reach a focusable element in skipped `auto` content (`hidden` content is
//! not reachable), scrolls it into view and makes the element relevant. So Tab compares the stop
//! gpui would pick with the focusable elements of skipped `auto` content, in tab order, and takes
//! the one that comes first.

use gpui::{App, Window};

use crate::events::sync_dispatch;
use crate::layout::content_visibility::ContentVisibility;
use crate::retained_tree::{RetainedElement, RetainedTree};

/// Where a tab stop is in the tab order: its `tabIndex` group, then its place in the document.
type Place = (isize, Vec<u32>);

/// The tab stop in skipped content that Tab (or Shift+Tab with `backwards`) moves to rather than
/// the one gpui would, if there is one.
pub(super) fn target(window: &Window, cx: &App, backwards: bool) -> Result<Option<u64>, String> {
    let view = sync_dispatch::root_view(window)?;
    if view.is_updating(cx) {
        return Ok(None);
    }
    let view = view.read(cx);
    let tree = view.tree.lock().unwrap();
    if tree.relevance.is_empty() {
        return Ok(None);
    }
    let mut candidates: Vec<(Place, u64)> = tree
        .focusable_ids
        .iter()
        .filter(|id| view.focus_handles.contains_key(id))
        .filter_map(|id| {
            let element = tree.elements.get(id)?;
            let (_, tab_stop) = crate::renderer::focus_handle_tab(element);
            (tab_stop && is_in_skipped_auto(&tree, element)).then(|| (place(&tree, element), *id))
        })
        .collect();
    if candidates.is_empty() {
        return Ok(None);
    }
    candidates.sort();
    if backwards {
        candidates.reverse();
    }
    let retained_id_of = |handle: &gpui::FocusHandle| {
        view.focus_handles
            .iter()
            .find_map(|(id, held)| (held.id() == handle.id()).then_some(*id))
    };
    let painted = if backwards { window.prev_tab_stop() } else { window.next_tab_stop() };
    let painted = match painted {
        Some(handle) => match retained_id_of(&handle) {
            Some(id) => Some(place(&tree, tree.elements.get(&id).ok_or("a tab stop has no element")?)),
            // A stop that is no element of the tree (a part of an editor) cannot be ordered.
            None => return Ok(None),
        },
        None => None,
    };
    let current = window
        .focused(cx)
        .and_then(|handle| retained_id_of(&handle))
        .and_then(|id| tree.elements.get(&id))
        .map(|element| place(&tree, element));

    Ok(choose(&candidates, painted, current, backwards))
}

/// The candidate Tab moves to instead of the painted stop, if one comes first. `candidates` are in
/// travel order, `painted` is where gpui would go and `current` where focus is.
fn choose(candidates: &[(Place, u64)], painted: Option<Place>, current: Option<Place>, backwards: bool) -> Option<u64> {
    // `a` is before `b` when it is met first.
    let before = |a: &Place, b: &Place| if backwards { a > b } else { a < b };
    let after_current = candidates
        .iter()
        .find(|(place, _)| current.as_ref().is_none_or(|current| before(current, place)));
    let first = candidates.first();
    let earlier = |candidate: Option<&(Place, u64)>, painted: &Place| {
        candidate.filter(|(place, _)| before(place, painted)).map(|(_, id)| *id)
    };
    match (painted, current) {
        (Some(painted), Some(current)) if !before(&current, &painted) => {
            // gpui wrapped around: what lies between here and the end comes first.
            after_current.map(|(_, id)| *id).or_else(|| earlier(first, &painted))
        }
        (Some(painted), _) => earlier(after_current, &painted),
        (None, _) => after_current.or(first).map(|(_, id)| *id),
    }
}

fn place(tree: &RetainedTree, element: &RetainedElement) -> Place {
    let mut path = Vec::new();
    let mut current = element;
    while let Some(parent) = current.parent.and_then(|parent| tree.elements.get(&parent)) {
        let index = parent.children.iter().position(|child| *child == current.id);
        path.push(index.unwrap_or(0) as u32);
        current = parent;
    }
    path.reverse();
    (crate::renderer::focus_handle_tab(element).0, path)
}

/// Whether `element` is below an `auto` element that skips its content, and reachable: no box of it
/// or of an element between is `display: none`, and none of them skips its content for good
/// (`hidden`).
fn is_in_skipped_auto(tree: &RetainedTree, element: &RetainedElement) -> bool {
    if crate::layout::box_generation::is_display_none(element) {
        return false;
    }
    let mut skipped_auto = false;
    let mut current = element.parent.and_then(|parent| tree.elements.get(&parent));
    while let Some(ancestor) = current {
        if crate::layout::box_generation::is_display_none(ancestor) {
            return false;
        }
        if crate::text::inline::is_host_container(ancestor) && !crate::layout::contents::is_display_contents(ancestor) {
            match tree.content_visibility_of(ancestor) {
                ContentVisibility::Hidden => return false,
                ContentVisibility::Auto => {
                    skipped_auto |= crate::layout::content_visibility::skips_content(tree, ancestor)
                }
                ContentVisibility::Visible => {}
            }
        }
        current = ancestor.parent.and_then(|parent| tree.elements.get(&parent));
    }
    skipped_auto
}

/// Focuses `id`, which `target` chose, and scrolls it into view. Its element is not built, so its box
/// is read from a layout of the skipped content around it (as a read of geometry does), and the frame
/// that follows builds the element, which holds the focus.
pub(super) fn enter(window: &mut Window, cx: &mut App, id: u64) -> Result<(), String> {
    super::change_focus(window, cx, move |view, window, cx| {
        if let Some(handle) = view.focus_handles.get(&id) {
            handle.focus(window, cx);
        }
    })?;
    let view = sync_dispatch::root_view(window)?;
    let tree = view.read(cx).tree.clone();
    let target = crate::renderer::read_after_layout(&tree, id, window, cx, crate::renderer::SkippedContent::Force, Window::draw_if_dirty, |window_id| {
        crate::renderer::painted_rect(window_id, id)
    })
    .map_err(|error| error.to_string())?;
    view.update(cx, |view, cx| {
        if view.scroll_element_into_view_at(window, id, crate::scroll::IntoView::NEAREST, target) {
            cx.notify();
            window.refresh();
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(group: isize, path: &[u32]) -> Place {
        (group, path.to_vec())
    }

    /// Candidates at paths [5], [7], [9], in travel order for the direction.
    fn candidates(backwards: bool) -> Vec<(Place, u64)> {
        let mut list = vec![(at(0, &[5]), 5), (at(0, &[7]), 7), (at(0, &[9]), 9)];
        if backwards {
            list.reverse();
        }
        list
    }

    #[test]
    fn a_candidate_between_here_and_the_painted_stop_comes_first() {
        let painted = Some(at(0, &[8]));
        assert_eq!(choose(&candidates(false), painted.clone(), Some(at(0, &[1])), false), Some(5));
        assert_eq!(choose(&candidates(false), painted, Some(at(0, &[6])), false), Some(7));
    }

    #[test]
    fn the_painted_stop_wins_when_it_comes_first() {
        let painted = Some(at(0, &[2]));
        assert_eq!(choose(&candidates(false), painted, Some(at(0, &[1])), false), None);
    }

    #[test]
    fn past_the_last_painted_stop_gpui_wraps_and_the_candidate_after_here_is_taken() {
        // Focus is at [3], the last painted stop; gpui would go back to the first, [0].
        assert_eq!(choose(&candidates(false), Some(at(0, &[0])), Some(at(0, &[3])), false), Some(5));
    }

    #[test]
    fn after_the_last_candidate_gpui_wraps_to_the_first_stop_or_the_first_candidate() {
        let at_the_end = Some(at(0, &[10]));
        assert_eq!(choose(&candidates(false), Some(at(0, &[1])), at_the_end.clone(), false), None);
        assert_eq!(choose(&candidates(false), Some(at(0, &[6])), at_the_end, false), Some(5));
    }

    #[test]
    fn with_nothing_focused_the_first_stop_in_the_order_is_taken() {
        assert_eq!(choose(&candidates(false), Some(at(0, &[2])), None, false), None);
        assert_eq!(choose(&candidates(false), Some(at(0, &[6])), None, false), Some(5));
        assert_eq!(choose(&candidates(false), None, None, false), Some(5));
    }

    #[test]
    fn shift_tab_goes_the_other_way() {
        let painted = Some(at(0, &[6]));
        assert_eq!(choose(&candidates(true), painted.clone(), Some(at(0, &[8])), true), Some(7));
        // From the first stop gpui wraps to the last painted one, [6]: the candidate at [9] is met before it.
        assert_eq!(choose(&candidates(true), painted, Some(at(0, &[4])), true), Some(9));
        assert_eq!(choose(&candidates(true), Some(at(0, &[10])), Some(at(0, &[1])), true), None);
    }

    #[test]
    fn a_positive_tab_index_is_met_before_the_zeros() {
        let positive = at(-3, &[40]);
        let list = vec![(positive.clone(), 1), (at(0, &[2]), 2)];
        assert_eq!(choose(&list, Some(at(0, &[0])), Some(at(-5, &[50])), false), Some(1));
        assert_eq!(choose(&list, Some(at(0, &[5])), Some(positive), false), Some(2));
    }
}
