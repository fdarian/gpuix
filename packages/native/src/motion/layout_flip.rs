//! The `layout` prop: when a box's layout position or size changes at a commit,
//! it glides from where it was shown to where layout put it, FLIP style, as
//! Motion's projection does.
//!
//! The glide is a transform ([`layout_projection`]): [`FlipOffset`] wraps an
//! element's built subtree and prepaints and paints it in a space that moves
//! the centre of the box and scales it about it, starting at the box that was
//! shown and easing to nothing, so the box is drawn, hit-tested and measured
//! where the animation has it. Layout itself is never touched. The element's
//! own corner radii and shadows are corrected by the scale
//! (`Window::with_projection_scale`), a `layout` box inside it is shown at its
//! own layout box and not stretched, and what else is inside is stretched
//! along, as in Motion.
//!
//! A move glides only when the tree changed since the box was last stepped,
//! which is when Motion measures (a snapshot before a render and the layout
//! after it, for the boxes of that render). A layout that moves because an
//! animation above the box ticked, or because scroll anchoring adjusted,
//! carries the box with it and starts no glide.
//!
//! The layout box it compares is in the layout's own frame, with the offsets
//! of scrolling taken back out, so scrolling does not read as a move.
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
use super::layout_projection::{self, sub, Axes, Projection, Rect, Vec2};
use super::track::Track;
use super::value::AnimValue;
use super::{MotionValueStore, Transitions};

/// Draw order among deferred elements: above the default of `deferred()`.
const LIFTED_PRIORITY: usize = 1;

fn add(a: Vec2, b: Vec2) -> Vec2 {
    [a[0] + b[0], a[1] + b[1]]
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
    axes: Axes,
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
/// `None` when nothing animates: `false`.
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
    let axes = match mode {
        Value::Bool(true) => Axes::Both,
        Value::Bool(false) => return Ok(None),
        Value::Null if source.is_object() => Axes::Both,
        Value::String(name) => match name.as_str() {
            "position" => Axes::Position,
            "size" => Axes::Size,
            other => return Err(format!("layout must be true, \"position\" or \"size\", not \"{other}\"")),
        },
        _ => return Err("layout must be true, \"position\" or \"size\"".to_string()),
    };
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
        axes,
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
    /// How the box is shown against where layout put it.
    projection: Projection,
    /// Whether the glide needs another frame.
    active: bool,
    /// A glide began on this frame, including a retarget or a takeover.
    started: bool,
    /// The glide reached layout on this frame. Not set for one that was
    /// replaced or dropped.
    completed: bool,
    /// The pointer holds the box, or it is still gliding home after the
    /// release: it is drawn above everything else.
    lifted: bool,
}

impl Step {
    const IDLE: Self = Self {
        projection: Projection::NONE,
        active: false,
        started: false,
        completed: false,
        lifted: false,
    };
}

