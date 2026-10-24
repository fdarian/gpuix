//! What a retained subtree writes into gpuix's own registries while it prepaints and paints, kept
//! so that a frame that replays the subtree writes it again (#37 step 5b).
//!
//! gpui replays the scene, hitboxes, listeners and accessibility nodes of a retained subtree, but
//! not what its listeners and canvases publish into the registries gpuix keeps outside the frame:
//! the painted boxes `getBoundingClientRect` reads, the selection and start regions, the inline
//! hit paragraphs, the painted text log, the measurements. Those registries are reset at the top
//! of every painted frame, so what a skipped subtree published last frame is gone this frame.
//!
//! A [`Tape`] is the list of those writes, per phase, as closures that hold what they write. The
//! [`Taped`] element around the root of a subtree sets the tape it records into while the subtree
//! prepaints and paints. Every registry write that a skipped subtree has to repeat goes through
//! [`record`], next to the write itself, so a registry that gains a writer in a paint listener
//! has one place to say so.
//!
//! The build is the other half. A subtree that is not built does not do what building it does
//! outside the frame (it marks its elements as measured, asks for the images its styles name):
//! those are [`BuildEffect`]s, kept by `retain` with the record of the subtree and applied when a
//! frame plans to skip it.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui::{AnyElement, App, Bounds, Edges, Element, ElementId, GlobalElementId, InspectorElementId, LayoutId, Pixels, Point, Window, WindowId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Prepaint,
    Paint,
    /// A pass that lays out and does not paint: the writes of both phases that such a pass makes
    /// too, see [`write_in_layout`].
    Layout,
    /// A pass that lays out and does not paint puts back what a pass that only laid out wrote of
    /// a subtree it built: the layout tape of the subtree, whose prepaint and publications are all
    /// writes of such a pass.
    LayoutRecorded,
    /// [`Phase::Layout`] for a subtree that the stacking context around its parent paints (a
    /// hole): its prepaint was replayed with the parent's, so only the writes that paint makes in
    /// such a pass are left.
    LayoutPaint,
    /// What an element the subtree handed to the stacking context around it wrote while the context
    /// painted it, by the ordinal of the element counted from the first the subtree handed over. It
    /// is outside the paint of the subtree, though a replay of the subtree writes it again.
    Lifted(u32),
}

/// The registry a write goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Bounds,
    StartRegion,
    Painted,
    Text,
    Paragraph,
    /// `Measurements::by_element`, which lives across frames and is not reset by one: the replay
    /// of a subtree leaves it as the build wrote it, and only the tape keeps the write, to say
    /// what the build would have written ([`Tape::same_writes`], a verified frame).
    Measured,
}

#[derive(Clone, Copy, Default)]
struct Kinds(u8);

impl Kinds {
    fn with(self, kind: Kind) -> Self {
        Self(self.0 | 1 << kind as u8)
    }

    fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    fn has(self, kind: Kind) -> bool {
        self.0 & 1 << kind as u8 != 0
    }

    fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// A registry write that is kept, said in words and handed to the registry that reads it back.
/// A write that holds a position is read `offset` away from where it was recorded: a subtree
/// that a scroller moved is replayed where the scroll put it.
trait Replay {
    fn describe(&self, offset: Point<Pixels>) -> String;
    /// Whether a pass that only lays out makes the write too, from prepaint.
    fn in_layout(&self) -> bool;
    fn kind(&self) -> Kind;
    fn data(&self) -> &dyn Any;
}

struct Write<D> {
    data: D,
    describe: fn(&D, Point<Pixels>) -> String,
    in_layout: bool,
    kind: Kind,
}

impl<D: 'static> Replay for Write<D> {
    fn describe(&self, offset: Point<Pixels>) -> String {
        (self.describe)(&self.data, offset)
    }

    fn in_layout(&self) -> bool {
        self.in_layout
    }

    fn kind(&self) -> Kind {
        self.kind
    }

    fn data(&self) -> &dyn Any {
        &self.data
    }
}

type Effect = Rc<dyn Replay>;

#[derive(Clone)]
enum Entry {
    Write(Effect),
    /// The writes of another subtree, `by` away from where they were made: a subtree that was
    /// replayed or built inside this one while it recorded. They are held by reference, so that
    /// recording a replay costs the same however much the replayed subtree wrote.
    Nested(Rc<Entries>, Point<Pixels>),
}

thread_local! {
    /// The list each list that was nested in another was last nested in, and by how much: how a
    /// list is found to be put back by a replay of the lists around it, see [`Live::delta_of`].
    static PARENTS: RefCell<rustc_hash::FxHashMap<u64, (u64, Point<Pixels>)>> = RefCell::new(rustc_hash::FxHashMap::default());
    static SERIAL: Cell<u64> = const { Cell::new(1) };
    /// The serial of the list the write being made is recorded into, see [`owner`].
    static OWNER: Cell<u64> = const { Cell::new(0) };
}

/// The writes of one phase of one subtree, in the order they were made. Once it has been
/// recorded it is never changed: a later recording makes a list of its own, so a registry that
/// holds this one (a replay that has not been read yet) reads what the subtree wrote then.
pub(crate) struct Entries {
    /// Names this list for as long as it lives, for a registry to say which writes it holds
    /// without holding the list (see [`owner`]).
    serial: u64,
    list: Vec<Entry>,
    /// Of the writes in the list and in the lists nested in it.
    kinds: Kinds,
    layout_kinds: Kinds,
}

/// A list of its own with the writes of this one: what a recording does to a list that a replay
/// already holds, so that the replay is left as it was.
impl Clone for Entries {
    fn clone(&self) -> Self {
        let serial = next_serial() | (self.serial & LAID_OUT);
        for entry in &self.list {
            if let Entry::Nested(child, by) = entry {
                PARENTS.with(|parents| parents.borrow_mut().insert(child.serial, (serial, *by)));
            }
        }
        Self { serial, list: self.list.clone(), kinds: self.kinds, layout_kinds: self.layout_kinds }
    }
}

fn next_serial() -> u64 {
    SERIAL.replace(SERIAL.get() + 1)
}

/// Set in the serial of a list of a layout tape, for a registry that keeps the writes of replays
/// by list to tell them from the lists of a painted tape: a pass that builds a subtree and leaves
/// no tape of its own must not forget the box a painted tape holds, nor the reverse.
const LAID_OUT: u64 = 1 << 63;

/// Whether a list, by its serial, belongs to a layout tape.
pub(crate) fn is_laid_out(serial: u64) -> bool {
    serial & LAID_OUT != 0
}

