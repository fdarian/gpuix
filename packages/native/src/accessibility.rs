//! GPUI accessibility props from React custom_props.
//!
//! A node reaches AccessKit only with both an `.id(...)` and a `.role(...)`.
//! GPUIX already sets the id. This module sets the role and ARIA fields.
//! Prop names match React DOM: `aria-label`, not `ariaLabel`, and the DOM `id`
//! is the node's author id.

mod aria;
pub(crate) mod label;
pub(crate) mod links;

use std::collections::HashMap;

use gpui::prelude::*;
use gpui::{AccessibleAction, Role};

use crate::events::synthetic_click::click_payload;
use crate::renderer::EventCallback;

/// Apply `role`, `id` and `aria-*` from React, plus a default role when the
/// app did not set one. `none` / `presentation` produce no node.
///
/// `host_id` is the element's renderer id, which finds the `<label>`s that
/// name it.
pub(crate) fn apply_accessibility<E>(
    mut el: E,
    host_id: u64,
    props: &HashMap<String, serde_json::Value>,
    default_role: Option<Role>,
) -> E
where
    E: StatefulInteractiveElement,
{
    let aria = aria::has_node_props(props).then(|| aria::AriaNode::parse(props));
    let hidden = aria.as_ref().is_some_and(aria::AriaNode::hidden);

    let explicit = props.get("role").and_then(|value| value.as_str());
    let role = if matches!(explicit, Some("none" | "presentation")) {
        None
    } else {
        explicit
            .and_then(role_from_aria)
            .or(default_role)
            .filter(|role| *role != Role::GenericContainer)
    };
    let links = links::current();
    let referenced_text = links.referenced_text(host_id);
    // `aria-hidden` removes the whole subtree, and only a node can carry that.
    // An element an `aria-*` reference names needs a node to be the target.
    let fallback_role = (hidden || referenced_text.is_some()).then_some(Role::Group);
    let named_by_content = role.is_none() && fallback_role.is_some();
    let Some(role) = role.or(fallback_role) else {
        return el;
    };
    let role = if role == Role::TextInput && aria::is_true_prop(props, "aria-multiline") {
        Role::MultilineTextInput
    } else {
        role
    };
    el = el.role(role);

    if let Some(text) = referenced_text.filter(|text| named_by_content && !text.is_empty()) {
        el = el.aria_label(text.to_string());
    }
    let author_id = aria::string_prop(props, "id")
        .or_else(|| links.label_author_id(host_id).map(str::to_string));
    if let Some(author_id) = author_id {
        el = el.accessibility_id(author_id);
    }
    if let Some(aria) = aria {
        el = el.a11y_node(move |node| aria.write(node));
    }

    let mut relations = aria::relations(props);
    // A DOM `aria-label` or `aria-labelledby` outranks a `<label>`.
    let named_by_author = props.contains_key("aria-label")
        || relations
            .iter()
            .any(|(relation, _)| *relation == gpui::A11yRelation::LabelledBy);
    if let Some(labels) = links.labels_of(host_id).filter(|_| !named_by_author) {
        let labels = labels
            .iter()
            .cloned()
            .map(gpui::SharedString::from)
            .collect();
        relations.push((gpui::A11yRelation::LabelledBy, labels));
    }
    for (relation, ids) in relations {
        el = el.a11y_relation(relation, ids);
    }
    el
}

/// `aria-label`, or `alt` when that is empty. Used by `<img>`.
pub(crate) fn apply_image_label<E>(
    el: E,
    props: &HashMap<String, serde_json::Value>,
    alt: &str,
) -> E
where
    E: StatefulInteractiveElement,
{
    if props.get("aria-label").is_some() || alt.is_empty() {
        return el;
    }
    el.aria_label(alt.to_string())
}

/// VoiceOver Press. GPUI auto-adds Click only for `.on_click()`. GPUIX
/// click is dispatched by `crate::events`, so register the action by hand and
/// emit the click a non-pointer activation makes. `path` is the element and its
/// retained ancestors, as built; it is empty for an element whose click nothing
/// can receive (see `click_path`), which gets no action.
pub(crate) fn apply_a11y_click<E>(
    el: E,
    events: &std::collections::HashSet<String>,
    path: Vec<u64>,
    callback: &Option<EventCallback>,
) -> E
where
    E: StatefulInteractiveElement,
{
    if !events.contains("click") || path.is_empty() {
        return el;
    }
    let callback = callback.clone();
    el.on_a11y_action(AccessibleAction::Click, move |_data, _window, _cx| {
        if let Some(callback) = &callback {
            callback(click_payload(path.clone()));
        }
    })
}

