//! Bounded caches for the CSS value parsers, keyed by the text.
//!
//! Every element of a frame is built from its style again, and elements of one
//! design share their strings, so most parses repeat one from the frame before.

use std::cell::RefCell;

use rustc_hash::FxHashMap;

use crate::color::ColorContext;

/// Past this many entries a cache is emptied, so a value that differs on every
/// frame (a motion interpolating a gradient) cannot grow it without bound.
const MAX_ENTRIES: usize = 1024;
/// Longer texts are parsed without being kept.
const MAX_TEXT_LEN: usize = 4096;
/// Results kept per text that depends on the colour context.
const MAX_CONTEXTS: usize = 8;

/// For a parser whose result depends on the text alone. A parser that starts
/// resolving anything from the element (a colour, an `em` or `vw` length) must
/// move to a cache that keys on it, or this one returns the first element's value.
pub(crate) struct Cache<V> {
    entries: RefCell<FxHashMap<Box<str>, V>>,
}

impl<V: Clone> Cache<V> {
    pub(crate) fn new() -> Self {
        Self { entries: RefCell::new(FxHashMap::default()) }
    }

    pub(crate) fn get_or_parse(&self, text: &str, parse: impl FnOnce() -> V) -> V {
        if let Some(value) = self.entries.borrow().get(text) {
            return value.clone();
        }
        let value = parse();
        if text.len() <= MAX_TEXT_LEN {
            let mut entries = self.entries.borrow_mut();
            if entries.len() >= MAX_ENTRIES {
                entries.clear();
            }
            entries.insert(text.into(), value.clone());
        }
        value
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.borrow().len()
    }
}

enum Entry<V> {
    /// The text names no colour function that reads the context.
    Free(V),
    PerContext(Vec<(ColorContext, V)>),
}

/// For a parser that resolves colours: a text that reads `currentColor`,
/// `light-dark()` or `color-mix()` gets one result per colour context, and any
/// other text one result for all of them.
pub(crate) struct ColorCache<V> {
    entries: RefCell<FxHashMap<Box<str>, Entry<V>>>,
}

impl<V: Clone> ColorCache<V> {
    pub(crate) fn new() -> Self {
        Self { entries: RefCell::new(FxHashMap::default()) }
    }

    /// `parse` runs on a miss, and its result must depend on `text` and on
    /// `context` only through what `ColorContext::resolves_like` compares.
    pub(crate) fn get_or_parse(
        &self,
        text: &str,
        context: &ColorContext,
        parse: impl FnOnce() -> V,
    ) -> V {
        match self.entries.borrow().get(text) {
            Some(Entry::Free(value)) => return value.clone(),
            Some(Entry::PerContext(results)) => {
                if let Some((_, value)) = results.iter().find(|(known, _)| known.resolves_like(context)) {
                    return value.clone();
                }
            }
            None => {}
        }
        let value = parse();
        if text.len() > MAX_TEXT_LEN {
            return value;
        }
        let mut entries = self.entries.borrow_mut();
        if entries.len() >= MAX_ENTRIES {
            entries.clear();
        }
        match entries.get_mut(text) {
            Some(Entry::PerContext(results)) => {
                if results.len() >= MAX_CONTEXTS {
                    results.clear();
                }
                results.push((*context, value.clone()));
            }
            Some(Entry::Free(_)) => unreachable!("a free entry is returned on lookup"),
            None => {
                let entry = if reads_context(text) {
                    Entry::PerContext(vec![(*context, value.clone())])
                } else {
                    Entry::Free(value.clone())
                };
                entries.insert(text.into(), entry);
            }
        }
        value
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.borrow().len()
    }
}

