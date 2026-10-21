//! The gpui half of text selection: the per-frame registry, the wash geometry,
//! and the window-level mouse and key listeners.
//!
//! Ported from Comet, MIT.
//! Upstream: https://github.com/zeronsh/comet/blob/main/crates/ui/src/markdown/render.rs
//! Reviewed fixes:
//! - https://github.com/zeronsh/comet/commit/f6911c311dc654734d31bc3097a84fb73659939f
//! - https://github.com/zeronsh/comet/commit/3536a3702ca405fec1321e95f54e280240c5d38f
//!
//! Why the registry is rebuilt during **paint** rather than during build:
//! a `list()` or `uniform_list()` decides at paint time which rows exist. Comet
//! learned this the hard way; do not move registration into `build_element`.
//!
//! The registry is in PAINT order, which is not document order: gpui lifts a
//! positioned element, so its text registers after the siblings that follow it.
//! Hit testing wants paint order (the topmost text wins). Selection and copy
//! want document order, so each entry carries a [`selection::DocPos`] and
//! `resolve_drag` sorts by it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use parking_lot::Mutex;

use gpui::{
    canvas, div, point, prelude::*, px, quad, size, BorderStyle, Bounds, Hsla, SharedString,
    StyledText, TextLayout, TextRow, TextRun, Window, WindowId,
};

use super::selection::{self, SelectionState};
use crate::tape::{Kind, Layered};

/// Shared selection state. `GpuixView` and `GpuixRenderer` both hold clones, and
/// so does every paint closure.
///
/// `Arc<Mutex<..>>` rather than `Rc<RefCell<..>>`: napi requires `GpuixRenderer`
/// to be `Send`, and the renderer needs a handle so `getSelectedText()` works
/// without an App context. All real access is single-threaded, so the mutex is
/// always uncontended.
pub type SharedSelection = Arc<Mutex<SelectionState>>;

/// One painted text element, registered per frame in paint order.
#[derive(Clone)]
struct RegEntry {
    key: Arc<str>,
    doc: selection::DocPos,
    text: SharedString,
    layout: TextLayout,
    /// See [`selection::RegisteredText::group`].
    group: Option<u64>,
    /// See [`selection::RegisteredText::line`].
    line: Option<Arc<str>>,
    /// See [`selection::RegisteredText::cell`].
    cell: Option<selection::TableCell>,
}

/// A subtree that was replayed puts back what it wrote here: the registries keep it until they are
/// read (see [`Layered`]).
pub(crate) fn publish(window: WindowId, live: &crate::tape::Live) {
    if [Kind::Text, Kind::StartRegion, Kind::Painted].iter().all(|kind| !live.has(*kind)) {
        return;
    }
    with_frame(window, |frame| {
        if live.has(Kind::Text) {
            frame.registry.push_live(live.clone());
        }
        if live.has(Kind::StartRegion) {
            frame.start_regions.push_live(live.clone());
        }
        if live.has(Kind::Painted) {
            frame.painted.push_live(live.clone());
        }
    });
}

impl Frame {
    fn registry(&mut self) -> &[RegEntry] {
        self.registry.all(Kind::Text, |data, _| {
            data.downcast_ref::<(WindowId, RegEntry)>().expect("a text write holds its entry").1.clone()
        })
    }

    fn start_regions(&mut self) -> &[StartRegion] {
        self.start_regions.all(Kind::StartRegion, |data, offset| {
            let (_, bounds, selectable) = data.downcast_ref::<(WindowId, Bounds<gpui::Pixels>, bool)>().expect("a start region write holds its box");
            StartRegion { bounds: Bounds::new(bounds.origin + offset, bounds.size), selectable: *selectable }
        })
    }

    fn painted(&mut self) -> &[SharedString] {
        self.painted.all(Kind::Painted, |data, _| {
            data.downcast_ref::<(WindowId, SharedString)>().expect("a painted text write holds its text").1.clone()
        })
    }
}

/// Full element box that owns whether a press may start a selection.
///
/// `userSelect: "none"` chrome and native inputs register `selectable: false`
/// so a same-row nearest-text clamp cannot steal their press. An explicit
/// `userSelect: "text"` island registers `true` and can override an ancestor.
struct StartRegion {
    bounds: Bounds<gpui::Pixels>,
    selectable: bool,
}

/// One highlight wash painted this frame, with the boxes it actually drew.
///
/// The rects are the point: a quad is invisible to `getPaintedText()`, and a
/// match that soft-wraps must produce two boxes. Without the geometry the only
/// way to assert that is a screenshot.
#[derive(Clone, Debug)]
pub struct PaintedHighlight {
    pub element_id: u64,
    pub sub: usize,
    pub text: SharedString,
    /// UTF-16 code-unit offsets, so JS can slice `text` directly.
    pub start: usize,
    pub end: usize,
    pub active: bool,
    /// `(x, y, width, height)` per visual row.
    pub rects: Vec<(f32, f32, f32, f32)>,
}

/// What one window painted this frame.
#[derive(Default)]
struct Frame {
    registry: Layered<RegEntry>,
    start_regions: Layered<StartRegion>,
    /// Every string painted this frame, selectable or not, in paint order.
    ///
    /// Native elements draw their text inside gpui, so it never appears in the
    /// retained tree and `getAllText()` cannot see it. Without this log the only
    /// way to assert what `<code>` or `<diff>` rendered is a screenshot, which
    /// tells you something changed but never what.
    painted: Layered<SharedString>,
    /// Same idea for highlight washes. See [`PaintedHighlight`].
    highlights: Vec<PaintedHighlight>,
}

thread_local! {
    /// By window, because every window runs the frame reset on this thread and a
    /// window's frame must not clear another's.
    static FRAMES: RefCell<HashMap<WindowId, Frame>> = RefCell::new(HashMap::new());
}

/// Runs `read` on the frame of `window`. Never nest calls: the map is borrowed.
fn with_frame<R>(window: WindowId, read: impl FnOnce(&mut Frame) -> R) -> R {
    FRAMES.with(|frames| read(frames.borrow_mut().entry(window).or_default()))
}

