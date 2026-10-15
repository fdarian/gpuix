//! The `layout` prop: when a box's layout position changes, it glides from
//! where it was shown to where layout put it, FLIP style.
//!
//! GPUI has no transforms, but everything prepainted inside
//! `Window::with_element_offset` (bounds, hitboxes, paint) is shifted by it.
//! [`FlipOffset`] wraps an element's built subtree and prepaints it inside an
//! offset that starts at `old position - new position` and eases to zero, so
//! the box is drawn, and hit-tested, where the animation has it. Layout itself
//! is never touched: only position animates, a size change snaps.
//!
//! The origin it compares is the layout origin with the ancestors' offsets
//! taken back out, so scrolling and an ancestor's own FLIP do not read as a move.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use serde_json::Value;
use web_time::Instant;

use super::curve::{Curve, Ease, Transition};
use super::track::Track;
use super::value::AnimValue;
use super::Transitions;

/// Layout moves smaller than this, in px, are rounding noise.
const MOVED: f64 = 0.01;

type Vec2 = [f64; 2];

fn sub(a: Vec2, b: Vec2) -> Vec2 {
    [a[0] - b[0], a[1] - b[1]]
}

fn is_zero(a: Vec2) -> bool {
    a[0].abs() < MOVED && a[1].abs() < MOVED
}

