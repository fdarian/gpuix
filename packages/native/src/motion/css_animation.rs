//! CSS animation declarations (CSS Animations Level 1): the `animation`
//! shorthand and its eight longhands, parsed into one [`Animation`] per name.
//!
//! A bad value is rejected when the style is set ([`validate_animations`]), so
//! it fails where it is written instead of silently never running. A name with
//! no keyframes defined is not an error, as in CSS: it just animates nothing.
//!
//! The runtime ([`CssAnimations`]) follows CSS Animations Level 1 and the Web
//! Animations timing model under it: each frame the renderer hands an element's
//! declared style and the style beneath the animations to
//! [`CssAnimations::update`], which returns the values to lay over it. Those
//! win over everything else the element declares, as animations do in CSS.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use web_time::Instant;

use super::css_transition::{
    check_seconds, is_timing_function, parse_delay, parse_duration, parse_list, parse_time,
    split_outside_parens, split_whitespace_outside_parens, TransitionStyle,
};
use super::curve::{ease, parse_timing_function, Curve, Ease, Repeat, Transition};
use super::keyframes::{Keyframes, KeyframesRegistry};
use super::measure::Measurement;
use super::property::{AnimProp, Scope};
use super::svg_attr::{attribute_text, SvgAttr};
use super::track::{Goal, Track};
use super::value::{AnimValue, Colors, Interpolation, ParentFont};
use super::web_animation::{ApiEvent, ApiEventKind, Command, Info, Kind, Playback};
use super::{needs_layout, resolve_keyframes, InterpolateSize, PropKey};
use crate::color::ColorContext;
use crate::retained_tree::RetainedTree;
use crate::style::StyleDesc;
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum IterationCount {
    /// Never negative. Fractions run a partial last iteration.
    Finite(f64),
    Infinite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Direction {
    Normal,
    Reverse,
    Alternate,
    AlternateReverse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FillMode {
    None,
    Forwards,
    Backwards,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PlayState {
    Running,
    Paused,
}

/// One entry of an element's `animation-*` lists, with the lists already
/// repeated or truncated to the length of `animation-name`.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Animation {
    /// `None` for `animation-name: none`: the slot exists but animates nothing.
    pub name: Option<String>,
    /// Seconds, never negative. `auto` is `0s`, as CSS Animations 1 treats it.
    pub duration: f64,
    /// Seconds. A negative delay starts the animation part-way through.
    pub delay: f64,
    pub timing: Ease,
    pub iteration_count: IterationCount,
    pub direction: Direction,
    pub fill_mode: FillMode,
    pub play_state: PlayState,
}

/// The resolved `animation-*` lists of one element. Empty when it sets none.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct AnimationList(pub Vec<Animation>);

impl AnimationList {
    pub(super) fn parse(style: &StyleDesc) -> Result<Self, String> {
        if animation_fields(style).into_iter().all(Option::is_none) {
            return Ok(Self::default());
        }
        let mut lists = match style.animation.as_deref() {
            Some(text) => {
                Lists::parse_shorthand(text).map_err(|error| format!("animation: {error}"))?
            }
            None => Lists::initial(),
        };
        if let Some(text) = style.animation_name.as_deref() {
            lists.names = parse_list(text, parse_name_or_none)
                .map_err(|error| format!("animationName: {error}"))?;
        }
        if let Some(text) = style.animation_duration.as_deref() {
            lists.durations = parse_list(text, parse_animation_duration)
                .map_err(|error| format!("animationDuration: {error}"))?;
        }
        if let Some(text) = style.animation_timing_function.as_deref() {
            lists.timings = parse_list(text, parse_timing_function)
                .map_err(|error| format!("animationTimingFunction: {error}"))?;
        }
        if let Some(text) = style.animation_delay.as_deref() {
            lists.delays = parse_list(text, parse_delay)
                .map_err(|error| format!("animationDelay: {error}"))?;
        }
        if let Some(text) = style.animation_iteration_count.as_deref() {
            lists.counts = parse_list(text, parse_iteration_count)
                .map_err(|error| format!("animationIterationCount: {error}"))?;
        }
        if let Some(text) = style.animation_direction.as_deref() {
            lists.directions = parse_list(text, |item| {
                parse_direction(item).ok_or_else(|| {
                    format!(
                        "`{item}` is not a direction (normal, reverse, alternate, alternate-reverse)"
                    )
                })
            })
            .map_err(|error| format!("animationDirection: {error}"))?;
        }
        if let Some(text) = style.animation_fill_mode.as_deref() {
            lists.fills = parse_list(text, |item| {
                parse_fill_mode(item).ok_or_else(|| {
                    format!("`{item}` is not a fill mode (none, forwards, backwards, both)")
                })
            })
            .map_err(|error| format!("animationFillMode: {error}"))?;
        }
        if let Some(text) = style.animation_play_state.as_deref() {
            lists.states = parse_list(text, |item| {
                parse_play_state(item)
                    .ok_or_else(|| format!("`{item}` is not a play state (running, paused)"))
            })
            .map_err(|error| format!("animationPlayState: {error}"))?;
        }
        Ok(lists.into_list())
    }
}

fn animation_fields(style: &StyleDesc) -> [&Option<String>; 9] {
    [
        &style.animation,
        &style.animation_name,
        &style.animation_duration,
        &style.animation_timing_function,
        &style.animation_delay,
        &style.animation_iteration_count,
        &style.animation_direction,
        &style.animation_fill_mode,
        &style.animation_play_state,
    ]
}

/// The eight `animation-*` lists. `names` sets the count and the others repeat
/// or truncate to it, as in CSS.
struct Lists {
    names: Vec<Option<String>>,
    durations: Vec<f64>,
    timings: Vec<Ease>,
    delays: Vec<f64>,
    counts: Vec<IterationCount>,
    directions: Vec<Direction>,
    fills: Vec<FillMode>,
    states: Vec<PlayState>,
}

fn default_timing() -> Ease {
    Ease::CubicBezier([0.25, 0.1, 0.25, 1.0])
}

impl Lists {
    fn initial() -> Self {
        Self {
            names: vec![None],
            durations: vec![0.0],
            timings: vec![default_timing()],
            delays: vec![0.0],
            counts: vec![IterationCount::Finite(1.0)],
            directions: vec![Direction::Normal],
            fills: vec![FillMode::None],
            states: vec![PlayState::Running],
        }
    }

    fn parse_shorthand(text: &str) -> Result<Self, String> {
        match text.trim().to_ascii_lowercase().as_str() {
            "initial" | "unset" => return Ok(Self::initial()),
            "inherit" | "revert" | "revert-layer" => {
                return Err(format!("{} is not supported", text.trim()));
            }
            _ => {}
        }
        let mut lists = Self {
            names: Vec::new(),
            durations: Vec::new(),
            timings: Vec::new(),
            delays: Vec::new(),
            counts: Vec::new(),
            directions: Vec::new(),
            fills: Vec::new(),
            states: Vec::new(),
        };
        for item in split_outside_parens(text, ',')? {
            let single = Single::parse(item)?;
            lists.names.push(single.name);
            lists.durations.push(single.duration.unwrap_or(0.0));
            lists.timings.push(single.timing.unwrap_or_else(default_timing));
            lists.delays.push(single.delay.unwrap_or(0.0));
            lists
                .counts
                .push(single.count.unwrap_or(IterationCount::Finite(1.0)));
            lists.directions.push(single.direction.unwrap_or(Direction::Normal));
            lists.fills.push(single.fill.unwrap_or(FillMode::None));
            lists.states.push(single.state.unwrap_or(PlayState::Running));
        }
        Ok(lists)
    }

    fn into_list(self) -> AnimationList {
        AnimationList(
            (0..self.names.len())
                .map(|index| Animation {
                    name: self.names[index].clone(),
                    duration: self.durations[index % self.durations.len()],
                    delay: self.delays[index % self.delays.len()],
                    timing: self.timings[index % self.timings.len()].clone(),
                    iteration_count: self.counts[index % self.counts.len()],
                    direction: self.directions[index % self.directions.len()],
                    fill_mode: self.fills[index % self.fills.len()],
                    play_state: self.states[index % self.states.len()],
                })
                .collect(),
        )
    }
}

/// One comma-separated item of the shorthand, each part set at most once.
#[derive(Default)]
struct Single {
    name: Option<String>,
    duration: Option<f64>,
    delay: Option<f64>,
    timing: Option<Ease>,
    count: Option<IterationCount>,
    direction: Option<Direction>,
    fill: Option<FillMode>,
    state: Option<PlayState>,
}

impl Single {
    /// Parts may come in any order, except that the first `<time>` is the
    /// duration and the second the delay. A keyword that fits a longhand not
    /// yet set goes to that longhand and only otherwise names the animation,
    /// so `animation: none` fills with `none` and `2s paused spin` names `spin`.
    fn parse(item: &str) -> Result<Self, String> {
        let mut single = Self::default();
        let mut named = false;
        for token in split_whitespace_outside_parens(item) {
            if let Some(time) = parse_time(token) {
                if single.duration.is_none() {
                    if time < 0.0 {
                        return Err(format!("a duration cannot be negative: {token}"));
                    }
                    single.duration = Some(check_seconds(time, token)?);
                } else if single.delay.is_none() {
                    single.delay = Some(check_seconds(time, token)?);
                } else {
                    return Err(format!("too many times in `{item}`"));
                }
            } else if is_timing_function(token) {
                if single.timing.replace(parse_timing_function(token)?).is_some() {
                    return Err(format!("two timing functions in `{item}`"));
                }
            } else if single.count.is_none() && looks_like_count(token) {
                single.count = Some(parse_iteration_count(token)?);
            } else if let Some(direction) = parse_direction(token).filter(|_| single.direction.is_none()) {
                single.direction = Some(direction);
            } else if let Some(fill) = parse_fill_mode(token).filter(|_| single.fill.is_none()) {
                single.fill = Some(fill);
            } else if let Some(state) = parse_play_state(token).filter(|_| single.state.is_none()) {
                single.state = Some(state);
            } else if named {
                return Err(format!("two names in `{item}`"));
            } else {
                single.name = parse_name_or_none(token)?;
                named = true;
            }
        }
        Ok(single)
    }
}

fn looks_like_count(token: &str) -> bool {
    token.eq_ignore_ascii_case("infinite")
        || token
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_digit() || matches!(first, '.' | '+' | '-'))
            && token.parse::<f64>().is_ok()
}

fn parse_iteration_count(text: &str) -> Result<IterationCount, String> {
    if text.eq_ignore_ascii_case("infinite") {
        return Ok(IterationCount::Infinite);
    }
    let count = text
        .parse::<f64>()
        .ok()
        .filter(|count| count.is_finite())
        .ok_or_else(|| format!("not an iteration count: {text}"))?;
    if count < 0.0 {
        return Err(format!("an iteration count cannot be negative: {text}"));
    }
    Ok(IterationCount::Finite(count))
}

fn parse_direction(text: &str) -> Option<Direction> {
    Some(match text.to_ascii_lowercase().as_str() {
        "normal" => Direction::Normal,
        "reverse" => Direction::Reverse,
        "alternate" => Direction::Alternate,
        "alternate-reverse" => Direction::AlternateReverse,
        _ => return None,
    })
}

fn parse_fill_mode(text: &str) -> Option<FillMode> {
    Some(match text.to_ascii_lowercase().as_str() {
        "none" => FillMode::None,
        "forwards" => FillMode::Forwards,
        "backwards" => FillMode::Backwards,
        "both" => FillMode::Both,
        _ => return None,
    })
}

fn parse_play_state(text: &str) -> Option<PlayState> {
    Some(match text.to_ascii_lowercase().as_str() {
        "running" => PlayState::Running,
        "paused" => PlayState::Paused,
        _ => return None,
    })
}

/// `auto` is `0s` in CSS Animations 1.
fn parse_animation_duration(text: &str) -> Result<f64, String> {
    if text.eq_ignore_ascii_case("auto") {
        return Ok(0.0);
    }
    parse_duration(text)
}

fn parse_name_or_none(text: &str) -> Result<Option<String>, String> {
    if text.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    check_keyframes_name(text)?;
    Ok(Some(text.to_string()))
}

/// A CSS identifier, case-sensitive, that is not a CSS-wide keyword. Also what
/// `defineKeyframes` accepts, so every defined name can be written in a style.
pub(super) fn check_keyframes_name(name: &str) -> Result<(), String> {
    let mut characters = name.chars();
    let starts_well = match characters.next() {
        Some('-') => characters
            .next()
            .is_some_and(|second| second.is_alphabetic() || matches!(second, '_' | '-')),
        Some(first) => first.is_alphabetic() || first == '_',
        None => false,
    };
    let valid = starts_well
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_'));
    if !valid {
        return Err(format!("`{name}` is not a valid animation name (a CSS identifier)"));
    }
    if matches!(
        name.to_ascii_lowercase().as_str(),
        "none" | "initial" | "inherit" | "unset" | "revert" | "revert-layer" | "default"
    ) {
        return Err(format!("`{name}` cannot be an animation name"));
    }
    Ok(())
}

/// Rejects a style whose `animation*` values, in it or in a state style, do
/// not parse.
pub(crate) fn validate_animations(style: &StyleDesc) -> Result<(), String> {
    AnimationList::parse(style)?;
    for (name, state) in super::state_styles(style) {
        if let Some(state) = state {
            AnimationList::parse(state).map_err(|error| format!("{name}.{error}"))?;
        }
    }
    Ok(())
}

// ── Runtime ──────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AnimationEventKind {
    Start,
    Iteration,
    End,
    Cancel,
}

impl AnimationEventKind {
    /// The event type the host maps to `onAnimation*`.
    pub(crate) fn event_type(self) -> &'static str {
        match self {
            Self::Start => "animationstart",
            Self::Iteration => "animationiteration",
            Self::End => "animationend",
            Self::Cancel => "animationcancel",
        }
    }
}

/// A DOM `AnimationEvent`, minus `pseudoElement`, which is always `""`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AnimationEvent {
    pub(crate) id: u64,
    pub(crate) kind: AnimationEventKind,
    pub(crate) name: String,
    /// Seconds.
    pub(crate) elapsed: f64,
}

/// An event with the instant it happened at, which can be before the frame
/// that noticed it.
struct Scheduled {
    event: AnimationEvent,
    at: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Before,
    Active,
    After,
}

/// Where an animation is at one local time (Web Animations, "calculating the
/// directed progress").
struct Evaluation {
    phase: Phase,
    /// Zero-based, meaningful in the active phase.
    iteration: u64,
    /// How far through the keyframes, 0 to 1, after the direction. `None` when
    /// the animation has no effect at this time: before the delay without
    /// `backwards`, or after the end without `forwards`.
    progress: Option<f64>,
}

fn active_duration(animation: &Animation) -> f64 {
    match animation.iteration_count {
        _ if animation.duration == 0.0 => 0.0,
        IterationCount::Infinite => f64::INFINITY,
        IterationCount::Finite(count) => animation.duration * count,
    }
}

/// `elapsedTime` of an `animationstart`: the part of the active interval a
/// negative delay already skipped.
fn initial_elapsed(animation: &Animation) -> f64 {
    (-animation.delay).min(active_duration(animation)).max(0.0)
}

/// `backwards` is a negative playback rate, which gives the boundaries to the
/// phase the time came from.
fn evaluate(animation: &Animation, local: f64, backwards_rate: bool) -> Evaluation {
    let duration = animation.duration;
    let active_duration = active_duration(animation);
    let end_time = (animation.delay + active_duration).max(0.0);
    let before_boundary = animation.delay.min(end_time).max(0.0);
    let phase = if local < before_boundary || (backwards_rate && local == before_boundary) {
        Phase::Before
    } else if local > end_time || (!backwards_rate && local == end_time) {
        Phase::After
    } else {
        Phase::Active
    };

    let backwards_fill = matches!(animation.fill_mode, FillMode::Backwards | FillMode::Both);
    let forwards_fill = matches!(animation.fill_mode, FillMode::Forwards | FillMode::Both);
    let active_time = match phase {
        Phase::Before => backwards_fill.then_some(0.0),
        Phase::Active => Some(local - animation.delay),
        Phase::After => forwards_fill.then(|| (local - animation.delay).clamp(0.0, active_duration)),
    };
    let Some(active_time) = active_time else {
        return Evaluation { phase, iteration: 0, progress: None };
    };

    let count = match animation.iteration_count {
        IterationCount::Infinite => f64::INFINITY,
        IterationCount::Finite(count) => count,
    };
    let overall = if duration == 0.0 {
        if phase == Phase::Before {
            0.0
        } else if count.is_infinite() {
            1.0
        } else {
            count
        }
    } else if active_time >= active_duration {
        // Not `active_time / duration`: that can land a hair past the count.
        count
    } else {
        active_time / duration
    };
    let (mut progress, mut iteration) = (overall % 1.0, overall.floor());
    if progress == 0.0 && phase != Phase::Before && active_time >= active_duration && count != 0.0 {
        // The end of an iteration is its last frame, not the next one's first.
        progress = 1.0;
        iteration = overall - 1.0;
    }
    let iteration = iteration.max(0.0) as u64;
    let backwards = match animation.direction {
        Direction::Normal => false,
        Direction::Reverse => true,
        Direction::Alternate => iteration % 2 == 1,
        Direction::AlternateReverse => iteration % 2 == 0,
    };
    Evaluation {
        phase,
        iteration,
        progress: Some(if backwards { 1.0 - progress } else { progress }),
    }
}

// ── Keyframes against an element ─────────────────────────────────────

/// What a property is when a keyframe set leaves out an endpoint.
#[derive(Clone, Debug, PartialEq)]
enum Underlying {
    Value(AnimValue),
    /// A size the style leaves unset, which CSS computes to `auto` (`none` for
    /// a max).
    Keyword,
    /// A style that only the parent can give (an inherited one): the endpoint is
    /// left out and the nearest keyframe holds.
    Unknown,
}

/// What a name in a keyframe set means on an element, which is also what it
/// animates: a layout element has styles, a shape inside an `<svg>` has the
/// SVG properties, and the `<svg>` itself has both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PropTarget {
    Element,
    /// A name that is both a style and an SVG property (`opacity`, `width`,
    /// `height`, `fontSize`) is the SVG one. A style with no SVG meaning is
    /// ignored, as Chrome computes it but never paints it.
    SvgShape,
    /// The style properties, and the SVG properties that no style property
    /// shares a name with. Those are written on the root `<svg>`, where the
    /// shapes inherit them.
    SvgHost,
}

impl PropTarget {
    fn takes(self, key: PropKey) -> bool {
        match (self, key) {
            (Self::Element | Self::SvgHost, PropKey::Style(_)) | (Self::SvgShape, PropKey::Svg(_)) => true,
            (Self::SvgHost, PropKey::Svg(attr)) => AnimProp::from_name(attr.name()).is_none(),
            _ => false,
        }
    }
}

/// The attributes of an SVG element a keyframe set's missing endpoints read.
pub(crate) struct SvgSource<'a> {
    pub own: &'a HashMap<String, Value>,
    /// The elements above it in the `<svg>`, the nearest last, which an inherited
    /// property it sets nothing for comes from.
    pub ancestors: &'a [&'a HashMap<String, Value>],
}

/// What an element's animations lay their values over.
pub(crate) struct Subject<'a> {
    /// The element's style beneath the animations: transitions and motion
    /// included, so a missing endpoint is what the element shows without them.
    style: &'a StyleDesc,
    target: PropTarget,
    svg: Option<SvgSource<'a>>,
}

impl<'a> Subject<'a> {
    pub(crate) fn element(style: &'a StyleDesc) -> Self {
        Self { style, target: PropTarget::Element, svg: None }
    }

    pub(crate) fn svg_host(style: &'a StyleDesc, own: &'a HashMap<String, Value>) -> Self {
        Self { style, target: PropTarget::SvgHost, svg: Some(SvgSource { own, ancestors: &[] }) }
    }

    pub(crate) fn svg_shape(style: &'a StyleDesc, source: SvgSource<'a>) -> Self {
        Self { style, target: PropTarget::SvgShape, svg: Some(source) }
    }

    fn underlying(&self, key: PropKey, colors: &Colors) -> Underlying {
        match key {
            PropKey::Style(prop) => {
                match prop.read(self.style, colors).or_else(|| prop.initial(Some(self.style), colors)) {
                    Some(value) => Underlying::Value(value),
                    None if prop.size_axis().is_some() => Underlying::Keyword,
                    None => Underlying::Unknown,
                }
            }
            PropKey::Svg(attr) => {
                let value = self.svg.as_ref().and_then(|source| {
                    let own = attr.read_css(source.own, &colors.element);
                    let inherited = || {
                        attr.inherits()
                            .then(|| {
                                source.ancestors.iter().rev().find_map(|above| attr.read_css(above, &colors.element))
                            })
                            .flatten()
                    };
                    own.or_else(inherited).or_else(|| attr.initial_css(colors.font.size))
                });
                value.map_or(Underlying::Unknown, Underlying::Value)
            }
        }
    }
}

