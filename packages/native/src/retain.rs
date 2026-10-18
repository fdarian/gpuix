//! Which subtrees of the element tree a frame leaves unbuilt, because gpui replays what they left in
//! the frame before (#37 step 5b).
//!
//! gpui consumes its elements every frame, so skipping a subtree means handing gpui a
//! `gpui::retained` element in its place and replaying what it prepainted and painted. A frame
//! decides before it builds an element: it skips it when the last build recorded it as eligible, its
//! entry key is the same, and no source of the frame names it or anything below it (see
//! `Sources`). Everything below is also how the last build of an element is judged after the fact,
//! which is how eligibility and the entry key are recorded.
//!
//! A built element has a *key*, what its build is a function of that the retained tree can see: its
//! `subtree_revision`, a fingerprint of what it inherits (hashed by content: group, container and
//! highlight scopes are fresh `Arc`s every frame), and its shape (where it is, what its parent made
//! of it). What the tree cannot see is *volatile*: pointer and focus state, running motion,
//! transitions and animations, scrolling, measurements, the selection. An element is *changed* when
//! its key or its volatile inputs differ from the last build, or it has never been built. The
//! ancestors of changed elements are the *spine*, which builds anyway.
//!
//! A subtree is *retained* when nothing in it changed and nothing in it is on the exclusion list
//! (`Exclusion`), and when it is a stacking context or has nothing in it that is lifted out of it.
//! The maximal such subtrees are the would-be placeholders. They are counted, and every other
//! element is counted under the first reason it is not in one.
//!
//! What an element reads of other elements is in its key, or the element is excluded: a link
//! (`<label>`, `aria-*` references, see `accessibility::links_key`) and whether a click on it is
//! taken (the click path) are in its shape, and a custom element says what it keeps for itself
//! (`CustomElement::retain_state`) or is rebuilt every frame. An `overflow: hidden` element is
//! no obstacle: its scroll offset is a volatile input like a scroller's, and what a clip does to
//! what is inside it is gpui's replay key (content mask, absolute bounds), not the build's.
//!
//! Known gap, found by the verify mode only where a test reaches it: a parent whose children are
//! chosen by `hold_display` (a `display` transition with `allow-discrete`) builds a different set
//! of children with an unchanged key.
//!
//! Not known at build time: that layout gives a retained subtree the same box it had. Step 5's
//! replay key checks that, so the numbers here are an upper bound for frames that move layout.

use std::collections::HashMap;
use std::hash::Hasher;
use std::rc::Rc;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet, FxHasher};

mod debug;

use crate::renderer::{GroupScope, Inherited};
use crate::tape::BuildEffect;

/// A hash of the contents of what the build reads, cheap enough to run once per element.
pub(crate) struct Fingerprint(FxHasher);

impl Fingerprint {
    pub(crate) fn new() -> Self {
        Self(FxHasher::default())
    }

    pub(crate) fn word(&mut self, word: u64) {
        self.0.write_u64(word);
    }

    pub(crate) fn bits(&mut self, flags: &[bool]) {
        self.0.write_u64(flags.iter().fold(0, |bits, flag| bits << 1 | u64::from(*flag)));
    }

    pub(crate) fn f32(&mut self, value: f32) {
        self.0.write_u32(value.to_bits());
    }

    pub(crate) fn f64(&mut self, value: f64) {
        self.0.write_u64(value.to_bits());
    }

    pub(crate) fn option_f64(&mut self, value: Option<f64>) {
        self.0.write_u64(value.map_or(u64::MAX, f64::to_bits));
    }

    pub(crate) fn str(&mut self, value: &str) {
        self.0.write(value.as_bytes());
        self.0.write_u8(0xff);
    }

    pub(crate) fn hash<T: std::hash::Hash>(&mut self, value: &T) {
        value.hash(&mut self.0);
    }

    pub(crate) fn finish(&self) -> u64 {
        self.0.finish()
    }
}

/// Everything in `Inherited` the build of an element below reads. A field added to `Inherited` has
/// to be added here: the size test below is there to say so.
fn inherited_fingerprint(inherited: &Inherited) -> u64 {
    let mut fp = Fingerprint::new();
    fp.bits(&[
        inherited.selectable,
        inherited.inert,
        inherited.pointer_events_none,
        inherited.frameless,
        inherited.above_root,
        inherited.table.collapse,
        inherited.table.caption_bottom,
        inherited.table.hide_empty_cells,
    ]);
    fp.f32(inherited.selection_wash.h);
    fp.f32(inherited.selection_wash.s);
    fp.f32(inherited.selection_wash.l);
    fp.f32(inherited.selection_wash.a);
    // A highlight excludes everything it covers, so which one is all the key needs.
    fp.word(inherited.highlight.as_ref().map_or(0, |highlight| highlight.declaration + 1));
    fp.hash(&inherited.app_region);
    inherited.colors.fingerprint(&mut fp);
    inherited.lengths.fingerprint(&mut fp);
    fp.f64(inherited.font_weight);
    inherited.underline_offset.fingerprint(&mut fp);
    fp.f32(inherited.underline_reach);
    fp.hash(&inherited.computed_variations);
    inherited.font_state.fingerprint(&mut fp);
    fp.hash(&inherited.interpolate_size);
    fp.f64(inherited.table.spacing.horizontal);
    fp.f64(inherited.table.spacing.vertical);
    fp.hash(&inherited.table_row);
    fp.hash(&inherited.table_cell);
    fp.hash(&inherited.container);
    let mut group = inherited.groups.as_deref();
    while let Some(scope) = group {
        group_fingerprint(scope, &mut fp);
        group = scope.parent.as_deref();
    }
    fp.word(0);
    if let Some(scope) = inherited.containers.as_deref() {
        scope.fingerprint(&mut fp);
    }
    fp.word(0);
    if let Some(scope) = inherited.self_container.as_deref() {
        scope.fingerprint(&mut fp);
    }
    fp.finish()
}

fn group_fingerprint(scope: &GroupScope, fp: &mut Fingerprint) {
    fp.str(&scope.name);
    fp.bits(&[scope.state.hovered, scope.state.active, scope.state.focused_within]);
}

/// Why an element cannot be inside a retained subtree, whatever happens around it. Step 5 lifts
/// them one at a time; the order here is only the order of the report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Exclusion {
    /// `onScroll` on an element that scrolls: its paint calls the listener. `onLayout` and the
    /// observers are read from the published boxes after the draw (`observers::frame_sweep`).
    PaintReporter,
    /// It declares a `highlight`, or is inside one.
    Highlight,
    /// A custom element that has no `retain_state`: `<input>` and `<textarea>` are views of their
    /// own that notify, take the input handler while focused and blink; `<code>`, `<markdown>`
    /// and `<diff>` hold scrollers whose offset only gpui knows.
    CustomElement,
    /// An `<svg>` that has not been painted yet or whose raster is still being made.
    Svg,
    VirtualList,
    /// Built by a virtual list in layout, after the render.
    VirtualRow,
    /// Inside a table with collapsed borders, which the table paints over its cells.
    CollapsedTable,
    DisplayContents,
    Anchored,
    Sticky,
    Fixed,
    /// `overflow: scroll` or `auto`.
    Scroller,
    /// `layout` prop: moves by an offset of its own.
    LayoutAnimation,
    /// Built as part of a paragraph's line.
    AtomicInline,
    /// `position: absolute`, an intrinsic width keyword or a natural width: gpui's layout
    /// rewrites the box after it is requested, so the layout nodes it left are not the next
    /// frame's to use.
    UnstableLayout,
}

impl Exclusion {
    const ALL: [Exclusion; 15] = [
        Self::PaintReporter,
        Self::Highlight,
        Self::CustomElement,
        Self::Svg,
        Self::VirtualList,
        Self::VirtualRow,
        Self::CollapsedTable,
        Self::DisplayContents,
        Self::Anchored,
        Self::Sticky,
        Self::Fixed,
        Self::Scroller,
        Self::LayoutAnimation,
        Self::AtomicInline,
        Self::UnstableLayout,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::PaintReporter => "paint-reporter",
            Self::Highlight => "highlight",
            Self::CustomElement => "custom-element",
            Self::Svg => "svg",
            Self::VirtualList => "virtual-list",
            Self::VirtualRow => "virtual-row",
            Self::CollapsedTable => "collapsed-table",
            Self::DisplayContents => "display-contents",
            Self::Anchored => "anchored",
            Self::Sticky => "sticky",
            Self::Fixed => "fixed",
            Self::Scroller => "scroller",
            Self::LayoutAnimation => "layout-animation",
            Self::AtomicInline => "atomic-inline",
            Self::UnstableLayout => "unstable-layout",
        }
    }
}

/// What made an element build differently from the last time, first of these that differs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum Cause {
    New,
    /// JS changed it or something below it.
    Edited,
    Inherited,
    Shape,
    Pointer,
    Focus,
    Scroll,
    Measurement,
    Selection,
    Motion,
    Transition,
    Animation,
    MotionValues,
    InlineMotion,
}

impl Cause {
    const ALL: [Cause; 14] = [
        Self::New,
        Self::Edited,
        Self::Inherited,
        Self::Shape,
        Self::Pointer,
        Self::Focus,
        Self::Scroll,
        Self::Measurement,
        Self::Selection,
        Self::Motion,
        Self::Transition,
        Self::Animation,
        Self::MotionValues,
        Self::InlineMotion,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::New => "new",
            Self::Edited => "edited",
            Self::Inherited => "inherited",
            Self::Shape => "shape",
            Self::Pointer => "pointer",
            Self::Focus => "focus",
            Self::Scroll => "scroll",
            Self::Measurement => "measurement",
            Self::Selection => "selection",
            Self::Motion => "motion",
            Self::Transition => "transition",
            Self::Animation => "animation",
            Self::MotionValues => "motion-values",
            Self::InlineMotion => "inline-motion",
        }
    }
}

/// Motion that changes what an element builds without the tree changing. Bits of `Vol::dynamic`.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Dynamic {
    Motion = 1,
    Transition = 2,
    Animation = 4,
    MotionValues = 8,
    InlineMotion = 16,
}

/// What the build of an element is a function of that the retained tree can see.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Key {
    revision: u64,
    inherited: u64,
    shape: u64,
}

/// The key an element had when it was entered, before its own build added to its shape: all a
/// frame knows of it before it builds it. A deferred build is entered with the key the frame
/// that deferred it computed, since the parent whose shape it is part of is not there.
#[derive(Clone, Copy)]
pub(crate) struct EnterKey(Key);

/// What the build of an element reads that the retained tree cannot see.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Vol {
    pointer: u16,
    inline_pointer: u64,
    focus: u16,
    scroll: u64,
    measurement: u64,
    selection: u64,
    dynamic: u8,
}

impl Vol {
    /// The first thing that differs, if any. Motion that is running differs from itself.
    fn cause_against(&self, before: &Vol) -> Option<Cause> {
        if self.pointer != before.pointer || self.inline_pointer != before.inline_pointer {
            return Some(Cause::Pointer);
        }
        if self.focus != before.focus {
            return Some(Cause::Focus);
        }
        if self.scroll != before.scroll {
            return Some(Cause::Scroll);
        }
        if self.measurement != before.measurement {
            return Some(Cause::Measurement);
        }
        if self.selection != before.selection {
            return Some(Cause::Selection);
        }
        let dynamic = self.dynamic | before.dynamic;
        [
            (Dynamic::Motion, Cause::Motion),
            (Dynamic::Transition, Cause::Transition),
            (Dynamic::Animation, Cause::Animation),
            (Dynamic::MotionValues, Cause::MotionValues),
            (Dynamic::InlineMotion, Cause::InlineMotion),
        ]
        .into_iter()
        .find(|(kind, _)| dynamic & *kind as u8 != 0)
        .map(|(_, cause)| cause)
    }
}

/// What the last build recorded about an element.
#[derive(Clone)]
struct Record {
    key: Key,
    enter: Key,
    vol: Vol,
    digest: u64,
    eligible: bool,
    /// Counts the frames that built the element on their own: a frame that finds the element
    /// changed builds it, and what gpui recorded of it before is of another build. See `token`.
    epoch: u64,
    /// What gpui's retained element is keyed by: equal for two frames only when the element was
    /// entered with the same key and has not been built on its own in between.
    token: u64,
    lift_z: Option<i32>,
    forms_context: bool,
    /// Elements in the subtree.
    size: u32,
    subtree: crate::layout::cull::Subtree,
    effects: Option<Rc<[BuildEffect]>>,
    /// Whether gpui has a recording of the subtree to replay: it asked for the build, or the frame
    /// before skipped the subtree.
    seasoned: bool,
}

const NO_PARENT: u32 = u32::MAX;

/// Where each reason is counted: `perf_counters` keeps one number per slot.
const EXCLUDED: usize = 0;
const UNDER: usize = EXCLUDED + Exclusion::ALL.len();
const CHANGED: usize = UNDER + Exclusion::ALL.len();
const SPINE: usize = CHANGED + Cause::ALL.len();
const LIFTS: usize = SPINE + 1;
const MOVED: usize = LIFTS + 1;
const OVERSIZED: usize = MOVED + 1;
pub(crate) const RETAINED: usize = OVERSIZED + 1;
pub(crate) const SLOTS: usize = RETAINED + 1;

