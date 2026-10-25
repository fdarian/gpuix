//! Native motion tracks resolved during GPUI rendering, outside React.
//!
//! A description (`initial`, `animate`, `transition`) becomes one [`Track`] per
//! animated property. A retarget restarts only the properties whose target or
//! transition changed, so the rest keep running undisturbed.

mod bindings;
pub(crate) mod containing;
mod css_animation;
mod css_transition;
mod curve;
pub(crate) mod effect;
mod inline;
mod keyframes;
mod layout_flip;
mod list;
pub(crate) mod measure;
mod property;
mod svg_attr;
mod track;
mod transform_value;
mod value;
mod values;
pub(crate) mod web_animation;

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use serde_json::{Map, Value};
use web_time::Instant;

use crate::dormant_wake::Wake;
use crate::style::StyleDesc;
pub(crate) use curve::cubic_bezier;
pub(crate) use inline::{InlineStyles, Timeline};
use css_animation::animates_width;
pub(crate) use css_animation::{
    validate_animations, AnimationEvent, CssAnimations, PropTarget, Subject, SvgSource,
};
pub(crate) use keyframes::{validate_keyframes, KeyframesRegistry};
pub(crate) use layout_flip::{drag_value_ids, LayoutEvent, LayoutFlips};
pub(crate) use css_transition::{
    declares_transitions, validate_transitions, CssTransitions, InterpolateSize, Svg, TransitionEvent,
    TransitionStyle,
};
pub(crate) use bindings::{BindingIndex, Demand, MotionValueEvent, MotionValueFrame, MotionValueStore};
pub(crate) use values::{PlaybackInfo, StartTime};

/// The duration and the iteration duration, in seconds, of an animation of a value that holds
/// `from`, found without a renderer.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) fn measure_motion_animation(
    from: &serde_json::Value,
    target: &serde_json::Value,
    transition: Option<&serde_json::Value>,
) -> Result<(f64, f64), String> {
    let info = bindings::measure_animation(from, target, transition)?;
    Ok((info.duration, info.iteration_duration))
}
use css_transition::transitions_width;
use curve::Transition;
use measure::Measurement;
use property::AnimProp;
pub(crate) use property::Scope;
pub(crate) use svg_attr::SvgAttr;
use track::{Goal, Track};
pub(crate) use value::{Colors, Inheritable, ParentValues, INITIAL_FONT_WEIGHT};
use value::{AnimValue, Interpolation};

/// The animated properties' current values, ready to lay over a style.
pub(crate) struct MotionStyle(Vec<(AnimProp, AnimValue)>);

impl MotionStyle {
    pub(crate) fn apply_to(self, style: &mut StyleDesc) {
        for (prop, value) in self.0 {
            prop.write(style, value);
        }
    }
}

/// What a track animates: a style property of a layout element, or an
/// attribute of an SVG shape. One description targets one or the other,
/// decided by the element it sits on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum PropKey {
    Style(AnimProp),
    Svg(SvgAttr),
}

impl PropKey {
    fn name(self) -> &'static str {
        match self {
            Self::Style(prop) => prop.name(),
            Self::Svg(attr) => attr.name(),
        }
    }

    fn from_name(target: Target, name: &str) -> Option<Self> {
        match target {
            Target::Style => AnimProp::from_name(name).map(Self::Style),
            Target::Svg => SvgAttr::from_name(name).map(Self::Svg),
        }
    }

    fn parse(self, value: &Value, colors: &Colors) -> Result<AnimValue, String> {
        match self {
            Self::Style(prop) => prop.parse(value, colors),
            Self::Svg(attr) => attr.parse(value, &colors.element),
        }
    }

    /// [`parse`](Self::parse) for keyframes of a CSS animation.
    fn parse_css(self, value: &Value, colors: &Colors) -> Result<AnimValue, String> {
        match self {
            Self::Style(prop) => prop.parse(value, colors),
            Self::Svg(attr) => attr.parse_css(value, &colors.element),
        }
    }

    /// See [`AnimProp::computed_text`].
    fn computed_text(self, value: &AnimValue) -> Result<String, String> {
        match self {
            Self::Style(prop) => prop.computed_text(value),
            Self::Svg(attr) => attr.computed_text(value),
        }
    }

    fn is_list_value(self, items: &[Value]) -> bool {
        match self {
            Self::Style(prop) => prop.is_list_value(items),
            Self::Svg(attr) => attr.is_list_value(items),
        }
    }

    fn clamp_value(self, value: AnimValue) -> AnimValue {
        match self {
            Self::Style(prop) => prop.clamp_value(value),
            Self::Svg(attr) => attr.clamp_value(value),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Style,
    Svg,
}

/// Where an animation starts from when `initial` does not name a property:
/// the element's own style, or its own attributes.
#[derive(Clone, Copy)]
enum Base<'a> {
    Style(Option<&'a StyleDesc>, &'a Colors),
    Svg(&'a HashMap<String, Value>, &'a Colors),
}

impl<'a> Base<'a> {
    fn target(self) -> Target {
        match self {
            Self::Style(..) => Target::Style,
            Self::Svg(..) => Target::Svg,
        }
    }

    fn colors(self) -> &'a Colors {
        match self {
            Self::Style(_, colors) | Self::Svg(_, colors) => colors,
        }
    }
}

type Styles = BTreeMap<PropKey, AnimValue>;
type Goals = BTreeMap<PropKey, Goal>;

/// Framer's normalised draw keys: how much of the shape's length is stroked
/// (`length`), where the dash starts (`offset`) and the gap after it
/// (`spacing`), each as a fraction of the length.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PathDraw {
    pub length: f64,
    pub offset: f64,
    pub spacing: f64,
}

impl Default for PathDraw {
    fn default() -> Self {
        Self {
            length: 1.0,
            offset: 0.0,
            spacing: 1.0,
        }
    }
}

/// An SVG shape's animated attributes at one instant.
#[derive(Debug, Default)]
pub(crate) struct SvgMotion {
    /// XML attribute name and text, replacing the element's own.
    pub attributes: Vec<(&'static str, String)>,
    /// Present when any of `pathLength`, `pathOffset` or `pathSpacing` animates.
    pub draw: Option<PathDraw>,
}

/// Fields of a transition object that describe one curve. Every other key is
/// `default` or the name of the property it applies to.
const CURVE_KEYS: [&str; 14] = [
    "type",
    "duration",
    "delay",
    "ease",
    "times",
    "repeat",
    "repeatType",
    "repeatDelay",
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
    per_prop: BTreeMap<PropKey, Transition>,
}

impl Transitions {
    /// A property's own transition is used as written, so a `times` or `ease`
    /// list of the wrong length is an error. A shared one only applies to the
    /// properties whose keyframes it fits.
    fn for_prop(&self, prop: PropKey, goal: &Goal) -> Cow<'_, Transition> {
        match self.per_prop.get(&prop) {
            Some(own) => Cow::Borrowed(own),
            None => self.fallback.for_shared_keyframes(goal.keyframe_count()),
        }
    }

    /// `reduced` replaces the transition of `width`, `height` and the insets
    /// with a jump, whatever the description names for it.
    fn parse(source: Option<&Value>, target: Target, reduced: bool) -> Result<Self, String> {
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
        // In written order, so a shorthand and a longhand it covers resolve as in CSS: the later one wins.
        for (key, value) in by_prop {
            let transition = Transition::parse(value)?;
            if let Some(props) = shorthand(target, &key) {
                for prop in props {
                    per_prop.insert(PropKey::Style(*prop), transition.clone());
                }
            } else if let Some(prop) = PropKey::from_name(target, &key) {
                per_prop.insert(prop, transition);
            } else {
                return Err(format!("unknown motion transition key: {key}"));
            }
        }
        if reduced && target == Target::Style {
            for prop in AnimProp::ALL.iter().filter(|prop| prop.is_positional()) {
                per_prop.insert(PropKey::Style(*prop), Transition::instant());
            }
        }
        Ok(Self { fallback, per_prop })
    }
}

fn shorthand(target: Target, name: &str) -> Option<&'static [AnimProp]> {
    match target {
        Target::Style => property::expand_shorthand(name),
        Target::Svg => None,
    }
}

/// What the element mounts at, which `initial` decides.
enum Initial {
    /// No `initial`: every property of `animate` starts from the element's own style,
    /// as Motion for React reads it from the DOM, and animates on mount.
    Unset,
    /// `initial: false`: nothing animates on mount, every property is at its last keyframe.
    Blocked,
    /// Properties not named here start as they do for [`Initial::Unset`].
    Style(Styles),
}

impl Initial {
    fn named(&self) -> Option<&Styles> {
        match self {
            Initial::Style(initial) => Some(initial),
            Initial::Unset | Initial::Blocked => None,
        }
    }
}

struct Description {
    generation: u64,
    is_exit: bool,
    initial: Initial,
    animate: Goals,
    transition: Transitions,
    /// `whileHover`, `whileTap` and `whileFocus`: what the element animates to while a
    /// gesture is on it.
    gestures: Vec<GestureTarget>,
    /// `inView`: `whileInView` is on.
    pub in_view: bool,
}

/// What Framer's `while*` props of a `motion.*` answer to, in the order of its `variantPriorityOrder`
/// (`animate`, `whileInView`, `whileFocus`, `whileHover`, `whileTap`): a later one wins a property it
/// shares with an earlier one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Gesture {
    InView,
    Focus,
    Hover,
    Tap,
}

/// Which gestures are on an element now. Hover is `:hover`, tap a primary press that began on
/// it and focus `:focus-visible`: the states its `hover`, `active` and `focusVisible` styles
/// answer to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Gestures {
    pub hover: bool,
    pub tap: bool,
    pub focus: bool,
    /// Whether the element is in view, which JS finds with an `IntersectionObserver` and
    /// writes into the description as `inView`.
    pub in_view: bool,
}

impl Gestures {
    fn has(self, gesture: Gesture) -> bool {
        match gesture {
            Gesture::InView => self.in_view,
            Gesture::Hover => self.hover,
            Gesture::Tap => self.tap,
            Gesture::Focus => self.focus,
        }
    }
}

/// The gestures on an element and on the ancestor that controls its variants: a target that
/// names a variant its controller's `while*` names answers to the controller's gesture, as in
/// Framer, where the variant label goes down to the children.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct GestureStates {
    pub own: Gestures,
    pub controller: Gestures,
}

impl GestureStates {
    fn has(self, target: &GestureTarget) -> bool {
        // JS watches the viewport of whichever element owns the `whileInView` and writes the
        // answer into this element's description, so there is only the one.
        match (target.gesture, target.source) {
            (Gesture::InView, _) | (_, GestureSource::Own) => self.own.has(target.gesture),
            (_, GestureSource::Controller) => self.controller.has(target.gesture),
        }
    }
}

/// Whose gesture a target answers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GestureSource {
    Own,
    Controller,
}

/// What one `while*` prop animates to, with the transition it carries, if it has one.
#[derive(Clone, Debug)]
struct GestureTarget {
    gesture: Gesture,
    source: GestureSource,
    goals: Goals,
    transition: Option<Transitions>,
}