/// One keyframe of one property.
#[derive(Clone, Debug)]
struct Stop {
    offset: f64,
    /// `None` is a size left unset, which is a keyword in CSS.
    value: Option<AnimValue>,
    /// Eases from this stop to the next.
    timing: Ease,
}

enum PropPlan {
    Continuous {
        key: PropKey,
        track: Track,
        /// The keyframes as the style wrote them. `track` runs through px where
        /// they are `auto` or a percentage, and the ends go back to the literal.
        literal: Vec<AnimValue>,
        measured: bool,
    },
    /// A length against a keyword, which swaps halfway through an interval
    /// unless `interpolate-size` allows keywords.
    Discrete { prop: AnimProp, stops: Vec<Stop> },
}

impl PropPlan {
    fn sample(&self, progress: f64) -> (PropKey, Option<AnimValue>) {
        match self {
            Self::Continuous { key, track, literal, measured } => {
                let value = match (measured, progress) {
                    (true, progress) if progress <= 0.0 => literal[0].clone(),
                    (true, progress) if progress >= 1.0 => literal[literal.len() - 1].clone(),
                    _ => key.clamp_value(track.value_at(progress)),
                };
                (*key, Some(value))
            }
            Self::Discrete { prop, stops } => {
                let last = &stops[stops.len() - 1];
                let value = if progress <= stops[0].offset {
                    stops[0].value.clone()
                } else if progress >= last.offset {
                    last.value.clone()
                } else {
                    let index = stops.partition_point(|stop| stop.offset <= progress) - 1;
                    let (from, to) = (&stops[index], &stops[index + 1]);
                    let local = (progress - from.offset) / (to.offset - from.offset);
                    if ease(local.clamp(0.0, 1.0), &from.timing) < 0.5 {
                        from.value.clone()
                    } else {
                        to.value.clone()
                    }
                };
                (PropKey::Style(*prop), value)
            }
        }
    }
}

/// Everything the plan was built from, so it is rebuilt only when one changes:
/// the style beneath the animation (for the endpoints a keyframe set leaves
/// out), the animation's own timing function, and what layout measured.
#[derive(Clone, Debug, PartialEq)]
struct PlanKey {
    target: PropTarget,
    timing: Ease,
    /// What the keyframes' colours resolved in.
    colors: Colors,
    allow_keywords: bool,
    underlying: Vec<Underlying>,
    measurement: Option<Measurement>,
}

struct Plan {
    key: PlanKey,
    props: Vec<PropPlan>,
    /// Some property reads layout, so a new measurement can change the plan.
    reads_layout: bool,
    /// Some property is waiting for a measurement and shows its underlying
    /// value until one exists.
    waiting: bool,
}

enum Endpoint {
    Value(Option<AnimValue>),
    Waiting,
    Absent,
}

struct Planner<'a> {
    keyframes: &'a Keyframes,
    colors: &'a Colors,
    timing: &'a Ease,
    measurement: Option<&'a Measurement>,
    allow_keywords: bool,
    /// Laid-out sizes taken for an unset size, once: the animation is what
    /// moves the box, so measuring it again would chase its own output.
    frozen: &'a mut BTreeMap<AnimProp, f64>,
    now: Instant,
    reads_layout: bool,
    waiting: bool,
}

impl Planner<'_> {
    fn endpoint(&mut self, key: PropKey, underlying: &Underlying) -> Endpoint {
        match underlying {
            Underlying::Value(value) => Endpoint::Value(Some(value.clone())),
            Underlying::Unknown => Endpoint::Absent,
            Underlying::Keyword => {
                let PropKey::Style(prop) = key else {
                    unreachable!("only a style size is a keyword");
                };
                if matches!(prop, AnimProp::Width | AnimProp::Height) {
                    return Endpoint::Value(Some(AnimValue::Auto));
                }
                if !self.allow_keywords {
                    return Endpoint::Value(None);
                }
                if let Some(size) = self.frozen.get(&prop) {
                    return Endpoint::Value(Some(AnimValue::Number(*size)));
                }
                self.reads_layout = true;
                match self.measurement.and_then(|measurement| measurement.laid_out(prop)) {
                    Some(size) => {
                        self.frozen.insert(prop, size);
                        Endpoint::Value(Some(AnimValue::Number(size)))
                    }
                    None => Endpoint::Waiting,
                }
            }
        }
    }

    /// The stops of `prop`: its keyframes, with the underlying value standing in
    /// for a missing 0% or 100%. `None` while an endpoint waits for layout.
    fn stops(&mut self, key: PropKey, underlying: &Underlying) -> Option<Vec<Stop>> {
        let mut stops: Vec<Stop> = self
            .keyframes
            .frames
            .iter()
            .filter_map(|frame| {
                let value = frame.value(key, self.colors)?;
                Some(Stop {
                    offset: frame.offset,
                    value: Some(value),
                    timing: frame.timing.clone().unwrap_or_else(|| self.timing.clone()),
                })
            })
            .collect();
        let missing_start = stops.first().is_some_and(|stop| stop.offset > 0.0);
        let missing_end = stops.last().is_some_and(|stop| stop.offset < 1.0);
        if missing_start || missing_end {
            let value = match self.endpoint(key, underlying) {
                Endpoint::Value(value) => Some(value),
                Endpoint::Waiting => return None,
                Endpoint::Absent => None,
            };
            if let Some(value) = value {
                let stop = |offset| Stop { offset, value: value.clone(), timing: self.timing.clone() };
                if missing_start {
                    stops.insert(0, stop(0.0));
                }
                if missing_end {
                    stops.push(stop(1.0));
                }
            }
        }
        if let [only] = stops.as_slice() {
            let mut other = only.clone();
            other.offset = if only.offset > 0.0 { 0.0 } else { 1.0 };
            if other.offset == 0.0 {
                stops.insert(0, other);
            } else {
                stops.push(other);
            }
        }
        Some(stops)
    }

    fn plan_prop(&mut self, key: PropKey, underlying: &Underlying) -> Option<PropPlan> {
        let stops = self.stops(key, underlying).or_else(|| {
            self.waiting = true;
            None
        })?;
        let keyword = |stop: &Stop| matches!(stop.value, None | Some(AnimValue::Auto));
        if let PropKey::Style(prop) = key {
            if prop.size_axis().is_some() && !self.allow_keywords && stops.iter().any(keyword) {
                return Some(PropPlan::Discrete { prop, stops });
            }
        }

        let literal: Vec<AnimValue> = stops
            .iter()
            .map(|stop| stop.value.clone().expect("an unset size is discrete unless keywords are allowed"))
            .collect();
        let resolved = if needs_layout(&literal) {
            let PropKey::Style(prop) = key else {
                unreachable!("only a style property is auto");
            };
            self.reads_layout = true;
            let has_auto = literal.contains(&AnimValue::Auto);
            let Some(measurement) = self
                .measurement
                .filter(|measurement| measurement.resolves(prop, has_auto))
            else {
                self.waiting = true;
                return None;
            };
            match resolve_keyframes(prop, &literal, measurement) {
                Ok(resolved) => resolved,
                Err(error) => {
                    log::warn!("CSS animation cannot resolve {}: {error}", prop.name());
                    return None;
                }
            }
        } else {
            literal.clone()
        };

        let transition = Transition {
            delay: 0.0,
            curve: Curve::Tween { duration: 1.0, ease: Ease::Name("linear".to_string()) },
            times: Some(stops.iter().map(|stop| stop.offset).collect()),
            segment_eases: Some(stops[..stops.len() - 1].iter().map(|stop| stop.timing.clone()).collect()),
            repeat: Repeat::default(),
        };
        let name = key.name();
        let built = Goal::keyframes(name, literal.iter().cloned().map(Some).collect()).and_then(|goal| {
            Track::keyframed(name, goal, literal.clone(), &resolved, Interpolation::Css, transition, self.now)
        });
        match built {
            Ok(track) => Some(PropPlan::Continuous { key, track, measured: literal != resolved, literal }),
            Err(error) => {
                log::warn!("CSS animation cannot interpolate {name}: {error}");
                None
            }
        }
    }
}

// ── One running animation ────────────────────────────────────────────

struct Run {
    name: String,
    /// The registry's definition of `name` as of the last frame.
    keyframes: Arc<Keyframes>,
    properties: Vec<PropKey>,
    params: Animation,
    /// The animation's local time (its `currentTime`), the API controls and its id.
    clock: Playback,
    phase: Phase,
    iteration: u64,
    /// The keyframe progress `CssAnimations::advance` found this frame, `None`
    /// while the animation has no effect.
    progress: Option<f64>,
    plan: Option<Plan>,
    frozen: BTreeMap<AnimProp, f64>,
    /// Until this instant a tick of the clock reports nothing and changes nothing a read can see
    /// (`CssAnimations::advance_clock` leaves it out). Anything that changes the clock or the
    /// parameters, or ticks it, forgets it.
    quiet_until: Option<Instant>,
}

impl Run {
    fn start(name: &str, keyframes: Arc<Keyframes>, params: &Animation, now: Instant) -> Self {
        Self {
            name: name.to_string(),
            properties: keyframes.properties(),
            keyframes,
            params: params.clone(),
            clock: Playback::start(now, params.play_state == PlayState::Running),
            phase: Phase::Before,
            iteration: 0,
            progress: None,
            plan: None,
            frozen: BTreeMap::new(),
            quiet_until: None,
        }
    }

    /// The instant the first change of phase, iteration or finish can have come, which ticks
    /// before it can leave out; a clock that is held has none before an hour has gone, and what
    /// moves it (a style, a command) forgets this. None when it cannot say.
    fn quiet_after(&self, now: Instant) -> Option<Instant> {
        const HELD: Duration = Duration::from_secs(3600);
        /// A tick a little before the boundary is only a tick too many.
        const MARGIN: f64 = 0.001;
        if self.clock.pending() {
            return None;
        }
        let rate = self.clock.rate();
        if !self.clock.ticking() || rate == 0.0 {
            return now.checked_add(HELD);
        }
        if rate < 0.0 {
            return None;
        }
        let params = &self.params;
        let active = active_duration(params);
        let local = self.local(now);
        let mut next = f64::INFINITY;
        let mut consider = |boundary: f64| {
            if boundary.is_finite() && boundary > local {
                next = next.min(boundary);
            }
        };
        consider(params.delay.max(0.0));
        consider((params.delay + active).max(0.0));
        consider(self.end_time());
        if params.duration > 0.0 && params.duration.is_finite() {
            consider(params.delay + params.duration * (((local - params.delay) / params.duration).floor() + 1.0));
        }
        if next.is_infinite() {
            return now.checked_add(HELD);
        }
        let wait = Duration::try_from_secs_f64(((next - local) / rate - MARGIN).max(0.0)).ok()?;
        now.checked_add(wait.min(HELD))
    }

    /// `@keyframes` is looked up at every style recalculation, so a redefinition
    /// changes what a running animation shows while its timeline carries on.
    fn redefine(&mut self, keyframes: &Arc<Keyframes>) {
        if Arc::ptr_eq(&self.keyframes, keyframes) {
            return;
        }
        self.keyframes = keyframes.clone();
        self.properties = keyframes.properties();
        self.plan = None;
    }

    /// Where the effect ends, in local time. Infinite for an animation that
    /// repeats forever.
    fn end_time(&self) -> f64 {
        (self.params.delay + active_duration(&self.params)).max(0.0)
    }

    fn local(&self, now: Instant) -> f64 {
        self.clock.current(now, self.end_time())
    }

    /// When `local` is or was, for ordering events of different animations.
    fn instant_at(&self, local: f64, now: Instant) -> Instant {
        self.clock.instant_at(local, now)
    }

    /// Takes the other `animation-*` values from the style; play state changes
    /// freeze or resume the clock where it is.
    fn retune(&mut self, params: &Animation, now: Instant) {
        self.quiet_until = None;
        let end = self.end_time();
        self.clock.follow_style(params.play_state == PlayState::Running, now, end);
        self.params = params.clone();
    }

    fn event(&self, id: u64, kind: AnimationEventKind, elapsed: f64, at: Instant) -> Scheduled {
        Scheduled {
            event: AnimationEvent { id, kind, name: self.name.clone(), elapsed },
            at,
        }
    }

    /// Moves to `now`, queueing the events the phase and iteration changes
    /// make (CSS Animations 2, "event dispatch").
    fn advance(&mut self, id: u64, now: Instant, events: &mut Vec<Scheduled>) -> Evaluation {
        use AnimationEventKind::{End, Iteration, Start};
        let local = self.local(now);
        let evaluation = evaluate(&self.params, local, self.clock.rate() < 0.0);
        let params = &self.params;
        let active_duration = active_duration(params);
        let starts = params.delay.max(0.0);
        let ends = (params.delay + active_duration).max(0.0);
        let initial = initial_elapsed(params);

        let from = self.phase;
        let to = evaluation.phase;
        let start = |at_local: f64, elapsed: f64| self.event(id, Start, elapsed, self.instant_at(at_local, now));
        let end = |at_local: f64, elapsed: f64| self.event(id, End, elapsed, self.instant_at(at_local, now));
        match (from, to) {
            (Phase::Before, Phase::Active) => events.push(start(starts, initial)),
            (Phase::Before, Phase::After) => {
                events.push(start(starts, initial));
                events.push(end(ends, active_duration));
            }
            (Phase::After, Phase::Active) => events.push(start(local, active_duration)),
            (Phase::After, Phase::Before) => {
                events.push(start(local, active_duration));
                events.push(end(local, initial));
            }
            (Phase::Active, Phase::After) => events.push(end(ends, active_duration)),
            (Phase::Active, Phase::Before) => events.push(end(local, initial)),
            (Phase::Active, Phase::Active) if evaluation.iteration != self.iteration => {
                let boundary = params.duration * evaluation.iteration as f64;
                events.push(self.event(
                    id,
                    Iteration,
                    boundary,
                    self.instant_at(params.delay + boundary, now),
                ));
            }
            _ => {}
        }
        self.phase = to;
        if to == Phase::Active {
            self.iteration = evaluation.iteration;
        }
        evaluation
    }

    /// The clock half of a frame: moves to `now` and reports a finish. It needs no style, which
    /// is what lets an element nothing is built for keep its animations on time.
    fn tick(&mut self, id: u64, now: Instant, events: &mut Vec<Scheduled>, api: &mut Vec<ApiEvent>) {
        self.quiet_until = None;
        self.clock.resolve_pending(now);
        self.progress = self.advance(id, now, events).progress;
        if self.clock.take_finish(now, self.end_time()) {
            api.push(ApiEvent {
                element: id,
                run: self.clock.id(),
                kind: ApiEventKind::Finish,
                info: self.info(now),
            });
        }
    }

    /// Reports an animation that stops before it ended. It is brought up to
    /// `now` first: one that ran out since the last frame ended, and its
    /// `animationend` is what is owed, not a cancel. The API hears of the
    /// cancel either way, as a finished animation is cancelled too.
    fn cancel(&mut self, id: u64, now: Instant, events: &mut Vec<Scheduled>, api: &mut Vec<ApiEvent>) {
        self.quiet_until = None;
        self.advance(id, now, events);
        api.push(ApiEvent {
            element: id,
            run: self.clock.id(),
            kind: ApiEventKind::Cancel,
            info: self.info(now).cancelled(),
        });
        if self.phase == Phase::After {
            return;
        }
        let elapsed = (self.local(now) - self.params.delay).clamp(0.0, active_duration(&self.params));
        events.push(self.event(id, AnimationEventKind::Cancel, elapsed, now));
    }

    fn values(
        &mut self,
        progress: f64,
        subject: &Subject,
        parent_colors: &ColorContext,
        parent_font: &ParentFont,
        measurement: Option<&Measurement>,
        allow_keywords: bool,
        now: Instant,
    ) -> Vec<(PropKey, Option<AnimValue>)> {
        let reads_layout = self.plan.as_ref().is_none_or(|plan| plan.reads_layout);
        let colors = Colors::inheriting(parent_colors, parent_font, Some(subject.style));
        let properties: Vec<PropKey> = self
            .properties
            .iter()
            .copied()
            .filter(|key| subject.target.takes(*key))
            .collect();
        let key = PlanKey {
            target: subject.target,
            timing: self.params.timing.clone(),
            colors: colors.clone(),
            allow_keywords,
            underlying: properties.iter().map(|key| subject.underlying(*key, &colors)).collect(),
            measurement: measurement.copied().filter(|_| reads_layout),
        };
        if self.plan.as_ref().is_none_or(|plan| plan.key != key) {
            let mut planner = Planner {
                keyframes: &self.keyframes,
                colors: &colors,
                timing: &self.params.timing,
                measurement,
                allow_keywords,
                frozen: &mut self.frozen,
                now,
                reads_layout: false,
                waiting: false,
            };
            let props = properties
                .iter()
                .zip(&key.underlying)
                .filter_map(|(key, underlying)| planner.plan_prop(*key, underlying))
                .collect();
            let (reads_layout, waiting) = (planner.reads_layout, planner.waiting);
            self.plan = Some(Plan { key, props, reads_layout, waiting });
        }
        let plan = self.plan.as_ref().expect("the plan was just built");
        plan.props.iter().map(|prop| prop.sample(progress)).collect()
    }

    /// Whether the window needs another frame for this animation: it is moving
    /// toward its end, a `play()` waits for a frame, or it waits on layout.
    fn wants_frame(&self, now: Instant) -> bool {
        self.clock.wants_frame(now, self.end_time())
            || self.plan.as_ref().is_some_and(|plan| plan.waiting)
    }

    /// Whether the animation is relevant in Web Animations terms: current
    /// (active, before its active phase going forward, or after it going back)
    /// or in effect through a fill. Chrome decides by the phase and the rate
    /// alone: a paused animation at the end of its effect is not current, and
    /// neither is one run back to the start.
    fn is_relevant(&self) -> bool {
        self.progress.is_some() || self.is_current()
    }

    /// Whether the animation is current: active, before its active phase going forward, or after it
    /// going back. One that only fills is in effect and not current.
    fn is_current(&self) -> bool {
        match self.phase {
            Phase::Before => self.clock.rate() > 0.0,
            Phase::Active => true,
            Phase::After => self.clock.rate() < 0.0,
        }
    }

    fn info(&self, now: Instant) -> Info {
        Info {
            id: self.clock.id(),
            kind: Kind::Animation,
            name: self.name.clone(),
            current_time: self.local(now) * 1000.0,
            end_time: self.end_time() * 1000.0,
            playback_rate: self.clock.rate(),
            status: self.clock.status(now, self.end_time()),
        }
    }
}

// ── Per-element state ────────────────────────────────────────────────

type Config = [Option<String>; 9];

struct ElementState {
    config: Config,
    list: AnimationList,
    runs: Vec<Run>,
    /// What the element's last update animated.
    target: PropTarget,
    /// Some animation was moving at the last update.
    moving: bool,
}

impl ElementState {
    fn new() -> Self {
        Self {
            config: Default::default(),
            list: AnimationList::default(),
            runs: Vec::new(),
            target: PropTarget::Element,
            moving: false,
        }
    }

    /// Whether what the animations show can change without the style or the
    /// clock moving: a run is still going, or its keyframes were defined or
    /// redefined since it last read them.
    fn shows_something_new(&self, registry: &KeyframesRegistry) -> bool {
        self.moving
            || self.runs.iter().any(|run| {
                registry.get(&run.name).is_some_and(|keyframes| !Arc::ptr_eq(keyframes, &run.keyframes))
            })
            || self.list.0.iter().filter_map(|animation| animation.name.as_ref()).any(|name| {
                registry.get(name).is_some() && !self.runs.iter().any(|run| run.name == *name)
            })
    }

    fn sync_config(&mut self, id: u64, style: &StyleDesc) {
        let fields = animation_fields(style);
        if fields.iter().zip(&self.config).all(|(field, kept)| *field == kept) {
            return;
        }
        self.config = fields.map(Clone::clone);
        self.list = AnimationList::parse(style).unwrap_or_else(|error| {
            log::warn!("Invalid animation on element {id}: {error}");
            AnimationList::default()
        });
    }