/// What a counted slot is called in a report.
pub(crate) fn slot_name(slot: usize) -> String {
    match slot {
        EXCLUDED..UNDER => format!("excluded:{}", Exclusion::ALL[slot - EXCLUDED].name()),
        UNDER..CHANGED => format!("under:{}", Exclusion::ALL[slot - UNDER].name()),
        CHANGED..SPINE => format!("changed:{}", Cause::ALL[slot - CHANGED].name()),
        SPINE => "spine".to_owned(),
        LIFTS => "lifts".to_owned(),
        MOVED => "moved:scroll".to_owned(),
        OVERSIZED => "oversized:scroll".to_owned(),
        RETAINED => "retained".to_owned(),
        _ => unreachable!("no slot {slot}"),
    }
}

/// A scroller, or other clip scrolled by script, that an element is inside of. A subtree inside it
/// that is skipped is recorded to be put elsewhere when the scroller moves it, see
/// [`gpui::Retained::relocatable`].
#[derive(Clone, Copy)]
pub(crate) struct Scroller {
    pub window: gpui::WindowId,
    /// The size of its box, which bounds what a root is worth recording.
    pub viewport: gpui::Size<gpui::Pixels>,
    /// How far it scrolls with one step of the viewport: its height, its width, or the larger.
    pub reach: gpui::Pixels,
}

/// Roots of at most this many viewports (per axis) are recorded to be moved by their scroller. A
/// bigger one is not a root: it is built, and what is below it is judged by itself.
const ROOT_VIEWPORTS: f32 = 2.0;

/// How far past the clip of its scroller a root is recorded, in viewports of the scroller. The
/// recording of a root is put elsewhere while the scroll since it was made is within it, so a
/// bigger one is built fewer times and replays more roots that are not shown.
const OVERSCAN_VIEWPORTS: f32 = 0.5;

impl Scroller {
    /// The scroller `handle` is, if its content can move: it scrolls for the user or by script
    /// along an axis, and it has been laid out.
    pub(crate) fn of(handle: &gpui::ScrollHandle, window: gpui::WindowId) -> Option<Self> {
        let viewport = handle.padding_box().size;
        if viewport.width <= gpui::px(0.0) || viewport.height <= gpui::px(0.0) {
            return None;
        }
        let (offset, max, user) = (handle.offset(), handle.max_offset(), handle.user_scrollable());
        let moves_x = (user.x && max.x > gpui::px(0.0)) || offset.x != gpui::px(0.0);
        let moves_y = (user.y && max.y > gpui::px(0.0)) || offset.y != gpui::px(0.0);
        let reach = match (moves_x, moves_y) {
            (false, false) => return None,
            (true, false) => viewport.width,
            (false, true) => viewport.height,
            (true, true) => viewport.width.max(viewport.height),
        };
        Some(Self { window, viewport, reach })
    }

    /// Whether the element's last box is small enough to be recorded to move: one that has no box
    /// yet (it never painted) is not.
    fn holds(&self, id: u64) -> bool {
        self.fits(id) == Some(true)
    }

    /// Whether the element's last box is too big for a root, if it has one.
    fn too_big(&self, id: u64) -> bool {
        self.fits(id) == Some(false)
    }

    fn fits(&self, id: u64) -> Option<bool> {
        crate::automation::get_bounds(self.window, id).map(|bounds| {
            bounds.width <= f64::from(f32::from(self.viewport.width) * ROOT_VIEWPORTS)
                && bounds.height <= f64::from(f32::from(self.viewport.height) * ROOT_VIEWPORTS)
        })
    }

    fn overscan(&self) -> gpui::Pixels {
        self.reach * OVERSCAN_VIEWPORTS
    }
}

/// An element a frame did not build, standing in for the record of its last build.
struct Skipped {
    size: u32,
    digest: u64,
}

struct Node {
    id: u64,
    parent: u32,
    /// One past the last node below this one.
    end: u32,
    key: Key,
    enter: Key,
    vol: Vol,
    exclusion: Option<Exclusion>,
    lift_z: Option<i32>,
    forms_context: bool,
    /// Something above scrolled, so the box this one was recorded at is not where it is.
    moved: bool,
    /// The scroller around it, if it is one gpui can move recordings for.
    scroller: Option<Scroller>,
    /// Only with `verify`: a hash of what the build made of the element itself.
    digest: u64,
    subtree: crate::layout::cull::Subtree,
    /// The build effects noted since the element was entered, and until it was left.
    effects_start: usize,
    effects_end: usize,
    skipped: Option<Skipped>,
}

/// What a node adds up to with what is below it, filled in bottom-up when the build is done.
#[derive(Clone, Copy, Default)]
struct Below {
    changed: Option<Cause>,
    /// The scroll offset of a scroller moved, which moves its children.
    scrolled: bool,
    changed_below: bool,
    /// The first excluded element below, in document order.
    excluded: Option<Exclusion>,
    /// Something below is lifted out of the elements between it and here.
    lifts: bool,
    digest: u64,
    size: u32,
}

/// The decision for the last build, for tests.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Summary {
    pub roots: Vec<u64>,
    pub elements_in_roots: usize,
    pub elements: usize,
}

/// What the build reads that the retained tree cannot show, as the frame found it. The elements
/// that differ from the frame before, and their ancestors, are the spine a frame builds; what is
/// off it can be skipped.
pub(crate) struct Sources<'a> {
    pub tree: &'a crate::retained_tree::RetainedTree,
    pub hovered: &'a [u64],
    pub active: &'a [u64],
    pub focus: &'a crate::focus::FocusStyles,
    pub focusable: &'a HashMap<u64, gpui::FocusHandle>,
    /// Whether the last input was the keyboard's, which `:focus-visible` follows.
    pub keyboard: bool,
    pub scrolls: &'a HashMap<u64, gpui::ScrollHandle>,
    /// The elements whose measurement changed since the frame before.
    pub measured: Vec<u64>,
    /// The elements whose accessibility links changed since the frame before.
    pub links: Vec<u64>,
    /// The state each custom element says it keeps for itself, see `CustomElement::retain_state`.
    pub customs: Vec<(u64, Option<u64>)>,
    /// The elements with a motion that is running.
    pub motion: Vec<u64>,
    pub selection: u64,
    /// The pass of the draw the frame is built in, counted from 1, or 0 for a frame built outside
    /// a draw. Only the first pass of a draw is a frame to the back-off.
    pub pass: usize,
    /// How many subtrees gpui has built late (it could not use the recording it had of them) and
    /// how many it has put elsewhere than they were recorded, since the window was made.
    pub lazy_builds: u64,
    pub relocations: u64,
    /// The frame is built again to be compared with the one that was shown: it is the same frame,
    /// and its roots are planned as that one's were.
    pub reference: bool,
}

/// What the sources were at the frame before, to tell what changed.
#[derive(Default)]
struct Seen {
    known: bool,
    hovered: Vec<u64>,
    active: Vec<u64>,
    within: Arc<[u64]>,
    visible: Option<u64>,
    dim: Option<Arc<[u64]>>,
    keyboard: bool,
    scrolls: FxHashMap<u64, (u32, u32)>,
    customs: FxHashMap<u64, Option<u64>>,
    motion: FxHashSet<u64>,
    selection: u64,
}

/// What the frame found different from the frame before.
#[derive(Default)]
struct Deltas {
    /// Nothing about the frame before holds (the first frame, a new selection).
    all: bool,
    /// The elements that changed, and their ancestors.
    spine: FxHashSet<u64>,
    /// The elements whose scroll offset moved, which moves what is below them.
    scrolled: FxHashSet<u64>,
    /// Whether a scroller that the frame before had is somewhere else, as opposed to new.
    scrolling: bool,
}

/// The most frames that follow a recorded frame which found nothing a later frame could skip,
/// before one is recorded again to look.
const BACKOFF_CAP: u32 = 8;

/// Recording a frame costs about 5% of building it. A page with nothing a later frame could skip
/// (a scroller whose rows move with every frame, boxes too small to be a subtree) gains nothing
/// from it, so after a recorded frame that found no such element the frames that follow are not
/// recorded, in stretches that double up to [`BACKOFF_CAP`], and one frame in nine looks again.
/// Recording starts again at once when the scrollers stop moving, since the page is then still.
#[derive(Default)]
struct Backoff {
    /// Frames still to go unrecorded.
    rest: u32,
    /// The length of the next stretch: 0 while recorded frames find something to skip.
    gap: u32,
    /// Whether the frame being built is one of the unrecorded.
    resting: bool,
    /// Whether a scroller moved in the last frame, recorded or not.
    scrolling: bool,
    /// How many elements the tree had when the stretch began: a page that gains or loses
    /// elements is not the page that had nothing to skip.
    elements: usize,
}

/// Whether the roots a scroller moved were put elsewhere or built again, which tells a frame
/// whether to plan on it: a scroller whose rows gpui cannot move (a page whose rows keep a
/// position, or sit under a rounded clip) is no better off with its rows skipped than built, and
/// is worse for the recording that follows.
#[derive(Default)]
struct Relocation {
    /// Roots the last first pass planned to have moved.
    planned: u32,
    /// `Sources::lazy_builds` and `Sources::relocations` at the first pass before.
    lazy_builds: u64,
    relocations: u64,
    /// Frames still to go without planning to move roots.
    rest: u32,
    /// The length of the next stretch: 0 while the roots that were planned to move did.
    gap: u32,
    /// Whether the frame being built plans to move roots.
    resting: bool,
}

pub(crate) struct Retain {
    /// `GPUIX_RETAIN_OFF` stops recording, to time a build without it.
    enabled: bool,
    /// Whether skipping was asked for, which `enabled` can still turn off.
    planning_wanted: bool,
    backoff: Backoff,
    relocation: Relocation,
    /// A subtree gpui asked for is being built inside one that is recorded to move: what is in it
    /// is built, since the recording of a subtree in another cannot be put elsewhere on its own.
    inside_relocatable: bool,
    /// The pass of the draw the frame being built is in, see [`Sources::pass`].
    pass: usize,
    /// How many elements the tree has in the frame being built.
    elements: usize,
    #[cfg(test)]
    frame_log: Vec<bool>,
    /// Whether a frame skips the building of what the sources say did not change, and hands it
    /// to gpui as a retained subtree. Off, the elements are only recorded and judged, which is
    /// what the tests of the judgement read.
    planning: bool,
    /// The exclusions to ignore, by `Exclusion as u32`: what the numbers would be if they were
    /// lifted. For measuring which to lift first; nothing reuses anything.
    lifted: u32,
    verify: bool,
    /// The smallest subtree that is skipped.
    min_root: u32,
    open: bool,
    /// A subtree gpui asked for after the frame was built is being built.
    deferred: bool,
    /// Above zero while a virtual list builds a row, which is outside any render.
    rows: u32,
    selection: u64,
    nodes: Vec<Node>,
    stack: Vec<u32>,
    below: Vec<Below>,
    retained: Vec<bool>,
    moved: Vec<bool>,
    records: FxHashMap<u64, Record>,
    /// The elements whose last build found a motion, transition or animation running.
    dynamic: FxHashSet<u64>,
    /// `RetainedTree::removals` when `records` and `dynamic` last dropped the destroyed elements.
    pruned_at: u64,
    seen: Seen,
    deltas: Deltas,
    summary: Summary,
    mismatches: Vec<String>,
    /// What `getRetainDebug` reports, filled only with `GPUIX_RETAIN_DEBUG`.
    debug: debug::Debug,
}

/// What `Retain::enter` is told about the element about to be built.
pub(crate) struct Entry<'a> {
    pub id: u64,
    pub revision: u64,
    pub inherited: &'a Inherited,
    /// Its index among its parent's children, the last step of its document path.
    pub at: u32,
    pub stacking_root: bool,
    pub stacking_item: bool,
    pub atomic_root: bool,
    /// `Some` for a child spliced out of `display: contents` elements: the key of the styles they
    /// hand down (`layout::contents::overlay_key`).
    pub contents_overlay: Option<u64>,
    pub anchor_scope: bool,
    /// What an ancestor's `text-box-trim` asks of the ends of the element's lines.
    pub text_box_trim: gpui::TextBoxTrim,
    /// Whether nothing around the element is waiting for something from its build that a skipped
    /// build could not give (the baseline of a box, the text of a line).
    pub plain: bool,
    /// For an element that is a query container or has `@container` blocks: the size layout last
    /// gave the container (`ContainerStore::key`). The scope the element builds for the elements
    /// below it is made from it, and nothing above the element has it.
    pub container: Option<u64>,
    /// Whether the element has a layout node of its own: a table row or row group does not, and
    /// has no layout result for a retained element to be laid out as.
    pub boxed: bool,
    /// The key of a subtree that was deferred, which the frame that deferred it computed.
    pub prekeyed: Option<EnterKey>,
    /// The scroller the element is inside of.
    pub scroller: Option<Scroller>,
}

/// An element that was skipped, and what its parent and the frame need of it.
pub(crate) struct Skip {
    pub token: u64,
    pub enter: EnterKey,
    pub lift_z: Option<i32>,
    pub subtree: crate::layout::cull::Subtree,
    pub effects: Option<Rc<[BuildEffect]>>,
    /// How far past the clip of its scroller the subtree is recorded, if it is to be put
    /// elsewhere when the scroller moves it.
    pub overscan: Option<gpui::Pixels>,
}

/// Why an element that is entered is built and not skipped; `getRetainDebug` reports the ones
/// that a record said could have been.
#[derive(Clone, Copy)]
enum Kept {
    PlanningOff,
    Everything,
    NotPlain,
    NotBoxed,
    Prekeyed,
    Excluded,
    Moved,
    Spine,
    NoRecord,
    Ineligible,
    TooSmall,
    Revision,
    Inherited,
    Shape,
}

