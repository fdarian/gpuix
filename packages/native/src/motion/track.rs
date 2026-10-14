//! One property's animation: a segment from a start to an end channel vector.

use std::time::Duration;

use web_time::Instant;

use super::curve::{ease, ease_slope, seconds, Curve, Ease, Spring, Transition};
use super::value::{pair, AnimValue, Layout};

#[derive(Clone, Debug)]
pub(super) struct Track {
    layout: Layout,
    from: Vec<f64>,
    /// Rate of change of `from` per second. Only springs read it; a retarget
    /// hands the interrupted motion's speed to the next segment.
    from_velocity: Vec<f64>,
    to: Vec<f64>,
    target: AnimValue,
    transition: Transition,
    started: Instant,
}

pub(super) struct Sample {
    pub(super) position: Vec<f64>,
    pub(super) velocity: Vec<f64>,
    pub(super) active: bool,
}

impl Track {
    pub(super) fn new(
        name: &str,
        from: &AnimValue,
        target: AnimValue,
        transition: Transition,
        now: Instant,
    ) -> Result<Self, String> {
        let pair = pair(name, from, &target)?;
        Ok(Self {
            from_velocity: vec![0.0; pair.from.len()],
            layout: pair.layout,
            from: pair.from,
            to: pair.to,
            target,
            transition,
            started: now,
        })
    }

    pub(super) fn target(&self) -> &AnimValue {
        &self.target
    }

    pub(super) fn transition(&self) -> &Transition {
        &self.transition
    }

    /// Restarts toward `target` from wherever this track is now. A spring
    /// continues from the current speed as well, so interrupting it bends the
    /// path instead of stopping and restarting. An interrupted tween hands over
    /// its instantaneous speed too, so a tween-to-spring switch does not stall;
    /// a spring-to-tween switch drops the speed, as the tween has no notion of
    /// it.
    pub(super) fn retarget(
        &self,
        name: &str,
        target: AnimValue,
        transition: Transition,
        now: Instant,
    ) -> Result<Self, String> {
        let current = self.sample(now);
        let from = self.layout.decode(&current.position);
        let mut next = Self::new(name, &from, target, transition, now)?;
        let is_spring = matches!(next.transition.curve, Curve::Spring(_));
        if is_spring && self.layout.carries_velocity_to(&next.layout) {
            next.from_velocity = current.velocity;
        }
        Ok(next)
    }

    /// Whether the segment starts away from rest, so a frame must run even
    /// when the start already equals the target.
    pub(super) fn has_motion(&self) -> bool {
        self.from != self.to
            || (matches!(self.transition.curve, Curve::Spring(_))
                && self.from_velocity.iter().any(|velocity| *velocity != 0.0))
    }

    pub(super) fn value(&self, channels: &[f64]) -> AnimValue {
        self.layout.decode(channels)
    }

    pub(super) fn sample(&self, now: Instant) -> Sample {
        let elapsed = now.saturating_duration_since(self.started);
        let delay = seconds(self.transition.delay);
        match &self.transition.curve {
            Curve::Tween { duration, ease } => {
                self.sample_tween(elapsed, delay, seconds(*duration), ease)
            }
            Curve::Spring(spring) => self.sample_spring(elapsed, delay, spring),
        }
    }

    fn sample_tween(
        &self,
        elapsed: Duration,
        delay: Duration,
        duration: Duration,
        curve: &Ease,
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
        let active = self.from != self.to && raw < 1.0;
        if raw >= 1.0 {
            return Sample {
                position: self.to.clone(),
                velocity: vec![0.0; self.to.len()],
                active,
            };
        }
        let progress = ease(raw.clamp(0.0, 1.0), curve);
        let progress_rate = if active && raw > 0.0 {
            ease_slope(raw, curve) / duration.as_secs_f64()
        } else {
            0.0
        };
        let (position, velocity) = self
            .from
            .iter()
            .zip(&self.to)
            .map(|(from, to)| (from + (to - from) * progress, (to - from) * progress_rate))
            .unzip();
        Sample {
            position,
            velocity,
            active,
        }
    }

    fn sample_spring(&self, elapsed: Duration, delay: Duration, spring: &Spring) -> Sample {
        // The delay holds the segment's start: the carried speed only applies
        // once the spring is released.
        let released = elapsed >= delay;
        let t = elapsed.saturating_sub(delay).as_secs_f64();

        let mut position = Vec::with_capacity(self.to.len());
        let mut velocity = Vec::with_capacity(self.to.len());
        let mut at_rest = true;
        for ((start, to), start_velocity) in self.from.iter().zip(&self.to).zip(&self.from_velocity)
        {
            if !released {
                position.push(*start);
                velocity.push(0.0);
                at_rest &= start == to && *start_velocity == 0.0;
                continue;
            }
            let (offset, rate) = spring.evolve(start - to, *start_velocity, t);
            let (rest_speed, rest_delta) = spring.rest_thresholds(start - to);
            at_rest &= offset.abs() <= rest_delta && rate.abs() <= rest_speed;
            position.push(to + offset);
            velocity.push(rate);
        }

        if at_rest {
            return Sample {
                position: self.to.clone(),
                velocity: vec![0.0; self.to.len()],
                active: false,
            };
        }
        Sample {
            position,
            velocity,
            active: true,
        }
    }
}
