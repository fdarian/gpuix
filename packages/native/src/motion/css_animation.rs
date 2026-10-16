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

use std::collections::{BTreeMap, HashMap, HashSet};
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
use super::property::AnimProp;
use super::track::{Goal, Track};
use super::value::{AnimValue, Colors, Interpolation};
use super::{needs_layout, resolve_keyframes, InterpolateSize};
use crate::color::ColorContext;
use crate::retained_tree::RetainedTree;
use crate::style::StyleDesc;

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

fn evaluate(animation: &Animation, local: f64) -> Evaluation {
    let duration = animation.duration;
    let active_duration = active_duration(animation);
    let end_time = (animation.delay + active_duration).max(0.0);
    let before_boundary = animation.delay.min(end_time).max(0.0);
    let phase = if local < before_boundary {
        Phase::Before
    } else if local >= end_time {
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
    /// Inherited or otherwise unresolvable here: the endpoint is left out and
    /// the nearest keyframe holds.
    Unknown,
}

fn underlying_of(prop: AnimProp, style: &StyleDesc, colors: &Colors) -> Underlying {
    match prop.read(style, colors).or_else(|| prop.initial(colors)) {
        Some(value) => Underlying::Value(value),
        None if prop.size_axis().is_some() => Underlying::Keyword,
        None => Underlying::Unknown,
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
        prop: AnimProp,
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
    fn sample(&self, progress: f64) -> (AnimProp, Option<AnimValue>) {
        match self {
            Self::Continuous { prop, track, literal, measured } => {
                let value = match (measured, progress) {
                    (true, progress) if progress <= 0.0 => literal[0].clone(),
                    (true, progress) if progress >= 1.0 => literal[literal.len() - 1].clone(),
                    _ => prop.clamp_value(track.value_at(progress)),
                };
                (*prop, Some(value))
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
                (*prop, value)
            }
        }
    }
}

/// Everything the plan was built from, so it is rebuilt only when one changes:
/// the style beneath the animation (for the endpoints a keyframe set leaves
/// out), the animation's own timing function, and what layout measured.
#[derive(Clone, Debug, PartialEq)]
struct PlanKey {
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
    fn endpoint(&mut self, prop: AnimProp, underlying: &Underlying) -> Endpoint {
        match underlying {
            Underlying::Value(value) => Endpoint::Value(Some(value.clone())),
            Underlying::Unknown => Endpoint::Absent,
            Underlying::Keyword if prop == AnimProp::Height => Endpoint::Value(Some(AnimValue::Auto)),
            Underlying::Keyword if !self.allow_keywords => Endpoint::Value(None),
            Underlying::Keyword => {
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
    fn stops(&mut self, prop: AnimProp, underlying: &Underlying) -> Option<Vec<Stop>> {
        let mut stops: Vec<Stop> = self
            .keyframes
            .frames
            .iter()
            .filter_map(|frame| {
                let value = frame.value(prop, self.colors)?;
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
            let value = match self.endpoint(prop, underlying) {
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

    fn plan_prop(&mut self, prop: AnimProp, underlying: &Underlying) -> Option<PropPlan> {
        let stops = self.stops(prop, underlying).or_else(|| {
            self.waiting = true;
            None
        })?;
        let keyword = |stop: &Stop| matches!(stop.value, None | Some(AnimValue::Auto));
        if prop.size_axis().is_some() && !self.allow_keywords && stops.iter().any(keyword) {
            return Some(PropPlan::Discrete { prop, stops });
        }

        let literal: Vec<AnimValue> = stops
            .iter()
            .map(|stop| stop.value.clone().expect("an unset size is discrete unless keywords are allowed"))
            .collect();
        let resolved = if needs_layout(&literal) {
            self.reads_layout = true;
            let Some(measurement) = self.measurement else {
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
        let name = prop.name();
        let built = Goal::keyframes(name, literal.iter().cloned().map(Some).collect()).and_then(|goal| {
            Track::keyframed(name, goal, literal.clone(), &resolved, Interpolation::Css, transition, self.now)
        });
        match built {
            Ok(track) => Some(PropPlan::Continuous { prop, track, measured: literal != resolved, literal }),
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
    properties: Vec<AnimProp>,
    params: Animation,
    /// The animation's local time, in seconds, at `anchor`. It advances with
    /// the clock while running and holds while paused.
    base: f64,
    anchor: Instant,
    running: bool,
    phase: Phase,
    iteration: u64,
    plan: Option<Plan>,
    frozen: BTreeMap<AnimProp, f64>,
}

impl Run {
    fn start(name: &str, keyframes: Arc<Keyframes>, params: &Animation, now: Instant) -> Self {
        Self {
            name: name.to_string(),
            properties: keyframes.properties(),
            keyframes,
            params: params.clone(),
            base: 0.0,
            anchor: now,
            running: params.play_state == PlayState::Running,
            phase: Phase::Before,
            iteration: 0,
            plan: None,
            frozen: BTreeMap::new(),
        }
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

    fn local(&self, now: Instant) -> f64 {
        if self.running {
            self.base + now.saturating_duration_since(self.anchor).as_secs_f64()
        } else {
            self.base
        }
    }

    /// When `local` is or was, for ordering events of different animations.
    fn instant_at(&self, local: f64, now: Instant) -> Instant {
        let behind = self.local(now) - local;
        if !self.running || behind <= 0.0 {
            return now;
        }
        Duration::try_from_secs_f64(behind)
            .ok()
            .and_then(|behind| now.checked_sub(behind))
            .unwrap_or(now)
    }

    /// Takes the other `animation-*` values from the style; play state changes
    /// freeze or resume the clock where it is.
    fn retune(&mut self, params: &Animation, now: Instant) {
        if self.params.play_state != params.play_state {
            if self.running {
                self.base = self.local(now);
                self.running = false;
            } else {
                self.anchor = now;
                self.running = true;
            }
        }
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
        let evaluation = evaluate(&self.params, local);
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

    /// Reports an animation that stops before it ended. It is brought up to
    /// `now` first: one that ran out since the last frame ended, and its
    /// `animationend` is what is owed, not a cancel.
    fn cancel(&mut self, id: u64, now: Instant, events: &mut Vec<Scheduled>) {
        self.advance(id, now, events);
        if self.phase == Phase::After {
            return;
        }
        let elapsed = (self.local(now) - self.params.delay).clamp(0.0, active_duration(&self.params));
        events.push(self.event(id, AnimationEventKind::Cancel, elapsed, now));
    }

    fn values(
        &mut self,
        progress: f64,
        underlying: &StyleDesc,
        parent_colors: &ColorContext,
        measurement: Option<&Measurement>,
        allow_keywords: bool,
        now: Instant,
    ) -> Vec<(AnimProp, Option<AnimValue>)> {
        let reads_layout = self.plan.as_ref().is_none_or(|plan| plan.reads_layout);
        let colors = Colors::of(parent_colors, Some(underlying));
        let key = PlanKey {
            timing: self.params.timing.clone(),
            colors,
            allow_keywords,
            underlying: self
                .properties
                .iter()
                .map(|prop| underlying_of(*prop, underlying, &colors))
                .collect(),
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
            let props = self
                .properties
                .iter()
                .zip(&key.underlying)
                .filter_map(|(prop, underlying)| planner.plan_prop(*prop, underlying))
                .collect();
            let (reads_layout, waiting) = (planner.reads_layout, planner.waiting);
            self.plan = Some(Plan { key, props, reads_layout, waiting });
        }
        let plan = self.plan.as_ref().expect("the plan was just built");
        plan.props.iter().map(|prop| prop.sample(progress)).collect()
    }

    /// Whether the window needs another frame for this animation: it is moving
    /// toward a phase change, or waiting on layout.
    fn wants_frame(&self) -> bool {
        (self.running && self.phase != Phase::After)
            || self.plan.as_ref().is_some_and(|plan| plan.waiting)
    }
}

// ── Per-element state ────────────────────────────────────────────────

type Config = [Option<String>; 9];

struct ElementState {
    config: Config,
    list: AnimationList,
    runs: Vec<Run>,
}

impl ElementState {
    fn new() -> Self {
        Self { config: Default::default(), list: AnimationList::default(), runs: Vec::new() }
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
            run.cancel(id, now, events);
        }
        self.runs = runs;
    }
}

/// The values of the animations running on an element, ready to lay over the
/// style beneath them.
pub(crate) struct AnimationStyle(Vec<(AnimProp, AnimValue)>);

impl AnimationStyle {
    /// Lays the animated values over `style`, except the properties a running
    /// transition holds: `style` already has the transition's value for them,
    /// and a transition beats an animation in the cascade.
    pub(crate) fn apply_to(self, style: &mut StyleDesc, transitions: Option<&TransitionStyle>) {
        for (prop, value) in self.0 {
            if transitions.is_some_and(|transitions| transitions.runs(prop)) {
                continue;
            }
            prop.write(style, value);
        }
    }
}

/// Whether `style` sets an animation name that is not `none`.
fn declares_animation(style: &StyleDesc) -> bool {
    [&style.animation, &style.animation_name]
        .into_iter()
        .any(|text| text.as_deref().is_some_and(|text| !text.trim().eq_ignore_ascii_case("none")))
}

/// The animation state of every element that declares one.
#[derive(Default)]
pub(crate) struct CssAnimations {
    states: HashMap<u64, ElementState>,
    events: Vec<Scheduled>,
    active: bool,
    /// The roots `display: none` has cancelled for, until they render again.
    hidden: HashSet<u64>,
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

    /// Feeds one frame. `declared` is the element's style with its state styles
    /// folded in, which holds the `animation*` values; `underlying` is the
    /// style the animations lay over, transitions and motion included, which
    /// supplies the endpoints a keyframe set leaves out. `inherited` is the
    /// `interpolateSize` of the parent.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn update(
        &mut self,
        id: u64,
        declared: &StyleDesc,
        underlying: &StyleDesc,
        parent_colors: &ColorContext,
        registry: &KeyframesRegistry,
        measurement: Option<&Measurement>,
        inherited: InterpolateSize,
        now: Instant,
    ) -> Option<AnimationStyle> {
        let state = self.states.entry(id).or_insert_with(ElementState::new);
        state.sync_config(id, declared);
        state.reconcile(id, registry, now, &mut self.events);

        let allow_keywords = InterpolateSize::declared(declared)
            .unwrap_or(inherited)
            .allows_keywords();
        let mut values: BTreeMap<AnimProp, Option<AnimValue>> = BTreeMap::new();
        for run in &mut state.runs {
            let evaluation = run.advance(id, now, &mut self.events);
            if let Some(progress) = evaluation.progress {
                // Later in the list is higher in the cascade.
                values.extend(run.values(
                    progress,
                    underlying,
                    parent_colors,
                    measurement,
                    allow_keywords,
                    now,
                ));
            }
            self.active |= run.wants_frame();
        }

        if state.runs.is_empty() && state.list.0.iter().all(|animation| animation.name.is_none()) {
            self.states.remove(&id);
        }
        let values: Vec<(AnimProp, AnimValue)> = values
            .into_iter()
            .filter_map(|(prop, value)| Some((prop, value?)))
            .collect();
        (!values.is_empty()).then_some(AnimationStyle(values))
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
            for run in &mut state.runs {
                run.cancel(hidden, now, &mut self.events);
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
        for name in ["groupHover", "groupActive", "groupFocusWithin"] {
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
            if let Some(values) = self.animations.update(
                1,
                declared,
                underlying,
                &self.parent_colors,
                &self.registry,
                measurement,
                self.inherited,
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
            let running = transitions.update(
                1,
                declared,
                &self.parent_colors,
                None,
                None,
                self.inherited,
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
                &underlying,
                &self.parent_colors,
                &self.registry,
                None,
                self.inherited,
                now,
            ) {
                values.apply_to(&mut resolved, running.as_ref());
            }
            resolved
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

    fn dimmed(opacity: f64, transition: &str) -> StyleDesc {
        style(&format!(
            r#"{{ "opacity": {opacity}, "transition": "{transition}", "animation": "dim 10s linear" }}"#
        ))
    }

    #[test]
    fn a_running_transition_beats_an_animation_on_the_same_property() {
        let mut frames = Frames::new();
        let mut transitions = CssTransitions::default();
        frames.define("dim", r#"{ "from": { "opacity": 0.2 }, "to": { "opacity": 0.2 } }"#);
        near(opacity(&frames.frame_with_transitions(&mut transitions, 0, &dimmed(1.0, "opacity 1s linear"))), 0.2);

        let target = dimmed(0.6, "opacity 1s linear");
        near(opacity(&frames.frame_with_transitions(&mut transitions, 100, &target)), 1.0);
        near(opacity(&frames.frame_with_transitions(&mut transitions, 600, &target)), 0.8);
        near(opacity(&frames.frame_with_transitions(&mut transitions, 1000, &target)), 0.64);

        near(opacity(&frames.frame_with_transitions(&mut transitions, 1100, &target)), 0.2);
        near(opacity(&frames.frame_with_transitions(&mut transitions, 1200, &target)), 0.2);
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
    }

    #[test]
    fn an_animation_alone_never_starts_a_transition() {
        let mut frames = Frames::new();
        let mut transitions = CssTransitions::default();
        frames.define("dim", r#"{ "from": { "opacity": 1 }, "to": { "opacity": 0 } }"#);
        let declared = dimmed(1.0, "opacity 1s linear");
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
        for name in ["groupHover", "groupActive", "groupFocusWithin"] {
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
            let values = frames.animations.update(
                id,
                declared,
                declared,
                &frames.parent_colors,
                &frames.registry,
                None,
                frames.inherited,
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
    fn an_endpoint_that_cannot_be_resolved_holds_the_nearest_keyframe() {
        let mut frames = Frames::new();
        frames.define("grow", r#"{ "to": { "fontSize": 20 } }"#);
        let animated = fading("grow 1s linear");
        assert_eq!(frames.frame(0, &animated).font_size, Some(20.0));
        assert_eq!(frames.frame(500, &animated).font_size, Some(20.0));
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

    fn measured(width: f32, height: f32, natural: f32) -> Measurement {
        Measurement {
            border_box: gpui::size(gpui::px(width), gpui::px(height)),
            natural_height: gpui::px(natural),
            parent_content: gpui::size(gpui::px(800.0), gpui::px(600.0)),
            scale_factor: 1.0,
            percent_height_resolves: true,
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
        assert_eq!(frames.frame_measured(250, &animated, Some(&measurement)).width, None);
        assert_eq!(
            frames.frame_measured(750, &animated, Some(&measurement)).width,
            Some(DimensionValue::Pixels(100.0))
        );
    }

    #[test]
    fn an_unset_size_endpoint_starts_from_the_laid_out_size_once_keywords_are_allowed() {
        let mut frames = Frames::new();
        frames.define("grow", r#"{ "to": { "width": 100 } }"#);
        let animated = style(
            r#"{ "animation": "grow 1s linear", "interpolateSize": "allow-keywords" }"#,
        );
        assert_eq!(frames.frame(0, &animated).width, None, "waits for a measurement");
        assert!(frames.animations.take_active());
        let first = measured(40.0, 10.0, 10.0);
        match frames.frame_measured(500, &animated, Some(&first)).width {
            Some(DimensionValue::Pixels(width)) => near(width, 70.0),
            other => panic!("{other:?}"),
        }
        // The box moved because of the animation; the start is not measured again.
        let moved = measured(70.0, 10.0, 10.0);
        match frames.frame_measured(500, &animated, Some(&moved)).width {
            Some(DimensionValue::Pixels(width)) => near(width, 70.0),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_percentage_meeting_a_length_runs_through_px_and_ends_as_the_percentage() {
        let mut frames = Frames::new();
        frames.define("grow", r#"{ "from": { "width": 0 }, "to": { "width": "50%" } }"#);
        let animated = fading("grow 1s linear forwards");
        let measurement = measured(40.0, 10.0, 10.0);
        frames.frame_measured(0, &animated, Some(&measurement));
        match frames.frame_measured(500, &animated, Some(&measurement)).width {
            Some(DimensionValue::Pixels(width)) => near(width, 200.0),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            frames.frame_measured(2000, &animated, Some(&measurement)).width,
            Some(DimensionValue::Percentage(0.5))
        );
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
}
