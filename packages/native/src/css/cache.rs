//! A bounded cache for the CSS value parsers, keyed by the text.
//!
//! Every element of a frame is built from its style again, and elements of one
//! design share their strings, so most parses repeat one from the frame before.
//! What a parser resolves from the element (a colour, an `em` or `vw` length)
//! is part of the key, but only when the text reads it: a text with neither
//! parses once for every element, and one with `1em` once per font size.

use std::cell::RefCell;

use rustc_hash::FxHashMap;

use super::length::{LengthContext, LengthReads};
use crate::color::ColorContext;

/// Past this many entries a cache is emptied, so a value that differs on every
/// frame (a motion interpolating a gradient) cannot grow it without bound.
const MAX_ENTRIES: usize = 1024;
/// Longer texts are parsed without being kept.
const MAX_TEXT_LEN: usize = 4096;
/// Results kept per text that depends on the context.
const MAX_CONTEXTS: usize = 8;

/// What a parse may read besides its text. A parser that starts resolving
/// anything else from the element must add it here, or the cache returns the
/// first element's value.
#[derive(Clone, Copy)]
pub(crate) struct Inputs<'a> {
    /// `None` for a parser that resolves no colour.
    pub(crate) colors: Option<&'a ColorContext>,
    pub(crate) lengths: &'a LengthContext,
}

/// The inputs a text depends on.
#[derive(Clone, Copy, Default)]
struct Reads {
    colors: bool,
    /// The text names `currentColor`: a hit is a read of the colour the element has, as a parse is.
    current: bool,
    lengths: LengthReads,
}

impl Reads {
    fn of(text: &str, inputs: &Inputs) -> Self {
        Self {
            colors: inputs.colors.is_some() && reads_colors(text),
            current: inputs.colors.is_some() && names(text, "currentcolor"),
            lengths: LengthReads::of(text),
        }
    }

    fn any(&self) -> bool {
        self.colors || self.lengths.any()
    }
}

#[derive(Clone, Copy)]
struct Key {
    colors: Option<ColorContext>,
    lengths: LengthContext,
}

impl Key {
    fn of(inputs: &Inputs) -> Self {
        Self { colors: inputs.colors.copied(), lengths: *inputs.lengths }
    }

    /// `parse` must depend on the inputs only through what `ColorContext::resolves_like`
    /// and `LengthContext::resolves_like` compare, for the inputs `reads` names.
    fn agrees(&self, inputs: &Inputs, reads: Reads) -> bool {
        let colors = match (&self.colors, inputs.colors) {
            (Some(known), Some(other)) if reads.colors => known.resolves_like(other),
            _ => true,
        };
        colors && self.lengths.resolves_like(inputs.lengths, reads.lengths)
    }
}

enum Entry<V> {
    /// The text reads nothing from the element.
    Free(V),
    PerContext(Reads, Vec<(Key, V)>),
}

pub(crate) struct Cache<V> {
    entries: RefCell<FxHashMap<Box<str>, Entry<V>>>,
}

impl<V: Clone> Cache<V> {
    pub(crate) fn new() -> Self {
        Self { entries: RefCell::new(FxHashMap::default()) }
    }

