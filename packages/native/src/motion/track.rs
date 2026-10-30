//! One property's animation: keyframes of channel vectors, played once or
//! repeated.

use std::sync::OnceLock;
use std::time::Duration;

use web_time::Instant;

use crate::dormant_wake::Wake;

use super::curve::{
    ease, ease_slope, seconds, Curve, Ease, RepeatCount, RepeatType, Spring, Transition,
};
use super::script::{Pose, Script};
use super::value::{align, align_in, pair, AnimValue, Interpolation, Layout};
use super::PropKey;

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

    pub(super) fn has_auto(&self) -> bool {
        self.frames.contains(&Some(AnimValue::Auto))
    }

    /// How many keyframes a track for this goal has: a single target runs
    /// from the current value.
    pub(super) fn keyframe_count(&self) -> usize {
        self.frames.len().max(2)
    }

    /// Whether Motion's `keyframes[1]` is the number 0: the target of a single goal, else the second keyframe.
    pub(super) fn ends_at_zero(&self) -> bool {
        let second = self.frames.get(1).unwrap_or(&self.frames[0]);
        matches!(second, Some(AnimValue::Number(number)) if *number == 0.0)
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
    /// The animation Motion would hand to the browser for this track, whose clock the track then
    /// follows: `started` is the clock of every other track.
    script: Option<Script>,
    /// Nobody has asked yet whether Motion would make an animation of the track.
    unscreened: bool,
    /// Where the track stands for good once its animation is over: what Motion committed when the animation
    /// finished, or the value it never left when the animation was cancelled.
    held: Option<Vec<f64>>,
    /// The animation whose end `held` is: it finished, or was cancelled.
    ended_by: Option<u64>,
    /// That animation, idle, while JS holds its `Animation`: Chrome plays a cancelled one again, and
    /// the track is driven by it from then on, see [`Self::revive`].
    idle: Option<Script>,
    /// Seconds a repeating spring takes to settle, which is the length of one
    /// of its iterations. Zero for everything else.
    pub(super) settle: f64,
    /// When a spring that plays once comes to rest. Found by a scan, so kept until the keyframes or
    /// the speed it leaves with change.
    rest: OnceLock<Option<Rest>>,
}