impl Kept {
    fn name(self) -> &'static str {
        match self {
            Self::PlanningOff => "planning-off",
            Self::Everything => "deltas-all",
            Self::NotPlain => "not-plain",
            Self::NotBoxed => "not-boxed",
            Self::Prekeyed => "prekeyed",
            Self::Excluded => "excluded-now",
            Self::Moved => "moved",
            Self::Spine => "spine",
            Self::NoRecord => "no-record",
            Self::Ineligible => "ineligible",
            Self::TooSmall => "too-small",
            Self::Revision => "enter-revision",
            Self::Inherited => "enter-inherited",
            Self::Shape => "enter-shape",
        }
    }
}

/// Elements below this many are not worth a retained element of their own. Measured on the
/// pages of the FF demo (showcase, changelog, command menu, hover, introduction): a one-element
/// root (a text, an icon, an empty box) costs less to replay than to build, 0.5 to 0.8 ms of 3 to
/// 6 ms per idle frame less than a bound of 2. `GPUIX_RETAIN_MIN_ROOT` sets it, to measure.
const MIN_ROOT: u32 = 1;

impl Retain {
    pub(crate) fn new() -> Self {
        let enabled = std::env::var_os("GPUIX_RETAIN_OFF").is_none();
        Self {
            enabled,
            planning_wanted: !cfg!(test),
            backoff: Backoff::default(),
            relocation: Relocation::default(),
            inside_relocatable: false,
            pass: 1,
            elements: 0,
            #[cfg(test)]
            frame_log: Vec::new(),
            planning: enabled && !cfg!(test),
            lifted: 0,
            verify: cfg!(test) || std::env::var_os("GPUIX_RETAIN_VERIFY").is_some(),
            min_root: std::env::var("GPUIX_RETAIN_MIN_ROOT")
                .ok()
                .and_then(|min| min.parse().ok())
                .unwrap_or(MIN_ROOT),
            open: false,
            deferred: false,
            rows: 0,
            selection: 0,
            nodes: Vec::new(),
            stack: Vec::new(),
            below: Vec::new(),
            retained: Vec::new(),
            moved: Vec::new(),
            records: FxHashMap::default(),
            dynamic: FxHashSet::default(),
            pruned_at: 0,
            seen: Seen::default(),
            deltas: Deltas::default(),
            summary: Summary::default(),
            mismatches: Vec::new(),
            debug: debug::Debug::from_env(),
        }
    }

    /// With `verify` every element hashes what it built, and an element that is judged unchanged
    /// has to hash as it did. A mismatch is a hole in the key. Costs a style formatted per element.
    /// A frame that skips what it judged unchanged has no build to hash, so it is gpui that
    /// compares the frame with one built whole, see [`Self::verifies_frames`].
    pub(crate) fn set_verify(&mut self, verify: bool) {
        self.verify = verify;
    }

    /// Whether gpui builds each frame that replayed something once more, whole, and compares the
    /// two.
    pub(crate) fn verifies_frames(&self) -> bool {
        self.verify && self.planning
    }

    #[cfg(test)]
    pub(crate) fn set_min_root(&mut self, min_root: u32) {
        self.min_root = min_root;
    }

    /// As if the roots that frames planned to have a scroller move were found built again, for
    /// as long as a test draws.
    #[cfg(test)]
    pub(crate) fn stop_relocating(&mut self) {
        self.relocation.rest = u32::MAX;
    }

    /// Turns the skipping of unchanged subtrees on or off, which the next frame starts from
    /// nothing for.
    pub(crate) fn set_planning(&mut self, planning: bool) {
        self.planning_wanted = planning;
        self.planning = planning && self.enabled;
        self.forget();
    }