fn frame_reset(window: WindowId) {
    FRAMES.with(|frames| frames.borrow_mut().insert(window, Frame::default()));
}

/// A zero-size canvas that clears the per-frame registries and installs the
/// frame's copy and mouse-down listeners. Paint it FIRST in the root, before
/// any text, so each frame holds exactly that frame's visible text elements
/// in paint order.
pub fn selection_frame_reset(
    selection: SharedSelection,
    on_drag_move: impl Fn(WindowId, gpui::Point<gpui::Pixels>, &mut gpui::App) + 'static,
    on_drag_end: impl Fn(&mut gpui::App) + 'static,
) -> impl IntoElement {
    canvas(
        // Prepaint, not paint: every run announces its find matches in prepaint, and the
        // reset canvas is the first to prepaint.
        |_, window, _| super::search::ordinal_frame_reset(window.window_handle().window_id()),
        move |_, _, window, _| {
            let id = window.window_handle().window_id();
            frame_reset(id);
            super::inline_hit::frame_reset(id);
            register_copy_listener(window, &selection);
            register_down_listener(window, &selection);
            register_drag_listeners(window, &selection, on_drag_move, on_drag_end);
        },
    )
    .absolute()
    .w(px(0.0))
    .h(px(0.0))
}

/// Record a selection-start region from an element's painted box.
///
/// Only `track_host_bounds` calls this, so a start region is
/// always the same box automation already uses. Last painted region that contains the point wins.
pub fn record_start_region(window: WindowId, bounds: Bounds<gpui::Pixels>, selectable: bool) {
    crate::tape::write(
        Kind::StartRegion,
        (window, bounds, selectable),
        |(window, bounds, selectable)| push_start_region(*window, *bounds, *selectable),
        |(_, bounds, selectable), offset| format!("start region {:?} {selectable}", Bounds::new(bounds.origin + offset, bounds.size)),
    );
}

fn push_start_region(window: WindowId, bounds: Bounds<gpui::Pixels>, selectable: bool) {
    with_frame(window, |frame| {
        frame.start_regions.push(StartRegion { bounds, selectable })
    });
}

/// Last painted start region that contains `position`.
fn start_region_at(window: WindowId, position: gpui::Point<gpui::Pixels>) -> Option<bool> {
    with_frame(window, |frame| {
        frame
            .start_regions()
            .iter()
            .rev()
            .find(|region| region.bounds.contains(&position))
            .map(|region| region.selectable)
    })
}

/// Every string `window` painted in its last frame, in paint order. Test-facing.
pub fn painted_text(window: WindowId) -> Vec<String> {
    with_frame(window, |frame| {
        frame.painted().iter().map(|s| s.to_string()).collect()
    })
}

/// Every highlight wash `window` painted in its last frame, in paint order. Test-facing.
pub fn painted_highlights(window: WindowId) -> Vec<PaintedHighlight> {
    with_frame(window, |frame| frame.highlights.clone())
}

/// Byte offset to UTF-16 code-unit offset, so the log speaks JS's units.
fn utf16_offset(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())]
        .chars()
        .map(char::len_utf16)
        .sum()
}

/// Record text painted by a custom element that owns its text layout.
pub fn log_painted_text(window: WindowId, text: SharedString) {
    crate::tape::write(
        Kind::Painted,
        (window, text),
        |(window, text)| with_frame(*window, |frame| frame.painted.push(text.clone())),
        |(_, text), _| format!("painted {text:?}"),
    );
}

/// Text that is deliberately NOT selectable: line-number gutters, language
/// tags, diff file headers. It still lands in the paint log so tests can assert
/// on it, but a drag across the block never copies it.
pub fn chrome_text(text: SharedString, runs: Option<Vec<TextRun>>) -> gpui::AnyElement {
    let styled = match runs {
        Some(runs) => StyledText::new(text.clone()).with_runs(runs),
        None => StyledText::new(text.clone()),
    };
    let log = canvas(
        |_, _, _| (),
        move |_, _, window, _| {
            if window.visibility() == gpui::Visibility::Visible {
                log_painted_text(window.window_handle().window_id(), text.clone())
            }
        },
    )
    .keeps_no_position()
    .absolute()
    .unlifted()
    .w(px(0.0))
    .h(px(0.0));
    div().relative().unlifted().child(log).child(styled).into_any_element()
}

/// Selection key for an element. `sub` distinguishes multiple text runs painted
/// by one element, such as the lines of a code block.
pub fn selection_key(element_id: u64, sub: usize) -> Arc<str> {
    format!("{element_id}:{sub}").into()
}

/// Inputs for [`selectable_text`].
/// Where a run's highlight washes come from. Never both: retained text is
/// located once for the whole subtree, while a string generated inside
/// `render()` is matched against itself.
#[derive(Clone)]
pub enum HighlightSource {
    /// Retained `<text>`. A match can span the several host nodes React makes
    /// for one interpolated line, because they were merged before matching.
    Resolved(Arc<super::search::HighlightContext>),
    /// `<code>`, `<markdown>`, `<diff>`: text the retained tree never sees.
    Native(Arc<super::search::HighlightContext>),
}

