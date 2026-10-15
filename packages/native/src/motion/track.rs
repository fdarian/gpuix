//! One property's animation: keyframes of channel vectors, played once or
//! repeated.

use std::time::Duration;

use web_time::Instant;

use super::curve::{
    ease, ease_slope, seconds, Curve, RepeatCount, RepeatType, Spring, Transition,
};
use super::value::{align, pair, AnimValue, Layout};

/// What a description asks one property to animate to: a single target, or
/// keyframes. A `None` keyframe repeats the one before it, and a leading `None`
/// stands for the value the property has when the animation starts.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Goal {
    frames: Vec<Option<AnimValue>>,
}

impl Goal {
    pub(super) fn single(target: AnimValue) -> Self {
        Self {
            frames: vec![Some(target)],
        }
    }

    pub(super) fn keyframes(name: &str, frames: Vec<Option<AnimValue>>) -> Result<Self, String> {
        if frames.is_empty() {
            return Err(format!("motion {name} keyframes need at least one value"));
        }
        if frames.iter().all(Option::is_none) {
            return Err(format!("motion {name} keyframes need at least one value besides null"));
        }
        Ok(Self { frames })
    }

    /// One target and nothing else: no keyframes to fill in.
    pub(super) fn is_single(&self) -> bool {
        self.frames.len() == 1
    }

    pub(super) fn frames(&self) -> &[Option<AnimValue>] {
        &self.frames
    }

    /// How many keyframes a track for this goal has: a single target runs
    /// from the current value.
    pub(super) fn keyframe_count(&self) -> usize {
        self.frames.len().max(2)
    }

    /// The target of a single goal, or the last keyframe.
    pub(super) fn last(&self) -> &AnimValue {
        self.frames
            .iter()
            .rev()
            .find_map(Option::as_ref)
            .expect("a goal holds a value")
    }
}

#[derive(Clone, Debug)]
pub(super) struct Track {
    layout: Layout,
    /// Keyframes as channel vectors, at least two. `frames[0]` is where the
    /// track starts.
    frames: Vec<Vec<f64>>,
    /// Rate of change of the first keyframe per second. Only springs read it; a
    /// retarget hands the interrupted motion's speed to the next segment.
    from_velocity: Vec<f64>,
    goal: Goal,
    /// The keyframes the style asked for, with wildcards filled in. `frames`
    /// differs from it where a value stands in for `auto` or a percentage.
    literal: Vec<AnimValue>,
    /// `frames` are numbers measured from layout standing in for `literal`,
    /// which the style goes back to once the track ends: `height: auto`, or a
    /// percentage that has to run through px against a px start.
    measured: bool,
    transition: Transition,
    started: Instant,
    /// Seconds a repeating spring takes to settle, which is the length of one
    /// of its iterations. Zero for everything else.
    pub(super) settle: f64,
}

pub(super) struct Sample {
    pub(super) position: Vec<f64>,
    pub(super) velocity: Vec<f64>,
    pub(super) active: bool,
}

/// How far a track has got through its iterations.
struct Progress {
    /// Zero-based count of the iteration playing.
    iteration: u64,
    /// How far through that iteration, from 0 to 1.
    fraction: f64,
    done: bool,
}

