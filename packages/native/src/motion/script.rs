//! The animations of a `motion` element that Motion would run with the Web Animations API of the
//! browser, listed by `getAnimations()` as the plain `Animation`s a script makes with
//! `Element.animate()`.
//!
//! Motion 14.1.0 hands a value to the browser when it is `opacity`, `clipPath`, `filter`,
//! `transform` or `backgroundColor`, or a colour (`color`, `outlineColor`, `borderColor`, `fill`,
//! `stroke`) with a keyframe in a syntax only the browser reads (`oklch()`, `color-mix()`, ...),
//! and the transition has no `repeatDelay`, no `repeatType: "mirror"` and no `damping: 0`, and the
//! value changes. Everything else runs in JavaScript and shows nowhere. The track keeps the clock it
//! always had for the value it shows; what is added here is the clock of the Web Animations
//! `Animation` Motion would make for it, and the effect that Animation has. A track that does not
//! qualify has none.
//!
//! Motion makes the effect from the transition: the delay, the length of one iteration, the repeats
//! as `iterations` (`direction: "alternate"` for `repeatType: "reverse"`), `fill: "both"`, the
//! easing of the whole effect, or per keyframe where `ease` is a list, and a spring as the
//! `linear()` easing of a generator sampled every 10 ms over the time it takes to come to rest
//! (`motion-dom`, `animation/generators/spring.mjs`, `waapi/utils/linear.mjs`).
//!
//! The gpuix `motion` components have no `onUpdate` or `transformTemplate` prop, so the two
//! conditions Motion for React puts on those do not arise. The value gpuix animates with is the
//! track's own: what an effect reads here is the shape the browser would have been given.

use std::cell::Cell;

use web_time::Instant;

use crate::dormant_wake::Wake;

use super::css_animation::{Direction, FillMode, IterationCount};
use super::curve::{parse_timing_function, Curve, Ease, RepeatCount, RepeatType, Spring, Transition};
use super::effect::{EffectInfo, KeyframeInfo, Timing};
use super::property::AnimProp;
use super::svg_attr::SvgAttr;
use super::text::{shortest, Text};
use super::track::Track;
use super::value::AnimValue;
use super::web_animation::{ApiEvent, Info, Kind, Note, Playback, Status};
use super::PropKey;

/// How far `calcGeneratorDuration` looks for the end of a generator, in ms.
const MAX_GENERATOR_DURATION: f64 = 20_000.0;
/// The step of `calcGeneratorDuration`, in ms.
const GENERATOR_STEP: f64 = 50.0;
/// The spacing of the points `generateLinearEasing` samples, in ms.
const LINEAR_RESOLUTION: f64 = 10.0;
/// The scale Motion runs a spring over to make an easing of it: from 0 to 100.
const SPRING_SCALE: f64 = 100.0;

#[derive(Clone, Debug)]
pub(super) struct Script {
    clock: Playback,
    /// The property as the DOM names it.
    name: &'static str,
    timing: Timing,
    /// The easing of the effect as a function; `linear` where the keyframes carry the easings.
    easing: Ease,
    /// The same as Chrome reads it back from `getTiming()`.
    easing_text: String,
    /// The easing of each keyframe as `getKeyframes()` reads it, `linear` where `ease` is no list.
    frame_easings: Vec<String>,
    /// Where `times` put the keyframes; `None` leaves them to the browser.
    offsets: Option<Vec<f64>>,
    /// A frame that was drawn has started the clock. Motion gives its animation a start time at once,
    /// so what a layout pass a read forced makes is not an animation yet.
    started: bool,
    /// When the track began, whose clock the track follows until a frame starts this one.
    created: Instant,
    /// Where the track picks up when the animation is cancelled: the instant its own clock would have started at.
    resume: Option<Instant>,
    /// An `Animation` object was handed to JS for this: only then is there anything to tell of its
    /// finish and cancel.
    observed: Cell<bool>,
}