impl Drop for Entries {
    fn drop(&mut self) {
        // The thread may be on its way out, with nothing left to forget.
        PARENTS.try_with(|parents| parents.borrow_mut().remove(&self.serial)).ok();
    }
}

impl Entries {
    fn new(laid_out: bool) -> Rc<Self> {
        Rc::new(Self {
            serial: next_serial() | if laid_out { LAID_OUT } else { 0 },
            list: Vec::new(),
            kinds: Kinds::default(),
            layout_kinds: Kinds::default(),
        })
    }

    fn push_write(&mut self, effect: Effect) {
        self.kinds = self.kinds.with(effect.kind());
        if effect.in_layout() {
            self.layout_kinds = self.layout_kinds.with(effect.kind());
        }
        self.list.push(Entry::Write(effect));
    }

    fn push_nested(&mut self, child: Rc<Entries>, by: Point<Pixels>) {
        if child.list.is_empty() {
            return;
        }
        self.kinds = self.kinds.union(child.kinds);
        self.layout_kinds = self.layout_kinds.union(child.layout_kinds);
        PARENTS.with(|parents| parents.borrow_mut().insert(child.serial, (self.serial, by)));
        self.list.push(Entry::Nested(child, by));
    }

    /// Calls `visit` with every write, `offset` away from where it was made, in the order it was
    /// made. A pass that only lays out makes `layout_only` of them.
    fn walk(&self, offset: Point<Pixels>, layout_only: bool, visit: &mut dyn FnMut(&Effect, Point<Pixels>)) {
        for entry in &self.list {
            match entry {
                Entry::Write(effect) => {
                    if !layout_only || effect.in_layout() {
                        visit(effect, offset);
                    }
                }
                Entry::Nested(child, by) => {
                    if !layout_only || !child.layout_kinds.is_empty() {
                        child.walk(offset + *by, layout_only, visit);
                    }
                }
            }
        }
    }

    fn described(&self, phase: &str, delta: Point<Pixels>) -> String {
        let mut text = String::new();
        self.walk(delta, false, &mut |effect, offset| {
            text.push_str(&format!(
                "{phase}{} {}\n",
                if effect.in_layout() { "+layout" } else { "" },
                effect.describe(offset)
            ));
        });
        text
    }

    fn lines(&self) -> Vec<(bool, String)> {
        self.lines_by(|effect, offset| effect.describe(offset))
    }

    /// [`Self::lines`] with the boxes described apart from the transforms around them.
    fn lines_apart_from_transforms(&self) -> Vec<(bool, String)> {
        self.lines_by(|effect, offset| match effect.kind() {
            Kind::Bounds => crate::automation::describe_apart_from_transform(effect.data(), offset),
            _ => effect.describe(offset),
        })
    }

    fn lines_by(&self, describe: impl Fn(&Effect, Point<Pixels>) -> String) -> Vec<(bool, String)> {
        let mut lines = Vec::new();
        self.walk(Point::default(), false, &mut |effect, offset| lines.push((effect.in_layout(), describe(effect, offset))));
        lines
    }
}

/// The writes of a replay that a registry has been told of and has not read, see [`publish`].
#[derive(Clone)]
pub(crate) struct Live {
    entries: Rc<Entries>,
    delta: Point<Pixels>,
    layout_only: bool,
}

impl Live {
    /// Whether the replay writes anything for the registry of `kind`.
    pub(crate) fn has(&self, kind: Kind) -> bool {
        if self.layout_only { self.entries.layout_kinds.has(kind) } else { self.entries.kinds.has(kind) }
    }

    /// Calls `read` with the data of every write to the registry of `kind`, and how far from
    /// where it was made it is made now, in the order it was made.
    pub(crate) fn each(&self, kind: Kind, mut read: impl FnMut(&dyn Any, Point<Pixels>)) {
        self.entries.walk(self.delta, self.layout_only, &mut |effect, offset| {
            if effect.kind() == kind {
                read(effect.data(), offset);
            }
        });
    }

    /// Notes the list that the replay puts back as live, with how far from where it was written
    /// it is, for [`owner`] to be looked up in with [`delta_of`].
    pub(crate) fn mark(&self, live: &mut rustc_hash::FxHashMap<u64, Point<Pixels>>) {
        live.insert(self.entries.serial, self.delta);
    }
}

/// How far from where it was written the list `owner` is put back, if a replay in `live` puts it
/// back: the replay of the list or of one that it was nested in, however deep.
pub(crate) fn delta_of(owner: u64, live: &rustc_hash::FxHashMap<u64, Point<Pixels>>) -> Option<Point<Pixels>> {
    PARENTS.with(|parents| {
        let parents = parents.borrow();
        let (mut list, mut offset) = (owner, Point::default());
        loop {
            if let Some(delta) = live.get(&list) {
                return Some(*delta + offset);
            }
            let (parent, by) = parents.get(&list)?;
            (list, offset) = (*parent, offset + *by);
        }
    })
}

/// Tells the registries that read in order, and the one that answers by element, that a replay
/// puts `live` back. They keep it as it is and read it when something asks.
fn publish(window: WindowId, live: &Live) {
    crate::automation::publish(window, live);
    crate::text::publish(window, live);
}

/// A registry that holds the writes of replays by reference, and the writes made around them, in
/// the order they came: reading it expands each replay into the writes it stands for.
pub(crate) struct Layered<T> {
    /// What was read.
    read: Vec<T>,
    /// What came since: a write is `Slot::One`, a replay that has not been expanded `Slot::Live`.
    pending: Vec<Slot<T>>,
}

enum Slot<T> {
    One(T),
    Live(Live),
}

impl<T> Default for Layered<T> {
    fn default() -> Self {
        Self { read: Vec::new(), pending: Vec::new() }
    }
}

impl<T> Layered<T> {
    pub(crate) fn push(&mut self, value: T) {
        if self.pending.is_empty() {
            self.read.push(value);
        } else {
            self.pending.push(Slot::One(value));
        }
    }

    pub(crate) fn push_live(&mut self, live: Live) {
        self.pending.push(Slot::Live(live));
    }

    /// Every write, in order. `from` makes one from the data of a write of `kind` and how far from
    /// where it was made it is made.
    pub(crate) fn all(&mut self, kind: Kind, from: impl Fn(&dyn Any, Point<Pixels>) -> T) -> &[T] {
        for slot in self.pending.drain(..) {
            match slot {
                Slot::One(value) => self.read.push(value),
                Slot::Live(live) => live.each(kind, |data, offset| self.read.push(from(data, offset))),
            }
        }
        &self.read
    }
}

