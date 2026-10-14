//! DOM focus model: what is focusable, tab order, `:focus-visible`, and the
//! four focus events.
//!
//! GPUI owns who has focus. This module owns what the DOM says about it. The
//! window's focus is observed from one place (`GpuixView::observe_focus`)
//! instead of one gpui subscription per element: gpui runs focus listeners in
//! subscription order, so per-element listeners fired in whatever order the
//! elements were created rather than blur(old) before focus(new).

use std::sync::{Arc, Mutex};

use crate::element_tree::EventPayload;
use crate::events::keyboard::{path_to_root, InputRouting};
use crate::renderer::{emit_event_full, EventCallback, GpuixView};
use crate::retained_tree::RetainedTree;

pub(crate) fn is_text_control(element_type: &str) -> bool {
    matches!(element_type, "input" | "textarea")
}

/// Largest `tabIndex` the DOM distinguishes.
const MAX_DOM_TAB_INDEX: isize = 32767;

/// gpui's `(tab_index, tab_stop)` for a DOM `tabIndex`.
///
/// gpui sorts tab stops by signed index, so a negative index comes before 0.
/// DOM puts positive indices first, in ascending order, and then the 0s. Shifting
/// every positive index below zero keeps their order and puts them ahead of the
/// 0s. A negative `tabIndex` is focusable but not a tab stop.
pub(crate) fn gpui_tab_stop(dom_tab_index: isize) -> (isize, bool) {
    if dom_tab_index > 0 {
        (
            dom_tab_index.min(MAX_DOM_TAB_INDEX) - (MAX_DOM_TAB_INDEX + 1),
            true,
        )
    } else {
        (0, dom_tab_index == 0)
    }
}

/// How the DOM focus state is read by the style pass.
#[derive(Clone, Default)]
pub(crate) struct FocusStyles {
    /// The focused control and its ancestors while it is focus-visible and not a
    /// text control, or `None`. Every other focusable dims while this is set.
    pub dim_path: Option<Arc<[u64]>>,
    /// The element matching `:focus-visible`.
    pub visible: Option<u64>,
    /// The focused element and its ancestors, which match `:focus-within`.
    pub within: Arc<[u64]>,
}

pub(crate) struct FocusChange {
    pub from: Option<u64>,
    pub to: Option<u64>,
    from_visible: bool,
    to_visible: bool,
}

#[derive(Default)]
pub(crate) struct FocusState {
    focused: Option<u64>,
    visible: bool,
    /// The next focus change is a script call (`focus()`, `autoFocus`), which
    /// inherits `:focus-visible` from the element it leaves.
    programmatic: bool,
}

impl FocusState {
    pub fn focused(&self) -> Option<u64> {
        self.focused
    }

    pub fn note_programmatic(&mut self) {
        self.programmatic = true;
    }

    /// Record where window focus is now, and report the move if there was one.
    ///
    /// `:focus-visible` follows the browser heuristic. A text control always
    /// matches. Otherwise focus matches when the keyboard moved it, or when a
    /// script moved it off an element that matched, or before the user has
    /// pressed any key or mouse button at all. A key pressed while an element is
    /// focused makes it match, whatever moved focus there.
    pub fn observe(
        &mut self,
        current: Option<u64>,
        tree: &RetainedTree,
        keyboard_modality: bool,
        user_input_seen: bool,
    ) -> Option<FocusChange> {
        let programmatic = std::mem::take(&mut self.programmatic);
        if current == self.focused {
            if current.is_some() && keyboard_modality {
                self.visible = true;
            }
            return None;
        }
        let from = self.focused;
        let from_visible = self.visible;
        let to_is_text_control = current
            .and_then(|id| tree.elements.get(&id))
            .is_some_and(|element| is_text_control(&element.element_type));
        self.focused = current;
        self.visible = current.is_some()
            && (to_is_text_control
                || keyboard_modality
                || (programmatic && (from_visible || !user_input_seen)));
        // A removed element loses focus without an event, like a removed DOM node.
        let from_alive = from.is_some_and(|id| tree.elements.contains_key(&id));
        Some(FocusChange {
            from: from.filter(|_| from_alive),
            to: current,
            from_visible,
            to_visible: self.visible,
        })
    }