    /// Matches the list against the running animations by name. One that stays
    /// in the list keeps running with the new values; one that left is
    /// cancelled; a name that is new starts if its keyframes are defined. A kept
    /// animation reads the current definition of its name.
    fn reconcile(
        &mut self,
        id: u64,
        registry: &KeyframesRegistry,
        now: Instant,
        events: &mut Vec<Scheduled>,
        api: &mut Vec<ApiEvent>,
    ) {
        let mut unclaimed: Vec<Option<Run>> =
            std::mem::take(&mut self.runs).into_iter().map(Some).collect();
        let mut runs = Vec::new();
        for animation in &self.list.0 {
            let Some(name) = &animation.name else {
                continue;
            };
            let kept = unclaimed
                .iter_mut()
                .find(|slot| slot.as_ref().is_some_and(|run| run.name == *name))
                .and_then(Option::take);
            match kept {
                Some(mut run) => {
                    run.retune(animation, now);
                    if let Some(keyframes) = registry.get(name) {
                        run.redefine(keyframes);
                    }
                    runs.push(run);
                }
                None => {
                    if let Some(keyframes) = registry.get(name) {
                        runs.push(Run::start(name, keyframes.clone(), animation, now));
                    }
                }
            }
        }
        for mut run in unclaimed.into_iter().flatten() {
            run.cancel(id, now, events, api);
        }
        self.runs = runs;
    }
}

/// The values of the animations running on an element, ready to lay over the
/// style beneath them.
pub(crate) struct AnimationStyle {
    style: Vec<(AnimProp, AnimValue)>,
    svg: Vec<(SvgAttr, AnimValue)>,
}

impl AnimationStyle {
    /// The SVG attributes the animations drive, as the XML writes them.
    pub(crate) fn svg_attributes(&self) -> Vec<(&'static str, String)> {
        self.svg
            .iter()
            .filter_map(|(attr, value)| Some((attr.attribute()?, attribute_text(value))))
            .collect()
    }

    /// Lays the animated values over `style`, except the properties a running
    /// transition holds: `style` already has the transition's value for them,
    /// and a transition beats an animation in the cascade.
    pub(crate) fn apply_to(self, style: &mut StyleDesc, transitions: Option<&TransitionStyle>) {
        for (prop, value) in self.style {
            if transitions.is_some_and(|transitions| transitions.runs(prop)) {
                continue;
            }
            prop.write(style, value);
        }
    }
}

/// Whether `style` sets an animation name that is not `none`.
pub(super) fn declares_animation(style: &StyleDesc) -> bool {
    [&style.animation, &style.animation_name]
        .into_iter()
        .any(|text| text.as_deref().is_some_and(|text| !text.trim().eq_ignore_ascii_case("none")))
}

/// Whether an animation of `style` has keyframes for `width`, which at `auto`
/// needs the natural width measured before the animation plans.
pub(crate) fn animates_width(style: &StyleDesc, registry: &KeyframesRegistry) -> bool {
    AnimationList::parse(style).is_ok_and(|list| {
        list.0
            .iter()
            .filter_map(|animation| animation.name.as_deref())
            .filter_map(|name| registry.get(name))
            .any(|keyframes| keyframes.properties().contains(&PropKey::Style(AnimProp::Width)))
    })
}

/// The animation state of every element that declares one.
#[derive(Default)]
pub(crate) struct CssAnimations {
    states: HashMap<u64, ElementState>,
    events: Vec<Scheduled>,
    /// What the Web Animations objects of JS are told: finishes and cancels.
    api_events: Vec<ApiEvent>,
    /// The elements a Web Animations command reached since the last frame.
    commanded: Vec<u64>,
    active: bool,
    /// The elements this frame leaves dormant (see `retain::Sources::dormant`): what keeps their
    /// animations going is a tick that builds nothing, so they do not ask for a drawn frame.
    dormant: HashSet<u64>,
    dormant_active: bool,
    sleepers: Sleepers,
    /// The roots `display: none` has cancelled for, until they render again.
    hidden: HashSet<u64>,
}

/// When the clocks of the dormant elements have something due, so that a tick looks at those and
/// not at every dormant element.
#[derive(Default)]
struct Sleepers {
    /// The instants at which an element's runs next have something to report, earliest first.
    due: BinaryHeap<Reverse<(Instant, u64)>>,
    /// The instant of the entry of each element that is live in `due`; an entry with another
    /// instant, or of an element that is not here, is left from a time it was.
    scheduled: HashMap<u64, Instant>,
    /// Elements to look at in the next tick whatever `due` says: one just dormant, one a build or a
    /// command touched, one with a run that cannot say when it is due.
    next: HashSet<u64>,
    /// The dormant elements with a run that is still moving, as of the last tick or build.
    moving: HashSet<u64>,
    #[cfg(test)]
    visits: u64,
}

impl CssAnimations {
    /// Whether `id` takes part in animations: it declares one, in its own style
    /// or a state style, or already has animation state to keep.
    pub(crate) fn wants(&self, id: u64, style: Option<&StyleDesc>) -> bool {
        self.states.contains_key(&id)
            || style.is_some_and(|style| {
                declares_animation(style)
                    || super::state_styles(style)
                        .into_iter()
                        .any(|(_, state)| state.is_some_and(declares_animation))
            })
    }

    /// The first half of a frame: brings the animations of `declared` (the
    /// element's style with its state styles folded in, which holds the
    /// `animation*` values) up to `now`, and returns the properties they
    /// currently drive, running or filling. Transitions need these before they
    /// look at the style: a property an animation drives has the same
    /// before-change and after-change value, so a declared change starts no
    /// transition on it. [`update`](Self::update) finishes the frame.
    pub(crate) fn advance(
        &mut self,
        id: u64,
        declared: &StyleDesc,
        registry: &KeyframesRegistry,
        scope: Scope,
        now: Instant,
    ) -> Vec<AnimProp> {
        let state = self.states.entry(id).or_insert_with(ElementState::new);
        state.sync_config(id, declared);
        state.reconcile(id, registry, now, &mut self.events, &mut self.api_events);
        // A build moves the clocks and the parameters of the runs, which the instants a dormant
        // element's clock was due at were taken from.
        if !self.dormant.is_empty() && self.dormant.contains(&id) {
            self.sleepers.next.insert(id);
        }

        let mut covered = Vec::new();
        for run in &mut state.runs {
            run.tick(id, now, &mut self.events, &mut self.api_events);
            if run.progress.is_some() {
                covered.extend(run.properties.iter().filter_map(|key| match key {
                    PropKey::Style(prop) if scope.animates(*prop) => Some(*prop),
                    _ => None,
                }));
            }
        }
        covered
    }

