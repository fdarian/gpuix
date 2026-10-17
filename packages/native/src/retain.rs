//! Which subtrees of an element build a retained replay could reuse (#37 step 4a).
//!
//! gpui consumes its elements every frame, so reusing a subtree means replaying what it prepainted
//! and painted instead of building it. That is step 5. This step only decides, per frame and
//! without changing anything, which subtrees *would* be reused if it could, and counts them, so the
//! share of a page that is worth it is known before the replay is written.
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
//! Not known at build time: that layout gives a retained subtree the same box it had. Step 5's
//! replay key checks that, so the numbers here are an upper bound for frames that move layout.

use std::hash::Hasher;

use rustc_hash::{FxHashMap, FxHasher};

use crate::renderer::{GroupScope, Inherited};

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
    /// `onLayout`, `onScroll`, `onResize` or `onIntersect`: its prepaint calls the listener.
    PaintReporter,
    /// It declares a query container, or its style reads one.
    ContainerQuery,
    /// It declares a `highlight`, or is inside one.
    Highlight,
    /// A custom element renders itself, and prepaints and paints for itself.
    CustomElement,
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
    /// `overflow: hidden`, which has a scroll offset JS can set.
    OverflowHidden,
    /// `layout` prop: moves by an offset of its own.
    LayoutAnimation,
    /// Built as part of a paragraph's line.
    AtomicInline,
    /// A `<label>` reads the control it names, which is not in its subtree.
    Label,
}

impl Exclusion {
    const ALL: [Exclusion; 17] = [
        Self::PaintReporter,
        Self::ContainerQuery,
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
        Self::OverflowHidden,
        Self::LayoutAnimation,
        Self::AtomicInline,
        Self::Label,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::PaintReporter => "paint-reporter",
            Self::ContainerQuery => "container-query",
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
            Self::OverflowHidden => "overflow-hidden",
            Self::LayoutAnimation => "layout-animation",
            Self::AtomicInline => "atomic-inline",
            Self::Label => "label",
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
#[derive(Clone, Copy)]
struct Record {
    key: Key,
    vol: Vol,
    digest: u64,
    eligible: bool,
}

const NO_PARENT: u32 = u32::MAX;

/// Where each reason is counted: `perf_counters` keeps one number per slot.
const EXCLUDED: usize = 0;
const UNDER: usize = EXCLUDED + Exclusion::ALL.len();
const CHANGED: usize = UNDER + Exclusion::ALL.len();
const SPINE: usize = CHANGED + Cause::ALL.len();
const LIFTS: usize = SPINE + 1;
const MOVED: usize = LIFTS + 1;
pub(crate) const RETAINED: usize = MOVED + 1;
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
        RETAINED => "retained".to_owned(),
        _ => unreachable!("no slot {slot}"),
    }
}

struct Node {
    id: u64,
    parent: u32,
    /// One past the last node below this one.
    end: u32,
    key: Key,
    vol: Vol,
    exclusion: Option<Exclusion>,
    lifted: bool,
    forms_context: bool,
    /// Only with `verify`: a hash of what the build made of the element itself.
    digest: u64,
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
}

/// The decision for the last build, for tests.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Summary {
    pub roots: Vec<u64>,
    pub elements_in_roots: usize,
    pub elements: usize,
}

pub(crate) struct Retain {
    /// `GPUIX_RETAIN_OFF` stops recording, to time a build without it.
    enabled: bool,
    verify: bool,
    open: bool,
    /// Above zero while a virtual list builds a row, which is outside any render.
    rows: u32,
    selection: u64,
    nodes: Vec<Node>,
    stack: Vec<u32>,
    below: Vec<Below>,
    retained: Vec<bool>,
    moved: Vec<bool>,
    last: FxHashMap<u64, Record>,
    next: FxHashMap<u64, Record>,
    summary: Summary,
    mismatches: Vec<String>,
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
    pub contents_overlay: bool,
    pub anchor_scope: bool,
    /// What an ancestor's `text-box-trim` asks of the ends of the element's lines.
    pub text_box_trim: gpui::TextBoxTrim,
}

impl Retain {
    pub(crate) fn new() -> Self {
        Self {
            enabled: std::env::var_os("GPUIX_RETAIN_OFF").is_none(),
            verify: cfg!(test) || std::env::var_os("GPUIX_RETAIN_VERIFY").is_some(),
            open: false,
            rows: 0,
            selection: 0,
            nodes: Vec::new(),
            stack: Vec::new(),
            below: Vec::new(),
            retained: Vec::new(),
            moved: Vec::new(),
            last: FxHashMap::default(),
            next: FxHashMap::default(),
            summary: Summary::default(),
            mismatches: Vec::new(),
        }
    }