/// The registry writes of one subtree, in the order they were made.
pub(crate) struct Tape {
    prepaint: Rc<Entries>,
    paint: Rc<Entries>,
    lifted: Vec<Rc<Entries>>,
    /// The writes made while it records are kept and not made, see [`Tape::capturing`].
    capture_only: bool,
    /// The tape of a subtree built by a pass that only lays out, see [`Tape::laid_out`].
    laid_out: bool,
}

impl Default for Tape {
    fn default() -> Self {
        Self { prepaint: Entries::new(false), paint: Entries::new(false), lifted: Vec::new(), capture_only: false, laid_out: false }
    }
}

impl Tape {
    /// The tape of a subtree built by a pass that only lays out. Its `prepaint` holds what the
    /// prepaint wrote, and its `paint` what the publications queued by the prepaint wrote once the
    /// whole tree was prepainted ([`Taped`]): a pass that lays out makes both, and a replay of it
    /// puts both back.
    pub(crate) fn laid_out() -> SharedTape {
        Rc::new(RefCell::new(Tape { prepaint: Entries::new(true), paint: Entries::new(true), laid_out: true, ..Tape::default() }))
    }

    /// A tape for a subtree that is prepainted and painted only to see what it would write: the
    /// writes are on the tape and not in the registries, which hold what the frame before made of
    /// the subtree. [`Tape::same_writes`] says whether the frame would have made the same.
    pub(crate) fn capturing() -> SharedTape {
        Rc::new(RefCell::new(Tape { capture_only: true, ..Tape::default() }))
    }

    /// Whether `other` holds the writes this does, phase by phase, in the same order, at the same
    /// places.
    pub(crate) fn same_writes(&self, other: &Tape) -> bool {
        self.prepaint.lines() == other.prepaint.lines()
            && self.paint.lines() == other.paint.lines()
            && self.lifted.len() == other.lifted.len()
            && self.lifted.iter().zip(&other.lifted).all(|(left, right)| left.lines() == right.lines())
    }

    /// [`Self::same_writes`] where the boxes of the elements may differ in the transforms around
    /// them. A patch of a transform that moves paints the same, and the boxes it publishes are the
    /// ones of [`Self::each_bounds`].
    pub(crate) fn same_writes_apart_from_transforms(&self, other: &Tape) -> bool {
        self.prepaint.lines_apart_from_transforms() == other.prepaint.lines_apart_from_transforms()
            && self.paint.lines_apart_from_transforms() == other.paint.lines_apart_from_transforms()
            && self.lifted.len() == other.lifted.len()
            && self
                .lifted
                .iter()
                .zip(&other.lifted)
                .all(|(left, right)| left.lines_apart_from_transforms() == right.lines_apart_from_transforms())
    }

    /// Calls `visit` with the data of every write to the registry of boxes, and how far from where
    /// it was made it is made now (none).
    pub(crate) fn each_bounds(&self, visit: &mut dyn FnMut(&dyn Any, Point<Pixels>)) {
        for entries in [&self.prepaint, &self.paint].into_iter().chain(&self.lifted) {
            entries.walk(Point::default(), false, &mut |effect, offset| {
                if effect.kind() == Kind::Bounds {
                    visit(effect.data(), offset);
                }
            });
        }
    }

    fn entries(&self, phase: Phase) -> Option<&Rc<Entries>> {
        match phase {
            Phase::Prepaint => Some(&self.prepaint),
            Phase::Paint => Some(&self.paint),
            Phase::Lifted(rel) => self.lifted.get(rel as usize),
            Phase::Layout | Phase::LayoutPaint | Phase::LayoutRecorded => {
                unreachable!("nothing records into the layout phases: they replay the others")
            }
        }
    }

    /// The list `phase` is recorded into, for a recording to add to.
    fn recording(&mut self, phase: Phase) -> &mut Entries {
        let list = match phase {
            Phase::Prepaint => &mut self.prepaint,
            Phase::Paint => &mut self.paint,
            Phase::Lifted(rel) => &mut self.lifted[rel as usize],
            Phase::Layout | Phase::LayoutPaint | Phase::LayoutRecorded => {
                unreachable!("nothing records into the layout phases: they replay the others")
            }
        };
        Rc::make_mut(list)
    }

    /// Starts a list of its own for `phase`, so that what a replay of the one before holds is left
    /// as it was.
    fn start(&mut self, phase: Phase) {
        match phase {
            Phase::Prepaint => self.prepaint = Entries::new(self.laid_out),
            Phase::Paint => self.paint = Entries::new(self.laid_out),
            Phase::Lifted(rel) => {
                let rel = rel as usize;
                let laid_out = self.laid_out;
                if self.lifted.len() <= rel {
                    self.lifted.resize_with(rel + 1, || Entries::new(laid_out));
                }
                self.lifted[rel] = Entries::new(laid_out);
            }
            Phase::Layout | Phase::LayoutPaint | Phase::LayoutRecorded => {
                unreachable!("nothing records into the layout phases: they replay the others")
            }
        }
    }

    /// Puts back what `phase` of the subtree wrote, `delta` from where it wrote it: the registries
    /// are told, and read it when something asks. A subtree inside another that is being recorded
    /// is part of its tape too.
    pub(crate) fn replay(&self, window: WindowId, phase: Phase, delta: Point<Pixels>) {
        debug_assert!(!is_capturing(), "a subtree that is only captured holds no subtree to replay");
        let put = |entries: &Rc<Entries>, layout_only: bool| {
            if !entries.list.is_empty() {
                publish(window, &Live { entries: entries.clone(), delta, layout_only });
            }
        };
        match phase {
            // A pass that only lays out records no tape, so this is not part of one. It makes the
            // writes of prepaint, and those of paint and of the elements handed to the stacking
            // context that it makes from prepaint too.
            Phase::Layout => {
                put(&self.prepaint, false);
                put(&self.paint, true);
                for lifted in &self.lifted {
                    put(lifted, true);
                }
                return;
            }
            Phase::LayoutPaint => return put(&self.paint, true),
            // The subtree was built by a pass that only laid out, and the pass that skips it is
            // inside another that is being recorded, if it is inside one: the writes are part of
            // that one's tape either way.
            Phase::LayoutRecorded => {
                debug_assert!(self.laid_out, "a layout record is replayed from a layout tape");
                put(&self.prepaint, false);
                put(&self.paint, false);
                RECORDING.with(|recordings| {
                    if let Some(recording) = recordings.borrow().last() {
                        debug_assert!(
                            recording.phase == Phase::Paint,
                            "the publications of a pass that only lays out are recorded in the paint list"
                        );
                        let mut outer = recording.tape.borrow_mut();
                        let outer = outer.recording(recording.phase);
                        outer.push_nested(self.prepaint.clone(), delta);
                        outer.push_nested(self.paint.clone(), delta);
                    }
                });
                return;
            }
            Phase::Prepaint | Phase::Paint | Phase::Lifted(_) => {}
        }
        let Some(entries) = self.entries(phase) else {
            return;
        };
        put(entries, false);
        RECORDING.with(|recordings| {
            if let Some(recording) = recordings.borrow().last() {
                // What the context paints, of the subtree or of what it handed over, is replayed
                // inside whichever of the two is being recorded.
                debug_assert!(
                    match phase {
                        Phase::Prepaint => recording.phase == Phase::Prepaint,
                        _ => matches!(recording.phase, Phase::Paint | Phase::Lifted(_)),
                    },
                    "a subtree is replayed in the phase it is recorded in"
                );
                recording.tape.borrow_mut().recording(recording.phase).push_nested(entries.clone(), delta);
            }
        });
    }

