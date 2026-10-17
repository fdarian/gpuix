//! `font-variant-numeric` and `font-feature-settings`, as OpenType features.

use std::sync::Arc;

use gpui::FontFeatures;

use super::syntax::{split_commas, split_whitespace};

pub(crate) type Features = Vec<(String, u32)>;

/// `font-variant-numeric`. `Some(vec![])` is the valid `normal`; `None` an
/// invalid value, which CSS ignores.
///
/// Each keyword group (figure, spacing, fraction) takes at most one value, and
/// `normal` stands alone.
pub(crate) fn parse_variant_numeric(value: &str) -> Option<Features> {
    let tokens = split_whitespace(value);
    if tokens.is_empty() {
        return None;
    }
    if tokens.len() == 1 && tokens[0].eq_ignore_ascii_case("normal") {
        return Some(Vec::new());
    }
    let mut features = Features::new();
    // One slot per mutually exclusive group: figure, spacing, fraction.
    let mut groups = [false; 3];
    let mut ordinal = false;
    let mut slashed_zero = false;
    for token in tokens {
        let (group, tag) = match token.to_ascii_lowercase().as_str() {
            "lining-nums" => (Some(0), "lnum"),
            "oldstyle-nums" => (Some(0), "onum"),
            "proportional-nums" => (Some(1), "pnum"),
            "tabular-nums" => (Some(1), "tnum"),
            "diagonal-fractions" => (Some(2), "frac"),
            "stacked-fractions" => (Some(2), "afrc"),
            "ordinal" => {
                if std::mem::replace(&mut ordinal, true) {
                    return None;
                }
                (None, "ordn")
            }
            "slashed-zero" => {
                if std::mem::replace(&mut slashed_zero, true) {
                    return None;
                }
                (None, "zero")
            }
            _ => return None,
        };
        if let Some(group) = group {
            if std::mem::replace(&mut groups[group], true) {
                return None;
            }
        }
        features.push((tag.to_owned(), 1));
    }
    Some(features)
}

/// `font-feature-settings`: `normal`, or a comma list of `"tag" [integer | on |
/// off]`. A tag is exactly four ASCII characters in quotes.
pub(crate) fn parse_feature_settings(value: &str) -> Option<Features> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("normal") {
        return Some(Vec::new());
    }
    let mut features = Features::new();
    for item in split_commas(value) {
        let (tag, setting) = parse_tag(item)?;
        let setting = setting.trim();
        let value = if setting.is_empty() || setting.eq_ignore_ascii_case("on") {
            1
        } else if setting.eq_ignore_ascii_case("off") {
            0
        } else {
            setting.parse::<u32>().ok()?
        };
        features.push((tag.to_owned(), value));
    }
    Some(features)
}

/// The quoted tag at the start of `item` and what follows it.
pub(crate) fn parse_tag(item: &str) -> Option<(&str, &str)> {
    let item = item.trim();
    let quote = item.chars().next().filter(|quote| matches!(quote, '"' | '\''))?;
    let rest = &item[1..];
    let end = rest.find(quote)?;
    let tag = &rest[..end];
    (tag.len() == 4 && tag.is_ascii() && tag.bytes().all(|byte| (0x20..=0x7e).contains(&byte)))
        .then_some((tag, &rest[end + 1..]))
}

/// What a subtree inherits for font features. The two CSS properties inherit
/// independently, so a descendant that sets only one keeps the other.
#[derive(Clone, Default)]
pub(crate) struct FontFeatureState {
    numeric: Arc<Features>,
    settings: Arc<Features>,
}

impl FontFeatureState {
    /// The state below an element styled with these two declarations, and the
    /// features to hand to GPUI when this element declares either. An invalid
    /// value is ignored and leaves the inherited one in place.
    pub(crate) fn descend(
        &self,
        variant_numeric: Option<&str>,
        feature_settings: Option<&str>,
    ) -> (Self, Option<FontFeatures>) {
        let mut next = self.clone();
        let mut declared = false;
        if let Some(numeric) = variant_numeric.and_then(parse_variant_numeric) {
            next.numeric = Arc::new(numeric);
            declared = true;
        }
        if let Some(settings) = feature_settings.and_then(parse_feature_settings) {
            next.settings = Arc::new(settings);
            declared = true;
        }
        let features = declared.then(|| next.combined());
        (next, features)
    }

