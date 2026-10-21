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
//! (`Exclusion`), and no absolute box in it has its containing block above it. What it lifts into the
//! stacking context around it does not matter: gpui keeps a lifted element that is a root of its own
//! as a hole in the recording of the subtree around it, and any other as a fragment of it. The
//! maximal such subtrees are the would-be placeholders. They are counted, and every other
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
mod patch;

pub(crate) use patch::Patching;

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
        inherited.visibility_hidden,
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
    fp.option_f64(inherited.line_height);
    fp.f32(inherited.underline_reach);
    fp.hash(&inherited.computed_variations);
    inherited.font_state.fingerprint(&mut fp);
    inherited.direction.fingerprint(&mut fp);
    inherited.custom.fingerprint(&mut fp);
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
    /// An `<svg>` that has not been painted yet, whose raster is still being made, or whose document
    /// changed and is too big to be rasterised in paint.
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

    /// Whether what the element paints or lays out follows the clock or the arrival of something
    /// (a caret, a raster, an image, rows measured late), so that a tick which builds only what
    /// runs on a clock would leave it as it was.
    pub(crate) fn varies_with_time(self) -> bool {
        matches!(
            self,
            Self::Highlight | Self::CustomElement | Self::Svg | Self::VirtualList | Self::VirtualRow | Self::LayoutAnimation
        )
    }

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
    /// What runs on a `display: contents` element the build was spliced out of, which only its
    /// children are built for: nothing on the child says what it is.
    Spliced = 32,
}

impl Dynamic {
    /// The kinds that only ever run on the element they are noted on, so that what the element
    /// is running can be asked of its own state (`Sources::dormant`).
    pub(crate) const OWN: u8 = Self::Motion as u8 | Self::Transition as u8 | Self::Animation as u8 | Self::MotionValues as u8;
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
    /// `Node::opacity_run`.
    opacity_run: bool,
    /// The element or something in it is lifted out of it, which a recording is not moved with.
    lifts: bool,
    /// What `Below::holes` added up to when it was built: what the lifted elements in it, however
    /// deep, were keyed by.
    holes: u64,
    /// Elements in the subtree.
    size: u32,
    /// The element is costly to build, see [`Entry::costly`]: all that makes a root of one worth it.
    costly: bool,
    subtree: crate::layout::cull::Subtree,
    effects: Option<Rc<[BuildEffect]>>,
    /// Whether gpui has a recording of the subtree to replay: it asked for the build, or the frame
    /// before skipped the subtree.
    seasoned: bool,
    /// The selection the element was built under. The frame that finds a new selection builds
    /// only the elements it visits, so one it did not (under `display: none`) is still of the old.
    selection: u64,
    /// Nothing in the subtree is excluded but for the parts of a collapsed table, which the table
    /// the element is paints itself: `eligible`, or that.
    unit_eligible: bool,
    /// What runs where a unit of the element would be rebuilt, see [`Motion`].
    motion: Motion,
    /// Why the element is no unit though something runs on it, if the build says.
    refusal: Option<Refusal>,
}

/// Why an element something runs on is no unit: the first of these that holds of its last build.
/// What a tick that is drawn whole for it is counted under (`getPatchCounters().unpatchable`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// The element is of a kind a patch cannot repeat, see [`Exclusion`].
    Excluded(Exclusion),
    /// Something in it is.
    ExcludedBelow(Exclusion),
    /// It is absolutely positioned against a containing block outside it, or holds something that is:
    /// gpui keeps no root whose layout is rewritten against a box it does not hold.
    Escapes,
    /// Something in it is lifted out of it into a stacking context it does not form.
    Lifts,
    /// Something in it scrolls, which a scroll bypasses the tape a patch compares with.
    Scrolls,
    /// What runs on it moves what it lays out.
    LayoutAnimation,
    /// It holds more elements than a unit may.
    TooLarge,
}

impl Refusal {
    pub(crate) fn key(self) -> String {
        match self {
            Self::Excluded(kind) => format!("Excluded:{}", kind.name()),
            Self::ExcludedBelow(kind) => format!("ExcludedBelow:{}", kind.name()),
            Self::Escapes => "Escapes".to_owned(),
            Self::Lifts => "Lifts".to_owned(),
            Self::Scrolls => "Scrolls".to_owned(),
            Self::LayoutAnimation => "LayoutAnimation".to_owned(),
            Self::TooLarge => "TooLarge".to_owned(),
        }
    }
}

/// The motion a unit would repaint: what runs on the element, or for a table with collapsed
/// borders on the element and its parts, which it paints the borders of.
#[derive(Clone, Copy, Default)]
struct Motion {
    /// `Vol::dynamic` of the elements, or-ed.
    dynamic: u8,
    /// What runs on each of them only changes what it paints, see [`Retain::paints_only`].
    paints_only: bool,
}

impl Record {
    /// Whether the last build of the element found it a unit to be: only paint-only motion runs on
    /// it, and it lifts nothing out of it (`end_unit` judges the build for what is inside).
    fn is_unit_candidate(&self) -> bool {
        let only_motion = self.motion.dynamic & Dynamic::OWN != 0 && self.motion.dynamic & !Dynamic::OWN == 0;
        // `Record::lifts` is true of every lifted element: whether it holds a descendant that lifts
        // out of it is for `end_unit` to find in the build.
        let lifts_out = self.lifts && self.lift_z.is_none();
        self.unit_eligible && !lifts_out && only_motion && self.motion.paints_only
    }
}

const NO_PARENT: u32 = u32::MAX;

/// Where each reason is counted: `perf_counters` keeps one number per slot.
const EXCLUDED: usize = 0;
const UNDER: usize = EXCLUDED + Exclusion::ALL.len();
const CHANGED: usize = UNDER + Exclusion::ALL.len();
const SPINE: usize = CHANGED + Cause::ALL.len();
const ESCAPES: usize = SPINE + 1;
const MOVED: usize = ESCAPES + 1;
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
        ESCAPES => "escapes".to_owned(),
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
    /// An animation of the element's opacity was current when it was built: the element is an effect group
    /// at every opacity it samples, so `lift_z` holds for as long as the run does.
    opacity_run: bool,
    /// The element is a containing block for the absolutely positioned boxes below it.
    positioned: bool,
    /// The element is absolutely positioned: gpui rewrites its box against its containing block.
    absolute: bool,
    /// Something above scrolled, so the box this one was recorded at is not where it is.
    moved: bool,
    /// The scroller around it, if it is one gpui can move recordings for.
    scroller: Option<Scroller>,
    costly: bool,
    /// Only with `verify`: a hash of what the build made of the element itself.
    digest: u64,
    /// The element was built as a `display: none` host, which paints and lays out nothing.
    absent: bool,
    /// Where among the children it built the ones that came back absent were.
    absent_children: Vec<usize>,
    subtree: crate::layout::cull::Subtree,
    /// The build effects noted since the element was entered, and until it was left.
    effects_start: usize,
    effects_end: usize,
    skipped: Option<Skipped>,
    paints_only: bool,
    /// The element is a table with collapsed borders, which paints the borders of its parts.
    collapsed_table: bool,
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
    /// The tokens of the lifted elements below that this one's recording holds a hole or a
    /// fragment for.
    holes: u64,
    epoch: u64,
    token: u64,
    /// Something below is absolutely positioned against a containing block that is not below this
    /// element, or is this element.
    escapes: bool,
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
    /// The elements a Web Animations command touched since the frame before. A transition that had
    /// finished runs again after `play()`, and nothing in its last build said it could.
    pub commanded: Vec<u64>,
    /// The elements with `content-visibility: auto` that are to show or skip their content
    /// otherwise than their last build did (`layout::content_visibility::Relevance::pending`).
    pub relevance: Vec<u64>,
    /// The state each custom element says it keeps for itself, see `CustomElement::retain_state`.
    pub customs: Vec<(u64, Option<u64>)>,
    /// The elements with a motion that is running.
    pub motion: Vec<u64>,
    /// Whether the element can be left alone while it is not seen: everything that runs on it
    /// only changes what it paints, and the last frame that reached it culled it (gpui walked it
    /// under an empty content mask). Chrome paints no frame for such a change, and a frame that
    /// builds it again when it shows reads the clock, so nothing it would have built is lost.
    pub dormant: &'a dyn Fn(u64) -> bool,
    /// Everything `dormant` reads that no build of an element tells the frame about: it changes
    /// when the answer may have changed for an element nothing built, so the elements that were
    /// found dormant are asked again.
    pub verdicts: u64,
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
    /// The pass only lays out (a bounds read): it is not a frame, and its output is thrown away.
    pub layout_only: bool,
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

/// The elements that became dormant and the ones that stopped being, for whoever keeps their
/// clocks (`CssAnimations::enter_dormant`).
#[derive(Default)]
pub(crate) struct DormantMoves {
    pub entered: Vec<u64>,
    pub left: Vec<u64>,
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

/// What the sources differ in from the frame before, see `Retain::compute_deltas`.
struct Found {
    all: bool,
    /// Changed by the pointer, focus, a scroll or a custom element: not by time.
    sourced: Vec<u64>,
    scrolled: FxHashSet<u64>,
    scrolling: bool,
    scrolls: FxHashMap<u64, (u32, u32)>,
    customs: FxHashMap<u64, Option<u64>>,
    motion: FxHashSet<u64>,
    /// Motions that run or just ended and are not dormant.
    running: Vec<u64>,
    /// Elements whose last build found something running on them, that are not dormant.
    dynamic: Vec<u64>,
    /// The elements that were asked whether they are dormant and are.
    entered: FxHashSet<u64>,
    /// The verdicts of the elements that were dormant are not the ones they were found with.
    stale: bool,
}

/// The most elements a unit holds. A unit is built whole in every frame that is built whole (nothing
/// inside it is replayed, so that a tick can build it again exactly) and again by every tick that
/// patches it, so what a patch saves over a frame is the page minus its units: a unit past this
/// has more to build in each of them than a card does, and the page is left to full frames.
const UNIT_MAX_ELEMENTS: u32 = 256;

/// A subtree of the last frame built whole that only its running animations change, kept with what
/// builds it again, see `GpuixView::build_unit`.
pub(crate) struct Unit {
    pub id: u64,
    pub env: crate::renderer::FrameEnv,
}

/// What the last frame built whole left for a tick to be patched against.
#[derive(Default)]
struct Plan {
    /// Counts the frames and passes that began, for the key of a unit to differ from the one of the
    /// frame before: a unit is recorded every frame, never replayed.
    serial: u64,
    /// A frame was recorded since the sources were last reset.
    recorded: bool,
    /// A pass that only lays out ran after it, and took the changes it saw out of the sources.
    layout_since: bool,
    /// It built an element whose paint follows the clock or something arriving
    /// ([`Exclusion::varies_with_time`]), which a patch would leave as it was.
    unsteady: bool,
    /// The revision of the root of the tree it was built from.
    revision: u64,
    units: Vec<Unit>,
    ids: FxHashSet<u64>,
}

/// Why a tick cannot be patched, see `Retain::preview`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unpatchable {
    /// No frame built whole is recorded: the retention is off, resting, or was started over.
    NoFrame,
    /// A pass that only lays out ran since, and took the changes it saw.
    LayoutPass,
    /// The frame built an element whose paint follows the clock, see [`Plan::unsteady`].
    Unsteady,
    ScrollAnimation,
    /// The tree is not the one the frame was built from.
    Edited,
    /// The selection changed, or the sources never had a frame to compare with.
    Everything,
    /// Something other than time changed: the pointer, focus, a scroll, a custom element, a
    /// measurement, a link, a command.
    Sourced,
    /// A time-driven change outside every unit that is not dormant, on an element the last build
    /// gives no reason for.
    Outside(u64),
    /// A time-driven change outside every unit that is not dormant, on an element that is no unit
    /// for this reason.
    Refused(Refusal),
    /// A unit that the last frame found nothing left running on (the animation ended in it, while
    /// a source goes on listing it): the frame built whole in the tick's place does not make it a
    /// unit, so a patch would repaint a root that frame does not have.
    Ended(u64),
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
    /// The pass being built only lays out, or is built again to be compared with the frame shown.
    layout_only: bool,
    reference: bool,
    /// The node a unit began at, while it is being built: nothing inside it is skipped.
    unit: Option<u32>,
    plan: Plan,
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
    dynamic: FxHashMap<u64, u8>,
    /// The elements the frames left out of `changed` because they are dormant (`Sources::dormant`),
    /// with the kinds that were running on them when they fell asleep. Their records stay as they
    /// are: the roots above them are replayed. An element is asked once, and sleeps until
    /// something can have changed its answer: a build of it (`woken`), or what `dormant` reads
    /// that no build tells (`verdicts`). Not in `dynamic`, so a frame looks at none of them.
    dormant: FxHashMap<u64, u8>,
    /// The dormant elements a build reached since the frame that last asked about them. They are
    /// still in `dormant` until that frame, which is what the frame that built them saw.
    woken: FxHashSet<u64>,
    /// `Sources::verdicts` when the dormant elements were last asked about.
    verdicts: u64,
    /// The elements that fell asleep or woke up since `take_dormant_moves`.
    moves: DormantMoves,
    /// The elements that skip their content (`content-visibility`) as of their last build.
    skipping: FxHashSet<u64>,
    /// `RetainedTree::removals` when `records` and `dynamic` last dropped the destroyed elements.
    pruned_at: u64,
    seen: Seen,
    deltas: Deltas,
    /// The elements on the spine of a frame since the last build of them. A pass that does not
    /// build one (`display: none` above it, for the layout of a read) does not see what it
    /// changed, and `seen` has moved on, so the frame that does must still find it changed.
    ///
    /// Only elements a build can enter: one it never enters (see
    /// [`RetainedElement::is_built_by_ancestor`]) would stay here for good, and its ancestors
    /// with it, since the spine is the upward closure of this set and what changed.
    unbuilt: FxHashSet<u64>,
    summary: Summary,
    /// The element `exit` left was built absent, until `take_exited_absent`.
    exited_absent: bool,
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
    /// hand down (`layout::contents::Overlay::key`).
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
    /// Whether an animation of the element's opacity is current now. It is the one input of the lift
    /// that moves with the clock and not with a revision: a record made on the other side of the
    /// start or the end of a run says the element is lifted, or is not, wrongly.
    pub opacity_run: &'a dyn Fn() -> bool,
    /// Whether building the element alone costs more than replaying it: it carries text or is
    /// not a plain host box (an image, an `<svg>`). Only a root of one element needs this.
    pub costly: bool,
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
    InUnit,
    Excluded,
    Moved,
    Spine,
    NoRecord,
    Ineligible,
    TooSmall,
    Revision,
    Inherited,
    Shape,
    OpacityRun,
}