    /// What the tape writes `delta` from where it wrote it, as text, one line per write.
    pub(crate) fn describe(&self, delta: Point<Pixels>) -> String {
        let lifted = (0..self.lifted.len() as u32).map(|rel| self.describe_lifted(rel)).collect::<String>();
        self.prepaint.described("prepaint", delta) + &self.paint.described("paint", delta) + &lifted
    }

    /// What the element handed to the stacking context as `rel` wrote, as text.
    fn describe_lifted(&self, rel: u32) -> String {
        self.lifted
            .get(rel as usize)
            .map_or_else(String::new, |entries| entries.described(&format!("lifted {rel} "), Point::default()))
    }
}

pub(crate) type SharedTape = Rc<RefCell<Tape>>;

/// The tape of the last build of each retained root, by element id.
#[derive(Default)]
pub(crate) struct TapeStore {
    tapes: RefCell<rustc_hash::FxHashMap<u64, SharedTape>>,
    /// The tape of the last build of each retained root by a pass that only lays out. Kept apart
    /// from `tapes`, which belong to the recordings of presented frames: a pass that lays out must
    /// leave those for the draw after it.
    laid_out: RefCell<rustc_hash::FxHashMap<u64, SharedTape>>,
    /// While gpui compares frames that replayed with frames built whole: what the tape of each
    /// root that replayed was, when it was replayed.
    verifying: std::cell::Cell<bool>,
    replayed: RefCell<Vec<(u64, String)>>,
    /// The writes of elements handed to the stacking context that a replay put back, by root and
    /// ordinal: a root that replayed has to have put back every one it wrote when it was built.
    replayed_lifted: RefCell<Vec<(u64, u32, String)>>,
}

pub(crate) type Tapes = Rc<TapeStore>;

impl TapeStore {
    pub(crate) fn set(&self, root: u64, tape: SharedTape) {
        self.tapes.borrow_mut().insert(root, tape);
    }

    /// `tape` is what a pass that only lays out wrote when it built `root`.
    pub(crate) fn set_laid_out(&self, root: u64, tape: SharedTape) {
        self.laid_out.borrow_mut().insert(root, tape);
    }

    /// Puts back what the pass that only laid out wrote when it built `root`, `delta` from where it
    /// wrote it.
    fn replay_laid_out(&self, window: WindowId, root: u64, delta: Point<Pixels>) {
        let tape = self
            .laid_out
            .borrow()
            .get(&root)
            .expect("a root skipped with its layout record has the tape of the build that made it")
            .clone();
        tape.borrow().replay(window, Phase::LayoutRecorded, delta);
    }

    /// Whether `captured`, the tape of a subtree prepainted and painted again, holds the writes
    /// that the tape of the build of `root` the frame was recorded from holds, the transforms around
    /// its elements aside.
    pub(crate) fn same_as_recorded(&self, root: u64, captured: &SharedTape) -> bool {
        self.tapes
            .borrow()
            .get(&root)
            .is_some_and(|recorded| captured.borrow().same_writes_apart_from_transforms(&recorded.borrow()))
    }

    /// Replays what the last build of `root` wrote in `phase`, `delta` from where it wrote it.
    pub(crate) fn replay(&self, window: WindowId, root: u64, phase: Phase, delta: Point<Pixels>) {
        if phase == Phase::LayoutRecorded {
            return self.replay_laid_out(window, root, delta);
        }
        let tape = self
            .tapes
            .borrow()
            .get(&root)
            .expect("a replayed root has the tape of the build it was recorded from")
            .clone();
        if self.verifying.get() {
            match phase {
                Phase::Prepaint => self.replayed.borrow_mut().push((root, tape.borrow().describe(delta))),
                Phase::Lifted(rel) => self.replayed_lifted.borrow_mut().push((root, rel, tape.borrow().describe_lifted(rel))),
                _ => {}
            }
        }
        tape.borrow().replay(window, phase, delta);
    }

    /// The context starts to paint an element that `root` handed to it: what the element writes
    /// until [`Self::end_lifted`] is on the tape of `root`, under `rel`.
    pub(crate) fn begin_lifted(&self, root: u64, rel: u32) {
        let tape = self
            .tapes
            .borrow()
            .get(&root)
            .expect("a root that hands an element to the stacking context has the tape of its build")
            .clone();
        tape.borrow_mut().start(Phase::Lifted(rel));
        RECORDING.with(|recordings| recordings.borrow_mut().push(Recording { tape, phase: Phase::Lifted(rel) }));
    }

    pub(crate) fn end_lifted(&self, root: u64) {
        let recording = RECORDING.with(|recordings| recordings.borrow_mut().pop().expect("a lifted element that begun is ended"));
        debug_assert!(
            matches!(recording.phase, Phase::Lifted(_))
                && self.tapes.borrow().get(&root).is_some_and(|tape| Rc::ptr_eq(tape, &recording.tape)),
            "the ends of lifted elements come in the reverse order of their beginnings"
        );
        recording.end();
    }

    pub(crate) fn verify(&self, verifying: bool) {
        self.verifying.set(verifying);
    }

    /// Forgets the roots that replayed in the pass before.
    pub(crate) fn forget_replays(&self) {
        self.replayed.borrow_mut().clear();
        self.replayed_lifted.borrow_mut().clear();
    }

