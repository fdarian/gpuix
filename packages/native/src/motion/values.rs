//! Motion values: a graph of nodes sampled at one clock instant.
//!
//! A source holds a value that is written (`set`, `jump`) or animated. A
//! transform maps another value through input and output ranges. A spring
//! follower chases another value, or its own target, with a spring. Nothing
//! ticks: a node is sampled at the instant the caller asks and samples the
//! nodes it reads at that same instant, so a derived value is always computed
//! from inputs of the same moment.
//!
//! Sampling moves forward. A follower retargets when it is sampled and finds
//! its input changed, as Framer's `useSpring` retargets on a frame, so its path
//! depends on the instants it is sampled at. Sample it at the frame clock
//! rather than at wall time, and sampling an instant twice changes nothing.
//!
//! A follower nobody samples would read its start value and retarget only at
//! the instant of the read. So every read and write first replays the frames
//! the followers missed, one per [`FRAME`], which gives the path a frame loop
//! would have taken.

use std::collections::HashMap;
use std::time::Duration;

use web_time::Instant;

use super::curve::{ease, ease_slope, validate_ease, Curve, Ease, Transition};
use super::track::{Goal, Track};
use super::value::{pair, AnimValue, Interpolation, Layout, Pair};

const NAME: &str = "motion value";

/// A velocity read from a value written this recently is current; older ones
/// are stale and read as zero. The gap between the two writes it was measured
/// over is capped to the same length, as motion-dom's `getVelocity` does, so a
/// pause between two writes does not read as a slow drag.
const MAX_VELOCITY_WINDOW: Duration = Duration::from_millis(30);

/// The step a follower that was not sampled catches up in.
const FRAME: Duration = Duration::from_millis(16);

/// Longest stretch replayed. A follower idle for longer retargets at the start
/// of this window, by when a spring has long settled.
const MAX_CATCH_UP_FRAMES: u32 = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct ValueId(pub(super) u32);

/// What a node shows at an instant.
#[derive(Clone, Debug)]
pub(super) struct Sample {
    pub(super) value: AnimValue,
    /// Rate of change per second, one entry per channel of the value. A
    /// number has one; colours and shadows have their interpolation channels.
    pub(super) velocity: Vec<f64>,
    /// Whether the value is still moving by itself at this instant.
    pub(super) active: bool,
}

#[derive(Clone, Debug)]
pub(super) struct TransformOptions {
    /// Hold the output at its first and last entries outside the input range
    /// instead of extending the first and last segments. Framer's default.
    pub(super) clamp: bool,
    /// One easing for every segment, or one per segment. Empty is linear.
    pub(super) ease: Vec<Ease>,
}

impl Default for TransformOptions {
    fn default() -> Self {
        Self {
            clamp: true,
            ease: Vec::new(),
        }
    }
}

/// Ids are chosen by the caller, so a value can exist on the JS side before it
/// is registered here, and the two sides agree on it without a round trip.
#[derive(Default)]
pub(super) struct MotionValues {
    nodes: HashMap<ValueId, Node>,
    /// The latest instant `catch_up` ran to. Every sample of a frame asks again
    /// for the same instant, and finding that there is nothing to do costs a
    /// scan of every node, so without this a frame costs values times nodes.
    /// Adding or removing a node forgets it.
    caught_up: Option<Instant>,
}

impl MotionValues {
    pub(super) fn source(&mut self, id: ValueId, initial: AnimValue) -> Result<(), String> {
        self.vacant(id)?;
        validate(&initial)?;
        self.insert(id, Node::Source(Source::new(initial)))
    }

    /// A value computed from a number-valued `input`. `input_range` is
    /// ascending, or descending as a whole, as Framer accepts.
    pub(super) fn transform(
        &mut self,
        id: ValueId,
        input: ValueId,
        input_range: Vec<f64>,
        output_range: Vec<AnimValue>,
        options: TransformOptions,
    ) -> Result<(), String> {
        self.vacant(id)?;
        self.node(input)?;
        let transform = Transform::new(input, input_range, output_range, options)?;
        self.insert(id, Node::Transform(transform))
    }

    /// A spring that chases `source`, starting at its value at `now`.
    pub(super) fn follow(
        &mut self,
        id: ValueId,
        source: ValueId,
        spring: Transition,
        now: Instant,
    ) -> Result<(), String> {
        self.vacant(id)?;
        require_spring(&spring)?;
        let start = self.sample(source, now)?.value;
        let follower = Follower::new(Some(source), start, spring, now)?;
        self.insert(id, Node::Spring(follower))
    }

    /// A spring with no source: `set` gives it a target and it springs there.
    pub(super) fn spring(
        &mut self,
        id: ValueId,
        initial: AnimValue,
        spring: Transition,
        now: Instant,
    ) -> Result<(), String> {
        self.vacant(id)?;
        require_spring(&spring)?;
        validate(&initial)?;
        let follower = Follower::new(None, initial, spring, now)?;
        self.insert(id, Node::Spring(follower))
    }

    /// Removes a node. A node that reads it fails to sample from then on.
    pub(super) fn remove(&mut self, id: ValueId) -> bool {
        self.caught_up = None;
        self.nodes.remove(&id).is_some()
    }

    pub(super) fn sample(&mut self, id: ValueId, now: Instant) -> Result<Sample, String> {
        self.catch_up(now);
        self.sample_now(id, now)
    }

    /// Samples the frames in `(synced_at, now)` that no one sampled, so a
    /// follower sees its source change at the frame after the change and not
    /// at the read. A follower that cannot be sampled is skipped: it reports
    /// its own error when it is read.
    fn catch_up(&mut self, now: Instant) {
        if self.caught_up.is_some_and(|done| now <= done) {
            return;
        }
        self.caught_up = Some(now);
        let mut followers: Vec<ValueId> = self
            .nodes
            .iter()
            .filter_map(|(id, node)| match node {
                Node::Spring(follower) if follower.source.is_some() => Some(*id),
                _ => None,
            })
            .collect();
        followers.sort_unstable_by_key(|id| id.0);
        let Some(start) = followers
            .iter()
            .filter_map(|id| match self.nodes.get(id) {
                Some(Node::Spring(follower)) => Some(follower.synced_at),
                _ => None,
            })
            .min()
        else {
            return;
        };
        let start = start.max(now.checked_sub(FRAME * MAX_CATCH_UP_FRAMES).unwrap_or(start));
        let mut at = start + FRAME;
        while at < now {
            for id in &followers {
                let behind = matches!(
                    self.nodes.get(id),
                    Some(Node::Spring(follower)) if follower.synced_at < at
                );
                if behind {
                    self.sample_now(*id, at).ok();
                }
            }
            at += FRAME;
        }
    }