/// What a description animates to with `active` gestures on: `animate`, then the target of
/// each active gesture over it, in priority order. A property a gesture moved that nothing
/// asks for any more goes back to the value the element has without it, which is its own
/// style: Framer animates it back, so `tracked` says which of them have been moved.
fn effective_goals(
    animate: &Goals,
    transition: &Transitions,
    gestures: &[GestureTarget],
    active: GestureStates,
    tracked: &dyn Fn(PropKey) -> bool,
    base: Base,
) -> (Goals, Transitions) {
    let mut goals = animate.clone();
    let mut transitions = transition.clone();
    let mut ordered: Vec<&GestureTarget> = gestures.iter().collect();
    ordered.sort_by_key(|target| target.gesture);
    for target in ordered.iter().filter(|target| active.has(target)) {
        for (key, goal) in &target.goals {
            goals.insert(*key, goal.clone());
            match &target.transition {
                Some(own) => {
                    transitions.per_prop.insert(*key, own.for_prop(*key, goal).into_owned());
                }
                None => {
                    transitions.per_prop.remove(key);
                    if let Some(own) = transition.per_prop.get(key) {
                        transitions.per_prop.insert(*key, own.clone());
                    }
                }
            }
        }
    }
    for key in gestures.iter().flat_map(|target| target.goals.keys()) {
        if !goals.contains_key(key) && tracked(*key) {
            if let Some(rest) = resolve_start(*key, base) {
                goals.insert(*key, Goal::single(rest));
            }
        }
    }
    (goals, transitions)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MotionFrame {
    pub active: bool,
    pub just_settled: bool,
    pub generation: u64,
}

/// An animation that cannot begin until its element has been measured: it
/// starts from the laid-out value, or runs to `height: auto`. The measurement
/// only fixes the numbers; the clock started when the style changed, as it does
/// for a px to px animation and in a browser, so the frame that resolves it is
/// already one frame in.
///
/// An element that was not measured by the frame painted after the animation
/// was set is not painted at all (inside a `display: none` subtree, say), so
/// the animation jumps to its target instead of asking for frames forever.
#[derive(Clone, Debug)]
struct Pending {
    /// `None` starts from layout.
    from: Option<AnimValue>,
    goal: Goal,
    transition: Transition,
    /// The count of painted frames when the animation was set.
    painted: u64,
    /// The frame clock when the animation was set: the resolved track starts here.
    started: Instant,
}

fn motion_description(tree: &crate::retained_tree::RetainedTree, id: u64) -> Option<&Value> {
    tree.elements.get(&id)?.custom_props().get("motion")
}

/// The nearest ancestor whose `motion` hands variant names down (`controls`), which is where
/// the gestures of a target that came from a variant label are read. Mirrors the React and Solid
/// variant context, which nests the same way.
pub(crate) fn gesture_controller(tree: &crate::retained_tree::RetainedTree, id: u64) -> Option<u64> {
    let inherits = motion_description(tree, id)?
        .get("inherit")
        .and_then(Value::as_array)
        .is_some_and(|names| !names.is_empty());
    if !inherits {
        return None;
    }
    let mut ancestor = tree.elements.get(&id)?.parent;
    while let Some(candidate) = ancestor {
        if motion_description(tree, candidate).is_some_and(|motion| motion.get("controls").is_some()) {
            return Some(candidate);
        }
        ancestor = tree.elements.get(&candidate)?.parent;
    }
    None
}

/// Whether `name` (`whileHover`, `whileTap`) is a gesture the element has a target for or hands
/// to its children, so that the pointer entering or leaving it has to be seen by the motion.
pub(crate) fn listens_for_gesture(tree: &crate::retained_tree::RetainedTree, id: u64, name: &str) -> bool {
    motion_description(tree, id).is_some_and(|motion| {
        motion.get(name).is_some_and(|target| !target.is_null())
            || motion
                .get("controls")
                .and_then(Value::as_array)
                .is_some_and(|names| names.iter().any(|controlled| controlled.as_str() == Some(name)))
    })
}

/// What an element's animations start from when a gesture changes, as `Base` is for the
/// description.
#[derive(Clone, Copy)]
pub(crate) enum GestureBase<'a> {
    Style(Option<&'a StyleDesc>),
    Svg(&'a HashMap<String, Value>),
}

pub(crate) struct MotionState {
    source: Value,
    /// What the targets were resolved in. A new one resolves them again, so an
    /// appearance change or a new `color` on a parent moves a `currentColor` or
    /// `light-dark()` target.
    colors: Colors,
    tracks: BTreeMap<PropKey, Track>,
    pending: BTreeMap<PropKey, Pending>,
    valid: bool,
    needs_settle: bool,
    generation: u64,
    /// The `while*` targets of the description, and which of them are on.
    gestures: Vec<GestureTarget>,
    active: GestureStates,
    /// What the description animates to without a gesture.
    resting: Goals,
    resting_transition: Option<Transitions>,
}

impl MotionState {
    /// `base` is the element's own style: Framer reads the DOM for the value
    /// an animation starts from when `initial` does not name it.
    pub(crate) fn new(
        source: &Value,
        base: Option<&StyleDesc>,
        colors: Colors,
        measurement: Option<&Measurement>,
        painted: u64,
        now: Instant,
    ) -> Result<Self, String> {
        Self::mount(source, Base::Style(base, &colors), Scope::Element, measurement, painted, now)
    }

    /// The state of an inline box (`<span>`), which only moves what its paragraph
    /// repaints without a layout: a key of any other property is an error, not a
    /// no-op ([`Scope::InlineBox`]).
    pub(crate) fn new_inline(
        source: &Value,
        base: Option<&StyleDesc>,
        colors: Colors,
        now: Instant,
    ) -> Result<Self, String> {
        Self::mount(source, Base::Style(base, &colors), Scope::InlineBox, None, 0, now)
    }

    /// The state of an SVG shape, whose animated keys are attributes.
    /// `attributes` are the element's props: Framer reads the DOM attribute
    /// for the value an animation starts from when `initial` does not name it.
    pub(crate) fn new_svg(
        source: &Value,
        attributes: &HashMap<String, Value>,
        colors: Colors,
        now: Instant,
    ) -> Result<Self, String> {
        Self::mount(source, Base::Svg(attributes, &colors), Scope::Element, None, 0, now)
    }

    fn mount(
        source: &Value,
        base: Base,
        scope: Scope,
        measurement: Option<&Measurement>,
        painted: u64,
        now: Instant,
    ) -> Result<Self, String> {
        let description = parse_description(source, base.target(), base.colors())?;
        check_scope(&description, scope)?;
        let (tracks, pending) = mount_tracks(&description, base, measurement, painted, now)?;
        Ok(Self {
            source: source.clone(),
            colors: base.colors().clone(),
            needs_settle: description.is_exit
                || !pending.is_empty()
                || tracks.values().any(Track::has_motion),
            tracks,
            pending,
            valid: true,
            generation: description.generation,
            resting: description.animate,
            resting_transition: Some(description.transition),
            gestures: description.gestures,
            active: GestureStates::default(),
        })
    }

    pub(crate) fn invalid(source: &Value, colors: Colors, _now: Instant) -> Self {
        Self {
            source: source.clone(),
            colors,
            tracks: BTreeMap::new(),
            pending: BTreeMap::new(),
            valid: false,
            needs_settle: source_is_exit(source),
            generation: source_generation(source),
            resting: Goals::new(),
            resting_transition: None,
            gestures: Vec::new(),
            active: GestureStates::default(),
        }
    }

    pub(crate) fn sync(
        &mut self,
        source: &Value,
        base: Option<&StyleDesc>,
        colors: Colors,
        measurement: Option<&Measurement>,
        painted: u64,
        now: Instant,
    ) -> Result<(), String> {
        self.retarget(source, Base::Style(base, &colors), Scope::Element, measurement, painted, now)
    }

    pub(crate) fn sync_inline(
        &mut self,
        source: &Value,
        base: Option<&StyleDesc>,
        colors: Colors,
        now: Instant,
    ) -> Result<(), String> {
        self.retarget(source, Base::Style(base, &colors), Scope::InlineBox, None, 0, now)
    }

    pub(crate) fn sync_svg(
        &mut self,
        source: &Value,
        attributes: &HashMap<String, Value>,
        colors: Colors,
        now: Instant,
    ) -> Result<(), String> {
        self.retarget(source, Base::Svg(attributes, &colors), Scope::Element, None, 0, now)
    }

    fn retarget(
        &mut self,
        source: &Value,
        base: Base,
        scope: Scope,
        measurement: Option<&Measurement>,
        painted: u64,
        now: Instant,
    ) -> Result<(), String> {
        self.resolve_pending(measurement, painted, now)?;
        if self.source == *source && self.colors == *base.colors() {
            return self.follow_layout(measurement, now);
        }
        let previous_generation = self.generation;
        self.colors = base.colors().clone();
        let mut description = match parse_description(source, base.target(), base.colors())
            .and_then(|description| check_scope(&description, scope).map(|()| description))
        {
            Ok(description) => description,
            Err(error) => {
                self.source = source.clone();
                self.valid = false;
                self.pending.clear();
                self.generation = source_generation(source);
                self.needs_settle =
                    source_is_exit(source) || self.generation != previous_generation;
                return Err(error);
            }
        };

        let resting = (description.animate.clone(), description.transition.clone());
        if description.gestures.is_empty() && !self.gestures.is_empty() {
            // The gestures are gone (the element is leaving, or the props dropped them): what they
            // moved goes back to what the description asks for or the element has.
            let tracked = |key: PropKey| self.tracks.contains_key(&key) || self.pending.contains_key(&key);
            let (goals, transitions) = effective_goals(
                &description.animate,
                &description.transition,
                &self.gestures,
                GestureStates::default(),
                &tracked,
                base,
            );
            description.animate = goals;
            description.transition = transitions;
        }
        if !description.gestures.is_empty() {
            let tracked = |key: PropKey| self.tracks.contains_key(&key) || self.pending.contains_key(&key);
            self.active.own.in_view = description.in_view;
            let (goals, transitions) = effective_goals(
                &description.animate,
                &description.transition,
                &description.gestures,
                self.active,
                &tracked,
                base,
            );
            description.animate = goals;
            description.transition = transitions;
        }
        let resolved = if self.valid {
            self.retarget_tracks(&description, base, measurement, painted, now)
        } else {
            mount_tracks(&description, base, measurement, painted, now).map(|(tracks, pending)| {
                let moving = !pending.is_empty() || tracks.values().any(Track::has_motion);
                (tracks, pending, moving)
            })
        };
        let (tracks, pending, moving) = match resolved {
            Ok(resolved) => resolved,
            Err(error) => {
                self.source = source.clone();
                self.valid = false;
                self.pending.clear();
                self.generation = source_generation(source);
                self.needs_settle =
                    source_is_exit(source) || self.generation != previous_generation;
                return Err(error);
            }
        };

        self.needs_settle =
            description.is_exit || description.generation != previous_generation || moving;
        self.tracks = tracks;
        self.pending = pending;
        self.source = source.clone();
        self.valid = true;
        self.generation = description.generation;
        self.resting = resting.0;
        self.resting_transition = Some(resting.1);
        self.gestures = description.gestures;
        Ok(())
    }

    /// Sets which gestures are on the element. One that changed sends the properties its
    /// target moves to the value it asks for, or back to the one the element has without it.
    pub(crate) fn set_gestures(
        &mut self,
        active: GestureStates,
        base: GestureBase,
        measurement: Option<&Measurement>,
        painted: u64,
        now: Instant,
    ) -> Result<(), String> {
        let active = GestureStates { own: Gestures { in_view: self.active.own.in_view, ..active.own }, ..active };
        let previous = std::mem::replace(&mut self.active, active);
        let Some(resting_transition) = self.resting_transition.as_ref().filter(|_| self.valid) else {
            return Ok(());
        };
        if previous == active || self.gestures.is_empty() {
            return Ok(());
        }
        let colors = self.colors.clone();
        let base = match base {
            GestureBase::Style(style) => Base::Style(style, &colors),
            GestureBase::Svg(attributes) => Base::Svg(attributes, &colors),
        };
        let tracked = |key: PropKey| self.tracks.contains_key(&key) || self.pending.contains_key(&key);
        let (animate, transition) =
            effective_goals(&self.resting, resting_transition, &self.gestures, active, &tracked, base);
        let description = Description {
            generation: self.generation,
            is_exit: false,
            initial: Initial::Unset,
            animate,
            transition,
            gestures: Vec::new(),
            in_view: false,
        };
        let (tracks, pending, moving) = self.retarget_tracks(&description, base, measurement, painted, now)?;
        self.needs_settle |= moving;
        self.tracks = tracks;
        self.pending = pending;
        Ok(())
    }

    /// Keeps a track whose target and transition are unchanged, restarts the
    /// others from where they are. A property the new description leaves out
    /// keeps running toward its old target, as in Framer.
    fn retarget_tracks(
        &self,
        description: &Description,
        base: Base,
        measurement: Option<&Measurement>,
        painted: u64,
        now: Instant,
    ) -> Result<(BTreeMap<PropKey, Track>, BTreeMap<PropKey, Pending>, bool), String> {
        let mut tracks = self.tracks.clone();
        let mut pending = self.pending.clone();
        let mut moving = false;
        for (prop, goal) in &description.animate {
            let transition = description.transition.for_prop(*prop, goal);
            let previous = self.tracks.get(prop);
            let waiting = self.pending.get(prop);
            let unchanged = match (previous, waiting) {
                (Some(track), _) => track.goal() == goal && track.transition() == &*transition,
                (None, Some(waiting)) => waiting.goal == *goal && waiting.transition == *transition,
                (None, None) => false,
            };
            if unchanged {
                continue;
            }
            let start = match (previous, waiting) {
                (Some(track), _) if track.rests_at_auto(now) => Start::Layout,
                (Some(track), _) => Start::Value(track.visible(now)),
                (None, Some(waiting)) => waiting.from.clone().map_or(Start::Layout, Start::Value),
                (None, None) => start_for(*prop, None, base),
            };
            let begun = begin(*prop, start, goal, &transition, measurement, painted, now)?;
            tracks.remove(prop);
            pending.remove(prop);
            match begun {
                Begun::Track(mut next) => {
                    if let Some(previous) = previous {
                        next.carry_velocity(previous, now);
                    }
                    moving |= next.has_motion();
                    tracks.insert(*prop, next);
                }
                Begun::Wait(waiting) => {
                    moving = true;
                    pending.insert(*prop, waiting);
                }
            }
        }
        Ok((tracks, pending, moving))
    }

    /// Starts the animations that were waiting for a measurement once there
    /// is one, or jumps them to their target once a frame was painted without
    /// the element in it.
    fn resolve_pending(
        &mut self,
        measurement: Option<&Measurement>,
        painted: u64,
        now: Instant,
    ) -> Result<(), String> {
        for (prop, waiting) in std::mem::take(&mut self.pending) {
            // A width `auto` is measured by the frame painted after the element
            // asked for it, so a measurement without it waits one more frame.
            let unresolved = match (prop, measurement) {
                (PropKey::Style(style_prop), Some(measurement)) => {
                    !measurement.resolves(style_prop, waiting.goal.has_auto())
                }
                _ => false,
            };
            let start = match (measurement, waiting.from.clone()) {
                (None, _) if painted <= waiting.painted => {
                    self.pending.insert(prop, waiting);
                    continue;
                }
                (Some(_), _) if unresolved && painted <= waiting.painted + 1 => {
                    self.pending.insert(prop, waiting);
                    continue;
                }
                (None, _) => Start::Rest,
                (Some(_), _) if unresolved => Start::Rest,
                (Some(_), Some(from)) => Start::Value(from),
                (Some(_), None) => Start::Layout,
            };
            match begin(
                prop,
                start,
                &waiting.goal,
                &waiting.transition,
                measurement,
                painted,
                waiting.started,
            ) {
                Ok(Begun::Track(track)) => {
                    self.tracks.insert(prop, track);
                }
                Ok(Begun::Wait(_)) => unreachable!("a measurement or a jump never waits"),
                Err(error) => {
                    self.valid = false;
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    /// Keeps the segments that run to a measured number (`height: auto`, a
    /// percentage reached from px) on the measurement as it changes under them.
    fn follow_layout(
        &mut self,
        measurement: Option<&Measurement>,
        now: Instant,
    ) -> Result<(), String> {
        let Some(measurement) = measurement else {
            return Ok(());
        };
        for (key, track) in &mut self.tracks {
            let PropKey::Style(prop) = key else {
                continue;
            };
            if !track.is_measured()
                || !track.sample(now).active
                || !measurement.resolves(*prop, track.goal().has_auto())
            {
                continue;
            }
            let followed = if track.restarts_on_follow() {
                measured_end(*prop, track.target(), measurement)
                    .and_then(|end| track.follow(prop.name(), end, now))
            } else {
                resolve_keyframes(*prop, track.literal(), measurement)
                    .and_then(|resolved| track.follow_keyframes(prop.name(), &resolved))
            };
            match followed {
                Ok(Some(next)) => *track = next,
                Ok(None) => {}
                Err(error) => {
                    self.valid = false;
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    /// Whether a `width` animation to or from `auto` is waiting for the natural
    /// width or following it as it changes.
    fn needs_natural_width(&self, now: Instant) -> bool {
        let key = PropKey::Style(AnimProp::Width);
        self.valid
            && (self.pending.get(&key).is_some_and(|waiting| waiting.goal.has_auto())
                || self.tracks.get(&key).is_some_and(|track| {
                    track.goal().has_auto() && track.is_measured() && track.sample(now).active
                }))
    }

    /// Whether this description animates `prop`, which then is not a CSS
    /// transition's to animate.
    fn owns(&self, key: PropKey) -> bool {
        self.valid && (self.tracks.contains_key(&key) || self.pending.contains_key(&key))
    }

    pub(crate) fn visible_style(&self, now: Instant) -> Option<MotionStyle> {
        self.valid.then(|| {
            let mut values = Vec::new();
            for (key, track) in &self.tracks {
                let PropKey::Style(prop) = key else {
                    continue;
                };
                let sample = track.sample(now);
                // The segment ran to a measured number; once it ends the style
                // goes back to the `auto` or percentage it asked for, which
                // follows layout from then on.
                let value = if track.is_measured() && !sample.active {
                    track.target().clone()
                } else {
                    prop.clamp_value(track.value(&sample.position))
                };
                values.push((*prop, value));
            }
            // Waiting for a measurement, an animation shows where it starts, or
            // nothing when it starts from layout.
            for (key, waiting) in &self.pending {
                if let (PropKey::Style(prop), Some(from)) = (key, &waiting.from) {
                    values.push((*prop, prop.clamp_value(from.clone())));
                }
            }
            MotionStyle(values)
        })
    }

    /// The animated attributes of an SVG shape, ready to lay over its own.
    pub(crate) fn visible_svg(&self, now: Instant) -> Option<SvgMotion> {
        self.valid.then(|| {
            let mut motion = SvgMotion::default();
            let mut draw = PathDraw::default();
            let mut drawing = false;
            for (key, track) in &self.tracks {
                let PropKey::Svg(attr) = key else {
                    continue;
                };
                let position = track.sample(now).position;
                let value = attr.clamp_value(track.value(&position));
                if let Some(name) = attr.attribute() {
                    motion.attributes.push((name, svg_attr::attribute_text(&value)));
                    continue;
                }
                let AnimValue::Number(number) = value else {
                    unreachable!("a draw key is always a number");
                };
                drawing = true;
                match attr {
                    SvgAttr::PathLength => draw.length = number,
                    SvgAttr::PathOffset => draw.offset = number,
                    _ => draw.spacing = number,
                }
            }
            motion.draw = drawing.then_some(draw);
            motion
        })
    }

    /// Whether everything this description animates, or waits to animate, only changes what the
    /// element paints. One that waits for a measurement is a layout animation.
    pub(crate) fn paints_only(&self) -> bool {
        self.pending.is_empty()
            && self.tracks.keys().all(|key| match key {
                PropKey::Style(prop) => prop.paints_only(),
                PropKey::Svg(_) => true,
            })
    }

    /// Whether the `opacity` track is moving at `now`: an animation of the opacity is current.
    pub(crate) fn animates_opacity(&self, now: Instant) -> bool {
        self.valid
            && self
                .tracks
                .get(&PropKey::Style(AnimProp::Opacity))
                .is_some_and(|track| track.sample(now).active)
    }

    /// Whether a track that moves, scales or turns the element is moving at `now`.
    pub(crate) fn animates_transform(&self, now: Instant) -> bool {
        self.valid
            && self.tracks.iter().any(|(key, track)| {
                matches!(key, PropKey::Style(prop) if prop.is_transform()) && track.sample(now).active
            })
    }

    /// Whether a track is still moving at `now`: what it shows differs from one frame to the next.
    pub(crate) fn is_animating(&self, now: Instant) -> bool {
        self.valid && (!self.pending.is_empty() || self.tracks.values().any(|track| track.sample(now).active))
    }

    /// When the motion next needs a sample for something other than the picture of it: its
    /// settling, which is an event. What an animation shows at a time is a function of the time,
    /// so a loop nobody sees is sampled when it is seen.
    pub(crate) fn wake(&self, now: Instant) -> Wake {
        if !self.valid {
            return Wake::Never;
        }
        if !self.pending.is_empty() {
            return Wake::NextFrame;
        }
        Wake::merged(self.tracks.values().map(|track| track.wake(now)))
    }

    pub(crate) fn frame(&mut self, now: Instant) -> MotionFrame {
        let active = self.valid
            && (!self.pending.is_empty() || self.tracks.values().any(|track| track.sample(now).active));
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

/// `id` and everything below it in the tree. What `display: none` takes out of
/// rendering: transitions and animations on all of it end.
pub(super) fn subtree_ids(tree: &crate::retained_tree::RetainedTree, id: u64) -> Vec<u64> {
    let mut ids = Vec::new();
    let mut pending = vec![id];
    while let Some(next) = pending.pop() {
        ids.push(next);
        if let Some(element) = tree.elements.get(&next) {
            pending.extend(&element.children);
        }
    }
    ids
}

/// Whether layout should also measure the width `declared` would have with
/// `width: auto`, which gpuix reads a frame later. It costs an extra layout of
/// the tree, so only an element whose motion, transitions or animations can
/// run on `width` to or from `auto` asks.
///
/// A transition or animation needs the measurement before the style changes,
/// so any that covers `width` asks, but only under `interpolate-size:
/// allow-keywords`: without it a change to or from `auto` is instant.
pub(crate) fn wants_natural_width(
    motion: Option<&MotionState>,
    declared: Option<&StyleDesc>,
    inherited: InterpolateSize,
    keyframes: &KeyframesRegistry,
    now: Instant,
) -> bool {
    if motion.is_some_and(|motion| motion.needs_natural_width(now)) {
        return true;
    }
    let Some(declared) = declared else {
        return false;
    };
    InterpolateSize::declared(declared).unwrap_or(inherited).allows_keywords()
        && (transitions_width(declared) || animates_width(declared, keyframes))
}

/// The state styles `fold_state_styles` can lay over `style`, by the name a
/// validation error gives them. A transition or animation declared in any of
/// them has to take part, or it never runs.
pub(super) fn state_styles(style: &StyleDesc) -> [(&'static str, Option<&StyleDesc>); 14] {
    [
        ("hover", style.hover.as_deref()),
        ("active", style.active.as_deref()),
        ("focusVisible", style.focus_visible.as_deref()),
        ("groupHover", style.group_hover.as_deref()),
        ("groupActive", style.group_active.as_deref()),
        ("groupFocusWithin", style.group_focus_within.as_deref()),
        ("hasHover", style.has_hover.as_deref()),
        ("hasActive", style.has_active.as_deref()),
        ("hasFocus", style.has_focus.as_deref()),
        ("hasFocusVisible", style.has_focus_visible.as_deref()),
        ("disabled", style.disabled.as_deref()),
        ("enabled", style.enabled.as_deref()),
        ("hasDisabled", style.has_disabled.as_deref()),
        ("hasEnabled", style.has_enabled.as_deref()),
    ]
}

/// Which `:focus-visible` refinements of an element apply, for the ones an
/// element applies where it tracks focus. An inline box has them in its
/// `PointerStates` instead.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FocusVisible {
    /// The element itself matches: `focusVisible`.
    pub own: bool,
    /// A strict descendant matches: `hasFocusVisible`.
    pub descendant: bool,
}

/// The style `style` has after the state styles that currently apply (focus,
/// hover, then press, the order GPUI refines them in), with the CSS transitions
/// running on it laid on top (`transitions` is `None` for an element that
/// declares none). The result has no state styles left, so GPUI's own
/// `.hover()` / `.active()` / `.focus_visible()` refinements do not apply them a
/// second time, instantly.
///
/// `before` is the style the element had before it first declared a transition:
/// the change that adds the declaration transitions from it.
///
/// `animations` is the element's CSS animations when it has any: they advance
/// here, as the first half of their frame, because a property they drive starts no
/// transition. `CssAnimations::update` finishes the frame.
pub(crate) fn resolve_declared_style(
    mut transitions: Option<&mut CssTransitions>,
    animations: Option<(&mut CssAnimations, &KeyframesRegistry)>,
    before: Option<&StyleDesc>,
    svg: Svg,
    parent_colors: &crate::color::ColorContext,
    parent_font: &ParentValues,
    pointer: crate::events::state_styles::PointerStates,
    motion: Option<&MotionState>,
    id: u64,
    style: &StyleDesc,
    focus_visible: FocusVisible,
    measurement: Option<&Measurement>,
    interpolate_size: InterpolateSize,
    scope: Scope,
    now: Instant,
) -> DeclaredStyle {
    if let (Some(transitions), Some(before)) = (transitions.as_deref_mut(), before) {
        let before = with_vars(fold_state_styles(before, focus_visible, pointer), parent_font);
        transitions.seed(id, &before, svg, parent_colors, parent_font);
    }
    let mut after = with_vars(fold_state_styles(style, focus_visible, pointer), parent_font);
    let covered = animations
        .map(|(animations, registry)| animations.advance(id, &after, registry, scope, now))
        .unwrap_or_default();
    let running = transitions.and_then(|transitions| {
        transitions.update(
            id, &after, svg, parent_colors, parent_font, motion, measurement, interpolate_size, scope, &covered, now,
        )
    });
    if let Some(running) = &running {
        running.apply_to(&mut after);
    }
    DeclaredStyle {
        style: after,
        transitions: running,
    }
}

/// What [`resolve_declared_style`] resolved.
pub(crate) struct DeclaredStyle {
    pub(crate) style: StyleDesc,
    /// The transitions running on `style`. A CSS animation on the same
    /// properties does not apply: a transition beats an animation in the
    /// cascade.
    pub(crate) transitions: Option<TransitionStyle>,
}

/// `style` with the `var()` of its transform properties substituted from the custom properties the
/// element computes to. Transitions and animations compare and blend computed values, so they are
/// given the style after this: a `--p` that changes is a change of the transform, from the value
/// the last style computed to the one this does.
fn with_vars(style: StyleDesc, parent_font: &ParentValues) -> StyleDesc {
    if !crate::css::transform::reads_vars(&style) {
        return style;
    }
    crate::css::custom_properties::reads::note();
    crate::css::transform::resolve_vars(&style, &parent_font.custom_of(&style)).unwrap_or(style)
}

fn fold_state_styles(
    style: &StyleDesc,
    focus_visible: FocusVisible,
    pointer: crate::events::state_styles::PointerStates,
) -> StyleDesc {
    use crate::events::state_styles::{resolve_pointer_states, PointerStates};

    let applies = (focus_visible.own && style.focus_visible.is_some())
        || (focus_visible.descendant && style.has_focus_visible.is_some());
    let focused = if applies {
        // The focus layers go in first and the pointer ones come back for the
        // second pass, so a hovered or pressed element is above them.
        let mut folded = resolve_pointer_states(
            style,
            PointerStates {
                has_focus_visible: focus_visible.descendant,
                focus_visible: focus_visible.own,
                ..Default::default()
            },
        );
        folded.hover = style.hover.clone();
        folded.active = style.active.clone();
        folded.group_hover = style.group_hover.clone();
        folded.group_active = style.group_active.clone();
        folded.group_focus_within = style.group_focus_within.clone();
        folded.has_hover = style.has_hover.clone();
        folded.has_active = style.has_active.clone();
        folded.has_focus = style.has_focus.clone();
        folded.disabled = style.disabled.clone();
        folded.enabled = style.enabled.clone();
        folded.has_disabled = style.has_disabled.clone();
        folded.has_enabled = style.has_enabled.clone();
        std::borrow::Cow::Owned(folded)
    } else {
        // Borrowed: `resolve_pointer_states` copies the style itself, so cloning
        // it here too paid for a second copy of a ~3.5 kB struct on every frame.
        std::borrow::Cow::Borrowed(style)
    };
    let mut after = resolve_pointer_states(&focused, pointer);
    after.focus_visible = None;
    after.has_focus_visible = None;
    after
}

/// What an animation starts from when `initial` does not name the property:
/// the element's own style, else CSS's initial value. `None` when that is
/// `auto` or inherited, which only layout can resolve.
fn resolve_start(key: PropKey, base: Base) -> Option<AnimValue> {
    match (key, base) {
        (PropKey::Style(prop), Base::Style(style, colors)) => style
            .and_then(|style| prop.read(style, colors))
            .or_else(|| prop.initial(style, colors)),
        (PropKey::Svg(attr), Base::Svg(attributes, colors)) => {
            attr.read(attributes, &colors.element).or_else(|| attr.initial())
        }
        _ => unreachable!("a description only holds keys of its own target"),
    }
}

/// Where a new segment begins.
enum Start {
    Value(AnimValue),
    /// The value layout produced, which a style that leaves the property unset
    /// or `auto` does not state.
    Layout,
    /// No start is known and none can be measured: the property is already at
    /// its target, unless the keyframes name their own first value.
    Jump,
    /// Nothing animates: the property is at its target, keyframes and all.
    /// Mounted without an `initial`, or never painted.
    Rest,
}

enum Begun {
    Track(Track),
    Wait(Pending),
}

/// Where an animation starts when `explicit` (from `initial`) is given or the
/// element's own style says.
fn start_for(key: PropKey, explicit: Option<&AnimValue>, base: Base) -> Start {
    match explicit {
        Some(AnimValue::Auto) => return Start::Layout,
        Some(value) => return Start::Value(value.clone()),
        None => {}
    }
    match resolve_start(key, base) {
        Some(value) => Start::Value(value),
        None if matches!(key, PropKey::Style(prop) if prop.size_axis().is_some()) => Start::Layout,
        None => Start::Jump,
    }
}

/// The px a target that layout has to resolve stands for: `auto` is the
/// natural size along the axis of `prop`, and a length with a percentage (met
/// next to an `auto`, which has no percentage to blend with) a share of the
/// parent's content box. A caller checks `Measurement::resolves` first.
fn measured_end(
    prop: AnimProp,
    target: &AnimValue,
    measurement: &Measurement,
) -> Result<AnimValue, String> {
    match target {
        AnimValue::Auto => measurement
            .natural_px(prop)
            .map(AnimValue::Number)
            .ok_or_else(|| format!("motion cannot resolve {} as auto: its natural size is not measured", prop.name())),
        AnimValue::Percent(_) | AnimValue::Calc { .. } => in_pixels(prop, target, measurement),
        other => unreachable!("{other:?} is not resolved through layout"),
    }
}

/// `value` in px: its percentage resolved against the parent's content box.
fn in_pixels(
    prop: AnimProp,
    value: &AnimValue,
    measurement: &Measurement,
) -> Result<AnimValue, String> {
    let (AnimValue::Percent(_) | AnimValue::Calc { .. }) = value else {
        return Ok(value.clone());
    };
    let (px, fraction) = value.as_length().expect("a percentage is a length");
    measurement
        .percent_px(prop, fraction)
        .map(|share| AnimValue::Number(px + share))
        .ok_or_else(|| {
            format!(
                "motion cannot resolve {} as a percentage: only sizes, margins and insets are measured against the parent",
                prop.name()
            )
        })
}

/// Builds the segment from `start` to `target`, or the wait for the
/// measurement it needs: only `auto` and a start the style leaves to layout
/// need one. Lengths blend as they are, px and percentage on their own channels.
fn begin(
    key: PropKey,
    start: Start,
    goal: &Goal,
    transition: &Transition,
    measurement: Option<&Measurement>,
    painted: u64,
    now: Instant,
) -> Result<Begun, String> {
    if !goal.is_single() {
        return begin_keyframes(key, start, goal, transition, measurement, painted, now);
    }
    let target = goal.last();
    let name = key.name();
    let wait = |from| {
        Begun::Wait(Pending {
            from,
            goal: goal.clone(),
            transition: transition.clone(),
            painted,
            started: now,
        })
    };

    if *target == AnimValue::Auto {
        let PropKey::Style(prop) = key else {
            unreachable!("only a style property is auto");
        };
        let from = match start {
            Start::Value(from) => from,
            // Already `auto`: there is no number to animate from.
            Start::Layout | Start::Jump | Start::Rest => {
                return Ok(Begun::Track(Track::resting_at_auto(transition.clone(), now)));
            }
        };
        let Some(measurement) = measurement.filter(|measurement| measurement.resolves(prop, true)) else {
            return Ok(wait(Some(from)));
        };
        let from = in_pixels(prop, &from, measurement)?;
        let natural = measured_end(prop, target, measurement)?;
        return Track::resolved(name, &from, AnimValue::Auto, natural, transition.clone(), now)
            .map(Begun::Track);
    }

    let from = match start {
        Start::Value(from) => from,
        Start::Jump | Start::Rest => target.clone(),
        Start::Layout => {
            let PropKey::Style(prop) = key else {
                unreachable!("only a style property starts from layout");
            };
            let Some(measurement) = measurement else {
                return Ok(wait(None));
            };
            match measurement.laid_out(prop) {
                Some(size) => AnimValue::Number(size),
                None => target.clone(),
            }
        }
    };
    Track::new(name, &from, target.clone(), transition.clone(), now).map(Begun::Track)
}

/// Keyframes: when one is `auto` the others are measured, and the style goes
/// back to the literal last one when the track ends.
fn begin_keyframes(
    key: PropKey,
    start: Start,
    goal: &Goal,
    transition: &Transition,
    measurement: Option<&Measurement>,
    painted: u64,
    now: Instant,
) -> Result<Begun, String> {
    let name = key.name();
    let wait = |from| {
        Begun::Wait(Pending {
            from,
            goal: goal.clone(),
            transition: transition.clone(),
            painted,
            started: now,
        })
    };
    let last = goal.last();
    let at_end = || {
        let track = if *last == AnimValue::Auto {
            Track::resting_at_auto(transition.clone(), now)
        } else {
            Track::new(name, last, last.clone(), transition.clone(), now)?
        };
        Ok(Begun::Track(track.for_goal(goal.clone())))
    };

    let mut literal: Vec<AnimValue> = Vec::with_capacity(goal.frames().len());
    for frame in goal.frames() {
        let value = match (frame, literal.last(), &start) {
            (_, _, Start::Rest) => return at_end(),
            (Some(value), _, _) => value.clone(),
            (None, Some(previous), _) => previous.clone(),
            (None, None, Start::Value(from)) => from.clone(),
            (None, None, Start::Jump) => return at_end(),
            (None, None, Start::Layout) => {
                let PropKey::Style(prop) = key else {
                    unreachable!("only a style property starts from layout");
                };
                let Some(measurement) = measurement else {
                    return Ok(wait(None));
                };
                match measurement.laid_out(prop) {
                    Some(size) => AnimValue::Number(size),
                    None => return at_end(),
                }
            }
        };
        literal.push(value);
    }

    let has_auto = literal.contains(&AnimValue::Auto);
    let measurement = match key {
        PropKey::Style(prop) => measurement.filter(|measurement| measurement.resolves(prop, has_auto)),
        PropKey::Svg(_) => measurement,
    };
    let resolved = match (key, needs_layout(&literal), measurement) {
        (PropKey::Style(prop), true, Some(measurement)) => {
            resolve_keyframes(prop, &literal, measurement)?
        }
        (_, true, None) => return Ok(wait(literal.first().cloned())),
        _ => literal.clone(),
    };
    Track::keyframed(
        name,
        goal.clone(),
        literal,
        &resolved,
        Interpolation::Oklab,
        transition.clone(),
        now,
    )
    .map(Begun::Track)
}

/// Whether the keyframes have to go through px: `auto` has no number of its
/// own, so a length beside it is a share of the parent's box.
fn needs_layout(literal: &[AnimValue]) -> bool {
    literal.contains(&AnimValue::Auto)
}

/// The keyframes in px where they need it, as `needs_layout` decides.
fn resolve_keyframes(
    prop: AnimProp,
    literal: &[AnimValue],
    measurement: &Measurement,
) -> Result<Vec<AnimValue>, String> {
    if !needs_layout(literal) {
        return Ok(literal.to_vec());
    }
    literal
        .iter()
        .map(|value| match value {
            AnimValue::Auto | AnimValue::Percent(_) | AnimValue::Calc { .. } => {
                measured_end(prop, value, measurement)
            }
            other => Ok(other.clone()),
        })
        .collect()
}

fn mount_tracks(
    description: &Description,
    base: Base,
    measurement: Option<&Measurement>,
    painted: u64,
    now: Instant,
) -> Result<(BTreeMap<PropKey, Track>, BTreeMap<PropKey, Pending>), String> {
    let mut tracks = BTreeMap::new();
    let mut pending = BTreeMap::new();
    for (prop, goal) in &description.animate {
        let start = match &description.initial {
            Initial::Blocked => Start::Rest,
            Initial::Unset => start_for(*prop, None, base),
            Initial::Style(initial) => start_for(*prop, initial.get(prop), base),
        };
        let transition = description.transition.for_prop(*prop, goal);
        match begin(*prop, start, goal, &transition, measurement, painted, now)? {
            Begun::Track(track) => {
                tracks.insert(*prop, track);
            }
            Begun::Wait(waiting) => {
                pending.insert(*prop, waiting);
            }
        }
    }
    // A property only `initial` names is applied and then left alone.
    for (prop, value) in description.initial.named().into_iter().flatten() {
        if !tracks.contains_key(prop) {
            let track = if *value == AnimValue::Auto {
                Track::resting_at_auto(Transition::default(), now)
            } else {
                Track::new(prop.name(), value, value.clone(), Transition::default(), now)?
            };
            tracks.insert(*prop, track);
        }
    }
    Ok((tracks, pending))
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

fn parse_description(
    source: &Value,
    target: Target,
    colors: &Colors,
) -> Result<Description, String> {
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
        None | Some(Value::Null) => Initial::Unset,
        Some(Value::Bool(false)) => Initial::Blocked,
        Some(Value::Object(style)) => Initial::Style(parse_initial(style, target, colors)?),
        Some(_) => return Err("motion initial only accepts false or a style object".to_string()),
    };
    let animate = match fields.get("animate") {
        Some(Value::Object(style)) => parse_goals("animate", style, target, colors)?,
        _ => return Err("motion animate must be a style object".to_string()),
    };
    let reduced = match fields.get("reducedMotion") {
        None | Some(Value::Null) => false,
        Some(value) => value
            .as_bool()
            .ok_or_else(|| "motion reducedMotion must be a boolean".to_string())?,
    };
    let inherited = match fields.get("inherit") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(names)) => names
            .iter()
            .map(|name| name.as_str().ok_or_else(|| "motion inherit must list gesture names".to_string()))
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => return Err("motion inherit must be a list of gesture names".to_string()),
    };
    let mut gestures = Vec::new();
    for (name, gesture) in [
        ("whileInView", Gesture::InView),
        ("whileHover", Gesture::Hover),
        ("whileTap", Gesture::Tap),
        ("whileFocus", Gesture::Focus),
    ] {
        match fields.get(name) {
            None | Some(Value::Null) => {}
            Some(Value::Object(style)) => {
                let mut style = style.clone();
                let own = style.remove("transition").filter(|value| !value.is_null());
                gestures.push(GestureTarget {
                    gesture,
                    source: if inherited.contains(&name) { GestureSource::Controller } else { GestureSource::Own },
                    goals: parse_goals(name, &style, target, colors)?,
                    transition: own
                        .map(|own| Transitions::parse(Some(&own), target, reduced))
                        .transpose()?,
                });
            }
            Some(_) => return Err(format!("motion {name} must be a style object")),
        }
    }
    let in_view = match fields.get("inView") {
        None | Some(Value::Null) => false,
        Some(value) => value
            .as_bool()
            .ok_or_else(|| "motion inView must be a boolean".to_string())?,
    };
    Ok(Description {
        generation,
        is_exit,
        initial,
        animate,
        transition: Transitions::parse(fields.get("transition"), target, reduced)?,
        gestures,
        in_view,
    })
}

/// A property `scope` cannot move would be left at its declared value by
/// whatever paints, so naming it is an error.
fn check_scope(description: &Description, scope: Scope) -> Result<(), String> {
    let named = description
        .initial
        .named()
        .into_iter()
        .flat_map(|initial| initial.keys().map(|key| ("initial", key)))
        .chain(description.animate.keys().map(|key| ("animate", key)))
        .chain(description.gestures.iter().flat_map(|target| {
            let context = match target.gesture {
                Gesture::InView => "whileInView",
                Gesture::Hover => "whileHover",
                Gesture::Tap => "whileTap",
                Gesture::Focus => "whileFocus",
            };
            target.goals.keys().map(move |key| (context, key))
        }));
    for (context, key) in named {
        if let PropKey::Style(prop) = key {
            if !scope.animates(*prop) {
                return Err(format!(
                    "motion {context} cannot animate {} on a span: an inline box moves only {}",
                    prop.name(),
                    Scope::INLINE_BOX_PROPERTIES,
                ));
            }
        }
    }
    Ok(())
}

/// `initial` holds a value per property, never keyframes.
fn parse_initial(
    style: &Map<String, Value>,
    target: Target,
    colors: &Colors,
) -> Result<Styles, String> {
    parse_goals("initial", style, target, colors)?
        .into_iter()
        .map(|(key, goal)| {
            if !goal.is_single() {
                return Err(format!(
                    "motion initial cannot hold keyframes for {}: it is the value the animation starts from",
                    key.name()
                ));
            }
            Ok((key, goal.last().clone()))
        })
        .collect()
}

/// Expands shorthands (`padding`, `margin`, `inset`, `borderRadius`, `borderWidth`,
/// `borderColor`, `gap`) into longhands, a multi-value string giving each its own
/// value. Where both are given the one written later wins, as in CSS.
fn parse_goals(
    context: &str,
    style: &Map<String, Value>,
    target: Target,
    colors: &Colors,
) -> Result<Goals, String> {
    let mut goals = Goals::new();
    for (key, value) in style {
        let split = match target {
            Target::Style => property::split_shorthand(key, value),
            Target::Svg => None,
        };
        if let Some(split) = split {
            let split = split?;
            for (prop, value) in &split.channels {
                let parsed = parse_goal(PropKey::Style(*prop), value, colors)
                    .map_err(|error| split.error(*prop, error))?;
                goals.insert(PropKey::Style(*prop), parsed);
            }
        } else if let Some(prop) = PropKey::from_name(target, key) {
            goals.insert(prop, parse_goal(prop, value, colors)?);
        } else {
            return Err(match target {
                Target::Style => format!(
                    "motion {context} cannot animate {key}: it is not an animatable property"
                ),
                Target::Svg => format!(
                    "motion {context} cannot animate {key}: it is not an animatable SVG attribute"
                ),
            });
        }
    }
    Ok(goals)
}

/// One target, or keyframes when `value` is an array that is not itself one
/// value of a list-valued property. `null` is a wildcard: the value before it,
/// or the value the animation starts from when it comes first.
fn parse_goal(key: PropKey, value: &Value, colors: &Colors) -> Result<Goal, String> {
    let Value::Array(items) = value else {
        return key.parse(value, colors).map(Goal::single);
    };
    if key.is_list_value(items) {
        return key.parse(value, colors).map(Goal::single);
    }
    let frames = items
        .iter()
        .map(|item| match item {
            Value::Null => Ok(None),
            item => key.parse(item, colors).map(Some),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if let [only] = frames.as_slice() {
        return match only {
            Some(only) => Ok(Goal::single(only.clone())),
            None => Err(format!("motion {} keyframes need a value besides null", key.name())),
        };
    }
    Goal::keyframes(key.name(), frames)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::css::length::LinearLength;
    use crate::style::{DimensionValue, IntrinsicKeyword};

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
        let mut state = MotionState::new(&initial, None, Colors::root(), None, 0, started).unwrap();

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
        state.sync(&reversed, None, Colors::root(), None, 0, reversed_at).unwrap();
        assert_eq!(state.applied(reversed_at).unwrap().width, Some(50.0));
        assert_eq!(
            state
                .applied(reversed_at + Duration::from_millis(500))
                .unwrap()
                .width,
            Some(25.0)
        );
    }

    mod wake {
        use super::*;

        fn state(transition: serde_json::Value, animate: serde_json::Value, now: Instant) -> MotionState {
            let description = serde_json::json!({ "initial": { "opacity": 1.0 }, "animate": animate, "transition": transition });
            MotionState::new(&description, None, Colors::root(), None, 0, now).unwrap()
        }

        #[test]
        fn a_timed_animation_wakes_at_its_end() {
            let now = Instant::now();
            let tween = state(serde_json::json!({ "duration": 0.5, "ease": "linear" }), serde_json::json!({ "opacity": 0.25 }), now);
            assert_eq!(tween.wake(now), Wake::At(now + Duration::from_millis(500)));
            assert_eq!(tween.wake(now + Duration::from_millis(200)), Wake::At(now + Duration::from_millis(500)));
            assert_eq!(tween.wake(now + Duration::from_millis(500)), Wake::Never, "it has ended");
        }

        #[test]
        fn the_end_counts_the_delay_and_the_repeats() {
            let now = Instant::now();
            let repeated = state(
                serde_json::json!({ "duration": 0.5, "ease": "linear", "delay": 0.25, "repeat": 2 }),
                serde_json::json!({ "opacity": 0.25 }),
                now,
            );
            assert_eq!(repeated.wake(now), Wake::At(now + Duration::from_millis(1750)));
        }

        #[test]
        fn a_loop_without_a_count_never_wakes() {
            let now = Instant::now();
            let forever = state(
                serde_json::json!({ "duration": 1.0, "ease": "linear", "repeat": "Infinity" }),
                serde_json::json!({ "opacity": [1.0, 0.25, 1.0] }),
                now,
            );
            assert!(forever.is_animating(now));
            assert_eq!(forever.wake(now), Wake::Never);
            assert_eq!(forever.wake(now + Duration::from_secs(90)), Wake::Never);
        }

        #[test]
        fn a_spring_wakes_where_it_comes_to_rest() {
            let now = Instant::now();
            let spring = state(
                serde_json::json!({ "type": "spring", "stiffness": 300.0, "damping": 20.0 }),
                serde_json::json!({ "opacity": 0.25 }),
                now,
            );
            let Wake::At(rest) = spring.wake(now) else {
                panic!("a spring that rests within 20 s says when");
            };
            assert!(rest > now + Duration::from_millis(300) && rest < now + Duration::from_secs(3));
            assert_eq!(spring.wake(now + Duration::from_millis(200)), Wake::At(rest), "it is the same instant later on");
            assert!(spring.is_animating(rest - Duration::from_millis(1)));
            assert!(!spring.is_animating(rest), "a frame at the wake finds it at rest");
            assert_eq!(spring.wake(rest), Wake::Never);
        }

        #[test]
        fn a_spring_waits_out_its_delay_before_it_wakes() {
            let now = Instant::now();
            let transition = |delay: f64| serde_json::json!({ "type": "spring", "stiffness": 300.0, "damping": 20.0, "delay": delay });
            let animate = serde_json::json!({ "opacity": 0.25 });
            let (Wake::At(plain), Wake::At(delayed)) = (
                state(transition(0.0), animate.clone(), now).wake(now),
                state(transition(0.5), animate, now).wake(now),
            ) else {
                panic!("both rest");
            };
            assert!((delayed - plain).abs_diff(Duration::from_millis(500)) < Duration::from_micros(10));
        }

        #[test]
        fn a_spring_that_rests_in_a_scan_step_is_known_to_before_the_step_ends() {
            let now = Instant::now();
            let spring = state(
                serde_json::json!({ "type": "spring", "stiffness": 300.0, "damping": 20.0 }),
                serde_json::json!({ "opacity": 0.25 }),
                now,
            );
            let Wake::At(rest) = spring.wake(now) else {
                panic!("it rests");
            };
            let scanned = spring.tracks.values().next().unwrap().span().end;
            let rested = (rest - now).as_secs_f64();
            assert!(rested <= scanned + 1e-6 && rested > scanned - 1.0 / 60.0, "{rested} against {scanned}");
        }

        #[test]
        fn a_spring_that_does_not_rest_within_the_scan_wakes_at_every_frame() {
            let now = Instant::now();
            let undamped = state(
                serde_json::json!({ "type": "spring", "stiffness": 100.0, "damping": 0.000001 }),
                serde_json::json!({ "opacity": 0.25 }),
                now,
            );
            assert_eq!(undamped.wake(now), Wake::NextFrame);
        }

        #[test]
        fn a_spring_repeated_a_number_of_times_wakes_at_its_end() {
            let now = Instant::now();
            let repeated = state(
                serde_json::json!({ "type": "spring", "stiffness": 300.0, "damping": 20.0, "repeat": 1 }),
                serde_json::json!({ "opacity": 0.25 }),
                now,
            );
            let once = state(
                serde_json::json!({ "type": "spring", "stiffness": 300.0, "damping": 20.0 }),
                serde_json::json!({ "opacity": 0.25 }),
                now,
            );
            let (Wake::At(twice), Wake::At(once)) = (repeated.wake(now), once.wake(now)) else {
                panic!("both end");
            };
            assert!(twice > once + Duration::from_millis(300), "two iterations last longer than one");
            assert!(!repeated.is_animating(twice));
            assert!(repeated.is_animating(twice - Duration::from_millis(1)));
        }

        #[test]
        fn a_motion_with_nothing_to_run_never_wakes() {
            let now = Instant::now();
            let resting = state(serde_json::json!({ "duration": 0.5 }), serde_json::json!({ "opacity": 1.0 }), now);
            assert_eq!(resting.wake(now), Wake::Never);
        }
    }

    #[test]
    fn disabled_initial_state_starts_at_the_target() {
        let now = Instant::now();
        let description = serde_json::json!({
            "initial": false,
            "animate": { "width": 260.0 },
            "transition": { "duration": 0.2 }
        });
        let mut state = MotionState::new(&description, None, Colors::root(), None, 0, now).unwrap();
        let frame = state.frame(now);

        assert_eq!(state.applied(now).unwrap().width, Some(260.0));
        assert!(!frame.active);
        assert!(!frame.just_settled);
    }

    #[test]
    fn a_target_handed_down_answers_to_the_gesture_of_the_controller() {
        let now = Instant::now();
        let later = now + Duration::from_millis(10);
        let description = serde_json::json!({
            "animate": {}, "transition": { "duration": 0.0 },
            "whileHover": { "opacity": 0.5 }, "inherit": ["whileHover"]
        });
        let mut state = MotionState::new(&description, None, Colors::root(), None, 0, now).unwrap();
        let hover = Gestures { hover: true, ..Default::default() };
        let set = |state: &mut MotionState, states: GestureStates| {
            state.set_gestures(states, GestureBase::Style(None), None, 0, now).unwrap();
        };
        set(&mut state, GestureStates { own: hover, controller: Gestures::default() });
        assert_eq!(state.applied(later).and_then(|applied| applied.opacity), None, "its own hover is not the target's");
        set(&mut state, GestureStates { own: Gestures::default(), controller: hover });
        assert_eq!(state.applied(later).unwrap().opacity, Some(0.5));
        set(&mut state, GestureStates::default());
        assert_eq!(state.applied(later).unwrap().opacity, Some(1.0));
    }

    #[test]
    fn dropping_the_gestures_from_the_description_sends_what_they_moved_back() {
        let now = Instant::now();
        let later = now + Duration::from_millis(10);
        let with = serde_json::json!({
            "animate": {}, "transition": { "duration": 0.0 }, "whileHover": { "opacity": 0.5 }
        });
        let mut state = MotionState::new(&with, None, Colors::root(), None, 0, now).unwrap();
        let hover = Gestures { hover: true, ..Default::default() };
        state
            .set_gestures(GestureStates { own: hover, ..Default::default() }, GestureBase::Style(None), None, 0, now)
            .unwrap();
        assert_eq!(state.applied(later).unwrap().opacity, Some(0.5));
        let without = serde_json::json!({ "animate": { "opacity": 0.8 }, "transition": { "duration": 0.0 } });
        state.sync(&without, None, Colors::root(), None, 0, now).unwrap();
        assert_eq!(state.applied(later).unwrap().opacity, Some(0.8));
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
            assert!(MotionState::new(&description, None, Colors::root(), None, 0, now).is_err());
        }
    }

    #[test]
    fn a_length_target_that_is_not_valid_names_the_property_and_the_shorthand() {
        let now = Instant::now();
        for (animate, name, reason) in [
            (serde_json::json!({ "paddingLeft": "wide" }), "paddingLeft", "must be a number or a CSS length"),
            (serde_json::json!({ "padding": "-2em" }), "padding", "must be non-negative"),
            (serde_json::json!({ "gap": "wide" }), "gap", "must be a number or a CSS length"),
            (serde_json::json!({ "borderWidth": "10%" }), "borderWidth", "cannot be a percentage"),
            (serde_json::json!({ "borderTopWidth": "thick" }), "borderTopWidth", "must be a number or a CSS length"),
            (serde_json::json!({ "outlineWidth": "calc(2em + 5%)" }), "outlineWidth", "cannot be a percentage"),
            (serde_json::json!({ "outlineOffset": "5%" }), "outlineOffset", "cannot be a percentage"),
            (serde_json::json!({ "fontSize": "-1px" }), "fontSize", "must be non-negative"),
        ] {
            let description = serde_json::json!({ "animate": animate, "transition": {} });
            let error = MotionState::new(&description, None, Colors::root(), None, 0, now).err().unwrap();
            assert!(error.contains(name) && error.contains(reason), "{error}");
        }
        let lengths = serde_json::json!({
            "animate": {
                "padding": "8px", "gap": "calc(10% + 1em)", "borderWidth": "0.1rem", "outlineOffset": 3,
                "fontSize": "1.5em", "borderRadius": "50%", "flexBasis": "calc(100% - 40px)"
            },
            "transition": {}
        });
        assert!(MotionState::new(&lengths, None, Colors::root(), None, 0, now).is_ok());
    }
    #[test]
    fn finishes_at_the_exact_target() {
        let started = Instant::now();
        let description = serde_json::json!({
            "initial": { "width": 0.0 },
            "animate": { "width": 100.0 },
            "transition": { "duration": 0.2, "ease": "linear" }
        });
        let mut state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
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

    fn reduced_description(reduced: bool) -> serde_json::Value {
        serde_json::json!({
            "generation": 1,
            "reducedMotion": reduced,
            "initial": { "width": 0.0, "paddingLeft": 0.0, "opacity": 0.0 },
            "animate": { "width": 100.0, "paddingLeft": 40.0, "opacity": 1.0 },
            "transition": {
                "duration": 1.0,
                "ease": "linear",
                "width": { "type": "spring", "stiffness": 100.0, "damping": 10.0, "delay": 0.5 }
            }
        })
    }

    #[test]
    fn reduced_motion_jumps_width_but_still_animates_padding_and_opacity() {
        let started = Instant::now();
        let mut state = MotionState::new(&reduced_description(true), None, Colors::root(), None, 0, started).unwrap();
        let mut style = StyleDesc::default();
        state.visible_style(started).unwrap().apply_to(&mut style);
        assert_eq!(style.width, Some(DimensionValue::Pixels(100.0)));
        assert_eq!(style.padding_left, Some(LinearLength::px(0.0)));
        assert_eq!(style.opacity, Some(0.0));

        let middle = started + Duration::from_millis(500);
        let frame = state.frame(middle);
        assert!(frame.active);
        assert_eq!(state.applied(middle).unwrap().opacity, Some(0.5));
        assert_eq!(state.applied(middle).unwrap().width, Some(100.0));
        assert_eq!(style_at(&state, middle).padding_left, Some(LinearLength::px(20.0)));

        let end = started + Duration::from_secs(1);
        let frame = state.frame(end);
        assert!(!frame.active);
        assert!(frame.just_settled);
    }

    #[test]
    fn reduced_motion_on_width_height_and_insets_alone_settles_on_the_first_frame() {
        let started = Instant::now();
        let source = serde_json::json!({
            "generation": 1,
            "reducedMotion": true,
            "initial": { "width": 0.0, "height": 0.0, "top": 0.0, "right": 0.0, "bottom": 0.0, "left": 0.0 },
            "animate": { "width": 10.0, "height": 50.0, "top": 1.0, "right": 2.0, "bottom": 3.0, "left": 4.0 },
            "transition": { "type": "spring" }
        });
        let mut state = MotionState::new(&source, None, Colors::root(), None, 0, started).unwrap();
        let style = style_at(&state, started);
        assert_eq!(style.width, Some(DimensionValue::Pixels(10.0)));
        assert_eq!(style.height, Some(DimensionValue::Pixels(50.0)));
        assert_eq!(style.top, Some(DimensionValue::Pixels(1.0)));
        assert_eq!(style.right, Some(DimensionValue::Pixels(2.0)));
        assert_eq!(style.bottom, Some(DimensionValue::Pixels(3.0)));
        assert_eq!(style.left, Some(DimensionValue::Pixels(4.0)));
        let frame = state.frame(started);
        assert!(!frame.active);
        assert!(frame.just_settled);
        assert_eq!(frame.generation, 1);
    }

    #[test]
    fn reduced_motion_keeps_animating_everything_but_width_height_and_the_insets() {
        let started = Instant::now();
        let source = serde_json::json!({
            "reducedMotion": true,
            "initial": {
                "padding": 0.0, "margin": 0.0, "gap": 0.0, "flexBasis": 0.0, "minWidth": 0.0, "maxHeight": 0.0,
                "opacity": 0.0, "backgroundColor": "#000000",
                "width": 0.0, "height": 0.0, "top": 0.0, "left": 0.0, "right": 0.0, "bottom": 0.0
            },
            "animate": {
                "padding": 10.0, "margin": 10.0, "gap": 10.0, "flexBasis": 10.0, "minWidth": 10.0, "maxHeight": 10.0,
                "opacity": 1.0, "backgroundColor": "#ffffff",
                "width": 10.0, "height": 10.0, "top": 10.0, "left": 10.0, "right": 10.0, "bottom": 10.0
            },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        let state = MotionState::new(&source, None, Colors::root(), None, 0, started).unwrap();
        let middle = style_at(&state, at(started, 0.5));
        let px = |value: Option<LinearLength>| value.unwrap().as_px().unwrap();
        let dimension = |value: Option<DimensionValue>| match value {
            Some(DimensionValue::Pixels(pixels)) => pixels,
            other => panic!("expected px, got {other:?}"),
        };
        for animating in [
            px(middle.padding_top),
            px(middle.padding_left),
            px(middle.row_gap),
            px(middle.column_gap),
            dimension(middle.margin_top),
            dimension(middle.margin_left),
            dimension(middle.flex_basis),
            dimension(middle.min_width),
            dimension(middle.max_height),
        ] {
            assert_eq!(animating, 5.0);
        }
        assert_eq!(middle.opacity, Some(0.5));
        assert_ne!(rgba(middle.background_color.as_deref().unwrap()), rgba("#000000"));
        assert_ne!(rgba(middle.background_color.as_deref().unwrap()), rgba("#ffffff"));
        for jumped in [
            dimension(middle.width),
            dimension(middle.height),
            dimension(middle.top),
            dimension(middle.left),
            dimension(middle.right),
            dimension(middle.bottom),
        ] {
            assert_eq!(jumped, 10.0);
        }
    }

    #[test]
    fn a_transition_naming_the_padding_shorthand_still_animates_under_reduced_motion_and_one_naming_width_still_jumps() {
        let started = Instant::now();
        let source = serde_json::json!({
            "reducedMotion": true,
            "initial": { "padding": 0.0, "gap": 0.0, "width": 0.0 },
            "animate": { "padding": 10.0, "gap": 10.0, "width": 10.0 },
            "transition": {
                "duration": 1.0,
                "ease": "linear",
                "padding": { "duration": 2.0, "ease": "linear" },
                "gap": { "duration": 2.0, "ease": "linear" },
                "width": { "duration": 2.0, "ease": "linear" }
            }
        });
        let state = MotionState::new(&source, None, Colors::root(), None, 0, started).unwrap();
        let middle = style_at(&state, at(started, 1.0));
        assert_eq!(middle.padding_right, Some(LinearLength::px(5.0)));
        assert_eq!(middle.column_gap, Some(LinearLength::px(5.0)));
        assert_eq!(middle.width, Some(DimensionValue::Pixels(10.0)));
    }

    #[test]
    fn flipping_reduced_motion_retargets_running_layout_tracks() {
        let started = Instant::now();
        let mut state = MotionState::new(&reduced_description(false), None, Colors::root(), None, 0, started).unwrap();
        let middle = started + Duration::from_millis(500);
        assert_eq!(state.applied(middle).unwrap().width, Some(0.0));

        let mut reduced = reduced_description(true);
        reduced["generation"] = serde_json::json!(2);
        reduced["initial"] = serde_json::json!(false);
        state.sync(&reduced, None, Colors::root(), None, 0, middle).unwrap();
        assert_eq!(state.applied(middle).unwrap().width, Some(100.0));
        assert_eq!(state.applied(middle).unwrap().opacity, Some(0.5));
        assert!(state.frame(middle).active);

        let mut full = reduced_description(false);
        full["generation"] = serde_json::json!(3);
        full["initial"] = serde_json::json!(false);
        state.sync(&full, None, Colors::root(), None, 0, middle).unwrap();
        // Already at the target, so the real transition has nowhere to go.
        assert_eq!(state.applied(middle).unwrap().width, Some(100.0));
    }

    #[test]
    fn reduced_motion_must_be_a_boolean() {
        let now = Instant::now();
        let source = serde_json::json!({
            "animate": { "opacity": 1.0 },
            "reducedMotion": "user"
        });
        assert!(MotionState::new(&source, None, Colors::root(), None, 0, now).is_err());
    }

    #[test]
    fn retarget_with_zero_duration_settles_on_the_next_frame() {
        let started = Instant::now();
        let initial = serde_json::json!({
            "initial": false,
            "animate": { "opacity": 1.0 },
            "transition": { "duration": 0.2, "ease": "linear" }
        });
        let mut state = MotionState::new(&initial, None, Colors::root(), None, 0, started).unwrap();
        assert!(!state.frame(started).just_settled);

        let exit = serde_json::json!({
            "initial": false,
            "animate": { "opacity": 0.0 },
            "transition": { "duration": 0.0, "ease": "linear" }
        });
        state.sync(&exit, None, Colors::root(), None, 0, started).unwrap();
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
        let mut state = MotionState::new(&initial, None, Colors::root(), None, 0, started).unwrap();

        let exit = serde_json::json!({
            "generation": 2,
            "initial": false,
            "animate": { "opacity": 1.0 }
        });
        state.sync(&exit, None, Colors::root(), None, 0, started).unwrap();

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
        let mut state = MotionState::new(&initial, None, Colors::root(), None, 0, started).unwrap();

        let exit = serde_json::json!({
            "generation": 2,
            "initial": false,
            "animate": { "opacity": 0.0 },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        state.sync(&exit, None, Colors::root(), None, 0, started).unwrap();

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
        state.tracks[&PropKey::Style(AnimProp::Width)].sample(now).position[0]
    }

    fn width_velocity(state: &MotionState, now: Instant) -> f64 {
        state.tracks[&PropKey::Style(AnimProp::Width)].sample(now).velocity[0]
    }

    #[test]
    fn critically_damped_spring_converges_without_overshoot() {
        let started = Instant::now();
        let description = spring_description(
            0.0,
            100.0,
            serde_json::json!({ "type": "spring", "stiffness": 100.0, "damping": 20.0 }),
        );
        let mut state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();

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
        let mut state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();

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
        let state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();

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
            let state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
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
            MotionState::new(&spring_description(0.0, 100.0, spring.clone()), None, Colors::root(), None, 0, started).unwrap();

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
        state.sync(&reversed, None, Colors::root(), None, 0, retarget_at).unwrap();

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
            MotionState::new(&spring_description(0.0, 100.0, spring.clone()), None, Colors::root(), None, 0, started).unwrap();
        let retarget_at = at(started, 0.05);
        let velocity = width_velocity(&state, retarget_at);

        // Changing only the transition makes a new segment aimed at the same
        // value; the element keeps its speed rather than freezing.
        let again = serde_json::json!({
            "initial": false,
            "animate": { "width": 100.0 },
            "transition": { "type": "spring", "stiffness": 171.0, "damping": 26.0 }
        });
        state.sync(&again, None, Colors::root(), None, 0, retarget_at).unwrap();
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
            let state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
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
        let mut state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();

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
        let mut state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();

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
        let state = MotionState::new(&fade_in, None, Colors::root(), None, 0, started).unwrap();
        let mut overshot = false;
        for step in 1..=200 {
            let now = at(started, step as f64 * 0.005);
            overshot |= state.tracks[&PropKey::Style(AnimProp::Opacity)].sample(now).position[0] > 1.0;
            let visible = state.applied(now).unwrap().opacity.unwrap();
            assert!((0.0..=1.0).contains(&visible));
        }
        assert!(overshot);

        let collapse = spring_description(100.0, 0.0, bouncy);
        let state = MotionState::new(&collapse, None, Colors::root(), None, 0, started).unwrap();
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
        let mut state = MotionState::new(&tween, None, Colors::root(), None, 0, started).unwrap();
        let switch_at = at(started, 0.5);
        assert!((width_velocity(&state, switch_at) - 100.0).abs() < 1e-9);

        let spring = serde_json::json!({
            "initial": false,
            "animate": { "width": 200.0 },
            "transition": { "type": "spring", "stiffness": 100.0, "damping": 20.0 }
        });
        state.sync(&spring, None, Colors::root(), None, 0, switch_at).unwrap();
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
        let mut state = MotionState::new(&spring, None, Colors::root(), None, 0, started).unwrap();
        let switch_at = at(started, 0.1);
        let position = width(&state, switch_at);

        let tween = serde_json::json!({
            "initial": false,
            "animate": { "width": 0.0 },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        state.sync(&tween, None, Colors::root(), None, 0, switch_at).unwrap();
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
                MotionState::new(&description, None, Colors::root(), None, 0, now).is_err(),
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
        let mut state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();

        let style = style_at(&state, at(started, 0.2));
        assert_eq!(style.opacity, Some(1.0));
        assert_eq!(style.margin_left, Some(DimensionValue::Pixels(8.0)));
        let spring_padding = style.padding_top.unwrap().as_px().unwrap();
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
        let state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
        let style = style_at(&state, at(started, 0.5));
        assert_eq!(style.padding_top, Some(LinearLength::px(50.0)));
        assert_eq!(style.padding_bottom, Some(LinearLength::px(25.0)));
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
        let state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
        let style = style_at(&state, at(started, 0.5));
        assert_eq!(style.opacity, Some(0.5));
        assert_eq!(style.padding_top, Some(LinearLength::px(0.0)));
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
        ] {
            assert!(
                MotionState::new(&description, None, Colors::root(), None, 0, now).is_err(),
                "{description} should be rejected"
            );
        }
    }

    #[test]
    fn a_shorthand_beats_the_corners_of_the_base_style() {
        let started = Instant::now();
        let base = StyleDesc {
            border_top_left_radius: Some(crate::style::CornerRadiusValue::px(20.0)),
            ..Default::default()
        };
        let description = serde_json::json!({
            "initial": { "borderRadius": 0.0 },
            "animate": { "borderRadius": 10.0 },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        let state = MotionState::new(&description, Some(&base), Colors::root(), None, 0, started).unwrap();

        let mut style = base.clone();
        state
            .visible_style(at(started, 0.5))
            .unwrap()
            .apply_to(&mut style);
        assert_eq!(style.border_top_left_radius, Some(crate::style::CornerRadiusValue::px(5.0)));
        assert_eq!(style.border_bottom_right_radius, Some(crate::style::CornerRadiusValue::px(5.0)));
    }

    fn halfway(animate: serde_json::Value, initial: serde_json::Value) -> StyleDesc {
        let started = Instant::now();
        let description = serde_json::json!({
            "initial": initial,
            "animate": animate,
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        let state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
        style_at(&state, at(started, 0.5))
    }

    fn describe_error(description: serde_json::Value) -> String {
        match MotionState::new(&description, None, Colors::root(), None, 0, Instant::now()) {
            Ok(_) => panic!("{description} was accepted"),
            Err(error) => error,
        }
    }

    #[test]
    fn a_multi_value_shorthand_animates_each_side_to_its_own_value() {
        let style = halfway(
            serde_json::json!({ "padding": "20px 40px", "inset": "10px 30px 50px", "borderWidth": "2px 4px 6px 8px" }),
            serde_json::json!({ "padding": "0 0", "inset": 0, "borderWidth": 0 }),
        );
        assert_eq!(style.padding_top, Some(LinearLength::px(10.0)));
        assert_eq!(style.padding_right, Some(LinearLength::px(20.0)));
        assert_eq!(style.padding_bottom, Some(LinearLength::px(10.0)));
        assert_eq!(style.padding_left, Some(LinearLength::px(20.0)));
        assert_eq!(style.top, Some(DimensionValue::Pixels(5.0)));
        assert_eq!(style.right, Some(DimensionValue::Pixels(15.0)));
        assert_eq!(style.bottom, Some(DimensionValue::Pixels(25.0)));
        assert_eq!(style.left, Some(DimensionValue::Pixels(15.0)));
        assert_eq!(style.border_top_width, Some(LinearLength::px(1.0)));
        assert_eq!(style.border_left_width, Some(LinearLength::px(4.0)));
    }

    #[test]
    fn gap_takes_the_row_then_the_column_and_a_margin_its_four_sides() {
        let style = halfway(
            serde_json::json!({ "gap": "4px 8px", "margin": "0 20px" }),
            serde_json::json!({ "gap": 0, "margin": "10px 0" }),
        );
        assert_eq!(style.row_gap, Some(LinearLength::px(2.0)));
        assert_eq!(style.column_gap, Some(LinearLength::px(4.0)));
        assert_eq!(style.margin_top, Some(DimensionValue::Pixels(5.0)));
        assert_eq!(style.margin_right, Some(DimensionValue::Pixels(10.0)));
    }

    #[test]
    fn a_border_colour_string_gives_each_side_its_colour() {
        let started = Instant::now();
        let description = serde_json::json!({
            "initial": { "borderColor": "#000000 #000000" },
            "animate": { "borderColor": "#ff0000 #0000ff" },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        let state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
        let end = style_at(&state, at(started, 1.0));
        assert_eq!(rgba(end.border_top_color.as_deref().unwrap()), rgba("#ff0000"));
        assert_eq!(rgba(end.border_bottom_color.as_deref().unwrap()), rgba("#ff0000"));
        assert_eq!(rgba(end.border_right_color.as_deref().unwrap()), rgba("#0000ff"));
        assert_eq!(rgba(end.border_left_color.as_deref().unwrap()), rgba("#0000ff"));
    }

    #[test]
    fn a_border_radius_with_a_slash_animates_the_horizontal_and_vertical_radii() {
        let style = halfway(
            serde_json::json!({ "borderRadius": "30px / 40px" }),
            serde_json::json!({ "borderRadius": "10px / 20px" }),
        );
        let ellipse = crate::style::CornerRadiusValue { x: LinearLength::px(20.0), y: Some(LinearLength::px(30.0)) };
        assert_eq!(style.border_top_left_radius, Some(ellipse));
        assert_eq!(style.border_bottom_right_radius, Some(ellipse));

        let style = halfway(
            serde_json::json!({ "borderRadius": "10px 20px 30px 40px / 0px" }),
            serde_json::json!({ "borderRadius": 0 }),
        );
        // Top left, top right, bottom right, bottom left in CSS order; a zero vertical radius is square.
        assert_eq!(
            style.border_top_right_radius,
            Some(crate::style::CornerRadiusValue { x: LinearLength::px(10.0), y: Some(LinearLength::px(0.0)) })
        );
        assert_eq!(style.border_bottom_right_radius.map(|corner| corner.x), Some(LinearLength::px(15.0)));
        assert_eq!(style.border_bottom_left_radius.map(|corner| corner.x), Some(LinearLength::px(20.0)));
    }

    #[test]
    fn an_elliptical_corner_longhand_and_a_shorthand_corner_blend_per_axis() {
        let style = halfway(
            serde_json::json!({ "borderRadius": 8, "borderTopLeftRadius": "10px 20px" }),
            serde_json::json!({ "borderTopLeftRadius": "0 0" }),
        );
        assert_eq!(
            style.border_top_left_radius,
            Some(crate::style::CornerRadiusValue { x: LinearLength::px(5.0), y: Some(LinearLength::px(10.0)) })
        );
    }

    #[test]
    fn a_keyframes_list_of_shorthand_strings_splits_every_frame() {
        let started = Instant::now();
        let description = serde_json::json!({
            "initial": { "padding": 0 },
            "animate": { "padding": ["0 0", "10px 20px", "30px 40px"] },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        let state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
        let style = style_at(&state, at(started, 0.5));
        assert_eq!(style.padding_top, Some(LinearLength::px(10.0)));
        assert_eq!(style.padding_left, Some(LinearLength::px(20.0)));
        let style = style_at(&state, at(started, 1.0));
        assert_eq!(style.padding_top, Some(LinearLength::px(30.0)));
        assert_eq!(style.padding_left, Some(LinearLength::px(40.0)));
    }

    #[test]
    fn a_longhand_written_after_a_multi_value_shorthand_beats_it_and_one_before_loses() {
        let after = halfway(
            serde_json::json!({ "padding": "20px 40px", "paddingLeft": 100 }),
            serde_json::json!({ "padding": 0 }),
        );
        assert_eq!(after.padding_left, Some(LinearLength::px(50.0)));
        assert_eq!(after.padding_right, Some(LinearLength::px(20.0)));

        let before = halfway(
            serde_json::json!({ "paddingLeft": 100, "padding": "20px 40px" }),
            serde_json::json!({ "padding": 0 }),
        );
        assert_eq!(before.padding_left, Some(LinearLength::px(20.0)));
        assert_eq!(before.padding_right, Some(LinearLength::px(20.0)));
    }

    #[test]
    fn auto_is_an_error_where_there_is_no_auto_to_measure_or_flip() {
        let single = describe_error(serde_json::json!({ "animate": { "minWidth": "auto" } }));
        assert!(single.contains("minWidth") && single.contains("auto"), "{single}");
        let sides = describe_error(serde_json::json!({ "animate": { "padding": "1px 2px 3px 4px 5px" } }));
        assert!(sides.contains("padding") && sides.contains("one to four values"), "{sides}");
        let sign = describe_error(serde_json::json!({ "animate": { "padding": "1px -2px" } }));
        assert!(sign.contains("paddingRight") && sign.contains("non-negative"), "{sign}");
        let same = describe_error(serde_json::json!({ "animate": { "padding": -2 } }));
        assert!(same.contains("motion padding must"), "{same}");
    }

    /// The style `base` animates to under `animate` (from `initial`, when given) one second
    /// linear, `seconds` in.
    fn run_to(base: serde_json::Value, initial: serde_json::Value, animate: serde_json::Value, seconds: f64) -> StyleDesc {
        let started = Instant::now();
        let base: StyleDesc = serde_json::from_value(base).unwrap();
        let description = serde_json::json!({
            "initial": initial,
            "animate": animate,
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        let state = MotionState::new(&description, Some(&base), Colors::root(), None, 0, started).unwrap();
        style_at(&state, at(started, seconds))
    }

    #[test]
    fn a_margin_flips_between_auto_and_a_length_at_the_halfway_point() {
        let flip = |initial: serde_json::Value, animate: serde_json::Value, seconds| {
            run_to(serde_json::json!({}), initial, animate, seconds)
        };
        let from_auto = serde_json::json!({ "marginLeft": "auto" });
        let to_length = serde_json::json!({ "marginLeft": 100 });
        assert_eq!(flip(from_auto.clone(), to_length.clone(), 0.4).margin_left, Some(DimensionValue::Auto));
        assert_eq!(flip(from_auto.clone(), to_length.clone(), 0.6).margin_left, Some(DimensionValue::Pixels(100.0)));
        assert_eq!(flip(to_length.clone(), from_auto.clone(), 0.4).margin_left, Some(DimensionValue::Pixels(100.0)));
        assert_eq!(flip(to_length, from_auto, 0.6).margin_left, Some(DimensionValue::Auto));

        // An unset margin is 0, not auto: it blends.
        let blended = flip(serde_json::json!({}), serde_json::json!({ "marginLeft": 100 }), 0.4);
        assert_eq!(blended.margin_left, Some(DimensionValue::Pixels(40.0)));

        // `margin: auto` covers the four sides.
        let all = flip(serde_json::json!({ "margin": 0 }), serde_json::json!({ "margin": "auto" }), 0.6);
        for side in [all.margin_top, all.margin_right, all.margin_bottom, all.margin_left] {
            assert_eq!(side, Some(DimensionValue::Auto));
        }
    }

    #[test]
    fn a_declared_auto_margin_in_the_style_is_where_an_animation_starts() {
        let style = run_to(
            serde_json::json!({ "margin": "0 auto" }),
            serde_json::json!({ "opacity": 0 }),
            serde_json::json!({ "marginLeft": 100, "marginTop": 10 }),
            0.4,
        );
        assert_eq!(style.margin_left, Some(DimensionValue::Auto));
        assert_eq!(style.margin_top, Some(DimensionValue::Pixels(4.0)));
    }

    #[test]
    fn an_unset_inset_is_auto_and_flips_to_a_length_at_the_halfway_point() {
        let at = |seconds| {
            run_to(serde_json::json!({}), serde_json::json!({ "opacity": 0 }), serde_json::json!({ "top": 12 }), seconds)
        };
        assert_eq!(at(0.4).top, Some(DimensionValue::Auto));
        assert_eq!(at(0.6).top, Some(DimensionValue::Pixels(12.0)));
    }

    #[test]
    fn an_unset_border_width_starts_at_medium_whatever_the_style_and_at_a_shorthands_width() {
        let width = |base: serde_json::Value| {
            run_to(base, serde_json::json!({ "opacity": 0 }), serde_json::json!({ "borderTopWidth": 7 }), 0.5).border_top_width
        };
        // Blink zeroes the width of a `none` side where it is used, not in the value an animation blends.
        for base in [
            serde_json::json!({ "borderStyle": "solid" }),
            serde_json::json!({ "borderTopStyle": "dashed" }),
            serde_json::json!({ "borderStyle": "none" }),
            serde_json::json!({ "borderStyle": "hidden" }),
            serde_json::json!({ "borderBottomStyle": "solid" }),
            serde_json::json!({}),
        ] {
            assert_eq!(width(base.clone()), Some(LinearLength::px(5.0)), "{base}");
        }
        assert_eq!(width(serde_json::json!({ "border": "1px solid red" })), Some(LinearLength::px(4.0)));
        assert_eq!(width(serde_json::json!({ "borderTop": "thick solid" })), Some(LinearLength::px(6.0)));
        assert_eq!(width(serde_json::json!({ "borderStyle": "solid", "borderWidth": 1 })), Some(LinearLength::px(4.0)));
    }

    #[test]
    fn an_unset_outline_width_starts_at_medium_or_the_shorthands_width() {
        let width = |base: serde_json::Value| {
            run_to(base, serde_json::json!({ "opacity": 0 }), serde_json::json!({ "outlineWidth": 7 }), 0.5).outline_width
        };
        assert_eq!(width(serde_json::json!({ "outlineStyle": "solid" })), Some(LinearLength::px(5.0)));
        assert_eq!(width(serde_json::json!({ "outlineStyle": "none" })), Some(LinearLength::px(5.0)));
        assert_eq!(width(serde_json::json!({})), Some(LinearLength::px(5.0)));
        assert_eq!(width(serde_json::json!({ "outline": "1px solid" })), Some(LinearLength::px(4.0)));
    }

    #[test]
    fn flex_animates_as_its_three_longhands() {
        let at = |seconds| {
            run_to(
                serde_json::json!({}),
                serde_json::json!({ "flex": "1 1 0px" }),
                serde_json::json!({ "flex": "3 1 100px" }),
                seconds,
            )
        };
        let midpoint = at(0.5);
        assert_eq!(midpoint.flex_grow, Some(2.0));
        assert_eq!(midpoint.flex_shrink, Some(1.0));
        assert_eq!(midpoint.flex_basis, Some(DimensionValue::Pixels(50.0)));

        // A lone number is `n 1 0%`.
        let number = run_to(
            serde_json::json!({}),
            serde_json::json!({ "flex": 0 }),
            serde_json::json!({ "flex": 4 }),
            0.5,
        );
        assert_eq!(
            (number.flex_grow, number.flex_shrink, number.flex_basis),
            (Some(2.0), Some(1.0), Some(DimensionValue::Percentage(0.0)))
        );

        // Under a `flex` in the style the longhands start from what it says.
        let from_style = run_to(
            serde_json::json!({ "flex": "2 3 20px" }),
            serde_json::json!({ "opacity": 0 }),
            serde_json::json!({ "flex": "4 1 40px" }),
            0.5,
        );
        assert_eq!(
            (from_style.flex_grow, from_style.flex_shrink, from_style.flex_basis),
            (Some(3.0), Some(2.0), Some(DimensionValue::Pixels(30.0)))
        );
    }

    #[test]
    fn flex_with_an_auto_basis_animates_grow_and_shrink_and_flips_the_basis() {
        let at = |seconds| {
            run_to(
                serde_json::json!({}),
                serde_json::json!({ "flex": "0 0 100px" }),
                serde_json::json!({ "flex": "auto" }),
                seconds,
            )
        };
        assert_eq!(at(0.4).flex_basis, Some(DimensionValue::Pixels(100.0)));
        assert_eq!(at(0.6).flex_basis, Some(DimensionValue::Auto));
        assert_eq!(at(0.5).flex_grow, Some(0.5));
    }

    #[test]
    fn flex_basis_flips_between_a_length_and_content_halfway() {
        let at = |seconds| {
            run_to(
                serde_json::json!({}),
                serde_json::json!({ "flexBasis": 100 }),
                serde_json::json!({ "flexBasis": "content" }),
                seconds,
            )
        };
        assert_eq!(at(0.4).flex_basis, Some(DimensionValue::Pixels(100.0)));
        assert_eq!(at(0.6).flex_basis, Some(DimensionValue::Intrinsic(IntrinsicKeyword::Content)));
        let between_keywords = run_to(
            serde_json::json!({}),
            serde_json::json!({ "flexBasis": "auto" }),
            serde_json::json!({ "flex": "1 1 content" }),
            0.6,
        );
        assert_eq!(between_keywords.flex_basis, Some(DimensionValue::Intrinsic(IntrinsicKeyword::Content)));
    }

    #[test]
    fn a_bad_flex_target_is_an_error_naming_flex() {
        let error = describe_error(serde_json::json!({ "animate": { "flex": "wide" } }));
        assert!(error.contains("flex"), "{error}");
        let transition = serde_json::json!({
            "animate": { "flex": 2 },
            "transition": { "flex": { "duration": 3.0 } }
        });
        assert!(MotionState::new(&transition, None, Colors::root(), None, 0, Instant::now()).is_ok());
    }

    #[test]
    fn the_later_of_a_longhand_and_a_shorthand_wins_in_the_same_description() {
        let now = Instant::now();
        for (animate, left) in [
            (serde_json::json!({ "padding": 9.0, "paddingLeft": 3.0 }), 3.0),
            (serde_json::json!({ "paddingLeft": 3.0, "padding": 9.0 }), 9.0),
        ] {
            let description = serde_json::json!({ "initial": false, "animate": animate });
            let state = MotionState::new(&description, None, Colors::root(), None, 0, now).unwrap();
            let style = style_at(&state, now);
            assert_eq!(style.padding_left, Some(LinearLength::px(left)), "{description}");
            assert_eq!(style.padding_top, Some(LinearLength::px(9.0)), "{description}");
        }
    }

    #[test]
    fn the_later_of_a_transition_for_a_longhand_and_one_for_its_shorthand_wins() {
        let now = Instant::now();
        let slow_left = |first_left: bool| {
            let slow = serde_json::json!({ "duration": 10.0, "ease": "linear" });
            let fast = serde_json::json!({ "duration": 1.0, "ease": "linear" });
            let transition = if first_left {
                serde_json::json!({ "paddingLeft": slow, "padding": fast })
            } else {
                serde_json::json!({ "padding": fast, "paddingLeft": slow })
            };
            let description = serde_json::json!({
                "initial": { "padding": 0.0 },
                "animate": { "padding": 100.0 },
                "transition": transition
            });
            let state = MotionState::new(&description, None, Colors::root(), None, 0, now).unwrap();
            style_at(&state, at(now, 1.0)).padding_left
        };
        assert_eq!(slow_left(false), Some(LinearLength::px(10.0)), "paddingLeft written last keeps its slow curve");
        assert_eq!(slow_left(true), Some(LinearLength::px(100.0)), "padding written last runs on every side");
    }

    #[test]
    fn starts_from_the_elements_own_style_when_initial_omits_the_key() {
        let started = Instant::now();
        let base = StyleDesc {
            padding: Some(crate::css::shorthand::Sides::all(LinearLength::px(40.0))),
            width: Some(DimensionValue::Pixels(200.0)),
            ..Default::default()
        };
        let description = serde_json::json!({
            "initial": { "opacity": 0.0 },
            "animate": { "opacity": 1.0, "paddingTop": 0.0, "width": 100.0, "marginTop": 10.0 },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        let state = MotionState::new(&description, Some(&base), Colors::root(), None, 0, started).unwrap();
        let style = style_at(&state, at(started, 0.5));
        // From the shorthand in the base style, the base width, and CSS's
        // initial margin of 0.
        assert_eq!(style.padding_top, Some(LinearLength::px(20.0)));
        assert_eq!(style.width, Some(DimensionValue::Pixels(150.0)));
        assert_eq!(style.margin_top, Some(DimensionValue::Pixels(5.0)));
    }

    #[test]
    fn a_value_layout_cannot_measure_begins_at_its_target() {
        let started = Instant::now();
        let description = serde_json::json!({
            "initial": { "opacity": 0.0 },
            "animate": { "opacity": 1.0, "aspectRatio": 2.0, "borderColor": "#ff0000" },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        let state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
        let style = style_at(&state, started);
        assert_eq!(style.aspect_ratio, Some(crate::layout::aspect_ratio::AspectRatio::Ratio(2.0)));
        // A colour has a start without layout: `currentColor`, inherited black.
        assert_eq!(rgba(style.border_top_color.as_deref().unwrap()), rgba("#000000"));
        // `borderColor` is the shorthand of the four sides, and animates each.
        for side in [&style.border_right_color, &style.border_bottom_color, &style.border_left_color] {
            assert_eq!(side, &style.border_top_color);
        }
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
        let colors = Colors::of(&crate::color::ColorContext::root(crate::color::Scheme::Light), Some(&base));
        let state = MotionState::new(&description, Some(&base), colors, None, 0, started).unwrap();
        let style = style_at(&state, started);
        assert_eq!(rgba(style.border_top_color.as_deref().unwrap()), rgba("#0000ff"));
    }

    #[test]
    fn animates_a_colour_through_oklab_and_settles_on_the_target() {
        let started = Instant::now();
        let description = serde_json::json!({
            "initial": { "backgroundColor": "#ff0000" },
            "animate": { "backgroundColor": "#0000ff" },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        let mut state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();

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
        let mut state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
        let track = |state: &MotionState| state.tracks[&PropKey::Style(AnimProp::BackgroundColor)].clone();

        let retarget_at = at(started, 0.1);
        let before = track(&state).sample(retarget_at);
        assert!(before.velocity.iter().take(3).any(|v| v.abs() > 0.5));

        let retargeted = serde_json::json!({
            "initial": false,
            "animate": { "backgroundColor": "#00ff00" },
            "transition": spring
        });
        state.sync(&retargeted, None, Colors::root(), None, 0, retarget_at).unwrap();
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
        let mut state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();

        let retarget_at = at(started, 0.5);
        let retargeted = serde_json::json!({
            "initial": { "width": 0.0, "opacity": 0.0 },
            "animate": { "width": 100.0, "opacity": 0.0 },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        state.sync(&retargeted, None, Colors::root(), None, 0, retarget_at).unwrap();

        // Width was not retargeted, so it keeps its original timeline.
        assert_eq!(width(&state, at(started, 0.75)), 75.0);
        // Opacity restarted from 0.5 toward 0 over a fresh second.
        let opacity = state.tracks[&PropKey::Style(AnimProp::Opacity)].sample(at(retarget_at, 0.5));
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
        let mut state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
        let exit = serde_json::json!({
            "initial": false,
            "animate": { "opacity": 0.0 },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        state.sync(&exit, None, Colors::root(), None, 0, at(started, 0.5)).unwrap();
        assert_eq!(width(&state, at(started, 0.75)), 75.0);
    }

    #[test]
    fn a_newly_named_property_starts_from_the_element_style() {
        let started = Instant::now();
        let base = StyleDesc {
            padding_top: Some(LinearLength::px(10.0)),
            ..Default::default()
        };
        let mut state = MotionState::new(
            &serde_json::json!({ "initial": false, "animate": { "opacity": 1.0 } }),
            Some(&base), Colors::root(),
            None,
            0,
            started,
        )
        .unwrap();
        let next = serde_json::json!({
            "initial": false,
            "animate": { "opacity": 1.0, "paddingTop": 30.0 },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        state.sync(&next, Some(&base), Colors::root(), None, 0, started).unwrap();
        assert_eq!(
            style_at(&state, at(started, 0.5)).padding_top,
            Some(LinearLength::px(20.0))
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
        let state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
        let mut undershot = false;
        for step in 1..=200 {
            let now = at(started, step as f64 * 0.005);
            undershot |= state.tracks[&PropKey::Style(AnimProp::PaddingTop)].sample(now).position[0] < 0.0;
            assert!(style_at(&state, now).padding_top.unwrap().as_px().unwrap() >= 0.0);
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
        let mut state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
        let style = style_at(&state, at(started, 0.5));
        let shadows = style.box_shadow.unwrap();
        assert_eq!(shadows.as_slice().len(), 1);
        assert_eq!(shadows.as_slice()[0].offset_y.as_px(), Some(4.0));
        assert_eq!(shadows.as_slice()[0].blur_radius.as_px(), Some(10.0));
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
        let rest = fold_state_styles(&style, FocusVisible::default(), Default::default());
        assert_eq!(rest.color.as_deref(), Some("#000000"));

        let focused = fold_state_styles(&style, FocusVisible { own: true, descendant: false }, Default::default());
        assert_eq!(focused.color.as_deref(), Some("#111111"));
        assert_eq!(focused.opacity, Some(0.9));

        let both = fold_state_styles(
            &style,
            FocusVisible { own: true, descendant: false },
            crate::events::state_styles::PointerStates {
                hovered: true,
                active: true,
                ..Default::default()
            },
        );
        assert_eq!(both.color.as_deref(), Some("#222222"), "hover beats focus-visible");
        assert_eq!(both.opacity, Some(0.5), "active beats focus-visible");

        for folded in [rest, focused, both] {
            assert_eq!((folded.hover, folded.active, folded.focus_visible), (None, None, None));
            assert_eq!(
                (folded.group_hover, folded.group_active, folded.group_focus_within),
                (None, None, None)
            );
            assert_eq!(
                (folded.has_hover, folded.has_active, folded.has_focus, folded.has_focus_visible),
                (None, None, None, None)
            );
        }
    }

    #[test]
    fn relational_styles_fold_between_the_element_s_own_states_and_the_groups() {
        use crate::events::state_styles::PointerStates;
        let style: StyleDesc = serde_json::from_str(
            r##"{
                "color": "#000000",
                "groupHover": { "color": "#111111", "opacity": 0.1 },
                "hasHover": { "opacity": 0.2 },
                "hover": { "color": "#333333" },
                "hasFocusVisible": { "color": "#222222", "opacity": 0.3 },
                "focusVisible": { "opacity": 0.4 }
            }"##,
        )
        .unwrap();
        let states = PointerStates {
            group_hovered: true,
            has_hovered: true,
            hovered: true,
            ..Default::default()
        };
        let folded = fold_state_styles(&style, FocusVisible::default(), states);
        assert_eq!(folded.color.as_deref(), Some("#333333"), "hover beats groupHover");
        assert_eq!(folded.opacity, Some(0.2), "hasHover beats groupHover");

        let descendant = FocusVisible { own: false, descendant: true };
        let focused = fold_state_styles(&style, descendant, Default::default());
        assert_eq!((focused.color.as_deref(), focused.opacity), (Some("#222222"), Some(0.3)));
        let both = fold_state_styles(&style, FocusVisible { own: true, descendant: true }, Default::default());
        assert_eq!(both.opacity, Some(0.4), "focusVisible beats hasFocusVisible");
        assert!(focused.has_focus_visible.is_none() && both.focus_visible.is_none());
    }

    #[test]
    fn control_styles_survive_the_focus_pass_and_are_cleared_after_it() {
        use crate::events::state_styles::PointerStates;
        let style: StyleDesc = serde_json::from_str(
            r##"{
                "opacity": 1,
                "focusVisible": { "opacity": 0.4, "color": "#444444" },
                "hasEnabled": { "opacity": 0.5, "width": 5 },
                "disabled": { "opacity": 0.6 }
            }"##,
        )
        .unwrap();
        let own = FocusVisible { own: true, descendant: false };
        let states = PointerStates { has_enabled: true, disabled: true, ..Default::default() };
        let folded = fold_state_styles(&style, own, states);
        assert_eq!(folded.opacity, Some(0.6), "disabled comes after the focus layer it sits over");
        assert_eq!(folded.color.as_deref(), Some("#444444"));
        assert_eq!(folded.width, style.has_enabled.as_ref().unwrap().width);
        assert!(folded.disabled.is_none() && folded.has_enabled.is_none());
        let focus_only = fold_state_styles(&style, own, Default::default());
        assert_eq!(focus_only.opacity, Some(0.4), "the control layers do not apply unless their state does");
    }

    #[test]
    fn group_styles_fold_between_the_element_s_own_states() {
        let style: StyleDesc = serde_json::from_str(
            r##"{
                "color": "#000000",
                "groupHover": { "color": "#111111", "opacity": 0.9, "group": "card" },
                "hover": { "opacity": 0.8 }
            }"##,
        )
        .unwrap();
        let states = crate::events::state_styles::PointerStates {
            hovered: true,
            group_hovered: true,
            ..Default::default()
        };
        let folded = fold_state_styles(&style, FocusVisible::default(), states);
        assert_eq!(folded.color.as_deref(), Some("#111111"));
        assert_eq!(folded.opacity, Some(0.8), "hover beats groupHover");
        assert_eq!(folded.group, None, "the followed group is not declared");
    }

    mod svg {
        use super::*;

        fn attributes(pairs: &[(&str, Value)]) -> HashMap<String, Value> {
            pairs
                .iter()
                .map(|(key, value)| (key.to_string(), value.clone()))
                .collect()
        }

        fn at(state: &MotionState, started: Instant, millis: u64) -> SvgMotion {
            state
                .visible_svg(started + Duration::from_millis(millis))
                .unwrap()
        }

        fn attribute<'a>(motion: &'a SvgMotion, name: &str) -> &'a str {
            &motion
                .attributes
                .iter()
                .find(|(attribute, _)| *attribute == name)
                .unwrap_or_else(|| panic!("no {name} in {motion:?}"))
                .1
        }

        #[test]
        fn draws_a_path_with_the_normalised_keys() {
            let started = Instant::now();
            let description = serde_json::json!({
                "initial": { "pathLength": 0.0 },
                "animate": { "pathLength": 1.0 },
                "transition": { "duration": 1.0, "ease": "linear" }
            });
            let state = MotionState::new_svg(&description, &attributes(&[]), Colors::root(), started).unwrap();

            let draw = at(&state, started, 250).draw.unwrap();
            assert!((draw.length - 0.25).abs() < 1e-9);
            assert_eq!((draw.offset, draw.spacing), (0.0, 1.0));
            assert!(at(&state, started, 250).attributes.is_empty());
            assert_eq!(at(&state, started, 1000).draw.unwrap().length, 1.0);
        }

        #[test]
        fn animates_filter_and_text_attributes() {
            let started = Instant::now();
            let description = serde_json::json!({
                "initial": { "stdDeviation": 0.0, "dx": 0.0, "floodColor": "#000000", "fontSize": 10.0 },
                "animate": { "stdDeviation": 8.0, "dx": 20.0, "floodColor": "#ffffff", "fontSize": 30.0 },
                "transition": { "duration": 1.0, "ease": "linear" }
            });
            let state = MotionState::new_svg(&description, &attributes(&[]), Colors::root(), started).unwrap();
            let halfway = at(&state, started, 500);
            assert_eq!(attribute(&halfway, "stdDeviation"), "4");
            assert_eq!(attribute(&halfway, "dx"), "10");
            assert_eq!(attribute(&halfway, "font-size"), "20");
            assert!(attribute(&halfway, "flood-color").starts_with("rgba(99."), "{halfway:?}");
        }

        #[test]
        fn offset_and_spacing_alone_still_draw() {
            let started = Instant::now();
            let description = serde_json::json!({
                "initial": false,
                "animate": { "pathOffset": 0.25, "pathSpacing": 0.5 },
                "transition": {}
            });
            let state = MotionState::new_svg(&description, &attributes(&[]), Colors::root(), started).unwrap();
            let draw = at(&state, started, 0).draw.unwrap();
            assert_eq!(
                draw,
                PathDraw {
                    length: 1.0,
                    offset: 0.25,
                    spacing: 0.5
                }
            );
        }

        #[test]
        fn starts_from_the_elements_own_attribute() {
            let started = Instant::now();
            let description = serde_json::json!({
                "initial": {},
                "animate": { "r": 20.0, "fill": "#0000ff" },
                "transition": { "duration": 1.0, "ease": "linear" }
            });
            let own = attributes(&[("r", serde_json::json!(10)), ("fill", serde_json::json!("#ff0000"))]);
            let state = MotionState::new_svg(&description, &own, Colors::root(), started).unwrap();
            assert_eq!(attribute(&at(&state, started, 0), "r"), "10");
            assert_eq!(attribute(&at(&state, started, 500), "r"), "15");
            assert_eq!(attribute(&at(&state, started, 1000), "r"), "20");
            // Oklab midpoint of red and blue, as for a style colour.
            let middle = attribute(&at(&state, started, 500), "fill").to_string();
            let rgba = crate::color::parse_color_rgba(&middle).unwrap();
            assert!((rgba.r as f64 * 255.0 - 140.0).abs() < 4.0, "{middle}");
        }

        #[test]
        fn morphs_paths_with_the_same_commands_and_flips_others() {
            let started = Instant::now();
            let morph = |to: &str| {
                let description = serde_json::json!({
                    "initial": { "d": "M0 0 L10 0" },
                    "animate": { "d": to },
                    "transition": { "duration": 1.0, "ease": "linear" }
                });
                MotionState::new_svg(&description, &attributes(&[]), Colors::root(), started).unwrap()
            };
            let same = morph("M10 10 L30 10");
            assert_eq!(attribute(&at(&same, started, 500), "d"), "M 5 5 L 20 5");
            let other = morph("M10 10 H30");
            assert_eq!(attribute(&at(&other, started, 499), "d"), "M 0 0 L 10 0");
            assert_eq!(attribute(&at(&other, started, 500), "d"), "M 10 10 H 30");
        }

        #[test]
        fn each_attribute_has_its_own_transition() {
            let started = Instant::now();
            let description = serde_json::json!({
                "initial": { "cx": 0.0, "cy": 0.0 },
                "animate": { "cx": 100.0, "cy": 100.0 },
                "transition": { "duration": 1.0, "ease": "linear", "cx": { "duration": 0.1, "ease": "linear" } }
            });
            let state = MotionState::new_svg(&description, &attributes(&[]), Colors::root(), started).unwrap();
            let motion = at(&state, started, 500);
            assert_eq!(attribute(&motion, "cx"), "100");
            assert_eq!(attribute(&motion, "cy"), "50");
        }

        #[test]
        fn retargets_from_the_visible_value() {
            let started = Instant::now();
            let first = serde_json::json!({
                "initial": { "strokeWidth": 0.0 },
                "animate": { "strokeWidth": 10.0 },
                "transition": { "duration": 1.0, "ease": "linear" }
            });
            let mut state = MotionState::new_svg(&first, &attributes(&[]), Colors::root(), started).unwrap();
            let second = serde_json::json!({
                "initial": false,
                "animate": { "strokeWidth": 0.0 },
                "transition": { "duration": 1.0, "ease": "linear" }
            });
            state
                .sync_svg(&second, &attributes(&[]), Colors::root(), started + Duration::from_millis(500))
                .unwrap();
            assert_eq!(attribute(&at(&state, started, 500), "stroke-width"), "5");
            assert_eq!(attribute(&at(&state, started, 1000), "stroke-width"), "2.5");
        }

        #[test]
        fn style_keys_are_not_svg_attributes_and_the_other_way_round() {
            let now = Instant::now();
            let svg_with_style_key = serde_json::json!({ "animate": { "backgroundColor": "#fff" }, "transition": {} });
            let error = MotionState::new_svg(&svg_with_style_key, &attributes(&[]), Colors::root(), now)
                .err()
                .unwrap();
            assert!(error.contains("animatable SVG attribute"), "{error}");
            let style_with_svg_key = serde_json::json!({ "animate": { "pathLength": 1.0 }, "transition": {} });
            assert!(MotionState::new(&style_with_svg_key, None, Colors::root(), None, 0, now).is_err());
            let unknown_transition = serde_json::json!({ "animate": {}, "transition": { "backgroundColor": { "duration": 1 } } });
            assert!(MotionState::new_svg(&unknown_transition, &attributes(&[]), Colors::root(), now).is_err());
        }

        #[test]
        fn springs_overshoot_without_leaving_the_valid_range() {
            let started = Instant::now();
            let description = serde_json::json!({
                "initial": { "opacity": 0.0 },
                "animate": { "opacity": 1.0 },
                "transition": { "type": "spring", "stiffness": 400, "damping": 5 }
            });
            let state = MotionState::new_svg(&description, &attributes(&[]), Colors::root(), started).unwrap();
            for millis in (0..600).step_by(10) {
                let opacity: f64 = attribute(&at(&state, started, millis), "opacity").parse().unwrap();
                assert!((0.0..=1.0).contains(&opacity), "{opacity}");
            }
        }
    }
    /// Animations that need layout: `height: "auto"` and starts the style
    /// leaves unset.
    mod measured {
        use super::*;

        fn measurement(width: f32, height: f32, natural: f32) -> Measurement {
            Measurement {
                border_box: gpui::size(gpui::px(width), gpui::px(height)),
                natural_height: gpui::px(natural),
                natural_width: None,
                parent_content: gpui::size(gpui::px(800.0), gpui::px(600.0)),
                scale_factor: 1.0,
                percent_height_resolves: true,
                flex_main: crate::motion::property::Axis::Width,
            }
        }

        /// A measurement whose element is `width` wide as laid out and would be
        /// `natural` wide with `width: auto`.
        fn wide(width: f32, natural: f32) -> Measurement {
            Measurement {
                natural_width: Some(gpui::px(natural)),
                ..measurement(width, 30.0, 30.0)
            }
        }

        /// `measurement` laid out in a parent whose content box is `width` by
        /// `height`.
        fn in_parent(measurement: Measurement, width: f32, height: f32) -> Measurement {
            Measurement {
                parent_content: gpui::size(gpui::px(width), gpui::px(height)),
                ..measurement
            }
        }

        fn height(state: &MotionState, now: Instant) -> Option<DimensionValue> {
            style_at(state, now).height
        }

        fn pixels(value: Option<DimensionValue>) -> f64 {
            match value {
                Some(DimensionValue::Pixels(pixels)) => pixels,
                other => panic!("expected px, got {other:?}"),
            }
        }

        fn linear(animate: serde_json::Value, initial: serde_json::Value) -> serde_json::Value {
            serde_json::json!({
                "initial": initial,
                "animate": animate,
                "transition": { "duration": 1.0, "ease": "linear" }
            })
        }

        fn open() -> serde_json::Value {
            linear(
                serde_json::json!({ "height": "auto" }),
                serde_json::json!({ "height": 0.0 }),
            )
        }

        fn height_track(state: &MotionState) -> &Track {
            &state.tracks[&PropKey::Style(AnimProp::Height)]
        }

        #[test]
        fn opening_waits_for_a_measurement_then_runs_to_the_content_and_settles_to_auto() {
            let started = Instant::now();
            let mut state = MotionState::new(&open(), None, Colors::root(), None, 0, started).unwrap();

            // Nothing is measured on the first frame: it holds the start and
            // keeps the frames coming.
            assert_eq!(pixels(height(&state, started)), 0.0);
            let waiting = state.frame(started);
            assert!(waiting.active);
            assert!(!waiting.just_settled);
            assert!(state.owns(PropKey::Style(AnimProp::Height)), "a transition must not take it over");

            // The clock started when the style changed, not at the first frame
            // that has a measurement.
            let measured_at = at(started, 0.25);
            let measured = measurement(100.0, 0.0, 80.0);
            state.sync(&open(), None, Colors::root(), Some(&measured), 0, measured_at).unwrap();
            assert!(state.frame(measured_at).active);
            assert_eq!(pixels(height(&state, measured_at)), 20.0);
            assert_eq!(pixels(height(&state, at(started, 0.5))), 40.0);

            let end = at(started, 1.0);
            state.sync(&open(), None, Colors::root(), Some(&measured), 0, end).unwrap();
            let frame = state.frame(end);
            assert!(!frame.active);
            assert!(frame.just_settled);
            assert_eq!(height(&state, end), Some(DimensionValue::Auto));
        }

        #[test]
        fn the_target_is_the_natural_height_rounded_up_to_the_device_pixel() {
            let started = Instant::now();
            let measured = Measurement {
                scale_factor: 2.0,
                ..measurement(100.0, 0.0, 80.2)
            };
            let state = MotionState::new(&open(), None, Colors::root(), Some(&measured), 0, started).unwrap();
            assert_eq!(pixels(height(&state, at(started, 0.5))), 40.25);
            let state = MotionState::new(&open(), None, Colors::root(), Some(&measurement(100.0, 0.0, 80.2)), 0, started)
                .unwrap();
            assert_eq!(pixels(height(&state, at(started, 0.5))), 40.5);
        }

        #[test]
        fn without_an_initial_height_the_animation_starts_from_the_style() {
            let started = Instant::now();
            let base = StyleDesc {
                height: Some(DimensionValue::Pixels(20.0)),
                ..Default::default()
            };
            let description = linear(
                serde_json::json!({ "height": "auto" }),
                serde_json::json!({ "opacity": 1.0 }),
            );
            let measured = measurement(100.0, 20.0, 60.0);
            let state =
                MotionState::new(&description, Some(&base), Colors::root(), Some(&measured), 0, started).unwrap();
            assert_eq!(pixels(height(&state, started)), 20.0);
            assert_eq!(pixels(height(&state, at(started, 0.5))), 40.0);
        }

        #[test]
        fn initial_false_to_auto_is_already_auto() {
            let started = Instant::now();
            let description = linear(
                serde_json::json!({ "height": "auto" }),
                serde_json::json!(false),
            );
            let mut state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
            assert_eq!(height(&state, started), Some(DimensionValue::Auto));
            assert!(!state.frame(started).active);
        }

        #[test]
        fn a_percentage_start_runs_to_auto_through_px() {
            let started = Instant::now();
            let description = linear(
                serde_json::json!({ "height": "auto" }),
                serde_json::json!({ "height": "50%" }),
            );
            let measured = in_parent(measurement(100.0, 40.0, 80.0), 300.0, 40.0);
            let mut state =
                MotionState::new(&description, None, Colors::root(), Some(&measured), 0, started).unwrap();
            assert_eq!(pixels(height(&state, started)), 20.0);
            assert_eq!(pixels(height(&state, at(started, 0.5))), 50.0);
            let end = at(started, 1.0);
            state.sync(&description, None, Colors::root(), Some(&measured), 0, end).unwrap();
            assert_eq!(height(&state, end), Some(DimensionValue::Auto));
        }

        #[test]
        fn the_target_follows_the_content_while_a_tween_runs() {
            let started = Instant::now();
            let mut state = MotionState::new(&open(), None, Colors::root(), Some(&measurement(100.0, 0.0, 80.0)), 0, started)
                .unwrap();
            let grown = measurement(100.0, 40.0, 120.0);
            let now = at(started, 0.5);
            state.sync(&open(), None, Colors::root(), Some(&grown), 0, now).unwrap();

            assert_eq!(pixels(height(&state, now)), 40.0, "the value showing does not jump");
            assert_eq!(pixels(height(&state, at(started, 1.0))), 80.0);
            let end = at(started, 1.5);
            state.sync(&open(), None, Colors::root(), Some(&grown), 0, end).unwrap();
            assert_eq!(height(&state, end), Some(DimensionValue::Auto));
            assert!(state.frame(end).just_settled);
        }

        #[test]
        fn a_spring_keeps_its_speed_when_the_target_moves() {
            let started = Instant::now();
            let description = serde_json::json!({
                "initial": { "height": 0.0 },
                "animate": { "height": "auto" },
                "transition": { "type": "spring", "stiffness": 200.0, "damping": 30.0 }
            });
            let mut state = MotionState::new(
                &description,
                None, Colors::root(),
                Some(&measurement(100.0, 0.0, 80.0)),
                0,
                started,
            )
            .unwrap();
            let now = at(started, 0.15);
            let before = height_track(&state).sample(now);
            assert!(before.velocity[0] > 0.0);

            state
                .sync(&description, None, Colors::root(), Some(&measurement(100.0, 30.0, 120.0)), 0, now)
                .unwrap();
            let after = height_track(&state).sample(now);
            assert_eq!(after.position, before.position);
            assert!((after.velocity[0] - before.velocity[0]).abs() < 1e-9);

            let end = at(started, 5.0);
            state
                .sync(&description, None, Colors::root(), Some(&measurement(100.0, 120.0, 120.0)), 0, end)
                .unwrap();
            assert_eq!(height(&state, end), Some(DimensionValue::Auto));
        }

        #[test]
        fn a_delayed_segment_changes_its_end_in_place() {
            let started = Instant::now();
            let description = serde_json::json!({
                "initial": { "height": 0.0 },
                "animate": { "height": "auto" },
                "transition": { "duration": 1.0, "ease": "linear", "delay": 0.5 }
            });
            let mut state = MotionState::new(
                &description,
                None, Colors::root(),
                Some(&measurement(100.0, 0.0, 80.0)),
                0,
                started,
            )
            .unwrap();
            let grown = measurement(100.0, 0.0, 120.0);
            state.sync(&description, None, Colors::root(), Some(&grown), 0, at(started, 0.2)).unwrap();
            assert_eq!(pixels(height(&state, at(started, 0.2))), 0.0);
            // The clock was not restarted: the delay still ends at 0.5s.
            assert_eq!(pixels(height(&state, at(started, 1.0))), 60.0);
        }

        #[test]
        fn collapsing_from_settled_auto_starts_at_the_height_layout_gave() {
            let started = Instant::now();
            let mut state = MotionState::new(
                &linear(serde_json::json!({ "height": "auto" }), serde_json::json!(false)),
                None, Colors::root(),
                None,
                0,
                started,
            )
            .unwrap();
            // The content grew after the open animation settled; the number the
            // segment ended on is stale.
            let grown = measurement(100.0, 100.0, 100.0);
            let closed = linear(serde_json::json!({ "height": 0.0 }), serde_json::json!(false));
            state.sync(&closed, None, Colors::root(), Some(&grown), 0, started).unwrap();
            assert_eq!(pixels(height(&state, started)), 100.0);
            assert_eq!(pixels(height(&state, at(started, 0.5))), 50.0);
            let end = at(started, 1.0);
            state.sync(&closed, None, Colors::root(), Some(&grown), 0, end).unwrap();
            assert_eq!(pixels(height(&state, end)), 0.0);
        }

        #[test]
        fn retargeting_from_a_running_auto_segment_continues_from_where_it_is() {
            let started = Instant::now();
            let measured = measurement(100.0, 0.0, 80.0);
            let mut state = MotionState::new(&open(), None, Colors::root(), Some(&measured), 0, started).unwrap();
            let closed = linear(serde_json::json!({ "height": 0.0 }), serde_json::json!(false));
            let now = at(started, 0.5);
            state.sync(&closed, None, Colors::root(), Some(&measured), 0, now).unwrap();
            assert_eq!(pixels(height(&state, now)), 40.0);
            assert_eq!(pixels(height(&state, at(now, 0.5))), 20.0);
        }

        #[test]
        fn a_new_description_while_waiting_replaces_the_wait() {
            let started = Instant::now();
            let mut state = MotionState::new(&open(), None, Colors::root(), None, 0, started).unwrap();
            let fixed = linear(serde_json::json!({ "height": 50.0 }), serde_json::json!(false));
            state.sync(&fixed, None, Colors::root(), None, 0, started).unwrap();
            assert!(state.pending.is_empty());
            assert_eq!(pixels(height(&state, started)), 0.0);
            assert_eq!(pixels(height(&state, at(started, 0.5))), 25.0);
        }

        #[test]
        fn an_element_missing_from_a_painted_frame_jumps_to_the_target() {
            let started = Instant::now();
            let mut state = MotionState::new(&open(), None, Colors::root(), None, 3, started).unwrap();
            let unset_width =
                linear(serde_json::json!({ "width": 100.0 }), serde_json::json!({ "opacity": 1.0 }));
            let mut width = MotionState::new(&unset_width, None, Colors::root(), None, 3, started).unwrap();
            assert!(width.frame(started).active);

            // No frame was painted since: however long it takes, it still waits.
            let late = at(started, 60.0);
            state.sync(&open(), None, Colors::root(), None, 3, late).unwrap();
            assert!(state.frame(late).active);
            assert_eq!(pixels(height(&state, late)), 0.0);

            // A frame was painted and the element was not in it.
            state.sync(&open(), None, Colors::root(), None, 4, late).unwrap();
            assert_eq!(height(&state, late), Some(DimensionValue::Auto));
            let frame = state.frame(late);
            assert!(!frame.active);
            assert!(frame.just_settled);
            width.sync(&unset_width, None, Colors::root(), None, 4, late).unwrap();
            assert_eq!(style_at(&width, late).width, Some(DimensionValue::Pixels(100.0)));
            assert!(!width.frame(late).active);
        }

        #[test]
        fn a_measurement_after_painted_frames_still_starts_the_animation() {
            let started = Instant::now();
            let mut state = MotionState::new(&open(), None, Colors::root(), None, 3, started).unwrap();
            let measured_at = at(started, 0.3);
            state
                .sync(&open(), None, Colors::root(), Some(&measurement(100.0, 0.0, 80.0)), 4, measured_at)
                .unwrap();
            assert!(state.pending.is_empty());
            assert_eq!(pixels(height(&state, at(started, 0.5))), 40.0);
        }

        #[test]
        fn an_unset_size_starts_from_the_laid_out_one() {
            let started = Instant::now();
            let description = linear(
                serde_json::json!({ "width": 100.0, "maxHeight": 0.0, "minHeight": 90.0 }),
                serde_json::json!({ "opacity": 1.0 }),
            );
            let mut state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
            // Waiting: nothing is applied, so the element keeps the size it has.
            let waiting = style_at(&state, started);
            assert_eq!((waiting.width, waiting.max_height, waiting.min_height), (None, None, None));
            assert!(state.frame(started).active);

            let measured_at = at(started, 0.2);
            let measured = measurement(60.0, 40.0, 40.0);
            state.sync(&description, None, Colors::root(), Some(&measured), 0, measured_at).unwrap();
            let halfway = style_at(&state, at(started, 0.5));
            assert_eq!(halfway.width, Some(DimensionValue::Pixels(80.0)));
            assert_eq!(halfway.max_height, Some(DimensionValue::Pixels(20.0)));
            assert_eq!(halfway.min_height, Some(DimensionValue::Pixels(65.0)));
        }

        #[test]
        fn an_auto_style_is_unset_and_starts_from_layout() {
            let started = Instant::now();
            let base = StyleDesc {
                height: Some(DimensionValue::Auto),
                ..Default::default()
            };
            let description = linear(
                serde_json::json!({ "height": 100.0 }),
                serde_json::json!({ "opacity": 1.0 }),
            );
            let state = MotionState::new(
                &description,
                Some(&base), Colors::root(),
                Some(&measurement(10.0, 60.0, 60.0)),
                0,
                started,
            )
            .unwrap();
            assert_eq!(pixels(height(&state, started)), 60.0);
            assert_eq!(pixels(height(&state, at(started, 0.5))), 80.0);
        }

        #[test]
        fn initial_auto_starts_from_layout_and_alone_applies_auto() {
            let started = Instant::now();
            let state = MotionState::new(
                &linear(
                    serde_json::json!({ "height": 0.0 }),
                    serde_json::json!({ "height": "auto" }),
                ),
                None, Colors::root(),
                Some(&measurement(10.0, 60.0, 60.0)),
                0,
                started,
            )
            .unwrap();
            assert_eq!(pixels(height(&state, started)), 60.0);

            let only = serde_json::json!({
                "initial": { "height": "auto" },
                "animate": { "opacity": 1.0 },
                "transition": {}
            });
            let state = MotionState::new(&only, None, Colors::root(), None, 0, started).unwrap();
            assert_eq!(height(&state, started), Some(DimensionValue::Auto));
        }

        fn open_wide() -> serde_json::Value {
            linear(
                serde_json::json!({ "width": "auto" }),
                serde_json::json!({ "width": 0.0 }),
            )
        }

        #[test]
        fn opening_a_width_waits_for_the_natural_width_then_runs_to_it_and_settles_to_auto() {
            let started = Instant::now();
            let mut state = MotionState::new(&open_wide(), None, Colors::root(), None, 0, started).unwrap();
            assert_eq!(pixels(style_at(&state, started).width), 0.0);
            assert!(state.frame(started).active);
            assert!(state.owns(PropKey::Style(AnimProp::Width)));
            assert!(state.needs_natural_width(started), "the element is flagged to measure it");

            // A measurement from a frame that did not measure the width does not
            // start it: the clock waits for the natural width.
            let laid_out_at = at(started, 0.1);
            state.sync(&open_wide(), None, Colors::root(), Some(&measurement(0.0, 30.0, 30.0)), 0, laid_out_at).unwrap();
            assert!(state.frame(laid_out_at).active);
            assert_eq!(pixels(style_at(&state, at(laid_out_at, 0.5)).width), 0.0);

            let measured_at = at(started, 0.25);
            let measured = wide(0.0, 120.0);
            state.sync(&open_wide(), None, Colors::root(), Some(&measured), 0, measured_at).unwrap();
            assert_eq!(pixels(style_at(&state, measured_at).width), 30.0);
            assert_eq!(pixels(style_at(&state, at(started, 0.5)).width), 60.0);
            assert!(state.needs_natural_width(at(started, 0.5)), "it follows the width while it runs");

            let end = at(started, 1.0);
            state.sync(&open_wide(), None, Colors::root(), Some(&measured), 0, end).unwrap();
            let frame = state.frame(end);
            assert!(!frame.active);
            assert!(frame.just_settled);
            assert_eq!(style_at(&state, end).width, Some(DimensionValue::Auto));
            assert!(!state.needs_natural_width(end), "a settled auto is not measured again");
        }

        #[test]
        fn the_natural_width_target_rounds_up_to_the_device_pixel() {
            let started = Instant::now();
            let measured = Measurement {
                scale_factor: 2.0,
                ..wide(0.0, 120.2)
            };
            let state = MotionState::new(&open_wide(), None, Colors::root(), Some(&measured), 0, started).unwrap();
            assert_eq!(pixels(style_at(&state, at(started, 0.5)).width), 60.25);
        }

        #[test]
        fn a_width_whose_natural_width_never_arrives_jumps_to_auto_instead_of_asking_for_frames() {
            let started = Instant::now();
            let mut state = MotionState::new(&open_wide(), None, Colors::root(), None, 0, started).unwrap();
            let unmeasured = measurement(0.0, 30.0, 30.0);
            let late = at(started, 0.5);
            state.sync(&open_wide(), None, Colors::root(), Some(&unmeasured), 1, late).unwrap();
            assert!(state.frame(late).active, "one more frame of grace");
            state.sync(&open_wide(), None, Colors::root(), Some(&unmeasured), 2, late).unwrap();
            assert_eq!(style_at(&state, late).width, Some(DimensionValue::Auto));
            assert!(!state.frame(late).active);
        }

        #[test]
        fn the_width_target_follows_the_content_while_a_tween_runs() {
            let started = Instant::now();
            let mut state =
                MotionState::new(&open_wide(), None, Colors::root(), Some(&wide(0.0, 120.0)), 0, started).unwrap();
            let grown = wide(60.0, 200.0);
            let now = at(started, 0.5);
            state.sync(&open_wide(), None, Colors::root(), Some(&grown), 0, now).unwrap();
            assert_eq!(pixels(style_at(&state, now).width), 60.0, "the value showing does not jump");
            assert_eq!(pixels(style_at(&state, at(started, 1.0)).width), 130.0);
        }

        #[test]
        fn a_measurement_without_the_natural_width_leaves_a_running_width_alone() {
            let started = Instant::now();
            let mut state =
                MotionState::new(&open_wide(), None, Colors::root(), Some(&wide(0.0, 120.0)), 0, started).unwrap();
            let now = at(started, 0.5);
            state.sync(&open_wide(), None, Colors::root(), Some(&measurement(60.0, 30.0, 30.0)), 0, now).unwrap();
            assert_eq!(pixels(style_at(&state, at(started, 0.9)).width), 108.0);
        }

        #[test]
        fn collapsing_from_a_settled_width_auto_starts_at_the_width_layout_gave() {
            let started = Instant::now();
            let mut state = MotionState::new(
                &linear(serde_json::json!({ "width": "auto" }), serde_json::json!(false)),
                None, Colors::root(),
                None,
                0,
                started,
            )
            .unwrap();
            let closed = linear(serde_json::json!({ "width": 0.0 }), serde_json::json!(false));
            state.sync(&closed, None, Colors::root(), Some(&wide(100.0, 100.0)), 0, started).unwrap();
            assert_eq!(pixels(style_at(&state, started).width), 100.0);
            assert_eq!(pixels(style_at(&state, at(started, 0.5)).width), 50.0);
        }

        #[test]
        fn a_number_target_retargeted_to_auto_waits_one_frame_for_the_natural_width() {
            let started = Instant::now();
            let fixed = linear(serde_json::json!({ "width": 50.0 }), serde_json::json!(false));
            let mut state = MotionState::new(&fixed, None, Colors::root(), Some(&wide(50.0, 50.0)), 0, started).unwrap();
            assert!(!state.needs_natural_width(started), "a number target does not ask for it");

            let now = at(started, 1.0);
            let unmeasured = measurement(50.0, 30.0, 30.0);
            state.sync(&open_wide(), None, Colors::root(), Some(&unmeasured), 0, now).unwrap();
            assert!(state.needs_natural_width(now), "the retarget asks for it");
            assert_eq!(pixels(style_at(&state, now).width), 50.0);

            let measured_at = at(now, 0.1);
            state.sync(&open_wide(), None, Colors::root(), Some(&wide(50.0, 120.0)), 0, measured_at).unwrap();
            assert_eq!(pixels(style_at(&state, at(now, 0.5)).width), 85.0);
        }

        const FRAME: f64 = 0.016;

        fn transitions() -> [(&'static str, serde_json::Value); 2] {
            [
                ("linear", serde_json::json!({ "duration": 0.1, "ease": "linear" })),
                ("spring", serde_json::json!({ "type": "spring", "stiffness": 170.0, "damping": 26.0 })),
            ]
        }

        fn width_motion(
            animate: serde_json::Value,
            initial: serde_json::Value,
            transition: &serde_json::Value,
        ) -> serde_json::Value {
            serde_json::json!({ "initial": initial, "animate": animate, "transition": transition })
        }

        /// The width on the first two frames after the style changed, a frame
        /// apart. `mounted` is the measurement the changing frame had; `next`
        /// the one the following frame has.
        fn widths_after_the_change(
            description: &serde_json::Value,
            mounted: Option<&Measurement>,
            next: &Measurement,
        ) -> [f64; 2] {
            let started = Instant::now();
            let mut state = MotionState::new(description, None, Colors::root(), mounted, 0, started).unwrap();
            let first = at(started, FRAME);
            state.sync(description, None, Colors::root(), Some(next), 0, first).unwrap();
            [pixels(style_at(&state, first).width), pixels(style_at(&state, at(started, 2.0 * FRAME)).width)]
        }

        #[test]
        fn an_auto_width_is_as_far_along_as_a_px_one_on_the_frame_after_the_change() {
            let measured = wide(0.0, 120.0);
            for (name, transition) in transitions() {
                let animate = |width: serde_json::Value| {
                    width_motion(serde_json::json!({ "width": width }), serde_json::json!({ "width": 0.0 }), &transition)
                };
                let px = widths_after_the_change(&animate(120.0.into()), Some(&measured), &measured);
                let auto = widths_after_the_change(&animate("auto".into()), None, &measured);
                assert!(px[0] > 0.0, "{name}: the px animation moved on the first frame after the change");
                assert_eq!(auto, px, "{name}: 0 to auto");
            }
            let linear = &transitions()[0].1;
            let auto = widths_after_the_change(
                &width_motion(serde_json::json!({ "width": "auto" }), serde_json::json!({ "width": 0.0 }), linear),
                None,
                &measured,
            );
            assert!((auto[0] - 0.16 * 120.0).abs() < 1e-3, "16ms of 100ms: {}", auto[0]);
        }

        #[test]
        fn a_width_retargeted_to_or_from_auto_is_as_far_along_as_a_px_one_on_the_frame_after_the_change() {
            for (name, transition) in transitions() {
                let fixed = |width: f64| width_motion(serde_json::json!({ "width": width }), serde_json::json!(false), &transition);
                let to_auto = width_motion(serde_json::json!({ "width": "auto" }), serde_json::json!(false), &transition);
                let to_zero = fixed(0.0);

                // A px width heading for auto, then for 0 again from there.
                let started = Instant::now();
                let mut auto =
                    MotionState::new(&fixed(50.0), None, Colors::root(), Some(&wide(50.0, 50.0)), 0, started).unwrap();
                auto.sync(&to_auto, None, Colors::root(), Some(&measurement(50.0, 30.0, 30.0)), 0, started).unwrap();
                let first = at(started, FRAME);
                auto.sync(&to_auto, None, Colors::root(), Some(&wide(50.0, 120.0)), 0, first).unwrap();
                let mut px =
                    MotionState::new(&fixed(50.0), None, Colors::root(), Some(&wide(50.0, 50.0)), 0, started).unwrap();
                px.sync(&fixed(120.0), None, Colors::root(), Some(&wide(50.0, 120.0)), 0, started).unwrap();
                for frame in [first, at(started, 2.0 * FRAME)] {
                    assert_eq!(
                        pixels(style_at(&auto, frame).width),
                        pixels(style_at(&px, frame).width),
                        "{name}: px to auto"
                    );
                }
                assert!(pixels(style_at(&auto, first).width) > 50.0, "{name}: moved on the first frame");

                // A settled auto width collapsing: the width it had comes from layout.
                let settled = |state: &mut MotionState| {
                    state.sync(&to_zero, None, Colors::root(), None, 0, started).unwrap();
                };
                let mut from_auto =
                    MotionState::new(&to_auto, None, Colors::root(), None, 0, started).unwrap();
                settled(&mut from_auto);
                from_auto.sync(&to_zero, None, Colors::root(), Some(&wide(100.0, 100.0)), 0, first).unwrap();
                let mut from_px =
                    MotionState::new(&fixed(100.0), None, Colors::root(), Some(&wide(100.0, 100.0)), 0, started).unwrap();
                from_px.sync(&to_zero, None, Colors::root(), Some(&wide(100.0, 100.0)), 0, started).unwrap();
                for frame in [first, at(started, 2.0 * FRAME)] {
                    assert_eq!(
                        pixels(style_at(&from_auto, frame).width),
                        pixels(style_at(&from_px, frame).width),
                        "{name}: auto to px"
                    );
                }
                assert!(pixels(style_at(&from_auto, first).width) < 100.0, "{name}: moved on the first frame");
            }
        }

        #[test]
        fn only_width_motion_toward_auto_asks_for_the_natural_width() {
            let started = Instant::now();
            for (description, asks) in [
                (open(), false),
                (linear(serde_json::json!({ "opacity": 1.0 }), serde_json::json!({ "opacity": 0.0 })), false),
                (linear(serde_json::json!({ "width": 100.0 }), serde_json::json!({ "width": 0.0 })), false),
                (open_wide(), true),
            ] {
                let state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
                assert_eq!(state.needs_natural_width(started), asks, "{description}");
            }
        }

        fn width(state: &MotionState, now: Instant) -> Option<DimensionValue> {
            style_at(state, now).width
        }

        fn width_track(state: &MotionState) -> &Track {
            &state.tracks[&PropKey::Style(AnimProp::Width)]
        }

        fn grow_to_half() -> serde_json::Value {
            linear(
                serde_json::json!({ "width": "50%" }),
                serde_json::json!({ "width": 0.0 }),
            )
        }

        fn in_auto_parent(measurement: Measurement) -> Measurement {
            Measurement {
                percent_height_resolves: false,
                ..in_parent(measurement, 200.0, 80.0)
            }
        }

        fn grow_to_half_height() -> serde_json::Value {
            linear(
                serde_json::json!({ "height": "50%" }),
                serde_json::json!({ "height": 0.0 }),
            )
        }

        fn calc(px: f64, percent: f64) -> Option<DimensionValue> {
            Some(DimensionValue::Calc(LinearLength { px, percent, ..LinearLength::default() }))
        }

        #[test]
        fn a_px_start_blends_with_a_percentage_as_a_calc_with_no_measurement() {
            let started = Instant::now();
            let description = linear(
                serde_json::json!({ "width": "50%" }),
                serde_json::json!({ "width": 20.0 }),
            );
            let mut state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
            assert!(state.pending.is_empty());
            assert_eq!(width(&state, started), Some(DimensionValue::Pixels(20.0)));
            // In a 400px parent this is 20px * 0.5 + 200px * 0.5 = 110px, as in Chrome.
            let halfway = width(&state, at(started, 0.5));
            assert_eq!(halfway, calc(10.0, 0.25));
            let Some(DimensionValue::Calc(length)) = halfway else { unreachable!() };
            assert_eq!(crate::css::length::LengthContext::for_tests().resolve(&length, 400.0), 110.0);
            assert!(state.frame(at(started, 0.5)).active);

            let end = at(started, 1.0);
            state.sync(&description, None, Colors::root(), None, 0, end).unwrap();
            assert_eq!(width(&state, end), Some(DimensionValue::Percentage(0.5)));
            let frame = state.frame(end);
            assert!(!frame.active);
            assert!(frame.just_settled);
        }

        #[test]
        fn a_percentage_start_blends_to_px_and_settles_to_px() {
            let started = Instant::now();
            let description = linear(
                serde_json::json!({ "width": 20.0 }),
                serde_json::json!({ "width": "50%" }),
            );
            let mut state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
            assert_eq!(width(&state, started), Some(DimensionValue::Percentage(0.5)));
            assert_eq!(width(&state, at(started, 0.5)), calc(10.0, 0.25));
            let end = at(started, 1.0);
            state.sync(&description, None, Colors::root(), None, 0, end).unwrap();
            assert_eq!(width(&state, end), Some(DimensionValue::Pixels(20.0)));
        }

        #[test]
        fn calc_to_calc_mixes_the_percentage_and_the_px_on_their_own() {
            let started = Instant::now();
            let description = linear(
                serde_json::json!({ "width": "calc(50% + 10px)" }),
                serde_json::json!({ "width": "calc(100% - 20px)" }),
            );
            let state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
            let halfway = width(&state, at(started, 0.5));
            // calc(75% - 5px): 295px in a 400px parent.
            assert_eq!(halfway, calc(-5.0, 0.75));
            let Some(DimensionValue::Calc(length)) = halfway else { unreachable!() };
            let lengths = crate::css::length::LengthContext::for_tests();
            assert_eq!(lengths.resolve(&length, 400.0), 295.0);
            assert_eq!(width(&state, at(started, 1.0)), calc(10.0, 0.5));
        }

        #[test]
        fn a_parent_that_resizes_mid_animation_leaves_the_mix_alone_for_layout_to_resolve() {
            let started = Instant::now();
            let description = linear(
                serde_json::json!({ "width": "calc(50% + 10px)" }),
                serde_json::json!({ "width": "calc(100% - 20px)" }),
            );
            let mut state = MotionState::new(
                &description,
                None, Colors::root(),
                Some(&in_parent(measurement(0.0, 10.0, 10.0), 400.0, 80.0)),
                0,
                started,
            )
            .unwrap();
            let now = at(started, 0.5);
            let before = width_track(&state).sample(now).position;
            state
                .sync(&description, None, Colors::root(), Some(&in_parent(measurement(0.0, 10.0, 10.0), 600.0, 80.0)), 0, now)
                .unwrap();
            assert_eq!(width_track(&state).sample(now).position, before);
            let Some(DimensionValue::Calc(length)) = width(&state, now) else {
                panic!("a calc");
            };
            let lengths = crate::css::length::LengthContext::for_tests();
            assert_eq!(lengths.resolve(&length, 400.0), 295.0);
            assert_eq!(lengths.resolve(&length, 600.0), 445.0, "the mix of the new basis, which layout resolves");
        }

        #[test]
        fn a_start_the_style_leaves_to_layout_waits_for_a_measurement_and_then_blends() {
            let started = Instant::now();
            let description = linear(
                serde_json::json!({ "width": "50%" }),
                serde_json::json!({ "opacity": 1.0 }),
            );
            let mut state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
            assert_eq!(width(&state, started), None, "the element keeps the width it has");
            assert!(state.frame(started).active);

            let measured = in_parent(measurement(60.0, 10.0, 10.0), 200.0, 80.0);
            state.sync(&description, None, Colors::root(), Some(&measured), 0, started).unwrap();
            assert_eq!(pixels(width(&state, started)), 60.0);
            assert_eq!(width(&state, at(started, 0.5)), calc(30.0, 0.25));
            let end = at(started, 1.0);
            state.sync(&description, None, Colors::root(), Some(&measured), 0, end).unwrap();
            assert_eq!(width(&state, end), Some(DimensionValue::Percentage(0.5)));
        }

        #[test]
        fn percentages_on_both_ends_blend_as_percentages_without_a_measurement() {
            let started = Instant::now();
            let description = linear(
                serde_json::json!({ "width": "50%" }),
                serde_json::json!({ "width": "10%" }),
            );
            let state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
            assert!(state.pending.is_empty());
            let halfway = width(&state, at(started, 0.5));
            let Some(DimensionValue::Percentage(fraction)) = halfway else {
                panic!("expected a percentage, got {halfway:?}");
            };
            assert!((fraction - 0.3).abs() < 1e-9, "{fraction}");
        }

        #[test]
        fn a_relative_unit_is_px_from_the_elements_font_and_window() {
            let started = Instant::now();
            let mut colors = Colors::root();
            colors.lengths = crate::css::length::LengthContext::for_tests();
            let description = linear(
                serde_json::json!({ "width": "calc(100% - 2em)" }),
                serde_json::json!({ "width": "10vw" }),
            );
            let state = MotionState::new(&description, None, colors, None, 0, started).unwrap();
            // 1em is 20px and 1vw is 8px here.
            assert_eq!(width(&state, started), Some(DimensionValue::Pixels(80.0)));
            assert_eq!(width(&state, at(started, 0.5)), calc(20.0, 0.5));
            assert_eq!(width(&state, at(started, 1.0)), calc(-40.0, 1.0));
        }

        /// Runs `property` from 0 to `"50%"` for a second, linear, and reads it
        /// half-way.
        fn halfway_to_half(property: &str) -> (MotionState, Instant, serde_json::Value) {
            let started = Instant::now();
            let description = linear(
                serde_json::json!({ property: "50%" }),
                serde_json::json!({ property: 0.0 }),
            );
            let state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
            (state, started, description)
        }

        #[test]
        fn a_margin_percentage_is_a_percentage_on_every_side() {
            for property in ["marginTop", "marginRight", "marginBottom", "marginLeft"] {
                let (mut state, started, description) = halfway_to_half(property);
                let margin = |style: StyleDesc| match property {
                    "marginTop" => style.margin_top,
                    "marginRight" => style.margin_right,
                    "marginBottom" => style.margin_bottom,
                    _ => style.margin_left,
                };
                assert_eq!(
                    margin(style_at(&state, at(started, 0.5))),
                    Some(DimensionValue::Percentage(0.25)),
                    "{property}"
                );
                let end = at(started, 1.0);
                state.sync(&description, None, Colors::root(), None, 0, end).unwrap();
                assert_eq!(
                    margin(style_at(&state, end)),
                    Some(DimensionValue::Percentage(0.5)),
                    "{property}"
                );
            }
        }

        #[test]
        fn an_inset_percentage_is_a_percentage_on_every_side() {
            for property in ["top", "bottom", "left", "right"] {
                let (state, started, _) = halfway_to_half(property);
                let style = style_at(&state, at(started, 0.5));
                let inset = match property {
                    "top" => style.top,
                    "bottom" => style.bottom,
                    "left" => style.left,
                    _ => style.right,
                };
                assert_eq!(inset, Some(DimensionValue::Percentage(0.25)), "{property}");
            }
        }

        #[test]
        fn a_padding_percentage_is_a_percentage_on_every_side_and_settles_on_it() {
            for property in ["paddingTop", "paddingRight", "paddingBottom", "paddingLeft"] {
                let (state, started, _) = halfway_to_half(property);
                let padding = |style: StyleDesc| match property {
                    "paddingTop" => style.padding_top,
                    "paddingRight" => style.padding_right,
                    "paddingBottom" => style.padding_bottom,
                    _ => style.padding_left,
                };
                let middle = padding(style_at(&state, at(started, 0.5)));
                assert_eq!(middle, Some(LinearLength::percent(0.25)), "{property}");
                let settled = padding(style_at(&state, at(started, 1.0)));
                assert_eq!(settled, Some(LinearLength::percent(0.5)), "{property}");
            }
        }

        #[test]
        fn a_flex_basis_percentage_is_a_percentage() {
            let (state, started, _) = halfway_to_half("flexBasis");
            assert_eq!(
                style_at(&state, at(started, 0.5)).flex_basis,
                Some(DimensionValue::Percentage(0.25))
            );
        }

        #[test]
        fn a_spring_to_a_percentage_keeps_its_speed_when_the_parent_resizes() {
            let started = Instant::now();
            let description = serde_json::json!({
                "initial": { "width": 0.0 },
                "animate": { "width": "50%" },
                "transition": { "type": "spring", "stiffness": 200.0, "damping": 30.0 }
            });
            let mut state = MotionState::new(
                &description,
                None, Colors::root(),
                Some(&in_parent(measurement(0.0, 10.0, 10.0), 200.0, 80.0)),
                0,
                started,
            )
            .unwrap();
            let now = at(started, 0.15);
            let before = width_track(&state).sample(now);
            // The percentage channel is the second of a calc's two.
            assert!(before.velocity[1] > 0.0);
            state
                .sync(
                    &description,
                    None, Colors::root(),
                    Some(&in_parent(measurement(30.0, 10.0, 10.0), 400.0, 80.0)),
                    0,
                    now,
                )
                .unwrap();
            let after = width_track(&state).sample(now);
            assert_eq!(after.position, before.position);
            assert_eq!(after.velocity, before.velocity);
        }

        #[test]
        fn a_spring_between_a_px_and_a_calc_moves_each_channel_on_its_own() {
            let started = Instant::now();
            let description = serde_json::json!({
                "initial": { "width": 100.0 },
                "animate": { "width": "calc(50% + 10px)" },
                "transition": { "type": "spring", "stiffness": 200.0, "damping": 30.0 }
            });
            let state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
            let sample = width_track(&state).sample(at(started, 0.15));
            assert!(sample.velocity[0] < 0.0 && sample.velocity[1] > 0.0, "{:?}", sample.velocity);
            let settled = style_at(&state, at(started, 5.0)).width;
            assert_eq!(settled, calc(10.0, 0.5));
        }

        #[test]
        fn a_settled_percentage_is_where_the_next_animation_starts() {
            let started = Instant::now();
            let mut state =
                MotionState::new(&grow_to_half(), None, Colors::root(), None, 0, started).unwrap();
            let settled = at(started, 2.0);
            state.sync(&grow_to_half(), None, Colors::root(), None, 0, settled).unwrap();

            // The percentage is the start, not the px it was last seen as in some parent.
            let to_px = linear(serde_json::json!({ "width": 30.0 }), serde_json::json!(false));
            state.sync(&to_px, None, Colors::root(), None, 0, settled).unwrap();
            assert_eq!(width(&state, settled), Some(DimensionValue::Percentage(0.5)));
            assert_eq!(width(&state, at(settled, 0.5)), calc(15.0, 0.25));

            // And another percentage stays a percentage.
            let mut again =
                MotionState::new(&grow_to_half(), None, Colors::root(), None, 0, started).unwrap();
            again.sync(&grow_to_half(), None, Colors::root(), None, 0, settled).unwrap();
            let quarter = linear(serde_json::json!({ "width": "25%" }), serde_json::json!(false));
            again.sync(&quarter, None, Colors::root(), None, 0, settled).unwrap();
            let Some(DimensionValue::Percentage(fraction)) = width(&again, at(settled, 0.5)) else {
                panic!("expected a percentage");
            };
            assert!((fraction - 0.375).abs() < 1e-9, "{fraction}");
        }

        #[test]
        fn retargeting_a_running_percentage_continues_from_where_it_is() {
            let started = Instant::now();
            let mut state =
                MotionState::new(&grow_to_half(), None, Colors::root(), None, 0, started).unwrap();
            let now = at(started, 0.5);
            let back = linear(serde_json::json!({ "width": 0.0 }), serde_json::json!(false));
            state.sync(&back, None, Colors::root(), None, 0, now).unwrap();
            assert_eq!(width(&state, now), Some(DimensionValue::Percentage(0.25)));
            assert_eq!(width(&state, at(now, 0.5)), Some(DimensionValue::Percentage(0.125)));
        }

        #[test]
        fn a_percentage_height_stays_a_percentage_whatever_the_parent_is() {
            let started = Instant::now();
            let measured = in_auto_parent(measurement(100.0, 0.0, 90.0));
            let state = MotionState::new(
                &grow_to_half_height(), None, Colors::root(), Some(&measured), 0, started,
            )
            .unwrap();
            // An auto parent makes the whole `height` behave as auto in layout, as in CSS.
            assert_eq!(height(&state, at(started, 0.5)), Some(DimensionValue::Percentage(0.25)));
        }

        #[test]
        fn auto_on_a_min_or_max_size_is_an_error_that_names_the_limit() {
            for description in [
                serde_json::json!({ "animate": { "minHeight": "auto" } }),
                serde_json::json!({ "initial": { "maxWidth": "auto" }, "animate": { "maxWidth": 10.0 } }),
            ] {
                let error = MotionState::new(&description, None, Colors::root(), None, 0, Instant::now())
                    .err()
                    .unwrap();
                assert!(error.contains("auto") && error.contains("only width and height"), "{error}");
                assert!(!error.contains("mixed units"), "{error}");
            }
        }

        #[test]
        fn properties_layout_cannot_measure_still_jump() {
            let started = Instant::now();
            let state = MotionState::new(
                &serde_json::json!({
                    "initial": false,
                    "animate": { "top": 12.0 },
                    "transition": { "duration": 1.0 }
                }),
                None, Colors::root(),
                None,
                0,
                started,
            )
            .unwrap();
            assert_eq!(style_at(&state, started).top, Some(DimensionValue::Pixels(12.0)));
            assert!(state.pending.is_empty());
        }
    }
    /// Keyframes, `times`, per-segment `ease` and `repeat`.
    mod keyframes {
        use super::*;
        use serde_json::json;

        fn linear(animate: serde_json::Value, transition: serde_json::Value) -> serde_json::Value {
            let mut transition = transition;
            if transition.get("type") != Some(&json!("spring")) && transition.get("ease").is_none() {
                transition["ease"] = json!("linear");
            }
            json!({ "initial": { "width": 0.0 }, "animate": animate, "transition": transition })
        }

        fn run(width_frames: serde_json::Value, transition: serde_json::Value) -> (MotionState, Instant) {
            let started = Instant::now();
            let description = linear(json!({ "width": width_frames }), transition);
            (MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap(), started)
        }

        fn near(actual: f64, expected: f64) {
            assert!((actual - expected).abs() < 1e-6, "expected {expected}, got {actual}");
        }

        #[test]
        fn keyframes_are_evenly_spaced_and_the_last_one_is_where_it_settles() {
            let (mut state, started) = run(json!([0.0, 100.0, 50.0]), json!({ "duration": 2.0 }));
            near(width(&state, at(started, 0.5)), 50.0);
            near(width(&state, at(started, 1.0)), 100.0);
            near(width(&state, at(started, 1.5)), 75.0);
            assert!(state.frame(at(started, 1.5)).active);
            let end = state.frame(at(started, 2.0));
            assert!(!end.active);
            assert!(end.just_settled);
            near(width(&state, at(started, 2.0)), 50.0);
        }

        #[test]
        fn times_place_the_keyframes_and_hold_the_ends() {
            let (state, started) = run(
                json!([0.0, 100.0, 0.0]),
                json!({ "duration": 1.0, "times": [0.0, 0.25, 1.0] }),
            );
            near(width(&state, at(started, 0.125)), 50.0);
            near(width(&state, at(started, 0.25)), 100.0);
            near(width(&state, at(started, 0.625)), 50.0);

            let (state, started) = run(
                json!([0.0, 100.0]),
                json!({ "duration": 1.0, "times": [0.5, 0.75] }),
            );
            near(width(&state, at(started, 0.25)), 0.0);
            near(width(&state, at(started, 0.625)), 50.0);
            near(width(&state, at(started, 0.9)), 100.0);
        }

        #[test]
        fn ease_may_name_one_easing_per_segment() {
            let (state, started) = run(
                json!([0.0, 100.0, 0.0]),
                json!({ "duration": 1.0, "ease": ["linear", "easeIn"] }),
            );
            near(width(&state, at(started, 0.25)), 50.0);
            // The second segment eases in: it has covered less than half at its middle.
            let held = width(&state, at(started, 0.75));
            assert!(held > 60.0 && held < 75.0, "{held}");
        }

        #[test]
        fn a_null_first_keyframe_is_the_current_value_and_a_later_one_repeats_the_previous() {
            let started = Instant::now();
            let description = json!({
                "initial": { "width": 40.0 },
                "animate": { "width": [null, 100.0, null, 20.0] },
                "transition": { "duration": 3.0, "ease": "linear" }
            });
            let state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
            near(width(&state, started), 40.0);
            near(width(&state, at(started, 1.0)), 100.0);
            near(width(&state, at(started, 1.5)), 100.0);
            near(width(&state, at(started, 2.0)), 100.0);
            near(width(&state, at(started, 2.5)), 60.0);

            let explicit = json!({
                "initial": { "width": 40.0 },
                "animate": { "width": [0.0, 100.0] },
                "transition": { "duration": 1.0, "ease": "linear" }
            });
            let state = MotionState::new(&explicit, None, Colors::root(), None, 0, started).unwrap();
            near(width(&state, started), 0.0);
        }

        #[test]
        fn keyframes_animate_on_mount_without_an_initial_from_the_first_keyframe() {
            let started = Instant::now();
            let description = json!({
                "animate": { "width": [10.0, 110.0, 10.0], "opacity": [0.0, 1.0, 0.0] },
                "transition": { "duration": 2.0, "ease": "linear", "repeat": "Infinity" }
            });
            let mut state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
            near(width(&state, started), 10.0);
            near(width(&state, at(started, 0.5)), 60.0);
            near(width(&state, at(started, 1.0)), 110.0);
            near(width(&state, at(started, 2.5)), 60.0);
            near(opacity(&state, at(started, 0.5)), 0.5);
            near(opacity(&state, at(started, 2.5)), 0.5);
            for seconds in [0.5, 2.5, 4001.5] {
                let frame = state.frame(at(started, seconds));
                assert!(frame.active && !frame.just_settled, "{seconds}");
            }
        }

        #[test]
        fn initial_false_rests_on_the_last_keyframe_even_when_repeating() {
            let started = Instant::now();
            let description = json!({
                "initial": false,
                "animate": { "width": [0.0, 100.0, 30.0] },
                "transition": { "duration": 1.0, "repeat": "Infinity", "repeatType": "mirror" }
            });
            let mut state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
            assert_eq!(state.applied(started).unwrap().width, Some(30.0));
            assert_eq!(state.applied(at(started, 7.3)).unwrap().width, Some(30.0));
            assert!(!state.frame(started).active);
        }

        #[test]
        fn keyframes_resting_at_their_end_stay_there_when_only_initial_changes() {
            let started = Instant::now();
            let blocked = json!({
                "generation": 1,
                "initial": false,
                "animate": { "width": [0.0, 100.0, 30.0] },
                "transition": { "duration": 1.0 }
            });
            let mut state = MotionState::new(&blocked, None, Colors::root(), None, 0, started).unwrap();
            // What an AnimatePresence does after its first render: `initial: false` becomes unset.
            let unset = json!({
                "generation": 2,
                "animate": { "width": [0.0, 100.0, 30.0] },
                "transition": { "duration": 1.0 }
            });
            state.sync(&unset, None, Colors::root(), None, 0, at(started, 5.0)).unwrap();
            assert_eq!(state.applied(at(started, 5.5)).unwrap().width, Some(30.0));
            assert!(!state.frame(at(started, 5.5)).active);
        }

        #[test]
        fn an_initial_naming_other_properties_leaves_the_keyframes_animating() {
            let started = Instant::now();
            let description = json!({
                "initial": { "opacity": 0.25 },
                "animate": { "width": [0.0, 100.0] },
                "transition": { "duration": 1.0, "ease": "linear", "repeat": "Infinity" }
            });
            let state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
            near(width(&state, at(started, 0.5)), 50.0);
            near(opacity(&state, at(started, 0.5)), 0.25);
        }

        #[test]
        fn a_null_first_keyframe_starts_from_the_elements_own_style_without_an_initial() {
            let started = Instant::now();
            let base = StyleDesc { width: Some(DimensionValue::Pixels(40.0)), ..Default::default() };
            let description = json!({
                "animate": { "width": [null, 100.0, 20.0], "opacity": [null, 0.0] },
                "transition": { "duration": 2.0, "ease": "linear", "repeat": "Infinity", "repeatType": "reverse" }
            });
            let state = MotionState::new(&description, Some(&base), Colors::root(), None, 0, started).unwrap();
            near(width(&state, started), 40.0);
            near(width(&state, at(started, 1.0)), 100.0);
            near(width(&state, at(started, 2.5)), 60.0);
            // Unset opacity is CSS's initial value, 1.
            near(opacity(&state, at(started, 1.0)), 0.5);
            near(opacity(&state, at(started, 2.5)), 0.25);
        }

        #[test]
        fn a_plain_value_without_an_initial_animates_from_the_elements_own_style() {
            let started = Instant::now();
            let base = StyleDesc { width: Some(DimensionValue::Pixels(40.0)), ..Default::default() };
            let description = json!({
                "animate": { "width": 100.0, "opacity": 0.0 },
                "transition": { "duration": 1.0, "ease": "linear" }
            });
            let mut state = MotionState::new(&description, Some(&base), Colors::root(), None, 0, started).unwrap();
            near(width(&state, started), 40.0);
            near(width(&state, at(started, 0.5)), 70.0);
            near(opacity(&state, at(started, 0.5)), 0.5);
            assert!(state.frame(at(started, 0.5)).active);
            assert!(state.frame(at(started, 1.0)).just_settled);
        }

        #[test]
        fn a_value_already_at_its_target_does_not_keep_the_frames_coming() {
            let started = Instant::now();
            let base = StyleDesc { width: Some(DimensionValue::Pixels(100.0)), ..Default::default() };
            let description = json!({
                "animate": { "width": 100.0, "opacity": 1.0 },
                "transition": { "duration": 1.0 }
            });
            let mut state = MotionState::new(&description, Some(&base), Colors::root(), None, 0, started).unwrap();
            assert!(!state.frame(started).active);
        }

        #[test]
        fn initial_false_rests_on_the_last_keyframe() {
            let started = Instant::now();
            let description = json!({
                "initial": false,
                "animate": { "width": [0.0, 100.0, 30.0] },
                "transition": { "duration": 1.0 }
            });
            let mut state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
            assert_eq!(state.applied(started).unwrap().width, Some(30.0));
            assert!(!state.frame(started).active);
        }

        #[test]
        fn a_keyframe_list_is_rejected_where_a_value_is_expected() {
            let started = Instant::now();
            let description = json!({
                "initial": { "width": [0.0, 10.0] },
                "animate": { "width": 100.0 }
            });
            let error = MotionState::new(&description, None, Colors::root(), None, 0, started).err().unwrap();
            assert!(error.contains("initial cannot hold keyframes for width"), "{error}");
            let only_null = json!({ "animate": { "width": [null] } });
            assert!(MotionState::new(&only_null, None, Colors::root(), None, 0, started).is_err());
            let empty = json!({ "animate": { "width": [] } });
            assert!(MotionState::new(&empty, None, Colors::root(), None, 0, started).is_err());
        }

        #[test]
        fn a_single_keyframe_is_just_a_target() {
            let (state, started) = run(json!([100.0]), json!({ "duration": 1.0 }));
            near(width(&state, at(started, 0.5)), 50.0);
        }

        #[test]
        fn a_propertys_own_times_and_ease_must_match_its_keyframes() {
            let started = Instant::now();
            let fails = |transition: serde_json::Value| {
                let description = linear(
                    json!({ "width": [0.0, 1.0, 2.0] }),
                    json!({ "duration": 1.0, "width": transition }),
                );
                MotionState::new(&description, None, Colors::root(), None, 0, started).err().unwrap()
            };
            assert!(fails(json!({ "times": [0.0, 1.0] })).contains("times lists 2"));
            assert!(fails(json!({ "ease": ["linear"] })).contains("needs 2 easings"));
            assert!(fails(json!({ "times": [0.0, 0.8, 0.5] })).contains("ascending"));
            assert!(fails(json!({ "times": [0.0, 0.5, 1.5] })).contains("0 to 1"));
            assert!(fails(json!({ "type": "spring", "times": [0.0, 0.5, 1.0] })).contains("times"));
            // `default` is as shared as the top level: it is the one that is lenient.
            let shared = linear(
                json!({ "width": [0.0, 1.0, 2.0] }),
                json!({ "default": { "duration": 1.0, "times": [0.0, 1.0] } }),
            );
            assert!(MotionState::new(&shared, None, Colors::root(), None, 0, started).is_ok());
        }

        fn opacity(state: &MotionState, now: Instant) -> f64 {
            state.tracks[&PropKey::Style(AnimProp::Opacity)].sample(now).position[0]
        }

        #[test]
        fn a_shared_times_applies_only_to_the_properties_it_fits() {
            let started = Instant::now();
            for transition in [
                json!({ "duration": 1.0, "ease": "linear", "times": [0.0, 0.2, 1.0] }),
                json!({ "default": { "duration": 1.0, "ease": "linear", "times": [0.0, 0.2, 1.0] } }),
            ] {
                let description = json!({
                    "initial": { "width": 0.0, "opacity": 0.0 },
                    "animate": { "width": [0.0, 50.0, 100.0], "opacity": 1.0 },
                    "transition": transition
                });
                let state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
                near(width(&state, at(started, 0.1)), 25.0);
                near(width(&state, at(started, 0.2)), 50.0);
                near(opacity(&state, at(started, 0.2)), 0.2);
                near(opacity(&state, at(started, 0.5)), 0.5);
            }
        }

        #[test]
        fn a_shared_ease_list_applies_only_to_the_properties_it_fits() {
            let started = Instant::now();
            let description = json!({
                "initial": { "width": 0.0, "opacity": 0.0 },
                "animate": { "width": [0.0, 100.0, 0.0], "opacity": 1.0 },
                "transition": { "duration": 1.0, "ease": ["linear", "easeIn"] }
            });
            let state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
            near(width(&state, at(started, 0.25)), 50.0);
            // Opacity has one segment, so it takes the default ease, not "linear".
            let expected = curve::ease(0.5, &curve::Ease::Name("easeOut".to_string()));
            near(opacity(&state, at(started, 0.5)), expected);
        }

        #[test]
        fn a_propertys_own_transition_does_not_borrow_the_shared_times() {
            let started = Instant::now();
            let description = json!({
                "initial": { "width": 0.0 },
                "animate": { "width": [0.0, 100.0] },
                "transition": {
                    "times": [0.0, 0.2, 1.0],
                    "width": { "duration": 1.0, "ease": "linear" }
                }
            });
            let state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
            near(width(&state, at(started, 0.5)), 50.0);
        }

        #[test]
        fn a_spring_only_animates_between_two_keyframes() {
            let started = Instant::now();
            let three = linear(json!({ "width": [0.0, 1.0, 2.0] }), json!({ "type": "spring" }));
            let error = MotionState::new(&three, None, Colors::root(), None, 0, started).err().unwrap();
            assert!(error.contains("a spring only animates between two"), "{error}");
            let two = linear(json!({ "width": [0.0, 100.0] }), json!({ "type": "spring" }));
            assert!(MotionState::new(&two, None, Colors::root(), None, 0, started).is_ok());
        }

        #[test]
        fn repeat_loops_and_then_rests_on_the_last_keyframe() {
            let (mut state, started) = run(
                json!([0.0, 100.0]),
                json!({ "duration": 1.0, "repeat": 2 }),
            );
            near(width(&state, at(started, 0.5)), 50.0);
            // Motion's iteration owns its last instant, so the loop restarts just after it.
            near(width(&state, at(started, 1.0)), 100.0);
            near(width(&state, at(started, 1.001)), 0.1);
            near(width(&state, at(started, 1.5)), 50.0);
            near(width(&state, at(started, 2.75)), 75.0);
            assert!(state.frame(at(started, 2.75)).active);
            let end = state.frame(at(started, 3.0));
            assert!(!end.active && end.just_settled);
            near(width(&state, at(started, 3.0)), 100.0);
            near(width(&state, at(started, 10.0)), 100.0);
        }

        #[test]
        fn reverse_plays_back_through_time_and_mirror_through_the_keyframes() {
            let transition = |kind: &str| {
                json!({ "duration": 1.0, "ease": "easeIn", "repeat": 1, "repeatType": kind })
            };
            let easing = |progress: f64| {
                curve::ease(progress, &curve::Ease::Name("easeIn".to_string()))
            };
            let (reverse, started) = run(json!([0.0, 100.0]), transition("reverse"));
            near(width(&reverse, at(started, 1.25)), 100.0 * easing(0.75));
            near(width(&reverse, at(started, 2.0)), 0.0);

            let (mirror, started) = run(json!([0.0, 100.0]), transition("mirror"));
            near(width(&mirror, at(started, 1.25)), 100.0 - 100.0 * easing(0.25));
            near(width(&mirror, at(started, 2.0)), 0.0);

            let (looping, started) = run(json!([0.0, 100.0]), transition("loop"));
            near(width(&looping, at(started, 1.25)), 100.0 * easing(0.25));
            near(width(&looping, at(started, 2.0)), 100.0);
        }

        #[test]
        fn repeat_delay_holds_the_end_of_each_iteration() {
            let (mut state, started) = run(
                json!([0.0, 100.0]),
                json!({ "duration": 1.0, "repeat": 1, "repeatDelay": 0.5 }),
            );
            near(width(&state, at(started, 1.25)), 100.0);
            near(width(&state, at(started, 1.5)), 100.0);
            near(width(&state, at(started, 1.5005)), 0.05);
            near(width(&state, at(started, 2.0)), 50.0);
            assert!(state.frame(at(started, 2.4)).active);
            let end = state.frame(at(started, 2.5));
            assert!(!end.active && end.just_settled);
            near(width(&state, at(started, 2.5)), 100.0);
        }

        #[test]
        fn the_delay_comes_once_before_the_first_iteration() {
            let (state, started) = run(
                json!([0.0, 100.0]),
                json!({ "duration": 1.0, "delay": 2.0, "repeat": 1 }),
            );
            near(width(&state, at(started, 1.0)), 0.0);
            near(width(&state, at(started, 2.5)), 50.0);
            near(width(&state, at(started, 3.5)), 50.0);
        }

        #[test]
        fn an_infinite_repeat_never_settles() {
            let (mut state, started) = run(
                json!([0.0, 100.0]),
                json!({ "duration": 1.0, "repeat": "Infinity", "repeatType": "reverse" }),
            );
            for seconds in [0.5, 10.5, 1000.25, 1_000_000.75] {
                let frame = state.frame(at(started, seconds));
                assert!(frame.active && !frame.just_settled, "{seconds}");
            }
            near(width(&state, at(started, 1000.25)), 25.0);
            near(width(&state, at(started, 1001.25)), 75.0);
        }

        #[test]
        fn repeat_must_be_a_whole_count_or_infinity() {
            let started = Instant::now();
            for repeat in [json!(-1), json!(1.5), json!("forever"), json!("infinite")] {
                let description = linear(json!({ "width": 10.0 }), json!({ "repeat": repeat }));
                assert!(MotionState::new(&description, None, Colors::root(), None, 0, started).is_err(), "{repeat}");
            }
            let description = linear(json!({ "width": 10.0 }), json!({ "repeatType": "bounce" }));
            let error = MotionState::new(&description, None, Colors::root(), None, 0, started).err().unwrap();
            assert!(error.contains("repeatType"), "{error}");
        }

        #[test]
        fn a_zero_length_iteration_does_not_loop() {
            let (mut state, started) = run(
                json!([0.0, 100.0]),
                json!({ "duration": 0.0, "repeat": "Infinity" }),
            );
            assert!(!state.frame(at(started, 0.1)).active);
            near(width(&state, at(started, 0.1)), 100.0);
        }

        #[test]
        fn a_repeating_spring_replays_after_it_settles() {
            let spring = json!({ "type": "spring", "stiffness": 200.0, "damping": 20.0 });
            let mut looping = spring.clone();
            looping["repeat"] = json!(1);
            let (state, started) = run(json!([0.0, 100.0]), looping.clone());
            let settle = state.tracks[&PropKey::Style(AnimProp::Width)].settle;
            assert!(settle > 0.2 && settle < 2.0, "{settle}");
            assert!(width(&state, at(started, settle * 0.5)) > 20.0);
            assert!(width(&state, at(started, settle * 1.05)) < 20.0, "the second iteration starts over");
            near(width(&state, at(started, settle * 2.0 + 0.1)), 100.0);

            looping["repeatType"] = json!("reverse");
            let (state, started) = run(json!([0.0, 100.0]), looping);
            near(width(&state, at(started, settle * 2.0 + 0.1)), 0.0);

            let (mut once, started) = run(json!([0.0, 100.0]), spring);
            assert!(!once.frame(at(started, 5.0)).active);
        }

        #[test]
        fn velocity_at_a_retarget_is_the_slope_of_the_segment_under_way() {
            let (mut state, started) = run(json!([0.0, 100.0, 0.0]), json!({ "duration": 2.0 }));
            near(width_velocity(&state, at(started, 0.5)), 100.0);
            near(width_velocity(&state, at(started, 1.5)), -100.0);

            let retarget_at = at(started, 1.5);
            let spring = json!({
                "initial": false,
                "animate": { "width": 200.0 },
                "transition": { "type": "spring", "stiffness": 100.0, "damping": 10.0 }
            });
            state.sync(&spring, None, Colors::root(), None, 0, retarget_at).unwrap();
            near(width(&state, retarget_at), 50.0);
            near(width_velocity(&state, retarget_at), -100.0);
        }

        #[test]
        fn the_same_keyframes_keep_running_and_a_changed_one_restarts_from_where_it_is() {
            let (mut state, started) = run(json!([0.0, 100.0, 0.0]), json!({ "duration": 2.0 }));
            let same = json!({
                "generation": 1,
                "initial": false,
                "animate": { "width": [0.0, 100.0, 0.0] },
                "transition": { "duration": 2.0, "ease": "linear" }
            });
            state.sync(&same, None, Colors::root(), None, 0, at(started, 0.5)).unwrap();
            near(width(&state, at(started, 1.5)), 50.0);

            let changed = json!({
                "initial": false,
                "animate": { "width": [null, 20.0, 40.0] },
                "transition": { "duration": 1.0, "ease": "linear" }
            });
            state.sync(&changed, None, Colors::root(), None, 0, at(started, 0.5)).unwrap();
            near(width(&state, at(started, 0.5)), 50.0);
            near(width(&state, at(started, 1.0)), 20.0);
            near(width(&state, at(started, 1.5)), 40.0);
        }

        #[test]
        fn reduced_motion_jumps_width_keyframes_to_the_last_one_and_still_animates_the_rest() {
            let started = Instant::now();
            let description = json!({
                "reducedMotion": true,
                "initial": { "width": 0.0, "left": 0.0, "paddingLeft": 0.0, "opacity": 0.0 },
                "animate": {
                    "width": [0.0, 300.0, 100.0],
                    "left": [0.0, 30.0, 10.0],
                    "paddingLeft": [0.0, 20.0, 40.0],
                    "opacity": [0.0, 1.0, 0.5]
                },
                "transition": { "duration": 1.0, "ease": "linear", "repeat": "Infinity" }
            });
            let mut state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
            assert_eq!(state.applied(started).unwrap().width, Some(100.0));
            let middle = at(started, 0.25);
            assert!(state.frame(middle).active);
            assert_eq!(state.applied(middle).unwrap().width, Some(100.0));
            near(state.applied(middle).unwrap().opacity.unwrap(), 0.5);

            let mut style = StyleDesc::default();
            state.visible_style(middle).unwrap().apply_to(&mut style);
            assert_eq!(style.left, Some(DimensionValue::Pixels(10.0)));
            assert_eq!(style.padding_left, Some(LinearLength::px(10.0)));
        }

        fn measurement(parent_width: f32, natural: f32) -> Measurement {
            Measurement {
                border_box: gpui::size(gpui::px(100.0), gpui::px(0.0)),
                natural_height: gpui::px(natural),
                natural_width: Some(gpui::px(natural)),
                parent_content: gpui::size(gpui::px(parent_width), gpui::px(600.0)),
                scale_factor: 1.0,
                percent_height_resolves: true,
                flex_main: crate::motion::property::Axis::Width,
            }
        }

        fn pixels(value: Option<DimensionValue>) -> f64 {
            match value {
                Some(DimensionValue::Pixels(pixels)) => pixels,
                other => panic!("expected px, got {other:?}"),
            }
        }

        fn auto_keyframes() -> serde_json::Value {
            json!({
                "initial": { "height": 0.0 },
                "animate": { "height": [0.0, 50.0, "auto"] },
                "transition": { "duration": 1.0, "ease": "linear" }
            })
        }

        #[test]
        fn auto_as_the_last_keyframe_runs_to_the_content_and_settles_to_auto() {
            let started = Instant::now();
            let mut state = MotionState::new(&auto_keyframes(), None, Colors::root(), None, 0, started).unwrap();
            assert!(state.frame(started).active, "waits for a measurement");
            assert_eq!(pixels(style_at(&state, started).height), 0.0);

            let measured_at = at(started, 0.2);
            let measured = measurement(800.0, 80.0);
            state.sync(&auto_keyframes(), None, Colors::root(), Some(&measured), 0, measured_at).unwrap();
            assert_eq!(pixels(style_at(&state, at(started, 0.5)).height), 50.0);
            assert_eq!(pixels(style_at(&state, at(started, 0.75)).height), 65.0);

            let end = at(started, 1.0);
            state.sync(&auto_keyframes(), None, Colors::root(), Some(&measured), 0, end).unwrap();
            let frame = state.frame(end);
            assert!(!frame.active && frame.just_settled);
            assert_eq!(style_at(&state, end).height, Some(DimensionValue::Auto));
        }

        fn wide_keyframes() -> serde_json::Value {
            json!({
                "initial": { "width": 0.0 },
                "animate": { "width": [0.0, 50.0, "auto"] },
                "transition": { "duration": 1.0, "ease": "linear" }
            })
        }

        #[test]
        fn auto_as_the_last_width_keyframe_runs_to_the_natural_width_and_settles_to_auto() {
            let started = Instant::now();
            let mut state = MotionState::new(&wide_keyframes(), None, Colors::root(), None, 0, started).unwrap();
            assert!(state.frame(started).active, "waits for a measurement");
            assert!(state.needs_natural_width(started));

            let measured_at = at(started, 0.2);
            let measured = Measurement {
                natural_width: Some(gpui::px(80.0)),
                ..measurement(800.0, 0.0)
            };
            state.sync(&wide_keyframes(), None, Colors::root(), Some(&measured), 0, measured_at).unwrap();
            assert_eq!(pixels(style_at(&state, at(started, 0.5)).width), 50.0);
            assert_eq!(pixels(style_at(&state, at(started, 0.75)).width), 65.0);

            let end = at(started, 1.0);
            state.sync(&wide_keyframes(), None, Colors::root(), Some(&measured), 0, end).unwrap();
            assert!(state.frame(end).just_settled);
            assert_eq!(style_at(&state, end).width, Some(DimensionValue::Auto));
        }

        #[test]
        fn a_measured_keyframe_follows_the_content_without_restarting_the_timing() {
            let started = Instant::now();
            let measured = measurement(800.0, 80.0);
            let mut state =
                MotionState::new(&auto_keyframes(), None, Colors::root(), Some(&measured), 0, started).unwrap();
            assert_eq!(pixels(style_at(&state, at(started, 0.75)).height), 65.0);

            let grown = measurement(800.0, 120.0);
            state.sync(&auto_keyframes(), None, Colors::root(), Some(&grown), 0, at(started, 0.5)).unwrap();
            assert_eq!(pixels(style_at(&state, at(started, 0.5)).height), 50.0);
            assert_eq!(pixels(style_at(&state, at(started, 0.75)).height), 85.0);
        }

        #[test]
        fn percentages_among_px_keyframes_blend_as_calcs_and_settle_to_the_percentage() {
            let started = Instant::now();
            let description = json!({
                "initial": { "width": 0.0 },
                "animate": { "width": [0.0, "50%", 200.0, "25%"] },
                "transition": { "duration": 3.0, "ease": "linear" }
            });
            let mut state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
            assert!(state.pending.is_empty(), "no measurement is needed");
            let width = |seconds: f64| style_at(&state, at(started, seconds)).width;
            let near = |value: Option<DimensionValue>, px: f64, percent: f64| {
                let Some(DimensionValue::Calc(length)) = value else {
                    panic!("expected a calc, got {value:?}");
                };
                assert!((length.px - px).abs() < 1e-9 && (length.percent - percent).abs() < 1e-9, "{length:?}");
            };
            assert_eq!(width(0.5), Some(DimensionValue::Percentage(0.25)));
            assert_eq!(width(1.0), Some(DimensionValue::Percentage(0.5)));
            near(width(1.5), 100.0, 0.25);
            assert_eq!(width(2.0), Some(DimensionValue::Pixels(200.0)));
            near(width(2.5), 100.0, 0.125);
            let end = at(started, 3.0);
            state.sync(&description, None, Colors::root(), None, 0, end).unwrap();
            assert!(!state.frame(end).active);
            assert_eq!(style_at(&state, end).width, Some(DimensionValue::Percentage(0.25)));
        }
        #[test]
        fn keyframes_of_one_unit_need_no_layout() {
            let started = Instant::now();
            let description = json!({
                "initial": { "width": "0%" },
                "animate": { "width": ["0%", "50%", "100%"] },
                "transition": { "duration": 2.0, "ease": "linear" }
            });
            let state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
            assert!(state.pending.is_empty());
            assert_eq!(
                style_at(&state, at(started, 1.0)).width,
                Some(DimensionValue::Percentage(0.5))
            );
        }

        #[test]
        fn a_mounted_keyframe_animation_that_is_never_painted_jumps_to_the_last_keyframe() {
            let started = Instant::now();
            let mut state = MotionState::new(&auto_keyframes(), None, Colors::root(), None, 3, started).unwrap();
            state.sync(&auto_keyframes(), None, Colors::root(), None, 4, at(started, 0.1)).unwrap();
            assert!(!state.frame(at(started, 0.1)).active);
            assert_eq!(style_at(&state, at(started, 0.1)).height, Some(DimensionValue::Auto));
        }

        mod svg {
            use super::*;

            fn svg(animate: serde_json::Value, transition: serde_json::Value) -> (MotionState, Instant) {
                let started = Instant::now();
                let description = json!({ "initial": false, "animate": animate, "transition": transition });
                let state = MotionState::new_svg(&description, &HashMap::new(), Colors::root(), started).unwrap();
                (state, started)
            }

            fn text(state: &MotionState, started: Instant, seconds: f64, name: &str) -> String {
                let motion = state.visible_svg(at(started, seconds)).unwrap();
                motion
                    .attributes
                    .iter()
                    .find(|(attribute, _)| *attribute == name)
                    .unwrap_or_else(|| panic!("no {name} in {motion:?}"))
                    .1
                    .clone()
            }

            #[test]
            fn attributes_take_keyframes_and_repeat() {
                let started = Instant::now();
                let description = json!({
                    "initial": { "strokeWidth": 0.0 },
                    "animate": { "strokeWidth": [null, 10.0, 0.0], "d": ["M0 0 L10 0", "M0 0 L20 0", "M0 0 L10 0"] },
                    "transition": { "duration": 2.0, "ease": "linear", "repeat": "Infinity" }
                });
                let mut state =
                    MotionState::new_svg(&description, &HashMap::new(), Colors::root(), started).unwrap();
                assert_eq!(text(&state, started, 0.5, "stroke-width"), "5");
                assert_eq!(text(&state, started, 1.0, "stroke-width"), "10");
                assert_eq!(text(&state, started, 5.5, "stroke-width"), "5");
                assert_eq!(text(&state, started, 1.0, "d"), "M 0 0 L 20 0");
                assert!(state.frame(at(started, 100.0)).active);
            }

            #[test]
            fn a_dash_list_is_still_one_value_and_lists_of_lists_are_keyframes() {
                let (state, started) = svg(
                    json!({ "strokeDasharray": [4.0, 2.0] }),
                    json!({ "duration": 1.0 }),
                );
                assert_eq!(text(&state, started, 0.0, "stroke-dasharray"), "4 2");

                let (state, started) = svg(
                    json!({ "strokeDasharray": [[0.0, 10.0], [10.0, 0.0], "5 5"] }),
                    json!({ "duration": 1.0, "ease": "linear" }),
                );
                assert_eq!(text(&state, started, 0.0, "stroke-dasharray"), "5 5");
                let started = Instant::now();
                let description = json!({
                    "initial": { "strokeDasharray": [0.0, 10.0] },
                    "animate": { "strokeDasharray": [null, [10.0, 0.0], "5 5"] },
                    "transition": { "duration": 2.0, "ease": "linear" }
                });
                let state = MotionState::new_svg(&description, &HashMap::new(), Colors::root(), started).unwrap();
                assert_eq!(text(&state, started, 0.5, "stroke-dasharray"), "5 5");
                assert_eq!(text(&state, started, 1.5, "stroke-dasharray"), "7.5 2.5");
            }

            #[test]
            fn paths_that_cannot_blend_flip_between_the_keyframes() {
                let (state, started) = svg(
                    json!({ "d": ["M0 0 L10 0", "M0 0 H20", "M0 0 L30 0"] }),
                    json!({ "duration": 0.0 }),
                );
                assert_eq!(text(&state, started, 1.0, "d"), "M 0 0 L 30 0");
                let started = Instant::now();
                let description = json!({
                    "initial": { "d": "M0 0 L10 0" },
                    "animate": { "d": [null, "M0 0 H20", "M0 0 L30 0"] },
                    "transition": { "duration": 4.0, "ease": "linear" }
                });
                let state = MotionState::new_svg(&description, &HashMap::new(), Colors::root(), started).unwrap();
                assert_eq!(text(&state, started, 0.9, "d"), "M 0 0 L 10 0");
                assert_eq!(text(&state, started, 1.1, "d"), "M 0 0 H 20");
                assert_eq!(text(&state, started, 3.1, "d"), "M 0 0 L 30 0");
            }
        }

        #[test]
        fn a_shadow_list_is_one_value_and_keyframes_wrap_each_list() {
            let started = Instant::now();
            let layer = |blur: f64| json!({
                "offsetX": 0.0, "offsetY": 0.0, "blurRadius": blur, "spreadRadius": 0.0,
                "color": "#000000"
            });
            let description = json!({
                "initial": { "boxShadow": [layer(0.0)] },
                "animate": { "boxShadow": [null, [layer(10.0)], [layer(0.0), layer(4.0)]] },
                "transition": { "duration": 2.0, "ease": "linear" }
            });
            let state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
            let shadows = |seconds: f64| match style_at(&state, at(started, seconds)).box_shadow {
                Some(crate::style::BoxShadowValues::Many(layers)) => layers,
                other => panic!("expected a list, got {other:?}"),
            };
            near(shadows(0.5)[0].blur_radius.as_px().unwrap(), 5.0);
            assert_eq!(shadows(1.5).len(), 2);
            near(shadows(1.5)[0].blur_radius.as_px().unwrap(), 5.0);

            let list = json!({ "animate": { "boxShadow": [layer(1.0), layer(2.0)] } });
            let state = MotionState::new(&list, None, Colors::root(), None, 0, started).unwrap();
            assert_eq!(state.tracks.len(), 1);
            assert!(state.tracks[&PropKey::Style(AnimProp::BoxShadow)].goal().is_single());
        }
    }

    mod colour_context {
        use serde_json::json;

        use super::*;
        use crate::color::{ColorContext, Scheme};

        fn parent(scheme: Scheme, color: &str) -> ColorContext {
            ColorContext::root(scheme).descend(&StyleDesc {
                color: Some(color.to_string()),
                color_scheme: Some(match scheme {
                    Scheme::Light => "light",
                    Scheme::Dark => "dark",
                }
                .to_string()),
                ..Default::default()
            })
        }

        fn near(actual: [f64; 4], expected: &str) {
            let expected = rgba(expected);
            for (actual, expected) in actual.iter().zip(expected) {
                assert!((actual - expected).abs() < 0.01, "{actual:?} vs {expected:?}");
            }
        }

        fn description() -> serde_json::Value {
            json!({
                "initial": { "backgroundColor": "#000000", "borderColor": "#000000", "color": "#000000" },
                "animate": {
                    "backgroundColor": "light-dark(#ff0000, #0000ff)",
                    "borderColor": "currentColor",
                    "color": "currentColor"
                },
                "transition": { "duration": 1.0, "ease": "linear" }
            })
        }

        fn end_of(state: &MotionState, started: Instant) -> StyleDesc {
            style_at(state, at(started, 1.0))
        }

        fn channels(style: &StyleDesc) -> ([f64; 4], [f64; 4], [f64; 4]) {
            (
                rgba(style.background_color.as_deref().unwrap()),
                rgba(style.border_top_color.as_deref().unwrap()),
                rgba(style.color.as_deref().unwrap()),
            )
        }

        #[test]
        fn targets_resolve_in_the_context_of_the_element() {
            let started = Instant::now();
            let base = StyleDesc { color: Some("#00ff00".to_string()), ..Default::default() };
            let above = parent(Scheme::Dark, "#808080");
            let colors = Colors::of(&above, Some(&base));
            let state = MotionState::new(&description(), Some(&base), colors, None, 0, started).unwrap();
            let (background, border, color) = channels(&end_of(&state, started));
            // `light-dark()` takes the dark side; a border's `currentColor` is the
            // element's own colour; `color: currentColor` is the parent's.
            near(background, "#0000ff");
            near(border, "#00ff00");
            near(color, "#808080");
        }

        #[test]
        fn an_unset_color_starts_from_the_inherited_one() {
            let started = Instant::now();
            let description = json!({
                "initial": { "opacity": 1.0 },
                "animate": { "color": "#0000ff" },
                "transition": { "duration": 1.0, "ease": "linear" }
            });
            let colors = Colors::of(&parent(Scheme::Light, "#ff0000"), None);
            let state = MotionState::new(&description, None, colors, None, 0, started).unwrap();
            near(rgba(style_at(&state, started).color.as_deref().unwrap()), "#ff0000");
            near(rgba(style_at(&state, at(started, 1.0)).color.as_deref().unwrap()), "#0000ff");

            // With no `color` above either, it is the `CanvasText` of the element's scheme.
            let base = StyleDesc { color_scheme: Some("dark".to_string()), ..Default::default() };
            let canvas = Colors::of(&ColorContext::root(Scheme::Light), Some(&base));
            let state = MotionState::new(&description, Some(&base), canvas, None, 0, started).unwrap();
            near(rgba(style_at(&state, started).color.as_deref().unwrap()), "#ffffff");
        }

        #[test]
        fn an_unset_font_size_and_weight_start_from_the_computed_ones() {
            let started = Instant::now();
            let description = json!({
                "initial": { "opacity": 1.0 },
                "animate": { "fontSize": 40, "fontWeight": 300 },
                "transition": { "duration": 1.0, "ease": "linear" }
            });
            let parent_font = ParentValues {
                lengths: crate::css::length::LengthContext::initial(800.0, 600.0)
                    .descend(Some(&crate::css::length::LinearLength::px(20.0)), true),
                above_root: false,
                weight: 700.0,
                variations: Default::default(),
                inheritable: Default::default(),
                custom: Default::default(),
            };
            let colors = Colors::inheriting(&ColorContext::root(Scheme::Light), &parent_font, None);
            let state = MotionState::new(&description, None, colors.clone(), None, 0, started).unwrap();
            let font_at = |at_time: Instant| {
                let style = style_at(&state, at_time);
                (
                    AnimProp::FontSize.read(&style, &colors),
                    AnimProp::FontWeight.read(&style, &colors),
                )
            };
            assert_eq!(font_at(started), (Some(AnimValue::Number(20.0)), Some(AnimValue::Number(700.0))));
            assert_eq!(font_at(at(started, 0.5)), (Some(AnimValue::Number(30.0)), Some(AnimValue::Number(500.0))));
            assert_eq!(font_at(at(started, 1.0)), (Some(AnimValue::Number(40.0)), Some(AnimValue::Number(300.0))));

            // A declared `em` size is relative to the parent's, whatever the style says.
            let base = StyleDesc { font_size: Some(crate::css::length::LinearLength { em: 2.0, ..Default::default() }), ..Default::default() };
            let colors = Colors::inheriting(&ColorContext::root(Scheme::Light), &parent_font, Some(&base));
            let state = MotionState::new(&description, Some(&base), colors.clone(), None, 0, started).unwrap();
            assert_eq!(AnimProp::FontSize.read(&style_at(&state, started), &colors), Some(AnimValue::Number(40.0)));
        }

        #[test]
        fn color_mix_and_color_function_targets_animate() {
            let started = Instant::now();
            let description = json!({
                "initial": { "backgroundColor": "#000000", "borderColor": "#000000" },
                "animate": {
                    "backgroundColor": "color-mix(in srgb, red 50%, blue)",
                    "borderColor": "color(srgb 0 1 0)"
                },
                "transition": { "duration": 1.0, "ease": "linear" }
            });
            let state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
            let style = end_of(&state, started);
            near(rgba(style.background_color.as_deref().unwrap()), "#800080");
            near(rgba(style.border_top_color.as_deref().unwrap()), "#00ff00");
        }

        #[test]
        fn an_appearance_change_resolves_the_targets_again() {
            let started = Instant::now();
            let light = Colors::of(&parent(Scheme::Light, "#000000"), None);
            let dark = Colors::of(&parent(Scheme::Dark, "#000000"), None);
            let mut state = MotionState::new(&description(), None, light, None, 0, started).unwrap();
            near(channels(&end_of(&state, started)).0, "#ff0000");

            // Same description, other appearance: the track runs to the new side.
            let flipped = at(started, 2.0);
            state.sync(&description(), None, dark, None, 0, flipped).unwrap();
            near(channels(&style_at(&state, flipped)).0, "#ff0000");
            near(channels(&style_at(&state, at(started, 3.0))).0, "#0000ff");
        }

        #[test]
        fn an_unchanged_context_keeps_the_running_tracks() {
            let started = Instant::now();
            let colors = Colors::of(&parent(Scheme::Light, "#000000"), None);
            let mut state = MotionState::new(&description(), None, colors.clone(), None, 0, started).unwrap();
            let middle = at(started, 0.5);
            let before = channels(&style_at(&state, middle)).0;
            state.sync(&description(), None, colors.clone(), None, 0, middle).unwrap();
            assert_eq!(channels(&style_at(&state, middle)).0, before);
            near(channels(&end_of(&state, started)).0, "#ff0000");
        }

        #[test]
        fn svg_fill_and_stroke_resolve_in_the_context() {
            let started = Instant::now();
            let description = json!({
                "initial": { "fill": "#000000", "stroke": "#000000" },
                "animate": { "fill": "light-dark(#ff0000, #0000ff)", "stroke": "currentColor" },
                "transition": { "duration": 1.0, "ease": "linear" }
            });
            let above = parent(Scheme::Dark, "#00ff00");
            let colors = Colors::of(&above, None);
            let state = MotionState::new_svg(&description, &HashMap::new(), colors, started).unwrap();
            let motion = state.visible_svg(at(started, 1.0)).unwrap();
            let attribute = |name: &str| {
                rgba(&motion.attributes.iter().find(|(attribute, _)| *attribute == name).unwrap().1)
            };
            near(attribute("fill"), "#0000ff");
            near(attribute("stroke"), "#00ff00");
        }

        #[test]
        fn motion_values_resolve_against_the_window_appearance() {
            let now = Instant::now();
            let mut store = MotionValueStore::default();
            store.set_appearance(Scheme::Dark);
            store.create_source(1.0, &json!("light-dark(#ffffff, #0000ff)")).unwrap();
            let value = store.get(1.0, now).unwrap();
            near(rgba(value.as_str().unwrap()), "#0000ff");
        }
    }

    mod variations {
        use serde_json::json;

        use super::*;
        use crate::color::{ColorContext, Scheme};

        #[test]
        fn font_variation_settings_animate_per_axis_from_initial_to_animate() {
            let started = Instant::now();
            let description = json!({
                "initial": { "fontVariationSettings": "'wght' 400, 'wdth' 100" },
                "animate": { "fontVariationSettings": "'wdth' 50, 'wght' 800" },
                "transition": { "duration": 1.0, "ease": "linear" }
            });
            let colors = Colors::root();
            let state = MotionState::new(&description, None, colors, None, 0, started).unwrap();
            let settings = |at_time: Instant| {
                String::from(style_at(&state, at_time).font_variation_settings.expect("animated"))
            };
            assert_eq!(settings(started), r#""wdth" 100, "wght" 400"#);
            assert_eq!(settings(at(started, 0.5)), r#""wdth" 75, "wght" 600"#);
            assert_eq!(settings(at(started, 1.0)), r#""wdth" 50, "wght" 800"#);
        }

        #[test]
        fn font_variation_settings_flip_at_half_when_the_axes_differ() {
            let started = Instant::now();
            let description = json!({
                "initial": { "fontVariationSettings": "'wght' 400" },
                "animate": { "fontVariationSettings": "normal" },
                "transition": { "duration": 1.0, "ease": "linear" }
            });
            let state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
            let settings = |at_time: Instant| {
                String::from(style_at(&state, at_time).font_variation_settings.expect("animated"))
            };
            assert_eq!(settings(at(started, 0.49)), r#""wght" 400"#);
            assert_eq!(settings(at(started, 0.5)), "normal");
        }

        #[test]
        fn font_variation_settings_keyframes_and_an_unset_start() {
            let started = Instant::now();
            let description = json!({
                "initial": { "opacity": 1.0 },
                "animate": { "fontVariationSettings": [null, "'wght' 900", "'wght' 100"] },
                "transition": { "duration": 2.0, "ease": "linear" }
            });
            let parent_font = ParentValues {
                variations: gpui::FontVariations::new([(u32::from_be_bytes(*b"wght"), 300.0)]),
                ..ParentValues::initial()
            };
            let colors = Colors::inheriting(&ColorContext::root(Scheme::Light), &parent_font, None);
            let state = MotionState::new(&description, None, colors, None, 0, started).unwrap();
            let settings = |seconds: f64| {
                String::from(style_at(&state, at(started, seconds)).font_variation_settings.expect("animated"))
            };
            assert_eq!(settings(0.0), r#""wght" 300"#);
            assert_eq!(settings(0.5), r#""wght" 600"#);
            assert_eq!(settings(1.0), r#""wght" 900"#);
            assert_eq!(settings(1.5), r#""wght" 500"#);
        }

        #[test]
        fn a_font_variation_settings_target_that_is_not_settings_is_an_error() {
            let description = json!({ "animate": { "fontVariationSettings": "wght 700" } });
            let error = MotionState::new(&description, None, Colors::root(), None, 0, Instant::now()).err().unwrap();
            assert!(error.contains("fontVariationSettings"), "{error}");
        }
    }
    mod custom_properties {
        use super::*;
        use crate::color::{ColorContext, Scheme};
        use crate::css::custom_properties::CustomProperties;

        fn style(json: &str) -> StyleDesc {
            serde_json::from_str(json).expect("a style")
        }

        fn parent(custom: &[(&str, &str)]) -> ParentValues {
            let declared = CustomProperties::declared(custom.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())));
            ParentValues {
                custom: declared.map_or_else(CustomProperties::default, |declared| CustomProperties::default().descend(&declared)),
                ..ParentValues::initial()
            }
        }

        /// The style a frame at `millis` gives the element, with `custom` above it.
        fn frame(transitions: &mut CssTransitions, started: Instant, millis: u64, style: &StyleDesc, custom: &[(&str, &str)]) -> DeclaredStyle {
            resolve_declared_style(
                Some(transitions),
                None,
                None,
                &ColorContext::root(Scheme::Light),
                &parent(custom),
                Default::default(),
                None,
                1,
                style,
                FocusVisible::default(),
                None,
                InterpolateSize::default(),
                Scope::Element,
                started + Duration::from_millis(millis),
            )
        }

        #[test]
        fn a_transition_of_a_transform_with_a_var_runs_from_the_last_computed_value_to_the_new_one() {
            let started = Instant::now();
            let read = |text: &str| style(&format!(r#"{{"transform":"{text}","transition":"transform 1s linear"}}"#));
            let var = read("translateX(calc(100px * var(--p)))");
            let mut with_var = CssTransitions::default();
            let mut literal = CssTransitions::default();
            for (millis, p) in [(0, "0.2"), (16, "0.2")] {
                assert!(frame(&mut with_var, started, millis, &var, &[("--p", p)]).transitions.is_none());
            }
            let began = frame(&mut with_var, started, 100, &var, &[("--p", "0.9")]);
            assert!(began.transitions.is_some(), "a new computed value starts a transition");
            for (millis, text) in [(0, "translateX(calc(100px * 0.2))"), (16, "translateX(calc(100px * 0.2))")] {
                frame(&mut literal, started, millis, &read(text), &[]);
            }
            frame(&mut literal, started, 100, &read("translateX(calc(100px * 0.9))"), &[]);
            // A frame later, with the same --p: the transition goes on from where it was, not from the new value.
            for millis in [600, 1100] {
                let tick = frame(&mut with_var, started, millis, &var, &[("--p", "0.9")]);
                let want = frame(&mut literal, started, millis, &read("translateX(calc(100px * 0.9))"), &[]);
                let text = |style: StyleDesc| style.transform.map(|text| text.replace(' ', ""));
                assert_eq!(text(tick.style), text(want.style), "at {millis}");
            }
        }
    }
}
