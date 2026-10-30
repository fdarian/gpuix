//! CSS transitions (CSS Transitions Level 1) for style props and SVG properties.
//!
//! Each frame the renderer hands an element's after-change style (state styles
//! already folded in) to [`CssTransitions::update`]. A change in the value of a
//! transitionable property starts a transition from the value the element
//! showed to the new one; the returned [`TransitionStyle`] holds the values of
//! the transitions still running, to lay over that style.
//!
//! The SVG properties (`fill`, `stroke-width`, `r`, `d`...) are the attributes of an `<svg>` and of
//! the shapes inside it. A shape is never built, so its `<svg>` steps its transitions beside its
//! animations ([`CssTransitions::animate_shape`]) and the values come back as attributes; the
//! `<svg>` itself steps its style properties and its attributes together.

use std::cell::Cell;
use std::collections::{BTreeMap, HashMap, HashSet};

use web_time::Instant;

use super::curve::{parse_timing_function, Ease};
use super::measure::Measurement;
use super::css_animation::{Beat, Direction, FillMode, IterationCount, PropTarget, SvgSource};
use super::effect::{DurationUpdate, EffectInfo, KeyframeInfo, Timing, TimingUpdate};
use super::property::{expand_shorthand, AnimProp, Scope};
use super::svg_attr::{attribute_text, SvgAttr};
use super::text::{pixels, Text};
use super::value::{pair_in, AnimValue, Colors, Interpolation, Pair, ParentValues};
use super::web_animation::{next_seq, ApiEvent, Command, Info, Kind, Playback, Status};
use super::{MotionState, PropKey};
use crate::color::ColorContext;
use crate::dormant_wake::Wake;
use crate::retained_tree::RetainedTree;
use crate::style::StyleDesc;

// ── Parsing ──────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
enum Matcher {
    All,
    /// The animatable properties a CSS name stands for. Empty for a name that
    /// is a valid property but not one gpuix animates (`perspective`), which
    /// matches nothing, as an unsupported property does in a browser. A name that
    /// is a style and an SVG property (`opacity`, `width`) stands for both: which
    /// one an element has is its [`PropTarget`]'s to say.
    Props(Vec<PropKey>),
}

#[derive(Clone, Debug, PartialEq)]
struct Entry {
    matcher: Matcher,
    /// Seconds, never negative.
    duration: f64,
    /// Seconds. A negative delay starts the transition part-way through.
    delay: f64,
    timing: Ease,
    /// `transition-behavior: allow-discrete`: the property may flip at half
    /// instead of being left out.
    allow_discrete: bool,
}

impl Entry {
    fn matches(&self, key: PropKey) -> bool {
        match &self.matcher {
            Matcher::All => true,
            Matcher::Props(keys) => keys.contains(&key),
        }
    }
}

/// The resolved `transition-*` lists of one element. Empty for `none`.
#[derive(Clone, Debug, Default, PartialEq)]
struct TransitionList(Vec<Entry>);

impl TransitionList {
    /// The last entry that names `prop`, as CSS picks the matching one.
    fn matching(&self, key: PropKey) -> Option<&Entry> {
        self.0.iter().rev().find(|entry| entry.matches(key))
    }

    fn parse(style: &StyleDesc) -> Result<Self, String> {
        if config_key(style).iter().all(Option::is_none) {
            return Ok(Self::default());
        }
        let mut lists = match style.transition.as_deref() {
            Some(text) => Lists::parse_shorthand(text).map_err(|error| format!("transition: {error}"))?,
            None => Lists::initial(),
        };
        if let Some(text) = style.transition_property.as_deref() {
            lists.properties =
                parse_properties(text).map_err(|error| format!("transitionProperty: {error}"))?;
        }
        if let Some(text) = style.transition_duration.as_deref() {
            lists.durations = parse_list(text, parse_duration)
                .map_err(|error| format!("transitionDuration: {error}"))?;
        }
        if let Some(text) = style.transition_timing_function.as_deref() {
            lists.timings = parse_list(text, parse_timing_function)
                .map_err(|error| format!("transitionTimingFunction: {error}"))?;
        }
        if let Some(text) = style.transition_delay.as_deref() {
            lists.delays = parse_list(text, parse_delay)
                .map_err(|error| format!("transitionDelay: {error}"))?;
        }
        if let Some(text) = style.transition_behavior.as_deref() {
            lists.behaviors = parse_list(text, parse_behavior)
                .map_err(|error| format!("transitionBehavior: {error}"))?;
        }
        Ok(lists.into_entries())
    }
}

#[derive(Clone, Debug, PartialEq)]
enum PropertyName {
    All,
    Ident(String),
}

#[derive(Clone, Debug, PartialEq)]
enum Properties {
    None,
    List(Vec<PropertyName>),
}

/// The five `transition-*` lists. They are as long as each was written; the
/// property list sets the count and the others repeat or truncate to it.
struct Lists {
    properties: Properties,
    durations: Vec<f64>,
    timings: Vec<Ease>,
    delays: Vec<f64>,
    behaviors: Vec<bool>,
}

impl Lists {
    fn initial() -> Self {
        Self {
            properties: Properties::List(vec![PropertyName::All]),
            durations: vec![0.0],
            timings: vec![default_timing()],
            delays: vec![0.0],
            behaviors: vec![false],
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
        let items = split_outside_parens(text, ',')?;
        let mut properties = Vec::new();
        let mut none = false;
        let mut lists = Self {
            properties: Properties::None,
            durations: Vec::new(),
            timings: Vec::new(),
            delays: Vec::new(),
            behaviors: Vec::new(),
        };
        for item in &items {
            let mut property = None;
            let mut duration = None;
            let mut delay = None;
            let mut timing = None;
            let mut behavior = None;
            for token in split_whitespace_outside_parens(item) {
                if let Some(time) = parse_time(token) {
                    if duration.is_none() {
                        if time < 0.0 {
                            return Err(format!("a duration cannot be negative: {token}"));
                        }
                        duration = Some(check_seconds(time, token)?);
                    } else if delay.is_none() {
                        delay = Some(check_seconds(time, token)?);
                    } else {
                        return Err(format!("too many times in `{item}`"));
                    }
                } else if is_timing_function(token) {
                    if timing.replace(parse_timing_function(token)?).is_some() {
                        return Err(format!("two timing functions in `{item}`"));
                    }
                } else if is_behavior(token) {
                    if behavior.replace(parse_behavior(token)?).is_some() {
                        return Err(format!("two transition behaviors in `{item}`"));
                    }
                } else if token.eq_ignore_ascii_case("none") {
                    none = true;
                    if property.replace(PropertyName::All).is_some() {
                        return Err(format!("two properties in `{item}`"));
                    }
                } else if property.replace(parse_property_name(token)?).is_some() {
                    return Err(format!("two properties in `{item}`"));
                }
            }
            properties.push(property.unwrap_or(PropertyName::All));
            lists.durations.push(duration.unwrap_or(0.0));
            lists.timings.push(timing.unwrap_or_else(default_timing));
            lists.delays.push(delay.unwrap_or(0.0));
            lists.behaviors.push(behavior.unwrap_or(false));
        }
        if none {
            if items.len() != 1 || properties != [PropertyName::All] {
                return Err("`none` cannot be combined with other values".to_string());
            }
            lists.properties = Properties::None;
            // `none` takes the other longhands' initial values.
            lists.durations = vec![0.0];
            lists.timings = vec![default_timing()];
            lists.delays = vec![0.0];
            lists.behaviors = vec![false];
        } else {
            lists.properties = Properties::List(properties);
        }
        Ok(lists)
    }

    fn into_entries(self) -> TransitionList {
        let Properties::List(names) = self.properties else {
            return TransitionList::default();
        };
        TransitionList(
            names
                .iter()
                .enumerate()
                .map(|(index, name)| Entry {
                    matcher: match name {
                        PropertyName::All => Matcher::All,
                        PropertyName::Ident(ident) => Matcher::Props(css_props(ident)),
                    },
                    duration: self.durations[index % self.durations.len()],
                    delay: self.delays[index % self.delays.len()],
                    timing: self.timings[index % self.timings.len()].clone(),
                    allow_discrete: self.behaviors[index % self.behaviors.len()],
                })
                .collect(),
        )
    }
}

fn default_timing() -> Ease {
    Ease::Name("ease".to_string())
}

/// Splits on `separator`, ignoring any inside parentheses. Empty items are an
/// error, as `a,,b` is in CSS.
pub(super) fn split_outside_parens(text: &str, separator: char) -> Result<Vec<&str>, String> {
    let mut items = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (index, character) in text.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if character == separator && depth == 0 => {
                items.push(text[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    items.push(text[start..].trim());
    if items.iter().any(|item| item.is_empty()) {
        return Err(format!("empty value in `{}`", text.trim()));
    }
    Ok(items)
}

pub(super) fn split_whitespace_outside_parens(text: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut depth = 0usize;
    let mut start = None;
    for (index, character) in text.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if character.is_whitespace() && depth == 0 {
            if let Some(begin) = start.take() {
                tokens.push(&text[begin..index]);
            }
        } else if start.is_none() {
            start = Some(index);
        }
    }
    if let Some(begin) = start {
        tokens.push(&text[begin..]);
    }
    tokens
}

pub(super) fn parse_list<T>(text: &str, parse: impl Fn(&str) -> Result<T, String>) -> Result<Vec<T>, String> {
    split_outside_parens(text, ',')?
        .into_iter()
        .map(|item| parse(item))
        .collect()
}

/// `<time>`: a number with `s` or `ms`. A bare `0` is not a time in CSS.
pub(super) fn parse_time(text: &str) -> Option<f64> {
    let lowered = text.to_ascii_lowercase();
    let (number, scale) = match lowered.strip_suffix("ms") {
        Some(number) => (number, 0.001),
        None => (lowered.strip_suffix('s')?, 1.0),
    };
    number
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite())
        .map(|number| number * scale)
}

pub(super) fn check_seconds(seconds: f64, text: &str) -> Result<f64, String> {
    if seconds.abs() > 1e9 {
        return Err(format!("{text} is out of range"));
    }
    Ok(seconds)
}

pub(super) fn parse_duration(text: &str) -> Result<f64, String> {
    let seconds = parse_time(text).ok_or_else(|| format!("not a time: {text}"))?;
    if seconds < 0.0 {
        return Err(format!("a duration cannot be negative: {text}"));
    }
    check_seconds(seconds, text)
}

pub(super) fn parse_delay(text: &str) -> Result<f64, String> {
    let seconds = parse_time(text).ok_or_else(|| format!("not a time: {text}"))?;
    check_seconds(seconds, text)
}

fn is_behavior(token: &str) -> bool {
    matches!(token.to_ascii_lowercase().as_str(), "normal" | "allow-discrete")
}

/// `transition-behavior`: whether the item allows discrete properties.
fn parse_behavior(text: &str) -> Result<bool, String> {
    match text.to_ascii_lowercase().as_str() {
        "normal" => Ok(false),
        "allow-discrete" => Ok(true),
        _ => Err(format!("not `normal` or `allow-discrete`: {text}")),
    }
}

pub(super) fn is_timing_function(token: &str) -> bool {
    token.ends_with(')')
        || matches!(
            token.to_ascii_lowercase().as_str(),
            "linear" | "ease" | "ease-in" | "ease-out" | "ease-in-out" | "step-start" | "step-end"
        )
}

fn parse_properties(text: &str) -> Result<Properties, String> {
    if text.trim().eq_ignore_ascii_case("none") {
        return Ok(Properties::None);
    }
    let names = parse_list(text, |item| {
        if item.eq_ignore_ascii_case("none") {
            return Err("`none` cannot be combined with other values".to_string());
        }
        parse_property_name(item)
    })?;
    Ok(Properties::List(names))
}

/// A CSS property name, or `all`. Any identifier is valid, as in a browser, but
/// a camelCase one is certainly the JS spelling of a kebab-case CSS name, so it
/// is an error instead of a transition that silently never runs.
fn parse_property_name(text: &str) -> Result<PropertyName, String> {
    if text.chars().any(|character| character.is_ascii_uppercase()) {
        return Err(format!(
            "`{text}` is not a CSS property name, write it kebab-case (background-color)"
        ));
    }
    let mut characters = text.chars();
    let valid = characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase() || first == '_' || first == '-')
        && !text.starts_with("-0")
        && !text.starts_with("-1")
        && text
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if !valid {
        return Err(format!("`{text}` is not a CSS property name"));
    }
    if text == "all" {
        return Ok(PropertyName::All);
    }
    if matches!(text, "initial" | "inherit" | "unset" | "revert" | "revert-layer" | "default") {
        return Err(format!("`{text}` cannot name a property"));
    }
    Ok(PropertyName::Ident(text.to_string()))
}

fn camel_case(kebab: &str) -> String {
    let mut camel = String::with_capacity(kebab.len());
    let mut upper = false;
    for character in kebab.chars() {
        if character == '-' {
            upper = true;
        } else if upper {
            camel.push(character.to_ascii_uppercase());
            upper = false;
        } else {
            camel.push(character);
        }
    }
    camel
}

fn kebab_case(camel: &str) -> String {
    let mut kebab = String::with_capacity(camel.len() + 2);
    for character in camel.chars() {
        if character.is_ascii_uppercase() {
            kebab.push('-');
        }
        kebab.push(character.to_ascii_lowercase());
    }
    kebab
}

/// The animatable properties a CSS property name covers, shorthands included.
fn css_props(ident: &str) -> Vec<PropKey> {
    let mut keys: Vec<PropKey> = style_props(ident).into_iter().map(PropKey::Style).collect();
    if let Some(attr) = SvgAttr::from_name(&camel_case(ident)).filter(|attr| attr.is_css_property()) {
        keys.push(PropKey::Svg(attr));
    }
    keys
}

fn style_props(ident: &str) -> Vec<AnimProp> {
    use AnimProp::*;
    match ident {
        "inset" => vec![Top, Right, Bottom, Left],
        "border" => vec![
            BorderTopWidth,
            BorderRightWidth,
            BorderBottomWidth,
            BorderLeftWidth,
            BorderTopColor,
            BorderRightColor,
            BorderBottomColor,
            BorderLeftColor,
        ],
        "outline" => vec![OutlineWidth, OutlineColor],
        "textDecoration" | "text-decoration" => vec![TextDecorationColor],
        "flex" => vec![FlexGrow, FlexShrink, FlexBasis],
        "overflow" => vec![OverflowX, OverflowY],
        "background" => vec![BackgroundColor],
        _ => {
            let camel = camel_case(ident);
            expand_shorthand(&camel)
                .map(<[AnimProp]>::to_vec)
                .or_else(|| AnimProp::from_css_name(&camel).map(|prop| vec![prop]))
                .unwrap_or_default()
        }
    }
}

/// CSS `interpolate-size`: whether a transition may run between a length and a
/// sizing keyword (`auto`, or the `auto`/`none` an unset size computes to).
/// Inherited, as in CSS.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum InterpolateSize {
    #[default]
    NumericOnly,
    AllowKeywords,
}

impl InterpolateSize {
    fn parse(text: &str) -> Result<Self, String> {
        match text.trim().to_ascii_lowercase().as_str() {
            "numeric-only" => Ok(Self::NumericOnly),
            "allow-keywords" => Ok(Self::AllowKeywords),
            _ => Err(format!(
                "interpolateSize must be \"numeric-only\" or \"allow-keywords\", got {text:?}"
            )),
        }
    }

    /// What `style` declares. A value that does not parse was rejected when the
    /// style was set, so it reads as undeclared here, as an invalid CSS
    /// declaration does.
    pub(crate) fn declared(style: &StyleDesc) -> Option<Self> {
        Self::parse(style.interpolate_size.as_deref()?).ok()
    }

    pub(super) fn allows_keywords(self) -> bool {
        self == Self::AllowKeywords
    }
}

/// Whether a transition on `prop` is between a length and a keyword, which
/// browsers swap at once unless `interpolate-size` allows it.
fn is_keyword_change(
    key: PropKey,
    before: Option<&AnimValue>,
    after: Option<&AnimValue>,
) -> bool {
    let keyword = |value: Option<&AnimValue>| matches!(value, None | Some(AnimValue::Auto));
    matches!(key, PropKey::Style(prop) if prop.size_axis().is_some()) && (keyword(before) || keyword(after))
}

/// Whether a transition of `style` can run on `width`, which to or from `auto`
/// needs the natural width measured a frame before the style changes.
pub(crate) fn transitions_width(style: &StyleDesc) -> bool {
    TransitionList::parse(style).is_ok_and(|list| list.matching(PropKey::Style(AnimProp::Width)).is_some())
}

/// Rejects a style whose `transition*` or `interpolateSize` values, in it or in
/// a state style, do not parse, so a bad value fails where it is set instead of
/// silently never running.
pub(crate) fn validate_transitions(style: &StyleDesc) -> Result<(), String> {
    TransitionList::parse(style)?;
    if let Some(text) = style.interpolate_size.as_deref() {
        InterpolateSize::parse(text)?;
    }
    for (name, state) in super::state_styles(style) {
        if let Some(state) = state {
            TransitionList::parse(state).map_err(|error| format!("{name}.{error}"))?;
            if let Some(text) = state.interpolate_size.as_deref() {
                InterpolateSize::parse(text).map_err(|error| format!("{name}.{error}"))?;
            }
        }
    }
    Ok(())
}

/// Whether `style`, or a state style in it, declares a transition.
pub(crate) fn declares_transitions(style: &StyleDesc) -> bool {
    declares_transition(style)
        || super::state_styles(style)
            .into_iter()
            .any(|(_, state)| state.is_some_and(declares_transition))
}

/// Whether `style` sets a `transition*` field that is more than `none`.
fn declares_transition(style: &StyleDesc) -> bool {
    let only_none = |text: &Option<String>| {
        text.as_deref()
            .is_none_or(|text| text.trim().eq_ignore_ascii_case("none"))
    };
    let set = style.transition.is_some()
        || style.transition_property.is_some()
        || style.transition_duration.is_some()
        || style.transition_timing_function.is_some()
        || style.transition_delay.is_some()
        || style.transition_behavior.is_some();
    set && !(only_none(&style.transition)
        && only_none(&style.transition_property)
        && style.transition_duration.is_none()
        && style.transition_timing_function.is_none()
        && style.transition_delay.is_none()
        && style.transition_behavior.is_none())
}

// ── Running transitions ──────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransitionEventKind {
    Run,
    Start,
    End,
    Cancel,
}

impl TransitionEventKind {
    /// The event type the host maps to `onTransition*`.
    pub(crate) fn event_type(self) -> &'static str {
        match self {
            Self::Run => "transitionrun",
            Self::Start => "transitionstart",
            Self::End => "transitionend",
            Self::Cancel => "transitioncancel",
        }
    }
}

/// A DOM `TransitionEvent`, minus `pseudoElement`, which is always `""`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TransitionEvent {
    pub(crate) id: u64,
    pub(crate) kind: TransitionEventKind,
    key: PropKey,
    /// Seconds.
    pub(crate) elapsed: f64,
    /// See [`next_seq`].
    pub(crate) seq: u64,
    /// Told after the events of the Web Animations objects made in the same pass: Chrome tells
    /// of a transition whose play just ran, or that a frame made, at the frame after.
    pub(crate) late: bool,
}

impl TransitionEvent {
    fn new(id: u64, kind: TransitionEventKind, key: PropKey, elapsed: f64) -> Self {
        Self { id, kind, key, elapsed, seq: next_seq(), late: false }
    }

    fn late(mut self, late: bool) -> Self {
        self.late = late;
        self
    }

    /// The CSS name, `background-color`.
    pub(crate) fn property_name(&self) -> String {
        kebab_case(self.key.name())
    }
}

/// The events a step of transitions queues: what the DOM hears, and what the
/// Web Animations objects hear. The two differ: a run back at its start
/// finishes before its active interval began without the DOM seeing a
/// `transitionend`.
#[derive(Default)]
struct Events {
    dom: Vec<TransitionEvent>,
    api: Vec<ApiEvent>,
    /// The step being taken is a frame that is drawn: a transition it makes is told of at the
    /// next frame.
    drawing: bool,
}

impl Events {
    fn push(&mut self, event: TransitionEvent) {
        self.dom.push(event);
    }

    /// What the clock of `running` noted since the last call, for the Web Animations objects.
    fn tell(&mut self, element: u64, key: PropKey, running: &mut Running, now: Instant) {
        for note in running.clock.take_notes() {
            self.api.push(ApiEvent::of(element, note, running.info(key, now)));
        }
    }
}

/// The timing of a transition beyond its delay, duration and easing, which only
/// `updateTiming()` changes.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Shape {
    iterations: IterationCount,
    direction: Direction,
    fill: FillMode,
    end_delay: f64,
    iteration_start: f64,
    /// The duration was set to `auto`, which plays as 0.
    auto_duration: bool,
}

impl Shape {
    /// What a transition has: one iteration, forwards, and a fill before its active phase.
    const TRANSITION: Self = Self {
        iterations: IterationCount::Finite(1.0),
        direction: Direction::Normal,
        fill: FillMode::Backwards,
        end_delay: 0.0,
        iteration_start: 0.0,
        auto_duration: false,
    };
}

#[derive(Clone, Debug)]
struct Running {
    /// Time since the transition started, with the delay inside it.
    clock: Playback,
    /// Which style change started it: transitions of one change are ordered by
    /// property name, those of different changes by when they began.
    generation: u64,
    delay: f64,
    duration: f64,
    /// The easing of the effect.
    timing: Ease,
    shape: Shape,
    /// The keyframes of the effect: where it runs from and to.
    from: AnimValue,
    to: AnimValue,
    segment: Pair,
    end: AnimValue,
    /// The computed value of `end`: a style that still has it leaves the
    /// transition alone.
    end_computed: AnimValue,
    /// The segment runs to a number measured from layout standing in for
    /// `end` (`auto`), and follows it.
    measured: bool,
    /// CSS "reversing-adjusted start value", as a computed value: where an
    /// interrupting change back to it counts as a reversal.
    reversing_start: AnimValue,
    shortening: f64,
    began: bool,
    /// The `elapsedTime` of the `transitionstart` owed when the run, which the DOM was told had
    /// ended, is in its active interval again.
    restart: Option<f64>,
    /// `transitionrun` is owed at the next frame: the run was cancelled and played again.
    announce: bool,
    /// The border box the transition started in, which Chrome resolves a `transform-origin` against
    /// when it writes the endpoints.
    border_box: Option<(f64, f64)>,
    /// An `Animation` object was handed to JS for the transition, so the style letting go of it
    /// keeps it, see [`ElementState::disowned`].
    handed: Cell<bool>,
    /// The style let go of it. A transition the API cancelled and the style still owns tells the DOM
    /// nothing of a run that finishes without playing; this one tells all of it.
    disowned: bool,
}

impl Running {
    /// `None` when the two values cannot be interpolated, which in CSS means
    /// the change is simply not transitioned.
    #[allow(clippy::too_many_arguments)]
    fn new(
        key: PropKey,
        from: &AnimValue,
        to: &AnimValue,
        (end, end_computed): (AnimValue, AnimValue),
        entry: &Entry,
        reversing_start: AnimValue,
        shortening: f64,
        now: Instant,
        measurement: Option<&Measurement>,
    ) -> Option<Self> {
        let flips = matches!(key, PropKey::Style(prop) if prop.flips_without_allow_discrete());
        let segment = pair_in(key.name(), from, to, Interpolation::Css)
            .ok()
            .filter(|pair| entry.allow_discrete || !pair.is_discrete() || flips)?;
        let delay = if entry.delay >= 0.0 {
            entry.delay
        } else {
            entry.delay * shortening
        };
        let duration = entry.duration * shortening;
        Some(Self {
            measured: end != *to,
            clock: Playback::start(now, false, (delay + duration).max(0.0)),
            generation: 0,
            delay,
            duration,
            timing: entry.timing.clone(),
            shape: Shape::TRANSITION,
            from: from.clone(),
            to: to.clone(),
            segment,
            end,
            end_computed,
            reversing_start,
            shortening,
            began: false,
            restart: None,
            announce: false,
            border_box: measurement.map(|measurement| {
                (f64::from(measurement.border_box.width.as_f32()), f64::from(measurement.border_box.height.as_f32()))
            }),
            handed: Cell::new(false),
            disowned: false,
        })
    }

    /// The timing of the effect.
    fn effect_timing(&self) -> Timing {
        Timing {
            delay: self.delay,
            duration: self.duration,
            iterations: self.shape.iterations,
            direction: self.shape.direction,
            fill: self.shape.fill,
            end_delay: self.shape.end_delay,
            iteration_start: self.shape.iteration_start,
        }
    }

    /// Where the transition ends, in time since it started.
    fn end_time(&self) -> f64 {
        self.effect_timing().end_time()
    }

    /// `getTiming()` and `getComputedTiming()`.
    fn effect_info(&self, now: Instant) -> Result<EffectInfo, String> {
        EffectInfo::of(
            &self.effect_timing(),
            self.shape.auto_duration,
            Some(&self.timing),
            self.clock.current_time(now, self.end_time()),
            self.clock.rate() < 0.0,
        )
    }

    /// What Chrome writes of an endpoint: a transition holds a `transform-origin` as the lengths of
    /// the box it started in.
    fn endpoint_text(&self, key: PropKey, value: &AnimValue) -> Result<Vec<(String, String)>, String> {
        let origin = matches!(key, PropKey::Style(AnimProp::TransformOrigin));
        match (origin, self.border_box, value.origin_parts()) {
            (true, Some((width, height)), Some((x, y, z))) => {
                let (x, y) = (pixels(x.0 + x.1 * width), pixels(y.0 + y.1 * height));
                Ok(vec![(key.name().to_string(), if z == 0.0 { format!("{x} {y}") } else { format!("{x} {y} {}", pixels(z)) })])
            }
            _ => key.computed_text(value, Text::Transition),
        }
    }

    /// `getKeyframes()`: the computed start and end values, eased by the effect and not by them.
    fn keyframe_info(&self, key: PropKey) -> Result<Vec<KeyframeInfo>, String> {
        [(0.0, &self.from), (1.0, &self.to)]
            .into_iter()
            .map(|(offset, value)| {
                Ok(KeyframeInfo {
                    offset,
                    written: true,
                    easing: "linear".to_string(),
                    composite: "auto",
                    values: self.endpoint_text(key, value)?,
                })
            })
            .collect()
    }

    /// `updateTiming()`: this transition only; a later change of the `transition-*` properties
    /// never reaches a transition that is running.
    fn update_timing(&mut self, update: &TimingUpdate, now: Instant) {
        let timing = update.over(self.effect_timing());
        self.delay = timing.delay;
        self.duration = timing.duration;
        self.shape = Shape {
            iterations: timing.iterations,
            direction: timing.direction,
            fill: timing.fill,
            end_delay: timing.end_delay,
            iteration_start: timing.iteration_start,
            auto_duration: match update.duration {
                Some(DurationUpdate::Auto) => true,
                Some(DurationUpdate::Seconds(_)) => false,
                None => self.shape.auto_duration,
            },
        };
        if let Some(easing) = &update.easing {
            self.timing = easing.clone();
        }
        self.clock.retime(now, self.end_time());
    }

    /// When the transition next has an event to report: the end of its delay, and its end.
    fn wake(&self, now: Instant) -> Wake {
        let started = if self.began { Wake::Never } else { self.clock.wake_at(self.delay, now) };
        started.merge(self.clock.wake_at(self.end_time(), now))
    }

    fn elapsed(&self, now: Instant) -> f64 {
        self.clock.current(now, self.end_time())
    }

    /// The eased progress. Before the delay ends it holds the start (the
    /// implicit `backwards` fill), after the end it holds the end.
    fn progress(&self, elapsed: f64) -> f64 {
        let evaluation = self.effect_timing().evaluate(elapsed, self.clock.rate() < 0.0).eased(Some(&self.timing));
        // Out of the effect the style shows, which is where the transition ends.
        evaluation.progress.unwrap_or(1.0)
    }

    fn value(&self, now: Instant) -> AnimValue {
        self.segment.at(self.progress(self.elapsed(now)))
    }

