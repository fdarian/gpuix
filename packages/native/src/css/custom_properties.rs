//! CSS custom properties: the `--name` keys of a style object.
//!
//! A declaration is kept as the text CSS computes it to for an unregistered property: comments
//! removed, the ends trimmed, a trailing `!important` dropped. The text inside is not touched, so
//! two values are equal when their text is, which is how `style(--x: v)` compares them in a browser.
//!
//! They inherit. [`CustomProperties::descend`] lays an element's declarations over its parent's
//! computed map. Two things read them: `style()` container queries, and [`substitute`], which puts
//! them into the text of the transform properties. No other property substitutes `var()`.

use std::borrow::Cow;
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

/// Which retained elements read the custom properties they inherit (see `retain::Record::reads_custom`):
/// a build notes a read, and an element that is built read if the count moved while it was.
pub(crate) mod reads {
    use std::cell::Cell;

    thread_local! {
        static READS: Cell<u64> = const { Cell::new(0) };
    }

    pub(crate) fn note() {
        READS.set(READS.get() + 1);
    }

    pub(crate) fn count() -> u64 {
        READS.get()
    }
}

/// Whether `text` has a `var()` in it.
pub(crate) fn has_var(text: &str) -> bool {
    find_var(text).is_some()
}

/// The byte index of the next `var(`, as a function and not the end of another name.
fn find_var(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let is_ident = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-' || !byte.is_ascii();
    (0..bytes.len().saturating_sub(3)).find(|&at| {
        bytes[at..at + 4].eq_ignore_ascii_case(b"var(") && (at == 0 || !is_ident(bytes[at - 1]))
    })
}

/// `text` with each `var(--name)` and `var(--name, fallback)` replaced by the value `custom`
/// computes for the name, or by the fallback (which may hold `var()` itself) when it computes to
/// none. `None` when one cannot be resolved: a name that is missing without a fallback, a custom
/// property that refers to itself, or a malformed call. CSS makes the declaration invalid at
/// computed-value time then.
///
/// A custom property whose own value holds `var()` is expanded against `custom` as well, which
/// is the map of the element reading it: a descendant that overrides a name the ancestor's value
/// refers to changes what the ancestor's value means here, where CSS computes it at the ancestor.
pub(crate) fn substitute<'a>(text: &'a str, custom: &CustomProperties) -> Option<Cow<'a, str>> {
    if !has_var(text) {
        return Some(Cow::Borrowed(text));
    }
    expand(text, custom, &mut Vec::new()).map(Cow::Owned)
}

fn expand(text: &str, custom: &CustomProperties, active: &mut Vec<String>) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = find_var(rest) {
        out.push_str(&rest[..start]);
        let after = &rest[start + 4..];
        let close = closing_paren(after)?;
        let arguments = &after[..close];
        let (name, fallback) = match arguments.find(',') {
            Some(comma) => (arguments[..comma].trim(), Some(arguments[comma + 1..].trim())),
            None => (arguments.trim(), None),
        };
        if !is_name(name) {
            return None;
        }
        let value = custom.get(name).filter(|_| !active.iter().any(|open| open == name)).and_then(|value| {
            active.push(name.to_owned());
            let expanded = expand(value, custom, active);
            active.pop();
            expanded
        });
        let replacement = match (value, fallback) {
            (Some(value), _) => value,
            (None, Some(fallback)) => expand(fallback, custom, active)?,
            (None, None) => return None,
        };
        // Spaces keep what the call stood for apart from the tokens around it, as tokens are.
        out.push(' ');
        out.push_str(&replacement);
        out.push(' ');
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// The index of the `)` that closes a call whose `(` was just before `text`.
fn closing_paren(text: &str) -> Option<usize> {
    let mut depth = 1usize;
    for (index, character) in text.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

impl CustomProperties {
    pub(crate) fn fingerprint(&self, fp: &mut crate::retain::Fingerprint) {
        for (name, value) in self.0.iter() {
            fp.str(name);
            fp.str(value);
        }
        fp.word(0);
    }

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

    #[test]
    fn var_is_replaced_by_the_computed_value_or_its_fallback() {
        let custom = declared(&[("--p", "0.4"), ("--len", "calc(1px + 2px)"), ("--twice", "calc(var(--p) * 2)")]);
        let sub = |text: &str| substitute(text, &custom).map(|text| text.split_whitespace().collect::<Vec<_>>().join(" "));
        assert_eq!(sub("translateX(calc(100% * var(--p)))").as_deref(), Some("translateX(calc(100% * 0.4 ))"));
        assert_eq!(sub("none").as_deref(), Some("none"));
        assert_eq!(sub("translate(var(--len), 0)").as_deref(), Some("translate( calc(1px + 2px) , 0)"));
        assert_eq!(sub("a var(--missing, 7px)").as_deref(), Some("a 7px"));
        assert_eq!(sub("a var(--missing,)").as_deref(), Some("a"));
        assert_eq!(sub("a var(--p, 7px)").as_deref(), Some("a 0.4"));
        assert_eq!(sub("a var(--missing, var(--p))").as_deref(), Some("a 0.4"));
        assert_eq!(sub("a var(--missing, var(--nope, 3px))").as_deref(), Some("a 3px"));
        assert_eq!(sub("a VAR( --p )").as_deref(), Some("a 0.4"));
        assert_eq!(sub("calc(var(--twice) + 1)").as_deref(), Some("calc( calc( 0.4 * 2) + 1)"));
        assert_eq!(sub("a var(--p) b var(--len)").as_deref(), Some("a 0.4 b calc(1px + 2px)"));
    }

    #[test]
    fn an_unresolvable_var_makes_the_whole_text_invalid() {
        let custom = declared(&[("--a", "var(--b)"), ("--b", "var(--a)"), ("--self", "var(--self)"), ("--ok", "1")]);
        for text in [
            "var(--missing)",
            "1 var(--ok) var(--missing)",
            "var(--a)",
            "var(--self)",
            "var(--ok",
            "var(ok)",
            "var()",
            "var(--missing, var(--also-missing))",
        ] {
            assert_eq!(substitute(text, &custom), None, "{text}");
        }
        assert_eq!(substitute("var(--self, 5px)", &custom).as_deref().map(str::trim), Some("5px"), "a cycle falls back");
    }

    #[test]
    fn only_a_var_function_is_found() {
        assert!(has_var("calc(var(--a))"));
        assert!(!has_var("myvar(--a)"));
        assert!(!has_var("translateX(10px)"));
        assert!(!has_var("var"));
    }
}
