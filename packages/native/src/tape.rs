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

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui::{AnyElement, App, Bounds, Edges, Element, ElementId, GlobalElementId, InspectorElementId, LayoutId, Pixels, Point, Window};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Prepaint,
    Paint,
    /// A pass that lays out and does not paint: the writes of both phases that such a pass makes
    /// too, see [`write_in_layout`].
    Layout,
    /// [`Phase::Layout`] for a subtree that the stacking context around its parent paints (a
    /// hole): its prepaint was replayed with the parent's, so only the writes that paint makes in
    /// such a pass are left.
    LayoutPaint,
    /// What an element the subtree handed to the stacking context around it wrote while the context
    /// painted it, by the ordinal of the element counted from the first the subtree handed over. It
    /// is outside the paint of the subtree, though a replay of the subtree writes it again.
    Lifted(u32),
}

/// A registry write that can be made again, and said in words. A write that holds a position is
/// made `offset` away from where it was recorded: a subtree that a scroller moved is replayed
/// where the scroll put it.
trait Replay {
    fn apply(&self, offset: Point<Pixels>);
    fn describe(&self, offset: Point<Pixels>) -> String;
    /// Whether a pass that only lays out makes the write too, from prepaint.
    fn in_layout(&self) -> bool;
}

struct Write<D> {
    data: D,
    apply: fn(&D, Point<Pixels>),
    describe: fn(&D, Point<Pixels>) -> String,
    in_layout: bool,
}

impl<D> Replay for Write<D> {
    fn apply(&self, offset: Point<Pixels>) {
        (self.apply)(&self.data, offset);
    }

    fn describe(&self, offset: Point<Pixels>) -> String {
        (self.describe)(&self.data, offset)
    }

    fn in_layout(&self) -> bool {
        self.in_layout
    }
}

type Effect = Rc<dyn Replay>;

/// A write and how far from where it was made it is to be made again. A tape that replays a
/// tape inside it holds the inner writes with the distance the inner one was replayed at, so
/// that replaying the outer one moves them by that and by its own.
type Entry = (Effect, Point<Pixels>);

/// The registry writes of one subtree, in the order they were made.
#[derive(Default)]
pub(crate) struct Tape {
    prepaint: Vec<Entry>,
    paint: Vec<Entry>,
    lifted: Vec<Vec<Entry>>,
    /// The writes made while it records are kept and not made, see [`Tape::capturing`].
    capture_only: bool,
}

impl Tape {
    /// A tape for a subtree that is prepainted and painted only to see what it would write: the
    /// writes are on the tape and not in the registries, which hold what the frame before made of
    /// the subtree. [`Tape::same_writes`] says whether the frame would have made the same.
    pub(crate) fn capturing() -> SharedTape {
        Rc::new(RefCell::new(Tape { capture_only: true, ..Tape::default() }))
    }

    /// Whether `other` holds the writes this does, phase by phase, in the same order, at the same
    /// places.
    pub(crate) fn same_writes(&self, other: &Tape) -> bool {
        fn same(left: &[Entry], right: &[Entry]) -> bool {
            left.len() == right.len()
                && left.iter().zip(right).all(|((left, left_offset), (right, right_offset))| {
                    left.in_layout() == right.in_layout() && left.describe(*left_offset) == right.describe(*right_offset)
                })
        }
        same(&self.prepaint, &other.prepaint)
            && same(&self.paint, &other.paint)
            && self.lifted.len() == other.lifted.len()
            && self.lifted.iter().zip(&other.lifted).all(|(left, right)| same(left, right))
    }

    fn of(&mut self, phase: Phase) -> &mut Vec<Entry> {
        match phase {
            Phase::Prepaint => &mut self.prepaint,
            Phase::Paint => &mut self.paint,
            Phase::Lifted(rel) => {
                let rel = rel as usize;
                if self.lifted.len() <= rel {
                    self.lifted.resize_with(rel + 1, Vec::new);
                }
                &mut self.lifted[rel]
            }
            Phase::Layout | Phase::LayoutPaint => {
                unreachable!("nothing records into the layout phases: they replay the others")
            }
        }
    }

