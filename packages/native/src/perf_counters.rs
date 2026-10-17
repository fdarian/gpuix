//! Work counters for the element build, read by the test renderer.
//!
//! They exist so a perf change can be checked by what the build does and not only by how long it
//! took: a frame that builds 2042 elements and clones a style per element is the same cost
//! whether the clock was noisy or not. Plain thread-local increments, always compiled: the build
//! runs on the one thread that owns the window.

use std::cell::{Cell, RefCell};

thread_local! {
    static ELEMENTS_BUILT: Cell<u64> = const { Cell::new(0) };
    static STYLE_DESC_CLONES: Cell<u64> = const { Cell::new(0) };
    static INHERITED_CLONES: Cell<u64> = const { Cell::new(0) };
    static RETAINABLE_ROOTS: Cell<u64> = const { Cell::new(0) };
    static RETAINABLE_ELEMENTS_LARGE: Cell<u64> = const { Cell::new(0) };
    static RETAIN_MISMATCHES: Cell<u64> = const { Cell::new(0) };
    /// Elements by the reason they are, or are not, in a subtree a retained replay could reuse.
    static RETAIN_SLOTS: RefCell<[u64; crate::retain::SLOTS]> = const { RefCell::new([0; crate::retain::SLOTS]) };
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    pub elements_built: u64,
    pub style_desc_clones: u64,
    pub inherited_clones: u64,
    /// The maximal unchanged, retainable subtrees of the builds, see `retain`.
    pub retainable_roots: u64,
    /// Elements in them.
    pub retainable_elements: u64,
    /// Elements in those roots that have at least `retain::LARGE_ROOT` elements.
    pub retainable_elements_large: u64,
    /// Elements judged unchanged that were built differently; none unless `retain` is verifying.
    pub retain_mismatches: u64,
}

pub(crate) fn element_built() {
    ELEMENTS_BUILT.with(|count| count.set(count.get() + 1));
}

pub(crate) fn retained(roots: u64, large_elements: u64) {
    RETAINABLE_ROOTS.with(|count| count.set(count.get() + roots));
    RETAINABLE_ELEMENTS_LARGE.with(|count| count.set(count.get() + large_elements));
}

pub(crate) fn retain_mismatch() {
    RETAIN_MISMATCHES.with(|count| count.set(count.get() + 1));
}

pub(crate) fn retain_slot(slot: usize, elements: u64) {
    RETAIN_SLOTS.with(|slots| slots.borrow_mut()[slot] += elements);
}

pub(crate) fn retain_slots(counts: &[u64; crate::retain::SLOTS]) {
    RETAIN_SLOTS.with(|slots| {
        for (slot, count) in slots.borrow_mut().iter_mut().zip(counts) {
            *slot += count;
        }
    });
}

/// Elements counted under each reason since the last reset, the reasons that have any, most first.
pub(crate) fn retain_reasons() -> Vec<(String, u64)> {
    let mut reasons: Vec<(String, u64)> = RETAIN_SLOTS.with(|slots| {
        slots
            .borrow()
            .iter()
            .enumerate()
            .filter(|(_, count)| **count > 0)
            .map(|(slot, count)| (crate::retain::slot_name(slot), *count))
            .collect()
    });
    reasons.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    reasons
}

pub(crate) fn snapshot() -> Counts {
    Counts {
        elements_built: ELEMENTS_BUILT.with(Cell::get),
        style_desc_clones: STYLE_DESC_CLONES.with(Cell::get),
        inherited_clones: INHERITED_CLONES.with(Cell::get),
        retainable_roots: RETAINABLE_ROOTS.with(Cell::get),
        retainable_elements: RETAIN_SLOTS.with(|slots| slots.borrow()[crate::retain::RETAINED]),
        retainable_elements_large: RETAINABLE_ELEMENTS_LARGE.with(Cell::get),
        retain_mismatches: RETAIN_MISMATCHES.with(Cell::get),
    }
}

pub(crate) fn reset() {
    ELEMENTS_BUILT.with(|count| count.set(0));
    STYLE_DESC_CLONES.with(|count| count.set(0));
    INHERITED_CLONES.with(|count| count.set(0));
    RETAINABLE_ROOTS.with(|count| count.set(0));
    RETAINABLE_ELEMENTS_LARGE.with(|count| count.set(0));
    RETAIN_MISMATCHES.with(|count| count.set(0));
    RETAIN_SLOTS.with(|slots| *slots.borrow_mut() = [0; crate::retain::SLOTS]);
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
