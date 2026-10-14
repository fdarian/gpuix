//! Native motion tracks resolved during GPUI rendering, outside React.
//!
//! A description (`initial`, `animate`, `transition`) becomes one [`Track`] per
//! animated property. A retarget restarts only the properties whose target or
//! transition changed, so the rest keep running undisturbed.

mod css_transition;
mod curve;
mod property;
pub(crate) mod state_source;
mod track;
mod value;

use std::collections::BTreeMap;

use serde_json::{Map, Value};
use web_time::Instant;

use crate::style::StyleDesc;
pub(crate) use curve::cubic_bezier;
pub(crate) use css_transition::{validate_transitions, CssTransitions, TransitionEvent};
use curve::Transition;
use property::AnimProp;
use track::Track;
use value::AnimValue;

/// The animated properties' current values, ready to lay over a style.
pub(crate) struct MotionStyle(Vec<(AnimProp, AnimValue)>);

impl MotionStyle {
    pub(crate) fn apply_to(self, style: &mut StyleDesc) {
        for (prop, value) in self.0 {
            prop.write(style, value);
        }
    }
}

type Styles = BTreeMap<AnimProp, AnimValue>;

/// Fields of a transition object that describe one curve. Every other key is
/// `default` or the name of the property it applies to.
const CURVE_KEYS: [&str; 10] = [
    "type",
    "duration",
    "delay",
    "ease",
    "stiffness",
    "damping",
    "mass",
    "bounce",
    "restSpeed",
    "restDelta",
];

/// Framer's per-property transition shape: `transition[prop]`, then
/// `transition.default`, then the top-level curve fields, then the engine
/// default. Each level replaces the next as a whole, it is never merged.
#[derive(Clone, Debug, PartialEq)]
struct Transitions {
    fallback: Transition,
    per_prop: BTreeMap<AnimProp, Transition>,
}

impl Transitions {
    fn for_prop(&self, prop: AnimProp) -> &Transition {
        self.per_prop.get(&prop).unwrap_or(&self.fallback)
    }

    fn parse(source: Option<&Value>) -> Result<Self, String> {
        let mut fields = match source {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(fields)) => fields.clone(),
            Some(_) => return Err("motion transition must be an object".to_string()),
        };

        let default = fields.remove("default").filter(|value| !value.is_null());
        let mut by_prop = Vec::new();
        for key in fields.keys().cloned().collect::<Vec<_>>() {
            if CURVE_KEYS.contains(&key.as_str()) {
                continue;
            }
            by_prop.push((key.clone(), fields.remove(&key).expect("key came from the map")));
        }
        let top_level = Transition::parse(Value::Object(fields))?;
        let fallback = match default {
            Some(default) => Transition::parse(default)?,
            None => top_level,
        };

        let mut per_prop = BTreeMap::new();
        // Shorthands first, so a longhand named beside them is the more specific.
        by_prop.sort_by_key(|(key, _)| property::expand_shorthand(key).is_none());
        for (key, value) in by_prop {
            let transition = Transition::parse(value)?;
            if let Some(props) = property::expand_shorthand(&key) {
                for prop in props {
                    per_prop.insert(*prop, transition.clone());
                }
            } else if let Some(prop) = AnimProp::from_name(&key) {
                per_prop.insert(prop, transition);
            } else {
                return Err(format!("unknown motion transition key: {key}"));
            }
        }
        Ok(Self { fallback, per_prop })
    }
}

struct Description {
    generation: u64,
    is_exit: bool,
    /// `None` for `initial: false` or no `initial`: nothing animates on mount.
    initial: Option<Styles>,
    animate: Styles,
    transition: Transitions,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MotionFrame {
    pub active: bool,
    pub just_settled: bool,
    pub generation: u64,
}

pub(crate) struct MotionState {
    source: Value,
    tracks: BTreeMap<AnimProp, Track>,
    valid: bool,
    needs_settle: bool,
    generation: u64,
}

impl MotionState {
    /// `base` is the element's own style: Framer reads the DOM for the value
    /// an animation starts from when `initial` does not name it.
    pub(crate) fn new(
        source: &Value,
        base: Option<&StyleDesc>,
        now: Instant,
    ) -> Result<Self, String> {
        let description = parse_description(source)?;
        let tracks = mount_tracks(&description, base, now)?;
        Ok(Self {
            source: source.clone(),
            needs_settle: description.is_exit || tracks.values().any(Track::has_motion),
            tracks,
            valid: true,
            generation: description.generation,
        })
    }

    pub(crate) fn invalid(source: &Value, _now: Instant) -> Self {
        Self {
            source: source.clone(),
            tracks: BTreeMap::new(),
            valid: false,
            needs_settle: source_is_exit(source),
            generation: source_generation(source),
        }
    }

    pub(crate) fn sync(
        &mut self,
        source: &Value,
        base: Option<&StyleDesc>,
        now: Instant,
    ) -> Result<(), String> {
        if self.source == *source {
            return Ok(());
        }

        let previous_generation = self.generation;
        let description = match parse_description(source) {
            Ok(description) => description,
            Err(error) => {
                self.source = source.clone();
                self.valid = false;
                self.generation = source_generation(source);
                self.needs_settle =
                    source_is_exit(source) || self.generation != previous_generation;
                return Err(error);
            }
        };

        let resolved = if self.valid {
            self.retarget_tracks(&description, base, now)
        } else {
            mount_tracks(&description, base, now).map(|tracks| {
                let moving = tracks.values().any(Track::has_motion);
                (tracks, moving)
            })
        };
        let (tracks, moving) = match resolved {
            Ok(resolved) => resolved,
            Err(error) => {
                self.source = source.clone();
                self.valid = false;
                self.generation = source_generation(source);
                self.needs_settle =
                    source_is_exit(source) || self.generation != previous_generation;
                return Err(error);
            }
        };

        self.needs_settle =
            description.is_exit || description.generation != previous_generation || moving;
        self.tracks = tracks;
        self.source = source.clone();
        self.valid = true;
        self.generation = description.generation;
        Ok(())
    }