    /// Writes again what `phase` of the subtree wrote, `delta` from where it wrote it. A subtree
    /// inside another that is being recorded is part of its tape too.
    pub(crate) fn replay(&self, phase: Phase, delta: Point<Pixels>) {
        debug_assert!(!is_capturing(), "a subtree that is only captured holds no subtree to replay");
        let effects = match phase {
            // A pass that only lays out records no tape, so this is not part of one. It makes the
            // writes of prepaint, and those of paint and of the elements handed to the stacking
            // context that it makes from prepaint too.
            Phase::Layout => {
                for (effect, offset) in &self.prepaint {
                    effect.apply(*offset + delta);
                }
                for (effect, offset) in self.paint.iter().chain(self.lifted.iter().flatten()).filter(|(effect, _)| effect.in_layout()) {
                    effect.apply(*offset + delta);
                }
                return;
            }
            Phase::LayoutPaint => {
                for (effect, offset) in self.paint.iter().filter(|(effect, _)| effect.in_layout()) {
                    effect.apply(*offset + delta);
                }
                return;
            }
            Phase::Prepaint => &self.prepaint,
            Phase::Paint => &self.paint,
            Phase::Lifted(rel) => match self.lifted.get(rel as usize) {
                Some(effects) => effects,
                None => return,
            },
        };
        for (effect, offset) in effects {
            effect.apply(*offset + delta);
        }
        RECORDING.with(|recordings| {
            for recording in recordings.borrow().iter() {
                // What the context paints, of the subtree or of what it handed over, is replayed
                // inside whichever of the two is being recorded.
                debug_assert!(
                    match phase {
                        Phase::Prepaint => recording.phase == Phase::Prepaint,
                        _ => matches!(recording.phase, Phase::Paint | Phase::Lifted(_)),
                    },
                    "a subtree is replayed in the phase it is recorded in"
                );
                recording
                    .tape
                    .borrow_mut()
                    .of(recording.phase)
                    .extend(effects.iter().map(|(effect, offset)| (effect.clone(), *offset + delta)));
            }
        });
    }

    /// What the tape writes `delta` from where it wrote it, as text, one line per write.
    pub(crate) fn describe(&self, delta: Point<Pixels>) -> String {
        let lifted = (0..self.lifted.len() as u32).map(|rel| self.describe_lifted(rel)).collect::<String>();
        describe_effects("prepaint", &self.prepaint, delta) + &describe_effects("paint", &self.paint, delta) + &lifted
    }

    /// What the element handed to the stacking context as `rel` wrote, as text.
    fn describe_lifted(&self, rel: u32) -> String {
        self.lifted
            .get(rel as usize)
            .map_or_else(String::new, |effects| describe_effects(&format!("lifted {rel} "), effects, Point::default()))
    }
}

fn describe_effects(phase: &str, effects: &[Entry], delta: Point<Pixels>) -> String {
    effects
        .iter()
        .map(|(effect, offset)| {
            format!(
                "{phase}{} {}\n",
                if effect.in_layout() { "+layout" } else { "" },
                effect.describe(*offset + delta)
            )
        })
        .collect()
}

pub(crate) type SharedTape = Rc<RefCell<Tape>>;

/// The tape of the last build of each retained root, by element id.
#[derive(Default)]
pub(crate) struct TapeStore {
    tapes: RefCell<rustc_hash::FxHashMap<u64, SharedTape>>,
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

    /// Whether `captured`, the tape of a subtree prepainted and painted again, holds the writes
    /// that the tape of the build of `root` the frame was recorded from holds.
    pub(crate) fn same_as_recorded(&self, root: u64, captured: &SharedTape) -> bool {
        self.tapes
            .borrow()
            .get(&root)
            .is_some_and(|recorded| captured.borrow().same_writes(&recorded.borrow()))
    }

