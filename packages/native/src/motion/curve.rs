//! Tween easing and spring physics shared by every animated property.

use std::time::Duration;

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub(super) enum Ease {
    Name(String),
    CubicBezier([f64; 4]),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Transition {
    pub(super) delay: f64,
    pub(super) curve: Curve,
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
        }
    }
}

fn default_duration() -> f64 {
    0.3
}

fn default_ease() -> Ease {
    Ease::Name("easeOut".to_string())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RawTransition {
    #[serde(rename = "type")]
    pub(super) kind: Option<String>,
    pub(super) duration: Option<f64>,
    pub(super) delay: Option<f64>,
    pub(super) ease: Option<Ease>,
    pub(super) stiffness: Option<f64>,
    pub(super) damping: Option<f64>,
    pub(super) mass: Option<f64>,
    pub(super) bounce: Option<f64>,
    pub(super) rest_speed: Option<f64>,
    pub(super) rest_delta: Option<f64>,
}

impl Transition {
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
                let ease = raw.ease.clone().unwrap_or_else(default_ease);
                validate_seconds(duration, "duration")?;
                validate_ease(&ease)?;
                Curve::Tween { duration, ease }
            }
            Some("spring") => {
                if raw.ease.is_some() {
                    return Err("motion spring transitions do not accept ease".to_string());
                }
                Curve::Spring(Spring::from_raw(&raw)?)
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
pub(super) struct Spring {
    pub(super) stiffness: f64,
    pub(super) damping: f64,
    pub(super) mass: f64,
    rest_speed: Option<f64>,
    rest_delta: Option<f64>,
}

impl Spring {
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
    pub(super) fn rest_thresholds(&self, distance: f64) -> (f64, f64) {
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

fn validate_ease(ease: &Ease) -> Result<(), String> {
    match ease {
        Ease::Name(name)
            if matches!(
                name.as_str(),
                "linear" | "ease" | "easeIn" | "easeOut" | "easeInOut"
            ) => {}
        Ease::Name(name) => return Err(format!("unknown motion easing: {name}")),
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

/// Slope of the easing curve at `progress`, by central difference over the
/// unit interval so it stays defined at both ends.
pub(super) fn ease_slope(progress: f64, curve: &Ease) -> f64 {
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