impl Kept {
    fn name(self) -> &'static str {
        match self {
            Self::PlanningOff => "planning-off",
            Self::Everything => "deltas-all",
            Self::NotPlain => "not-plain",
            Self::NotBoxed => "not-boxed",
            Self::Prekeyed => "prekeyed",
            Self::InUnit => "in-unit",
            Self::Excluded => "excluded-now",
            Self::Moved => "moved",
            Self::Spine => "spine",
            Self::NoRecord => "no-record",
            Self::Ineligible => "ineligible",
            Self::TooSmall => "too-small",
            Self::Revision => "enter-revision",
            Self::Inherited => "enter-inherited",
            Self::Shape => "enter-shape",
            Self::OpacityRun => "opacity-run",
        }
    }
}

/// A root of fewer elements than this is worth a retained element of its own only if the element
/// is costly to build (`Entry::costly`). Replaying a root has a cost of its own, about what
/// building a plain box costs: 2000 childless boxes in a scroller that nothing changes took 1.3
/// times as long replayed as built (a rounded box 1.0, a text 0.4, since a text is the build that
/// is worth skipping). Measured on the pages of the FF demo (showcase, changelog, command menu,
/// hover, introduction), the texts and icons that are roots of one element are 0.5 to 0.8 ms of
/// 3 to 6 ms per idle frame. `GPUIX_RETAIN_MIN_ROOT=1` takes every box, to measure.
const MIN_ROOT: u32 = 2;

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
            layout_only: false,
            reference: false,
            unit: None,
            plan: Plan::default(),
            rows: 0,
            selection: 0,
            nodes: Vec::new(),
            stack: Vec::new(),
            below: Vec::new(),
            retained: Vec::new(),
            moved: Vec::new(),
            records: FxHashMap::default(),
            dynamic: FxHashMap::default(),
            dormant: FxHashMap::default(),
            skipping: FxHashSet::default(),
            woken: FxHashSet::default(),
            verdicts: 0,
            moves: DormantMoves::default(),
            pruned_at: 0,
            seen: Seen::default(),
            deltas: Deltas::default(),
            unbuilt: FxHashSet::default(),
            summary: Summary::default(),
            exited_absent: false,
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

    /// Whether a build is being hashed (`digest`): the one that builds a frame whole to compare it.
    pub(crate) fn verifying(&self) -> bool {
        self.verify && !self.planning
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
        self.release_dormant(false);
        self.plan.recorded = false;
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
        [self.records.len(), self.dynamic.len() + self.dormant.len()]
    }

    /// Whether the frame being built (or the last one) leaves `id` dormant, see `Sources::dormant`.
    pub(crate) fn is_dormant(&self, id: u64) -> bool {
        self.dormant.contains_key(&id)
    }

    /// The elements that fell asleep or woke up in the frames since the last call.
    pub(crate) fn take_dormant_moves(&mut self) -> DormantMoves {
        std::mem::take(&mut self.moves)
    }

    /// Every dormant element wakes. `restore` puts what ran on them back among the dynamic
    /// elements, for the frame after to ask about them as one that was never dormant.
    fn release_dormant(&mut self, restore: bool) {
        for (id, bits) in std::mem::take(&mut self.dormant) {
            self.moves.left.push(id);
            if restore && bits != 0 {
                self.dynamic.entry(id).or_insert(bits);
            }
        }
        self.woken.clear();
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
        self.layout_only = sources.layout_only;
        self.reference = sources.reference;
        self.unit = None;
        self.plan.serial += 1;
        if sources.layout_only {
            self.plan.layout_since = true;
        } else if !sources.reference {
            self.plan = Plan {
                serial: self.plan.serial,
                revision: sources
                    .tree
                    .root_id
                    .and_then(|id| sources.tree.elements.get(&id))
                    .map_or(0, |root| root.subtree_revision),
                ..Plan::default()
            };
        }
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
        // The pass that rebuilds a frame to compare it plans nothing, and builds the elements the
        // frame left dormant, which must stay dormant for what they ask of the window.
        if !sources.reference && !(self.open && self.planning) {
            self.release_dormant(true);
        }
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
        self.dynamic.retain(|id, _| tree.elements.contains_key(id));
        self.unbuilt.retain(|id| tree.elements.contains_key(id));
        let moves = &mut self.moves;
        self.dormant.retain(|id, _| {
            let exists = tree.elements.contains_key(id);
            if !exists {
                moves.left.push(*id);
            }
            exists
        });
        self.woken.retain(|id| tree.elements.contains_key(id));
        self.skipping.retain(|id| tree.elements.contains_key(id));
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

    /// Whether a root of `size` elements saves more than replaying it costs. A verified run takes
    /// every root, since it is replay that it checks.
    fn worth(&self, size: u32, costly: bool) -> bool {
        self.verify || size >= self.min_root || costly
    }

    /// A verified run records every frame, since it is the recording that it checks.
    fn backs_off(&self) -> bool {
        self.planning && !self.verify
    }

    /// What changed in `sources` since the frame before, as the elements that differ and their
    /// ancestors.
    fn find_deltas(&mut self, sources: &Sources) {
        let found = self.compute_deltas(sources);
        self.commit_deltas(sources, found);
    }

    /// What `sources` differ in from what the frame before saw. Changes nothing, so that a tick can
    /// ask whether a frame would find something without being that frame.
    fn compute_deltas(&self, sources: &Sources) -> Found {
        let mut changed: Vec<u64> = Vec::new();
        let seen = &self.seen;
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

        // A motion element answers to the gestures of the ancestor that controls its variants,
        // which are not on its own chain.
        if !changed.is_empty() {
            let moved: FxHashSet<u64> = changed.iter().copied().collect();
            for &id in &sources.tree.motion_ids {
                if crate::motion::gesture_controller(sources.tree, id).is_some_and(|controller| moved.contains(&controller)) {
                    changed.push(id);
                }
            }
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

        let motion: FxHashSet<u64> = sources.motion.iter().copied().collect();
        // A dormant element is not a change: the roots above it keep their recording, and a frame
        // that shows it builds the root late, with the clock of that frame. One that was found
        // dormant is not asked about again until a build reaches it or `verdicts` moves.
        let stale = sources.verdicts != self.verdicts;
        let sleeps = |id: u64| !stale && self.dormant.contains_key(&id) && !self.woken.contains(&id);
        let mut entered = FxHashSet::default();
        let mut running = Vec::new();
        for &id in &motion {
            if sleeps(id) {
                continue;
            }
            if (sources.dormant)(id) {
                entered.insert(id);
            } else {
                running.push(id);
            }
        }
        running.extend(seen.motion.iter().filter(|id| !motion.contains(id)));
        let mut dynamic = Vec::new();
        // Everything that is asleep is asked again when the answers may have moved.
        let sleepers = self.dormant.iter().filter(|(id, bits)| stale && **bits != 0 && !self.dynamic.contains_key(id));
        for (&id, &bits) in self.dynamic.iter().chain(sleepers) {
            if entered.contains(&id) || (bits & !Dynamic::OWN == 0 && (sources.dormant)(id)) {
                entered.insert(id);
            } else {
                dynamic.push(id);
            }
        }
        Found { all, sourced: changed, scrolled, scrolling, scrolls, customs, motion, running, dynamic, entered, stale }
    }

    /// Whether `id` is dormant in the frame `found` is the delta of.
    fn asleep_in(&self, found: &Found, id: u64) -> bool {
        found.entered.contains(&id) || (!found.stale && self.dormant.contains_key(&id) && !self.woken.contains(&id))
    }

    /// Makes what `compute_deltas` found the delta of the frame being built, and what the next
    /// frame is compared with.
    fn commit_deltas(&mut self, sources: &Sources, found: Found) {
        let Found { all, sourced, scrolled, scrolling, scrolls, customs, motion, running, dynamic, entered, stale } = found;
        let mut changed = sourced;
        let seen = &mut self.seen;
        self.debug.sources("pointer-focus-scroll-custom", changed.iter().copied());
        self.debug.sources("motion-just-ended", seen.motion.iter().copied().filter(|id| !sources.motion.contains(id)));
        self.debug.sources("motion-running", running.iter().copied());
        self.debug.sources("dynamic-last-build", dynamic.iter().copied());
        self.debug.sources("dormant", entered.iter().copied());
        self.debug.sources("measurement", sources.measured.iter().copied());
        self.debug.sources("link", sources.links.iter().copied());
        self.debug.sources("web-animation-command", sources.commanded.iter().copied());
        self.debug.sources("content-visibility", sources.relevance.iter().copied());
        changed.extend(running);
        changed.extend(&sources.measured);
        changed.extend(&sources.links);
        changed.extend(&sources.commanded);
        changed.extend(&sources.relevance);
        changed.extend(dynamic);

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
        self.settle_dormant(sources.verdicts, stale, entered);

        // Closed under parent, which is what makes stopping at the first id already in it right:
        // every id it holds had its whole chain walked. Seeding it with `unbuilt` as it is would
        // not be, as the built ancestors of an unbuilt id have left that set.
        let mut spine = FxHashSet::default();
        for id in self.unbuilt.iter().copied().chain(changed) {
            let mut at = Some(id);
            while let Some(id) = at {
                if !spine.insert(id) {
                    break;
                }
                at = sources.tree.elements.get(&id).and_then(|element| element.parent);
            }
        }
        if self.verify {
            check_spine(&spine, sources.tree, &mut self.mismatches);
        }
        self.unbuilt = spine
            .iter()
            .copied()
            .filter(|id| sources.tree.elements.get(id).is_none_or(|element| !element.is_built_by_ancestor(sources.tree)))
            .collect();
        self.deltas = Deltas { all, spine, scrolled, scrolling };
    }

    /// Makes the elements `compute_deltas` found dormant the dormant ones, and wakes those it asked
    /// about again and found otherwise.
    fn settle_dormant(&mut self, verdicts: u64, stale: bool, entered: FxHashSet<u64>) {
        let asked: Vec<u64> = if stale {
            self.dormant.keys().copied().collect()
        } else {
            self.woken.iter().copied().filter(|id| self.dormant.contains_key(id)).collect()
        };
        for id in asked {
            if entered.contains(&id) {
                continue;
            }
            let bits = self.dormant.remove(&id).expect("asked about the dormant");
            self.moves.left.push(id);
            // One that a build reached has what ran on it from that build.
            if bits != 0 && !self.woken.contains(&id) {
                self.dynamic.entry(id).or_insert(bits);
            }
        }
        for id in entered {
            let running = self.dynamic.remove(&id);
            match self.dormant.entry(id) {
                std::collections::hash_map::Entry::Occupied(mut asleep) => {
                    if let Some(bits) = running {
                        *asleep.get_mut() = bits;
                    }
                }
                std::collections::hash_map::Entry::Vacant(slot) => {
                    slot.insert(running.unwrap_or(0));
                    self.moves.entered.push(id);
                }
            }
        }
        self.woken.clear();
        self.verdicts = verdicts;
    }

    /// Starts the build of a subtree that gpui asked for after the frame was built, because the
    /// frame before did not leave it recorded as the same, or does not lay out where it was.
    pub(crate) fn begin_deferred(&mut self, inside_relocatable: bool) {
        self.open = self.enabled && !self.backoff.resting;
        self.deferred = true;
        self.unit = None;
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

    /// Stops recording, for a build that is not part of a frame (`GpuixView::build_unit`): nothing
    /// is entered, skipped or noted until `resume` gives back what it returns.
    pub(crate) fn suspend(&mut self) -> bool {
        debug_assert!(self.stack.is_empty(), "a build is not suspended inside another");
        std::mem::replace(&mut self.open, false)
    }

    pub(crate) fn resume(&mut self, open: bool) {
        self.open = open;
    }

    /// Whether the element being built is recorded, so the caller can skip work only the record needs.
    pub(crate) fn recording(&self) -> bool {
        self.open && self.rows == 0
    }

    /// The element being built (not skipped, just entered) is the root of a unit if the last build
    /// of it found only paint-only motion running on it, nothing is dormant, and the build
    /// around it hands it nothing it could not be built again without (`plain`). Nothing inside
    /// a unit is skipped, and `end_unit` says whether what was built is one. Returns the key the
    /// element was entered with, for the unit to be built again with.
    pub(crate) fn begin_unit(&mut self, plain: bool) -> Option<EnterKey> {
        if !plain
            || !self.planning
            || !self.recording()
            || self.deferred
            || self.layout_only
            || self.unit.is_some()
        {
            return None;
        }
        let at = *self.stack.last()?;
        let node = &self.nodes[at as usize];
        if node.exclusion.is_some() || node.skipped.is_some() || self.dormant.contains_key(&node.id) {
            return None;
        }
        let record = self.records.get(&node.id)?;
        if !record.is_unit_candidate() {
            return None;
        }
        if record.size > UNIT_MAX_ELEMENTS {
            crate::perf_counters::unit_oversized();
            return None;
        }
        self.unit = Some(at);
        Some(EnterKey(node.enter))
    }

    /// The build of the element that `begin_unit` made a unit is over: whether what it built is
    /// one, which is judged by the build and not by the record of the build before. The element
    /// and what it holds are exactly the nodes since, and a unit holds none that is excluded, that
    /// scrolls (a scroll bypasses the tape that a patch compares), lifts something out of it, or
    /// is positioned against something outside it. The element itself may be lifted, into the
    /// stacking context around it, as an element with an opacity is: the `z` it is lifted at is
    /// returned with the subtree, for the unit to be handed over as gpui wants it, and what it holds
    /// must be consumed by the context it forms, or there is none (a positioned element
    /// without a `z-index`), and nothing in it is lifted.
    pub(crate) fn end_unit(&mut self) -> Option<(crate::layout::cull::Subtree, Option<i32>)> {
        let at = *self.stack.last()?;
        if self.unit != Some(at) {
            return None;
        }
        self.unit = None;
        let first = at as usize;
        let size = self.nodes.len() - first;
        if size > UNIT_MAX_ELEMENTS as usize {
            crate::perf_counters::unit_oversized();
            return None;
        }
        if self.nodes[first].absent {
            return None;
        }
        // Children come after their parent, as in `finish_session`.
        let mut lifts = vec![false; size];
        let mut escapes = vec![false; size];
        let parts_of_unit = self.nodes[first].collapsed_table;
        for index in (0..size).rev() {
            let node = &self.nodes[first + index];
            let excluded = node.exclusion.is_some_and(|reason| index == 0 || !parts_of_unit || reason != Exclusion::CollapsedTable);
            if excluded || node.skipped.is_some() || node.vol.scroll != 0 {
                return None;
            }
            let lifts_below = !node.forms_context && lifts[index];
            let lifts_out = node.lift_z.is_some() || lifts_below;
            let escaped = node.absolute || (!node.positioned && escapes[index]);
            if index == 0 {
                if lifts_below || escaped {
                    return None;
                }
                break;
            }
            let parent = node.parent as usize - first;
            lifts[parent] |= lifts_out;
            escapes[parent] |= escaped;
        }
        crate::perf_counters::unit_built();
        Some((self.nodes[first].subtree, self.nodes[first].lift_z))
    }

    /// Keeps the unit built since `begin_unit` for the ticks that follow the frame.
    pub(crate) fn add_unit(&mut self, unit: Unit) {
        // The pass that rebuilds a frame for comparison (after a patch it is the frame shown) wraps
        // the units of the frame it stands for, and leaves the plan as that frame made it.
        if self.reference {
            return;
        }
        self.plan.ids.insert(unit.id);
        self.plan.units.push(unit);
    }

    /// What differs between the keys of two units of the same element in two frames.
    pub(crate) fn unit_key(&self) -> u64 {
        self.plan.serial
    }

    /// The unit of the last frame built whole that is the element `id`.
    pub(crate) fn unit(&self, id: u64) -> Option<&Unit> {
        self.plan.units.iter().find(|unit| unit.id == id)
    }

    /// The units of the last frame built whole, in the order it built them.
    #[cfg(test)]
    pub(crate) fn units(&self) -> &[Unit] {
        &self.plan.units
    }

    /// Notes that what runs on the element being built only changes what it paints (or does not):
    /// the element is the root of a unit in a later frame only if it does.
    pub(crate) fn paints_only(&mut self, paints_only: bool) {
        if let Some(node) = self.top() {
            node.paints_only = paints_only;
        }
    }

    /// Whether a tick that finds `sources` can be drawn by building again the units that hold what
    /// changed, and which they are (by element id, in the order the last frame built them). It
    /// can when, since the last frame that was built whole, nothing but time changed (the animations
    /// that run, a motion that ended, an element not yet built), every element it changed is in a
    /// unit or is dormant, the tree is the one that frame was built from, and that frame built
    /// nothing that follows the clock outside the units. Changes nothing: a tick that is not drawn
    /// this way is drawn whole, and finds the same changes.
    ///
    /// `scroll_animating` says whether a scroll animation (or a scrollbar fading) has frames to run.
    /// `sources` is what the tick would be built from, with the peeks of the sources that are taken
    /// when read (`Measurements::peek_changed`, `links::peek_changed`, `take_commanded`'s peek).
    pub(crate) fn preview(&self, sources: &Sources, scroll_animating: bool) -> Result<Vec<u64>, Unpatchable> {
        if !self.planning || !self.plan.recorded || !self.seen.known {
            return Err(Unpatchable::NoFrame);
        }
        if self.plan.layout_since {
            return Err(Unpatchable::LayoutPass);
        }
        if self.plan.unsteady {
            return Err(Unpatchable::Unsteady);
        }
        if scroll_animating {
            return Err(Unpatchable::ScrollAnimation);
        }
        let revision = sources
            .tree
            .root_id
            .and_then(|id| sources.tree.elements.get(&id))
            .map_or(0, |root| root.subtree_revision);
        if revision != self.plan.revision {
            return Err(Unpatchable::Edited);
        }
        let found = self.compute_deltas(sources);
        if found.all {
            return Err(Unpatchable::Everything);
        }
        if !found.sourced.is_empty() || !sources.measured.is_empty() || !sources.links.is_empty() || !sources.commanded.is_empty() || !sources.relevance.is_empty() {
            // The boxes a layout animation moves are measured, which is a source of its own in the
            // ticks they land in: the tick is whole because of the animation.
            let lays_out = found
                .running
                .iter()
                .chain(&found.dynamic)
                .filter(|id| !self.asleep_in(&found, **id))
                .any(|id| self.records.get(id).is_some_and(|record| record.refusal == Some(Refusal::LayoutAnimation)));
            return Err(if lays_out { Unpatchable::Refused(Refusal::LayoutAnimation) } else { Unpatchable::Sourced });
        }
        let mut touched = FxHashSet::default();
        for &id in found.running.iter().chain(&found.dynamic).chain(&self.unbuilt) {
            if self.asleep_in(&found, id) {
                continue;
            }
            let mut at = Some(id);
            let unit = loop {
                match at {
                    Some(up) if self.plan.ids.contains(&up) => break Some(up),
                    Some(up) => at = sources.tree.elements.get(&up).and_then(|element| element.parent),
                    None => break None,
                }
            };
            match unit {
                Some(unit) => {
                    touched.insert(unit);
                }
                None => {
                    return Err(match self.records.get(&id).and_then(|record| record.refusal) {
                        Some(refusal) => Unpatchable::Refused(refusal),
                        None => Unpatchable::Outside(id),
                    });
                }
            }
        }
        let units: Vec<u64> = self
            .plan
            .units
            .iter()
            .map(|unit| unit.id)
            .filter(|id| touched.contains(id))
            .collect();
        if let Some(&ended) = units
            .iter()
            .find(|id| !self.records.get(id).is_some_and(Record::is_unit_candidate))
        {
            return Err(Unpatchable::Ended(ended));
        }
        Ok(units)
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
            opacity_run: false,
            positioned: false,
            absolute: false,
            moved,
            scroller: entry.scroller,
            costly: entry.costly,
            digest: 0,
            absent: false,
            absent_children: Vec::new(),
            subtree: crate::layout::cull::Subtree::empty(),
            effects_start: crate::tape::build_effects_len(),
            effects_end: 0,
            skipped: None,
            paints_only: false,
            collapsed_table: false,
        };
        let at = self.nodes.len() as u32;
        match self.skippable(&node, &entry) {
            Ok(record) => {
                node.key = record.key;
                node.vol = record.vol;
                node.lift_z = record.lift_z;
                node.forms_context = record.forms_context;
                node.opacity_run = record.opacity_run;
                node.subtree = record.subtree;
                node.end = at + 1;
                node.skipped = Some(Skipped { size: record.size, digest: record.digest });
                crate::perf_counters::elements_skipped(u64::from(record.size));
                let overscan = node
                    .scroller
                    .filter(|scroller| scroller.holds(node.id) && !record.lifts)
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
    fn relocates(&self, node: &Node, record: Option<&Record>) -> bool {
        !self.relocation.resting
            && record.is_some_and(|record| !record.lifts)
            && node.scroller.is_some_and(|scroller| scroller.holds(node.id))
    }

    /// The record of the last build of the element if the frame can leave it unbuilt, or the
    /// first reason it cannot.
    fn skippable(&self, node: &Node, entry: &Entry) -> Result<&Record, Kept> {
        if !self.planning || self.lifted != 0 {
            return Err(Kept::PlanningOff);
        }
        // A unit is built whole, so that a tick can build it again to what it is, not to what is
        // left of it.
        if self.unit.is_some() {
            return Err(Kept::InUnit);
        }
        if self.deltas.all {
            return Err(Kept::Everything);
        }
        if self.records.get(&node.id).is_some_and(|record| record.selection != self.selection) {
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
            || (node.moved && !self.relocates(node, self.records.get(&node.id)))
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
        if !self.worth(record.size, record.costly) {
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
        // The wrapper of a skipped root carries the lift of the record, and the build that replaces
        // a replay that cannot be used makes the element as it is lifted now.
        if record.opacity_run != (entry.opacity_run)() {
            return Err(Kept::OpacityRun);
        }
        Ok(record)
    }

    pub(crate) fn exit(&mut self) {
        self.exited_absent = false;
        if self.rows > 0 || !self.open {
            return;
        }
        // A skipped element was never pushed.
        let at = self.stack.pop().expect("exit without enter");
        let end = self.nodes.len() as u32;
        let node = &mut self.nodes[at as usize];
        node.end = end;
        node.effects_end = crate::tape::build_effects_len();
        self.exited_absent = node.absent;
    }

    /// The element being built is a `display: none` host. A parent that built it anyway, because
    /// it still counted a box for it, has the same digest as one that left it out: the frame a
    /// `display` transition ends in is built with it, the next frame without.
    pub(crate) fn absent(&mut self) {
        if let Some(node) = self.top() {
            node.absent = true;
        }
    }

    /// Whether the element left last came back absent, once: the parent says where it put it.
    pub(crate) fn take_exited_absent(&mut self) -> bool {
        std::mem::take(&mut self.exited_absent)
    }

    /// The element being built put a child that came back absent at `index` among its content.
    pub(crate) fn absent_child(&mut self, index: usize) {
        if let Some(node) = self.top() {
            node.absent_children.push(index);
        }
    }

    /// Where the children of the element being built that came back absent are.
    pub(crate) fn absent_children(&mut self) -> Vec<usize> {
        self.top().map(|node| std::mem::take(&mut node.absent_children)).unwrap_or_default()
    }

    fn top(&mut self) -> Option<&mut Node> {
        if self.rows > 0 {
            return None;
        }
        let at = *self.stack.last()?;
        Some(&mut self.nodes[at as usize])
    }

    /// The first reason wins: it is the one the element is counted under. The parts of a collapsed
    /// table are excluded on entry for every element below the table, which says nothing of
    /// the others: a reason found in the build replaces it, so that a unit that is the table can
    /// tell the parts the table paints from what is excluded for another reason.
    pub(crate) fn exclude(&mut self, reason: Exclusion) {
        let Some(reason) = self.unless_lifted(reason) else { return };
        if let Some(node) = self.top() {
            match node.exclusion {
                Some(first) if first != Exclusion::CollapsedTable => {}
                _ => node.exclusion = Some(reason),
            }
        }
    }

    /// The element being built skips its content (`content-visibility`): nothing below it is built,
    /// and what the frames before recorded of what is below is of builds no frame replays. A
    /// subtree that shows again is built whole, and nothing of it keeps a frame busy while it is
    /// skipped: no record, no running motion, no sleep. Returns the elements that were dormant, for
    /// whoever keeps their clocks. Does its work when the element begins to skip, and is cheap
    /// while it goes on.
    pub(crate) fn skip_content(&mut self, tree: &crate::retained_tree::RetainedTree, id: u64) -> Vec<u64> {
        if !self.skipping.insert(id) {
            return Vec::new();
        }
        let mut asleep = Vec::new();
        let mut pending: Vec<u64> = tree.elements.get(&id).map(|element| element.children.clone()).unwrap_or_default();
        while let Some(below) = pending.pop() {
            self.records.remove(&below);
            self.dynamic.remove(&below);
            self.unbuilt.remove(&below);
            self.woken.remove(&below);
            if self.dormant.remove(&below).is_some() {
                self.moves.left.push(below);
                asleep.push(below);
            }
            if let Some(element) = tree.elements.get(&below) {
                pending.extend(&element.children);
            }
        }
        asleep
    }

    /// The element being built shows its content.
    pub(crate) fn show_content(&mut self, id: u64) {
        if !self.skipping.is_empty() {
            self.skipping.remove(&id);
        }
    }

    /// The element being built is a table with collapsed borders. A unit that is the table builds
    /// its parts again with it, since it is the table that paints their borders.
    pub(crate) fn collapsed_table(&mut self) {
        if let Some(node) = self.top() {
            node.collapsed_table = true;
        }
    }

    /// Whether the element is a containing block (anything but `position: static`) and whether it
    /// is absolutely positioned. The nodes of an absolute box are rewritten by gpui's layout
    /// against its containing block after they are requested, so a subtree can be kept only if
    /// the containing block of every absolute box in it is in it too (gpui checks the same).
    pub(crate) fn containing_block(&mut self, positioned: bool, absolute: bool) {
        if let Some(node) = self.top() {
            node.positioned = positioned;
            node.absolute = absolute;
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
        self.dynamic_bits(kind as u8);
    }

    /// `Dynamic` kinds as bits. A spliced child notes what runs on the `display: contents` element
    /// it came out of: that element is never built, so it cannot say so itself, and its child's
    /// record is what puts the way up to it on the spine while it runs.
    pub(crate) fn dynamic_bits(&mut self, bits: u8) {
        if bits == 0 {
            return;
        }
        if let Some(node) = self.top() {
            node.vol.dynamic |= bits;
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

    /// Whether an animation of the element's opacity is current in this build.
    pub(crate) fn opacity_run(&mut self, runs: bool) {
        if let Some(node) = self.top() {
            node.opacity_run = runs;
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

    /// Whether the build of the node `at` can be a unit if nothing else is against it, and the motion
    /// that would be repainted. A table with collapsed borders paints the borders of its parts, so
    /// the parts of the table are excluded from everything but a unit that is the table, and the motion
    /// that repaints them is the motion of the table and what is in it.
    fn unit_facts(&self, at: usize, eligible: bool, escaped: bool) -> (bool, Motion) {
        let node = &self.nodes[at];
        let own = Motion { dynamic: node.vol.dynamic, paints_only: node.paints_only };
        if !node.collapsed_table || node.skipped.is_some() {
            return (eligible, own);
        }
        let inside = &self.nodes[at + 1..node.end as usize];
        let only_parts = node.exclusion.is_none()
            && !escaped
            && inside.iter().all(|inner| inner.exclusion.is_none_or(|reason| reason == Exclusion::CollapsedTable));
        // An element with nothing running on it says nothing about whether it paints only.
        let start = Motion { paints_only: own.paints_only || own.dynamic == 0, ..own };
        let motion = inside.iter().fold(start, |motion, inner| Motion {
            dynamic: motion.dynamic | inner.vol.dynamic,
            paints_only: motion.paints_only && (inner.vol.dynamic == 0 || inner.paints_only),
        });
        (eligible || only_parts, motion)
    }

    /// Why the node `at`, which something runs on, is no unit, in the order `begin_unit` and `end_unit`
    /// refuse: what they judge of the build is judged here of the same facts.
    fn refusal(&self, at: usize, own: Below, escaped: bool, unit_eligible: bool, motion: Motion) -> Option<Refusal> {
        let node = &self.nodes[at];
        if !unit_eligible {
            // The parts of a collapsed table are excluded for the table to paint over: what is.
            let below = if node.collapsed_table {
                self.nodes[at + 1..node.end as usize]
                    .iter()
                    .find_map(|inner| inner.exclusion.filter(|kind| *kind != Exclusion::CollapsedTable))
            } else {
                own.excluded
            };
            return Some(match (node.exclusion, below) {
                (Some(kind), _) => Refusal::Excluded(kind),
                (None, Some(kind)) => Refusal::ExcludedBelow(kind),
                (None, None) => {
                    debug_assert!(escaped, "an element that is not eligible is excluded or escaped");
                    Refusal::Escapes
                }
            });
        }
        if !motion.paints_only {
            return Some(Refusal::LayoutAnimation);
        }
        if !node.forms_context && own.lifts {
            return Some(Refusal::Lifts);
        }
        if own.size > UNIT_MAX_ELEMENTS {
            return Some(Refusal::TooLarge);
        }
        if self.nodes[at..node.end as usize].iter().any(|inner| inner.vol.scroll != 0) {
            return Some(Refusal::Scrolls);
        }
        None
    }

    /// Why the part of a collapsed table at `at` is no unit: the table is the unit of what runs on
    /// its parts, so it is the table's reason.
    fn table_refusal(&self, at: usize) -> Option<Refusal> {
        let mut up = self.nodes[at].parent;
        while up != NO_PARENT {
            let table = &self.nodes[up as usize];
            if table.collapsed_table {
                return self.records.get(&table.id).and_then(|record| record.refusal);
            }
            up = table.parent;
        }
        None
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
            let escaped = node.absolute || (!node.positioned && own.escapes);
            let (escapes, epoch, token, holes) = match (&node.skipped, self.records.get(&node.id)) {
                // Its children were never entered, so `own.lifts` is empty: what they lift is what its
                // last build folded, which is `Record::lifts`.
                (Some(_), Some(record)) => (record.lifts, record.epoch, record.token, record.holes),
                (Some(_), None) => unreachable!("an element is skipped by the record of its last build"),
                (None, before) => {
                    let escapes = node.lift_z.is_some() || (!node.forms_context && own.lifts);
                    let epoch = match (before, self.deferred) {
                        (Some(before), true) => before.epoch,
                        (Some(before), false) => before.epoch + 1,
                        (None, _) => 1,
                    };
                    let mut token = Fingerprint::new();
                    token.word(node.enter.revision);
                    token.word(node.enter.inherited);
                    token.word(node.enter.shape);
                    token.word(epoch);
                    // A hole is not asked for alone: the recording around it is of the build that
                    // made it, so it is not the one for another build of the hole.
                    token.word(own.holes);
                    (escapes, epoch, token.finish(), own.holes)
                }
            };
            self.below[at].epoch = epoch;
            self.below[at].token = token;
            self.below[at].holes = holes;
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
            if node.lift_z.is_some() {
                parent.holes = parent.holes.rotate_left(7) ^ token;
            } else if !node.forms_context {
                parent.holes = parent.holes.rotate_left(7) ^ holes;
            }
            parent.escapes |= escaped;
            parent.size += size;
            if let Some(excluded) = node.exclusion.or(own.excluded) {
                parent.excluded = Some(excluded);
            }
            if !node.absent {
                parent.digest = parent.digest.rotate_left(5).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ digest;
            }
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
        let mut unsteady = false;
        let keep_slots = self.debug_wants_slots();
        let mut slots = Vec::new();
        for at in 0..count {
            let node = &self.nodes[at];
            let own = self.below[at];
            let escaped = node.absolute || (!node.positioned && own.escapes);
            let eligible = node.exclusion.is_none() && own.excluded.is_none() && !escaped;
            let lifts = node.lift_z.is_some() || (!node.forms_context && own.lifts);
            let clean = own.changed.is_none() && !own.changed_below;
            let parent = (node.parent != NO_PARENT).then_some(node.parent as usize);
            self.moved[at] = parent.is_some_and(|parent| self.moved[parent] || self.below[parent].scrolled);
            // A root under a scroller that gpui can record to move is skipped by a frame that finds
            // the scroller moved. One that it cannot is built when it does.
            let holds = eligible
                && !lifts
                && node.skipped.is_none()
                && node.scroller.is_some_and(|scroller| scroller.holds(node.id));
            let relocates = holds && !self.relocation.resting;
            let too_big = eligible
                && node.skipped.is_none()
                && node.scroller.is_some_and(|scroller| scroller.too_big(node.id));
            let oversized = clean && too_big;
            // What a scroller that only appeared moves is not a loss: the frame after it finds the rows still.
            let carried = (node.moved || self.moved[at]) && self.deltas.scrolling && !relocates;
            candidates |= node.skipped.is_some() || (eligible && self.worth(own.size, node.costly) && !carried && !too_big);

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
                    debug_assert!(escaped, "an element that is not retained is changed, excluded or escaped");
                    ESCAPES
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
            unsteady |= node.exclusion.is_some_and(Exclusion::varies_with_time);
            self.unbuilt.remove(&node.id);
            if self.retained[at] && self.verify && !self.planning {
                check(&self.records, &mut self.mismatches, node.id, own.digest, eligible);
            }
            if !self.dormant.is_empty() && self.dormant.contains_key(&node.id) {
                self.woken.insert(node.id);
            }
            if node.vol.dynamic != 0 {
                self.dynamic.insert(node.id, node.vol.dynamic);
                built_dynamic.insert(node.id);
            } else {
                self.dynamic.remove(&node.id);
            }
            let (epoch, token, holes) = (own.epoch, own.token, own.holes);
            let (unit_eligible, motion) = self.unit_facts(at, eligible, escaped);
            let refusal = if node.exclusion == Some(Exclusion::CollapsedTable) {
                self.table_refusal(at)
            } else if motion.dynamic & Dynamic::OWN != 0 {
                self.refusal(at, own, escaped, unit_eligible, motion)
            } else {
                None
            };
            let effects = (node.effects_end > node.effects_start)
                .then(|| Rc::<[BuildEffect]>::from(&all_effects[node.effects_start..node.effects_end]));
            self.records.insert(
                node.id,
                Record {
                    key: node.key,
                    enter: node.enter,
                    vol: node.vol,
                    digest: own.digest,
                    eligible,
                    epoch,
                    token,
                    lift_z: node.lift_z,
                    forms_context: node.forms_context,
                    opacity_run: node.opacity_run,
                    lifts,
                    holes,
                    size: own.size,
                    costly: node.costly,
                    subtree: node.subtree,
                    effects,
                    seasoned: self.deferred,
                    selection: self.selection,
                    unit_eligible,
                    motion,
                    refusal,
                },
            );
        }

        if !self.reference && !self.layout_only {
            self.plan.recorded = true;
            self.plan.unsteady |= unsteady;
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
    /// one below it, unless the dynamic one is dormant. So a dynamic element that this frame did not visit sits under one that has no
    /// box (`display: none`), and its record is of a build that is gone: it would keep every
    /// ancestor on the spine for as long as the window lives. Its record goes too, since a frame
    /// that shows it again must build it, not take what was recorded while its motion ran.
    fn forget_unbuilt_dynamic(&mut self, built: &FxHashSet<u64>) {
        let stale: Vec<u64> = self
            .dynamic
            .keys()
            .copied()
            .filter(|id| !built.contains(id) && !self.dormant.contains_key(id))
            .collect();
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
    report(mismatches, format!("element {id} was judged unchanged and {problem}"));
}

fn report(mismatches: &mut Vec<String>, message: String) {
    crate::perf_counters::retain_mismatch();
    // Loud, for a suite run under `GPUIX_RETAIN_VERIFY`: nothing reads the counter there.
    eprintln!("[retain] {message}");
    if mismatches.len() < 32 {
        mismatches.push(message);
    }
}

/// The spine has to hold the parent of every element on it: a root is skipped when it is off the
/// spine, so one missing above a changed element replays its subtree with that element frozen.
fn check_spine(spine: &FxHashSet<u64>, tree: &crate::retained_tree::RetainedTree, mismatches: &mut Vec<String>) {
    for id in spine {
        let Some(parent) = tree.elements.get(id).and_then(|element| element.parent) else { continue };
        if !spine.contains(&parent) {
            report(mismatches, format!("element {id} is on the spine and its parent {parent} is not"));
        }
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
        states.disabled,
        states.enabled,
        states.has_disabled,
        states.has_enabled,
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
        assert_eq!(std::mem::size_of::<Inherited>(), 440);
    }

    #[test]
    fn every_pointer_state_has_a_bit_of_its_own() {
        use crate::events::state_styles::PointerStates;
        // A state added to `PointerStates` changes this size, and `pointer_bits` has to learn it.
        assert_eq!(std::mem::size_of::<PointerStates>(), 15);
        let each = [
            PointerStates { hovered: true, ..Default::default() },
            PointerStates { active: true, ..Default::default() },
            PointerStates { group_hovered: true, ..Default::default() },
            PointerStates { group_active: true, ..Default::default() },
            PointerStates { group_focused_within: true, ..Default::default() },
            PointerStates { has_hovered: true, ..Default::default() },
            PointerStates { has_active: true, ..Default::default() },
            PointerStates { has_focused: true, ..Default::default() },
            PointerStates { disabled: true, ..Default::default() },
            PointerStates { enabled: true, ..Default::default() },
            PointerStates { has_disabled: true, ..Default::default() },
            PointerStates { has_enabled: true, ..Default::default() },
            PointerStates { focus_within: true, ..Default::default() },
            PointerStates { has_focus_visible: true, ..Default::default() },
            PointerStates { focus_visible: true, ..Default::default() },
        ];
        let mut seen = 0;
        for states in each {
            assert!(states.any());
            let bits = pointer_bits(states);
            assert_eq!(bits.count_ones(), 1, "{states:?}");
            assert_eq!(seen & bits, 0, "{states:?} shares a bit");
            seen |= bits;
        }
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
            commanded: Vec::new(),
            relevance: Vec::new(),
            customs: Vec::new(),
            motion: Vec::new(),
            dormant: &|_| false,
            verdicts: 0,
            selection: 0,
            pass: 1,
            lazy_builds: 0,
            relocations: 0,
            reference: false,
            layout_only: false,
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
            opacity_run: &|| false,
            costly: true,
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

    /// What a frame is told changed, by the sources the tests below vary.
    #[derive(Default)]
    struct Changes {
        hovered: Vec<u64>,
        active: Vec<u64>,
        within: Vec<u64>,
        measured: Vec<u64>,
        links: Vec<u64>,
        commanded: Vec<u64>,
        relevance: Vec<u64>,
        selection: u64,
        /// An animation of the opacity of element 1 is current.
        opacity_run: bool,
    }

    /// A frame of the one element 1, which it builds or leaves unvisited (a layout-only pass with
    /// `display: none` above it). Returns whether the frame skipped it.
    fn frame(retain: &mut Retain, changes: Changes, visit: bool) -> bool {
        let opacity_run = changes.opacity_run;
        let (tree, focus) = (RetainedTree::new(), crate::focus::FocusStyles {
            within: changes.within.into(),
            ..Default::default()
        });
        let (handles, scrolls) = (HashMap::new(), HashMap::new());
        retain.begin_frame(Sources {
            tree: &tree,
            hovered: &changes.hovered,
            active: &changes.active,
            focus: &focus,
            focusable: &handles,
            keyboard: false,
            scrolls: &scrolls,
            measured: changes.measured,
            links: changes.links,
            commanded: changes.commanded,
            relevance: changes.relevance,
            customs: Vec::new(),
            motion: Vec::new(),
            dormant: &|_| false,
            verdicts: 0,
            selection: changes.selection,
            pass: 1,
            lazy_builds: 0,
            relocations: 0,
            reference: false,
            layout_only: false,
        });
        let inherited = root_inherited();
        let mut skipped = false;
        if visit {
            skipped = retain
                .enter(Entry {
                    id: 1,
                    revision: 7,
                    inherited: &inherited,
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
                    opacity_run: &|| opacity_run,
                    costly: true,
                })
                .is_some();
            if !skipped {
                retain.opacity_run(opacity_run);
                retain.stacking(|| (opacity_run.then_some(0), true));
                retain.exit();
            }
        }
        retain.finish_frame();
        skipped
    }

    #[test]
    fn what_changed_in_a_pass_that_did_not_visit_an_element_is_found_by_the_pass_that_does() {
        let changes: [(&str, fn() -> Changes); 7] = [
            ("hovered", || Changes { hovered: vec![1], ..Default::default() }),
            ("active", || Changes { active: vec![1], ..Default::default() }),
            ("focus within", || Changes { within: vec![1], ..Default::default() }),
            ("measured", || Changes { measured: vec![1], ..Default::default() }),
            ("links", || Changes { links: vec![1], ..Default::default() }),
            ("commanded", || Changes { commanded: vec![1], ..Default::default() }),
            ("content-visibility", || Changes { relevance: vec![1], ..Default::default() }),
        ];
        for (name, change) in changes {
            let mut retain = Retain::new();
            retain.set_planning(true);
            for _ in 0..3 {
                frame(&mut retain, Changes::default(), true);
            }
            assert!(frame(&mut retain, Changes::default(), true), "{name}: an element that did not change is skipped");

            // The pass is told, and never gets to the element: the sources do not tell again.
            frame(&mut retain, change(), false);
            frame(&mut retain, Changes::default(), false);
            assert!(!frame(&mut retain, Changes::default(), true), "{name}: the element is built");
            assert!(frame(&mut retain, Changes::default(), true), "{name}: then it is skipped again");
        }
    }

    #[test]
    fn a_record_made_on_the_other_side_of_the_start_or_end_of_an_opacity_run_is_not_skipped() {
        let run = |opacity_run| Changes { opacity_run, ..Default::default() };
        let mut retain = Retain::new();
        retain.set_planning(true);
        for _ in 0..3 {
            frame(&mut retain, run(true), true);
        }
        assert!(frame(&mut retain, run(true), true), "a run that goes on leaves the lift as the record has it");
        // The wrapper of a skipped root carries the lift of the record, which the run made: with the
        // run over, the element is lifted as its static opacity says.
        assert!(!frame(&mut retain, run(false), true), "the run is over");
        assert!(frame(&mut retain, run(false), true), "a record made after it is skipped again");
        assert!(!frame(&mut retain, run(true), true), "a run has started");
        assert!(frame(&mut retain, run(true), true));
    }

    #[test]
    fn an_element_a_new_selection_did_not_reach_is_built_when_it_is_visited() {
        let mut retain = Retain::new();
        retain.set_planning(true);
        for _ in 0..3 {
            frame(&mut retain, Changes::default(), true);
        }
        frame(&mut retain, Changes { selection: 5, ..Default::default() }, false);
        assert!(!frame(&mut retain, Changes { selection: 5, ..Default::default() }, true));
    }

    /// `(id, parent, type, display)` as a tree, the first being the root.
    fn tree_of(nodes: &[(u64, Option<u64>, &str, Option<&str>)]) -> RetainedTree {
        let mut tree = RetainedTree::new();
        for (id, _, kind, display) in nodes {
            tree.create_element(*id, kind.to_string());
            tree.set_style(*id, Arc::new(crate::style::StyleDesc {
                display: display.map(str::to_owned),
                ..Default::default()
            }));
        }
        for (id, parent, ..) in nodes {
            if let Some(parent) = parent {
                tree.append_child(*parent, *id);
            }
        }
        tree.set_root(nodes[0].0);
        tree
    }

    /// A frame over `tree` in which `motion` runs and a build enters `built`, nested in that order.
    fn frame_of(retain: &mut Retain, tree: &RetainedTree, motion: &[u64], built: &[u64]) {
        let focus = crate::focus::FocusStyles::default();
        let (handles, scrolls) = (HashMap::new(), HashMap::new());
        retain.begin_frame(Sources {
            tree,
            hovered: &[],
            active: &[],
            focus: &focus,
            focusable: &handles,
            keyboard: false,
            scrolls: &scrolls,
            measured: Vec::new(),
            links: Vec::new(),
            commanded: Vec::new(),
            relevance: Vec::new(),
            customs: Vec::new(),
            motion: motion.to_vec(),
            dormant: &|_| false,
            verdicts: 0,
            selection: 0,
            pass: 1,
            lazy_builds: 0,
            relocations: 0,
            reference: false,
            layout_only: false,
        });
        let inherited = root_inherited();
        for id in built {
            retain.enter(Entry {
                id: *id,
                revision: 7,
                inherited: &inherited,
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
                opacity_run: &|| false,
                costly: true,
            });
            retain.stacking(|| (None, true));
        }
        for _ in built {
            retain.exit();
        }
        retain.finish_frame();
    }

    fn sorted(ids: &FxHashSet<u64>) -> Vec<u64> {
        let mut ids: Vec<u64> = ids.iter().copied().collect();
        ids.sort_unstable();
        ids
    }

    #[test]
    fn a_changed_element_below_an_unbuilt_wrapper_still_has_its_ancestors_on_the_spine() {
        // 1 > 2 > 3 (display: contents) > 4, the one that moves.
        let tree = tree_of(&[
            (1, None, "div", None),
            (2, Some(1), "div", None),
            (3, Some(2), "div", Some("contents")),
            (4, Some(3), "div", None),
        ]);
        let mut retain = Retain::new();
        retain.set_planning(true);
        frame_of(&mut retain, &tree, &[4], &[1, 2]);
        assert_eq!(sorted(&retain.deltas.spine), vec![1, 2, 3, 4]);
        // 4 was not reached by the build, and 3 is spliced out of 2.
        assert_eq!(sorted(&retain.unbuilt), vec![4]);
        // The walk from 4 used to stop at 3, which the frame before had left in `unbuilt`.
        frame_of(&mut retain, &tree, &[4], &[1, 2]);
        assert_eq!(sorted(&retain.deltas.spine), vec![1, 2, 3, 4]);
    }

    #[test]
    fn the_spine_missing_the_parent_of_an_element_on_it_is_a_mismatch() {
        // 1 > 2 > 3 (display: contents) > 4.
        let tree = tree_of(&[
            (1, None, "div", None),
            (2, Some(1), "div", None),
            (3, Some(2), "div", Some("contents")),
            (4, Some(3), "div", None),
        ]);
        let spine = |ids: &[u64]| ids.iter().copied().collect::<FxHashSet<u64>>();
        let mut mismatches = Vec::new();
        check_spine(&spine(&[1, 2, 3, 4]), &tree, &mut mismatches);
        assert!(mismatches.is_empty(), "{mismatches:?}");
        // What the walk that stopped at the first id it had left behind: 3 is not on it.
        check_spine(&spine(&[1, 2, 4]), &tree, &mut mismatches);
        assert_eq!(mismatches, vec!["element 4 is on the spine and its parent 3 is not".to_owned()]);
    }

    #[test]
    fn a_verified_frame_finds_the_spine_closed_under_parent() {
        let tree = tree_of(&[
            (1, None, "div", None),
            (2, Some(1), "div", None),
            (3, Some(2), "div", Some("contents")),
            (4, Some(3), "div", None),
        ]);
        let mut retain = Retain::new();
        retain.set_planning(true);
        retain.set_verify(true);
        frame_of(&mut retain, &tree, &[4], &[1, 2]);
        frame_of(&mut retain, &tree, &[4], &[1, 2]);
        assert!(retain.mismatches().is_empty(), "{:?}", retain.mismatches());
    }

    /// A frame of 1 > 2 > 3 in which 3 is lifted out of 2 and 1, and `revision` is the revision 1
    /// is entered with. Returns whether the frame skipped 2.
    fn lifted_frame(retain: &mut Retain, revision: u64) -> bool {
        let tree = RetainedTree::new();
        let (focus, handles, scrolls) = (crate::focus::FocusStyles::default(), HashMap::new(), HashMap::new());
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
            commanded: Vec::new(),
            relevance: Vec::new(),
            customs: Vec::new(),
            motion: Vec::new(),
            dormant: &|_| false,
            verdicts: 0,
            selection: 0,
            pass: 1,
            lazy_builds: 0,
            relocations: 0,
            reference: false,
            layout_only: false,
        });
        let inherited = root_inherited();
        let enter = |retain: &mut Retain, id: u64, revision: u64| {
            retain.enter(Entry {
                id,
                revision,
                inherited: &inherited,
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
                opacity_run: &|| false,
                costly: true,
            })
        };
        enter(retain, 1, revision);
        retain.stacking(|| (None, false));
        let skipped = enter(retain, 2, 7).is_some();
        if !skipped {
            retain.stacking(|| (None, false));
            enter(retain, 3, 7);
            retain.stacking(|| (Some(1), false));
            retain.exit();
            retain.exit();
        }
        retain.exit();
        retain.finish_frame();
        skipped
    }

    #[test]
    fn a_skipped_element_lifts_what_its_last_build_lifted() {
        let mut retain = Retain::new();
        retain.set_planning(true);
        assert!(!lifted_frame(&mut retain, 1));
        assert!(retain.records[&1].lifts && retain.records[&2].lifts);
        // 1 is edited and built; 2 is not, and its children are not entered.
        for revision in 2..6 {
            assert!(lifted_frame(&mut retain, revision), "2 is skipped");
            assert!(retain.records[&2].lifts, "revision {revision}: 2 is still recorded to lift");
            assert!(retain.records[&1].lifts, "revision {revision}: 1 lifts what 2 lifted, though it did not enter it");
        }
    }

    #[test]
    fn what_no_build_enters_is_not_carried_but_what_it_may_enter_is() {
        let carried = |tree: &RetainedTree, id: u64, built: &[u64]| {
            let mut retain = Retain::new();
            retain.set_planning(true);
            frame_of(&mut retain, tree, &[id], built);
            sorted(&retain.unbuilt)
        };
        // Below an `<svg>`, which is entered and paints them as one image.
        let svg = tree_of(&[
            (1, None, "div", None),
            (2, Some(1), "svg", None),
            (3, Some(2), "g", None),
            (4, Some(3), "path", None),
        ]);
        assert_eq!(carried(&svg, 4, &[1]), vec![2]);
        // An inline `<span>` and the one in it are parts of the paragraph of the div around them.
        let inline = tree_of(&[(1, None, "div", None), (2, Some(1), "span", None), (3, Some(2), "span", None)]);
        assert_eq!(carried(&inline, 3, &[1]), Vec::<u64>::new());
        // The root of a tree is built as a host, not spliced out of a parent.
        let contents_root = tree_of(&[(1, None, "div", Some("contents")), (2, Some(1), "div", None)]);
        assert_eq!(carried(&contents_root, 2, &[]), vec![1, 2]);
        // A `display: none` element is not entered by a pass that skips it, and its children can be.
        let hidden = tree_of(&[(1, None, "div", None), (2, Some(1), "div", Some("none")), (3, Some(2), "div", None)]);
        assert_eq!(carried(&hidden, 3, &[1]), vec![2, 3]);
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
            // An unset one of these in a transition or a motion below starts from it.
            ("a hidden visibility", r#""visibility": "hidden""#),
            ("a unitless line height", r#""lineHeight": 1.5"#),
            ("a pointer-events of none", r#""pointerEvents": "none""#),
            ("a border spacing", r#""borderSpacing": "3px 5px""#),
            // The start of a logical inline pair below it is the other side.
            ("a right to left direction", r#""direction": "rtl""#),
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
    fn an_absolute_box_is_retained_with_the_positioned_box_that_places_it(cx: &mut gpui::TestAppContext) {
        let mut elements = page([CARD; 3]);
        elements.push(div(40, Some(1), r#"{"position": "relative", "width": 100, "height": 30}"#));
        elements.push(div(41, Some(40), r#"{"height": 20}"#));
        elements.push(div(42, Some(41), r#"{"position": "absolute", "top": 0, "left": 0, "width": 10, "height": 10}"#));
        let scene = Scene::new(cx, elements);
        scene.settle(cx);
        // The box is in the relative one, which is in the page: nothing is in the way.
        assert_eq!(scene.roots(cx), vec![1]);
    }

    #[gpui::test]
    fn an_absolute_box_with_no_positioned_ancestor_below_a_root_ends_the_roots_above_it(cx: &mut gpui::TestAppContext) {
        let mut elements = page([CARD; 3]);
        elements.push(div(40, Some(1), r#"{"width": 100, "height": 30}"#));
        elements.push(div(41, Some(40), r#"{"height": 20}"#));
        elements.push(div(42, Some(41), r#"{"position": "absolute", "top": 0, "left": 0, "width": 10, "height": 10}"#));
        let scene = Scene::new(cx, elements);
        scene.settle(cx);
        // Its containing block is above the page's children, so only the three cards are roots.
        assert_eq!(scene.roots(cx), vec![10, 20, 30]);
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
                "transition": { "duration": 0.6, "ease": "linear" }
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
    fn an_svg_whose_shape_runs_a_css_animation_is_changed_every_frame_of_it(cx: &mut gpui::TestAppContext) {
        let mut elements = icon(SMALL_ICON, None);
        elements.iter_mut().find(|element| element.id == 41).unwrap().style =
            r#"{"animation": "grow 600ms linear"}"#;
        let scene = Scene::new(cx, elements);
        scene
            .window
            .update(cx, |view, _, _| {
                view.keyframes.define("grow", r#"{"from": {"r": 2}, "to": {"r": 10}}"#).unwrap();
            })
            .unwrap();
        scene.draw(cx);
        crate::perf_counters::reset();
        for _ in 0..4 {
            scene.window.update(cx, |view, _, _| view.clock.fast_forward_ms(16.0)).unwrap();
            scene.draw(cx);
            // The circle is never built, so its animation is advanced by the svg writing it, and
            // the svg is the one that changes.
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

    /// The sources of a frame over `tree` in which `motion` runs and `hovered` is under the pointer.
    fn sources_of<'a>(
        tree: &'a RetainedTree,
        focus: &'a crate::focus::FocusStyles,
        handles: &'a HashMap<u64, gpui::FocusHandle>,
        scrolls: &'a HashMap<u64, gpui::ScrollHandle>,
        hovered: &'a [u64],
        layout_only: bool,
    ) -> Sources<'a> {
        Sources {
            tree,
            hovered,
            active: &[],
            focus,
            focusable: handles,
            keyboard: false,
            scrolls,
            measured: Vec::new(),
            links: Vec::new(),
            commanded: Vec::new(),
            relevance: Vec::new(),
            customs: Vec::new(),
            motion: Vec::new(),
            dormant: &|_| false,
            verdicts: 0,
            selection: 0,
            pass: 1,
            lazy_builds: 0,
            relocations: 0,
            reference: false,
            layout_only,
        }
    }

    /// A page of `1 > 2 > (3, 4)` and `1 > 5`, with whatever `build` enters in a frame of it.
    struct Units {
        tree: RetainedTree,
        retain: Retain,
        inherited: Inherited,
        focus: crate::focus::FocusStyles,
        handles: HashMap<u64, gpui::FocusHandle>,
        scrolls: HashMap<u64, gpui::ScrollHandle>,
        /// The revision each element is entered with, 7 for those not edited.
        revisions: HashMap<u64, u64>,
    }

    impl Units {
        fn new() -> Self {
            let tree = tree_of(&[
                (1, None, "div", None),
                (2, Some(1), "div", None),
                (3, Some(2), "div", None),
                (4, Some(2), "div", None),
                (5, Some(1), "div", None),
            ]);
            let mut retain = Retain::new();
            retain.set_planning(true);
            Self {
                tree,
                retain,
                inherited: root_inherited(),
                focus: Default::default(),
                handles: HashMap::new(),
                scrolls: HashMap::new(),
                revisions: HashMap::new(),
            }
        }

        /// The next frame finds `id` edited, as one that starts to run something is.
        fn edit(&mut self, id: u64) {
            *self.revisions.entry(id).or_insert(7) += 1;
        }

        /// A frame in which `hovered` is under the pointer, that `build` builds.
        fn frame(&mut self, hovered: &[u64], layout_only: bool, build: impl FnOnce(&mut Self)) {
            self.retain
                .begin_frame(sources_of(&self.tree, &self.focus, &self.handles, &self.scrolls, hovered, layout_only));
            build(self);
            self.retain.finish_frame();
        }

        fn enter(&mut self, id: u64) -> bool {
            let skipped = self
                .retain
                .enter(Entry {
                    id,
                    revision: self.revisions.get(&id).copied().unwrap_or(7),
                    inherited: &self.inherited,
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
                    opacity_run: &|| false,
                    costly: true,
                })
                .is_some();
            if !skipped {
                self.retain.stacking(|| (None, true));
            }
            skipped
        }

        /// Builds the element `id` the way `build_element_in` does, with `inside` building what is
        /// in it. Says whether the element was begun as a unit, and if so whether what was built
        /// is one. Nothing if the frame skipped it.
        fn element(&mut self, id: u64, motion: Option<bool>, inside: impl FnOnce(&mut Self)) -> Option<bool> {
            if self.enter(id) {
                return None;
            }
            let enter = self.retain.begin_unit(true);
            if let Some(paints_only) = motion {
                self.retain.dynamic(Dynamic::Animation);
                self.retain.paints_only(paints_only);
            }
            inside(self);
            let unit = enter.map(|enter| (enter, self.retain.end_unit().is_some()));
            if let Some((enter, true)) = unit {
                let env = crate::renderer::FrameEnv::for_test(self.inherited.clone(), enter, web_time::Instant::now());
                self.retain.add_unit(Unit { id, env });
            }
            self.retain.exit();
            unit.map(|(_, built)| built)
        }

        fn leaf(&mut self, id: u64) {
            self.element(id, None, |_| {});
        }

        fn ids(&self) -> Vec<u64> {
            self.retain.units().iter().map(|unit| unit.id).collect()
        }

        fn preview(&self, hovered: &[u64], scroll_animating: bool) -> Result<Vec<u64>, Unpatchable> {
            let sources = sources_of(&self.tree, &self.focus, &self.handles, &self.scrolls, hovered, false);
            self.retain.preview(&sources, scroll_animating)
        }

        /// The page, with 2 and 4 running something (whether it only paints, if so).
        fn page(&mut self, hovered: &[u64], two: Option<bool>, four: Option<bool>) {
            self.pass(hovered, false, two, four);
        }

        fn pass(&mut self, hovered: &[u64], layout_only: bool, two: Option<bool>, four: Option<bool>) {
            self.frame(hovered, layout_only, |page| {
                page.element(1, None, |page| {
                    page.element(2, two, |page| {
                        page.leaf(3);
                        page.element(4, four, |_| {});
                    });
                    page.leaf(5);
                });
            });
        }
    }

    #[test]
    fn a_tick_that_only_runs_the_animation_of_a_unit_previews_that_unit() {
        let mut page = Units::new();
        page.page(&[], Some(true), None);
        assert_eq!(page.ids(), Vec::<u64>::new(), "an element is a unit once a build found it running");
        page.page(&[], Some(true), None);
        assert_eq!(page.ids(), vec![2]);
        assert_eq!(page.preview(&[], false), Ok(vec![2]));
        let (unbuilt, seen) = (sorted(&page.retain.unbuilt), page.retain.seen.hovered.clone());
        assert_eq!(page.preview(&[], false), Ok(vec![2]), "a preview takes nothing from the next one");
        assert_eq!((sorted(&page.retain.unbuilt), page.retain.seen.hovered.clone()), (unbuilt, seen));
        // The frame that follows finds what the preview found.
        page.page(&[], Some(true), None);
        assert_eq!(page.ids(), vec![2]);
    }

    #[test]
    fn a_unit_whose_animation_ended_in_the_last_frame_is_not_patched() {
        let mut page = Units::new();
        page.page(&[], Some(true), None);
        page.page(&[], Some(true), None);
        // The frame that finds the animation over is still built as the unit the frame before
        // found, and a source goes on saying that the element runs something (an SVG shape with
        // an animation that holds its end value does): the next frame builds it as no unit.
        page.page(&[], None, None);
        assert_eq!(page.ids(), vec![2]);
        let mut sources = sources_of(&page.tree, &page.focus, &page.handles, &page.scrolls, &[], false);
        sources.motion = vec![2];
        assert_eq!(page.retain.preview(&sources, false), Err(Unpatchable::Ended(2)));
        page.page(&[], None, None);
        assert_eq!(page.ids(), Vec::<u64>::new());
    }

    #[test]
    fn a_unit_is_the_outermost_element_that_runs_something_that_only_paints() {
        let mut page = Units::new();
        page.page(&[], Some(true), Some(true));
        page.page(&[], Some(true), Some(true));
        assert_eq!(page.ids(), vec![2], "the element inside it is part of it");
        assert_eq!(page.preview(&[], false), Ok(vec![2]));
    }

    #[test]
    fn an_element_whose_animation_moves_what_it_lays_out_is_not_a_unit() {
        let mut page = Units::new();
        page.page(&[], Some(false), None);
        page.page(&[], Some(false), None);
        assert_eq!(page.ids(), Vec::<u64>::new());
        assert_eq!(page.preview(&[], false), Err(Unpatchable::Refused(Refusal::LayoutAnimation)));
        // One inside the one that lays out is the unit.
        page.edit(4);
        page.page(&[], Some(false), Some(true));
        page.page(&[], Some(false), Some(true));
        assert_eq!(page.ids(), vec![4]);
        assert_eq!(page.preview(&[], false), Err(Unpatchable::Refused(Refusal::LayoutAnimation)));
    }

    #[test]
    fn an_element_with_more_elements_than_a_unit_may_hold_is_not_one() {
        let mut page = Units::new();
        let big = |page: &mut Units| {
            page.frame(&[], false, |page| {
                page.element(1, None, |page| {
                    page.element(2, Some(true), |page| {
                        for id in 100..100 + UNIT_MAX_ELEMENTS as u64 {
                            page.leaf(id);
                        }
                    });
                });
            });
        };
        crate::perf_counters::reset();
        big(&mut page);
        big(&mut page);
        assert_eq!(page.ids(), Vec::<u64>::new());
        assert_eq!(crate::perf_counters::snapshot().units_oversized, 1);
        assert_eq!(page.preview(&[], false), Err(Unpatchable::Refused(Refusal::TooLarge)));
    }

    #[test]
    fn what_a_patch_could_not_repeat_makes_what_holds_it_no_unit() {
        let holds: [(&str, fn(&mut Retain), Refusal); 4] = [
            ("an exclusion", |retain| retain.exclude(Exclusion::Scroller), Refusal::ExcludedBelow(Exclusion::Scroller)),
            ("a scroll", |retain| retain.scroll(gpui::point(gpui::px(0.0), gpui::px(-4.0))), Refusal::Scrolls),
            ("a lifted element", |retain| retain.stacking(|| (Some(1), false)), Refusal::Lifts),
            ("an absolute box against a parent outside", |retain| retain.containing_block(false, true), Refusal::Escapes),
        ];
        for (what, note, refusal) in holds {
            let mut page = Units::new();
            page.page(&[], Some(true), None);
            let mut unit = None;
            page.frame(&[], false, |page| {
                page.element(1, None, |page| {
                    unit = page.element(2, Some(true), |page| {
                        // Nothing it holds is handed to a stacking context of its own.
                        page.retain.stacking(|| (None, false));
                        page.leaf(3);
                        page.element(4, None, |page| note(&mut page.retain));
                    });
                    page.leaf(5);
                });
            });
            assert_eq!(unit, Some(false), "{what}: begun as a unit, and not one when it was built");
            assert_eq!(page.ids(), Vec::<u64>::new(), "{what}");
            assert_eq!(page.preview(&[], false), Err(Unpatchable::Refused(refusal)), "{what}: the tick says why");
        }
        // The same page without any of them has its unit.
        let mut page = Units::new();
        page.page(&[], Some(true), None);
        page.page(&[], Some(true), None);
        assert_eq!(page.ids(), vec![2]);
    }

    #[test]
    fn an_element_that_is_excluded_or_absolute_itself_is_no_unit_and_a_tick_says_why() {
        let own: [(&str, fn(&mut Retain), Refusal); 2] = [
            ("an excluded kind", |retain| retain.exclude(Exclusion::AtomicInline), Refusal::Excluded(Exclusion::AtomicInline)),
            ("absolute against a containing block outside", |retain| retain.containing_block(false, true), Refusal::Escapes),
        ];
        for (what, note, refusal) in own {
            let mut page = Units::new();
            for _ in 0..2 {
                page.frame(&[], false, |page| {
                    page.element(1, None, |page| {
                        page.element(2, Some(true), |page| note(&mut page.retain));
                    });
                });
            }
            assert_eq!(page.ids(), Vec::<u64>::new(), "{what}");
            assert_eq!(page.preview(&[], false), Err(Unpatchable::Refused(refusal)), "{what}");
        }
    }

    #[test]
    fn a_refusal_is_counted_under_a_key_that_names_it() {
        let keys: Vec<String> = [
            Refusal::Excluded(Exclusion::AtomicInline),
            Refusal::ExcludedBelow(Exclusion::Svg),
            Refusal::Escapes,
            Refusal::Lifts,
            Refusal::Scrolls,
            Refusal::LayoutAnimation,
            Refusal::TooLarge,
        ]
        .map(Refusal::key)
        .to_vec();
        assert_eq!(
            keys,
            ["Excluded:atomic-inline", "ExcludedBelow:svg", "Escapes", "Lifts", "Scrolls", "LayoutAnimation", "TooLarge"]
        );
    }

    impl Units {
        /// The page with 2 a table with collapsed borders, which 3 (a row) and 4 are the parts of; `row` is
        /// what runs on the row, `cell` what 4 notes (an exclusion, for instance).
        fn table(&mut self, row: Option<bool>, cell: impl Fn(&mut Retain)) {
            self.frame(&[], false, |page| {
                page.element(1, None, |page| {
                    page.element(2, None, |page| {
                        page.retain.collapsed_table();
                        page.inherited.table.collapse = true;
                        page.element(3, row, |_| {});
                        page.element(4, None, |page| cell(&mut page.retain));
                        page.inherited.table.collapse = false;
                    });
                    page.leaf(5);
                });
            });
        }
    }

    #[test]
    fn a_table_with_collapsed_borders_is_the_unit_of_the_paint_only_animation_of_its_parts() {
        let mut page = Units::new();
        page.table(Some(true), |_| {});
        assert_eq!(page.ids(), Vec::<u64>::new(), "a unit is made once a build found something running");
        page.table(Some(true), |_| {});
        assert_eq!(page.ids(), vec![2], "the table, not the row that runs it");
        assert_eq!(page.preview(&[], false), Ok(vec![2]));
        page.table(Some(true), |_| {});
        assert_eq!(page.ids(), vec![2]);

        // The frame that finds it over is still built as the unit, and the one after as none.
        page.table(None, |_| {});
        assert_eq!(page.ids(), vec![2]);
        page.table(None, |_| {});
        assert_eq!(page.ids(), Vec::<u64>::new(), "nothing runs in the table any more");
    }

    #[test]
    fn a_table_with_collapsed_borders_is_no_unit_when_a_part_moves_what_it_lays_out() {
        let mut page = Units::new();
        page.table(Some(false), |_| {});
        page.table(Some(false), |_| {});
        assert_eq!(page.ids(), Vec::<u64>::new());
        assert_eq!(page.preview(&[], false), Err(Unpatchable::Refused(Refusal::LayoutAnimation)));
    }

    #[test]
    fn a_table_with_collapsed_borders_is_no_unit_when_a_part_is_excluded_for_another_reason() {
        for (what, exclude) in [("a scroller", Exclusion::Scroller), ("an input", Exclusion::CustomElement)] {
            let mut page = Units::new();
            page.table(Some(true), |retain| retain.exclude(exclude));
            page.table(Some(true), |retain| retain.exclude(exclude));
            assert_eq!(page.ids(), Vec::<u64>::new(), "{what} in a part");
        }
        // The parts of the table are no reason for the elements around it to be retained.
        let mut page = Units::new();
        page.table(None, |_| {});
        assert_eq!(page.retain.records.get(&2).map(|record| record.eligible), Some(false));
        assert_eq!(page.retain.records.get(&1).map(|record| record.eligible), Some(false));
    }

    #[test]
    fn a_lifted_element_is_a_unit_unless_it_lifts_what_is_in_it_out_with_it() {
        // (the element forms a stacking context, something in it is lifted, whether it is a unit)
        let cases = [
            (true, false, true),
            (false, false, true),
            (true, true, true),
            (false, true, false),
        ];
        for (forms_context, child_lifted, is_unit) in cases {
            let mut page = Units::new();
            let mut unit = None;
            for _ in 0..3 {
                page.frame(&[], false, |page| {
                    page.element(1, None, |page| {
                        unit = page.element(2, Some(true), |page| {
                            page.retain.stacking(|| (Some(0), forms_context));
                            page.element(4, None, |page| {
                                if child_lifted {
                                    page.retain.stacking(|| (Some(1), false));
                                }
                            });
                        });
                    });
                });
            }
            let expected = is_unit.then_some(2);
            assert_eq!(page.ids().first().copied(), expected, "context {forms_context}, lifted child {child_lifted}");
            assert_eq!(unit, Some(is_unit), "context {forms_context}, lifted child {child_lifted}");
        }
    }

    #[test]
    fn nothing_inside_a_unit_is_skipped() {
        let mut page = Units::new();
        // Frame by frame: the first records, the second sees 3 and 4 unchanged and 2 running.
        page.page(&[], Some(true), None);
        page.page(&[], Some(true), None);
        page.frame(&[], false, |page| {
            page.element(1, None, |page| {
                assert!(!page.enter(2));
                assert!(page.retain.begin_unit(true).is_some());
                page.retain.dynamic(Dynamic::Animation);
                page.retain.paints_only(true);
                assert!(!page.enter(3), "3 is unchanged and would be skipped outside a unit");
                page.retain.exit();
                assert!(page.retain.end_unit().is_some());
                page.retain.exit();
            });
        });
        page.frame(&[], false, |page| {
            page.element(1, None, |page| {
                assert!(!page.enter(2));
                page.retain.dynamic(Dynamic::Animation);
                page.retain.paints_only(true);
                assert!(page.enter(3), "the same 3 is skipped when 2 is not a unit");
                page.retain.exit();
            });
        });
    }

    #[test]
    fn a_tick_that_changes_more_than_time_is_not_previewed() {
        let mut page = Units::new();
        page.page(&[], Some(true), None);
        page.page(&[], Some(true), None);
        assert_eq!(page.preview(&[3], false), Err(Unpatchable::Sourced), "the pointer moved");
        assert_eq!(page.preview(&[], true), Err(Unpatchable::ScrollAnimation));
        page.tree.set_text(3, "words".to_owned());
        assert_eq!(page.preview(&[], false), Err(Unpatchable::Edited));
    }

    #[test]
    fn a_tick_after_a_pass_that_only_lays_out_is_not_previewed() {
        let mut page = Units::new();
        page.page(&[], Some(true), None);
        page.page(&[], Some(true), None);
        page.pass(&[], true, Some(true), None);
        assert_eq!(page.preview(&[], false), Err(Unpatchable::LayoutPass));
        page.page(&[], Some(true), None);
        assert_eq!(page.preview(&[], false), Ok(vec![2]), "a frame built whole gives a new start");
    }

    #[test]
    fn a_frame_that_built_what_follows_the_clock_outside_the_units_is_not_previewed() {
        let mut page = Units::new();
        let build = |page: &mut Units, varies: Exclusion| {
            page.frame(&[], false, |page| {
                page.element(1, None, |page| {
                    page.element(2, Some(true), |_| {});
                    page.element(5, None, |page| page.retain.exclude(varies));
                });
            });
        };
        build(&mut page, Exclusion::Sticky);
        build(&mut page, Exclusion::Sticky);
        assert_eq!(page.ids(), vec![2]);
        assert_eq!(page.preview(&[], false), Ok(vec![2]), "a sticky box follows the scroll, not the clock");
        for varies in [
            Exclusion::CustomElement,
            Exclusion::Highlight,
            Exclusion::Svg,
            Exclusion::VirtualList,
            Exclusion::LayoutAnimation,
        ] {
            build(&mut page, varies);
            assert_eq!(page.preview(&[], false), Err(Unpatchable::Unsteady), "{varies:?}");
        }
    }

    #[test]
    fn an_element_that_runs_outside_the_units_blocks_the_tick_unless_nobody_sees_it() {
        let mut page = Units::new();
        page.page(&[], Some(true), None);
        page.page(&[], Some(true), None);
        // 5 runs something that lays out.
        let both = |page: &mut Units| {
            page.frame(&[], false, |page| {
                page.element(1, None, |page| {
                    page.element(2, Some(true), |_| {});
                    page.element(5, Some(false), |_| {});
                });
            });
        };
        page.edit(5);
        both(&mut page);
        both(&mut page);
        assert_eq!(page.preview(&[], false), Err(Unpatchable::Refused(Refusal::LayoutAnimation)));
        let sources = Sources { dormant: &|id| id == 5, ..sources_of(&page.tree, &page.focus, &page.handles, &page.scrolls, &[], false) };
        assert_eq!(page.retain.preview(&sources, false), Ok(vec![2]), "dormant: the tick carries its clock without building it");
    }

    /// A frame of the page in which 3 and 4 run something that paints, and `culled` says which
    /// of them the frame finds dormant. Gives the ids it was asked about and what moved.
    fn dormancy_frame(page: &mut Units, culled: &dyn Fn(u64) -> bool, verdicts: u64) -> (Vec<u64>, DormantMoves) {
        let asked = std::cell::RefCell::new(Vec::new());
        let ask = |id: u64| {
            asked.borrow_mut().push(id);
            culled(id)
        };
        let mut sources = sources_of(&page.tree, &page.focus, &page.handles, &page.scrolls, &[], false);
        sources.dormant = &ask;
        sources.verdicts = verdicts;
        page.retain.begin_frame(sources);
        let moves = page.retain.take_dormant_moves();
        page.element(1, None, |page| {
            page.element(2, None, |page| {
                page.element(3, Some(true), |_| {});
                page.element(4, Some(true), |_| {});
            });
            page.leaf(5);
        });
        page.retain.finish_frame();
        let mut asked = asked.into_inner();
        asked.sort_unstable();
        (asked, moves)
    }

    #[test]
    fn a_dormant_element_is_asked_about_once_and_again_only_when_something_can_have_changed_the_answer() {
        let mut page = Units::new();
        let culled = |_: u64| true;
        // The first frame builds everything, and the elements that run something are asked about.
        dormancy_frame(&mut page, &culled, 0);
        let (asked, moves) = dormancy_frame(&mut page, &culled, 0);
        assert_eq!(asked, vec![3, 4]);
        assert_eq!((in_order(moves.entered), moves.left), (vec![3, 4], vec![]));
        assert!(page.retain.is_dormant(3) && page.retain.is_dormant(4));
        for _ in 0..3 {
            let (asked, moves) = dormancy_frame(&mut page, &culled, 0);
            assert_eq!(asked, Vec::<u64>::new(), "a frame in which nothing changed asks about no one");
            assert!(moves.entered.is_empty() && moves.left.is_empty());
        }

        // A build that reaches 4 (with the elements above it, as an edit does) wakes it for the frame after.
        for id in [1, 2, 4] {
            page.edit(id);
        }
        let (asked, _) = dormancy_frame(&mut page, &culled, 0);
        assert_eq!(asked, Vec::<u64>::new(), "the frame that builds it still sees what the frame before found");
        assert!(page.retain.is_dormant(4));
        let shown = |id: u64| id != 4;
        let (asked, moves) = dormancy_frame(&mut page, &shown, 0);
        assert_eq!(asked, vec![4], "only the element that was built is asked about");
        assert_eq!((moves.entered, moves.left), (vec![], vec![4]));
        assert!(page.retain.is_dormant(3) && !page.retain.is_dormant(4));

        // What the answers are read from moves with no build of an element: all that sleep are asked.
        let (asked, moves) = dormancy_frame(&mut page, &shown, 1);
        assert_eq!(asked, vec![3, 4], "4 runs something and is asked about in every frame, 3 is asked again");
        assert!(moves.entered.is_empty() && moves.left.is_empty());
        let none = |_: u64| false;
        let (asked, moves) = dormancy_frame(&mut page, &none, 2);
        assert_eq!(asked, vec![3, 4]);
        assert_eq!(moves.left, vec![3]);
        assert!(!page.retain.is_dormant(3));
        let (asked, _) = dormancy_frame(&mut page, &none, 2);
        assert_eq!(asked, vec![3, 4], "what runs on an element that is not dormant is asked about in every frame");
    }

    fn in_order(mut ids: Vec<u64>) -> Vec<u64> {
        ids.sort_unstable();
        ids
    }

    #[test]
    fn dormant_elements_wake_when_retention_stops_and_when_they_are_destroyed() {
        let mut page = Units::new();
        let culled = |_: u64| true;
        dormancy_frame(&mut page, &culled, 0);
        dormancy_frame(&mut page, &culled, 0);
        assert!(page.retain.is_dormant(3) && page.retain.is_dormant(4));
        page.tree.destroy_element(4);
        page.retain.begin_frame(sources_of(&page.tree, &page.focus, &page.handles, &page.scrolls, &[], false));
        let moves = page.retain.take_dormant_moves();
        assert_eq!(moves.left, vec![4], "the clock of a destroyed element is let go");
        assert!(page.retain.is_dormant(3) && !page.retain.is_dormant(4));
        page.retain.finish_frame();
        page.retain.set_planning(false);
        assert!(!page.retain.is_dormant(3));
        assert_eq!(page.retain.take_dormant_moves().left, vec![3]);
    }

    const TINTED: &str = r#"{"height": 60, "animation": "tint 600ms linear infinite"}"#;

    /// A page whose second card runs a colour animation, drawn until its card is a unit.
    fn tinted(cx: &mut gpui::TestAppContext, verify: bool) -> Scene {
        let scene = Scene::new(cx, page([CARD, TINTED, CARD]));
        scene
            .window
            .update(cx, |view, _, _| {
                view.retain.set_planning(true);
                view.retain.set_verify(verify);
                view.keyframes
                    .define("tint", r##"{"from": {"color": "#ff0000"}, "to": {"color": "#0000ff"}}"##)
                    .unwrap();
            })
            .unwrap();
        for _ in 0..6 {
            scene.window.update(cx, |view, _, _| view.clock.fast_forward_ms(16.0)).unwrap();
            scene.notified_draw(cx);
        }
        scene
    }

    fn units_of(scene: &Scene, cx: &mut gpui::TestAppContext) -> Vec<u64> {
        scene
            .window
            .update(cx, |view, _, _| view.retain.units().iter().map(|unit| unit.id).collect())
            .unwrap()
    }

    #[gpui::test]
    fn a_card_that_runs_a_colour_is_a_unit_and_the_other_cards_are_not(cx: &mut gpui::TestAppContext) {
        crate::perf_counters::reset();
        let scene = tinted(cx, false);
        assert_eq!(units_of(&scene, cx), vec![20]);
        assert!(crate::perf_counters::snapshot().units_built >= 1);
        // What a frame could reuse is judged as it was without the unit: the lines of the card
        // are roots beside the cards that are still, though the unit builds them.
        assert_eq!(scene.roots(cx), vec![10, 21, 22, 23, 30]);
    }

    #[gpui::test]
    fn a_frame_built_whole_around_units_is_the_frame_it_was_without_them(cx: &mut gpui::TestAppContext) {
        // Every frame that replayed something is built again whole and compared with it.
        let scene = tinted(cx, true);
        assert_eq!(units_of(&scene, cx), vec![20]);
        scene.window.update(cx, |view, _, _| view.clock.fast_forward_ms(16.0)).unwrap();
        scene.notified_draw(cx);
        scene.assert_sound(cx);
    }

    #[gpui::test]
    fn a_tick_of_a_colour_is_previewed_and_its_unit_built_alone(cx: &mut gpui::TestAppContext) {
        let scene = tinted(cx, false);
        let tapes = scene.window.update(cx, |view, _, _| view.tapes.clone()).unwrap();
        scene.window.update(cx, |view, _, _| view.clock.fast_forward_ms(16.0)).unwrap();
        let (preview, built) = scene
            .window
            .update(cx, |view, window, cx| {
                let now = view.clock.now();
                let records = view.retain.record_counts();
                let preview = view.preview_tick(window, now);
                let built = view.build_unit(20, now, window, cx);
                assert!(view.build_unit(10, now, window, cx).is_none(), "10 is not a unit");
                assert_eq!(view.retain.record_counts(), records, "nothing of the frames is touched");
                assert_eq!(view.frame_instant, Some(now));
                (preview, built)
            })
            .unwrap();
        assert_eq!(preview, Ok(vec![20]));
        let built = built.expect("20 is a unit");
        assert_eq!(built.id, crate::renderer::retained_id(20));
        // It was not painted: it wrote nothing, while the frame wrote the boxes of the card.
        assert!(!tapes.same_as_recorded(20, &built.tape));
        // And the tick is still the same tick: nothing was taken by the preview or the build.
        assert_eq!(
            scene.window.update(cx, |view, window, _| view.preview_tick(window, view.clock.now())).unwrap(),
            Ok(vec![20])
        );
    }

    impl Scene {
        /// A frame that is a tick: the window is not told, the clock moved.
        fn tick(&self, cx: &mut gpui::TestAppContext) {
            cx.update_window(self.window.into(), |_, window, app| {
                window.request_tick();
                window.draw(app).clear(app);
            })
            .unwrap();
            self.assert_sound(cx);
        }

        fn patches(&self, cx: &mut gpui::TestAppContext) -> gpui::PatchCounters {
            cx.update_window(self.window.into(), |_, window, _| window.patch_counters()).unwrap()
        }
    }

    #[gpui::test]
    fn a_tick_of_a_colour_is_drawn_by_patching_the_frame_before(cx: &mut gpui::TestAppContext) {
        let scene = tinted(cx, true);
        let before = scene.patches(cx);
        for _ in 0..4 {
            scene.window.update(cx, |view, _, _| view.clock.fast_forward_ms(16.0)).unwrap();
            scene.tick(cx);
        }
        let after = scene.patches(cx);
        assert_eq!(after.patched_frames - before.patched_frames, 4);
        assert_eq!(after.patched_units - before.patched_units, 4);
        assert_eq!(after.total_declines(), before.total_declines());
        // The frame a pointer or a commit asks for (the test app draws it as the notification flushes)
        // is built whole, and the ticks go on after it.
        scene.window.update(cx, |view, _, cx| {
            view.clock.fast_forward_ms(16.0);
            cx.notify();
        })
        .unwrap();
        assert_eq!(scene.patches(cx).patched_frames, after.patched_frames);
        for _ in 0..2 {
            scene.window.update(cx, |view, _, _| view.clock.fast_forward_ms(16.0)).unwrap();
            scene.tick(cx);
        }
        assert_eq!(scene.patches(cx).patched_frames, after.patched_frames + 2);
    }

    /// A page whose `<svg>` (a unit once drawn) runs a shape animation, with `size` its box.
    fn spinning(cx: &mut gpui::TestAppContext, size: &'static str) -> Scene {
        spinning_as(cx, size, r#"{"animation": "grow 600ms linear infinite"}"#)
    }

    /// `spinning` with the style of the shape.
    fn spinning_as(cx: &mut gpui::TestAppContext, size: &'static str, style: &'static str) -> Scene {
        let mut elements = icon(size, None);
        elements.iter_mut().find(|element| element.id == 41).unwrap().style = style;
        let scene = Scene::new(cx, elements);
        scene
            .window
            .update(cx, |view, _, _| {
                view.retain.set_planning(true);
                view.retain.set_verify(true);
                view.keyframes.define("grow", r#"{"from": {"r": 2}, "to": {"r": 10}}"#).unwrap();
            })
            .unwrap();
        for _ in 0..6 {
            scene.window.update(cx, |view, _, _| view.clock.fast_forward_ms(16.0)).unwrap();
            scene.notified_draw(cx);
            cx.run_until_parked();
        }
        scene
    }

    #[gpui::test]
    fn an_svg_whose_shape_runs_an_animation_is_a_unit_and_its_ticks_are_patched(cx: &mut gpui::TestAppContext) {
        let scene = spinning(cx, SMALL_ICON);
        assert_eq!(units_of(&scene, cx), vec![40]);
        let before = scene.patches(cx);
        for _ in 0..4 {
            scene.window.update(cx, |view, _, _| view.clock.fast_forward_ms(16.0)).unwrap();
            scene.tick(cx);
        }
        let after = scene.patches(cx);
        assert_eq!(after.patched_frames - before.patched_frames, 4);
        assert_eq!(after.total_declines(), before.total_declines());
    }

    #[gpui::test]
    fn the_tick_after_the_frame_that_found_a_shape_animation_over_is_built_whole(cx: &mut gpui::TestAppContext) {
        let scene = spinning_as(cx, SMALL_ICON, r#"{"animation": "grow 600ms linear forwards"}"#);
        assert_eq!(units_of(&scene, cx), vec![40]);
        // The frame that finds the animation over is still built as the unit the frame before found.
        scene.window.update(cx, |view, _, _| view.clock.fast_forward_ms(1000.0)).unwrap();
        cx.update_window(scene.window.into(), |_, window, app| {
            window.refresh();
            window.draw(app).clear(app);
        })
        .unwrap();
        assert_eq!(units_of(&scene, cx), vec![40]);
        // The shape holds its end value and is listed as animated, but the next frame built whole makes
        // no unit of the svg: a patch would repaint a root that frame does not have.
        let before = scene.patches(cx);
        scene.window.update(cx, |view, _, _| view.clock.fast_forward_ms(16.0)).unwrap();
        scene.tick(cx);
        let after = scene.patches(cx);
        assert_eq!(after.patched_frames, before.patched_frames);
        assert_eq!(units_of(&scene, cx), Vec::<u64>::new());
    }

    #[gpui::test]
    fn an_svg_too_big_to_raster_in_paint_whose_shape_runs_is_no_unit(cx: &mut gpui::TestAppContext) {
        let scene = spinning(cx, r#"{"width": 300, "height": 200}"#);
        assert_eq!(units_of(&scene, cx), Vec::<u64>::new());
        let before = scene.patches(cx);
        for _ in 0..4 {
            scene.window.update(cx, |view, _, _| view.clock.fast_forward_ms(16.0)).unwrap();
            scene.tick(cx);
            cx.run_until_parked();
        }
        assert_eq!(scene.patches(cx).patched_frames, before.patched_frames);
    }

    #[gpui::test]
    fn a_tick_declined_after_its_clocks_stepped_emits_each_event_once(cx: &mut gpui::TestAppContext) {
        let scene = tinted(cx, false);
        let seen = Arc::new(Mutex::new(Vec::new()));
        scene
            .window
            .update(cx, |view, _, _| {
                let seen = seen.clone();
                view.event_callback = Some(Arc::new(move |payload| seen.lock().unwrap().push(payload.event_type)));
            })
            .unwrap();
        scene.tree.lock().unwrap().set_event_listener(20, "animationiteration".to_owned(), true);
        let iterations = || seen.lock().unwrap().iter().filter(|event| *event == "animationiteration").count();
        // 600 ms loops, 96 ms in: a tick on each side of the end of the first iteration.
        scene.window.update(cx, |view, _, _| view.clock.fast_forward_ms(480.0)).unwrap();
        scene.notified_draw(cx);
        assert_eq!(iterations(), 0);
        scene.window.update(cx, |view, _, _| view.clock.fast_forward_ms(48.0)).unwrap();
        let built = scene
            .window
            .update(cx, |view, window, cx| view.build_patch(window, cx).map(|built| built.units.len()))
            .unwrap();
        assert_eq!(built, Some(1), "the tick was stepped and its unit built; gpui declines the patch");
        assert_eq!(iterations(), 1);
        scene.notified_draw(cx);
        assert_eq!(iterations(), 1, "the frame built whole in its place finds the iteration reported");
        scene.window.update(cx, |view, _, _| view.clock.fast_forward_ms(16.0)).unwrap();
        scene.tick(cx);
        assert_eq!(iterations(), 1);
    }

    #[gpui::test]
    fn a_card_with_more_elements_than_a_unit_may_hold_is_built_as_before(cx: &mut gpui::TestAppContext) {
        let mut elements = page([CARD, TINTED, CARD]);
        for index in 0..UNIT_MAX_ELEMENTS as u64 {
            elements.push(div(1000 + index, Some(20), r#"{"height": 1}"#));
        }
        crate::perf_counters::reset();
        let scene = Scene::new(cx, elements);
        scene
            .window
            .update(cx, |view, _, _| {
                view.retain.set_planning(true);
                view.keyframes
                    .define("tint", r##"{"from": {"color": "#ff0000"}, "to": {"color": "#0000ff"}}"##)
                    .unwrap();
            })
            .unwrap();
        for _ in 0..6 {
            scene.window.update(cx, |view, _, _| view.clock.fast_forward_ms(16.0)).unwrap();
            scene.notified_draw(cx);
        }
        assert_eq!(units_of(&scene, cx), Vec::<u64>::new());
        assert!(crate::perf_counters::snapshot().units_oversized >= 1);
    }
}