/// Whether Motion gives the browser the value `key` animates through `literal`.
fn accelerated(key: PropKey, literal: &[AnimValue]) -> bool {
    match key {
        PropKey::Style(AnimProp::Opacity | AnimProp::BackgroundColor) | PropKey::Svg(SvgAttr::Opacity) => true,
        PropKey::Style(AnimProp::Color | AnimProp::OutlineColor) | PropKey::Svg(SvgAttr::Fill | SvgAttr::Stroke) => {
            literal.iter().any(AnimValue::is_browser_colour)
        }
        _ => false,
    }
}

/// `Math.round` of JavaScript, which rounds a half up.
fn js_round(value: f64) -> f64 {
    let floor = value.floor();
    if value - floor >= 0.5 {
        floor + 1.0
    } else {
        floor
    }
}

/// Motion's spring generator over the scale an easing is made on, `next(t)` for `t` in ms.
struct Generator {
    /// `Some(ms)` for a spring given by `duration` and `bounce`, which ends when that time has passed.
    time_defined: Option<f64>,
    rest_speed: f64,
    rest_delta: f64,
    kind: SpringKind,
}

enum SpringKind {
    Under { decay: f64, angular_freq: f64, a: f64 },
    Critical { freq: f64, velocity: f64 },
    Over { slow: f64, fast: f64, s: f64, f: f64, limit: f64, decay: f64 },
}

impl Generator {
    /// `velocity` is the rate of change per second of the value the spring takes over from, which Motion hands to the
    /// spring as it is, on its scale of 100, unless the spring is defined by its time.
    fn of(spring: &Spring, velocity: f64) -> Self {
        let delta = SPRING_SCALE;
        let damping_ratio = spring.damping / (2.0 * (spring.stiffness * spring.mass).sqrt());
        let undamped_freq = (spring.stiffness / spring.mass).sqrt() / 1000.0;
        let decay = damping_ratio * undamped_freq;
        let time_defined = spring.time_defined.map(|seconds| seconds.clamp(0.01, 10.0) * 1000.0);
        let velocity = if time_defined.is_some() { 0.0 } else { -velocity / 1000.0 };
        let kind = if damping_ratio < 1.0 {
            let angular_freq = undamped_freq * (1.0 - damping_ratio * damping_ratio).sqrt();
            SpringKind::Under { decay, angular_freq, a: (velocity + decay * delta) / angular_freq }
        } else if damping_ratio == 1.0 {
            SpringKind::Critical { freq: undamped_freq, velocity }
        } else {
            let damped = undamped_freq * (damping_ratio * damping_ratio - 1.0).sqrt();
            let p = (velocity + decay * delta) / damped;
            SpringKind::Over {
                slow: decay - damped,
                fast: decay + damped,
                s: (delta + p) / 2.0,
                f: (delta - p) / 2.0,
                limit: if time_defined.is_some() { f64::INFINITY } else { 300.0 / damped },
                decay,
            }
        };
        // The move is 100 units, never a granular one.
        Self {
            time_defined,
            rest_speed: spring.rest_speed().unwrap_or(2.0),
            rest_delta: spring.rest_delta().unwrap_or(0.5),
            kind,
        }
    }

    fn position(&self, t: f64) -> f64 {
        let (target, delta) = (SPRING_SCALE, SPRING_SCALE);
        match &self.kind {
            SpringKind::Under { decay, angular_freq, a } => {
                let env = (-decay * t).exp();
                target - env * (a * (angular_freq * t).sin() + delta * (angular_freq * t).cos())
            }
            SpringKind::Critical { freq, velocity } => {
                target - (-freq * t).exp() * (delta + (velocity + freq * delta) * t)
            }
            SpringKind::Over { slow, fast, s, f, limit, decay } => {
                let decay_at = |rate: f64| {
                    (if t > *limit { -rate * limit - decay * (t - limit) } else { -rate * t }).exp()
                };
                target - s * decay_at(*slow) - f * decay_at(*fast)
            }
        }
    }