pub struct SelectableText {
    /// Element that owns the run, and the run's index within it. The selection
    /// key is derived from these, so nothing has to parse it back apart.
    pub element_id: u64,
    pub sub: usize,
    /// Where the owning element sits in the document, see [`selection::DocPos`]. Child indices
    /// from the root through the retained tree, which for a virtual-list row start at the list's.
    pub path: selection::DocPath,
    pub text: SharedString,
    /// `None` is the important case for plain `<text>` nodes: gpui then derives
    /// one run from `window.text_style()`, so colour, weight and family keep
    /// inheriting from ancestor `style` props. Pass `Some(..)` only when the
    /// element owns its own colours, as `<code>` and `<diff>` do.
    pub runs: Option<Vec<TextRun>>,
    /// Per-run changes laid over the inherited text style, for a paragraph whose
    /// inline boxes restyle parts of it. Empty means plain text; otherwise the
    /// lengths cover `text` exactly. Mutually exclusive with `runs`.
    pub run_styles: Vec<super::paragraph::RunStyle>,
    pub selection: SharedSelection,
    pub wash_color: Hsla,
    /// Paints additional quads under the glyphs before the selection wash:
    /// inline-code pills, word-diff highlights. Receives the laid-out text so
    /// it can turn byte ranges into rects with [`range_rects`].
    pub extra_wash: Option<Box<dyn Fn(&TextLayout, &mut Window)>>,
    /// Clickable byte ranges and their payloads, typically link URLs.
    pub links: Vec<(Range<usize>, String)>,
    /// Called with the payload of the range under a click.
    pub on_link: Option<Arc<dyn Fn(&str)>>,
    /// False under `userSelect: "none"`: the text is still painted, logged and
    /// clickable, but it does not join the selection registry.
    pub selectable: bool,
    /// See [`crate::text::selection::RegisteredText::group`]. `None` for a run
    /// that must never merge with its neighbour, which is every custom element.
    pub group: Option<u64>,
    /// See [`crate::text::selection::RegisteredText::line`].
    pub line: Option<Arc<str>>,
    /// See [`crate::text::selection::RegisteredText::cell`].
    pub cell: Option<selection::TableCell>,
    pub highlight: Option<HighlightSource>,
    /// The ends of the text whose empty line space `text-box-trim` cuts.
    pub text_box_trim: gpui::TextBoxTrim,
    /// The inline boxes of a paragraph that the pointer can target, registered
    /// under `element_id` as the container.
    pub inline_targets: Option<Arc<super::inline_hit::Targets>>,
    /// The inline elements of a paragraph that have a box of their own. Their
    /// bounds are recorded from the paragraph's paint.
    pub inline_boxes: Vec<super::inline_bounds::InlineBox>,
    /// The bytes of a paragraph whose inline boxes ask for a `cursor`, see
    /// [`super::inline_cursor`].
    pub inline_cursors: Vec<super::inline_cursor::CursorBox>,
    /// The inline boxes of a paragraph whose `appRegion` differs from the one around them, see
    /// [`super::inline_region`].
    pub inline_regions: Vec<super::inline_region::RegionBox>,
    /// The margin, border and padding of the paragraph's inline boxes along the
    /// line, see [`super::inline_insets`].
    pub inline_insets: Option<std::rc::Rc<super::inline_insets::InsetSpec>>,
    /// The inline boxes of a paragraph that can take focus, in tree order, see
    /// [`super::inline_focus`].
    pub inline_focus: Vec<super::inline_focus::FocusableBox>,
    /// What the inline boxes of a paragraph paint around their text: background
    /// and border behind the glyphs, outline over them, see
    /// [`super::inline_decoration`].
    pub inline_decorations: Vec<super::inline_decoration::BoxDecoration>,
    /// The inline boxes with an `opacity` below one, painted as groups after the rest of the
    /// text, see [`super::paragraph::OpacityGroup`].
    pub opacity_groups: Vec<super::paragraph::OpacityGroup>,
    /// The boxes of a paragraph the line layout aligns to their lines, see
    /// [`super::paragraph::listed_boxes`].
    pub aligned_boxes: Vec<super::paragraph::BoxStyle>,
    /// The atomic inlines of a paragraph. `parent` of each is an index of `aligned_boxes`.
    pub atomics: Vec<gpui::AtomicInline>,
    /// How far past its box the text can draw, if it may be skipped outside its clip, see
    /// [`gpui::InteractiveElement::cullable`].
    pub cull_outset: Option<gpui::Edges<gpui::Pixels>>,
}

impl SelectableText {
    pub fn new(
        element_id: u64,
        sub: usize,
        path: selection::DocPath,
        text: SharedString,
        runs: Option<Vec<TextRun>>,
        selection: SharedSelection,
        wash_color: Hsla,
    ) -> Self {
        Self {
            element_id,
            sub,
            path,
            text,
            runs,
            run_styles: Vec::new(),
            selection,
            wash_color,
            extra_wash: None,
            links: Vec::new(),
            on_link: None,
            selectable: true,
            group: None,
            line: None,
            cell: None,
            highlight: None,
            text_box_trim: gpui::TextBoxTrim::default(),
            inline_targets: None,
            inline_boxes: Vec::new(),
            inline_cursors: Vec::new(),
            inline_regions: Vec::new(),
            inline_insets: None,
            inline_focus: Vec::new(),
            inline_decorations: Vec::new(),
            opacity_groups: Vec::new(),
            aligned_boxes: Vec::new(),
            atomics: Vec::new(),
            cull_outset: None,
        }
    }
}

