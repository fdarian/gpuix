//! Work counters for the element build, read by the test renderer.
//!
//! They exist so a perf change can be checked by what the build does and not only by how long it
//! took: a frame that builds 2042 elements and clones a style per element is the same cost
//! whether the clock was noisy or not. Plain thread-local increments, always compiled: the build
//! runs on the one thread that owns the window.

use std::cell::Cell;

thread_local! {
    static ELEMENTS_BUILT: Cell<u64> = const { Cell::new(0) };
    static STYLE_DESC_CLONES: Cell<u64> = const { Cell::new(0) };
    static INHERITED_CLONES: Cell<u64> = const { Cell::new(0) };
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    pub elements_built: u64,
    pub style_desc_clones: u64,
    pub inherited_clones: u64,
}

pub(crate) fn element_built() {
    ELEMENTS_BUILT.with(|count| count.set(count.get() + 1));
}

pub(crate) fn snapshot() -> Counts {
    Counts {
        elements_built: ELEMENTS_BUILT.with(Cell::get),
        style_desc_clones: STYLE_DESC_CLONES.with(Cell::get),
        inherited_clones: INHERITED_CLONES.with(Cell::get),
    }
}

pub(crate) fn reset() {
    ELEMENTS_BUILT.with(|count| count.set(0));
    STYLE_DESC_CLONES.with(|count| count.set(0));
    INHERITED_CLONES.with(|count| count.set(0));
}

/// Which counter a [`CloneProbe`] bumps.
pub(crate) const STYLE_DESC: u8 = 0;
pub(crate) const INHERITED: u8 = 1;

/// A field with no data whose `Clone` counts. A derived `Clone` of the struct that holds it clones
/// every field, so this sees every clone of the struct, nested ones (a state style inside a style)
/// included, without listing the clone sites.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CloneProbe<const COUNTER: u8>;

impl<const COUNTER: u8> Clone for CloneProbe<COUNTER> {
    fn clone(&self) -> Self {
        let counter = match COUNTER {
            STYLE_DESC => &STYLE_DESC_CLONES,
            INHERITED => &INHERITED_CLONES,
            _ => unreachable!("no counter {COUNTER}"),
        };
        counter.with(|count| count.set(count.get() + 1));
        Self
    }
}