    /// Per ms.
    fn velocity(&self, t: f64) -> f64 {
        let delta = SPRING_SCALE;
        match &self.kind {
            SpringKind::Under { decay, angular_freq, a } => {
                let env = (-decay * t).exp();
                let sin_c = decay * a + delta * angular_freq;
                let cos_c = decay * delta - a * angular_freq;
                env * (sin_c * (angular_freq * t).sin() + cos_c * (angular_freq * t).cos())
            }
            SpringKind::Critical { freq, velocity } => {
                let c = velocity + freq * delta;
                (-freq * t).exp() * (freq * c * t - velocity)
            }
            SpringKind::Over { slow, fast, s, f, limit, decay } => {
                let decay_at = |rate: f64| {
                    (if t > *limit { -rate * limit - decay * (t - limit) } else { -rate * t }).exp()
                };
                slow * s * decay_at(*slow) + fast * f * decay_at(*fast)
            }
        }
    }

    /// The value at `t` ms, and whether the spring is done.
    fn next(&self, t: f64) -> (f64, bool) {
        let current = self.position(t);
        let done = match self.time_defined {
            Some(duration) => t >= duration,
            None => {
                (self.velocity(t) * 1000.0).abs() <= self.rest_speed && (SPRING_SCALE - current).abs() <= self.rest_delta
            }
        };
        (if done { SPRING_SCALE } else { current }, done)
    }

    /// `calcGeneratorDuration`, in ms, capped at the longest Motion looks for.
    fn duration(&self) -> f64 {
        let mut duration = 0.0;
        let mut done = self.next(duration).1;
        while !done && duration < MAX_GENERATOR_DURATION {
            duration += GENERATOR_STEP;
            done = self.next(duration).1;
        }
        duration.min(MAX_GENERATOR_DURATION)
    }
}

/// The easing Motion gives the browser for a spring: its length in ms, the `linear()` it writes
/// (`generateLinearEasing` over the spring as a generator) and the same as Chrome reads it back.
fn spring_easing(spring: &Spring, velocity: f64) -> (f64, String, String) {
    let generator = Generator::of(spring, velocity);
    let duration = generator.duration();
    let count = js_round(duration / LINEAR_RESOLUTION).max(2.0) as usize;
    let outputs: Vec<f64> = (0..count)
        .map(|index| {
            let progress = index as f64 / (count - 1) as f64;
            js_round(generator.next(duration * progress).0 / SPRING_SCALE * 10_000.0) / 10_000.0
        })
        .collect();
    let written = format!("linear({})", outputs.iter().map(|output| shortest(*output)).collect::<Vec<_>>().join(", "));
    // Chrome spreads the stops that have no input between the ends in `double`s it writes out whole.
    // Its sums carry rounding noise of their own that is not reproduced here.
    let read_back = format!(
        "linear({})",
        outputs
            .iter()
            .enumerate()
            .map(|(index, output)| {
                let percent = if index == 0 {
                    0.0
                } else if index == count - 1 {
                    100.0
                } else {
                    100.0 * index as f64 / (count - 1) as f64
                };
                format!("{} {}%", shortest(*output), shortest(percent))
            })
            .collect::<Vec<_>>()
            .join(", ")
    );
    (duration, written, read_back)
}

/// What the effect of a track is made of, from its transition.
struct Shape {
    /// Seconds of one iteration.
    duration: f64,
    easing: Ease,
    easing_text: String,
    frame_easings: Vec<String>,
}

impl Shape {
    fn of(transition: &Transition, velocity: f64) -> Option<Self> {
        match &transition.curve {
            Curve::Tween { duration, ease } => {
                // Motion lists a duration of 0 for one frame, and not reliably.
                if *duration == 0.0 {
                    return None;
                }
                let linear = || Ease::Name("linear".to_string());
                match &transition.segment_eases {
                    Some(eases) => {
                        let listed = eases.iter().map(Ease::to_css).collect::<Result<Vec<_>, _>>().ok()?;
                        // A list shorter than the keyframes starts over, and Motion's has one fewer: the last
                        // keyframe takes the first easing.
                        let frame_easings = (0..=listed.len()).map(|index| listed[index % listed.len()].clone()).collect();
                        Some(Self { duration: *duration, easing: linear(), easing_text: "linear".to_string(), frame_easings })
                    }
                    None => Some(Self {
                        duration: *duration,
                        easing: ease.clone(),
                        easing_text: ease.to_css().ok()?,
                        frame_easings: Vec::new(),
                    }),
                }
            }
            Curve::Spring(spring) => {
                let (milliseconds, written, read_back) = spring_easing(spring, velocity);
                Some(Self {
                    duration: milliseconds / 1000.0,
                    easing: parse_timing_function(&written).ok()?,
                    easing_text: read_back,
                    frame_easings: Vec::new(),
                })
            }
        }
    }
}