/// ARIA token → AccessKit role. Unknown tokens are ignored, like a bad style.
fn role_from_aria(token: &str) -> Option<Role> {
    Some(match token {
        "alert" => Role::Alert,
        "alertdialog" => Role::AlertDialog,
        "application" => Role::Application,
        "article" => Role::Article,
        "banner" => Role::Banner,
        "blockquote" => Role::Blockquote,
        "button" => Role::Button,
        "caption" => Role::Caption,
        "cell" => Role::Cell,
        "checkbox" => Role::CheckBox,
        "code" => Role::Code,
        "columnheader" => Role::ColumnHeader,
        "combobox" => Role::ComboBox,
        "complementary" => Role::Complementary,
        "contentinfo" => Role::ContentInfo,
        "definition" => Role::Definition,
        "deletion" => Role::ContentDeletion,
        "dialog" => Role::Dialog,
        "document" => Role::Document,
        "emphasis" => Role::Emphasis,
        "feed" => Role::Feed,
        "figure" => Role::Figure,
        "form" => Role::Form,
        "generic" => Role::GenericContainer,
        "grid" => Role::Grid,
        "gridcell" => Role::GridCell,
        "group" => Role::Group,
        "heading" => Role::Heading,
        "img" => Role::Image,
        "insertion" => Role::ContentInsertion,
        "link" => Role::Link,
        "list" => Role::List,
        "listbox" => Role::ListBox,
        "listitem" => Role::ListItem,
        "log" => Role::Log,
        "main" => Role::Main,
        "marquee" => Role::Marquee,
        "math" => Role::Math,
        "menu" => Role::Menu,
        "menubar" => Role::MenuBar,
        "menuitem" => Role::MenuItem,
        "menuitemcheckbox" => Role::MenuItemCheckBox,
        "menuitemradio" => Role::MenuItemRadio,
        "meter" => Role::Meter,
        "navigation" => Role::Navigation,
        "none" | "presentation" => Role::GenericContainer,
        "note" => Role::Note,
        "option" => Role::ListBoxOption,
        "paragraph" => Role::Paragraph,
        "progressbar" => Role::ProgressIndicator,
        "radio" => Role::RadioButton,
        "radiogroup" => Role::RadioGroup,
        "region" => Role::Region,
        "row" => Role::Row,
        "rowgroup" => Role::RowGroup,
        "rowheader" => Role::RowHeader,
        "scrollbar" => Role::ScrollBar,
        "search" => Role::Search,
        "searchbox" => Role::SearchInput,
        "separator" => Role::Splitter,
        "slider" => Role::Slider,
        "spinbutton" => Role::SpinButton,
        "status" => Role::Status,
        "strong" => Role::Strong,
        "subscript" => Role::GenericContainer,
        "superscript" => Role::GenericContainer,
        "switch" => Role::Switch,
        "tab" => Role::Tab,
        "table" => Role::Table,
        "tablist" => Role::TabList,
        "tabpanel" => Role::TabPanel,
        "term" => Role::Term,
        "textbox" => Role::TextInput,
        "time" => Role::Time,
        "timer" => Role::Timer,
        "toolbar" => Role::Toolbar,
        "tooltip" => Role::Tooltip,
        "tree" => Role::Tree,
        "treegrid" => Role::TreeGrid,
        "treeitem" => Role::TreeItem,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::role_from_aria;
    use gpui::Role;

    #[test]
    fn maps_common_aria_tokens() {
        assert_eq!(role_from_aria("button"), Some(Role::Button));
        assert_eq!(role_from_aria("heading"), Some(Role::Heading));
        assert_eq!(role_from_aria("textbox"), Some(Role::TextInput));
        assert_eq!(role_from_aria("img"), Some(Role::Image));
        assert_eq!(role_from_aria("option"), Some(Role::ListBoxOption));
        assert_eq!(role_from_aria("none"), Some(Role::GenericContainer));
        assert_eq!(role_from_aria("presentation"), Some(Role::GenericContainer));
        assert_eq!(role_from_aria("Button"), None);
        assert_eq!(role_from_aria("not-a-role"), None);
    }
}