    /// What the roots that replayed wrote: in the pass that replayed them, what their tapes said
    /// when they did; in the pass that built them, what their tapes say now.
    pub(crate) fn describe_replays(&self, built: bool) -> String {
        let mut replayed = self.replayed.borrow().clone();
        replayed.sort_by_key(|(root, _)| *root);
        replayed
            .into_iter()
            .map(|(root, replayed)| {
                let (tape, lifted) = if built {
                    self.tapes.borrow().get(&root).map_or_else(Default::default, |tape| {
                        let tape = tape.borrow();
                        let lifted = (0..tape.lifted.len() as u32)
                            .map(|rel| tape.describe_lifted(rel))
                            .filter(|slice| !slice.is_empty())
                            .collect::<String>();
                        (tape.describe(Point::default()), lifted)
                    })
                } else {
                    let mut slices: Vec<(u32, String)> = self
                        .replayed_lifted
                        .borrow()
                        .iter()
                        .filter(|(replayed_root, _, slice)| *replayed_root == root && !slice.is_empty())
                        .map(|(_, rel, slice)| (*rel, slice.clone()))
                        .collect();
                    slices.sort_by_key(|(rel, _)| *rel);
                    (replayed, slices.into_iter().map(|(_, slice)| slice).collect())
                };
                format!("tape {root}\n{tape}put back\n{lifted}")
            })
            .collect()
    }

    pub(crate) fn retain(&self, mut keep: impl FnMut(u64) -> bool) {
        self.tapes.borrow_mut().retain(|root, _| keep(*root));
        self.laid_out.borrow_mut().retain(|root, _| keep(*root));
    }
}

struct Recording {
    tape: SharedTape,
    phase: Phase,
}

impl Recording {
    /// The subtree is done: what it wrote is part of the subtree it is inside, if it is inside one.
    fn end(self) {
        let written = self.tape.borrow().entries(self.phase).expect("a recording has its list").clone();
        RECORDING.with(|recordings| {
            if let Some(outer) = recordings.borrow().last() {
                outer.tape.borrow_mut().recording(outer.phase).push_nested(written, Point::default());
            }
        });
    }
}

thread_local! {
    /// The subtrees being prepainted or painted, innermost last. A write belongs to the tape of
    /// the innermost, which the subtrees around it hold by reference once it is done.
    static RECORDING: RefCell<Vec<Recording>> = const { RefCell::new(Vec::new()) };
    /// How many of them are on a tape that [captures](Tape::capturing).
    static CAPTURING: Cell<u32> = const { Cell::new(0) };
}

/// Whether the writes being made are only noted, see [`Tape::capturing`]. For a registry that is
/// written beside the tape, outside [`write`], to leave alone what the frame before made.
pub(crate) fn is_capturing() -> bool {
    CAPTURING.get() > 0
}

/// Which list the write being made is recorded into, or 0 when it is made outside any subtree
/// that records. A registry that answers by element, and does not keep a write per replay, notes it
/// with the write and asks [`Live::mark`] which lists a replay put back.
pub(crate) fn owner() -> u64 {
    OWNER.get()
}

/// Makes a registry write with `apply`, and notes it on the tape of the subtree being prepainted
/// or painted, if there is one, for `describe` to say in words and a frame that replays the
/// subtree to put back. A frame without retained subtrees pays one thread-local read per write.
///
/// `apply` makes the write where it was made; a replay is read by the registry, which is given
/// how far from there it is (`describe` takes that too: a write that holds a position moves by it,
/// any other ignores it).
pub(crate) fn write<D: 'static>(kind: Kind, data: D, apply: fn(&D), describe: fn(&D, Point<Pixels>) -> String) {
    write_with(kind, data, apply, describe, false);
}

/// [`write`] for a registry that a pass which only lays out (a bounds read) writes too, from
/// prepaint: the painted boxes. A subtree that such a pass does not build repeats it from its tape.
pub(crate) fn write_in_layout<D: 'static>(kind: Kind, data: D, apply: fn(&D), describe: fn(&D, Point<Pixels>) -> String) {
    write_with(kind, data, apply, describe, true);
}

fn write_with<D: 'static>(kind: Kind, data: D, apply: fn(&D), describe: fn(&D, Point<Pixels>) -> String, in_layout: bool) {
    let list = RECORDING.with(|recordings| {
        recordings.borrow().last().map(|recording| recording.tape.borrow_mut().recording(recording.phase).serial)
    });
    if !is_capturing() {
        OWNER.set(list.unwrap_or(0));
        apply(&data);
        OWNER.set(0);
    }
    if list.is_some() {
        let effect: Effect = Rc::new(Write { data, describe, in_layout, kind });
        RECORDING.with(|recordings| {
            let recordings = recordings.borrow();
            let recording = recordings.last().expect("a list was found");
            recording.tape.borrow_mut().recording(recording.phase).push_write(effect);
        });
    }
}

/// Puts `tape` under `phase` for as long as it lives. A guard, so that an unwinding paint does not
/// leave a stale tape for the next frame's unrelated writes.
struct Recorder {
    capturing: bool,
}

impl Recorder {
    fn begin(tape: &SharedTape, phase: Phase) -> Self {
        let capturing = tape.borrow().capture_only;
        tape.borrow_mut().start(phase);
        RECORDING.with(|recordings| {
            recordings.borrow_mut().push(Recording { tape: tape.clone(), phase });
        });
        if capturing {
            CAPTURING.set(CAPTURING.get() + 1);
        }
        Self { capturing }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        let recording = RECORDING.with(|recordings| recordings.borrow_mut().pop());
        if let Some(recording) = recording {
            recording.end();
        }
        if self.capturing {
            CAPTURING.set(CAPTURING.get() - 1);
        }
    }
}

/// The root of a retained subtree, recording what the subtree writes into the registries.
///
/// It has no node of its own: it asks for the layout of the element it holds and says what that
/// element says of its stacking and clip, as `Retained` needs the subtree's root to.
pub(crate) struct Taped {
    child: AnyElement,
    tape: SharedTape,
}

impl Taped {
    pub(crate) fn new(child: AnyElement, tape: SharedTape) -> Self {
        Self { child, tape }
    }

    /// The root of a subtree built by a pass that only lays out, with the tape of [`Tape::laid_out`].
    pub(crate) fn laid_out(child: AnyElement, tape: SharedTape) -> Self {
        debug_assert!(tape.borrow().laid_out);
        Self { child, tape }
    }