    /// With `verify` every element hashes what it built, and an element that is judged unchanged
    /// has to hash as it did. A mismatch is a hole in the key. Costs a style formatted per element.
    pub(crate) fn set_verify(&mut self, verify: bool) {
        self.verify = verify;
    }

    pub(crate) fn verifying(&self) -> bool {
        self.verify
    }

    pub(crate) fn mismatches(&self) -> &[String] {
        &self.mismatches
    }

    pub(crate) fn summary(&self) -> &Summary {
        &self.summary
    }

    /// `selection` is the identity of the selected text, 0 for none: text that is in a selection
    /// that moved paints differently.
    pub(crate) fn begin_frame(&mut self, selection: u64) {
        self.open = self.enabled;
        self.selection = selection;
        self.nodes.clear();
        self.stack.clear();
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

    pub(crate) fn enter(&mut self, entry: Entry) {
        if !self.enabled {
            return;
        }
        if self.rows > 0 {
            crate::perf_counters::retain_slot(EXCLUDED + Exclusion::VirtualRow as usize, 1);
            return;
        }
        if !self.open {
            return;
        }
        let parent = self.stack.last().copied().unwrap_or(NO_PARENT);
        let mut shape = Fingerprint::new();
        shape.word(entry.id);
        shape.word(u64::from(entry.at));
        shape.word(self.nodes.get(parent as usize).map_or(0, |parent| parent.key.shape));
        shape.bits(&[
            entry.stacking_root,
            entry.stacking_item,
            entry.atomic_root,
            entry.contents_overlay,
            entry.anchor_scope,
            entry.text_box_trim.start,
            entry.text_box_trim.end,
        ]);
        let mut node = Node {
            id: entry.id,
            parent,
            end: 0,
            key: Key {
                revision: entry.revision,
                inherited: inherited_fingerprint(entry.inherited),
                shape: shape.finish(),
            },
            vol: Vol::default(),
            exclusion: None,
            lifted: false,
            forms_context: false,
            digest: 0,
        };
        if entry.atomic_root {
            node.exclusion = Some(Exclusion::AtomicInline);
        } else if entry.contents_overlay {
            node.exclusion = Some(Exclusion::DisplayContents);
        } else if entry.anchor_scope {
            node.exclusion = Some(Exclusion::Anchored);
        } else if entry.inherited.highlight.is_some() {
            node.exclusion = Some(Exclusion::Highlight);
        } else if entry.inherited.table.collapse {
            node.exclusion = Some(Exclusion::CollapsedTable);
        }
        self.stack.push(self.nodes.len() as u32);
        self.nodes.push(node);
    }

    pub(crate) fn exit(&mut self) {
        if self.rows > 0 || !self.open {
            return;
        }
        let at = self.stack.pop().expect("exit without enter");
        self.nodes[at as usize].end = self.nodes.len() as u32;
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

    /// Whether the element is lifted into the stacking context around it, and whether it is one.
    pub(crate) fn stacking(&mut self, decide: impl FnOnce() -> (bool, bool)) {
        if self.recording() {
            let (lifted, forms_context) = decide();
            if let Some(node) = self.top() {
                node.lifted = lifted;
                node.forms_context = forms_context;
            }
        }
    }

    /// Adds what the build made of the element to its digest. Only formatted when verifying.
    pub(crate) fn digest(&mut self, built: impl FnOnce() -> String) {
        if !self.verify {
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

    /// Decides what this build would have reused, counts it, and keeps what the next build compares
    /// itself with.
    pub(crate) fn finish_frame(&mut self) {
        if !self.open {
            return;
        }
        self.open = false;
        debug_assert!(self.stack.is_empty(), "an element was entered and never exited");
        self.stack.clear();
        let count = self.nodes.len();

        self.below.clear();
        self.below.resize(count, Below::default());
        for (at, node) in self.nodes.iter().enumerate() {
            let before = self.last.get(&node.id);
            let below = &mut self.below[at];
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
            let escapes = node.lifted || (!node.forms_context && own.lifts);
            let mut fp = Fingerprint::new();
            fp.word(node.digest);
            fp.word(own.digest);
            let digest = fp.finish();
            self.below[at].digest = digest;
            if node.parent == NO_PARENT {
                continue;
            }
            let parent = &mut self.below[node.parent as usize];
            parent.changed_below |= own.changed.is_some() || own.changed_below;
            parent.lifts |= escapes;
            if let Some(excluded) = node.exclusion.or(own.excluded) {
                parent.excluded = Some(excluded);
            }
            parent.digest = parent.digest.rotate_left(5).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ digest;
        }

        self.retained.clear();
        self.retained.resize(count, false);
        self.moved.clear();
        self.moved.resize(count, false);
        self.next.clear();
        self.summary = Summary { roots: Vec::new(), elements_in_roots: 0, elements: count };
        let mut counts = [0u64; SLOTS];
        let mut large = 0;
        for at in 0..count {
            let node = &self.nodes[at];
            let own = self.below[at];
            let eligible = node.exclusion.is_none() && own.excluded.is_none() && (node.forms_context || !own.lifts);
            let clean = own.changed.is_none() && !own.changed_below;
            let parent = (node.parent != NO_PARENT).then_some(node.parent as usize);
            self.moved[at] = parent.is_some_and(|parent| self.moved[parent] || self.below[parent].scrolled);

            if parent.is_some_and(|parent| self.retained[parent]) {
                self.retained[at] = true;
            } else if clean && eligible && self.moved[at] {
                counts[MOVED] += 1;
            } else if clean && eligible {
                self.retained[at] = true;
                let size = (node.end as usize) - at;
                self.summary.roots.push(node.id);
                self.summary.elements_in_roots += size;
                if size >= LARGE_ROOT {
                    large += size;
                }
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
            }

            if self.retained[at] && self.verify {
                check(&self.last, &mut self.mismatches, node.id, own.digest, eligible);
            }
            self.next.insert(
                node.id,
                Record { key: node.key, vol: node.vol, digest: own.digest, eligible },
            );
        }
        std::mem::swap(&mut self.last, &mut self.next);

        counts[RETAINED] = self.summary.elements_in_roots as u64;
        crate::perf_counters::retain_slots(&counts);
        crate::perf_counters::retained(self.summary.roots.len() as u64, large as u64);
    }
}

/// An element judged unchanged has to have been built as it was last time: the hash of what the
/// build made of it and of everything below it is the one the last build recorded.
fn check(last: &FxHashMap<u64, Record>, mismatches: &mut Vec<String>, id: u64, digest: u64, eligible: bool) {
    let Some(before) = last.get(&id) else { return };
    let problem = if before.digest != digest {
        "built differently"
    } else if before.eligible != eligible {
        "was judged differently"
    } else {
        return;
    };
    crate::perf_counters::retain_mismatch();
    let message = format!("element {id} was judged unchanged and {problem}");
    log::error!("{message}");
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
        retain.begin_frame(0);
        retain.enter(Entry {
            id: 1,
            revision: 7,
            inherited,
            at: 0,
            stacking_root: true,
            stacking_item: false,
            atomic_root: false,
            contents_overlay: false,
            anchor_scope: false,
            text_box_trim: Default::default(),
        });
        retain.stacking(|| (false, true));
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

        fn set_style(&self, id: u64, style: &str) {
            let mut tree = self.tree.lock().unwrap();
            let style = tree.styles.intern(style.as_bytes()).unwrap();
            tree.set_style(id, style);
        }
    }

    const CARD: &str = r#"{"height": 60}"#;
    const HOVER_CARD: &str = r##"{"height": 60, "hover": {"backgroundColor": "#aa0000"}}"##;

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
    fn a_container_that_resizes_is_never_retained_and_neither_is_what_reads_it(cx: &mut gpui::TestAppContext) {
        let mut elements = page([CARD; 3]);
        elements.push(div(40, Some(1), r#"{"width": 200, "height": 30, "containerType": "inline-size"}"#));
        elements.push(div(41, Some(40), r#"{"width": "50cqw", "height": 10}"#));
        let scene = Scene::new(cx, elements);
        scene.settle(cx);
        assert_eq!(scene.roots(cx), vec![10, 20, 30]);
        scene.set_style(40, r#"{"width": 100, "height": 30, "containerType": "inline-size"}"#);
        scene.draw(cx);
        scene.draw(cx);
        assert_eq!(scene.roots(cx), vec![10, 20, 30]);
        scene.set_style(40, r#"{"width": 100, "height": 30}"#);
        scene.set_style(41, r#"{"width": 50, "height": 10}"#);
        scene.settle(cx);
        assert_eq!(scene.roots(cx), vec![1], "no longer a container, so nothing in the way");
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
}