    pub fn styles(&self, tree: &RetainedTree, keyboard_focus_dim: bool) -> FocusStyles {
        let Some(focused) = self.focused else {
            return FocusStyles::default();
        };
        let within: Arc<[u64]> = std::iter::successors(Some(focused), |id| {
            tree.elements.get(id).and_then(|element| element.parent)
        })
        .collect();
        let is_text_control = tree
            .elements
            .get(&focused)
            .is_some_and(|element| is_text_control(&element.element_type));
        FocusStyles {
            // Typing is keyboard input too, and dimming the window while someone
            // types is noise.
            dim_path: (self.visible && keyboard_focus_dim && !is_text_control)
                .then(|| within.clone()),
            visible: self.visible.then_some(focused),
            within,
        }
    }

    /// `element.matches(selector)` for the focus pseudo-classes.
    pub fn matches(&self, tree: &RetainedTree, id: u64, selector: &str) -> Result<bool, String> {
        match selector.trim() {
            ":focus" => Ok(self.focused == Some(id)),
            ":focus-visible" => Ok(self.focused == Some(id) && self.visible),
            ":focus-within" => Ok(std::iter::successors(self.focused, |id| {
                tree.elements.get(id).and_then(|element| element.parent)
            })
            .any(|ancestor| ancestor == id)),
            other => Err(format!(
                "matches() supports only :focus, :focus-visible and :focus-within, got `{other}`"
            )),
        }
    }
}

#[derive(Clone, Copy)]
enum FocusEventKind {
    Blur,
    FocusOut,
    Focus,
    FocusIn,
}

impl FocusEventKind {
    fn event_type(self) -> &'static str {
        match self {
            Self::Blur => "blur",
            Self::FocusOut => "focusOut",
            Self::Focus => "focus",
            Self::FocusIn => "focusIn",
        }
    }
}

/// Emit blur(old), focusout(old), focus(new), focusin(new), in that order.
///
/// Each carries its `target`, the propagation `path` from it to the root, and
/// the element on the other side of the move as `relatedTarget`. JS decides
/// which of them bubble.
pub(crate) fn emit_change(
    callback: &Option<EventCallback>,
    tree: &RetainedTree,
    change: &FocusChange,
) {
    if let Some(from) = change.from {
        for kind in [FocusEventKind::Blur, FocusEventKind::FocusOut] {
            emit(callback, tree, kind, from, change.to, change.from_visible);
        }
    }
    if let Some(to) = change.to {
        for kind in [FocusEventKind::Focus, FocusEventKind::FocusIn] {
            emit(callback, tree, kind, to, change.from, change.to_visible);
        }
    }
}

/// The one place a focus event payload is built.
fn emit(
    callback: &Option<EventCallback>,
    tree: &RetainedTree,
    kind: FocusEventKind,
    target: u64,
    related: Option<u64>,
    visible: bool,
) {
    emit_event_full(callback, target, kind.event_type(), |payload: &mut EventPayload| {
        payload.focus_visible = Some(visible);
        payload.target = Some(target as f64);
        payload.path = Some(
            path_to_root(tree, target)
                .into_iter()
                .map(|id| id as f64)
                .collect(),
        );
        payload.related_target = related.map(|id| id as f64);
    });
}

/// The DOM default of a mouse press: focus the nearest focusable element under
/// it, or nothing.
///
/// A focusable element under a bare pointer does that itself and calls
/// `prevent_default`, which is also what a JS `preventDefault()` on `mousedown`
/// maps to. What is left is a press gpui's own transfer did not take: the
/// pointer is over something that does not block it from the ancestors (a
/// filled box does), or over nothing focusable at all. It waits until the
/// dispatch is over: every listener, however deep, has had its say by then, and
/// stopping propagation does not skip it.
pub(crate) fn register_mouse_down_default(
    window: &mut gpui::Window,
    tree: Arc<Mutex<RetainedTree>>,
    routing: InputRouting,
) {
    window.on_mouse_event(move |event: &gpui::MouseDownEvent, phase, window, cx| {
        if phase != gpui::DispatchPhase::Capture {
            return;
        }
        routing.note_user_input();
        let position = event.position;
        let routing = routing.clone();
        let tree = tree.clone();
        window.defer(cx, move |window, cx| {
            if window.default_prevented() {
                return;
            }
            let under_pointer = tree.lock().unwrap().elements_at(
                f32::from(position.x) as f64,
                f32::from(position.y) as f64,
                &crate::automation::get_bounds,
            );
            match routing.first_focusable(&under_pointer) {
                Some(handle) => window.focus(&handle, cx),
                None => window.blur(),
            }
        });
    });
}