    fn sample_now(&mut self, id: ValueId, now: Instant) -> Result<Sample, String> {
        let input = self.node(id)?.input();
        let upstream = match input {
            Some(input) => Some(self.sample_now(input, now)?),
            None => None,
        };
        match (self.node_mut(id)?, upstream) {
            (Node::Source(source), None) => source.sample(now),
            (Node::Transform(transform), Some(input)) => transform.sample(&input),
            (Node::Spring(follower), upstream) => follower.sample(upstream, now),
            _ => unreachable!("a node's input is sampled exactly when it has one"),
        }
    }

    pub(super) fn get(&mut self, id: ValueId, now: Instant) -> Result<AnimValue, String> {
        Ok(self.sample(id, now)?.value)
    }

    pub(super) fn is_animating(&mut self, id: ValueId, now: Instant) -> Result<bool, String> {
        Ok(self.sample(id, now)?.active)
    }

    /// Writes `value`. Over a running animation this does not stop it: the
    /// value reads back as written at this instant, and the animation takes
    /// over again from the next one, as in motion-dom. Call [`Self::stop`]
    /// first to take the value from the animation.
    pub(super) fn set(
        &mut self,
        id: ValueId,
        value: AnimValue,
        now: Instant,
    ) -> Result<(), String> {
        validate(&value)?;
        self.catch_up(now);
        match self.writable(id)? {
            Node::Source(source) => source.set(value, now),
            Node::Spring(follower) => follower.set(value, now),
            Node::Transform(_) => unreachable!("writable refuses transforms"),
        }
    }

    /// Writes `value`, ending any animation and dropping the velocity.
    pub(super) fn jump(
        &mut self,
        id: ValueId,
        value: AnimValue,
        now: Instant,
    ) -> Result<(), String> {
        validate(&value)?;
        self.catch_up(now);
        match self.writable(id)? {
            Node::Source(source) => source.jump(value, now),
            Node::Spring(follower) => follower.jump(value, now),
            Node::Transform(_) => unreachable!("writable refuses transforms"),
        }
    }

    /// Animates a source from its value at `now`, with the speed it has, to
    /// `target`. A running animation is replaced, and a spring picks up its
    /// speed.
    pub(super) fn animate(
        &mut self,
        id: ValueId,
        target: AnimValue,
        transition: Transition,
        now: Instant,
    ) -> Result<(), String> {
        self.animate_goal(id, Goal::single(target), transition, now)
    }

    /// [`Self::animate`] through keyframes. A leading `None` is the value the
    /// source has at `now`; a later one repeats the keyframe before it.
    pub(super) fn animate_keyframes(
        &mut self,
        id: ValueId,
        frames: Vec<Option<AnimValue>>,
        transition: Transition,
        now: Instant,
    ) -> Result<(), String> {
        self.animate_goal(id, Goal::keyframes(NAME, frames)?, transition, now)
    }

    fn animate_goal(
        &mut self,
        id: ValueId,
        goal: Goal,
        transition: Transition,
        now: Instant,
    ) -> Result<(), String> {
        for frame in goal.frames().iter().flatten() {
            validate(frame)?;
        }
        self.catch_up(now);
        match self.node_mut(id)? {
            Node::Source(source) => source.animate(goal, transition, now),
            Node::Spring(_) => Err(format!(
                "{NAME} {} is a spring: set it to move its target",
                id.0
            )),
            Node::Transform(_) => Err(derived(id)),
        }
    }

    /// Ends an animation where it is. A transform has none of its own, so this
    /// does nothing for it; a spring stops where it is until its source moves.
    pub(super) fn stop(&mut self, id: ValueId, now: Instant) -> Result<(), String> {
        self.catch_up(now);
        match self.node_mut(id)? {
            Node::Source(source) => source.stop(now),
            Node::Spring(follower) => follower.stop(now),
            Node::Transform(_) => Ok(()),
        }
    }

    fn vacant(&self, id: ValueId) -> Result<(), String> {
        if self.nodes.contains_key(&id) {
            return Err(format!("{NAME} {} already exists", id.0));
        }
        Ok(())
    }

    fn insert(&mut self, id: ValueId, node: Node) -> Result<(), String> {
        self.vacant(id)?;
        self.caught_up = None;
        self.nodes.insert(id, node);
        Ok(())
    }

    fn node(&self, id: ValueId) -> Result<&Node, String> {
        self.nodes
            .get(&id)
            .ok_or_else(|| format!("unknown {NAME} {}", id.0))
    }

    fn node_mut(&mut self, id: ValueId) -> Result<&mut Node, String> {
        self.nodes
            .get_mut(&id)
            .ok_or_else(|| format!("unknown {NAME} {}", id.0))
    }

    fn writable(&mut self, id: ValueId) -> Result<&mut Node, String> {
        match self.node_mut(id)? {
            Node::Transform(_) => Err(derived(id)),
            node => Ok(node),
        }
    }
}

fn derived(id: ValueId) -> String {
    format!(
        "{NAME} {} is computed from another value and cannot be written",
        id.0
    )
}

fn require_spring(transition: &Transition) -> Result<(), String> {
    match transition.curve {
        Curve::Spring(_) => Ok(()),
        Curve::Tween { .. } => Err("a motion spring value needs a spring transition".to_string()),
    }
}

/// `auto` has no number to hold, and an empty list no channel.
fn validate(value: &AnimValue) -> Result<(), String> {
    if *value == AnimValue::Auto {
        return Err(format!("a {NAME} cannot hold auto"));
    }
    pair(NAME, value, value).map(drop)
}

enum Node {
    Source(Source),
    Transform(Transform),
    Spring(Follower),
}

impl Node {
    fn input(&self) -> Option<ValueId> {
        match self {
            Self::Source(_) => None,
            Self::Transform(transform) => Some(transform.input),
            Self::Spring(follower) => follower.source,
        }
    }
}

/// A speed in the channels of `layout`, measured at `at`.
#[derive(Clone, Debug)]
struct Rate {
    layout: Layout,
    per_second: Vec<f64>,
    at: Instant,
}

struct Source {
    /// The value outside an animation, as of `written_at`.
    rest: AnimValue,
    written_at: Option<Instant>,
    /// What `rest` was before the write at an earlier instant, to measure the
    /// speed of a value that is being dragged.
    before: Option<(AnimValue, Instant)>,
    rate: Option<Rate>,
    track: Option<Track>,
    /// A `set` made while the track runs, which holds until the next instant.
    held: Option<(AnimValue, Instant)>,
}

impl Source {
    fn new(initial: AnimValue) -> Self {
        Self {
            rest: initial,
            written_at: None,
            before: None,
            rate: None,
            track: None,
            held: None,
        }
    }