/// A selectable text element: `StyledText` with a canvas underlay that paints
/// the selection wash and registers into the frame registry.
pub fn selectable_text(opts: SelectableText) -> gpui::AnyElement {
    let SelectableText {
        element_id,
        sub,
        path,
        text,
        runs,
        run_styles,
        selection,
        wash_color,
        extra_wash,
        links,
        on_link,
        selectable,
        group,
        line,
        cell,
        highlight,
        text_box_trim,
        inline_targets,
        inline_boxes,
        inline_cursors,
        inline_regions,
        inline_insets,
        inline_focus,
        inline_decorations,
        opacity_groups,
        aligned_boxes,
        atomics,
        cull_outset,
    } = opts;
    let key = selection_key(element_id, sub);
    let doc = selection::DocPos::new(path, sub);

    debug_assert!(
        runs.is_none() || run_styles.is_empty(),
        "a text has either explicit runs or inline run styles"
    );
    let (layout, glyphs) = if run_styles.is_empty() {
        let styled = match runs {
            Some(runs) => StyledText::new(text.clone()).with_runs(runs),
            None => StyledText::new(text.clone()),
        };
        let styled = match inline_insets {
            Some(spec) => styled.with_insets_for_width(spec.text_key(), move |width| spec.at(width).text()),
            None => styled,
        }
        .with_text_box_trim(text_box_trim);
        (styled.layout().clone(), styled.into_any_element())
    } else {
        let inline = super::paragraph::InlineText::new(text.clone(), run_styles)
            .with_boxes(aligned_boxes, atomics);
        let inline = match inline_insets {
            Some(spec) => inline.with_insets_for_width(spec.text_key(), move |width| spec.at(width).text()),
            None => inline,
        }
        .with_text_box_trim(text_box_trim);
        (inline.layout().clone(), inline.into_any_element())
    };
    super::atomic::note(element_id, &layout);

    let behind: Vec<_> = inline_decorations
        .iter()
        .filter(|decoration| decoration.group == 0 && decoration.paint.paints())
        .cloned()
        .collect();
    let rings: Vec<_> = inline_decorations
        .iter()
        .filter(|decoration| decoration.group == 0 && decoration.outline.is_some())
        .cloned()
        .collect();
    let decorations = (!rings.is_empty())
        .then(|| super::inline_decoration::overlay(layout.clone(), rings));
    let groups = (!opacity_groups.is_empty())
        .then(|| opacity_group_pass(layout.clone(), text.clone(), opacity_groups, inline_decorations));
    let cursor_hitboxes = (!inline_cursors.is_empty())
        .then(|| super::inline_cursor::hitboxes(layout.clone(), inline_cursors));
    let region_hitboxes = (!inline_regions.is_empty())
        .then(|| super::inline_region::regions(layout.clone(), inline_regions));
    let press_focus: Vec<_> = inline_focus
        .iter()
        .map(|focusable| (focusable.id, focusable.handle.clone()))
        .collect();
    let announced = highlight.clone().map(|source| (source, key.clone(), doc.clone(), text.clone()));
    let inline_boxes = std::rc::Rc::new(inline_boxes);
    let layout_of_boxes = layout.clone();
    let boxes_to_record = inline_boxes.clone();
    // The glyphs are placed from the snapped origin of the paragraph; a span reports its box from
    // the origin layout gave it. The wrapper starts where the glyphs do.
    let wrapper = div().relative().unlifted();
    let (wrapper, laid_out) = if inline_boxes.is_empty() {
        (wrapper, None)
    } else {
        let (wrapper, laid_out) = crate::automation::capture_layout_box(wrapper, false);
        (wrapper, Some(laid_out))
    };
    let laid_out_in_layout_pass = laid_out.clone();
    let underlay = canvas(
        move |_, window, _| {
            // Numbers the find matches in document order, which needs every run's matches
            // before the first one paints. See `search::wash`.
            if window.visibility() == gpui::Visibility::Hidden {
                return;
            }
            // A pass that lays out and does not paint (a bounds read) records the boxes of the
            // inline elements once every run is prepainted: this canvas comes before the glyphs
            // that finish the paragraph's line layout.
            if window.is_layout_pass() && !boxes_to_record.is_empty() {
                let layout = layout_of_boxes.clone();
                let boxes = boxes_to_record.clone();
                let laid_out = laid_out_in_layout_pass.clone().expect("a paragraph with boxes captures its own");
                window.publish_layout(move |window, _| {
                    super::inline_bounds::record(&layout, &boxes, laid_out.shift(), window)
                });
            }
            let Some((source, key, doc, text)) = &announced else {
                return;
            };
            let window_id = window.window_handle().window_id();
            match source {
                HighlightSource::Resolved(ctx) => {
                    super::search::announce_retained_run(window_id, ctx, key, doc)
                }
                HighlightSource::Native(ctx) => {
                    super::search::announce_native_run(window_id, ctx, key, doc, text)
                }
            }
        },
        move |_, _, window, cx| {
            // Hidden text is not on screen: it takes no part in selection, links, search washes
            // or the painted-text log.
            if window.visibility() == gpui::Visibility::Hidden {
                return;
            }
            let window_id = window.window_handle().window_id();
            super::inline_decoration::paint_behind(&layout, &behind, window, cx);
            if let Some(paint) = &extra_wash {
                paint(&layout, window);
            }
            // Search washes sit UNDER the selection wash, so a selection over a
            // match still reads as a selection.
            let washes = match &highlight {
                Some(HighlightSource::Resolved(ctx)) => {
                    super::search::washes_for_retained_run(window_id, ctx, &key)
                }
                Some(HighlightSource::Native(ctx)) => {
                    super::search::washes_for_native_run(window_id, ctx, &key, &text)
                }
                None => Vec::new(),
            };
            paint_highlight_washes(&layout, element_id, sub, &text, &washes, window);
            let ranges = if selectable {
                selection.lock().wash_ranges(&key)
            } else {
                Vec::new()
            };
            for range in ranges {
                let mut rects = range_rects(&layout, &range, 0.0, 0.0);
                super::inline_insets::trim_wash(&layout, &text, &range, &mut rects);
                for rect in rects {
                    window.paint_quad(quad(
                        rect,
                        px(0.0),
                        wash_color,
                        px(0.0),
                        gpui::transparent_black(),
                        BorderStyle::default(),
                    ));
                }
            }
            if selectable {
                let entry = RegEntry {
                    key: key.clone(),
                    doc: doc.clone(),
                    text: text.clone(),
                    layout: layout.clone(),
                    group,
                    line: line.clone(),
                    cell,
                };
                crate::tape::write(
                    Kind::Text,
                    (window_id, entry),
                    // Its layout is moved by the frame, with the subtree.
                    |(window, entry)| with_frame(*window, |frame| frame.registry.push(entry.clone())),
                    |(_, entry), _| {
                        format!(
                            "text {} {:?} {:?} {:?} {:?} {:?} {:?}",
                            entry.key,
                            entry.doc,
                            entry.text,
                            entry.layout.bounds(),
                            entry.group,
                            entry.line,
                            entry.cell
                        )
                    },
                );
            }
            log_painted_text(window_id, text.clone());
            if let Some(targets) = &inline_targets {
                super::inline_hit::register(
                    window_id,
                    element_id,
                    layout.clone(),
                    targets.clone(),
                );
            }
            if let Some(laid_out) = &laid_out {
                super::inline_bounds::record(&layout, &inline_boxes, laid_out.shift(), window);
            }
            super::inline_focus::focus_on_press(window, element_id, press_focus.clone());
            if let Some(on_link) = &on_link {
                register_link_listener(window, &layout, &links, on_link, &selection);
            }
        },
    )
    .keeps_no_position()
    .absolute()
    .unlifted()
    .size_full();

    // Positioned only to hold the underlay. Lifted, every text run would rise above the
    // later boxes of its context.
    let wrapper = match cull_outset {
        Some(outset) => wrapper.cullable(outset),
        None => wrapper,
    };
    wrapper
        .child(underlay)
        .child(glyphs)
        .children(decorations)
        .children(groups)
        .children(cursor_hitboxes)
        .children(region_hitboxes)
        .children(super::inline_focus::anchors(inline_focus))
        .into_any_element()
}