    /// A pass that only lays out has no paint: what the listeners of the subtree write from the
    /// publications they queued in prepaint is recorded between two publications queued around the
    /// prepaint, which run in the order they were queued, outside every prepaint.
    fn prepaint_laid_out(&mut self, window: &mut Window, cx: &mut App) {
        let publishing: Rc<RefCell<Option<Recorder>>> = Rc::default();
        window.publish_layout({
            let (publishing, tape) = (publishing.clone(), self.tape.clone());
            move |_, _| *publishing.borrow_mut() = Some(Recorder::begin(&tape, Phase::Paint))
        });
        {
            self.tape.borrow_mut().lifted.clear();
            let _recording = Recorder::begin(&self.tape, Phase::Prepaint);
            self.child.prepaint(window, cx);
        }
        window.publish_layout(move |_, _| drop(publishing.borrow_mut().take()));
    }
}

impl gpui::IntoElement for Taped {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for Taped {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn escapes_to(&self) -> Option<gpui::Escape> {
        self.child.escapes_to()
    }

    fn lift_z(&self) -> Option<i32> {
        self.child.lift_z()
    }

    fn cull_outset(&self) -> Option<Edges<Pixels>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if self.tape.borrow().laid_out {
            return self.prepaint_laid_out(window, cx);
        }
        self.tape.borrow_mut().lifted.clear();
        let _recording = Recorder::begin(&self.tape, Phase::Prepaint);
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let _recording = Recorder::begin(&self.tape, Phase::Paint);
        self.child.paint(window, cx);
    }
}

/// What building an element does outside the frame, which a frame that skips the element does
/// not do and so repeats from the record of the last build.
#[derive(Clone, Debug)]
pub(crate) enum BuildEffect {
    /// The element was wrapped to be measured, which keeps its measurement from being pruned.
    Measured(u64),
    /// A style asked for the image at the address, which keeps it loaded and from being evicted.
    Image(gpui::ImageKey, Rc<str>),
    /// A style read the content box of a query container, which the container compares with the
    /// one it has after layout (`ContainerScope::assumption_failed`). The container is outside the
    /// subtree or inside it; one inside has no scope to repeat the read on, and does not need it.
    ContainerRead(u64, crate::css::container_query::Axes),
}

