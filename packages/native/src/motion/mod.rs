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
mod keyframes;
mod layout_flip;
pub(crate) mod measure;
mod property;
mod svg_attr;
mod track;
mod value;
mod values;

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use serde_json::{Map, Value};
use web_time::Instant;

use crate::style::StyleDesc;
pub(crate) use curve::cubic_bezier;
pub(crate) use css_animation::{validate_animations, AnimationEvent, CssAnimations};
pub(crate) use keyframes::{validate_keyframes, KeyframesRegistry};
pub(crate) use layout_flip::{drag_value_ids, LayoutEvent, LayoutFlips};
pub(crate) use css_transition::{
    declares_transitions, validate_transitions, CssTransitions, InterpolateSize, TransitionEvent,
    TransitionStyle,
};
pub(crate) use bindings::{MotionValueEvent, MotionValueFrame, MotionValueStore};
use curve::Transition;
use measure::Measurement;
use property::AnimProp;
use svg_attr::SvgAttr;
use track::{Goal, Track};
pub(crate) use value::Colors;
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
enum PropKey {
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

    fn is_list_value(self, items: &[Value]) -> bool {
        match self {
            Self::Style(prop) => prop.is_list_value(items),
            Self::Svg(attr) => attr.is_list_value(items),
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

    /// `reduced` replaces the transition of every position and size property
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
        // Shorthands first, so a longhand named beside them is the more specific.
        by_prop.sort_by_key(|(key, _)| shorthand(target, key).is_none());
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
            for prop in AnimProp::ALL.iter().filter(|prop| prop.is_layout()) {
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

struct Description {
    generation: u64,
    is_exit: bool,
    /// `None` for `initial: false` or no `initial`: nothing animates on mount.
    initial: Option<Styles>,
    animate: Goals,
    transition: Transitions,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MotionFrame {
    pub active: bool,
    pub just_settled: bool,
    pub generation: u64,
}

/// An animation that cannot begin until its element has been measured: it
/// starts from the laid-out value, or runs to `height: auto`. The clock starts
/// on the first frame a measurement exists.
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
        Self::mount(source, Base::Style(base, &colors), measurement, painted, now)
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
        Self::mount(source, Base::Svg(attributes, &colors), None, 0, now)
    }

    fn mount(
        source: &Value,
        base: Base,
        measurement: Option<&Measurement>,
        painted: u64,
        now: Instant,
    ) -> Result<Self, String> {
        let description = parse_description(source, base.target(), base.colors())?;
        let (tracks, pending) = mount_tracks(&description, base, measurement, painted, now)?;
        Ok(Self {
            source: source.clone(),
            colors: *base.colors(),
            needs_settle: description.is_exit
                || !pending.is_empty()
                || tracks.values().any(Track::has_motion),
            tracks,
            pending,
            valid: true,
            generation: description.generation,
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
        self.retarget(source, Base::Style(base, &colors), measurement, painted, now)
    }

    pub(crate) fn sync_svg(
        &mut self,
        source: &Value,
        attributes: &HashMap<String, Value>,
        colors: Colors,
        now: Instant,
    ) -> Result<(), String> {
        self.retarget(source, Base::Svg(attributes, &colors), None, 0, now)
    }

    fn retarget(
        &mut self,
        source: &Value,
        base: Base,
        measurement: Option<&Measurement>,
        painted: u64,
        now: Instant,
    ) -> Result<(), String> {
        self.resolve_pending(measurement, painted, now)?;
        if self.source == *source && self.colors == *base.colors() {
            return self.follow_layout(measurement, now);
        }

        let previous_generation = self.generation;
        self.colors = *base.colors();
        let description = match parse_description(source, base.target(), base.colors()) {
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
            let start = match (measurement, waiting.from.clone()) {
                (None, _) if painted <= waiting.painted => {
                    self.pending.insert(prop, waiting);
                    continue;
                }
                (None, _) => Start::Rest,
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
                now,
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
            if !track.is_measured() || !track.sample(now).active {
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

    /// Whether this description animates `prop`, which then is not a CSS
    /// transition's to animate.
    fn owns(&self, prop: AnimProp) -> bool {
        let key = PropKey::Style(prop);
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

/// The state styles `fold_state_styles` can lay over `style`, by the name a
/// validation error gives them. A transition or animation declared in any of
/// them has to take part, or it never runs.
pub(super) fn state_styles(style: &StyleDesc) -> [(&'static str, Option<&StyleDesc>); 6] {
    [
        ("hover", style.hover.as_deref()),
        ("active", style.active.as_deref()),
        ("focusVisible", style.focus_visible.as_deref()),
        ("groupHover", style.group_hover.as_deref()),
        ("groupActive", style.group_active.as_deref()),
        ("groupFocusWithin", style.group_focus_within.as_deref()),
    ]
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
pub(crate) fn resolve_declared_style(
    mut transitions: Option<&mut CssTransitions>,
    before: Option<&StyleDesc>,
    parent_colors: &crate::color::ColorContext,
    pointer: crate::events::state_styles::PointerStates,
    motion: Option<&MotionState>,
    id: u64,
    style: &StyleDesc,
    focus_visible: bool,
    measurement: Option<&Measurement>,
    interpolate_size: InterpolateSize,
    now: Instant,
) -> DeclaredStyle {
    if let (Some(transitions), Some(before)) = (transitions.as_deref_mut(), before) {
        transitions.seed(id, &fold_state_styles(before, focus_visible, pointer), parent_colors);
    }
    let mut after = fold_state_styles(style, focus_visible, pointer);
    let running = transitions.and_then(|transitions| {
        transitions.update(id, &after, parent_colors, motion, measurement, interpolate_size, now)
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

fn fold_state_styles(
    style: &StyleDesc,
    focus_visible: bool,
    pointer: crate::events::state_styles::PointerStates,
) -> StyleDesc {
    use crate::events::state_styles::resolve_pointer_states;

    let focused = match style.focus_visible.as_deref() {
        // `focusVisible` rides through the fold as a `hover` of its own, so the
        // fold's field rules apply to it; the real `hover` and `active` come
        // back for the second pass.
        Some(declared) if focus_visible => {
            let mut carrier = style.clone();
            carrier.hover = Some(Box::new(declared.clone()));
            carrier.active = None;
            let mut folded = resolve_pointer_states(
                &carrier,
                crate::events::state_styles::PointerStates {
                    hovered: true,
                    ..Default::default()
                },
            );
            folded.hover = style.hover.clone();
            folded.active = style.active.clone();
            folded.group_hover = style.group_hover.clone();
            folded.group_active = style.group_active.clone();
            folded.group_focus_within = style.group_focus_within.clone();
            std::borrow::Cow::Owned(folded)
        }
        // Borrowed: `resolve_pointer_states` copies the style itself, so cloning
        // it here too paid for a second copy of a ~3.5 kB struct on every frame.
        _ => std::borrow::Cow::Borrowed(style),
    };
    let mut after = resolve_pointer_states(&focused, pointer);
    after.focus_visible = None;
    after
}

/// What an animation starts from when `initial` does not name the property:
/// the element's own style, else CSS's initial value. `None` when that is
/// `auto` or inherited, which only layout can resolve.
fn resolve_start(key: PropKey, base: Base) -> Option<AnimValue> {
    match (key, base) {
        (PropKey::Style(prop), Base::Style(style, colors)) => style
            .and_then(|style| prop.read(style, colors))
            .or_else(|| prop.initial(colors)),
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

/// The px a target that layout has to resolve stands for: `height: auto` is
/// the natural height, a percentage a share of the parent's content box.
fn measured_end(
    prop: AnimProp,
    target: &AnimValue,
    measurement: &Measurement,
) -> Result<AnimValue, String> {
    match target {
        AnimValue::Auto => Ok(AnimValue::Number(measurement.natural_height_px())),
        AnimValue::Percent(fraction) => in_pixels(prop, &AnimValue::Percent(*fraction), measurement),
        other => unreachable!("{other:?} is not resolved through layout"),
    }
}

/// `value` in px: a percentage resolved against the parent's content box.
fn in_pixels(
    prop: AnimProp,
    value: &AnimValue,
    measurement: &Measurement,
) -> Result<AnimValue, String> {
    let AnimValue::Percent(fraction) = value else {
        return Ok(value.clone());
    };
    measurement
        .percent_px(prop, *fraction)
        .map(AnimValue::Number)
        .ok_or_else(|| {
            format!(
                "motion cannot resolve {} as a percentage: only sizes, margins and insets are measured against the parent",
                prop.name()
            )
        })
}

/// Builds the segment from `start` to `target`, or the wait for the
/// measurement it needs. A percentage meeting a px length runs through px, and
/// the style goes back to the percentage when the segment ends.
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
        let Some(measurement) = measurement else {
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
    let mixed = matches!(
        (&from, target),
        (AnimValue::Number(_), AnimValue::Percent(_))
            | (AnimValue::Percent(_), AnimValue::Number(_))
    );
    if !mixed {
        return Track::new(name, &from, target.clone(), transition.clone(), now).map(Begun::Track);
    }
    let PropKey::Style(prop) = key else {
        unreachable!("only a style property holds a percentage");
    };
    let Some(measurement) = measurement else {
        return Ok(wait(Some(from)));
    };
    let from = in_pixels(prop, &from, measurement)?;
    let to = in_pixels(prop, target, measurement)?;
    Track::resolved(name, &from, target.clone(), to, transition.clone(), now).map(Begun::Track)
}

/// Keyframes: each one that layout has to resolve is measured, and the style
/// goes back to the literal last one when the track ends.
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
        })
    };
    let last = goal.last();
    let at_end = || {
        if *last == AnimValue::Auto {
            return Ok(Begun::Track(Track::resting_at_auto(transition.clone(), now)));
        }
        Track::new(name, last, last.clone(), transition.clone(), now).map(Begun::Track)
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
/// own, and a percentage meeting a px length is a share of the parent's box.
fn needs_layout(literal: &[AnimValue]) -> bool {
    let has = |wanted: fn(&AnimValue) -> bool| literal.iter().any(wanted);
    has(|value| *value == AnimValue::Auto)
        || (has(|value| matches!(value, AnimValue::Number(_)))
            && has(|value| matches!(value, AnimValue::Percent(_))))
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
            AnimValue::Auto | AnimValue::Percent(_) => measured_end(prop, value, measurement),
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
            None => Start::Rest,
            Some(initial) => start_for(*prop, initial.get(prop), base),
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
    for (prop, value) in description.initial.iter().flatten() {
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
        None | Some(Value::Null) | Some(Value::Bool(false)) => None,
        Some(Value::Object(style)) => Some(parse_initial(style, target, colors)?),
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
    Ok(Description {
        generation,
        is_exit,
        initial,
        animate,
        transition: Transitions::parse(fields.get("transition"), target, reduced)?,
    })
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

/// Expands shorthands (`padding`, `margin`, `borderRadius`, `borderWidth`,
/// `gap`) into longhands. Where both are given the longhand wins, as the more
/// specific one does in CSS.
fn parse_goals(
    context: &str,
    style: &Map<String, Value>,
    target: Target,
    colors: &Colors,
) -> Result<Goals, String> {
    let mut goals = Goals::new();
    let mut longhands = Vec::new();
    for (key, value) in style {
        if let Some(props) = shorthand(target, key) {
            let parsed = parse_goal(PropKey::Style(props[0]), value, colors)
                .map_err(|error| rename(error, props[0], key))?;
            for prop in props {
                goals.insert(PropKey::Style(*prop), parsed.clone());
            }
        } else if let Some(prop) = PropKey::from_name(target, key) {
            longhands.push((prop, value));
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
    for (prop, value) in longhands {
        goals.insert(prop, parse_goal(prop, value, colors)?);
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

/// A shorthand is validated through its first longhand, so name the shorthand
/// the author wrote in the error.
fn rename(error: String, prop: AnimProp, shorthand: &str) -> String {
    error.replacen(prop.name(), shorthand, 1)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::css::length::LinearLength;
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
    fn a_padding_or_gap_target_must_be_px_and_the_shorthand_error_names_the_shorthand() {
        let now = Instant::now();
        for (animate, name) in [
            (serde_json::json!({ "paddingLeft": "calc(10% + 4px)" }), "paddingLeft"),
            (serde_json::json!({ "padding": "2em" }), "padding"),
            (serde_json::json!({ "columnGap": "1rem" }), "columnGap"),
            (serde_json::json!({ "gap": "10%" }), "gap"),
            (serde_json::json!({ "flexBasis": "calc(100% - 40px)" }), "flexBasis"),
        ] {
            let description = serde_json::json!({ "animate": animate, "transition": {} });
            let error = MotionState::new(&description, None, Colors::root(), None, 0, now).err().unwrap();
            assert!(error.contains(name) && error.contains("CSS length"), "{error}");
        }
        let px = serde_json::json!({ "animate": { "padding": 8, "gap": 4 }, "transition": {} });
        assert!(MotionState::new(&px, None, Colors::root(), None, 0, now).is_ok());
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
    fn reduced_motion_jumps_position_and_size_but_still_animates_the_rest() {
        let started = Instant::now();
        let mut state = MotionState::new(&reduced_description(true), None, Colors::root(), None, 0, started).unwrap();
        let mut style = StyleDesc::default();
        state.visible_style(started).unwrap().apply_to(&mut style);
        assert_eq!(style.width, Some(DimensionValue::Pixels(100.0)));
        assert_eq!(style.padding_left, Some(LinearLength::px(40.0)));
        assert_eq!(style.opacity, Some(0.0));

        let middle = started + Duration::from_millis(500);
        let frame = state.frame(middle);
        assert!(frame.active);
        assert_eq!(state.applied(middle).unwrap().opacity, Some(0.5));
        assert_eq!(state.applied(middle).unwrap().width, Some(100.0));

        let end = started + Duration::from_secs(1);
        let frame = state.frame(end);
        assert!(!frame.active);
        assert!(frame.just_settled);
    }

    #[test]
    fn reduced_motion_on_layout_alone_settles_on_the_first_frame() {
        let started = Instant::now();
        let source = serde_json::json!({
            "generation": 1,
            "reducedMotion": true,
            "initial": { "height": 0.0, "marginTop": 0.0, "gap": 0.0 },
            "animate": { "height": 50.0, "marginTop": 8.0, "gap": 4.0 },
            "transition": { "type": "spring" }
        });
        let mut state = MotionState::new(&source, None, Colors::root(), None, 0, started).unwrap();
        let mut style = StyleDesc::default();
        state.visible_style(started).unwrap().apply_to(&mut style);
        assert_eq!(style.height, Some(DimensionValue::Pixels(50.0)));
        assert_eq!(style.margin_top, Some(DimensionValue::Pixels(8.0)));
        assert_eq!(style.row_gap, Some(LinearLength::px(4.0)));
        let frame = state.frame(started);
        assert!(!frame.active);
        assert!(frame.just_settled);
        assert_eq!(frame.generation, 1);
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
            serde_json::json!({ "initial": { "width": "auto" }, "animate": { "width": 10.0 } }),
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
            border_top_left_radius: Some(20.0),
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
        let state = MotionState::new(&description, None, Colors::root(), None, 0, now).unwrap();
        let style = style_at(&state, now);
        assert_eq!(style.padding_left, Some(LinearLength::px(3.0)));
        assert_eq!(style.padding_top, Some(LinearLength::px(9.0)));
    }

    #[test]
    fn starts_from_the_elements_own_style_when_initial_omits_the_key() {
        let started = Instant::now();
        let base = StyleDesc {
            padding: Some(LinearLength::px(40.0)),
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
            "animate": { "opacity": 1.0, "top": 12.0, "borderColor": "#ff0000" },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        let state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
        let style = style_at(&state, started);
        assert_eq!(style.top, Some(DimensionValue::Pixels(12.0)));
        // A colour has a start without layout: `currentColor`, inherited black.
        assert_eq!(rgba(style.border_color.as_deref().unwrap()), rgba("#000000"));
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
        let rest = fold_state_styles(&style, false, Default::default());
        assert_eq!(rest.color.as_deref(), Some("#000000"));

        let focused = fold_state_styles(&style, true, Default::default());
        assert_eq!(focused.color.as_deref(), Some("#111111"));
        assert_eq!(focused.opacity, Some(0.9));

        let both = fold_state_styles(
            &style,
            true,
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
        }
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
        let folded = fold_state_styles(&style, false, states);
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
                parent_content: gpui::size(gpui::px(800.0), gpui::px(600.0)),
                scale_factor: 1.0,
                percent_height_resolves: true,
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
            assert!(state.owns(AnimProp::Height), "a transition must not take it over");

            // The clock starts at the first frame that has a measurement.
            let measured_at = at(started, 0.3);
            let measured = measurement(100.0, 0.0, 80.0);
            state.sync(&open(), None, Colors::root(), Some(&measured), 0, measured_at).unwrap();
            assert!(state.frame(measured_at).active);
            assert_eq!(pixels(height(&state, measured_at)), 0.0);
            assert_eq!(pixels(height(&state, at(measured_at, 0.5))), 40.0);

            let end = at(measured_at, 1.0);
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
            assert_eq!(pixels(height(&state, at(measured_at, 0.5))), 40.0);
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
            let halfway = style_at(&state, at(measured_at, 0.5));
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

        #[test]
        fn a_px_start_runs_to_a_percentage_through_px_and_settles_to_the_percentage() {
            let started = Instant::now();
            let measured = in_parent(measurement(0.0, 10.0, 10.0), 200.0, 80.0);
            let mut state =
                MotionState::new(&grow_to_half(), None, Colors::root(), Some(&measured), 0, started).unwrap();
            assert_eq!(pixels(width(&state, started)), 0.0);
            assert_eq!(pixels(width(&state, at(started, 0.5))), 50.0);
            assert!(state.frame(at(started, 0.5)).active);

            let end = at(started, 1.0);
            state.sync(&grow_to_half(), None, Colors::root(), Some(&measured), 0, end).unwrap();
            assert_eq!(width(&state, end), Some(DimensionValue::Percentage(0.5)));
            let frame = state.frame(end);
            assert!(!frame.active);
            assert!(frame.just_settled);
        }

        #[test]
        fn a_percentage_start_runs_to_px_and_settles_to_px() {
            let started = Instant::now();
            let description = linear(
                serde_json::json!({ "width": 20.0 }),
                serde_json::json!({ "width": "50%" }),
            );
            let measured = in_parent(measurement(100.0, 10.0, 10.0), 200.0, 80.0);
            let mut state =
                MotionState::new(&description, None, Colors::root(), Some(&measured), 0, started).unwrap();
            assert_eq!(pixels(width(&state, started)), 100.0);
            assert_eq!(pixels(width(&state, at(started, 0.5))), 60.0);
            let end = at(started, 1.0);
            state.sync(&description, None, Colors::root(), Some(&measured), 0, end).unwrap();
            assert_eq!(width(&state, end), Some(DimensionValue::Pixels(20.0)));
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
        fn a_mixed_start_waits_for_a_measurement_showing_the_start() {
            let started = Instant::now();
            let description = linear(
                serde_json::json!({ "width": 40.0 }),
                serde_json::json!({ "width": "50%" }),
            );
            let mut state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
            assert_eq!(width(&state, started), Some(DimensionValue::Percentage(0.5)));
            assert!(state.frame(started).active);

            let measured_at = at(started, 0.3);
            let measured = in_parent(measurement(100.0, 10.0, 10.0), 200.0, 80.0);
            state.sync(&description, None, Colors::root(), Some(&measured), 0, measured_at).unwrap();
            assert!(state.pending.is_empty());
            assert_eq!(pixels(width(&state, measured_at)), 100.0);
            assert_eq!(pixels(width(&state, at(measured_at, 0.5))), 70.0);
        }

        #[test]
        fn a_percentage_target_from_an_unset_start_runs_from_the_laid_out_size() {
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
            assert_eq!(pixels(width(&state, at(started, 0.5))), 80.0);
            let end = at(started, 1.0);
            state.sync(&description, None, Colors::root(), Some(&measured), 0, end).unwrap();
            assert_eq!(width(&state, end), Some(DimensionValue::Percentage(0.5)));
        }

        #[test]
        fn the_percentage_end_follows_the_parent_while_it_runs() {
            let started = Instant::now();
            let mut state = MotionState::new(
                &grow_to_half(),
                None, Colors::root(),
                Some(&in_parent(measurement(0.0, 10.0, 10.0), 200.0, 80.0)),
                0,
                started,
            )
            .unwrap();
            let now = at(started, 0.5);
            let resized = in_parent(measurement(50.0, 10.0, 10.0), 400.0, 80.0);
            state.sync(&grow_to_half(), None, Colors::root(), Some(&resized), 0, now).unwrap();
            assert_eq!(pixels(width(&state, now)), 50.0, "the value showing does not jump");
            assert_eq!(pixels(width(&state, at(started, 1.0))), 125.0);
            let end = at(started, 1.5);
            state.sync(&grow_to_half(), None, Colors::root(), Some(&resized), 0, end).unwrap();
            assert_eq!(width(&state, end), Some(DimensionValue::Percentage(0.5)));
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

        #[test]
        fn a_percentage_height_in_an_auto_parent_runs_to_the_natural_height_and_settles_to_the_percentage() {
            let started = Instant::now();
            let measured = in_auto_parent(measurement(100.0, 0.0, 90.0));
            let description = grow_to_half_height();
            let mut state =
                MotionState::new(&description, None, Colors::root(), Some(&measured), 0, started).unwrap();
            // The parent's 80px is not what 50% is here.
            assert_eq!(pixels(height(&state, at(started, 0.5))), 45.0);
            let end = at(started, 1.0);
            state.sync(&description, None, Colors::root(), Some(&measured), 0, end).unwrap();
            assert_eq!(height(&state, end), Some(DimensionValue::Percentage(0.5)));
        }

        #[test]
        fn the_natural_height_end_follows_the_content_of_an_auto_parent() {
            let started = Instant::now();
            let description = grow_to_half_height();
            let mut state = MotionState::new(
                &description,
                None, Colors::root(),
                Some(&in_auto_parent(measurement(100.0, 0.0, 80.0))),
                0,
                started,
            )
            .unwrap();
            let now = at(started, 0.5);
            let grown = in_auto_parent(measurement(100.0, 40.0, 160.0));
            state.sync(&description, None, Colors::root(), Some(&grown), 0, now).unwrap();
            assert_eq!(pixels(height(&state, now)), 40.0, "the value showing does not jump");
            assert_eq!(pixels(height(&state, at(started, 1.0))), 100.0, "halfway from 40 to the new 160");
        }

        #[test]
        fn a_percentage_height_in_a_definite_parent_is_a_share_of_it() {
            let started = Instant::now();
            let measured = in_parent(measurement(100.0, 0.0, 90.0), 200.0, 80.0);
            let state = MotionState::new(
                &grow_to_half_height(), None, Colors::root(), Some(&measured), 0, started,
            )
            .unwrap();
            assert_eq!(pixels(height(&state, at(started, 0.5))), 20.0);
        }

        /// Runs `property` from 0 to `"50%"` for a second, linear, in a parent
        /// 200 wide and 80 tall, and reads it half-way.
        fn halfway_to_half(property: &str) -> (MotionState, Instant, serde_json::Value) {
            let started = Instant::now();
            let description = linear(
                serde_json::json!({ property: "50%" }),
                serde_json::json!({ property: 0.0 }),
            );
            let measured = in_parent(measurement(0.0, 10.0, 10.0), 200.0, 80.0);
            let state =
                MotionState::new(&description, None, Colors::root(), Some(&measured), 0, started).unwrap();
            (state, started, description)
        }

        #[test]
        fn a_margin_percentage_is_a_share_of_the_parent_width_on_every_side() {
            for property in ["marginTop", "marginRight", "marginBottom", "marginLeft"] {
                let (state, started, description) = halfway_to_half(property);
                let mut state = state;
                let margin = |style: StyleDesc| match property {
                    "marginTop" => style.margin_top,
                    "marginRight" => style.margin_right,
                    "marginBottom" => style.margin_bottom,
                    _ => style.margin_left,
                };
                assert_eq!(pixels(margin(style_at(&state, at(started, 0.5)))), 50.0, "{property}");
                let end = at(started, 1.0);
                let measured = in_parent(measurement(0.0, 10.0, 10.0), 200.0, 80.0);
                state.sync(&description, None, Colors::root(), Some(&measured), 0, end).unwrap();
                assert_eq!(
                    margin(style_at(&state, end)),
                    Some(DimensionValue::Percentage(0.5)),
                    "{property}"
                );
            }
        }

        #[test]
        fn an_inset_percentage_is_a_share_of_the_parent_height_for_top_and_bottom_and_width_for_left_and_right() {
            for (property, halfway) in [("top", 20.0), ("bottom", 20.0), ("left", 50.0), ("right", 50.0)] {
                let (state, started, _) = halfway_to_half(property);
                let style = style_at(&state, at(started, 0.5));
                let inset = match property {
                    "top" => style.top,
                    "bottom" => style.bottom,
                    "left" => style.left,
                    _ => style.right,
                };
                assert_eq!(pixels(inset), halfway, "{property}");
            }
        }

        #[test]
        fn a_margin_percentage_end_follows_the_parent_width_while_it_runs() {
            let (mut state, started, description) = halfway_to_half("marginLeft");
            let now = at(started, 0.5);
            let resized = in_parent(measurement(0.0, 10.0, 10.0), 400.0, 80.0);
            state.sync(&description, None, Colors::root(), Some(&resized), 0, now).unwrap();
            assert_eq!(pixels(style_at(&state, now).margin_left), 50.0, "the value showing does not jump");
            assert_eq!(pixels(style_at(&state, at(started, 1.0)).margin_left), 125.0);
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
            assert!(before.velocity[0] > 0.0);
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
            assert!((after.position[0] - before.position[0]).abs() < 1e-9);
            assert!((after.velocity[0] - before.velocity[0]).abs() < 1e-9);
        }

        #[test]
        fn a_settled_percentage_is_where_the_next_animation_starts() {
            let started = Instant::now();
            let measured = in_parent(measurement(0.0, 10.0, 10.0), 200.0, 80.0);
            let mut state =
                MotionState::new(&grow_to_half(), None, Colors::root(), Some(&measured), 0, started).unwrap();
            let settled = at(started, 2.0);
            state.sync(&grow_to_half(), None, Colors::root(), Some(&measured), 0, settled).unwrap();

            // The parent changed since: the percentage is 150px of 300px now,
            // not the 100px the segment ended on.
            let wider = in_parent(measurement(150.0, 10.0, 10.0), 300.0, 80.0);
            let to_px = linear(serde_json::json!({ "width": 30.0 }), serde_json::json!(false));
            state.sync(&to_px, None, Colors::root(), Some(&wider), 0, settled).unwrap();
            assert_eq!(pixels(width(&state, settled)), 150.0);
            assert_eq!(pixels(width(&state, at(settled, 0.5))), 90.0);

            // And another percentage stays a percentage.
            let mut again =
                MotionState::new(&grow_to_half(), None, Colors::root(), Some(&measured), 0, started).unwrap();
            again.sync(&grow_to_half(), None, Colors::root(), Some(&measured), 0, settled).unwrap();
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
            let measured = in_parent(measurement(0.0, 10.0, 10.0), 200.0, 80.0);
            let mut state =
                MotionState::new(&grow_to_half(), None, Colors::root(), Some(&measured), 0, started).unwrap();
            let now = at(started, 0.5);
            let back = linear(serde_json::json!({ "width": 0.0 }), serde_json::json!(false));
            state.sync(&back, None, Colors::root(), Some(&measured), 0, now).unwrap();
            assert_eq!(pixels(width(&state, now)), 50.0);
            assert_eq!(pixels(width(&state, at(now, 0.5))), 25.0);
        }

        #[test]
        fn heights_and_widths_resolve_against_their_own_parent_axis() {
            let started = Instant::now();
            let description = linear(
                serde_json::json!({ "height": "50%", "maxWidth": "50%" }),
                serde_json::json!({ "height": 0.0, "maxWidth": 0.0 }),
            );
            let measured = in_parent(measurement(0.0, 0.0, 0.0), 200.0, 80.0);
            let state =
                MotionState::new(&description, None, Colors::root(), Some(&measured), 0, started).unwrap();
            let halfway = style_at(&state, at(started, 0.5));
            assert_eq!(halfway.height, Some(DimensionValue::Pixels(20.0)));
            assert_eq!(halfway.max_width, Some(DimensionValue::Pixels(50.0)));
        }

        #[test]
        fn a_root_resolves_a_percentage_against_the_window() {
            let started = Instant::now();
            let state = MotionState::new(
                &grow_to_half(),
                None, Colors::root(),
                Some(&measurement(0.0, 10.0, 10.0)),
                0,
                started,
            )
            .unwrap();
            assert_eq!(pixels(style_at(&state, at(started, 0.5)).width), 200.0);
        }

        #[test]
        fn auto_width_is_an_error_that_names_the_limit() {
            for description in [
                serde_json::json!({ "animate": { "width": "auto" } }),
                serde_json::json!({ "initial": { "width": "auto" }, "animate": { "width": 10.0 } }),
                serde_json::json!({ "animate": { "minHeight": "auto" } }),
            ] {
                let error = MotionState::new(&description, None, Colors::root(), None, 0, Instant::now())
                    .err()
                    .unwrap();
                assert!(error.contains("auto") && error.contains("only height"), "{error}");
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
        fn mounting_without_an_initial_rests_on_the_last_keyframe() {
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
            near(width(&state, at(started, 1.0)), 0.0);
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
            near(width(&state, at(started, 1.5)), 0.0);
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
        fn reduced_motion_jumps_layout_keyframes_to_the_last_one_and_still_animates_the_rest() {
            let started = Instant::now();
            let description = json!({
                "reducedMotion": true,
                "initial": { "width": 0.0, "opacity": 0.0 },
                "animate": { "width": [0.0, 300.0, 100.0], "opacity": [0.0, 1.0, 0.5] },
                "transition": { "duration": 1.0, "ease": "linear", "repeat": "Infinity" }
            });
            let mut state = MotionState::new(&description, None, Colors::root(), None, 0, started).unwrap();
            assert_eq!(state.applied(started).unwrap().width, Some(100.0));
            let middle = at(started, 0.25);
            assert!(state.frame(middle).active);
            assert_eq!(state.applied(middle).unwrap().width, Some(100.0));
            near(state.applied(middle).unwrap().opacity.unwrap(), 0.5);
        }

        fn measurement(parent_width: f32, natural: f32) -> Measurement {
            Measurement {
                border_box: gpui::size(gpui::px(100.0), gpui::px(0.0)),
                natural_height: gpui::px(natural),
                parent_content: gpui::size(gpui::px(parent_width), gpui::px(600.0)),
                scale_factor: 1.0,
                percent_height_resolves: true,
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
            assert_eq!(pixels(style_at(&state, at(measured_at, 0.5)).height), 50.0);
            assert_eq!(pixels(style_at(&state, at(measured_at, 0.75)).height), 65.0);

            let end = at(measured_at, 1.0);
            state.sync(&auto_keyframes(), None, Colors::root(), Some(&measured), 0, end).unwrap();
            let frame = state.frame(end);
            assert!(!frame.active && frame.just_settled);
            assert_eq!(style_at(&state, end).height, Some(DimensionValue::Auto));
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
        fn percentages_among_px_keyframes_run_through_px_and_settle_to_the_percentage() {
            let started = Instant::now();
            let description = json!({
                "initial": { "width": 0.0 },
                "animate": { "width": [0.0, "50%", 200.0, "25%"] },
                "transition": { "duration": 3.0, "ease": "linear" }
            });
            let measured = measurement(800.0, 0.0);
            let mut state = MotionState::new(&description, None, Colors::root(), Some(&measured), 0, started).unwrap();
            assert_eq!(pixels(style_at(&state, at(started, 1.0)).width), 400.0);
            assert_eq!(pixels(style_at(&state, at(started, 2.0)).width), 200.0);
            assert_eq!(pixels(style_at(&state, at(started, 2.5)).width), 200.0);
            let end = at(started, 3.0);
            state.sync(&description, None, Colors::root(), Some(&measured), 0, end).unwrap();
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
            near(shadows(0.5)[0].blur_radius, 5.0);
            assert_eq!(shadows(1.5).len(), 2);
            near(shadows(1.5)[0].blur_radius, 5.0);

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
                rgba(style.border_color.as_deref().unwrap()),
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
            near(rgba(style.border_color.as_deref().unwrap()), "#00ff00");
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
            let mut state = MotionState::new(&description(), None, colors, None, 0, started).unwrap();
            let middle = at(started, 0.5);
            let before = channels(&style_at(&state, middle)).0;
            state.sync(&description(), None, colors, None, 0, middle).unwrap();
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
}