/// Paints every opacity group of a paragraph, after its glyphs and rings: CSS paints a box with
/// an `opacity` below one after the inline content of the stacking context around it.
fn opacity_group_pass(
    layout: TextLayout,
    text: SharedString,
    groups: Vec<super::paragraph::OpacityGroup>,
    decorations: Vec<super::inline_decoration::BoxDecoration>,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |_, _, window, cx| {
            if window.visibility() == gpui::Visibility::Hidden {
                return;
            }
            for root in groups.iter().filter(|group| group.parent == 0) {
                paint_opacity_group(&layout, &text, &groups, &decorations, root, window, cx);
            }
        },
    )
    .absolute()
    .size_full()
}

/// One group: what it paints, composited at its `opacity`, with its own groups inside it, over
/// its glyphs.
fn paint_opacity_group(
    layout: &TextLayout,
    text: &str,
    groups: &[super::paragraph::OpacityGroup],
    decorations: &[super::inline_decoration::BoxDecoration],
    group: &super::paragraph::OpacityGroup,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    let effect = gpui::GroupEffect { opacity: group.opacity, ..Default::default() };
    window.with_effect_group(effect, None, |window| {
        let own: Vec<_> = decorations
            .iter()
            .filter(|decoration| decoration.group == group.id)
            .cloned()
            .collect();
        let behind: Vec<_> = own.iter().filter(|decoration| decoration.paint.paints()).cloned().collect();
        super::inline_decoration::paint_behind(layout, &behind, window, cx);
        layout.paint_group(text, group.id, window, cx);
        super::inline_decoration::paint_rings(layout, &own, window, cx);
        for inner in groups.iter().filter(|inner| inner.parent == group.id) {
            paint_opacity_group(layout, text, groups, decorations, inner, window, cx);
        }
    });
}

/// Paint one run's highlight washes and log their geometry.
fn paint_highlight_washes(
    layout: &TextLayout,
    element_id: u64,
    sub: usize,
    text: &SharedString,
    washes: &[super::search::Wash],
    window: &mut Window,
) {
    for wash in washes {
        let mut rects = range_rects(layout, &wash.range, 0.0, 0.0);
        super::inline_insets::trim_wash(layout, text, &wash.range, &mut rects);
        if rects.is_empty() {
            continue;
        }
        for rect in &rects {
            window.paint_quad(quad(
                *rect,
                px(wash.radius),
                wash.color,
                px(0.0),
                gpui::transparent_black(),
                BorderStyle::default(),
            ));
        }
        with_frame(window.window_handle().window_id(), |frame| {
            frame.highlights.push(PaintedHighlight {
                element_id,
                sub,
                text: text.clone(),
                start: utf16_offset(text, wash.range.start),
                end: utf16_offset(text, wash.range.end),
                active: wash.active,
                rects: rects
                    .iter()
                    .map(|r| {
                        (
                            f32::from(r.origin.x),
                            f32::from(r.origin.y),
                            f32::from(r.size.width),
                            f32::from(r.size.height),
                        )
                    })
                    .collect(),
            })
        });
    }
}

/// Fire `on_link` for the range under a click.
///
/// Registered on mouse UP and skipped when a selection exists, so a drag that
/// happens to end on a link selects text instead of navigating. gpui's
/// `InteractiveText` does per-range hit testing too, but it owns the
/// `StyledText` and would displace the selection underlay.
fn register_link_listener(
    window: &mut Window,
    layout: &TextLayout,
    links: &[(Range<usize>, String)],
    on_link: &Arc<dyn Fn(&str)>,
    selection: &SharedSelection,
) {
    use gpui::{DispatchPhase, MouseButton, MouseUpEvent};

    if links.is_empty() {
        return;
    }
    let (layout, links, on_link, selection) = (
        layout.clone(),
        links.to_vec(),
        on_link.clone(),
        selection.clone(),
    );
    window.on_mouse_event(move |e: &MouseUpEvent, phase, _window, _cx| {
        if phase != DispatchPhase::Bubble || e.button != MouseButton::Left {
            return;
        }
        if !layout.bounds().contains(&e.position) {
            return;
        }
        // A drag that ends on a link is a selection, not a navigation. gpui
        // dispatches bubble listeners in reverse registration order, so the
        // spans from the drag's mouse-moves are already resolved here.
        if selection.lock().selected_text().is_some() {
            return;
        }
        // `index_for_position` returns Err with the nearest index when the
        // point is past the end of a line. Only an exact hit counts, otherwise
        // clicking the empty space after a paragraph would open its last link.
        let Ok(ix) = layout.index_for_position(e.position) else {
            return;
        };
        if let Some((_, payload)) = links.iter().find(|(range, _)| range.contains(&ix)) {
            on_link(payload);
        }
    });
}

