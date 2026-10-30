//! Tween easing and spring physics shared by every animated property.

use std::borrow::Cow;
use std::time::Duration;

use serde::Deserialize;

use super::text::shortest;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub(super) enum Ease {
    Name(String),
    CubicBezier([f64; 4]),
    /// CSS `steps()`. Only produced by [`parse_timing_function`].
    #[serde(skip)]
    Steps(Steps),
    /// CSS `linear()` stops as `(input, output)`, inputs non-decreasing. Only
    /// produced by [`parse_timing_function`].
    #[serde(skip)]
    Stops(Vec<(f64, f64)>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Steps {
    count: u32,
    jump: Jump,
    /// Written `jump-start`, which Chrome keeps apart from `start`.
    jump_start_term: bool,
}

/// Where the jumps of a `steps()` function fall.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Jump {
    Start,
    End,
    None,
    Both,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Transition {
    pub(super) delay: f64,
    pub(super) curve: Curve,
    /// Where each keyframe sits in the iteration, as fractions of its duration.
    /// `None` spaces them evenly. Tweens only.
    pub(super) times: Option<Vec<f64>>,
    /// One easing per keyframe segment. When set it replaces the tween's
    /// single `ease`. Tweens only.
    pub(super) segment_eases: Option<Vec<Ease>>,
    pub(super) repeat: Repeat,
    /// Only `delay` and the repeat fields were written, so the curve is Motion's default for the
    /// key (`default_transition.rs`) and the `curve` here stands for nothing until it is resolved.
    pub(super) defaulted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Repeat {
    pub(super) count: RepeatCount,
    pub(super) kind: RepeatType,
    /// Seconds held at the end of each iteration before the next one.
    pub(super) delay: f64,
}

impl Default for Repeat {
    fn default() -> Self {
        Self {
            count: RepeatCount::Finite(0),
            kind: RepeatType::Loop,
            delay: 0.0,
        }
    }
}

/// How many times an iteration runs again after the first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum RepeatCount {
    Finite(u32),
    Infinite,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum RepeatType {
    /// Every iteration plays from the first keyframe to the last.
    Loop,
    /// Odd iterations play backwards in time, so their easing runs backwards too.
    Reverse,
    /// Odd iterations play the keyframes in reverse order with the same
    /// timing and easing as the first.
    Mirror,
}

impl Repeat {
    pub(super) fn repeats(&self) -> bool {
        self.count != RepeatCount::Finite(0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Curve {
    Tween { duration: f64, ease: Ease },
    Spring(Spring),
}

impl Default for Transition {
    fn default() -> Self {
        Self {
            delay: 0.0,
            curve: Curve::Tween {
                duration: default_duration(),
                ease: default_ease(),
            },
            times: None,
            segment_eases: None,
            repeat: Repeat::default(),
            defaulted: false,
        }
    }
}

fn default_duration() -> f64 {
    0.3
}

fn default_ease() -> Ease {
    Ease::Name("easeOut".to_string())
}

/// A single easing, or one per keyframe segment.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum EaseSpec {
    One(Ease),
    Each(Vec<Ease>),
}

/// A repeat count: a number, or the string `"Infinity"` because JSON has no
/// infinity.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum RawRepeat {
    Count(f64),
    Name(String),
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RawTransition {
    #[serde(rename = "type")]
    pub(super) kind: Option<String>,
    pub(super) duration: Option<f64>,
    pub(super) delay: Option<f64>,
    pub(super) ease: Option<EaseSpec>,
    pub(super) times: Option<Vec<f64>>,
    pub(super) repeat: Option<RawRepeat>,
    pub(super) repeat_type: Option<String>,
    pub(super) repeat_delay: Option<f64>,
    pub(super) stiffness: Option<f64>,
    pub(super) damping: Option<f64>,
    pub(super) mass: Option<f64>,
    pub(super) bounce: Option<f64>,
    pub(super) rest_speed: Option<f64>,
    pub(super) rest_delta: Option<f64>,
}

/// Framer's names are camelCase; anything else is a CSS `<easing-function>`
/// (`ease-in`, `steps(4)`, `linear(..)`).
fn resolve_ease(ease: Ease) -> Result<Ease, String> {
    match ease {
        Ease::Name(name) if !FRAMER_EASES.contains(&name.as_str()) => parse_timing_function(&name)
            .map_err(|error| format!("unknown motion easing: {name} ({error})")),
        other => Ok(other),
    }
}

/// An easing from a description: a name, a CSS easing function or four
/// cubic-bezier numbers.
pub(super) fn parse_ease(value: serde_json::Value) -> Result<Ease, String> {
    let ease: Ease = serde_json::from_value(value)
        .map_err(|error| format!("motion easing must be a name or four numbers: {error}"))?;
    let ease = resolve_ease(ease)?;
    validate_ease(&ease)?;
    Ok(ease)
}

impl Transition {
    /// A tween that ends on its first frame, so the track still settles and
    /// reports completion.
    pub(super) fn instant() -> Self {
        Self {
            delay: 0.0,
            curve: Curve::Tween {
                duration: 0.0,
                ease: Ease::Name("linear".to_string()),
            },
            times: None,
            segment_eases: None,
            repeat: Repeat::default(),
            defaulted: false,
        }
    }

    /// A transition that describes nothing: Motion's default curve for the key, no delay, no repeat.
    pub(super) fn undescribed() -> Self {
        Self { defaulted: true, ..Self::default() }
    }

    pub(super) fn parse(value: serde_json::Value) -> Result<Self, String> {
        let raw: RawTransition =
            serde_json::from_value(value).map_err(|error| format!("motion transition: {error}"))?;
        Self::try_from(raw)
    }
}

impl TryFrom<RawTransition> for Transition {
    type Error = String;

    fn try_from(raw: RawTransition) -> Result<Self, String> {
        let delay = raw.delay.unwrap_or(0.0);
        validate_seconds(delay, "delay")?;
        let repeat = parse_repeat(&raw)?;
        let defaulted = raw.kind.is_none()
            && raw.duration.is_none()
            && raw.ease.is_none()
            && raw.times.is_none()
            && raw.stiffness.is_none()
            && raw.damping.is_none()
            && raw.mass.is_none()
            && raw.bounce.is_none()
            && raw.rest_speed.is_none()
            && raw.rest_delta.is_none();
        let mut times = None;
        let mut segment_eases = None;
        let curve = match raw.kind.as_deref() {
            None | Some("tween") => {
                for (name, value) in [
                    ("stiffness", raw.stiffness),
                    ("damping", raw.damping),
                    ("mass", raw.mass),
                    ("bounce", raw.bounce),
                    ("restSpeed", raw.rest_speed),
                    ("restDelta", raw.rest_delta),
                ] {
                    if value.is_some() {
                        return Err(format!("motion transition {name} requires type \"spring\""));
                    }
                }
                let duration = raw.duration.unwrap_or_else(default_duration);
                validate_seconds(duration, "duration")?;
                let ease = match raw.ease.clone() {
                    None => default_ease(),
                    Some(EaseSpec::One(ease)) => resolve_ease(ease)?,
                    Some(EaseSpec::Each(list)) => {
                        let list = list
                            .into_iter()
                            .map(resolve_ease)
                            .collect::<Result<Vec<_>, _>>()?;
                        let first = list
                            .first()
                            .cloned()
                            .ok_or_else(|| "motion ease list needs at least one easing".to_string())?;
                        segment_eases = Some(list);
                        first
                    }
                };
                for ease in segment_eases.iter().flatten() {
                    validate_ease(ease)?;
                }
                validate_ease(&ease)?;
                times = raw.times.clone().map(validate_times).transpose()?;
                Curve::Tween { duration, ease }
            }
            Some("spring") => {
                if raw.ease.is_some() {
                    return Err("motion spring transitions do not accept ease".to_string());
                }
                if raw.times.is_some() {
                    return Err("motion spring transitions do not accept times".to_string());
                }
                Curve::Spring(Spring::from_raw(&raw)?)
            }
            Some(other) => return Err(format!("unknown motion transition type: {other}")),
        };
        Ok(Self {
            delay,
            curve,
            times,
            segment_eases,
            repeat,
            defaulted,
        })
    }
}

fn parse_repeat(raw: &RawTransition) -> Result<Repeat, String> {
    let count = match &raw.repeat {
        None => RepeatCount::Finite(0),
        Some(RawRepeat::Name(name)) if name == "Infinity" => RepeatCount::Infinite,
        Some(RawRepeat::Name(name)) => {
            return Err(format!(
                "motion repeat must be a count or \"Infinity\", not \"{name}\""
            ))
        }
        Some(RawRepeat::Count(count)) => {
            if !count.is_finite() || *count < 0.0 || count.fract() != 0.0 || *count > f64::from(u32::MAX)
            {
                return Err("motion repeat must be a whole number of at least 0, or Infinity".to_string());
            }
            RepeatCount::Finite(*count as u32)
        }
    };
    let kind = match raw.repeat_type.as_deref() {
        None | Some("loop") => RepeatType::Loop,
        Some("reverse") => RepeatType::Reverse,
        Some("mirror") => RepeatType::Mirror,
        Some(other) => {
            return Err(format!(
                "unknown motion repeatType: {other} (expected loop, reverse or mirror)"
            ))
        }
    };
    let delay = raw.repeat_delay.unwrap_or(0.0);
    validate_seconds(delay, "repeatDelay")?;
    Ok(Repeat { count, kind, delay })
}

fn validate_times(times: Vec<f64>) -> Result<Vec<f64>, String> {
    if times.is_empty()
        || times.iter().any(|time| !(0.0..=1.0).contains(time))
        || times.windows(2).any(|pair| pair[1] < pair[0])
    {
        return Err("motion times must be numbers from 0 to 1 in ascending order".to_string());
    }
    Ok(times)
}

impl Transition {
    /// Checks the fields that depend on how many keyframes a track has.
    pub(super) fn check_keyframes(&self, name: &str, frames: usize) -> Result<(), String> {
        if matches!(self.curve, Curve::Spring(_)) && frames > 2 {
            return Err(format!(
                "motion {name} has {frames} keyframes, but a spring only animates between two: use a tween"
            ));
        }
        if let Some(times) = &self.times {
            if times.len() != frames {
                return Err(format!(
                    "motion {name} has {frames} keyframes but times lists {}",
                    times.len()
                ));
            }
        }
        if let Some(eases) = &self.segment_eases {
            if eases.len() != frames - 1 {
                return Err(format!(
                    "motion {name} has {frames} keyframes, so ease needs {} easings, not {}",
                    frames - 1,
                    eases.len()
                ));
            }
        }
        Ok(())
    }

    /// This transition for a property of `frames` keyframes when it is shared:
    /// a `times` or `ease` list that does not fit that many keyframes is left
    /// out, and a single `ease` takes the place of a dropped `ease` list.
    pub(super) fn for_shared_keyframes(&self, frames: usize) -> Cow<'_, Transition> {
        let times_fit = self.times.as_ref().is_none_or(|times| times.len() == frames);
        let eases_fit = self
            .segment_eases
            .as_ref()
            .is_none_or(|eases| eases.len() == frames - 1);
        if times_fit && eases_fit {
            return Cow::Borrowed(self);
        }
        let mut fitted = self.clone();
        if !times_fit {
            fitted.times = None;
        }
        if !eases_fit {
            fitted.segment_eases = None;
            if let Curve::Tween { ease, .. } = &mut fitted.curve {
                *ease = default_ease();
            }
        }
        Cow::Owned(fitted)
    }

    /// Where the `frames` keyframes sit in an iteration.
    pub(super) fn keyframe_times(&self, frames: usize) -> Vec<f64> {
        self.times.clone().unwrap_or_else(|| {
            (0..frames)
                .map(|index| index as f64 / (frames - 1) as f64)
                .collect()
        })
    }

    /// The easing of keyframe segment `index`. Only tweens have one.
    pub(super) fn segment_ease(&self, index: usize) -> &Ease {
        if let Some(eases) = &self.segment_eases {
            return &eases[index];
        }
        match &self.curve {
            Curve::Tween { ease, .. } => ease,
            Curve::Spring(_) => unreachable!("a spring has no easing"),
        }
    }
}

// Spring defaults and limits follow `springDefaults` in motion-dom's
// `animation/generators/spring.mjs` (motion 12/14, the engine behind Framer
// Motion). `duration` is in seconds here; motion-dom's internal unit is ms.
const DEFAULT_STIFFNESS: f64 = 100.0;
const DEFAULT_DAMPING: f64 = 10.0;
const DEFAULT_MASS: f64 = 1.0;
const DEFAULT_SPRING_DURATION: f64 = 0.8;
const DEFAULT_SPRING_BOUNCE: f64 = 0.3;
const MIN_SPRING_DURATION: f64 = 0.01;
const MAX_SPRING_DURATION: f64 = 10.0;
const MIN_DAMPING_RATIO: f64 = 0.05;
const GRANULAR_DISTANCE: f64 = 5.0;
const GRANULAR_REST_SPEED: f64 = 0.01;
const DEFAULT_REST_SPEED: f64 = 2.0;
const GRANULAR_REST_DELTA: f64 = 0.005;
const DEFAULT_REST_DELTA: f64 = 0.5;
/// What one unit of a percentage channel (a fraction of the basis) is worth next
/// to a px one: the thresholds are written in px, or in percentage points.
const PERCENT_REST_SCALE: f64 = 0.01;
/// Largest `sqrt(stiffness / mass)` and `damping / (2 * mass)` in rad/s. Past
/// this the closed-form terms can overflow to infinity and NaN out a frame.
const MAX_SPRING_RATE: f64 = 1e6;
/// A damping ratio this close to 1 is evaluated as critically damped; the
/// over- and under-damped forms divide by a term that vanishes at 1.
const CRITICAL_DAMPING_TOLERANCE: f64 = 1e-6;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Spring {
    pub(super) stiffness: f64,
    pub(super) damping: f64,
    pub(super) mass: f64,
    rest_speed: Option<f64>,
    rest_delta: Option<f64>,
    /// Seconds a spring given as `duration` and `bounce` is defined to take: Motion ends such a
    /// spring when that time has passed, where one of physics ends when it comes to rest.
    pub(super) time_defined: Option<f64>,
}

impl Spring {
    pub(super) fn physics(stiffness: f64, damping: f64, rest_speed: f64) -> Self {
        Self {
            stiffness,
            damping,
            mass: DEFAULT_MASS,
            rest_speed: Some(rest_speed),
            rest_delta: None,
            time_defined: None,
        }
    }

    pub(super) fn from_raw(raw: &RawTransition) -> Result<Self, String> {
        let has_physics = raw.stiffness.is_some() || raw.damping.is_some() || raw.mass.is_some();
        let has_time = raw.duration.is_some() || raw.bounce.is_some();
        if has_physics && has_time {
            return Err(
                "motion spring accepts either stiffness, damping and mass or duration and bounce, not both"
                    .to_string(),
            );
        }

        let (stiffness, damping, mass) = if has_time {
            let duration = raw.duration.unwrap_or(DEFAULT_SPRING_DURATION);
            let bounce = raw.bounce.unwrap_or(DEFAULT_SPRING_BOUNCE);
            if !(MIN_SPRING_DURATION..=MAX_SPRING_DURATION).contains(&duration) {
                return Err(format!(
                    "motion spring duration must be between {MIN_SPRING_DURATION} and {MAX_SPRING_DURATION} seconds"
                ));
            }
            if !(0.0..=1.0).contains(&bounce) {
                return Err("motion spring bounce must be between 0 and 1".to_string());
            }
            let (stiffness, damping) = find_spring(duration, bounce)?;
            (stiffness, damping, 1.0)
        } else {
            (
                raw.stiffness.unwrap_or(DEFAULT_STIFFNESS),
                raw.damping.unwrap_or(DEFAULT_DAMPING),
                raw.mass.unwrap_or(DEFAULT_MASS),
            )
        };

        for (name, value) in [
            ("stiffness", stiffness),
            ("damping", damping),
            ("mass", mass),
        ] {
            if !value.is_finite() || value <= 0.0 {
                return Err(format!(
                    "motion spring {name} must be a finite number greater than 0"
                ));
            }
        }
        for (name, value) in [("restSpeed", raw.rest_speed), ("restDelta", raw.rest_delta)] {
            if value.is_some_and(|value| !value.is_finite() || value <= 0.0) {
                return Err(format!(
                    "motion spring {name} must be a finite number greater than 0"
                ));
            }
        }

        let spring = Self {
            stiffness,
            damping,
            mass,
            rest_speed: raw.rest_speed,
            rest_delta: raw.rest_delta,
            time_defined: has_time.then(|| raw.duration.unwrap_or(DEFAULT_SPRING_DURATION)),
        };
        if !(spring.angular_frequency() <= MAX_SPRING_RATE && spring.decay() <= MAX_SPRING_RATE) {
            return Err(
                "motion spring is too stiff: sqrt(stiffness / mass) and damping / (2 * mass) must be at most 1e6"
                    .to_string(),
            );
        }
        Ok(spring)
    }

    pub(super) fn rest_speed(&self) -> Option<f64> {
        self.rest_speed
    }

    pub(super) fn rest_delta(&self) -> Option<f64> {
        self.rest_delta
    }

    fn angular_frequency(&self) -> f64 {
        (self.stiffness / self.mass).sqrt()
    }

    /// The exponential decay rate of the envelope, `damping ratio * frequency`.
    fn decay(&self) -> f64 {
        self.damping / (2.0 * self.mass)
    }

    /// Rest thresholds as `(speed, displacement)`. Short moves use tighter
    /// ones, as motion-dom's `setRestThresholds` does.
    ///
    /// A `percent` channel is a fraction of a basis that is usually hundreds of px,
    /// so its thresholds are the tight ones divided by 100 whatever the distance:
    /// the default 0.005 of a fraction would snap by 2px of a 400px parent, where
    /// this snaps by under 0.1px up to a 1000px one. `restSpeed` and `restDelta`
    /// are in percentage points there.
    pub(super) fn rest_thresholds(&self, distance: f64, percent: bool) -> (f64, f64) {
        let granular = percent || distance.abs() < GRANULAR_DISTANCE;
        let unit = if percent { PERCENT_REST_SCALE } else { 1.0 };
        (
            self.rest_speed.unwrap_or(if granular {
                GRANULAR_REST_SPEED
            } else {
                DEFAULT_REST_SPEED
            }) * unit,
            self.rest_delta.unwrap_or(if granular {
                GRANULAR_REST_DELTA
            } else {
                DEFAULT_REST_DELTA
            }) * unit,
        )
    }

    /// Closed-form solution of `m x'' + c x' + k x = 0` for the offset from the
    /// target, given the offset and its rate at `t = 0`. Returns the offset and
    /// its rate after `t` seconds. Evaluated from the segment start, so it does
    /// not depend on how often it is sampled.
    pub(super) fn evolve(&self, offset: f64, rate: f64, t: f64) -> (f64, f64) {
        let omega = self.angular_frequency();
        let decay = self.decay();
        let damping_ratio = decay / omega;

        if (damping_ratio - 1.0).abs() < CRITICAL_DAMPING_TOLERANCE {
            let envelope = (-omega * t).exp();
            let coefficient = rate + omega * offset;
            (
                envelope * (offset + coefficient * t),
                envelope * (rate - omega * coefficient * t),
            )
        } else if damping_ratio < 1.0 {
            let damped = omega * (1.0 - damping_ratio * damping_ratio).sqrt();
            let envelope = (-decay * t).exp();
            let (sin, cos) = (damped * t).sin_cos();
            let sin_coefficient = (rate + decay * offset) / damped;
            (
                envelope * (offset * cos + sin_coefficient * sin),
                envelope * (rate * cos - (omega * omega * offset + decay * rate) / damped * sin),
            )
        } else {
            // A sum of two decaying exponentials rather than cosh and sinh, so
            // neither term can overflow however long the spring has run.
            let spread = omega * (damping_ratio * damping_ratio - 1.0).sqrt();
            let slow = spread - decay;
            let fast = -spread - decay;
            let slow_weight = (rate - fast * offset) / (2.0 * spread);
            let fast_weight = offset - slow_weight;
            let slow_decay = (slow * t).exp();
            let fast_decay = (fast * t).exp();
            (
                slow_weight * slow_decay + fast_weight * fast_decay,
                slow_weight * slow * slow_decay + fast_weight * fast * fast_decay,
            )
        }
    }
}

/// Resolves `duration` (seconds to settle) and `bounce` (0 critically damped,
/// toward 1 undamped) into `(stiffness, damping)` for a unit mass.
///
/// Ported from `findSpring` and `bounceToDampingRatio` in motion-dom
/// (`animation/generators/spring.mjs`, which Framer Motion's `spring` uses): a
/// Newton search for the undamped frequency at which the envelope has decayed
/// to `SAFE_MIN` of the distance after `duration`. Only the `0..=1` bounce range
/// is ported; motion-dom also accepts negative (overdamped) bounce.
pub(super) fn find_spring(duration: f64, bounce: f64) -> Result<(f64, f64), String> {
    const SAFE_MIN: f64 = 0.001;
    const ROOT_ITERATIONS: usize = 12;

    let damping_ratio = (1.0 - bounce).max(MIN_DAMPING_RATIO);
    let envelope = |undamped_freq: f64| {
        if damping_ratio < 1.0 {
            let exponential_decay = undamped_freq * damping_ratio;
            let damped_freq = undamped_freq * (1.0 - damping_ratio * damping_ratio).sqrt();
            SAFE_MIN - (exponential_decay / damped_freq) * (-exponential_decay * duration).exp()
        } else {
            -SAFE_MIN + (-undamped_freq * duration).exp() * (undamped_freq * duration + 1.0)
        }
    };
    let derivative = |undamped_freq: f64| {
        if damping_ratio < 1.0 {
            let exponential_decay = undamped_freq * damping_ratio;
            let e = damping_ratio * damping_ratio * undamped_freq * undamped_freq * duration;
            let f = (-exponential_decay * duration).exp();
            let g = undamped_freq * undamped_freq * (1.0 - damping_ratio * damping_ratio).sqrt();
            let factor = if -envelope(undamped_freq) + SAFE_MIN > 0.0 {
                -1.0
            } else {
                1.0
            };
            (factor * -e * f) / g
        } else {
            (-undamped_freq * duration).exp() * (-undamped_freq * duration * duration)
        }
    };

    let mut undamped_freq = 5.0 / duration;
    for _ in 1..ROOT_ITERATIONS {
        undamped_freq -= envelope(undamped_freq) / derivative(undamped_freq);
    }
    let stiffness = undamped_freq * undamped_freq;
    let damping = damping_ratio * 2.0 * stiffness.sqrt();
    if !(stiffness.is_finite() && damping.is_finite()) {
        return Err(format!(
            "motion spring duration {duration} and bounce {bounce} did not resolve to a spring"
        ));
    }
    Ok((stiffness, damping))
}
pub(super) fn validate_seconds(value: f64, name: &str) -> Result<(), String> {
    if !value.is_finite() || value < 0.0 || Duration::try_from_secs_f64(value).is_err() {
        return Err(format!(
            "motion {name} must be a supported finite non-negative number"
        ));
    }
    Ok(())
}

const FRAMER_EASES: [&str; 5] = ["linear", "ease", "easeIn", "easeOut", "easeInOut"];

pub(super) fn validate_ease(ease: &Ease) -> Result<(), String> {
    match ease {
        Ease::Name(name) if FRAMER_EASES.contains(&name.as_str()) => {}
        Ease::Name(name) => return Err(format!("unknown motion easing: {name}")),
        Ease::Steps(_) | Ease::Stops(_) => {}
        Ease::CubicBezier([x1, y1, x2, y2]) => {
            if ![x1, y1, x2, y2].iter().all(|value| value.is_finite())
                || !(0.0..=1.0).contains(x1)
                || !(0.0..=1.0).contains(x2)
            {
                return Err(
                    "motion cubic bezier values must be finite and x values must be 0..1"
                        .to_string(),
                );
            }
        }
    }
    Ok(())
}

pub(super) fn seconds(value: f64) -> Duration {
    Duration::try_from_secs_f64(value).expect("motion durations are validated when parsed")
}

pub(super) fn ease(progress: f64, ease: &Ease) -> f64 {
    let curve = match ease {
        Ease::Steps(steps) => return steps.output(progress),
        Ease::Stops(stops) => return linear_stops(progress, stops),
        Ease::CubicBezier(curve) => *curve,
        Ease::Name(name) => match name.as_str() {
            "linear" => return progress,
            "easeIn" => [0.42, 0.0, 1.0, 1.0],
            "easeInOut" => [0.42, 0.0, 0.58, 1.0],
            "ease" => [0.25, 0.1, 0.25, 1.0],
            _ => [0.0, 0.0, 0.58, 1.0],
        },
    };
    cubic_bezier(progress, curve)
}

impl Steps {
    /// CSS Easing 1 `steps()` output for an input in `0..=1`.
    fn output(self, progress: f64) -> f64 {
        let count = f64::from(self.count);
        let mut step = (progress * count).floor();
        if matches!(self.jump, Jump::Start | Jump::Both) {
            step += 1.0;
        }
        let steps = match self.jump {
            Jump::Both => count + 1.0,
            Jump::None => count - 1.0,
            Jump::Start | Jump::End => count,
        };
        (step / steps).clamp(0.0, 1.0)
    }
}

/// Piecewise-linear output of CSS Easing 2 `linear()`. A jump (two stops at one
/// input) takes the later stop's output, and inputs outside the stops continue
/// along the nearest segment.
fn linear_stops(progress: f64, stops: &[(f64, f64)]) -> f64 {
    let (from, to) = stops_segment(progress, stops);
    if to.0 == from.0 {
        return if progress < stops[0].0 { stops[0].1 } else { stops[stops.len() - 1].1 };
    }
    from.1 + (to.1 - from.1) * (progress - from.0) / (to.0 - from.0)
}

/// The two stops `linear_stops` interpolates between at `progress`.
fn stops_segment(progress: f64, stops: &[(f64, f64)]) -> ((f64, f64), (f64, f64)) {
    let (first, last) = (stops[0], stops[stops.len() - 1]);
    if progress < first.0 {
        (first, stops[1])
    } else if progress >= last.0 {
        (stops[stops.len() - 2], last)
    } else {
        let next = stops.partition_point(|stop| stop.0 <= progress);
        (stops[next - 1], stops[next])
    }
}

/// Parses a CSS `<easing-function>`: `linear`, `ease`, `ease-in`, `ease-out`,
/// `ease-in-out`, `cubic-bezier()`, `steps()`, `step-start`, `step-end` and
/// `linear()` with stops.
pub(super) fn parse_timing_function(text: &str) -> Result<Ease, String> {
    let text = text.trim().to_ascii_lowercase();
    // The keywords stay names, as Chrome reads `ease` back as `ease` and `cubic-bezier(.25, .1, .25, 1)` as itself.
    let named = |name: &str| Ok(Ease::Name(name.to_string()));
    match text.as_str() {
        "linear" => return named("linear"),
        "ease" => return named("ease"),
        "ease-in" => return named("easeIn"),
        "ease-out" => return named("easeOut"),
        "ease-in-out" => return named("easeInOut"),
        "step-start" => return Ok(Ease::Steps(Steps { count: 1, jump: Jump::Start, jump_start_term: false })),
        "step-end" => return Ok(Ease::Steps(Steps { count: 1, jump: Jump::End, jump_start_term: false })),
        _ => {}
    }
    let (name, arguments) = text
        .strip_suffix(')')
        .and_then(|text| text.split_once('('))
        .ok_or_else(|| format!("not a timing function: {text}"))?;
    let arguments: Vec<&str> = arguments.split(',').map(str::trim).collect();
    match name.trim_end() {
        "cubic-bezier" => {
            let [x1, y1, x2, y2] = arguments[..] else {
                return Err("cubic-bezier() takes four numbers".to_string());
            };
            let (x1, y1, x2, y2) = (
                parse_number(x1)?,
                parse_number(y1)?,
                parse_number(x2)?,
                parse_number(y2)?,
            );
            if !(0.0..=1.0).contains(&x1) || !(0.0..=1.0).contains(&x2) {
                return Err("cubic-bezier() x values must be between 0 and 1".to_string());
            }
            Ok(Ease::CubicBezier([x1, y1, x2, y2]))
        }
        "steps" => parse_steps(&arguments),
        "linear" => parse_linear_stops(&arguments),
        other => Err(format!("unknown timing function: {other}()")),
    }
}

impl Ease {
    /// The text Chrome gives an easing in `getTiming().easing` and in the `easing` of a keyframe:
    /// the keyword for a keyword, `cubic-bezier()` for four numbers as they were written (the
    /// numbers of `ease` written out are not `ease`), `steps(n)` for a jump at the end,
    /// `steps(n, start)` or `steps(n, jump-start)` as the term was written, and `linear()` with
    /// every stop's input in percent.
    pub(super) fn to_css(&self) -> Result<String, String> {
        match self {
            Self::Name(name) => match name.as_str() {
                "linear" => Ok("linear".to_string()),
                "ease" => Ok("ease".to_string()),
                "easeIn" => Ok("ease-in".to_string()),
                "easeOut" => Ok("ease-out".to_string()),
                "easeInOut" => Ok("ease-in-out".to_string()),
                other => Err(format!("`{other}` is not an easing")),
            },
            Self::CubicBezier(curve) => Ok(format!(
                "cubic-bezier({}, {}, {}, {})",
                shortest(curve[0]),
                shortest(curve[1]),
                shortest(curve[2]),
                shortest(curve[3])
            )),
            Self::Steps(steps) => Ok(match steps.jump {
                Jump::End => format!("steps({})", steps.count),
                Jump::Start if steps.jump_start_term => format!("steps({}, jump-start)", steps.count),
                Jump::Start => format!("steps({}, start)", steps.count),
                Jump::None => format!("steps({}, jump-none)", steps.count),
                Jump::Both => format!("steps({}, jump-both)", steps.count),
            }),
            Self::Stops(stops) => Ok(format!(
                "linear({})",
                stops
                    .iter()
                    .map(|(input, output)| format!("{} {}%", shortest(*output), shortest(percent(*input))))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }
}

/// An input of `linear()` in percent, without the noise of the division (`0.58 * 100` is `57.99999999999999`).
fn percent(input: f64) -> f64 {
    format!("{:.12e}", input * 100.0).parse().expect("a float written in scientific notation reads back")
}

fn parse_number(text: &str) -> Result<f64, String> {
    text.parse::<f64>()
        .ok()
        .filter(|number| number.is_finite())
        .ok_or_else(|| format!("not a number: {text}"))
}

fn parse_steps(arguments: &[&str]) -> Result<Ease, String> {
    let (count, jump_term) = match arguments {
        [count] => (count, "jump-end"),
        [count, jump] => (count, *jump),
        _ => return Err("steps() takes a count and an optional jump term".to_string()),
    };
    let count: u32 = count
        .parse()
        .ok()
        .filter(|count| *count >= 1)
        .ok_or_else(|| format!("steps() count must be a positive integer: {count}"))?;
    let jump = match jump_term {
        "jump-start" | "start" => Jump::Start,
        "jump-end" | "end" => Jump::End,
        "jump-none" => Jump::None,
        "jump-both" => Jump::Both,
        other => return Err(format!("unknown steps() jump term: {other}")),
    };
    if jump == Jump::None && count < 2 {
        return Err("steps() with jump-none needs at least two steps".to_string());
    }
    Ok(Ease::Steps(Steps { count, jump, jump_start_term: jump_term == "jump-start" }))
}

/// CSS Easing 2 `linear()`: each stop is a number with up to two percentage
/// inputs. Missing inputs are filled in as the spec says: the first stop is 0%,
/// the last 100%, a stop below an earlier one rises to it, and a run of unset
/// stops spreads evenly between its neighbours.
fn parse_linear_stops(arguments: &[&str]) -> Result<Ease, String> {
    let mut stops: Vec<(Option<f64>, f64)> = Vec::new();
    for argument in arguments {
        let mut parts = argument.split_whitespace();
        let output = parse_number(parts.next().ok_or("linear() has an empty stop")?)?;
        let inputs: Vec<f64> = parts
            .map(|part| {
                let percent = part
                    .strip_suffix('%')
                    .ok_or_else(|| format!("linear() stop input must be a percentage: {part}"))?;
                Ok(parse_number(percent)? / 100.0)
            })
            .collect::<Result<_, String>>()?;
        match inputs[..] {
            [] => stops.push((None, output)),
            [input] => stops.push((Some(input), output)),
            [start, end] => {
                stops.push((Some(start), output));
                stops.push((Some(end), output));
            }
            _ => return Err("linear() stop takes at most two inputs".to_string()),
        }
    }
    if stops.len() < 2 {
        return Err("linear() needs at least two stops".to_string());
    }
    let last = stops.len() - 1;
    stops[0].0.get_or_insert(0.0);
    if stops[last].0.is_none() {
        stops[last].0 = Some(1.0);
    }
    let mut largest = f64::NEG_INFINITY;
    for stop in &mut stops {
        if let Some(input) = stop.0 {
            largest = largest.max(input);
            stop.0 = Some(largest);
        }
    }
    let mut resolved = Vec::with_capacity(stops.len());
    let mut index = 0;
    while index < stops.len() {
        if let Some(input) = stops[index].0 {
            resolved.push((input, stops[index].1));
            index += 1;
            continue;
        }
        let run_end = (index..stops.len())
            .find(|candidate| stops[*candidate].0.is_some())
            .expect("the last stop always has an input");
        let before = resolved[index - 1].0;
        let after = stops[run_end].0.expect("found above");
        let count = (run_end - index + 1) as f64;
        for offset in index..run_end {
            let fraction = (offset - index + 1) as f64 / count;
            resolved.push((before + (after - before) * fraction, stops[offset].1));
        }
        index = run_end;
    }
    Ok(Ease::Stops(resolved))
}

/// Slope of the easing curve at `progress`. `steps()` is flat between its
/// jumps and `linear()` is flat across one, so both are 0 there: a central
/// difference would straddle the jump and report thousands of units per second,
/// which a spring retargeted next to it would take as the speed to carry. A
/// `linear()` segment has its own constant slope. A bezier is smooth, so a
/// central difference over the unit interval is enough and stays defined at
/// both ends.
pub(super) fn ease_slope(progress: f64, curve: &Ease) -> f64 {
    const STEP: f64 = 1e-4;
    match curve {
        Ease::Steps(_) => 0.0,
        Ease::Stops(stops) => {
            let (from, to) = stops_segment(progress, stops);
            if to.0 == from.0 {
                0.0
            } else {
                (to.1 - from.1) / (to.0 - from.0)
            }
        }
        Ease::Name(name) if name == "linear" => 1.0,
        Ease::Name(_) | Ease::CubicBezier(_) => {
            let low = (progress - STEP).max(0.0);
            let high = (progress + STEP).min(1.0);
            (ease(high, curve) - ease(low, curve)) / (high - low)
        }
    }
}

pub(crate) fn cubic_bezier(x: f64, [x1, y1, x2, y2]: [f64; 4]) -> f64 {
    fn sample(t: f64, a: f64, b: f64) -> f64 {
        let c = 3.0 * a;
        let b = 3.0 * (b - a) - c;
        let a = 1.0 - c - b;
        ((a * t + b) * t + c) * t
    }

    if x <= 0.0 || x >= 1.0 {
        return x.clamp(0.0, 1.0);
    }
    let mut low = 0.0;
    let mut high = 1.0;
    for _ in 0..20 {
        let middle = (low + high) / 2.0;
        if sample(middle, x1, x2) < x {
            low = middle;
        } else {
            high = middle;
        }
    }
    // Not clamped: CSS and Framer both let `y` leave 0..1, which is how a
    // back-ease overshoots.
    sample((low + high) / 2.0, y1, y2)
}

#[cfg(test)]
mod tests {

    #[test]
    fn an_easing_reads_back_as_it_was_written() {
        let back = |text: &str| css(text).to_css().unwrap();
        assert_eq!(back("EASE-IN"), "ease-in");
        assert_eq!(back("cubic-bezier(.25,.1,.25,1)"), "cubic-bezier(0.25, 0.1, 0.25, 1)");
        assert_eq!(back("cubic-bezier(0, 0, 1, 1)"), "cubic-bezier(0, 0, 1, 1)");
        assert_eq!(back("cubic-bezier(0.1, 1e-7, 0.9, 1)"), "cubic-bezier(0.1, 1e-7, 0.9, 1)");
        assert_eq!(back("step-start"), "steps(1, start)");
        assert_eq!(back("step-end"), "steps(1)");
        assert_eq!(back("steps( 2 , END )"), "steps(2)");
        assert_eq!(back("steps(4, start)"), "steps(4, start)");
        assert_eq!(back("steps(4, jump-start)"), "steps(4, jump-start)");
        assert_eq!(back("steps(3, jump-both)"), "steps(3, jump-both)");
        assert_eq!(back("linear(0, 1)"), "linear(0 0%, 1 100%)");
        assert_eq!(back("linear(0, 0.5 58%, 1)"), "linear(0 0%, 0.5 58%, 1 100%)");
        assert_eq!(back("linear(0 30%, 0.5 20%, 1)"), "linear(0 30%, 0.5 30%, 1 100%)");
    }
    use super::*;

    #[test]
    fn a_bare_spring_uses_framer_defaults() {
        let raw: RawTransition =
            serde_json::from_value(serde_json::json!({ "type": "spring" })).unwrap();
        let spring = Spring::from_raw(&raw).unwrap();
        assert_eq!(
            (spring.stiffness, spring.damping, spring.mass),
            (100.0, 10.0, 1.0)
        );
    }

    #[test]
    fn find_spring_resolves_every_supported_input() {
        for duration in [0.01, 0.05, 0.08, 0.3, 1.0, 5.0, 10.0] {
            for bounce in [0.0, 0.05, 0.12, 0.25, 0.5, 0.9, 1.0] {
                let (stiffness, damping) = find_spring(duration, bounce).unwrap();
                assert!(stiffness > 0.0 && damping > 0.0, "{duration} {bounce}");
                let raw = RawTransition {
                    kind: Some("spring".to_string()),
                    duration: Some(duration),
                    bounce: Some(bounce),
                    ..RawTransition::default()
                };
                Spring::from_raw(&raw).unwrap();
            }
        }
    }
    fn css(text: &str) -> Ease {
        parse_timing_function(text).unwrap()
    }

    fn at(text: &str, progress: f64) -> f64 {
        ease(progress, &css(text))
    }

    #[test]
    fn css_keywords_are_the_standard_cubic_beziers() {
        assert_eq!(css("ease"), Ease::Name("ease".to_string()));
        assert_eq!(css("EASE-IN"), Ease::Name("easeIn".to_string()));
        assert_eq!(css("ease-out"), Ease::Name("easeOut".to_string()));
        assert_eq!(at("ease", 0.3), at("cubic-bezier(0.25, 0.1, 0.25, 1)", 0.3));
        assert!((at("ease-in-out", 0.5) - 0.5).abs() < 1e-3);
        assert_eq!(at("linear", 0.3), 0.3);
        assert_eq!(
            css(" cubic-bezier( 0.1 , 0.2,0.3 , 1 ) "),
            Ease::CubicBezier([0.1, 0.2, 0.3, 1.0])
        );
    }

    #[test]
    fn a_cubic_bezier_may_overshoot() {
        let back = "cubic-bezier(0.34, 1.56, 0.64, 1)";
        assert!(at(back, 0.6) > 1.05, "{}", at(back, 0.6));
        assert_eq!(at(back, 1.0), 1.0);
        assert!(at("cubic-bezier(0.6, -0.5, 0.7, 0.7)", 0.2) < 0.0);
    }

    #[test]
    fn steps_jump_at_the_ends_the_term_names() {
        let outputs = |text: &str| {
            [0.0, 0.24, 0.25, 0.5, 0.99, 1.0].map(|progress| at(text, progress))
        };
        assert_eq!(outputs("steps(4)"), [0.0, 0.0, 0.25, 0.5, 0.75, 1.0]);
        assert_eq!(outputs("steps(4, end)"), outputs("steps(4, jump-end)"));
        assert_eq!(outputs("steps(4, jump-start)"), [0.25, 0.25, 0.5, 0.75, 1.0, 1.0]);
        assert_eq!(outputs("steps(4, start)"), outputs("steps(4, jump-start)"));
        assert_eq!(at("steps(3, jump-none)", 0.0), 0.0);
        assert_eq!(at("steps(3, jump-none)", 0.5), 0.5);
        assert_eq!(at("steps(3, jump-none)", 0.7), 1.0);
        assert_eq!(at("steps(2, jump-both)", 0.0), 1.0 / 3.0);
        assert_eq!(at("steps(2, jump-both)", 0.5), 2.0 / 3.0);
        assert_eq!(at("steps(2, jump-both)", 1.0), 1.0);
        assert_eq!((at("step-start", 0.0), at("step-start", 0.5)), (1.0, 1.0));
        assert_eq!((at("step-end", 0.99), at("step-end", 1.0)), (0.0, 1.0));
    }

    #[test]
    fn linear_stops_interpolate_between_their_inputs() {
        assert_eq!(at("linear(0, 0.25 75%, 1)", 0.375), 0.125);
        assert_eq!(at("linear(0, 0.25 75%, 1)", 0.75), 0.25);
        assert_eq!(at("linear(0, 0.25 75%, 1)", 0.875), 0.625);
        assert_eq!(at("linear(0, 0.25 75%, 1)", 1.0), 1.0);
    }

    #[test]
    fn linear_stops_without_an_input_spread_evenly() {
        let stops = css("linear(0, 0.5, 0.25, 1)");
        assert_eq!(
            stops,
            Ease::Stops(vec![(0.0, 0.0), (1.0 / 3.0, 0.5), (2.0 / 3.0, 0.25), (1.0, 1.0)])
        );
    }

    #[test]
    fn linear_stops_with_two_inputs_hold_and_a_stop_never_runs_backwards() {
        assert_eq!(
            css("linear(0, 0.5 20% 40%, 1)"),
            Ease::Stops(vec![(0.0, 0.0), (0.2, 0.5), (0.4, 0.5), (1.0, 1.0)])
        );
        assert_eq!(
            css("linear(0, 0.5 60%, 0.8 30%, 1)"),
            Ease::Stops(vec![(0.0, 0.0), (0.6, 0.5), (0.6, 0.8), (1.0, 1.0)])
        );
    }

    #[test]
    fn a_linear_jump_takes_the_later_stop_and_the_ends_extrapolate() {
        let jump = "linear(0, 0.5 50%, 1 50%, 1)";
        assert!((at(jump, 0.49) - 0.49).abs() < 1e-9);
        assert_eq!(at(jump, 0.5), 1.0);
        assert_eq!(at(jump, 0.9), 1.0);
        assert_eq!(at("linear(0 10%, 1 20%)", 0.0), -1.0);
        assert_eq!(at("linear(0, 1 50%)", 0.75), 1.5);
    }

    #[test]
    fn rejects_malformed_timing_functions() {
        for bad in [
            "",
            "bouncy",
            "ease(2)",
            "cubic-bezier(0, 0, 1)",
            "cubic-bezier(2, 0, 0, 1)",
            "cubic-bezier(a, 0, 0, 1)",
            "steps(0)",
            "steps(1.5)",
            "steps(1, jump-none)",
            "steps(2, middle)",
            "steps()",
            "linear(1)",
            "linear(0, 1 50)",
            "linear(0, 1 10% 20% 30%)",
            "linear(0, nan)",
        ] {
            assert!(parse_timing_function(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_motion_ease_accepts_css_timing_functions() {
        let tween = |ease: &str| {
            Transition::parse(serde_json::json!({ "duration": 1.0, "ease": ease }))
        };
        for accepted in ["ease-in", "steps(4, jump-start)", "linear(0, 1)", "easeInOut"] {
            assert!(tween(accepted).is_ok(), "{accepted}");
        }
        assert!(tween("steps(0)").is_err());
        assert!(tween("sproing").is_err());
    }

    #[test]
    fn slope_is_zero_at_and_next_to_a_steps_jump() {
        for text in ["steps(4)", "steps(4, jump-start)", "steps(4, jump-both)", "steps(3, jump-none)", "step-end"] {
            let curve = css(text);
            for progress in [0.0, 0.25 - 1e-9, 0.25, 0.25 + 1e-9, 0.5 - 5e-5, 0.5, 1.0 - 1e-5, 1.0] {
                assert_eq!(ease_slope(progress, &curve), 0.0, "{text} at {progress}");
            }
        }
    }

    #[test]
    fn slope_of_linear_stops_is_the_segment_slope_and_zero_across_a_jump() {
        let curve = css("linear(0, 0.5 25%, 0.5 50%, 1 50%, 0.5 100%)");
        assert!((ease_slope(0.1, &curve) - 2.0).abs() < 1e-9);
        // The plateau between 25% and 50%.
        assert_eq!(ease_slope(0.3, &curve), 0.0);
        // Immediately either side of the jump at 50%, and on it.
        assert_eq!(ease_slope(0.5 - 1e-6, &curve), 0.0);
        assert!((ease_slope(0.5, &curve) + 1.0).abs() < 1e-9);
        assert!((ease_slope(0.75, &curve) + 1.0).abs() < 1e-9);
        let jump_only = css("linear(0, 0 50%, 1 50%, 1)");
        for progress in [0.5 - 1e-6, 0.5 - 1e-12, 0.5, 0.5 + 1e-12, 0.5 + 1e-6] {
            assert_eq!(ease_slope(progress, &jump_only), 0.0, "at {progress}");
        }
    }

    #[test]
    fn slope_of_the_linear_name_is_one_and_a_bezier_stays_finite_at_its_ends() {
        assert_eq!(ease_slope(0.0, &Ease::Name("linear".to_string())), 1.0);
        assert_eq!(ease_slope(1.0, &Ease::Name("linear".to_string())), 1.0);
        for text in ["ease", "ease-in", "ease-out", "ease-in-out"] {
            let curve = css(text);
            for progress in [0.0, 0.5, 1.0] {
                assert!(ease_slope(progress, &curve).is_finite(), "{text} at {progress}");
            }
        }
    }

    #[test]
    fn a_percentage_channel_rests_at_a_hundredth_of_the_tight_thresholds() {
        let spring = Spring::from_raw(&RawTransition {
            kind: Some("spring".to_string()),
            ..RawTransition::default()
        })
        .unwrap();
        // px: tight under 5px, loose beyond.
        assert_eq!(spring.rest_thresholds(1.0, false), (0.01, 0.005));
        assert_eq!(spring.rest_thresholds(100.0, false), (2.0, 0.5));
        // A fraction of 1 is the whole basis, so even a full-width move is tight:
        // 5e-5 of a 1000px basis is 0.05px.
        assert_eq!(spring.rest_thresholds(1.0, true), (0.0001, 0.00005));
        assert_eq!(spring.rest_thresholds(0.01, true), (0.0001, 0.00005));

        let explicit = Spring::from_raw(&RawTransition {
            kind: Some("spring".to_string()),
            rest_speed: Some(3.0),
            rest_delta: Some(0.2),
            ..RawTransition::default()
        })
        .unwrap();
        assert_eq!(explicit.rest_thresholds(100.0, false), (3.0, 0.2));
        assert_eq!(explicit.rest_thresholds(1.0, true), (0.03, 0.002));
    }
}