    /// Moves the end of a transition running to a measured number onto the
    /// measurement as it changes, without moving the value showing now: the
    /// start is solved so the same eased progress still lands on the old value.
    /// The schedule and the events are untouched.
    fn follow(&mut self, key: PropKey, measurement: &Measurement, now: Instant) {
        // Too near the end the start would be solved from almost nothing, and
        // the transition is about to hand over to the literal style anyway.
        const NEAR_END: f64 = 0.999;
        let PropKey::Style(prop) = key else {
            return;
        };
        if !self.measured {
            return;
        }
        let end = match &self.end {
            AnimValue::Auto => measurement.natural_px(prop),
            _ => None,
        };
        let progress = self.progress(self.elapsed(now));
        let Some(end) = end.filter(|end| self.segment.to != [*end] && progress < NEAR_END) else {
            return;
        };
        let from = self.segment.from[0];
        let current = from + (self.segment.to[0] - from) * progress;
        self.segment.from[0] = (current - end * progress) / (1.0 - progress);
        self.segment.to[0] = end;
    }

    fn info(&self, key: PropKey, now: Instant) -> Info {
        Info::of(Kind::Transition, kebab_case(key.name()), &self.clock, now, self.end_time())
    }

    /// [`Self::info`] for what the API reads, which is the `Animation` object JS holds from here on.
    fn handed_info(&self, key: PropKey, now: Instant) -> Info {
        self.handed.set(true);
        self.info(key, now)
    }

    /// Whether `getAnimations()` lists it: current (active, before its active
    /// phase going forward, after it going back) or in effect through the fill
    /// a transition has before its active phase. Chrome decides by the phase
    /// and the rate alone, so a transition paused at its end is not listed.
    fn is_relevant(&self, now: Instant) -> bool {
        let end = self.end_time();
        let time = self.elapsed(now);
        let backwards = self.clock.rate() < 0.0;
        let after = time > end || (!backwards && time == end);
        !after || backwards
    }

    /// `elapsedTime` of `transitionrun` and `transitionstart`.
    fn initial_elapsed(&self) -> f64 {
        self.effect_timing().initial_elapsed()
    }
}

/// What a style declares for `prop`: `Auto` for a width or height that is unset
/// or `auto`, which a transition runs to or from the laid-out size.
fn declared_value(prop: AnimProp, style: &StyleDesc, colors: &Colors) -> Option<AnimValue> {
    let value = super::resolve_start(
        super::PropKey::Style(prop),
        super::Base::Style(Some(style), colors),
    );
    match (value, prop) {
        (None, AnimProp::Width | AnimProp::Height) => Some(AnimValue::Auto),
        (value, _) => value,
    }
}

/// The CSS computed value of `prop`, which is what decides whether a style
/// changed it. Unlike [`declared_value`] a colour keeps `currentColor`, so it is
/// the same after a new `color`, and resolves `light-dark()` in the scheme the
/// element is in, so it differs after a flip.
fn computed_value(prop: AnimProp, style: &StyleDesc, colors: &Colors) -> Option<AnimValue> {
    declared_value(prop, style, &colors.with_current_color_pinned())
}

/// The number a transition leaves from. A value the style leaves unset or
/// `auto` is the size layout gave the element; an SVG property has no layout.
fn laid_out_start(
    key: PropKey,
    declared: Option<&AnimValue>,
    measurement: Option<&Measurement>,
) -> Option<AnimValue> {
    let PropKey::Style(prop) = key else {
        return declared.cloned();
    };
    match declared {
        Some(AnimValue::Auto) | None => measurement?.laid_out(prop).map(AnimValue::Number),
        Some(value) => Some(value.clone()),
    }
}

/// The number a transition runs to: `auto` is the size the content wants.
/// `None` when that is not measured yet.
fn laid_out_end(
    key: PropKey,
    declared: &AnimValue,
    measurement: Option<&Measurement>,
) -> Option<AnimValue> {
    let PropKey::Style(prop) = key else {
        return Some(declared.clone());
    };
    match declared {
        AnimValue::Auto => measurement?.natural_px(prop).map(AnimValue::Number),
        value => Some(value.clone()),
    }
}

/// The six `transition*` strings, compared to notice a change without
/// reparsing every frame.
type ConfigKey = [Option<String>; 6];

fn config_key(style: &StyleDesc) -> ConfigKey {
    [
        style.transition.clone(),
        style.transition_property.clone(),
        style.transition_duration.clone(),
        style.transition_timing_function.clone(),
        style.transition_delay.clone(),
        style.transition_behavior.clone(),
    ]
}

/// What the SVG properties of an element were in a frame. `declared` is the value as the element shows
/// it, `computed` the one CSS compares: `currentColor` stays itself.
#[derive(Clone, Debug, Default, PartialEq)]
struct SvgValues(BTreeMap<SvgAttr, (Option<AnimValue>, Option<AnimValue>)>);

impl SvgValues {
    fn read(source: &SvgSource, target: PropTarget, colors: &Colors) -> Self {
        let pinned = colors.with_current_color_pinned();
        Self(
            SvgAttr::ALL
                .iter()
                .copied()
                .filter(|attr| attr.is_css_property() && target.takes(PropKey::Svg(*attr)))
                .map(|attr| (attr, (source.computed(attr, colors), source.computed(attr, &pinned))))
                .collect(),
        )
    }
}

/// One frame's style of an element, to read a property of.
struct Frame<'a> {
    style: &'a StyleDesc,
    svg: &'a SvgValues,
    colors: &'a Colors,
}

impl Frame<'_> {
    fn declared(&self, key: PropKey) -> Option<AnimValue> {
        match key {
            PropKey::Style(prop) => declared_value(prop, self.style, self.colors),
            PropKey::Svg(attr) => self.svg.0.get(&attr)?.0.clone(),
        }
    }

    fn computed(&self, key: PropKey) -> Option<AnimValue> {
        match key {
            PropKey::Style(prop) => computed_value(prop, self.style, self.colors),
            PropKey::Svg(attr) => self.svg.0.get(&attr)?.1.clone(),
        }
    }
}

#[derive(Clone)]
struct ElementState {
    /// The previous frame's after-change style: the before-change style of the
    /// next style change.
    previous: StyleDesc,
    /// The SVG properties of the previous frame.
    previous_svg: SvgValues,
    /// What the last frame's after-change style resolved its colours in.
    colors: Colors,
    config: ConfigKey,
    list: TransitionList,
    /// Which properties the element has: a layout element its styles, a shape in an `<svg>` the SVG
    /// properties, and the `<svg>` both.
    target: PropTarget,
    running: BTreeMap<PropKey, Running>,
    /// Counts the steps, to tell which style change started a transition.
    generation: u64,
    /// Transitions that finished at their end, so a style that still has the
    /// end value does not start the transition again, and the Web Animations
    /// API can still read and play them as in a browser.
    completed: BTreeMap<PropKey, Completed>,
    cancelled: BTreeMap<PropKey, Cancelled>,
    /// The transitions the style let go of (the property is no longer transitioned, or has a new
    /// target) that JS holds, in the order they were made. A command brings one back as Chrome does:
    /// the same endpoints and timing, driving its property whatever the style says from here on.
    disowned: Vec<(PropKey, Running)>,
}

/// Where an element keeps a transition, see [`ElementState::take_run`].
enum Held {
    Running,
    Completed,
    Cancelled,
    /// At this place among the disowned ones.
    Disowned(usize),
}

/// What one tick of a transition found.
struct Tick {
    /// The run turned finished: its `transitionend` is owed, see [`told_end`].
    finished: bool,
    /// It is at its end, going forward.
    at_end: bool,
}

/// The clock half of a frame for one transition: starts a play that waits, tells the Web Animations
/// objects what the clock noted and the DOM what began.
fn tick(id: u64, key: PropKey, running: &mut Running, now: Instant, beat: Beat, events: &mut Events) -> Tick {
    let drawing = beat != Beat::Layout;
    let mut outcome = Tick { finished: false, at_end: false };
    let end = running.end_time();
    let started = drawing && running.clock.pending_play();
    if started {
        running.clock.resolve_pending(now, end);
    }
    running.clock.settle(now, end);
    events.tell(id, key, running, now);
    if beat == Beat::Held && running.clock.pending_pause() {
        running.clock.resolve_pending(now, end);
        events.tell(id, key, running, now);
    }
    if beat != Beat::Frame || running.clock.pending_play() {
        return outcome;
    }
    if running.announce && running.clock.is_finished(now, end) && !running.disowned {
        // Cancelled, then finished: Chrome tells nothing of a run that did not play.
        running.announce = false;
        running.began = true;
        running.clock.take_end(now, end);
    }
    if running.announce {
        running.announce = false;
        events.push(TransitionEvent::new(id, TransitionEventKind::Run, key, running.initial_elapsed()).late(started));
    }
    let elapsed = running.elapsed(now);
    if !running.began && elapsed >= running.delay {
        running.began = true;
        events.push(TransitionEvent::new(id, TransitionEventKind::Start, key, running.initial_elapsed()).late(started));
    }
    let rate = running.clock.rate();
    if let Some(restart) = running.restart {
        let inside = elapsed > running.delay || (elapsed == running.delay && rate >= 0.0);
        let at_end = if running.disowned { running.clock.reached_end(now, end) } else { running.clock.is_finished(now, end) };
        if inside && !at_end {
            running.restart = None;
            events.push(TransitionEvent::new(id, TransitionEventKind::Start, key, restart).late(started));
        }
    }
    let ended = if running.disowned && rate >= 0.0 {
        running.clock.take_end_reached(now, end)
    } else {
        running.clock.take_end(now, end)
    };
    if ended {
        outcome.finished = true;
    } else if running.began && running.restart.is_none() && (elapsed < running.delay || (elapsed == running.delay && rate < 0.0)) {
        // Told it began, and now back before its active interval without having finished
        // there: it ended at that edge.
        running.restart = Some(running.initial_elapsed());
        events.push(TransitionEvent::new(id, TransitionEventKind::End, key, running.initial_elapsed()).late(started));
    }
    if running.clock.status(now, end) == Status::Finished && running.clock.rate() >= 0.0 {
        outcome.at_end = true;
    }
    if running.clock.pending_pause() {
        running.clock.resolve_pending(now, end);
        events.tell(id, key, running, now);
    }
    outcome
}

/// The `transitionend` of a transition that finished, as the DOM tells it.
fn told_end(id: u64, key: PropKey, running: &mut Running, events: &mut Events) {
    if running.clock.rate() >= 0.0 {
        let active = running.effect_timing().active_duration();
        running.restart = Some(active);
        events.push(TransitionEvent::new(id, TransitionEventKind::End, key, active));
    } else if running.began {
        // Run back to its start, it ends at the other edge of its
        // active interval.
        running.restart = Some(running.initial_elapsed());
        events.push(TransitionEvent::new(id, TransitionEventKind::End, key, running.initial_elapsed()));
    }
    // Still before its active interval: the DOM saw nothing begin.
}

/// A transition that ran to its end and left the running ones. It stays until
/// the style moves off its end value.
#[derive(Clone)]
struct Completed {
    /// The computed end value.
    end: AnimValue,
    run: Running,
}

/// A transition `animation.cancel()` made idle. It stays, for `play()` and the like to bring
/// back, until the style changes the property to another value or stops transitioning it; a
/// style that still has its end value starts nothing.
#[derive(Clone)]
struct Cancelled {
    /// The computed end value.
    end: AnimValue,
    run: Running,
}

fn parse_config(id: u64, style: &StyleDesc) -> TransitionList {
    TransitionList::parse(style).unwrap_or_else(|error| {
        log::warn!("Invalid transition on element {id}: {error}");
        TransitionList::default()
    })
}

/// The SVG properties of an element: what it declares and what it is, see [`PropTarget`].
pub(crate) type Svg<'a> = Option<(&'a SvgSource<'a>, PropTarget)>;

impl ElementState {
    fn new(id: u64, after: &StyleDesc, colors: Colors, svg: Svg) -> Self {
        let target = svg.map_or(PropTarget::Element, |(_, target)| target);
        Self {
            previous: after.clone(),
            previous_svg: svg.map_or_else(SvgValues::default, |(source, target)| SvgValues::read(source, target, &colors)),
            colors,
            config: config_key(after),
            list: parse_config(id, after),
            target,
            running: BTreeMap::new(),
            generation: 0,
            completed: BTreeMap::new(),
            cancelled: BTreeMap::new(),
            disowned: Vec::new(),
        }
    }

    /// Nothing runs and nothing finished that a later style could repeat.
    fn is_idle(&self) -> bool {
        self.running.is_empty() && self.completed.is_empty() && self.disowned.is_empty()
    }

    /// Some transition is running, or one the style let go of is playing.
    fn drives(&self) -> bool {
        !self.running.is_empty() || self.disowned.iter().any(|(_, running)| !running.clock.is_idle())
    }

    /// The style lets go of `running`, which stays for a command to bring back if JS holds it.
    fn disown(&mut self, key: PropKey, mut running: Running) {
        if !running.handed.get() {
            return;
        }
        running.began = false;
        running.restart = None;
        running.announce = false;
        running.disowned = true;
        self.disowned.push((key, running));
    }

    /// The properties the disowned transitions drive right now, which no style change starts a
    /// transition on: they are in effect, running or filling.
    fn disowned_keys(&self, now: Instant) -> Vec<PropKey> {
        self.disowned
            .iter()
            .filter(|(_, running)| !running.clock.is_idle() && running.is_relevant(now))
            .map(|(key, _)| *key)
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn step(
        &mut self,
        id: u64,
        after: &StyleDesc,
        svg: Svg,
        parent_colors: &ColorContext,
        parent_font: &ParentValues,
        motion: Option<&MotionState>,
        measurement: Option<&Measurement>,
        interpolate_size: InterpolateSize,
        (scope, covered): (Scope, &[PropKey]),
        now: Instant,
        drawing: bool,
        events: &mut Events,
    ) {
        self.generation += 1;
        events.drawing = drawing;
        self.target = svg.map_or(PropTarget::Element, |(_, target)| target);
        let config = config_key(after);
        let config_changed = config != self.config;
        if config_changed {
            self.list = parse_config(id, after);
            self.config = config;
        }
        let beat = if drawing { Beat::Frame } else { Beat::Layout };
        self.advance(id, now, beat, events);
        let driven;
        let covered = if self.disowned.is_empty() {
            covered
        } else {
            driven = covered.iter().copied().chain(self.disowned_keys(now)).collect::<Vec<_>>();
            &driven
        };

        // A new `color` or appearance (`colorScheme`, or the window's) resolves
        // `currentColor` and `light-dark()` to another colour in a style that did
        // not change. CSS judges that by computed values: `currentColor` computes
        // to itself, so it follows the colour without a transition of its own,
        // while `light-dark()` computes to the colour its scheme picks, so a flip
        // is a change like any other. The before-change style is the last
        // frame's, in the colours of the last frame.
        let colors = Colors::inheriting(parent_colors, parent_font, Some(after));
        let previous_colors = std::mem::replace(&mut self.colors, colors.clone());
        let colors_changed = colors != previous_colors;
        let style_changed = self.previous != *after;
        let after_svg = svg.map_or_else(SvgValues::default, |(source, target)| SvgValues::read(source, target, &colors));
        let svg_changed = self.previous_svg != after_svg;
        if style_changed || config_changed || colors_changed || svg_changed {
            let keys: Vec<PropKey> = if style_changed || colors_changed || svg_changed {
                self.target_keys(scope)
            } else {
                self.running
                    .keys()
                    .chain(self.completed.keys())
                    .chain(self.cancelled.keys())
                    .copied()
                    .collect()
            };
            let now_frame = Frame { style: after, svg: &after_svg, colors: &colors };
            for key in keys {
                self.restyle(
                    id,
                    key,
                    &now_frame,
                    &previous_colors,
                    motion,
                    measurement,
                    interpolate_size,
                    covered,
                    now,
                    events,
                );
            }
            if style_changed {
                self.previous = after.clone();
            }
            self.previous_svg = after_svg;
        }
        self.advance(id, now, beat, events);
        if let Some(measurement) = measurement {
            for (key, running) in &mut self.running {
                running.follow(*key, measurement, now);
            }
        }
    }

    /// Every property the element has that a transition may move.
    fn target_keys(&self, scope: Scope) -> Vec<PropKey> {
        let styles = AnimProp::ALL
            .iter()
            .copied()
            .filter(|prop| scope.animates(*prop) && !prop.is_motion_only())
            .map(PropKey::Style);
        let svg = SvgAttr::ALL.iter().copied().filter(|attr| attr.is_css_property()).map(PropKey::Svg);
        let mut keys: Vec<PropKey> = styles.chain(svg).filter(|key| self.target.takes(*key)).collect();
        // The order the transitions are listed in, which the events of one time follow.
        keys.sort_by_key(|key| kebab_case(key.name()));
        keys
    }

    /// Reports transitions that began and finished, and moves the ones at their
    /// end to `completed`. One run back at its start stays running: it is
    /// before its active phase with the fill a transition has there, so the
    /// element keeps showing the value it started from until the style changes.
    ///
    /// The DOM hears of a run in a frame that is drawn (`drawing`), not in a layout pass a read
    /// forced, and a run whose play waits for a frame is not evaluated before it: see
    /// `Run::tick` of the animations, which this is the other half of.
    fn advance(&mut self, id: u64, now: Instant, beat: Beat, events: &mut Events) {
        let mut finished = Vec::new();
        let mut at_end = Vec::new();
        for (key, running) in &mut self.running {
            let outcome = tick(id, *key, running, now, beat, events);
            if outcome.finished {
                finished.push(*key);
            }
            if outcome.at_end {
                at_end.push(*key);
            }
        }
        // Events come in the order of the times they are scheduled at.
        finished.sort_by(|first, second| {
            self.running[first].end_time().total_cmp(&self.running[second].end_time())
        });
        for key in finished {
            let Some(running) = self.running.get_mut(&key) else {
                continue;
            };
            told_end(id, key, running, events);
        }
        for key in at_end {
            if let Some(running) = self.running.remove(&key) {
                self.completed.insert(key, Completed { end: running.end_computed.clone(), run: running });
            }
        }
        for (key, running) in &mut self.disowned {
            if running.clock.is_idle() {
                continue;
            }
            if tick(id, *key, running, now, beat, events).finished {
                told_end(id, *key, running, events);
            }
        }
    }

    fn cancel(
        &mut self,
        id: u64,
        key: PropKey,
        now: Instant,
        events: &mut Events,
    ) {
        if let Some(mut running) = self.running.remove(&key) {
            let active = running.elapsed(now) - running.delay;
            let end = running.end_time();
            running.clock.apply(Command::Cancel, now, end).expect("cancelling cannot fail");
            events.tell(id, key, &mut running, now);
            events.push(TransitionEvent::new(
                id,
                TransitionEventKind::Cancel,
                key,
                active.clamp(0.0, running.effect_timing().active_duration()),
            ));
            self.disown(key, running);
        }
    }

    /// A transition that finished is cancelled, to the Web Animations objects
    /// that still hold it, once the style moves off its end value. The DOM has
    /// no `transitioncancel` for it.
    fn forget(&mut self, id: u64, key: PropKey, now: Instant, events: &mut Events) {
        if let Some(mut completed) = self.completed.remove(&key) {
            let end = completed.run.end_time();
            completed.run.clock.apply(Command::Cancel, now, end).expect("cancelling cannot fail");
            events.tell(id, key, &mut completed.run, now);
            self.disown(key, completed.run);
        }
    }

    fn begin(
        &mut self,
        id: u64,
        key: PropKey,
        mut running: Running,
        events: &mut Events,
    ) {
        running.generation = self.generation;
        let late = events.drawing;
        events.push(TransitionEvent::new(id, TransitionEventKind::Run, key, running.initial_elapsed()).late(late));
        // Chrome starts a transition with no delay as it makes it, before its play has a frame.
        if running.delay <= 0.0 {
            running.began = true;
            events.push(TransitionEvent::new(id, TransitionEventKind::Start, key, running.initial_elapsed()).late(late));
        }
        self.running.insert(key, running);
    }

    /// The start rules of CSS Transitions 1 §3 for one property, run when its
    /// style or the element's `transition*` changed.
    #[allow(clippy::too_many_arguments)]
    fn restyle(
        &mut self,
        id: u64,
        key: PropKey,
        after: &Frame,
        previous_colors: &Colors,
        motion: Option<&MotionState>,
        measurement: Option<&Measurement>,
        interpolate_size: InterpolateSize,
        covered: &[PropKey],
        now: Instant,
        events: &mut Events,
    ) {
        let entry = if motion.is_some_and(|motion| motion.owns(key)) {
            None
        } else {
            self.list.matching(key).cloned()
        };
        let Some(entry) = entry else {
            self.cancel(id, key, now, events);
            self.forget(id, key, now, events);
            if let Some(cancelled) = self.cancelled.remove(&key) {
                self.disown(key, cancelled.run);
            }
            return;
        };

        let after_value = after.declared(key);
        let after_computed = after.computed(key);
        if self.completed.get(&key).is_some_and(|completed| after_computed.as_ref() != Some(&completed.end)) {
            self.forget(id, key, now, events);
        }
        if self.cancelled.get(&key).is_some_and(|cancelled| after_computed.as_ref() != Some(&cancelled.end)) {
            if let Some(cancelled) = self.cancelled.remove(&key) {
                self.disown(key, cancelled.run);
            }
        }
        let has_duration = entry.duration + entry.delay > 0.0;

        // A running animation drives the property, so its before-change and
        // after-change values are both the animated one and nothing starts. A
        // transition that ran toward the old declared value is cancelled by the
        // change; one the style still agrees with keeps running over the animation.
        if covered.contains(&key) {
            if self
                .running
                .get(&key)
                .is_some_and(|old| after_computed.as_ref() != Some(&old.end_computed))
            {
                self.cancel(id, key, now, events);
            }
            return;
        }

        let Some(old) = self.running.get(&key) else {
            if self.completed.contains_key(&key) || self.cancelled.contains_key(&key) || !has_duration {
                return;
            }
            let before = Frame { style: &self.previous, svg: &self.previous_svg, colors: previous_colors };
            let (before, before_computed) = (before.declared(key), before.computed(key));
            if before_computed == after_computed
                || (!interpolate_size.allows_keywords()
                    && is_keyword_change(key, before.as_ref(), after_value.as_ref()))
            {
                return;
            }
            let from = laid_out_start(key, before.as_ref(), measurement);
            let to = after_value.as_ref().and_then(|end| laid_out_end(key, end, measurement));
            if let (Some(from), Some(to), Some(end), Some(end_computed)) =
                (from, to, after_value, after_computed)
            {
                let reversing_start = before_computed.unwrap_or_else(|| from.clone());
                if let Some(running) = Running::new(
                    key, &from, &to, (end, end_computed), &entry, reversing_start, 1.0, now, measurement,
                ) {
                    self.begin(id, key, running, events);
                }
            }
            return;
        };
        if after_computed.as_ref() == Some(&old.end_computed) {
            return;
        }

        let current = old.value(now);
        let reversal = (after_computed.as_ref() == Some(&old.reversing_start)).then(|| {
            let shortening = (old.progress(old.elapsed(now)) * old.shortening
                + (1.0 - old.shortening))
                .abs()
                .clamp(0.0, 1.0);
            (old.end_computed.clone(), shortening)
        });
        self.cancel(id, key, now, events);
        let (Some(end), Some(end_computed)) = (after_value, after_computed) else {
            return;
        };
        if !interpolate_size.allows_keywords() && is_keyword_change(key, Some(&current), Some(&end))
        {
            return;
        }
        let Some(to) = laid_out_end(key, &end, measurement) else {
            return;
        };
        if current == to || !has_duration {
            return;
        }
        let (reversing_start, shortening) = reversal.unwrap_or_else(|| (current.clone(), 1.0));
        if let Some(running) = Running::new(
            key, &current, &to, (end, end_computed), &entry, reversing_start, shortening, now, measurement,
        ) {
            self.begin(id, key, running, events);
        }
    }

    /// Takes the transition `id` out of wherever this element keeps it, and where it was.
    fn take_run(&mut self, id: u64) -> Option<(PropKey, Running, Held)> {
        let is = |running: &Running| running.clock.id() == id;
        if let Some(key) = self.running.iter().find(|(_, run)| is(run)).map(|(key, _)| *key) {
            Some((key, self.running.remove(&key)?, Held::Running))
        } else if let Some(key) = self.completed.iter().find(|(_, done)| is(&done.run)).map(|(key, _)| *key) {
            Some((key, self.completed.remove(&key)?.run, Held::Completed))
        } else if let Some(key) = self.cancelled.iter().find(|(_, held)| is(&held.run)).map(|(key, _)| *key) {
            Some((key, self.cancelled.remove(&key)?.run, Held::Cancelled))
        } else {
            let at = self.disowned.iter().position(|(_, run)| is(run))?;
            let (key, run) = self.disowned.remove(at);
            Some((key, run, Held::Disowned(at)))
        }
    }

    /// `updateTiming()` of the transition `id` of this element, wherever it is. `None` when it
    /// is not here.
    fn update_timing(
        &mut self,
        element: u64,
        id: u64,
        update: &TimingUpdate,
        now: Instant,
        events: &mut Events,
    ) -> Option<()> {
        let (key, mut run, held) = self.take_run(id)?;
        run.update_timing(update, now);
        events.tell(element, key, &mut run, now);
        if let Held::Disowned(at) = held {
            self.disowned.insert(at, (key, run));
        } else if run.clock.is_idle() {
            self.cancelled.insert(key, Cancelled { end: run.end_computed.clone(), run });
        } else {
            // The next step puts a finished one back.
            self.running.insert(key, run);
        }
        Some(())
    }

    /// Applies `command` to the transition `id` of this element, wherever it is. `None` when
    /// it is not here.
    fn command(
        &mut self,
        element: u64,
        id: u64,
        command: Command,
        now: Instant,
        events: &mut Events,
    ) -> Option<Result<(), String>> {
        let (key, mut run, held) = self.take_run(id)?;
        let was_running = matches!(held, Held::Running | Held::Disowned(_));
        let end = run.end_time();
        let was_idle = run.clock.is_idle();
        // What the DOM is owed for a run that is stopped while it runs.
        let owed = (command == Command::Cancel && was_running && !run.clock.is_finished(now, end))
            .then(|| (run.elapsed(now) - run.delay).clamp(0.0, run.effect_timing().active_duration()));
        let outcome = run.clock.apply(command, now, end);
        events.tell(element, key, &mut run, now);
        if let Some(elapsed) = owed {
            events.push(TransitionEvent::new(element, TransitionEventKind::Cancel, key, elapsed));
        }
        if run.clock.is_idle() {
            run.began = false;
            run.restart = None;
            match held {
                Held::Disowned(at) => self.disowned.insert(at, (key, run)),
                _ => {
                    self.cancelled.insert(key, Cancelled { end: run.end_computed.clone(), run });
                }
            }
        } else {
            if was_idle {
                run.announce = true;
                run.began = false;
                run.restart = None;
            }
            match held {
                Held::Disowned(at) => self.disowned.insert(at, (key, run)),
                // The next step puts a finished one back.
                _ => {
                    self.running.insert(key, run);
                }
            }
        }
        Some(outcome)
    }

    fn values(&self, now: Instant) -> Vec<(PropKey, AnimValue)> {
        // The ones the style let go of come last, which wins: they are the highest in the stack.
        let disowned = self
            .disowned
            .iter()
            .filter(|(_, running)| !running.clock.is_idle() && running.is_relevant(now))
            .map(|(key, running)| (*key, key.clamp_value(running.value(now))));
        self.running
            .iter()
            .map(|(key, running)| (*key, key.clamp_value(running.value(now))))
            .chain(disowned)
            .collect()
    }
}

/// The values of the transitions running on an element, ready to lay over its
/// after-change style.
pub(crate) struct TransitionStyle(Vec<(PropKey, AnimValue)>);

impl TransitionStyle {
    pub(crate) fn apply_to(&self, style: &mut StyleDesc) {
        for (key, value) in &self.0 {
            if let PropKey::Style(prop) = key {
                prop.write(style, value.clone());
            }
        }
    }

    /// The SVG attributes the transitions drive, as the XML writes them.
    pub(crate) fn svg_attributes(&self) -> Vec<(&'static str, String)> {
        self.0
            .iter()
            .filter_map(|(key, value)| match key {
                PropKey::Svg(attr) => Some((attr.attribute()?, attribute_text(value))),
                PropKey::Style(_) => None,
            })
            .collect()
    }