impl Script {
    /// The animation Motion would make for the track of `key`, started at `now` and waiting for the frame
    /// that draws it. `None` when Motion keeps the value to itself.
    pub(super) fn of(key: PropKey, track: &Track, now: Instant) -> Option<Self> {
        let transition = track.transition();
        if transition.repeat.delay != 0.0 || transition.repeat.kind == RepeatType::Mirror || !track.moves() {
            return None;
        }
        if !accelerated(key, track.literal()) {
            return None;
        }
        let shape = Shape::of(transition, if key == PropKey::Style(AnimProp::Opacity) { track.start_velocity() } else { 0.0 })?;
        let timing = Timing {
            delay: transition.delay,
            duration: shape.duration,
            iterations: match transition.repeat.count {
                RepeatCount::Finite(repeats) => IterationCount::Finite(f64::from(repeats) + 1.0),
                RepeatCount::Infinite => IterationCount::Infinite,
            },
            direction: if transition.repeat.kind == RepeatType::Reverse { Direction::Alternate } else { Direction::Normal },
            fill: FillMode::Both,
            end_delay: 0.0,
            iteration_start: 0.0,
        };
        Some(Self {
            clock: Playback::start(now, false, timing.end_time()),
            name: key.name(),
            timing,
            easing: shape.easing,
            easing_text: shape.easing_text,
            frame_easings: shape.frame_easings,
            offsets: transition.times.clone(),
            started: false,
            created: now,
            resume: None,
            observed: Cell::new(false),
        })
    }

    pub(super) fn id(&self) -> u64 {
        self.clock.id()
    }

    fn end(&self) -> f64 {
        self.timing.end_time()
    }

    /// How far into the effect the track shows the value of: the time of the clock, or the wall time since the track began
    /// while no frame has started the clock.
    pub(super) fn elapsed(&self, now: Instant) -> std::time::Duration {
        if !self.started {
            return now.saturating_duration_since(self.created);
        }
        let time = self.clock.time(now).unwrap_or(0.0).max(0.0);
        std::time::Duration::try_from_secs_f64(time).unwrap_or(std::time::Duration::MAX)
    }

    /// Whether the animation has not ended yet: the track is still being driven by it.
    pub(super) fn running(&self, now: Instant) -> bool {
        !self.over(now)
    }

    fn over(&self, now: Instant) -> bool {
        if !self.started {
            // No frame has started the clock: the track's own has run out.
            return now.saturating_duration_since(self.created).as_secs_f64() >= self.end();
        }
        self.clock.is_idle() || self.clock.status(now, self.end()) == Status::Finished
    }

    /// Whether `getAnimations()` lists it: Motion's animation is there from the first frame after
    /// the value starts to move until it finishes or is cancelled.
    pub(super) fn listed(&self, now: Instant) -> bool {
        self.started && !self.over(now)
    }

    pub(super) fn info(&self, now: Instant) -> Info {
        self.observed.set(true);
        Info::of(Kind::Script, self.name.to_string(), &self.clock, now, self.end())
    }

    pub(super) fn effect_info(&self, now: Instant) -> Result<EffectInfo, String> {
        self.observed.set(true);
        let mut info = EffectInfo::of(
            &self.timing,
            false,
            Some(&self.easing),
            self.clock.current_time(now, self.end()),
            self.clock.rate() < 0.0,
        )?;
        info.easing = self.easing_text.clone();
        Ok(info)
    }

    /// `getKeyframes()`: the keyframes of the track as the browser was given them.
    pub(super) fn keyframe_info(&self, key: PropKey, literal: &[AnimValue]) -> Result<Vec<KeyframeInfo>, String> {
        self.observed.set(true);
        let count = literal.len();
        literal
            .iter()
            .enumerate()
            .map(|(index, value)| {
                Ok(KeyframeInfo {
                    offset: match &self.offsets {
                        Some(offsets) => offsets[index],
                        None => index as f64 / (count - 1) as f64,
                    },
                    written: self.offsets.is_some(),
                    easing: self.frame_easings.get(index).cloned().unwrap_or_else(|| "linear".to_string()),
                    composite: "auto",
                    values: key.computed_text(value, Text::Keyframe)?,
                })
            })
            .collect()
    }