    /// Turns recording on or off, as `GPUIX_RETAIN_OFF` does for a whole run. The next frame
    /// starts from nothing.
    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.planning = enabled && self.planning_wanted;
        self.forget();
    }

    /// Drops what the frames before recorded: the next recorded frame builds everything.
    fn forget(&mut self) {
        // The offsets stay, for the next frame to tell whether a scroller moved since.
        let scrolls = std::mem::take(&mut self.seen.scrolls);
        self.seen = Seen { scrolls, ..Seen::default() };
        self.records.clear();
        self.dynamic.clear();
        self.backoff = Backoff::default();
    }

    /// Counts as if the exclusions named here, by `Exclusion::name`, were lifted.
    pub(crate) fn lift(&mut self, names: &[String]) -> Result<(), String> {
        let mut lifted = 0;
        for name in names {
            let reason = Exclusion::ALL
                .into_iter()
                .find(|reason| reason.name() == name)
                .ok_or_else(|| format!("no exclusion called {name}"))?;
            lifted |= 1 << reason as u32;
        }
        self.lifted = lifted;
        // What was recorded under other exclusions says nothing about these.
        self.records.clear();
        Ok(())
    }

    fn unless_lifted(&self, reason: Exclusion) -> Option<Exclusion> {
        (self.lifted & (1 << reason as u32) == 0).then_some(reason)
    }

    /// Whether each frame since the last call was recorded, a draw's later passes not counted.
    #[cfg(test)]
    pub(crate) fn take_frame_log(&mut self) -> Vec<bool> {
        std::mem::take(&mut self.frame_log)
    }

    pub(crate) fn record_counts(&self) -> [usize; 2] {
        [self.records.len(), self.dynamic.len()]
    }

    pub(crate) fn mismatches(&self) -> &[String] {
        &self.mismatches
    }

    #[cfg(test)]
    pub(crate) fn summary(&self) -> &Summary {
        &self.summary
    }

    /// Starts the build of a frame. `selection` in `sources` is the identity of the selected
    /// text, 0 for none: text that is in a selection that moved paints differently.
    pub(crate) fn begin_frame(&mut self, sources: Sources) {
        self.prune(sources.tree);
        self.pass = sources.pass;
        if sources.pass == 1 && !sources.reference {
            self.learn_relocation(sources.lazy_builds, sources.relocations);
        }
        self.elements = sources.tree.elements.len();
        self.backoff.resting = self.enabled && self.rests(&sources);
        #[cfg(test)]
        if sources.pass == 1 && self.enabled {
            self.frame_log.push(!self.backoff.resting);
        }
        self.open = self.enabled && !self.backoff.resting;
        self.deferred = false;
        self.selection = sources.selection;
        self.nodes.clear();
        self.stack.clear();
        self.debug.begin_frame(sources.pass);
        crate::tape::clear_build_effects();
        if self.open && self.planning {
            self.find_deltas(&sources);
        }
    }

    /// Judges the roots the frame before planned to have moved, by how many gpui had to build
    /// anyway, and decides whether this frame plans on moving any.
    fn learn_relocation(&mut self, lazy_builds: u64, relocations: u64) {
        let relocation = &mut self.relocation;
        let built = lazy_builds.wrapping_sub(relocation.lazy_builds);
        let moved = relocations.wrapping_sub(relocation.relocations);
        relocation.lazy_builds = lazy_builds;
        relocation.relocations = relocations;
        if relocation.planned > 0 {
            // A root put elsewhere is a build that was not made, and one built late costs about a
            // fifth of a build more than building it in the frame: a few that are put are worth
            // many that are not.
            if moved * 4 < built {
                relocation.gap = (relocation.gap * 2).clamp(1, BACKOFF_CAP);
                relocation.rest = relocation.gap;
            } else {
                relocation.gap = 0;
            }
        }
        relocation.planned = 0;
        relocation.resting = relocation.rest > 0;
        relocation.rest = relocation.rest.saturating_sub(1);
    }

    /// Forgets the elements the tree destroyed. A record of one is never read again (ids are not
    /// reused), but it would stay for as long as the window lives, so a page that mounts and
    /// unmounts rows grows it for good. Looks only when the tree destroyed something.
    fn prune(&mut self, tree: &crate::retained_tree::RetainedTree) {
        if tree.removals() == self.pruned_at {
            return;
        }
        self.pruned_at = tree.removals();
        self.records.retain(|id, _| tree.elements.contains_key(id));
        self.dynamic.retain(|id| tree.elements.contains_key(id));
    }

    /// Whether the frame about to be built is one that is not recorded, see [`Backoff`].
    fn rests(&mut self, sources: &Sources) -> bool {
        if !self.backs_off() {
            self.backoff = Backoff::default();
            return false;
        }
        // The passes after the first of a draw see the scrollers where the first left them, and
        // are not frames of their own: they follow the first, or the stretch it began.
        if sources.pass != 1 {
            return self.backoff.resting || self.backoff.rest > 0;
        }
        if self.backoff.rest == 0 {
            return false;
        }
        if self.backoff.elements != self.elements {
            self.backoff = Backoff::default();
            return false;
        }
        let scrolls = scroll_bits(sources.scrolls);
        let moving = scrolls != self.seen.scrolls;
        self.seen.scrolls = scrolls;
        let backoff = &mut self.backoff;
        let stopped = backoff.scrolling && !moving;
        backoff.scrolling = moving;
        if stopped {
            backoff.rest = 0;
            return false;
        }
        backoff.rest -= 1;
        true
    }

    /// A verified run records every frame, since it is the recording that it checks.
    fn backs_off(&self) -> bool {
        self.planning && !self.verify
    }

    /// What changed in `sources` since the frame before, as the elements that differ and their
    /// ancestors.
    fn find_deltas(&mut self, sources: &Sources) {
        let mut changed: Vec<u64> = Vec::new();
        let seen = &mut self.seen;
        let all = !seen.known || seen.selection != sources.selection;

        differ(&seen.hovered, sources.hovered, &mut changed);
        differ(&seen.active, sources.active, &mut changed);
        differ(&seen.within, &sources.focus.within, &mut changed);
        // The target's ancestors stay in the chain, and `:has(:hover)` changes for the one the
        // target moves to or from.
        if seen.hovered.first() != sources.hovered.first() {
            changed.extend(seen.hovered.first().into_iter().chain(sources.hovered.first()));
        }
        if seen.active.first() != sources.active.first() {
            changed.extend(seen.active.first().into_iter().chain(sources.active.first()));
        }
        if seen.within.first() != sources.focus.within.first() {
            changed.extend(seen.within.first().into_iter().chain(sources.focus.within.first()));
        }
        if seen.visible != sources.focus.visible {
            changed.extend(seen.visible.into_iter().chain(sources.focus.visible));
        }
        if seen.keyboard != sources.keyboard {
            changed.extend(sources.focus.within.first());
        }
        match (&seen.dim, &sources.focus.dim_path) {
            (None, None) => {}
            (Some(before), Some(now)) => differ(before, now, &mut changed),
            // Every focusable element that is not on the path dims, or stops dimming.
            _ => changed.extend(sources.focusable.keys()),
        }

        let mut scrolled = FxHashSet::default();
        let scrolls = scroll_bits(sources.scrolls);
        let mut scrolling = false;
        for (id, bits) in &scrolls {
            match seen.scrolls.get(id) {
                Some(before) if before == bits => continue,
                Some(_) => scrolling = true,
                None => {}
            }
            changed.push(*id);
            scrolled.insert(*id);
        }
        changed.extend(seen.scrolls.keys().filter(|id| !scrolls.contains_key(id)));

        let mut customs = FxHashMap::default();
        for (id, state) in &sources.customs {
            if seen.customs.get(id) != Some(state) {
                changed.push(*id);
            }
            customs.insert(*id, *state);
        }
        changed.extend(seen.customs.keys().filter(|id| !customs.contains_key(id)));

        self.debug.sources("pointer-focus-scroll-custom", changed.iter().copied());
        self.debug.sources("motion-running", sources.motion.iter().copied());
        self.debug.sources("motion-just-ended", seen.motion.iter().copied().filter(|id| !sources.motion.contains(id)));
        self.debug.sources("dynamic-last-build", self.dynamic.iter().copied());
        self.debug.sources("measurement", sources.measured.iter().copied());
        self.debug.sources("link", sources.links.iter().copied());
        let motion: FxHashSet<u64> = sources.motion.iter().copied().collect();
        changed.extend(motion.union(&seen.motion));
        changed.extend(&sources.measured);
        changed.extend(&sources.links);
        changed.extend(&self.dynamic);

        seen.known = true;
        seen.hovered = sources.hovered.to_vec();
        seen.active = sources.active.to_vec();
        seen.within = sources.focus.within.clone();
        seen.visible = sources.focus.visible;
        seen.dim = sources.focus.dim_path.clone();
        seen.keyboard = sources.keyboard;
        seen.scrolls = scrolls;
        seen.customs = customs;
        seen.motion = motion;
        seen.selection = sources.selection;

        let mut spine = FxHashSet::default();
        for id in changed {
            let mut at = Some(id);
            while let Some(id) = at {
                if !spine.insert(id) {
                    break;
                }
                at = sources.tree.elements.get(&id).and_then(|element| element.parent);
            }
        }
        self.deltas = Deltas { all, spine, scrolled, scrolling };
    }

    /// Starts the build of a subtree that gpui asked for after the frame was built, because the
    /// frame before did not leave it recorded as the same, or does not lay out where it was.
    pub(crate) fn begin_deferred(&mut self, inside_relocatable: bool) {
        self.open = self.enabled && !self.backoff.resting;
        self.deferred = true;
        self.inside_relocatable = inside_relocatable;
        self.nodes.clear();
        self.stack.clear();
    }

    pub(crate) fn finish_deferred(&mut self) {
        self.debug.deferred(self.nodes.first(), self.nodes.len());
        let _ = self.finish_session();
        self.deferred = false;
        self.inside_relocatable = false;
    }

    /// Whether the element being built is recorded, so the caller can skip work only the record needs.
    pub(crate) fn recording(&self) -> bool {
        self.open && self.rows == 0
    }

    pub(crate) fn enter_row(&mut self) {
        self.rows += 1;
    }

    pub(crate) fn leave_row(&mut self) {
        self.rows -= 1;
    }

    /// Enters the element about to be built, or decides to skip it: when the frame found nothing
    /// that its last build read different, and gpui can be handed the subtree to keep. A skipped
    /// element is left entered and exited, and the caller builds a retained element instead.
    pub(crate) fn enter(&mut self, entry: Entry) -> Option<Skip> {
        if !self.enabled {
            return None;
        }
        if self.rows > 0 {
            crate::perf_counters::retain_slot(EXCLUDED + Exclusion::VirtualRow as usize, 1);
            return None;
        }
        if !self.open {
            return None;
        }
        let parent = self.stack.last().copied().unwrap_or(NO_PARENT);
        let enter = match entry.prekeyed {
            Some(prekeyed) => prekeyed.0,
            None => {
                let mut shape = Fingerprint::new();
                shape.word(entry.id);
                shape.word(u64::from(entry.at));
                shape.word(self.nodes.get(parent as usize).map_or(0, |parent| parent.key.shape));
                shape.word(entry.container.map_or(0, |size| size | 1));
                shape.word(entry.contents_overlay.map_or(0, |key| key | 1));
                shape.bits(&[
                    entry.stacking_root,
                    entry.stacking_item,
                    entry.atomic_root,
                    entry.anchor_scope,
                    entry.text_box_trim.start,
                    entry.text_box_trim.end,
                ]);
                Key {
                    revision: entry.revision,
                    inherited: inherited_fingerprint(entry.inherited),
                    shape: shape.finish(),
                }
            }
        };
        let moved = self.nodes.get(parent as usize).is_some_and(|parent| {
            parent.moved || self.deltas.scrolled.contains(&parent.id)
        });
        let exclusion = if entry.atomic_root {
            self.unless_lifted(Exclusion::AtomicInline)
        } else if entry.anchor_scope {
            self.unless_lifted(Exclusion::Anchored)
        } else if entry.inherited.highlight.is_some() {
            self.unless_lifted(Exclusion::Highlight)
        } else if entry.inherited.table.collapse {
            self.unless_lifted(Exclusion::CollapsedTable)
        } else {
            None
        };
        let mut node = Node {
            id: entry.id,
            parent,
            end: 0,
            key: enter,
            enter,
            vol: Vol::default(),
            exclusion,
            lift_z: None,
            forms_context: false,
            moved,
            scroller: entry.scroller,
            digest: 0,
            subtree: crate::layout::cull::Subtree::empty(),
            effects_start: crate::tape::build_effects_len(),
            effects_end: 0,
            skipped: None,
        };
        let at = self.nodes.len() as u32;
        match self.skippable(&node, &entry) {
            Ok(record) => {
                node.key = record.key;
                node.vol = record.vol;
                node.lift_z = record.lift_z;
                node.forms_context = record.forms_context;
                node.subtree = record.subtree;
                node.end = at + 1;
                node.skipped = Some(Skipped { size: record.size, digest: record.digest });
                crate::perf_counters::elements_skipped(u64::from(record.size));
                let overscan = node
                    .scroller
                    .filter(|scroller| scroller.holds(node.id))
                    .map(|scroller| scroller.overscan());
                let planned = overscan.is_some() && moved && record.seasoned;
                let skip = Skip {
                    token: record.token,
                    enter: EnterKey(enter),
                    lift_z: record.lift_z,
                    subtree: record.subtree,
                    effects: record.effects.clone(),
                    overscan,
                };
                self.nodes.push(node);
                self.relocation.planned += u32::from(planned);
                if let Some(record) = self.records.get_mut(&entry.id) {
                    record.seasoned = true;
                }
                return Some(skip);
            }
            Err(kept) => self.debug_kept(kept, node.id),
        }
        self.stack.push(at);
        self.nodes.push(node);
        None
    }

    /// Whether a move of the element by its scroller is not a reason to build it: it is recorded
    /// to be moved, and frames are not resting from planning on it.
    fn relocates(&self, node: &Node) -> bool {
        !self.relocation.resting && node.scroller.is_some_and(|scroller| scroller.holds(node.id))
    }

    /// The record of the last build of the element if the frame can leave it unbuilt, or the
    /// first reason it cannot.
    fn skippable(&self, node: &Node, entry: &Entry) -> Result<&Record, Kept> {
        if !self.planning || self.lifted != 0 {
            return Err(Kept::PlanningOff);
        }
        if self.deltas.all {
            return Err(Kept::Everything);
        }
        if !entry.plain {
            return Err(Kept::NotPlain);
        }
        if !entry.boxed {
            return Err(Kept::NotBoxed);
        }
        if entry.prekeyed.is_some() {
            return Err(Kept::Prekeyed);
        }
        if node.exclusion.is_some() {
            return Err(Kept::Excluded);
        }
        // A root under a scroller is one gpui can move or one that is built: a box too big to be
        // recorded to move is not a root, and what is moved by a scroller is built unless it can be.
        // What a subtree built late holds is built with it, since the recording of a subtree in
        // another cannot be put elsewhere on its own.
        if self.inside_relocatable
            || node.scroller.is_some_and(|scroller| scroller.too_big(node.id))
            || (node.moved && !self.relocates(node))
        {
            return Err(Kept::Moved);
        }
        if self.deltas.spine.contains(&node.id) {
            return Err(Kept::Spine);
        }
        let record = self.records.get(&node.id).ok_or(Kept::NoRecord)?;
        if !record.eligible {
            return Err(Kept::Ineligible);
        }
        if record.size < self.min_root {
            return Err(Kept::TooSmall);
        }
        if record.enter.revision != node.enter.revision {
            return Err(Kept::Revision);
        }
        if record.enter.inherited != node.enter.inherited {
            return Err(Kept::Inherited);
        }
        if record.enter.shape != node.enter.shape {
            return Err(Kept::Shape);
        }
        Ok(record)
    }

    pub(crate) fn exit(&mut self) {
        if self.rows > 0 || !self.open {
            return;
        }
        // A skipped element was never pushed.
        let at = self.stack.pop().expect("exit without enter");
        let end = self.nodes.len() as u32;
        let node = &mut self.nodes[at as usize];
        node.end = end;
        node.effects_end = crate::tape::build_effects_len();
    }

    fn top(&mut self) -> Option<&mut Node> {
        if self.rows > 0 {
            return None;
        }
        let at = *self.stack.last()?;
        Some(&mut self.nodes[at as usize])
    }

    /// The first reason wins: it is the one the element is counted under.
    pub(crate) fn exclude(&mut self, reason: Exclusion) {
        let Some(reason) = self.unless_lifted(reason) else { return };
        if let Some(node) = self.top() {
            node.exclusion.get_or_insert(reason);
        }
    }

    /// What of the shape of the element's build the retained tree does not say.
    pub(crate) fn shape(&mut self, word: u64) {
        if let Some(node) = self.top() {
            let mut fp = Fingerprint::new();
            fp.word(node.key.shape);
            fp.word(word);
            node.key.shape = fp.finish();
        }
    }

    pub(crate) fn pointer(&mut self, states: crate::events::state_styles::PointerStates) {
        if let Some(node) = self.top() {
            node.vol.pointer |= pointer_bits(states);
        }
    }

    /// The states of the inline boxes of a paragraph the element holds, see [`mix_pointer`].
    pub(crate) fn inline_pointer(&mut self, mixed: u64) {
        if let Some(node) = self.top() {
            node.vol.inline_pointer = node.vol.inline_pointer.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ mixed;
        }
    }

    pub(crate) fn focus(&mut self, focus: &crate::focus::FocusStyles, id: u64, has_handle: bool) {
        if let Some(node) = self.top() {
            node.vol.focus |= focus_bits(focus, id, has_handle);
        }
    }

    /// A focused handle that `:focus-visible` follows the keyboard for.
    pub(crate) fn focus_visible_own(&mut self, own: bool) {
        if let Some(node) = self.top() {
            node.vol.focus |= u16::from(own) << 8;
        }
    }

    pub(crate) fn scroll(&mut self, offset: gpui::Point<gpui::Pixels>) {
        if let Some(node) = self.top() {
            let mut fp = Fingerprint::new();
            fp.f32(f32::from(offset.x));
            fp.f32(f32::from(offset.y));
            node.vol.scroll = fp.finish() | 1;
        }
    }

    pub(crate) fn measurement(&mut self, measurement: Option<&crate::motion::measure::Measurement>) {
        let Some(measurement) = measurement else { return };
        if let Some(node) = self.top() {
            let mut fp = Fingerprint::new();
            measurement.fingerprint(&mut fp);
            node.vol.measurement = fp.finish() | 1;
        }
    }

    pub(crate) fn dynamic(&mut self, kind: Dynamic) {
        if let Some(node) = self.top() {
            node.vol.dynamic |= kind as u8;
        }
    }

    /// The element holds text of its own, which a selection paints over.
    pub(crate) fn text(&mut self) {
        let selection = self.selection;
        if let Some(node) = self.top() {
            node.vol.selection = selection;
        }
    }

    /// The `z-index` the element is lifted into the stacking context around it with, if it is,
    /// and whether it is one.
    pub(crate) fn stacking(&mut self, decide: impl FnOnce() -> (Option<i32>, bool)) {
        if self.recording() {
            let (lift_z, forms_context) = decide();
            if let Some(node) = self.top() {
                node.lift_z = lift_z;
                node.forms_context = forms_context;
            }
        }
    }

    /// What the element's parent needs to know of how far it draws.
    pub(crate) fn subtree(&mut self, subtree: crate::layout::cull::Subtree) {
        if let Some(node) = self.top() {
            node.subtree = subtree;
        }
    }

    /// Adds what the build made of the element to its digest. Only formatted when verifying.
    pub(crate) fn digest(&mut self, built: impl FnOnce() -> String) {
        if !self.verify || self.planning {
            return;
        }
        let built = built();
        if let Some(node) = self.top() {
            let mut fp = Fingerprint::new();
            fp.word(node.digest);
            fp.str(&built);
            node.digest = fp.finish();
        }
    }

    /// Ends the build of a frame.
    pub(crate) fn finish_frame(&mut self) {
        let recorded = self.open;
        let candidates = self.finish_session();
        if recorded && self.backs_off() && self.pass == 1 {
            self.back_off(candidates);
        }
    }

    /// Plans the frames that follow a recorded one that found `candidates`, or none, to skip.
    fn back_off(&mut self, candidates: bool) {
        if candidates || self.nodes.is_empty() {
            self.backoff.gap = 0;
            return;
        }
        self.backoff.gap = (self.backoff.gap * 2).clamp(1, BACKOFF_CAP);
        let scrolling = self.deltas.scrolling;
        let gap = self.backoff.gap;
        self.forget();
        self.backoff = Backoff { rest: gap, gap, resting: false, scrolling, elements: self.elements };
    }

    /// Decides what this build would have reused, counts it, and keeps what the next build compares
    /// itself with. Returns whether some element in it could be skipped by a frame that finds it
    /// unchanged and unmoved.
    fn finish_session(&mut self) -> bool {
        if !self.open {
            return false;
        }
        self.open = false;
        debug_assert!(self.stack.is_empty(), "an element was entered and never exited");
        self.stack.clear();
        let count = self.nodes.len();

        self.below.clear();
        self.below.resize(count, Below::default());
        for (at, node) in self.nodes.iter().enumerate() {
            let below = &mut self.below[at];
            if node.skipped.is_some() {
                continue;
            }
            let before = self.records.get(&node.id);
            below.changed = match before {
                None => Some(Cause::New),
                Some(before) if before.key.revision != node.key.revision => Some(Cause::Edited),
                Some(before) if before.key.inherited != node.key.inherited => Some(Cause::Inherited),
                Some(before) if before.key.shape != node.key.shape => Some(Cause::Shape),
                Some(before) => node.vol.cause_against(&before.vol),
            };
            below.scrolled = before.is_some_and(|before| before.vol.scroll != node.vol.scroll);
        }
        // Children come after their parent, so walking backwards finishes a node before its parent
        // hears of it. Later siblings are folded first, so the first excluded one is left in the end.
        for at in (0..count).rev() {
            let node = &self.nodes[at];
            let own = self.below[at];
            let escapes = node.lift_z.is_some() || (!node.forms_context && own.lifts);
            let digest = match &node.skipped {
                Some(skipped) => skipped.digest,
                None => {
                    let mut fp = Fingerprint::new();
                    fp.word(node.digest);
                    fp.word(own.digest);
                    fp.finish()
                }
            };
            let size = match &node.skipped {
                Some(skipped) => skipped.size,
                None => own.size + 1,
            };
            self.below[at].digest = digest;
            self.below[at].size = size;
            if node.parent == NO_PARENT {
                continue;
            }
            let parent = &mut self.below[node.parent as usize];
            parent.changed_below |= own.changed.is_some() || own.changed_below;
            parent.lifts |= escapes;
            parent.size += size;
            if let Some(excluded) = node.exclusion.or(own.excluded) {
                parent.excluded = Some(excluded);
            }
            parent.digest = parent.digest.rotate_left(5).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ digest;
        }

        self.retained.clear();
        self.retained.resize(count, false);
        self.moved.clear();
        self.moved.resize(count, false);
        self.summary = Summary { roots: Vec::new(), elements_in_roots: 0, elements: count };
        let mut counts = [0u64; SLOTS];
        let mut large = 0;
        let mut candidates = false;
        let all_effects = crate::tape::build_effects_since(0);
        let mut built_dynamic = FxHashSet::default();
        let keep_slots = self.debug_wants_slots();
        let mut slots = Vec::new();
        for at in 0..count {
            let node = &self.nodes[at];
            let own = self.below[at];
            let eligible = node.exclusion.is_none() && own.excluded.is_none() && (node.forms_context || !own.lifts);
            let clean = own.changed.is_none() && !own.changed_below;
            let parent = (node.parent != NO_PARENT).then_some(node.parent as usize);
            self.moved[at] = parent.is_some_and(|parent| self.moved[parent] || self.below[parent].scrolled);
            // A root under a scroller that gpui can record to move is skipped by a frame that finds
            // the scroller moved. One that it cannot is built when it does.
            let holds = eligible
                && node.skipped.is_none()
                && node.scroller.is_some_and(|scroller| scroller.holds(node.id));
            let relocates = holds && !self.relocation.resting;
            let too_big = eligible
                && node.skipped.is_none()
                && node.scroller.is_some_and(|scroller| scroller.too_big(node.id));
            let oversized = clean && too_big;
            // What a scroller that only appeared moves is not a loss: the frame after it finds the rows still.
            let carried = (node.moved || self.moved[at]) && self.deltas.scrolling && !relocates;
            candidates |= node.skipped.is_some() || (eligible && own.size >= self.min_root && !carried && !too_big);

            let slot = if parent.is_some_and(|parent| self.retained[parent]) {
                self.retained[at] = true;
                RETAINED
            } else if oversized {
                counts[OVERSIZED] += 1;
                OVERSIZED
            } else if node.skipped.is_none() && clean && eligible && self.moved[at] && !relocates {
                counts[MOVED] += 1;
                MOVED
            } else if node.skipped.is_some() || (clean && eligible) {
                self.retained[at] = true;
                let size = own.size as usize;
                self.summary.roots.push(node.id);
                self.summary.elements_in_roots += size;
                if size >= LARGE_ROOT {
                    large += size;
                }
                RETAINED
            } else {
                let slot = if let Some(excluded) = node.exclusion {
                    EXCLUDED + excluded as usize
                } else if let Some(cause) = own.changed {
                    CHANGED + cause as usize
                } else if own.changed_below {
                    SPINE
                } else if let Some(excluded) = own.excluded {
                    UNDER + excluded as usize
                } else {
                    LIFTS
                };
                counts[slot] += 1;
                slot
            };
            if keep_slots {
                slots.push(slot);
            }

            if node.skipped.is_some() {
                continue;
            }
            if self.retained[at] && self.verify && !self.planning {
                check(&self.records, &mut self.mismatches, node.id, own.digest, eligible);
            }
            if node.vol.dynamic != 0 {
                self.dynamic.insert(node.id);
                built_dynamic.insert(node.id);
            } else {
                self.dynamic.remove(&node.id);
            }
            let before = self.records.get(&node.id);
            let epoch = match (before, self.deferred) {
                (Some(before), true) => before.epoch,
                (Some(before), false) => before.epoch + 1,
                (None, _) => 1,
            };
            let effects = (node.effects_end > node.effects_start)
                .then(|| Rc::<[BuildEffect]>::from(&all_effects[node.effects_start..node.effects_end]));
            let mut token = Fingerprint::new();
            token.word(node.enter.revision);
            token.word(node.enter.inherited);
            token.word(node.enter.shape);
            token.word(epoch);
            self.records.insert(
                node.id,
                Record {
                    key: node.key,
                    enter: node.enter,
                    vol: node.vol,
                    digest: own.digest,
                    eligible,
                    epoch,
                    token: token.finish(),
                    lift_z: node.lift_z,
                    forms_context: node.forms_context,
                    size: own.size,
                    subtree: node.subtree,
                    effects,
                    seasoned: self.deferred,
                },
            );
        }

        if !self.deferred {
            self.forget_unbuilt_dynamic(&built_dynamic);
        }
        if keep_slots && !self.deferred {
            self.debug_capture(&slots);
        }

        counts[RETAINED] = self.summary.elements_in_roots as u64;
        crate::perf_counters::retain_slots(&counts);
        crate::perf_counters::retained(self.summary.roots.len() as u64, large as u64);
        candidates
    }

    /// A whole frame visits every element it does not skip, and it skips none that has a dynamic
    /// one below it. So a dynamic element that this frame did not visit sits under one that has no
    /// box (`display: none`), and its record is of a build that is gone: it would keep every
    /// ancestor on the spine for as long as the window lives. Its record goes too, since a frame
    /// that shows it again must build it, not take what was recorded while its motion ran.
    fn forget_unbuilt_dynamic(&mut self, built: &FxHashSet<u64>) {
        let stale: Vec<u64> = self.dynamic.iter().copied().filter(|id| !built.contains(id)).collect();
        for id in stale {
            self.dynamic.remove(&id);
            self.records.remove(&id);
        }
    }
}