/// Where a box was last shown, for another box to start from.
#[derive(Clone, Debug)]
struct Held {
    /// The box as it was drawn, in the frame of layout.
    visual: Rect,
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
    /// with the offsets of scrolling taken back out.
    layout: Option<Rect>,
    /// The box as it was drawn on that frame.
    shown: Option<Rect>,
    track: Option<Track>,
    held: Option<Held>,
    /// The frame it was first built on.
    born: u64,
    /// The tree revision it was last stepped at.
    seen: u64,
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
            layout: None,
            shown: None,
            track: None,
            held: None,
            born,
            seen: 0,
            started: false,
            dragged: None,
            drag_shift: [0.0; 2],
            lifted: false,
        }
    }

    fn layout_id(&self) -> Option<&str> {
        self.config.as_ref()?.layout_id.as_deref()
    }

    /// Runs the glide from `from`, at the speed `previous` had.
    fn begin(&mut self, from: Projection, previous: Option<&Track>, now: Instant) {
        let Some(config) = &self.config else {
            return;
        };
        let started = Track::new(
            "layout",
            &AnimValue::Numbers(from.to_channels().to_vec()),
            AnimValue::Numbers(Projection::NONE.to_channels().to_vec()),
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

    /// Runs the glide that takes the box `shown` to the box layout put, as far as the
    /// prop animates it.
    fn glide_from(&mut self, shown: Rect, layout: Rect, previous: Option<&Track>, now: Instant) {
        let Some(config) = &self.config else {
            return;
        };
        let from = Projection::between(shown, layout, config.axes);
        if from.is_none() {
            self.track = None;
            return;
        }
        self.begin(from, previous, now);
    }

    /// Starts a box that is new to the layout where `holder` was shown.
    fn take_over(&mut self, layout: Rect, holder: &Held, now: Instant) {
        let Some(config) = &self.config else {
            return;
        };
        if config.reduced {
            return;
        }
        self.glide_from(holder.visual, layout, holder.track.as_ref(), now);
    }

    /// `committed` is whether the tree changed since this box was last stepped.
    /// Only a move measured at a commit glides, as Motion measures a snapshot
    /// and the new layout around one render: a box that layout moved because
    /// an animation above it ticked, or because scroll anchoring adjusted,
    /// goes with it and starts nothing.
    fn step(&mut self, layout: Rect, drag: Drag, committed: bool, now: Instant) -> Step {
        let Some(config) = &self.config else {
            return Step::IDLE;
        };
        let (reduced, axes) = (config.reduced, config.axes);
        let previous = self.layout.replace(layout);
        // Where the box is drawn at this instant, which is what Motion's snapshot measures.
        let shown_before = match (previous, &self.track) {
            (Some(previous), Some(track)) => {
                Some(Projection::of_channels(&track.sample(now).position).shown(previous))
            }
            _ => self.shown,
        };

        if drag.active {
            let moved = previous.map_or([0.0; 2], |previous| sub(layout.min, previous.min));
            return self.drag_step(layout, moved, drag.offset, now);
        }
        let released = self.dragged.take();
        if released.is_some() {
            self.lifted = true;
        }
        if reduced {
            self.track = None;
        } else if released.is_some() {
            // The box was shown where the pointer left it: glide from the same spot on screen.
            self.track = None;
            if let Some(shown) = shown_before {
                self.glide_from(shown, layout, None, now);
            }
        } else if committed && previous.is_some_and(|previous| axes.changed(previous, layout)) {
            if let Some(shown) = shown_before {
                let previous = self.track.take();
                self.glide_from(shown, layout, previous.as_ref(), now);
            }
        }

        let (projection, active, completed) = match &self.track {
            Some(track) => {
                let sample = track.sample(now);
                (Projection::of_channels(&sample.position), sample.active, !sample.active)
            }
            None => (Projection::NONE, false, false),
        };
        if !active {
            self.track = None;
            self.lifted = false;
        }
        let projection = if active { projection } else { Projection::NONE };
        self.shown = Some(projection.shown(layout));
        Step {
            projection,
            active,
            started: std::mem::take(&mut self.started),
            completed,
            lifted: active && self.lifted,
        }
    }

    /// A frame of a box the pointer holds. Its own glide gives way to the
    /// pointer, and keeps where it was shown as the place the drag starts from.
    fn drag_step(&mut self, layout: Rect, moved: Vec2, offset: Vec2, now: Instant) -> Step {
        if self.dragged.is_none() {
            self.drag_shift = self.track.take().map_or([0.0; 2], |track| {
                Projection::of_channels(&track.sample(now).position).translate
            });
            self.started = false;
        }
        self.drag_shift = sub(self.drag_shift, moved);
        let shown = add(offset, self.drag_shift);
        self.dragged = Some(shown);
        self.lifted = true;
        let projection = Projection {
            translate: shown,
            scale: [1.0; 2],
        };
        self.shown = Some(projection.shown(layout));
        Step {
            projection,
            active: false,
            started: false,
            completed: false,
            lifted: true,
        }
    }
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

/// What a flipping box tells the ones inside it about the space it made.
#[derive(Clone, Copy)]
struct Enclosing {
    /// How the box is shown against where layout put it, in the space around it, however
    /// many flipping boxes are between it and the window.
    net: gpui::TransformationMatrix,
    /// The ambient transform inside the box.
    inside: gpui::TransformationMatrix,
}

#[derive(Default)]
struct Store {
    flips: HashMap<u64, Flip>,
    /// The glide of the flipping box being prepainted, for the boxes inside.
    enclosing: Option<Enclosing>,
    /// Counts the renders that pruned `flips`.
    frame: u64,
    /// The revision of the tree the latest render built, which moves with every commit.
    revision: u64,
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
    ///
    /// `revision` is the tree's: a box whose last step saw another one has a commit to measure.
    pub(crate) fn retain(&self, revision: u64, mut keep: impl FnMut(u64) -> bool) {
        let mut store = self.0.borrow_mut();
        store.frame += 1;
        store.revision = revision;
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
            changed.layout = flip.layout;
            changed.shown = flip.shown;
            changed.seen = flip.seen;
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

    fn step(&self, id: u64, layout: Rect, drag: Drag, now: Instant) -> Option<Step> {
        let mut store = self.0.borrow_mut();
        let frame = store.frame;
        let revision = store.revision;
        let flip = store.flips.get(&id)?;
        let handoff = match flip.layout_id() {
            Some(layout_id) if flip.layout.is_none() && flip.held.is_none() => {
                Some(layout_id.to_string())
            }
            _ => None,
        };
        let holder = handoff.and_then(|layout_id| store.holder(id, &layout_id));
        let flip = store.flips.get_mut(&id)?;
        if let Some(holder) = holder {
            flip.take_over(layout, &holder, now);
        }
        let committed = flip.seen != revision;
        flip.seen = revision;
        let step = flip.step(layout, drag, committed, now);
        let visual = flip.shown?;
        flip.held = Some(Held {
            visual,
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

/// Prepaints and paints its child in the space of the glide.
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

/// The space the child is in, kept from prepaint to paint.
struct Glide {
    space: gpui::ElementSpace,
    /// How far the child is stretched, for its radii and shadows.
    scale: Option<[f32; 2]>,
}

impl Glide {
    fn enter<R>(&self, window: &mut gpui::Window, f: impl FnOnce(&mut gpui::Window) -> R) -> R {
        window.with_element_space(self.space.clone(), |window| match self.scale {
            Some(scale) => window.with_projection_scale(scale, f),
            None => f(window),
        })
    }
}

impl gpui::IntoElement for FlipOffset {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl gpui::Element for FlipOffset {
    type RequestLayoutState = ();
    type PrepaintState = Option<Glide>;

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
    ) -> Option<Glide> {
        // `layout_bounds` added the snapped offset, so that is what comes off.
        let inherited_offset = window.pixel_snap_point(window.element_offset());
        let origin = bounds.origin - inherited_offset;
        let layout = Rect {
            min: [f64::from(f32::from(origin.x)), f64::from(f32::from(origin.y))],
            size: [f64::from(f32::from(bounds.size.width)), f64::from(f32::from(bounds.size.height))],
        };

        let Some(step) = self.flips.step(self.id, layout, self.drag, self.now) else {
            self.child.prepaint(window, cx);
            return None;
        };
        let centre = Rect {
            min: [f64::from(f32::from(bounds.origin.x)), f64::from(f32::from(bounds.origin.y))],
            size: layout.size,
        }
        .center();
        let net = step.projection.matrix_about(centre);
        let ambient = window.transform();
        let enclosing = self.flips.0.borrow().enclosing;
        let matrix = match enclosing {
            Some(enclosing) => layout_projection::inside(enclosing.net, enclosing.inside, ambient, net),
            None => net,
        };
        let glide = Glide {
            space: gpui::ElementSpace {
                matrix: gpui::Matrix3d::from_2d(&matrix),
                animating: step.active,
                ..gpui::ElementSpace::default()
            },
            scale: (step.projection.scale != [1.0; 2])
                .then(|| [step.projection.scale[0] as f32, step.projection.scale[1] as f32]),
        };
        if step.lifted {
            use gpui::IntoElement;

            // GPUI has no z-index. `defer_draw` is what `deferred()` does: the
            // child is prepainted and painted after the rest of the window,
            // in the space it has here, so it covers its siblings. Its
            // content mask is the one it has here, so a clipping ancestor
            // still clips it, and the transforms around it still turn it.
            // The only box inside it that reads an enclosing glide is
            // another `layout` box held at the same time, which reads none.
            self.deferred = true;
            let child = std::mem::replace(&mut self.child, gpui::Empty.into_any_element());
            window.with_element_space(glide.space.clone(), |window| {
                let at = window.element_offset();
                let mask = window.content_mask();
                window.defer_draw(child, at, LIFTED_PRIORITY, Some(mask));
            });
        } else {
            glide.enter(window, |window| {
                let inside = window.transform();
                let outer = self
                    .flips
                    .0
                    .borrow_mut()
                    .enclosing
                    .replace(Enclosing { net, inside });
                self.child.prepaint(window, cx);
                self.flips.0.borrow_mut().enclosing = outer;
            });
        }
        // The events leave with the next render, which a settled box would not ask for.
        if step.active || step.started || step.completed {
            window.request_animation_frame();
        }
        Some(glide)
    }

    fn paint(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        _: gpui::Bounds<gpui::Pixels>,
        _: &mut (),
        glide: &mut Option<Glide>,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        if self.deferred {
            return;
        }
        match glide {
            Some(glide) => glide.enter(window, |window| self.child.paint(window, cx)),
            None => self.child.paint(window, cx),
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

    /// A box 100 by 40 px whose layout corner is at `origin`.
    fn rect(origin: Vec2) -> Rect {
        Rect {
            min: origin,
            size: [100.0, 40.0],
        }
    }

    fn stepped(flip: &mut Flip, origin: Vec2, drag: Drag, committed: bool, now: Instant) -> Step {
        flip.step(rect(origin), drag, committed, now)
    }

    impl Step {
        fn offset(&self) -> Vec2 {
            self.projection.translate
        }
    }

    fn linear() -> Value {
        json!({ "mode": true, "transition": { "duration": 1.0, "ease": "linear" } })
    }

    #[test]
    fn the_first_frame_has_nowhere_to_glide_from() {
        let start = Instant::now();
        let mut flip = flip(linear());
        let step = stepped(&mut flip, [10.0, 20.0], Drag::default(), true, start);
        assert_eq!(step.offset(), [0.0, 0.0]);
        assert!(!step.active);
    }

    #[test]
    fn a_move_starts_where_the_box_was_shown_and_ends_on_layout() {
        let start = Instant::now();
        let mut flip = flip(linear());
        stepped(&mut flip, [0.0, 0.0], Drag::default(), true, start);

        // Layout puts the box 100 px right and 40 px down.
        let first = stepped(&mut flip, [100.0, 40.0], Drag::default(), true, at(start, 10));
        assert_eq!(first.offset(), [-100.0, -40.0]);
        assert!(first.active);

        let half = stepped(&mut flip, [100.0, 40.0], Drag::default(), true, at(start, 510));
        assert!((half.offset()[0] + 50.0).abs() < 1e-6, "{:?}", half.offset());
        assert!((half.offset()[1] + 20.0).abs() < 1e-6);

        let done = stepped(&mut flip, [100.0, 40.0], Drag::default(), true, at(start, 1100));
        assert_eq!(done.offset(), [0.0, 0.0]);
        assert!(!done.active);
        assert!(flip.track.is_none());
    }

    #[test]
    fn a_move_in_flight_continues_from_where_the_box_is_shown() {
        let start = Instant::now();
        let mut flip = flip(linear());
        stepped(&mut flip, [0.0, 0.0], Drag::default(), true, start);
        stepped(&mut flip, [100.0, 0.0], Drag::default(), true, start);
        // Half way: shown 50 px left of layout. Layout moves another 20 px.
        let retarget = stepped(&mut flip, [120.0, 0.0], Drag::default(), true, at(start, 500));
        // Shown spot stays at layout(100) - 50 = 50, new layout 120.
        assert!((retarget.offset()[0] + 70.0).abs() < 1e-6, "{:?}", retarget.offset());
        assert!(retarget.active);
    }

    #[test]
    fn a_move_nobody_committed_starts_no_glide() {
        let start = Instant::now();
        let mut flip = flip(linear());
        stepped(&mut flip, [0.0, 0.0], Drag::default(), true, start);

        // An animation above moved layout by 30 px; the tree did not change.
        let ticked = stepped(&mut flip, [0.0, 30.0], Drag::default(), false, at(start, 16));
        assert_eq!(ticked.offset(), [0.0, 0.0]);
        assert!(!ticked.active && !ticked.started);
        assert!(flip.track.is_none());
    }

    #[test]
    fn a_tick_leaves_a_glide_in_flight_alone_and_a_commit_retargets_it() {
        let start = Instant::now();
        let mut flip = flip(linear());
        stepped(&mut flip, [0.0, 0.0], Drag::default(), true, start);
        stepped(&mut flip, [100.0, 0.0], Drag::default(), true, start);
        let half = stepped(&mut flip, [100.0, 0.0], Drag::default(), false, at(start, 500));
        assert!((half.offset()[0] + 50.0).abs() < 1e-6);

        // Layout moves 40 px more under a tick: the offset is the same, the box goes with layout.
        let ticked = stepped(&mut flip, [100.0, 40.0], Drag::default(), false, at(start, 500));
        assert_eq!(ticked.offset(), half.offset());
        assert!(!ticked.started);

        // The same move at a commit starts from where the box is shown.
        let committed = stepped(&mut flip, [100.0, 80.0], Drag::default(), true, at(start, 500));
        assert!(committed.started);
        assert!((committed.offset()[1] + 40.0).abs() < 1e-6, "{:?}", committed.offset());
    }

    #[test]
    fn a_spring_keeps_the_speed_it_was_moving_at() {
        let start = Instant::now();
        let spring = json!({
            "mode": true,
            "transition": { "type": "spring", "stiffness": 200.0, "damping": 20.0 }
        });
        let mut flip = flip(spring);
        stepped(&mut flip, [0.0, 0.0], Drag::default(), true, start);
        stepped(&mut flip, [100.0, 0.0], Drag::default(), true, start);
        let moving = stepped(&mut flip, [100.0, 0.0], Drag::default(), true, at(start, 100));
        assert!(moving.active);
        let speed = flip.track.as_ref().unwrap().sample(at(start, 100)).velocity[0];
        assert!(speed > 0.0, "the offset is closing on zero from below");

        stepped(&mut flip, [100.0, 50.0], Drag::default(), true, at(start, 100));
        let carried = flip.track.as_ref().unwrap().sample(at(start, 100)).velocity[0];
        assert!((carried - speed).abs() < 1e-6, "{carried} vs {speed}");
    }

    #[test]
    fn reduced_motion_jumps() {
        let start = Instant::now();
        let mut flip = flip(json!({ "mode": true, "reducedMotion": true }));
        stepped(&mut flip, [0.0, 0.0], Drag::default(), true, start);
        let moved = stepped(&mut flip, [100.0, 0.0], Drag::default(), true, at(start, 10));
        assert_eq!(moved.offset(), [0.0, 0.0]);
        assert!(!moved.active);
    }

    #[test]
    fn turning_on_reduced_motion_mid_flight_drops_the_glide() {
        let start = Instant::now();
        let mut flip = flip(linear());
        stepped(&mut flip, [0.0, 0.0], Drag::default(), true, start);
        stepped(&mut flip, [100.0, 0.0], Drag::default(), true, start);
        assert!(flip.track.is_some());
        let mut reduced = Flip::new(&json!({ "mode": true, "reducedMotion": true }), 0);
        reduced.layout = flip.layout;
        reduced.shown = flip.shown;
        reduced.track = flip.track.take();
        let step = stepped(&mut reduced, [100.0, 0.0], Drag::default(), true, at(start, 100));
        assert_eq!(step.offset(), [0.0, 0.0]);
        assert!(!step.active);
    }

    #[test]
    fn rounding_noise_is_not_a_move() {
        let start = Instant::now();
        let mut flip = flip(linear());
        stepped(&mut flip, [10.0, 10.0], Drag::default(), true, start);
        let step = stepped(&mut flip, [10.004, 9.996], Drag::default(), true, at(start, 16));
        assert!(!step.active);
    }

    #[test]
    fn a_delay_holds_the_box_at_its_old_place() {
        let start = Instant::now();
        let mut flip = flip(json!({
            "mode": true,
            "transition": { "duration": 1.0, "ease": "linear", "delay": 0.5 }
        }));
        stepped(&mut flip, [0.0, 0.0], Drag::default(), true, start);
        stepped(&mut flip, [100.0, 0.0], Drag::default(), true, start);
        let held = stepped(&mut flip, [100.0, 0.0], Drag::default(), true, at(start, 400));
        assert_eq!(held.offset(), [-100.0, 0.0]);
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
        assert_eq!(parse(&json!("size")).unwrap().unwrap().axes, Axes::Size);
        assert_eq!(parse(&json!("position")).unwrap().unwrap().axes, Axes::Position);
        assert_eq!(parse(&json!(true)).unwrap().unwrap().axes, Axes::Both);
        assert!(parse(&json!(false)).unwrap().is_none());
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
        stepped(&mut flip, [0.0, 0.0], Drag::default(), true, start);
        let step = stepped(&mut flip, [50.0, 0.0], Drag::default(), true, at(start, 10));
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
        let revision = flips.0.borrow().revision + 1;
        flips.retain(revision, |id| alive.contains(&id));
    }

    #[test]
    fn a_newcomer_starts_where_the_box_that_just_left_was_shown() {
        let start = Instant::now();
        let flips = LayoutFlips::default();
        render(&flips, &[1]);
        mount(&flips, 1, &shared("tab"));
        flips.step(1, rect([0.0, 0.0]), Drag::default(), start).unwrap();

        // Unmounted and replaced on the same frame, 100 px to the right.
        render(&flips, &[2]);
        mount(&flips, 2, &shared("tab"));
        let step = flips.step(2, rect([100.0, 0.0]), Drag::default(), at(start, 16)).unwrap();
        assert_eq!(step.offset(), [-100.0, 0.0]);
        assert!(step.started && step.active);
        assert_eq!(flips.take_events(), vec![LayoutEvent { id: 2, kind: LayoutEventKind::Start }]);

        let half = flips.step(2, rect([100.0, 0.0]), Drag::default(), at(start, 516)).unwrap();
        assert!((half.offset()[0] + 50.0).abs() < 1e-6, "{:?}", half.offset());
    }

    #[test]
    fn the_holder_is_remembered_for_one_frame_after_it_left() {
        let start = Instant::now();
        let flips = LayoutFlips::default();
        render(&flips, &[1]);
        mount(&flips, 1, &shared("tab"));
        flips.step(1, rect([0.0, 0.0]), Drag::default(), start).unwrap();

        render(&flips, &[]);
        render(&flips, &[2]);
        mount(&flips, 2, &shared("tab"));
        let next_frame = flips.step(2, rect([100.0, 0.0]), Drag::default(), at(start, 16)).unwrap();
        assert!(next_frame.active, "one frame later it is still there");

        let flips = LayoutFlips::default();
        render(&flips, &[1]);
        mount(&flips, 1, &shared("tab"));
        flips.step(1, rect([0.0, 0.0]), Drag::default(), start).unwrap();
        render(&flips, &[]);
        render(&flips, &[]);
        render(&flips, &[2]);
        mount(&flips, 2, &shared("tab"));
        let late = flips.step(2, rect([100.0, 0.0]), Drag::default(), at(start, 16)).unwrap();
        assert!(!late.active, "two frames later it is forgotten");
    }

    #[test]
    fn a_holder_that_is_still_mounted_hands_over_its_place() {
        let start = Instant::now();
        let flips = LayoutFlips::default();
        render(&flips, &[1]);
        mount(&flips, 1, &shared("tab"));
        flips.step(1, rect([20.0, 0.0]), Drag::default(), start).unwrap();

        render(&flips, &[1, 2]);
        mount(&flips, 2, &shared("tab"));
        let step = flips.step(2, rect([120.0, 0.0]), Drag::default(), at(start, 16)).unwrap();
        assert_eq!(step.offset(), [-100.0, 0.0]);
        // The old one is untouched: hiding it is up to the app.
        let old = flips.step(1, rect([20.0, 0.0]), Drag::default(), at(start, 16)).unwrap();
        assert!(!old.active);
    }

    #[test]
    fn a_live_holder_is_where_it_is_drawn_not_where_layout_put_it() {
        let start = Instant::now();
        let flips = LayoutFlips::default();
        render(&flips, &[1]);
        mount(&flips, 1, &shared("tab"));
        flips.step(1, rect([0.0, 0.0]), Drag::default(), start).unwrap();
        render(&flips, &[1]);
        flips.step(1, rect([100.0, 0.0]), Drag::default(), at(start, 16)).unwrap();
        // Half way: drawn at 50.
        flips.step(1, rect([100.0, 0.0]), Drag::default(), at(start, 516)).unwrap();

        render(&flips, &[1, 2]);
        mount(&flips, 2, &shared("tab"));
        let step = flips.step(2, rect([300.0, 0.0]), Drag::default(), at(start, 516)).unwrap();
        assert!((step.offset()[0] + 250.0).abs() < 1e-3, "{:?}", step.offset());
    }

    #[test]
    fn two_newcomers_with_one_id_do_not_start_from_each_other() {
        let start = Instant::now();
        let flips = LayoutFlips::default();
        render(&flips, &[1, 2]);
        mount(&flips, 1, &shared("tab"));
        mount(&flips, 2, &shared("tab"));
        flips.step(1, rect([0.0, 0.0]), Drag::default(), start).unwrap();
        let second = flips.step(2, rect([100.0, 0.0]), Drag::default(), start).unwrap();
        assert!(!second.active);
    }

    #[test]
    fn other_ids_and_boxes_without_an_id_take_nothing() {
        let start = Instant::now();
        let flips = LayoutFlips::default();
        render(&flips, &[1]);
        mount(&flips, 1, &shared("a"));
        flips.step(1, rect([0.0, 0.0]), Drag::default(), start).unwrap();
        render(&flips, &[2, 3]);
        mount(&flips, 2, &shared("b"));
        mount(&flips, 3, &linear());
        assert!(!flips.step(2, rect([100.0, 0.0]), Drag::default(), start).unwrap().active);
        assert!(!flips.step(3, rect([100.0, 0.0]), Drag::default(), start).unwrap().active);
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
        flips.step(1, rect([0.0, 0.0]), Drag::default(), start).unwrap();
        render(&flips, &[1]);
        flips.step(1, rect([100.0, 0.0]), Drag::default(), at(start, 16)).unwrap();
        flips.step(1, rect([100.0, 0.0]), Drag::default(), at(start, 116)).unwrap();
        let speed = {
            let store = flips.0.borrow();
            let track = store.flips[&1].track.as_ref().unwrap();
            track.sample(at(start, 116)).velocity[0]
        };
        assert!(speed > 0.0);

        render(&flips, &[2]);
        mount(&flips, 2, &spring);
        flips.step(2, rect([100.0, 0.0]), Drag::default(), at(start, 116)).unwrap();
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
        flips.step(1, rect([30.0, 30.0]), Drag::default(), start).unwrap();
        render(&flips, &[2]);
        mount(&flips, 2, &shared("tab"));
        let step = flips.step(2, rect([30.0, 30.0]), Drag::default(), start).unwrap();
        assert!(!step.active && !step.started);
    }

    #[test]
    fn reduced_motion_takes_nothing_over() {
        let start = Instant::now();
        let reduced = json!({ "mode": true, "layoutId": "tab", "reducedMotion": true });
        let flips = LayoutFlips::default();
        render(&flips, &[1]);
        mount(&flips, 1, &reduced);
        flips.step(1, rect([0.0, 0.0]), Drag::default(), start).unwrap();
        render(&flips, &[2]);
        mount(&flips, 2, &reduced);
        let step = flips.step(2, rect([100.0, 0.0]), Drag::default(), start).unwrap();
        assert!(!step.active);
        assert!(flips.take_events().is_empty());
    }

    #[test]
    fn a_glide_reports_its_start_and_its_end_once() {
        let start = Instant::now();
        let mut flip = flip(linear());
        assert!(!stepped(&mut flip, [0.0, 0.0], Drag::default(), true, start).started);
        let first = stepped(&mut flip, [100.0, 0.0], Drag::default(), true, at(start, 10));
        assert!(first.started && !first.completed);
        let middle = stepped(&mut flip, [100.0, 0.0], Drag::default(), true, at(start, 500));
        assert!(!middle.started && !middle.completed);
        let done = stepped(&mut flip, [100.0, 0.0], Drag::default(), true, at(start, 1100));
        assert!(done.completed && !done.started);
        let after = stepped(&mut flip, [100.0, 0.0], Drag::default(), true, at(start, 1200));
        assert!(!after.completed);
    }

    #[test]
    fn a_retarget_starts_again_without_completing_the_one_it_replaced() {
        let start = Instant::now();
        let mut flip = flip(linear());
        stepped(&mut flip, [0.0, 0.0], Drag::default(), true, start);
        stepped(&mut flip, [100.0, 0.0], Drag::default(), true, at(start, 10));
        let retarget = stepped(&mut flip, [200.0, 0.0], Drag::default(), true, at(start, 500));
        assert!(retarget.started && !retarget.completed);
    }

    #[test]
    fn reduced_motion_raises_no_events() {
        let start = Instant::now();
        let mut flip = flip(json!({ "mode": true, "reducedMotion": true }));
        stepped(&mut flip, [0.0, 0.0], Drag::default(), true, start);
        let moved = stepped(&mut flip, [100.0, 0.0], Drag::default(), true, at(start, 10));
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
        stepped(&mut flip, [0.0, 0.0], Drag::default(), true, start);
        let step = stepped(&mut flip, [0.0, 0.0], held([0.0, 30.0]), true, at(start, 10));
        assert_eq!(step.offset(), [0.0, 30.0]);
        assert!(!step.active, "the pointer moves it, not a clock");
        assert!(step.lifted);
        let later = stepped(&mut flip, [0.0, 0.0], held([5.0, 42.0]), true, at(start, 20));
        assert_eq!(later.offset(), [5.0, 42.0]);
    }

    #[test]
    fn a_held_box_stays_under_the_pointer_when_layout_moves_its_slot() {
        let start = Instant::now();
        let mut flip = flip(linear());
        stepped(&mut flip, [0.0, 0.0], Drag::default(), true, start);
        stepped(&mut flip, [0.0, 0.0], held([0.0, 30.0]), true, at(start, 10));
        // A swap moved its slot 20 px down; the pointer has not moved.
        let swapped = stepped(&mut flip, [0.0, 20.0], held([0.0, 30.0]), true, at(start, 20));
        assert_eq!(swapped.offset(), [0.0, 10.0]);
        assert!(!swapped.active && !swapped.started, "no glide for a held box");
        // The pointer then moves 5 px further.
        let after = stepped(&mut flip, [0.0, 20.0], held([0.0, 35.0]), true, at(start, 30));
        assert_eq!(after.offset(), [0.0, 15.0]);
    }

    #[test]
    fn a_released_box_glides_from_where_it_was_shown_to_its_slot() {
        let start = Instant::now();
        let mut flip = flip(linear());
        stepped(&mut flip, [0.0, 0.0], Drag::default(), true, start);
        stepped(&mut flip, [0.0, 20.0], held([0.0, 30.0]), true, at(start, 10));
        // Released: the drag values are read as whatever they are now.
        let release = Drag {
            active: false,
            offset: [0.0, 0.0],
        };
        // Layout put the slot 20 px down while it was held, so it is 10 px from it.
        let first = stepped(&mut flip, [0.0, 20.0], release, true, at(start, 20));
        assert_eq!(first.offset(), [0.0, 10.0]);
        assert!(first.active && first.started);
        assert!(first.lifted, "it stays above its neighbours on the way home");
        let half = stepped(&mut flip, [0.0, 20.0], release, true, at(start, 520));
        assert!((half.offset()[1] - 5.0).abs() < 1e-6, "{:?}", half.offset());
        let done = stepped(&mut flip, [0.0, 20.0], release, true, at(start, 1100));
        assert_eq!(done.offset(), [0.0, 0.0]);
        assert!(done.completed && !done.lifted);
    }

    #[test]
    fn releasing_where_the_box_belongs_has_nothing_to_glide() {
        let start = Instant::now();
        let mut flip = flip(linear());
        stepped(&mut flip, [0.0, 0.0], Drag::default(), true, start);
        stepped(&mut flip, [0.0, 0.0], held([0.0, 0.0]), true, at(start, 10));
        let step = stepped(&mut flip, [0.0, 0.0], Drag::default(), true, at(start, 20));
        assert!(!step.active && !step.started && !step.completed);
    }

    #[test]
    fn a_box_picked_up_mid_glide_starts_from_where_it_is_shown() {
        let start = Instant::now();
        let mut flip = flip(linear());
        stepped(&mut flip, [0.0, 0.0], Drag::default(), true, start);
        stepped(&mut flip, [0.0, 100.0], Drag::default(), true, at(start, 10));
        // Half way: shown 50 px above its slot.
        let held = stepped(&mut flip, [0.0, 100.0], held([0.0, 8.0]), true, at(start, 510));
        assert!((held.offset()[1] + 42.0).abs() < 1e-6, "{:?}", held.offset());
    }

    #[test]
    fn a_release_under_reduced_motion_jumps() {
        let start = Instant::now();
        let mut flip = flip(json!({ "mode": true, "reducedMotion": true }));
        stepped(&mut flip, [0.0, 0.0], Drag::default(), true, start);
        stepped(&mut flip, [0.0, 0.0], held([0.0, 30.0]), true, at(start, 10));
        let step = stepped(&mut flip, [0.0, 0.0], Drag::default(), true, at(start, 20));
        assert_eq!(step.offset(), [0.0, 0.0]);
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

    fn wide(origin: Vec2, height: f64) -> Rect {
        Rect {
            min: origin,
            size: [100.0, height],
        }
    }

    #[test]
    fn a_resize_glides_from_the_shown_size_to_the_one_layout_has() {
        let start = Instant::now();
        let mut flip = flip(linear());
        flip.step(wide([0.0, 0.0], 40.0), Drag::default(), true, start);
        let first = flip.step(wide([0.0, 0.0], 80.0), Drag::default(), true, at(start, 10));
        assert!(first.started && first.active);
        assert_eq!(first.projection.scale, [1.0, 0.5]);
        assert_eq!(first.projection.translate, [0.0, -20.0]);
        let shown = first.projection.shown(wide([0.0, 0.0], 80.0));
        assert_eq!((shown.min, shown.size), ([0.0, 0.0], [100.0, 40.0]));

        let half = flip.step(wide([0.0, 0.0], 80.0), Drag::default(), false, at(start, 510));
        assert!((half.projection.scale[1] - 0.75).abs() < 1e-6, "{:?}", half.projection);
        assert!((half.projection.translate[1] + 10.0).abs() < 1e-6);

        let done = flip.step(wide([0.0, 0.0], 80.0), Drag::default(), false, at(start, 1100));
        assert_eq!(done.projection, Projection::NONE);
        assert!(done.completed);
    }

    #[test]
    fn position_animates_the_corner_and_size_the_size() {
        let start = Instant::now();
        let source = |mode: &str| json!({ "mode": mode, "transition": { "duration": 1.0, "ease": "linear" } });

        let mut position = flip(source("position"));
        position.step(wide([0.0, 0.0], 40.0), Drag::default(), true, start);
        let step = position.step(wide([50.0, 0.0], 80.0), Drag::default(), true, at(start, 10));
        assert_eq!(step.projection.scale, [1.0, 1.0], "the size snaps");
        assert_eq!(step.projection.translate, [-50.0, 0.0]);

        let mut size = flip(source("size"));
        size.step(wide([0.0, 0.0], 40.0), Drag::default(), true, start);
        let step = size.step(wide([50.0, 0.0], 80.0), Drag::default(), true, at(start, 10));
        assert_eq!(step.projection.scale, [1.0, 0.5]);
        let shown = step.projection.shown(wide([50.0, 0.0], 80.0));
        assert_eq!(shown.min, [50.0, 0.0], "the corner snaps");

        let mut moved = flip(source("size"));
        moved.step(wide([0.0, 0.0], 40.0), Drag::default(), true, start);
        let step = moved.step(wide([50.0, 0.0], 40.0), Drag::default(), true, at(start, 10));
        assert!(!step.active, "a move alone is no size glide");
    }

    #[test]
    fn a_size_nobody_committed_follows_at_once() {
        let start = Instant::now();
        let mut flip = flip(linear());
        flip.step(wide([0.0, 0.0], 40.0), Drag::default(), true, start);
        let step = flip.step(wide([0.0, 0.0], 80.0), Drag::default(), false, at(start, 16));
        assert_eq!(step.projection, Projection::NONE);
        assert!(!step.active);
    }

    #[test]
    fn a_newcomer_takes_the_size_of_the_box_it_replaces() {
        let start = Instant::now();
        let flips = LayoutFlips::default();
        render(&flips, &[1]);
        mount(&flips, 1, &shared("card"));
        flips.step(1, wide([0.0, 0.0], 40.0), Drag::default(), start).unwrap();

        render(&flips, &[2]);
        mount(&flips, 2, &shared("card"));
        let step = flips.step(2, wide([0.0, 100.0], 80.0), Drag::default(), at(start, 16)).unwrap();
        assert!(step.started);
        assert_eq!(step.projection.scale, [1.0, 0.5]);
        let shown = step.projection.shown(wide([0.0, 100.0], 80.0));
        assert_eq!((shown.min, shown.size), ([0.0, 0.0], [100.0, 40.0]));
    }
}