/// Whether a colour in `text` can resolve differently per element. Conservative:
/// a match inside another word only costs a cache entry per context.
fn reads_context(text: &str) -> bool {
    ["currentcolor", "light-dark(", "color-mix("].iter().any(|name| {
        text.as_bytes()
            .windows(name.len())
            .any(|window| window.eq_ignore_ascii_case(name.as_bytes()))
    })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::color::Scheme;

    fn context(scheme: Scheme, current: f32) -> ColorContext {
        let mut context = ColorContext::root(scheme);
        context.scheme = scheme;
        context.current = gpui::Rgba { r: current, g: current, b: current, a: 1.0 };
        context
    }

    #[test]
    fn a_text_parses_once_for_every_context_when_it_reads_none() {
        let cache = ColorCache::new();
        let parses = Cell::new(0);
        let parse = || {
            parses.set(parses.get() + 1);
            parses.get()
        };
        let text = "linear-gradient(red, blue)";
        let first = cache.get_or_parse(text, &context(Scheme::Light, 0.0), parse);
        let second = cache.get_or_parse(text, &context(Scheme::Dark, 1.0), parse);
        assert_eq!((first, second, parses.get()), (1, 1, 1));
    }

    #[test]
    fn a_text_that_reads_the_context_keeps_a_result_per_context() {
        let cache = ColorCache::new();
        let parses = Cell::new(0);
        let parse = || {
            parses.set(parses.get() + 1);
            parses.get()
        };
        for text in ["linear-gradient(currentColor, blue)", "linear-gradient(light-dark(red, blue), blue)", "linear-gradient(color-mix(in srgb, red, blue), blue)"] {
            parses.set(0);
            let black_light = cache.get_or_parse(text, &context(Scheme::Light, 0.0), parse);
            let white_light = cache.get_or_parse(text, &context(Scheme::Light, 1.0), parse);
            let black_dark = cache.get_or_parse(text, &context(Scheme::Dark, 0.0), parse);
            assert_eq!([black_light, white_light, black_dark], [1, 2, 3], "{text}");
            let again = cache.get_or_parse(text, &context(Scheme::Light, 1.0), parse);
            assert_eq!((again, parses.get()), (2, 3), "{text}");
        }
    }

    #[test]
    fn the_context_fields_a_colour_never_reads_do_not_split_entries() {
        let cache = ColorCache::new();
        let text = "linear-gradient(currentColor, blue)";
        let mut other = context(Scheme::Light, 0.5);
        other.declared = Some(gpui::Rgba { r: 1.0, g: 0.0, b: 0.0, a: 1.0 });
        other.appearance = Scheme::Dark;
        let first = cache.get_or_parse(text, &context(Scheme::Light, 0.5), || 1);
        let second = cache.get_or_parse(text, &other, || 2);
        assert_eq!((first, second), (1, 1));
    }

    #[test]
    fn the_caches_stay_bounded_when_every_text_is_new() {
        let free = Cache::new();
        let colors = ColorCache::new();
        let context = context(Scheme::Light, 0.0);
        for index in 0..MAX_ENTRIES * 3 {
            free.get_or_parse(&format!("blur({index}px)"), || index);
            colors.get_or_parse(&format!("linear-gradient(red, blue {index}px)"), &context, || index);
            assert!(free.len() <= MAX_ENTRIES && colors.len() <= MAX_ENTRIES);
        }
        assert!(free.len() > 0 && colors.len() > 0);
    }

    #[test]
    fn the_contexts_kept_per_text_are_bounded() {
        let cache = ColorCache::new();
        let text = "linear-gradient(currentColor, blue)";
        for index in 0..MAX_CONTEXTS * 3 {
            cache.get_or_parse(text, &context(Scheme::Light, index as f32 / 100.0), || index);
        }
        let entries = cache.entries.borrow();
        let Some(Entry::PerContext(results)) = entries.get(text) else {
            panic!("a text that reads currentColor has per-context results");
        };
        assert!(results.len() <= MAX_CONTEXTS);
    }

    #[test]
    fn a_long_text_is_parsed_but_not_kept() {
        let cache = Cache::new();
        let text = "a".repeat(MAX_TEXT_LEN + 1);
        assert_eq!(cache.get_or_parse(&text, || 7), 7);
        assert_eq!(cache.len(), 0);
    }
}