    fn running(&self, now: Instant) -> bool {
        self.track
            .as_ref()
            .is_some_and(|track| track.sample(now).active)
    }

    /// Folds an animation that has ended into the resting value.
    fn settle(&mut self, now: Instant) {
        if self.running(now) {
            return;
        }
        if let Some(track) = self.track.take() {
            self.rest = track.current(now);
            self.written_at = None;
            self.before = None;
            self.rate = None;
        }
        self.held = None;
    }

    fn value(&self, now: Instant) -> AnimValue {
        if let Some((held, at)) = &self.held {
            if now <= *at {
                return held.clone();
            }
        }
        match &self.track {
            Some(track) => track.current(now),
            None => self.rest.clone(),
        }
    }

    fn sample(&self, now: Instant) -> Result<Sample, String> {
        let value = self.value(now);
        if let Some(track) = &self.track {
            let sample = track.sample(now);
            if sample.active {
                return Ok(Sample {
                    value,
                    velocity: sample.velocity,
                    active: true,
                });
            }
        }
        let velocity = self.rest_velocity(&value, now)?;
        Ok(Sample {
            value,
            velocity,
            active: false,
        })
    }

    fn rest_velocity(&self, value: &AnimValue, now: Instant) -> Result<Vec<f64>, String> {
        match &self.rate {
            Some(rate) if fresh(rate.at, now) => Ok(rate.per_second.clone()),
            _ => Ok(pair(NAME, value, value)?.from.iter().map(|_| 0.0).collect()),
        }
    }

    fn set(&mut self, value: AnimValue, now: Instant) -> Result<(), String> {
        let current = self.value(now);
        pair(NAME, &current, &value)?;
        if self.running(now) {
            self.held = Some((value, now));
            return Ok(());
        }
        self.settle(now);
        self.write(value, now)
    }

    fn write(&mut self, value: AnimValue, now: Instant) -> Result<(), String> {
        if self.written_at != Some(now) {
            self.before = self.written_at.map(|at| (self.rest.clone(), at));
        }
        if let Some(before) = &self.before {
            let window = now.saturating_duration_since(before.1);
            self.rate = rate_between(&before.0, &value, window.min(MAX_VELOCITY_WINDOW), now)?;
        }
        self.rest = value;
        self.written_at = Some(now);
        Ok(())
    }

    fn jump(&mut self, value: AnimValue, now: Instant) -> Result<(), String> {
        let current = self.value(now);
        pair(NAME, &current, &value)?;
        self.track = None;
        self.held = None;
        self.rest = value;
        self.written_at = Some(now);
        self.before = None;
        self.rate = None;
        Ok(())
    }

    fn animate(&mut self, goal: Goal, transition: Transition, now: Instant) -> Result<(), String> {
        let from = self.value(now);
        let starts_here = goal.is_single() || goal.frames()[0].is_none();
        let mut track = if goal.is_single() {
            Track::new(NAME, &from, goal.last().clone(), transition, now)?
        } else {
            let mut literal: Vec<AnimValue> = Vec::with_capacity(goal.frames().len());
            for frame in goal.frames() {
                literal.push(match (frame, literal.last()) {
                    (Some(value), _) => value.clone(),
                    (None, Some(previous)) => previous.clone(),
                    (None, None) => from.clone(),
                });
            }
            Track::keyframed(NAME, goal, literal.clone(), &literal, Interpolation::Oklab, transition, now)?
        };
        // A track that opens on a keyframe of its own does not start at the
        // speed the value had.
        if starts_here {
            match &self.track {
                Some(previous) => track.carry_velocity(previous, now),
                None => {
                    if let Some(rate) = self.rate.as_ref().filter(|rate| fresh(rate.at, now)) {
                        track.carry_rate(&rate.layout, rate.per_second.clone());
                    }
                }
            }
        }
        self.track = Some(track);
        self.held = None;
        Ok(())
    }

    /// Keeps the value and the speed it had, so a drag that starts by stopping
    /// the animation can still hand its speed on.
    fn stop(&mut self, now: Instant) -> Result<(), String> {
        let value = self.value(now);
        let Some(track) = self.track.take() else {
            return Ok(());
        };
        let sample = track.sample(now);
        self.rest = value;
        self.held = None;
        self.written_at = Some(now);
        self.before = None;
        self.rate = sample.active.then(|| Rate {
            layout: track.layout().clone(),
            per_second: sample.velocity,
            at: now,
        });
        Ok(())
    }
}

fn fresh(at: Instant, now: Instant) -> bool {
    now.saturating_duration_since(at) <= MAX_VELOCITY_WINDOW
}

fn rate_between(
    from: &AnimValue,
    to: &AnimValue,
    window: Duration,
    now: Instant,
) -> Result<Option<Rate>, String> {
    if window.is_zero() {
        return Ok(None);
    }
    let pair = pair(NAME, from, to)?;
    let seconds = window.as_secs_f64();
    Ok(Some(Rate {
        per_second: pair
            .from
            .iter()
            .zip(&pair.to)
            .map(|(from, to)| (to - from) / seconds)
            .collect(),
        layout: pair.layout,
        at: now,
    }))
}

struct Segment {
    pair: Pair,
    ease: Ease,
}

struct Transform {
    input: ValueId,
    input_range: Vec<f64>,
    segments: Vec<Segment>,
    clamp: bool,
}

impl Transform {
    fn new(
        input: ValueId,
        mut input_range: Vec<f64>,
        mut output_range: Vec<AnimValue>,
        options: TransformOptions,
    ) -> Result<Self, String> {
        if input_range.len() < 2 {
            return Err("a motion transform needs at least two input values".to_string());
        }
        if input_range.len() != output_range.len() {
            return Err(format!(
                "a motion transform's input range has {} values and its output range {}",
                input_range.len(),
                output_range.len()
            ));
        }
        if input_range.iter().any(|value| !value.is_finite()) {
            return Err("a motion transform's input range must be finite numbers".to_string());
        }
        if input_range[0] > input_range[input_range.len() - 1] {
            input_range.reverse();
            output_range.reverse();
        }
        // A tie is a step, as in Framer: the later output wins from the tie on.
        if input_range.windows(2).any(|window| window[0] > window[1]) {
            return Err("a motion transform's input range must be ascending or descending".to_string());
        }

        let count = input_range.len() - 1;
        let eases = match options.ease.len() {
            0 => vec![Ease::Name("linear".to_string()); count],
            1 => vec![options.ease[0].clone(); count],
            length if length == count => options.ease,
            length => return Err(format!(
                "a motion transform with {} input values needs one easing or {count}, not {length}",
                input_range.len()
            )),
        };
        let mut segments = Vec::with_capacity(count);
        for ((outputs, ease), index) in output_range.windows(2).zip(eases).zip(0..) {
            validate(&outputs[0])?;
            validate(&outputs[1])?;
            validate_ease(&ease)?;
            let pair = pair(NAME, &outputs[0], &outputs[1]).map_err(|error| {
                format!("{error} (output range entries {index} and {})", index + 1)
            })?;
            segments.push(Segment { pair, ease });
        }
        Ok(Self {
            input,
            input_range,
            segments,
            clamp: options.clamp,
        })
    }

