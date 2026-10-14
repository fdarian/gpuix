//! CSS transitions (CSS Transitions Level 1) for style props.
//!
//! Each frame the renderer hands an element's after-change style (state styles
//! already folded in) to [`CssTransitions::update`]. A change in the value of a
//! transitionable property starts a transition from the value the element
//! showed to the new one; the returned [`TransitionStyle`] holds the values of
//! the transitions still running, to lay over that style.

use std::collections::{BTreeMap, HashMap};

use web_time::Instant;

use super::curve::{ease, parse_timing_function, Ease};
use super::property::{expand_shorthand, AnimProp};
use super::value::{pair, AnimValue, Pair};
use super::MotionState;
use crate::style::StyleDesc;

// ── Parsing ──────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
enum Matcher {
    All,
    /// The animatable properties a CSS name stands for. Empty for a name that
    /// is a valid property but not one gpuix animates (`transform`), which
    /// matches nothing, as an unsupported property does in a browser.
    Props(Vec<AnimProp>),
}

#[derive(Clone, Debug, PartialEq)]
struct Entry {
    matcher: Matcher,
    /// Seconds, never negative.
    duration: f64,
    /// Seconds. A negative delay starts the transition part-way through.
    delay: f64,
    timing: Ease,
}

impl Entry {
    fn matches(&self, prop: AnimProp) -> bool {
        match &self.matcher {
            Matcher::All => true,
            Matcher::Props(props) => props.contains(&prop),
        }
    }
}

/// The resolved `transition-*` lists of one element. Empty for `none`.
#[derive(Clone, Debug, Default, PartialEq)]
struct TransitionList(Vec<Entry>);

impl TransitionList {
    /// The last entry that names `prop`, as CSS picks the matching one.
    fn matching(&self, prop: AnimProp) -> Option<&Entry> {
        self.0.iter().rev().find(|entry| entry.matches(prop))
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

/// The four `transition-*` lists. They are as long as each was written; the
/// property list sets the count and the others repeat or truncate to it.
struct Lists {
    properties: Properties,
    durations: Vec<f64>,
    timings: Vec<Ease>,
    delays: Vec<f64>,
}

impl Lists {
    fn initial() -> Self {
        Self {
            properties: Properties::List(vec![PropertyName::All]),
            durations: vec![0.0],
            timings: vec![default_timing()],
            delays: vec![0.0],
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
        };
        for item in &items {
            let mut property = None;
            let mut duration = None;
            let mut delay = None;
            let mut timing = None;
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
                })
                .collect(),
        )
    }
}

fn default_timing() -> Ease {
    Ease::CubicBezier([0.25, 0.1, 0.25, 1.0])
}