/// Seconds after its delay that a spring is at rest.
#[derive(Clone, Copy, Debug)]
struct Rest {
    /// Where Motion's `calcGeneratorDuration` ends the spring: its `duration` when that defines it, else the first
    /// step of a scan every 50 ms that finds it at rest. From there the track shows its target and is no longer active.
    scanned: f64,
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

/// The step of motion-dom's `calcGeneratorDuration`.
const SPRING_SETTLE_STEP: f64 = 0.05;
const NANOSECOND: f64 = 1e-9;
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
        interpolation: Interpolation,
        transition: Transition,
        now: Instant,
    ) -> Result<Self, String> {
        transition.check_keyframes(name, resolved.len())?;
        let values: Vec<&AnimValue> = resolved.iter().collect();
        let aligned = align_in(name, &values, interpolation)?;
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
            script: None,
            unscreened: true,
            held: None,
            ended_by: None,
            idle: None,
            settle: 0.0,
            rest: OnceLock::new(),
        };
        track.settle = track.spring_settle();
        track
    }

    /// Gives the track the animation Motion would make of it, if it would make one.
    pub(super) fn screen(&mut self, key: PropKey, now: Instant) {
        self.unscreened = false;
        self.script = Script::of(key, self, now);
        if self.script.is_some() {
            self.blend_as_css(key);
        }
    }

    /// Motion hands the value to the browser, which blends two legacy colours in sRGB as CSS does
    /// where a value Motion keeps to itself is blended in Oklab.
    fn blend_as_css(&mut self, key: PropKey) {
        if !matches!(self.layout, Layout::Color(_)) {
            return;
        }
        let values: Vec<&AnimValue> = self.literal.iter().collect();
        let aligned = align_in(key.name(), &values, Interpolation::Css)
            .expect("the values of a track align as they did when it was made");
        self.layout = aligned.layout;
    }

    /// The track has not been through [`Self::screen`].
    pub(super) fn is_unscreened(&self) -> bool {
        self.unscreened
    }

    pub(super) fn animation(&self) -> Option<&Script> {
        self.script.as_ref()
    }

    pub(super) fn animation_mut(&mut self) -> Option<&mut Script> {
        self.script.as_mut()
    }

    /// Takes the animation off the track, which stays where it is.
    pub(super) fn take_animation(&mut self) -> Option<Script> {
        self.script.take()
    }

    /// The animation finished and Motion committed the final keyframe, or the first when it ran backwards.
    pub(super) fn finish_animation(&mut self, backwards: bool) -> Option<Script> {
        self.held = Some(self.final_position(backwards));
        let script = self.script.take();
        self.ended_by = script.as_ref().map(Script::id);
        script
    }

    /// The animation was cancelled: Motion never hears of that, so the value it had is the one the track started from.
    pub(super) fn cancel_animation(&mut self) -> Option<Script> {
        self.held = Some(self.frames[0].clone());
        let script = self.script.take();
        self.ended_by = script.as_ref().map(Script::id);
        script
    }

    /// Keeps `script`, which ended this track and was cancelled, for a command to bring back. Not
    /// another one's: a track that was replaced since has its own.
    pub(super) fn park(&mut self, script: Script) {
        if self.ended_by == Some(script.id()) && self.script.is_none() {
            self.idle = Some(script);
        }
    }

    pub(super) fn idle_animation(&self) -> Option<&Script> {
        self.idle.as_ref()
    }

    pub(super) fn idle_animation_mut(&mut self) -> Option<&mut Script> {
        self.idle.as_mut()
    }

    pub(super) fn take_idle(&mut self) -> Option<Script> {
        self.idle.take()
    }

    /// The parked animation plays again: the track shows what it does from here.
    pub(super) fn revive(&mut self, script: Script) {
        self.held = None;
        self.ended_by = None;
        self.script = Some(script);
    }

    /// The rate of change of the first channel per second at the start, which a spring takes from the motion it interrupted.
    pub(super) fn start_velocity(&self) -> f64 {
        self.from_velocity.first().copied().unwrap_or(0.0)
    }

    /// The track goes from one keyframe to another, and does not rest where it starts.
    pub(super) fn moves(&self) -> bool {
        !self.is_static()
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

    /// The same track standing for `goal`: one that rests at the end of keyframes it does not play
    /// must still be told apart from a new target by what was asked for, not by what it shows.
    pub(super) fn for_goal(mut self, goal: Goal) -> Self {
        self.goal = goal;
        self
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
        if !matches!(self.transition.curve, Curve::Spring(_)) {
            return;
        }
        if let Some(rate) = layout.velocity_in(&self.layout, &rate) {
            self.from_velocity = rate;
            self.settle = self.spring_settle();
            self.rest = OnceLock::new();
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
                rest: OnceLock::new(),
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
            rest: OnceLock::new(),
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
        if let Some(position) = &self.held {
            return Sample { position: position.clone(), velocity: vec![0.0; position.len()], active: false };
        }
        let Some(script) = &self.script else {
            return self.sample_at(now.saturating_duration_since(self.started));
        };
        let mut sample = match script.pose(now) {
            Pose::At(elapsed) => self.sample_at(elapsed),
            Pose::Eased(progress) => Sample {
                position: self.keyframes_at(progress.unwrap_or(0.0), false, true).0,
                velocity: vec![0.0; self.frames[0].len()],
                active: false,
            },
        };
        // The animation outlives the value when Motion's length of a spring is the longer.
        sample.active |= script.running(now);
        sample
    }

    /// Whether the track needs a frame to move on: its value is changing, not just waiting for a command.
    pub(super) fn moves_at(&self, now: Instant) -> bool {
        self.sample(now).active && self.script.as_ref().is_none_or(|script| script.wants_frame(now))
    }

    /// [`Self::sample`] at `elapsed` since the track started, for a clock that
    /// is not the wall: one that holds, seeks or runs at a rate.
    pub(super) fn sample_at(&self, elapsed: Duration) -> Sample {
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
        let cycles = released / cycle;
        // An iteration owns its last instant, as in Motion: at exactly n cycles the n-th is still
        // on its final frame, which a loop shows as the end rather than the start.
        let iteration = if cycles >= 1.0 && (cycles - cycles.round()).abs() < 1e-9 {
            cycles.round() - 1.0
        } else {
            cycles.floor()
        };
        // The repeat delay holds the iteration's last frame.
        let local = (released - iteration * cycle).min(duration);
        Progress {
            iteration: iteration as u64,
            fraction: local / duration,
            done: false,
        }
    }

    /// The value `fraction` of the way through the keyframes, easings applied,
    /// independent of the track's own clock. CSS animations own their timeline
    /// and ask the track only how its keyframes interpolate.
    pub(super) fn value_at(&self, fraction: f64) -> AnimValue {
        self.layout.decode(&self.keyframes_at(fraction, false, false).0)
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
        let (position, rate) = self.keyframes_at(along, direction == Direction::Mirrored, false);
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
    ///
    /// `through_effect` leaves out the easing of the transition when it is one for the whole iteration: an
    /// effect that eases the progress itself has applied it.
    fn keyframes_at(&self, along: f64, mirrored: bool, through_effect: bool) -> (Vec<f64>, Vec<f64>) {
        let count = self.frames.len();
        let frame = |index: usize| &self.frames[if mirrored { count - 1 - index } else { index }];
        let zeros = || vec![0.0; frame(0).len()];
        let times = self.transition.keyframe_times(count);
        // An effect's easing can take the progress past either end, which runs on along the first or last segment.
        let overshoots = through_effect
            && self.transition.segment_eases.is_none()
            && ((along < 0.0 && times[0] == 0.0) || (along > 1.0 && times[count - 1] == 1.0));
        if !overshoots && along <= times[0] {
            return (frame(0).clone(), zeros());
        }
        if !overshoots && along >= times[count - 1] {
            return (frame(count - 1).clone(), zeros());
        }
        let segment = if overshoots && along > 1.0 { count - 2 } else { times.partition_point(|time| *time <= along).saturating_sub(1) };
        let span = times[segment + 1] - times[segment];
        let local = (along - times[segment]) / span;
        let linear;
        let easing = if through_effect && self.transition.segment_eases.is_none() {
            linear = Ease::Name("linear".to_string());
            &linear
        } else {
            self.transition.segment_ease(segment)
        };
        let progress = if overshoots { local } else { ease(local.clamp(0.0, 1.0), easing) };
        let progress_rate = ease_slope(local, easing) / span;
        let (from, to) = (frame(segment), frame(segment + 1));
        // The layout knows segments in the keyframes' own order.
        let blended = if mirrored { count - 2 - segment } else { segment };
        let rate = from
            .iter()
            .zip(to)
            .map(|(from, to)| (to - from) * progress_rate)
            .collect();
        (self.layout.blend(blended, from, to, progress), rate)
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
        // Motion ends a spring when its duration is over, whatever the spring reads then: a value at
        // that instant is the target, and so it is for good.
        if !self.has_motion() || self.rest().is_some_and(|rest| t + NANOSECOND >= rest.scanned) {
            return Sample {
                position: to.clone(),
                velocity: vec![0.0; to.len()],
                active: false,
            };
        }
        let evolved = evolve_all(&self.layout, spring, start, to, &self.from_velocity, t);
        // Until then the generator reads the target at an instant it is at rest in, and the spring is running.
        Sample {
            position: if evolved.at_rest { to.clone() } else { evolved.position },
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
        let evolved = evolve_all(&self.layout, spring, start, to, speed, time);
        Sample {
            position: if evolved.at_rest { to.clone() } else { evolved.position },
            velocity: evolved.velocity.iter().map(|rate| rate * sign).collect(),
            active,
        }
    }

    /// Seconds one iteration of a repeating spring lasts: until every channel
    /// is at rest, sampled every 50 ms as motion-dom measures a generator's
    /// duration, and no longer than 20 s.
    fn spring_settle(&self) -> f64 {
        if !self.transition.repeat.repeats() || self.is_static() {
            return 0.0;
        }
        self.settle_time().unwrap_or(SPRING_SETTLE_LIMIT)
    }

    /// Seconds a spring takes to come to rest, found by the scan of
    /// [`Self::spring_settle`]; `None` if it is still moving after 20 s.
    fn settle_time(&self) -> Option<f64> {
        self.rest().map(|rest| rest.scanned)
    }

    fn rest(&self) -> Option<Rest> {
        *self.rest.get_or_init(|| {
            let Curve::Spring(spring) = &self.transition.curve else {
                return None;
            };
            let carried = self.from_velocity.iter().any(|velocity| *velocity != 0.0);
            if let (Some(duration), false) = (spring.time_defined, carried) {
                return Some(Rest { scanned: duration });
            }
            let at_rest = |t: f64| {
                evolve_all(&self.layout, spring, &self.frames[0], &self.frames[1], &self.from_velocity, t).at_rest
            };
            let steps = (SPRING_SETTLE_LIMIT / SPRING_SETTLE_STEP).round() as u32;
            (1..=steps)
                .map(|step| f64::from(step) * SPRING_SETTLE_STEP)
                .find(|t| at_rest(*t))
                .map(|scanned| Rest { scanned })
        })
    }

    /// How long the track plays. A spring that has not come to rest after 20 s
    /// plays for ever, as does a repeat without a count.
    pub(super) fn span(&self) -> Span {
        let iteration = match &self.transition.curve {
            Curve::Tween { duration, .. } => *duration,
            Curve::Spring(_) if !self.has_motion() => 0.0,
            Curve::Spring(_) if self.transition.repeat.repeats() => self.settle,
            Curve::Spring(_) => self.settle_time().unwrap_or(f64::INFINITY),
        };
        let delay = self.transition.delay;
        let repeat = &self.transition.repeat;
        let end = match repeat.count {
            // A zero-length iteration repeated never advances.
            _ if iteration == 0.0 => delay,
            RepeatCount::Finite(count) => {
                let repeats = f64::from(count);
                delay + iteration * (repeats + 1.0) + repeat.delay * repeats
            }
            RepeatCount::Infinite => f64::INFINITY,
        };
        let rests = match &self.transition.curve {
            Curve::Spring(_) if self.has_motion() && !repeat.repeats() => {
                self.rest().map_or(f64::INFINITY, |rest| delay + rest.scanned)
            }
            _ => end,
        };
        Span { iteration, end, rests }
    }

    /// When the track next has to be sampled for anything but its value, which is a function of
    /// the time: the instant it ends, or for a spring that plays once the instant it comes to rest.
    /// A repeat without a count never ends, and a spring that is still moving after the 20 s its
    /// scan covers has no instant to give, so it is sampled in every frame.
    pub(super) fn wake(&self, now: Instant) -> Wake {
        if !self.sample(now).active {
            return Wake::Never;
        }
        let Some(script) = &self.script else {
            return self.timed_wake();
        };
        match self.transition.curve {
            // The animation ends the track's timed span, on a clock of its own, and keeps the track
            // active until it does: a spring under one is done where the animation is, a little after
            // it comes to rest. One that never rests is sampled in every frame.
            Curve::Tween { .. } => script.wake(now),
            Curve::Spring(_) if self.transition.repeat.count == RepeatCount::Infinite => script.wake(now),
            Curve::Spring(_) if !script.wants_frame(now) || self.span().rests.is_finite() => script.wake(now),
            Curve::Spring(_) => Wake::NextFrame,
        }
    }

    fn timed_wake(&self) -> Wake {
        if self.transition.repeat.count == RepeatCount::Infinite {
            return Wake::Never;
        }
        let rests = self.span().rests;
        if rests.is_finite() {
            return Wake::At(self.started + Duration::from_secs_f64(rests));
        }
        match self.transition.curve {
            Curve::Tween { .. } => Wake::Never,
            Curve::Spring(_) => Wake::NextFrame,
        }
    }

    /// The channels the track holds once it has finished: the last keyframe,
    /// or the first when it is run backwards or ends on a reversed iteration.
    pub(super) fn final_position(&self, backwards: bool) -> Vec<f64> {
        let repeat = &self.transition.repeat;
        let ends_reversed = match repeat.count {
            RepeatCount::Finite(count) => count % 2 == 1 && repeat.kind != RepeatType::Loop,
            RepeatCount::Infinite => false,
        };
        let first = backwards || ends_reversed;
        self.frames[if first { 0 } else { self.frames.len() - 1 }].clone()
    }
}

/// How long a track plays, in seconds.
#[derive(Clone, Copy, Debug)]
pub(super) struct Span {
    /// One iteration, without the delay or the repeats.
    pub(super) iteration: f64,
    /// Where the track ends, the delay and the repeats included.
    pub(super) end: f64,
    /// The first instant at which it no longer has anything to report: `end`, or for a spring that
    /// plays once, the instant Motion ends it, which is `end`.
    pub(super) rests: f64,
}

struct Evolved {
    position: Vec<f64>,
    velocity: Vec<f64>,
    at_rest: bool,
}

/// Every channel of a spring that left `start` at `rate` and is pulled to `to`,
/// `t` seconds in.
fn evolve_all(
    layout: &Layout,
    spring: &Spring,
    start: &[f64],
    to: &[f64],
    rate: &[f64],
    t: f64,
) -> Evolved {
    let mut position = Vec::with_capacity(to.len());
    let mut velocity = Vec::with_capacity(to.len());
    // Motion ends a spring given as `duration` and `bounce` when the duration is over, whatever its thresholds.
    // One that carries a speed (Motion drops it for such a spring) must be at rest as well: it may not have settled.
    let (timed, carried) = (
        spring.time_defined.map(|duration| t + NANOSECOND >= duration),
        rate.iter().any(|velocity| *velocity != 0.0),
    );
    let mut at_rest = timed != Some(false);
    for (channel, ((start, to), start_velocity)) in start.iter().zip(to).zip(rate).enumerate() {
        let (offset, speed) = spring.evolve(start - to, *start_velocity, t);
        let (rest_speed, rest_delta) =
            spring.rest_thresholds(start - to, layout.is_percent_channel(channel));
        if timed.is_none() || carried {
            at_rest &= offset.abs() <= rest_delta && speed.abs() <= rest_speed;
        }
        position.push(to + offset);
        velocity.push(speed);
    }
    Evolved {
        position,
        velocity,
        at_rest,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn number(value: f64) -> AnimValue {
        AnimValue::Number(value)
    }

    fn track(transition: serde_json::Value) -> Track {
        Track::new(
            "test",
            &number(0.0),
            number(100.0),
            Transition::parse(transition).unwrap(),
            Instant::now(),
        )
        .unwrap()
    }

    fn at(track: &Track, seconds: f64) -> f64 {
        match track.value(&track.sample_at(Duration::from_secs_f64(seconds)).position) {
            AnimValue::Number(value) => value,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn sampling_at_an_elapsed_time_is_sampling_at_an_instant() {
        let track = track(json!({ "duration": 2, "ease": "linear" }));
        let started = Instant::now();
        let again = Track { started, ..track.clone() };
        assert_eq!(
            again.sample(started + Duration::from_millis(500)).position,
            track.sample_at(Duration::from_millis(500)).position
        );
        assert_eq!(at(&track, 0.5), 25.0);
    }

    #[test]
    fn the_span_counts_the_delay_once_and_the_repeat_delay_between_iterations() {
        let once = track(json!({ "duration": 2, "delay": 0.5 }));
        assert_eq!((once.span().iteration, once.span().end), (2.0, 2.5));
        let repeated = track(json!({ "duration": 2, "delay": 0.5, "repeat": 2, "repeatDelay": 0.25 }));
        assert_eq!(repeated.span().end, 0.5 + 3.0 * 2.0 + 2.0 * 0.25);
        assert_eq!(track(json!({ "duration": 2, "repeat": "Infinity" })).span().end, f64::INFINITY);
        // Nothing to repeat, so it ends when its delay does.
        assert_eq!(track(json!({ "duration": 0, "delay": 1, "repeat": "Infinity" })).span().end, 1.0);
    }

    #[test]
    fn the_end_of_a_track_is_where_it_rests() {
        let track = track(json!({ "duration": 1, "ease": "linear", "repeat": 1, "repeatType": "reverse" }));
        assert_eq!(at(&track, track.span().end), 0.0);
        assert_eq!(track.final_position(false), track.final_position(true));
        let loops = self::track(json!({ "duration": 1, "repeat": 1 }));
        assert_eq!(at(&loops, loops.span().end), 100.0);
        assert_ne!(loops.final_position(false), loops.final_position(true));
    }

    #[test]
    fn a_spring_has_a_span_whether_or_not_it_repeats() {
        let spring = json!({ "type": "spring", "stiffness": 200, "damping": 12 });
        let once = track(spring.clone());
        let mut again = spring;
        again["repeat"] = json!(1);
        let repeated = track(again);
        assert!(once.span().iteration > 0.0 && once.span().iteration.is_finite());
        assert_eq!(once.span().iteration, repeated.span().iteration);
        assert_eq!(repeated.span().end, repeated.span().iteration * 2.0);
    }

    fn spring_track(from: f64, to: f64, transition: serde_json::Value) -> Track {
        Track::new("test", &number(from), number(to), Transition::parse(transition).unwrap(), Instant::now()).unwrap()
    }

    fn reads(track: &Track, seconds: f64) -> (f64, bool) {
        let sample = track.sample_at(Duration::from_secs_f64(seconds));
        match track.value(&sample.position) {
            AnimValue::Number(value) => (value, sample.active),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_spring_given_by_duration_and_bounce_is_the_target_from_its_duration_on() {
        // The scale of a banner: from 0.4 to 1 in 0.24 s. Its thresholds are 0.005 and 0.01, and the spring is within them
        // at 0.278 s and at 0.388 s but not between, so a reader that came at 0.32 s found it at 1.0017 and kept it there.
        let spring = spring_track(0.4, 1.0, json!({ "type": "spring", "duration": 0.24, "bounce": 0.12, "delay": 0.08 }));
        assert_eq!(spring.span().end, 0.08 + 0.24);
        let (before, active) = reads(&spring, 0.08 + 0.2399);
        assert!(active && before != 1.0, "{before}");
        for millis in 320..2000 {
            let (value, active) = reads(&spring, f64::from(millis) / 1000.0);
            assert_eq!((value, active), (1.0, false), "at {millis} ms");
        }
    }

    #[test]
    fn a_spring_given_by_duration_does_not_snap_to_the_target_before_its_duration() {
        // 0.0011 short of the target at 0.2 s: within the thresholds of 0.005, and still a spring in motion.
        let spring = spring_track(0.4, 1.0, json!({ "type": "spring", "duration": 0.24, "bounce": 0.12 }));
        let (value, active) = reads(&spring, 0.2);
        assert!(active && (value - 1.0).abs() > 1e-6 && (value - 1.0).abs() < 0.005, "{value}");
        let given = spring_track(
            0.4,
            1.0,
            json!({ "type": "spring", "duration": 0.24, "bounce": 0.12, "restDelta": 0.0005, "restSpeed": 0.0005 }),
        );
        assert_eq!(given.span().end, spring.span().end, "the thresholds of a duration spring are not read");
    }

    #[test]
    fn a_spring_of_physics_ends_at_the_first_multiple_of_50_ms_it_is_at_rest_at() {
        let spring = spring_track(0.0, 100.0, json!({ "type": "spring", "stiffness": 100, "damping": 10 }));
        let end = spring.span().end;
        assert!((end / 0.05 - (end / 0.05).round()).abs() < 1e-9, "{end}");
        // Motion's oracle: 1.05 s.
        assert!((end - 1.05).abs() < 1e-9, "{end}");
        assert!(reads(&spring, end - 0.001).1);
        for step in 0..100 {
            assert_eq!(reads(&spring, end + f64::from(step) * 0.01), (100.0, false));
        }
    }

    #[test]
    fn a_duration_spring_that_carries_a_speed_is_done_once_it_is_also_within_its_thresholds() {
        let mut spring = spring_track(0.0, 100.0, json!({ "type": "spring", "duration": 0.3, "bounce": 0.3 }));
        let layout = spring.layout().clone();
        spring.carry_rate(&layout, vec![3000.0]);
        let end = spring.span().end;
        assert!(end >= 0.3 && (end / 0.05 - (end / 0.05).round()).abs() < 1e-9, "{end}");
        assert_eq!(reads(&spring, end), (100.0, false));
    }
}
