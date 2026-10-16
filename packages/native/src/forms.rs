//! The `<form>` element: which controls belong to it, validation, and
//! submission.
//!
//! A control belongs to its nearest `form` ancestor. Submission validates first:
//! every `required` control with an empty value gets a non-bubbling `invalid`
//! event, and the first one no handler cancelled takes focus. When nothing is
//! invalid the form gets a cancelable, bubbling `submit` event carrying the
//! `[name, value]` entries of its named, enabled controls in document order.
//!
//! Enter in an `<input>` submits its form by HTML's implicit submission rule:
//! with no submit button in gpuix, only when the form has at most one `<input>`.
//!
//! Both events are sent with `sync_dispatch::emit`, with the root view released:
//! a handler is free to call back into the renderer.

use gpui::{App, Window};

use crate::element_tree::EventPayload;
use crate::events::keyboard::path_to_root;
use crate::events::sync_dispatch;
use crate::focus::is_text_control;
use crate::renderer::{EventCallback, GpuixView};
use crate::retained_tree::RetainedTree;

/// The nearest `form` above `control`, not counting `control` itself.
pub(crate) fn enclosing_form(tree: &RetainedTree, control: u64) -> Option<u64> {
    path_to_root(tree, control)
        .into_iter()
        .skip(1)
        .find(|id| is_form(tree, *id))
}

fn is_form(tree: &RetainedTree, id: u64) -> bool {
    tree.elements
        .get(&id)
        .is_some_and(|element| element.element_type == "form")
}

/// The controls that belong to `form`, in document order. A nested form's
/// controls belong to that form.
fn controls_of(tree: &RetainedTree, form: u64) -> Vec<u64> {
    let mut controls = Vec::new();
    let mut pending: Vec<u64> = tree
        .elements
        .get(&form)
        .map(|element| element.children.iter().rev().copied().collect())
        .unwrap_or_default();
    while let Some(id) = pending.pop() {
        let Some(element) = tree.elements.get(&id) else {
            continue;
        };
        if element.element_type == "form" {
            continue;
        }
        if is_text_control(&element.element_type) {
            controls.push(id);
        }
        pending.extend(element.children.iter().rev().copied());
    }
    controls
}

struct Control {
    id: u64,
    path: Vec<u64>,
    name: Option<String>,
    value: String,
    required: bool,
    disabled: bool,
    read_only: bool,
}

impl Control {
    /// Disabled and read-only controls are barred from constraint validation.
    fn is_invalid(&self) -> bool {
        self.required && self.value.is_empty() && !self.disabled && !self.read_only
    }

    /// `[name, value]`, for a control that has a name and is not disabled.
    fn entry(&self) -> Option<Vec<String>> {
        let name = self.name.as_ref().filter(|name| !name.is_empty())?;
        (!self.disabled).then(|| vec![name.clone(), self.value.clone()])
    }
}

struct FormRead {
    callback: Option<EventCallback>,
    path: Vec<u64>,
    controls: Vec<Control>,
}

impl GpuixView {
    fn read_form(&mut self, form: u64, cx: &mut gpui::Context<Self>) -> Result<FormRead, String> {
        let (path, members) = {
            let tree = self.tree.lock().unwrap();
            if !is_form(&tree, form) {
                return Err(format!("Element {form} is not a <form>"));
            }
            let members: Vec<_> = controls_of(&tree, form)
                .into_iter()
                .filter_map(|id| {
                    let element = tree.elements.get(&id)?;
                    Some((
                        id,
                        element.element_type.clone(),
                        element.custom_props().clone(),
                        path_to_root(&tree, id),
                    ))
                })
                .collect();
            (path_to_root(&tree, form), members)
        };

        let mut controls = Vec::with_capacity(members.len());
        for (id, element_type, props, control_path) in members {
            let value = self
                .custom_registry
                .with_element(id, &element_type, &props, |element| element.text_value(cx))
                .flatten()
                .ok_or_else(|| format!("Element {id} has no text value"))?;
            let flag = |key: &str| props.get(key).and_then(|value| value.as_bool()) == Some(true);
            controls.push(Control {
                id,
                path: control_path,
                name: props.get("name").and_then(|name| name.as_str()).map(str::to_string),
                value,
                required: flag("required"),
                disabled: flag("disabled"),
                read_only: flag("readOnly"),
            });
        }
        Ok(FormRead {
            callback: self.event_callback.clone(),
            path,
            controls,
        })
    }
}

fn path_payload(payload: &mut EventPayload, path: &[u64]) {
    payload.target = Some(path[0] as f64);
    payload.path = Some(path.iter().map(|id| *id as f64).collect());
}

/// `form.requestSubmit()`: validate, then submit.
pub(crate) fn request_submit(window: &mut Window, cx: &mut App, form: u64) -> Result<(), String> {
    let view = window
        .root::<GpuixView>()
        .flatten()
        .ok_or_else(|| "The window has no GpuixView".to_string())?;
    if view.is_updating(cx) {
        return Err(sync_dispatch::VIEW_BUSY.to_string());
    }
    let read = view.update(cx, |view, cx| view.read_form(form, cx))?;

    let mut first_unhandled = None;
    for control in read.controls.iter().filter(|control| control.is_invalid()) {
        let outcome = sync_dispatch::emit_full(
            window,
            cx,
            &read.callback,
            control.id,
            "invalid",
            |payload| path_payload(payload, &control.path),
        );
        if !outcome.default_prevented && first_unhandled.is_none() {
            first_unhandled = Some(control.id);
        }
    }
    if read.controls.iter().any(Control::is_invalid) {
        if let Some(id) = first_unhandled {
            if view.is_updating(cx) {
                return Err(sync_dispatch::VIEW_BUSY.to_string());
            }
            crate::focus::change_focus(window, cx, move |view, window, cx| {
                view.request_focus(id, window, cx)
            })?;
        }
        return Ok(());
    }

    let entries: Vec<Vec<String>> = read.controls.iter().filter_map(Control::entry).collect();
    sync_dispatch::emit_full(window, cx, &read.callback, form, "submit", |payload| {
        path_payload(payload, &read.path);
        payload.form_entries = Some(entries);
    });
    Ok(())
}