    pub(super) fn runs(&self, key: PropKey) -> bool {
        self.0.iter().any(|(running, _)| *running == key)
    }
}

/// The transition state of every element that declares one.
#[derive(Default)]
pub(crate) struct CssTransitions {
    states: HashMap<u64, ElementState>,
    events: Events,
    /// The elements a Web Animations command reached since the last frame.
    commanded: Vec<u64>,
    /// The pass being built is a layout pass a read forced, not a frame that is drawn.
    layout_only: bool,
    active: bool,
    /// See `CssAnimations::dormant`.
    dormant: HashSet<u64>,
    /// See `CssAnimations::held`.
    held: HashSet<u64>,
    /// The dormant elements with a transition running, the only ones a tick has anything to do for.
    running_dormant: HashSet<u64>,
    /// The roots `display: none` has cancelled for, until they render again.
    hidden: HashSet<u64>,
}

/// See `css_animation::Rewind`.
pub(crate) struct Rewind {
    id: u64,
    state: Option<ElementState>,
    dom_events: usize,
    api_events: usize,
    active: bool,
    running_dormant: bool,
}

impl CssTransitions {
    pub(crate) fn rewind_point(&self, id: u64) -> Rewind {
        Rewind {
            id,
            state: self.states.get(&id).cloned(),
            dom_events: self.events.dom.len(),
            api_events: self.events.api.len(),
            active: self.active,
            running_dormant: self.running_dormant.contains(&id),
        }
    }

    /// A transition the pass started stays, with what it told: a read styles the element as in
    /// Chrome, and the transition runs from there (see `CssAnimations::rewind`).
    pub(crate) fn rewind(&mut self, point: Rewind) {
        let started = self.states.get(&point.id).is_some_and(|after| {
            after
                .running
                .keys()
                .any(|key| !point.state.as_ref().is_some_and(|before| before.running.contains_key(key)))
        });
        if started {
            return;
        }
        match point.state {
            Some(state) => {
                self.states.insert(point.id, state);
            }
            None => {
                self.states.remove(&point.id);
            }
        }
        self.events.dom.truncate(point.dom_events);
        self.events.api.truncate(point.api_events);
        self.active = point.active;
        if point.running_dormant {
            self.running_dormant.insert(point.id);
        } else {
            self.running_dormant.remove(&point.id);
        }
    }

    /// Whether `id` takes part in transitions: it declares one, in its own
    /// style or a state style, or still has transitions to settle.
    ///
    /// An element that stopped declaring keeps its state for the one update
    /// that cancels what was running (`transitioncancel`); once the state is
    /// idle the next call drops it.
    pub(crate) fn wants(&mut self, id: u64, style: Option<&StyleDesc>) -> bool {
        if style.is_some_and(declares_transitions) {
            return true;
        }
        match self.states.get(&id) {
            None => false,
            Some(state) if state.is_idle() => {
                self.states.remove(&id);
                false
            }
            Some(_) => true,
        }
    }

    /// The elements that were rendered and transition `display` under
    /// `allow-discrete`, so a `display: none` they now declare may still be
    /// shown while the transition runs. The element decides once it has stepped
    /// the transition.
    pub(crate) fn held_displays(&self) -> HashSet<u64> {
        self.states
            .iter()
            .filter(|(_, state)| {
                state.running.contains_key(&PropKey::Style(AnimProp::Display))
                    || state.list.matching(PropKey::Style(AnimProp::Display)).is_some_and(|entry| {
                        entry.allow_discrete && entry.duration + entry.delay > 0.0
                    })
            })
            .map(|(id, _)| *id)
            .collect()
    }

    /// Whether `id` has transition state: a style it is seen with can transition from the one before.
    pub(crate) fn tracks(&self, id: u64) -> bool {
        self.states.contains_key(&id)
    }

    /// Starts tracking `id` from `before`, the style it had before it first
    /// declared a transition. The transitions of a change come from the style
    /// after it, so a change that also adds the declaration must not be the
    /// first style the element is seen with.
    pub(crate) fn seed(
        &mut self,
        id: u64,
        before: &StyleDesc,
        svg: Svg,
        parent_colors: &ColorContext,
        parent_font: &ParentValues,
    ) {
        if let std::collections::hash_map::Entry::Vacant(slot) = self.states.entry(id) {
            let colors = Colors::inheriting(parent_colors, parent_font, Some(before));
            slot.insert(ElementState::new(id, before, colors, svg));
        }
    }

    /// Feeds one frame's after-change style. The first style an element is seen
    /// with never transitions: there is nothing before it. `inherited` is the
    /// `interpolateSize` of the parent, which `after` overrides when it sets one.
    /// `covered` are the properties a CSS animation drives right now. `svg` is where the SVG properties
    /// of an `<svg>` or of a shape in one come from.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn update(
        &mut self,
        id: u64,
        after: &StyleDesc,
        svg: Svg,
        parent_colors: &ColorContext,
        parent_font: &ParentValues,
        motion: Option<&MotionState>,
        measurement: Option<&Measurement>,
        inherited: InterpolateSize,
        scope: Scope,
        covered: &[PropKey],
        now: Instant,
    ) -> Option<TransitionStyle> {
        let Some(state) = self.states.get_mut(&id) else {
            let colors = Colors::inheriting(parent_colors, parent_font, Some(after));
            self.states.insert(id, ElementState::new(id, after, colors, svg));
            return None;
        };
        let interpolate_size = InterpolateSize::declared(after).unwrap_or(inherited);
        state.step(
            id,
            after,
            svg,
            parent_colors,
            parent_font,
            motion,
            measurement,
            interpolate_size,
            (scope, covered),
            now,
            !self.layout_only,
            &mut self.events,
        );
        if !self.dormant.contains(&id) {
            self.active |= state
                .running
                .values()
                .chain(state.disowned.iter().map(|(_, running)| running))
                .any(|running| running.clock.wants_frame(now, running.end_time()));
        }
        let values = state.values(now);
        self.note_running(id);
        (!values.is_empty()).then_some(TransitionStyle(values))
    }