/// The scroll offsets of the scrollers, as the bits that tell one frame's from another's.
fn scroll_bits(scrolls: &HashMap<u64, gpui::ScrollHandle>) -> FxHashMap<u64, (u32, u32)> {
    scrolls
        .iter()
        .map(|(id, handle)| {
            let offset = handle.offset();
            (*id, (f32::from(offset.x).to_bits(), f32::from(offset.y).to_bits()))
        })
        .collect()
}

/// The ids that are in one of `before` and `now`, not in both, added to `changed`.
fn differ(before: &[u64], now: &[u64], changed: &mut Vec<u64>) {
    changed.extend(before.iter().filter(|id| !now.contains(id)));
    changed.extend(now.iter().filter(|id| !before.contains(id)));
}

/// An element judged unchanged has to have been built as it was last time: the hash of what the
/// build made of it and of everything below it is the one the last build recorded.
fn check(records: &FxHashMap<u64, Record>, mismatches: &mut Vec<String>, id: u64, digest: u64, eligible: bool) {
    let Some(before) = records.get(&id) else { return };
    let problem = if before.digest != digest {
        "built differently"
    } else if before.eligible != eligible {
        "was judged differently"
    } else {
        return;
    };
    crate::perf_counters::retain_mismatch();
    let message = format!("element {id} was judged unchanged and {problem}");
    // Loud, for a suite run under `GPUIX_RETAIN_VERIFY`: nothing reads the counter there.
    eprintln!("[retain] {message}");
    if mismatches.len() < 32 {
        mismatches.push(message);
    }
}

/// Roots of at least this many elements are counted again on their own: a one-element placeholder
/// costs about what it saves.
const LARGE_ROOT: usize = 8;

/// `mixed` with the states of the `index`th inline box, which only count when one applies.
pub(crate) fn mix_pointer(mixed: u64, index: usize, states: crate::events::state_styles::PointerStates) -> u64 {
    match pointer_bits(states) {
        0 => mixed,
        bits => mixed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (index as u64) << 16 ^ u64::from(bits),
    }
}

fn pointer_bits(states: crate::events::state_styles::PointerStates) -> u16 {
    [
        states.hovered,
        states.active,
        states.group_hovered,
        states.group_active,
        states.group_focused_within,
        states.has_hovered,
        states.has_active,
        states.has_focused,
        states.focus_within,
        states.has_focus_visible,
        states.focus_visible,
    ]
    .into_iter()
    .fold(0, |bits, flag| bits << 1 | u16::from(flag))
}

