//! Native motion tracks resolved during GPUI rendering, outside React.

use std::time::Duration;

use serde::Deserialize;
use web_time::Instant;

use crate::style::{DimensionValue, StyleDesc};

const PROPERTY_COUNT: usize = 8;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MotionStyle {
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub opacity: Option<f64>,
    pub top: Option<f64>,
    pub right: Option<f64>,
    pub bottom: Option<f64>,
    pub left: Option<f64>,
    pub border_radius: Option<f64>,
}

impl MotionStyle {
    fn with_fallback(self, fallback: Self) -> Self {
        Self {
            width: self.width.or(fallback.width),
            height: self.height.or(fallback.height),
            opacity: self.opacity.or(fallback.opacity),
            top: self.top.or(fallback.top),
            right: self.right.or(fallback.right),
            bottom: self.bottom.or(fallback.bottom),
            left: self.left.or(fallback.left),
            border_radius: self.border_radius.or(fallback.border_radius),
        }
    }

    fn interpolate(self, target: Self, progress: f64) -> Self {
        fn value(from: Option<f64>, to: Option<f64>, progress: f64) -> Option<f64> {
            to.map(|to| from.unwrap_or(to) + (to - from.unwrap_or(to)) * progress)
        }

        Self {
            width: value(self.width, target.width, progress),
            height: value(self.height, target.height, progress),
            opacity: value(self.opacity, target.opacity, progress),
            top: value(self.top, target.top, progress),
            right: value(self.right, target.right, progress),
            bottom: value(self.bottom, target.bottom, progress),
            left: value(self.left, target.left, progress),
            border_radius: value(self.border_radius, target.border_radius, progress),
        }
    }

    fn interpolate_rate(self, target: Self, rate: f64) -> Self {
        fn value(from: Option<f64>, to: Option<f64>, rate: f64) -> Option<f64> {
            to.map(|to| (to - from.unwrap_or(to)) * rate)
        }

        Self {
            width: value(self.width, target.width, rate),
            height: value(self.height, target.height, rate),
            opacity: value(self.opacity, target.opacity, rate),
            top: value(self.top, target.top, rate),
            right: value(self.right, target.right, rate),
            bottom: value(self.bottom, target.bottom, rate),
            left: value(self.left, target.left, rate),
            border_radius: value(self.border_radius, target.border_radius, rate),
        }
    }

    fn to_array(self) -> [Option<f64>; PROPERTY_COUNT] {
        [
            self.width,
            self.height,
            self.opacity,
            self.top,
            self.right,
            self.bottom,
            self.left,
            self.border_radius,
        ]
    }

    fn from_array(values: [Option<f64>; PROPERTY_COUNT]) -> Self {
        Self {
            width: values[0],
            height: values[1],
            opacity: values[2],
            top: values[3],
            right: values[4],
            bottom: values[5],
            left: values[6],
            border_radius: values[7],
        }
    }

    /// Springs overshoot their target, so what is applied has to stay inside
    /// the range GPUI accepts. The unclamped value keeps driving the physics.
    fn clamped(self) -> Self {
        Self {
            width: self.width.map(|value| value.max(0.0)),
            height: self.height.map(|value| value.max(0.0)),
            opacity: self.opacity.map(|value| value.clamp(0.0, 1.0)),
            border_radius: self.border_radius.map(|value| value.max(0.0)),
            ..self
        }
    }

