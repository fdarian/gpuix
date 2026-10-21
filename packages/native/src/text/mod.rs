//! Selectable text: the shared foundation for every text GPUIX paints.
//!
//! Ported from Comet (https://github.com/zeronsh/comet), MIT.
//!
//! Every text node in GPUIX goes through [`paint::selectable_text`], so a drag
//! can start in a plain `<text>` and end inside a `<code>` block. That only
//! works because all of them register into the same per-frame registry in paint
//! order. Adding a new text-painting element means calling that helper, never
//! `div().child(string)`.

pub mod atomic;
pub mod inline;
pub mod inline_bounds;
pub mod inline_cursor;
pub mod inline_decoration;
pub mod inline_focus;
pub mod inline_hit;
pub mod inline_insets;
pub mod inline_region;
pub mod paint;
pub mod paragraph;
pub mod runs;
pub mod search;
pub mod selection;
pub mod white_space;

/// A subtree that was replayed puts back what it wrote into the registries of the frame.
pub(crate) fn publish(window: gpui::WindowId, live: &crate::tape::Live) {
    paint::publish(window, live);
    inline_hit::publish(window, live);
}

pub use paint::{
    chrome_text, log_painted_text, painted_highlights, painted_text, range_rects,
    record_start_region, selectable_text, selection_frame_reset, selection_key, HighlightSource,
    PaintedHighlight, SelectableText, SharedSelection,
};
pub use search::{GroupList, HighlightContext, HighlightSet};
