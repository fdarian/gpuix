//! CSS `word-break` and `overflow-wrap` (CSS Text 3 §5): where a line may break
//! inside a word, and whether a word that does not fit is broken.

use serde::{Deserialize, Serialize};

/// `word-break`. `break-word` is the deprecated value that means
/// `word-break: normal` with `overflow-wrap: anywhere`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", try_from = "String")]
pub enum WordBreak {
    Normal,
    BreakAll,
    KeepAll,
    BreakWord,
}

impl TryFrom<String> for WordBreak {
    type Error = String;

    fn try_from(value: String) -> Result<Self, String> {
        match value.as_str() {
            "normal" => Ok(Self::Normal),
            "break-all" => Ok(Self::BreakAll),
            "keep-all" => Ok(Self::KeepAll),
            "break-word" => Ok(Self::BreakWord),
            _ => Err(format!("invalid wordBreak: {value:?}")),
        }
    }
}

/// `overflow-wrap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", try_from = "String")]
pub enum OverflowWrap {
    Normal,
    BreakWord,
    Anywhere,
}

impl TryFrom<String> for OverflowWrap {
    type Error = String;

    fn try_from(value: String) -> Result<Self, String> {
        match value.as_str() {
            "normal" => Ok(Self::Normal),
            "break-word" => Ok(Self::BreakWord),
            "anywhere" => Ok(Self::Anywhere),
            _ => Err(format!("invalid overflowWrap: {value:?}")),
        }
    }
}

impl OverflowWrap {
    fn to_gpui(self) -> gpui::OverflowWrap {
        match self {
            Self::Normal => gpui::OverflowWrap::Normal,
            Self::BreakWord => gpui::OverflowWrap::BreakWord,
            Self::Anywhere => gpui::OverflowWrap::Anywhere,
        }
    }
}

/// What the root gives all text, which is CSS's initial value for both and not
/// gpui's default `overflow-wrap`: a word longer than its line overflows it.
pub fn set_defaults(style: &mut gpui::TextStyleRefinement) {
    style.word_break = Some(gpui::WordBreak::Normal);
    style.overflow_wrap = Some(OverflowWrap::Normal.to_gpui());
}