    fn sample(&self, input: &Sample) -> Result<Sample, String> {
        let AnimValue::Number(x) = input.value else {
            return Err(format!(
                "a motion transform needs a number input, got {:?}",
                input.value
            ));
        };
        let first = self.input_range[0];
        let last = self.input_range[self.input_range.len() - 1];
        let outside = x < first || x > last;
        let x = if self.clamp { x.clamp(first, last) } else { x };

        let last_segment = self.segments.len() - 1;
        let index = (0..last_segment)
            .find(|index| x < self.input_range[index + 1])
            .unwrap_or(last_segment);
        let segment = &self.segments[index];
        let span = self.input_range[index + 1] - self.input_range[index];
        // Only a tie at an end can be reached with no span: anywhere else the
        // segment search steps over it.
        let (progress, step) = if span == 0.0 {
            (f64::from(u8::from(x >= self.input_range[index])), true)
        } else {
            ((x - self.input_range[index]) / span, false)
        };
        let (eased, slope) = apply_ease(progress, &segment.ease);

        let input_rate = if (self.clamp && outside) || step {
            0.0
        } else {
            input.velocity[0]
        };
        let velocity = segment
            .pair
            .from
            .iter()
            .zip(&segment.pair.to)
            .map(|(from, to)| (to - from) * slope / if step { 1.0 } else { span } * input_rate)
            .collect();
        Ok(Sample {
            value: segment.pair.at(eased),
            velocity,
            active: input.active,
        })
    }
}

/// The eased progress and its slope. Outside a segment it continues in a
/// straight line from the easing's end, at the slope it ends with.
fn apply_ease(progress: f64, curve: &Ease) -> (f64, f64) {
    let edge = progress.clamp(0.0, 1.0);
    let slope = ease_slope(edge, curve);
    (ease(edge, curve) + slope * (progress - edge), slope)
}

struct Follower {
    /// `None` for a spring with its own target.
    source: Option<ValueId>,
    transition: Transition,
    track: Track,
    synced_at: Instant,
}

impl Follower {
    fn new(
        source: Option<ValueId>,
        start: AnimValue,
        transition: Transition,
        now: Instant,
    ) -> Result<Self, String> {
        Ok(Self {
            source,
            track: Track::new(NAME, &start, start.clone(), transition.clone(), now)?,
            transition,
            synced_at: now,
        })
    }

    /// Moves the spring to `target` from where it is at `now`, with its speed.
    fn retarget(&mut self, target: AnimValue, now: Instant) -> Result<(), String> {
        let mut next = Track::new(
            NAME,
            &self.track.current(now),
            target,
            self.transition.clone(),
            now,
        )?;
        next.carry_velocity(&self.track, now);
        self.track = next;
        self.synced_at = self.synced_at.max(now);
        Ok(())
    }

    fn sample(&mut self, upstream: Option<Sample>, now: Instant) -> Result<Sample, String> {
        if let Some(input) = &upstream {
            if now > self.synced_at && input.value != *self.track.target() {
                self.retarget(input.value.clone(), now)?;
            }
            self.synced_at = self.synced_at.max(now);
        }
        let sample = self.track.sample(now);
        Ok(Sample {
            value: self.track.value(&sample.position),
            velocity: sample.velocity,
            active: sample.active || upstream.is_some_and(|input| input.active),
        })
    }

    fn own_target(&self) -> Result<(), String> {
        if self.source.is_some() {
            return Err(format!(
                "a {NAME} that follows another value takes its target from it"
            ));
        }
        Ok(())
    }

    fn set(&mut self, target: AnimValue, now: Instant) -> Result<(), String> {
        self.own_target()?;
        self.retarget(target, now)
    }

    fn jump(&mut self, value: AnimValue, now: Instant) -> Result<(), String> {
        self.own_target()?;
        self.track = Track::new(NAME, &value, value.clone(), self.transition.clone(), now)?;
        self.synced_at = self.synced_at.max(now);
        Ok(())
    }

    fn stop(&mut self, now: Instant) -> Result<(), String> {
        let here = self.track.current(now);
        self.track = Track::new(NAME, &here, here.clone(), self.transition.clone(), now)?;
        self.synced_at = self.synced_at.max(now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::curve::parse_timing_function;
    use super::super::value::Color;
    use super::*;

    struct Clock(Instant);

    impl Clock {
        fn new() -> Self {
            Self(Instant::now())
        }

        fn at(&self, millis: u64) -> Instant {
            self.0 + Duration::from_millis(millis)
        }
    }

    fn number(value: f64) -> AnimValue {
        AnimValue::Number(value)
    }

    fn color(value: &str) -> AnimValue {
        AnimValue::Color(Color::parse(value, &crate::motion::Colors::root().element).unwrap())
    }

    fn tween(duration: f64) -> Transition {
        Transition::parse(json!({ "duration": duration, "ease": "linear" })).unwrap()
    }

    fn spring() -> Transition {
        Transition::parse(json!({ "type": "spring", "stiffness": 200, "damping": 12 })).unwrap()
    }

    fn read(values: &mut MotionValues, id: ValueId, at: Instant) -> f64 {
        match values.get(id, at).unwrap() {
            AnimValue::Number(value) => value,
            other => panic!("expected a number, got {other:?}"),
        }
    }

    fn rate(values: &mut MotionValues, id: ValueId, at: Instant) -> f64 {
        values.velocity(id, at).unwrap()[0]
    }

    fn close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-6,
            "expected {expected}, got {actual}"
        );
    }

    impl MotionValues {
        fn velocity(&mut self, id: ValueId, now: Instant) -> Result<Vec<f64>, String> {
            Ok(self.sample(id, now)?.velocity)
        }

        fn fresh(&self) -> ValueId {
            ValueId(self.nodes.keys().map(|id| id.0 + 1).max().unwrap_or(0))
        }

        fn new_source(&mut self, initial: AnimValue) -> Result<ValueId, String> {
            let id = self.fresh();
            self.source(id, initial).map(|()| id)
        }