    /// A frame of a shape inside an `<svg>`: its transitions advance and the SVG properties they drive
    /// come back. A shape has no layout, so nothing waits for a measurement.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn animate_shape(
        &mut self,
        id: u64,
        declared: &StyleDesc,
        source: &SvgSource,
        parent_colors: &ColorContext,
        parent_font: &ParentValues,
        motion: Option<&MotionState>,
        covered: &[PropKey],
        now: Instant,
    ) -> Option<TransitionStyle> {
        self.update(
            id,
            declared,
            Some((source, PropTarget::SvgShape)),
            parent_colors,
            parent_font,
            motion,
            None,
            InterpolateSize::default(),
            Scope::Element,
            covered,
            now,
        )
    }

    /// The shapes inside an `<svg>` with a transition running, which their `<svg>` has to write again
    /// every frame.
    pub(crate) fn svg_shapes(&self) -> impl Iterator<Item = u64> + '_ {
        self.states
            .iter()
            .filter(|(_, state)| state.target == PropTarget::SvgShape && state.drives())
            .map(|(id, _)| *id)
    }

    /// Whether the shape `id` inside an `<svg>` is one of `svg_shapes`.
    pub(crate) fn shape_moves(&self, id: u64) -> bool {
        self.states
            .get(&id)
            .is_some_and(|state| state.target == PropTarget::SvgShape && state.drives())
    }

    /// Brings the transitions of `id` up to `now` without a style, see `CssAnimations::advance_clock`.
    pub(crate) fn advance_clock(&mut self, id: u64, now: Instant) {
        let Some(state) = self.states.get_mut(&id) else {
            return;
        };
        let beat = if self.held.contains(&id) { Beat::Held } else { Beat::Frame };
        state.advance(id, now, beat, &mut self.events);
    }

    /// Whether a transition of `opacity` is running on `id`.
    pub(crate) fn animates_opacity(&self, id: u64) -> bool {
        self.states.get(&id).is_some_and(|state| {
            state.running.contains_key(&PropKey::Style(AnimProp::Opacity))
                || state.disowned.iter().any(|(key, _)| *key == PropKey::Style(AnimProp::Opacity))
        })
    }

    /// Whether a transition of a property that moves, scales or turns the element is running on `id`.
    pub(crate) fn animates_transform(&self, id: u64) -> bool {
        self.states
            .get(&id)
            .is_some_and(|state| {
                state.running.keys().chain(state.disowned.iter().map(|(key, _)| key)).any(|key| matches!(key, PropKey::Style(prop) if prop.is_transform()))
            })
    }

    /// Whether every transition running on `id` is on a property that only changes what it paints.
    pub(crate) fn paints_only(&self, id: u64) -> bool {
        self.states.get(&id).is_none_or(|state| {
            state.running.keys().chain(state.disowned.iter().map(|(key, _)| key)).all(|key| match key {
                PropKey::Style(prop) => prop.paints_only(),
                // The attributes of an `<svg>` and its shapes change its picture, never its box.
                PropKey::Svg(_) => true,
            })
        })
    }

    pub(crate) fn enter_dormant(&mut self, id: u64) {
        self.dormant.insert(id);
        self.note_running(id);
    }

    pub(crate) fn leave_dormant(&mut self, id: u64) {
        if !self.held.contains(&id) && self.dormant.remove(&id) {
            self.running_dormant.remove(&id);
        }
    }

    /// `id` is built for a read inside content that stays skipped, see `CssAnimations::hold`.
    pub(crate) fn hold(&mut self, id: u64) {
        if self.held.insert(id) {
            self.dormant.insert(id);
        }
        self.note_running(id);
    }

    /// Whether a read has made runs that are kept on time without a build.
    pub(crate) fn holds_any(&self) -> bool {
        !self.held.is_empty()
    }

    /// `id` is built for a frame: the DOM hears of its transitions from here on.
    pub(crate) fn release(&mut self, id: u64) {
        if self.held.is_empty() || !self.held.remove(&id) {
            return;
        }
        self.dormant.remove(&id);
        self.running_dormant.remove(&id);
    }

    fn note_running(&mut self, id: u64) {
        if self.dormant.is_empty() {
            return;
        }
        if self.dormant.contains(&id) && self.states.get(&id).is_some_and(ElementState::drives) {
            self.running_dormant.insert(id);
        } else {
            self.running_dormant.remove(&id);
        }
    }

    /// `advance_clock` for the dormant elements that have a transition running.
    pub(crate) fn advance_dormant(&mut self, now: Instant) {
        if self.running_dormant.is_empty() {
            return;
        }
        let running: Vec<u64> = self.running_dormant.iter().copied().collect();
        for id in running {
            self.advance_clock(id, now);
            self.note_running(id);
        }
    }

    /// When the dormant elements next have a transition to start or end, which is what a tick is for.
    pub(crate) fn dormant_wake(&self, now: Instant) -> Wake {
        Wake::merged(
            self.running_dormant
                .iter()
                .filter_map(|id| self.states.get(id))
                .flat_map(|state| state.running.values())
                .map(|running| running.wake(now)),
        )
    }

    /// `id` is `display: none`, which takes it and everything below it out of
    /// rendering: its transitions are cancelled and its state dropped. The
    /// next style it renders with is the first it is seen with, so a change
    /// made while it was hidden does not transition when it shows again.
    /// Cheap to call every frame the element stays hidden.
    pub(crate) fn hide(&mut self, tree: &RetainedTree, id: u64, now: Instant) {
        if !self.hidden.insert(id) || self.states.is_empty() {
            return;
        }
        for hidden in super::subtree_ids(tree, id) {
            let Some(mut state) = self.states.remove(&hidden) else {
                continue;
            };
            self.release(hidden);
            self.running_dormant.remove(&hidden);
            let running: Vec<PropKey> = state.running.keys().copied().collect();
            for key in running {
                state.cancel(hidden, key, now, &mut self.events);
            }
            let completed: Vec<PropKey> = state.completed.keys().copied().collect();
            for key in completed {
                state.forget(hidden, key, now, &mut self.events);
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
        self.held.retain(|id| keep(*id));
    }

    /// Events queued since the last call, in the order they happened.
    pub(crate) fn take_events(&mut self) -> Vec<TransitionEvent> {
        std::mem::take(&mut self.events.dom)
    }

    /// The transitions on `element` that are relevant, in composite order: by
    /// the style change that started them, then by property name.
    pub(crate) fn list(&self, element: u64, now: Instant) -> Vec<Info> {
        let Some(state) = self.states.get(&element) else {
            return Vec::new();
        };
        let mut running: Vec<(u64, Info)> = state
            .running
            .iter()
            .filter(|(_, running)| running.is_relevant(now))
            .map(|(key, running)| (running.generation, running.handed_info(*key, now)))
            .collect();
        running.sort_by(|(first_generation, first), (second_generation, second)| {
            first_generation.cmp(second_generation).then_with(|| first.name.cmp(&second.name))
        });
        running.into_iter().map(|(_, info)| info).collect()
    }

    /// The transitions on `element` that the style let go of and are relevant, in the order they were made.
    pub(crate) fn list_disowned(&self, element: u64, now: Instant) -> Vec<Info> {
        let Some(state) = self.states.get(&element) else {
            return Vec::new();
        };
        state
            .disowned
            .iter()
            .filter(|(_, running)| !running.clock.is_idle() && running.is_relevant(now))
            .map(|(key, running)| running.handed_info(*key, now))
            .collect()
    }

    /// The transition `id`, relevant or not, running or finished at its end.
    /// `None` once it was cancelled, or the style moved off its end value.
    pub(crate) fn info(&self, id: u64, now: Instant) -> Option<Info> {
        self.states.values().find_map(|state| {
            state
                .running
                .iter()
                .chain(state.completed.iter().map(|(key, completed)| (key, &completed.run)))
                .chain(state.cancelled.iter().map(|(key, cancelled)| (key, &cancelled.run)))
                .chain(state.disowned.iter().map(|(key, running)| (key, running)))
                .find(|(_, running)| running.clock.id() == id)
                .map(|(key, running)| running.handed_info(*key, now))
        })
    }

    /// Whether the last frame left `id` dormant.
    pub(crate) fn is_dormant(&self, id: u64) -> bool {
        self.dormant.contains(&id)
    }

    /// JS holds no `Animation` for the transition `id` any more: one the style let go of is dropped.
    pub(crate) fn forget(&mut self, id: u64) {
        for state in self.states.values_mut() {
            state.disowned.retain(|(_, running)| running.clock.id() != id);
        }
    }

    fn run(&self, id: u64) -> Option<(PropKey, &Running)> {
        self.states.values().find_map(|state| {
            state
                .running
                .iter()
                .chain(state.completed.iter().map(|(key, completed)| (key, &completed.run)))
                .chain(state.cancelled.iter().map(|(key, cancelled)| (key, &cancelled.run)))
                .chain(state.disowned.iter().map(|(key, running)| (key, running)))
                .find(|(_, running)| running.clock.id() == id)
                .map(|(key, running)| (*key, running))
        })
    }

    /// The timing and the computed timing of the transition `id`. `None` once it was cancelled
    /// for good.
    pub(crate) fn effect(&self, id: u64, now: Instant) -> Option<Result<EffectInfo, String>> {
        self.run(id).map(|(_, running)| running.effect_info(now))
    }

    /// The keyframes of the transition `id`: where it runs from and to.
    pub(crate) fn keyframes(&self, id: u64) -> Option<Result<Vec<KeyframeInfo>, String>> {
        self.run(id).map(|(key, running)| running.keyframe_info(key))
    }

    /// `updateTiming()` of the transition `id` at `now`, the time of the last frame; the
    /// transition alone. `None` for an id that is not here.
    pub(crate) fn update_timing(&mut self, id: u64, update: &TimingUpdate, now: Instant) -> Option<()> {
        for (element, state) in &mut self.states {
            if state.update_timing(*element, id, update, now, &mut self.events).is_none() {
                continue;
            }
            self.commanded.push(*element);
            if self.dormant.contains(element) {
                self.running_dormant.insert(*element);
            }
            return Some(());
        }
        None
    }

    /// The element the transition `id` is of, running or finished at its end.
    pub(crate) fn element_of(&self, id: u64) -> Option<u64> {
        self.states
            .iter()
            .find(|(_, state)| {
                state.running.values().any(|running| running.clock.id() == id)
                    || state.completed.values().any(|completed| completed.run.clock.id() == id)
                    || state.cancelled.values().any(|cancelled| cancelled.run.clock.id() == id)
                    || state.disowned.iter().any(|(_, running)| running.clock.id() == id)
            })
            .map(|(element, _)| *element)
    }

    /// Whether the pass being built is a layout pass a read forced rather than a frame that is
    /// drawn, see `CssAnimations::set_layout_only`.
    pub(crate) fn set_layout_only(&mut self, layout_only: bool) {
        self.layout_only = layout_only;
    }

    /// Applies `command` to the transition `id` at `now`, the time of the last
    /// frame. One that finished at its end runs again from there, as in a
    /// browser, where the finished transition is still an `Animation`; so does
    /// one that `cancel()` made idle, once a command gives it a time. `None`
    /// for an id that is neither running, finished at its end nor cancelled.
    pub(crate) fn command(&mut self, id: u64, command: Command, now: Instant) -> Option<Result<(), String>> {
        for (element, state) in &mut self.states {
            let Some(outcome) = state.command(*element, id, command, now, &mut self.events) else {
                continue;
            };
            self.commanded.push(*element);
            if self.dormant.contains(element) {
                self.running_dormant.insert(*element);
            }
            return Some(outcome);
        }
        None
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
        std::mem::take(&mut self.events.api)
    }

    /// Whether a transition was still running at the last update, so the
    /// window needs another frame.
    pub(crate) fn take_active(&mut self) -> bool {
        std::mem::take(&mut self.active)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use super::super::web_animation::ApiEventKind;
    use crate::css::length::LinearLength;
    use crate::events::state_styles::resolve_state_styles;
    use crate::style::DimensionValue;

    fn style(json: &str) -> StyleDesc {
        serde_json::from_str(json).unwrap()
    }

    fn list(json: &str) -> TransitionList {
        TransitionList::parse(&style(json)).unwrap()
    }

    fn parse_error(json: &str) -> String {
        TransitionList::parse(&style(json)).unwrap_err()
    }

    fn entry(list: &TransitionList, index: usize) -> (&Matcher, f64, f64) {
        let entry = &list.0[index];
        (&entry.matcher, entry.duration, entry.delay)
    }

    #[test]
    fn the_shorthand_reads_property_times_and_timing_in_any_order() {
        let transitions = list(
            r#"{ "transition": "background-color 150ms ease-out, opacity .2s 50ms, all 1s linear -100ms" }"#,
        );
        assert_eq!(transitions.0.len(), 3);
        assert_eq!(
            transitions.0[0],
            Entry {
                matcher: Matcher::Props(vec![PropKey::Style(AnimProp::BackgroundColor)]),
                duration: 0.15,
                delay: 0.0,
                timing: Ease::Name("easeOut".to_string()),
                allow_discrete: false,
            }
        );
        assert_eq!(entry(&transitions, 1).1, 0.2);
        assert_eq!(entry(&transitions, 1).2, 0.05);
        assert_eq!(entry(&transitions, 1).0, &Matcher::Props(vec![PropKey::Style(AnimProp::Opacity), PropKey::Svg(SvgAttr::Opacity)]));
        assert_eq!(entry(&transitions, 2), (&Matcher::All, 1.0, -0.1));

        let reordered = list(r#"{ "transition": "ease-in 1s 2s width" }"#);
        assert_eq!(entry(&reordered, 0).1, 1.0);
        assert_eq!(entry(&reordered, 0).2, 2.0);
        assert_eq!(reordered.0[0].timing, Ease::Name("easeIn".to_string()));
    }

    #[test]
    fn a_shorthand_without_a_property_is_all_and_none_is_empty() {
        assert_eq!(entry(&list(r#"{ "transition": "1s" }"#), 0).0, &Matcher::All);
        for none in ["none", " NONE "] {
            let transitions = list(&format!(r#"{{ "transition": "{none}" }}"#));
            assert!(transitions.0.is_empty());
        }
        assert!(list(r#"{}"#).0.is_empty());
        assert!(list(r#"{ "transitionProperty": "none", "transitionDuration": "1s" }"#).0.is_empty());
    }

    #[test]
    fn a_timing_function_with_commas_stays_one_value() {
        let transitions = list(r#"{ "transition": "width 1s cubic-bezier(0.4, 0, 0.2, 1), height 2s steps(4, end)" }"#);
        assert_eq!(transitions.0.len(), 2);
        assert_eq!(transitions.0[0].timing, Ease::CubicBezier([0.4, 0.0, 0.2, 1.0]));
        assert!(matches!(transitions.0[1].timing, Ease::Steps(_)));
    }

    #[test]
    fn longhands_win_over_the_shorthand() {
        let transitions = list(
            r#"{ "transition": "width 1s linear", "transitionDuration": "3s", "transitionDelay": "100ms" }"#,
        );
        assert_eq!(entry(&transitions, 0), (&Matcher::Props(vec![PropKey::Style(AnimProp::Width), PropKey::Svg(SvgAttr::Width)]), 3.0, 0.1));
        assert_eq!(transitions.0[0].timing, Ease::Name("linear".to_string()));
    }

    #[test]
    fn longhand_lists_repeat_and_truncate_to_the_property_list() {
        let transitions = list(
            r#"{
                "transitionProperty": "width, height, opacity",
                "transitionDuration": "1s, 2s",
                "transitionDelay": "5s, 6s, 7s, 8s",
                "transitionTimingFunction": "linear"
            }"#,
        );
        let durations: Vec<f64> = transitions.0.iter().map(|entry| entry.duration).collect();
        let delays: Vec<f64> = transitions.0.iter().map(|entry| entry.delay).collect();
        assert_eq!(durations, [1.0, 2.0, 1.0]);
        assert_eq!(delays, [5.0, 6.0, 7.0]);
        assert!(transitions
            .0
            .iter()
            .all(|entry| entry.timing == Ease::Name("linear".to_string())));
    }

    #[test]
    fn longhands_alone_default_to_all_and_a_zero_duration() {
        let transitions = list(r#"{ "transitionDuration": "1s" }"#);
        assert_eq!(entry(&transitions, 0), (&Matcher::All, 1.0, 0.0));
        assert_eq!(entry(&list(r#"{ "transitionProperty": "width" }"#), 0).1, 0.0);
    }

    #[test]
    fn shorthand_property_names_cover_their_longhands() {
        let padding = list(r#"{ "transition": "padding 1s" }"#);
        for prop in [AnimProp::PaddingTop, AnimProp::PaddingLeft] {
            assert!(padding.matching(PropKey::Style(prop)).is_some());
        }
        assert!(padding.matching(PropKey::Style(AnimProp::MarginTop)).is_none());
        let colours = list(r#"{ "transition": "border-color 1s, text-decoration 1s" }"#);
        for prop in [AnimProp::BorderTopColor, AnimProp::BorderLeftColor, AnimProp::TextDecorationColor] {
            assert!(colours.matching(PropKey::Style(prop)).is_some(), "{prop:?}");
        }
        assert!(colours.matching(PropKey::Style(AnimProp::OutlineColor)).is_none());
        let border = list(r#"{ "transition": "border 1s, border-radius 1s, inset 1s, flex 1s" }"#);
        for prop in [
            AnimProp::BorderLeftWidth,
            AnimProp::BorderTopColor,
            AnimProp::BorderLeftColor,
            AnimProp::BorderTopLeftRadius,
            AnimProp::Left,
            AnimProp::FlexBasis,
        ] {
            assert!(border.matching(PropKey::Style(prop)).is_some(), "{prop:?}");
        }
    }

    #[test]
    fn the_last_matching_entry_wins() {
        let transitions = list(r#"{ "transition": "all 1s, opacity 2s" }"#);
        assert_eq!(transitions.matching(PropKey::Style(AnimProp::Opacity)).unwrap().duration, 2.0);
        assert_eq!(transitions.matching(PropKey::Style(AnimProp::Width)).unwrap().duration, 1.0);
        let reversed = list(r#"{ "transition": "opacity 2s, all 1s" }"#);
        assert_eq!(reversed.matching(PropKey::Style(AnimProp::Opacity)).unwrap().duration, 1.0);
    }

    #[test]
    fn a_valid_property_gpuix_cannot_animate_matches_nothing() {
        let transitions = list(r#"{ "transition": "perspective 1s, display 1s, opacity 1s" }"#);
        assert_eq!(transitions.0.len(), 3);
        assert!(transitions.matching(PropKey::Style(AnimProp::Opacity)).is_some());
        assert!(transitions.matching(PropKey::Style(AnimProp::Width)).is_none());
    }

    #[test]
    fn validation_covers_the_state_styles() {
        assert!(validate_transitions(&style(r#"{ "transition": "width 1s" }"#)).is_ok());
        let error = validate_transitions(&style(r#"{ "hover": { "transitionDelay": "soon" } }"#))
            .unwrap_err();
        assert!(error.starts_with("hover.transitionDelay"), "{error}");
        assert!(validate_transitions(&style(r#"{ "transition": "width bogus" }"#)).is_err());
        for name in ["groupHover", "groupActive", "groupFocusWithin", "hasHover", "hasActive", "hasFocus", "hasFocusVisible", "disabled", "enabled", "hasDisabled", "hasEnabled"] {
            let error = validate_transitions(&style(&format!(r#"{{ "{name}": {{ "transitionDelay": "soon" }} }}"#)))
                .unwrap_err();
            assert!(error.starts_with(&format!("{name}.transitionDelay")), "{error}");
        }
    }

    #[test]
    fn invalid_values_are_errors() {
        for (json, mentions) in [
            (r#"{ "transition": "backgroundColor 1s" }"#, "kebab-case"),
            (r#"{ "transition": "width 1x" }"#, "not a CSS property"),
            (r#"{ "transition": "width 0" }"#, "not a CSS property"),
            (r#"{ "transition": "width -1s" }"#, "negative"),
            (r#"{ "transition": "width 1s 2s 3s" }"#, "too many"),
            (r#"{ "transition": "width height 1s" }"#, "two properties"),
            (r#"{ "transition": "width 1s ease linear" }"#, "two timing"),
            (r#"{ "transition": "width 1s steps(0)" }"#, "steps"),
            (r#"{ "transition": "none, width 1s" }"#, "none"),
            (r#"{ "transition": "width 1s," }"#, "empty"),
            (r#"{ "transition": "inherit" }"#, "not supported"),
            (r#"{ "transitionProperty": "width, none" }"#, "none"),
            (r#"{ "transitionProperty": "7up" }"#, "not a CSS property"),
            (r#"{ "transitionDuration": "-1s" }"#, "negative"),
            (r#"{ "transitionDuration": "1" }"#, "not a time"),
            (r#"{ "transitionTimingFunction": "fast" }"#, "timing"),
            (r#"{ "transitionDelay": "soon" }"#, "not a time"),
        ] {
            let error = parse_error(json);
            assert!(error.contains(mentions), "{json}: {error}");
        }
    }

    fn behaviors(transitions: &TransitionList) -> Vec<bool> {
        transitions.0.iter().map(|entry| entry.allow_discrete).collect()
    }

    #[test]
    fn the_shorthand_reads_a_behavior_in_any_position() {
        let transitions = list(
            r#"{ "transition": "display 1s allow-discrete, opacity normal .2s, allow-discrete 2s width" }"#,
        );
        assert_eq!(behaviors(&transitions), [true, false, true]);
        assert_eq!(entry(&transitions, 0), (&Matcher::Props(vec![PropKey::Style(AnimProp::Display)]), 1.0, 0.0));
        assert_eq!(entry(&transitions, 1).0, &Matcher::Props(vec![PropKey::Style(AnimProp::Opacity), PropKey::Svg(SvgAttr::Opacity)]));
        assert_eq!(entry(&transitions, 2).0, &Matcher::Props(vec![PropKey::Style(AnimProp::Width), PropKey::Svg(SvgAttr::Width)]));
        assert_eq!(behaviors(&list(r#"{ "transition": "all 1s ALLOW-DISCRETE" }"#)), [true]);
        assert_eq!(behaviors(&list(r#"{ "transition": "all 1s" }"#)), [false]);
        assert_eq!(behaviors(&list(r#"{ "transition": "1s" }"#)), [false]);
    }

    #[test]
    fn the_behavior_longhand_wins_and_repeats_to_the_property_list() {
        let transitions = list(
            r#"{
                "transition": "width 1s allow-discrete",
                "transitionBehavior": "normal"
            }"#,
        );
        assert_eq!(behaviors(&transitions), [false]);
        let transitions = list(
            r#"{
                "transitionProperty": "display, visibility, opacity",
                "transitionDuration": "1s",
                "transitionBehavior": "allow-discrete, normal"
            }"#,
        );
        assert_eq!(behaviors(&transitions), [true, false, true]);
        let transitions = list(
            r#"{
                "transitionProperty": "display",
                "transitionDuration": "1s",
                "transitionBehavior": "normal, allow-discrete"
            }"#,
        );
        assert_eq!(behaviors(&transitions), [false]);
        assert!(list(r#"{ "transition": "none", "transitionBehavior": "allow-discrete" }"#).0.is_empty());
    }

    #[test]
    fn a_bad_behavior_is_an_error() {
        for (json, mentions) in [
            (r#"{ "transitionBehavior": "discrete" }"#, "transitionBehavior"),
            (r#"{ "transitionBehavior": "normal," }"#, "empty"),
            (r#"{ "transitionBehavior": "normal allow-discrete" }"#, "allow-discrete"),
            (r#"{ "transition": "display 1s allow-discrete normal" }"#, "two transition behaviors"),
        ] {
            let error = parse_error(json);
            assert!(error.contains(mentions), "{json}: {error}");
        }
    }

    #[test]
    fn a_behavior_alone_declares_a_transition() {
        assert!(declares_transition(&style(r#"{ "transitionBehavior": "allow-discrete" }"#)));
        assert!(validate_transitions(&style(r#"{ "hover": { "transitionBehavior": "soon" } }"#)).is_err());
    }

    // ── State machine ────────────────────────────────────────────────

    struct Clock {
        origin: Instant,
        transitions: CssTransitions,
        /// The `interpolateSize` an ancestor gives the element.
        inherited: InterpolateSize,
        parent_colors: ColorContext,
        parent_font: ParentValues,
    }

    impl Clock {
        fn new() -> Self {
            Self {
                origin: Instant::now(),
                transitions: CssTransitions::default(),
                inherited: InterpolateSize::default(),
                parent_colors: ColorContext::root(crate::color::Scheme::Light),
                parent_font: ParentValues::initial(),
            }
        }

        fn at(&self, millis: u64) -> Instant {
            self.origin + Duration::from_millis(millis)
        }

        /// One frame at `millis`: the style as laid out, with transitions.
        fn frame(&mut self, millis: u64, after: &StyleDesc) -> StyleDesc {
            self.frame_with(millis, after, None)
        }

        fn frame_with(
            &mut self,
            millis: u64,
            after: &StyleDesc,
            motion: Option<&MotionState>,
        ) -> StyleDesc {
            self.frame_measured(millis, after, motion, None)
        }

        fn frame_measured(
            &mut self,
            millis: u64,
            after: &StyleDesc,
            motion: Option<&MotionState>,
            measurement: Option<&Measurement>,
        ) -> StyleDesc {
            self.frame_of(1, millis, after, motion, measurement)
        }

        fn frame_of(
            &mut self,
            id: u64,
            millis: u64,
            after: &StyleDesc,
            motion: Option<&MotionState>,
            measurement: Option<&Measurement>,
        ) -> StyleDesc {
            let mut resolved = after.clone();
            if let Some(values) =
                self.transitions
                    .update(
                        id,
                        after,
                        None,
                        &self.parent_colors,
                        &self.parent_font,
                        motion,
                        measurement,
                        self.inherited,
                        Scope::Element,
                        &[],
                        self.at(millis),
                    )
            {
                values.apply_to(&mut resolved);
            }
            resolved
        }

        fn events(&mut self) -> Vec<(TransitionEventKind, String, f64)> {
            self.transitions
                .take_events()
                .into_iter()
                .map(|event| (event.kind, event.property_name(), event.elapsed))
                .collect()
        }
    }

    use TransitionEventKind::{Cancel, End, Run, Start};

    fn width_of(style: &StyleDesc) -> f64 {
        match style.width {
            Some(DimensionValue::Pixels(width)) => width,
            ref other => panic!("expected px, got {other:?}"),
        }
    }

    fn sized(width: f64, transition: &str) -> StyleDesc {
        style(&format!(r#"{{ "width": {width}, "transition": "{transition}" }}"#))
    }

    #[test]
    fn a_changed_value_animates_from_the_old_one_and_ends_on_the_new() {
        let mut clock = Clock::new();
        assert_eq!(width_of(&clock.frame(0, &sized(0.0, "width 1s linear"))), 0.0);
        assert!(clock.events().is_empty(), "the first frame never transitions");

        let target = sized(100.0, "width 1s linear");
        assert_eq!(width_of(&clock.frame(100, &target)), 0.0);
        assert_eq!(clock.events(), [(Run, "width".to_string(), 0.0), (Start, "width".to_string(), 0.0)]);
        assert_eq!(width_of(&clock.frame(600, &target)), 50.0);
        assert!(clock.events().is_empty());
        assert!(clock.transitions.take_active());

        assert_eq!(width_of(&clock.frame(1100, &target)), 100.0);
        assert_eq!(clock.events(), [(End, "width".to_string(), 1.0)]);
        assert!(!clock.transitions.take_active());
        assert_eq!(width_of(&clock.frame(1200, &target)), 100.0);
        assert!(clock.events().is_empty(), "a finished transition does not restart");
    }

    #[test]
    fn a_dormant_transition_wakes_for_the_end_of_its_delay_and_its_end_and_a_finished_one_for_nothing() {
        let mut clock = Clock::new();
        clock.frame(0, &sized(0.0, "width 1s linear 300ms"));
        clock.transitions.enter_dormant(1);
        assert_eq!(clock.transitions.dormant_wake(clock.at(0)), Wake::Never, "nothing runs");
        clock.frame(100, &sized(100.0, "width 1s linear 300ms"));
        assert_eq!(clock.transitions.dormant_wake(clock.at(100)), Wake::At(clock.at(400)), "the delay ends first");
        clock.transitions.advance_dormant(clock.at(450));
        assert_eq!(clock.events().iter().filter(|event| event.0 == Start).count(), 1);
        assert_eq!(clock.transitions.dormant_wake(clock.at(450)), Wake::At(clock.at(1400)));
        clock.transitions.leave_dormant(1);
        assert_eq!(clock.transitions.dormant_wake(clock.at(450)), Wake::Never, "an element that left is not asked");
        clock.transitions.enter_dormant(1);
        clock.transitions.advance_dormant(clock.at(1450));
        assert_eq!(clock.events().iter().filter(|event| event.0 == End).count(), 1);
        assert_eq!(clock.transitions.dormant_wake(clock.at(1450)), Wake::Never);
    }

    #[test]
    fn a_dormant_transition_that_is_held_has_no_wake_and_one_that_waits_for_a_frame_asks_for_it() {
        let (mut clock, _target, id) = moving();
        clock.transitions.enter_dormant(1);
        assert_eq!(clock.transitions.dormant_wake(clock.at(100)), Wake::At(clock.at(1100)));
        clock.transitions.command(id, Command::Pause, clock.at(100)).unwrap().unwrap();
        assert_eq!(clock.transitions.dormant_wake(clock.at(100)), Wake::NextFrame, "the pause waits for its frame");
        clock.transitions.advance_dormant(clock.at(100));
        assert_eq!(clock.transitions.dormant_wake(clock.at(100)), Wake::Never);
        clock.transitions.command(id, Command::Play, clock.at(200)).unwrap().unwrap();
        assert_eq!(clock.transitions.dormant_wake(clock.at(200)), Wake::NextFrame);
    }

    #[test]
    fn a_change_without_a_duration_applies_at_once() {
        let mut clock = Clock::new();
        clock.frame(0, &sized(0.0, "width 0s"));
        assert_eq!(width_of(&clock.frame(10, &sized(100.0, "width 0s"))), 100.0);
        clock.frame(20, &sized(0.0, "none"));
        assert_eq!(width_of(&clock.frame(30, &sized(50.0, "none"))), 50.0);
        assert!(clock.events().is_empty());
    }

    #[test]
    fn a_property_the_list_does_not_name_is_not_transitioned() {
        let mut clock = Clock::new();
        clock.frame(0, &sized(0.0, "opacity 1s"));
        assert_eq!(width_of(&clock.frame(10, &sized(100.0, "opacity 1s"))), 100.0);
    }

    #[test]
    fn a_delay_holds_the_start_and_start_waits_for_it() {
        let mut clock = Clock::new();
        clock.frame(0, &sized(0.0, "width 1s linear 500ms"));
        let target = sized(100.0, "width 1s linear 500ms");
        assert_eq!(width_of(&clock.frame(0, &target)), 0.0);
        assert_eq!(clock.events(), [(Run, "width".to_string(), 0.0)]);
        assert_eq!(width_of(&clock.frame(400, &target)), 0.0);
        assert!(clock.events().is_empty());
        assert_eq!(width_of(&clock.frame(1000, &target)), 50.0);
        assert_eq!(clock.events(), [(Start, "width".to_string(), 0.0)]);
        clock.frame(1500, &target);
        assert_eq!(clock.events(), [(End, "width".to_string(), 1.0)]);
    }

    #[test]
    fn a_negative_delay_starts_part_way() {
        let mut clock = Clock::new();
        clock.frame(0, &sized(0.0, "width 1s linear -500ms"));
        let target = sized(100.0, "width 1s linear -500ms");
        assert_eq!(width_of(&clock.frame(0, &target)), 50.0);
        assert_eq!(
            clock.events(),
            [(Run, "width".to_string(), 0.5), (Start, "width".to_string(), 0.5)]
        );
        clock.frame(500, &target);
        assert_eq!(clock.events(), [(End, "width".to_string(), 1.0)]);

        let mut clock = Clock::new();
        clock.frame(0, &sized(0.0, "width 1s -2s"));
        assert_eq!(width_of(&clock.frame(0, &sized(100.0, "width 1s -2s"))), 100.0);
        assert!(clock.events().is_empty(), "a combined duration under zero does not run");
    }

    #[test]
    fn an_interrupt_starts_from_the_visible_value() {
        let mut clock = Clock::new();
        clock.frame(0, &sized(0.0, "width 1s linear"));
        clock.frame(0, &sized(100.0, "width 1s linear"));
        clock.events();
        assert_eq!(width_of(&clock.frame(500, &sized(100.0, "width 1s linear"))), 50.0);

        let retarget = sized(200.0, "width 1s linear");
        assert_eq!(width_of(&clock.frame(500, &retarget)), 50.0);
        assert_eq!(
            clock.events(),
            [
                (Cancel, "width".to_string(), 0.5),
                (Run, "width".to_string(), 0.0),
                (Start, "width".to_string(), 0.0)
            ]
        );
        assert_eq!(width_of(&clock.frame(1000, &retarget)), 125.0);
    }

    #[test]
    fn reversing_a_transition_shortens_the_way_back() {
        let mut clock = Clock::new();
        clock.frame(0, &sized(0.0, "width 1s linear"));
        clock.frame(0, &sized(100.0, "width 1s linear"));
        clock.events();
        clock.frame(250, &sized(100.0, "width 1s linear"));

        let back = sized(0.0, "width 1s linear");
        assert_eq!(width_of(&clock.frame(250, &back)), 25.0);
        clock.events();
        // 25% of the way out, so the way back takes 25% of the duration.
        assert_eq!(width_of(&clock.frame(375, &back)), 12.5);
        assert_eq!(width_of(&clock.frame(500, &back)), 0.0);
        assert_eq!(clock.events(), [(End, "width".to_string(), 0.25)]);
    }

    #[test]
    fn a_second_reversal_compounds_the_shortening() {
        let mut clock = Clock::new();
        let out = sized(100.0, "width 1s linear");
        let back = sized(0.0, "width 1s linear");
        clock.frame(0, &back);
        clock.frame(0, &out);
        clock.frame(500, &back);
        // Reversed half way: 50 -> 0 over 0.5s.
        assert_eq!(width_of(&clock.frame(750, &back)), 25.0);
        // Reversed again a quarter in: the shortening factor is 0.5 * 0.5 + 0.5.
        assert_eq!(width_of(&clock.frame(750, &out)), 25.0);
        // 25 -> 100 over 0.75s.
        assert_eq!(width_of(&clock.frame(1125, &out)), 62.5);
    }

    #[test]
    fn dropping_a_property_from_the_list_cancels_its_transition() {
        let mut clock = Clock::new();
        clock.frame(0, &sized(0.0, "width 1s linear"));
        clock.frame(0, &sized(100.0, "width 1s linear"));
        clock.events();
        let without = sized(100.0, "opacity 1s");
        clock.transitions.take_active();
        assert_eq!(width_of(&clock.frame(500, &without)), 100.0);
        assert_eq!(clock.events(), [(Cancel, "width".to_string(), 0.5)]);
        assert!(!clock.transitions.take_active());
    }

    #[test]
    fn changing_the_transition_leaves_a_running_one_alone() {
        let mut clock = Clock::new();
        clock.frame(0, &sized(0.0, "width 1s linear"));
        clock.frame(0, &sized(100.0, "width 1s linear"));
        clock.events();
        let faster = sized(100.0, "width 100ms ease-in, opacity 1s");
        assert_eq!(width_of(&clock.frame(500, &faster)), 50.0);
        assert!(clock.events().is_empty());
        assert_eq!(width_of(&clock.frame(900, &faster)), 90.0);
    }

    #[test]
    fn the_new_transition_decides_the_next_change() {
        let mut clock = Clock::new();
        clock.frame(0, &sized(0.0, "width 1s linear"));
        clock.frame(0, &sized(100.0, "width 4s linear"));
        assert_eq!(width_of(&clock.frame(2000, &sized(100.0, "width 4s linear"))), 50.0);
    }

    #[test]
    fn adding_a_transition_in_the_same_change_animates_it() {
        let mut clock = Clock::new();
        clock.frame(0, &sized(0.0, "none"));
        clock.frame(0, &sized(100.0, "width 1s linear"));
        assert_eq!(width_of(&clock.frame(500, &sized(100.0, "width 1s linear"))), 50.0);
    }

    fn timed(width: f64, duration: &str) -> StyleDesc {
        style(&format!(
            r#"{{ "width": {width}, "transitionProperty": "width", "transitionDuration": "{duration}", "transitionTimingFunction": "linear" }}"#
        ))
    }

    #[test]
    fn an_element_seeded_with_the_style_before_its_declaration_transitions_the_change_that_adds_it() {
        let mut clock = Clock::new();
        let before = style(r#"{ "width": 0 }"#);
        clock.transitions.seed(1, &before, None, &clock.parent_colors, &clock.parent_font);
        assert_eq!(width_of(&clock.frame(0, &sized(100.0, "width 1s linear"))), 0.0);
        assert_eq!(clock.events(), [(Run, "width".to_string(), 0.0), (Start, "width".to_string(), 0.0)]);
        assert_eq!(width_of(&clock.frame(500, &sized(100.0, "width 1s linear"))), 50.0);
    }

    #[test]
    fn seeding_an_element_already_tracked_leaves_its_state_alone() {
        let mut clock = Clock::new();
        clock.frame(0, &sized(0.0, "width 1s linear"));
        clock.frame(0, &sized(100.0, "width 1s linear"));
        clock.transitions.seed(1, &style(r#"{ "width": 7 }"#), None, &clock.parent_colors, &clock.parent_font);
        assert_eq!(width_of(&clock.frame(500, &sized(100.0, "width 1s linear"))), 50.0);
    }

    #[test]
    fn a_duration_that_arrives_with_the_value_runs() {
        let mut clock = Clock::new();
        clock.frame(0, &timed(0.0, "0s"));
        assert_eq!(width_of(&clock.frame(0, &timed(100.0, "1s"))), 0.0);
        assert_eq!(width_of(&clock.frame(500, &timed(100.0, "1s"))), 50.0);
    }

    #[test]
    fn a_zero_duration_that_arrives_with_the_value_applies_it_at_once_and_cancels_the_running_one() {
        let mut clock = Clock::new();
        clock.frame(0, &timed(0.0, "1s"));
        clock.frame(0, &timed(100.0, "1s"));
        clock.events();
        clock.transitions.take_active();
        assert_eq!(width_of(&clock.frame(500, &timed(200.0, "0s"))), 200.0);
        assert_eq!(clock.events(), [(Cancel, "width".to_string(), 0.5)]);
        assert!(!clock.transitions.take_active());
    }

    #[test]
    fn removing_the_transition_with_the_value_change_applies_it_at_once_and_cancels_the_running_one() {
        let mut clock = Clock::new();
        clock.frame(0, &sized(0.0, "width 1s linear"));
        clock.frame(0, &sized(100.0, "width 1s linear"));
        clock.events();
        assert_eq!(width_of(&clock.frame(500, &sized(200.0, "none"))), 200.0);
        assert_eq!(clock.events(), [(Cancel, "width".to_string(), 0.5)]);

        let mut idle = Clock::new();
        idle.frame(0, &sized(0.0, "width 1s linear"));
        assert_eq!(width_of(&idle.frame(10, &sized(100.0, "none"))), 100.0);
        assert!(idle.events().is_empty(), "no transition starts under the style after the change");
    }

    #[test]
    fn a_duration_change_alone_leaves_a_running_transition_its_own_timing() {
        let mut clock = Clock::new();
        clock.frame(0, &timed(0.0, "1s"));
        clock.frame(0, &timed(100.0, "1s"));
        clock.events();
        assert_eq!(width_of(&clock.frame(500, &timed(100.0, "100ms"))), 50.0);
        assert_eq!(width_of(&clock.frame(900, &timed(100.0, "100ms"))), 90.0);
        assert!(clock.events().is_empty());
        clock.frame(1000, &timed(100.0, "100ms"));
        assert_eq!(clock.events(), [(End, "width".to_string(), 1.0)]);
    }

    #[test]
    fn a_hover_that_carries_the_value_and_the_transition_runs() {
        let base = style(
            r#"{ "width": 0, "hover": { "width": 100, "transition": "width 1s linear" } }"#,
        );
        let mut clock = Clock::new();
        clock.frame(0, &resolve_state_styles(&base, false, false));
        let hovered = resolve_state_styles(&base, true, false);
        assert_eq!(width_of(&clock.frame(0, &hovered)), 0.0);
        assert_eq!(width_of(&clock.frame(500, &hovered)), 50.0);
    }

    #[test]
    fn motion_owned_properties_are_not_transitioned() {
        let now = Instant::now();
        let motion = MotionState::new(
            &serde_json::json!({ "animate": { "width": 100.0 }, "transition": { "duration": 1.0 } }),
            None, Colors::root(),
            None,
            0,
            now,
        )
        .unwrap();
        let mut clock = Clock::new();
        clock.frame_with(0, &sized(0.0, "all 1s linear"), Some(&motion));
        let target = style(r#"{ "width": 100, "opacity": 0.5, "transition": "all 1s linear" }"#);
        let resolved = clock.frame_with(0, &target, Some(&motion));
        assert_eq!(width_of(&resolved), 100.0);
        assert_eq!(resolved.opacity, Some(1.0), "opacity is not motion's, so it runs");
    }

    /// A background that moves from `from` to `to` over a second, linearly, and
    /// the colour it shows after `millis`.
    fn background_at(from: &str, to: &str, millis: u64) -> String {
        let styled = |color: &str| {
            style(&format!(
                r#"{{ "backgroundColor": "{color}", "transition": "background-color 1s linear" }}"#
            ))
        };
        let mut clock = Clock::new();
        clock.frame(0, &styled(from));
        let after = styled(to);
        clock.frame(0, &after);
        clock.frame(millis, &after).background_color.unwrap()
    }

    #[test]
    fn legacy_colours_transition_through_srgb_and_end_exact() {
        let mut clock = Clock::new();
        let red = style(r##"{ "backgroundColor": "#ff0000", "transition": "background-color 1s linear" }"##);
        let blue = style(r##"{ "backgroundColor": "#0000ff", "transition": "background-color 1s linear" }"##);
        clock.frame(0, &red);
        clock.frame(0, &blue);
        near(&clock.frame(500, &blue).background_color.unwrap(), "#800080");
        assert_eq!(clock.frame(1000, &blue).background_color.as_deref(), Some("#0000ff"));
        near(&background_at("red", "blue", 500), "#800080");
        near(&background_at("#ff0000", "hsl(240 100% 50%)", 500), "#800080");
        near(&background_at("rgb(255 0 0)", "hsla(240, 100%, 50%, 1)", 250), "#bf0040");
    }

    #[test]
    fn a_modern_syntax_makes_the_transition_blend_in_oklab() {
        let oklab_middle = "oklab(0.539985 0.096200 -0.092840)";
        near(&background_at("oklab(0.62796 0.22486 0.12585)", "blue", 500), oklab_middle);
        near(&background_at("red", "oklch(0.45201 0.31321 264.052)", 500), oklab_middle);
        near(&background_at("oklch(0.62796 0.25768 29.234)", "oklch(0.45201 0.31321 264.052)", 500), oklab_middle);
        // Black to blue: `currentColor` counts as a modern syntax.
        near(&background_at("currentColor", "blue", 500), "oklab(0.22601 -0.01623 -0.15577)");
    }

    #[test]
    fn transparent_and_alpha_blend_premultiplied_in_srgb() {
        let halfway = crate::color::parse_color_rgba(&background_at("transparent", "#ff0000", 500)).unwrap();
        assert_eq!([halfway.r, halfway.g, halfway.b], [1.0, 0.0, 0.0]);
        assert!((halfway.a - 0.5).abs() < 0.01, "{}", halfway.a);

        let mixed = crate::color::parse_color_rgba(&background_at(
            "rgba(255, 0, 0, 0.2)",
            "rgba(0, 0, 255, 0.8)",
            500,
        ))
        .unwrap();
        near(&format!("rgb({} {} {})", mixed.r * 255.0, mixed.g * 255.0, mixed.b * 255.0), "#3300cc");
        assert!((mixed.a - 0.5).abs() < 0.01, "{}", mixed.a);
    }

    #[test]
    fn an_interrupted_legacy_transition_continues_in_srgb() {
        let styled = |color: &str| {
            style(&format!(
                r#"{{ "backgroundColor": "{color}", "transition": "background-color 1s linear" }}"#
            ))
        };
        let mut clock = Clock::new();
        clock.frame(0, &styled("#ff0000"));
        clock.frame(0, &styled("#0000ff"));
        near(&clock.frame(500, &styled("#0000ff")).background_color.unwrap(), "#800080");
        // Back to green from the purple it reached: sRGB again, not Oklab.
        clock.frame(500, &styled("#00ff00"));
        near(&clock.frame(1000, &styled("#00ff00")).background_color.unwrap(), "#408040");
    }

    fn near(text: &str, expected: &str) {
        let rgba = |value: &str| crate::color::parse_color_rgba(value).unwrap();
        let (actual, expected) = (rgba(text), rgba(expected));
        for (actual, expected) in [actual.r, actual.g, actual.b].into_iter().zip([expected.r, expected.g, expected.b]) {
            assert!((actual - expected).abs() < 0.01, "{text} vs {expected}");
        }
    }

    /// `backgroundColor` over one second, in a scheme that follows the window.
    fn tinted_by_appearance(background: &str) -> StyleDesc {
        style(&format!(
            r#"{{ "colorScheme": "light dark", "backgroundColor": "{background}", "transition": "background-color 1s linear" }}"#
        ))
    }

    fn dark_window() -> ColorContext {
        ColorContext::root(crate::color::Scheme::Dark)
    }

    #[test]
    fn a_new_color_moves_current_colour_without_a_transition_of_its_own() {
        let tinted = |color: &str| {
            style(&format!(
                r#"{{ "color": "{color}", "backgroundColor": "currentColor", "borderColor": "currentColor", "transition": "background-color 1s linear, border-color 1s linear" }}"#
            ))
        };
        let mut clock = Clock::new();
        clock.frame(0, &tinted("#ff0000"));
        // `currentColor` computes to itself, so its computed value did not change.
        let after = clock.frame(0, &tinted("#0000ff"));
        assert_eq!(after.background_color.as_deref(), Some("currentColor"));
        assert_eq!(after.border_color.as_deref(), Some("currentColor"));
        assert!(clock.events().is_empty());
        assert!(clock.transitions.states.get(&1).unwrap().running.is_empty());

        // Nor does the parent's colour, which an element with no `color` of its own inherits.
        let mut clock = Clock::new();
        let bordered = style(r#"{ "backgroundColor": "currentColor", "borderColor": "currentColor", "transition": "all 1s linear" }"#);
        let parent = |color: &str| ColorContext::root(crate::color::Scheme::Light).descend(&style(&format!(r#"{{ "color": "{color}" }}"#)));
        clock.parent_colors = parent("#ff0000");
        clock.frame(0, &bordered);
        clock.parent_colors = parent("#0000ff");
        clock.frame(0, &bordered);
        assert!(clock.events().is_empty());
        assert!(clock.transitions.states.get(&1).unwrap().running.is_empty());
    }

    #[test]
    fn a_colour_that_replaces_current_colour_still_transitions() {
        let rgba = |value: &str| crate::color::parse_color_rgba(value).unwrap();
        let mut clock = Clock::new();
        let to = |background: &str| {
            style(&format!(
                r##"{{ "color": "#0000ff", "backgroundColor": "{background}", "transition": "background-color 1s linear" }}"##
            ))
        };
        clock.frame(0, &to("#ff0000"));
        clock.frame(0, &to("currentColor"));
        assert_eq!(clock.events(), [(Run, "background-color".to_string(), 0.0), (Start, "background-color".to_string(), 0.0)]);
        let middle = clock.frame(500, &to("currentColor")).background_color.unwrap();
        assert!(rgba(&middle).r > 0.2 && rgba(&middle).b > 0.2, "{middle}");
    }

    #[test]
    fn light_dark_follows_a_parents_scheme_through_a_transition() {
        let rgba = |value: &str| crate::color::parse_color_rgba(value).unwrap();
        let mut clock = Clock::new();
        let themed = |scheme: &str| {
            style(&format!(
                r#"{{ "colorScheme": "{scheme}", "backgroundColor": "light-dark(#ff0000, #0000ff)", "transition": "background-color 1s linear" }}"#
            ))
        };
        clock.frame(0, &themed("light"));
        clock.frame(0, &themed("dark"));
        let middle = clock.frame(500, &themed("dark")).background_color.unwrap();
        assert!(rgba(&middle).r > 0.2 && rgba(&middle).b > 0.2, "{middle}");
    }

    #[test]
    fn a_scheme_flip_starts_a_transition_on_an_idle_element() {
        let rgba = |value: &str| crate::color::parse_color_rgba(value).unwrap();
        let mut clock = Clock::new();
        let following = tinted_by_appearance("light-dark(#ff0000, #0000ff)");
        clock.frame(0, &following);
        clock.parent_colors = dark_window();
        let start = clock.frame(0, &following).background_color.unwrap();
        near(&start, "#ff0000");
        let name = || "background-color".to_string();
        assert_eq!(clock.events(), [(Run, name(), 0.0), (Start, name(), 0.0)]);
        let middle = clock.frame(500, &following).background_color.unwrap();
        assert!(rgba(&middle).r > 0.2 && rgba(&middle).b > 0.2, "{middle}");
        assert_eq!(clock.frame(1000, &following).background_color.as_deref(), Some("light-dark(#ff0000, #0000ff)"));
        assert_eq!(clock.events(), [(End, name(), 1.0)]);

        // And back: the finished transition does not hold the flip.
        clock.parent_colors = ColorContext::root(crate::color::Scheme::Light);
        clock.frame(1000, &following);
        assert_eq!(clock.events(), [(Run, name(), 0.0), (Start, name(), 0.0)]);
    }

    #[test]
    fn a_flip_that_changes_the_end_of_a_running_transition_restarts_it_from_the_current_value() {
        let rgba = |value: &str| crate::color::parse_color_rgba(value).unwrap();
        let name = || "background-color".to_string();
        let mut clock = Clock::new();
        // Red runs to green in a light window; dark makes the end blue.
        clock.frame(0, &tinted_by_appearance("#ff0000"));
        let target = tinted_by_appearance("light-dark(#00ff00, #0000ff)");
        clock.frame(0, &target);
        clock.events();
        clock.frame(500, &target);
        clock.parent_colors = dark_window();
        let restarted = clock.frame(500, &target).background_color.unwrap();
        let (green, red) = (rgba("#00ff00"), rgba("#ff0000"));
        assert!(rgba(&restarted).g < green.g && rgba(&restarted).r < red.r, "{restarted}");
        assert_eq!(clock.events(), [(Cancel, name(), 0.5), (Run, name(), 0.0), (Start, name(), 0.0)]);
        let state = clock.transitions.states.get(&1).unwrap();
        assert_eq!(state.running.len(), 1);
        clock.frame(1500, &target);
        assert_eq!(clock.events(), [(End, name(), 1.0)]);
    }

    #[test]
    fn a_flip_back_to_the_reversing_start_shortens_the_new_transition() {
        let name = || "background-color".to_string();
        let mut clock = Clock::new();
        // Red runs to green; in a dark window the end is red again.
        clock.frame(0, &tinted_by_appearance("#ff0000"));
        let target = tinted_by_appearance("light-dark(#00ff00, #ff0000)");
        clock.frame(0, &target);
        clock.events();
        clock.parent_colors = dark_window();
        clock.frame(500, &target);
        assert_eq!(clock.events(), [(Cancel, name(), 0.5), (Run, name(), 0.0), (Start, name(), 0.0)]);
        // Half way out, so half the duration back.
        clock.frame(1000, &target);
        assert_eq!(clock.events(), [(End, name(), 0.5)]);
    }

    #[test]
    fn a_flip_that_leaves_the_end_alone_does_not_touch_a_running_transition() {
        let name = || "background-color".to_string();
        let run = |flip: bool| {
            let mut clock = Clock::new();
            clock.frame(0, &tinted_by_appearance("light-dark(#ff0000, #0000ff)"));
            let target = tinted_by_appearance("#00ff00");
            clock.frame(0, &target);
            clock.events();
            clock.frame(500, &target);
            if flip {
                clock.parent_colors = dark_window();
            }
            let middle = clock.frame(500, &target).background_color;
            let events = clock.events();
            let later = clock.frame(750, &target).background_color;
            (middle, events, later, clock)
        };
        let (middle, events, later, mut clock) = run(true);
        let (control_middle, _, control_later, _) = run(false);
        assert!(events.is_empty(), "{events:?}");
        assert_eq!((middle, later), (control_middle, control_later));
        clock.frame(1000, &tinted_by_appearance("#00ff00"));
        assert_eq!(clock.events(), [(End, name(), 1.0)]);
    }

    #[test]
    fn a_px_to_percent_change_transitions_at_once_and_an_auto_change_waits_for_layout() {
        let mut clock = Clock::new();
        clock.frame(0, &style(r#"{ "width": 10, "transition": "all 1s linear" }"#));
        let percent = style(r#"{ "width": "50%", "transition": "all 1s linear" }"#);
        assert_eq!(clock.frame(10, &percent).width, Some(DimensionValue::Pixels(10.0)));
        assert_eq!(clock.events().len(), 2, "run and start");
        assert_eq!(
            clock.frame(510, &percent).width,
            Some(DimensionValue::Calc(LinearLength { px: 5.0, percent: 0.25, ..LinearLength::default() }))
        );
        let auto = style(r#"{ "width": "auto", "transition": "all 1s linear" }"#);
        assert_eq!(clock.frame(20, &auto).width, Some(DimensionValue::Auto));
    }

    #[test]
    fn an_unset_value_transitions_from_the_css_initial_value() {
        let mut clock = Clock::new();
        clock.frame(0, &style(r#"{ "transition": "opacity 1s linear, padding-left 1s linear" }"#));
        let target = style(
            r#"{ "opacity": 0, "paddingLeft": 10, "transition": "opacity 1s linear, padding-left 1s linear" }"#,
        );
        clock.frame(0, &target);
        let middle = clock.frame(500, &target);
        assert_eq!(middle.opacity, Some(0.5));
        assert_eq!(middle.padding_left, Some(LinearLength::px(5.0)));
    }

    #[test]
    fn an_overshooting_easing_stays_in_the_property_range() {
        let mut clock = Clock::new();
        let back = "opacity 1s cubic-bezier(0.34, 1.56, 0.64, 1)";
        clock.frame(0, &style(&format!(r#"{{ "opacity": 0, "transition": "{back}" }}"#)));
        let target = style(&format!(r#"{{ "opacity": 1, "transition": "{back}" }}"#));
        clock.frame(0, &target);
        assert_eq!(clock.frame(600, &target).opacity, Some(1.0));
    }

    #[test]
    fn hover_transitions_in_with_the_hover_transition_and_out_with_the_base_one() {
        let base = style(
            r##"{
                "backgroundColor": "#000000", "transition": "background-color 1s linear",
                "hover": { "backgroundColor": "#ffffff", "transition": "background-color 100ms linear" }
            }"##,
        );
        let mut clock = Clock::new();
        let rest = resolve_state_styles(&base, false, false);
        let hovered = resolve_state_styles(&base, true, false);
        clock.frame(0, &rest);
        clock.frame(0, &hovered);
        assert_eq!(clock.frame(100, &hovered).background_color.as_deref(), Some("#ffffff"));
        clock.events();
        clock.frame(200, &rest);
        let leaving = clock.frame(700, &rest).background_color.unwrap();
        assert_ne!(leaving, "#ffffff");
        assert_ne!(leaving, "#000000");
        assert_eq!(clock.frame(1200, &rest).background_color.as_deref(), Some("#000000"));
    }

    #[test]
    fn wants_follows_declared_transitions_in_any_state_style() {
        let mut clock = CssTransitions::default();
        assert!(!clock.wants(1, None));
        assert!(!clock.wants(1, Some(&style(r#"{ "width": 1 }"#))));
        assert!(!clock.wants(1, Some(&style(r#"{ "transition": "none" }"#))));
        assert!(clock.wants(1, Some(&style(r#"{ "transition": "width 1s" }"#))));
        assert!(clock.wants(1, Some(&style(r#"{ "hover": { "transitionDuration": "1s" } }"#))));
        assert!(clock.wants(1, Some(&style(r#"{ "focusVisible": { "transition": "all 1s" } }"#))));
        assert!(clock.wants(1, Some(&style(r#"{ "groupHover": { "transition": "all 1s" } }"#))));
        assert!(clock.wants(1, Some(&style(r#"{ "groupActive": { "transitionDuration": "1s" } }"#))));
        assert!(clock.wants(1, Some(&style(r#"{ "groupFocusWithin": { "transition": "all 1s" } }"#))));
        for name in ["hasHover", "hasActive", "hasFocus", "hasFocusVisible", "disabled", "enabled", "hasDisabled", "hasEnabled"] {
            assert!(clock.wants(1, Some(&style(&format!(r#"{{ "{name}": {{ "transition": "all 1s" }} }}"#)))), "{name}");
        }
    }

    #[test]
    fn a_transition_declared_only_in_has_hover_runs() {
        use crate::events::state_styles::{resolve_pointer_states, PointerStates};
        let base = style(
            r##"{
                "backgroundColor": "#000000",
                "hasHover": { "backgroundColor": "#ffffff", "transition": "background-color 1s linear" }
            }"##,
        );
        let mut transitions = CssTransitions::default();
        assert!(transitions.wants(1, Some(&base)));
        let mut clock = Clock::new();
        let rest = resolve_pointer_states(&base, PointerStates::default());
        let hovered = resolve_pointer_states(&base, PointerStates { has_hovered: true, ..Default::default() });
        clock.frame(0, &rest);
        clock.frame(0, &hovered);
        let halfway = clock.frame(500, &hovered).background_color.unwrap();
        assert_ne!(halfway, "#ffffff");
        assert_ne!(halfway, "#000000");
        assert_eq!(clock.frame(1000, &hovered).background_color.as_deref(), Some("#ffffff"));
    }

    #[test]
    fn a_transition_declared_only_in_disabled_runs() {
        use crate::events::state_styles::{resolve_pointer_states, PointerStates};
        let base = style(
            r##"{
                "backgroundColor": "#000000",
                "disabled": { "backgroundColor": "#ffffff", "transition": "background-color 1s linear" }
            }"##,
        );
        let mut transitions = CssTransitions::default();
        assert!(transitions.wants(1, Some(&base)));
        let mut clock = Clock::new();
        let rest = resolve_pointer_states(&base, PointerStates::default());
        let disabled = resolve_pointer_states(&base, PointerStates { disabled: true, ..Default::default() });
        clock.frame(0, &rest);
        clock.frame(0, &disabled);
        let halfway = clock.frame(500, &disabled).background_color.unwrap();
        assert_ne!(halfway, "#ffffff");
        assert_ne!(halfway, "#000000");
        assert_eq!(clock.frame(1000, &disabled).background_color.as_deref(), Some("#ffffff"));
    }

    #[test]
    fn a_transition_declared_only_in_group_hover_runs() {
        use crate::events::state_styles::{resolve_pointer_states, PointerStates};
        let base = style(
            r##"{
                "backgroundColor": "#000000",
                "groupHover": { "backgroundColor": "#ffffff", "transition": "background-color 1s linear" }
            }"##,
        );
        let mut transitions = CssTransitions::default();
        assert!(transitions.wants(1, Some(&base)));
        let mut clock = Clock::new();
        let rest = resolve_pointer_states(&base, PointerStates::default());
        let hovered = resolve_pointer_states(&base, PointerStates { group_hovered: true, ..Default::default() });
        clock.frame(0, &rest);
        clock.frame(0, &hovered);
        let halfway = clock.frame(500, &hovered).background_color.unwrap();
        assert_ne!(halfway, "#ffffff");
        assert_ne!(halfway, "#000000");
        assert_eq!(clock.frame(1000, &hovered).background_color.as_deref(), Some("#ffffff"));
    }

    #[test]
    fn a_transition_declared_only_in_group_focus_within_runs() {
        use crate::events::state_styles::{resolve_pointer_states, PointerStates};
        let base = style(
            r##"{
                "backgroundColor": "#000000",
                "groupFocusWithin": { "backgroundColor": "#ffffff", "transition": "background-color 1s linear" }
            }"##,
        );
        let mut transitions = CssTransitions::default();
        assert!(transitions.wants(1, Some(&base)));
        let mut clock = Clock::new();
        let rest = resolve_pointer_states(&base, PointerStates::default());
        let focused = resolve_pointer_states(&base, PointerStates { group_focused_within: true, ..Default::default() });
        clock.frame(0, &rest);
        clock.frame(0, &focused);
        let halfway = clock.frame(500, &focused).background_color.unwrap();
        assert_ne!(halfway, "#ffffff");
        assert_ne!(halfway, "#000000");
        assert_eq!(clock.frame(1000, &focused).background_color.as_deref(), Some("#ffffff"));
    }

    fn hidden_pair() -> RetainedTree {
        let mut tree = RetainedTree::new();
        tree.create_element(1, "div".to_string());
        tree.create_element(2, "div".to_string());
        tree.append_child(1, 2);
        tree
    }

    #[test]
    fn display_none_cancels_the_transitions_of_the_element_and_its_descendants() {
        let tree = hidden_pair();
        let mut clock = Clock::new();
        let (start, target) = (sized(0.0, "width 1s linear"), sized(100.0, "width 1s linear"));
        for id in [1, 2] {
            clock.frame_of(id, 0, &start, None, None);
            clock.frame_of(id, 100, &target, None, None);
        }
        clock.events();
        assert_eq!(width_of(&clock.frame_of(2, 600, &target, None, None)), 50.0);
        clock.events();

        clock.transitions.hide(&tree, 1, clock.at(600));
        assert_eq!(clock.events(), [(Cancel, "width".to_string(), 0.5), (Cancel, "width".to_string(), 0.5)]);
        // Hidden frames after the first change nothing.
        clock.transitions.hide(&tree, 1, clock.at(616));
        assert!(clock.events().is_empty());
        assert!(!clock.transitions.wants(1, None), "no state is left to settle");
        assert!(!clock.transitions.wants(2, None));
    }

    #[test]
    fn a_style_changed_while_hidden_does_not_transition_when_it_shows_again() {
        let tree = hidden_pair();
        let mut clock = Clock::new();
        clock.frame(0, &sized(0.0, "width 1s linear"));
        clock.transitions.hide(&tree, 1, clock.at(100));
        // Changed while `display: none`, then shown with the new value.
        clock.transitions.show(1);
        let target = sized(100.0, "width 1s linear");
        assert_eq!(width_of(&clock.frame(500, &target)), 100.0);
        assert!(clock.events().is_empty());
        // It transitions normally from then on.
        assert_eq!(width_of(&clock.frame(600, &sized(0.0, "width 1s linear"))), 100.0);
        assert_eq!(width_of(&clock.frame(1100, &sized(0.0, "width 1s linear"))), 50.0);
    }

    #[test]
    fn an_element_that_hides_again_after_showing_is_cancelled_again() {
        let tree = hidden_pair();
        let mut clock = Clock::new();
        clock.frame(0, &sized(0.0, "width 1s linear"));
        clock.transitions.hide(&tree, 1, clock.at(0));
        clock.transitions.show(1);
        clock.frame(100, &sized(0.0, "width 1s linear"));
        clock.frame(200, &sized(100.0, "width 1s linear"));
        clock.events();
        clock.transitions.hide(&tree, 1, clock.at(700));
        assert_eq!(clock.events(), [(Cancel, "width".to_string(), 0.5)]);
    }

    #[test]
    fn an_invalid_transition_is_no_transition() {
        let mut clock = Clock::new();
        clock.frame(0, &sized(0.0, "width bogus"));
        assert_eq!(width_of(&clock.frame(10, &sized(100.0, "width bogus"))), 100.0);
    }

    #[test]
    fn retain_drops_state_for_removed_elements() {
        let mut clock = Clock::new();
        clock.frame(0, &sized(0.0, "width 1s"));
        clock.frame(0, &sized(100.0, "width 1s"));
        assert!(clock.transitions.wants(1, None));
        clock.transitions.retain(|_| false);
        assert!(clock.transitions.states.is_empty());
    }

    #[test]
    fn removing_the_declaration_cancels_what_runs_and_then_drops_the_state() {
        let mut clock = Clock::new();
        clock.frame(0, &sized(0.0, "width 1s linear"));
        clock.frame(0, &sized(100.0, "width 1s linear"));
        clock.events();

        let plain = style(r#"{ "width": 100 }"#);
        assert!(clock.transitions.wants(1, Some(&plain)), "the running transition still has to be cancelled");
        assert_eq!(width_of(&clock.frame(500, &plain)), 100.0);
        assert_eq!(clock.events(), [(Cancel, "width".to_string(), 0.5)]);

        assert!(!clock.transitions.wants(1, Some(&plain)));
        assert!(clock.transitions.states.is_empty());
    }

    #[test]
    fn a_finished_transition_does_not_keep_state_once_the_declaration_is_gone() {
        let mut clock = Clock::new();
        clock.frame(0, &sized(0.0, "width 1s linear"));
        clock.frame(0, &sized(100.0, "width 1s linear"));
        clock.frame(1000, &sized(100.0, "width 1s linear"));
        assert_eq!(clock.events().last(), Some(&(End, "width".to_string(), 1.0)));

        let plain = style(r#"{ "width": 100 }"#);
        assert!(clock.transitions.wants(1, Some(&plain)));
        clock.frame(1100, &plain);
        assert!(clock.events().is_empty(), "an ended transition is not cancelled");
        assert!(!clock.transitions.wants(1, Some(&plain)));
    }

    #[test]
    fn a_transition_declared_in_a_state_style_keeps_its_state_while_the_element_rests() {
        let base = style(
            r##"{ "width": 0, "hover": { "width": 100, "transition": "width 1s linear" } }"##,
        );
        let mut clock = Clock::new();
        let rest = resolve_state_styles(&base, false, false);
        clock.frame(0, &rest);
        assert!(clock.transitions.wants(1, Some(&base)));
        assert!(!clock.transitions.states.is_empty());
    }

    mod measured {
        use super::*;

        fn measurement(height: f32, natural: f32) -> Measurement {
            Measurement {
                border_box: gpui::size(gpui::px(100.0), gpui::px(height)),
                natural_height: gpui::px(natural),
                natural_width: None,
                parent_content: gpui::size(gpui::px(800.0), gpui::px(600.0)),
                scale_factor: 1.0,
                percent_height_resolves: true,
                flex_main: crate::motion::property::Axis::Width,
            }
        }

        fn closed() -> StyleDesc {
            style(r#"{ "height": 0, "interpolateSize": "allow-keywords", "transition": "height 1s linear" }"#)
        }

        fn auto() -> StyleDesc {
            style(r#"{ "interpolateSize": "allow-keywords", "transition": "height 1s linear" }"#)
        }

        fn height_of(style: &StyleDesc) -> f64 {
            match style.height {
                Some(DimensionValue::Pixels(height)) => height,
                ref other => panic!("expected px, got {other:?}"),
            }
        }

        #[test]
        fn a_height_runs_to_the_natural_height_and_ends_on_auto() {
            let mut clock = Clock::new();
            let measured = measurement(0.0, 80.2);
            clock.frame_measured(0, &closed(), None, Some(&measured));

            let started = clock.frame_measured(100, &auto(), None, Some(&measured));
            assert_eq!(height_of(&started), 0.0);
            assert_eq!(
                clock.events(),
                [(Run, "height".to_string(), 0.0), (Start, "height".to_string(), 0.0)]
            );
            // The target is the natural height rounded up to the device pixel.
            let middle = clock.frame_measured(600, &auto(), None, Some(&measured));
            assert_eq!(height_of(&middle), 40.5);
            assert!(clock.transitions.take_active());

            let ended = clock.frame_measured(1100, &auto(), None, Some(&measured));
            assert_eq!(ended.height, None, "the style is literal auto again");
            assert_eq!(clock.events(), [(End, "height".to_string(), 1.0)]);
            assert!(!clock.transitions.take_active());
            assert_eq!(clock.frame_measured(1200, &auto(), None, Some(&measured)).height, None);
            assert!(clock.events().is_empty(), "a finished transition does not restart");
        }

        #[test]
        fn leaving_auto_starts_from_the_laid_out_height() {
            let mut clock = Clock::new();
            let measured = measurement(80.0, 80.0);
            clock.frame_measured(0, &auto(), None, Some(&measured));

            let target = style(r#"{ "height": 100, "interpolateSize": "allow-keywords", "transition": "height 1s linear" }"#);
            assert_eq!(height_of(&clock.frame_measured(100, &target, None, Some(&measured))), 80.0);
            assert_eq!(height_of(&clock.frame_measured(600, &target, None, Some(&measured))), 90.0);
            assert_eq!(height_of(&clock.frame_measured(1100, &target, None, Some(&measured))), 100.0);
        }

        #[test]
        fn the_end_follows_the_content_without_moving_the_value_or_the_schedule() {
            let mut clock = Clock::new();
            clock.frame_measured(0, &closed(), None, Some(&measurement(0.0, 80.0)));
            clock.frame_measured(100, &auto(), None, Some(&measurement(0.0, 80.0)));
            clock.events();

            let grown = measurement(40.0, 120.0);
            assert_eq!(height_of(&clock.frame_measured(600, &auto(), None, Some(&grown))), 40.0);
            assert_eq!(height_of(&clock.frame_measured(850, &auto(), None, Some(&grown))), 80.0);
            assert!(clock.events().is_empty(), "following is not a new transition");

            let ended = clock.frame_measured(1100, &auto(), None, Some(&grown));
            assert_eq!(ended.height, None);
            assert_eq!(clock.events(), [(End, "height".to_string(), 1.0)]);
        }

        #[test]
        fn interrupting_a_pixel_transition_toward_auto_runs_to_the_natural_height() {
            let mut clock = Clock::new();
            let measured = measurement(50.0, 80.0);
            let tall = style(r#"{ "height": 100, "interpolateSize": "allow-keywords", "transition": "height 1s linear" }"#);
            clock.frame_measured(0, &closed(), None, Some(&measured));
            clock.frame_measured(0, &tall, None, Some(&measured));
            assert_eq!(height_of(&clock.frame_measured(500, &tall, None, Some(&measured))), 50.0);

            assert_eq!(height_of(&clock.frame_measured(500, &auto(), None, Some(&measured))), 50.0);
            assert_eq!(height_of(&clock.frame_measured(1000, &auto(), None, Some(&measured))), 65.0);
            assert_eq!(clock.frame_measured(1500, &auto(), None, Some(&measured)).height, None);
        }

        #[test]
        fn without_a_measurement_the_change_is_instant() {
            let mut clock = Clock::new();
            clock.frame(0, &closed());
            assert_eq!(clock.frame(100, &auto()).height, None);
            assert!(clock.events().is_empty());
            clock.frame(200, &auto());
            let target = style(r#"{ "height": 100, "interpolateSize": "allow-keywords", "transition": "height 1s linear" }"#);
            assert_eq!(height_of(&clock.frame(300, &target)), 100.0);
        }

        /// An element `width` wide that would be `natural` wide with `width: auto`.
        fn wide(width: f32, natural: f32) -> Measurement {
            Measurement {
                natural_width: Some(gpui::px(natural)),
                border_box: gpui::size(gpui::px(width), gpui::px(10.0)),
                ..measurement(10.0, 10.0)
            }
        }

        fn narrow() -> StyleDesc {
            style(r#"{ "width": 0, "interpolateSize": "allow-keywords", "transition": "width 1s linear" }"#)
        }

        fn auto_width() -> StyleDesc {
            style(r#"{ "interpolateSize": "allow-keywords", "transition": "width 1s linear" }"#)
        }

        #[test]
        fn a_width_runs_to_the_natural_width_and_ends_on_auto() {
            let mut clock = Clock::new();
            let measured = wide(0.0, 120.2);
            clock.frame_measured(0, &narrow(), None, Some(&measured));

            let started = clock.frame_measured(100, &auto_width(), None, Some(&measured));
            assert_eq!(width_of(&started), 0.0);
            assert_eq!(
                clock.events(),
                [(Run, "width".to_string(), 0.0), (Start, "width".to_string(), 0.0)]
            );
            // The target is the natural width rounded up to the device pixel.
            assert_eq!(width_of(&clock.frame_measured(600, &auto_width(), None, Some(&measured))), 60.5);
            assert!(clock.transitions.take_active());

            let ended = clock.frame_measured(1100, &auto_width(), None, Some(&measured));
            assert_eq!(ended.width, None, "the style is literal auto again");
            assert_eq!(clock.events(), [(End, "width".to_string(), 1.0)]);
            assert!(!clock.transitions.take_active());
        }

        #[test]
        fn leaving_a_width_of_auto_starts_from_the_laid_out_width() {
            let mut clock = Clock::new();
            let measured = wide(80.0, 80.0);
            clock.frame_measured(0, &auto_width(), None, Some(&measured));

            let target = style(r#"{ "width": 100, "interpolateSize": "allow-keywords", "transition": "width 1s linear" }"#);
            assert_eq!(width_of(&clock.frame_measured(100, &target, None, Some(&measured))), 80.0);
            assert_eq!(width_of(&clock.frame_measured(600, &target, None, Some(&measured))), 90.0);
            assert_eq!(width_of(&clock.frame_measured(1100, &target, None, Some(&measured))), 100.0);
        }

        #[test]
        fn the_width_end_follows_the_content_without_moving_the_value_or_the_schedule() {
            let mut clock = Clock::new();
            clock.frame_measured(0, &narrow(), None, Some(&wide(0.0, 80.0)));
            clock.frame_measured(100, &auto_width(), None, Some(&wide(0.0, 80.0)));
            clock.events();

            let grown = wide(40.0, 120.0);
            assert_eq!(width_of(&clock.frame_measured(600, &auto_width(), None, Some(&grown))), 40.0);
            assert_eq!(width_of(&clock.frame_measured(850, &auto_width(), None, Some(&grown))), 80.0);
            assert!(clock.events().is_empty(), "following is not a new transition");
        }

        #[test]
        fn without_the_natural_width_the_change_is_instant() {
            let mut clock = Clock::new();
            // Laid out, but by a frame that did not measure the width.
            let unmeasured = measurement(10.0, 10.0);
            clock.frame_measured(0, &narrow(), None, Some(&unmeasured));
            assert_eq!(clock.frame_measured(100, &auto_width(), None, Some(&unmeasured)).width, None);
            assert!(clock.events().is_empty());
        }

        #[test]
        fn width_to_or_from_auto_is_instant_unless_keywords_are_allowed() {
            let mut clock = Clock::new();
            let measured = wide(10.0, 10.0);
            let sized = style(r#"{ "width": 50, "transition": "width 1s linear" }"#);
            let unset = style(r#"{ "transition": "width 1s linear" }"#);
            clock.frame_measured(0, &sized, None, Some(&measured));
            assert_eq!(clock.frame_measured(100, &unset, None, Some(&measured)).width, None);
            assert_eq!(width_of(&clock.frame_measured(200, &sized, None, Some(&measured))), 50.0);
            assert!(clock.events().is_empty());
        }

        #[test]
        fn only_a_transition_that_covers_width_asks_for_the_natural_width() {
            assert!(transitions_width(&style(r#"{ "transition": "width 1s" }"#)));
            assert!(transitions_width(&style(r#"{ "transition": "all 1s" }"#)));
            assert!(transitions_width(&style(r#"{ "transitionProperty": "opacity, width" }"#)));
            assert!(!transitions_width(&style(r#"{ "transition": "opacity 1s" }"#)));
            assert!(!transitions_width(&style(r#"{ "transition": "height 1s" }"#)));
            assert!(!transitions_width(&style(r#"{ "transition": "none" }"#)));
            assert!(!transitions_width(&style(r#"{ "width": 10 }"#)));
        }

        #[test]
        fn an_unset_max_height_starts_from_the_laid_out_height() {
            let mut clock = Clock::new();
            let measured = measurement(80.0, 80.0);
            let unset = style(r#"{ "interpolateSize": "allow-keywords", "transition": "max-height 1s linear" }"#);
            let capped = style(r#"{ "maxHeight": 0, "interpolateSize": "allow-keywords", "transition": "max-height 1s linear" }"#);
            clock.frame_measured(0, &unset, None, Some(&measured));
            let started = clock.frame_measured(100, &capped, None, Some(&measured));
            assert_eq!(started.max_height, Some(DimensionValue::Pixels(80.0)));
            let middle = clock.frame_measured(600, &capped, None, Some(&measured));
            assert_eq!(middle.max_height, Some(DimensionValue::Pixels(40.0)));
        }

        fn numeric_only(declared: &str) -> StyleDesc {
            style(&format!(r#"{{ {declared} "transition": "height 1s linear" }}"#))
        }

        #[test]
        fn a_keyword_change_is_discrete_by_default() {
            let mut clock = Clock::new();
            let measured = measurement(0.0, 80.0);
            let closed = numeric_only(r#""height": 0,"#);
            let open = numeric_only("");
            clock.frame_measured(0, &closed, None, Some(&measured));
            assert_eq!(clock.frame_measured(100, &open, None, Some(&measured)).height, None);
            assert_eq!(clock.frame_measured(600, &open, None, Some(&measured)).height, None);
            assert!(clock.events().is_empty());

            let measured = measurement(80.0, 80.0);
            assert_eq!(
                height_of(&clock.frame_measured(700, &closed, None, Some(&measured))),
                0.0
            );
            assert!(clock.events().is_empty(), "leaving auto is discrete too");
        }

        #[test]
        fn an_unset_max_height_is_a_keyword() {
            let mut clock = Clock::new();
            let measured = measurement(80.0, 80.0);
            let unset = style(r#"{ "transition": "max-height 1s linear" }"#);
            let capped = style(r#"{ "maxHeight": 0, "transition": "max-height 1s linear" }"#);
            clock.frame_measured(0, &unset, None, Some(&measured));
            let started = clock.frame_measured(100, &capped, None, Some(&measured));
            assert_eq!(started.max_height, Some(DimensionValue::Pixels(0.0)));
            assert!(clock.events().is_empty());
        }

        #[test]
        fn lengths_still_transition_by_default() {
            let mut clock = Clock::new();
            let measured = measurement(0.0, 80.0);
            clock.frame_measured(0, &numeric_only(r#""height": 0,"#), None, Some(&measured));
            let tall = numeric_only(r#""height": 100,"#);
            clock.frame_measured(0, &tall, None, Some(&measured));
            assert_eq!(height_of(&clock.frame_measured(500, &tall, None, Some(&measured))), 50.0);
        }

        #[test]
        fn interpolate_size_is_inherited_and_the_element_overrides_it() {
            let measured = measurement(0.0, 80.0);
            let closed = numeric_only(r#""height": 0,"#);

            let mut clock = Clock::new();
            clock.inherited = InterpolateSize::AllowKeywords;
            clock.frame_measured(0, &closed, None, Some(&measured));
            clock.frame_measured(100, &numeric_only(""), None, Some(&measured));
            let middle = clock.frame_measured(600, &numeric_only(""), None, Some(&measured));
            assert_eq!(height_of(&middle), 40.0);

            let mut clock = Clock::new();
            clock.inherited = InterpolateSize::AllowKeywords;
            let own = numeric_only(r#""interpolateSize": "numeric-only","#);
            clock.frame_measured(0, &closed, None, Some(&measured));
            clock.frame_measured(100, &own, None, Some(&measured));
            assert_eq!(clock.frame_measured(600, &own, None, Some(&measured)).height, None);
        }

        #[test]
        fn an_interrupt_toward_a_keyword_is_discrete() {
            let mut clock = Clock::new();
            let measured = measurement(50.0, 80.0);
            let tall = numeric_only(r#""height": 100,"#);
            clock.frame_measured(0, &numeric_only(r#""height": 0,"#), None, Some(&measured));
            clock.frame_measured(0, &tall, None, Some(&measured));
            clock.frame_measured(500, &tall, None, Some(&measured));
            clock.events();

            assert_eq!(clock.frame_measured(500, &numeric_only(""), None, Some(&measured)).height, None);
            assert_eq!(clock.events(), [(Cancel, "height".to_string(), 0.5)]);
        }

        #[test]
        fn a_running_transition_survives_the_value_switching_off() {
            let mut clock = Clock::new();
            let measured = measurement(0.0, 80.0);
            clock.frame_measured(0, &closed(), None, Some(&measured));
            clock.frame_measured(100, &auto(), None, Some(&measured));
            let off = numeric_only(r#""interpolateSize": "numeric-only","#);
            assert_eq!(height_of(&clock.frame_measured(600, &off, None, Some(&measured))), 40.0);
        }

        #[test]
        fn a_state_style_can_switch_it_on() {
            let mut clock = Clock::new();
            let measured = measurement(0.0, 80.0);
            let rest = style(r#"{ "height": 0, "transition": "height 1s linear" }"#);
            let hovered = resolve_state_styles(
                &style(
                    r#"{ "height": 0, "transition": "height 1s linear",
                         "hover": { "height": "auto", "interpolateSize": "allow-keywords" } }"#,
                ),
                true,
                false,
            );
            clock.frame_measured(0, &rest, None, Some(&measured));
            clock.frame_measured(100, &hovered, None, Some(&measured));
            assert_eq!(height_of(&clock.frame_measured(600, &hovered, None, Some(&measured))), 40.0);
        }

        #[test]
        fn an_invalid_interpolate_size_is_an_error() {
            assert!(validate_transitions(&style(r#"{ "interpolateSize": "allow-keywords" }"#)).is_ok());
            let error = validate_transitions(&style(r#"{ "interpolateSize": "always" }"#))
                .unwrap_err();
            assert!(error.contains("interpolateSize"), "{error}");
            let error = validate_transitions(&style(r#"{ "hover": { "interpolateSize": "x" } }"#))
                .unwrap_err();
            assert!(error.starts_with("hover."), "{error}");
        }

        fn sized(width: &str) -> StyleDesc {
            style(&format!(r#"{{ "width": {width}, "transition": "width 1s linear" }}"#))
        }

        #[test]
        fn a_percentage_end_is_a_percentage_whatever_the_parent_looks_like() {
            let mut clock = Clock::new();
            let tall = |height: &str| {
                style(&format!(r#"{{ "height": {height}, "transition": "height 1s linear" }}"#))
            };
            let auto_parent = Measurement {
                percent_height_resolves: false,
                natural_height: gpui::px(60.0),
                ..measurement(30.0, 30.0)
            };
            clock.frame_measured(0, &tall("0"), None, Some(&auto_parent));
            clock.frame_measured(0, &tall(r#""50%""#), None, Some(&auto_parent));
            let middle = clock.frame_measured(500, &tall(r#""50%""#), None, Some(&auto_parent));
            assert_eq!(middle.height, Some(DimensionValue::Percentage(0.25)));
        }

        #[test]
        fn a_parent_resize_does_not_touch_a_running_blend() {
            let mut clock = Clock::new();
            let narrow = Measurement {
                parent_content: gpui::size(gpui::px(200.0), gpui::px(80.0)),
                ..measurement(30.0, 30.0)
            };
            let wide = Measurement {
                parent_content: gpui::size(gpui::px(400.0), gpui::px(80.0)),
                ..narrow
            };
            clock.frame_measured(0, &sized("20"), None, Some(&narrow));
            clock.frame_measured(0, &sized(r#""50%""#), None, Some(&narrow));
            let before = clock.frame_measured(500, &sized(r#""50%""#), None, Some(&narrow)).width;
            let after = clock.frame_measured(500, &sized(r#""50%""#), None, Some(&wide)).width;
            assert_eq!(
                before,
                Some(DimensionValue::Calc(LinearLength { px: 10.0, percent: 0.25, ..LinearLength::default() }))
            );
            assert_eq!(after, before, "the percentage is still a share of whatever the parent is");
        }
    }
    /// A length is its px and its share of the percentage basis, which blend on
    /// their own as the two parts of a `calc()`, and `em` is px by then.
    mod lengths {
        use super::*;

        fn sized(width: &str) -> StyleDesc {
            style(&format!(r#"{{ "width": {width}, "transition": "width 1s linear" }}"#))
        }

        fn calc(px: f64, percent: f64) -> Option<DimensionValue> {
            Some(DimensionValue::Calc(LinearLength { px, percent, ..LinearLength::default() }))
        }

        /// `from` to `to` for a second, linear, read at `millis`.
        fn width_at(from: &str, to: &str, millis: u64) -> Option<DimensionValue> {
            let mut clock = Clock::new();
            clock.frame(0, &sized(from));
            clock.frame(0, &sized(to));
            clock.frame(millis, &sized(to)).width
        }

        #[test]
        fn px_to_a_percentage_blends_as_a_calc_with_no_measurement_and_ends_on_the_percentage() {
            let mut clock = Clock::new();
            clock.frame(0, &sized("20"));
            let started = clock.frame(100, &sized(r#""50%""#));
            assert_eq!(started.width, Some(DimensionValue::Pixels(20.0)));
            assert_eq!(clock.frame(600, &sized(r#""50%""#)).width, calc(10.0, 0.25));
            assert!(clock.transitions.take_active());

            let ended = clock.frame(1100, &sized(r#""50%""#));
            assert_eq!(ended.width, Some(DimensionValue::Percentage(0.5)));
            assert_eq!(
                clock.events(),
                [
                    (Run, "width".to_string(), 0.0),
                    (Start, "width".to_string(), 0.0),
                    (End, "width".to_string(), 1.0),
                ]
            );
            assert!(clock.frame(1200, &sized(r#""50%""#)).width == Some(DimensionValue::Percentage(0.5)));
            assert!(clock.events().is_empty(), "the style keeps the percentage, so nothing starts again");
        }

        #[test]
        fn a_percentage_to_px_blends_from_the_percentage() {
            assert_eq!(width_at(r#""50%""#, "20", 500), calc(10.0, 0.25));
            assert_eq!(width_at(r#""50%""#, "20", 1000), Some(DimensionValue::Pixels(20.0)));
            let Some(DimensionValue::Percentage(fraction)) = width_at(r#""10%""#, r#""50%""#, 500) else {
                panic!("two percentages stay a percentage");
            };
            assert!((fraction - 0.3).abs() < 1e-9, "{fraction}");
        }

        #[test]
        fn a_calc_transitions_from_and_to_a_calc_as_chrome_mixes_it() {
            // calc(100% - 20px) to calc(50% + 10px): the percentages mix and so do the px.
            let middle = width_at(r#""calc(100% - 20px)""#, r#""calc(50% + 10px)""#, 500);
            assert_eq!(middle, calc(-5.0, 0.75));
            let Some(DimensionValue::Calc(length)) = middle else { unreachable!() };
            let basis = |parent: f64| crate::css::length::LengthContext::for_tests().resolve(&length, parent);
            assert_eq!(basis(400.0), 295.0);
            assert_eq!(basis(600.0), 445.0, "a parent that resized stays the mix of the new basis");
            assert_eq!(width_at(r#""calc(100% - 20px)""#, r#""calc(50% + 10px)""#, 1000), calc(10.0, 0.5));
        }

        #[test]
        fn a_calc_meets_a_px_length_and_a_percentage() {
            assert_eq!(width_at(r#""calc(100% - 20px)""#, "40", 500), calc(10.0, 0.5));
            assert_eq!(width_at("40", r#""calc(100% - 20px)""#, 500), calc(10.0, 0.5));
            assert_eq!(width_at(r#""calc(100% - 20px)""#, r#""50%""#, 500), calc(-10.0, 0.75));
        }

        #[test]
        fn em_is_px_by_the_time_it_blends() {
            // The root's font is 16px.
            assert_eq!(width_at(r#""1em""#, r#""2em""#, 500), Some(DimensionValue::Pixels(24.0)));
            assert_eq!(width_at(r#""calc(100% - 1em)""#, r#""10px""#, 500), calc(-3.0, 0.5));
        }

        #[test]
        fn a_changed_font_size_moves_an_em_length_and_starts_a_transition() {
            let sized_in = |font: &str| {
                style(&format!(
                    r#"{{ "fontSize": "{font}", "width": "2em", "transition": "width 1s linear" }}"#
                ))
            };
            let mut clock = Clock::new();
            clock.frame(0, &sized_in("10px"));
            clock.frame(0, &sized_in("20px"));
            assert_eq!(clock.frame(500, &sized_in("20px")).width, Some(DimensionValue::Pixels(30.0)));
        }

        #[test]
        fn every_percent_length_property_blends_a_percentage_with_px() {
            let mut clock = Clock::new();
            let styled = |value: &str| {
                style(&format!(
                    r#"{{ "marginTop": {value}, "top": {value}, "paddingLeft": {value}, "flexBasis": {value},
                         "rowGap": {value}, "borderTopLeftRadius": {value},
                         "transition": "margin-top 1s linear, top 1s linear, padding-left 1s linear, flex-basis 1s linear, row-gap 1s linear, border-top-left-radius 1s linear" }}"#
                ))
            };
            clock.frame(0, &styled("0"));
            clock.frame(0, &styled(r#""50%""#));
            let middle = clock.frame(500, &styled(r#""50%""#));
            assert_eq!(middle.margin_top, Some(DimensionValue::Percentage(0.25)));
            assert_eq!(middle.top, Some(DimensionValue::Percentage(0.25)));
            assert_eq!(middle.flex_basis, Some(DimensionValue::Percentage(0.25)));
            assert_eq!(middle.padding_left, Some(LinearLength::percent(0.25)));
            assert_eq!(middle.row_gap, Some(LinearLength::percent(0.25)));
            assert_eq!(
                middle.border_top_left_radius,
                Some(crate::style::CornerRadiusValue::circular(LinearLength::percent(0.25)))
            );
        }

        #[test]
        fn number_length_properties_take_relative_units() {
            let mut clock = Clock::new();
            let styled = |gap: &str, border: &str, offset: &str, size: &str| {
                style(&format!(
                    r#"{{ "rowGap": "{gap}", "borderTopWidth": "{border}", "outlineOffset": "{offset}", "fontSize": "{size}",
                         "transition": "row-gap 1s linear, border-top-width 1s linear, outline-offset 1s linear, font-size 1s linear" }}"#
                ))
            };
            clock.frame(0, &styled("10px", "2px", "0px", "10px"));
            clock.frame(0, &styled("calc(1em + 2px)", "0.5em", "-0.5em", "200%"));
            let middle = clock.frame(500, &styled("calc(1em + 2px)", "0.5em", "-0.5em", "200%"));
            // The element's font is the declared 10px at the start and 32px (200% of the
            // parent's 16px) at the end: `em` is px by the time the style is read.
            assert_eq!(middle.font_size, Some(LinearLength::px(21.0)));
            assert_eq!(middle.border_top_width, Some(LinearLength::px(2.0 + (16.0 - 2.0) / 2.0)));
            assert_eq!(middle.outline_offset, Some(LinearLength::px(-8.0)));
        }
    }
    /// A box shorthand written with several values is one value per side, axis or corner,
    /// and each blends on its own.
    mod shorthands {
        use super::*;
        use crate::style::CornerRadiusValue;

        fn blended(property: &str, from: &str, to: &str) -> StyleDesc {
            let styled = |value: &str| {
                style(&format!(
                    r#"{{ "{property}": "{value}", "transition": "{} 1s linear" }}"#,
                    kebab_case(property)
                ))
            };
            let mut clock = Clock::new();
            clock.frame(0, &styled(from));
            clock.frame(0, &styled(to));
            clock.frame(500, &styled(to))
        }

        #[test]
        fn a_padding_string_blends_each_side_to_its_own_value() {
            // The root's font is 16px.
            let middle = blended("padding", "0.5em 0.75em", "1em 1.5em");
            assert_eq!(middle.padding_top, Some(LinearLength::px(12.0)));
            assert_eq!(middle.padding_bottom, Some(LinearLength::px(12.0)));
            assert_eq!(middle.padding_left, Some(LinearLength::px(18.0)));
            assert_eq!(middle.padding_right, Some(LinearLength::px(18.0)));
        }

        #[test]
        fn inset_gap_and_border_width_blend_per_side_and_per_axis() {
            let middle = blended("inset", "0 10px", "20px 30px");
            assert_eq!(middle.top, Some(DimensionValue::Pixels(10.0)));
            assert_eq!(middle.right, Some(DimensionValue::Pixels(20.0)));
            let middle = blended("gap", "0 4px", "8px 12px");
            assert_eq!(middle.row_gap, Some(LinearLength::px(4.0)));
            assert_eq!(middle.column_gap, Some(LinearLength::px(8.0)));
            let middle = blended("borderWidth", "1px 2px 3px 4px", "3px 4px 5px 6px");
            assert_eq!(middle.border_top_width, Some(LinearLength::px(2.0)));
            assert_eq!(middle.border_left_width, Some(LinearLength::px(5.0)));
        }

        #[test]
        fn an_elliptical_border_radius_blends_both_radii() {
            let middle = blended("borderRadius", "10px / 20px", "30px / 40px");
            let ellipse = CornerRadiusValue { x: LinearLength::px(20.0), y: Some(LinearLength::px(30.0)) };
            assert_eq!(middle.border_top_left_radius, Some(ellipse));
            assert_eq!(middle.border_bottom_right_radius, Some(ellipse));
        }

        #[test]
        fn a_circular_corner_blends_with_an_elliptical_one() {
            let middle = blended("borderTopLeftRadius", "10px", "20px 40px");
            assert_eq!(
                middle.border_top_left_radius,
                Some(CornerRadiusValue { x: LinearLength::px(15.0), y: Some(LinearLength::px(25.0)) })
            );
        }
    }
    mod discrete {
        use super::*;

        /// A style with one discrete property set, transitioning it for a
        /// second without easing.
        fn with(property: &str, value: &str, behavior: &str) -> StyleDesc {
            let css = kebab_case(property);
            style(&format!(
                r#"{{ "{property}": "{value}", "transition": "{css} 1s linear {behavior}" }}"#
            ))
        }

        fn display(style: &StyleDesc) -> Option<&str> {
            style.display.as_deref()
        }

        fn visibility(style: &StyleDesc) -> Option<&str> {
            style.visibility.as_deref()
        }

        #[test]
        fn a_discrete_property_flips_at_half_with_allow_discrete() {
            let mut clock = Clock::new();
            let row = with("flexDirection", "row", "allow-discrete");
            let column = with("flexDirection", "column", "allow-discrete");
            clock.frame(0, &row);
            assert_eq!(clock.frame(0, &column).flex_direction.as_deref(), Some("row"));
            assert_eq!(clock.frame(499, &column).flex_direction.as_deref(), Some("row"));
            assert_eq!(clock.frame(500, &column).flex_direction.as_deref(), Some("column"));
            assert_eq!(clock.frame(999, &column).flex_direction.as_deref(), Some("column"));
            assert!(clock.transitions.take_active());
            assert_eq!(clock.frame(1000, &column).flex_direction.as_deref(), Some("column"));
            assert!(!clock.transitions.take_active());
        }

        #[test]
        fn the_flip_follows_the_timing_function_and_the_delay() {
            let mut clock = Clock::new();
            let style_of = |direction: &str| {
                style(&format!(
                    r#"{{ "flexDirection": "{direction}",
                         "transition": "flex-direction 1s ease-in 500ms allow-discrete" }}"#
                ))
            };
            clock.frame(0, &style_of("row"));
            let column = style_of("column");
            clock.frame(0, &column);
            // Linear progress 0.6 is eased below a half by ease-in.
            assert_eq!(clock.frame(1100, &column).flex_direction.as_deref(), Some("row"));
            assert_eq!(clock.frame(1400, &column).flex_direction.as_deref(), Some("column"));
        }

        #[test]
        fn normal_leaves_a_discrete_property_alone() {
            for behavior in ["", "normal"] {
                let mut clock = Clock::new();
                clock.frame(0, &with("flexDirection", "row", behavior));
                let column = with("flexDirection", "column", behavior);
                assert_eq!(clock.frame(10, &column).flex_direction.as_deref(), Some("column"));
                assert!(clock.events().is_empty(), "{behavior:?}");
            }
        }

        #[test]
        fn all_covers_discrete_properties_only_with_allow_discrete() {
            let all = |direction: &str, behavior: &str| {
                style(&format!(
                    r#"{{ "flexDirection": "{direction}", "transition": "all 1s linear {behavior}" }}"#
                ))
            };
            let mut clock = Clock::new();
            clock.frame(0, &all("row", "allow-discrete"));
            assert_eq!(
                clock.frame(0, &all("column", "allow-discrete")).flex_direction.as_deref(),
                Some("row")
            );
            let mut clock = Clock::new();
            clock.frame(0, &all("row", ""));
            assert_eq!(clock.frame(0, &all("column", "")).flex_direction.as_deref(), Some("column"));
        }

        #[test]
        fn position_overflow_and_pointer_events_flip_at_half_with_allow_discrete() {
            fn flips<T: PartialEq + std::fmt::Debug>(
                property: &str,
                from: &str,
                to: &str,
                read: impl Fn(&StyleDesc) -> T,
            ) {
                let mut clock = Clock::new();
                let start = with(property, from, "allow-discrete");
                let end = with(property, to, "allow-discrete");
                let first = read(&clock.frame(0, &start));
                assert_eq!(read(&clock.frame(0, &end)), first, "{property}");
                assert_eq!(read(&clock.frame(499, &end)), first, "{property}");
                let last = read(&clock.frame(500, &end));
                assert_ne!(last, first, "{property}");
                assert_eq!(read(&clock.frame(1000, &end)), last, "{property}");
                // Without allow-discrete it jumps
                let mut clock = Clock::new();
                clock.frame(0, &with(property, from, ""));
                assert_eq!(read(&clock.frame(0, &with(property, to, ""))), last, "{property}");
            }
            flips("position", "static", "relative", |style| style.position.clone());
            flips("overflowX", "hidden", "scroll", |style| style.overflow_x.clone());
            flips("overflowY", "visible", "hidden", |style| style.overflow_y.clone());
            flips("pointerEvents", "auto", "none", |style| style.pointer_events.clone());
            flips("tableLayout", "auto", "fixed", |style| style.table_layout);
            flips("borderCollapse", "separate", "collapse", |style| style.border_collapse);
            flips("captionSide", "top", "bottom", |style| style.caption_side);
            flips("emptyCells", "show", "hide", |style| style.empty_cells);
        }

        #[test]
        fn the_overflow_shorthand_covers_both_axes() {
            let mut clock = Clock::new();
            let with_overflow = |overflow: &str| {
                style(&format!(
                    r#"{{ "overflow": "{overflow}", "transition": "overflow 1s linear allow-discrete" }}"#
                ))
            };
            clock.frame(0, &with_overflow("visible"));
            let hidden = with_overflow("hidden");
            let before = clock.frame(100, &hidden);
            assert_eq!((before.overflow_x.as_deref(), before.overflow_y.as_deref()), (Some("visible"), Some("visible")));
            let after = clock.frame(600, &hidden);
            assert_eq!((after.overflow_x.as_deref(), after.overflow_y.as_deref()), (Some("hidden"), Some("hidden")));
        }

        #[test]
        fn border_spacing_blends_both_lengths() {
            let mut clock = Clock::new();
            let spaced = |spacing: &str| {
                style(&format!(r#"{{ "borderSpacing": "{spacing}", "transition": "border-spacing 1s linear" }}"#))
            };
            clock.frame(0, &spaced("4px 8px"));
            let end = spaced("12px 20px");
            let at = |clock: &mut Clock, ms| {
                let spacing = clock.frame(ms, &end).border_spacing.expect("a spacing");
                spacing.resolve(&crate::css::length::LengthContext::for_tests())
            };
            clock.frame(0, &end);
            let half = at(&mut clock, 500);
            assert_eq!((half.horizontal, half.vertical), (8.0, 14.0));
            let quarter = at(&mut clock, 250);
            assert_eq!((quarter.horizontal, quarter.vertical), (6.0, 11.0));
        }

        #[test]
        fn z_index_blends_as_an_integer_rounding_half_up() {
            let mut clock = Clock::new();
            let stacked = |z: i32| style(&format!(r#"{{ "position": "relative", "zIndex": {z}, "transition": "z-index 1s linear" }}"#));
            clock.frame(0, &stacked(-5));
            let end = stacked(4);
            clock.frame(0, &end);
            let z = |clock: &mut Clock, ms| clock.frame(ms, &end).z_index.expect("a z-index");
            assert_eq!(z(&mut clock, 250), -3.0);
            assert_eq!(z(&mut clock, 500), 0.0);
            assert_eq!(z(&mut clock, 750), 2.0);
            // Without a z-index there is no number to blend from
            let mut clock = Clock::new();
            clock.frame(0, &style(r#"{ "position": "relative", "transition": "z-index 1s linear" }"#));
            let auto_to_five = style(r#"{ "position": "relative", "zIndex": 5, "transition": "z-index 1s linear" }"#);
            assert_eq!(clock.frame(10, &auto_to_five).z_index, Some(5.0));
        }

        #[test]
        fn vertical_align_blends_lengths_and_flips_against_keywords() {
            let mut clock = Clock::new();
            let aligned = |align: &str, behavior: &str| {
                style(&format!(r#"{{ "verticalAlign": "{align}", "transition": "vertical-align 1s linear {behavior}" }}"#))
            };
            clock.frame(0, &aligned("0px", ""));
            let end = aligned("100%", "");
            clock.frame(0, &end);
            let resolved = |style: StyleDesc| {
                style.vertical_align.expect("an alignment").resolve(&crate::css::length::LengthContext::for_tests())
            };
            // 0px against 100% mixes the two parts of a calc(): a quarter of the line height
            match resolved(clock.frame(250, &end)) {
                crate::css::vertical_align::ResolvedVerticalAlign::Offset { px, line_height_fraction } => {
                    assert_eq!((px, line_height_fraction), (0.0, 0.25));
                }
                other => panic!("{other:?}"),
            }
            // A keyword against a length is discrete: no transition without allow-discrete
            let mut clock = Clock::new();
            clock.frame(0, &aligned("baseline", ""));
            let length = aligned("12px", "");
            assert!(matches!(
                resolved(clock.frame(0, &length)),
                crate::css::vertical_align::ResolvedVerticalAlign::Offset { px, .. } if px == 12.0
            ));
            let mut clock = Clock::new();
            clock.frame(0, &aligned("sub", "allow-discrete"));
            let sup = aligned("super", "allow-discrete");
            assert!(matches!(resolved(clock.frame(0, &sup)), crate::css::vertical_align::ResolvedVerticalAlign::Sub));
            assert!(matches!(resolved(clock.frame(500, &sup)), crate::css::vertical_align::ResolvedVerticalAlign::Super));
        }

        #[test]
        fn display_none_is_shown_for_the_whole_duration() {
            let mut clock = Clock::new();
            // block to none: block until the end.
            clock.frame(0, &with("display", "block", "allow-discrete"));
            let none = with("display", "none", "allow-discrete");
            assert_eq!(display(&clock.frame(0, &none)), Some("block"));
            assert_eq!(display(&clock.frame(1, &none)), Some("block"));
            assert_eq!(display(&clock.frame(600, &none)), Some("block"));
            assert_eq!(display(&clock.frame(999, &none)), Some("block"));
            assert_eq!(display(&clock.frame(1000, &none)), Some("none"));

            // none to flex: flex from the first frame it runs.
            let mut clock = Clock::new();
            clock.frame(0, &with("display", "none", "allow-discrete"));
            let flex = with("display", "flex", "allow-discrete");
            assert_eq!(display(&clock.frame(0, &flex)), Some("none"));
            assert_eq!(display(&clock.frame(1, &flex)), Some("flex"));
            assert_eq!(display(&clock.frame(600, &flex)), Some("flex"));
        }

        #[test]
        fn display_between_visible_values_flips_at_half() {
            let mut clock = Clock::new();
            clock.frame(0, &with("display", "flex", "allow-discrete"));
            let block = with("display", "block", "allow-discrete");
            assert_eq!(display(&clock.frame(100, &block)), Some("flex"));
            assert_eq!(display(&clock.frame(600, &block)), Some("block"));
        }

        #[test]
        fn visibility_is_visible_for_the_whole_duration_when_either_end_is() {
            let mut clock = Clock::new();
            clock.frame(0, &with("visibility", "visible", "allow-discrete"));
            let hidden = with("visibility", "hidden", "allow-discrete");
            assert_eq!(visibility(&clock.frame(0, &hidden)), Some("visible"));
            assert_eq!(visibility(&clock.frame(900, &hidden)), Some("visible"));
            assert_eq!(visibility(&clock.frame(1000, &hidden)), Some("hidden"));

            // hidden to visible shows at once, not at half.
            let visible = with("visibility", "visible", "allow-discrete");
            assert_eq!(visibility(&clock.frame(1000, &visible)), Some("hidden"));
            assert_eq!(visibility(&clock.frame(1100, &visible)), Some("visible"));
            assert_eq!(visibility(&clock.frame(1900, &visible)), Some("visible"));
        }

        #[test]
        fn visibility_needs_no_allow_discrete_and_display_does() {
            // Chrome animates `visibility` by its own rule, so a plain `transition` runs it.
            let mut clock = Clock::new();
            clock.frame(0, &with("visibility", "hidden", ""));
            let visible = with("visibility", "visible", "");
            assert_eq!(visibility(&clock.frame(0, &visible)), Some("hidden"));
            assert_eq!(visibility(&clock.frame(1, &visible)), Some("visible"));
            assert_eq!(visibility(&clock.frame(1000, &visible)), Some("visible"));

            let mut clock = Clock::new();
            clock.frame(0, &with("display", "flex", ""));
            assert_eq!(display(&clock.frame(100, &with("display", "block", ""))), Some("block"));
        }

        #[test]
        fn an_unset_visibility_is_visible() {
            let mut clock = Clock::new();
            let unset = style(r#"{ "transition": "visibility 1s linear allow-discrete" }"#);
            clock.frame(0, &unset);
            let hidden = with("visibility", "hidden", "allow-discrete");
            assert_eq!(visibility(&clock.frame(0, &hidden)), Some("visible"));
            assert_eq!(visibility(&clock.frame(900, &hidden)), Some("visible"));
        }

        #[test]
        fn an_unset_value_is_a_value_of_its_own() {
            let mut clock = Clock::new();
            let unset = style(r#"{ "transition": "display 1s linear allow-discrete" }"#);
            clock.frame(0, &unset);
            let none = with("display", "none", "allow-discrete");
            assert_eq!(display(&clock.frame(0, &none)), None);
            assert_eq!(display(&clock.frame(500, &none)), None);
            assert_eq!(display(&clock.frame(1000, &none)), Some("none"));
        }

        #[test]
        fn discrete_transitions_fire_events_like_any_other() {
            let mut clock = Clock::new();
            clock.frame(0, &with("flexDirection", "row", "allow-discrete"));
            let column = with("flexDirection", "column", "allow-discrete");
            clock.frame(0, &column);
            assert_eq!(
                clock.events(),
                [(Run, "flex-direction".to_string(), 0.0), (Start, "flex-direction".to_string(), 0.0)]
            );
            clock.frame(1000, &column);
            assert_eq!(clock.events(), [(End, "flex-direction".to_string(), 1.0)]);

            // An interrupt past the flip cancels and starts again from the value showing.
            let row = with("flexDirection", "row", "allow-discrete");
            clock.frame(1000, &row);
            clock.events();
            clock.frame(1700, &row);
            clock.frame(1700, &column);
            assert_eq!(
                clock.events(),
                [
                    (Cancel, "flex-direction".to_string(), 0.7),
                    (Run, "flex-direction".to_string(), 0.0),
                    (Start, "flex-direction".to_string(), 0.0)
                ]
            );
        }

        #[test]
        fn reversing_a_discrete_transition_shortens_the_way_back() {
            let mut clock = Clock::new();
            let row = with("flexDirection", "row", "allow-discrete");
            let column = with("flexDirection", "column", "allow-discrete");
            clock.frame(0, &row);
            clock.frame(0, &column);
            clock.frame(600, &column);
            clock.events();
            // 60% in, so the way back takes 600ms and flips at its middle.
            assert_eq!(clock.frame(600, &row).flex_direction.as_deref(), Some("column"));
            assert_eq!(clock.frame(890, &row).flex_direction.as_deref(), Some("column"));
            assert_eq!(clock.frame(900, &row).flex_direction.as_deref(), Some("row"));
            clock.events();
            clock.frame(1200, &row);
            assert_eq!(clock.events(), [(End, "flex-direction".to_string(), 0.6)]);
        }

        #[test]
        fn a_shadow_that_cannot_blend_flips_with_allow_discrete() {
            let shadow = |inset: bool, behavior: &str| {
                style(&format!(
                    r##"{{ "boxShadow": {{ "offsetX": 0, "offsetY": 0, "blurRadius": 4, "spreadRadius": 0,
                                         "color": "#000", "inset": {inset} }},
                         "transition": "box-shadow 1s linear {behavior}" }}"##
                ))
            };
            let mut clock = Clock::new();
            clock.frame(0, &shadow(false, "allow-discrete"));
            let inset = |style: &StyleDesc| match &style.box_shadow {
                Some(crate::style::BoxShadowValues::Many(layers)) => layers[0].inset,
                Some(crate::style::BoxShadowValues::One(layer)) => layer.inset,
                None => panic!("no shadow"),
            };
            assert!(!inset(&clock.frame(0, &shadow(true, "allow-discrete"))));
            assert!(inset(&clock.frame(600, &shadow(true, "allow-discrete"))));

            let mut clock = Clock::new();
            clock.frame(0, &shadow(false, "normal"));
            assert!(inset(&clock.frame(0, &shadow(true, "normal"))));
            assert!(clock.events().is_empty());
        }

        #[test]
        fn a_filter_transitions_between_lists_and_against_none() {
            let filter = |value: &str, behavior: &str| {
                style(&format!(r#"{{ "filter": "{value}", "transition": "filter 1s linear {behavior}" }}"#))
            };
            let mut clock = Clock::new();
            clock.frame(0, &filter("blur(4px)", ""));
            let running = clock.frame(0, &filter("blur(0px) brightness(2)", ""));
            assert_eq!(running.filter.as_deref(), Some("blur(4px) brightness(1)"));
            assert_eq!(clock.frame(500, &filter("blur(0px) brightness(2)", "")).filter.as_deref(), Some("blur(2px) brightness(1.5)"));
            assert!(clock.transitions.paints_only(1));
            clock.frame(1000, &filter("blur(0px) brightness(2)", ""));
            assert!(clock.events().iter().any(|(kind, property, _)| *kind == End && property == "filter"));

            let mut clock = Clock::new();
            clock.frame(0, &style(r#"{ "transition": "filter 1s linear" }"#));
            let from_nothing = clock.frame(0, &filter("brightness(2) blur(8px)", ""));
            assert_eq!(from_nothing.filter.as_deref(), Some("brightness(1) blur(0px)"));
            assert_eq!(clock.frame(250, &filter("brightness(2) blur(8px)", "")).filter.as_deref(), Some("brightness(1.25) blur(2px)"));
        }

        #[test]
        fn lists_that_cannot_blend_start_no_filter_transition_unless_discrete_is_allowed() {
            let filter = |value: &str, behavior: &str| {
                style(&format!(r#"{{ "filter": "{value}", "transition": "filter 1s linear {behavior}" }}"#))
            };
            let mut clock = Clock::new();
            clock.frame(0, &filter("blur(4px)", ""));
            assert_eq!(clock.frame(0, &filter("brightness(2)", "")).filter.as_deref(), Some("brightness(2)"));
            assert!(clock.events().is_empty());

            let mut clock = Clock::new();
            clock.frame(0, &filter("blur(4px)", "allow-discrete"));
            assert_eq!(clock.frame(0, &filter("brightness(2)", "allow-discrete")).filter.as_deref(), Some("blur(4px)"));
            assert_eq!(clock.frame(600, &filter("brightness(2)", "allow-discrete")).filter.as_deref(), Some("brightness(2)"));
        }

        #[test]
        fn display_is_held_while_it_transitions_to_none() {
            let held = |clock: &Clock| clock.transitions.held_displays().contains(&1);
            let mut clock = Clock::new();
            assert!(!held(&clock));
            clock.frame(0, &with("display", "block", "allow-discrete"));
            assert!(held(&clock));
            clock.frame(0, &with("display", "none", "allow-discrete"));
            assert!(held(&clock));

            let mut clock = Clock::new();
            clock.frame(0, &with("display", "block", ""));
            assert!(!held(&clock));
            let instant = style(
                r#"{ "display": "block", "transition": "display 0s allow-discrete" }"#,
            );
            clock.frame(0, &instant);
            assert!(!held(&clock), "a zero duration holds nothing");
        }
    }

    /// A property the style leaves unset has a CSS computed value to transition
    /// from when it is interpolable, and none when it is `auto` or `normal`.
    mod unset_start {
        use super::*;

        const ALL: &str = "all 1s linear";

        fn with_transition(declarations: &str) -> StyleDesc {
            style(&format!(r#"{{ {declarations}"transition": "{ALL}" }}"#))
        }

        /// The number `prop` reads after half a second of a transition from the
        /// unset style to one that declares `declaration`.
        fn halfway(declaration: &str, prop: AnimProp) -> Option<AnimValue> {
            let mut clock = Clock::new();
            clock.frame(0, &with_transition(""));
            let target = with_transition(&format!("{declaration},"));
            clock.frame(0, &target);
            prop.read(&clock.frame(500, &target), &Colors::root())
        }

        #[test]
        fn an_interpolable_property_leaves_from_its_computed_value() {
            for (declaration, prop, expected) in [
                (r#""opacity": 0"#, AnimProp::Opacity, 0.5),
                (r#""flexGrow": 2"#, AnimProp::FlexGrow, 1.0),
                (r#""flexShrink": 0"#, AnimProp::FlexShrink, 0.5),
                (r#""paddingTop": 10"#, AnimProp::PaddingTop, 5.0),
                (r#""marginLeft": 10"#, AnimProp::MarginLeft, 5.0),
                (r#""rowGap": 10"#, AnimProp::RowGap, 5.0),
                (r#""columnGap": 10"#, AnimProp::ColumnGap, 5.0),
                // An unset width computes to `medium`, 3px, whatever the border style.
                (r#""borderBottomWidth": 10"#, AnimProp::BorderBottomWidth, 6.5),
                (r#""borderTopLeftRadius": 10"#, AnimProp::BorderTopLeftRadius, 5.0),
                (r#""outlineWidth": 10"#, AnimProp::OutlineWidth, 6.5),
                (r#""outlineOffset": 10"#, AnimProp::OutlineOffset, 5.0),
            ] {
                assert_eq!(
                    halfway(declaration, prop),
                    Some(AnimValue::Number(expected)),
                    "{declaration}"
                );
            }
        }

        #[test]
        fn an_unset_shadow_and_background_leave_from_none_and_transparent() {
            let mut clock = Clock::new();
            clock.frame(0, &with_transition(""));
            let target = with_transition(
                r##""backgroundColor": "#ff0000", "boxShadow": { "offsetX": 0, "offsetY": 4, "blurRadius": 8, "spreadRadius": 0, "color": "#000000" },"##,
            );
            clock.frame(0, &target);
            let middle = clock.frame(500, &target);
            let background = crate::color::parse_color_rgba(middle.background_color.as_deref().unwrap()).unwrap();
            assert!((background.a - 0.5).abs() < 0.01, "{}", background.a);
            let Some(crate::style::BoxShadowValues::Many(layers)) = middle.box_shadow else {
                panic!("expected a shadow list");
            };
            assert_eq!(layers[0].offset_y, crate::css::length::LinearLength::px(2.0));
            assert_eq!(layers[0].blur_radius, crate::css::length::LinearLength::px(4.0));
        }

        fn colour_after(clock: &mut Clock, millis: u64, after: &StyleDesc) -> String {
            clock.frame(millis, after).color.unwrap()
        }

        #[test]
        fn an_unset_color_leaves_from_the_inherited_one() {
            let mut clock = Clock::new();
            clock.parent_colors = ColorContext::root(crate::color::Scheme::Light)
                .descend(&style(r##"{ "color": "#ff0000" }"##));
            clock.frame(0, &with_transition(""));
            let target = with_transition(r##""color": "#0000ff","##);
            near(&colour_after(&mut clock, 0, &target), "#ff0000");
            assert_eq!(clock.events(), [(Run, "color".to_string(), 0.0), (Start, "color".to_string(), 0.0)]);
            near(&colour_after(&mut clock, 500, &target), "#800080");
            assert_eq!(colour_after(&mut clock, 1000, &target), "#0000ff");
        }

        #[test]
        fn a_color_that_becomes_unset_leaves_to_the_inherited_one() {
            let mut clock = Clock::new();
            clock.parent_colors = ColorContext::root(crate::color::Scheme::Light)
                .descend(&style(r##"{ "color": "#ff0000" }"##));
            clock.frame(0, &with_transition(r##""color": "#0000ff","##));
            let unset = with_transition("");
            near(&colour_after(&mut clock, 0, &unset), "#0000ff");
            near(&colour_after(&mut clock, 500, &unset), "#800080");
            let finished = clock.frame(1000, &unset);
            assert_eq!(finished.color, None, "the inherited colour paints again");
        }

        #[test]
        fn an_unset_color_with_no_parent_colour_leaves_from_the_canvas_text_of_its_scheme() {
            let mut clock = Clock::new();
            let dark = |declarations: &str| {
                style(&format!(r#"{{ "colorScheme": "dark", {declarations}"transition": "{ALL}" }}"#))
            };
            clock.frame(0, &dark(""));
            near(&colour_after(&mut clock, 0, &dark(r##""color": "#000000","##)), "#ffffff");
        }

        #[test]
        fn a_parents_new_colour_is_not_a_change_of_an_unset_color() {
            let mut clock = Clock::new();
            let inherited = |color: &str| {
                ColorContext::root(crate::color::Scheme::Light).descend(&style(&format!(r#"{{ "color": "{color}" }}"#)))
            };
            clock.parent_colors = inherited("#ff0000");
            clock.frame(0, &with_transition(""));
            clock.parent_colors = inherited("#0000ff");
            clock.frame(0, &with_transition(""));
            assert!(clock.events().is_empty());
        }

        /// `auto` and `normal` are keywords CSS does not blend with a length or
        /// a number, so a transition out of an unset one takes the target at once.
        #[test]
        fn a_property_whose_computed_value_is_a_keyword_does_not_animate_from_unset() {
            for (declaration, prop, target) in [
                (r#""top": 10"#, AnimProp::Top, AnimValue::Number(10.0)),
                (r#""right": 10"#, AnimProp::Right, AnimValue::Number(10.0)),
                (r#""bottom": 10"#, AnimProp::Bottom, AnimValue::Number(10.0)),
                (r#""left": 10"#, AnimProp::Left, AnimValue::Number(10.0)),
                (r#""flexBasis": 100"#, AnimProp::FlexBasis, AnimValue::Number(100.0)),
                (r#""lineHeight": 20"#, AnimProp::LineHeight, AnimValue::Number(20.0)),
                (r#""aspectRatio": 2"#, AnimProp::AspectRatio, AnimValue::Number(2.0)),
            ] {
                let mut clock = Clock::new();
                clock.frame(0, &with_transition(""));
                let target_style = with_transition(&format!("{declaration},"));
                let at_once = clock.frame(0, &target_style);
                assert_eq!(prop.read(&at_once, &Colors::root()), Some(target), "{declaration}");
                assert!(clock.events().is_empty(), "{declaration}: {:?}", clock.events());
            }
        }

        /// A parent whose computed font is 20px at weight 700, in a window of 800 by 600.
        fn inside_a_large_bold_parent(clock: &mut Clock) {
            clock.parent_font = ParentValues {
                lengths: crate::css::length::LengthContext::initial(800.0, 600.0)
                    .descend(Some(&LinearLength::px(20.0)), true),
                above_root: false,
                weight: 700.0,
                variations: Default::default(),
                inheritable: Default::default(),
                custom: Default::default(),
            };
        }

        #[test]
        fn an_unset_font_size_and_weight_leave_from_the_inherited_ones() {
            let mut clock = Clock::new();
            inside_a_large_bold_parent(&mut clock);
            clock.frame(0, &with_transition(""));
            let target = with_transition(r#""fontSize": 40, "fontWeight": 300,"#);
            clock.frame(0, &target);
            let middle = clock.frame(500, &target);
            assert_eq!(AnimProp::FontSize.read(&middle, &Colors::root()), Some(AnimValue::Number(30.0)));
            assert_eq!(AnimProp::FontWeight.read(&middle, &Colors::root()), Some(AnimValue::Number(500.0)));
        }

        #[test]
        fn a_font_size_in_em_computes_against_the_inherited_size_for_the_start_of_a_transition() {
            let mut clock = Clock::new();
            inside_a_large_bold_parent(&mut clock);
            clock.frame(0, &with_transition(r#""fontSize": "2em","#));
            let target = with_transition(r#""fontSize": 20,"#);
            clock.frame(0, &target);
            let middle = clock.frame(500, &target);
            assert_eq!(AnimProp::FontSize.read(&middle, &Colors::root()), Some(AnimValue::Number(30.0)));
        }

        #[test]
        fn line_height_transitions_between_two_declared_values() {
            let mut clock = Clock::new();
            clock.frame(0, &with_transition(r#""lineHeight": 20,"#));
            let target = with_transition(r#""lineHeight": 40,"#);
            clock.frame(0, &target);
            assert_eq!(
                AnimProp::LineHeight.read(&clock.frame(500, &target), &Colors::root()),
                Some(AnimValue::Number(30.0))
            );
        }
    }

    /// `font-variation-settings` blends per axis between values that name the same
    /// axes, and is discrete otherwise, as in Chrome.
    mod variations {
        use super::*;

        fn with(settings: &str, behavior: &str) -> StyleDesc {
            style(&format!(
                r#"{{ "fontVariationSettings": "{settings}",
                     "transition": "font-variation-settings 1s linear {behavior}" }}"#
            ))
        }

        fn axes(style: &StyleDesc) -> Vec<(String, f64)> {
            let text = String::from(style.font_variation_settings.clone().expect("declared"));
            if text == "normal" {
                return Vec::new();
            }
            text.split(", ")
                .map(|axis| {
                    let (tag, value) = axis.split_once(' ').unwrap();
                    (tag.trim_matches('"').to_string(), value.parse().unwrap())
                })
                .collect()
        }

        fn wght(value: f64) -> Vec<(String, f64)> {
            vec![("wght".to_string(), value)]
        }

        #[test]
        fn the_same_axes_blend_without_allow_discrete() {
            let mut clock = Clock::new();
            clock.frame(0, &with("'wght' 400", ""));
            let to = with("'wght' 700", "");
            assert_eq!(axes(&clock.frame(0, &to)), wght(400.0));
            assert_eq!(axes(&clock.frame(500, &to)), wght(550.0));
            assert_eq!(axes(&clock.frame(1000, &to)), wght(700.0));
        }

        #[test]
        fn two_axes_blend_in_any_order() {
            let mut clock = Clock::new();
            clock.frame(0, &with("'wght' 400, 'wdth' 100", ""));
            let to = with("'wdth' 50, 'wght' 800", "");
            clock.frame(0, &to);
            assert_eq!(
                axes(&clock.frame(500, &to)),
                vec![("wdth".to_string(), 75.0), ("wght".to_string(), 600.0)]
            );
        }

        #[test]
        fn other_axes_and_normal_do_not_transition_by_default() {
            for (from, to) in [
                ("'wght' 400", "'wdth' 80"),
                ("'wght' 400", "'wght' 400, 'wdth' 80"),
                ("normal", "'wght' 700"),
                ("'wght' 700", "normal"),
            ] {
                let mut clock = Clock::new();
                clock.frame(0, &with(from, ""));
                let end = clock.frame(10, &with(to, ""));
                assert_eq!(end.font_variation_settings, with(to, "").font_variation_settings, "{from} to {to}");
                assert!(clock.events().is_empty(), "{from} to {to}: {:?}", clock.events());
            }
        }

        #[test]
        fn other_axes_and_normal_flip_at_half_with_allow_discrete() {
            for (from, to, before, after) in [
                ("'wght' 400", "'wdth' 80", wght(400.0), vec![("wdth".to_string(), 80.0)]),
                ("normal", "'wght' 700", Vec::new(), wght(700.0)),
                ("'wght' 700", "normal", wght(700.0), Vec::new()),
            ] {
                let mut clock = Clock::new();
                clock.frame(0, &with(from, "allow-discrete"));
                let end = with(to, "allow-discrete");
                assert_eq!(axes(&clock.frame(0, &end)), before, "{from} to {to}");
                assert_eq!(axes(&clock.frame(499, &end)), before, "{from} to {to}");
                assert_eq!(axes(&clock.frame(500, &end)), after, "{from} to {to}");
                assert_eq!(axes(&clock.frame(1000, &end)), after, "{from} to {to}");
            }
        }

        #[test]
        fn an_unset_value_leaves_from_the_inherited_settings() {
            let mut clock = Clock::new();
            clock.parent_font.variations = gpui::FontVariations::new([(u32::from_be_bytes(*b"wght"), 300.0)]);
            let unset = style(r#"{ "transition": "font-variation-settings 1s linear" }"#);
            clock.frame(0, &unset);
            let to = with("'wght' 700", "");
            clock.frame(0, &to);
            assert_eq!(axes(&clock.frame(500, &to)), wght(500.0));

            // Inheriting nothing, `normal` names no axis, so the change is discrete.
            let mut clock = Clock::new();
            clock.frame(0, &unset);
            assert_eq!(axes(&clock.frame(500, &to)), wght(700.0));
            assert!(clock.events().is_empty());
        }
    }

    mod auto_and_border_widths {
        use super::*;

        fn margin(value: &str, behavior: &str) -> StyleDesc {
            style(&format!(
                r#"{{ "marginLeft": {value}, "transition": "margin-left 1s linear {behavior}" }}"#
            ))
        }

        #[test]
        fn auto_to_a_length_is_discrete_so_it_jumps_without_allow_discrete() {
            let mut clock = Clock::new();
            clock.frame(0, &margin(r#""auto""#, ""));
            let end = margin("100", "");
            assert_eq!(clock.frame(10, &end).margin_left, Some(DimensionValue::Pixels(100.0)));
            assert!(clock.events().is_empty());
            let mut clock = Clock::new();
            clock.frame(0, &margin("100", ""));
            assert_eq!(clock.frame(10, &margin(r#""auto""#, "")).margin_left, Some(DimensionValue::Auto));
            assert!(clock.events().is_empty());
        }

        #[test]
        fn allow_discrete_flips_auto_and_a_length_at_half() {
            let mut clock = Clock::new();
            clock.frame(0, &margin(r#""auto""#, "allow-discrete"));
            let end = margin("100", "allow-discrete");
            assert_eq!(clock.frame(0, &end).margin_left, Some(DimensionValue::Auto));
            assert_eq!(clock.frame(400, &end).margin_left, Some(DimensionValue::Auto));
            assert_eq!(clock.frame(600, &end).margin_left, Some(DimensionValue::Pixels(100.0)));
            assert_eq!(clock.frame(1000, &end).margin_left, Some(DimensionValue::Pixels(100.0)));
        }

        #[test]
        fn an_unset_margin_is_zero_and_blends_with_a_length() {
            let mut clock = Clock::new();
            clock.frame(0, &style(r#"{ "transition": "margin-left 1s linear" }"#));
            let end = margin("100", "");
            assert_eq!(clock.frame(0, &end).margin_left, Some(DimensionValue::Pixels(0.0)));
            assert_eq!(clock.frame(400, &end).margin_left, Some(DimensionValue::Pixels(40.0)));
        }

        #[test]
        fn an_unset_inset_is_auto_and_flips_with_allow_discrete_only() {
            let inset = |value: &str, behavior: &str| {
                style(&format!(r#"{{ "top": {value}, "transition": "top 1s linear {behavior}" }}"#))
            };
            let mut clock = Clock::new();
            clock.frame(0, &style(r#"{ "transition": "top 1s linear allow-discrete" }"#));
            let end = inset("100", "allow-discrete");
            clock.frame(0, &end);
            assert_eq!(clock.frame(400, &end).top, Some(DimensionValue::Auto));
            assert_eq!(clock.frame(600, &end).top, Some(DimensionValue::Pixels(100.0)));

            let mut clock = Clock::new();
            clock.frame(0, &style(r#"{ "transition": "top 1s linear" }"#));
            assert_eq!(clock.frame(10, &inset("100", "")).top, Some(DimensionValue::Pixels(100.0)));
            assert!(clock.events().is_empty());
        }

        fn border(extra: &str) -> StyleDesc {
            style(&format!(r#"{{ {extra} "transition": "border-top-width 1s linear" }}"#))
        }

        #[test]
        fn a_border_style_appearing_starts_no_transition_on_an_unset_width() {
            let mut clock = Clock::new();
            clock.frame(0, &border(""));
            let solid = border(r#""borderTopStyle": "solid","#);
            assert_eq!(clock.frame(10, &solid).border_top_width, None);
            assert!(clock.events().is_empty());
        }

        #[test]
        fn an_unset_width_leaves_from_medium_with_or_without_a_style() {
            for extra in [r#""borderTopStyle": "solid","#, ""] {
                let mut clock = Clock::new();
                clock.frame(0, &border(extra));
                let wide = border(&format!(r#"{extra} "borderTopWidth": 7,"#));
                clock.frame(0, &wide);
                assert_eq!(clock.frame(500, &wide).border_top_width, Some(LinearLength::px(5.0)), "{extra}");
            }
        }

        #[test]
        fn an_unset_outline_width_leaves_from_medium() {
            let outline = |extra: &str| style(&format!(r#"{{ {extra} "transition": "outline-width 1s linear" }}"#));
            let mut clock = Clock::new();
            clock.frame(0, &outline(r#""outlineStyle": "solid","#));
            let wide = outline(r#""outlineStyle": "solid", "outlineWidth": 7,"#);
            clock.frame(0, &wide);
            assert_eq!(clock.frame(500, &wide).outline_width, Some(LinearLength::px(5.0)));
        }
    }

    // ── Web Animations controls ──────────────────────────────────────

    fn close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-9, "{actual} is not {expected}");
    }

    fn listed(clock: &Clock, millis: u64) -> Vec<Info> {
        clock.transitions.list(1, clock.at(millis))
    }

    fn only(clock: &Clock, millis: u64) -> Info {
        let mut listed = listed(clock, millis);
        assert_eq!(listed.len(), 1, "{listed:?}");
        listed.remove(0)
    }

    fn command(clock: &mut Clock, millis: u64, id: u64, command: Command) {
        let at = clock.at(millis);
        clock.transitions.command(id, command, at).expect("the transition exists").unwrap();
    }

    /// The finishes and cancels since the last call, whole.
    fn finishes_and_cancels(clock: &mut Clock) -> Vec<ApiEvent> {
        clock.transitions.take_api_events().into_iter().filter(|event| event.kind != ApiEventKind::Ready).collect()
    }

    fn api_events(clock: &mut Clock) -> Vec<(ApiEventKind, u64)> {
        clock
            .transitions
            .take_api_events()
            .into_iter()
            .filter(|event| event.kind != ApiEventKind::Ready)
            .map(|event| (event.kind, event.run))
            .collect()
    }

    /// A width transition of 0 to 100 that starts at 100 ms.
    fn moving() -> (Clock, StyleDesc, u64) {
        let mut clock = Clock::new();
        clock.frame(0, &sized(0.0, "width 1s linear"));
        let target = sized(100.0, "width 1s linear");
        clock.frame(100, &target);
        let id = only(&clock, 100).id;
        (clock, target, id)
    }

    #[test]
    fn a_transition_has_one_id_for_its_run_and_each_run_its_own() {
        let (mut clock, target, id) = moving();
        let info = only(&clock, 100);
        assert_eq!((info.kind, info.name.as_str()), (Kind::Transition, "width"));
        clock.frame(300, &target);
        clock.frame(600, &target);
        assert_eq!(only(&clock, 600).id, id);

        // Another target interrupts it: the old run is cancelled and a new one runs.
        let back = sized(10.0, "width 1s linear");
        clock.frame(700, &back);
        let replaced = only(&clock, 700).id;
        assert_ne!(replaced, id);
        assert_eq!(api_events(&mut clock), [(ApiEventKind::Cancel, id)]);
        let kept = clock.transitions.info(id, clock.at(700)).expect("JS holds it, so a command can bring it back");
        assert_eq!(kept.status, Status::Idle);
        clock.transitions.forget(id);
        assert!(clock.transitions.info(id, clock.at(700)).is_none());
        assert!(clock.transitions.command(id, Command::Pause, clock.at(700)).is_none());
    }

    #[test]
    fn a_transition_the_style_let_go_of_plays_again_by_a_command_and_drives_its_property() {
        let (mut clock, _, id) = moving();
        close(width_of(&clock.frame(500, &sized(100.0, "width 1s linear"))), 40.0);
        close(width_of(&clock.frame(600, &sized(100.0, "none"))), 100.0);
        assert!(listed(&clock, 600).is_empty());
        assert_eq!(clock.transitions.info(id, clock.at(600)).unwrap().status, Status::Idle);
        clock.events();

        command(&mut clock, 600, id, Command::Play);
        let none = sized(100.0, "none");
        close(width_of(&clock.frame(616, &none)), 0.0);
        let info = only_disowned(&clock, 616);
        assert_eq!((info.id, info.status), (id, Status::Running));
        close(width_of(&clock.frame(1116, &none)), 50.0);
        assert_eq!(clock.events(), [(Run, "width".to_string(), 0.0), (Start, "width".to_string(), 0.0)]);

        // The style moves the property and transitions it again: the one that plays drives it, so nothing starts.
        let moved = sized(30.0, "width 1s linear");
        close(width_of(&clock.frame(1216, &moved)), 60.0);
        assert!(listed(&clock, 1216).is_empty());
        close(width_of(&clock.frame(1700, &moved)), 30.0);
        assert_eq!(clock.events(), [(End, "width".to_string(), 1.0)]);
    }

    #[test]
    fn a_transition_the_style_let_go_of_that_is_sought_to_its_end_tells_the_dom_it_ended_once() {
        let (mut clock, _, id) = moving();
        clock.frame(500, &sized(100.0, "width 1s linear"));
        let none = sized(100.0, "none");
        clock.frame(600, &none);
        command(&mut clock, 600, id, Command::SetCurrentTime(1000.0));
        clock.events();
        clock.frame(616, &none);
        clock.frame(700, &none);
        assert_eq!(
            clock.events(),
            [(Run, "width".to_string(), 0.0), (Start, "width".to_string(), 0.0), (End, "width".to_string(), 1.0)],
            "held at its end it is not finished, and it ended"
        );
        let held = clock.transitions.info(id, clock.at(700)).unwrap();
        assert_eq!((held.status, held.current_time), (Status::Paused, 1000.0));
    }

    fn only_disowned(clock: &Clock, millis: u64) -> Info {
        let mut listed = clock.transitions.list_disowned(1, clock.at(millis));
        assert_eq!(listed.len(), 1, "{listed:?}");
        listed.remove(0)
    }

    #[test]
    fn pause_holds_the_time_and_the_value() {
        let (mut clock, target, id) = moving();
        close(width_of(&clock.frame(500, &target)), 40.0);
        command(&mut clock, 500, id, Command::Pause);
        let held = only(&clock, 500);
        assert_eq!(held.status, Status::Paused);
        close(held.current_time, 400.0);
        close(width_of(&clock.frame(900, &target)), 40.0);
        close(width_of(&clock.frame(2000, &target)), 40.0);
        assert_eq!(only(&clock, 2000).status, Status::Paused);
        clock.transitions.take_active();
        clock.frame(3000, &target);
        assert!(!clock.transitions.take_active(), "a held transition asks for no frames");
        assert!(api_events(&mut clock).is_empty());
        assert!(clock.events().iter().all(|(kind, ..)| *kind != End));
    }

    fn updated(clock: &mut Clock, millis: u64, id: u64, update: TimingUpdate) {
        let at = clock.at(millis);
        clock.transitions.update_timing(id, &update, at).expect("the transition exists");
    }

    #[test]
    fn an_updated_timing_changes_that_transition_and_the_keyframes_are_its_start_and_end() {
        let (mut clock, target, id) = moving();
        let info = clock.transitions.effect(id, clock.at(100)).unwrap().unwrap();
        assert_eq!((info.duration, info.fill, info.easing.as_str()), (Some(1000.0), "backwards", "linear"));
        let keyframes = clock.transitions.keyframes(id).unwrap().unwrap();
        let shown: Vec<(f64, &str, &str)> =
            keyframes.iter().map(|frame| (frame.offset, frame.composite, frame.values[0].1.as_str())).collect();
        assert_eq!(shown, [(0.0, "auto", "0px"), (1.0, "auto", "100px")]);

        updated(&mut clock, 100, id, TimingUpdate { duration: Some(DurationUpdate::Seconds(2.0)), ..Default::default() });
        close(width_of(&clock.frame(1100, &target)), 50.0);
        // A change of the transition properties of the style does not reach a running transition.
        let slower = sized(100.0, "width 9s linear");
        close(width_of(&clock.frame(1100, &slower)), 50.0);
        assert_eq!(clock.transitions.effect(id, clock.at(1100)).unwrap().unwrap().duration, Some(2000.0));
    }

    #[test]
    fn more_iterations_and_an_alternate_direction_run_the_transition_back() {
        let (mut clock, target, id) = moving();
        updated(
            &mut clock,
            100,
            id,
            TimingUpdate { iterations: Some(IterationCount::Finite(2.0)), direction: Some(Direction::Alternate), ..Default::default() },
        );
        close(width_of(&clock.frame(1600, &target)), 50.0);
        let info = clock.transitions.effect(id, clock.at(1600)).unwrap().unwrap();
        assert_eq!((info.end_time, info.current_iteration), (2000.0, Some(1)));
    }

    #[test]
    fn play_is_pending_until_the_next_frame_and_then_continues() {
        let (mut clock, target, id) = moving();
        clock.frame(500, &target);
        command(&mut clock, 500, id, Command::Pause);
        clock.frame(900, &target);
        clock.transitions.take_active();

        command(&mut clock, 900, id, Command::Play);
        close(only(&clock, 900).current_time, 400.0);
        close(width_of(&clock.frame(1000, &target)), 40.0);
        assert!(clock.transitions.take_active());
        close(width_of(&clock.frame(1200, &target)), 60.0);
    }

    #[test]
    fn seek_sets_the_time_at_once_and_the_next_frame_paints_it() {
        let (mut clock, target, id) = moving();
        command(&mut clock, 100, id, Command::SetCurrentTime(750.0));
        close(only(&clock, 100).current_time, 750.0);
        close(width_of(&clock.frame(200, &target)), 85.0);

        command(&mut clock, 200, id, Command::Pause);
        command(&mut clock, 200, id, Command::SetCurrentTime(200.0));
        close(width_of(&clock.frame(900, &target)), 20.0);
    }

    #[test]
    fn a_delay_is_part_of_the_time_too() {
        let mut clock = Clock::new();
        clock.frame(0, &sized(0.0, "width 1s linear 500ms"));
        let target = sized(100.0, "width 1s linear 500ms");
        close(width_of(&clock.frame(0, &target)), 0.0);
        let id = only(&clock, 0).id;
        close(width_of(&clock.frame(750, &target)), 25.0);
        close(only(&clock, 750).current_time, 750.0);
        command(&mut clock, 750, id, Command::SetCurrentTime(1000.0));
        close(width_of(&clock.frame(750, &target)), 50.0);
    }

    #[test]
    fn a_rate_scales_time_and_keeps_it_continuous() {
        let (mut clock, target, id) = moving();
        command(&mut clock, 100, id, Command::SetPlaybackRate(2.0));
        close(width_of(&clock.frame(350, &target)), 50.0);
        command(&mut clock, 350, id, Command::SetPlaybackRate(0.5));
        close(only(&clock, 350).current_time, 500.0);
        close(width_of(&clock.frame(850, &target)), 75.0);
        assert_eq!(only(&clock, 850).playback_rate, 0.5);
    }

    #[test]
    fn a_faster_rate_finishes_sooner_with_the_events() {
        let (mut clock, target, id) = moving();
        clock.events();
        command(&mut clock, 100, id, Command::SetPlaybackRate(2.0));
        close(width_of(&clock.frame(400, &target)), 60.0);
        close(width_of(&clock.frame(700, &target)), 100.0);
        assert_eq!(api_events(&mut clock), [(ApiEventKind::Finish, id)]);
        assert_eq!(clock.events(), [(End, "width".to_string(), 1.0)]);
        assert!(listed(&clock, 700).is_empty(), "a finished transition is not listed");
        assert_eq!(clock.transitions.info(id, clock.at(700)).unwrap().status, Status::Finished);
    }

    /// The transition of `moving`, run back from 600 ms, and finished at its start.
    fn held_at_its_start() -> (Clock, StyleDesc, u64) {
        let (mut clock, target, id) = moving();
        clock.frame(600, &target);
        clock.events();
        command(&mut clock, 600, id, Command::SetPlaybackRate(-1.0));
        clock.frame(1200, &target);
        (clock, target, id)
    }

    #[test]
    fn a_negative_rate_runs_back() {
        let (mut clock, target, id) = moving();
        clock.frame(600, &target);
        command(&mut clock, 600, id, Command::SetPlaybackRate(-1.0));
        close(width_of(&clock.frame(700, &target)), 40.0);
        assert_eq!(only(&clock, 700).status, Status::Running);
        assert!(clock.transitions.take_active());
        assert!(api_events(&mut clock).is_empty());
    }

    #[test]
    fn a_transition_run_back_to_its_start_stays_in_effect_with_the_value_it_started_from() {
        let (mut clock, target, id) = held_at_its_start();
        // The other edge of the active interval: `transitionend`.
        assert_eq!(clock.events(), [(End, "width".to_string(), 0.0)]);
        let events = finishes_and_cancels(&mut clock);
        assert_eq!(events.len(), 1);
        assert_eq!((events[0].kind, events[0].run), (ApiEventKind::Finish, id));
        assert_eq!(events[0].info.status, Status::Finished);

        let held = only(&clock, 1200);
        assert_eq!((held.id, held.status, held.playback_rate), (id, Status::Finished, -1.0));
        close(held.current_time, 0.0);
        clock.transitions.take_active();
        close(width_of(&clock.frame(1300, &target)), 0.0);
        close(width_of(&clock.frame(5000, &target)), 0.0);
        assert!(!clock.transitions.take_active(), "it asks for no frames");
        assert!(api_events(&mut clock).is_empty());
        assert!(clock.events().is_empty());
        assert_eq!(only(&clock, 5000).id, id);
    }

    #[test]
    fn a_style_change_cancels_a_transition_held_at_its_start_and_a_new_one_starts_from_there() {
        let (mut clock, _target, id) = held_at_its_start();
        clock.transitions.take_api_events();
        clock.events();
        let elsewhere = sized(60.0, "width 1s linear");
        close(width_of(&clock.frame(1300, &elsewhere)), 0.0);
        let events = finishes_and_cancels(&mut clock);
        assert_eq!(events.len(), 1);
        assert_eq!((events[0].kind, events[0].run), (ApiEventKind::Cancel, id));
        assert_eq!(events[0].info.status, Status::Idle);
        let next = only(&clock, 1300);
        assert_ne!(next.id, id);
        close(width_of(&clock.frame(1800, &elsewhere)), 30.0);
    }

    #[test]
    fn the_same_style_leaves_a_transition_held_at_its_start() {
        let (mut clock, target, id) = held_at_its_start();
        clock.transitions.take_api_events();
        clock.events();
        close(width_of(&clock.frame(1300, &target)), 0.0);
        assert!(api_events(&mut clock).is_empty());
        assert_eq!(only(&clock, 1300).id, id);
    }

    #[test]
    fn play_on_a_transition_held_at_its_start_goes_to_its_end_and_finishes_there() {
        let (mut clock, target, id) = held_at_its_start();
        clock.transitions.take_api_events();
        clock.events();
        command(&mut clock, 1200, id, Command::Play);
        close(only(&clock, 1200).current_time, 1000.0);
        close(width_of(&clock.frame(1300, &target)), 100.0);
        close(width_of(&clock.frame(1800, &target)), 50.0);
        close(width_of(&clock.frame(2400, &target)), 0.0);
        assert_eq!(api_events(&mut clock), [(ApiEventKind::Finish, id)]);
        // It starts running back from the end, and ends at its start.
        assert_eq!(clock.events(), [(Start, "width".to_string(), 0.0), (End, "width".to_string(), 0.0)]);
    }

    #[test]
    fn a_held_transition_runs_forward_again_with_a_positive_rate_and_ends_at_its_end() {
        let (mut clock, target, id) = held_at_its_start();
        clock.transitions.take_api_events();
        clock.events();
        command(&mut clock, 1200, id, Command::SetPlaybackRate(1.0));
        assert_eq!(only(&clock, 1200).status, Status::Running);
        close(width_of(&clock.frame(1700, &target)), 50.0);
        clock.frame(2300, &target);
        assert_eq!(api_events(&mut clock), [(ApiEventKind::Finish, id)]);
        assert_eq!(clock.events(), [(Start, "width".to_string(), 0.0), (End, "width".to_string(), 1.0)]);
        assert!(listed(&clock, 2300).is_empty());
    }

    #[test]
    fn a_transition_that_finished_can_be_read_and_played_again() {
        let (mut clock, target, id) = moving();
        clock.frame(1200, &target);
        clock.transitions.take_api_events();
        clock.events();
        assert!(listed(&clock, 1200).is_empty());
        let finished = clock.transitions.info(id, clock.at(1200)).unwrap();
        assert_eq!(finished.status, Status::Finished);
        close(finished.current_time, 1000.0);

        command(&mut clock, 1200, id, Command::Play);
        let again = only(&clock, 1200);
        assert_eq!((again.id, again.status), (id, Status::Running));
        close(again.current_time, 0.0);
        close(width_of(&clock.frame(1300, &target)), 0.0);
        close(width_of(&clock.frame(1800, &target)), 50.0);
        clock.frame(2400, &target);
        assert_eq!(api_events(&mut clock), [(ApiEventKind::Finish, id)]);
        // Played again, it is in its active interval again: the DOM is told it started.
        assert_eq!(clock.events(), [(Start, "width".to_string(), 1.0), (End, "width".to_string(), 1.0)]);
        assert!(listed(&clock, 2400).is_empty());
        assert_eq!(clock.transitions.info(id, clock.at(2400)).unwrap().status, Status::Finished);
    }

    #[test]
    fn pause_and_seek_on_a_transition_that_finished() {
        let (mut clock, target, id) = moving();
        clock.frame(1200, &target);
        clock.transitions.take_api_events();
        command(&mut clock, 1200, id, Command::Pause);
        let held = clock.transitions.info(id, clock.at(1200)).unwrap();
        assert_eq!(held.status, Status::Paused);
        assert!(listed(&clock, 1200).is_empty(), "paused at its end it is not current");
        close(width_of(&clock.frame(1300, &target)), 100.0);

        command(&mut clock, 1300, id, Command::SetCurrentTime(500.0));
        assert_eq!(only(&clock, 1300).id, id);
        close(width_of(&clock.frame(1400, &target)), 50.0);
        command(&mut clock, 1400, id, Command::SetCurrentTime(1000.0));
        assert!(listed(&clock, 1400).is_empty());
        close(width_of(&clock.frame(1500, &target)), 100.0);
        assert!(api_events(&mut clock).is_empty(), "held, it finishes nothing");
    }

    #[test]
    fn a_rate_on_a_transition_that_finished_runs_it_back_or_leaves_it_finished() {
        let (mut clock, target, id) = moving();
        clock.frame(1200, &target);
        clock.transitions.take_api_events();
        clock.events();
        command(&mut clock, 1200, id, Command::SetPlaybackRate(2.0));
        assert!(listed(&clock, 1200).is_empty());
        clock.frame(1300, &target);
        clock.frame(1900, &target);
        assert!(api_events(&mut clock).is_empty(), "it was finished before and is now");
        assert!(clock.events().is_empty());
        assert_eq!(clock.transitions.info(id, clock.at(1900)).unwrap().status, Status::Finished);

        command(&mut clock, 1900, id, Command::SetPlaybackRate(-1.0));
        assert_eq!(only(&clock, 1900).status, Status::Running);
        close(width_of(&clock.frame(2400, &target)), 50.0);
    }

    #[test]
    fn a_style_change_cancels_a_transition_that_finished() {
        let (mut clock, target, id) = moving();
        clock.frame(1200, &target);
        clock.transitions.take_api_events();
        clock.events();
        clock.frame(1300, &target);
        assert!(api_events(&mut clock).is_empty());

        clock.frame(1400, &sized(100.0, "width 2s linear"));
        assert!(api_events(&mut clock).is_empty(), "the end value is the same");
        clock.frame(1500, &sized(80.0, "width 2s linear"));
        let events = finishes_and_cancels(&mut clock);
        assert_eq!(events.iter().map(|event| (event.kind, event.run)).collect::<Vec<_>>(), [(ApiEventKind::Cancel, id)]);
        assert_eq!(events[0].info.status, Status::Idle);
        assert_eq!(clock.transitions.info(id, clock.at(1500)).unwrap().status, Status::Idle);
        clock.transitions.forget(id);
        assert!(clock.transitions.info(id, clock.at(1500)).is_none());
        assert!(clock.transitions.command(id, Command::Play, clock.at(1500)).is_none());
        assert!(clock.events().iter().all(|(kind, ..)| *kind != Cancel), "the DOM has no cancel for it");
    }

    #[test]
    fn a_command_names_its_element_for_the_next_frame_to_build() {
        let (mut clock, target, id) = moving();
        clock.frame(1200, &target);
        assert!(clock.transitions.take_commanded().is_empty());
        command(&mut clock, 1200, id, Command::Play);
        assert_eq!(clock.transitions.take_commanded(), [1], "it finished, and the frame before could not tell it runs");
        assert!(clock.transitions.take_commanded().is_empty());
    }

    #[test]
    fn a_transition_that_finished_is_cancelled_when_it_is_no_longer_declared() {
        let (mut clock, target, id) = moving();
        clock.frame(1200, &target);
        clock.transitions.take_api_events();
        clock.frame(1300, &style(r#"{ "width": 100 }"#));
        assert_eq!(api_events(&mut clock), [(ApiEventKind::Cancel, id)]);
    }

    #[test]
    fn play_after_a_seek_to_the_end_rewinds_a_paused_transition() {
        let (mut clock, target, id) = moving();
        clock.frame(300, &target);
        command(&mut clock, 300, id, Command::Pause);
        command(&mut clock, 300, id, Command::SetCurrentTime(1000.0));
        clock.frame(400, &target);
        assert!(listed(&clock, 400).is_empty(), "paused at its end it is not current");
        let at_end = clock.transitions.info(id, clock.at(400)).unwrap();
        assert_eq!(at_end.status, Status::Paused, "held at its end, it has not finished");
        close(at_end.current_time, 1000.0);
        assert!(api_events(&mut clock).is_empty());

        command(&mut clock, 400, id, Command::Play);
        close(only(&clock, 400).current_time, 0.0);
        assert_eq!(only(&clock, 400).status, Status::Running);
        clock.frame(500, &target);
        close(width_of(&clock.frame(750, &target)), 25.0);
    }

    #[test]
    fn a_seek_to_the_end_of_a_running_transition_finishes_it() {
        let (mut clock, target, id) = moving();
        command(&mut clock, 100, id, Command::SetCurrentTime(1500.0));
        assert_eq!(clock.transitions.info(id, clock.at(100)).unwrap().status, Status::Finished);
        close(width_of(&clock.frame(200, &target)), 100.0);
        assert_eq!(api_events(&mut clock), [(ApiEventKind::Finish, id)]);
    }

    #[test]
    fn transitions_are_listed_by_the_change_that_started_them_then_by_name() {
        let mut clock = Clock::new();
        let declare = |width: f64, opacity: f64, height: f64| {
            style(&format!(
                r#"{{ "width": {width}, "opacity": {opacity}, "height": {height}, "transition": "all 1s linear" }}"#
            ))
        };
        clock.frame(0, &declare(0.0, 1.0, 0.0));
        clock.frame(100, &declare(100.0, 0.5, 0.0));
        let names = |clock: &Clock, millis| {
            listed(clock, millis).into_iter().map(|info| info.name).collect::<Vec<_>>()
        };
        assert_eq!(names(&clock, 100), ["opacity", "width"]);

        // A later change adds `height` after both, though it sorts before `width`.
        clock.frame(300, &declare(100.0, 0.5, 50.0));
        assert_eq!(names(&clock, 300), ["opacity", "width", "height"]);
        let ids: Vec<u64> = listed(&clock, 300).iter().map(|info| info.id).collect();
        clock.frame(350, &declare(100.0, 0.5, 50.0));
        assert_eq!(listed(&clock, 350).iter().map(|info| info.id).collect::<Vec<_>>(), ids);

        // Finished ones are gone; the later one stays.
        clock.frame(1200, &declare(100.0, 0.5, 50.0));
        assert_eq!(names(&clock, 1200), ["height"]);
    }

    #[test]
    fn the_events_of_one_time_come_in_the_order_of_the_transitions_and_ends_by_their_time() {
        let mut clock = Clock::new();
        let declare = |width: f64, opacity: f64| {
            style(&format!(
                r#"{{ "width": {width}, "opacity": {opacity}, "transition": "width 1s linear, opacity 2s linear" }}"#
            ))
        };
        clock.frame(0, &declare(0.0, 1.0));
        clock.frame(100, &declare(100.0, 0.5));
        let event = |kind, name: &str, elapsed| (kind, name.to_string(), elapsed);
        assert_eq!(
            clock.events(),
            [
                event(Run, "opacity", 0.0),
                event(Start, "opacity", 0.0),
                event(Run, "width", 0.0),
                event(Start, "width", 0.0),
            ],
            "each starts right after its run, in the order of their names"
        );
        clock.frame(3100, &declare(100.0, 0.5));
        assert_eq!(clock.events(), [event(End, "width", 1.0), event(End, "opacity", 2.0)]);
    }

    #[test]
    fn display_none_cancels_for_the_api_too() {
        let mut tree = crate::retained_tree::RetainedTree::new();
        tree.create_element(1, "div".to_string());
        let (mut clock, _target, id) = moving();
        clock.transitions.hide(&tree, 1, clock.at(200));
        assert_eq!(api_events(&mut clock), [(ApiEventKind::Cancel, id)]);
    }

    #[test]
    fn display_none_cancels_a_finished_transition_too() {
        let mut tree = crate::retained_tree::RetainedTree::new();
        tree.create_element(1, "div".to_string());
        let (mut clock, target, id) = moving();
        clock.frame(1200, &target);
        clock.transitions.take_api_events();
        clock.transitions.hide(&tree, 1, clock.at(1300));
        assert_eq!(api_events(&mut clock), [(ApiEventKind::Cancel, id)]);
    }

    #[test]
    fn a_bad_value_is_refused() {
        let (mut clock, _target, id) = moving();
        let at = clock.at(100);
        assert!(clock.transitions.command(id, Command::SetCurrentTime(f64::NAN), at).unwrap().is_err());
        assert!(clock.transitions.command(id, Command::SetPlaybackRate(f64::NAN), at).unwrap().is_err());
        assert!(clock.transitions.command(u64::MAX, Command::Pause, at).is_none());
    }

    // ── SVG properties ───────────────────────────────────────────────

    fn attributes(pairs: &[(&str, serde_json::Value)]) -> HashMap<String, serde_json::Value> {
        pairs.iter().map(|(name, value)| (name.to_string(), value.clone())).collect()
    }

    impl Clock {
        /// One frame of a shape at `millis`: the SVG attributes its transitions write.
        fn shape(
            &mut self,
            millis: u64,
            declared: &StyleDesc,
            own: &HashMap<String, serde_json::Value>,
            ancestors: &[&HashMap<String, serde_json::Value>],
        ) -> Vec<(&'static str, String)> {
            let source = SvgSource { own, ancestors };
            self.transitions
                .animate_shape(1, declared, &source, &self.parent_colors, &self.parent_font, None, &[], self.at(millis))
                .map(|running| running.svg_attributes())
                .unwrap_or_default()
        }
    }

    fn written(attributes: &[(&'static str, String)], name: &str) -> Option<String> {
        attributes.iter().find(|(attribute, _)| *attribute == name).map(|(_, text)| text.clone())
    }

    #[test]
    fn a_css_name_stands_for_the_style_and_the_svg_property_it_names() {
        assert_eq!(css_props("stroke-width"), [PropKey::Svg(SvgAttr::StrokeWidth)]);
        assert_eq!(css_props("fill"), [PropKey::Svg(SvgAttr::Fill)]);
        assert_eq!(
            css_props("opacity"),
            [PropKey::Style(AnimProp::Opacity), PropKey::Svg(SvgAttr::Opacity)],
            "a shape's opacity is the SVG one and a box's the style one"
        );
        assert!(css_props("x1").is_empty(), "an attribute that is no CSS property is not transitioned");
        assert_eq!(css_props("perspective"), [PropKey::Style(AnimProp::Perspective)], "a style property that no SVG attribute shares");
    }

    #[test]
    fn a_shape_transitions_an_attribute_that_changed_and_ends_on_it() {
        let mut clock = Clock::new();
        let declared = style(r#"{ "transition": "r 1s linear" }"#);
        let small = attributes(&[("r", serde_json::json!(5))]);
        let large = attributes(&[("r", serde_json::json!(15))]);
        assert!(clock.shape(0, &declared, &small, &[]).is_empty());
        assert!(clock.events().is_empty(), "the first frame never transitions");

        assert_eq!(written(&clock.shape(100, &declared, &large, &[]), "r").as_deref(), Some("5"));
        assert_eq!(clock.events(), [(Run, "r".to_string(), 0.0), (Start, "r".to_string(), 0.0)]);
        assert_eq!(written(&clock.shape(600, &declared, &large, &[]), "r").as_deref(), Some("10"));
        assert!(clock.shape(1200, &declared, &large, &[]).is_empty());
        assert_eq!(clock.events(), [(End, "r".to_string(), 1.0)]);
        assert_eq!(clock.transitions.list(1, clock.at(1200)).len(), 0);
    }

    #[test]
    fn a_shape_lists_its_transitions_by_the_css_name() {
        let mut clock = Clock::new();
        let declared = style(r#"{ "transition": "all 1s linear" }"#);
        let before = attributes(&[("strokeWidth", serde_json::json!(2)), ("fill", serde_json::json!("#ff0000"))]);
        let after = attributes(&[("strokeWidth", serde_json::json!(6)), ("fill", serde_json::json!("#0000ff"))]);
        clock.shape(0, &declared, &before, &[]);
        let running = clock.shape(250, &declared, &after, &[]);
        assert_eq!(written(&running, "stroke-width").as_deref(), Some("2"));
        let names: Vec<String> = clock.transitions.list(1, clock.at(250)).into_iter().map(|info| info.name).collect();
        assert_eq!(names, ["fill", "stroke-width"]);
        assert!(clock.transitions.paints_only(1));
        assert!(clock.transitions.shape_moves(1));
        assert_eq!(clock.transitions.svg_shapes().collect::<Vec<_>>(), [1]);
    }

    #[test]
    fn what_cannot_blend_waits_for_allow_discrete_and_then_swaps_halfway() {
        let none = attributes(&[("fill", serde_json::json!("none"))]);
        let red = attributes(&[("fill", serde_json::json!("#ff0000"))]);
        let mut clock = Clock::new();
        let normal = style(r#"{ "transition": "fill 1s linear" }"#);
        clock.shape(0, &normal, &none, &[]);
        assert!(clock.shape(100, &normal, &red, &[]).is_empty());
        assert!(clock.events().is_empty());

        let mut clock = Clock::new();
        let discrete = style(r#"{ "transition": "fill 1s linear allow-discrete" }"#);
        clock.shape(0, &discrete, &none, &[]);
        assert_eq!(written(&clock.shape(100, &discrete, &red, &[]), "fill").as_deref(), Some("none"));
        assert_eq!(written(&clock.shape(400, &discrete, &red, &[]), "fill").as_deref(), Some("none"));
        let flipped = written(&clock.shape(600, &discrete, &red, &[]), "fill").unwrap();
        assert!(flipped.starts_with("rgba(255"), "{flipped}");
    }

    #[test]
    fn paths_of_other_commands_and_a_dash_list_against_none_are_discrete() {
        let mut clock = Clock::new();
        let normal = style(r#"{ "transition": "d 1s linear, stroke-dasharray 1s linear" }"#);
        let before = attributes(&[("d", serde_json::json!("M0 0 L10 0")), ("strokeDasharray", serde_json::json!("none"))]);
        let after = attributes(&[("d", serde_json::json!("M0 0 H10")), ("strokeDasharray", serde_json::json!("4 2"))]);
        clock.shape(0, &normal, &before, &[]);
        assert!(clock.shape(100, &normal, &after, &[]).is_empty());

        let blending = attributes(&[("d", serde_json::json!("M0 0 L30 0"))]);
        let mut clock = Clock::new();
        clock.shape(0, &normal, &before, &[]);
        assert_eq!(written(&clock.shape(500, &normal, &blending, &[]), "d").as_deref(), Some("M 0 0 L 10 0"));
        assert_eq!(written(&clock.shape(1000, &normal, &blending, &[]), "d").as_deref(), Some("M 0 0 L 20 0"));
    }

    #[test]
    fn a_shape_follows_the_inherited_value_it_does_not_set() {
        let declared = style(r#"{ "transition": "fill 1s linear" }"#);
        let red = attributes(&[("fill", serde_json::json!("#ff0000"))]);
        let blue = attributes(&[("fill", serde_json::json!("#0000ff"))]);
        let none = attributes(&[]);
        let mut clock = Clock::new();
        clock.shape(0, &declared, &none, &[&red]);
        let running = clock.shape(100, &declared, &none, &[&blue]);
        assert!(written(&running, "fill").unwrap().starts_with("rgba(255"), "{running:?}");

        // A shape that sets its own value does not follow the group, and neither does a property that does not inherit.
        let own = attributes(&[("fill", serde_json::json!("#00ff00"))]);
        let mut clock = Clock::new();
        clock.shape(0, &declared, &own, &[&red]);
        assert!(clock.shape(100, &declared, &own, &[&blue]).is_empty());
        let opacity = style(r#"{ "transition": "opacity 1s linear" }"#);
        let (dim, bright) = (attributes(&[("opacity", serde_json::json!(0.2))]), attributes(&[("opacity", serde_json::json!(1))]));
        let mut clock = Clock::new();
        clock.shape(0, &opacity, &none, &[&bright]);
        assert!(clock.shape(100, &opacity, &none, &[&dim]).is_empty());
    }

    #[test]
    fn the_svg_transitions_its_styles_and_its_attributes_together() {
        let mut clock = Clock::new();
        let declared = |width: f64| style(&format!(r#"{{ "width": {width}, "transition": "width 1s linear, fill 1s linear" }}"#));
        let red = attributes(&[("fill", serde_json::json!("#ff0000"))]);
        let blue = attributes(&[("fill", serde_json::json!("#0000ff"))]);
        let step = |clock: &mut Clock, millis, width, own: &HashMap<String, serde_json::Value>| {
            let source = SvgSource { own, ancestors: &[] };
            let after = declared(width);
            clock.transitions.update(
                1,
                &after,
                Some((&source, PropTarget::SvgHost)),
                &clock.parent_colors.clone(),
                &clock.parent_font.clone(),
                None,
                None,
                InterpolateSize::default(),
                Scope::Element,
                &[],
                clock.at(millis),
            )
        };
        assert!(step(&mut clock, 0, 100.0, &red).is_none());
        let running = step(&mut clock, 500, 200.0, &blue).expect("both change");
        let mut resolved = declared(200.0);
        running.apply_to(&mut resolved);
        assert_eq!(width_of(&resolved), 100.0);
        assert!(written(&running.svg_attributes(), "fill").unwrap().starts_with("rgba(255"));
        let names: Vec<String> = clock.transitions.list(1, clock.at(500)).into_iter().map(|info| info.name).collect();
        assert_eq!(names, ["fill", "width"]);
        assert!(!clock.transitions.svg_shapes().any(|id| id == 1), "the svg is built itself");
    }

    #[test]
    fn a_property_the_svg_shares_a_name_with_a_style_is_the_style() {
        let target = PropTarget::SvgHost;
        assert!(target.takes(PropKey::Style(AnimProp::Opacity)) && !target.takes(PropKey::Svg(SvgAttr::Opacity)));
        assert!(target.takes(PropKey::Svg(SvgAttr::Fill)));
        let shape = PropTarget::SvgShape;
        assert!(shape.takes(PropKey::Svg(SvgAttr::Opacity)) && !shape.takes(PropKey::Style(AnimProp::Opacity)));
    }
}