/// Sets what an element declares. `word-break: break-word` wins over
/// `overflow-wrap`, as in CSS.
pub fn apply(
    style: &mut gpui::TextStyleRefinement,
    word_break: Option<WordBreak>,
    overflow_wrap: Option<OverflowWrap>,
) {
    if let Some(overflow_wrap) = overflow_wrap {
        style.overflow_wrap = Some(overflow_wrap.to_gpui());
    }
    match word_break {
        Some(WordBreak::Normal) => style.word_break = Some(gpui::WordBreak::Normal),
        Some(WordBreak::BreakAll) => style.word_break = Some(gpui::WordBreak::BreakAll),
        Some(WordBreak::KeepAll) => style.word_break = Some(gpui::WordBreak::KeepAll),
        Some(WordBreak::BreakWord) => {
            style.word_break = Some(gpui::WordBreak::Normal);
            style.overflow_wrap = Some(gpui::OverflowWrap::Anywhere);
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word_break(json: &str) -> Result<WordBreak, serde_json::Error> {
        serde_json::from_str(json)
    }

    fn overflow_wrap(json: &str) -> Result<OverflowWrap, serde_json::Error> {
        serde_json::from_str(json)
    }

    fn applied(
        word_break: Option<WordBreak>,
        overflow_wrap: Option<OverflowWrap>,
    ) -> (Option<gpui::WordBreak>, Option<gpui::OverflowWrap>) {
        let mut style = gpui::TextStyleRefinement::default();
        apply(&mut style, word_break, overflow_wrap);
        (style.word_break, style.overflow_wrap)
    }

    #[test]
    fn parses_the_values_of_both_properties() {
        for (text, value) in [
            ("normal", WordBreak::Normal),
            ("break-all", WordBreak::BreakAll),
            ("keep-all", WordBreak::KeepAll),
            ("break-word", WordBreak::BreakWord),
        ] {
            assert_eq!(word_break(&format!("\"{text}\"")).unwrap(), value);
        }
        for (text, value) in [
            ("normal", OverflowWrap::Normal),
            ("break-word", OverflowWrap::BreakWord),
            ("anywhere", OverflowWrap::Anywhere),
        ] {
            assert_eq!(overflow_wrap(&format!("\"{text}\"")).unwrap(), value);
        }
    }

    #[test]
    fn what_css_rejects_is_an_error() {
        for json in ["\"anywhere\"", "\"Break-All\"", "\"\"", "1", "true"] {
            assert!(word_break(json).is_err(), "{json} is a word-break");
        }
        for json in ["\"break-all\"", "\"keep-all\"", "\"\"", "1", "null"] {
            assert!(overflow_wrap(json).is_err(), "{json} is an overflow-wrap");
        }
    }

    #[test]
    fn a_style_reads_them_and_an_invalid_one_is_not_interned() {
        let style: crate::style::StyleDesc =
            serde_json::from_str(r#"{"wordBreak": "keep-all", "overflowWrap": "anywhere"}"#)
                .unwrap();
        assert_eq!(style.word_break, Some(WordBreak::KeepAll));
        assert_eq!(style.overflow_wrap, Some(OverflowWrap::Anywhere));
        let mut table = crate::retained_tree::StyleTable::default();
        assert!(table.intern(br#"{"wordBreak": "break-all"}"#).is_ok());
        let error = table.intern(br#"{"wordBreak": "wide"}"#).unwrap_err();
        assert!(error.contains("invalid wordBreak"), "{error}");
        let error = table
            .intern(br#"{"overflowWrap": "break-all"}"#)
            .unwrap_err();
        assert!(error.contains("invalid overflowWrap"), "{error}");
    }

    #[test]
    fn each_value_reaches_gpui() {
        assert_eq!(
            applied(Some(WordBreak::BreakAll), Some(OverflowWrap::BreakWord)),
            (
                Some(gpui::WordBreak::BreakAll),
                Some(gpui::OverflowWrap::BreakWord)
            )
        );
        assert_eq!(
            applied(Some(WordBreak::KeepAll), None),
            (Some(gpui::WordBreak::KeepAll), None)
        );
        assert_eq!(
            applied(None, Some(OverflowWrap::Anywhere)),
            (None, Some(gpui::OverflowWrap::Anywhere))
        );
        assert_eq!(applied(None, None), (None, None));
    }

    #[test]
    fn word_break_break_word_is_overflow_wrap_anywhere() {
        let expected = (
            Some(gpui::WordBreak::Normal),
            Some(gpui::OverflowWrap::Anywhere),
        );
        assert_eq!(applied(Some(WordBreak::BreakWord), None), expected);
        // Whatever overflow-wrap says on the same element.
        assert_eq!(
            applied(Some(WordBreak::BreakWord), Some(OverflowWrap::Normal)),
            expected
        );
    }

    #[test]
    fn an_element_refines_what_the_root_gave_and_nothing_else() {
        use gpui::Refineable as _;
        let options = |word_break, overflow_wrap| {
            let mut root = gpui::TextStyleRefinement::default();
            set_defaults(&mut root);
            let mut text = gpui::TextStyle::default();
            text.refine(&root);
            let mut element = gpui::TextStyleRefinement::default();
            apply(&mut element, word_break, overflow_wrap);
            text.refine(&element);
            text.layout_options()
        };
        let initial = options(None, None);
        assert_eq!(initial.word_break, gpui::WordBreak::Normal);
        assert_eq!(initial.overflow_wrap, gpui::OverflowWrap::Normal);
        let broken = options(Some(WordBreak::BreakAll), None);
        assert_eq!(broken.word_break, gpui::WordBreak::BreakAll);
        assert_eq!(broken.overflow_wrap, gpui::OverflowWrap::Normal);
        let wrapped = options(None, Some(OverflowWrap::BreakWord));
        assert_eq!(wrapped.word_break, gpui::WordBreak::Normal);
        assert_eq!(wrapped.overflow_wrap, gpui::OverflowWrap::BreakWord);
    }

    #[test]
    fn the_root_gives_text_the_css_initial_values() {
        let mut style = gpui::TextStyleRefinement::default();
        set_defaults(&mut style);
        assert_eq!(style.word_break, Some(gpui::WordBreak::Normal));
        assert_eq!(style.overflow_wrap, Some(gpui::OverflowWrap::Normal));
        // Not what gpui does for text that sets nothing.
        assert_ne!(
            gpui::TextStyle::default().overflow_wrap,
            gpui::OverflowWrap::Normal
        );
    }
}