    /// When the clock next has something to tell: the end of the effect.
    pub(super) fn wake(&self, now: Instant) -> Wake {
        if !self.started {
            return Wake::NextFrame;
        }
        self.clock.wake_at(self.end(), now)
    }

    /// What a frame does for the clock: a drawn one starts it, and what it noted is told to JS if
    /// JS holds the animation. The animation that is over is cancelled, as Motion cancels its
    /// animation when it finishes, and is not this track's any more.
    pub(super) fn advance(&mut self, element: u64, now: Instant, drawing: bool, events: &mut Vec<ApiEvent>) -> Advance {
        let end = self.end();
        if drawing && !self.started {
            self.clock.resolve_pending(now, end);
            self.started = true;
        }
        if self.started {
            self.clock.settle(now, end);
        }
        self.tell(element, now, events);
        if !self.over(now) {
            return Advance::Running;
        }
        // Motion's `onfinish` cancels the animation and commits the final style.
        self.cancel(element, now, events);
        Advance::Over
    }

    /// The animation is cancelled, and JS hears of it.
    pub(super) fn cancel(&mut self, element: u64, now: Instant, events: &mut Vec<ApiEvent>) {
        self.resume = Some(self.started_at(now));
        let end = self.end();
        self.clock.apply(super::web_animation::Command::Cancel, now, end).expect("cancelling cannot fail");
        self.tell(element, now, events);
    }

    fn tell(&mut self, element: u64, now: Instant, events: &mut Vec<ApiEvent>) {
        let notes = self.clock.take_notes();
        if !self.observed.get() {
            return;
        }
        for note in notes {
            // The animation was made running, so it has nothing it was waiting to start.
            if note == Note::Ready {
                continue;
            }
            events.push(ApiEvent::of(element, note, self.info_at(now)));
        }
    }

    fn info_at(&self, now: Instant) -> Info {
        Info::of(Kind::Script, self.name.to_string(), &self.clock, now, self.end())
    }

    /// The instant the track's own clock would have been started at to be where this one is.
    pub(super) fn started_at(&self, now: Instant) -> Instant {
        if let Some(resume) = self.resume {
            return resume;
        }
        if !self.started {
            return self.created;
        }
        let time = self.clock.run_time(now).unwrap_or(0.0).max(0.0);
        std::time::Duration::try_from_secs_f64(time)
            .ok()
            .and_then(|elapsed| now.checked_sub(elapsed))
            .unwrap_or(now)
    }
}