impl Progress {
    /// Whether the iteration plays against the keyframes' order or time.
    fn direction(&self, kind: RepeatType) -> Direction {
        match (self.iteration % 2 == 1, kind) {
            (true, RepeatType::Reverse) => Direction::Reversed,
            (true, RepeatType::Mirror) => Direction::Mirrored,
            _ => Direction::Forward,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Direction {
    Forward,
    /// Time runs backwards, so easing does too.
    Reversed,
    /// The keyframes are in reverse order; time and easing run forwards.
    Mirrored,
}

const SPRING_SETTLE_STEP: f64 = 1.0 / 60.0;
const SPRING_SETTLE_LIMIT: f64 = 20.0;

impl Track {
    pub(super) fn new(
        name: &str,
        from: &AnimValue,
        target: AnimValue,
        transition: Transition,
        now: Instant,
    ) -> Result<Self, String> {
        Self::resolved(name, from, target.clone(), target, transition, now)
    }

    /// A segment that runs to `to` while the style asked for `target`: they
    /// differ for `height: auto` and for a percentage reached from px, which
    /// run to the measured px.
    pub(super) fn resolved(
        name: &str,
        from: &AnimValue,
        target: AnimValue,
        to: AnimValue,
        transition: Transition,
        now: Instant,
    ) -> Result<Self, String> {
        transition.check_keyframes(name, 2)?;
        let aligned = align(name, &[from, &to])?;
        let measured = target != to;
        Ok(Self::build(
            aligned.layout,
            aligned.frames,
            Goal::single(target.clone()),
            vec![from.clone(), target],
            measured,
            transition,
            now,
        ))
    }

    /// A track through `resolved`, which stands in for the `literal` keyframes
    /// the style asked for where they need layout to become numbers.
    pub(super) fn keyframed(
        name: &str,
        goal: Goal,
        literal: Vec<AnimValue>,
        resolved: &[AnimValue],
        transition: Transition,
        now: Instant,
    ) -> Result<Self, String> {
        transition.check_keyframes(name, resolved.len())?;
        let values: Vec<&AnimValue> = resolved.iter().collect();
        let aligned = align(name, &values)?;
        let measured = if resolved.len() == 2 && !transition.repeat.repeats() {
            literal.last() != resolved.last()
        } else {
            literal != resolved
        };
        Ok(Self::build(
            aligned.layout,
            aligned.frames,
            goal,
            literal,
            measured,
            transition,
            now,
        ))
    }

    fn build(
        layout: Layout,
        frames: Vec<Vec<f64>>,
        goal: Goal,
        literal: Vec<AnimValue>,
        measured: bool,
        transition: Transition,
        now: Instant,
    ) -> Self {
        let mut track = Self {
            from_velocity: vec![0.0; frames[0].len()],
            layout,
            frames,
            goal,
            literal,
            measured,
            transition,
            started: now,
            settle: 0.0,
        };
        track.settle = track.spring_settle();
        track
    }

    /// A `height: auto` that was never anything else, so there is nothing to
    /// animate.
    pub(super) fn resting_at_auto(transition: Transition, now: Instant) -> Self {
        Self::build(
            Layout::Number,
            vec![vec![0.0], vec![0.0]],
            Goal::single(AnimValue::Auto),
            vec![AnimValue::Auto, AnimValue::Auto],
            true,
            transition,
            now,
        )
    }

    pub(super) fn goal(&self) -> &Goal {
        &self.goal
    }

    /// What the style has once the track ends: the last keyframe it asked for.
    pub(super) fn target(&self) -> &AnimValue {
        self.literal.last().expect("a track has keyframes")
    }

    /// The keyframes the style asked for.
    pub(super) fn literal(&self) -> &[AnimValue] {
        &self.literal
    }

    pub(super) fn transition(&self) -> &Transition {
        &self.transition
    }

    pub(super) fn is_auto(&self) -> bool {
        *self.target() == AnimValue::Auto
    }

    /// Whether the end follows a measurement: layout's numbers move under it
    /// as the content and the parent change.
    pub(super) fn is_measured(&self) -> bool {
        self.measured
    }

    /// Whether following a new measurement restarts the segment from where it
    /// is (two keyframes, played once) or edits the keyframes in place.
    pub(super) fn restarts_on_follow(&self) -> bool {
        self.frames.len() == 2 && !self.transition.repeat.repeats()
    }

    /// Whether the style has gone back to literal `auto`, which is the layout's
    /// height again and not the number the segment ended on.
    pub(super) fn rests_at_auto(&self, now: Instant) -> bool {
        self.is_auto() && !self.sample(now).active
    }

    /// The value the style has at `now`. Once a measured segment ends that is
    /// its literal target, so a later animation starts from the unit the style
    /// is in.
    pub(super) fn visible(&self, now: Instant) -> AnimValue {
        let sample = self.sample(now);
        if self.measured && !sample.active {
            return self.target().clone();
        }
        self.layout.decode(&sample.position)
    }

    /// The value the track shows at `now`.
    pub(super) fn current(&self, now: Instant) -> AnimValue {
        self.layout.decode(&self.sample(now).position)
    }

    /// Hands the speed `previous` had at `now` to this segment. Only a spring
    /// reads it: interrupting one bends the path instead of stopping and
    /// restarting. An interrupted tween hands over its instantaneous speed
    /// too, so a tween-to-spring switch does not stall; a spring-to-tween
    /// switch drops it, as the tween has no notion of it.
    pub(super) fn carry_velocity(&mut self, previous: &Self, now: Instant) {
        self.carry_rate(&previous.layout, previous.sample(now).velocity);
    }

    /// [`Self::carry_velocity`] for a speed that did not come from a track,
    /// such as a value being dragged by `set`, given in the channels of `layout`.
    pub(super) fn carry_rate(&mut self, layout: &Layout, rate: Vec<f64>) {
        let is_spring = matches!(self.transition.curve, Curve::Spring(_));
        if is_spring && layout.carries_velocity_to(&self.layout) {
            self.from_velocity = rate;
            self.settle = self.spring_settle();
        }
    }

    pub(super) fn layout(&self) -> &Layout {
        &self.layout
    }

    /// Moves the end of a two-keyframe segment to a new measurement. While the
    /// delay still holds the start nothing has moved, so the end changes in
    /// place; after that the segment restarts from where it is, with its speed.
    /// `None` when the end is already `to`.
    pub(super) fn follow(
        &self,
        name: &str,
        to: AnimValue,
        now: Instant,
    ) -> Result<Option<Self>, String> {
        debug_assert!(self.restarts_on_follow());
        let start = self.layout.decode(&self.frames[0]);
        let moved = pair(name, &start, &to)?;
        if moved.to == self.frames[1] {
            return Ok(None);
        }
        let elapsed = now.saturating_duration_since(self.started);
        if elapsed < seconds(self.transition.delay) {
            return Ok(Some(Self {
                frames: vec![self.frames[0].clone(), moved.to],
                ..self.clone()
            }));
        }
        let mut next = Self::resolved(
            name,
            &self.current(now),
            self.target().clone(),
            to,
            self.transition.clone(),
            now,
        )?;
        next.goal = self.goal.clone();
        next.carry_velocity(self, now);
        Ok(Some(next))
    }

    /// Moves a measured track's keyframes to a new measurement without
    /// disturbing its timing: the animation under way has no single segment to
    /// restart. `None` when nothing moved.
    pub(super) fn follow_keyframes(
        &self,
        name: &str,
        resolved: &[AnimValue],
    ) -> Result<Option<Self>, String> {
        let values: Vec<&AnimValue> = resolved.iter().collect();
        let aligned = align(name, &values)?;
        if aligned.frames == self.frames {
            return Ok(None);
        }
        let mut next = Self {
            frames: aligned.frames,
            ..self.clone()
        };
        next.settle = next.spring_settle();
        Ok(Some(next))
    }

    /// Whether the track starts away from rest, so a frame must run even when
    /// the start already equals the target.
    pub(super) fn has_motion(&self) -> bool {
        !self.is_static()
            || (matches!(self.transition.curve, Curve::Spring(_))
                && self.from_velocity.iter().any(|velocity| *velocity != 0.0))
    }

    /// Every keyframe is the same, so there is nothing to move through.
    fn is_static(&self) -> bool {
        self.frames.iter().all(|frame| *frame == self.frames[0])
    }

    pub(super) fn value(&self, channels: &[f64]) -> AnimValue {
        self.layout.decode(channels)
    }

    pub(super) fn sample(&self, now: Instant) -> Sample {
        let elapsed = now.saturating_duration_since(self.started);
        let delay = seconds(self.transition.delay);
        match &self.transition.curve {
            Curve::Tween { duration, .. } => {
                self.sample_tween(elapsed, delay, seconds(*duration).as_secs_f64())
            }
            Curve::Spring(spring) if self.transition.repeat.repeats() => {
                self.sample_spring_iterations(elapsed, delay, spring)
            }
            Curve::Spring(spring) => self.sample_spring(elapsed, delay, spring),
        }
    }

    /// Where `elapsed` falls among iterations of `duration` seconds that start
    /// after `delay`. The delay comes once, before the first iteration.
    fn progress(&self, elapsed: Duration, delay: Duration, duration: f64) -> Progress {
        let repeat = &self.transition.repeat;
        let released = elapsed.saturating_sub(delay).as_secs_f64();
        // A zero-length iteration repeated would never advance.
        if duration == 0.0 || !repeat.repeats() {
            let raw = if duration == 0.0 {
                if elapsed < delay {
                    0.0
                } else {
                    1.0
                }
            } else if elapsed <= delay {
                0.0
            } else {
                released / duration
            };
            return Progress {
                iteration: 0,
                fraction: raw.min(1.0),
                done: raw >= 1.0,
            };
        }
        let cycle = duration + repeat.delay;
        if let RepeatCount::Finite(count) = repeat.count {
            let iterations = f64::from(count) + 1.0;
            if released >= iterations * cycle - repeat.delay {
                return Progress {
                    iteration: u64::from(count),
                    fraction: 1.0,
                    done: true,
                };
            }
        }
        let iteration = (released / cycle).floor();
        // The repeat delay holds the iteration's last frame.
        let local = (released - iteration * cycle).min(duration);
        Progress {
            iteration: iteration as u64,
            fraction: local / duration,
            done: false,
        }
    }

    fn sample_tween(&self, elapsed: Duration, delay: Duration, duration: f64) -> Sample {
        let progress = self.progress(elapsed, delay, duration);
        let active = !self.is_static() && !progress.done;
        let direction = progress.direction(self.transition.repeat.kind);
        let along = if direction == Direction::Reversed {
            1.0 - progress.fraction
        } else {
            progress.fraction
        };
        let (position, rate) = self.keyframes_at(along, direction == Direction::Mirrored);
        if progress.done {
            return Sample {
                position,
                velocity: vec![0.0; self.frames[0].len()],
                active,
            };
        }
        let sign = if direction == Direction::Reversed { -1.0 } else { 1.0 };
        let velocity = if active && progress.fraction > 0.0 {
            rate.iter().map(|rate| rate * sign / duration).collect()
        } else {
            vec![0.0; self.frames[0].len()]
        };
        Sample {
            position,
            velocity,
            active,
        }
    }

    /// The channels `along` of the way through one iteration, and how fast
    /// they change per unit of `along`.
    fn keyframes_at(&self, along: f64, mirrored: bool) -> (Vec<f64>, Vec<f64>) {
        let count = self.frames.len();
        let frame = |index: usize| &self.frames[if mirrored { count - 1 - index } else { index }];
        let zeros = || vec![0.0; frame(0).len()];
        let times = self.transition.keyframe_times(count);
        if along <= times[0] {
            return (frame(0).clone(), zeros());
        }
        if along >= times[count - 1] {
            return (frame(count - 1).clone(), zeros());
        }
        let segment = times.partition_point(|time| *time <= along) - 1;
        let span = times[segment + 1] - times[segment];
        let local = (along - times[segment]) / span;
        let easing = self.transition.segment_ease(segment);
        let progress = ease(local.clamp(0.0, 1.0), easing);
        let progress_rate = ease_slope(local, easing) / span;
        frame(segment)
            .iter()
            .zip(frame(segment + 1))
            .map(|(from, to)| (from + (to - from) * progress, (to - from) * progress_rate))
            .unzip()
    }

    fn sample_spring(&self, elapsed: Duration, delay: Duration, spring: &Spring) -> Sample {
        // The delay holds the segment's start: the carried speed only applies
        // once the spring is released.
        let released = elapsed >= delay;
        let t = elapsed.saturating_sub(delay).as_secs_f64();
        let (start, to) = (&self.frames[0], &self.frames[1]);

        if !released {
            let at_rest = start == to && self.from_velocity.iter().all(|velocity| *velocity == 0.0);
            return Sample {
                position: start.clone(),
                velocity: vec![0.0; to.len()],
                active: !at_rest,
            };
        }
        let evolved = evolve_all(spring, start, to, &self.from_velocity, t);
        if evolved.at_rest {
            return Sample {
                position: to.clone(),
                velocity: vec![0.0; to.len()],
                active: false,
            };
        }
        Sample {
            position: evolved.position,
            velocity: evolved.velocity,
            active: true,
        }
    }

    /// A spring played over and over: each iteration lasts as long as the
    /// spring takes to settle, and only the first carries the incoming speed.
    fn sample_spring_iterations(
        &self,
        elapsed: Duration,
        delay: Duration,
        spring: &Spring,
    ) -> Sample {
        let progress = self.progress(elapsed, delay, self.settle);
        let direction = progress.direction(self.transition.repeat.kind);
        let (first, last) = (&self.frames[0], &self.frames[1]);
        let channels = first.len();
        let active = !self.is_static() && !progress.done;
        if progress.done || elapsed < delay {
            let at = match direction {
                Direction::Forward => last,
                Direction::Reversed | Direction::Mirrored => first,
            };
            return Sample {
                position: if elapsed < delay { first.clone() } else { at.clone() },
                velocity: vec![0.0; channels],
                active,
            };
        }

        let local = progress.fraction * self.settle;
        let still = vec![0.0; channels];
        let (start, to, time, speed, sign) = match direction {
            Direction::Forward => {
                let speed = if progress.iteration == 0 { &self.from_velocity } else { &still };
                (first, last, local, speed, 1.0)
            }
            Direction::Reversed => (first, last, self.settle - local, &still, -1.0),
            Direction::Mirrored => (last, first, local, &still, 1.0),
        };
        let evolved = evolve_all(spring, start, to, speed, time);
        Sample {
            position: evolved.position,
            velocity: evolved.velocity.iter().map(|rate| rate * sign).collect(),
            active,
        }
    }

    /// Seconds one iteration of a repeating spring lasts: until every channel
    /// is at rest, sampled at 60 Hz as motion-dom measures a generator's
    /// duration, and no longer than 20 s.
    fn spring_settle(&self) -> f64 {
        let Curve::Spring(spring) = &self.transition.curve else {
            return 0.0;
        };
        if !self.transition.repeat.repeats() || self.is_static() {
            return 0.0;
        }
        let steps = (SPRING_SETTLE_LIMIT / SPRING_SETTLE_STEP) as u32;
        (1..=steps)
            .map(|step| f64::from(step) * SPRING_SETTLE_STEP)
            .find(|t| {
                evolve_all(spring, &self.frames[0], &self.frames[1], &self.from_velocity, *t).at_rest
            })
            .unwrap_or(SPRING_SETTLE_LIMIT)
    }
}

struct Evolved {
    position: Vec<f64>,
    velocity: Vec<f64>,
    at_rest: bool,
}

/// Every channel of a spring that left `start` at `rate` and is pulled to `to`,
/// `t` seconds in.
fn evolve_all(spring: &Spring, start: &[f64], to: &[f64], rate: &[f64], t: f64) -> Evolved {
    let mut position = Vec::with_capacity(to.len());
    let mut velocity = Vec::with_capacity(to.len());
    let mut at_rest = true;
    for ((start, to), start_velocity) in start.iter().zip(to).zip(rate) {
        let (offset, speed) = spring.evolve(start - to, *start_velocity, t);
        let (rest_speed, rest_delta) = spring.rest_thresholds(start - to);
        at_rest &= offset.abs() <= rest_delta && speed.abs() <= rest_speed;
        position.push(to + offset);
        velocity.push(speed);
    }
    Evolved {
        position,
        velocity,
        at_rest,
    }
}