/// Splits on `separator`, ignoring any inside parentheses. Empty items are an
/// error, as `a,,b` is in CSS.
fn split_outside_parens(text: &str, separator: char) -> Result<Vec<&str>, String> {
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

fn split_whitespace_outside_parens(text: &str) -> Vec<&str> {
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

fn parse_list<T>(text: &str, parse: impl Fn(&str) -> Result<T, String>) -> Result<Vec<T>, String> {
    split_outside_parens(text, ',')?
        .into_iter()
        .map(|item| parse(item))
        .collect()
}

/// `<time>`: a number with `s` or `ms`. A bare `0` is not a time in CSS.
fn parse_time(text: &str) -> Option<f64> {
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

fn check_seconds(seconds: f64, text: &str) -> Result<f64, String> {
    if seconds.abs() > 1e9 {
        return Err(format!("{text} is out of range"));
    }
    Ok(seconds)
}

fn parse_duration(text: &str) -> Result<f64, String> {
    let seconds = parse_time(text).ok_or_else(|| format!("not a time: {text}"))?;
    if seconds < 0.0 {
        return Err(format!("a duration cannot be negative: {text}"));
    }
    check_seconds(seconds, text)
}

fn parse_delay(text: &str) -> Result<f64, String> {
    let seconds = parse_time(text).ok_or_else(|| format!("not a time: {text}"))?;
    check_seconds(seconds, text)
}

fn is_timing_function(token: &str) -> bool {
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
fn css_props(ident: &str) -> Vec<AnimProp> {
    use AnimProp::*;
    match ident {
        "inset" => vec![Top, Right, Bottom, Left],
        "border" => vec![
            BorderTopWidth,
            BorderRightWidth,
            BorderBottomWidth,
            BorderLeftWidth,
            BorderColor,
        ],
        "outline" => vec![OutlineWidth, OutlineColor],
        "flex" => vec![FlexGrow, FlexShrink, FlexBasis],
        "background" => vec![BackgroundColor],
        _ => {
            let camel = camel_case(ident);
            expand_shorthand(&camel)
                .map(<[AnimProp]>::to_vec)
                .or_else(|| AnimProp::from_name(&camel).map(|prop| vec![prop]))
                .unwrap_or_default()
        }
    }
}

/// Rejects a style whose `transition*` values, in it or in a state style, do
/// not parse, so a bad value fails where it is set instead of silently never
/// running.
pub(crate) fn validate_transitions(style: &StyleDesc) -> Result<(), String> {
    TransitionList::parse(style)?;
    for (name, state) in [
        ("hover", &style.hover),
        ("active", &style.active),
        ("focusVisible", &style.focus_visible),
    ] {
        if let Some(state) = state {
            TransitionList::parse(state).map_err(|error| format!("{name}.{error}"))?;
        }
    }
    Ok(())
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
        || style.transition_delay.is_some();
    set && !(only_none(&style.transition)
        && only_none(&style.transition_property)
        && style.transition_duration.is_none()
        && style.transition_timing_function.is_none()
        && style.transition_delay.is_none())
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
            Self::Run => "transitionRun",
            Self::Start => "transitionStart",
            Self::End => "transitionEnd",
            Self::Cancel => "transitionCancel",
        }
    }
}

/// A DOM `TransitionEvent`, minus `pseudoElement`, which is always `""`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TransitionEvent {
    pub(crate) id: u64,
    pub(crate) kind: TransitionEventKind,
    prop: AnimProp,
    /// Seconds.
    pub(crate) elapsed: f64,
}

impl TransitionEvent {
    /// The CSS name, `background-color`.
    pub(crate) fn property_name(&self) -> String {
        kebab_case(self.prop.name())
    }
}

#[derive(Clone, Debug)]
struct Running {
    started: Instant,
    delay: f64,
    duration: f64,
    timing: Ease,
    segment: Pair,
    end: AnimValue,
    /// CSS "reversing-adjusted start value": where an interrupting change
    /// back to it counts as a reversal.
    reversing_start: AnimValue,
    shortening: f64,
    began: bool,
}

impl Running {
    /// `None` when the two values cannot be interpolated, which in CSS means
    /// the change is simply not transitioned.
    fn new(
        prop: AnimProp,
        from: &AnimValue,
        to: &AnimValue,
        entry: &Entry,
        reversing_start: AnimValue,
        shortening: f64,
        now: Instant,
    ) -> Option<Self> {
        let segment = pair(prop.name(), from, to).ok().filter(|pair| !pair.is_discrete())?;
        Some(Self {
            started: now,
            delay: if entry.delay >= 0.0 {
                entry.delay
            } else {
                entry.delay * shortening
            },
            duration: entry.duration * shortening,
            timing: entry.timing.clone(),
            segment,
            end: to.clone(),
            reversing_start,
            shortening,
            began: false,
        })
    }

    fn elapsed(&self, now: Instant) -> f64 {
        now.saturating_duration_since(self.started).as_secs_f64()
    }

    /// The eased progress. Before the delay ends it holds the start (the
    /// implicit `backwards` fill), after the end it holds the end.
    fn progress(&self, elapsed: f64) -> f64 {
        let linear = if elapsed < self.delay {
            0.0
        } else if self.duration <= 0.0 {
            1.0
        } else {
            ((elapsed - self.delay) / self.duration).clamp(0.0, 1.0)
        };
        ease(linear, &self.timing)
    }

    fn value(&self, now: Instant) -> AnimValue {
        self.segment.at(self.progress(self.elapsed(now)))
    }

    /// `elapsedTime` of `transitionrun` and `transitionstart`.
    fn initial_elapsed(&self) -> f64 {
        (-self.delay).min(self.duration).max(0.0)
    }
}

/// The five `transition*` strings, compared to notice a change without
/// reparsing every frame.
type ConfigKey = [Option<String>; 5];

fn config_key(style: &StyleDesc) -> ConfigKey {
    [
        style.transition.clone(),
        style.transition_property.clone(),
        style.transition_duration.clone(),
        style.transition_timing_function.clone(),
        style.transition_delay.clone(),
    ]
}

struct ElementState {
    /// The previous frame's after-change style: the before-change style of the
    /// next style change.
    previous: StyleDesc,
    config: ConfigKey,
    list: TransitionList,
    running: BTreeMap<AnimProp, Running>,
    /// End values of finished transitions, so a style that still has the end
    /// value does not start the transition again.
    completed: BTreeMap<AnimProp, AnimValue>,
}

fn parse_config(id: u64, style: &StyleDesc) -> TransitionList {
    TransitionList::parse(style).unwrap_or_else(|error| {
        log::warn!("Invalid transition on element {id}: {error}");
        TransitionList::default()
    })
}

impl ElementState {
    fn new(id: u64, after: &StyleDesc) -> Self {
        Self {
            previous: after.clone(),
            config: config_key(after),
            list: parse_config(id, after),
            running: BTreeMap::new(),
            completed: BTreeMap::new(),
        }
    }

    fn step(
        &mut self,
        id: u64,
        after: &StyleDesc,
        motion: Option<&MotionState>,
        now: Instant,
        events: &mut Vec<TransitionEvent>,
    ) {
        let config = config_key(after);
        let config_changed = config != self.config;
        if config_changed {
            self.list = parse_config(id, after);
            self.config = config;
        }
        self.advance(id, now, events);

        let style_changed = self.previous != *after;
        if style_changed || config_changed {
            let props: Vec<AnimProp> = if style_changed {
                AnimProp::ALL.to_vec()
            } else {
                self.running
                    .keys()
                    .chain(self.completed.keys())
                    .copied()
                    .collect()
            };
            for prop in props {
                self.restyle(id, prop, after, motion, now, events);
            }
            if style_changed {
                self.previous = after.clone();
            }
        }
        self.advance(id, now, events);
    }

    /// Reports transitions that began and moves finished ones to `completed`.
    fn advance(&mut self, id: u64, now: Instant, events: &mut Vec<TransitionEvent>) {
        let mut finished = Vec::new();
        for (prop, running) in &mut self.running {
            let elapsed = running.elapsed(now);
            if !running.began && elapsed >= running.delay {
                running.began = true;
                events.push(TransitionEvent {
                    id,
                    kind: TransitionEventKind::Start,
                    prop: *prop,
                    elapsed: running.initial_elapsed(),
                });
            }
            if elapsed >= running.delay + running.duration {
                finished.push(*prop);
            }
        }
        for prop in finished {
            if let Some(running) = self.running.remove(&prop) {
                events.push(TransitionEvent {
                    id,
                    kind: TransitionEventKind::End,
                    prop,
                    elapsed: running.duration,
                });
                self.completed.insert(prop, running.end);
            }
        }
    }

    fn cancel(
        &mut self,
        id: u64,
        prop: AnimProp,
        now: Instant,
        events: &mut Vec<TransitionEvent>,
    ) {
        if let Some(running) = self.running.remove(&prop) {
            let active = running.elapsed(now) - running.delay;
            events.push(TransitionEvent {
                id,
                kind: TransitionEventKind::Cancel,
                prop,
                elapsed: active.clamp(0.0, running.duration),
            });
        }
    }

    fn begin(
        &mut self,
        id: u64,
        prop: AnimProp,
        running: Running,
        events: &mut Vec<TransitionEvent>,
    ) {
        events.push(TransitionEvent {
            id,
            kind: TransitionEventKind::Run,
            prop,
            elapsed: running.initial_elapsed(),
        });
        self.running.insert(prop, running);
    }

    /// The start rules of CSS Transitions 1 §3 for one property, run when its
    /// style or the element's `transition*` changed.
    fn restyle(
        &mut self,
        id: u64,
        prop: AnimProp,
        after: &StyleDesc,
        motion: Option<&MotionState>,
        now: Instant,
        events: &mut Vec<TransitionEvent>,
    ) {
        let entry = if motion.is_some_and(|motion| motion.owns(prop)) {
            None
        } else {
            self.list.matching(prop).cloned()
        };
        let Some(entry) = entry else {
            self.cancel(id, prop, now, events);
            self.completed.remove(&prop);
            return;
        };

        let after_value = super::resolve_start(super::PropKey::Style(prop), super::Base::Style(Some(after)));
        if let Some(end) = self.completed.get(&prop) {
            if after_value.as_ref() != Some(end) {
                self.completed.remove(&prop);
            }
        }
        let has_duration = entry.duration + entry.delay > 0.0;

        let Some(old) = self.running.get(&prop) else {
            if self.completed.contains_key(&prop) || !has_duration {
                return;
            }
            let before = super::resolve_start(super::PropKey::Style(prop), super::Base::Style(Some(&self.previous)));
            if let (Some(before), Some(after_value)) = (before, after_value) {
                if before != after_value {
                    if let Some(running) =
                        Running::new(prop, &before, &after_value, &entry, before.clone(), 1.0, now)
                    {
                        self.begin(id, prop, running, events);
                    }
                }
            }
            return;
        };
        if after_value.as_ref() == Some(&old.end) {
            return;
        }

        let current = old.value(now);
        let reversal = (after_value.as_ref() == Some(&old.reversing_start)).then(|| {
            let shortening = (old.progress(old.elapsed(now)) * old.shortening
                + (1.0 - old.shortening))
                .abs()
                .clamp(0.0, 1.0);
            (old.end.clone(), shortening)
        });
        self.cancel(id, prop, now, events);
        let Some(after_value) = after_value else {
            return;
        };
        if current == after_value || !has_duration {
            return;
        }
        let (reversing_start, shortening) = reversal.unwrap_or_else(|| (current.clone(), 1.0));
        if let Some(running) = Running::new(
            prop,
            &current,
            &after_value,
            &entry,
            reversing_start,
            shortening,
            now,
        ) {
            self.begin(id, prop, running, events);
        }
    }

    fn values(&self, now: Instant) -> Vec<(AnimProp, AnimValue)> {
        self.running
            .iter()
            .map(|(prop, running)| (*prop, prop.clamp_value(running.value(now))))
            .collect()
    }
}

/// The values of the transitions running on an element, ready to lay over its
/// after-change style.
pub(crate) struct TransitionStyle(Vec<(AnimProp, AnimValue)>);

impl TransitionStyle {
    pub(crate) fn apply_to(self, style: &mut StyleDesc) {
        for (prop, value) in self.0 {
            prop.write(style, value);
        }
    }
}

/// The transition state of every element that declares one.
#[derive(Default)]
pub(crate) struct CssTransitions {
    states: HashMap<u64, ElementState>,
    events: Vec<TransitionEvent>,
    active: bool,
}

impl CssTransitions {
    /// Whether `id` takes part in transitions: it declares one, in its own
    /// style or a state style, or already has transition state to keep.
    pub(crate) fn wants(&self, id: u64, style: Option<&StyleDesc>) -> bool {
        self.states.contains_key(&id)
            || style.is_some_and(|style| {
                declares_transition(style)
                    || [&style.hover, &style.active, &style.focus_visible]
                        .into_iter()
                        .any(|state| state.as_deref().is_some_and(declares_transition))
            })
    }

    /// Feeds one frame's after-change style. The first style an element is seen
    /// with never transitions: there is nothing before it.
    pub(crate) fn update(
        &mut self,
        id: u64,
        after: &StyleDesc,
        motion: Option<&MotionState>,
        now: Instant,
    ) -> Option<TransitionStyle> {
        let Some(state) = self.states.get_mut(&id) else {
            self.states.insert(id, ElementState::new(id, after));
            return None;
        };
        state.step(id, after, motion, now, &mut self.events);
        self.active |= !state.running.is_empty();
        let values = state.values(now);
        (!values.is_empty()).then_some(TransitionStyle(values))
    }

    pub(crate) fn retain(&mut self, mut keep: impl FnMut(u64) -> bool) {
        self.states.retain(|id, _| keep(*id));
    }

    /// Events queued since the last call, in the order they happened.
    pub(crate) fn take_events(&mut self) -> Vec<TransitionEvent> {
        std::mem::take(&mut self.events)
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
    use crate::motion::value::Color;
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
                matcher: Matcher::Props(vec![AnimProp::BackgroundColor]),
                duration: 0.15,
                delay: 0.0,
                timing: Ease::CubicBezier([0.0, 0.0, 0.58, 1.0]),
            }
        );
        assert_eq!(entry(&transitions, 1).1, 0.2);
        assert_eq!(entry(&transitions, 1).2, 0.05);
        assert_eq!(entry(&transitions, 1).0, &Matcher::Props(vec![AnimProp::Opacity]));
        assert_eq!(entry(&transitions, 2), (&Matcher::All, 1.0, -0.1));

        let reordered = list(r#"{ "transition": "ease-in 1s 2s width" }"#);
        assert_eq!(entry(&reordered, 0).1, 1.0);
        assert_eq!(entry(&reordered, 0).2, 2.0);
        assert_eq!(reordered.0[0].timing, Ease::CubicBezier([0.42, 0.0, 1.0, 1.0]));
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
        assert_eq!(entry(&transitions, 0), (&Matcher::Props(vec![AnimProp::Width]), 3.0, 0.1));
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
            assert!(padding.matching(prop).is_some());
        }
        assert!(padding.matching(AnimProp::MarginTop).is_none());
        let border = list(r#"{ "transition": "border 1s, border-radius 1s, inset 1s, flex 1s" }"#);
        for prop in [
            AnimProp::BorderLeftWidth,
            AnimProp::BorderColor,
            AnimProp::BorderTopLeftRadius,
            AnimProp::Left,
            AnimProp::FlexBasis,
        ] {
            assert!(border.matching(prop).is_some(), "{prop:?}");
        }
    }

    #[test]
    fn the_last_matching_entry_wins() {
        let transitions = list(r#"{ "transition": "all 1s, opacity 2s" }"#);
        assert_eq!(transitions.matching(AnimProp::Opacity).unwrap().duration, 2.0);
        assert_eq!(transitions.matching(AnimProp::Width).unwrap().duration, 1.0);
        let reversed = list(r#"{ "transition": "opacity 2s, all 1s" }"#);
        assert_eq!(reversed.matching(AnimProp::Opacity).unwrap().duration, 1.0);
    }

    #[test]
    fn a_valid_property_gpuix_cannot_animate_matches_nothing() {
        let transitions = list(r#"{ "transition": "transform 1s, display 1s, opacity 1s" }"#);
        assert_eq!(transitions.0.len(), 3);
        assert!(transitions.matching(AnimProp::Opacity).is_some());
        assert!(transitions.matching(AnimProp::Width).is_none());
    }

    #[test]
    fn validation_covers_the_state_styles() {
        assert!(validate_transitions(&style(r#"{ "transition": "width 1s" }"#)).is_ok());
        let error = validate_transitions(&style(r#"{ "hover": { "transitionDelay": "soon" } }"#))
            .unwrap_err();
        assert!(error.starts_with("hover.transitionDelay"), "{error}");
        assert!(validate_transitions(&style(r#"{ "transition": "width bogus" }"#)).is_err());
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

    // ── State machine ────────────────────────────────────────────────

    struct Clock {
        origin: Instant,
        transitions: CssTransitions,
    }

    impl Clock {
        fn new() -> Self {
            Self {
                origin: Instant::now(),
                transitions: CssTransitions::default(),
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
            let mut resolved = after.clone();
            if let Some(values) = self.transitions.update(1, after, motion, self.at(millis)) {
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

    #[test]
    fn motion_owned_properties_are_not_transitioned() {
        let now = Instant::now();
        let motion = MotionState::new(
            &serde_json::json!({ "animate": { "width": 100.0 }, "transition": { "duration": 1.0 } }),
            None,
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

    #[test]
    fn a_colour_blends_through_oklab_and_ends_exact() {
        let mut clock = Clock::new();
        let red = style(r##"{ "backgroundColor": "#ff0000", "transition": "background-color 1s linear" }"##);
        let blue = style(r##"{ "backgroundColor": "#0000ff", "transition": "background-color 1s linear" }"##);
        clock.frame(0, &red);
        clock.frame(0, &blue);
        let middle = clock.frame(500, &blue).background_color.unwrap();
        assert_ne!(middle, "#ff0000");
        assert_ne!(middle, "#0000ff");
        let blend = pair(
            "backgroundColor",
            &AnimValue::Color(Color::parse("#ff0000").unwrap()),
            &AnimValue::Color(Color::parse("#0000ff").unwrap()),
        )
        .unwrap();
        let AnimValue::Color(expected) = blend.at(0.5) else {
            panic!("a colour pair decodes to a colour");
        };
        assert_eq!(middle, expected.to_css());
        assert_eq!(clock.frame(1000, &blue).background_color.as_deref(), Some("#0000ff"));
    }

    #[test]
    fn mixed_units_and_auto_change_without_transitioning() {
        let mut clock = Clock::new();
        clock.frame(0, &style(r#"{ "width": 10, "transition": "all 1s" }"#));
        let percent = style(r#"{ "width": "50%", "transition": "all 1s" }"#);
        assert_eq!(clock.frame(10, &percent).width, Some(DimensionValue::Percentage(0.5)));
        let auto = style(r#"{ "width": "auto", "transition": "all 1s" }"#);
        assert_eq!(clock.frame(20, &auto).width, Some(DimensionValue::Auto));
        assert!(clock.events().is_empty());
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
        assert_eq!(middle.padding_left, Some(5.0));
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
        let clock = CssTransitions::default();
        assert!(!clock.wants(1, None));
        assert!(!clock.wants(1, Some(&style(r#"{ "width": 1 }"#))));
        assert!(!clock.wants(1, Some(&style(r#"{ "transition": "none" }"#))));
        assert!(clock.wants(1, Some(&style(r#"{ "transition": "width 1s" }"#))));
        assert!(clock.wants(1, Some(&style(r#"{ "hover": { "transitionDuration": "1s" } }"#))));
        assert!(clock.wants(1, Some(&style(r#"{ "focusVisible": { "transition": "all 1s" } }"#))));
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
        assert!(clock.transitions.wants(1, None));
        clock.transitions.retain(|_| false);
        assert!(!clock.transitions.wants(1, None));
    }
}