    pub(crate) fn apply_to(self, style: &mut StyleDesc) {
        if let Some(value) = self.width {
            style.width = Some(DimensionValue::Pixels(value));
        }
        if let Some(value) = self.height {
            style.height = Some(DimensionValue::Pixels(value));
        }
        if let Some(value) = self.opacity {
            style.opacity = Some(value);
        }
        if let Some(value) = self.top {
            style.top = Some(DimensionValue::Pixels(value));
        }
        if let Some(value) = self.right {
            style.right = Some(DimensionValue::Pixels(value));
        }
        if let Some(value) = self.bottom {
            style.bottom = Some(DimensionValue::Pixels(value));
        }
        if let Some(value) = self.left {
            style.left = Some(DimensionValue::Pixels(value));
        }
        if let Some(value) = self.border_radius {
            style.border_radius = Some(value);
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
enum MotionInitial {
    Disabled(bool),
    Style(MotionStyle),
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
enum MotionEase {
    Name(String),
    CubicBezier([f64; 4]),
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(try_from = "RawTransition")]
struct MotionTransition {
    delay: f64,
    curve: MotionCurve,
}

#[derive(Clone, Debug, PartialEq)]
enum MotionCurve {
    Tween { duration: f64, ease: MotionEase },
    Spring(Spring),
}

impl Default for MotionTransition {
    fn default() -> Self {
        Self {
            delay: 0.0,
            curve: MotionCurve::Tween {
                duration: default_duration(),
                ease: default_ease(),
            },
        }
    }
}

fn default_duration() -> f64 {
    0.3
}

fn default_ease() -> MotionEase {
    MotionEase::Name("easeOut".to_string())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTransition {
    #[serde(rename = "type")]
    kind: Option<String>,
    duration: Option<f64>,
    delay: Option<f64>,
    ease: Option<MotionEase>,
    stiffness: Option<f64>,
    damping: Option<f64>,
    mass: Option<f64>,
    bounce: Option<f64>,
    rest_speed: Option<f64>,
    rest_delta: Option<f64>,
}

impl TryFrom<RawTransition> for MotionTransition {
    type Error = String;

    fn try_from(raw: RawTransition) -> Result<Self, String> {
        let delay = raw.delay.unwrap_or(0.0);
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
                MotionCurve::Tween {
                    duration: raw.duration.unwrap_or_else(default_duration),
                    ease: raw.ease.clone().unwrap_or_else(default_ease),
                }
            }
            Some("spring") => {
                if raw.ease.is_some() {
                    return Err("motion spring transitions do not accept ease".to_string());
                }
                MotionCurve::Spring(Spring::from_raw(&raw)?)
            }
            Some(other) => return Err(format!("unknown motion transition type: {other}")),
        };
        Ok(Self { delay, curve })
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
/// Largest `sqrt(stiffness / mass)` and `damping / (2 * mass)` in rad/s. Past
/// this the closed-form terms can overflow to infinity and NaN out a frame.
const MAX_SPRING_RATE: f64 = 1e6;
/// A damping ratio this close to 1 is evaluated as critically damped; the
/// over- and under-damped forms divide by a term that vanishes at 1.
const CRITICAL_DAMPING_TOLERANCE: f64 = 1e-6;

#[derive(Clone, Debug, PartialEq)]
struct Spring {
    stiffness: f64,
    damping: f64,
    mass: f64,
    rest_speed: Option<f64>,
    rest_delta: Option<f64>,
}

impl Spring {
    fn from_raw(raw: &RawTransition) -> Result<Self, String> {
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
        };
        if !(spring.angular_frequency() <= MAX_SPRING_RATE && spring.decay() <= MAX_SPRING_RATE) {
            return Err(
                "motion spring is too stiff: sqrt(stiffness / mass) and damping / (2 * mass) must be at most 1e6"
                    .to_string(),
            );
        }
        Ok(spring)
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
    fn rest_thresholds(&self, distance: f64) -> (f64, f64) {
        let granular = distance.abs() < GRANULAR_DISTANCE;
        (
            self.rest_speed.unwrap_or(if granular {
                GRANULAR_REST_SPEED
            } else {
                DEFAULT_REST_SPEED
            }),
            self.rest_delta.unwrap_or(if granular {
                GRANULAR_REST_DELTA
            } else {
                DEFAULT_REST_DELTA
            }),
        )
    }

    /// Closed-form solution of `m x'' + c x' + k x = 0` for the offset from the
    /// target, given the offset and its rate at `t = 0`. Returns the offset and
    /// its rate after `t` seconds. Evaluated from the segment start, so it does
    /// not depend on how often it is sampled.
    fn evolve(&self, offset: f64, rate: f64, t: f64) -> (f64, f64) {
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
fn find_spring(duration: f64, bounce: f64) -> Result<(f64, f64), String> {
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

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct MotionDescription {
    #[serde(default)]
    generation: u64,
    #[serde(default)]
    is_exit: bool,
    #[serde(default)]
    initial: Option<MotionInitial>,
    animate: MotionStyle,
    #[serde(default)]
    transition: MotionTransition,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct MotionFrame {
    pub active: bool,
    pub just_settled: bool,
    pub generation: u64,
}

pub(crate) struct MotionState {
    source: serde_json::Value,
    from: MotionStyle,
    /// Rate of change of `from` in units per second. Only springs read it; a
    /// retarget hands the interrupted motion's speed to the next segment.
    from_velocity: MotionStyle,
    target: MotionStyle,
    transition: MotionTransition,
    started: Instant,
    valid: bool,
    needs_settle: bool,
    generation: u64,
}

struct Sample {
    position: MotionStyle,
    velocity: MotionStyle,
    active: bool,
}

impl MotionState {
    pub(crate) fn new(source: &serde_json::Value, now: Instant) -> Result<Self, String> {
        let description = parse_description(source)?;
        let from = match description.initial {
            Some(MotionInitial::Style(style)) => style,
            Some(MotionInitial::Disabled(false)) | None => description.animate,
            Some(MotionInitial::Disabled(true)) => unreachable!("validated above"),
        };

        let target = description.animate.with_fallback(from);
        Ok(Self {
            source: source.clone(),
            from,
            from_velocity: MotionStyle::default(),
            target,
            transition: description.transition,
            started: now,
            valid: true,
            needs_settle: description.is_exit || from != target,
            generation: description.generation,
        })
    }

    pub(crate) fn invalid(source: &serde_json::Value, now: Instant) -> Self {
        Self {
            source: source.clone(),
            from: MotionStyle::default(),
            from_velocity: MotionStyle::default(),
            target: MotionStyle::default(),
            transition: MotionTransition::default(),
            started: now,
            valid: false,
            needs_settle: source_is_exit(source),
            generation: source_generation(source),
        }
    }

    /// Retargets from wherever the element is now. A spring continues from the
    /// current speed as well, so interrupting it bends the path instead of
    /// stopping and restarting. An interrupted tween hands over its
    /// instantaneous speed too, so a tween-to-spring switch does not stall; a
    /// spring-to-tween switch drops the speed, as the tween has no notion of it.
    pub(crate) fn sync(&mut self, source: &serde_json::Value, now: Instant) -> Result<(), String> {
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
        let is_spring = matches!(description.transition.curve, MotionCurve::Spring(_));
        if self.valid {
            let current = self.sample(now);
            self.from = current.position;
            self.from_velocity = if is_spring {
                current.velocity
            } else {
                MotionStyle::default()
            };
        } else {
            self.from = match description.initial {
                Some(MotionInitial::Style(style)) => style,
                Some(MotionInitial::Disabled(false)) | None => description.animate,
                Some(MotionInitial::Disabled(true)) => unreachable!("validated above"),
            };
            self.from_velocity = MotionStyle::default();
        }
        self.target = description.animate.with_fallback(self.from);
        self.transition = description.transition;
        self.started = now;
        self.source = source.clone();
        self.valid = true;
        self.generation = description.generation;
        self.needs_settle = description.is_exit
            || self.generation != previous_generation
            || self.from != self.target
            || (is_spring
                && self
                    .from_velocity
                    .to_array()
                    .iter()
                    .flatten()
                    .any(|v| *v != 0.0));
        Ok(())
    }

    pub(crate) fn visible_style(&self, now: Instant) -> Option<MotionStyle> {
        self.valid.then(|| self.sample(now).position.clamped())
    }

    fn sample(&self, now: Instant) -> Sample {
        let elapsed = now.saturating_duration_since(self.started);
        let delay = seconds(self.transition.delay);
        match &self.transition.curve {
            MotionCurve::Tween { duration, ease } => {
                self.sample_tween(elapsed, delay, seconds(*duration), ease)
            }
            MotionCurve::Spring(spring) => self.sample_spring(elapsed, delay, spring),
        }
    }

    fn sample_tween(
        &self,
        elapsed: Duration,
        delay: Duration,
        duration: Duration,
        curve: &MotionEase,
    ) -> Sample {
        let raw = if duration.is_zero() {
            if elapsed < delay {
                0.0
            } else {
                1.0
            }
        } else if elapsed <= delay {
            0.0
        } else {
            elapsed.saturating_sub(delay).as_secs_f64() / duration.as_secs_f64()
        };
        let active = self.from != self.target && raw < 1.0;
        let progress = ease(raw.clamp(0.0, 1.0), curve);
        let progress_rate = if active && raw > 0.0 {
            ease_slope(raw, curve) / duration.as_secs_f64()
        } else {
            0.0
        };
        Sample {
            position: self.from.interpolate(self.target, progress),
            velocity: self.from.interpolate_rate(self.target, progress_rate),
            active,
        }
    }

    fn sample_spring(&self, elapsed: Duration, delay: Duration, spring: &Spring) -> Sample {
        // The delay holds the segment's start: the carried speed only applies
        // once the spring is released.
        let released = elapsed >= delay;
        let t = elapsed.saturating_sub(delay).as_secs_f64();
        let from = self.from.to_array();
        let target = self.target.to_array();
        let from_velocity = self.from_velocity.to_array();

        let mut position = [None; PROPERTY_COUNT];
        let mut velocity = [None; PROPERTY_COUNT];
        let mut at_rest = true;
        for index in 0..PROPERTY_COUNT {
            let Some(to) = target[index] else {
                continue;
            };
            let start = from[index].unwrap_or(to);
            let start_velocity = from_velocity[index].unwrap_or(0.0);
            if !released {
                position[index] = Some(start);
                velocity[index] = Some(0.0);
                at_rest &= start == to && start_velocity == 0.0;
                continue;
            }
            let (offset, rate) = spring.evolve(start - to, start_velocity, t);
            let (rest_speed, rest_delta) = spring.rest_thresholds(start - to);
            at_rest &= offset.abs() <= rest_delta && rate.abs() <= rest_speed;
            position[index] = Some(to + offset);
            velocity[index] = Some(rate);
        }

        if at_rest {
            return Sample {
                position: self.target,
                velocity: MotionStyle::from_array(target.map(|value| value.map(|_| 0.0))),
                active: false,
            };
        }
        Sample {
            position: MotionStyle::from_array(position),
            velocity: MotionStyle::from_array(velocity),
            active: true,
        }
    }

    pub(crate) fn frame(&mut self, now: Instant) -> MotionFrame {
        let active = self.valid && self.sample(now).active;
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

fn source_generation(source: &serde_json::Value) -> u64 {
    source
        .get("generation")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default()
}

fn source_is_exit(source: &serde_json::Value) -> bool {
    source
        .get("isExit")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or_default()
}

fn parse_description(source: &serde_json::Value) -> Result<MotionDescription, String> {
    let description: MotionDescription =
        serde_json::from_value(source.clone()).map_err(|error| error.to_string())?;

    if matches!(description.initial, Some(MotionInitial::Disabled(true))) {
        return Err("motion initial only accepts false or a style object".to_string());
    }
    validate_style(&description.animate)?;
    if let Some(MotionInitial::Style(initial)) = &description.initial {
        validate_style(initial)?;
    }
    validate_seconds(description.transition.delay, "delay")?;
    if let MotionCurve::Tween { duration, ease } = &description.transition.curve {
        validate_seconds(*duration, "duration")?;
        validate_ease(ease)?;
    }
    Ok(description)
}

fn validate_style(style: &MotionStyle) -> Result<(), String> {
    for (name, value) in [
        ("width", style.width),
        ("height", style.height),
        ("opacity", style.opacity),
        ("top", style.top),
        ("right", style.right),
        ("bottom", style.bottom),
        ("left", style.left),
        ("borderRadius", style.border_radius),
    ] {
        if value.is_some_and(|value| !value.is_finite() || value.abs() > f32::MAX as f64) {
            return Err(format!("motion {name} must fit a finite 32-bit float"));
        }
    }
    if style.width.is_some_and(|value| value < 0.0)
        || style.height.is_some_and(|value| value < 0.0)
        || style.border_radius.is_some_and(|value| value < 0.0)
    {
        return Err("motion sizes and borderRadius must be non-negative".to_string());
    }
    if style
        .opacity
        .is_some_and(|value| !(0.0..=1.0).contains(&value))
    {
        return Err("motion opacity must be between 0 and 1".to_string());
    }
    Ok(())
}

fn validate_seconds(value: f64, name: &str) -> Result<(), String> {
    if !value.is_finite() || value < 0.0 || Duration::try_from_secs_f64(value).is_err() {
        return Err(format!(
            "motion {name} must be a supported finite non-negative number"
        ));
    }
    Ok(())
}

fn validate_ease(ease: &MotionEase) -> Result<(), String> {
    match ease {
        MotionEase::Name(name)
            if matches!(
                name.as_str(),
                "linear" | "ease" | "easeIn" | "easeOut" | "easeInOut"
            ) => {}
        MotionEase::Name(name) => return Err(format!("unknown motion easing: {name}")),
        MotionEase::CubicBezier([x1, y1, x2, y2]) => {
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

fn seconds(value: f64) -> Duration {
    Duration::try_from_secs_f64(value).expect("motion durations are validated when parsed")
}

fn ease(progress: f64, ease: &MotionEase) -> f64 {
    let curve = match ease {
        MotionEase::CubicBezier(curve) => *curve,
        MotionEase::Name(name) => match name.as_str() {
            "linear" => return progress,
            "easeIn" => [0.42, 0.0, 1.0, 1.0],
            "easeInOut" => [0.42, 0.0, 0.58, 1.0],
            "ease" => [0.25, 0.1, 0.25, 1.0],
            _ => [0.0, 0.0, 0.58, 1.0],
        },
    };
    cubic_bezier(progress, curve)
}

/// Slope of the easing curve at `progress`, by central difference over the
/// unit interval so it stays defined at both ends.
fn ease_slope(progress: f64, curve: &MotionEase) -> f64 {
    const STEP: f64 = 1e-4;
    let low = (progress - STEP).max(0.0);
    let high = (progress + STEP).min(1.0);
    (ease(high, curve) - ease(low, curve)) / (high - low)
}

pub(crate) fn cubic_bezier(x: f64, [x1, y1, x2, y2]: [f64; 4]) -> f64 {
    fn sample(t: f64, a: f64, b: f64) -> f64 {
        let c = 3.0 * a;
        let b = 3.0 * (b - a) - c;
        let a = 1.0 - c - b;
        ((a * t + b) * t + c) * t
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
    sample((low + high) / 2.0, y1, y2).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolates_and_retargets_from_the_visible_value() {
        let started = Instant::now();
        let initial = serde_json::json!({
            "initial": { "width": 0.0 },
            "animate": { "width": 100.0 },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        let mut state = MotionState::new(&initial, started).unwrap();

        let middle = state.frame(started + Duration::from_millis(500));
        assert_eq!(
            state
                .visible_style(started + Duration::from_millis(500))
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
        state.sync(&reversed, reversed_at).unwrap();
        assert_eq!(state.visible_style(reversed_at).unwrap().width, Some(50.0));
        assert_eq!(
            state
                .visible_style(reversed_at + Duration::from_millis(500))
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
        let mut state = MotionState::new(&description, now).unwrap();
        let frame = state.frame(now);

        assert_eq!(state.visible_style(now).unwrap().width, Some(260.0));
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
            assert!(MotionState::new(&description, now).is_err());
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
        let mut state = MotionState::new(&description, started).unwrap();
        let frame = state.frame(started + Duration::from_millis(200));

        assert_eq!(
            state
                .visible_style(started + Duration::from_millis(200))
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
        let mut state = MotionState::new(&initial, started).unwrap();
        assert!(!state.frame(started).just_settled);

        let exit = serde_json::json!({
            "initial": false,
            "animate": { "opacity": 0.0 },
            "transition": { "duration": 0.0, "ease": "linear" }
        });
        state.sync(&exit, started).unwrap();
        let frame = state.frame(started);
        assert_eq!(state.visible_style(started).unwrap().opacity, Some(0.0));
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
        let mut state = MotionState::new(&initial, started).unwrap();

        let exit = serde_json::json!({
            "generation": 2,
            "initial": false,
            "animate": { "opacity": 1.0 }
        });
        state.sync(&exit, started).unwrap();

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
        let mut state = MotionState::new(&initial, started).unwrap();

        let exit = serde_json::json!({
            "generation": 2,
            "initial": false,
            "animate": { "opacity": 0.0 },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        state.sync(&exit, started).unwrap();

        state.frame(started + Duration::from_millis(500));
        let style = state
            .visible_style(started + Duration::from_millis(500))
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
        state.sample(now).position.width.unwrap()
    }

    fn width_velocity(state: &MotionState, now: Instant) -> f64 {
        state.sample(now).velocity.width.unwrap()
    }

    #[test]
    fn critically_damped_spring_converges_without_overshoot() {
        let started = Instant::now();
        let description = spring_description(
            0.0,
            100.0,
            serde_json::json!({ "type": "spring", "stiffness": 100.0, "damping": 20.0 }),
        );
        let mut state = MotionState::new(&description, started).unwrap();

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
            state.visible_style(at(started, 3.0)).unwrap().width,
            Some(100.0)
        );
    }

    #[test]
    fn underdamped_spring_overshoots_then_settles() {
        let started = Instant::now();
        let description = spring_description(0.0, 100.0, serde_json::json!({ "type": "spring" }));
        let mut state = MotionState::new(&description, started).unwrap();

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
        let state = MotionState::new(&description, started).unwrap();

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
            let state = MotionState::new(&description, started).unwrap();
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
            MotionState::new(&spring_description(0.0, 100.0, spring.clone()), started).unwrap();

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
        state.sync(&reversed, retarget_at).unwrap();

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
            MotionState::new(&spring_description(0.0, 100.0, spring.clone()), started).unwrap();
        let retarget_at = at(started, 0.05);
        let velocity = width_velocity(&state, retarget_at);

        // Changing only the transition makes a new segment aimed at the same
        // value; the element keeps its speed rather than freezing.
        let again = serde_json::json!({
            "initial": false,
            "animate": { "width": 100.0 },
            "transition": { "type": "spring", "stiffness": 171.0, "damping": 26.0 }
        });
        state.sync(&again, retarget_at).unwrap();
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
            let state = MotionState::new(&description, started).unwrap();
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
    fn spring_settles_once_and_is_idle_afterwards() {
        let started = Instant::now();
        let description = spring_description(
            0.0,
            100.0,
            serde_json::json!({ "type": "spring", "duration": 0.2, "bounce": 0.0 }),
        );
        let mut state = MotionState::new(&description, started).unwrap();

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
            state.visible_style(at(started, 1.0)).unwrap().width,
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
        let mut state = MotionState::new(&description, started).unwrap();

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
        let state = MotionState::new(&fade_in, started).unwrap();
        let mut overshot = false;
        for step in 1..=200 {
            let now = at(started, step as f64 * 0.005);
            overshot |= state.sample(now).position.opacity.unwrap() > 1.0;
            let visible = state.visible_style(now).unwrap().opacity.unwrap();
            assert!((0.0..=1.0).contains(&visible));
        }
        assert!(overshot);

        let collapse = spring_description(100.0, 0.0, bouncy);
        let state = MotionState::new(&collapse, started).unwrap();
        let mut undershot = false;
        for step in 1..=200 {
            let now = at(started, step as f64 * 0.005);
            undershot |= width(&state, now) < 0.0;
            assert!(state.visible_style(now).unwrap().width.unwrap() >= 0.0);
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
        let mut state = MotionState::new(&tween, started).unwrap();
        let switch_at = at(started, 0.5);
        assert!((width_velocity(&state, switch_at) - 100.0).abs() < 1e-9);

        let spring = serde_json::json!({
            "initial": false,
            "animate": { "width": 200.0 },
            "transition": { "type": "spring", "stiffness": 100.0, "damping": 20.0 }
        });
        state.sync(&spring, switch_at).unwrap();
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
        let mut state = MotionState::new(&spring, started).unwrap();
        let switch_at = at(started, 0.1);
        let position = width(&state, switch_at);

        let tween = serde_json::json!({
            "initial": false,
            "animate": { "width": 0.0 },
            "transition": { "duration": 1.0, "ease": "linear" }
        });
        state.sync(&tween, switch_at).unwrap();
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
                MotionState::new(&description, now).is_err(),
                "{transition} should be rejected"
            );
        }
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
                    delay: None,
                    ease: None,
                    stiffness: None,
                    damping: None,
                    mass: None,
                    rest_speed: None,
                    rest_delta: None,
                };
                Spring::from_raw(&raw).unwrap();
            }
        }
    }
}