/// `(element index, byte offset)` for a window position.
///
/// The index is into the registry, which is in paint order. Prefers the element whose full
/// bounds contain the point, taking the LAST such element in paint order so an overlay wins over
/// what it covers. Only
/// when the point is outside every text does it fall back to the nearest by
/// vertical then horizontal distance. `index_for_position` then clamps: left
/// of a line is the line start, right of a line is the line end.
///
/// Mouse-down uses [`registry_point_on_line`] so a press in a composer or
/// titlebar does not start a selection on the nearest paragraph. The drag
/// head keeps the unbounded clamp so a selection that already started can
/// still run into the gutter or past the last line.
///
/// Comet compares Y only, because its transcript is a single column where two
/// texts never share a vertical band. GPUIX lays out arbitrary React trees: a
/// Y-only match picks the leftmost text in a flex row no matter where the
/// pointer actually is.
fn registry_point(
    window: WindowId,
    position: gpui::Point<gpui::Pixels>,
) -> Option<(usize, usize)> {
    with_frame(window, |frame| {
        let reg = frame.registry();
        let mut contained: Option<usize> = None;
        let mut nearest: Option<(usize, (f32, f32))> = None;

        for (ei, entry) in reg.iter().enumerate() {
            let b = entry.layout.bounds();
            if b.contains(&position) {
                contained = Some(ei);
                continue;
            }
            let dy = if position.y < b.top() {
                f32::from(b.top() - position.y)
            } else if position.y > b.bottom() {
                f32::from(position.y - b.bottom())
            } else {
                0.0
            };
            // Within a shared band, break the tie on horizontal distance so a
            // drag in the right-hand column does not snap to the left one.
            // Compared lexicographically: vertical distance dominates outright,
            // because a weighted sum lets a huge dx beat a 1px dy.
            let dx = if position.x < b.left() {
                f32::from(b.left() - position.x)
            } else if position.x > b.right() {
                f32::from(position.x - b.right())
            } else {
                0.0
            };
            let distance = (dy, dx);
            if nearest.is_none_or(|(_, best): (usize, (f32, f32))| {
                (distance.0, distance.1) < (best.0, best.1)
            }) {
                nearest = Some((ei, distance));
            }
        }

        let ei = contained.or(nearest.map(|(ei, _)| ei))?;
        let ix = match reg[ei].layout.index_for_position(position) {
            Ok(ix) | Err(ix) => ix,
        };
        Some((ei, ix))
    })
}

/// Like [`registry_point`], but only when the pointer shares a text's vertical
/// band. That is the empty start or end of the line, a gutter, or parent
/// padding on that row. A press above or below every line is chrome.
fn registry_point_on_line(
    window: WindowId,
    position: gpui::Point<gpui::Pixels>,
) -> Option<(usize, usize)> {
    let (ei, ix) = registry_point(window, position)?;
    with_frame(window, |frame| {
        let b = frame.registry().get(ei)?.layout.bounds();
        (position.y >= b.top() && position.y <= b.bottom()).then_some((ei, ix))
    })
}

/// Resolve the drag head, an index into the registry's paint order, into spans in document
/// order.
fn resolve_drag(window: WindowId, selection: &SharedSelection, head: (usize, usize)) -> bool {
    with_frame(window, |frame| {
        let painted: Vec<(selection::RegisteredText, &selection::DocPos)> = frame
            .registry()
            .iter()
            .map(|e| {
                let text = selection::RegisteredText {
                    line: e.line.as_deref(),
                    cell: e.cell,
                    ..selection::RegisteredText::whole(e.key.as_ref(), e.text.as_ref(), e.group)
                };
                (text, &e.doc)
            })
            .collect();
        selection.lock().update_drag_painted(&painted, head)
    })
}

/// Continue an active drag at a window position.
pub(crate) fn update_drag_at(
    window: WindowId,
    selection: &SharedSelection,
    position: gpui::Point<gpui::Pixels>,
) -> bool {
    let Some(head) = registry_point(window, position) else {
        return false;
    };
    resolve_drag(window, selection, head)
}

/// One window-level mouse-down for the whole frame.
///
/// Per-element downs required the press to land inside a `TextLayout` box,
/// which is the glyph bounds, not the parent padding. A single listener
/// clamps with [`registry_point_on_line`].
fn register_down_listener(window: &mut Window, selection: &SharedSelection) {
    use gpui::{DispatchPhase, MouseButton, MouseDownEvent};

    let selection = selection.clone();
    window.on_mouse_event(move |e: &MouseDownEvent, phase, window, _cx| {
        if phase != DispatchPhase::Bubble || e.button != MouseButton::Left {
            return;
        }
        // As in the DOM, a prevented mousedown starts no selection.
        if window.default_prevented() {
            return;
        }
        let window_id = window.window_handle().window_id();
        if start_region_at(window_id, e.position) == Some(false) {
            let mut sel = selection.lock();
            if !sel.is_active() {
                return;
            }
            sel.clear();
            drop(sel);
            window.refresh();
            return;
        }
        let hit = registry_point_on_line(window_id, e.position).and_then(|(ei, ix)| {
            with_frame(window_id, |frame| {
                frame
                    .registry()
                    .get(ei)
                    .map(|entry| (entry.key.clone(), entry.text.clone(), ix))
            })
        });
        let mut sel = selection.lock();
        let was_active = sel.is_active();
        if let Some((key, text, ix)) = hit {
            match e.click_count {
                2 => {
                    window.blur();
                    let range = selection::word_range(&text, ix);
                    sel.begin_with_span(&key, &text, range);
                }
                n if n >= 3 => {
                    window.blur();
                    sel.begin_with_span(&key, &text, 0..text.len());
                }
                // A tap must not select or blur. iOS uses that gesture to
                // scroll or to focus an input; the first dragging move
                // promotes this press into a real selection.
                _ => sel.arm(&key, ix),
            }
        } else if sel.is_active() || sel.is_pending() {
            sel.clear();
        } else {
            return;
        }
        let needs_refresh = was_active || sel.is_active();
        drop(sel);
        if needs_refresh {
            window.refresh();
        }
    });
}