    /// The second half of a frame, after [`advance`](Self::advance): the values
    /// of the animations. `declared` is the style [`advance`](Self::advance) read
    /// the `animation*` values from, plus the transitions; `subject` is what the
    /// animations lay over, transitions and motion included, which supplies the
    /// endpoints a keyframe set leaves out. `inherited` is the
    /// `interpolateSize` of the parent.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn update(
        &mut self,
        id: u64,
        declared: &StyleDesc,
        subject: &Subject,
        parent_colors: &ColorContext,
        parent_font: &ParentFont,
        measurement: Option<&Measurement>,
        inherited: InterpolateSize,
        scope: Scope,
        now: Instant,
    ) -> Option<AnimationStyle> {
        let state = self.states.get_mut(&id).expect("advance made the state of this frame");
        state.target = subject.target;
        let allow_keywords = InterpolateSize::declared(declared)
            .unwrap_or(inherited)
            .allows_keywords();
        let mut values: BTreeMap<PropKey, Option<AnimValue>> = BTreeMap::new();
        for run in &mut state.runs {
            if let Some(progress) = run.progress {
                // Later in the list is higher in the cascade.
                values.extend(run.values(
                    progress,
                    subject,
                    parent_colors,
                    parent_font,
                    measurement,
                    allow_keywords,
                    now,
                ));
            }
            let wants = run.wants_frame(now);
            if self.dormant.contains(&id) {
                self.dormant_active |= wants;
            } else {
                self.active |= wants;
            }
        }
        state.moving = state.runs.iter().any(|run| run.wants_frame(now));

        if state.runs.is_empty() && state.list.0.iter().all(|animation| animation.name.is_none()) {
            self.states.remove(&id);
        }
        self.note_moving(id);
        let mut animated = AnimationStyle { style: Vec::new(), svg: Vec::new() };
        for (key, value) in values {
            let Some(value) = value else {
                continue;
            };
            match key {
                PropKey::Style(prop) if scope.animates(prop) => animated.style.push((prop, value)),
                PropKey::Svg(attr) => animated.svg.push((attr, value)),
                PropKey::Style(_) => {}
            }
        }
        (!animated.style.is_empty() || !animated.svg.is_empty()).then_some(animated)
    }

    /// Brings the runs of `id` up to `now` without a style: iterations and finishes are reported
    /// as `advance` would, `getAnimations` reads the new time, and the element asks for another
    /// tick if a run is still moving. The frame that builds the element afterwards at the same
    /// instant finds nothing left to report.
    pub(crate) fn advance_clock(&mut self, id: u64, now: Instant) {
        let Some(state) = self.states.get_mut(&id) else {
            return;
        };
        let mut ticked = false;
        for run in &mut state.runs {
            if run.quiet_until.is_some_and(|until| now < until) {
                continue;
            }
            run.tick(id, now, &mut self.events, &mut self.api_events);
            run.quiet_until = run.quiet_after(now);
            ticked = true;
        }
        if ticked {
            state.moving = state.runs.iter().any(|run| run.wants_frame(now));
        }
        self.dormant_active |= state.moving;
        self.note_moving(id);
    }

    /// Whether everything running on `id` only changes what it paints: every property its
    /// keyframes animate is one that moves no box and no line of text. A name whose keyframes are
    /// not defined, or were redefined since the run read them, may start or change what is
    /// animated, so it is not.
    pub(crate) fn paints_only(&self, id: u64, registry: &KeyframesRegistry) -> bool {
        let Some(state) = self.states.get(&id) else {
            return true;
        };
        let defined = state.list.0.iter().filter_map(|animation| animation.name.as_deref()).all(|name| {
            registry.get(name).is_some_and(|keyframes| {
                state.runs.iter().any(|run| run.name == name && Arc::ptr_eq(&run.keyframes, keyframes))
            })
        });
        defined
            && state.runs.iter().all(|run| {
                run.properties.iter().all(|key| match key {
                    PropKey::Style(prop) => prop.paints_only(),
                    // The attributes of an SVG shape change the picture of the `<svg>`, never its box.
                    PropKey::Svg(_) => true,
                })
            })
    }

    /// Whether an animation of `id` that is current animates `opacity`.
    pub(crate) fn animates_opacity(&self, id: u64) -> bool {
        self.states.get(&id).is_some_and(|state| {
            state
                .runs
                .iter()
                .any(|run| run.is_current() && run.properties.contains(&PropKey::Style(AnimProp::Opacity)))
        })
    }

    /// `id` is left dormant by the frame being built: its clock is carried on by
    /// `advance_dormant`, and what it asks of the window goes to a tick that draws nothing.
    pub(crate) fn enter_dormant(&mut self, id: u64) {
        self.dormant.insert(id);
        self.sleepers.next.insert(id);
        self.note_moving(id);
    }

    pub(crate) fn leave_dormant(&mut self, id: u64) {
        if !self.dormant.remove(&id) {
            return;
        }
        self.sleepers.next.remove(&id);
        self.sleepers.scheduled.remove(&id);
        self.sleepers.moving.remove(&id);
    }

    fn note_moving(&mut self, id: u64) {
        if self.dormant.is_empty() {
            return;
        }
        if self.dormant.contains(&id) && self.states.get(&id).is_some_and(|state| state.moving) {
            self.sleepers.moving.insert(id);
        } else {
            self.sleepers.moving.remove(&id);
        }
    }

    /// `advance_clock` for the dormant elements whose runs have something due at `now`: the ones
    /// that are new, that a build or a command touched, or whose next boundary has come. The rest
    /// would report nothing and change nothing, and are not looked at.
    pub(crate) fn advance_dormant(&mut self, now: Instant) {
        let sleepers = &mut self.sleepers;
        let mut visit: HashSet<u64> = std::mem::take(&mut sleepers.next);
        while let Some(&Reverse((at, id))) = sleepers.due.peek() {
            if at > now {
                break;
            }
            sleepers.due.pop();
            if sleepers.scheduled.get(&id) == Some(&at) {
                sleepers.scheduled.remove(&id);
                visit.insert(id);
            }
        }
        for id in visit {
            if !self.dormant.contains(&id) {
                continue;
            }
            #[cfg(test)]
            {
                self.sleepers.visits += 1;
            }
            self.advance_clock(id, now);
            self.schedule(id);
        }
        let sleepers = &mut self.sleepers;
        if sleepers.due.len() > 2 * sleepers.scheduled.len() + 64 {
            sleepers.due = sleepers.scheduled.iter().map(|(id, at)| Reverse((*at, *id))).collect();
        }
        self.dormant_active |= !self.sleepers.moving.is_empty();
    }

    /// Notes when `id` is next due: at the earliest instant one of its runs is quiet until, or in
    /// the next tick if a run cannot say.
    fn schedule(&mut self, id: u64) {
        let Some(state) = self.states.get(&id) else {
            return;
        };
        let mut earliest: Option<Instant> = None;
        for run in &state.runs {
            let Some(until) = run.quiet_until else {
                self.sleepers.next.insert(id);
                return;
            };
            earliest = Some(earliest.map_or(until, |earliest| earliest.min(until)));
        }
        if let Some(at) = earliest {
            self.sleepers.due.push(Reverse((at, id)));
            self.sleepers.scheduled.insert(id, at);
        }
    }

    /// Whether a dormant element still has a run that moves, as of the last frame or tick.
    pub(crate) fn take_dormant_active(&mut self) -> bool {
        std::mem::take(&mut self.dormant_active)
    }

    /// A frame of a shape inside an `<svg>`: its animations advance and the SVG
    /// properties they drive come back. A shape has no layout and starts no
    /// transitions, so nothing waits between the two halves of the frame.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn animate_shape(
        &mut self,
        id: u64,
        declared: &StyleDesc,
        registry: &KeyframesRegistry,
        subject: &Subject,
        parent_colors: &ColorContext,
        parent_font: &ParentFont,
        now: Instant,
    ) -> Option<AnimationStyle> {
        self.advance(id, declared, registry, Scope::Element, now);
        self.update(
            id,
            declared,
            subject,
            parent_colors,
            parent_font,
            None,
            InterpolateSize::default(),
            Scope::Element,
            now,
        )
    }

    /// The shapes inside an `<svg>` whose animations show something new next
    /// frame. A shape is not built itself, so its `<svg>` has to be, to write it.
    pub(crate) fn svg_shapes<'a>(&'a self, registry: &'a KeyframesRegistry) -> impl Iterator<Item = u64> + 'a {
        self.states
            .iter()
            .filter(|(_, state)| state.target == PropTarget::SvgShape && state.shows_something_new(registry))
            .map(|(id, _)| *id)
    }

    /// Whether the shape `id` inside an `<svg>` is one of `svg_shapes`.
    pub(crate) fn shape_moves(&self, id: u64, registry: &KeyframesRegistry) -> bool {
        self.states
            .get(&id)
            .is_some_and(|state| state.target == PropTarget::SvgShape && state.shows_something_new(registry))
    }

    /// `id` is `display: none`, which takes it and everything below it out of
    /// rendering: its animations are cancelled and its state dropped, so they
    /// start over when it renders again. Cheap to call every frame the element
    /// stays hidden.
    pub(crate) fn hide(&mut self, tree: &RetainedTree, id: u64, now: Instant) {
        if !self.hidden.insert(id) || self.states.is_empty() {
            return;
        }
        for hidden in super::subtree_ids(tree, id) {
            let Some(mut state) = self.states.remove(&hidden) else {
                continue;
            };
            self.sleepers.moving.remove(&hidden);
            for run in &mut state.runs {
                run.cancel(hidden, now, &mut self.events, &mut self.api_events);
            }
        }
    }

    /// `id` renders: it is no longer hidden.
    pub(crate) fn show(&mut self, id: u64) {
        if !self.hidden.is_empty() {
            self.hidden.remove(&id);
        }
    }

    pub(crate) fn retain(&mut self, mut keep: impl FnMut(u64) -> bool) {
        self.states.retain(|id, _| keep(*id));
        self.hidden.retain(|id| keep(*id));
    }

    /// Events queued since the last call, in the order they happened.
    pub(crate) fn take_events(&mut self) -> Vec<AnimationEvent> {
        let mut scheduled = std::mem::take(&mut self.events);
        scheduled.sort_by_key(|scheduled| scheduled.at);
        scheduled.into_iter().map(|scheduled| scheduled.event).collect()
    }

    /// Whether an animation still needs frames as of the last update.
    pub(crate) fn take_active(&mut self) -> bool {
        std::mem::take(&mut self.active)
    }

    /// The animations of `element` in composite order (`animation-name` list
    /// position) that are relevant: current, or in effect through a fill.
    pub(crate) fn list(&self, element: u64, now: Instant) -> Vec<Info> {
        self.states.get(&element).map_or_else(Vec::new, |state| {
            state.runs.iter().filter(|run| run.is_relevant()).map(|run| run.info(now)).collect()
        })
    }

    /// The animation `id`, relevant or not. `None` once it is cancelled.
    pub(crate) fn info(&self, id: u64, now: Instant) -> Option<Info> {
        self.run(id).map(|run| run.info(now))
    }

    /// Applies `command` to the animation `id` at `now`, the time of the last
    /// frame. `None` for an id that is not running.
    pub(crate) fn command(&mut self, id: u64, command: Command, now: Instant) -> Option<Result<(), String>> {
        let (element, run) = self
            .states
            .iter_mut()
            .flat_map(|(element, state)| state.runs.iter_mut().map(move |run| (*element, run)))
            .find(|(_, run)| run.clock.id() == id)?;
        let end = run.end_time();
        run.quiet_until = None;
        let outcome = run.clock.apply(command, now, end);
        self.commanded.push(element);
        if self.dormant.contains(&element) {
            self.sleepers.next.insert(element);
        }
        Some(outcome)
    }

    fn run(&self, id: u64) -> Option<&Run> {
        self.states.values().flat_map(|state| state.runs.iter()).find(|run| run.clock.id() == id)
    }

    /// The elements a command reached since the last call, which the frame has to build again.
    pub(crate) fn take_commanded(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.commanded)
    }

    /// [`Self::take_commanded`], leaving them for the frame that takes them.
    pub(crate) fn peek_commanded(&self) -> &[u64] {
        &self.commanded
    }

    /// Finishes and cancels since the last call, for the Web Animations objects.
    pub(crate) fn take_api_events(&mut self) -> Vec<ApiEvent> {
        std::mem::take(&mut self.api_events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(json: &str) -> StyleDesc {
        serde_json::from_str(json).unwrap()
    }

    fn parse(json: &str) -> Vec<Animation> {
        AnimationList::parse(&style(json)).unwrap().0
    }

    fn error(json: &str) -> String {
        AnimationList::parse(&style(json)).unwrap_err()
    }

    #[test]
    fn a_style_without_animation_fields_has_no_animations() {
        assert!(parse(r#"{ "opacity": 1 }"#).is_empty());
    }

    #[test]
    fn the_shorthand_takes_its_parts_in_any_order() {
        let animations = parse(r#"{ "animation": "spin 2s linear 500ms infinite alternate both paused" }"#);
        assert_eq!(
            animations,
            vec![Animation {
                name: Some("spin".to_string()),
                duration: 2.0,
                delay: 0.5,
                timing: Ease::Name("linear".to_string()),
                iteration_count: IterationCount::Infinite,
                direction: Direction::Alternate,
                fill_mode: FillMode::Both,
                play_state: PlayState::Paused,
            }]
        );
        let reordered = parse(r#"{ "animation": "paused both alternate infinite 500ms linear 2s spin" }"#);
        // The first time is still the duration, so the times swap here.
        assert_eq!((reordered[0].duration, reordered[0].delay), (0.5, 2.0));
        assert_eq!(reordered[0].name.as_deref(), Some("spin"));
    }

    #[test]
    fn omitted_parts_take_their_initial_values() {
        let animations = parse(r#"{ "animation": "fade 1s" }"#);
        assert_eq!(animations[0].timing, Ease::CubicBezier([0.25, 0.1, 0.25, 1.0]));
        assert_eq!(animations[0].delay, 0.0);
        assert_eq!(animations[0].iteration_count, IterationCount::Finite(1.0));
        assert_eq!(animations[0].direction, Direction::Normal);
        assert_eq!(animations[0].fill_mode, FillMode::None);
        assert_eq!(animations[0].play_state, PlayState::Running);
    }

    #[test]
    fn a_comma_list_makes_one_animation_per_item() {
        let animations = parse(r#"{ "animation": "a 1s, b 2s cubic-bezier(0, 0, 1, 1) 3, c" }"#);
        assert_eq!(animations.len(), 3);
        assert_eq!(animations[1].name.as_deref(), Some("b"));
        assert_eq!(animations[1].timing, Ease::CubicBezier([0.0, 0.0, 1.0, 1.0]));
        assert_eq!(animations[1].iteration_count, IterationCount::Finite(3.0));
        assert_eq!(animations[2].duration, 0.0);
    }

    #[test]
    fn a_keyword_for_another_longhand_is_not_the_name() {
        let animations = parse(r#"{ "animation": "none" }"#);
        assert_eq!(animations.len(), 1);
        assert_eq!(animations[0].name, None);
        assert_eq!(animations[0].fill_mode, FillMode::None);

        // `forwards` is the fill mode; `reverse` is taken, so the second is the name.
        let animations = parse(r#"{ "animation": "1s reverse forwards" }"#);
        assert_eq!(animations[0].name, None);
        assert_eq!(animations[0].fill_mode, FillMode::Forwards);
        assert_eq!(animations[0].direction, Direction::Reverse);

        let animations = parse(r#"{ "animation": "forwards forwards 1s" }"#);
        assert_eq!(animations[0].name.as_deref(), Some("forwards"));
        assert_eq!(animations[0].fill_mode, FillMode::Forwards);
    }

    #[test]
    fn a_name_is_case_sensitive_and_keywords_are_not() {
        let animations = parse(r#"{ "animation": "Spin 1s INFINITE ALTERNATE" }"#);
        assert_eq!(animations[0].name.as_deref(), Some("Spin"));
        assert_eq!(animations[0].iteration_count, IterationCount::Infinite);
        assert_eq!(animations[0].direction, Direction::Alternate);
    }

    #[test]
    fn fractional_and_zero_counts_are_valid() {
        let animations = parse(r#"{ "animation": "spin 1s 2.5" }"#);
        assert_eq!(animations[0].iteration_count, IterationCount::Finite(2.5));
        let animations = parse(r#"{ "animationIterationCount": "0" }"#);
        assert_eq!(animations[0].iteration_count, IterationCount::Finite(0.0));
    }

    #[test]
    fn a_negative_delay_is_kept() {
        let animations = parse(r#"{ "animation": "spin 2s -500ms" }"#);
        assert_eq!(animations[0].delay, -0.5);
    }

    #[test]
    fn longhands_win_over_the_shorthand() {
        let animations = parse(
            r#"{ "animation": "spin 1s linear", "animationDuration": "3s", "animationPlayState": "paused" }"#,
        );
        assert_eq!(animations[0].name.as_deref(), Some("spin"));
        assert_eq!(animations[0].duration, 3.0);
        assert_eq!(animations[0].play_state, PlayState::Paused);
        assert_eq!(animations[0].timing, Ease::Name("linear".to_string()));
    }

    #[test]
    fn longhands_alone_start_from_initial_values() {
        let animations = parse(r#"{ "animationName": "spin", "animationDuration": "1s" }"#);
        assert_eq!(animations.len(), 1);
        assert_eq!(animations[0].fill_mode, FillMode::None);
        assert_eq!(animations[0].iteration_count, IterationCount::Finite(1.0));
    }

    #[test]
    fn shorter_lists_repeat_and_longer_ones_truncate_to_the_name_count() {
        let animations = parse(
            r#"{
                "animationName": "a, b, c",
                "animationDuration": "1s, 2s",
                "animationDirection": "reverse, normal, alternate, alternate-reverse"
            }"#,
        );
        assert_eq!(animations.len(), 3);
        let durations: Vec<f64> = animations.iter().map(|animation| animation.duration).collect();
        assert_eq!(durations, [1.0, 2.0, 1.0]);
        assert_eq!(animations[2].direction, Direction::Alternate);
    }

    #[test]
    fn name_none_keeps_its_slot_and_animates_nothing() {
        let animations = parse(r#"{ "animationName": "none, spin", "animationDuration": "1s, 2s" }"#);
        assert_eq!(animations[0].name, None);
        assert_eq!(animations[1].name.as_deref(), Some("spin"));
        assert_eq!(animations[1].duration, 2.0);
    }

    #[test]
    fn auto_duration_is_zero() {
        assert_eq!(parse(r#"{ "animationDuration": "auto" }"#)[0].duration, 0.0);
    }

    #[test]
    fn initial_and_unset_reset_the_shorthand() {
        let animations = parse(r#"{ "animation": "initial" }"#);
        assert_eq!(animations[0].name, None);
        assert!(error(r#"{ "animation": "inherit" }"#).contains("not supported"));
    }

    #[test]
    fn invalid_values_name_the_field() {
        assert!(error(r#"{ "animation": "spin 1s 2s 3s" }"#).starts_with("animation: too many times"));
        assert!(error(r#"{ "animation": "spin 1s ease linear" }"#).contains("two timing functions"));
        assert!(error(r#"{ "animation": "a b 1s" }"#).contains("two names"));
        assert!(error(r#"{ "animation": "spin -1s" }"#).contains("cannot be negative"));
        assert!(error(r#"{ "animation": "spin 1s -2" }"#).contains("cannot be negative"));
        assert!(error(r#"{ "animation": "1s bogus(" }"#).starts_with("animation:"));
        assert!(error(r#"{ "animation": "" }"#).starts_with("animation:"));
        assert!(error(r#"{ "animation": "a,,b" }"#).starts_with("animation:"));
        assert!(error(r#"{ "animationName": "3d" }"#).starts_with("animationName:"));
        assert!(error(r#"{ "animationName": "initial" }"#).starts_with("animationName:"));
        assert!(error(r#"{ "animationDuration": "fast" }"#).starts_with("animationDuration:"));
        assert!(error(r#"{ "animationDuration": "-1s" }"#).starts_with("animationDuration:"));
        assert!(error(r#"{ "animationTimingFunction": "wobble" }"#)
            .starts_with("animationTimingFunction:"));
        assert!(error(r#"{ "animationDelay": "1" }"#).starts_with("animationDelay:"));
        assert!(error(r#"{ "animationIterationCount": "many" }"#)
            .starts_with("animationIterationCount:"));
        assert!(error(r#"{ "animationIterationCount": "NaN" }"#)
            .starts_with("animationIterationCount:"));
        assert!(error(r#"{ "animationDirection": "sideways" }"#).starts_with("animationDirection:"));
        assert!(error(r#"{ "animationFillMode": "after" }"#).starts_with("animationFillMode:"));
        assert!(error(r#"{ "animationPlayState": "stopped" }"#).starts_with("animationPlayState:"));
    }

    #[test]
    fn validation_covers_state_styles() {
        assert!(validate_animations(&style(r#"{ "animation": "spin 1s" }"#)).is_ok());
        let error = validate_animations(&style(r#"{ "hover": { "animationDelay": "soon" } }"#))
            .unwrap_err();
        assert!(error.starts_with("hover.animationDelay:"), "{error}");
        assert!(validate_animations(&style(r#"{ "focusVisible": { "animation": "a b" } }"#)).is_err());
        for name in ["groupHover", "groupActive", "groupFocusWithin", "hasHover", "hasActive", "hasFocus", "hasFocusVisible", "disabled", "enabled", "hasDisabled", "hasEnabled"] {
            let error = validate_animations(&style(&format!(r#"{{ "{name}": {{ "animationDelay": "soon" }} }}"#)))
                .unwrap_err();
            assert!(error.starts_with(&format!("{name}.animationDelay:")), "{error}");
        }
    }

    #[test]
    fn keyframes_names_are_identifiers() {
        for name in ["spin", "fade-in", "_x", "-vendor-spin", "Spin2", "é"] {
            assert!(check_keyframes_name(name).is_ok(), "{name}");
        }
        for name in ["", "2d", "-2", "a b", "a,b", "none", "Inherit", "a(b)"] {
            assert!(check_keyframes_name(name).is_err(), "{name}");
        }
    }

    // ── Runtime ──────────────────────────────────────────────────────

    use crate::motion::CssTransitions;
    use crate::style::DimensionValue;
    use AnimationEventKind::{Cancel, End, Iteration, Start};

    struct Frames {
        origin: Instant,
        animations: CssAnimations,
        registry: KeyframesRegistry,
        inherited: InterpolateSize,
        parent_colors: ColorContext,
        parent_font: ParentFont,
    }

    type Seen = Vec<(AnimationEventKind, String, f64)>;

    impl Frames {
        fn new() -> Self {
            Self {
                origin: Instant::now(),
                animations: CssAnimations::default(),
                registry: KeyframesRegistry::default(),
                inherited: InterpolateSize::default(),
                parent_colors: ColorContext::root(crate::color::Scheme::Light),
                parent_font: ParentFont::initial(),
            }
        }

        fn define(&mut self, name: &str, frames: &str) {
            self.registry.define(name, frames).unwrap();
        }

        fn at(&self, millis: u64) -> Instant {
            self.origin + Duration::from_millis(millis)
        }

        /// One frame at `millis`: `style` is both what declares the animation
        /// and what the animation lays over.
        fn frame(&mut self, millis: u64, style: &StyleDesc) -> StyleDesc {
            self.frame_measured(millis, style, None)
        }

        fn frame_measured(
            &mut self,
            millis: u64,
            style: &StyleDesc,
            measurement: Option<&Measurement>,
        ) -> StyleDesc {
            self.frame_over(millis, style, style, measurement)
        }

        fn frame_over(
            &mut self,
            millis: u64,
            declared: &StyleDesc,
            underlying: &StyleDesc,
            measurement: Option<&Measurement>,
        ) -> StyleDesc {
            let mut resolved = underlying.clone();
            let now = self.at(millis);
            self.animations.advance(1, declared, &self.registry, Scope::Element, now);
            if let Some(values) = self.animations.update(
                1,
                declared,
                &Subject::element(underlying),
                &self.parent_colors,
                &self.parent_font,
                measurement,
                self.inherited,
                Scope::Element,
                now,
            ) {
                values.apply_to(&mut resolved, None);
            }
            resolved
        }

        /// One frame the way `build_element` stacks it: the transitions on
        /// `declared`, then the animations over the result.
        fn frame_with_transitions(
            &mut self,
            transitions: &mut CssTransitions,
            millis: u64,
            declared: &StyleDesc,
        ) -> StyleDesc {
            let now = self.at(millis);
            let covered = self.animations.advance(1, declared, &self.registry, Scope::Element, now);
            let running = transitions.update(
                1,
                declared,
                &self.parent_colors,
                &self.parent_font,
                None,
                None,
                self.inherited,
                Scope::Element,
                &covered,
                now,
            );
            let mut resolved = declared.clone();
            if let Some(running) = &running {
                running.apply_to(&mut resolved);
            }
            let underlying = resolved.clone();
            if let Some(values) = self.animations.update(
                1,
                declared,
                &Subject::element(&underlying),
                &self.parent_colors,
                &self.parent_font,
                None,
                self.inherited,
                Scope::Element,
                now,
            ) {
                values.apply_to(&mut resolved, running.as_ref());
            }
            resolved
        }

        /// One frame of a shape inside an `<svg>` at `millis`: the SVG attributes
        /// its animations write.
        fn shape(
            &mut self,
            millis: u64,
            declared: &StyleDesc,
            own: &HashMap<String, Value>,
            ancestors: &[&HashMap<String, Value>],
        ) -> Vec<(&'static str, String)> {
            let now = self.at(millis);
            let subject = Subject::svg_shape(declared, SvgSource { own, ancestors });
            self.animations
                .animate_shape(1, declared, &self.registry, &subject, &self.parent_colors, &self.parent_font, now)
                .map(|values| values.svg_attributes())
                .unwrap_or_default()
        }

        fn events(&mut self) -> Seen {
            self.animations
                .take_events()
                .into_iter()
                .map(|event| (event.kind, event.name, event.elapsed))
                .collect()
        }
    }

    fn near(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-9, "{actual} is not {expected}");
    }

    fn opacity(style: &StyleDesc) -> f64 {
        style.opacity.expect("the style has an opacity")
    }

    fn named(kind: AnimationEventKind, name: &str, elapsed: f64) -> (AnimationEventKind, String, f64) {
        (kind, name.to_string(), elapsed)
    }

    const FADE: &str = r#"{ "from": { "opacity": 0 }, "to": { "opacity": 1 } }"#;

    fn fading(animation: &str) -> StyleDesc {
        style(&format!(r#"{{ "animation": "{animation}" }}"#))
    }

    #[test]
    fn display_none_cancels_the_animation_and_it_starts_over_when_shown() {
        let mut tree = crate::retained_tree::RetainedTree::new();
        tree.create_element(1, "div".to_string());
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s linear");
        frames.frame(0, &animated);
        frames.frame(400, &animated);
        frames.events();

        frames.animations.hide(&tree, 1, frames.at(400));
        assert_eq!(frames.events(), [named(Cancel, "fade", 0.4)]);
        frames.animations.hide(&tree, 1, frames.at(416));
        assert!(frames.events().is_empty());

        frames.animations.show(1);
        near(opacity(&frames.frame(2000, &animated)), 0.0);
        assert_eq!(frames.events(), [named(Start, "fade", 0.0)]);
        near(opacity(&frames.frame(2500, &animated)), 0.5);
    }

    #[test]
    fn keyframes_of_a_multi_value_shorthand_move_each_side_to_its_own_value() {
        let mut frames = Frames::new();
        frames.define(
            "pad",
            r#"{ "from": { "padding": "0 0", "borderRadius": "0" }, "to": { "padding": "20px 40px", "borderRadius": "10px / 20px" } }"#,
        );
        let padded = style(r#"{ "animation": "pad 1s linear" }"#);
        frames.frame(0, &padded);
        let middle = frames.frame(500, &padded);
        assert_eq!(middle.padding_top, Some(crate::css::length::LinearLength::px(10.0)));
        assert_eq!(middle.padding_left, Some(crate::css::length::LinearLength::px(20.0)));
        assert_eq!(
            middle.border_top_left_radius,
            Some(crate::style::CornerRadiusValue { x: crate::css::length::LinearLength::px(5.0), y: Some(crate::css::length::LinearLength::px(10.0)) })
        );
    }

    fn dimmed(opacity: f64, animation: &str) -> StyleDesc {
        style(&format!(
            r#"{{ "opacity": {opacity}, "transition": "opacity 1s linear", "animation": "{animation}" }}"#
        ))
    }

    const DIM: &str = r#"{ "from": { "opacity": 0.2 }, "to": { "opacity": 0.2 } }"#;

    fn transition_events(transitions: &mut CssTransitions) -> Vec<String> {
        transitions
            .take_events()
            .into_iter()
            .map(|event| format!("{} {}", event.kind.event_type(), event.property_name()))
            .collect()
    }

    #[test]
    fn a_running_transition_beats_an_animation_that_started_after_it() {
        let mut frames = Frames::new();
        let mut transitions = CssTransitions::default();
        frames.define("dim", DIM);
        frames.frame_with_transitions(&mut transitions, 0, &dimmed(1.0, "none"));

        let moving = dimmed(0.6, "none");
        near(opacity(&frames.frame_with_transitions(&mut transitions, 100, &moving)), 1.0);
        let dimming = dimmed(0.6, "dim 10s linear");
        near(opacity(&frames.frame_with_transitions(&mut transitions, 600, &dimming)), 0.8);
        near(opacity(&frames.frame_with_transitions(&mut transitions, 1000, &dimming)), 0.64);

        near(opacity(&frames.frame_with_transitions(&mut transitions, 1100, &dimming)), 0.2);
        near(opacity(&frames.frame_with_transitions(&mut transitions, 1200, &dimming)), 0.2);
    }

    #[test]
    fn a_declared_change_under_a_running_animation_starts_no_transition() {
        let mut frames = Frames::new();
        let mut transitions = CssTransitions::default();
        frames.define("dim", DIM);
        near(opacity(&frames.frame_with_transitions(&mut transitions, 0, &dimmed(1.0, "dim 10s linear"))), 0.2);
        transition_events(&mut transitions);

        let target = dimmed(0.6, "dim 10s linear");
        for millis in [100, 600, 5000] {
            near(opacity(&frames.frame_with_transitions(&mut transitions, millis, &target)), 0.2);
        }
        assert!(transition_events(&mut transitions).is_empty());
        assert!(!transitions.take_active());

        // The declared value shows when the animation ends, with no transition from
        // the value the animation showed.
        for millis in [10_000, 10_100, 10_600] {
            near(opacity(&frames.frame_with_transitions(&mut transitions, millis, &target)), 0.6);
        }
        assert!(transition_events(&mut transitions).is_empty());
        assert!(!transitions.take_active());
    }

    #[test]
    fn an_animation_added_with_the_change_covers_it() {
        let mut frames = Frames::new();
        let mut transitions = CssTransitions::default();
        frames.define("dim", DIM);
        frames.frame_with_transitions(&mut transitions, 0, &dimmed(1.0, "none"));
        let target = dimmed(0.6, "dim 10s linear");
        near(opacity(&frames.frame_with_transitions(&mut transitions, 100, &target)), 0.2);
        assert!(transition_events(&mut transitions).is_empty());
        near(opacity(&frames.frame_with_transitions(&mut transitions, 10_200, &target)), 0.6);
    }

    #[test]
    fn an_animation_still_in_its_delay_does_not_cover() {
        let mut frames = Frames::new();
        let mut transitions = CssTransitions::default();
        frames.define("dim", DIM);
        frames.frame_with_transitions(&mut transitions, 0, &dimmed(1.0, "dim 10s linear 5s"));
        let target = dimmed(0.6, "dim 10s linear 5s");
        near(opacity(&frames.frame_with_transitions(&mut transitions, 100, &target)), 1.0);
        near(opacity(&frames.frame_with_transitions(&mut transitions, 600, &target)), 0.8);
        assert_eq!(transition_events(&mut transitions), ["transitionrun opacity", "transitionstart opacity"]);
    }

    #[test]
    fn a_change_after_the_animation_ended_transitions() {
        let mut frames = Frames::new();
        let mut transitions = CssTransitions::default();
        frames.define("dim", DIM);
        let animation = "dim 1s linear";
        near(opacity(&frames.frame_with_transitions(&mut transitions, 0, &dimmed(1.0, animation))), 0.2);
        near(opacity(&frames.frame_with_transitions(&mut transitions, 1500, &dimmed(1.0, animation))), 1.0);

        let target = dimmed(0.6, animation);
        near(opacity(&frames.frame_with_transitions(&mut transitions, 2000, &target)), 1.0);
        near(opacity(&frames.frame_with_transitions(&mut transitions, 2500, &target)), 0.8);
        near(opacity(&frames.frame_with_transitions(&mut transitions, 3000, &target)), 0.6);
    }

    #[test]
    fn a_forwards_fill_keeps_covering_the_property() {
        let mut frames = Frames::new();
        let mut transitions = CssTransitions::default();
        frames.define("dim", DIM);
        let animation = "dim 1s linear forwards";
        frames.frame_with_transitions(&mut transitions, 0, &dimmed(1.0, animation));
        near(opacity(&frames.frame_with_transitions(&mut transitions, 1500, &dimmed(1.0, animation))), 0.2);
        transition_events(&mut transitions);

        let target = dimmed(0.6, animation);
        for millis in [2000, 2500, 3000] {
            near(opacity(&frames.frame_with_transitions(&mut transitions, millis, &target)), 0.2);
        }
        assert!(transition_events(&mut transitions).is_empty());

        // Dropping the animation uncovers the property: the declared value shows, as
        // the before-change style already had it.
        let released = dimmed(0.6, "none");
        near(opacity(&frames.frame_with_transitions(&mut transitions, 3100, &released)), 0.6);
        assert!(transition_events(&mut transitions).is_empty());
    }

    #[test]
    fn a_declared_change_cancels_the_transition_it_made_stale_under_an_animation() {
        let mut frames = Frames::new();
        let mut transitions = CssTransitions::default();
        frames.define("dim", DIM);
        frames.frame_with_transitions(&mut transitions, 0, &dimmed(1.0, "none"));
        frames.frame_with_transitions(&mut transitions, 100, &dimmed(0.6, "none"));
        let dimming = dimmed(0.6, "dim 10s linear");
        near(opacity(&frames.frame_with_transitions(&mut transitions, 300, &dimming)), 0.92);
        transition_events(&mut transitions);

        let changed = dimmed(0.3, "dim 10s linear");
        near(opacity(&frames.frame_with_transitions(&mut transitions, 400, &changed)), 0.2);
        assert_eq!(transition_events(&mut transitions), ["transitioncancel opacity"]);
        near(opacity(&frames.frame_with_transitions(&mut transitions, 11_000, &changed)), 0.3);
        assert!(transition_events(&mut transitions).is_empty());
    }

    #[test]
    fn a_transition_on_another_property_leaves_the_animation_alone() {
        let mut frames = Frames::new();
        let mut transitions = CssTransitions::default();
        frames.define("dim", r#"{ "from": { "opacity": 0.2 }, "to": { "opacity": 0.2 } }"#);
        let sized = |width: f64| {
            style(&format!(
                r#"{{ "width": {width}, "transition": "width 1s linear", "animation": "dim 10s linear" }}"#
            ))
        };
        frames.frame_with_transitions(&mut transitions, 0, &sized(0.0));
        let resolved = frames.frame_with_transitions(&mut transitions, 500, &sized(100.0));
        near(opacity(&resolved), 0.2);
        assert!(matches!(resolved.width, Some(DimensionValue::Pixels(_))));
        let halfway = frames.frame_with_transitions(&mut transitions, 1000, &sized(100.0));
        assert!(matches!(halfway.width, Some(DimensionValue::Pixels(px)) if (px - 50.0).abs() < 1e-6));
        assert_eq!(transition_events(&mut transitions), ["transitionrun width", "transitionstart width"]);
    }

    #[test]
    fn an_animation_alone_never_starts_a_transition() {
        let mut frames = Frames::new();
        let mut transitions = CssTransitions::default();
        frames.define("dim", r#"{ "from": { "opacity": 1 }, "to": { "opacity": 0 } }"#);
        let declared = dimmed(1.0, "dim 10s linear");
        for millis in [0, 100, 500, 900] {
            frames.frame_with_transitions(&mut transitions, millis, &declared);
        }
        near(opacity(&frames.frame_with_transitions(&mut transitions, 5000, &declared)), 0.5);
        assert!(!transitions.take_active());
    }

    #[test]
    fn an_element_without_animation_fields_takes_no_part() {
        let frames = Frames::new();
        assert!(!frames.animations.wants(1, Some(&style(r#"{ "opacity": 1 }"#))));
        assert!(!frames.animations.wants(1, Some(&fading("none"))));
        assert!(!frames.animations.wants(1, None));
        assert!(frames.animations.wants(1, Some(&fading("fade 1s"))));
        assert!(frames.animations.wants(1, Some(&style(r#"{ "animationName": "fade" }"#))));
        assert!(frames
            .animations
            .wants(1, Some(&style(r#"{ "hover": { "animation": "fade 1s" } }"#))));
        for name in ["groupHover", "groupActive", "groupFocusWithin", "hasHover", "hasActive", "hasFocus", "hasFocusVisible", "disabled", "enabled", "hasDisabled", "hasEnabled"] {
            assert!(frames
                .animations
                .wants(1, Some(&style(&format!(r#"{{ "{name}": {{ "animation": "fade 1s" }} }}"#)))));
        }
    }

    #[test]
    fn an_animation_runs_from_the_first_style_and_ends_on_the_underlying_value() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s linear");
        near(opacity(&frames.frame(0, &animated)), 0.0);
        assert_eq!(frames.events(), [named(Start, "fade", 0.0)]);
        near(opacity(&frames.frame(250, &animated)), 0.25);
        near(opacity(&frames.frame(750, &animated)), 0.75);
        assert!(frames.animations.take_active());

        // Fill mode none: once it ends the element is as it was declared.
        let finished = frames.frame(1000, &animated);
        assert_eq!(finished.opacity, None);
        assert_eq!(frames.events(), [named(End, "fade", 1.0)]);
        assert!(!frames.animations.take_active());
        assert_eq!(frames.frame(5000, &animated).opacity, None);
        assert!(frames.events().is_empty(), "a finished animation does not restart");
    }

    #[test]
    fn fill_modes_decide_what_shows_before_the_delay_and_after_the_end() {
        let shown = |fill: &str| {
            let mut frames = Frames::new();
            frames.define("fade", r#"{ "from": { "opacity": 0.1 }, "to": { "opacity": 0.8 } }"#);
            let animated = fading(&format!("fade 1s linear 1s {fill}"));
            let during_delay = frames.frame(0, &animated).opacity;
            let running = frames.frame(1500, &animated).opacity;
            let finished = frames.frame(2500, &animated).opacity;
            near(running.unwrap(), 0.45);
            (during_delay, finished)
        };
        assert_eq!(shown("none"), (None, None));
        assert_eq!(shown("forwards"), (None, Some(0.8)));
        assert_eq!(shown("backwards"), (Some(0.1), None));
        assert_eq!(shown("both"), (Some(0.1), Some(0.8)));
    }

    #[test]
    fn directions_pick_which_way_each_iteration_plays() {
        let at = |direction: &str| {
            let mut frames = Frames::new();
            frames.define("fade", FADE);
            let animated = fading(&format!("fade 1s linear 3 {direction}"));
            frames.frame(0, &animated);
            [250, 1250, 2250].map(|millis| opacity(&frames.frame(millis, &animated)))
        };
        let expect = |actual: [f64; 3], expected: [f64; 3]| {
            actual.iter().zip(expected).for_each(|(actual, expected)| near(*actual, expected));
        };
        expect(at("normal"), [0.25, 0.25, 0.25]);
        expect(at("reverse"), [0.75, 0.75, 0.75]);
        expect(at("alternate"), [0.25, 0.75, 0.25]);
        expect(at("alternate-reverse"), [0.75, 0.25, 0.75]);
    }

    #[test]
    fn reversing_plays_time_backwards_but_keeps_each_interval_easing() {
        let ease_in = Ease::CubicBezier([0.42, 0.0, 1.0, 1.0]);
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s ease-in reverse");
        frames.frame(0, &animated);
        // A quarter of the way in, the keyframes are three quarters through.
        near(opacity(&frames.frame(250, &animated)), ease(0.75, &ease_in));
    }

    #[test]
    fn a_whole_iteration_count_ends_on_the_last_frame_of_the_last_iteration() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let forwards = fading("fade 1s linear 2 forwards");
        frames.frame(0, &forwards);
        near(opacity(&frames.frame(5000, &forwards)), 1.0);

        let mut frames = Frames::new();
        frames.define("fade", FADE);
        // The second iteration runs backwards, so it ends where it began.
        let alternating = fading("fade 1s linear 2 alternate forwards");
        frames.frame(0, &alternating);
        near(opacity(&frames.frame(5000, &alternating)), 0.0);
    }

    #[test]
    fn a_fractional_count_stops_part_way_through_the_last_iteration() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s linear 2.5 forwards");
        frames.frame(0, &animated);
        near(opacity(&frames.frame(2250, &animated)), 0.25);
        assert!(frames.animations.take_active());
        near(opacity(&frames.frame(2500, &animated)), 0.5);
        near(opacity(&frames.frame(9000, &animated)), 0.5);
        assert!(!frames.animations.take_active());
        assert_eq!(frames.events().last(), Some(&named(End, "fade", 2.5)));

        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let reversed = fading("fade 1s linear 1.25 alternate forwards");
        frames.frame(0, &reversed);
        // The partial iteration is the second, which plays backwards.
        near(opacity(&frames.frame(9000, &reversed)), 0.75);
    }

    #[test]
    fn infinite_never_ends_and_keeps_asking_for_frames() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s linear infinite");
        frames.frame(0, &animated);
        near(opacity(&frames.frame(1_000_000_250, &animated)), 0.25);
        assert!(frames.animations.take_active());
        assert!(!frames.events().iter().any(|event| event.0 == End));
    }

    #[test]
    fn a_zero_count_or_duration_starts_and_ends_at_once() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let none = fading("fade 1s 0");
        assert_eq!(frames.frame(0, &none).opacity, None);
        assert_eq!(frames.events(), [named(Start, "fade", 0.0), named(End, "fade", 0.0)]);

        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let instant = fading("fade 0s 3 forwards");
        near(opacity(&frames.frame(0, &instant)), 1.0);
        assert_eq!(frames.events(), [named(Start, "fade", 0.0), named(End, "fade", 0.0)]);
    }

    #[test]
    fn a_negative_delay_starts_part_way_through() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 2s linear -500ms");
        near(opacity(&frames.frame(0, &animated)), 0.25);
        assert_eq!(frames.events(), [named(Start, "fade", 0.5)]);
        near(opacity(&frames.frame(1000, &animated)), 0.75);
        assert_eq!(frames.frame(1500, &animated).opacity, None);
        assert_eq!(frames.events(), [named(End, "fade", 2.0)]);
    }

    #[test]
    fn a_delay_holds_until_it_ends_then_reports_the_start() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s linear 500ms");
        frames.frame(0, &animated);
        frames.frame(400, &animated);
        assert!(frames.events().is_empty());
        assert!(frames.animations.take_active());
        near(opacity(&frames.frame(1000, &animated)), 0.5);
        assert_eq!(frames.events(), [named(Start, "fade", 0.0)]);
    }

    #[test]
    fn pausing_freezes_the_animation_and_resuming_continues_it() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let running = fading("fade 1s linear");
        let paused = style(r#"{ "animation": "fade 1s linear", "animationPlayState": "paused" }"#);
        near(opacity(&frames.frame(0, &running)), 0.0);
        near(opacity(&frames.frame(500, &running)), 0.5);
        near(opacity(&frames.frame(500, &paused)), 0.5);
        frames.animations.take_active();
        near(opacity(&frames.frame(5000, &paused)), 0.5);
        assert!(!frames.animations.take_active(), "a paused animation needs no frames");
        near(opacity(&frames.frame(5000, &running)), 0.5);
        near(opacity(&frames.frame(5250, &running)), 0.75);
        assert_eq!(frames.frame(5500, &running).opacity, None);
        assert_eq!(frames.events().last(), Some(&named(End, "fade", 1.0)));
    }

    #[test]
    fn an_animation_that_starts_paused_stays_at_its_first_frame() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let paused = style(r#"{ "animation": "fade 1s linear paused" }"#);
        near(opacity(&frames.frame(0, &paused)), 0.0);
        near(opacity(&frames.frame(10_000, &paused)), 0.0);
        assert_eq!(frames.events(), [named(Start, "fade", 0.0)]);
    }

    #[test]
    fn a_new_name_cancels_the_old_animation_and_restarts() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        frames.define("dim", r#"{ "from": { "opacity": 0.5 }, "to": { "opacity": 1 } }"#);
        frames.frame(0, &fading("fade 1s linear"));
        near(opacity(&frames.frame(600, &fading("fade 1s linear"))), 0.6);
        frames.events();

        let swapped = fading("dim 1s linear");
        near(opacity(&frames.frame(700, &swapped)), 0.5);
        assert_eq!(frames.events(), [named(Cancel, "fade", 0.7), named(Start, "dim", 0.0)]);
        near(opacity(&frames.frame(1200, &swapped)), 0.75);
    }

    #[test]
    fn the_same_name_keeps_running_when_its_other_values_change() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        frames.frame(0, &fading("fade 1s linear"));
        frames.events();
        near(opacity(&frames.frame(500, &fading("fade 2s linear"))), 0.25);
        assert!(frames.events().is_empty());
    }

    #[test]
    fn removing_the_animation_cancels_it_unless_it_already_ended() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        frames.frame(0, &fading("fade 1s linear"));
        frames.events();
        assert_eq!(frames.frame(400, &fading("none")).opacity, None);
        assert_eq!(frames.events(), [named(Cancel, "fade", 0.4)]);
        assert!(!frames.animations.wants(1, Some(&fading("none"))));
        assert!(frames.animations.states.is_empty(), "the state goes with the cancel");

        let mut frames = Frames::new();
        frames.define("fade", FADE);
        frames.frame(0, &fading("fade 1s linear 1s"));
        frames.events();
        frames.frame(300, &fading("none"));
        assert_eq!(frames.events(), [named(Cancel, "fade", 0.0)], "cancelled during its delay");

        let mut frames = Frames::new();
        frames.define("fade", FADE);
        frames.frame(0, &fading("fade 1s linear forwards"));
        frames.frame(2000, &fading("fade 1s linear forwards"));
        frames.events();
        frames.frame(3000, &fading("none"));
        assert!(frames.events().is_empty(), "an animation that ended is not cancelled");
        assert!(frames.animations.states.is_empty());
    }

    #[test]
    fn an_animation_that_ran_out_between_frames_ends_instead_of_being_cancelled() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        frames.frame(0, &fading("fade 1s linear 2"));
        frames.frame(500, &fading("fade 1s linear 2"));
        frames.events();
        // The two iterations were over at 2000 ms, and the next frame is 2400 ms.
        frames.frame(2400, &fading("none"));
        assert_eq!(frames.events(), [named(End, "fade", 2.0)]);

        // The same when a new name replaces it, and when the element is hidden.
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        frames.define("dim", r#"{ "from": { "opacity": 0.5 }, "to": { "opacity": 1 } }"#);
        frames.frame(0, &fading("fade 1s linear"));
        frames.events();
        frames.frame(1500, &fading("dim 1s linear"));
        assert_eq!(
            frames.events(),
            [named(End, "fade", 1.0), named(Start, "dim", 0.0)]
        );

        let tree = crate::retained_tree::RetainedTree::new();
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        frames.frame(0, &fading("fade 1s linear"));
        frames.events();
        frames.animations.hide(&tree, 1, frames.at(1500));
        assert_eq!(frames.events(), [named(End, "fade", 1.0)]);
    }

    #[test]
    fn an_undefined_name_animates_nothing_until_it_is_defined() {
        let mut frames = Frames::new();
        let animated = fading("fade 1s linear");
        assert_eq!(frames.frame(0, &animated).opacity, None);
        assert!(frames.events().is_empty());

        frames.define("fade", FADE);
        near(opacity(&frames.frame(2000, &animated)), 0.0);
        assert_eq!(frames.events(), [named(Start, "fade", 0.0)]);
        near(opacity(&frames.frame(2500, &animated)), 0.5);
    }

    #[test]
    fn redefining_the_keyframes_changes_a_running_animation_and_keeps_its_timeline() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s linear");
        frames.frame(0, &animated);
        near(opacity(&frames.frame(250, &animated)), 0.25);
        frames.events();

        frames.define("fade", r#"{ "from": { "opacity": 1 }, "to": { "opacity": 0 } }"#);
        near(opacity(&frames.frame(500, &animated)), 0.5);
        near(opacity(&frames.frame(750, &animated)), 0.25);
        assert!(frames.events().is_empty(), "a redefinition neither restarts nor cancels");
        frames.frame(1000, &animated);
        assert_eq!(frames.events(), [named(End, "fade", 1.0)]);
    }

    #[test]
    fn a_redefinition_keeps_the_iteration_direction_and_fill() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s linear 2 alternate forwards");
        frames.frame(0, &animated);
        near(opacity(&frames.frame(1250, &animated)), 0.75);
        frames.events();

        frames.define("fade", r#"{ "from": { "opacity": 0.2 }, "to": { "opacity": 0.6 } }"#);
        // The second iteration plays backwards: 0.25 of the way to 0.2.
        near(opacity(&frames.frame(1250, &animated)), 0.5);
        assert!(frames.events().is_empty());
        // Forwards fill holds the end of the new definition.
        near(opacity(&frames.frame(3000, &animated)), 0.2);
        frames.define("fade", r#"{ "from": { "opacity": 0.4 }, "to": { "opacity": 0.7 } }"#);
        near(opacity(&frames.frame(3500, &animated)), 0.4);
    }

    #[test]
    fn a_redefinition_adds_and_drops_properties() {
        let mut frames = Frames::new();
        frames.define("move", r#"{ "from": { "width": 0 }, "to": { "width": 100 } }"#);
        let animated = fading("move 1s linear");
        frames.frame(0, &animated);
        assert!(frames.frame(500, &animated).width.is_some());

        frames.define("move", r#"{ "from": { "opacity": 0 }, "to": { "opacity": 1 } }"#);
        let sample = frames.frame(500, &animated);
        assert_eq!(sample.width, None);
        near(opacity(&sample), 0.5);
    }

    #[test]
    fn a_redefinition_reaches_every_animation_that_names_it() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s linear");
        let later = fading("fade 1s linear");
        let sample = |frames: &mut Frames, id: u64, millis: u64, declared: &StyleDesc| {
            let now = frames.at(millis);
            frames.animations.advance(id, declared, &frames.registry, Scope::Element, now);
            let values = frames.animations.update(
                id,
                declared,
                &Subject::element(declared),
                &frames.parent_colors,
                &frames.parent_font,
                None,
                frames.inherited,
                Scope::Element,
                now,
            );
            let mut resolved = declared.clone();
            values.expect("the animation has an effect").apply_to(&mut resolved, None);
            opacity(&resolved)
        };
        near(sample(&mut frames, 1, 0, &animated), 0.0);
        near(sample(&mut frames, 2, 500, &later), 0.0);

        frames.define("fade", r#"{ "from": { "opacity": 1 }, "to": { "opacity": 0 } }"#);
        near(sample(&mut frames, 1, 500, &animated), 0.5);
        near(sample(&mut frames, 1, 750, &animated), 0.25);
        near(sample(&mut frames, 2, 750, &later), 0.75);
    }

    #[test]
    fn a_redefinition_reaches_a_paused_animation() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let paused = fading("fade 1s linear paused");
        near(opacity(&frames.frame(0, &paused)), 0.0);
        frames.define("fade", r#"{ "from": { "opacity": 0.5 }, "to": { "opacity": 1 } }"#);
        near(opacity(&frames.frame(100, &paused)), 0.5);
    }

    #[test]
    fn the_animation_timing_function_eases_every_keyframe_interval() {
        let ease_in = Ease::CubicBezier([0.42, 0.0, 1.0, 1.0]);
        let mut frames = Frames::new();
        frames.define(
            "pulse",
            r#"{ "0%": { "opacity": 0 }, "50%": { "opacity": 1 }, "100%": { "opacity": 0 } }"#,
        );
        let animated = fading("pulse 2s ease-in");
        frames.frame(0, &animated);
        near(opacity(&frames.frame(500, &animated)), ease(0.5, &ease_in));
        near(opacity(&frames.frame(1000, &animated)), 1.0);
        near(opacity(&frames.frame(1500, &animated)), 1.0 - ease(0.5, &ease_in));
    }

    #[test]
    fn a_keyframe_timing_function_eases_from_that_keyframe_to_the_next() {
        let mut frames = Frames::new();
        frames.define(
            "pulse",
            r#"{
                "0%": { "opacity": 0, "animationTimingFunction": "steps(2, jump-end)" },
                "50%": { "opacity": 1 },
                "100%": { "opacity": 0.5 }
            }"#,
        );
        let animated = fading("pulse 2s linear");
        frames.frame(0, &animated);
        // Two steps over the first interval: 0, then 0.5 from its midpoint.
        near(opacity(&frames.frame(250, &animated)), 0.0);
        near(opacity(&frames.frame(500, &animated)), 0.5);
        near(opacity(&frames.frame(1000, &animated)), 1.0);
        // The second interval has no easing of its own: the animation's, linear.
        near(opacity(&frames.frame(1500, &animated)), 0.75);
    }

    #[test]
    fn a_keyframe_only_matters_to_the_properties_it_names() {
        let mut frames = Frames::new();
        frames.define(
            "grow",
            r#"{
                "from": { "width": 0, "opacity": 0 },
                "50%": { "opacity": 1, "animationTimingFunction": "steps(1, jump-end)" },
                "to": { "width": 100, "opacity": 0 }
            }"#,
        );
        let animated = fading("grow 2s linear");
        frames.frame(0, &animated);
        let sample = frames.frame(500, &animated);
        // Width has no 50% keyframe, so its interval spans the whole animation
        // and the step easing of that keyframe is not its own.
        match sample.width {
            Some(DimensionValue::Pixels(width)) => near(width, 25.0),
            other => panic!("{other:?}"),
        }
        near(opacity(&sample), 0.5);
    }

    #[test]
    fn missing_endpoints_are_the_underlying_value() {
        let mut frames = Frames::new();
        frames.define("dip", r#"{ "50%": { "opacity": 0.2 } }"#);
        let animated = style(r#"{ "animation": "dip 1s linear", "opacity": 0.9 }"#);
        near(opacity(&frames.frame(0, &animated)), 0.9);
        near(opacity(&frames.frame(250, &animated)), 0.55);
        near(opacity(&frames.frame(500, &animated)), 0.2);
        near(opacity(&frames.frame(750, &animated)), 0.55);
    }

    #[test]
    fn a_to_only_animation_starts_from_the_underlying_value() {
        let mut frames = Frames::new();
        frames.define("rise", r#"{ "to": { "opacity": 1 } }"#);
        let animated = style(r#"{ "animation": "rise 1s linear", "opacity": 0.2 }"#);
        near(opacity(&frames.frame(0, &animated)), 0.2);
        near(opacity(&frames.frame(500, &animated)), 0.6);
        near(opacity(&frames.frame(999, &animated)), 0.9992);
    }

    #[test]
    fn a_property_in_only_the_middle_interpolates_from_and_back_to_the_underlying_value() {
        let mut frames = Frames::new();
        frames.define(
            "dip",
            r#"{ "40%": { "opacity": 0.2, "animationTimingFunction": "linear" }, "to": { "width": 100 } }"#,
        );
        let animated = style(r#"{ "animation": "dip 1s linear", "opacity": 0.6, "width": 20 }"#);
        near(opacity(&frames.frame(0, &animated)), 0.6);
        near(opacity(&frames.frame(200, &animated)), 0.4);
        near(opacity(&frames.frame(400, &animated)), 0.2);
        near(opacity(&frames.frame(700, &animated)), 0.4);
        near(opacity(&frames.frame(1000, &animated)), 0.6);
    }

    #[test]
    fn properties_take_their_own_underlying_values_independently() {
        let mut frames = Frames::new();
        frames.define(
            "mix",
            r#"{ "from": { "opacity": 0 }, "50%": { "width": 100 }, "to": { "opacity": 1, "width": 0 } }"#,
        );
        let animated = style(r#"{ "animation": "mix 1s linear", "opacity": 0.5, "width": 20 }"#);
        let width = |sample: &StyleDesc| match sample.width {
            Some(DimensionValue::Pixels(width)) => width,
            ref other => panic!("{other:?}"),
        };
        // Opacity has no 50% keyframe; width has no 0% one.
        let start = frames.frame(0, &animated);
        near(opacity(&start), 0.0);
        near(width(&start), 20.0);
        let middle = frames.frame(500, &animated);
        near(opacity(&middle), 0.5);
        near(width(&middle), 100.0);
        let late = frames.frame(750, &animated);
        near(opacity(&late), 0.75);
        near(width(&late), 50.0);
    }

    #[test]
    fn implicit_endpoints_follow_the_iteration_and_the_direction() {
        let mut frames = Frames::new();
        frames.define("rise", r#"{ "to": { "opacity": 1 } }"#);
        let animated = style(r#"{ "animation": "rise 1s linear 2 alternate", "opacity": 0.2 }"#);
        near(opacity(&frames.frame(0, &animated)), 0.2);
        near(opacity(&frames.frame(500, &animated)), 0.6);
        // The second iteration runs backwards, from the explicit 1 to the implicit 0.2.
        near(opacity(&frames.frame(1250, &animated)), 0.8);
        near(opacity(&frames.frame(1750, &animated)), 0.4);
        let reversed = style(r#"{ "animation": "rise 1s linear reverse", "opacity": 0.2 }"#);
        let mut frames = Frames::new();
        frames.define("rise", r#"{ "to": { "opacity": 1 } }"#);
        near(opacity(&frames.frame(0, &reversed)), 1.0);
        near(opacity(&frames.frame(250, &reversed)), 0.8);
    }

    #[test]
    fn a_missing_endpoint_follows_the_underlying_value_as_it_changes() {
        let mut frames = Frames::new();
        frames.define("fade", r#"{ "from": { "opacity": 0 } }"#);
        let declared = fading("fade 1s linear forwards");
        let first = style(r#"{ "animation": "fade 1s linear forwards", "opacity": 0.8 }"#);
        let second = style(r#"{ "animation": "fade 1s linear forwards", "opacity": 0.4 }"#);
        frames.frame_over(0, &declared, &first, None);
        near(opacity(&frames.frame_over(250, &declared, &first, None)), 0.2);
        near(opacity(&frames.frame_over(500, &declared, &second, None)), 0.2);
        near(opacity(&frames.frame_over(2000, &declared, &second, None)), 0.4);
    }

    #[test]
    fn a_missing_color_endpoint_is_the_inherited_colour() {
        let mut frames = Frames::new();
        frames.parent_colors = ColorContext::root(crate::color::Scheme::Light)
            .descend(&style(r##"{ "color": "#ff0000" }"##));
        frames.define("tint", r##"{ "to": { "color": "#0000ff" } }"##);
        let animated = fading("tint 1s linear");
        let color = |sample: StyleDesc| crate::color::parse_color_rgba(&sample.color.unwrap()).unwrap();
        assert_eq!(color(frames.frame(0, &animated)).r, 1.0);
        let middle = color(frames.frame(500, &animated));
        assert!(middle.r > 0.4 && middle.b > 0.4, "{middle:?}");
    }

    #[test]
    fn an_endpoint_that_cannot_be_resolved_holds_the_nearest_keyframe() {
        let mut frames = Frames::new();
        frames.define("grow", r#"{ "to": { "lineHeight": 2 } }"#);
        let animated = fading("grow 1s linear");
        let multiplier = Some(crate::css::line_height::LineHeight::Multiplier(2.0));
        assert_eq!(frames.frame(0, &animated).line_height, multiplier);
        assert_eq!(frames.frame(500, &animated).line_height, multiplier);
    }

    #[test]
    fn a_missing_font_size_endpoint_is_the_inherited_size() {
        let mut frames = Frames::new();
        frames.parent_font = ParentFont {
            lengths: crate::css::length::LengthContext::initial(800.0, 600.0)
                .descend(Some(&crate::css::length::LinearLength::px(20.0)), true),
            above_root: false,
            weight: 700.0,
            variations: Default::default(),
            inheritable: Default::default(),
        };
        frames.define("grow", r#"{ "to": { "fontSize": 40, "fontWeight": 300 } }"#);
        let animated = fading("grow 1s linear");
        assert_eq!(frames.frame(0, &animated).font_size, Some(crate::css::length::LinearLength::px(20.0)));
        let middle = frames.frame(500, &animated);
        assert_eq!(middle.font_size, Some(crate::css::length::LinearLength::px(30.0)));
        assert_eq!(AnimProp::FontWeight.read(&middle, &Colors::root()), Some(AnimValue::Number(500.0)));
    }

    #[test]
    fn an_animation_beats_the_value_it_lays_over_and_the_later_one_wins() {
        let mut frames = Frames::new();
        frames.define("a", r#"{ "from": { "opacity": 0.2 }, "to": { "opacity": 0.2 } }"#);
        frames.define("b", r#"{ "from": { "opacity": 0.6 }, "to": { "opacity": 0.6 } }"#);
        let both = style(r#"{ "animation": "a 1s, b 1s", "opacity": 1 }"#);
        near(opacity(&frames.frame(0, &both)), 0.6);
        let reversed = style(r#"{ "animation": "b 1s, a 1s", "opacity": 1 }"#);
        let mut frames = Frames::new();
        frames.define("a", r#"{ "from": { "opacity": 0.2 }, "to": { "opacity": 0.2 } }"#);
        frames.define("b", r#"{ "from": { "opacity": 0.6 }, "to": { "opacity": 0.6 } }"#);
        near(opacity(&frames.frame(0, &reversed)), 0.2);
    }

    #[test]
    fn events_follow_the_animation_through_its_iterations() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s linear 3");
        frames.frame(0, &animated);
        assert_eq!(frames.events(), [named(Start, "fade", 0.0)]);
        frames.frame(500, &animated);
        assert!(frames.events().is_empty());
        frames.frame(1100, &animated);
        assert_eq!(frames.events(), [named(Iteration, "fade", 1.0)]);
        frames.frame(2100, &animated);
        assert_eq!(frames.events(), [named(Iteration, "fade", 2.0)]);
        frames.frame(3100, &animated);
        assert_eq!(frames.events(), [named(End, "fade", 3.0)]);
    }

    #[test]
    fn a_frame_that_skips_iterations_reports_the_latest_one_once() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s linear 5");
        frames.frame(0, &animated);
        frames.events();
        frames.frame(3500, &animated);
        assert_eq!(frames.events(), [named(Iteration, "fade", 3.0)]);
    }

    #[test]
    fn a_frame_that_skips_the_whole_animation_reports_start_then_end() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s linear 1s");
        frames.frame(0, &animated);
        assert!(frames.events().is_empty());
        frames.frame(10_000, &animated);
        assert_eq!(frames.events(), [named(Start, "fade", 0.0), named(End, "fade", 1.0)]);
    }

    #[test]
    fn events_of_different_animations_come_in_the_order_they_happened() {
        let mut frames = Frames::new();
        frames.define("a", FADE);
        frames.define("b", FADE);
        let animated = style(r#"{ "animation": "a 1s linear, b 1s linear 500ms" }"#);
        frames.frame(0, &animated);
        assert_eq!(frames.events(), [named(Start, "a", 0.0)]);
        // One late frame: b starts half a second in, before a ends at one second.
        frames.frame(3000, &animated);
        assert_eq!(
            frames.events(),
            [named(Start, "b", 0.0), named(End, "a", 1.0), named(End, "b", 1.0)]
        );
    }

    /// An element `width` wide that would be `natural` wide with `width: auto`.
    fn measured_wide(width: f32, natural: f32) -> Measurement {
        Measurement {
            natural_width: Some(gpui::px(natural)),
            ..measured(width, 10.0, 10.0)
        }
    }

    fn measured(width: f32, height: f32, natural: f32) -> Measurement {
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

    fn height_of(style: &StyleDesc) -> DimensionValue {
        style.height.clone().expect("the style has a height")
    }

    #[test]
    fn height_auto_runs_through_the_measured_height_and_goes_back_to_auto() {
        let mut frames = Frames::new();
        frames.define("open", r#"{ "from": { "height": 0 }, "to": { "height": "auto" } }"#);
        let animated = style(
            r#"{ "animation": "open 1s linear forwards", "interpolateSize": "allow-keywords" }"#,
        );
        // Until layout is known the element shows where the animation starts.
        assert_eq!(frames.frame(0, &animated).height, None);
        assert!(frames.animations.take_active());

        let measurement = measured(100.0, 0.0, 120.0);
        let halfway = frames.frame_measured(500, &animated, Some(&measurement));
        assert_eq!(height_of(&halfway), DimensionValue::Pixels(60.0));
        // The end follows the measurement as the content grows.
        let grown = measured(100.0, 60.0, 200.0);
        assert_eq!(
            height_of(&frames.frame_measured(500, &animated, Some(&grown))),
            DimensionValue::Pixels(100.0)
        );
        assert_eq!(
            height_of(&frames.frame_measured(1000, &animated, Some(&grown))),
            DimensionValue::Auto
        );
        assert_eq!(
            height_of(&frames.frame_measured(1000, &animated, Some(&grown))),
            DimensionValue::Auto
        );
    }

    #[test]
    fn a_length_against_a_keyword_swaps_halfway_unless_keywords_are_allowed() {
        let mut frames = Frames::new();
        frames.define("open", r#"{ "from": { "height": 0 }, "to": { "height": "auto" } }"#);
        let discrete = fading("open 1s linear");
        let measurement = measured(100.0, 0.0, 120.0);
        frames.frame_measured(0, &discrete, Some(&measurement));
        assert_eq!(
            height_of(&frames.frame_measured(400, &discrete, Some(&measurement))),
            DimensionValue::Pixels(0.0)
        );
        assert_eq!(
            height_of(&frames.frame_measured(600, &discrete, Some(&measurement))),
            DimensionValue::Auto
        );

        // `interpolateSize` is inherited.
        let mut frames = Frames::new();
        frames.define("open", r#"{ "from": { "height": 0 }, "to": { "height": "auto" } }"#);
        frames.inherited = InterpolateSize::AllowKeywords;
        frames.frame_measured(0, &discrete, Some(&measurement));
        assert_eq!(
            height_of(&frames.frame_measured(500, &discrete, Some(&measurement))),
            DimensionValue::Pixels(60.0)
        );
    }

    #[test]
    fn an_unset_size_endpoint_is_a_keyword_that_swaps_without_interpolate_size() {
        let mut frames = Frames::new();
        frames.define("grow", r#"{ "to": { "width": 100 } }"#);
        let animated = fading("grow 1s linear forwards");
        let measurement = measured(40.0, 10.0, 10.0);
        frames.frame_measured(0, &animated, Some(&measurement));
        assert_eq!(
            frames.frame_measured(250, &animated, Some(&measurement)).width,
            Some(DimensionValue::Auto)
        );
        assert_eq!(
            frames.frame_measured(750, &animated, Some(&measurement)).width,
            Some(DimensionValue::Pixels(100.0))
        );
    }

    #[test]
    fn an_unset_width_endpoint_starts_from_the_natural_width_once_keywords_are_allowed() {
        let mut frames = Frames::new();
        frames.define("grow", r#"{ "to": { "width": 100 } }"#);
        let animated = style(
            r#"{ "animation": "grow 1s linear", "interpolateSize": "allow-keywords" }"#,
        );
        assert_eq!(frames.frame(0, &animated).width, None, "waits for a measurement");
        assert!(frames.animations.take_active());
        // A measurement that did not measure the width keeps waiting.
        assert_eq!(frames.frame_measured(250, &animated, Some(&measured(40.0, 10.0, 10.0))).width, None);
        assert!(frames.animations.take_active());
        let first = measured_wide(40.0, 40.0);
        match frames.frame_measured(500, &animated, Some(&first)).width {
            Some(DimensionValue::Pixels(width)) => near(width, 70.0),
            other => panic!("{other:?}"),
        }
        // The box moved because of the animation, but its natural width did not.
        let moved = measured_wide(70.0, 40.0);
        match frames.frame_measured(500, &animated, Some(&moved)).width {
            Some(DimensionValue::Pixels(width)) => near(width, 70.0),
            other => panic!("{other:?}"),
        }
    }

    fn width_of(style: &StyleDesc) -> DimensionValue {
        style.width.clone().expect("the style has a width")
    }

    #[test]
    fn width_auto_runs_through_the_natural_width_and_goes_back_to_auto() {
        let mut frames = Frames::new();
        frames.define("open", r#"{ "from": { "width": 0 }, "to": { "width": "auto" } }"#);
        let animated = style(
            r#"{ "animation": "open 1s linear forwards", "interpolateSize": "allow-keywords" }"#,
        );
        assert_eq!(frames.frame(0, &animated).width, None);
        assert!(frames.animations.take_active());
        // A measurement from a frame that did not measure the width keeps it waiting.
        assert_eq!(frames.frame_measured(100, &animated, Some(&measured(0.0, 10.0, 10.0))).width, None);
        assert!(frames.animations.take_active());

        let halfway = frames.frame_measured(500, &animated, Some(&measured_wide(0.0, 120.0)));
        assert_eq!(width_of(&halfway), DimensionValue::Pixels(60.0));
        // The end follows the measurement as the content grows.
        let grown = measured_wide(60.0, 200.0);
        assert_eq!(
            width_of(&frames.frame_measured(500, &animated, Some(&grown))),
            DimensionValue::Pixels(100.0)
        );
        assert_eq!(
            width_of(&frames.frame_measured(1000, &animated, Some(&grown))),
            DimensionValue::Auto
        );
    }

    #[test]
    fn a_width_against_auto_swaps_halfway_unless_keywords_are_allowed() {
        let mut frames = Frames::new();
        frames.define("open", r#"{ "from": { "width": 0 }, "to": { "width": "auto" } }"#);
        let discrete = fading("open 1s linear");
        let measurement = measured_wide(0.0, 120.0);
        frames.frame_measured(0, &discrete, Some(&measurement));
        assert_eq!(
            width_of(&frames.frame_measured(400, &discrete, Some(&measurement))),
            DimensionValue::Pixels(0.0)
        );
        assert_eq!(
            width_of(&frames.frame_measured(600, &discrete, Some(&measurement))),
            DimensionValue::Auto
        );
    }

    #[test]
    fn only_keyframes_for_width_under_allow_keywords_ask_for_the_natural_width() {
        let mut registry = KeyframesRegistry::default();
        registry.define("open", r#"{ "to": { "width": "auto" } }"#).unwrap();
        registry.define("fade", r#"{ "to": { "opacity": 0 } }"#).unwrap();
        assert!(animates_width(&style(r#"{ "animation": "open 1s" }"#), &registry));
        assert!(animates_width(&style(r#"{ "animationName": "fade, open" }"#), &registry));
        assert!(!animates_width(&style(r#"{ "animation": "fade 1s" }"#), &registry));
        assert!(!animates_width(&style(r#"{ "animation": "missing 1s" }"#), &registry));
        assert!(!animates_width(&style(r#"{ "width": 10 }"#), &registry));
    }

    #[test]
    fn a_percentage_meeting_a_length_blends_as_a_calc_and_ends_as_the_percentage() {
        let mut frames = Frames::new();
        frames.define("grow", r#"{ "from": { "width": 20 }, "to": { "width": "50%" } }"#);
        let animated = fading("grow 1s linear forwards");
        let measurement = measured(40.0, 10.0, 10.0);
        frames.frame_measured(0, &animated, Some(&measurement));
        let middle = frames.frame_measured(500, &animated, Some(&measurement)).width;
        assert_eq!(
            middle,
            Some(DimensionValue::Calc(crate::css::length::LinearLength { px: 10.0, percent: 0.25, ..Default::default() }))
        );
        assert_eq!(
            frames.frame_measured(2000, &animated, Some(&measurement)).width,
            Some(DimensionValue::Percentage(0.5))
        );
    }

    #[test]
    fn calc_keyframes_and_em_keyframes_blend_per_part() {
        let mut frames = Frames::new();
        frames.define(
            "grow",
            r#"{ "from": { "width": "calc(100% - 20px)" }, "50%": { "width": "calc(50% + 1em)" }, "to": { "width": 100 } }"#,
        );
        let animated = fading("grow 1s linear forwards");
        frames.frame(0, &animated);
        let calc = |px: f64, percent: f64| {
            Some(DimensionValue::Calc(crate::css::length::LinearLength { px, percent, ..Default::default() }))
        };
        assert_eq!(frames.frame(250, &animated).width, calc(-2.0, 0.75));
        assert_eq!(frames.frame(500, &animated).width, calc(16.0, 0.5));
        assert_eq!(frames.frame(750, &animated).width, calc(58.0, 0.25));
        assert_eq!(frames.frame(1000, &animated).width, Some(DimensionValue::Pixels(100.0)));
    }

    fn background(style: &StyleDesc) -> [f32; 3] {
        let color = crate::color::parse_color_rgba(style.background_color.as_deref().unwrap()).unwrap();
        [color.r, color.g, color.b]
    }

    fn assert_colour(actual: [f32; 3], expected: &str) {
        let expected = crate::color::parse_color_rgba(expected).unwrap();
        for (actual, expected) in actual.into_iter().zip([expected.r, expected.g, expected.b]) {
            assert!((actual - expected).abs() < 0.01, "{actual:?} vs {expected:?}");
        }
    }

    #[test]
    fn legacy_keyframe_colours_interpolate_in_srgb() {
        let mut frames = Frames::new();
        frames.define(
            "tint",
            r##"{ "from": { "backgroundColor": "red" }, "to": { "backgroundColor": "hsl(240 100% 50%)" } }"##,
        );
        let animated = fading("tint 1s linear");
        frames.frame(0, &animated);
        assert_colour(background(&frames.frame(500, &animated)), "#800080");
    }

    #[test]
    fn a_modern_keyframe_colour_interpolates_in_oklab() {
        let mut frames = Frames::new();
        frames.define(
            "tint",
            r##"{ "from": { "backgroundColor": "oklch(0.62796 0.25768 29.234)" }, "to": { "backgroundColor": "blue" } }"##,
        );
        let animated = fading("tint 1s linear");
        frames.frame(0, &animated);
        assert_colour(background(&frames.frame(500, &animated)), "oklab(0.539985 0.096200 -0.092840)");
    }

    #[test]
    fn each_pair_of_neighbouring_keyframes_picks_its_own_space() {
        let mut frames = Frames::new();
        frames.define(
            "tint",
            r##"{
                "0%": { "backgroundColor": "red" },
                "50%": { "backgroundColor": "blue" },
                "100%": { "backgroundColor": "oklch(0.7 0.15 150)" }
            }"##,
        );
        let animated = fading("tint 1s linear");
        frames.frame(0, &animated);
        // Legacy to legacy, then legacy to modern.
        assert_colour(background(&frames.frame(250, &animated)), "#800080");
        assert_colour(background(&frames.frame(750, &animated)), "oklab(0.576005 -0.081182 -0.118265)");
    }

    #[test]
    fn keyframe_colours_resolve_per_element_and_again_when_the_context_changes() {
        use crate::color::Scheme;
        let channels = |style: &StyleDesc| {
            let color = crate::color::parse_color_rgba(style.background_color.as_deref().unwrap()).unwrap();
            [color.r, color.g, color.b]
        };
        let close = |actual: [f32; 3], expected: [f32; 3]| {
            for (actual, expected) in actual.into_iter().zip(expected) {
                assert!((actual - expected).abs() < 0.01, "{actual:?} vs {expected:?}");
            }
        };
        let mut frames = Frames::new();
        frames.define(
            "tint",
            r##"{ "from": { "backgroundColor": "currentColor" }, "to": { "backgroundColor": "light-dark(#ff0000, #0000ff)" } }"##,
        );
        let animated = style(
            r##"{ "color": "#00ff00", "colorScheme": "light dark", "animation": "tint 1s linear forwards" }"##,
        );
        // From the element's own `color` to the light side.
        close(channels(&frames.frame(0, &animated)), [0.0, 1.0, 0.0]);
        close(channels(&frames.frame(1000, &animated)), [1.0, 0.0, 0.0]);

        // The window turned dark: the keyframes resolve again, to the dark side.
        frames.parent_colors = ColorContext::root(Scheme::Dark);
        close(channels(&frames.frame(1500, &animated)), [0.0, 0.0, 1.0]);

        // A second element with another `color` does not share the first's colours.
        let mut other = Frames::new();
        other.define(
            "tint",
            r##"{ "from": { "backgroundColor": "currentColor" }, "to": { "backgroundColor": "light-dark(#ff0000, #0000ff)" } }"##,
        );
        let purple = style(r##"{ "color": "#800080", "animation": "tint 1s linear forwards" }"##);
        let start = channels(&other.frame(0, &purple));
        close(start, [128.0 / 255.0, 0.0, 128.0 / 255.0]);
    }

    #[test]
    fn overshooting_easing_stays_inside_the_range_a_property_accepts() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s cubic-bezier(0.5, 2, 0.5, 2)");
        let value = opacity(&frames.frame(250, &animated));
        assert!((0.0..=1.0).contains(&value), "{value}");
    }

    #[test]
    fn state_for_an_element_that_leaves_the_tree_is_dropped() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        frames.frame(0, &fading("fade 1s linear"));
        assert!(frames.animations.wants(1, None));
        frames.animations.retain(|_| false);
        assert!(!frames.animations.wants(1, None));
    }

    fn variation_axes(style: &StyleDesc) -> String {
        String::from(style.font_variation_settings.clone().expect("declared"))
    }

    #[test]
    fn font_variation_keyframes_blend_the_same_axes_and_flip_otherwise() {
        let mut frames = Frames::new();
        frames.define(
            "weigh",
            r#"{ "from": { "fontVariationSettings": "'wght' 100, 'wdth' 100" },
                 "to": { "fontVariationSettings": "'wdth' 50, 'wght' 900" } }"#,
        );
        let animated = fading("weigh 1s linear");
        assert_eq!(variation_axes(&frames.frame(0, &animated)), r#""wdth" 100, "wght" 100"#);
        assert_eq!(variation_axes(&frames.frame(500, &animated)), r#""wdth" 75, "wght" 500"#);

        // An animation flips with no `allow-discrete` to ask for.
        frames.define(
            "swap",
            r#"{ "from": { "fontVariationSettings": "'wght' 100" },
                 "to": { "fontVariationSettings": "normal" } }"#,
        );
        let swapped = fading("swap 1s linear");
        assert_eq!(variation_axes(&frames.frame(0, &swapped)), r#""wght" 100"#);
        assert_eq!(variation_axes(&frames.frame(499, &swapped)), r#""wght" 100"#);
        assert_eq!(variation_axes(&frames.frame(500, &swapped)), "normal");
    }

    #[test]
    fn a_missing_font_variation_endpoint_is_the_inherited_settings() {
        let mut frames = Frames::new();
        frames.parent_font.variations = gpui::FontVariations::new([(u32::from_be_bytes(*b"wght"), 300.0)]);
        frames.define("grow", r#"{ "to": { "fontVariationSettings": "'wght' 700" } }"#);
        let animated = fading("grow 1s linear");
        assert_eq!(variation_axes(&frames.frame(0, &animated)), r#""wght" 300"#);
        assert_eq!(variation_axes(&frames.frame(500, &animated)), r#""wght" 500"#);
    }

    // ── Web Animations controls ──────────────────────────────────────

    use super::super::web_animation::Status;

    fn listed(frames: &Frames, millis: u64) -> Vec<Info> {
        frames.animations.list(1, frames.at(millis))
    }

    fn only(frames: &Frames, millis: u64) -> Info {
        let mut listed = listed(frames, millis);
        assert_eq!(listed.len(), 1, "{listed:?}");
        listed.remove(0)
    }

    /// `command` between frames, at the time of the frame at `millis`.
    fn command(frames: &mut Frames, millis: u64, id: u64, command: Command) {
        let at = frames.at(millis);
        frames.animations.command(id, command, at).expect("the animation exists").unwrap();
    }

    fn api_events(frames: &mut Frames) -> Vec<(ApiEventKind, u64)> {
        frames.animations.take_api_events().into_iter().map(|event| (event.kind, event.run)).collect()
    }

    #[test]
    fn an_animation_has_one_id_for_its_whole_run_and_each_run_its_own() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s linear infinite, fade 2s linear infinite");
        frames.frame(0, &animated);
        let first = listed(&frames, 0);
        assert_eq!(first.len(), 2);
        assert_ne!(first[0].id, first[1].id);
        frames.frame(300, &animated);
        frames.frame(900, &animated);
        assert_eq!(listed(&frames, 900).iter().map(|info| info.id).collect::<Vec<_>>(), [first[0].id, first[1].id]);

        // Changing the duration keeps the run, and so its id.
        let changed = fading("fade 3s linear infinite, fade 2s linear infinite");
        frames.frame(1000, &changed);
        assert_eq!(listed(&frames, 1000)[0].id, first[0].id);

        // Hidden and shown again, it starts over as a new run.
        let mut tree = crate::retained_tree::RetainedTree::new();
        tree.create_element(1, "div".to_string());
        frames.animations.hide(&tree, 1, frames.at(1100));
        frames.animations.show(1);
        frames.frame(1200, &changed);
        let restarted = listed(&frames, 1200);
        assert!(restarted.iter().all(|info| !first.iter().any(|old| old.id == info.id)));
        assert_eq!(restarted[0].name, "fade");
        assert_eq!(restarted[0].kind, Kind::Animation);
    }

    #[test]
    fn pause_holds_the_time_and_the_frame_paints_it() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s linear");
        frames.frame(0, &animated);
        frames.frame(400, &animated);
        let id = only(&frames, 400).id;
        command(&mut frames, 400, id, Command::Pause);
        assert_eq!(only(&frames, 400).status, Status::Paused);
        near(only(&frames, 400).current_time, 400.0);
        near(opacity(&frames.frame(900, &animated)), 0.4);
        near(only(&frames, 900).current_time, 400.0);
        frames.animations.take_active();
        frames.frame(1500, &animated);
        assert!(!frames.animations.take_active(), "a held animation asks for no frames");
        assert!(api_events(&mut frames).is_empty());
    }

    #[test]
    fn play_is_pending_until_the_next_frame_and_then_continues() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s linear");
        frames.frame(0, &animated);
        frames.frame(400, &animated);
        let id = only(&frames, 400).id;
        command(&mut frames, 400, id, Command::Pause);
        frames.frame(900, &animated);
        frames.animations.take_active();

        command(&mut frames, 900, id, Command::Play);
        near(only(&frames, 900).current_time, 400.0);
        assert_eq!(only(&frames, 900).status, Status::Running);
        // A frame is owed even though nothing else asks for one.
        near(opacity(&frames.frame(1000, &animated)), 0.4);
        assert!(frames.animations.take_active());
        near(opacity(&frames.frame(1200, &animated)), 0.6);
    }

    #[test]
    fn seek_sets_the_time_at_once_and_the_next_frame_paints_it() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s linear");
        frames.frame(0, &animated);
        let id = only(&frames, 0).id;
        command(&mut frames, 0, id, Command::SetCurrentTime(750.0));
        near(only(&frames, 0).current_time, 750.0);
        near(opacity(&frames.frame(100, &animated)), 0.85);

        // Paused, it stays where it was put.
        command(&mut frames, 100, id, Command::Pause);
        command(&mut frames, 100, id, Command::SetCurrentTime(200.0));
        near(opacity(&frames.frame(600, &animated)), 0.2);
        assert_eq!(only(&frames, 600).status, Status::Paused);
    }

    #[test]
    fn a_negative_delay_starts_at_time_zero_with_the_active_time_ahead() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s linear -500ms");
        near(opacity(&frames.frame(0, &animated)), 0.5);
        near(only(&frames, 0).current_time, 0.0);
        near(opacity(&frames.frame(200, &animated)), 0.7);
        near(only(&frames, 200).current_time, 200.0);
        // Time 500 is the end: the delay is inside the effect.
        assert_eq!(only(&frames, 400).status, Status::Running);
        let id = only(&frames, 400).id;
        frames.frame(500, &animated);
        let finished = frames.animations.info(id, frames.at(500)).unwrap();
        assert_eq!(finished.status, Status::Finished);
        assert!(listed(&frames, 500).is_empty(), "finished without a fill, it is not relevant");
    }

    #[test]
    fn a_paused_animation_at_its_end_is_listed_only_through_a_fill() {
        for (animation, listed_at_end) in [("fade 1s linear", false), ("fade 1s linear forwards", true)] {
            let mut frames = Frames::new();
            frames.define("fade", FADE);
            let animated = fading(animation);
            frames.frame(0, &animated);
            let id = only(&frames, 0).id;
            command(&mut frames, 0, id, Command::Pause);
            command(&mut frames, 0, id, Command::SetCurrentTime(999.0));
            frames.frame(100, &animated);
            assert_eq!(only(&frames, 100).status, Status::Paused);
            command(&mut frames, 100, id, Command::SetCurrentTime(1000.0));
            frames.frame(200, &animated);
            let held = frames.animations.info(id, frames.at(200)).unwrap();
            assert_eq!(held.status, Status::Paused, "{animation}");
            near(held.current_time, 1000.0);
            near(held.end_time, 1000.0);
            assert_eq!(listed(&frames, 200).len(), usize::from(listed_at_end), "{animation}");
        }
    }

    #[test]
    fn an_animation_run_back_to_its_start_is_finished_and_listed_only_through_a_fill() {
        for (animation, listed_at_start) in [("fade 1s linear", false), ("fade 1s linear backwards", true)] {
            let mut frames = Frames::new();
            frames.define("fade", FADE);
            let animated = fading(animation);
            frames.frame(0, &animated);
            let id = only(&frames, 0).id;
            command(&mut frames, 0, id, Command::SetCurrentTime(500.0));
            command(&mut frames, 0, id, Command::SetPlaybackRate(-1.0));
            frames.frame(100, &animated);
            assert_eq!(only(&frames, 100).status, Status::Running, "{animation}");
            frames.frame(1000, &animated);
            let info = frames.animations.info(id, frames.at(1000)).unwrap();
            assert_eq!(info.status, Status::Finished, "{animation}");
            assert_eq!(listed(&frames, 1000).len(), usize::from(listed_at_start), "{animation}");
        }
    }

    #[test]
    fn a_positive_delay_is_part_of_the_time_too() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s linear 500ms both");
        near(opacity(&frames.frame(0, &animated)), 0.0);
        near(opacity(&frames.frame(750, &animated)), 0.25);
        near(only(&frames, 750).current_time, 750.0);
        let id = only(&frames, 750).id;
        command(&mut frames, 750, id, Command::SetCurrentTime(1000.0));
        near(opacity(&frames.frame(750, &animated)), 0.5);
    }

    #[test]
    fn a_rate_scales_time_and_keeps_it_continuous() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s linear");
        frames.frame(0, &animated);
        let id = only(&frames, 0).id;
        command(&mut frames, 0, id, Command::SetPlaybackRate(2.0));
        near(opacity(&frames.frame(250, &animated)), 0.5);
        assert_eq!(only(&frames, 250).playback_rate, 2.0);

        command(&mut frames, 250, id, Command::SetPlaybackRate(0.5));
        near(only(&frames, 250).current_time, 500.0);
        near(opacity(&frames.frame(750, &animated)), 0.75);
    }

    #[test]
    fn a_negative_rate_runs_the_animation_back_and_finishes_at_time_zero() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s linear both");
        frames.frame(0, &animated);
        frames.frame(600, &animated);
        frames.events();
        let id = only(&frames, 600).id;
        command(&mut frames, 600, id, Command::SetPlaybackRate(-1.0));
        near(opacity(&frames.frame(700, &animated)), 0.5);
        assert_eq!(only(&frames, 700).status, Status::Running);
        assert!(frames.animations.take_active());

        near(opacity(&frames.frame(1500, &animated)), 0.0);
        let finished = only(&frames, 1500);
        assert_eq!(finished.status, Status::Finished);
        near(finished.current_time, 0.0);
        assert_eq!(api_events(&mut frames), [(ApiEventKind::Finish, id)]);
        // Back at the start it ends, as it does at the other edge going forward.
        assert_eq!(frames.events(), [named(End, "fade", 0.0)]);
        frames.animations.take_active();
        frames.frame(2500, &animated);
        assert!(!frames.animations.take_active());
        assert!(api_events(&mut frames).is_empty());
    }

    #[test]
    fn finish_is_reported_once_for_any_rate_sign() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s linear forwards");
        frames.frame(0, &animated);
        let id = only(&frames, 0).id;
        frames.frame(900, &animated);
        assert!(api_events(&mut frames).is_empty());
        near(only(&frames, 900).current_time, 900.0);
        frames.frame(1300, &animated);
        assert_eq!(api_events(&mut frames), [(ApiEventKind::Finish, id)]);
        assert_eq!(only(&frames, 1300).status, Status::Finished);
        near(only(&frames, 1300).current_time, 1000.0);
        frames.frame(2000, &animated);
        assert!(api_events(&mut frames).is_empty());
        near(only(&frames, 5000).current_time, 1000.0);
    }

    #[test]
    fn play_on_a_finished_animation_rewinds_it() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s linear forwards");
        frames.frame(0, &animated);
        let id = only(&frames, 0).id;
        frames.frame(1200, &animated);
        api_events(&mut frames);

        command(&mut frames, 1200, id, Command::Play);
        near(only(&frames, 1200).current_time, 0.0);
        assert_eq!(only(&frames, 1200).status, Status::Running);
        near(opacity(&frames.frame(1300, &animated)), 0.0);
        near(opacity(&frames.frame(1800, &animated)), 0.5);
        frames.frame(2400, &animated);
        assert_eq!(api_events(&mut frames), [(ApiEventKind::Finish, id)], "it finishes again");
    }

    #[test]
    fn play_on_a_finished_animation_running_back_rewinds_to_the_end() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s linear both");
        frames.frame(0, &animated);
        let id = only(&frames, 0).id;
        command(&mut frames, 0, id, Command::SetPlaybackRate(-1.0));
        frames.frame(100, &animated);
        assert_eq!(only(&frames, 100).status, Status::Finished);

        command(&mut frames, 100, id, Command::Play);
        near(only(&frames, 100).current_time, 1000.0);
        frames.frame(200, &animated);
        near(opacity(&frames.frame(500, &animated)), 0.7);
    }

    #[test]
    fn the_api_overrides_animation_play_state_for_good() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let running = fading("fade 10s linear running");
        let paused = fading("fade 10s linear paused");
        frames.frame(0, &running);
        let id = only(&frames, 0).id;
        frames.frame(1000, &paused);
        assert_eq!(only(&frames, 1000).status, Status::Paused, "the style decides until the API is used");
        frames.frame(2000, &running);
        near(only(&frames, 2000).current_time, 1000.0);

        command(&mut frames, 2000, id, Command::Pause);
        near(only(&frames, 2000).current_time, 1000.0);
        frames.frame(3000, &running);
        assert_eq!(only(&frames, 3000).status, Status::Paused, "running in the style is ignored");
        near(opacity(&frames.frame(4000, &running)), 0.1);

        command(&mut frames, 4000, id, Command::Play);
        frames.frame(5000, &paused);
        frames.frame(6000, &paused);
        assert_eq!(only(&frames, 6000).status, Status::Running, "paused in the style is ignored");
        near(only(&frames, 6000).current_time, 2000.0);
    }

    #[test]
    fn play_starts_an_animation_the_style_paused() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let paused = fading("fade 1s linear paused");
        near(opacity(&frames.frame(0, &paused)), 0.0);
        let id = only(&frames, 0).id;
        command(&mut frames, 0, id, Command::Play);
        frames.frame(100, &paused);
        near(opacity(&frames.frame(600, &paused)), 0.5);
    }

    #[test]
    fn only_relevant_animations_are_listed_in_the_order_of_animation_name() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        frames.define("grow", r#"{ "to": { "width": 10 } }"#);
        frames.define("tint", r#"{ "to": { "color": "red" } }"#);
        frames.define("later", r#"{ "to": { "height": 10 } }"#);
        let animated = fading(
            "fade 1s linear, grow 1s linear forwards, tint 500ms linear paused, later 1s linear 5s",
        );
        frames.frame(0, &animated);
        let names = |frames: &Frames, millis| {
            listed(frames, millis).into_iter().map(|info| info.name).collect::<Vec<_>>()
        };
        assert_eq!(names(&frames, 0), ["fade", "grow", "tint", "later"]);

        // `fade` finished without a fill; `tint` is held; `later` waits in its delay.
        frames.frame(1500, &animated);
        assert_eq!(names(&frames, 1500), ["grow", "tint", "later"]);
        assert_eq!(listed(&frames, 1500)[0].status, Status::Finished);
        assert_eq!(listed(&frames, 1500)[1].status, Status::Paused);

        // Played to its end, `tint` has no fill and leaves.
        let tint = listed(&frames, 1500)[1].id;
        command(&mut frames, 1500, tint, Command::SetCurrentTime(400.0));
        command(&mut frames, 1500, tint, Command::Play);
        frames.frame(1600, &animated);
        frames.frame(2100, &animated);
        assert_eq!(names(&frames, 2100), ["grow", "later"]);
    }

    #[test]
    fn a_finished_animation_run_backwards_is_current_again() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s linear");
        frames.frame(0, &animated);
        let id = only(&frames, 0).id;
        frames.frame(2000, &animated);
        assert!(listed(&frames, 2000).is_empty());
        let at = frames.at(2000);
        frames.animations.command(id, Command::SetPlaybackRate(-1.0), at).unwrap().unwrap();
        assert_eq!(listed(&frames, 2000).len(), 1);
    }

    #[test]
    fn an_animation_that_leaves_the_style_is_cancelled_for_the_api() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        frames.define("grow", r#"{ "to": { "width": 10 } }"#);
        frames.frame(0, &fading("fade 1s linear, grow 1s linear"));
        let ids: Vec<u64> = listed(&frames, 0).iter().map(|info| info.id).collect();
        frames.frame(300, &fading("fade 1s linear"));
        assert_eq!(api_events(&mut frames), [(ApiEventKind::Cancel, ids[1])]);
        assert!(frames.animations.info(ids[1], frames.at(300)).is_none());
        assert!(frames.animations.command(ids[1], Command::Pause, frames.at(300)).is_none());

        // A finished one is cancelled too: the object it is must go idle.
        frames.frame(2000, &fading("fade 1s linear"));
        assert_eq!(api_events(&mut frames), [(ApiEventKind::Finish, ids[0])]);
        frames.frame(2100, &fading("none"));
        assert_eq!(api_events(&mut frames), [(ApiEventKind::Cancel, ids[0])]);
    }

    #[test]
    fn display_none_cancels_for_the_api_too() {
        let mut tree = crate::retained_tree::RetainedTree::new();
        tree.create_element(1, "div".to_string());
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s linear");
        frames.frame(0, &animated);
        let id = only(&frames, 0).id;
        frames.animations.hide(&tree, 1, frames.at(100));
        assert_eq!(api_events(&mut frames), [(ApiEventKind::Cancel, id)]);
    }

    #[test]
    fn a_command_on_an_unknown_id_finds_nothing_and_a_bad_value_is_refused() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s linear");
        frames.frame(0, &animated);
        let at = frames.at(0);
        assert!(frames.animations.command(u64::MAX, Command::Pause, at).is_none());
        assert!(frames.animations.info(u64::MAX, at).is_none());
        let id = only(&frames, 0).id;
        assert!(frames.animations.command(id, Command::SetCurrentTime(f64::NAN), at).unwrap().is_err());
        assert!(frames.animations.command(id, Command::SetPlaybackRate(f64::INFINITY), at).unwrap().is_err());
    }

    #[test]
    fn nothing_changes_when_the_api_is_not_used() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s linear 100ms 2 alternate forwards");
        frames.frame(0, &animated);
        frames.frame(100, &animated);
        near(opacity(&frames.frame(600, &animated)), 0.5);
        near(opacity(&frames.frame(1600, &animated)), 0.5);
        near(opacity(&frames.frame(5000, &animated)), 0.0);
        assert_eq!(
            frames.events(),
            [
                named(Start, "fade", 0.0),
                named(Iteration, "fade", 1.0),
                named(End, "fade", 2.0),
            ]
        );
    }

    // ── SVG properties ───────────────────────────────────────────────

    fn attrs(pairs: &[(&str, Value)]) -> HashMap<String, Value> {
        pairs.iter().map(|(key, value)| (key.to_string(), value.clone())).collect()
    }

    fn attribute(written: &[(&'static str, String)], name: &str) -> String {
        written
            .iter()
            .find(|(attribute, _)| *attribute == name)
            .unwrap_or_else(|| panic!("{name} is not written: {written:?}"))
            .1
            .clone()
    }

    const DRAW: &str = r#"{ "from": { "strokeDashoffset": 100 }, "to": { "strokeDashoffset": 0 } }"#;

    #[test]
    fn keyframes_drive_the_svg_properties_of_a_shape() {
        let mut frames = Frames::new();
        frames.define("draw", DRAW);
        let declared = fading("draw 1s linear forwards");
        let own = attrs(&[]);
        for (millis, expected) in [(0, "100"), (250, "75"), (500, "50"), (1000, "0")] {
            let written = frames.shape(millis, &declared, &own, &[]);
            assert_eq!(attribute(&written, "stroke-dashoffset"), expected, "at {millis}");
        }
        assert_eq!(
            frames.events(),
            [named(Start, "draw", 0.0), named(End, "draw", 1.0)]
        );
    }

    #[test]
    fn a_shape_animation_lists_its_shape_and_lists_for_the_api() {
        let mut frames = Frames::new();
        frames.define("draw", DRAW);
        let declared = fading("draw 1s linear");
        assert_eq!(frames.animations.svg_shapes(&frames.registry).count(), 0);
        frames.shape(0, &declared, &attrs(&[]), &[]);
        assert_eq!(only(&frames, 0).name, "draw");
        // It is listed while it moves, and not once it has ended.
        frames.shape(500, &declared, &attrs(&[]), &[]);
        assert_eq!(frames.animations.svg_shapes(&frames.registry).collect::<Vec<_>>(), [1]);
        frames.shape(2000, &declared, &attrs(&[]), &[]);
        assert_eq!(frames.animations.svg_shapes(&frames.registry).count(), 0);
        // New keyframes under the name bring it back, once.
        frames.define("draw", r#"{ "to": { "strokeDashoffset": 5 } }"#);
        assert_eq!(frames.animations.svg_shapes(&frames.registry).collect::<Vec<_>>(), [1]);
        frames.shape(2100, &declared, &attrs(&[]), &[]);
        assert_eq!(frames.animations.svg_shapes(&frames.registry).count(), 0);
    }

    #[test]
    fn a_missing_endpoint_is_the_attribute_the_shape_has() {
        let mut frames = Frames::new();
        frames.define("grow", r#"{ "to": { "r": 20 } }"#);
        let declared = fading("grow 1s linear");
        frames.shape(0, &declared, &attrs(&[("r", Value::from(10))]), &[]);
        let written = frames.shape(500, &declared, &attrs(&[("r", Value::from(10))]), &[]);
        assert_eq!(attribute(&written, "r"), "15");
        // The attribute moving moves the animation's start.
        let written = frames.shape(500, &declared, &attrs(&[("r", Value::from(0))]), &[]);
        assert_eq!(attribute(&written, "r"), "10");
    }

    #[test]
    fn an_inherited_property_starts_from_the_nearest_ancestor_that_sets_it() {
        let mut frames = Frames::new();
        frames.define("thick", r#"{ "to": { "strokeWidth": 10, "r": 10 } }"#);
        let declared = fading("thick 1s linear");
        let group = attrs(&[("strokeWidth", Value::from(2)), ("r", Value::from(99))]);
        let svg = attrs(&[("strokeWidth", Value::from(100))]);
        frames.shape(0, &declared, &attrs(&[]), &[&svg, &group]);
        let written = frames.shape(500, &declared, &attrs(&[]), &[&svg, &group]);
        assert_eq!(attribute(&written, "stroke-width"), "6");
        // `r` is not inherited: it starts from its initial value, 0, and not from the group's.
        assert_eq!(attribute(&written, "r"), "5");
        // With nothing above, CSS's initial value is the start.
        let written = frames.shape(500, &declared, &attrs(&[]), &[]);
        assert_eq!(attribute(&written, "stroke-width"), "5.5");
    }

    #[test]
    fn colours_blend_and_dashes_blend_item_by_item() {
        let mut frames = Frames::new();
        frames.define(
            "mix",
            r##"{ "from": { "fill": "#ff0000", "strokeDasharray": "4 2", "stroke": "none" },
                  "to": { "fill": "#0000ff", "strokeDasharray": [8, 4], "stroke": "#000000" } }"##,
        );
        let declared = fading("mix 1s linear");
        frames.shape(0, &declared, &attrs(&[]), &[]);
        let written = frames.shape(500, &declared, &attrs(&[]), &[]);
        assert_eq!(attribute(&written, "stroke-dasharray"), "6 3");
        assert!(attribute(&written, "fill").starts_with("rgba(127.5"), "{written:?}");
        // `none` against a colour is not blended: it swaps halfway.
        assert!(attribute(&written, "stroke").starts_with("rgba(0.000, 0.000, 0.000, 1"), "{written:?}");
        let written = frames.shape(250, &declared, &attrs(&[]), &[]);
        assert_eq!(attribute(&written, "stroke"), "none");
    }

    #[test]
    fn a_missing_endpoint_of_an_unset_property_is_its_initial_value() {
        let mut frames = Frames::new();
        frames.define(
            "to",
            r##"{ "to": { "strokeDashoffset": 10, "r": 20, "fillOpacity": 0, "strokeWidth": 5, "stopColor": "#ff0000",
                          "strokeMiterlimit": 10, "floodOpacity": 0, "cx": 20, "fontSize": 30 } }"##,
        );
        frames.define(
            "from",
            r##"{ "from": { "strokeDashoffset": 10, "r": 20, "fillOpacity": 0, "strokeWidth": 5, "stopColor": "#ff0000",
                            "strokeMiterlimit": 10, "floodOpacity": 0, "cx": 20, "fontSize": 30 } }"##,
        );
        let at = |frames: &mut Frames, name: &str, millis| {
            let declared = fading(&format!("{name} 1s linear"));
            frames.shape(millis, &declared, &attrs(&[]), &[])
        };
        at(&mut frames, "to", 0);
        let written = at(&mut frames, "to", 500);
        for (name, expected) in [
            ("stroke-dashoffset", "5"),
            ("r", "10"),
            ("fill-opacity", "0.5"),
            ("stroke-width", "3"),
            ("stroke-miterlimit", "7"),
            ("flood-opacity", "0.5"),
            ("cx", "10"),
            ("font-size", "23"),
        ] {
            assert_eq!(attribute(&written, name), expected, "to, {name}");
        }
        assert!(attribute(&written, "stop-color").starts_with("rgba(127.500, 0.000, 0.000"), "{written:?}");
        at(&mut frames, "from", 0);
        let written = at(&mut frames, "from", 500);
        for (name, expected) in [
            ("stroke-dashoffset", "5"),
            ("r", "10"),
            ("fill-opacity", "0.5"),
            ("stroke-width", "3"),
            ("stroke-miterlimit", "7"),
            ("flood-opacity", "0.5"),
            ("cx", "10"),
            ("font-size", "23"),
        ] {
            assert_eq!(attribute(&written, name), expected, "from, {name}");
        }
    }

    #[test]
    fn a_missing_endpoint_is_the_font_the_shape_inherits() {
        let mut frames = Frames::new();
        frames.define("grow", r#"{ "to": { "fontSize": 40 } }"#);
        let declared = fading("grow 1s linear");
        frames.shape(0, &declared, &attrs(&[]), &[]);
        let own = attrs(&[("fontSize", Value::from(20))]);
        let written = frames.shape(500, &declared, &own, &[]);
        assert_eq!(attribute(&written, "font-size"), "30");
        let group = attrs(&[("fontSize", Value::from(10))]);
        let written = frames.shape(500, &declared, &attrs(&[]), &[&group]);
        assert_eq!(attribute(&written, "font-size"), "25");
    }

    #[test]
    fn a_keyword_initial_value_swaps_halfway_against_a_value() {
        let mut frames = Frames::new();
        frames.define(
            "keywords",
            r#"{ "to": { "strokeDasharray": [10, 4], "d": "M0 0 L10 10", "width": 20, "rx": 5 } }"#,
        );
        let declared = fading("keywords 1s linear");
        frames.shape(0, &declared, &attrs(&[]), &[]);
        let before = frames.shape(499, &declared, &attrs(&[]), &[]);
        let after = frames.shape(501, &declared, &attrs(&[]), &[]);
        for (name, none, value) in [
            ("stroke-dasharray", "none", "10 4"),
            ("d", "none", "M 0 0 L 10 10"),
            ("width", "auto", "20"),
            ("rx", "auto", "5"),
        ] {
            assert_eq!(attribute(&before, name), none, "before, {name}");
            assert_eq!(attribute(&after, name), value, "after, {name}");
        }
    }

    #[test]
    fn a_stroke_nothing_sets_is_none_and_a_fill_is_black() {
        let mut frames = Frames::new();
        frames.define("paint", r##"{ "to": { "stroke": "#ff0000", "fill": "#ff0000" } }"##);
        let declared = fading("paint 1s linear");
        frames.shape(0, &declared, &attrs(&[]), &[]);
        let written = frames.shape(250, &declared, &attrs(&[]), &[]);
        assert_eq!(attribute(&written, "stroke"), "none");
        assert!(attribute(&written, "fill").starts_with("rgba(63.75"), "{written:?}");
    }

    #[test]
    fn a_path_morphs_between_keyframes_of_one_shape() {
        let mut frames = Frames::new();
        frames.define(
            "morph",
            r#"{ "from": { "d": "M0 0 L10 0" }, "to": { "d": "path('M0 10 L10 10')" } }"#,
        );
        frames.shape(0, &fading("morph 1s linear"), &attrs(&[]), &[]);
        let written = frames.shape(500, &fading("morph 1s linear"), &attrs(&[]), &[]);
        assert_eq!(attribute(&written, "d"), "M 0 5 L 10 5");
    }

    #[test]
    fn overshooting_easing_stays_inside_the_range_of_an_svg_property() {
        let mut frames = Frames::new();
        frames.define("fade", r#"{ "from": { "fillOpacity": 0 }, "to": { "fillOpacity": 1 } }"#);
        let declared = fading("fade 1s cubic-bezier(0.5, 2, 0.5, 2)");
        let written = frames.shape(250, &declared, &attrs(&[]), &[]);
        let value: f64 = attribute(&written, "fill-opacity").parse().unwrap();
        assert!((0.0..=1.0).contains(&value), "{value}");
    }

    #[test]
    fn a_name_in_both_tables_is_the_svg_one_on_a_shape_and_the_style_one_elsewhere() {
        let mut frames = Frames::new();
        frames.define("dim", r#"{ "from": { "opacity": 0 }, "to": { "opacity": 1 } }"#);
        let declared = fading("dim 1s linear");
        frames.shape(0, &declared, &attrs(&[]), &[]);
        let written = frames.shape(500, &declared, &attrs(&[]), &[]);
        assert_eq!(attribute(&written, "opacity"), "0.5");
        // The same keyframes on a layout element are the style.
        let mut element = Frames::new();
        element.define("dim", r#"{ "from": { "opacity": 0 }, "to": { "opacity": 1 } }"#);
        element.frame(0, &declared);
        near(opacity(&element.frame(500, &declared)), 0.5);
    }

    #[test]
    fn a_value_one_table_cannot_read_is_left_to_the_other() {
        let mut frames = Frames::new();
        frames.define("size", r#"{ "from": { "width": "10%" }, "to": { "width": "30%" } }"#);
        let declared = fading("size 1s linear");
        // A percentage is no SVG number: nothing animates on the shape.
        assert!(frames.shape(0, &declared, &attrs(&[]), &[]).is_empty());
        assert!(frames.shape(500, &declared, &attrs(&[]), &[]).is_empty());
        frames.frame(0, &declared);
        assert_eq!(width_of(&frames.frame(500, &declared)), DimensionValue::Percentage(0.2));
    }

    #[test]
    fn an_svg_property_on_a_layout_element_is_ignored() {
        let mut frames = Frames::new();
        frames.define("draw", DRAW);
        let declared = fading("draw 1s linear");
        let sample = frames.frame(500, &declared);
        assert_eq!(sample, declared);
        assert!(frames.animations.svg_shapes(&frames.registry).next().is_none());
    }

    #[test]
    fn the_svg_host_animates_styles_and_the_svg_properties_no_style_shares_a_name_with() {
        let mut frames = Frames::new();
        frames.define(
            "pulse",
            r#"{ "from": { "opacity": 0, "strokeWidth": 2, "r": 1 }, "to": { "opacity": 1, "strokeWidth": 4, "r": 3 } }"#,
        );
        let declared = fading("pulse 1s linear");
        let own = attrs(&[]);
        let mut host = |frames: &mut Frames, millis: u64| {
            let now = frames.at(millis);
            frames.animations.advance(1, &declared, &frames.registry, Scope::Element, now);
            frames.animations.update(
                1,
                &declared,
                &Subject::svg_host(&declared, &own),
                &frames.parent_colors,
                &frames.parent_font,
                None,
                frames.inherited,
                Scope::Element,
                now,
            )
        };
        host(&mut frames, 0);
        let values = host(&mut frames, 500).expect("the animation has an effect");
        let written = values.svg_attributes();
        assert_eq!(attribute(&written, "stroke-width"), "3");
        assert_eq!(attribute(&written, "r"), "2");
        assert!(written.iter().all(|(name, _)| *name != "opacity"), "opacity is the box's: {written:?}");
        let mut resolved = declared.clone();
        values.apply_to(&mut resolved, None);
        near(opacity(&resolved), 0.5);
    }

    #[test]
    fn a_shape_whose_animation_is_removed_is_cancelled() {
        let mut frames = Frames::new();
        frames.define("draw", DRAW);
        frames.shape(0, &fading("draw 1s linear"), &attrs(&[]), &[]);
        frames.shape(500, &fading("draw 1s linear"), &attrs(&[]), &[]);
        frames.events();
        assert!(frames.shape(600, &fading("none"), &attrs(&[]), &[]).is_empty());
        assert_eq!(frames.events(), [named(Cancel, "draw", 0.6)]);
    }

    #[test]
    fn a_shape_animation_with_a_fill_holds_its_end_value() {
        let mut frames = Frames::new();
        frames.define("draw", DRAW);
        let declared = fading("draw 1s linear 500ms forwards");
        let own = attrs(&[("strokeDashoffset", Value::from(7))]);
        // Before the delay there is no effect, so the attribute stays.
        assert!(frames.shape(0, &declared, &own, &[]).is_empty());
        let written = frames.shape(5000, &declared, &own, &[]);
        assert_eq!(attribute(&written, "stroke-dashoffset"), "0");
    }

    #[test]
    fn a_clock_only_advance_reports_what_a_frame_would_have_and_leaves_that_frame_nothing() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s linear 3");
        frames.frame(0, &animated);
        frames.events();
        frames.animations.take_active();
        frames.animations.enter_dormant(1);
        frames.animations.advance_clock(1, frames.at(1100));
        assert_eq!(frames.events(), [named(Iteration, "fade", 1.0)]);
        assert!(frames.animations.take_dormant_active());
        assert!(!frames.animations.take_active());
        near(opacity(&frames.frame(1100, &animated)), 0.1);
        assert!(frames.events().is_empty(), "the frame at the same instant has nothing left to report");
        assert!(frames.animations.take_dormant_active(), "a dormant element that is built still asks for ticks, not frames");
        assert!(!frames.animations.take_active());
        frames.animations.advance_clock(1, frames.at(3100));
        assert_eq!(frames.events(), [named(End, "fade", 3.0)]);
        assert!(!frames.animations.take_dormant_active(), "an animation that ended asks for nothing");
    }

    #[test]
    fn a_clock_that_nothing_changes_before_its_next_boundary_is_not_ticked_and_reports_at_the_frame_that_passes_it() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s linear 3");
        frames.frame(0, &animated);
        frames.events();
        frames.animations.enter_dormant(1);
        let mut reported = Vec::new();
        let mut ms = 0;
        while ms < 3200 {
            ms += 16;
            frames.animations.advance_clock(1, frames.at(ms));
            for event in frames.events() {
                reported.push((ms, event));
            }
        }
        assert_eq!(
            reported,
            [(1008, named(Iteration, "fade", 1.0)), (2000, named(Iteration, "fade", 2.0)), (3008, named(End, "fade", 3.0))]
        );

        let mut frames = Frames::new();
        frames.define("fade", FADE);
        frames.frame(0, &animated);
        frames.animations.enter_dormant(1);
        frames.animations.advance_clock(1, frames.at(16));
        let run = &frames.animations.states[&1].runs[0];
        assert!(run.quiet_until.is_some_and(|until| until > frames.at(900)), "nothing is due before the second");
        frames.animations.advance_clock(1, frames.at(32));
        assert!(frames.animations.take_dormant_active(), "a run that is not ticked still asks for ticks");
    }

    #[test]
    fn dormant_elements_are_looked_at_only_when_something_is_due_and_report_at_the_frames_a_tick_of_every_one_does() {
        let styles = [
            (1u64, fading("fade 700ms linear infinite")),
            (2, fading("fade 1300ms linear 2")),
            (3, fading("fade 2s linear 500ms infinite")),
            (4, fading("fade 1s linear 4 paused")),
        ];
        let build = |frames: &mut Frames, id: u64, millis: u64, declared: &StyleDesc| {
            let now = frames.at(millis);
            frames.animations.advance(id, declared, &frames.registry, Scope::Element, now);
            frames.animations.update(
                id,
                declared,
                &Subject::element(declared),
                &frames.parent_colors,
                &frames.parent_font,
                None,
                frames.inherited,
                Scope::Element,
                now,
            );
        };
        let events = |frames: &mut Frames| {
            frames.animations.take_events().into_iter().map(|event| (event.id, event.kind, event.elapsed)).collect::<Vec<_>>()
        };
        // Both are built at 0, left dormant, and rebuilt at 800 ms (2 gets a shorter duration, as a style that retunes it).
        let rebuilt = fading("fade 900ms linear 2");
        let mut every = Frames::new();
        let mut due = Frames::new();
        for frames in [&mut every, &mut due] {
            frames.define("fade", FADE);
            for (id, declared) in &styles {
                build(frames, *id, 0, declared);
                frames.animations.enter_dormant(*id);
            }
            events(frames);
        }
        let (mut every_reported, mut due_reported) = (Vec::new(), Vec::new());
        let mut ms = 0;
        while ms < 6000 {
            ms += 16;
            if ms == 800 {
                build(&mut every, 2, ms, &rebuilt);
                build(&mut due, 2, ms, &rebuilt);
            }
            for id in 1..=4 {
                every.animations.advance_clock(id, every.at(ms));
            }
            due.animations.advance_dormant(due.at(ms));
            every_reported.extend(events(&mut every).into_iter().map(|event| (ms, event)));
            due_reported.extend(events(&mut due).into_iter().map(|event| (ms, event)));
            assert_eq!(
                due.animations.take_dormant_active(),
                every.animations.take_dormant_active(),
                "asks for ticks as long as a run moves, at {ms} ms"
            );
        }
        assert!(every_reported.len() >= 10, "{every_reported:?}");
        assert_eq!(due_reported, every_reported, "the same events at the same frames");
        // 375 ticks of 4 elements look at them once each at the start and a few times at each boundary.
        assert!(due.animations.sleepers.visits < 60, "{} visits", due.animations.sleepers.visits);
    }

    #[test]
    fn a_dormant_element_that_leaves_is_not_looked_at_and_one_that_comes_back_is() {
        let mut frames = Frames::new();
        frames.define("fade", FADE);
        let animated = fading("fade 1s linear infinite");
        frames.frame(0, &animated);
        frames.events();
        frames.animations.enter_dormant(1);
        frames.animations.advance_dormant(frames.at(16));
        let visits = frames.animations.sleepers.visits;
        frames.animations.advance_dormant(frames.at(32));
        assert_eq!(frames.animations.sleepers.visits, visits, "nothing is due before the second");
        frames.animations.leave_dormant(1);
        frames.animations.advance_dormant(frames.at(1100));
        assert_eq!(frames.animations.sleepers.visits, visits, "an element that left is not ticked");
        assert!(frames.events().is_empty());
        frames.animations.enter_dormant(1);
        frames.animations.advance_dormant(frames.at(1116));
        assert_eq!(frames.animations.sleepers.visits, visits + 1);
    }

    #[test]
    fn an_animation_only_paints_when_every_property_its_keyframes_move_does() {
        let mut frames = Frames::new();
        frames.define("tint", r#"{ "from": { "color": "red", "backgroundColor": "red" }, "to": { "color": "blue", "backgroundColor": "blue" } }"#);
        frames.define("grow", r#"{ "from": { "color": "red", "width": 10 }, "to": { "color": "blue", "width": 20 } }"#);
        let tinted = fading("tint 1s linear");
        frames.frame(0, &tinted);
        assert!(frames.animations.paints_only(1, &frames.registry));
        let growing = fading("grow 1s linear");
        frames.frame(0, &growing);
        assert!(!frames.animations.paints_only(1, &frames.registry));
        frames.frame(0, &fading("tint 1s linear, missing 1s"));
        assert!(!frames.animations.paints_only(1, &frames.registry), "keyframes that are not defined may start later");
        frames.define("tint", FADE);
        assert!(!frames.animations.paints_only(1, &frames.registry), "a redefined name is read again by a frame");
    }
}