    /// Replays what the last build of `root` wrote in `phase`, `delta` from where it wrote it.
    pub(crate) fn replay(&self, root: u64, phase: Phase, delta: Point<Pixels>) {
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
        tape.borrow().replay(phase, delta);
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
        tape.borrow_mut().of(Phase::Lifted(rel)).clear();
        RECORDING.with(|recordings| recordings.borrow_mut().push(Recording { tape, phase: Phase::Lifted(rel) }));
    }

    pub(crate) fn end_lifted(&self, root: u64) {
        RECORDING.with(|recordings| {
            let recording = recordings.borrow_mut().pop().expect("a lifted element that begun is ended");
            debug_assert!(
                matches!(recording.phase, Phase::Lifted(_))
                    && self.tapes.borrow().get(&root).is_some_and(|tape| Rc::ptr_eq(tape, &recording.tape)),
                "the ends of lifted elements come in the reverse order of their beginnings"
            );
        });
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
    }
}

struct Recording {
    tape: SharedTape,
    phase: Phase,
}

thread_local! {
    /// The subtrees being prepainted or painted, innermost last. A write belongs to the tape of
    /// each: a subtree inside another is part of it.
    static RECORDING: RefCell<Vec<Recording>> = const { RefCell::new(Vec::new()) };
    /// How many of them are on a tape that [captures](Tape::capturing).
    static CAPTURING: Cell<u32> = const { Cell::new(0) };
}

/// Whether the writes being made are only noted, see [`Tape::capturing`]. For a registry that is
/// written beside the tape, outside [`write`], to leave alone what the frame before made.
pub(crate) fn is_capturing() -> bool {
    CAPTURING.get() > 0
}

/// Makes a registry write with `apply`, and notes it on the tape of the subtree being prepainted
/// or painted, if there is one, for `describe` to say in words and a frame that replays the
/// subtree to make again. A frame without retained subtrees pays one thread-local read per write.
///
/// Both functions take how far from `data` the write is made, which is zero when it is made for
/// the first time: a write that holds a position moves by it, any other ignores it.
pub(crate) fn write<D: 'static>(data: D, apply: fn(&D, Point<Pixels>), describe: fn(&D, Point<Pixels>) -> String) {
    write_with(data, apply, describe, false);
}

/// [`write`] for a registry that a pass which only lays out (a bounds read) writes too, from
/// prepaint: the painted boxes. A subtree that such a pass does not build repeats it from its tape.
pub(crate) fn write_in_layout<D: 'static>(
    data: D,
    apply: fn(&D, Point<Pixels>),
    describe: fn(&D, Point<Pixels>) -> String,
) {
    write_with(data, apply, describe, true);
}

fn write_with<D: 'static>(
    data: D,
    apply: fn(&D, Point<Pixels>),
    describe: fn(&D, Point<Pixels>) -> String,
    in_layout: bool,
) {
    if !is_capturing() {
        apply(&data, Point::default());
    }
    RECORDING.with(|recordings| {
        let recordings = recordings.borrow();
        if recordings.is_empty() {
            return;
        }
        let effect: Effect = Rc::new(Write { data, apply, describe, in_layout });
        for recording in recordings.iter() {
            recording.tape.borrow_mut().of(recording.phase).push((effect.clone(), Point::default()));
        }
    });
}

/// Puts `tape` under `phase` for as long as it lives. A guard, so that an unwinding paint does not
/// leave a stale tape for the next frame's unrelated writes.
struct Recorder {
    capturing: bool,
}

impl Recorder {
    fn begin(tape: &SharedTape, phase: Phase) -> Self {
        let capturing = tape.borrow().capture_only;
        tape.borrow_mut().of(phase).clear();
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
        RECORDING.with(|recordings| {
            recordings.borrow_mut().pop();
        });
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
        write(amount, |amount, _| REGISTRY.set(REGISTRY.get() + amount), |amount, _| format!("add {amount}"));
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