/// Whether the animation goes on after a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Advance {
    Running,
    Over,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::{json, Value};

    use super::super::{Colors, MotionState};
    use super::super::web_animation::ApiEventKind;
    use super::*;

    fn at(origin: Instant, millis: u64) -> Instant {
        origin + Duration::from_millis(millis)
    }

    fn state(description: Value, now: Instant) -> MotionState {
        MotionState::new(&description, None, Colors::root(), None, 0, now).unwrap()
    }

    fn tween(animate: Value, initial: Value, transition: Value, now: Instant) -> MotionState {
        state(json!({ "initial": initial, "animate": animate, "transition": transition }), now)
    }

    /// The animations Motion has in the browser for `state`, after the frame at `now` was drawn.
    fn listed(state: &mut MotionState, now: Instant) -> Vec<Info> {
        state.advance_scripts(1, now, true, &mut Vec::new());
        state.script_list(now)
    }

    #[test]
    fn only_the_values_motion_hands_to_the_browser_are_listed() {
        let now = Instant::now();
        let linear = json!({ "duration": 1.0, "ease": "linear" });
        for (animate, initial, expected) in [
            (json!({ "opacity": 0.5 }), json!({ "opacity": 1.0 }), vec!["opacity"]),
            (json!({ "backgroundColor": "#0000ff" }), json!({ "backgroundColor": "#ff0000" }), vec!["backgroundColor"]),
            (json!({ "color": "#0000ff" }), json!({ "color": "#ff0000" }), vec![]),
            (json!({ "color": "oklch(0.5 0.2 250)" }), json!({ "color": "oklch(0.7 0.2 30)" }), vec!["color"]),
            (json!({ "color": "oklch(0.5 0.2 250)" }), json!({ "color": "#ff0000" }), vec!["color"]),
            (json!({ "width": 200.0 }), json!({ "width": 100.0 }), vec![]),
            (json!({ "x": 100.0 }), json!({ "x": 0.0 }), vec![]),
            (json!({ "opacity": 1.0 }), json!({ "opacity": 1.0 }), vec![]),
        ] {
            let mut state = tween(animate.clone(), initial, linear.clone(), now);
            let names: Vec<String> = listed(&mut state, now).into_iter().map(|info| info.name).collect();
            assert_eq!(names, expected, "{animate}");
        }
    }

    #[test]
    fn a_repeat_delay_and_a_mirrored_repeat_keep_the_animation_to_motion() {
        let now = Instant::now();
        for (transition, expected) in [
            (json!({ "duration": 0.3, "repeat": 1, "repeatDelay": 0.2 }), 0),
            (json!({ "duration": 0.3, "repeat": 2, "repeatType": "mirror" }), 0),
            (json!({ "duration": 0.3, "repeat": 2, "repeatType": "reverse" }), 1),
            (json!({ "duration": 0.3, "repeat": 2 }), 1),
        ] {
            let mut state = tween(json!({ "opacity": 0.2 }), json!({ "opacity": 1.0 }), transition.clone(), now);
            assert_eq!(listed(&mut state, now).len(), expected, "{transition}");
        }
    }

    #[test]
    fn an_animation_is_listed_from_the_frame_that_is_drawn_until_it_ends() {
        let origin = Instant::now();
        let mut state = tween(json!({ "opacity": 0.5 }), json!({ "opacity": 1.0 }), json!({ "duration": 0.4, "ease": "linear" }), origin);
        let mut events = Vec::new();
        assert!(state.script_list(origin).is_empty(), "a layout a read forced makes nothing yet");
        state.advance_scripts(1, origin, false, &mut events);
        assert!(state.script_list(at(origin, 16)).is_empty());

        state.advance_scripts(1, at(origin, 16), true, &mut events);
        let [info] = &state.script_list(at(origin, 16))[..] else { panic!("one animation") };
        assert_eq!((info.kind, info.status, info.current_time, info.pending), (Kind::Script, Status::Running, 0.0, false));
        assert_eq!(info.start_offset, Some(0.0));
        let [info] = &state.script_list(at(origin, 216))[..] else { panic!("one animation") };
        assert_eq!(info.current_time, 200.0);
        assert_eq!(info.start_offset, Some(-200.0));

        state.advance_scripts(1, at(origin, 420), true, &mut events);
        assert!(state.script_list(at(origin, 420)).is_empty(), "Motion cancels its animation when it finishes");
        assert_eq!(
            events.iter().map(|event| event.kind).collect::<Vec<_>>(),
            [ApiEventKind::Finish, ApiEventKind::Cancel],
            "told because the animation was listed"
        );
    }

    #[test]
    fn an_animation_nobody_listed_finishes_without_a_word() {
        let origin = Instant::now();
        let mut state = tween(json!({ "opacity": 0.5 }), json!({ "opacity": 1.0 }), json!({ "duration": 0.1 }), origin);
        let mut events = Vec::new();
        state.advance_scripts(1, origin, true, &mut events);
        state.advance_scripts(1, at(origin, 200), true, &mut events);
        assert!(events.is_empty());
        assert!(state.script_list(at(origin, 200)).is_empty());
    }

    #[test]
    fn a_new_target_for_the_value_cancels_the_animation_at_once_and_replaces_it_a_frame_later() {
        let origin = Instant::now();
        let transition = json!({ "duration": 1.0, "ease": "linear" });
        let mut state = tween(json!({ "opacity": 0.0 }), json!({ "opacity": 1.0 }), transition.clone(), origin);
        let first = listed(&mut state, origin)[0].id;
        let mut events = Vec::new();
        let later = at(origin, 250);
        state
            .sync(&json!({ "animate": { "opacity": 1.0 }, "transition": transition }), None, Colors::root(), None, 0, later)
            .unwrap();
        assert!(state.script_list(later).is_empty());
        state.advance_scripts(1, later, false, &mut events);
        assert_eq!(events.iter().map(|event| (event.run, event.kind)).collect::<Vec<_>>(), [(first, ApiEventKind::Cancel)]);
        let second = listed(&mut state, at(origin, 266));
        assert_eq!(second.len(), 1);
        assert_ne!(second[0].id, first);
    }

    #[test]
    fn the_effect_is_what_motion_gives_the_browser() {
        let origin = Instant::now();
        let mut state = tween(
            json!({ "opacity": [1.0, 0.2, 0.8] }),
            json!({}),
            json!({ "duration": 0.3, "delay": 0.1, "repeat": 2, "repeatType": "reverse", "times": [0.0, 0.3, 1.0], "ease": "easeInOut" }),
            origin,
        );
        let id = listed(&mut state, origin)[0].id;
        let effect = state.script_effect(id, origin).unwrap().unwrap();
        assert_eq!(
            (effect.delay, effect.duration, effect.iterations, effect.direction, effect.fill, effect.easing.as_str(), effect.end_delay),
            (100.0, Some(300.0), 3.0, "alternate", "both", "ease-in-out", 0.0)
        );
        let keyframes = state.script_keyframes(id).unwrap().unwrap();
        assert_eq!(keyframes.iter().map(|frame| (frame.offset, frame.written)).collect::<Vec<_>>(), [(0.0, true), (0.3, true), (1.0, true)]);
        assert!(keyframes.iter().all(|frame| frame.easing == "linear"));
        assert_eq!(keyframes[1].values, [("opacity".to_string(), "0.2".to_string())]);
    }

    #[test]
    fn keyframes_with_no_times_have_no_offset_and_a_list_of_eases_goes_to_the_keyframes_and_starts_over() {
        let origin = Instant::now();
        let mut state = tween(
            json!({ "opacity": [1.0, 0.2, 0.8] }),
            json!({}),
            json!({ "duration": 0.4, "ease": ["easeIn", "easeOut"] }),
            origin,
        );
        let id = listed(&mut state, origin)[0].id;
        let effect = state.script_effect(id, origin).unwrap().unwrap();
        assert_eq!(effect.easing, "linear");
        let keyframes = state.script_keyframes(id).unwrap().unwrap();
        assert_eq!(keyframes.iter().map(|frame| (frame.offset, frame.written)).collect::<Vec<_>>(), [(0.0, false), (0.5, false), (1.0, false)]);
        assert_eq!(keyframes.iter().map(|frame| frame.easing.as_str()).collect::<Vec<_>>(), ["ease-in", "ease-out", "ease-in"]);
    }

    #[test]
    fn a_spring_is_the_linear_easing_of_its_generator_over_the_time_it_takes_to_rest() {
        let origin = Instant::now();
        let mut state = tween(json!({ "opacity": 0.5 }), json!({ "opacity": 1.0 }), json!({ "type": "spring" }), origin);
        let id = listed(&mut state, origin)[0].id;
        let effect = state.script_effect(id, origin).unwrap().unwrap();
        // The default spring of Motion 14.1.0 takes 1050 ms, which is 105 points of 10 ms.
        assert_eq!(effect.duration, Some(1050.0));
        assert!(effect.easing.starts_with("linear(0 0%, 0.0049 0.96153846153846"), "{}", effect.easing);
        assert_eq!(effect.easing.matches('%').count(), 105);
        assert!(effect.easing.ends_with("1 100%)"));

        let mut timed = tween(
            json!({ "opacity": 0.5 }),
            json!({ "opacity": 1.0 }),
            json!({ "type": "spring", "duration": 0.5, "bounce": 0.3 }),
            origin,
        );
        let id = listed(&mut timed, origin)[0].id;
        let effect = timed.script_effect(id, origin).unwrap().unwrap();
        assert_eq!(effect.duration, Some(500.0));
        assert_eq!(effect.easing.matches('%').count(), 50);
    }
}