/// Opacity multiplier for every other focusable element while a control has
/// keyboard focus. Paint only, so it moves nothing.
const KEYBOARD_FOCUS_DIM: f32 = 0.4;

/// `focusWithin`, for any element: an ancestor of the focused element is not
/// focusable itself, so this cannot wait for `apply_focus_styles`.
pub(crate) fn apply_focus_within<E: gpui::Styled>(
    mut el: E,
    id: u64,
    style: Option<&crate::style::StyleDesc>,
    focus: &crate::focus::FocusStyles,
) -> E {
    if focus.within.contains(&id) {
        if let Some(declared) = style.and_then(|style| style.focus_within.as_deref()) {
            el = crate::renderer::apply_styles(el, declared);
        }
    }
    el
}

/// Focus look for a focusable element, applied right after `track_focus`.
///
/// `focusWithin` and `focusVisible` follow the DOM focus state in
/// `crate::focus`, not gpui's live input modality: gpui's `.focus_visible()`
/// would hide the ring on a text field focused by a click. While a control is
/// focus-visible, every other element dims unless its style sets
/// `keyboardFocusDim: false`; that also shows at a glance where Tab can go.
pub(crate) fn apply_focus_styles<E: gpui::Styled>(
    mut el: E,
    id: u64,
    style: Option<&crate::style::StyleDesc>,
    focus: &crate::focus::FocusStyles,
) -> E {
    el = apply_focus_within(el, id, style, focus);
    if focus.visible == Some(id) {
        if let Some(declared) = style.and_then(|style| style.focus_visible.as_deref()) {
            el = crate::renderer::apply_styles(el, declared);
        }
    }
    let opted_out = style.and_then(|style| style.keyboard_focus_dim) == Some(false);
    match focus.dim_path.as_deref() {
        // Opacity applies to the subtree, so the focused element's own
        // focusable ancestors must stay at full opacity too.
        Some(path) if !opted_out && !path.contains(&id) => {
            let base = style.and_then(|style| style.opacity).unwrap_or(1.0) as f32;
            el.opacity(base * KEYBOARD_FOCUS_DIM)
        }
        _ => el,
    }
}

impl GpuixView {
    /// `element.blur()`: a no-op unless `id` is the focused element.
    pub(crate) fn blur_element(&mut self, id: u64, window: &mut gpui::Window, cx: &mut gpui::Context<Self>) {
        self.observe_focus(window);
        if self.focus.focused() == Some(id) {
            window.blur();
            self.observe_focus(window);
        }
        cx.notify();
    }

    /// `element.matches(':focus' | ':focus-visible' | ':focus-within')`.
    pub(crate) fn matches_focus(
        &mut self,
        id: u64,
        selector: &str,
        window: &gpui::Window,
    ) -> std::result::Result<bool, String> {
        self.observe_focus(window);
        let tree = self.tree.lock().unwrap();
        self.focus.matches(&tree, id, selector)
    }

    /// Record where window focus is now and emit the events for a move.
    ///
    /// The one place focus changes are noticed. Called every frame from
    /// `sync_focus_handles`, and right after anything that moves focus or asks
    /// about it, so `focus()` and `matches()` agree without waiting for a frame.
    /// Never call it from inside a build: it takes the tree lock.
    pub(crate) fn observe_focus(&mut self, window: &gpui::Window) {
        let tree_arc = self.tree.clone();
        let tree = tree_arc.lock().unwrap();
        self.observe_focus_in(&tree, window);
    }