thread_local! {
    /// The build effects of the session being built, in the order they were made.
    static BUILD_EFFECTS: RefCell<Vec<BuildEffect>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn note_build(effect: BuildEffect) {
    BUILD_EFFECTS.with(|effects| effects.borrow_mut().push(effect));
}

pub(crate) fn build_effects_len() -> usize {
    BUILD_EFFECTS.with(|effects| effects.borrow().len())
}

/// The effects noted from `start`, for the record of the element that was built since.
pub(crate) fn build_effects_since(start: usize) -> Vec<BuildEffect> {
    BUILD_EFFECTS.with(|effects| effects.borrow().get(start..).map_or_else(Vec::new, <[BuildEffect]>::to_vec))
}

/// Forgets the notes made from `len`: a build that is not a session's (see
/// `GpuixView::build_unit`) leaves the list as it found it.
pub(crate) fn truncate_build_effects(len: usize) {
    BUILD_EFFECTS.with(|effects| effects.borrow_mut().truncate(len));
}

/// Forgets every note: the frame (or the deferred build) that started a session starts the list
/// over, since a record keeps its own slice.
pub(crate) fn clear_build_effects() {
    BUILD_EFFECTS.with(|effects| effects.borrow_mut().clear());
}


#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        static REGISTRY: Cell<i32> = const { Cell::new(0) };
    }

    fn add(amount: i32) {
        write(Kind::Painted, amount, |amount| REGISTRY.set(REGISTRY.get() + amount), |amount, _| format!("add {amount}"));
    }

    fn record(tape: &SharedTape, writes: &[i32]) {
        let _recording = Recorder::begin(tape, Phase::Paint);
        for amount in writes {
            add(*amount);
        }
    }

    #[test]
    fn a_tape_that_captures_notes_the_writes_and_makes_none() {
        REGISTRY.set(0);
        let recorded = SharedTape::default();
        record(&recorded, &[2, 3]);
        assert_eq!(REGISTRY.get(), 5);

        let captured = Tape::capturing();
        record(&captured, &[2, 3]);
        assert_eq!(REGISTRY.get(), 5, "a capture leaves the registry as the frame made it");
        assert!(!is_capturing(), "and only while it records");
        assert!(captured.borrow().same_writes(&recorded.borrow()));

        let other = Tape::capturing();
        record(&other, &[2, 4]);
        assert!(!other.borrow().same_writes(&recorded.borrow()), "a write that differs");
        let fewer = Tape::capturing();
        record(&fewer, &[2]);
        assert!(!fewer.borrow().same_writes(&recorded.borrow()), "a write that is missing");
        add(1);
        assert_eq!(REGISTRY.get(), 6, "a write outside any capture is made");
    }

    #[test]
    fn the_tape_of_a_root_is_compared_with_the_capture_of_a_build_of_it() {
        let tapes = TapeStore::default();
        let recorded = SharedTape::default();
        record(&recorded, &[7]);
        tapes.set(1, recorded);
        let same = Tape::capturing();
        record(&same, &[7]);
        assert!(tapes.same_as_recorded(1, &same));
        let different = Tape::capturing();
        record(&different, &[8]);
        assert!(!tapes.same_as_recorded(1, &different));
        assert!(!tapes.same_as_recorded(2, &same), "a root with no recording has nothing to be the same as");
    }

    fn laid_out(to_window: gpui::TransformationMatrix) -> crate::automation::LaidOut {
        let mut laid_out = crate::automation::LaidOut::of_box(
            gpui::Bounds::new(gpui::point(gpui::px(10.0), gpui::px(20.0)), gpui::size(gpui::px(30.0), gpui::px(40.0))),
            1.0,
        );
        laid_out.to_window = to_window;
        laid_out
    }

    fn record_box(tape: &SharedTape, window: WindowId, to_window: gpui::TransformationMatrix, width: f32) {
        let _recording = Recorder::begin(tape, Phase::Paint);
        let mut laid_out = laid_out(to_window);
        laid_out.border_box.size.width = gpui::px(width);
        crate::automation::record_bounds(window, 1, laid_out, gpui::point(gpui::px(10.0), gpui::px(20.0)), Default::default());
    }

    #[test]
    fn a_capture_may_differ_from_the_recording_in_the_transforms_and_nothing_else() {
        let window = WindowId::from(911);
        let moved = |dx: f32| gpui::TransformationMatrix { rotation_scale: [[1.0, 0.0], [0.0, 1.0]], translation: [dx, 0.0] };
        let tapes = TapeStore::default();
        let recorded = SharedTape::default();
        record_box(&recorded, window, moved(5.0), 30.0);
        tapes.set(1, recorded.clone());

        let later = Tape::capturing();
        record_box(&later, window, moved(9.0), 30.0);
        assert!(!later.borrow().same_writes(&recorded.borrow()), "the placement is a write like the others");
        assert!(tapes.same_as_recorded(1, &later), "but a patch of a transform that moves paints the same");
        let wider = Tape::capturing();
        record_box(&wider, window, moved(5.0), 31.0);
        assert!(!tapes.same_as_recorded(1, &wider), "a box that is another size is not the same");

        let mut published = 0;
        later.borrow().each_bounds(&mut |data, offset| {
            published += 1;
            crate::automation::publish_captured_bounds(data, offset);
        });
        assert_eq!(published, 1);
        assert_eq!(crate::automation::get_bounds(window, 1).unwrap().to_window, moved(9.0));
    }

    fn log(window: WindowId, text: &'static str) {
        crate::text::log_painted_text(window, text.into());
    }

    fn record_logs(tape: &SharedTape, window: WindowId, texts: &[&'static str]) {
        let _recording = Recorder::begin(tape, Phase::Paint);
        for text in texts {
            log(window, text);
        }
    }

    fn replay(tape: &SharedTape, window: WindowId) {
        tape.borrow().replay(window, Phase::Paint, Point::default());
    }

    #[test]
    fn a_replay_is_read_among_the_writes_made_around_it_in_the_order_they_came() {
        let (recorded_in, window) = (WindowId::from(901), WindowId::from(902));
        let tape = SharedTape::default();
        record_logs(&tape, recorded_in, &["a", "b"]);

        log(window, "before");
        replay(&tape, window);
        log(window, "between");
        replay(&tape, window);
        log(window, "after");
        assert_eq!(crate::text::painted_text(window), ["before", "a", "b", "between", "a", "b", "after"]);
    }

    #[test]
    fn a_replay_holds_what_the_subtree_wrote_then_and_not_what_it_writes_when_built_again() {
        let (recorded_in, window) = (WindowId::from(903), WindowId::from(904));
        let tape = SharedTape::default();
        record_logs(&tape, recorded_in, &["old"]);
        replay(&tape, window);
        record_logs(&tape, recorded_in, &["new"]);
        assert_eq!(crate::text::painted_text(window), ["old"]);
        replay(&tape, window);
        assert_eq!(crate::text::painted_text(window), ["old", "new"]);
    }

    #[test]
    fn a_subtree_that_replays_or_is_built_inside_another_is_part_of_its_tape_either_way() {
        let (recorded_in, window) = (WindowId::from(905), WindowId::from(906));
        let inner = SharedTape::default();
        record_logs(&inner, recorded_in, &["i1", "i2"]);

        let replayed = SharedTape::default();
        {
            let _recording = Recorder::begin(&replayed, Phase::Paint);
            log(window, "o1");
            replay(&inner, window);
            log(window, "o2");
        }
        let built = SharedTape::default();
        {
            let _recording = Recorder::begin(&built, Phase::Paint);
            log(window, "o1");
            record_logs(&SharedTape::default(), window, &["i1", "i2"]);
            log(window, "o2");
        }
        let whole = SharedTape::default();
        record_logs(&whole, window, &["o1", "i1", "i2", "o2"]);

        let said = whole.borrow().describe(Point::default());
        assert_eq!(replayed.borrow().describe(Point::default()), said);
        assert_eq!(built.borrow().describe(Point::default()), said);
        assert!(replayed.borrow().same_writes(&whole.borrow()));
        assert!(built.borrow().same_writes(&whole.borrow()));
    }

    fn box_at(window: WindowId, id: u64, x: f32) {
        use crate::automation::{record_bounds, LaidOut};
        let bounds = Bounds::new(gpui::point(gpui::px(x), gpui::px(0.0)), gpui::size(gpui::px(5.0), gpui::px(5.0)));
        record_bounds(window, id, LaidOut::of_box(bounds, 1.0), bounds.origin, Default::default());
    }

    fn x_of(window: WindowId, id: u64) -> Option<f64> {
        crate::automation::get_bounds(window, id).map(|bounds| bounds.x)
    }

    #[test]
    fn a_box_is_the_one_the_frame_wrote_or_the_one_a_replayed_subtree_recorded_moved_by_the_replay() {
        let window = WindowId::from(907);
        let tape = SharedTape::default();
        {
            let _recording = Recorder::begin(&tape, Phase::Paint);
            box_at(window, 7, 10.0);
        }
        crate::automation::forget_window(window);
        assert_eq!(x_of(window, 7), None, "a frame forgets the boxes of the one before");

        tape.borrow().replay(window, Phase::Paint, gpui::point(gpui::px(3.0), gpui::px(0.0)));
        assert_eq!(x_of(window, 7), Some(13.0));

        // A pass that builds the element and records nothing writes it where the build put it,
        // and the pass after it that replays the subtree finds the box the tape holds.
        crate::automation::forget_window(window);
        box_at(window, 7, 99.0);
        assert_eq!(x_of(window, 7), Some(99.0));
        crate::automation::forget_window(window);
        replay(&tape, window);
        assert_eq!(x_of(window, 7), Some(10.0));

        crate::automation::forget_window(window);
        assert_eq!(x_of(window, 7), None);
    }

    #[test]
    fn a_box_of_a_subtree_inside_another_is_put_back_by_the_replay_of_either() {
        let window = WindowId::from(908);
        let (outer, inner) = (SharedTape::default(), SharedTape::default());
        {
            let _outer = Recorder::begin(&outer, Phase::Paint);
            box_at(window, 1, 5.0);
            let _inner = Recorder::begin(&inner, Phase::Paint);
            box_at(window, 2, 20.0);
        }
        let by = gpui::point(gpui::px(3.0), gpui::px(0.0));

        crate::automation::forget_window(window);
        outer.borrow().replay(window, Phase::Paint, by);
        assert_eq!((x_of(window, 1), x_of(window, 2)), (Some(8.0), Some(23.0)), "the replay moves what is nested in it");

        crate::automation::forget_window(window);
        inner.borrow().replay(window, Phase::Paint, by);
        assert_eq!((x_of(window, 1), x_of(window, 2)), (None, Some(23.0)), "and the replay of what is nested only what it holds");
    }

    fn record_laid_out(tape: &SharedTape, window: WindowId, id: u64, x: f32) {
        let _recording = Recorder::begin(tape, Phase::Paint);
        box_at(window, id, x);
    }

    #[test]
    fn a_box_of_a_layout_tape_and_one_of_a_painted_tape_are_each_put_back_by_their_own_replay() {
        let window = WindowId::from(909);
        let painted = SharedTape::default();
        {
            let _recording = Recorder::begin(&painted, Phase::Paint);
            box_at(window, 1, 10.0);
        }
        let laid_out = Tape::laid_out();
        record_laid_out(&laid_out, window, 1, 20.0);
        assert!(is_laid_out(laid_out.borrow().paint.serial) && !is_laid_out(painted.borrow().paint.serial));

        // The box the layout tape wrote last is the box of the frame, and each replay puts back its own.
        crate::automation::forget_window(window);
        replay(&painted, window);
        assert_eq!(x_of(window, 1), Some(10.0));
        crate::automation::forget_window(window);
        laid_out.borrow().replay(window, Phase::LayoutRecorded, Point::default());
        assert_eq!(x_of(window, 1), Some(20.0));

        // A build that writes without recording leaves both.
        crate::automation::forget_window(window);
        box_at(window, 1, 99.0);
        crate::automation::forget_window(window);
        replay(&painted, window);
        assert_eq!(x_of(window, 1), Some(10.0));
        crate::automation::forget_window(window);
        laid_out.borrow().replay(window, Phase::LayoutRecorded, Point::default());
        assert_eq!(x_of(window, 1), Some(20.0));
    }

    #[test]
    fn a_layout_tape_skipped_inside_another_is_part_of_it() {
        let window = WindowId::from(910);
        let inner = Tape::laid_out();
        record_laid_out(&inner, window, 2, 20.0);

        // The pass that builds the outer subtree skips the inner one, which publishes when the
        // publications of the pass run, inside the outer one's bracket.
        let outer = Tape::laid_out();
        crate::automation::forget_window(window);
        {
            let _recording = Recorder::begin(&outer, Phase::Paint);
            box_at(window, 1, 5.0);
            inner.borrow().replay(window, Phase::LayoutRecorded, Point::default());
        }
        assert_eq!((x_of(window, 1), x_of(window, 2)), (Some(5.0), Some(20.0)));

        crate::automation::forget_window(window);
        outer.borrow().replay(window, Phase::LayoutRecorded, gpui::point(gpui::px(3.0), gpui::px(0.0)));
        assert_eq!((x_of(window, 1), x_of(window, 2)), (Some(8.0), Some(23.0)), "the replay of the outer moves what is nested");
    }

    #[test]
    fn a_root_skipped_with_its_layout_record_is_put_back_where_the_skip_says() {
        let window = WindowId::from(912);
        let store = TapeStore::default();
        let laid_out = Tape::laid_out();
        record_laid_out(&laid_out, window, 1, 20.0);
        store.set_laid_out(7, laid_out);

        // Each skip is as far from the build as the place says, and not from the skip before.
        for by in [3.0_f32, 0.0, 10.0, 10.0, -4.0] {
            crate::automation::forget_window(window);
            store.replay(window, 7, Phase::LayoutRecorded, gpui::point(gpui::px(by), gpui::px(0.0)));
            assert_eq!(x_of(window, 1), Some(f64::from(20.0 + by)));
        }
    }

    #[test]
    fn the_box_of_a_layout_tape_outlasts_the_frames_of_a_page_of_many_boxes() {
        let window = WindowId::from(911);
        let laid_out = Tape::laid_out();
        record_laid_out(&laid_out, window, 1, 20.0);
        // The first reset purges whatever there is, the ones after it only past 4096 boxes.
        crate::automation::forget_window(window);
        for frame in 0..3 {
            for id in 0..5000 {
                box_at(window, 100 + id, frame as f32);
            }
            crate::automation::forget_window(window);
        }
        laid_out.borrow().replay(window, Phase::LayoutRecorded, Point::default());
        assert_eq!(x_of(window, 1), Some(20.0), "the pass that lays the content out again finds its box");

        // Until the element is destroyed.
        crate::automation::retain_bounds(window, |id| id != 1);
        crate::automation::forget_window(window);
        laid_out.borrow().replay(window, Phase::LayoutRecorded, Point::default());
        assert_eq!(x_of(window, 1), None);
    }

    #[test]
    fn the_boxes_a_forced_pass_left_are_read_after_frames_that_did_not_write_them() {
        let window = WindowId::from(913);
        let laid_out = Tape::laid_out();
        record_laid_out(&laid_out, window, 2, 20.0);

        // The pass writes one box and puts back another by a layout record, moved by 3.
        crate::automation::forget_window(window);
        box_at(window, 1, 5.0);
        laid_out.borrow().replay(window, Phase::LayoutRecorded, gpui::point(gpui::px(3.0), gpui::px(0.0)));
        assert_eq!((x_of(window, 1), x_of(window, 2)), (Some(5.0), Some(23.0)));
        crate::automation::keep_bounds(window);

        // The frames after it write and replay neither; one writes another box.
        for frame in 0..3 {
            crate::automation::forget_window(window);
            box_at(window, 3, frame as f32);
            assert_eq!((x_of(window, 1), x_of(window, 2)), (None, None));
        }
        let kept = crate::automation::reading_kept(|| (x_of(window, 1), x_of(window, 2), x_of(window, 3), x_of(window, 4)));
        assert_eq!(kept, (Some(5.0), Some(23.0), Some(2.0), None), "the box a frame wrote is the frame's, and an element no pass wrote has none");
        assert_eq!((x_of(window, 1), x_of(window, 2)), (None, None), "and a read that is not of the pass's does not see them");

        // The boxes outlast a purge of a page of many boxes while they are the pass's.
        for frame in 0..3 {
            for id in 0..5000 {
                box_at(window, 100 + id, frame as f32);
            }
            crate::automation::forget_window(window);
        }
        assert_eq!(crate::automation::reading_kept(|| (x_of(window, 1), x_of(window, 2))), (Some(5.0), Some(23.0)));
    }

    #[test]
    fn the_notes_of_a_build_that_is_not_a_frames_are_taken_back() {
        clear_build_effects();
        note_build(BuildEffect::Measured(1));
        let start = build_effects_len();
        note_build(BuildEffect::Measured(2));
        truncate_build_effects(start);
        assert_eq!(build_effects_len(), 1);
    }
}
