//! A borrow of a host element for the helpers that take an element and hand it back.
//!
//! A `Stateful<Div>` is 1.6 KB, and the builders thread it through a few dozen `el = helper(el)`
//! calls per element, each a copy in and a copy out. Those helpers are generic over gpui's
//! `Styled` and `InteractiveElement`, so they take a `Slot` instead, which moves eight bytes.
//! What a `Slot` cannot do is what only `Div` itself has (`stacking_context`, `paint_ranks`):
//! call those on the element once the slot is dropped.

use gpui::{Div, InteractiveElement, Interactivity, Stateful, StatefulInteractiveElement, Styled};

pub(crate) struct Slot<'a>(&'a mut Stateful<Div>);

impl<'a> Slot<'a> {
    pub(crate) fn new(element: &'a mut Stateful<Div>) -> Self {
        Self(element)
    }
}

impl Styled for Slot<'_> {
    fn style(&mut self) -> &mut gpui::StyleRefinement {
        self.0.style()
    }
}

impl InteractiveElement for Slot<'_> {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.0.interactivity()
    }
}

impl StatefulInteractiveElement for Slot<'_> {}