/// One window-level move and up listener for the frame.
///
/// These are independent of the anchor run, so virtualization cannot remove
/// the listener that owns the active drag. The reset canvas is the first root
/// child, so gpui's reverse bubble order runs these last. Do not stop
/// propagation on mouse-up elsewhere, or a drag and its edge-scroll timer
/// stay armed.
fn register_drag_listeners(
    window: &mut Window,
    selection: &SharedSelection,
    on_drag_move: impl Fn(WindowId, gpui::Point<gpui::Pixels>, &mut gpui::App) + 'static,
    on_drag_end: impl Fn(&mut gpui::App) + 'static,
) {
    use gpui::{DispatchPhase, MouseButton, MouseMoveEvent, MouseUpEvent};

    let move_selection = selection.clone();
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
        if phase != DispatchPhase::Bubble || !event.dragging() {
            return;
        }
        if move_selection.lock().promote_pending() {
            window.blur();
        }
        let window_id = window.window_handle().window_id();
        if update_drag_at(window_id, &move_selection, event.position) {
            window.refresh();
        }
        if move_selection.lock().is_dragging() {
            on_drag_move(window_id, event.position, cx);
        }
    });

    let up_selection = selection.clone();
    window.on_mouse_event(move |event: &MouseUpEvent, phase, _window, cx| {
        if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
            return;
        }
        let mut selection = up_selection.lock();
        let was_dragging = selection.is_dragging();
        selection.cancel_pending();
        selection.end_active_drag();
        drop(selection);
        // A tap never started a drag. Calling on_drag_end anyway nested-updates
        // GpuixView while GPUI still holds the root lease for this mouse-up.
        if was_dragging {
            on_drag_end(cx);
        }
    });
}

fn register_copy_listener(window: &mut Window, selection: &SharedSelection) {
    use gpui::{ClipboardItem, DispatchPhase, KeyDownEvent};

    let selection = selection.clone();
    window.on_root_key_event(move |e: &KeyDownEvent, phase, _window, cx| {
        if phase != DispatchPhase::Bubble {
            return;
        }
        let modifiers = &e.keystroke.modifiers;
        if e.keystroke.key != "c" || !(modifiers.platform || modifiers.control) {
            return;
        }
        // Release the selection lock before entering the platform clipboard.
        let text = selection.lock().selected_text();
        if let Some(text) = text {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            cx.stop_propagation();
        }
    });
}

/// The wash boxes for one byte range: one box per visual line the range covers,
/// since soft wraps split it, in window coordinates from the laid-out text's own
/// geometry.
///
/// `pad_x` overhangs the box horizontally (inline code); `inset_y` shrinks it
/// vertically. Both are 0 for a selection wash, which wants full-height boxes
/// that tile seamlessly across wrapped rows, however tall each row is.
pub fn range_rects(
    layout: &TextLayout,
    range: &Range<usize>,
    pad_x: f32,
    inset_y: f32,
) -> Vec<Bounds<gpui::Pixels>> {
    range_rows_with_positions(
        layout.bounds(),
        range,
        pad_x,
        inset_y,
        |index| position_on_row(layout, index),
        |index| layout.row_for_index(index),
        |index| layout.align_offset_at(index),
    )
    .into_iter()
    .map(|row| row.rect)
    .collect()
}

/// Where the byte `index` is along its row, and the top of the row. gpui puts the placeholder
/// of an atomic inline at the top of the margin box of the box, which is not on the top of the
/// row, and a range along a row is measured from the row.
fn position_on_row(layout: &TextLayout, index: usize) -> Option<gpui::Point<gpui::Pixels>> {
    let position = layout.position_for_index(index)?;
    let row = layout.row_for_index(index)?;
    Some(point(position.x, row.top))
}

/// A visual line a byte range covers.
pub struct RangeRow {
    /// The part of the range on the row, the full height of the row.
    pub rect: Bounds<gpui::Pixels>,
    /// The byte the part starts at, which is where its line's metrics are found.
    pub start: usize,
    /// The byte the part ends at. The glyph at `end` is on the next row when this one is
    /// followed by another: a wrap boundary is the end of the row above for an index, and the
    /// start of the row below for a glyph.
    pub end: usize,
    /// The row the part is on.
    pub row: TextRow,
}

/// [`range_rects`] without padding, and with the row each box is on.
pub fn range_rows(layout: &TextLayout, range: &Range<usize>) -> Vec<RangeRow> {
    range_rows_with_positions(
        layout.bounds(),
        range,
        0.0,
        0.0,
        |index| position_on_row(layout, index),
        |index| layout.row_for_index(index),
        |index| layout.align_offset_at(index),
    )
}