    /// Settings come last so they win on a tag both name, as in CSS.
    fn combined(&self) -> FontFeatures {
        let mut merged = Features::new();
        for (tag, value) in self.numeric.iter().chain(self.settings.iter()) {
            match merged.iter_mut().find(|(existing, _)| existing == tag) {
                Some(entry) => entry.1 = *value,
                None => merged.push((tag.clone(), *value)),
            }
        }
        FontFeatures(Arc::new(merged))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(features: Option<Features>) -> Vec<(String, u32)> {
        features.expect("valid")
    }

    fn pairs(list: &[(&str, u32)]) -> Vec<(String, u32)> {
        list.iter().map(|(tag, value)| (tag.to_string(), *value)).collect()
    }

    #[test]
    fn maps_every_numeric_keyword() {
        for (keyword, tag) in [
            ("tabular-nums", "tnum"),
            ("proportional-nums", "pnum"),
            ("lining-nums", "lnum"),
            ("oldstyle-nums", "onum"),
            ("slashed-zero", "zero"),
            ("diagonal-fractions", "frac"),
            ("stacked-fractions", "afrc"),
            ("ordinal", "ordn"),
        ] {
            assert_eq!(tags(parse_variant_numeric(keyword)), pairs(&[(tag, 1)]), "{keyword}");
        }
        assert_eq!(tags(parse_variant_numeric("normal")), Vec::new());
    }

    #[test]
    fn combines_keywords_from_different_groups() {
        assert_eq!(
            tags(parse_variant_numeric("Tabular-Nums  slashed-zero lining-nums")),
            pairs(&[("tnum", 1), ("zero", 1), ("lnum", 1)])
        );
    }

    #[test]
    fn rejects_conflicting_or_unknown_numeric_values() {
        for input in [
            "",
            "tabular-nums proportional-nums",
            "lining-nums oldstyle-nums",
            "diagonal-fractions stacked-fractions",
            "ordinal ordinal",
            "normal tabular-nums",
            "tabular",
        ] {
            assert_eq!(parse_variant_numeric(input), None, "{input}");
        }
    }

    #[test]
    fn parses_feature_settings() {
        assert_eq!(
            tags(parse_feature_settings(r#""tnum" 1, 'liga' 0, "ss01", "calt" off, "zero" on, "cv01" 3"#)),
            pairs(&[("tnum", 1), ("liga", 0), ("ss01", 1), ("calt", 0), ("zero", 1), ("cv01", 3)])
        );
        assert_eq!(tags(parse_feature_settings("normal")), Vec::new());
    }

    #[test]
    fn rejects_malformed_feature_settings() {
        for input in [
            "",
            "tnum",
            r#""tnu" 1"#,
            r#""tnum5" 1"#,
            r#""tnum" -1"#,
            r#""tnum" maybe"#,
            r#""tnum" 1,"#,
            r#""tnum 1"#,
        ] {
            assert_eq!(parse_feature_settings(input), None, "{input}");
        }
    }

    #[test]
    fn settings_win_over_numeric_keywords_on_a_shared_tag() {
        let (state, features) =
            FontFeatureState::default().descend(Some("tabular-nums slashed-zero"), Some(r#""tnum" 0, "liga" 0"#));
        let features = features.expect("declared");
        assert_eq!(*features.0, pairs(&[("tnum", 0), ("zero", 1), ("liga", 0)]));
        assert_eq!(state.numeric.len(), 2);
    }

    #[test]
    fn a_descendant_keeps_the_property_it_does_not_set() {
        let (parent, _) = FontFeatureState::default().descend(None, Some(r#""liga" 0"#));
        let (_, features) = parent.descend(Some("tabular-nums"), None);
        assert_eq!(*features.expect("declared").0, pairs(&[("tnum", 1), ("liga", 0)]));
    }

    #[test]
    fn normal_resets_and_invalid_values_are_ignored() {
        let (parent, _) = FontFeatureState::default().descend(Some("tabular-nums"), None);
        let (_, reset) = parent.descend(Some("normal"), None);
        assert!(reset.expect("normal is a declaration").0.is_empty());
        let (_, ignored) = parent.descend(Some("tabular-nums proportional-nums"), None);
        assert!(ignored.is_none());
        let (_, undeclared) = parent.descend(None, None);
        assert!(undeclared.is_none());
    }
}