    /// Keeps a track whose target and transition are unchanged, restarts the
    /// others from where they are. A property the new description leaves out
    /// keeps running toward its old target, as in Framer.
    fn retarget_tracks(
        &self,
        description: &Description,
        base: Option<&StyleDesc>,
        now: Instant,
    ) -> Result<(BTreeMap<AnimProp, Track>, bool), String> {
        let mut tracks = self.tracks.clone();
        let mut moving = false;
        for (prop, target) in &description.animate {
            let transition = description.transition.for_prop(*prop);
            let next = match self.tracks.get(prop) {
                Some(track)
                    if track.target() == target && track.transition() == transition =>
                {
                    continue;
                }
                Some(track) => {
                    track.retarget(prop.name(), target.clone(), transition.clone(), now)?
                }
                None => {
                    let from = resolve_start(*prop, base).unwrap_or_else(|| target.clone());
                    Track::new(prop.name(), &from, target.clone(), transition.clone(), now)?
                }
            };
            moving |= next.has_motion();
            tracks.insert(*prop, next);
        }
        Ok((tracks, moving))
    }

    /// Whether this description animates `prop`, which then is not a CSS
    /// transition's to animate.
    fn owns(&self, prop: AnimProp) -> bool {
        self.valid && self.tracks.contains_key(&prop)
    }

    pub(crate) fn visible_style(&self, now: Instant) -> Option<MotionStyle> {
        self.valid.then(|| {
            MotionStyle(
                self.tracks
                    .iter()
                    .map(|(prop, track)| {
                        let position = track.sample(now).position;
                        (*prop, prop.clamp_value(track.value(&position)))
                    })
                    .collect(),
            )
        })
    }

    pub(crate) fn frame(&mut self, now: Instant) -> MotionFrame {
        let active = self.valid && self.tracks.values().any(|track| track.sample(now).active);
        if active {
            self.needs_settle = true;
        }
        let just_settled = self.needs_settle && !active;
        if just_settled {
            self.needs_settle = false;
        }
        MotionFrame {
            active,
            just_settled,
            generation: self.generation,
        }
    }
}

/// The style `style` has after the state styles that currently apply (focus,
/// hover, then press, the order GPUI refines them in), with the CSS transitions
/// running on it laid on top. The result has no state styles left, so GPUI's own
/// `.hover()` / `.active()` / `.focus_visible()` refinements do not apply them a
/// second time, instantly.
pub(crate) fn resolve_transitioned_style(
    transitions: &mut CssTransitions,
    pointer: (bool, bool),
    motion: Option<&MotionState>,
    id: u64,
    style: &StyleDesc,
    focus_visible: bool,
    now: Instant,
) -> StyleDesc {
    let mut after = fold_state_styles(style, focus_visible, pointer);
    if let Some(running) = transitions.update(id, &after, motion, now) {
        running.apply_to(&mut after);
    }
    after
}

fn fold_state_styles(
    style: &StyleDesc,
    focus_visible: bool,
    (hovered, pressed): (bool, bool),
) -> StyleDesc {
    use crate::events::state_styles::resolve_state_styles;

    let focused = match style.focus_visible.as_deref() {
        // `focusVisible` rides through the fold as a `hover` of its own, so the
        // fold's field rules apply to it; the real `hover` and `active` come
        // back for the second pass.
        Some(declared) if focus_visible => {
            let mut carrier = style.clone();
            carrier.hover = Some(Box::new(declared.clone()));
            carrier.active = None;
            let mut folded = resolve_state_styles(&carrier, true, false);
            folded.hover = style.hover.clone();
            folded.active = style.active.clone();
            folded
        }
        _ => style.clone(),
    };
    let mut after = resolve_state_styles(&focused, hovered, pressed);
    after.focus_visible = None;
    after
}

/// What an animation starts from when `initial` does not name the property:
/// the element's own style, else CSS's initial value. `None` when that is
/// `auto` or inherited, which only layout can resolve.
fn resolve_start(prop: AnimProp, base: Option<&StyleDesc>) -> Option<AnimValue> {
    base.and_then(|style| prop.read(style))
        .or_else(|| prop.initial(base))
}

fn mount_tracks(
    description: &Description,
    base: Option<&StyleDesc>,
    now: Instant,
) -> Result<BTreeMap<AnimProp, Track>, String> {
    let mut tracks = BTreeMap::new();
    for (prop, target) in &description.animate {
        let from = match &description.initial {
            None => target.clone(),
            Some(initial) => initial
                .get(prop)
                .cloned()
                .or_else(|| resolve_start(*prop, base))
                .unwrap_or_else(|| target.clone()),
        };
        let transition = description.transition.for_prop(*prop).clone();
        tracks.insert(
            *prop,
            Track::new(prop.name(), &from, target.clone(), transition, now)?,
        );
    }
    // A property only `initial` names is applied and then left alone.
    for (prop, value) in description.initial.iter().flatten() {
        if !tracks.contains_key(prop) {
            tracks.insert(
                *prop,
                Track::new(prop.name(), value, value.clone(), Transition::default(), now)?,
            );
        }
    }
    Ok(tracks)
}

fn source_generation(source: &Value) -> u64 {
    source
        .get("generation")
        .and_then(Value::as_u64)
        .unwrap_or_default()
}

fn source_is_exit(source: &Value) -> bool {
    source
        .get("isExit")
        .and_then(Value::as_bool)
        .unwrap_or_default()
}

fn parse_description(source: &Value) -> Result<Description, String> {
    let fields = source
        .as_object()
        .ok_or_else(|| "motion description must be an object".to_string())?;

    let generation = match fields.get("generation") {
        None | Some(Value::Null) => 0,
        Some(value) => value
            .as_u64()
            .ok_or_else(|| "motion generation must be a non-negative integer".to_string())?,
    };
    let is_exit = match fields.get("isExit") {
        None | Some(Value::Null) => false,
        Some(value) => value
            .as_bool()
            .ok_or_else(|| "motion isExit must be a boolean".to_string())?,
    };
    let initial = match fields.get("initial") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => None,
        Some(Value::Object(style)) => Some(parse_styles("initial", style)?),
        Some(_) => return Err("motion initial only accepts false or a style object".to_string()),
    };
    let animate = match fields.get("animate") {
        Some(Value::Object(style)) => parse_styles("animate", style)?,
        _ => return Err("motion animate must be a style object".to_string()),
    };
    Ok(Description {
        generation,
        is_exit,
        initial,
        animate,
        transition: Transitions::parse(fields.get("transition"))?,
    })
}

