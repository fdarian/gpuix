//! `aria-*` props to AccessKit node properties.
//!
//! Values follow React DOM: `aria-expanded={true}` and `aria-expanded="true"`
//! mean the same. A value the attribute does not allow is ignored, as a
//! browser ignores it.

use std::collections::HashMap;

use gpui::accesskit::{
    AriaCurrent, AutoComplete, HasPopup, Invalid, Live, Node, Orientation, SortDirection, Toggled,
};
use gpui::A11yRelation;
use serde_json::Value;

type Props = HashMap<String, Value>;

/// Whether any `aria-*` prop, `disabled` or `inert` is present.
pub(super) fn has_node_props(props: &Props) -> bool {
    props.contains_key("disabled")
        || props.contains_key("inert")
        || props.keys().any(|key| key.starts_with("aria-"))
}

/// Whether `aria-hidden` or `inert` takes the element and everything below it out of the tree.
pub(super) fn hides_subtree(props: &Props) -> bool {
    is_true(props, "aria-hidden") || is_true(props, "inert")
}

/// The attribute text of a prop: strings as they are, booleans and numbers as
/// React DOM would stringify them.
fn text(props: &Props, key: &str) -> Option<String> {
    match props.get(key)? {
        Value::String(value) => Some(value.clone()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn nonempty_text(props: &Props, key: &str) -> Option<String> {
    text(props, key).filter(|value| !value.is_empty())
}

fn boolean(props: &Props, key: &str) -> Option<bool> {
    match text(props, key)?.as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn is_true(props: &Props, key: &str) -> bool {
    boolean(props, key) == Some(true)
}

fn number(props: &Props, key: &str) -> Option<f64> {
    text(props, key)?
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

/// A positive integer. ARIA allows `-1` for "unknown" counts, which AccessKit
/// cannot express, so those are dropped with the rest of the invalid values.
fn count(props: &Props, key: &str) -> Option<usize> {
    let value = number(props, key)?;
    (value >= 0.0 && value.fract() == 0.0).then_some(value as usize)
}

fn toggled(props: &Props, key: &str) -> Option<Toggled> {
    match text(props, key)?.as_str() {
        "true" => Some(Toggled::True),
        "false" => Some(Toggled::False),
        "mixed" => Some(Toggled::Mixed),
        _ => None,
    }
}

/// Everything `aria-*` and `disabled` say about one node, parsed once so the
/// per-frame closure does not hold the prop map.
#[derive(Default)]
pub(super) struct AriaNode {
    label: Option<String>,
    description: Option<String>,
    value_text: Option<String>,
    placeholder: Option<String>,
    key_shortcuts: Option<String>,
    role_description: Option<String>,
    braille_label: Option<String>,
    braille_role_description: Option<String>,
    row_index_text: Option<String>,
    column_index_text: Option<String>,
    expanded: Option<bool>,
    selected: Option<bool>,
    toggled: Option<Toggled>,
    disabled: bool,
    hidden: bool,
    required: bool,
    read_only: bool,
    multiselectable: bool,
    busy: bool,
    live_atomic: bool,
    modal: bool,
    invalid: Option<Invalid>,
    live: Option<Live>,
    current: Option<AriaCurrent>,
    has_popup: Option<HasPopup>,
    orientation: Option<Orientation>,
    sort_direction: Option<SortDirection>,
    auto_complete: Option<AutoComplete>,
    value_now: Option<f64>,
    value_min: Option<f64>,
    value_max: Option<f64>,
    level: Option<usize>,
    position_in_set: Option<usize>,
    size_of_set: Option<usize>,
    row_index: Option<usize>,
    column_index: Option<usize>,
    row_count: Option<usize>,
    column_count: Option<usize>,
    row_span: Option<usize>,
    column_span: Option<usize>,
}

impl AriaNode {
    pub(super) fn parse(props: &Props) -> Self {
        Self {
            label: nonempty_text(props, "aria-label"),
            description: nonempty_text(props, "aria-description"),
            value_text: nonempty_text(props, "aria-valuetext"),
            placeholder: nonempty_text(props, "aria-placeholder"),
            key_shortcuts: nonempty_text(props, "aria-keyshortcuts"),
            role_description: nonempty_text(props, "aria-roledescription"),
            braille_label: nonempty_text(props, "aria-braillelabel"),
            braille_role_description: nonempty_text(props, "aria-brailleroledescription"),
            row_index_text: nonempty_text(props, "aria-rowindextext"),
            column_index_text: nonempty_text(props, "aria-colindextext"),
            expanded: boolean(props, "aria-expanded"),
            selected: boolean(props, "aria-selected"),
            toggled: toggled(props, "aria-pressed").or_else(|| toggled(props, "aria-checked")),
            disabled: is_true(props, "disabled") || is_true(props, "aria-disabled"),
            // An inert subtree is out of the accessibility tree, like aria-hidden.
            hidden: hides_subtree(props),
            required: is_true(props, "aria-required"),
            read_only: is_true(props, "aria-readonly"),
            multiselectable: is_true(props, "aria-multiselectable"),
            busy: is_true(props, "aria-busy"),
            live_atomic: is_true(props, "aria-atomic"),
            modal: is_true(props, "aria-modal"),
            invalid: text(props, "aria-invalid").and_then(|value| match value.as_str() {
                "true" => Some(Invalid::True),
                "grammar" => Some(Invalid::Grammar),
                "spelling" => Some(Invalid::Spelling),
                _ => None,
            }),
            live: text(props, "aria-live").and_then(|value| match value.as_str() {
                "off" => Some(Live::Off),
                "polite" => Some(Live::Polite),
                "assertive" => Some(Live::Assertive),
                _ => None,
            }),
            current: text(props, "aria-current").and_then(|value| match value.as_str() {
                "true" => Some(AriaCurrent::True),
                "false" => Some(AriaCurrent::False),
                "page" => Some(AriaCurrent::Page),
                "step" => Some(AriaCurrent::Step),
                "location" => Some(AriaCurrent::Location),
                "date" => Some(AriaCurrent::Date),
                "time" => Some(AriaCurrent::Time),
                _ => None,
            }),
            has_popup: text(props, "aria-haspopup").and_then(|value| match value.as_str() {
                // `true` is `menu` in ARIA.
                "true" | "menu" => Some(HasPopup::Menu),
                "listbox" => Some(HasPopup::Listbox),
                "tree" => Some(HasPopup::Tree),
                "grid" => Some(HasPopup::Grid),
                "dialog" => Some(HasPopup::Dialog),
                _ => None,
            }),
            orientation: text(props, "aria-orientation").and_then(|value| match value.as_str() {
                "horizontal" => Some(Orientation::Horizontal),
                "vertical" => Some(Orientation::Vertical),
                _ => None,
            }),
            sort_direction: text(props, "aria-sort").and_then(|value| match value.as_str() {
                "ascending" => Some(SortDirection::Ascending),
                "descending" => Some(SortDirection::Descending),
                "other" => Some(SortDirection::Other),
                _ => None,
            }),
            auto_complete: text(props, "aria-autocomplete").and_then(|value| {
                match value.as_str() {
                    "inline" => Some(AutoComplete::Inline),
                    "list" => Some(AutoComplete::List),
                    "both" => Some(AutoComplete::Both),
                    _ => None,
                }
            }),
            value_now: number(props, "aria-valuenow"),
            value_min: number(props, "aria-valuemin"),
            value_max: number(props, "aria-valuemax"),
            level: count(props, "aria-level").filter(|level| *level > 0),
            position_in_set: count(props, "aria-posinset"),
            size_of_set: count(props, "aria-setsize"),
            row_index: count(props, "aria-rowindex"),
            column_index: count(props, "aria-colindex"),
            row_count: count(props, "aria-rowcount"),
            column_count: count(props, "aria-colcount"),
            row_span: count(props, "aria-rowspan"),
            column_span: count(props, "aria-colspan"),
        }
    }

    pub(super) fn hidden(&self) -> bool {
        self.hidden
    }

    pub(super) fn write(&self, node: &mut Node) {
        if let Some(value) = &self.label {
            node.set_label(value.as_str());
        }
        if let Some(value) = &self.description {
            node.set_description(value.as_str());
        }
        if let Some(value) = &self.value_text {
            node.set_value(value.as_str());
        }
        if let Some(value) = &self.placeholder {
            node.set_placeholder(value.as_str());
        }
        if let Some(value) = &self.key_shortcuts {
            node.set_keyboard_shortcut(value.as_str());
        }
        if let Some(value) = &self.role_description {
            node.set_role_description(value.as_str());
        }
        if let Some(value) = &self.braille_label {
            node.set_braille_label(value.as_str());
        }
        if let Some(value) = &self.braille_role_description {
            node.set_braille_role_description(value.as_str());
        }
        if let Some(value) = &self.row_index_text {
            node.set_row_index_text(value.as_str());
        }
        if let Some(value) = &self.column_index_text {
            node.set_column_index_text(value.as_str());
        }
        if let Some(value) = self.expanded {
            node.set_expanded(value);
        }
        if let Some(value) = self.selected {
            node.set_selected(value);
        }
        if let Some(value) = self.toggled {
            node.set_toggled(value);
        }
        if self.disabled {
            node.set_disabled();
        }
        if self.hidden {
            node.set_hidden();
        }
        if self.required {
            node.set_required();
        }
        if self.read_only {
            node.set_read_only();
        }
        if self.multiselectable {
            node.set_multiselectable();
        }
        if self.busy {
            node.set_busy();
        }
        if self.live_atomic {
            node.set_live_atomic();
        }
        if self.modal {
            node.set_modal();
        }
        if let Some(value) = self.invalid {
            node.set_invalid(value);
        }
        if let Some(value) = self.live {
            node.set_live(value);
        }
        if let Some(value) = self.current {
            node.set_aria_current(value);
        }
        if let Some(value) = self.has_popup {
            node.set_has_popup(value);
        }
        if let Some(value) = self.orientation {
            node.set_orientation(value);
        }
        if let Some(value) = self.sort_direction {
            node.set_sort_direction(value);
        }
        if let Some(value) = self.auto_complete {
            node.set_auto_complete(value);
        }
        if let Some(value) = self.value_now {
            node.set_numeric_value(value);
        }
        if let Some(value) = self.value_min {
            node.set_min_numeric_value(value);
        }
        if let Some(value) = self.value_max {
            node.set_max_numeric_value(value);
        }
        if let Some(value) = self.level {
            node.set_level(value);
        }
        if let Some(value) = self.position_in_set {
            node.set_position_in_set(value);
        }
        if let Some(value) = self.size_of_set {
            node.set_size_of_set(value);
        }
        if let Some(value) = self.row_index {
            node.set_row_index(value);
        }
        if let Some(value) = self.column_index {
            node.set_column_index(value);
        }
        if let Some(value) = self.row_count {
            node.set_row_count(value);
        }
        if let Some(value) = self.column_count {
            node.set_column_count(value);
        }
        if let Some(value) = self.row_span {
            node.set_row_span(value);
        }
        if let Some(value) = self.column_span {
            node.set_column_span(value);
        }
    }
}

/// `aria-*` props that name other elements, with the relation each becomes.
pub(super) const RELATION_PROPS: [(&str, A11yRelation); 8] = [
    ("aria-labelledby", A11yRelation::LabelledBy),
    ("aria-describedby", A11yRelation::DescribedBy),
    ("aria-controls", A11yRelation::Controls),
    ("aria-owns", A11yRelation::Owns),
    ("aria-details", A11yRelation::Details),
    ("aria-flowto", A11yRelation::FlowTo),
    ("aria-errormessage", A11yRelation::ErrorMessage),
    ("aria-activedescendant", A11yRelation::ActiveDescendant),
];

/// The id references an element declares, as author ids.
pub(super) fn relations(props: &Props) -> Vec<(A11yRelation, Vec<gpui::SharedString>)> {
    RELATION_PROPS
        .iter()
        .filter_map(|(key, relation)| {
            let value = nonempty_text(props, key)?;
            let ids: Vec<gpui::SharedString> = value
                .split_whitespace()
                .map(|id| gpui::SharedString::from(id.to_string()))
                .collect();
            (!ids.is_empty()).then_some((*relation, ids))
        })
        .collect()
}

pub(super) fn is_true_prop(props: &Props, key: &str) -> bool {
    is_true(props, key)
}

pub(super) fn string_prop(props: &Props, key: &str) -> Option<String> {
    nonempty_text(props, key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn props(value: Value) -> Props {
        serde_json::from_value(value).expect("props are an object")
    }

    fn written(value: Value) -> Node {
        let mut node = Node::new(gpui::accesskit::Role::Button);
        AriaNode::parse(&props(value)).write(&mut node);
        node
    }

    #[test]
    fn booleans_and_strings_mean_the_same() {
        for expanded in [json!(true), json!("true")] {
            assert_eq!(
                written(json!({ "aria-expanded": expanded })).is_expanded(),
                Some(true)
            );
        }
        assert_eq!(
            written(json!({ "aria-selected": false })).is_selected(),
            Some(false)
        );
        assert_eq!(
            written(json!({ "aria-expanded": "maybe" })).is_expanded(),
            None
        );
    }

    #[test]
    fn checked_and_pressed_map_to_toggled_including_mixed() {
        assert_eq!(
            written(json!({ "aria-checked": "mixed" })).toggled(),
            Some(Toggled::Mixed)
        );
        assert_eq!(
            written(json!({ "aria-pressed": true })).toggled(),
            Some(Toggled::True)
        );
        assert_eq!(
            written(json!({ "aria-checked": false })).toggled(),
            Some(Toggled::False)
        );
    }

    #[test]
    fn flags_numbers_and_tokens() {
        let node = written(json!({
            "aria-disabled": true,
            "aria-hidden": "true",
            "aria-required": true,
            "aria-readonly": true,
            "aria-busy": true,
            "aria-atomic": true,
            "aria-modal": true,
            "aria-multiselectable": true,
            "aria-valuenow": 3,
            "aria-valuemin": "0",
            "aria-valuemax": 10.5,
            "aria-live": "assertive",
            "aria-invalid": "spelling",
            "aria-current": "page",
            "aria-haspopup": true,
            "aria-orientation": "vertical",
            "aria-sort": "descending",
            "aria-autocomplete": "both",
            "aria-posinset": 2,
            "aria-setsize": 5,
            "aria-rowindex": 4,
            "aria-colspan": 2,
            "aria-level": 3,
        }));
        assert!(node.is_disabled() && node.is_hidden() && node.is_required());
        assert!(node.is_read_only() && node.is_busy() && node.is_live_atomic());
        assert!(node.is_modal() && node.is_multiselectable());
        assert_eq!(node.numeric_value(), Some(3.0));
        assert_eq!(node.min_numeric_value(), Some(0.0));
        assert_eq!(node.max_numeric_value(), Some(10.5));
        assert_eq!(node.live(), Some(Live::Assertive));
        assert_eq!(node.invalid(), Some(Invalid::Spelling));
        assert_eq!(node.aria_current(), Some(AriaCurrent::Page));
        assert_eq!(node.has_popup(), Some(HasPopup::Menu));
        assert_eq!(node.orientation(), Some(Orientation::Vertical));
        assert_eq!(node.sort_direction(), Some(SortDirection::Descending));
        assert_eq!(node.auto_complete(), Some(AutoComplete::Both));
        assert_eq!(node.position_in_set(), Some(2));
        assert_eq!(node.size_of_set(), Some(5));
        assert_eq!(node.row_index(), Some(4));
        assert_eq!(node.column_span(), Some(2));
        assert_eq!(node.level(), Some(3));
    }

    #[test]
    fn false_flags_and_bad_tokens_write_nothing() {
        let node = written(json!({
            "aria-disabled": false,
            "aria-hidden": "false",
            "aria-live": "loud",
            "aria-rowcount": -1,
            "aria-valuenow": "abc",
        }));
        assert!(!node.is_disabled() && !node.is_hidden());
        assert_eq!(node.live(), None);
        assert_eq!(node.row_count(), None);
        assert_eq!(node.numeric_value(), None);
    }

    #[test]
    fn the_disabled_prop_disables_the_node() {
        assert!(written(json!({ "disabled": true })).is_disabled());
    }

    #[test]
    fn an_inert_element_hides_its_subtree_from_the_tree() {
        assert!(written(json!({ "inert": true })).is_hidden());
        assert!(!written(json!({ "inert": false })).is_hidden());
        assert!(has_node_props(&props(json!({ "inert": true }))));
    }

    #[test]
    fn id_references_split_on_whitespace() {
        let found = relations(&props(json!({
            "aria-labelledby": " a  b\tc ",
            "aria-errormessage": "err",
            "aria-controls": "",
        })));
        let names: Vec<(A11yRelation, Vec<&str>)> = found
            .iter()
            .map(|(relation, ids)| (*relation, ids.iter().map(|id| id.as_ref()).collect()))
            .collect();
        assert_eq!(
            names,
            vec![
                (A11yRelation::LabelledBy, vec!["a", "b", "c"]),
                (A11yRelation::ErrorMessage, vec!["err"]),
            ]
        );
    }
}