/// What a `layout` prop asks for.
#[derive(Clone, Debug, PartialEq)]
struct Config {
    transition: Transition,
    reduced: bool,
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
/// change snaps (there is no transform to scale by).
fn parse(source: &Value) -> Result<Option<Config>, String> {
    let empty = serde_json::Map::new();
    let (mode, fields) = match source {
        Value::Object(fields) => (fields.get("mode").unwrap_or(&Value::Null), fields),
        other => (other, &empty),
    };
    let animates_position = match mode {
        Value::Bool(on) => *on,
        Value::Null if source.is_object() => true,
        Value::String(name) => match name.as_str() {
            "position" => true,
            "size" => false,
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
    Ok(Some(Config { transition, reduced }))
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
}

impl Flip {
    fn new(source: &Value) -> Self {
        let config = parse(source).unwrap_or_else(|error| {
            log::warn!("Invalid layout description: {error}");
            None
        });
        Self {
            source: source.clone(),
            config,
            origin: None,
            track: None,
        }
    }

    /// `inherited` is how far the nearest enclosing flipping box's layout
    /// moved this frame: it carries this one with it and animates for both.
    fn step(&mut self, origin: Vec2, inherited: Vec2, now: Instant) -> Step {
        let Some(config) = &self.config else {
            return Step {
                offset: [0.0; 2],
                active: false,
                moved: [0.0; 2],
            };
        };
        let moved = self.origin.map_or([0.0; 2], |last| sub(origin, last));
        self.origin = Some(origin);
        let own = sub(moved, inherited);

        if config.reduced {
            self.track = None;
        } else if !is_zero(own) {
            let shown = self
                .track
                .as_ref()
                .map_or([0.0; 2], |track| to_vec2(&track.sample(now).position));
            // The box was drawn `shown` away from its old place, and its place
            // just moved by `own`: start from the same spot on screen.
            let from = sub(shown, own);
            let started = Track::new(
                "layout",
                &AnimValue::Numbers(from.to_vec()),
                AnimValue::Numbers(vec![0.0, 0.0]),
                config.transition.clone(),
                now,
            );
            match started {
                Ok(mut next) => {
                    if let Some(previous) = &self.track {
                        next.carry_velocity(previous, now);
                    }
                    self.track = Some(next);
                }
                Err(error) => {
                    log::warn!("Invalid layout transition: {error}");
                    self.track = None;
                }
            }
        }

        let (offset, active) = match &self.track {
            Some(track) => {
                let sample = track.sample(now);
                (to_vec2(&sample.position), sample.active)
            }
            None => ([0.0; 2], false),
        };
        if !active {
            self.track = None;
        }
        Step {
            offset: if active { offset } else { [0.0; 2] },
            active,
            moved,
        }
    }
}

fn to_vec2(channels: &[f64]) -> Vec2 {
    [channels[0], channels[1]]
}

#[derive(Default)]
struct Store {
    flips: HashMap<u64, Flip>,
    /// The layout move of the flipping box being prepainted, for the boxes inside.
    enclosing: Vec2,
}

/// The glide of every element with a `layout` prop, shared with the wrappers
/// the build hands to GPUI.
#[derive(Clone, Default)]
pub(crate) struct LayoutFlips(Rc<RefCell<Store>>);

impl LayoutFlips {
    pub(crate) fn retain(&self, mut keep: impl FnMut(u64) -> bool) {
        self.0.borrow_mut().flips.retain(|id, _| keep(*id));
    }

    /// `child` wrapped to glide, or `child` itself when `source` asks for
    /// nothing that can animate.
    pub(crate) fn wrap(
        &self,
        id: u64,
        source: &Value,
        child: gpui::AnyElement,
        now: Instant,
    ) -> gpui::AnyElement {
        use gpui::IntoElement;

        let mut store = self.0.borrow_mut();
        let flip = store.flips.entry(id).or_insert_with(|| Flip::new(source));
        if flip.source != *source {
            let origin = flip.origin;
            let track = flip.track.take();
            *flip = Flip::new(source);
            flip.origin = origin;
            flip.track = track;
        }
        if flip.config.is_none() {
            return child;
        }
        drop(store);
        FlipOffset {
            id,
            flips: self.clone(),
            now,
            child,
        }
        .into_any_element()
    }

    fn step(&self, id: u64, origin: Vec2, now: Instant) -> Option<Step> {
        let mut store = self.0.borrow_mut();
        let inherited = store.enclosing;
        Some(store.flips.get_mut(&id)?.step(origin, inherited, now))
    }
}

/// Prepaints its child shifted by the glide.
///
/// It answers layout with the child's own node, as `ListWithScrollbar` does,
/// so wrapping changes nothing the style asked for.
struct FlipOffset {
    id: u64,
    flips: LayoutFlips,
    now: Instant,
    child: gpui::AnyElement,
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

        let Some(step) = self.flips.step(self.id, origin, self.now) else {
            self.child.prepaint(window, cx);
            return;
        };
        let offset = gpui::point(gpui::px(step.offset[0] as f32), gpui::px(step.offset[1] as f32));
        let outer = std::mem::replace(&mut self.flips.0.borrow_mut().enclosing, step.moved);
        window.with_element_offset(offset, |window| self.child.prepaint(window, cx));
        self.flips.0.borrow_mut().enclosing = outer;
        if step.active {
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
        self.child.paint(window, cx);
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
        Flip::new(&source)
    }

    fn linear() -> Value {
        json!({ "mode": true, "transition": { "duration": 1.0, "ease": "linear" } })
    }

    #[test]
    fn the_first_frame_has_nowhere_to_glide_from() {
        let start = Instant::now();
        let mut flip = flip(linear());
        let step = flip.step([10.0, 20.0], [0.0; 2], start);
        assert_eq!(step.offset, [0.0, 0.0]);
        assert!(!step.active);
    }

    #[test]
    fn a_move_starts_where_the_box_was_shown_and_ends_on_layout() {
        let start = Instant::now();
        let mut flip = flip(linear());
        flip.step([0.0, 0.0], [0.0; 2], start);

        // Layout puts the box 100 px right and 40 px down.
        let first = flip.step([100.0, 40.0], [0.0; 2], at(start, 10));
        assert_eq!(first.offset, [-100.0, -40.0]);
        assert!(first.active);
        assert_eq!(first.moved, [100.0, 40.0]);

        let half = flip.step([100.0, 40.0], [0.0; 2], at(start, 510));
        assert!((half.offset[0] + 50.0).abs() < 1e-6, "{:?}", half.offset);
        assert!((half.offset[1] + 20.0).abs() < 1e-6);
        assert_eq!(half.moved, [0.0, 0.0]);

        let done = flip.step([100.0, 40.0], [0.0; 2], at(start, 1100));
        assert_eq!(done.offset, [0.0, 0.0]);
        assert!(!done.active);
        assert!(flip.track.is_none());
    }

    #[test]
    fn a_move_in_flight_continues_from_where_the_box_is_shown() {
        let start = Instant::now();
        let mut flip = flip(linear());
        flip.step([0.0, 0.0], [0.0; 2], start);
        flip.step([100.0, 0.0], [0.0; 2], start);
        // Half way: shown 50 px left of layout. Layout moves another 20 px.
        let retarget = flip.step([120.0, 0.0], [0.0; 2], at(start, 500));
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
        flip.step([0.0, 0.0], [0.0; 2], start);
        flip.step([100.0, 0.0], [0.0; 2], start);
        let moving = flip.step([100.0, 0.0], [0.0; 2], at(start, 100));
        assert!(moving.active);
        let speed = flip.track.as_ref().unwrap().sample(at(start, 100)).velocity[0];
        assert!(speed > 0.0, "the offset is closing on zero from below");

        flip.step([100.0, 50.0], [0.0; 2], at(start, 100));
        let carried = flip.track.as_ref().unwrap().sample(at(start, 100)).velocity[0];
        assert!((carried - speed).abs() < 1e-6, "{carried} vs {speed}");
    }

    #[test]
    fn a_box_carried_by_an_enclosing_one_glides_only_by_the_rest() {
        let start = Instant::now();
        let mut flip = flip(linear());
        flip.step([0.0, 0.0], [0.0; 2], start);
        // The parent's layout moved 100 and so did this box: nothing of its own.
        let carried = flip.step([100.0, 0.0], [100.0, 0.0], at(start, 10));
        assert_eq!(carried.offset, [0.0, 0.0]);
        assert!(!carried.active);
        // 30 px further than the parent.
        let extra = flip.step([230.0, 0.0], [100.0, 0.0], at(start, 20));
        assert_eq!(extra.offset, [-30.0, 0.0]);
    }

    #[test]
    fn reduced_motion_jumps() {
        let start = Instant::now();
        let mut flip = flip(json!({ "mode": true, "reducedMotion": true }));
        flip.step([0.0, 0.0], [0.0; 2], start);
        let moved = flip.step([100.0, 0.0], [0.0; 2], at(start, 10));
        assert_eq!(moved.offset, [0.0, 0.0]);
        assert!(!moved.active);
        assert_eq!(moved.moved, [100.0, 0.0], "boxes inside still read the move");
    }

    #[test]
    fn turning_on_reduced_motion_mid_flight_drops_the_glide() {
        let start = Instant::now();
        let mut flip = flip(linear());
        flip.step([0.0, 0.0], [0.0; 2], start);
        flip.step([100.0, 0.0], [0.0; 2], start);
        assert!(flip.track.is_some());
        let mut reduced = Flip::new(&json!({ "mode": true, "reducedMotion": true }));
        reduced.origin = flip.origin;
        reduced.track = flip.track.take();
        let step = reduced.step([100.0, 0.0], [0.0; 2], at(start, 100));
        assert_eq!(step.offset, [0.0, 0.0]);
        assert!(!step.active);
    }

    #[test]
    fn rounding_noise_is_not_a_move() {
        let start = Instant::now();
        let mut flip = flip(linear());
        flip.step([10.0, 10.0], [0.0; 2], start);
        let step = flip.step([10.004, 9.996], [0.0; 2], at(start, 16));
        assert!(!step.active);
    }

    #[test]
    fn a_delay_holds_the_box_at_its_old_place() {
        let start = Instant::now();
        let mut flip = flip(json!({
            "mode": true,
            "transition": { "duration": 1.0, "ease": "linear", "delay": 0.5 }
        }));
        flip.step([0.0, 0.0], [0.0; 2], start);
        flip.step([100.0, 0.0], [0.0; 2], start);
        let held = flip.step([100.0, 0.0], [0.0; 2], at(start, 400));
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
        flip.step([0.0, 0.0], [0.0; 2], start);
        let step = flip.step([50.0, 0.0], [0.0; 2], at(start, 10));
        assert!(!step.active);
    }
}
