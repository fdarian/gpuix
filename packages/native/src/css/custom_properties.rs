//! CSS custom properties: the `--name` keys of a style object.
//!
//! A declaration is kept as the text CSS computes it to for an unregistered property: comments
//! removed, the ends trimmed, a trailing `!important` dropped. The text inside is not touched, so
//! two values are equal when their text is, which is how `style(--x: v)` compares them in a browser.
//!
//! They inherit. [`CustomProperties::descend`] lays an element's declarations over its parent's
//! computed map. Nothing reads them except `style()` container queries: `var()` is not substituted
//! into other properties.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CustomProperties(Arc<BTreeMap<String, String>>);

/// Whether `name` is a custom property name: `--` and at least one more character, and a CSS
/// identifier after it.
pub(crate) fn is_name(name: &str) -> bool {
    name.strip_prefix("--").is_some_and(|rest| {
        !rest.is_empty() && rest.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || !c.is_ascii())
    })
}

/// The text of a declared value as CSS computes it: comments removed, trimmed, without a trailing
/// `!important`.
pub(crate) fn normalise(text: &str) -> String {
    let mut stripped = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("/*") {
        stripped.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => {
                rest = "";
                break;
            }
        }
    }
    stripped.push_str(rest);
    let trimmed = stripped.trim();
    let without_important = trimmed
        .rsplit_once('!')
        .filter(|(_, keyword)| keyword.trim().eq_ignore_ascii_case("important"))
        .map_or(trimmed, |(value, _)| value);
    without_important.trim().to_owned()
}

/// `initial` makes the property guaranteed-invalid, which is the same as not declaring it.
pub(crate) fn is_initial(value: &str) -> bool {
    value.eq_ignore_ascii_case("initial")
}

fn is_inherit(value: &str) -> bool {
    ["inherit", "unset"].iter().any(|keyword| value.eq_ignore_ascii_case(keyword))
}

impl CustomProperties {
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }

    /// The declarations of one style object, in written order, a later one winning. Values are
    /// normalised. An empty value declares nothing, as React DOM removes the property for it. `None`
    /// when there are none.
    pub(crate) fn declared(entries: impl IntoIterator<Item = (String, String)>) -> Option<Self> {
        let map: BTreeMap<String, String> = entries
            .into_iter()
            .map(|(name, value)| (name, normalise(&value)))
            .filter(|(_, value)| !value.is_empty())
            .collect();
        (!map.is_empty()).then(|| Self(Arc::new(map)))
    }

    /// What an element computes to: `declared` laid over what it inherits (`self`). `inherit`
    /// and `unset` keep the inherited value, `initial` removes it.
    pub(crate) fn descend(&self, declared: &Self) -> Self {
        if declared.is_empty() {
            return self.clone();
        }
        let mut computed = (*self.0).clone();
        for (name, value) in declared.0.iter() {
            if is_initial(value) {
                computed.remove(name);
            } else if !is_inherit(value) {
                computed.insert(name.clone(), value.clone());
            }
        }
        Self(Arc::new(computed))
    }

    /// `over`'s declarations on top of `base`'s, keywords kept as written: a state style or an
    /// `@container` block over the style it applies to.
    pub(crate) fn layered(base: Option<&Self>, over: Option<&Self>) -> Option<Self> {
        match (base, over) {
            (Some(base), Some(over)) => {
                let mut map = (*base.0).clone();
                map.extend(over.0.iter().map(|(name, value)| (name.clone(), value.clone())));
                Some(Self(Arc::new(map)))
            }
            (Some(only), None) | (None, Some(only)) => Some(only.clone()),
            (None, None) => None,
        }
    }
}

impl Serialize for CustomProperties {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (name, value) in self.0.iter() {
            map.serialize_entry(name, value)?;
        }
        map.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared(entries: &[(&str, &str)]) -> CustomProperties {
        CustomProperties::declared(entries.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())))
            .unwrap_or_default()
    }

    #[test]
    fn names_are_two_dashes_and_an_identifier() {
        assert!(is_name("--x"));
        assert!(is_name("--brand-color_2"));
        assert!(is_name("---"));
        assert!(!is_name("--"));
        assert!(!is_name("-x"));
        assert!(!is_name("x"));
        assert!(!is_name("--a b"));
        assert!(!is_name("--a:b"));
    }

    #[test]
    fn a_value_is_trimmed_without_comments_or_important() {
        assert_eq!(normalise("  dark  "), "dark");
        assert_eq!(normalise("1 /* c */"), "1");
        assert_eq!(normalise("a /* b */ c"), "a  c");
        assert_eq!(normalise("red !important"), "red");
        assert_eq!(normalise("red!IMPORTANT"), "red");
        assert_eq!(normalise("a   b"), "a   b", "the text inside is kept");
        assert_eq!(normalise("calc(1px+2px)"), "calc(1px+2px)");
        assert_eq!(normalise("1 /* open"), "1");
    }

    #[test]
    fn a_child_computes_its_declarations_over_the_parents_map() {
        let parent = CustomProperties::default().descend(&declared(&[("--a", "1"), ("--b", "2")]));
        assert_eq!((parent.get("--a"), parent.get("--b")), (Some("1"), Some("2")));
        let child = parent.descend(&declared(&[("--b", "3"), ("--c", "4")]));
        assert_eq!((child.get("--a"), child.get("--b"), child.get("--c")), (Some("1"), Some("3"), Some("4")));
        assert_eq!(parent.get("--b"), Some("2"), "the parent is untouched");
    }

    #[test]
    fn the_css_wide_keywords_inherit_or_reset() {
        let parent = declared(&[("--a", "1"), ("--b", "2")]);
        let child = parent.descend(&declared(&[("--a", "inherit"), ("--b", "initial")]));
        assert_eq!((child.get("--a"), child.get("--b")), (Some("1"), None));
        assert_eq!(parent.descend(&declared(&[("--a", "UNSET")])).get("--a"), Some("1"));
    }

    #[test]
    fn a_layer_over_another_replaces_per_property_and_keeps_keywords() {
        let base = declared(&[("--a", "1"), ("--b", "2")]);
        let over = declared(&[("--b", "initial"), ("--c", "3")]);
        let layered = CustomProperties::layered(Some(&base), Some(&over)).unwrap();
        assert_eq!((layered.get("--a"), layered.get("--b"), layered.get("--c")), (Some("1"), Some("initial"), Some("3")));
        assert_eq!(CustomProperties::layered(None, None), None);
        assert_eq!(CustomProperties::layered(Some(&base), None), Some(base));
    }
}
