//! The `layout` prop: when a box's layout position changes, it glides from
//! where it was shown to where layout put it, FLIP style.
//!
//! It is not a transform: everything prepainted inside
//! `Window::with_element_offset` (bounds, hitboxes, paint) is shifted by it.
//! [`FlipOffset`] wraps an element's built subtree and prepaints it inside an
//! offset that starts at `old position - new position` and eases to zero, so
//! the box is drawn, and hit-tested, where the animation has it. Layout itself
//! is never touched: only position animates, a size change snaps.
//!
//! The origin it compares is the layout origin with the ancestors' offsets
//! taken back out, so scrolling and an ancestor's own FLIP do not read as a move.
//!
//! `dragOffset` lets a pointer drag move the box through the same offset: three
//! motion values (`x`, `y`, and `active`) that JS writes. While `active`, the
//! box is shown at `x, y` from where layout puts it, and if layout moves it
//! meanwhile (a reorder swapped its slot) the box stays under the pointer
//! instead of gliding. When `active` goes off, it glides from where it was
//! shown to its slot like any other layout move; `x` and `y` are ignored from
//! then on, so JS may reset them in any order.
//!
//! `layoutId` shares a glide between elements: an element that appears with an
//! id some other element held starts from where that one was last shown, with
//! the speed it had. The holder is either gone for at most a frame, or still
//! around (the newcomer takes over; hiding the old one is up to the app).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use serde_json::Value;
use web_time::Instant;

use super::curve::{Curve, Ease, Transition};
use super::track::Track;
use super::value::AnimValue;
use super::{MotionValueStore, Transitions};

/// Draw order among deferred elements: above the default of `deferred()`.
const LIFTED_PRIORITY: usize = 1;

/// Layout moves smaller than this, in px, are rounding noise.
const MOVED: f64 = 0.01;

type Vec2 = [f64; 2];

fn sub(a: Vec2, b: Vec2) -> Vec2 {
    [a[0] - b[0], a[1] - b[1]]
}

fn add(a: Vec2, b: Vec2) -> Vec2 {
    [a[0] + b[0], a[1] + b[1]]
}

fn is_zero(a: Vec2) -> bool {
    a[0].abs() < MOVED && a[1].abs() < MOVED
}

/// What the drag values read on one frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Drag {
    active: bool,
    offset: Vec2,
}

/// The motion value ids of `dragOffset`, in `x`, `y`, `active` order.
fn drag_ids(source: &Value) -> Result<Option<[f64; 3]>, String> {
    let Some(drag) = source.get("dragOffset").filter(|drag| !drag.is_null()) else {
        return Ok(None);
    };
    let id = |name: &str| {
        drag.get(name)
            .and_then(Value::as_f64)
            .ok_or_else(|| format!("layout dragOffset.{name} must be a motion value id"))
    };
    Ok(Some([id("x")?, id("y")?, id("active")?]))
}

/// The motion values a `layout` prop wants sampled every frame.
pub(crate) fn drag_value_ids(source: &Value) -> Vec<f64> {
    drag_ids(source).ok().flatten().map_or_else(Vec::new, |ids| ids.to_vec())
}

/// A drag value that is not bound yet reads as no drag: JS binds the values
/// in an effect, a beat after the prop that names them.
fn read_drag(source: &Value, values: &MotionValueStore) -> Drag {
    let Ok(Some([x, y, active])) = drag_ids(source) else {
        return Drag::default();
    };
    let (Some(x), Some(y), Some(active)) =
        (values.sampled(x), values.sampled(y), values.sampled(active))
    else {
        return Drag::default();
    };
    Drag {
        active: active > 0.5,
        offset: [x, y],
    }
}

/// What a `layout` prop asks for.
#[derive(Clone, Debug, PartialEq)]
struct Config {
    transition: Transition,
    reduced: bool,
    /// Elements with the same id hand their glide to each other.
    layout_id: Option<String>,
}

/// Framer's default for a layout animation.
fn default_transition() -> Transition {
    Transition {
        curve: Curve::Tween {
            duration: 0.45,
            ease: Ease::CubicBezier([0.4, 0.0, 0.1, 1.0]),
        },
        ..Transition::default()
    }
}

/// `true`, `"position"` or `"size"`, or an object holding that as `mode` next
/// to the `transition` and `reducedMotion` of the motion description.
///
/// `None` when nothing animates natively: `false`, and `"size"`, as a size
/// change snaps (the glide is an offset, which scales nothing). A `layoutId` still moves
/// the box with `"size"`, as the point of sharing an id is the move.
fn parse(source: &Value) -> Result<Option<Config>, String> {
    let empty = serde_json::Map::new();
    let (mode, fields) = match source {
        Value::Object(fields) => (fields.get("mode").unwrap_or(&Value::Null), fields),
        other => (other, &empty),
    };
    let layout_id = match fields.get("layoutId") {
        None | Some(Value::Null) => None,
        Some(Value::String(id)) if !id.is_empty() => Some(id.clone()),
        Some(_) => return Err("layoutId must be a non-empty string".to_string()),
    };
    drag_ids(source)?;
    let animates_position = match mode {
        Value::Bool(on) => *on,
        Value::Null if source.is_object() => true,
        Value::String(name) => match name.as_str() {
            "position" => true,
            "size" => layout_id.is_some(),
            other => return Err(format!("layout must be true, \"position\" or \"size\", not \"{other}\"")),
        },
        _ => return Err("layout must be true, \"position\" or \"size\"".to_string()),
    };
    if !animates_position {
        return Ok(None);
    }
    let reduced = match fields.get("reducedMotion") {
        None | Some(Value::Null) => false,
        Some(value) => value
            .as_bool()
            .ok_or_else(|| "layout reducedMotion must be a boolean".to_string())?,
    };
    let transition = match fields.get("transition") {
        None | Some(Value::Null) => default_transition(),
        Some(Value::Object(transition)) => layout_transition(transition)?,
        Some(_) => return Err("layout transition must be an object".to_string()),
    };
    if transition.repeat.repeats() {
        return Err("a layout transition cannot repeat".to_string());
    }
    Ok(Some(Config {
        transition,
        reduced,
        layout_id,
    }))
}