        fn new_transform(
            &mut self,
            input: ValueId,
            input_range: Vec<f64>,
            output_range: Vec<AnimValue>,
            options: TransformOptions,
        ) -> Result<ValueId, String> {
            let id = self.fresh();
            self.transform(id, input, input_range, output_range, options)
                .map(|()| id)
        }

        fn new_follow(
            &mut self,
            source: ValueId,
            spring: Transition,
            now: Instant,
        ) -> Result<ValueId, String> {
            let id = self.fresh();
            self.follow(id, source, spring, now).map(|()| id)
        }

        fn new_spring(
            &mut self,
            initial: AnimValue,
            spring: Transition,
            now: Instant,
        ) -> Result<ValueId, String> {
            let id = self.fresh();
            self.spring(id, initial, spring, now).map(|()| id)
        }
    }

    fn map(values: &mut MotionValues, input: ValueId) -> ValueId {
        values
            .new_transform(
                input,
                vec![0.0, 100.0],
                vec![number(10.0), number(30.0)],
                TransformOptions::default(),
            )
            .unwrap()
    }

    #[test]
    fn a_source_holds_what_it_was_given() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let id = values.new_source(number(3.0)).unwrap();
        assert_eq!(read(&mut values, id, clock.at(0)), 3.0);
        values.set(id, number(8.0), clock.at(10)).unwrap();
        assert_eq!(read(&mut values, id, clock.at(10)), 8.0);
        values.jump(id, number(1.0), clock.at(20)).unwrap();
        assert_eq!(read(&mut values, id, clock.at(500)), 1.0);
        assert!(!values.is_animating(id, clock.at(500)).unwrap());
    }

    #[test]
    fn animate_runs_a_tween_from_the_current_value() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let id = values.new_source(number(10.0)).unwrap();
        values
            .animate(id, number(110.0), tween(1.0), clock.at(0))
            .unwrap();
        assert!(values.is_animating(id, clock.at(250)).unwrap());
        close(read(&mut values, id, clock.at(250)), 35.0);
        close(rate(&mut values, id, clock.at(250)), 100.0);
        assert_eq!(read(&mut values, id, clock.at(1000)), 110.0);
        assert!(!values.is_animating(id, clock.at(1000)).unwrap());
        assert_eq!(rate(&mut values, id, clock.at(1000)), 0.0);
    }

    #[test]
    fn animate_works_on_colours() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let id = values.new_source(color("#000000")).unwrap();
        values
            .animate(id, color("#ffffff"), tween(1.0), clock.at(0))
            .unwrap();
        assert_eq!(values.get(id, clock.at(1000)).unwrap(), color("#ffffff"));
        assert_ne!(values.get(id, clock.at(500)).unwrap(), color("#ffffff"));
    }

    #[test]
    fn set_does_not_stop_an_animation_but_stop_does() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let id = values.new_source(number(0.0)).unwrap();
        values
            .animate(id, number(100.0), tween(1.0), clock.at(0))
            .unwrap();

        values.set(id, number(-5.0), clock.at(500)).unwrap();
        assert_eq!(read(&mut values, id, clock.at(500)), -5.0);
        assert!(values.is_animating(id, clock.at(500)).unwrap());
        close(read(&mut values, id, clock.at(600)), 60.0);

        values.stop(id, clock.at(600)).unwrap();
        values.set(id, number(7.0), clock.at(600)).unwrap();
        assert_eq!(read(&mut values, id, clock.at(900)), 7.0);
        assert!(!values.is_animating(id, clock.at(900)).unwrap());
    }

    #[test]
    fn stop_keeps_the_value_and_hands_its_speed_on() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let id = values.new_source(number(0.0)).unwrap();
        values
            .animate(id, number(100.0), tween(1.0), clock.at(0))
            .unwrap();
        values.stop(id, clock.at(400)).unwrap();
        close(read(&mut values, id, clock.at(400)), 40.0);
        close(read(&mut values, id, clock.at(2000)), 40.0);
        close(rate(&mut values, id, clock.at(410)), 100.0);
        assert_eq!(rate(&mut values, id, clock.at(2000)), 0.0);
    }

    #[test]
    fn a_dragged_value_has_the_speed_of_its_last_writes() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let id = values.new_source(number(0.0)).unwrap();
        values.set(id, number(0.0), clock.at(0)).unwrap();
        values.set(id, number(10.0), clock.at(10)).unwrap();
        close(rate(&mut values, id, clock.at(10)), 1000.0);
        assert_eq!(rate(&mut values, id, clock.at(100)), 0.0);

        values.set(id, number(10.0), clock.at(500)).unwrap();
        values.set(id, number(11.0), clock.at(900)).unwrap();
        // 400 ms between the writes counts as the 30 ms window.
        close(rate(&mut values, id, clock.at(900)), 1.0 / 0.03);
    }

    #[test]
    fn a_second_write_at_the_same_instant_keeps_the_first_one_as_the_reference() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let id = values.new_source(number(0.0)).unwrap();
        values.set(id, number(0.0), clock.at(0)).unwrap();
        values.set(id, number(5.0), clock.at(10)).unwrap();
        values.set(id, number(10.0), clock.at(10)).unwrap();
        close(rate(&mut values, id, clock.at(10)), 1000.0);
    }

    #[test]
    fn jump_drops_the_velocity() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let id = values.new_source(number(0.0)).unwrap();
        values.set(id, number(0.0), clock.at(0)).unwrap();
        values.set(id, number(10.0), clock.at(10)).unwrap();
        values.jump(id, number(10.0), clock.at(11)).unwrap();
        assert_eq!(rate(&mut values, id, clock.at(11)), 0.0);
    }

    #[test]
    fn a_spring_started_on_a_dragged_value_carries_its_speed() {
        let clock = Clock::new();
        let carried = {
            let mut values = MotionValues::default();
            let id = values.new_source(number(0.0)).unwrap();
            values.set(id, number(0.0), clock.at(0)).unwrap();
            values.set(id, number(10.0), clock.at(10)).unwrap();
            values
                .animate(id, number(10.0), spring(), clock.at(10))
                .unwrap();
            read(&mut values, id, clock.at(110))
        };
        let at_rest = {
            let mut values = MotionValues::default();
            let id = values.new_source(number(10.0)).unwrap();
            values
                .animate(id, number(10.0), spring(), clock.at(10))
                .unwrap();
            read(&mut values, id, clock.at(110))
        };
        assert_eq!(at_rest, 10.0);
        assert!(carried > 10.5, "the spring overshoots at 1000/s: {carried}");
    }

    #[test]
    fn retargeting_a_spring_keeps_its_position_and_speed() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let id = values.new_source(number(0.0)).unwrap();
        values
            .animate(id, number(100.0), spring(), clock.at(0))
            .unwrap();
        let position = read(&mut values, id, clock.at(100));
        let speed = rate(&mut values, id, clock.at(100));
        values
            .animate(id, number(-100.0), spring(), clock.at(100))
            .unwrap();
        close(read(&mut values, id, clock.at(100)), position);
        close(rate(&mut values, id, clock.at(100)), speed);
        assert!(read(&mut values, id, clock.at(116)) > position);
    }

    #[test]
    fn a_transform_maps_its_input_and_clamps_by_default() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let input = values.new_source(number(50.0)).unwrap();
        let output = map(&mut values, input);
        close(read(&mut values, output, clock.at(0)), 20.0);
        values.set(input, number(250.0), clock.at(1)).unwrap();
        close(read(&mut values, output, clock.at(1)), 30.0);
        values.set(input, number(-50.0), clock.at(2)).unwrap();
        close(read(&mut values, output, clock.at(2)), 10.0);
    }

    #[test]
    fn a_transform_without_clamp_extends_its_end_segments() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let input = values.new_source(number(200.0)).unwrap();
        let output = values
            .new_transform(
                input,
                vec![0.0, 100.0],
                vec![number(10.0), number(30.0)],
                TransformOptions {
                    clamp: false,
                    ease: vec![],
                },
            )
            .unwrap();
        close(read(&mut values, output, clock.at(0)), 50.0);
        values.set(input, number(-100.0), clock.at(1)).unwrap();
        close(read(&mut values, output, clock.at(1)), -10.0);
    }

    #[test]
    fn a_transform_has_one_segment_per_gap_with_its_own_easing() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let input = values.new_source(number(0.0)).unwrap();
        let output = values
            .new_transform(
                input,
                vec![0.0, 1.0, 3.0],
                vec![number(0.0), number(10.0), number(20.0)],
                TransformOptions {
                    clamp: true,
                    ease: vec![
                        Ease::Name("linear".to_string()),
                        Ease::Name("easeIn".to_string()),
                    ],
                },
            )
            .unwrap();
        values.set(input, number(0.5), clock.at(1)).unwrap();
        close(read(&mut values, output, clock.at(1)), 5.0);
        values.set(input, number(2.0), clock.at(2)).unwrap();
        let eased = read(&mut values, output, clock.at(2));
        assert!(
            eased > 10.0 && eased < 15.0,
            "easeIn is below linear: {eased}"
        );
        values.set(input, number(3.0), clock.at(3)).unwrap();
        close(read(&mut values, output, clock.at(3)), 20.0);
    }

    #[test]
    fn a_transform_accepts_a_descending_input_range() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let input = values.new_source(number(25.0)).unwrap();
        let output = values
            .new_transform(
                input,
                vec![100.0, 0.0],
                vec![number(0.0), number(1.0)],
                TransformOptions::default(),
            )
            .unwrap();
        close(read(&mut values, output, clock.at(0)), 0.75);
    }

    #[test]
    fn a_transform_blends_colours() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let input = values.new_source(number(0.0)).unwrap();
        let output = values
            .new_transform(
                input,
                vec![0.0, 100.0],
                vec![color("#000000"), color("#ffffff")],
                TransformOptions::default(),
            )
            .unwrap();
        assert_eq!(values.get(output, clock.at(0)).unwrap(), color("#000000"));
        values.set(input, number(100.0), clock.at(1)).unwrap();
        assert_eq!(values.get(output, clock.at(1)).unwrap(), color("#ffffff"));
        values.set(input, number(50.0), clock.at(2)).unwrap();
        let middle = values.get(output, clock.at(2)).unwrap();
        assert!(middle != color("#000000") && middle != color("#ffffff"));
    }

    #[test]
    fn a_transform_is_computed_from_its_input_at_the_same_instant() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let input = values.new_source(number(0.0)).unwrap();
        let output = map(&mut values, input);
        values
            .animate(input, number(100.0), tween(1.0), clock.at(0))
            .unwrap();
        assert!(values.is_animating(output, clock.at(500)).unwrap());
        close(read(&mut values, input, clock.at(500)), 50.0);
        close(read(&mut values, output, clock.at(500)), 20.0);
        close(read(&mut values, output, clock.at(250)), 10.0 + 20.0 * 0.25);
        assert!(!values.is_animating(output, clock.at(1000)).unwrap());
    }

    #[test]
    fn a_transform_scales_the_speed_of_its_input() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let input = values.new_source(number(0.0)).unwrap();
        let output = map(&mut values, input);
        values
            .animate(input, number(100.0), tween(1.0), clock.at(0))
            .unwrap();
        // 100 per second through a slope of 0.2.
        close(rate(&mut values, output, clock.at(500)), 20.0);

        let mut eased = MotionValues::default();
        let input = eased.new_source(number(0.0)).unwrap();
        let output = eased
            .new_transform(
                input,
                vec![0.0, 100.0],
                vec![number(0.0), number(100.0)],
                TransformOptions {
                    clamp: true,
                    ease: vec![Ease::Name("easeIn".to_string())],
                },
            )
            .unwrap();
        eased
            .animate(input, number(100.0), tween(1.0), clock.at(0))
            .unwrap();
        let before = read(&mut eased, output, clock.at(500));
        let after = read(&mut eased, output, clock.at(501));
        let measured = (after - before) / 0.001;
        let reported = rate(&mut eased, output, clock.at(500));
        assert!((measured - reported).abs() / reported.abs() < 0.01);
    }

    #[test]
    fn a_transform_through_steps_has_no_speed_next_to_a_jump() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let input = values.new_source(number(0.0)).unwrap();
        let output = values
            .new_transform(
                input,
                vec![0.0, 100.0],
                vec![number(0.0), number(100.0)],
                TransformOptions {
                    clamp: true,
                    ease: vec![parse_timing_function("steps(4)").unwrap()],
                },
            )
            .unwrap();
        values
            .animate(input, number(100.0), tween(1.0), clock.at(0))
            .unwrap();
        // The input crosses the first jump (25) at 250 ms; a central difference over 1e-4 of the
        // progress straddles it and reports ~1e5 units/s from 249.9 ms on.
        for millis in [249, 250, 251] {
            close(rate(&mut values, output, clock.at(millis)), 0.0);
        }
    }

    #[test]
    fn a_clamped_transform_has_no_speed_outside_its_range() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let input = values.new_source(number(0.0)).unwrap();
        let output = map(&mut values, input);
        values.set(input, number(-50.0), clock.at(0)).unwrap();
        values.set(input, number(-40.0), clock.at(10)).unwrap();
        assert_eq!(rate(&mut values, output, clock.at(10)), 0.0);
    }

    #[test]
    fn a_tie_in_the_input_range_is_a_step() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let input = values.new_source(number(0.0)).unwrap();
        let output = values
            .new_transform(
                input,
                vec![0.0, 10.0, 10.0, 20.0],
                vec![number(0.0), number(1.0), number(5.0), number(6.0)],
                TransformOptions::default(),
            )
            .unwrap();
        for (x, expected) in [(0.0, 0.0), (5.0, 0.5), (9.999, 0.9999), (10.0, 5.0), (15.0, 5.5), (30.0, 6.0)] {
            values.set(input, number(x), clock.at(0)).unwrap();
            close(read(&mut values, output, clock.at(0)), expected);
        }

        let at_the_start = values
            .new_transform(
                input,
                vec![0.0, 0.0, 10.0],
                vec![number(2.0), number(3.0), number(4.0)],
                TransformOptions { clamp: false, ease: Vec::new() },
            )
            .unwrap();
        for (x, expected) in [(-5.0, 2.0), (0.0, 3.0), (5.0, 3.5), (20.0, 5.0)] {
            values.set(input, number(x), clock.at(1)).unwrap();
            close(read(&mut values, at_the_start, clock.at(1)), expected);
        }
    }

    #[test]
    fn an_id_can_only_be_used_once() {
        let mut values = MotionValues::default();
        values.source(ValueId(7), number(0.0)).unwrap();
        assert!(values.source(ValueId(7), number(1.0)).is_err());
        assert!(values.remove(ValueId(7)));
        values.source(ValueId(7), number(1.0)).unwrap();
    }

    #[test]
    fn a_transform_rejects_ranges_that_cannot_work() {
        let mut values = MotionValues::default();
        let input = values.new_source(number(0.0)).unwrap();
        let build = |values: &mut MotionValues,
                     inputs: Vec<f64>,
                     outputs: Vec<AnimValue>,
                     ease: Vec<Ease>| {
            values.new_transform(
                input,
                inputs,
                outputs,
                TransformOptions { clamp: true, ease },
            )
        };
        assert!(build(&mut values, vec![0.0], vec![number(0.0)], vec![]).is_err());
        assert!(build(&mut values, vec![0.0, 1.0], vec![number(0.0)], vec![]).is_err());
        assert!(build(
            &mut values,
            vec![0.0, 2.0, 1.0],
            vec![number(0.0), number(1.0), number(2.0)],
            vec![]
        )
        .is_err());
        assert!(build(
            &mut values,
            vec![0.0, 1.0, 2.0],
            vec![number(0.0), number(1.0), number(2.0)],
            vec![Ease::Name("linear".to_string()); 3]
        )
        .is_err());
        assert!(build(
            &mut values,
            vec![0.0, 1.0],
            vec![number(0.0), color("red")],
            vec![]
        )
        .is_err());
        assert!(build(
            &mut values,
            vec![0.0, 1.0],
            vec![number(0.0), number(1.0)],
            vec![Ease::Name("bounce".to_string())]
        )
        .is_err());
        assert!(values
            .new_transform(
                ValueId(99),
                vec![0.0, 1.0],
                vec![number(0.0), number(1.0)],
                TransformOptions::default()
            )
            .is_err());
    }

    #[test]
    fn a_transform_of_a_colour_fails_when_sampled() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let input = values.new_source(color("red")).unwrap();
        let output = map(&mut values, input);
        assert!(values.get(output, clock.at(0)).is_err());
    }

    #[test]
    fn derived_values_cannot_be_written() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let input = values.new_source(number(0.0)).unwrap();
        let output = map(&mut values, input);
        assert!(values.set(output, number(1.0), clock.at(0)).is_err());
        assert!(values.jump(output, number(1.0), clock.at(0)).is_err());
        assert!(values
            .animate(output, number(1.0), tween(1.0), clock.at(0))
            .is_err());
        assert!(values.stop(output, clock.at(0)).is_ok());
    }

    #[test]
    fn a_source_refuses_another_kind_and_auto() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let id = values.new_source(number(0.0)).unwrap();
        assert!(values.set(id, color("red"), clock.at(0)).is_err());
        assert!(values.jump(id, AnimValue::Auto, clock.at(0)).is_err());
        assert!(values.new_source(AnimValue::Auto).is_err());
        assert!(values
            .animate(id, color("red"), tween(1.0), clock.at(0))
            .is_err());
        assert_eq!(read(&mut values, id, clock.at(1)), 0.0);
    }

    #[test]
    fn a_removed_value_fails_everything_that_reads_it() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let input = values.new_source(number(0.0)).unwrap();
        let output = map(&mut values, input);
        assert!(values.remove(input));
        assert!(!values.remove(input));
        assert!(values.get(input, clock.at(0)).is_err());
        assert!(values.get(output, clock.at(0)).is_err());
    }

    #[test]
    fn a_follower_springs_after_its_source() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let source = values.new_source(number(0.0)).unwrap();
        let follower = values.new_follow(source, spring(), clock.at(0)).unwrap();
        assert_eq!(read(&mut values, follower, clock.at(0)), 0.0);
        assert!(!values.is_animating(follower, clock.at(0)).unwrap());

        values.set(source, number(100.0), clock.at(16)).unwrap();
        let first = read(&mut values, follower, clock.at(16));
        let second = read(&mut values, follower, clock.at(116));
        assert_eq!(first, 0.0);
        assert!(second > 0.0 && second < 100.0 + 50.0);
        assert!(values.is_animating(follower, clock.at(116)).unwrap());

        close(read(&mut values, follower, clock.at(20_000)), 100.0);
        assert!(!values.is_animating(follower, clock.at(20_000)).unwrap());
    }

    #[test]
    fn a_follower_nobody_sampled_catches_up_on_the_frames_it_missed() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let source = values.new_source(number(0.0)).unwrap();
        let follower = values.new_follow(source, spring(), clock.at(0)).unwrap();
        values.set(source, number(100.0), clock.at(0)).unwrap();

        let mut frames = MotionValues::default();
        let framed_source = frames.new_source(number(0.0)).unwrap();
        let framed = frames.new_follow(framed_source, spring(), clock.at(0)).unwrap();
        frames.set(framed_source, number(100.0), clock.at(0)).unwrap();
        for ms in (16..=160).step_by(16) {
            read(&mut frames, framed, clock.at(ms));
        }

        let late = read(&mut values, follower, clock.at(160));
        assert!(late > 0.0 && late < 100.0 + 50.0, "{late}");
        close(late, read(&mut frames, framed, clock.at(160)));
    }

    #[test]
    fn reading_other_values_at_the_same_instant_first_does_not_change_what_a_follower_reads() {
        let clock = Clock::new();
        let build = || {
            let mut values = MotionValues::default();
            let source = values.new_source(number(0.0)).unwrap();
            let follower = values.new_follow(source, spring(), clock.at(0)).unwrap();
            values.set(source, number(100.0), clock.at(0)).unwrap();
            (values, source, follower)
        };
        let (mut direct, _, direct_follower) = build();
        let (mut crowded, source, follower) = build();
        // The first sample at an instant does the catch-up for every follower; the ones after it find it done.
        read(&mut crowded, source, clock.at(160));
        let extra = crowded.new_source(number(1.0)).unwrap();
        read(&mut crowded, extra, clock.at(160));
        close(read(&mut crowded, follower, clock.at(160)), read(&mut direct, direct_follower, clock.at(160)));

        // A follower added once the instant was caught up is caught up too when the clock moves on.
        let late = crowded.new_follow(source, spring(), clock.at(160)).unwrap();
        read(&mut crowded, late, clock.at(320));
        assert!(read(&mut crowded, late, clock.at(480)) > 0.0);
    }

    #[test]
    fn a_write_after_a_long_gap_does_not_move_the_follower_back_in_time() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let source = values.new_source(number(0.0)).unwrap();
        let follower = values.new_follow(source, spring(), clock.at(0)).unwrap();
        values.set(source, number(100.0), clock.at(1_000)).unwrap();
        // The source was still 0 for the first second.
        assert_eq!(read(&mut values, follower, clock.at(1_000)), 0.0);
        let after = read(&mut values, follower, clock.at(1_160));
        assert!(after > 0.0 && after < 150.0, "{after}");
    }

    #[test]
    fn a_follower_that_is_retargeted_carries_its_velocity() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let source = values.new_source(number(0.0)).unwrap();
        let follower = values.new_follow(source, spring(), clock.at(0)).unwrap();
        values.set(source, number(100.0), clock.at(0)).unwrap();
        read(&mut values, follower, clock.at(16));
        let position = read(&mut values, follower, clock.at(150));
        let speed = rate(&mut values, follower, clock.at(150));
        assert!(speed > 0.0);

        values.set(source, number(-100.0), clock.at(150)).unwrap();
        let carried = read(&mut values, follower, clock.at(166));
        assert!(carried > position, "still moving up on the carried speed");
        assert!(read(&mut values, follower, clock.at(1500)) < carried);
    }

    #[test]
    fn sampling_an_instant_twice_changes_nothing() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let source = values.new_source(number(0.0)).unwrap();
        let follower = values.new_follow(source, spring(), clock.at(0)).unwrap();
        values.set(source, number(100.0), clock.at(0)).unwrap();
        let once = read(&mut values, follower, clock.at(100));
        let twice = read(&mut values, follower, clock.at(100));
        assert_eq!(once, twice);
        assert_eq!(
            rate(&mut values, follower, clock.at(100)),
            rate(&mut values, follower, clock.at(100))
        );
    }

    #[test]
    fn a_transform_of_a_follower_reads_the_same_instant_as_the_follower() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let source = values.new_source(number(0.0)).unwrap();
        let follower = values.new_follow(source, spring(), clock.at(0)).unwrap();
        let output = map(&mut values, follower);
        values.set(source, number(100.0), clock.at(0)).unwrap();
        read(&mut values, output, clock.at(16));
        for millis in [32, 48, 200, 400] {
            let derived = read(&mut values, output, clock.at(millis));
            let base = read(&mut values, follower, clock.at(millis));
            close(derived, 10.0 + 20.0 * base.clamp(0.0, 100.0) / 100.0);
        }
    }

    #[test]
    fn a_follower_follows_a_tweened_source_one_frame_behind() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let source = values.new_source(number(0.0)).unwrap();
        let follower = values.new_follow(source, spring(), clock.at(0)).unwrap();
        values
            .animate(source, number(100.0), tween(1.0), clock.at(0))
            .unwrap();
        assert!(values.is_animating(follower, clock.at(0)).unwrap());
        let mut last = 0.0;
        for frame in 1..=120 {
            let now = read(&mut values, follower, clock.at(frame * 16));
            if frame <= 60 {
                assert!(now >= last, "frame {frame}: {now} < {last}");
            }
            last = now;
        }
        close(read(&mut values, follower, clock.at(30_000)), 100.0);
    }

    #[test]
    fn a_spring_with_its_own_target_springs_when_set() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let id = values.new_spring(number(0.0), spring(), clock.at(0)).unwrap();
        values.set(id, number(100.0), clock.at(0)).unwrap();
        assert!(values.is_animating(id, clock.at(100)).unwrap());
        let moving = read(&mut values, id, clock.at(100));
        assert!(moving > 0.0 && moving != 100.0);
        close(read(&mut values, id, clock.at(20_000)), 100.0);

        values.jump(id, number(5.0), clock.at(20_001)).unwrap();
        assert_eq!(read(&mut values, id, clock.at(20_002)), 5.0);
        assert!(!values.is_animating(id, clock.at(20_002)).unwrap());
    }

    #[test]
    fn stopping_a_spring_holds_it_where_it_is() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let id = values.new_spring(number(0.0), spring(), clock.at(0)).unwrap();
        values.set(id, number(100.0), clock.at(0)).unwrap();
        let here = read(&mut values, id, clock.at(100));
        values.stop(id, clock.at(100)).unwrap();
        close(read(&mut values, id, clock.at(5000)), here);
        assert!(!values.is_animating(id, clock.at(5000)).unwrap());
    }

    #[test]
    fn a_following_spring_takes_its_target_from_its_source() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let source = values.new_source(number(0.0)).unwrap();
        let follower = values.new_follow(source, spring(), clock.at(0)).unwrap();
        assert!(values.set(follower, number(1.0), clock.at(0)).is_err());
        assert!(values.jump(follower, number(1.0), clock.at(0)).is_err());
        assert!(values
            .animate(follower, number(1.0), spring(), clock.at(0))
            .is_err());
        assert!(values.new_follow(source, tween(1.0), clock.at(0)).is_err());
        assert!(values.new_spring(number(0.0), tween(1.0), clock.at(0)).is_err());
    }

    #[test]
    fn a_follower_follows_a_colour() {
        let clock = Clock::new();
        let mut values = MotionValues::default();
        let source = values.new_source(color("#000000")).unwrap();
        let follower = values.new_follow(source, spring(), clock.at(0)).unwrap();
        values.set(source, color("#ffffff"), clock.at(0)).unwrap();
        values.get(follower, clock.at(16)).unwrap();
        assert_ne!(
            values.get(follower, clock.at(100)).unwrap(),
            color("#000000")
        );
        assert_eq!(
            values.get(follower, clock.at(30_000)).unwrap(),
            color("#ffffff")
        );
    }
}