    pub(crate) fn observe_focus_in(&mut self, tree: &RetainedTree, window: &gpui::Window) {
        // gpui reports no focus path while the window is inactive, and so does DOM
        // (the window's blur event blurs the active element).
        let current = if window.is_window_active() {
            self.focused_element_id(window)
                .filter(|id| tree.focusable_ids.contains(id))
        } else {
            None
        };
        if let Some(change) = self.focus.observe(
            current,
            tree,
            window.last_input_was_keyboard(),
            self.routing.user_input_seen(),
        ) {
            crate::focus::emit_change(&self.event_callback, tree, &change);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree_with(elements: &[(u64, &str)]) -> RetainedTree {
        let mut tree = RetainedTree::new();
        for (id, element_type) in elements {
            tree.create_element(*id, element_type.to_string());
        }
        tree
    }

    #[test]
    fn positive_tab_indices_sort_before_zero_in_ascending_order() {
        let mut order: Vec<(isize, isize)> = [3, 0, 1, 32767, 2, 0]
            .into_iter()
            .map(|dom| (gpui_tab_stop(dom).0, dom))
            .collect();
        order.sort_by_key(|(gpui, _)| *gpui);
        let dom_order: Vec<isize> = order.into_iter().map(|(_, dom)| dom).collect();
        assert_eq!(dom_order, [1, 2, 3, 32767, 0, 0]);
    }

    #[test]
    fn negative_tab_index_is_focusable_but_not_a_tab_stop() {
        assert!(!gpui_tab_stop(-1).1);
        assert!(gpui_tab_stop(0).1);
        assert!(gpui_tab_stop(5).1);
    }

    #[test]
    fn keyboard_focus_is_visible_and_mouse_focus_is_not() {
        let tree = tree_with(&[(1, "div"), (2, "div")]);
        let mut state = FocusState::default();
        state.observe(Some(1), &tree, false, true);
        assert!(!state.matches(&tree, 1, ":focus-visible").unwrap());
        state.observe(Some(2), &tree, true, true);
        assert!(state.matches(&tree, 2, ":focus-visible").unwrap());
    }

    #[test]
    fn a_key_while_focused_makes_focus_visible() {
        let tree = tree_with(&[(1, "div")]);
        let mut state = FocusState::default();
        state.observe(Some(1), &tree, false, true);
        assert!(!state.matches(&tree, 1, ":focus-visible").unwrap());
        assert!(state.observe(Some(1), &tree, true, true).is_none());
        assert!(state.matches(&tree, 1, ":focus-visible").unwrap());
    }

    #[test]
    fn text_controls_are_always_focus_visible() {
        let tree = tree_with(&[(1, "input")]);
        let mut state = FocusState::default();
        state.observe(Some(1), &tree, false, true);
        assert!(state.matches(&tree, 1, ":focus-visible").unwrap());
    }

    #[test]
    fn script_focus_inherits_visibility_from_the_element_it_leaves() {
        let tree = tree_with(&[(1, "div"), (2, "div"), (3, "div")]);
        let mut state = FocusState::default();
        state.observe(Some(1), &tree, true, true);
        state.note_programmatic();
        state.observe(Some(2), &tree, false, true);
        assert!(state.matches(&tree, 2, ":focus-visible").unwrap());

        let mut state = FocusState::default();
        state.observe(Some(1), &tree, false, true);
        state.note_programmatic();
        state.observe(Some(3), &tree, false, true);
        assert!(!state.matches(&tree, 3, ":focus-visible").unwrap());
    }

    #[test]
    fn script_focus_before_any_user_input_is_focus_visible() {
        let tree = tree_with(&[(1, "div")]);
        let mut state = FocusState::default();
        state.note_programmatic();
        state.observe(Some(1), &tree, false, false);
        assert!(state.matches(&tree, 1, ":focus-visible").unwrap());
    }

    #[test]
    fn a_change_reports_both_ends_and_a_removed_element_is_silent() {
        let mut tree = tree_with(&[(1, "div"), (2, "div")]);
        let mut state = FocusState::default();
        state.observe(Some(1), &tree, false, true);
        let change = state.observe(Some(2), &tree, false, true).unwrap();
        assert_eq!((change.from, change.to), (Some(1), Some(2)));

        tree.destroy_element(2);
        let change = state.observe(None, &tree, false, true).unwrap();
        assert_eq!((change.from, change.to), (None, None));
    }

    #[test]
    fn matches_rejects_other_selectors() {
        let tree = tree_with(&[(1, "div")]);
        let state = FocusState::default();
        assert!(state.matches(&tree, 1, ":hover").is_err());
        assert!(state.matches(&tree, 1, "div").is_err());
    }
}