/// `transition.layout` when there is one, else the transition the description
/// would use for anything: `default`, then the top-level curve.
fn layout_transition(source: &serde_json::Map<String, Value>) -> Result<Transition, String> {
    let mut rest = source.clone();
    match rest.remove("layout") {
        Some(own) if !own.is_null() => Transition::parse(own),
        _ if rest.is_empty() => Ok(default_transition()),
        _ => Ok(Transitions::parse(
            Some(&Value::Object(rest)),
            super::Target::Style,
            false,
        )?
        .fallback),
    }
}

/// One frame of a box's glide.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Step {
    /// How far to shift the box from where layout put it.
    offset: Vec2,
    /// Whether the glide needs another frame.
    active: bool,
    /// How far layout itself moved since the last frame. Boxes inside this
    /// one moved with it, and their own glide is what is left over.
    moved: Vec2,
    /// A glide began on this frame, including a retarget or a takeover.
    started: bool,
    /// The glide reached layout on this frame. Not set for one that was
    /// replaced or dropped.
    completed: bool,
    /// The pointer holds the box, or it is still gliding home after the
    /// release: it is drawn above everything else.
    lifted: bool,
}

/// Where a box was last shown, for another box to start from.
#[derive(Clone, Debug)]
struct Held {
    /// The un-offset layout origin plus the offset it was drawn with.
    visual: Vec2,
    /// The glide it was in, whose speed the next box continues.
    track: Option<Track>,
    /// The frame it was shown on.
    frame: u64,
}

#[derive(Debug)]
struct Flip {
    /// The prop as JS set it, to tell a change without parsing.
    source: Value,
    config: Option<Config>,
    /// Where layout put the box on the last frame it was prepainted, in px,
    /// with every ancestor's offset taken back out.
    origin: Option<Vec2>,
    track: Option<Track>,
    held: Option<Held>,
    /// The frame it was first built on.
    born: u64,
    /// A glide began since the last step, which reports it.
    started: bool,
    /// While dragged, the offset the box was last shown with.
    dragged: Option<Vec2>,
    /// While dragged, how far layout moved the box under the pointer.
    drag_shift: Vec2,
    /// Held, or gliding home after being held.
    lifted: bool,
}

impl Flip {
    fn new(source: &Value, born: u64) -> Self {
        let config = parse(source).unwrap_or_else(|error| {
            log::warn!("Invalid layout description: {error}");
            None
        });
        Self {
            source: source.clone(),
            config,
            origin: None,
            track: None,
            held: None,
            born,
            started: false,
            dragged: None,
            drag_shift: [0.0; 2],
            lifted: false,
        }
    }

    fn layout_id(&self) -> Option<&str> {
        self.config.as_ref()?.layout_id.as_deref()
    }

    /// Runs the glide from `from`, an offset from where layout puts the box,
    /// at the speed `previous` had.
    fn begin(&mut self, from: Vec2, previous: Option<&Track>, now: Instant) {
        let Some(config) = &self.config else {
            return;
        };
        let started = Track::new(
            "layout",
            &AnimValue::Numbers(from.to_vec()),
            AnimValue::Numbers(vec![0.0, 0.0]),
            config.transition.clone(),
            now,
        );
        match started {
            Ok(mut next) => {
                if let Some(previous) = previous {
                    next.carry_velocity(previous, now);
                }
                self.track = Some(next);
                self.started = true;
            }
            Err(error) => {
                log::warn!("Invalid layout transition: {error}");
                self.track = None;
            }
        }
    }

    /// Starts a box that is new to the layout where `holder` was shown.
    fn take_over(&mut self, origin: Vec2, holder: &Held, now: Instant) {
        let Some(config) = &self.config else {
            return;
        };
        let from = sub(holder.visual, origin);
        if config.reduced || is_zero(from) {
            return;
        }
        self.begin(from, holder.track.as_ref(), now);
    }

    /// `inherited` is how far the nearest enclosing flipping box's layout
    /// moved this frame: it carries this one with it and animates for both.
    fn step(&mut self, origin: Vec2, inherited: Vec2, drag: Drag, now: Instant) -> Step {
        let Some(config) = &self.config else {
            return Step {
                offset: [0.0; 2],
                active: false,
                moved: [0.0; 2],
                started: false,
                completed: false,
                lifted: false,
            };
        };
        let reduced = config.reduced;
        let moved = self.origin.map_or([0.0; 2], |last| sub(origin, last));
        self.origin = Some(origin);
        let own = sub(moved, inherited);

        if drag.active {
            return self.drag_step(moved, own, drag.offset, now);
        }
        let released = self.dragged.take();
        if released.is_some() {
            self.lifted = true;
        }
        if reduced {
            self.track = None;
        } else if let Some(shown) = released {
            // The box was shown `shown` away from its old place, and its place
            // just moved by `own`: glide from the same spot on screen.
            let from = sub(shown, own);
            self.track = None;
            if !is_zero(from) {
                self.begin(from, None, now);
            }
        } else if !is_zero(own) {
            let shown = self
                .track
                .as_ref()
                .map_or([0.0; 2], |track| to_vec2(&track.sample(now).position));
            // The box was drawn `shown` away from its old place, and its place
            // just moved by `own`: start from the same spot on screen.
            let from = sub(shown, own);
            let previous = self.track.take();
            self.begin(from, previous.as_ref(), now);
        }

        let (offset, active, completed) = match &self.track {
            Some(track) => {
                let sample = track.sample(now);
                (to_vec2(&sample.position), sample.active, !sample.active)
            }
            None => ([0.0; 2], false, false),
        };
        if !active {
            self.track = None;
            self.lifted = false;
        }
        Step {
            offset: if active { offset } else { [0.0; 2] },
            active,
            moved,
            started: std::mem::take(&mut self.started),
            completed,
            lifted: active && self.lifted,
        }
    }