    /// `parse` runs on a miss.
    pub(crate) fn get_or_parse(&self, text: &str, inputs: Inputs, parse: impl FnOnce() -> V) -> V {
        match self.entries.borrow().get(text) {
            Some(Entry::Free(value)) => return value.clone(),
            Some(Entry::PerContext(reads, results)) => {
                if let Some((_, value)) = results.iter().find(|(key, _)| key.agrees(&inputs, *reads)) {
                    if let Some(colors) = inputs.colors.filter(|_| reads.current) {
                        colors.note_read();
                    }
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
            Some(Entry::PerContext(_, results)) => {
                if results.len() >= MAX_CONTEXTS {
                    results.clear();
                }
                results.push((Key::of(&inputs), value.clone()));
            }
            Some(Entry::Free(_)) => unreachable!("a free entry is returned on lookup"),
            None => {
                let reads = Reads::of(text, &inputs);
                let entry = if reads.any() {
                    Entry::PerContext(reads, vec![(Key::of(&inputs), value.clone())])
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
fn reads_colors(text: &str) -> bool {
    ["currentcolor", "light-dark(", "color-mix("].iter().any(|name| names(text, name))
}

fn names(text: &str, name: &str) -> bool {
    text.as_bytes()
        .windows(name.len())
        .any(|window| window.eq_ignore_ascii_case(name.as_bytes()))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::color::Scheme;

    fn colors(scheme: Scheme, current: f32) -> ColorContext {
        let mut context = ColorContext::root(scheme);
        context.scheme = scheme;
        context = context.with_current(gpui::Rgba { r: current, g: current, b: current, a: 1.0 });
        context
    }

    fn lengths() -> LengthContext {
        LengthContext::for_tests()
    }

    fn with_colors<'a>(colors: &'a ColorContext, lengths: &'a LengthContext) -> Inputs<'a> {
        Inputs { colors: Some(colors), lengths }
    }

    fn counting() -> (Cell<i32>, impl Fn(&Cell<i32>) -> i32) {
        (Cell::new(0), |parses: &Cell<i32>| {
            parses.set(parses.get() + 1);
            parses.get()
        })
    }

    #[test]
    fn a_text_parses_once_for_every_context_when_it_reads_none() {
        let cache = Cache::new();
        let (parses, parse) = counting();
        let text = "linear-gradient(red, blue 50%)";
        let lengths = lengths();
        let first = cache.get_or_parse(text, with_colors(&colors(Scheme::Light, 0.0), &lengths), || parse(&parses));
        let second = cache.get_or_parse(
            text,
            with_colors(&colors(Scheme::Dark, 1.0), &lengths.with_viewport(1.0, 2.0).descend(Some(&super::super::length::LinearLength::px(3.0)), false)),
            || parse(&parses),
        );
        assert_eq!((first, second, parses.get()), (1, 1, 1));
    }

    #[test]
    fn a_text_that_reads_a_colour_keeps_a_result_per_colour_context() {
        let cache = Cache::new();
        let lengths = lengths();
        for text in [
            "linear-gradient(currentColor, blue)",
            "linear-gradient(light-dark(red, blue), blue)",
            "linear-gradient(color-mix(in srgb, red, blue), blue)",
        ] {
            let (parses, parse) = counting();
            let mut get = |colors: ColorContext| cache.get_or_parse(text, with_colors(&colors, &lengths), || parse(&parses));
            let black_light = get(colors(Scheme::Light, 0.0));
            let white_light = get(colors(Scheme::Light, 1.0));
            let black_dark = get(colors(Scheme::Dark, 0.0));
            assert_eq!([black_light, white_light, black_dark], [1, 2, 3], "{text}");
            assert_eq!(get(colors(Scheme::Light, 1.0)), 2, "{text}");
        }
    }

    #[test]
    fn the_colour_fields_a_text_never_reads_do_not_split_entries() {
        let cache = Cache::new();
        let lengths = lengths();
        let text = "linear-gradient(currentColor, blue)";
        let mut other = colors(Scheme::Light, 0.5);
        other = other.with_declared(gpui::Rgba { r: 1.0, g: 0.0, b: 0.0, a: 1.0 });
        other.appearance = Scheme::Dark;
        let first = cache.get_or_parse(text, with_colors(&colors(Scheme::Light, 0.5), &lengths), || 1);
        let second = cache.get_or_parse(text, with_colors(&other, &lengths), || 2);
        assert_eq!((first, second), (1, 1));
    }

    #[test]
    fn a_text_that_reads_a_length_keeps_a_result_per_value_it_reads() {
        let cache = Cache::new();
        let base = lengths();
        let px = |size: f64| super::super::length::LinearLength::px(size);
        let big_font = base.descend(Some(&px(40.0)), false);
        let other_root = base.descend(Some(&px(30.0)), true);
        let other_window = base.with_viewport(1000.0, 700.0);
        // (text, a context that differs in what the text reads, one that differs in nothing it reads)
        let cases: [(&str, LengthContext, LengthContext); 5] = [
            ("blur(1em)", big_font, other_window),
            ("blur(calc(2px + 0.5EM))", big_font, other_root.descend(Some(&px(20.0)), false)),
            ("radial-gradient(red 1rem, blue)", other_root, big_font),
            ("linear-gradient(red 10vw, blue)", other_window, big_font),
            ("polygon(0 0, 5vmax 0, 0 5vmin)", other_window, big_font),
        ];
        for (text, differs, same) in cases {
            let (parses, parse) = counting();
            let mut get = |lengths: &LengthContext| cache.get_or_parse(text, Inputs { colors: None, lengths }, || parse(&parses));
            let first = get(&base);
            assert_ne!(get(&differs), first, "{text} shared a result across a changed unit");
            assert_eq!(get(&same), first, "{text} was split by a unit it does not read");
            assert_eq!(get(&base), first, "{text}");
            assert_eq!(parses.get(), 2, "{text}");
        }
    }

    #[test]
    fn a_unit_is_a_number_followed_by_its_name() {
        let cache = Cache::new();
        let base = lengths();
        let big_font = base.descend(Some(&super::super::length::LinearLength::px(40.0)), false);
        let (parses, parse) = counting();
        let mut get = |lengths: &LengthContext| {
            cache.get_or_parse("linear-gradient(item, system-ui, #123)", Inputs { colors: None, lengths }, || parse(&parses))
        };
        assert_eq!((get(&base), get(&big_font), get(&base.with_viewport(1.0, 1.0))), (1, 1, 1));
    }

    #[test]
    fn the_caches_stay_bounded_when_every_text_is_new() {
        let cache = Cache::new();
        let colors = colors(Scheme::Light, 0.0);
        let lengths = lengths();
        for index in 0..MAX_ENTRIES * 3 {
            cache.get_or_parse(&format!("linear-gradient(red, blue {index}px)"), with_colors(&colors, &lengths), || index);
            cache.get_or_parse(&format!("blur({index}em)"), Inputs { colors: None, lengths: &lengths }, || index);
            assert!(cache.len() <= MAX_ENTRIES);
        }
        assert!(cache.len() > 0);
    }

    #[test]
    fn the_contexts_kept_per_text_are_bounded() {
        let cache = Cache::new();
        let lengths = lengths();
        let text = "linear-gradient(currentColor 1em, blue)";
        for index in 0..MAX_CONTEXTS * 3 {
            let colors = colors(Scheme::Light, index as f32 / 100.0);
            cache.get_or_parse(text, with_colors(&colors, &lengths), || index);
        }
        let entries = cache.entries.borrow();
        let Some(Entry::PerContext(_, results)) = entries.get(text) else {
            panic!("a text that reads currentColor has per-context results");
        };
        assert!(results.len() <= MAX_CONTEXTS);
    }

    #[test]
    fn a_long_text_is_parsed_but_not_kept() {
        let cache = Cache::new();
        let text = "a".repeat(MAX_TEXT_LEN + 1);
        let lengths = lengths();
        assert_eq!(cache.get_or_parse(&text, Inputs { colors: None, lengths: &lengths }, || 7), 7);
        assert_eq!(cache.len(), 0);
    }
}