/// How the focus state touches `id`: nothing is focused for most of the elements most of the time.
fn focus_bits(focus: &crate::focus::FocusStyles, id: u64, has_handle: bool) -> u16 {
    let mut bits = u16::from(has_handle);
    if focus.within.is_empty() {
        return bits;
    }
    bits |= u16::from(focus.within.contains(&id)) << 1;
    bits |= u16::from(focus.visible == Some(id)) << 2;
    bits |= u16::from(focus.has_focus(id)) << 3;
    bits |= u16::from(focus.has_focus_visible(id)) << 4;
    bits |= u16::from(has_handle && focus.dim_path.as_deref().is_some_and(|path| !path.contains(&id))) << 5;
    bits
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use gpui::AppContext as _;

    use super::*;
    use crate::renderer::GpuixView;
    use crate::retained_tree::RetainedTree;

    #[test]
    fn the_fingerprint_names_every_field_of_inherited() {
        // `inherited_fingerprint` lists the fields by hand. Adding one changes this size, and the
        // fingerprint has to learn it or the key will not see it change.
        assert_eq!(std::mem::size_of::<Inherited>(), 408);
    }

    fn root_inherited() -> Inherited {
        Inherited::root(
            &crate::theme::Theme::dark(),
            gpui::WindowAppearance::Light,
            false,
            gpui::size(gpui::px(800.0), gpui::px(600.0)),
        )
    }

    fn build(retain: &mut Retain, inherited: &Inherited, built: &str) {
        let (tree, focus) = (RetainedTree::new(), crate::focus::FocusStyles::default());
        let (handles, scrolls) = (HashMap::new(), HashMap::new());
        retain.begin_frame(Sources {
            tree: &tree,
            hovered: &[],
            active: &[],
            focus: &focus,
            focusable: &handles,
            keyboard: false,
            scrolls: &scrolls,
            measured: Vec::new(),
            links: Vec::new(),
            customs: Vec::new(),
            motion: Vec::new(),
            selection: 0,
            pass: 1,
            lazy_builds: 0,
            relocations: 0,
            reference: false,
        });
        retain.enter(Entry {
            id: 1,
            revision: 7,
            inherited,
            at: 0,
            stacking_root: true,
            stacking_item: false,
            atomic_root: false,
            contents_overlay: None,
            anchor_scope: false,
            text_box_trim: Default::default(),
            plain: true,
            boxed: true,
            container: None,
            prekeyed: None,
            scroller: None,
        });
        retain.stacking(|| (None, true));
        retain.digest(|| built.to_owned());
        retain.exit();
        retain.finish_frame();
    }

    #[test]
    fn what_was_judged_unchanged_but_built_differently_is_reported() {
        let inherited = root_inherited();
        let mut retain = Retain::new();
        retain.set_verify(true);
        build(&mut retain, &inherited, "red");
        build(&mut retain, &inherited, "red");
        assert_eq!(retain.summary().roots, vec![1]);
        assert!(retain.mismatches().is_empty());
        // The key did not move, the build did: this is the hole the check exists to find.
        build(&mut retain, &inherited, "blue");
        assert_eq!(retain.mismatches().len(), 1, "{:?}", retain.mismatches());
    }

    #[test]
    fn a_new_element_is_a_change_and_a_second_look_is_not() {
        let inherited = root_inherited();
        let mut retain = Retain::new();
        build(&mut retain, &inherited, "");
        assert_eq!(retain.summary().roots, Vec::<u64>::new());
        build(&mut retain, &inherited, "");
        assert_eq!(retain.summary().roots, vec![1]);
    }

    /// An element of the page under test.
    struct El {
        id: u64,
        parent: Option<u64>,
        element_type: &'static str,
        style: &'static str,
        text: Option<&'static str>,
        props: Vec<(&'static str, serde_json::Value)>,
    }

    fn div(id: u64, parent: Option<u64>, style: &'static str) -> El {
        El { id, parent, element_type: "div", style, text: None, props: Vec::new() }
    }

    fn text(id: u64, parent: u64, content: &'static str) -> El {
        El { id, parent: Some(parent), element_type: "text", style: "{}", text: Some(content), props: Vec::new() }
    }

    fn typed(id: u64, parent: u64, element_type: &'static str, style: &'static str) -> El {
        El { id, parent: Some(parent), element_type, style, text: None, props: Vec::new() }
    }

    fn with(mut element: El, key: &'static str, value: serde_json::Value) -> El {
        element.props.push((key, value));
        element
    }

    /// A column of three cards of three lines each: 1 is the page, 10, 20 and 30 the cards, and
    /// 11 to 13, 21 to 23, 31 to 33 their lines, each holding a text.
    fn page(card_styles: [&'static str; 3]) -> Vec<El> {
        let mut elements = vec![div(
            1,
            None,
            r##"{"display": "flex", "flexDirection": "column", "width": 400, "height": 300, "color": "#223344"}"##,
        )];
        for (card, style) in [10, 20, 30].into_iter().zip(card_styles) {
            elements.push(div(card, Some(1), style));
            for line in 1..=3 {
                elements.push(div(card + line, Some(card), r#"{"height": 10}"#));
                elements.push(text(card * 10 + line, card + line, "some words"));
            }
        }
        elements
    }

    struct Scene {
        tree: Arc<Mutex<RetainedTree>>,
        window: gpui::WindowHandle<GpuixView>,
    }

    impl Scene {
        fn new(cx: &mut gpui::TestAppContext, elements: Vec<El>) -> Self {
            let tree = Arc::new(Mutex::new(RetainedTree::new()));
            {
                let mut tree = tree.lock().unwrap();
                for element in &elements {
                    tree.create_element(element.id, element.element_type.to_owned());
                    let style = tree.styles.intern(element.style.as_bytes()).unwrap();
                    tree.set_style(element.id, style);
                    if let Some(content) = element.text {
                        tree.set_text(element.id, content.to_owned());
                    }
                    for (key, value) in &element.props {
                        tree.set_custom_prop(element.id, (*key).to_owned(), value.clone());
                    }
                }
                for element in &elements {
                    if let Some(parent) = element.parent {
                        tree.append_child(parent, element.id);
                    }
                }
                tree.set_root(elements[0].id);
            }
            let view_tree = tree.clone();
            let window = cx.add_window(|window, _| {
                crate::events::enable_dom_hit_testing(window);
                crate::layout::container::allow_redraw_passes(window);
                let mut view = GpuixView::new(
                    view_tree,
                    None,
                    "test".to_owned(),
                    crate::text::SharedSelection::default(),
                    crate::events::state::PointerRequests::default(),
                );
                view.clock.pause();
                view
            });
            let scene = Self { tree, window };
            scene.draw(cx);
            scene
        }

        /// A frame, then a check that nothing judged unchanged was built differently.
        fn draw(&self, cx: &mut gpui::TestAppContext) {
            cx.update_window(self.window.into(), |_, window, app| {
                window.refresh();
                window.draw(app).clear(app);
            })
            .unwrap();
            self.assert_sound(cx);
        }

        fn assert_sound(&self, cx: &mut gpui::TestAppContext) {
            let mismatches = self.window.update(cx, |view, _, _| view.retain.mismatches().to_vec()).unwrap();
            assert!(mismatches.is_empty(), "{mismatches:?}");
            let replayed = cx
                .update_window(self.window.into(), |_, window, _| window.retention_mismatches().to_vec())
                .unwrap();
            assert!(replayed.is_empty(), "{replayed:?}");
        }

        /// The roots the last build would have reused, sorted.
        fn roots(&self, cx: &mut gpui::TestAppContext) -> Vec<u64> {
            let mut roots = self.window.update(cx, |view, _, _| view.retain.summary().roots.clone()).unwrap();
            roots.sort_unstable();
            roots
        }

        fn elements_in_roots(&self, cx: &mut gpui::TestAppContext) -> (usize, usize) {
            self.window
                .update(cx, |view, _, _| (view.retain.summary().elements_in_roots, view.retain.summary().elements))
                .unwrap()
        }

        /// Draws until a frame changes nothing, which a page that is not animating does by the second.
        fn settle(&self, cx: &mut gpui::TestAppContext) {
            for _ in 0..3 {
                self.draw(cx);
            }
        }

        /// A test app draws inline when an input changes what is shown, so the frame this makes is
        /// the one the pointer is already in.
        fn mouse_to(&self, cx: &mut gpui::TestAppContext, id: u64) {
            let bounds = crate::automation::get_bounds(self.window.window_id(), id).expect("painted");
            let inside = gpui::point(gpui::px((bounds.x + 2.0) as f32), gpui::px((bounds.y + 2.0) as f32));
            gpui::VisualTestContext::from_window(self.window.into(), cx).simulate_mouse_move(
                inside,
                None,
                Default::default(),
            );
            self.assert_sound(cx);
        }

        fn mouse_away(&self, cx: &mut gpui::TestAppContext) {
            gpui::VisualTestContext::from_window(self.window.into(), cx).simulate_mouse_move(
                gpui::point(gpui::px(790.0), gpui::px(590.0)),
                None,
                Default::default(),
            );
            self.assert_sound(cx);
        }

        fn set_prop(&self, id: u64, key: &str, value: serde_json::Value) {
            self.tree.lock().unwrap().set_custom_prop(id, key.to_owned(), value);
        }

        /// How many elements the last frames counted under `reason`.
        fn counted(reason: &str) -> u64 {
            crate::perf_counters::retain_reasons()
                .iter()
                .find(|(name, _)| name == reason)
                .map_or(0, |(_, count)| *count)
        }

        fn set_style(&self, id: u64, style: &str) {
            let mut tree = self.tree.lock().unwrap();
            let style = tree.styles.intern(style.as_bytes()).unwrap();
            tree.set_style(id, style);
        }
    }

    /// A page that skips what it judges unchanged, drawn until its roots are recorded. With
    /// `verify`, every frame that replays something is built again and compared.
    fn planned(cx: &mut gpui::TestAppContext, card_styles: [&'static str; 3], verify: bool) -> Scene {
        let scene = Scene::new(cx, page(card_styles));
        scene
            .window
            .update(cx, |view, _, _| {
                view.retain.set_planning(true);
                view.retain.set_verify(verify);
            })
            .unwrap();
        for _ in 0..4 {
            scene.notified_draw(cx);
        }
        scene
    }

    impl Scene {
        /// A frame of a view that notified, which does not refresh the window.
        fn notified_draw(&self, cx: &mut gpui::TestAppContext) {
            self.window.update(cx, |_, _, cx| cx.notify()).unwrap();
            cx.update_window(self.window.into(), |_, window, app| window.draw(app).clear(app)).unwrap();
            self.assert_sound(cx);
        }
    }

    fn retained_counters(scene: &Scene, cx: &mut gpui::TestAppContext) -> gpui::RetainedCounters {
        cx.update_window(scene.window.into(), |_, window, _| window.retained_counters()).unwrap()
    }

    #[gpui::test]
    fn a_replayed_page_is_the_page_a_build_makes(cx: &mut gpui::TestAppContext) {
        let scene = planned(cx, [CARD; 3], true);
        let before = retained_counters(&scene, cx);
        scene.notified_draw(cx);
        scene.notified_draw(cx);
        let after = retained_counters(&scene, cx);
        assert!(after.verified > before.verified, "frames that replayed were compared: {after:?}");
        assert_eq!(after.mismatches, 0);
    }

    impl Scene {
        /// A read of a bounds on a window that has a change to show: a pass that lays out and
        /// does not paint. Returns the boxes it published, how many elements it built and what the
        /// retained subtrees did, all as the pass left them (the test app draws the window once it
        /// is over).
        fn layout_only(&self, cx: &mut gpui::TestAppContext) -> (String, u64, gpui::RetainedCounters) {
            crate::perf_counters::reset();
            let result = cx
                .update_window(self.window.into(), |view, window, app| {
                    view.downcast::<GpuixView>().unwrap().update(app, |_, cx| cx.notify());
                    assert!(window.draw_if_dirty(app), "a pass ran");
                    (
                        crate::automation::describe_bounds(window.window_handle().window_id()),
                        crate::perf_counters::snapshot().elements_built,
                        window.retained_counters(),
                    )
                })
                .unwrap();
            self.assert_sound(cx);
            result
        }

        /// The boxes of the last frame, after a frame that builds every element.
        fn built_bounds(&self, cx: &mut gpui::TestAppContext) -> String {
            self.draw(cx);
            crate::automation::describe_bounds(self.window.window_id())
        }
    }

    #[gpui::test]
    fn a_layout_only_pass_publishes_the_boxes_a_build_publishes(cx: &mut gpui::TestAppContext) {
        let scene = planned(cx, [CARD; 3], true);
        let all = 1 + 3 * (1 + 3 * 2);

        // The last card grows, and nothing before it moves.
        scene.set_style(30, r#"{"height": 90}"#);
        let (published, _, during) = scene.layout_only(cx);
        assert!(during.layout_replays >= 5, "{during:?}");
        assert!(during.verified > 0, "the pass was compared with a build of it: {during:?}");
        assert_eq!(published, scene.built_bounds(cx));
        assert!(published.lines().count() >= all as usize - 3, "the boxes of what was skipped are there:\n{published}");

        // The first card grows and moves the others, which are built where they are.
        scene.settle(cx);
        scene.set_style(10, r#"{"height": 90}"#);
        let (published, _, _) = scene.layout_only(cx);
        assert_eq!(published, scene.built_bounds(cx));

        // And the frame after a pass is the frame a build makes, which replays what the pass
        // left alone.
        scene.settle(cx);
        let before = retained_counters(&scene, cx);
        scene.set_style(30, r#"{"height": 60}"#);
        scene.layout_only(cx);
        scene.notified_draw(cx);
        let after = retained_counters(&scene, cx);
        assert!(after.replays > before.replays, "{after:?}");
        assert_eq!(after.mismatches, 0, "{after:?}");
        assert!(after.verified > before.verified);
    }

    #[gpui::test]
    fn a_layout_only_pass_builds_the_spine_and_what_changed(cx: &mut gpui::TestAppContext) {
        let scene = planned(cx, [CARD; 3], false);
        let pass = |cx: &mut gpui::TestAppContext| {
            let before = retained_counters(&scene, cx);
            let (_, built, after) = scene.layout_only(cx);
            (built, after.layout_skips - before.layout_skips, after.layout_replays - before.layout_replays, after.layout_builds - before.layout_builds)
        };
        // Nothing changed: the page is the one root, and the pass skips it.
        assert_eq!(pass(cx), (0, 1, 1, 0));
        scene.settle(cx);

        // The page and the last card are built; the other cards and the lines of the last are
        // not, and are where they were.
        scene.set_style(30, r#"{"height": 90}"#);
        assert_eq!(pass(cx), (2, 5, 5, 0));
        scene.settle(cx);

        // The cards after the first move, and are built where they are.
        scene.set_style(10, r#"{"height": 90}"#);
        let (built, skips, replays, builds) = pass(cx);
        assert!(built > 2, "{built}");
        // The cards moved, and so did the lines in them, which are skipped inside the cards built.
        assert_eq!((skips, replays, builds), (17, 3, 14));
    }

    #[gpui::test]
    fn a_box_that_goes_away_with_a_transition_running_and_comes_back(cx: &mut gpui::TestAppContext) {
        let shown = |width: u32, display: &str| {
            format!(
                r#"{{"width": {width}, "height": 20, "display": "{display}", "transition": "width 1s linear"}}"#
            )
        };
        let elements = vec![
            div(1, None, r#"{"width": 400, "height": 300}"#),
            div(2, Some(1), Box::leak(shown(100, "block").into_boxed_str())),
        ];
        let scene = Scene::new(cx, elements);
        scene
            .window
            .update(cx, |view, _, _| {
                view.retain.set_planning(true);
                view.retain.set_verify(true);
                view.retain.set_min_root(1);
            })
            .unwrap();
        let step = |cx: &mut gpui::TestAppContext, width: u32, display: &str, ms: f64| {
            scene.set_style(2, &shown(width, display));
            scene.window.update(cx, |view, _, _| { view.clock.fast_forward_ms(ms); }).unwrap();
            scene.notified_draw(cx);
        };
        for _ in 0..3 {
            scene.notified_draw(cx);
        }
        step(cx, 200, "block", 0.0);
        step(cx, 200, "block", 500.0);
        step(cx, 200, "none", 0.0);
        step(cx, 200, "none", 16.0);
        step(cx, 300, "none", 0.0);
        step(cx, 300, "none", 16.0);
        step(cx, 300, "block", 0.0);
        step(cx, 300, "block", 500.0);
    }

    #[gpui::test]
    fn a_page_that_does_not_move_is_not_built(cx: &mut gpui::TestAppContext) {
        let scene = planned(cx, [CARD; 3], false);
        let before = retained_counters(&scene, cx);
        crate::perf_counters::reset();
        scene.notified_draw(cx);
        scene.notified_draw(cx);
        let after = retained_counters(&scene, cx);
        assert_eq!(crate::perf_counters::snapshot().elements_built, 0);
        assert!(after.replays > before.replays, "{after:?}");
        assert_eq!(after.builds, before.builds);
        assert_eq!(after.lazy_builds, before.lazy_builds);
        assert_eq!(after.total_non_reusable(), 0, "{after:?}");
    }

    const CARD: &str = r#"{"height": 60}"#;
    const HOVER_CARD: &str = r##"{"height": 60, "hover": {"backgroundColor": "#aa0000"}}"##;

    /// A scroller of three cards, which are subtrees a frame could skip when the scroller is
    /// where it was, under a page that is not one since it holds a scroller.
    fn scroller_page() -> Vec<El> {
        let mut elements = vec![
            div(1, None, r#"{"display": "flex", "flexDirection": "column", "width": 400, "height": 300}"#),
            div(
                2,
                Some(1),
                r#"{"display": "flex", "flexDirection": "column", "width": 400, "height": 40, "overflowY": "auto"}"#,
            ),
        ];
        for card in [10, 20, 30] {
            elements.push(div(card, Some(2), r#"{"height": 30}"#));
            for line in 1..=2 {
                elements.push(div(card + line, Some(card), r#"{"height": 10}"#));
                elements.push(text(card * 10 + line, card + line, "some words"));
            }
        }
        elements
    }

    /// A scroller of boxes of one element each, which no frame skips on its own.
    fn boxes_page() -> Vec<El> {
        let mut elements = vec![
            div(1, None, r#"{"display": "flex", "flexDirection": "column", "width": 400, "height": 300}"#),
            div(
                2,
                Some(1),
                r#"{"display": "flex", "flexDirection": "column", "width": 400, "height": 40, "overflowY": "auto"}"#,
            ),
        ];
        for id in 10..16 {
            elements.push(div(id, Some(2), r#"{"height": 30}"#));
        }
        elements
    }

    /// A scene that skips what it can and backs off, as a window does outside a test.
    fn backing_off(cx: &mut gpui::TestAppContext, elements: Vec<El>) -> Scene {
        let scene = Scene::new(cx, elements);
        scene
            .window
            .update(cx, |view, _, _| {
                view.retain.set_planning(true);
                view.retain.set_verify(false);
                // A box of one element is a subtree to skip, unless this says it is not.
                view.retain.set_min_root(2);
            })
            .unwrap();
        scene
    }

    /// Draws `frames` frames and says for each whether it was recorded. A test app draws more
    /// than once when it flushes effects, so these are the frames the retention saw.
    fn frame_log(scene: &Scene, cx: &mut gpui::TestAppContext, draw: impl FnOnce(&Scene, &mut gpui::TestAppContext)) -> Vec<bool> {
        scene.window.update(cx, |view, _, _| view.retain.take_frame_log()).unwrap();
        draw(scene, cx);
        scene.window.update(cx, |view, _, _| view.retain.take_frame_log()).unwrap()
    }

    fn draws(frames: usize) -> impl FnOnce(&Scene, &mut gpui::TestAppContext) {
        move |scene, cx| (0..frames).for_each(|_| scene.notified_draw(cx))
    }

    #[gpui::test]
    fn a_page_with_nothing_to_skip_is_recorded_less_and_less_often(cx: &mut gpui::TestAppContext) {
        let scene = backing_off(cx, boxes_page());
        let log = frame_log(&scene, cx, draws(12));
        let recorded: Vec<usize> = (0..log.len()).filter(|at| log[*at]).collect();
        // Stretches of 1, 2, 4, 8 and 8 unrecorded frames between the recorded ones.
        let gaps: Vec<usize> = recorded.windows(2).map(|pair| pair[1] - pair[0] - 1).collect();
        assert_eq!(gaps, [1, 2, 4, 8, 8, 8][..gaps.len()], "{log:?}");
        assert!(gaps.len() >= 3, "{log:?}");
    }

    #[gpui::test]
    fn a_page_that_gains_an_element_is_recorded_at_once(cx: &mut gpui::TestAppContext) {
        let scene = backing_off(cx, boxes_page());
        let log = frame_log(&scene, cx, draws(8));
        assert!(!log[log.len() - 1], "in the middle of a stretch: {log:?}");
        {
            let mut tree = scene.tree.lock().unwrap();
            tree.create_element(99, "div".to_owned());
            let style = tree.styles.intern(br#"{"height": 30}"#).unwrap();
            tree.set_style(99, style);
            tree.append_child(2, 99);
        }
        let log = frame_log(&scene, cx, draws(1));
        assert!(log[0], "{log:?}");
    }

    #[gpui::test]
    fn a_page_with_a_subtree_to_skip_is_recorded_every_frame(cx: &mut gpui::TestAppContext) {
        let scene = backing_off(cx, page([CARD; 3]));
        let log = frame_log(&scene, cx, draws(12));
        assert!(log.iter().all(|recorded| *recorded), "{log:?}");
        assert_eq!(scene.roots(cx), vec![1]);
    }

    #[gpui::test]
    fn a_verified_page_is_recorded_every_frame(cx: &mut gpui::TestAppContext) {
        let scene = Scene::new(cx, boxes_page());
        scene.window.update(cx, |view, _, _| view.retain.set_planning(true)).unwrap();
        let log = frame_log(&scene, cx, draws(12));
        assert!(log.iter().all(|recorded| *recorded), "{log:?}");
    }

    #[gpui::test]
    fn a_scroller_whose_cards_can_be_moved_is_recorded_every_frame_it_moves(cx: &mut gpui::TestAppContext) {
        let scene = backing_off(cx, scroller_page());
        scene.notified_draw(cx);
        scene.notified_draw(cx);
        let moving = frame_log(&scene, cx, |scene, cx| {
            (1..=12).for_each(|top| {
                scene
                    .window
                    .update(cx, |view, _, _| {
                        view.scroll_handles[&2].set_offset(gpui::point(gpui::px(0.), gpui::px(-(top as f32))));
                    })
                    .unwrap();
                scene.draw(cx);
            })
        });
        // The cards are not in the same place twice, and a frame can put them where they are.
        assert!(moving.iter().all(|recorded| *recorded), "{moving:?}");
    }

    #[gpui::test]
    fn frames_are_recorded_again_the_moment_the_scrollers_stop(cx: &mut gpui::TestAppContext) {
        // A scroller whose cards gpui was found not to be moving: they are built when it moves.
        let scene = backing_off(cx, scroller_page());
        scene.window.update(cx, |view, _, _| view.retain.stop_relocating()).unwrap();
        scene.notified_draw(cx);
        let scroll = |scene: &Scene, cx: &mut gpui::TestAppContext, top: f32| {
            scene
                .window
                .update(cx, |view, _, _| {
                    view.scroll_handles
                        .get(&2)
                        .expect("the scroller has a handle")
                        .set_offset(gpui::point(gpui::px(0.), gpui::px(-top)));
                })
                .unwrap();
            scene.draw(cx);
        };
        // Moving every frame, the cards are never in the same place twice: recorded to look,
        // 1, 2, 4 frames apart.
        let moving = frame_log(&scene, cx, |scene, cx| (1..=12).for_each(|top| scroll(scene, cx, top as f32)));
        assert!(moving.iter().filter(|recorded| !**recorded).count() >= 6, "{moving:?}");
        // Stopped, whatever the stretch it is in: the next frame is recorded, and the one after
        // replays the cards.
        let rest = moving.iter().rev().take_while(|recorded| !**recorded).count();
        let stopped = frame_log(&scene, cx, |scene, cx| scroll(scene, cx, 12.));
        assert!(stopped[0], "{moving:?} {stopped:?} {rest}");
        scene.notified_draw(cx);
        assert_eq!(scene.roots(cx), vec![10, 20, 30]);
        let before = retained_counters(&scene, cx);
        scene.notified_draw(cx);
        assert!(retained_counters(&scene, cx).replays > before.replays, "and the cards are replayed again");
    }

    #[test]
    fn roots_gpui_builds_again_after_a_scroll_are_not_planned_to_move_for_a_while() {
        let mut retain = Retain::new();
        retain.relocation.planned = 20;
        // Twenty planned, five put elsewhere and as many built again as were put: kept.
        retain.learn_relocation(5, 5);
        assert!(!retain.relocation.resting);
        retain.relocation.planned = 20;
        // Built again more often than put elsewhere: rests for 1, then 2, then 4 frames, up to 8,
        // counting the frame that follows the judged one, which rests too.
        let mut lazy = 5;
        for gap in [1, 2, 4, 8, 8] {
            lazy += 10;
            retain.relocation.planned = 20;
            retain.learn_relocation(lazy, 5);
            assert!(retain.relocation.resting, "{gap}: the frame after the one that was found built again");
            let rested = (0..gap + 3)
                .map(|_| {
                    retain.learn_relocation(lazy, 5);
                    retain.relocation.resting
                })
                .collect::<Vec<_>>();
            assert_eq!(rested.iter().filter(|resting| **resting).count(), gap - 1, "{gap}: {rested:?}");
            assert!(!rested[gap], "{gap}: looks again after the stretch: {rested:?}");
        }
        // And once the roots that were planned to move did, the next stretch starts from one again.
        retain.relocation.planned = 20;
        retain.learn_relocation(lazy, 100);
        assert_eq!(retain.relocation.gap, 0);
    }

    #[gpui::test]
    fn a_page_that_does_not_move_is_one_retainable_root(cx: &mut gpui::TestAppContext) {
        let scene = Scene::new(cx, page([CARD; 3]));
        scene.settle(cx);
        assert_eq!(scene.roots(cx), vec![1]);
        let (inside, all) = scene.elements_in_roots(cx);
        assert_eq!(inside, all);
        assert_eq!(all, 1 + 3 * (1 + 3 * 2));
    }

    #[gpui::test]
    fn hovering_a_card_changes_it_and_leaves_the_rest(cx: &mut gpui::TestAppContext) {
        let scene = Scene::new(cx, page([HOVER_CARD, CARD, CARD]));
        scene.settle(cx);
        scene.mouse_to(cx, 10);
        // The page is the spine and the hovered card builds; what is in the card does not inherit
        // its background, so its lines are reusable.
        assert_eq!(scene.roots(cx), vec![11, 12, 13, 20, 30]);
        scene.draw(cx);
        assert_eq!(scene.roots(cx), vec![1], "it is still hovered, and builds as it did");
        scene.mouse_away(cx);
        assert_eq!(scene.roots(cx), vec![11, 12, 13, 20, 30], "its style went back");
        scene.settle(cx);
        assert_eq!(scene.roots(cx), vec![1]);
    }

    #[gpui::test]
    fn a_hover_that_changes_what_children_inherit_changes_them_too(cx: &mut gpui::TestAppContext) {
        let scene = Scene::new(
            cx,
            page([r##"{"height": 60, "hover": {"color": "#ff0000"}}"##, CARD, CARD]),
        );
        scene.settle(cx);
        scene.mouse_to(cx, 10);
        assert_eq!(scene.roots(cx), vec![20, 30]);
        scene.mouse_away(cx);
        assert_eq!(scene.roots(cx), vec![20, 30]);
        scene.settle(cx);
        assert_eq!(scene.roots(cx), vec![1]);
    }

    #[gpui::test]
    fn a_color_the_page_hands_down_changes_everything_below_it(cx: &mut gpui::TestAppContext) {
        let scene = Scene::new(cx, page([CARD; 3]));
        scene.settle(cx);
        scene.set_style(
            1,
            r##"{"display": "flex", "flexDirection": "column", "width": 400, "height": 300, "color": "#ffaa00"}"##,
        );
        scene.draw(cx);
        assert_eq!(scene.roots(cx), Vec::<u64>::new());
        scene.settle(cx);
        // A background is not inherited.
        scene.set_style(
            1,
            r##"{"display": "flex", "flexDirection": "column", "width": 400, "height": 300, "color": "#ffaa00", "backgroundColor": "#101010"}"##,
        );
        scene.draw(cx);
        assert_eq!(scene.roots(cx), vec![10, 20, 30]);
    }

    #[gpui::test]
    fn what_text_inherits_from_the_page_changes_everything_below_it(cx: &mut gpui::TestAppContext) {
        let scene = Scene::new(cx, page([CARD; 3]));
        scene.settle(cx);
        let page_style = |extra: &str| {
            format!(
                r##"{{"display": "flex", "flexDirection": "column", "width": 400, "height": 300, "color": "#223344", {extra}}}"##
            )
        };
        for (what, extra) in [
            ("an underline offset", r#""textUnderlineOffset": 3"#),
            ("variation settings", r#""fontVariationSettings": "\"wght\" 650""#),
            ("a text box trim", r#""textBox": "trim-both cap alphabetic""#),
        ] {
            scene.set_style(1, &page_style(extra));
            scene.draw(cx);
            assert_eq!(scene.roots(cx), Vec::<u64>::new(), "{what}");
            scene.settle(cx);
            assert_eq!(scene.roots(cx), vec![1], "{what}");
        }
    }

    #[gpui::test]
    fn a_window_resize_changes_what_viewport_units_mean(cx: &mut gpui::TestAppContext) {
        let scene = Scene::new(cx, page([r#"{"height": 60, "fontSize": "2vw"}"#, CARD, CARD]));
        scene.settle(cx);
        assert_eq!(scene.roots(cx), vec![1]);
        cx.simulate_window_resize(scene.window.into(), gpui::size(gpui::px(500.0), gpui::px(300.0)));
        scene.assert_sound(cx);
        assert_eq!(scene.roots(cx), Vec::<u64>::new(), "the viewport is in what everything inherits");
        scene.settle(cx);
        assert_eq!(scene.roots(cx), vec![1]);
    }

    #[gpui::test]
    fn a_container_that_resizes_is_retained_with_what_reads_it_once_the_size_is_settled(cx: &mut gpui::TestAppContext) {
        let mut elements = page([CARD; 3]);
        elements.push(div(40, Some(1), r#"{"width": 200, "height": 30, "containerType": "inline-size"}"#));
        elements.push(div(41, Some(40), r#"{"width": "50cqw", "height": 10}"#));
        let scene = Scene::new(cx, elements);
        scene.settle(cx);
        assert_eq!(scene.roots(cx), vec![1]);
        // The size is part of the key of the container and of what is in it: a frame that finds
        // the container resized builds it, and the page above it.
        scene.set_style(40, r#"{"width": 100, "height": 30, "containerType": "inline-size"}"#);
        scene.draw(cx);
        assert_eq!(scene.roots(cx), vec![10, 20, 30]);
        scene.settle(cx);
        assert_eq!(scene.roots(cx), vec![1]);
    }

    #[gpui::test]
    fn a_parent_of_a_child_with_container_blocks_is_retained(cx: &mut gpui::TestAppContext) {
        let mut elements = page([CARD; 3]);
        elements.push(div(40, Some(1), r#"{"width": 200, "height": 30, "containerType": "inline-size"}"#));
        elements.push(div(41, Some(40), r#"{"height": 10}"#));
        elements.push(div(42, Some(41), r#"{"height": 5, "@container": {"(min-width: 300px)": {"display": "none"}}}"#));
        let scene = Scene::new(cx, elements);
        scene.settle(cx);
        assert_eq!(scene.roots(cx), vec![1]);
        scene.set_style(40, r#"{"width": 400, "height": 30, "containerType": "inline-size"}"#);
        scene.draw(cx);
        scene.settle(cx);
        assert_eq!(scene.roots(cx), vec![1]);
    }

    #[gpui::test]
    fn the_debug_report_names_what_a_frame_built_and_what_it_skipped(cx: &mut gpui::TestAppContext) {
        let scene = backing_off(cx, page([CARD; 3]));
        scene.window.update(cx, |view, _, _| view.retain.set_debug(true)).unwrap();
        let report = |cx: &mut gpui::TestAppContext| -> serde_json::Value {
            let report = scene
                .window
                .update(cx, |view, _, _| view.retain.debug_report(&scene.tree.lock().unwrap()))
                .unwrap();
            serde_json::from_str(&report).unwrap()
        };
        // The first frame builds all of the page, and calls its first element new.
        scene.draw(cx);
        let first = report(cx);
        assert_eq!(first["builtNodes"], 22, "{first}");
        assert_eq!(first["skippedRoots"], 0, "{first}");
        // Once it is settled the page is one skipped root.
        scene.settle(cx);
        let settled = report(cx);
        assert_eq!(settled["builtNodes"], 0, "{settled}");
        assert_eq!(settled["skippedRoots"], 1, "{settled}");
        assert_eq!(settled["skippedElements"], 22, "{settled}");
    }

    #[gpui::test]
    fn an_animating_node_is_changed_every_frame_and_nothing_else_is(cx: &mut gpui::TestAppContext) {
        let mut elements = page([CARD; 3]);
        elements.push(with(
            div(40, Some(1), r#"{"height": 30, "opacity": 0.2}"#),
            "motion",
            serde_json::json!({
                "generation": 1,
                "initial": { "opacity": 0.2 },
                "animate": { "opacity": 0.9 },
                "transition": { "duration": 600.0, "ease": "linear" }
            }),
        ));
        let scene = Scene::new(cx, elements);
        scene.settle(cx);
        for _ in 0..5 {
            scene.window.update(cx, |view, _, _| view.clock.fast_forward_ms(16.0)).unwrap();
            scene.draw(cx);
            assert_eq!(scene.roots(cx), vec![10, 20, 30]);
        }
    }

    #[gpui::test]
    fn a_change_of_text_changes_the_line_that_holds_it_and_what_is_above(cx: &mut gpui::TestAppContext) {
        let scene = Scene::new(cx, page([CARD; 3]));
        scene.settle(cx);
        scene.tree.lock().unwrap().set_text(101, "other words".to_owned());
        scene.draw(cx);
        assert_eq!(scene.roots(cx), vec![12, 13, 20, 30]);
        scene.settle(cx);
        assert_eq!(scene.roots(cx), vec![1]);
    }

    #[gpui::test]
    fn focus_changes_the_focused_element_and_dims_the_other_focusable_ones(cx: &mut gpui::TestAppContext) {
        let focusable = |id, parent| {
            with(
                div(id, Some(parent), r##"{"height": 20, "focusVisible": {"outlineWidth": 2, "outlineColor": "#0000ff"}}"##),
                "tabIndex",
                serde_json::json!(0),
            )
        };
        let mut elements = page([CARD; 3]);
        elements.push(focusable(40, 1));
        elements.push(focusable(50, 1));
        let scene = Scene::new(cx, elements);
        scene.settle(cx);
        assert_eq!(scene.roots(cx), vec![1]);
        scene.window.update(cx, |_, window, _| window.activate_window()).unwrap();
        cx.run_until_parked();
        crate::perf_counters::reset();
        // The inline draws of a test app are the frames that see the focus, so the counters
        // of those are what to look at, and that none of them built an unchanged element differently.
        scene.window.update(cx, |view, window, cx| view.request_focus(40, window, cx)).unwrap();
        scene.settle(cx);
        let reasons = crate::perf_counters::retain_reasons();
        let changed_by_focus = reasons.iter().find(|(reason, _)| reason == "changed:focus").map_or(0, |(_, count)| *count);
        // The focused one, and the other, which dims while a control is focus-visible.
        assert!(changed_by_focus >= 2, "{reasons:?}");
        assert_eq!(scene.roots(cx), vec![1]);
        scene.window.update(cx, |_, window, _| window.blur()).unwrap();
        scene.settle(cx);
        assert_eq!(scene.roots(cx), vec![1]);
    }

    const HIDDEN_CARD: &str = r#"{"height": 60, "overflow": "hidden"}"#;

    /// `page` with lines taller than their card, so a hidden card has something to scroll.
    fn page_with_tall_lines(card_styles: [&'static str; 3]) -> Vec<El> {
        let mut elements = page(card_styles);
        for element in &mut elements {
            if element.style == r#"{"height": 10}"# {
                element.style = r#"{"height": 30}"#;
            }
        }
        elements
    }

    #[gpui::test]
    fn a_hidden_overflow_is_retained_until_script_moves_what_it_clips(cx: &mut gpui::TestAppContext) {
        let scene = Scene::new(cx, page_with_tall_lines([HIDDEN_CARD, CARD, CARD]));
        scene.settle(cx);
        assert_eq!(scene.roots(cx), vec![1], "the clipping card is no obstacle");
        crate::perf_counters::reset();
        scene
            .window
            .update(cx, |view, _, _| {
                view.scroll_handles[&10].set_offset(gpui::point(gpui::px(0.0), gpui::px(-20.0)));
            })
            .unwrap();
        scene.draw(cx);
        // The card read a new offset, and its lines sit somewhere else because of it: they are
        // roots that a frame can put where they are, as the lines of a scroller are.
        assert_eq!(scene.roots(cx), vec![11, 12, 13, 20, 30]);
        assert_eq!(Scene::counted("changed:scroll"), 1);
        scene.settle(cx);
        assert_eq!(scene.roots(cx), vec![1]);
    }

    /// The page, and under it a label of the control `email`, then the control.
    fn labelled_input() -> Vec<El> {
        let mut elements = page([CARD; 3]);
        elements.push(with(div(40, Some(1), r#"{"height": 20}"#), "id", serde_json::json!("row")));
        elements.push(with(typed(41, 1, "label", r#"{"height": 10}"#), "htmlFor", serde_json::json!("email")));
        elements.push(text(42, 41, "Email"));
        elements.push(with(typed(43, 1, "input", r#"{"height": 20}"#), "id", serde_json::json!("email")));
        elements
    }

    #[gpui::test]
    fn a_label_is_retained_and_changes_when_the_control_it_names_stops_taking_its_click(
        cx: &mut gpui::TestAppContext,
    ) {
        let scene = Scene::new(cx, labelled_input());
        scene.settle(cx);
        // An input rebuilds itself every frame, so the page above it is the spine.
        assert_eq!(scene.roots(cx), vec![10, 20, 30, 40, 41]);
        crate::perf_counters::reset();
        scene.set_prop(43, "disabled", serde_json::json!(true));
        scene.draw(cx);
        // The label's own subtree is untouched. What it reads of the control is whether a click
        // on it goes anywhere, which is in its shape, and the shape of what it holds follows it.
        assert_eq!(scene.roots(cx), vec![10, 20, 30, 40]);
        assert_eq!(Scene::counted("changed:shape"), 2);
        scene.settle(cx);
        assert_eq!(scene.roots(cx), vec![10, 20, 30, 40, 41]);
        scene.set_prop(43, "disabled", serde_json::json!(false));
        scene.draw(cx);
        assert_eq!(scene.roots(cx), vec![10, 20, 30, 40]);
    }

    #[gpui::test]
    fn an_element_an_aria_reference_names_changes_when_the_reference_does(cx: &mut gpui::TestAppContext) {
        let mut elements = page([CARD; 3]);
        elements.push(with(typed(40, 1, "div", r#"{"height": 20}"#), "id", serde_json::json!("hint")));
        elements.push(text(41, 40, "Be brief"));
        elements.push(typed(50, 1, "div", r#"{"height": 20}"#));
        let scene = Scene::new(cx, elements);
        scene.settle(cx);
        assert_eq!(scene.roots(cx), vec![1]);
        crate::perf_counters::reset();
        scene.set_prop(50, "aria-describedby", serde_json::json!("hint"));
        scene.draw(cx);
        // 40 is named by 50 now and gets a node for it, with nothing of its own having changed.
        // Its text is keyed by the shape it sits in.
        assert_eq!(Scene::counted("changed:shape"), 2);
        assert_eq!(scene.roots(cx), vec![10, 20, 30]);
        scene.settle(cx);
        assert_eq!(scene.roots(cx), vec![1]);
    }

    fn icon(size: &'static str, motion: Option<serde_json::Value>) -> Vec<El> {
        let mut elements = page([CARD; 3]);
        elements.push(typed(40, 1, "svg", size));
        let mut circle = with(
            with(typed(41, 40, "circle", "{}"), "cx", serde_json::json!(12)),
            "r",
            serde_json::json!(6),
        );
        if let Some(motion) = motion {
            circle = with(circle, "motion", motion);
        }
        elements.push(circle);
        elements
    }

    const SMALL_ICON: &str = r##"{"width": 24, "height": 24, "color": "#336699"}"##;

    #[gpui::test]
    fn an_svg_is_retained_once_it_has_been_painted(cx: &mut gpui::TestAppContext) {
        let scene = Scene::new(cx, icon(SMALL_ICON, None));
        scene.settle(cx);
        assert_eq!(scene.roots(cx), vec![1]);
        scene.set_prop(41, "r", serde_json::json!(8));
        scene.draw(cx);
        assert_eq!(scene.roots(cx), vec![10, 20, 30]);
        scene.settle(cx);
        assert_eq!(scene.roots(cx), vec![1]);
    }

    #[gpui::test]
    fn an_svg_whose_shapes_move_is_changed_every_frame_of_it(cx: &mut gpui::TestAppContext) {
        let elements = icon(
            SMALL_ICON,
            Some(serde_json::json!({
                "generation": 1,
                "initial": { "r": 2.0 },
                "animate": { "r": 10.0 },
                "transition": { "duration": 600.0, "ease": "linear" }
            })),
        );
        let scene = Scene::new(cx, elements);
        scene.settle(cx);
        crate::perf_counters::reset();
        for _ in 0..4 {
            scene.window.update(cx, |view, _, _| view.clock.fast_forward_ms(16.0)).unwrap();
            scene.draw(cx);
            // The circle is never built as an element; its animated attributes reach the svg
            // as part of the XML, and the svg is the one that changes.
            assert_eq!(scene.roots(cx), vec![10, 20, 30]);
        }
        assert!(Scene::counted("changed:shape") >= 4, "{:?}", crate::perf_counters::retain_reasons());
        scene.window.update(cx, |view, _, _| view.clock.fast_forward_ms(2000.0)).unwrap();
        scene.settle(cx);
        assert_eq!(scene.roots(cx), vec![1], "it came to rest");
    }

    #[gpui::test]
    fn an_svg_too_big_to_raster_in_paint_is_not_retained_until_the_raster_lands(cx: &mut gpui::TestAppContext) {
        let scene = Scene::new(cx, icon(r#"{"width": 300, "height": 200}"#, None));
        crate::perf_counters::reset();
        scene.draw(cx);
        assert!(Scene::counted("excluded:svg") >= 1, "{:?}", crate::perf_counters::retain_reasons());
        assert_eq!(scene.roots(cx), vec![10, 20, 30]);
        cx.run_until_parked();
        scene.settle(cx);
        assert_eq!(scene.roots(cx), vec![1]);
    }

    #[gpui::test]
    fn an_img_is_retained_once_it_has_loaded_and_changes_when_script_hands_it_an_image(
        cx: &mut gpui::TestAppContext,
    ) {
        let mut elements = page([CARD; 3]);
        elements.push(with(
            typed(40, 1, "img", r#"{"width": 16, "height": 16}"#),
            "src",
            serde_json::json!("data:image/svg+xml,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%20width%3D%2216%22%20height%3D%2216%22%2F%3E"),
        ));
        let scene = Scene::new(cx, elements);
        scene.settle(cx);
        assert_eq!(scene.roots(cx), vec![1]);
        crate::perf_counters::reset();
        scene
            .window
            .update(cx, |view, window, cx| {
                view.set_image_pixels(
                    40,
                    1,
                    1,
                    vec![255; 4],
                    crate::custom_elements::img::PixelFormat::Rgba,
                    window,
                    cx,
                )
                .unwrap();
            })
            .unwrap();
        // The app draws as soon as the image is set.
        scene.assert_sound(cx);
        // Nothing in the tree moved: the source is what the element keeps for itself.
        assert_eq!(Scene::counted("changed:shape"), 1, "{:?}", crate::perf_counters::retain_reasons());
        assert_eq!(scene.roots(cx), vec![10, 20, 30]);
        scene.settle(cx);
        assert_eq!(scene.roots(cx), vec![1]);
    }

    #[gpui::test]
    fn an_img_still_loading_and_an_input_are_never_retained(cx: &mut gpui::TestAppContext) {
        let mut elements = page([CARD; 3]);
        elements.push(with(
            typed(40, 1, "img", r#"{"width": 16, "height": 16}"#),
            "src",
            serde_json::json!("blob:never-arrives"),
        ));
        elements.push(typed(41, 1, "input", r#"{"height": 20}"#));
        let scene = Scene::new(cx, elements);
        scene.settle(cx);
        crate::perf_counters::reset();
        scene.draw(cx);
        assert_eq!(Scene::counted("excluded:custom-element"), 2);
        assert_eq!(scene.roots(cx), vec![10, 20, 30]);
    }
}