    /// A frame of a box the pointer holds. Its own glide gives way to the
    /// pointer, and keeps where it was shown as the place the drag starts from.
    fn drag_step(&mut self, moved: Vec2, own: Vec2, offset: Vec2, now: Instant) -> Step {
        if self.dragged.is_none() {
            self.drag_shift = self
                .track
                .take()
                .map_or([0.0; 2], |track| to_vec2(&track.sample(now).position));
            self.started = false;
        }
        self.drag_shift = sub(self.drag_shift, own);
        let shown = add(offset, self.drag_shift);
        self.dragged = Some(shown);
        self.lifted = true;
        Step {
            offset: shown,
            active: false,
            moved,
            started: false,
            completed: false,
            lifted: true,
        }
    }
}

fn to_vec2(channels: &[f64]) -> Vec2 {
    [channels[0], channels[1]]
}

/// Frames a departed box stays available to a newcomer with its id.
const DEPARTED_FRAMES: u64 = 1;

#[derive(Debug)]
struct Departed {
    held: Held,
    /// The frame its element left the tree on.
    gone: u64,
}

/// What the glides tell JS.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum LayoutEventKind {
    Start,
    Complete,
}

impl LayoutEventKind {
    /// The event type the host maps to `onLayoutAnimation*`.
    pub(crate) fn event_type(self) -> &'static str {
        match self {
            Self::Start => "layoutAnimationStart",
            Self::Complete => "layoutAnimationComplete",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LayoutEvent {
    pub(crate) id: u64,
    pub(crate) kind: LayoutEventKind,
}

#[derive(Default)]
struct Store {
    flips: HashMap<u64, Flip>,
    /// The layout move of the flipping box being prepainted, for the boxes inside.
    enclosing: Vec2,
    /// Counts the renders that pruned `flips`.
    frame: u64,
    departed: HashMap<String, Departed>,
    events: Vec<LayoutEvent>,
}

impl Store {
    /// The last shown state of the box that held `layout_id` before the box
    /// `id`, which is new. A box built on this very frame is not a holder:
    /// two newcomers with one id are neither's predecessor.
    fn holder(&mut self, id: u64, layout_id: &str) -> Option<Held> {
        let frame = self.frame;
        let live = self
            .flips
            .iter()
            .filter(|(other, flip)| {
                **other != id && flip.born < frame && flip.layout_id() == Some(layout_id)
            })
            .filter_map(|(other, flip)| flip.held.as_ref().map(|held| (held, *other)))
            .max_by_key(|(held, other)| (held.frame, *other))
            .map(|(held, _)| held.clone());
        let departed = self
            .departed
            .remove(layout_id)
            .filter(|departed| frame - departed.gone <= DEPARTED_FRAMES)
            .map(|departed| departed.held);
        match (live, departed) {
            (Some(live), Some(departed)) if live.frame > departed.frame => Some(live),
            (live, departed) => departed.or(live),
        }
    }
}

/// The glide of every element with a `layout` prop, shared with the wrappers
/// the build hands to GPUI.
#[derive(Clone, Default)]
pub(crate) struct LayoutFlips(Rc<RefCell<Store>>);

impl LayoutFlips {
    /// Forgets the boxes `keep` refuses. Called once per render, which is what
    /// a frame is here: a box that left keeps its last place for the next
    /// frame, for a newcomer with its `layoutId` to start from.
    pub(crate) fn retain(&self, mut keep: impl FnMut(u64) -> bool) {
        let mut store = self.0.borrow_mut();
        store.frame += 1;
        let frame = store.frame;
        let gone: Vec<u64> = store
            .flips
            .keys()
            .copied()
            .filter(|id| !keep(*id))
            .collect();
        for id in gone {
            let Some(flip) = store.flips.remove(&id) else {
                continue;
            };
            let (Some(layout_id), Some(held)) = (flip.layout_id(), flip.held.clone()) else {
                continue;
            };
            let newer = store
                .departed
                .get(layout_id)
                .is_none_or(|other| other.held.frame <= held.frame);
            if newer {
                store
                    .departed
                    .insert(layout_id.to_string(), Departed { held, gone: frame });
            }
        }
        store
            .departed
            .retain(|_, departed| frame - departed.gone <= DEPARTED_FRAMES);
    }

    /// The events the glides raised since the last call.
    pub(crate) fn take_events(&self) -> Vec<LayoutEvent> {
        std::mem::take(&mut self.0.borrow_mut().events)
    }

    /// `child` wrapped to glide, or `child` itself when `source` asks for
    /// nothing that can animate.
    pub(crate) fn wrap(
        &self,
        id: u64,
        source: &Value,
        child: gpui::AnyElement,
        now: Instant,
        values: &MotionValueStore,
    ) -> gpui::AnyElement {
        use gpui::IntoElement;

        let mut store = self.0.borrow_mut();
        let frame = store.frame;
        let flip = store
            .flips
            .entry(id)
            .or_insert_with(|| Flip::new(source, frame));
        if flip.source != *source {
            let mut changed = Flip::new(source, flip.born);
            changed.origin = flip.origin;
            changed.track = flip.track.take();
            changed.held = flip.held.take();
            changed.dragged = flip.dragged;
            changed.drag_shift = flip.drag_shift;
            changed.lifted = flip.lifted;
            *flip = changed;
        }
        if flip.config.is_none() {
            return child;
        }
        drop(store);
        FlipOffset {
            id,
            flips: self.clone(),
            now,
            drag: read_drag(source, values),
            child,
            deferred: false,
        }
        .into_any_element()
    }

    fn step(&self, id: u64, origin: Vec2, drag: Drag, now: Instant) -> Option<Step> {
        let mut store = self.0.borrow_mut();
        let inherited = store.enclosing;
        let frame = store.frame;
        let flip = store.flips.get(&id)?;
        let handoff = match flip.layout_id() {
            Some(layout_id) if flip.origin.is_none() && flip.held.is_none() => {
                Some(layout_id.to_string())
            }
            _ => None,
        };
        let holder = handoff.and_then(|layout_id| store.holder(id, &layout_id));
        let flip = store.flips.get_mut(&id)?;
        if let Some(holder) = holder {
            flip.take_over(origin, &holder, now);
        }
        let step = flip.step(origin, inherited, drag, now);
        flip.held = Some(Held {
            visual: [origin[0] + step.offset[0], origin[1] + step.offset[1]],
            track: flip.track.clone(),
            frame,
        });
        if step.started {
            store.events.push(LayoutEvent {
                id,
                kind: LayoutEventKind::Start,
            });
        }
        if step.completed {
            store.events.push(LayoutEvent {
                id,
                kind: LayoutEventKind::Complete,
            });
        }
        Some(step)
    }
}

/// Prepaints its child shifted by the glide.
///
/// It answers layout with the child's own node, so wrapping changes nothing the style
/// asked for.
struct FlipOffset {
    id: u64,
    flips: LayoutFlips,
    now: Instant,
    drag: Drag,
    child: gpui::AnyElement,
    /// The child was handed to `defer_draw`, which paints it.
    deferred: bool,
}

impl gpui::IntoElement for FlipOffset {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl gpui::Element for FlipOffset {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) -> (gpui::LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    // A positioned `layout` box is lifted into its stacking context as the wrapper, glide and all.
    fn lift_z(&self) -> Option<i32> {
        self.child.lift_z()
    }

    fn prepaint(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: gpui::Bounds<gpui::Pixels>,
        _: &mut (),
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        // `layout_bounds` added the snapped offset, so that is what comes off.
        let inherited_offset = window.pixel_snap_point(window.element_offset());
        let origin = bounds.origin - inherited_offset;
        let origin = [f64::from(f32::from(origin.x)), f64::from(f32::from(origin.y))];

        let Some(step) = self.flips.step(self.id, origin, self.drag, self.now) else {
            self.child.prepaint(window, cx);
            return;
        };
        let offset = gpui::point(gpui::px(step.offset[0] as f32), gpui::px(step.offset[1] as f32));
        if step.lifted {
            use gpui::IntoElement;

            // GPUI has no z-index. `defer_draw` is what `deferred()` does: the
            // child is prepainted and painted after the rest of the window,
            // at the offset it has here, so it covers its siblings. Its
            // content mask is the one it has here, so a clipping ancestor
            // still clips it, and the transforms around it still turn it.
            // Boxes inside it read no enclosing move: the only ones are
            // another `layout` box held at the same time.
            self.deferred = true;
            let child = std::mem::replace(&mut self.child, gpui::Empty.into_any_element());
            window.with_element_offset(offset, |window| {
                let at = window.element_offset();
                let mask = window.content_mask();
                window.defer_draw(child, at, LIFTED_PRIORITY, Some(mask));
            });
        } else {
            let outer = std::mem::replace(&mut self.flips.0.borrow_mut().enclosing, step.moved);
            window.with_element_offset(offset, |window| self.child.prepaint(window, cx));
            self.flips.0.borrow_mut().enclosing = outer;
        }
        // The events leave with the next render, which a settled box would not ask for.
        if step.active || step.started || step.completed {
            window.request_animation_frame();
        }
    }

    fn paint(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        _: gpui::Bounds<gpui::Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        if !self.deferred {
            self.child.paint(window, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;

    use super::*;

    fn at(start: Instant, millis: u64) -> Instant {
        start + Duration::from_millis(millis)
    }

    fn flip(source: Value) -> Flip {
        Flip::new(&source, 0)
    }

    fn linear() -> Value {
        json!({ "mode": true, "transition": { "duration": 1.0, "ease": "linear" } })
    }

    #[test]
    fn the_first_frame_has_nowhere_to_glide_from() {
        let start = Instant::now();
        let mut flip = flip(linear());
        let step = flip.step([10.0, 20.0], [0.0; 2], Drag::default(), start);
        assert_eq!(step.offset, [0.0, 0.0]);
        assert!(!step.active);
    }

    #[test]
    fn a_move_starts_where_the_box_was_shown_and_ends_on_layout() {
        let start = Instant::now();
        let mut flip = flip(linear());
        flip.step([0.0, 0.0], [0.0; 2], Drag::default(), start);

        // Layout puts the box 100 px right and 40 px down.
        let first = flip.step([100.0, 40.0], [0.0; 2], Drag::default(), at(start, 10));
        assert_eq!(first.offset, [-100.0, -40.0]);
        assert!(first.active);
        assert_eq!(first.moved, [100.0, 40.0]);

        let half = flip.step([100.0, 40.0], [0.0; 2], Drag::default(), at(start, 510));
        assert!((half.offset[0] + 50.0).abs() < 1e-6, "{:?}", half.offset);
        assert!((half.offset[1] + 20.0).abs() < 1e-6);
        assert_eq!(half.moved, [0.0, 0.0]);

        let done = flip.step([100.0, 40.0], [0.0; 2], Drag::default(), at(start, 1100));
        assert_eq!(done.offset, [0.0, 0.0]);
        assert!(!done.active);
        assert!(flip.track.is_none());
    }

    #[test]
    fn a_move_in_flight_continues_from_where_the_box_is_shown() {
        let start = Instant::now();
        let mut flip = flip(linear());
        flip.step([0.0, 0.0], [0.0; 2], Drag::default(), start);
        flip.step([100.0, 0.0], [0.0; 2], Drag::default(), start);
        // Half way: shown 50 px left of layout. Layout moves another 20 px.
        let retarget = flip.step([120.0, 0.0], [0.0; 2], Drag::default(), at(start, 500));
        // Shown spot stays at layout(100) - 50 = 50, new layout 120.
        assert!((retarget.offset[0] + 70.0).abs() < 1e-6, "{:?}", retarget.offset);
        assert!(retarget.active);
    }

    #[test]
    fn a_spring_keeps_the_speed_it_was_moving_at() {
        let start = Instant::now();
        let spring = json!({
            "mode": true,
            "transition": { "type": "spring", "stiffness": 200.0, "damping": 20.0 }
        });
        let mut flip = flip(spring);
        flip.step([0.0, 0.0], [0.0; 2], Drag::default(), start);
        flip.step([100.0, 0.0], [0.0; 2], Drag::default(), start);
        let moving = flip.step([100.0, 0.0], [0.0; 2], Drag::default(), at(start, 100));
        assert!(moving.active);
        let speed = flip.track.as_ref().unwrap().sample(at(start, 100)).velocity[0];
        assert!(speed > 0.0, "the offset is closing on zero from below");

        flip.step([100.0, 50.0], [0.0; 2], Drag::default(), at(start, 100));
        let carried = flip.track.as_ref().unwrap().sample(at(start, 100)).velocity[0];
        assert!((carried - speed).abs() < 1e-6, "{carried} vs {speed}");
    }

    #[test]
    fn a_box_carried_by_an_enclosing_one_glides_only_by_the_rest() {
        let start = Instant::now();
        let mut flip = flip(linear());
        flip.step([0.0, 0.0], [0.0; 2], Drag::default(), start);
        // The parent's layout moved 100 and so did this box: nothing of its own.
        let carried = flip.step([100.0, 0.0], [100.0, 0.0], Drag::default(), at(start, 10));
        assert_eq!(carried.offset, [0.0, 0.0]);
        assert!(!carried.active);
        // 30 px further than the parent.
        let extra = flip.step([230.0, 0.0], [100.0, 0.0], Drag::default(), at(start, 20));
        assert_eq!(extra.offset, [-30.0, 0.0]);
    }

    #[test]
    fn reduced_motion_jumps() {
        let start = Instant::now();
        let mut flip = flip(json!({ "mode": true, "reducedMotion": true }));
        flip.step([0.0, 0.0], [0.0; 2], Drag::default(), start);
        let moved = flip.step([100.0, 0.0], [0.0; 2], Drag::default(), at(start, 10));
        assert_eq!(moved.offset, [0.0, 0.0]);
        assert!(!moved.active);
        assert_eq!(moved.moved, [100.0, 0.0], "boxes inside still read the move");
    }

    #[test]
    fn turning_on_reduced_motion_mid_flight_drops_the_glide() {
        let start = Instant::now();
        let mut flip = flip(linear());
        flip.step([0.0, 0.0], [0.0; 2], Drag::default(), start);
        flip.step([100.0, 0.0], [0.0; 2], Drag::default(), start);
        assert!(flip.track.is_some());
        let mut reduced = Flip::new(&json!({ "mode": true, "reducedMotion": true }), 0);
        reduced.origin = flip.origin;
        reduced.track = flip.track.take();
        let step = reduced.step([100.0, 0.0], [0.0; 2], Drag::default(), at(start, 100));
        assert_eq!(step.offset, [0.0, 0.0]);
        assert!(!step.active);
    }

    #[test]
    fn rounding_noise_is_not_a_move() {
        let start = Instant::now();
        let mut flip = flip(linear());
        flip.step([10.0, 10.0], [0.0; 2], Drag::default(), start);
        let step = flip.step([10.004, 9.996], [0.0; 2], Drag::default(), at(start, 16));
        assert!(!step.active);
    }

    #[test]
    fn a_delay_holds_the_box_at_its_old_place() {
        let start = Instant::now();
        let mut flip = flip(json!({
            "mode": true,
            "transition": { "duration": 1.0, "ease": "linear", "delay": 0.5 }
        }));
        flip.step([0.0, 0.0], [0.0; 2], Drag::default(), start);
        flip.step([100.0, 0.0], [0.0; 2], Drag::default(), start);
        let held = flip.step([100.0, 0.0], [0.0; 2], Drag::default(), at(start, 400));
        assert_eq!(held.offset, [-100.0, 0.0]);
        assert!(held.active);
    }

    #[test]
    fn transition_layout_wins_over_the_transition_for_everything_else() {
        let source = json!({
            "mode": true,
            "transition": {
                "duration": 9.0,
                "layout": { "duration": 0.2, "ease": "linear" }
            }
        });
        let config = parse(&source).unwrap().unwrap();
        match config.transition.curve {
            Curve::Tween { duration, .. } => assert_eq!(duration, 0.2),
            Curve::Spring(_) => panic!("a tween was asked for"),
        }

        let plain = json!({ "mode": true, "transition": { "duration": 0.7, "opacity": { "duration": 3.0 } } });
        match parse(&plain).unwrap().unwrap().transition.curve {
            Curve::Tween { duration, .. } => assert_eq!(duration, 0.7),
            Curve::Spring(_) => panic!("a tween was asked for"),
        }
    }

    #[test]
    fn without_a_transition_it_uses_the_layout_default() {
        let config = parse(&json!(true)).unwrap().unwrap();
        assert_eq!(config.transition, default_transition());
        assert!(!config.reduced);
    }

    #[test]
    fn the_shorthands_and_their_mistakes() {
        assert!(parse(&json!("position")).unwrap().is_some());
        assert!(parse(&json!("size")).unwrap().is_none(), "a size change snaps");
        assert!(parse(&json!(false)).unwrap().is_none());
        assert!(parse(&json!({ "mode": "size" })).unwrap().is_none());
        assert!(parse(&json!({})).unwrap().is_some());
        assert!(parse(&json!("both")).is_err());
        assert!(parse(&json!(3)).is_err());
        assert!(parse(&json!({ "reducedMotion": "user" })).is_err());
        assert!(parse(&json!({ "transition": 5 })).is_err());
        assert!(parse(&json!({ "transition": { "repeat": 2 } })).is_err());
    }

    #[test]
    fn an_invalid_prop_never_animates() {
        let start = Instant::now();
        let mut flip = flip(json!("nonsense"));
        flip.step([0.0, 0.0], [0.0; 2], Drag::default(), start);
        let step = flip.step([50.0, 0.0], [0.0; 2], Drag::default(), at(start, 10));
        assert!(!step.active);
    }

    fn shared(id: &str) -> Value {
        json!({ "mode": true, "layoutId": id, "transition": { "duration": 1.0, "ease": "linear" } })
    }

    /// What the build does for an element: a flip, born on the current frame.
    fn mount(flips: &LayoutFlips, id: u64, source: &Value) {
        let mut store = flips.0.borrow_mut();
        let frame = store.frame;
        store.flips.insert(id, Flip::new(source, frame));
    }

    /// What a render does before the build: forget what left.
    fn render(flips: &LayoutFlips, alive: &[u64]) {
        flips.retain(|id| alive.contains(&id));
    }

    #[test]
    fn a_newcomer_starts_where_the_box_that_just_left_was_shown() {
        let start = Instant::now();
        let flips = LayoutFlips::default();
        render(&flips, &[1]);
        mount(&flips, 1, &shared("tab"));
        flips.step(1, [0.0, 0.0], Drag::default(), start).unwrap();

        // Unmounted and replaced on the same frame, 100 px to the right.
        render(&flips, &[2]);
        mount(&flips, 2, &shared("tab"));
        let step = flips.step(2, [100.0, 0.0], Drag::default(), at(start, 16)).unwrap();
        assert_eq!(step.offset, [-100.0, 0.0]);
        assert!(step.started && step.active);
        assert_eq!(flips.take_events(), vec![LayoutEvent { id: 2, kind: LayoutEventKind::Start }]);

        let half = flips.step(2, [100.0, 0.0], Drag::default(), at(start, 516)).unwrap();
        assert!((half.offset[0] + 50.0).abs() < 1e-6, "{:?}", half.offset);
    }

    #[test]
    fn the_holder_is_remembered_for_one_frame_after_it_left() {
        let start = Instant::now();
        let flips = LayoutFlips::default();
        render(&flips, &[1]);
        mount(&flips, 1, &shared("tab"));
        flips.step(1, [0.0, 0.0], Drag::default(), start).unwrap();

        render(&flips, &[]);
        render(&flips, &[2]);
        mount(&flips, 2, &shared("tab"));
        let next_frame = flips.step(2, [100.0, 0.0], Drag::default(), at(start, 16)).unwrap();
        assert!(next_frame.active, "one frame later it is still there");

        let flips = LayoutFlips::default();
        render(&flips, &[1]);
        mount(&flips, 1, &shared("tab"));
        flips.step(1, [0.0, 0.0], Drag::default(), start).unwrap();
        render(&flips, &[]);
        render(&flips, &[]);
        render(&flips, &[2]);
        mount(&flips, 2, &shared("tab"));
        let late = flips.step(2, [100.0, 0.0], Drag::default(), at(start, 16)).unwrap();
        assert!(!late.active, "two frames later it is forgotten");
    }

    #[test]
    fn a_holder_that_is_still_mounted_hands_over_its_place() {
        let start = Instant::now();
        let flips = LayoutFlips::default();
        render(&flips, &[1]);
        mount(&flips, 1, &shared("tab"));
        flips.step(1, [20.0, 0.0], Drag::default(), start).unwrap();

        render(&flips, &[1, 2]);
        mount(&flips, 2, &shared("tab"));
        let step = flips.step(2, [120.0, 0.0], Drag::default(), at(start, 16)).unwrap();
        assert_eq!(step.offset, [-100.0, 0.0]);
        // The old one is untouched: hiding it is up to the app.
        let old = flips.step(1, [20.0, 0.0], Drag::default(), at(start, 16)).unwrap();
        assert!(!old.active);
    }

    #[test]
    fn a_live_holder_is_where_it_is_drawn_not_where_layout_put_it() {
        let start = Instant::now();
        let flips = LayoutFlips::default();
        render(&flips, &[1]);
        mount(&flips, 1, &shared("tab"));
        flips.step(1, [0.0, 0.0], Drag::default(), start).unwrap();
        flips.step(1, [100.0, 0.0], Drag::default(), at(start, 16)).unwrap();
        // Half way: drawn at 50.
        flips.step(1, [100.0, 0.0], Drag::default(), at(start, 516)).unwrap();

        render(&flips, &[1, 2]);
        mount(&flips, 2, &shared("tab"));
        let step = flips.step(2, [300.0, 0.0], Drag::default(), at(start, 516)).unwrap();
        assert!((step.offset[0] + 250.0).abs() < 1e-3, "{:?}", step.offset);
    }

    #[test]
    fn two_newcomers_with_one_id_do_not_start_from_each_other() {
        let start = Instant::now();
        let flips = LayoutFlips::default();
        render(&flips, &[1, 2]);
        mount(&flips, 1, &shared("tab"));
        mount(&flips, 2, &shared("tab"));
        flips.step(1, [0.0, 0.0], Drag::default(), start).unwrap();
        let second = flips.step(2, [100.0, 0.0], Drag::default(), start).unwrap();
        assert!(!second.active);
    }

    #[test]
    fn other_ids_and_boxes_without_an_id_take_nothing() {
        let start = Instant::now();
        let flips = LayoutFlips::default();
        render(&flips, &[1]);
        mount(&flips, 1, &shared("a"));
        flips.step(1, [0.0, 0.0], Drag::default(), start).unwrap();
        render(&flips, &[2, 3]);
        mount(&flips, 2, &shared("b"));
        mount(&flips, 3, &linear());
        assert!(!flips.step(2, [100.0, 0.0], Drag::default(), start).unwrap().active);
        assert!(!flips.step(3, [100.0, 0.0], Drag::default(), start).unwrap().active);
    }

    #[test]
    fn a_spring_continues_at_the_speed_of_the_box_that_left() {
        let start = Instant::now();
        let spring = json!({
            "mode": true,
            "layoutId": "tab",
            "transition": { "type": "spring", "stiffness": 200.0, "damping": 20.0 }
        });
        let flips = LayoutFlips::default();
        render(&flips, &[1]);
        mount(&flips, 1, &spring);
        flips.step(1, [0.0, 0.0], Drag::default(), start).unwrap();
        flips.step(1, [100.0, 0.0], Drag::default(), at(start, 16)).unwrap();
        flips.step(1, [100.0, 0.0], Drag::default(), at(start, 116)).unwrap();
        let speed = {
            let store = flips.0.borrow();
            let track = store.flips[&1].track.as_ref().unwrap();
            track.sample(at(start, 116)).velocity[0]
        };
        assert!(speed > 0.0);

        render(&flips, &[2]);
        mount(&flips, 2, &spring);
        flips.step(2, [100.0, 0.0], Drag::default(), at(start, 116)).unwrap();
        let carried = {
            let store = flips.0.borrow();
            let track = store.flips[&2].track.as_ref().unwrap();
            track.sample(at(start, 116)).velocity[0]
        };
        assert!((carried - speed).abs() < 1e-6, "{carried} vs {speed}");
    }

    #[test]
    fn a_box_that_arrives_where_the_holder_was_does_not_glide() {
        let start = Instant::now();
        let flips = LayoutFlips::default();
        render(&flips, &[1]);
        mount(&flips, 1, &shared("tab"));
        flips.step(1, [30.0, 30.0], Drag::default(), start).unwrap();
        render(&flips, &[2]);
        mount(&flips, 2, &shared("tab"));
        let step = flips.step(2, [30.0, 30.0], Drag::default(), start).unwrap();
        assert!(!step.active && !step.started);
    }

    #[test]
    fn reduced_motion_takes_nothing_over() {
        let start = Instant::now();
        let reduced = json!({ "mode": true, "layoutId": "tab", "reducedMotion": true });
        let flips = LayoutFlips::default();
        render(&flips, &[1]);
        mount(&flips, 1, &reduced);
        flips.step(1, [0.0, 0.0], Drag::default(), start).unwrap();
        render(&flips, &[2]);
        mount(&flips, 2, &reduced);
        let step = flips.step(2, [100.0, 0.0], Drag::default(), start).unwrap();
        assert!(!step.active);
        assert!(flips.take_events().is_empty());
    }

    #[test]
    fn a_glide_reports_its_start_and_its_end_once() {
        let start = Instant::now();
        let mut flip = flip(linear());
        assert!(!flip.step([0.0, 0.0], [0.0; 2], Drag::default(), start).started);
        let first = flip.step([100.0, 0.0], [0.0; 2], Drag::default(), at(start, 10));
        assert!(first.started && !first.completed);
        let middle = flip.step([100.0, 0.0], [0.0; 2], Drag::default(), at(start, 500));
        assert!(!middle.started && !middle.completed);
        let done = flip.step([100.0, 0.0], [0.0; 2], Drag::default(), at(start, 1100));
        assert!(done.completed && !done.started);
        let after = flip.step([100.0, 0.0], [0.0; 2], Drag::default(), at(start, 1200));
        assert!(!after.completed);
    }

    #[test]
    fn a_retarget_starts_again_without_completing_the_one_it_replaced() {
        let start = Instant::now();
        let mut flip = flip(linear());
        flip.step([0.0, 0.0], [0.0; 2], Drag::default(), start);
        flip.step([100.0, 0.0], [0.0; 2], Drag::default(), at(start, 10));
        let retarget = flip.step([200.0, 0.0], [0.0; 2], Drag::default(), at(start, 500));
        assert!(retarget.started && !retarget.completed);
    }

    #[test]
    fn reduced_motion_raises_no_events() {
        let start = Instant::now();
        let mut flip = flip(json!({ "mode": true, "reducedMotion": true }));
        flip.step([0.0, 0.0], [0.0; 2], Drag::default(), start);
        let moved = flip.step([100.0, 0.0], [0.0; 2], Drag::default(), at(start, 10));
        assert!(!moved.started && !moved.completed);
    }

    #[test]
    fn layout_id_is_an_opaque_non_empty_string() {
        let config = parse(&json!({ "layoutId": "group:tab" })).unwrap().unwrap();
        assert_eq!(config.layout_id.as_deref(), Some("group:tab"));
        assert!(parse(&json!({ "layoutId": "" })).is_err());
        assert!(parse(&json!({ "layoutId": 3 })).is_err());
        assert_eq!(parse(&json!({ "layoutId": null })).unwrap().unwrap().layout_id, None);
        assert!(parse(&json!({ "mode": "size", "layoutId": "a" })).unwrap().is_some());
        assert!(parse(&json!({ "mode": false, "layoutId": "a" })).unwrap().is_none());
    }

    fn held(offset: Vec2) -> Drag {
        Drag {
            active: true,
            offset,
        }
    }

    #[test]
    fn a_held_box_is_shown_where_the_pointer_took_it() {
        let start = Instant::now();
        let mut flip = flip(linear());
        flip.step([0.0, 0.0], [0.0; 2], Drag::default(), start);
        let step = flip.step([0.0, 0.0], [0.0; 2], held([0.0, 30.0]), at(start, 10));
        assert_eq!(step.offset, [0.0, 30.0]);
        assert!(!step.active, "the pointer moves it, not a clock");
        assert!(step.lifted);
        let later = flip.step([0.0, 0.0], [0.0; 2], held([5.0, 42.0]), at(start, 20));
        assert_eq!(later.offset, [5.0, 42.0]);
    }

    #[test]
    fn a_held_box_stays_under_the_pointer_when_layout_moves_its_slot() {
        let start = Instant::now();
        let mut flip = flip(linear());
        flip.step([0.0, 0.0], [0.0; 2], Drag::default(), start);
        flip.step([0.0, 0.0], [0.0; 2], held([0.0, 30.0]), at(start, 10));
        // A swap moved its slot 20 px down; the pointer has not moved.
        let swapped = flip.step([0.0, 20.0], [0.0; 2], held([0.0, 30.0]), at(start, 20));
        assert_eq!(swapped.offset, [0.0, 10.0]);
        assert!(!swapped.active && !swapped.started, "no glide for a held box");
        assert_eq!(swapped.moved, [0.0, 20.0], "boxes inside still read the move");
        // The pointer then moves 5 px further.
        let after = flip.step([0.0, 20.0], [0.0; 2], held([0.0, 35.0]), at(start, 30));
        assert_eq!(after.offset, [0.0, 15.0]);
    }

    #[test]
    fn a_released_box_glides_from_where_it_was_shown_to_its_slot() {
        let start = Instant::now();
        let mut flip = flip(linear());
        flip.step([0.0, 0.0], [0.0; 2], Drag::default(), start);
        flip.step([0.0, 20.0], [0.0; 2], held([0.0, 30.0]), at(start, 10));
        // Released: the drag values are read as whatever they are now.
        let release = Drag {
            active: false,
            offset: [0.0, 0.0],
        };
        // Layout put the slot 20 px down while it was held, so it is 10 px from it.
        let first = flip.step([0.0, 20.0], [0.0; 2], release, at(start, 20));
        assert_eq!(first.offset, [0.0, 10.0]);
        assert!(first.active && first.started);
        assert!(first.lifted, "it stays above its neighbours on the way home");
        let half = flip.step([0.0, 20.0], [0.0; 2], release, at(start, 520));
        assert!((half.offset[1] - 5.0).abs() < 1e-6, "{:?}", half.offset);
        let done = flip.step([0.0, 20.0], [0.0; 2], release, at(start, 1100));
        assert_eq!(done.offset, [0.0, 0.0]);
        assert!(done.completed && !done.lifted);
    }

    #[test]
    fn releasing_where_the_box_belongs_has_nothing_to_glide() {
        let start = Instant::now();
        let mut flip = flip(linear());
        flip.step([0.0, 0.0], [0.0; 2], Drag::default(), start);
        flip.step([0.0, 0.0], [0.0; 2], held([0.0, 0.0]), at(start, 10));
        let step = flip.step([0.0, 0.0], [0.0; 2], Drag::default(), at(start, 20));
        assert!(!step.active && !step.started && !step.completed);
    }

    #[test]
    fn a_box_picked_up_mid_glide_starts_from_where_it_is_shown() {
        let start = Instant::now();
        let mut flip = flip(linear());
        flip.step([0.0, 0.0], [0.0; 2], Drag::default(), start);
        flip.step([0.0, 100.0], [0.0; 2], Drag::default(), at(start, 10));
        // Half way: shown 50 px above its slot.
        let held = flip.step([0.0, 100.0], [0.0; 2], held([0.0, 8.0]), at(start, 510));
        assert!((held.offset[1] + 42.0).abs() < 1e-6, "{:?}", held.offset);
    }

    #[test]
    fn a_release_under_reduced_motion_jumps() {
        let start = Instant::now();
        let mut flip = flip(json!({ "mode": true, "reducedMotion": true }));
        flip.step([0.0, 0.0], [0.0; 2], Drag::default(), start);
        flip.step([0.0, 0.0], [0.0; 2], held([0.0, 30.0]), at(start, 10));
        let step = flip.step([0.0, 0.0], [0.0; 2], Drag::default(), at(start, 20));
        assert_eq!(step.offset, [0.0, 0.0]);
        assert!(!step.active);
    }

    #[test]
    fn drag_offset_names_three_motion_values() {
        let source = json!({ "mode": true, "dragOffset": { "x": 4, "y": 5, "active": 6 } });
        assert!(parse(&source).unwrap().is_some());
        assert_eq!(drag_value_ids(&source), vec![4.0, 5.0, 6.0]);
        assert!(drag_value_ids(&json!(true)).is_empty());
        assert!(parse(&json!({ "dragOffset": { "x": 4, "y": 5 } })).is_err());
        assert!(parse(&json!({ "dragOffset": null })).unwrap().is_some());
    }

    #[test]
    fn a_drag_value_that_is_not_bound_reads_as_no_drag() {
        let source = json!({ "mode": true, "dragOffset": { "x": 4, "y": 5, "active": 6 } });
        let values = MotionValueStore::default();
        assert_eq!(read_drag(&source, &values), Drag::default());
    }
}