/// `align_offset` is how far text alignment moves the row a byte is on from the left of `bounds`.
fn range_rows_with_positions(
    bounds: Bounds<gpui::Pixels>,
    range: &Range<usize>,
    pad_x: f32,
    inset_y: f32,
    position_for_index: impl Fn(usize) -> Option<gpui::Point<gpui::Pixels>>,
    row_for_index: impl Fn(usize) -> Option<TextRow>,
    align_offset: impl Fn(usize) -> gpui::Pixels,
) -> Vec<RangeRow> {
    let mut rects = Vec::new();
    let mut cur = range.start;
    // Walk the range one visual row at a time: binary search for the furthest
    // index that still sits on the current row.
    let mut guard = 0;
    while cur < range.end && guard < 256 {
        guard += 1;
        let Some(mut p1) = position_for_index(cur) else {
            break;
        };
        let mut row_index = cur;
        if let Some(after) = position_for_index(cur.saturating_add(1)) {
            if after.y > p1.y {
                p1 = point(bounds.left() + align_offset(cur + 1), after.y);
                row_index = cur + 1;
            }
        }
        // A soft-wrap boundary closes one row with upstream affinity and starts
        // the next row with the downstream correction above.
        let (seg_end, next) = match position_for_index(range.end) {
            Some(pe) if pe.y == p1.y => (range.end, range.end),
            _ => {
                let (mut lo, mut hi) = (cur, range.end);
                while hi - lo > 1 {
                    let mid = lo + (hi - lo) / 2;
                    match position_for_index(mid) {
                        Some(pm) if pm.y == p1.y => lo = mid,
                        _ => hi = mid,
                    }
                }
                (lo, lo)
            }
        };
        if let Some(p2) = position_for_index(seg_end) {
            if p2.x > p1.x {
                let row = row_for_index(row_index)
                    .expect("an index with a position is on a row");
                rects.push(RangeRow {
                    rect: Bounds::new(
                        point(p1.x - px(pad_x), p1.y + px(inset_y)),
                        size(p2.x - p1.x + px(2.0 * pad_x), row.height - px(2.0 * inset_y)),
                    ),
                    start: cur,
                    end: seg_end,
                    row,
                });
            }
        }
        if next <= cur {
            break;
        }
        cur = next;
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Model GPUI's upstream affinity at a soft-wrap boundary: byte 5 is
    /// reported at the end of row 0, while byte 6 is after the first glyph on
    /// row 1.
    fn wrapped_position(ix: usize) -> Option<gpui::Point<gpui::Pixels>> {
        (ix <= 9).then(|| {
            if ix <= 5 {
                point(px(ix as f32 * 10.0), px(0.0))
            } else {
                point(px((ix - 5) as f32 * 10.0), px(22.0))
            }
        })
    }

    fn row_of_height(y: f32, height: f32) -> TextRow {
        TextRow {
            top: px(y),
            height: px(height),
            baseline: px(y + height * 0.75),
        }
    }

    /// Rows 22px tall, like the wrapped text of `wrapped_position`.
    fn even_row(ix: usize) -> Option<TextRow> {
        (ix <= 9).then(|| row_of_height(if ix <= 5 { 0.0 } else { 22.0 }, 22.0))
    }

    fn wrapped_range_rows(range: Range<usize>) -> Vec<RangeRow> {
        range_rows_with_positions(
            Bounds::new(point(px(0.0), px(0.0)), size(px(50.0), px(44.0))),
            &range,
            0.0,
            0.0,
            wrapped_position,
            even_row,
            |_| px(0.0),
        )
    }

    fn wrapped_range_rects(range: Range<usize>) -> Vec<Bounds<gpui::Pixels>> {
        wrapped_range_rows(range).into_iter().map(|row| row.rect).collect()
    }

    #[test]
    fn range_starting_at_soft_wrap_includes_first_glyph() {
        let rects = wrapped_range_rects(5..9);
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].origin, point(px(0.0), px(22.0)));
        assert_eq!(rects[0].size, size(px(40.0), px(22.0)));
    }

    #[test]
    fn a_row_names_the_bytes_it_starts_and_ends_at() {
        let rows = wrapped_range_rows(2..9);
        assert_eq!(rows.iter().map(|row| row.start).collect::<Vec<_>>(), vec![2, 5]);
        assert_eq!(rows.iter().map(|row| row.end).collect::<Vec<_>>(), vec![5, 9]);
    }

    #[test]
    fn range_crossing_soft_wrap_includes_first_continuation_glyph() {
        let rects = wrapped_range_rects(2..9);
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0].origin, point(px(20.0), px(0.0)));
        assert_eq!(rects[0].size, size(px(30.0), px(22.0)));
        assert_eq!(rects[1].origin, point(px(0.0), px(22.0)));
        assert_eq!(rects[1].size, size(px(40.0), px(22.0)));
    }

    /// The first row of `wrapped_position` is 30px tall and the second 50px.
    fn uneven_row(ix: usize) -> Option<TextRow> {
        (ix <= 9).then(|| {
            if ix <= 5 {
                row_of_height(0.0, 30.0)
            } else {
                row_of_height(30.0, 50.0)
            }
        })
    }

    fn uneven_positions(ix: usize) -> Option<gpui::Point<gpui::Pixels>> {
        wrapped_position(ix).map(|position| {
            if position.y > px(0.0) {
                point(position.x, px(30.0))
            } else {
                position
            }
        })
    }

    #[test]
    fn each_box_is_as_tall_as_its_own_row() {
        let rows = range_rows_with_positions(
            Bounds::new(point(px(0.0), px(0.0)), size(px(50.0), px(80.0))),
            &(2..9),
            0.0,
            0.0,
            uneven_positions,
            uneven_row,
            |_| px(0.0),
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].rect.origin, point(px(20.0), px(0.0)));
        assert_eq!(rows[0].rect.size.height, px(30.0));
        assert_eq!(rows[1].rect.origin, point(px(0.0), px(30.0)));
        assert_eq!(rows[1].rect.size.height, px(50.0));
        assert_eq!(rows[0].row, row_of_height(0.0, 30.0));
        assert_eq!(rows[1].row, row_of_height(30.0, 50.0));
    }

    #[test]
    fn an_inset_shrinks_a_tall_row_by_the_same_amount() {
        let rows = range_rows_with_positions(
            Bounds::new(point(px(0.0), px(0.0)), size(px(50.0), px(80.0))),
            &(2..9),
            0.0,
            1.5,
            uneven_positions,
            uneven_row,
            |_| px(0.0),
        );
        assert_eq!(rows[0].rect.size.height, px(27.0));
        assert_eq!(rows[1].rect.origin.y, px(31.5));
        assert_eq!(rows[1].rect.size.height, px(47.0));
    }

    fn paint_into(window: WindowId, label: &str) {
        let text = SharedString::from(label.to_owned());
        with_frame(window, |frame| {
            frame.registry.push(RegEntry {
                key: selection_key(1, 0),
                doc: selection::DocPos::new(Arc::from([]), 0),
                text: text.clone(),
                layout: TextLayout::default(),
                group: None,
                line: None,
                cell: None,
            });
            frame.highlights.push(PaintedHighlight {
                element_id: 1,
                sub: 0,
                text: text.clone(),
                start: 0,
                end: 1,
                active: false,
                rects: Vec::new(),
            });
        });
        record_start_region(
            window,
            Bounds::new(point(px(0.0), px(0.0)), size(px(10.0), px(10.0))),
            false,
        );
        log_painted_text(window, text);
    }

    #[test]
    fn a_window_resetting_its_frame_leaves_another_windows_text_alone() {
        let (one, two) = (WindowId::from(1), WindowId::from(2));
        frame_reset(one);
        frame_reset(two);
        paint_into(one, "one");
        paint_into(two, "two");

        frame_reset(two);

        assert_eq!(painted_text(one), ["one"]);
        assert_eq!(painted_highlights(one).len(), 1);
        assert_eq!(start_region_at(one, point(px(5.0), px(5.0))), Some(false));
        assert_eq!(with_frame(one, |frame| frame.registry().len()), 1);

        assert!(painted_text(two).is_empty());
        assert!(painted_highlights(two).is_empty());
        assert_eq!(start_region_at(two, point(px(5.0), px(5.0))), None);
        assert_eq!(with_frame(two, |frame| frame.registry().len()), 0);
    }
}