/// Expands shorthands (`padding`, `margin`, `borderRadius`, `borderWidth`,
/// `gap`) into longhands. Where both are given the longhand wins, as the more
/// specific one does in CSS.
fn parse_styles(context: &str, style: &Map<String, Value>) -> Result<Styles, String> {
    let mut styles = Styles::new();
    let mut longhands = Vec::new();
    for (key, value) in style {
        if let Some(props) = property::expand_shorthand(key) {
            let parsed = props[0].parse(value).map_err(|error| rename(error, props[0], key))?;
            for prop in props {
                styles.insert(*prop, parsed.clone());
            }
        } else if let Some(prop) = AnimProp::from_name(key) {
            longhands.push((prop, value));
        } else {
            return Err(format!(
                "motion {context} cannot animate {key}: it is not an animatable property"
            ));
        }
    }
    for (prop, value) in longhands {
        styles.insert(prop, prop.parse(value)?);
    }
    Ok(styles)
}

/// A shorthand is validated through its first longhand, so name the shorthand
/// the author wrote in the error.
fn rename(error: String, prop: AnimProp, shorthand: &str) -> String {
    error.replacen(prop.name(), shorthand, 1)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::style::DimensionValue;

    /// The values a frame would lay over the element's style.
    struct Applied {
        width: Option<f64>,
        opacity: Option<f64>,
    }

    trait AppliedStyle {
        fn applied(&self, now: Instant) -> Option<Applied>;
    }

    impl AppliedStyle for MotionState {
        fn applied(&self, now: Instant) -> Option<Applied> {
            let mut style = StyleDesc::default();
            self.visible_style(now)?.apply_to(&mut style);
            Some(Applied {
                width: style.width.map(|width| match width {
                    DimensionValue::Pixels(width) => width,
                    other => panic!("expected px, got {other:?}"),
                }),
                opacity: style.opacity,
            })
        }
    }

    #[test]
    fn interpolates_and_retargets_from_the_visible_value() {
        let started = Instant::now();
        let initial = serde_json::json!({
            "initial": { "width": 0.0 },
            "animate": { "width": 100.0 },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        let mut state = MotionState::new(&initial, None, started).unwrap();

        let middle = state.frame(started + Duration::from_millis(500));
        assert_eq!(
            state
                .applied(started + Duration::from_millis(500))
                .unwrap()
                .width,
            Some(50.0)
        );
        assert!(middle.active);
        assert!(!middle.just_settled);

        let reversed = serde_json::json!({
            "initial": false,
            "animate": { "width": 0.0 },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        let reversed_at = started + Duration::from_millis(500);
        state.sync(&reversed, None, reversed_at).unwrap();
        assert_eq!(state.applied(reversed_at).unwrap().width, Some(50.0));
        assert_eq!(
            state
                .applied(reversed_at + Duration::from_millis(500))
                .unwrap()
                .width,
            Some(25.0)
        );
    }

    #[test]
    fn disabled_initial_state_starts_at_the_target() {
        let now = Instant::now();
        let description = serde_json::json!({
            "initial": false,
            "animate": { "width": 260.0 },
            "transition": { "duration": 0.2 }
        });
        let mut state = MotionState::new(&description, None, now).unwrap();
        let frame = state.frame(now);

        assert_eq!(state.applied(now).unwrap().width, Some(260.0));
        assert!(!frame.active);
        assert!(!frame.just_settled);
    }

    #[test]
    fn rejects_unsafe_numbers_and_invalid_initial_booleans() {
        let now = Instant::now();
        for description in [
            serde_json::json!({ "animate": { "width": 1e300 }, "transition": {} }),
            serde_json::json!({ "animate": { "opacity": 2.0 }, "transition": {} }),
            serde_json::json!({ "animate": {}, "transition": { "duration": 1e300 } }),
            serde_json::json!({ "initial": true, "animate": {}, "transition": {} }),
        ] {
            assert!(MotionState::new(&description, None, now).is_err());
        }
    }

    #[test]
    fn finishes_at_the_exact_target() {
        let started = Instant::now();
        let description = serde_json::json!({
            "initial": { "width": 0.0 },
            "animate": { "width": 100.0 },
            "transition": { "duration": 0.2, "ease": "linear" }
        });
        let mut state = MotionState::new(&description, None, started).unwrap();
        let frame = state.frame(started + Duration::from_millis(200));

        assert_eq!(
            state
                .applied(started + Duration::from_millis(200))
                .unwrap()
                .width,
            Some(100.0)
        );
        assert!(!frame.active);
        assert!(frame.just_settled);
        assert!(
            !state
                .frame(started + Duration::from_millis(201))
                .just_settled
        );
    }

    #[test]
    fn retarget_with_zero_duration_settles_on_the_next_frame() {
        let started = Instant::now();
        let initial = serde_json::json!({
            "initial": false,
            "animate": { "opacity": 1.0 },
            "transition": { "duration": 0.2, "ease": "linear" }
        });
        let mut state = MotionState::new(&initial, None, started).unwrap();
        assert!(!state.frame(started).just_settled);

        let exit = serde_json::json!({
            "initial": false,
            "animate": { "opacity": 0.0 },
            "transition": { "duration": 0.0, "ease": "linear" }
        });
        state.sync(&exit, None, started).unwrap();
        let frame = state.frame(started);
        assert_eq!(state.applied(started).unwrap().opacity, Some(0.0));
        assert!(!frame.active);
        assert!(frame.just_settled);
    }

    #[test]
    fn a_new_generation_settles_when_the_target_already_matches() {
        let started = Instant::now();
        let initial = serde_json::json!({
            "generation": 1,
            "initial": false,
            "animate": { "opacity": 1.0 }
        });
        let mut state = MotionState::new(&initial, None, started).unwrap();

        let exit = serde_json::json!({
            "generation": 2,
            "initial": false,
            "animate": { "opacity": 1.0 }
        });
        state.sync(&exit, None, started).unwrap();

        assert!(state.frame(started).just_settled);
        assert!(!state.frame(started).just_settled);
    }

    #[test]
    fn retarget_keeps_values_omitted_from_the_new_target() {
        let started = Instant::now();
        let initial = serde_json::json!({
            "generation": 1,
            "initial": false,
            "animate": { "width": 100.0, "opacity": 1.0 },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        let mut state = MotionState::new(&initial, None, started).unwrap();

        let exit = serde_json::json!({
            "generation": 2,
            "initial": false,
            "animate": { "opacity": 0.0 },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        state.sync(&exit, None, started).unwrap();

        state.frame(started + Duration::from_millis(500));
        let style = state
            .applied(started + Duration::from_millis(500))
            .unwrap();
        assert_eq!(style.width, Some(100.0));
        assert_eq!(style.opacity, Some(0.5));
    }

    fn spring_description(from: f64, to: f64, transition: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "initial": { "width": from },
            "animate": { "width": to },
            "transition": transition
        })
    }

    fn at(started: Instant, seconds: f64) -> Instant {
        started + Duration::from_secs_f64(seconds)
    }

    fn width(state: &MotionState, now: Instant) -> f64 {
        state.tracks[&AnimProp::Width].sample(now).position[0]
    }

    fn width_velocity(state: &MotionState, now: Instant) -> f64 {
        state.tracks[&AnimProp::Width].sample(now).velocity[0]
    }

    #[test]
    fn critically_damped_spring_converges_without_overshoot() {
        let started = Instant::now();
        let description = spring_description(
            0.0,
            100.0,
            serde_json::json!({ "type": "spring", "stiffness": 100.0, "damping": 20.0 }),
        );
        let mut state = MotionState::new(&description, None, started).unwrap();

        let mut previous = 0.0;
        for step in 1..=300 {
            let value = width(&state, at(started, step as f64 * 0.01));
            assert!(value >= previous, "moved backwards at step {step}");
            assert!(value <= 100.0, "overshot at step {step}: {value}");
            previous = value;
        }
        let frame = state.frame(at(started, 3.0));
        assert!(!frame.active);
        assert!(frame.just_settled);
        assert_eq!(
            state.applied(at(started, 3.0)).unwrap().width,
            Some(100.0)
        );
    }

    #[test]
    fn underdamped_spring_overshoots_then_settles() {
        let started = Instant::now();
        let description = spring_description(0.0, 100.0, serde_json::json!({ "type": "spring" }));
        let mut state = MotionState::new(&description, None, started).unwrap();

        let peak = (1..=200)
            .map(|step| width(&state, at(started, step as f64 * 0.005)))
            .fold(0.0, f64::max);
        assert!(
            peak > 100.0,
            "default spring (zeta 0.5) must overshoot, peak {peak}"
        );
        assert!(state.frame(at(started, 0.3)).active);
        let frame = state.frame(at(started, 10.0));
        assert!(frame.just_settled);
        assert_eq!(width(&state, at(started, 10.0)), 100.0);
    }

    #[test]
    fn overdamped_spring_converges_without_overshoot() {
        let started = Instant::now();
        let description = spring_description(
            0.0,
            100.0,
            serde_json::json!({ "type": "spring", "stiffness": 100.0, "damping": 60.0 }),
        );
        let state = MotionState::new(&description, None, started).unwrap();

        let mut previous = 0.0;
        for step in 1..=1000 {
            let value = width(&state, at(started, step as f64 * 0.01));
            assert!(value >= previous - 1e-9);
            assert!(value <= 100.0 + 1e-9);
            previous = value;
        }
        assert!(previous > 99.0);
    }

    #[test]
    fn analytic_velocity_is_the_derivative_of_position() {
        let started = Instant::now();
        for damping in [4.0, 20.0, 60.0] {
            let description = spring_description(
                10.0,
                90.0,
                serde_json::json!({ "type": "spring", "stiffness": 100.0, "damping": damping }),
            );
            let state = MotionState::new(&description, None, started).unwrap();
            let t = 0.15;
            let h = 1e-6;
            let numeric =
                (width(&state, at(started, t + h)) - width(&state, at(started, t - h))) / (2.0 * h);
            let analytic = width_velocity(&state, at(started, t));
            assert!(
                (numeric - analytic).abs() < 1e-3 * analytic.abs().max(1.0),
                "damping {damping}: numeric {numeric}, analytic {analytic}"
            );
        }
    }

    #[test]
    fn retarget_keeps_position_and_velocity_continuous() {
        let started = Instant::now();
        let spring = serde_json::json!({ "type": "spring", "stiffness": 170.0, "damping": 8.0 });
        let mut state =
            MotionState::new(&spring_description(0.0, 100.0, spring.clone()), None, started).unwrap();

        let retarget_at = at(started, 0.12);
        let position = width(&state, retarget_at);
        let velocity = width_velocity(&state, retarget_at);
        assert!(
            velocity > 100.0,
            "test needs a fast-moving spring, got {velocity}"
        );

        let reversed = serde_json::json!({
            "initial": false,
            "animate": { "width": 0.0 },
            "transition": spring
        });
        state.sync(&reversed, None, retarget_at).unwrap();

        assert!((width(&state, retarget_at) - position).abs() < 1e-9);
        assert!((width_velocity(&state, retarget_at) - velocity).abs() < 1e-9);

        let h = 1e-6;
        let forward = (width(&state, at(retarget_at, h)) - position) / h;
        assert!(
            (forward - velocity).abs() < 1e-2 * velocity.abs(),
            "slope after retarget {forward}, before {velocity}"
        );
        // Without the carried velocity the first millisecond would barely move.
        assert!(width(&state, at(retarget_at, 0.01)) > position);
    }

    #[test]
    fn retarget_to_the_same_value_keeps_moving_and_settles() {
        let started = Instant::now();
        let spring = serde_json::json!({ "type": "spring", "stiffness": 170.0, "damping": 26.0 });
        let mut state =
            MotionState::new(&spring_description(0.0, 100.0, spring.clone()), None, started).unwrap();
        let retarget_at = at(started, 0.05);
        let velocity = width_velocity(&state, retarget_at);

        // Changing only the transition makes a new segment aimed at the same
        // value; the element keeps its speed rather than freezing.
        let again = serde_json::json!({
            "initial": false,
            "animate": { "width": 100.0 },
            "transition": { "type": "spring", "stiffness": 171.0, "damping": 26.0 }
        });
        state.sync(&again, None, retarget_at).unwrap();
        assert!((width_velocity(&state, retarget_at) - velocity).abs() < 1e-9);
        assert!(state.frame(retarget_at).active);
        assert!(state.frame(at(retarget_at, 5.0)).just_settled);
    }

    #[test]
    fn duration_and_bounce_match_motion_dom() {
        // Reference positions and velocities for keyframes 0 to 1 from
        // motion-dom 14.1.0 `spring({ duration, bounce, keyframes: [0, 1] })`,
        // `next(t).value` and `velocity(t)` (per second).
        let references: [(f64, f64, [(f64, f64, f64); 3]); 4] = [
            (
                0.08,
                0.0,
                [
                    (0.01, 0.3207513939921435, 42.00413335129555),
                    (0.02, 0.6710675192857651, 26.489237609876266),
                    (0.05, 0.9788951383177866, 2.076116748122251),
                ],
            ),
            (
                0.16,
                0.0,
                [
                    (0.01, 0.11441604611076239, 18.700719662122943),
                    (0.05, 0.7830753102255497, 9.296579632382155),
                    (0.1, 0.9788951383177866, 1.0380583740611256),
                ],
            ),
            (
                0.24,
                0.12,
                [
                    (0.01, 0.051518672971349155, 9.232608987851766),
                    (0.05, 0.572466868115509, 11.711727290983525),
                    (0.1, 0.9252801398646724, 3.238268799252324),
                ],
            ),
            (
                0.5,
                0.25,
                [
                    (0.01, 0.016003156531554708, 3.0483416235364165),
                    (0.05, 0.27106431728307356, 8.157677665622069),
                    (0.1, 0.6578276388570644, 6.5706651461312555),
                ],
            ),
        ];
        let started = Instant::now();
        for (duration, bounce, samples) in references {
            let description = spring_description(
                0.0,
                1.0,
                serde_json::json!({ "type": "spring", "duration": duration, "bounce": bounce }),
            );
            let state = MotionState::new(&description, None, started).unwrap();
            for (time, position, velocity) in samples {
                let now = at(started, time);
                assert!(
                    (width(&state, now) - position).abs() < 1e-6,
                    "duration {duration} bounce {bounce} at {time}s: {} vs {position}",
                    width(&state, now)
                );
                assert!(
                    (width_velocity(&state, now) - velocity).abs() < 1e-4,
                    "duration {duration} bounce {bounce} at {time}s: {} vs {velocity}",
                    width_velocity(&state, now)
                );
            }
        }
    }


    #[test]
    fn spring_settles_once_and_is_idle_afterwards() {
        let started = Instant::now();
        let description = spring_description(
            0.0,
            100.0,
            serde_json::json!({ "type": "spring", "duration": 0.2, "bounce": 0.0 }),
        );
        let mut state = MotionState::new(&description, None, started).unwrap();

        let mut settled_frames = 0;
        let mut last_active = 0;
        for step in 0..=100 {
            let frame = state.frame(at(started, step as f64 * 0.01));
            if frame.active {
                last_active = step;
            }
            if frame.just_settled {
                settled_frames += 1;
            }
        }
        assert_eq!(settled_frames, 1);
        assert!(last_active < 100);
        assert_eq!(
            state.applied(at(started, 1.0)).unwrap().width,
            Some(100.0)
        );
    }

    #[test]
    fn spring_waits_for_its_delay() {
        let started = Instant::now();
        let description = spring_description(
            0.0,
            100.0,
            serde_json::json!({ "type": "spring", "delay": 0.5 }),
        );
        let mut state = MotionState::new(&description, None, started).unwrap();

        assert_eq!(width(&state, at(started, 0.4)), 0.0);
        assert!(state.frame(at(started, 0.4)).active);
        assert!(width(&state, at(started, 0.7)) > 0.0);
    }

    #[test]
    fn spring_output_stays_in_range_but_physics_overshoots() {
        let started = Instant::now();
        let bouncy = serde_json::json!({ "type": "spring", "stiffness": 200.0, "damping": 4.0 });

        let fade_in = serde_json::json!({
            "initial": { "opacity": 0.0 },
            "animate": { "opacity": 1.0 },
            "transition": bouncy
        });
        let state = MotionState::new(&fade_in, None, started).unwrap();
        let mut overshot = false;
        for step in 1..=200 {
            let now = at(started, step as f64 * 0.005);
            overshot |= state.tracks[&AnimProp::Opacity].sample(now).position[0] > 1.0;
            let visible = state.applied(now).unwrap().opacity.unwrap();
            assert!((0.0..=1.0).contains(&visible));
        }
        assert!(overshot);

        let collapse = spring_description(100.0, 0.0, bouncy);
        let state = MotionState::new(&collapse, None, started).unwrap();
        let mut undershot = false;
        for step in 1..=200 {
            let now = at(started, step as f64 * 0.005);
            undershot |= width(&state, now) < 0.0;
            assert!(state.applied(now).unwrap().width.unwrap() >= 0.0);
        }
        assert!(undershot);
    }

    #[test]
    fn tween_to_spring_inherits_the_tween_speed() {
        let started = Instant::now();
        let tween = spring_description(
            0.0,
            100.0,
            serde_json::json!({ "duration": 1.0, "ease": "linear" }),
        );
        let mut state = MotionState::new(&tween, None, started).unwrap();
        let switch_at = at(started, 0.5);
        assert!((width_velocity(&state, switch_at) - 100.0).abs() < 1e-9);

        let spring = serde_json::json!({
            "initial": false,
            "animate": { "width": 200.0 },
            "transition": { "type": "spring", "stiffness": 100.0, "damping": 20.0 }
        });
        state.sync(&spring, None, switch_at).unwrap();
        assert_eq!(width(&state, switch_at), 50.0);
        assert!((width_velocity(&state, switch_at) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn spring_to_tween_drops_the_speed() {
        let started = Instant::now();
        let spring = spring_description(
            0.0,
            100.0,
            serde_json::json!({ "type": "spring", "stiffness": 170.0, "damping": 8.0 }),
        );
        let mut state = MotionState::new(&spring, None, started).unwrap();
        let switch_at = at(started, 0.1);
        let position = width(&state, switch_at);

        let tween = serde_json::json!({
            "initial": false,
            "animate": { "width": 0.0 },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        state.sync(&tween, None, switch_at).unwrap();
        assert_eq!(width(&state, switch_at), position);
        assert_eq!(width(&state, at(switch_at, 0.5)), position / 2.0);
    }

    #[test]
    fn rejects_invalid_spring_transitions() {
        let now = Instant::now();
        for transition in [
            serde_json::json!({ "type": "bouncy" }),
            serde_json::json!({ "type": "spring", "stiffness": 0.0 }),
            serde_json::json!({ "type": "spring", "stiffness": -5.0 }),
            serde_json::json!({ "type": "spring", "damping": 0.0 }),
            serde_json::json!({ "type": "spring", "mass": 0.0 }),
            serde_json::json!({ "type": "spring", "stiffness": 1e300 }),
            serde_json::json!({ "type": "spring", "mass": 1e-300 }),
            serde_json::json!({ "type": "spring", "duration": 0.0, "bounce": 0.2 }),
            serde_json::json!({ "type": "spring", "duration": 11.0 }),
            serde_json::json!({ "type": "spring", "duration": 0.3, "bounce": 1.5 }),
            serde_json::json!({ "type": "spring", "duration": 0.3, "bounce": -0.1 }),
            serde_json::json!({ "type": "spring", "duration": 0.3, "stiffness": 100.0 }),
            serde_json::json!({ "type": "spring", "ease": "linear" }),
            serde_json::json!({ "type": "spring", "restDelta": 0.0 }),
            serde_json::json!({ "type": "spring", "restSpeed": -1.0 }),
            serde_json::json!({ "type": "spring", "delay": -1.0 }),
            serde_json::json!({ "stiffness": 100.0 }),
            serde_json::json!({ "type": "tween", "bounce": 0.2 }),
        ] {
            let description = spring_description(0.0, 1.0, transition.clone());
            assert!(
                MotionState::new(&description, None, now).is_err(),
                "{transition} should be rejected"
            );
        }
    }

    fn style_at(state: &MotionState, now: Instant) -> StyleDesc {
        let mut style = StyleDesc::default();
        state.visible_style(now).unwrap().apply_to(&mut style);
        style
    }

    fn rgba(style: &str) -> [f64; 4] {
        let color = crate::color::parse_color_rgba(style).unwrap();
        [color.r as f64, color.g as f64, color.b as f64, color.a as f64]
    }

    #[test]
    fn each_property_follows_its_own_transition() {
        let started = Instant::now();
        let description = serde_json::json!({
            "initial": { "opacity": 0.0, "paddingTop": 0.0, "marginLeft": 0.0 },
            "animate": { "opacity": 1.0, "paddingTop": 100.0, "marginLeft": 40.0 },
            "transition": {
                "duration": 1.0,
                "ease": "linear",
                "opacity": { "duration": 0.2, "ease": "linear" },
                "paddingTop": { "type": "spring", "stiffness": 100.0, "damping": 20.0 }
            }
        });
        let mut state = MotionState::new(&description, None, started).unwrap();

        let style = style_at(&state, at(started, 0.2));
        assert_eq!(style.opacity, Some(1.0));
        assert_eq!(style.margin_left, Some(DimensionValue::Pixels(8.0)));
        let spring_padding = style.padding_top.unwrap();
        assert!(spring_padding > 0.0 && spring_padding < 100.0);
        assert!(state.frame(at(started, 0.2)).active);

        let style = style_at(&state, at(started, 1.0));
        assert_eq!(style.margin_left, Some(DimensionValue::Pixels(40.0)));
    }

    #[test]
    fn default_beats_top_level_curve_fields() {
        let started = Instant::now();
        let description = serde_json::json!({
            "initial": { "paddingTop": 0.0, "paddingBottom": 0.0 },
            "animate": { "paddingTop": 100.0, "paddingBottom": 100.0 },
            "transition": {
                "duration": 9.0,
                "default": { "duration": 1.0, "ease": "linear" },
                "paddingBottom": { "duration": 2.0, "ease": "linear" }
            }
        });
        let state = MotionState::new(&description, None, started).unwrap();
        let style = style_at(&state, at(started, 0.5));
        assert_eq!(style.padding_top, Some(50.0));
        assert_eq!(style.padding_bottom, Some(25.0));
    }

    #[test]
    fn per_property_delay_is_honoured() {
        let started = Instant::now();
        let description = serde_json::json!({
            "initial": { "opacity": 0.0, "paddingTop": 0.0 },
            "animate": { "opacity": 1.0, "paddingTop": 10.0 },
            "transition": {
                "duration": 1.0,
                "ease": "linear",
                "paddingTop": { "duration": 1.0, "ease": "linear", "delay": 0.5 }
            }
        });
        let state = MotionState::new(&description, None, started).unwrap();
        let style = style_at(&state, at(started, 0.5));
        assert_eq!(style.opacity, Some(0.5));
        assert_eq!(style.padding_top, Some(0.0));
    }

    #[test]
    fn rejects_unknown_and_discrete_keys() {
        let now = Instant::now();
        for description in [
            serde_json::json!({ "animate": { "display": "none" } }),
            serde_json::json!({ "animate": { "bogus": 1.0 } }),
            serde_json::json!({ "initial": { "position": "absolute" }, "animate": {} }),
            serde_json::json!({ "animate": { "opacity": 1.0 }, "transition": { "bogus": {} } }),
            serde_json::json!({ "animate": { "opacity": 1.0 }, "transition": { "opacity": { "default": {} } } }),
            serde_json::json!({ "initial": { "width": 0.0 }, "animate": { "width": "50%" } }),
            serde_json::json!({ "initial": { "width": "auto" }, "animate": { "width": 10.0 } }),
        ] {
            assert!(
                MotionState::new(&description, None, now).is_err(),
                "{description} should be rejected"
            );
        }
    }

    #[test]
    fn a_shorthand_beats_the_corners_of_the_base_style() {
        let started = Instant::now();
        let base = StyleDesc {
            border_top_left_radius: Some(20.0),
            ..Default::default()
        };
        let description = serde_json::json!({
            "initial": { "borderRadius": 0.0 },
            "animate": { "borderRadius": 10.0 },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        let state = MotionState::new(&description, Some(&base), started).unwrap();

        let mut style = base.clone();
        state
            .visible_style(at(started, 0.5))
            .unwrap()
            .apply_to(&mut style);
        assert_eq!(style.border_top_left_radius, Some(5.0));
        assert_eq!(style.border_bottom_right_radius, Some(5.0));
    }

    #[test]
    fn a_longhand_beats_the_shorthand_in_the_same_description() {
        let now = Instant::now();
        let description = serde_json::json!({
            "initial": false,
            "animate": { "paddingLeft": 3.0, "padding": 9.0 }
        });
        let state = MotionState::new(&description, None, now).unwrap();
        let style = style_at(&state, now);
        assert_eq!(style.padding_left, Some(3.0));
        assert_eq!(style.padding_top, Some(9.0));
    }

    #[test]
    fn starts_from_the_elements_own_style_when_initial_omits_the_key() {
        let started = Instant::now();
        let base = StyleDesc {
            padding: Some(40.0),
            width: Some(DimensionValue::Pixels(200.0)),
            ..Default::default()
        };
        let description = serde_json::json!({
            "initial": { "opacity": 0.0 },
            "animate": { "opacity": 1.0, "paddingTop": 0.0, "width": 100.0, "marginTop": 10.0 },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        let state = MotionState::new(&description, Some(&base), started).unwrap();
        let style = style_at(&state, at(started, 0.5));
        // From the shorthand in the base style, the base width, and CSS's
        // initial margin of 0.
        assert_eq!(style.padding_top, Some(20.0));
        assert_eq!(style.width, Some(DimensionValue::Pixels(150.0)));
        assert_eq!(style.margin_top, Some(DimensionValue::Pixels(5.0)));
    }

    #[test]
    fn a_value_with_no_resolvable_start_begins_at_its_target() {
        let started = Instant::now();
        let description = serde_json::json!({
            "initial": { "opacity": 0.0 },
            "animate": { "opacity": 1.0, "height": 80.0, "top": 12.0, "borderColor": "#ff0000" },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        let state = MotionState::new(&description, None, started).unwrap();
        let style = style_at(&state, started);
        assert_eq!(style.height, Some(DimensionValue::Pixels(80.0)));
        assert_eq!(style.top, Some(DimensionValue::Pixels(12.0)));
        assert_eq!(rgba(style.border_color.as_deref().unwrap()), rgba("#ff0000"));
    }

    #[test]
    fn border_colour_starts_from_the_elements_own_colour() {
        let started = Instant::now();
        let base = StyleDesc {
            color: Some("#0000ff".to_string()),
            ..Default::default()
        };
        let description = serde_json::json!({
            "initial": { "opacity": 1.0 },
            "animate": { "borderColor": "#ff0000" },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        let state = MotionState::new(&description, Some(&base), started).unwrap();
        let style = style_at(&state, started);
        assert_eq!(rgba(style.border_color.as_deref().unwrap()), rgba("#0000ff"));
    }

    #[test]
    fn animates_a_colour_through_oklab_and_settles_on_the_target() {
        let started = Instant::now();
        let description = serde_json::json!({
            "initial": { "backgroundColor": "#ff0000" },
            "animate": { "backgroundColor": "#0000ff" },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        let mut state = MotionState::new(&description, None, started).unwrap();

        let middle = style_at(&state, at(started, 0.5));
        let middle = rgba(middle.background_color.as_deref().unwrap());
        let oklab_middle = rgba("oklab(0.539985 0.096200 -0.092840)");
        for (actual, expected) in middle.iter().zip(oklab_middle) {
            assert!((actual - expected).abs() < 1.5 / 255.0, "{middle:?}");
        }

        let frame = state.frame(at(started, 1.0));
        assert!(frame.just_settled);
        let end = style_at(&state, at(started, 1.0));
        let end = rgba(end.background_color.as_deref().unwrap());
        for (actual, expected) in end.iter().zip(rgba("#0000ff")) {
            assert!((actual - expected).abs() < 1.0 / 255.0, "{end:?}");
        }
    }

    #[test]
    fn a_colour_retarget_carries_velocity_per_channel() {
        let started = Instant::now();
        let spring = serde_json::json!({ "type": "spring", "stiffness": 170.0, "damping": 8.0 });
        let description = serde_json::json!({
            "initial": { "backgroundColor": "#ff0000" },
            "animate": { "backgroundColor": "#0000ff" },
            "transition": spring
        });
        let mut state = MotionState::new(&description, None, started).unwrap();
        let track = |state: &MotionState| state.tracks[&AnimProp::BackgroundColor].clone();

        let retarget_at = at(started, 0.1);
        let before = track(&state).sample(retarget_at);
        assert!(before.velocity.iter().take(3).any(|v| v.abs() > 0.5));

        let retargeted = serde_json::json!({
            "initial": false,
            "animate": { "backgroundColor": "#00ff00" },
            "transition": spring
        });
        state.sync(&retargeted, None, retarget_at).unwrap();
        let after = track(&state).sample(retarget_at);
        for channel in 0..4 {
            assert!((after.position[channel] - before.position[channel]).abs() < 1e-9);
            assert!((after.velocity[channel] - before.velocity[channel]).abs() < 1e-9);
        }
        // Moving toward green: the alpha channel was never in motion.
        assert_eq!(after.velocity[3], 0.0);
    }

    #[test]
    fn a_retarget_restarts_only_the_properties_that_changed() {
        let started = Instant::now();
        let description = serde_json::json!({
            "initial": { "width": 0.0, "opacity": 0.0 },
            "animate": { "width": 100.0, "opacity": 1.0 },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        let mut state = MotionState::new(&description, None, started).unwrap();

        let retarget_at = at(started, 0.5);
        let retargeted = serde_json::json!({
            "initial": { "width": 0.0, "opacity": 0.0 },
            "animate": { "width": 100.0, "opacity": 0.0 },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        state.sync(&retargeted, None, retarget_at).unwrap();

        // Width was not retargeted, so it keeps its original timeline.
        assert_eq!(width(&state, at(started, 0.75)), 75.0);
        // Opacity restarted from 0.5 toward 0 over a fresh second.
        let opacity = state.tracks[&AnimProp::Opacity].sample(at(retarget_at, 0.5));
        assert_eq!(opacity.position, vec![0.25]);
    }

    #[test]
    fn a_property_missing_from_the_retarget_keeps_running() {
        let started = Instant::now();
        let description = serde_json::json!({
            "initial": { "width": 0.0 },
            "animate": { "width": 100.0 },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        let mut state = MotionState::new(&description, None, started).unwrap();
        let exit = serde_json::json!({
            "initial": false,
            "animate": { "opacity": 0.0 },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        state.sync(&exit, None, at(started, 0.5)).unwrap();
        assert_eq!(width(&state, at(started, 0.75)), 75.0);
    }

    #[test]
    fn a_newly_named_property_starts_from_the_element_style() {
        let started = Instant::now();
        let base = StyleDesc {
            padding_top: Some(10.0),
            ..Default::default()
        };
        let mut state = MotionState::new(
            &serde_json::json!({ "initial": false, "animate": { "opacity": 1.0 } }),
            Some(&base),
            started,
        )
        .unwrap();
        let next = serde_json::json!({
            "initial": false,
            "animate": { "opacity": 1.0, "paddingTop": 30.0 },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        state.sync(&next, Some(&base), started).unwrap();
        assert_eq!(
            style_at(&state, at(started, 0.5)).padding_top,
            Some(20.0)
        );
    }

    #[test]
    fn applied_padding_stays_non_negative_while_the_spring_overshoots() {
        let started = Instant::now();
        let description = serde_json::json!({
            "initial": { "paddingTop": 100.0 },
            "animate": { "paddingTop": 0.0 },
            "transition": { "type": "spring", "stiffness": 200.0, "damping": 4.0 }
        });
        let state = MotionState::new(&description, None, started).unwrap();
        let mut undershot = false;
        for step in 1..=200 {
            let now = at(started, step as f64 * 0.005);
            undershot |= state.tracks[&AnimProp::PaddingTop].sample(now).position[0] < 0.0;
            assert!(style_at(&state, now).padding_top.unwrap() >= 0.0);
        }
        assert!(undershot);
    }

    #[test]
    fn animates_a_box_shadow_list() {
        let started = Instant::now();
        let description = serde_json::json!({
            "initial": { "boxShadow": [] },
            "animate": { "boxShadow": [
                { "offsetX": 0, "offsetY": 8, "blurRadius": 20, "spreadRadius": 0, "color": "#000000" }
            ] },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        let mut state = MotionState::new(&description, None, started).unwrap();
        let style = style_at(&state, at(started, 0.5));
        let shadows = style.box_shadow.unwrap();
        assert_eq!(shadows.as_slice().len(), 1);
        assert_eq!(shadows.as_slice()[0].offset_y, 4.0);
        assert_eq!(shadows.as_slice()[0].blur_radius, 10.0);
        assert!((rgba(&shadows.as_slice()[0].color)[3] - 0.5).abs() < 0.01);
        assert!(state.frame(at(started, 1.0)).just_settled);
    }
    #[test]
    fn state_styles_fold_in_gpui_order_and_leave_none_behind() {
        let style: StyleDesc = serde_json::from_str(
            r##"{
                "color": "#000000", "opacity": 1,
                "focusVisible": { "color": "#111111", "opacity": 0.9 },
                "hover": { "color": "#222222" },
                "active": { "opacity": 0.5 }
            }"##,
        )
        .unwrap();
        let rest = fold_state_styles(&style, false, (false, false));
        assert_eq!(rest.color.as_deref(), Some("#000000"));

        let focused = fold_state_styles(&style, true, (false, false));
        assert_eq!(focused.color.as_deref(), Some("#111111"));
        assert_eq!(focused.opacity, Some(0.9));

        let both = fold_state_styles(&style, true, (true, true));
        assert_eq!(both.color.as_deref(), Some("#222222"), "hover beats focus-visible");
        assert_eq!(both.opacity, Some(0.5), "active beats focus-visible");

        for folded in [rest, focused, both] {
            assert_eq!((folded.hover, folded.active, folded.focus_visible), (None, None, None));
        }
    }
}