/// Whether Enter in a text field submits `form`, by HTML's implicit submission
/// rule. gpuix has no submit button (a `div` with `role="button"` is not one),
/// so no form has a default button and the rule is the fallback: submit only
/// when at most one field blocks implicit submission. Every `<input>` blocks it,
/// whatever its `type`, `disabled` or `readOnly`; a `<textarea>` does not.
fn implicit_submission_allowed(tree: &RetainedTree, form: u64) -> bool {
    controls_of(tree, form)
        .into_iter()
        .filter(|id| {
            tree.elements
                .get(id)
                .is_some_and(|element| element.element_type == "input")
        })
        .count()
        <= 1
}

/// Enter in an `<input>`: submit its form, if it has one and the form's fields
/// allow it. Runs once the key dispatch is over, so the form is read after the
/// keydown handlers have changed it.
pub(crate) fn implicit_submission(
    view: gpui::WeakEntity<GpuixView>,
    control: u64,
    window: &mut Window,
    cx: &mut App,
) {
    window.defer(cx, move |window, cx| {
        let Some(view) = view.upgrade() else {
            return;
        };
        let form = {
            let tree = view.read(cx).tree.lock().unwrap();
            enclosing_form(&tree, control).filter(|form| implicit_submission_allowed(&tree, *form))
        };
        let Some(form) = form else {
            return;
        };
        if let Err(error) = request_submit(window, cx, form) {
            log::error!("Implicit submission of form {form} failed: {error}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree_of(elements: &[(u64, &str)], edges: &[(u64, u64)]) -> RetainedTree {
        let mut tree = RetainedTree::new();
        for (id, element_type) in elements {
            tree.create_element(*id, element_type.to_string());
        }
        for (parent, child) in edges {
            tree.append_child(*parent, *child);
        }
        tree
    }

    #[test]
    fn a_control_belongs_to_its_nearest_form() {
        let tree = tree_of(
            &[(1, "form"), (2, "div"), (3, "form"), (4, "input"), (5, "input")],
            &[(1, 2), (2, 3), (3, 4), (2, 5)],
        );
        assert_eq!(enclosing_form(&tree, 4), Some(3));
        assert_eq!(enclosing_form(&tree, 5), Some(1));
        assert_eq!(enclosing_form(&tree, 1), None);
    }

    #[test]
    fn controls_come_in_document_order_without_a_nested_forms_controls() {
        let tree = tree_of(
            &[
                (1, "form"),
                (2, "input"),
                (3, "div"),
                (4, "textarea"),
                (5, "form"),
                (6, "input"),
                (7, "input"),
            ],
            &[(1, 2), (1, 3), (3, 4), (1, 5), (5, 6), (1, 7)],
        );
        assert_eq!(controls_of(&tree, 1), vec![2, 4, 7]);
        assert_eq!(controls_of(&tree, 5), vec![6]);
    }

    #[test]
    fn enter_submits_only_a_form_with_at_most_one_input() {
        let one = tree_of(
            &[(1, "form"), (2, "input"), (3, "textarea"), (4, "form"), (5, "input")],
            &[(1, 2), (1, 3), (1, 4), (4, 5)],
        );
        assert!(implicit_submission_allowed(&one, 1));
        let two = tree_of(
            &[(1, "form"), (2, "input"), (3, "div"), (4, "input")],
            &[(1, 2), (1, 3), (3, 4)],
        );
        assert!(!implicit_submission_allowed(&two, 1));
        let none = tree_of(&[(1, "form"), (2, "textarea")], &[(1, 2)]);
        assert!(implicit_submission_allowed(&none, 1));
    }

    fn control(name: Option<&str>, value: &str) -> Control {
        Control {
            id: 1,
            path: vec![1],
            name: name.map(str::to_string),
            value: value.to_string(),
            required: false,
            disabled: false,
            read_only: false,
        }
    }

    #[test]
    fn only_named_enabled_controls_make_entries() {
        assert_eq!(
            control(Some("a"), "x").entry(),
            Some(vec!["a".to_string(), "x".to_string()])
        );
        assert_eq!(control(None, "x").entry(), None);
        assert_eq!(control(Some(""), "x").entry(), None);
        let mut disabled = control(Some("a"), "x");
        disabled.disabled = true;
        assert_eq!(disabled.entry(), None);
    }

    #[test]
    fn required_empty_controls_are_invalid_unless_barred() {
        let mut required = control(Some("a"), "");
        required.required = true;
        assert!(required.is_invalid());
        required.value = "x".to_string();
        assert!(!required.is_invalid());
        required.value.clear();
        required.read_only = true;
        assert!(!required.is_invalid());
        required.read_only = false;
        required.disabled = true;
        assert!(!required.is_invalid());
    }
}
