//! The motion value graph as the renderer and JS see it: ids and values as
//! JSON, a per-frame sample of the values something depends on, and the
//! `motionValues` style binding.
//!
//! The graph is lazy (see [`super::values`]). This layer is what makes it tick:
//! each frame samples every value that is bound to an element or has a
//! listener, in one pass at the frame's instant, so a spring follower sees every
//! frame and a transform reads the same instant as the value it maps.

use std::collections::{HashMap, HashSet};

use serde::Deserialize;
use serde_json::{Map, Value};
use web_time::Instant;

use super::curve::{parse_ease, Curve, Transition};
use super::list::ListValue;
use super::property::{
    check_finite, expand_shorthand, parse_shadow_list, shadow_layers, split_list, AnimProp, Scope,
};
use super::value::{AnimValue, Color, ParentValues};
use super::values::{MotionValues, PlaybackInfo, TransformOptions, ValueId};
use crate::color::{ColorContext, Scheme};
use crate::css::dimension::parse_linear_length;
use crate::css::angle::parse_degrees;
use crate::css::transform::{format_list, parse_transform, Value as TransformValue};
use crate::css::length::{LengthContext, LinearLength};
use crate::css::font_variations::VariationSettings;
use crate::css::syntax::split_slash_tokens;
use crate::dormant_wake::Wake;
use crate::style::StyleDesc;

const NAME: &str = "motion value";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MotionValueEventKind {
    Change,
    AnimationStart,
    AnimationComplete,
}

impl MotionValueEventKind {
    pub(crate) fn event_type(self) -> &'static str {
        match self {
            Self::Change => "motionValueChange",
            Self::AnimationStart => "motionValueAnimationStart",
            Self::AnimationComplete => "motionValueAnimationComplete",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MotionValueEvent {
    pub(crate) id: u32,
    pub(crate) kind: MotionValueEventKind,
    /// The value as JSON text, for a change.
    pub(crate) value: Option<String>,
}

/// What the values that move ask of the frames to come.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Demand {
    /// Nothing moves by itself.
    Idle,
    /// Only values that nothing seen shows move (bound to dormant elements, or only listened to):
    /// their clock and events go on, and no frame is drawn for them.
    Dormant,
    /// A value that an element in view is bound to, or that a drag reads, moves.
    Frame,
}

#[derive(Default)]
pub(crate) struct MotionValueFrame {
    /// Something sampled is still moving by itself, so another frame is due. A held
    /// animation is not.
    pub(crate) active: bool,
    pub(crate) events: Vec<MotionValueEvent>,
}

struct Watch {
    change: bool,
    animation: bool,
    reported: AnimValue,
    was_active: bool,
}

#[derive(Default)]
pub(crate) struct MotionValueStore {
    /// The window's appearance. A value is not tied to one element, so its colours
    /// resolve as on a root that declares `colorScheme: "light dark"`:
    /// `light-dark()` follows this and `currentColor` is the initial black. A
    /// colour keeps what it resolved to when it was written.
    appearance: Scheme,
    /// The window's size in px, which `vw` and `vh` mean. Like a colour, a length
    /// keeps what it resolved to when it was written, unlike `em` and `rem`.
    viewport: (f64, f64),
    graph: MotionValues,
    watches: HashMap<ValueId, Watch>,
    /// What the last frame sampled for the values elements bind.
    frame_values: HashMap<ValueId, AnimValue>,
    /// The values of `frame_values` that were changing by themselves (an animation or a spring) when sampled.
    frame_moving: HashSet<ValueId>,
    /// The values with a listener that were changing by themselves when last observed.
    watched_moving: HashSet<ValueId>,
    /// A value the renderer reads for its own use (a drag's offset) was changing by itself.
    read_moving: bool,
    /// The values the last frame read for the renderer's own use, and which of them were moving.
    read_ids: HashSet<ValueId>,
    read_moving_ids: HashSet<ValueId>,
    /// The values the last frame found bound to something that is not paint-only, or read for
    /// the renderer's own use (a drag's offset), which is not known to be.
    layout_driven: HashSet<ValueId>,
    /// The values the last frame found bound to a transform key, as `layout_driven` is.
    transform_driven: HashSet<ValueId>,
    /// The values written, animated or controlled since the last frame sampled: an element bound to
    /// one of them, or to a value computed from it, shows something else at the next frame.
    written: HashSet<ValueId>,
    /// Events found while writing, delivered by the next frame.
    pending: Vec<MotionValueEvent>,
    warned: HashSet<String>,
}

/// The values each element's `motionValues` binds, and the elements each value is bound to, parsed
/// once for as long as the bindings are as they were, so that a tick does not parse them all
/// again to ask which of them a moving value drives.
pub(crate) struct BindingIndex {
    /// `None` for a binding that does not parse.
    by_element: HashMap<u64, Option<Vec<ValueId>>>,
    by_value: HashMap<ValueId, Vec<u64>>,
}

impl BindingIndex {
    pub(crate) fn new<'a>(bindings: impl IntoIterator<Item = (u64, &'a Value)>) -> Self {
        let mut by_element = HashMap::new();
        let mut by_value: HashMap<ValueId, Vec<u64>> = HashMap::new();
        for (element, binding) in bindings {
            match parse_binding(binding) {
                Ok(pairs) => {
                    let ids: Vec<ValueId> = pairs.iter().map(Bound::id).collect();
                    for id in &ids {
                        by_value.entry(*id).or_default().push(element);
                    }
                    by_element.insert(element, Some(ids));
                }
                Err(_) => {
                    by_element.insert(element, None);
                }
            }
        }
        Self { by_element, by_value }
    }

    /// Whether `asleep` holds of every element that binds the value `id`, and one does.
    fn bound_only_to(&self, id: &ValueId, asleep: &dyn Fn(u64) -> bool) -> bool {
        self.by_value.get(id).is_some_and(|elements| !elements.is_empty() && elements.iter().all(|element| asleep(*element)))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawTransformOptions {
    clamp: Option<bool>,
    ease: Option<Value>,
}

impl MotionValueStore {
    pub(crate) fn set_appearance(&mut self, appearance: Scheme) {
        self.appearance = appearance;
    }

    pub(crate) fn set_viewport(&mut self, width: f64, height: f64) {
        self.viewport = (width, height);
    }

    /// What a value's colours and lengths mean. It is not tied to an element, so `em`
    /// and `rem` stay multiples of the font size ([`AnimValue::Relative`]) and resolve
    /// against each element the value is bound to; `lengths` is for the other units.
    fn context(&self) -> ValueContext {
        ValueContext {
            colors: ColorContext::root(self.appearance).descend(&StyleDesc {
                color_scheme: Some("light dark".to_string()),
                ..Default::default()
            }),
            lengths: LengthContext::initial(self.viewport.0, self.viewport.1),
        }
    }

    pub(crate) fn create_source(&mut self, id: f64, initial: &Value) -> Result<(), String> {
        self.graph.source(value_id(id)?, parse_value(initial, &self.context())?)
    }

    pub(crate) fn create_transform(
        &mut self,
        id: f64,
        input: f64,
        input_range: Vec<f64>,
        output_range: &Value,
        options: Option<&Value>,
    ) -> Result<(), String> {
        let context = self.context();
        let outputs = output_range
            .as_array()
            .ok_or_else(|| format!("a {NAME} transform's output range must be an array"))?
            .iter()
            .map(|output| parse_value(output, &context))
            .collect::<Result<Vec<_>, _>>()?;
        self.graph.transform(
            value_id(id)?,
            value_id(input)?,
            input_range,
            outputs,
            parse_transform_options(options)?,
        )
    }

    /// A spring that chases another value.
    pub(crate) fn create_follower(
        &mut self,
        id: f64,
        source: f64,
        config: &Value,
        now: Instant,
    ) -> Result<(), String> {
        self.graph
            .follow(value_id(id)?, value_id(source)?, parse_spring(config)?, now)
    }

    /// A spring with a target of its own, moved by `set`.
    pub(crate) fn create_spring(
        &mut self,
        id: f64,
        initial: &Value,
        config: &Value,
        now: Instant,
    ) -> Result<(), String> {
        self.graph
            .spring(value_id(id)?, parse_value(initial, &self.context())?, parse_spring(config)?, now)
    }

    pub(crate) fn dispose(&mut self, id: f64) -> Result<bool, String> {
        let id = value_id(id)?;
        self.watches.remove(&id);
        self.frame_values.remove(&id);
        self.frame_moving.remove(&id);
        Ok(self.graph.remove(id))
    }

    pub(crate) fn get(&mut self, id: f64, now: Instant) -> Result<Value, String> {
        to_json(&self.graph.get(value_id(id)?, now)?)
    }

    /// Per second. Only a number has one rate, so any other value is an error.
    pub(crate) fn velocity(&mut self, id: f64, now: Instant) -> Result<f64, String> {
        let sample = self.graph.sample(value_id(id)?, now)?;
        match (sample.value, sample.velocity.as_slice()) {
            (AnimValue::Number(_), [rate]) => Ok(*rate),
            _ => Err(format!("getVelocity is for {NAME}s that hold a number")),
        }
    }

    pub(crate) fn is_animating(&mut self, id: f64, now: Instant) -> Result<bool, String> {
        self.graph.is_animating(value_id(id)?, now)
    }

    /// Whether a write to the value, or the animation that starts from it, can only change what
    /// the elements paint, as of the last frame: no element binds it, or a value computed from it,
    /// to a property that can move a box. A tree change since that frame is not for this to see
    /// (it invalidates the layout on its own).
    pub(crate) fn writes_paint_only(&self, id: f64) -> Result<bool, String> {
        Ok(!self.graph.feeds_any(value_id(id)?, &self.layout_driven))
    }

    /// Whether a write to the value, or the animation that starts from it, leaves the transform of
    /// every element alone, as of the last frame: no element binds it, or a value computed from it, to
    /// a transform key.
    pub(crate) fn writes_no_transform(&self, id: f64) -> Result<bool, String> {
        Ok(!self.graph.feeds_any(value_id(id)?, &self.transform_driven))
    }

    /// Notes that a script touched the value `id`, which the elements bound to it show differently
    /// from the next frame on.
    fn wrote(&mut self, id: f64) -> Result<ValueId, String> {
        let id = value_id(id)?;
        self.written.insert(id);
        Ok(id)
    }

    /// Whether an element with this `motionValues` binding can show something else at the next
    /// frame than the frame that built it did: a value it binds was moving by itself when that frame
    /// sampled it, or it, or a value it is computed from, was written since. A binding that does not
    /// parse is not known not to.
    pub(crate) fn binding_due(&self, binding: &Value) -> bool {
        parse_binding(binding).map_or(true, |pairs| {
            pairs.iter().any(|bound| {
                let id = bound.id();
                self.frame_moving.contains(&id) || self.graph.follows_any(id, &self.written)
            })
        })
    }

    /// [`Self::binding_due`] for the binding of `element`, as `index` parsed it.
    pub(crate) fn element_due(&self, index: &BindingIndex, element: u64) -> bool {
        match index.by_element.get(&element) {
            None => false,
            Some(None) => true,
            Some(Some(ids)) => ids
                .iter()
                .any(|id| self.frame_moving.contains(id) || self.graph.follows_any(*id, &self.written)),
        }
    }

    /// [`Self::demand`] for the elements `index` holds, which are asked about only if a value
    /// they are bound to moves. `dormant` says of an element whether it is.
    pub(crate) fn demand_in(&self, index: &BindingIndex, dormant: impl Fn(u64) -> bool) -> Demand {
        if self.read_moving {
            return Demand::Frame;
        }
        if self.frame_moving.is_empty() && self.watched_moving.is_empty() {
            return Demand::Idle;
        }
        let seen = self
            .frame_moving
            .iter()
            .any(|id| index.by_value.get(id).is_some_and(|elements| elements.iter().any(|element| !dormant(*element))));
        if seen {
            Demand::Frame
        } else {
            Demand::Dormant
        }
    }

    /// What the values that were moving by themselves at the last sample ask of the frames to come.
    /// `bound` is each element with a binding and whether it is dormant (not seen, and what runs on
    /// it only paints): a value that only dormant elements bind, or that is only listened to, is
    /// carried on by a tick that draws nothing, and the frame is for the ones in view.
    pub(crate) fn demand<'a>(&self, bound: impl IntoIterator<Item = (bool, &'a Value)>) -> Demand {
        if self.read_moving {
            return Demand::Frame;
        }
        if self.frame_moving.is_empty() && self.watched_moving.is_empty() {
            return Demand::Idle;
        }
        let seen = bound.into_iter().any(|(dormant, binding)| {
            !dormant
                && parse_binding(binding)
                    .is_ok_and(|pairs| pairs.iter().any(|bound| self.frame_moving.contains(&bound.id())))
        });
        if seen {
            Demand::Frame
        } else {
            Demand::Dormant
        }
    }

    /// What a frame does for values nothing draws: finds the events of the listened ones at `now`.
    /// The values only dormant elements bind are not sampled: a value is a function of the time,
    /// and the frame that draws an element again samples it, so a spring that follows another
    /// catches up there. Nothing a build reads is touched: not the frame's values, which of them
    /// moved, nor what was written, so the frame that draws an element again finds it as it was left.
    pub(crate) fn advance_dormant(&mut self, now: Instant) -> MotionValueFrame {
        let active = self.observe(now);
        MotionValueFrame {
            active,
            events: std::mem::take(&mut self.pending),
        }
    }

    /// When the values that nothing draws next need a sample, which is for the events of a listened
    /// one: Motion's `change` follows the animation and not the element being seen, so a value with a
    /// `change` listener needs every frame it moves in; one with only an `animation` listener needs
    /// the instant its animation ends, if that is known, and every frame if not. A value no one
    /// listens to needs none, whoever it is bound to.
    pub(crate) fn dormant_wake(&self, now: Instant) -> Wake {
        Wake::merged(self.watched_moving.iter().filter_map(|id| {
            let watch = self.watches.get(id)?;
            Some(if watch.change {
                Wake::NextFrame
            } else {
                self.graph.wake(*id, now)
            })
        }))
    }

    /// Whether a `motionValues` binding drives only properties that change what an element paints.
    /// A binding that does not parse is not known to.
    pub(crate) fn binding_paints_only(binding: &Value) -> bool {
        parse_binding(binding).is_ok_and(|pairs| pairs.iter().all(Bound::paints_only))
    }

    pub(crate) fn set(&mut self, id: f64, value: &Value, now: Instant) -> Result<(), String> {
        let id = self.wrote(id)?;
        self.graph.set(id, parse_value(value, &self.context())?, now)?;
        self.observe(now);
        Ok(())
    }

    pub(crate) fn jump(&mut self, id: f64, value: &Value, now: Instant) -> Result<(), String> {
        let id = self.wrote(id)?;
        self.graph.jump(id, parse_value(value, &self.context())?, now)?;
        self.observe(now);
        Ok(())
    }

    /// With no `transition`, the tween every animation defaults to. An array
    /// target is keyframes unless it is one value of the kind the source holds
    /// (a list of numbers, a list of shadows); `null` in it stands for the
    /// value before it, or the source's own value when it comes first.
    pub(crate) fn animate(
        &mut self,
        id: f64,
        target: &Value,
        transition: Option<&Value>,
        now: Instant,
    ) -> Result<(), String> {
        let transition = match transition {
            Some(transition) => Transition::parse(transition.clone())?,
            None => Transition::default(),
        };
        let id = self.wrote(id)?;
        let holds = self.graph.get(id, now)?;
        match parse_keyframes(target, &holds, &self.context())? {
            Keyframes::Single(target) => self.graph.animate(id, target, transition, now)?,
            Keyframes::Many(frames) => self.graph.animate_keyframes(id, frames, transition, now)?,
        }
        self.observe(now);
        Ok(())
    }

    pub(crate) fn stop(&mut self, id: f64, now: Instant) -> Result<(), String> {
        let id = self.wrote(id)?;
        self.graph.stop(id, now)?;
        self.observe(now);
        Ok(())
    }

    pub(crate) fn pause(&mut self, id: f64, now: Instant) -> Result<(), String> {
        self.command(id, now, |graph, id| graph.pause(id, now))
    }

    pub(crate) fn play(&mut self, id: f64, now: Instant) -> Result<(), String> {
        self.command(id, now, |graph, id| graph.play(id, now))
    }

    pub(crate) fn seek(&mut self, id: f64, seconds: f64, now: Instant) -> Result<(), String> {
        self.command(id, now, |graph, id| graph.seek(id, seconds, now))
    }

    pub(crate) fn set_speed(&mut self, id: f64, speed: f64, now: Instant) -> Result<(), String> {
        self.command(id, now, |graph, id| graph.set_speed(id, speed, now))
    }

    pub(crate) fn complete(&mut self, id: f64, now: Instant) -> Result<(), String> {
        self.command(id, now, |graph, id| graph.complete(id, now))
    }

    /// A control on the animation of a value. The events up to `now` are found first: a finish
    /// that no frame saw would otherwise be undone by `play` or `seek` before anyone heard of it.
    fn command(
        &mut self,
        id: f64,
        now: Instant,
        run: impl FnOnce(&mut MotionValues, ValueId) -> Result<(), String>,
    ) -> Result<(), String> {
        let id = self.wrote(id)?;
        self.observe(now);
        run(&mut self.graph, id)?;
        self.observe(now);
        Ok(())
    }

    /// `None` for a value that has not been animated, or whose animation was stopped.
    pub(crate) fn playback(&mut self, id: f64, now: Instant) -> Result<Option<PlaybackInfo>, String> {
        self.graph.playback(value_id(id)?, now)
    }

    /// Chooses which events a value delivers. Neither removes the listener.
    pub(crate) fn listen(
        &mut self,
        id: f64,
        change: bool,
        animation: bool,
        now: Instant,
    ) -> Result<(), String> {
        let id = value_id(id)?;
        if !change && !animation {
            self.watches.remove(&id);
            return Ok(());
        }
        if let Some(watch) = self.watches.get_mut(&id) {
            watch.change = change;
            watch.animation = animation;
            return Ok(());
        }
        let sample = self.graph.sample(id, now)?;
        self.watches.insert(
            id,
            Watch {
                change,
                animation,
                reported: sample.value,
                was_active: sample.active,
            },
        );
        Ok(())
    }

    /// Samples everything something depends on at `now`: the values the
    /// elements' `motionValues` bind and the values with a listener.
    ///
    /// `read` names values to sample for the renderer's own use, not bound to
    /// a style property (see [`Self::sampled`]). One that has no node yet is
    /// skipped without a word.
    pub(crate) fn frame<'a>(
        &mut self,
        bindings: impl IntoIterator<Item = &'a Value>,
        read: &[f64],
        now: Instant,
    ) -> MotionValueFrame {
        self.frame_values.clear();
        self.frame_moving.clear();
        self.read_moving = false;
        self.read_ids.clear();
        self.read_moving_ids.clear();
        self.layout_driven.clear();
        self.transform_driven.clear();
        self.written.clear();
        let mut active = self.sample_unsampled(bindings, read, now);
        active |= self.observe(now);
        MotionValueFrame {
            active,
            events: std::mem::take(&mut self.pending),
        }
    }

    /// What a tick does where a frame samples every value: the values that moved by themselves
    /// when last sampled, and the ones a write since can have changed, are sampled again, and
    /// every other value is what the frame left it, since nothing but time and a write moves a
    /// value. For a tick in which the bindings, and the values read for the renderer's own use,
    /// are the ones of that frame.
    ///
    /// A value that only elements which `asleep` holds of are bound to is left as it is, moving:
    /// nobody sees it, and the frame that does samples it.
    pub(crate) fn step(&mut self, now: Instant, index: &BindingIndex, asleep: &dyn Fn(u64) -> bool) -> MotionValueFrame {
        let (unseen, seen): (Vec<ValueId>, Vec<ValueId>) =
            self.frame_moving.iter().copied().partition(|id| index.bound_only_to(id, asleep));
        let mut again: Vec<ValueId> = seen.into_iter().chain(self.read_moving_ids.iter().copied()).collect();
        if !self.written.is_empty() {
            again.extend(self.frame_values.keys().copied().filter(|id| self.graph.follows_any(*id, &self.written)));
        }
        again.sort_unstable_by_key(|id| id.0);
        again.dedup();
        self.read_moving_ids.clear();
        self.frame_moving.clear();
        self.frame_moving.extend(unseen);
        self.read_moving = false;
        self.written.clear();
        let mut active = false;
        for id in again {
            crate::perf_counters::motion_value_sampled();
            match self.graph.sample(id, now) {
                Ok(sample) => {
                    active |= sample.moving;
                    if sample.moving {
                        if self.read_ids.contains(&id) {
                            self.read_moving = true;
                            self.read_moving_ids.insert(id);
                        } else {
                            self.frame_moving.insert(id);
                        }
                    }
                    self.frame_values.insert(id, sample.value);
                }
                Err(error) => self.warn(error),
            }
        }
        active |= self.observe(now);
        MotionValueFrame {
            active,
            events: std::mem::take(&mut self.pending),
        }
    }

    /// Samples the values `bindings` and `read` name that this frame has not
    /// sampled yet, and says whether one is still moving. A virtual row mounts
    /// after the root's [`Self::frame`], so the values only it binds are read
    /// here, at the row's own instant. The ones the frame already holds keep
    /// the frame's instant, so every element still sees one value per frame.
    pub(crate) fn sample_unsampled<'a>(
        &mut self,
        bindings: impl IntoIterator<Item = &'a Value>,
        read: &[f64],
        now: Instant,
    ) -> bool {
        let mut active = false;
        let mut bound = Vec::new();
        let mut parsed = Vec::new();
        for binding in bindings {
            match parse_binding(binding) {
                Ok(pairs) => {
                    bound.extend(pairs.iter().map(Bound::id));
                    parsed.push(pairs);
                }
                Err(error) => self.warn(error),
            }
        }
        for pairs in &parsed {
            self.layout_driven
                .extend(pairs.iter().filter(|bound| !bound.paints_only()).map(Bound::id));
            self.transform_driven
                .extend(pairs.iter().filter(|bound| bound.drives_transform()).map(Bound::id));
        }
        bound.sort_unstable_by_key(|id| id.0);
        bound.dedup();

        for id in bound {
            if self.frame_values.contains_key(&id) {
                continue;
            }
            crate::perf_counters::motion_value_sampled();
            match self.graph.sample(id, now) {
                Ok(sample) => {
                    active |= sample.moving;
                    if sample.moving {
                        self.frame_moving.insert(id);
                    }
                    self.frame_values.insert(id, sample.value);
                }
                Err(error) => self.warn(error),
            }
        }
        let lengths = LengthContext::initial(self.viewport.0, self.viewport.1);
        let rejected: Vec<String> = parsed
            .iter()
            .flatten()
            .filter_map(|bound| match (bound, self.frame_values.get(&bound.id())) {
                (Bound::Shorthand { name, props, .. }, Some(AnimValue::List(list))) => {
                    split_for(name, props, list, &lengths).err()
                }
                _ => None,
            })
            .collect();
        for error in rejected {
            self.warn(error);
        }
        for id in read {
            let Ok(id) = value_id(*id) else {
                continue;
            };
            self.layout_driven.insert(id);
            self.read_ids.insert(id);
            if self.frame_values.contains_key(&id) {
                continue;
            }
            if let Ok(sample) = self.graph.sample(id, now) {
                active |= sample.moving;
                self.read_moving |= sample.moving;
                if sample.moving {
                    self.read_moving_ids.insert(id);
                }
                self.frame_values.insert(id, sample.value);
            }
        }
        active
    }

    /// Whether a value bound to `opacity` by `binding` was changing by itself when the frame sampled
    /// it: an animation of the opacity is current on the element.
    pub(crate) fn animates_opacity(&self, binding: &Value) -> bool {
        parse_binding(binding).is_ok_and(|bound| {
            bound.iter().any(|bound| match bound {
                Bound::Longhand { prop: AnimProp::Opacity, id } => self.frame_moving.contains(id),
                Bound::Longhand { .. } => false,
                Bound::Shorthand { props, id, .. } => {
                    props.contains(&AnimProp::Opacity) && self.frame_moving.contains(id)
                }
            })
        })
    }

    /// Whether a value bound to a key of the transform by `binding` was changing by itself when the
    /// frame sampled it.
    pub(crate) fn animates_transform(&self, binding: &Value) -> bool {
        parse_binding(binding).is_ok_and(|bound| {
            bound.iter().any(|bound| match bound {
                Bound::Longhand { prop, id } => prop.is_transform() && self.frame_moving.contains(id),
                Bound::Shorthand { .. } => false,
            })
        })
    }

    /// The number the last frame sampled for a value passed as `read`.
    pub(crate) fn sampled(&self, id: f64) -> Option<f64> {
        match self.frame_values.get(&value_id(id).ok()?)? {
            AnimValue::Number(number) => Some(*number),
            _ => None,
        }
    }

    /// Lays the bound values the last frame sampled over `style`, the style of an
    /// element whose parent has the font `parent`. A multi-value string bound to a
    /// shorthand gives each longhand its own component. `em` and `rem` resolve here,
    /// against this element, so one value bound to two elements can be two sizes.
    pub(crate) fn apply(&self, binding: &Value, style: &mut StyleDesc, parent: &ParentValues) {
        self.apply_in(binding, style, parent, Scope::Element);
    }

    /// [`Self::apply`] for the style of something that is not an element: a property
    /// `scope` cannot move is left out, as JS refuses to bind one there.
    pub(crate) fn apply_in(&self, binding: &Value, style: &mut StyleDesc, parent: &ParentValues, scope: Scope) {
        let Ok(mut bound) = parse_binding(binding) else {
            return;
        };
        bound.retain(|bound| match bound {
            Bound::Longhand { prop, .. } => scope.animates(*prop),
            Bound::Shorthand { props, .. } => props.iter().all(|prop| scope.animates(*prop)),
        });
        // The `em` of a font size is the parent's and every other `em` is of the
        // element's own size, so the size is applied first.
        let (sizes, others): (Vec<&Bound>, Vec<&Bound>) = bound
            .iter()
            .partition(|bound| matches!(bound, Bound::Longhand { prop: AnimProp::FontSize, .. }));
        for bound in sizes {
            let Some(value) = self.frame_values.get(&bound.id()) else {
                continue;
            };
            let value = match value {
                AnimValue::Relative { px, percent, em, rem } => {
                    let declared = LinearLength { px: *px, percent: *percent, em: *em, rem: *rem, ..Default::default() };
                    AnimValue::Number(parent.lengths.descend(Some(&declared), parent.above_root).font_size)
                }
                other => other.clone(),
            };
            if AnimProp::FontSize.accepts(&value) {
                AnimProp::FontSize.write(style, AnimProp::FontSize.clamp_value(value));
            }
        }
        let lengths = parent.lengths_of(Some(style));
        let write = |style: &mut StyleDesc, prop: AnimProp, value: &AnimValue| {
            let value = value.resolved_in(&lengths);
            if prop.accepts(&value) {
                prop.write(style, prop.clamp_value(value));
            }
        };
        for bound in others {
            let Some(value) = self.frame_values.get(&bound.id()) else {
                continue;
            };
            match (bound, value) {
                (Bound::Shorthand { name, props, .. }, AnimValue::List(list)) => {
                    if let Ok(values) = split_for(name, props, list, &lengths) {
                        for (prop, value) in props.iter().zip(&values) {
                            write(style, *prop, value);
                        }
                    }
                }
                (Bound::Shorthand { props, .. }, value) => {
                    for prop in props.iter() {
                        write(style, *prop, value);
                    }
                }
                (Bound::Longhand { prop, .. }, value) => write(style, *prop, value),
            }
        }
    }

    /// Whether any listened value is moving. Events it finds wait in `pending`. A
    /// held animation starts no frame and has not completed: the events follow
    /// whether one is in effect.
    fn observe(&mut self, now: Instant) -> bool {
        let mut active = false;
        self.watched_moving.clear();
        let mut failed = Vec::new();
        let mut ids: Vec<ValueId> = self.watches.keys().copied().collect();
        ids.sort_unstable_by_key(|id| id.0);
        for id in ids {
            let sample = match self.graph.sample(id, now) {
                Ok(sample) => sample,
                Err(error) => {
                    failed.push(error);
                    continue;
                }
            };
            active |= sample.moving;
            if sample.moving {
                self.watched_moving.insert(id);
            }
            let Some(watch) = self.watches.get_mut(&id) else {
                continue;
            };
            if watch.change && sample.value != watch.reported {
                match to_json(&sample.value).and_then(|json| {
                    serde_json::to_string(&json).map_err(|error| error.to_string())
                }) {
                    Ok(text) => self.pending.push(MotionValueEvent {
                        id: id.0,
                        kind: MotionValueEventKind::Change,
                        value: Some(text),
                    }),
                    Err(error) => failed.push(error),
                }
            }
            watch.reported = sample.value;
            if watch.animation && sample.active != watch.was_active {
                self.pending.push(MotionValueEvent {
                    id: id.0,
                    kind: if sample.active {
                        MotionValueEventKind::AnimationStart
                    } else {
                        MotionValueEventKind::AnimationComplete
                    },
                    value: None,
                });
            }
            watch.was_active = sample.active;
        }
        for error in failed {
            self.warn(error);
        }
        active
    }

    /// A bad binding or a value that cannot be read is found again on every
    /// frame, so each message is logged once.
    fn warn(&mut self, message: String) {
        if self.warned.insert(message.clone()) {
            log::warn!("{message}");
        }
    }
}

fn value_id(id: f64) -> Result<ValueId, String> {
    if id.fract() != 0.0 || id < 0.0 || id > f64::from(u32::MAX) {
        return Err(format!("a {NAME} id must be a whole number up to {}, got {id}", u32::MAX));
    }
    Ok(ValueId(id as u32))
}

/// What a value's strings are read against.
struct ValueContext {
    colors: ColorContext,
    lengths: LengthContext,
}

/// A number, a CSS length, colour, angle (`"0.25turn"`, kept in degrees), CSS transform list
/// or `fontVariationSettings` string, a
/// multi-value string of those (`0.5em 0.75em`, `10px / 20px`, `red blue`), a list
/// of numbers, or a `boxShadow` layer or list. A length string (`20px`, `50%`, `2em`,
/// `calc(100% - 20px)`) is its px and its share of the percentage basis, as in a
/// motion property: one with only one of the two reads as the number or the
/// percentage.
fn parse_value(value: &Value, context: &ValueContext) -> Result<AnimValue, String> {
    match value {
        Value::Number(number) => {
            let number = number
                .as_f64()
                .ok_or_else(|| format!("a {NAME} number must fit a double"))?;
            Ok(AnimValue::Number(check_finite(NAME, number)?))
        }
        Value::String(text) => {
            if let Some(value) = parse_component(text, context)? {
                return Ok(value);
            }
            if let Some(degrees) = parse_degrees(text) {
                return Ok(AnimValue::Angle(check_finite(NAME, degrees)?));
            }
            if let Ok(settings) = VariationSettings::try_from(text.clone()) {
                return Ok(AnimValue::Variations(settings.axes()));
            }
            // Last, so that a colour or a length that looks like a function is not read as one.
            match parse_transform(text, &context.lengths) {
                Some(TransformValue::None) => return Ok(AnimValue::Transform(Vec::new())),
                Some(TransformValue::List(functions)) => return Ok(AnimValue::Transform(functions)),
                None => {}
            }
            if split_slash_tokens(text).len() < 2 {
                return Err(format!(
                    "a {NAME} string must be a colour, a CSS length, a CSS angle or a CSS transform: {text}"
                ));
            }
            ListValue::parse(text, |token| parse_component(token, context))
                .map(AnimValue::List)
                .map_err(|error| format!("a {NAME} string must be a colour, a CSS length or a list of them: {error}"))
        }
        Value::Array(items) if !items.is_empty() && items.iter().all(Value::is_number) => {
            items
                .iter()
                .map(|item| {
                    let number = item.as_f64().ok_or_else(|| format!("a {NAME} number must fit a double"))?;
                    check_finite(NAME, number)
                })
                .collect::<Result<_, _>>()
                .map(AnimValue::Numbers)
        }
        Value::Array(_) | Value::Object(_) => {
            parse_shadow_list(NAME, value, &context.colors).map(AnimValue::Shadows)
        }
        other => Err(format!("a {NAME} cannot hold {other}")),
    }
}

/// One length or colour, which is a whole string or a component of a list.
fn parse_component(text: &str, context: &ValueContext) -> Result<Option<AnimValue>, String> {
    // A plain percentage stays one, so `0%` is not read as the number 0.
    if let Some(percent) = text.strip_suffix('%').and_then(|percent| percent.trim().parse::<f64>().ok()) {
        return Ok(Some(AnimValue::Percent(check_finite(NAME, percent / 100.0)?)));
    }
    if let Some(length) = parse_linear_length(text) {
        let (px, percent) = context.lengths.split(&LinearLength { em: 0.0, rem: 0.0, ..length });
        return Ok(Some(AnimValue::relative(
            check_finite(NAME, px)?,
            check_finite(NAME, percent)?,
            check_finite(NAME, length.em)?,
            check_finite(NAME, length.rem)?,
        )));
    }
    Ok(Color::parse(text, &context.colors).map(AnimValue::Color))
}

enum Keyframes {
    Single(AnimValue),
    Many(Vec<Option<AnimValue>>),
}

/// Whether `target` is one value or a list of keyframes, by what the source
/// holds: a list of numbers and a list of shadow layers are values, so for
/// those an array is only keyframes when it is not such a list.
fn parse_keyframes(
    target: &Value,
    holds: &AnimValue,
    context: &ValueContext,
) -> Result<Keyframes, String> {
    let Value::Array(items) = target else {
        return parse_value(target, context).map(Keyframes::Single);
    };
    let is_value = match holds {
        AnimValue::Numbers(_) => items.iter().all(Value::is_number),
        AnimValue::Shadows(_) => items.iter().all(Value::is_object),
        _ => false,
    };
    if is_value {
        return parse_value(target, context).map(Keyframes::Single);
    }
    let frames = items
        .iter()
        .map(|item| match item {
            Value::Null => Ok(None),
            item => parse_value(item, context).map(Some),
        })
        .collect::<Result<Vec<_>, _>>()?;
    match frames.as_slice() {
        [] => Err(format!("{NAME} keyframes need at least one value")),
        [Some(only)] => Ok(Keyframes::Single(only.clone())),
        [None] => Err(format!("{NAME} keyframes need a value besides null")),
        _ => Ok(Keyframes::Many(frames)),
    }
}

fn to_json(value: &AnimValue) -> Result<Value, String> {
    let number = |number: f64| {
        serde_json::Number::from_f64(number)
            .map(Value::Number)
            .ok_or_else(|| format!("a {NAME} reached a number JSON cannot hold"))
    };
    match value {
        AnimValue::Number(value) => number(*value),
        AnimValue::Percent(fraction) => Ok(Value::String(format!("{}%", fraction * 100.0))),
        AnimValue::Calc { px, percent } => Ok(Value::String(
            LinearLength { px: *px, percent: *percent, ..Default::default() }.to_string(),
        )),
        AnimValue::Relative { px, percent, em, rem } => Ok(Value::String(
            LinearLength { px: *px, percent: *percent, em: *em, rem: *rem, ..Default::default() }.to_string(),
        )),
        AnimValue::Color(color) => Ok(Value::String(color.to_css())),
        AnimValue::List(list) => list.to_css().map(Value::String),
        AnimValue::Shadows(shadows) => serde_json::to_value(shadow_layers(shadows.clone()))
            .map_err(|error| error.to_string()),
        AnimValue::Numbers(values) => values
            .iter()
            .map(|value| number(*value))
            .collect::<Result<_, _>>()
            .map(Value::Array),
        AnimValue::Variations(axes) => {
            Ok(Value::String(VariationSettings::from_axes(axes).into()))
        }
        AnimValue::Angle(degrees) => Ok(Value::String(format!("{degrees}deg"))),
        AnimValue::Transform(functions) => Ok(Value::String(format_list(functions))),
        AnimValue::Ellipse { .. } | AnimValue::Path(_) | AnimValue::Auto | AnimValue::Keyword(_) => {
            Err(format!("a {NAME} cannot hold {value:?}"))
        }
    }
}

fn parse_transform_options(options: Option<&Value>) -> Result<TransformOptions, String> {
    let Some(options) = options.filter(|options| !options.is_null()) else {
        return Ok(TransformOptions::default());
    };
    let raw: RawTransformOptions = serde_json::from_value(options.clone())
        .map_err(|error| format!("{NAME} transform options: {error}"))?;
    let ease = match raw.ease {
        None => Vec::new(),
        // Four numbers are one cubic bezier; a list of easings has one per segment.
        Some(Value::Array(items)) if !items.iter().all(Value::is_number) => {
            items.into_iter().map(parse_ease).collect::<Result<_, _>>()?
        }
        Some(single) => vec![parse_ease(single)?],
    };
    Ok(TransformOptions {
        clamp: raw.clamp.unwrap_or(true),
        ease,
    })
}

fn parse_spring(config: &Value) -> Result<Transition, String> {
    let mut config = match config {
        Value::Object(config) => config.clone(),
        Value::Null => Map::new(),
        _ => return Err(format!("a {NAME} spring config must be an object")),
    };
    config
        .entry("type")
        .or_insert_with(|| Value::String("spring".to_string()));
    let transition = Transition::parse(Value::Object(config))?;
    match transition.curve {
        Curve::Spring(_) => Ok(transition),
        Curve::Tween { .. } => Err(format!("a {NAME} spring needs a spring transition")),
    }
}

/// A style property a motion value drives.
enum Bound {
    Longhand { prop: AnimProp, id: ValueId },
    /// A shorthand, which gives each of its longhands a component of a multi-value
    /// string and the whole of any other value.
    Shorthand { name: String, props: &'static [AnimProp], id: ValueId },
}

impl Bound {
    fn id(&self) -> ValueId {
        match self {
            Self::Longhand { id, .. } | Self::Shorthand { id, .. } => *id,
        }
    }

    fn paints_only(&self) -> bool {
        match self {
            Self::Longhand { prop, .. } => prop.paints_only(),
            Self::Shorthand { props, .. } => props.iter().all(|prop| prop.paints_only()),
        }
    }

    fn drives_transform(&self) -> bool {
        match self {
            Self::Longhand { prop, .. } => prop.is_transform_key(),
            Self::Shorthand { props, .. } => props.iter().any(|prop| prop.is_transform_key()),
        }
    }
}

/// `{ opacity: 3, padding: 4 }`: style properties to the values that drive them.
fn parse_binding(binding: &Value) -> Result<Vec<Bound>, String> {
    let map = binding
        .as_object()
        .ok_or_else(|| "motionValues must be an object of property to motion value".to_string())?;
    let mut bound = Vec::new();
    for (name, id) in map {
        let id = value_id(
            id.as_f64()
                .ok_or_else(|| format!("motionValues.{name} must be a motion value id"))?,
        )?;
        if AnimProp::is_depth_key(name) {
            continue;
        }
        match (AnimProp::from_name(name), expand_shorthand(name)) {
            (Some(prop), _) => bound.push(Bound::Longhand { prop, id }),
            (None, Some(_)) if name == "flex" => {
                return Err("motionValues cannot bind flex: bind flexGrow, flexShrink and flexBasis".to_string());
            }
            (None, Some(props)) => bound.push(Bound::Shorthand { name: name.clone(), props, id }),
            (None, None) => return Err(format!("motionValues cannot bind {name}")),
        }
    }
    Ok(bound)
}

/// What each longhand of the shorthand `name` takes from a multi-value string,
/// or why the string is no value of it.
fn split_for(
    name: &str,
    props: &[AnimProp],
    list: &ListValue,
    lengths: &LengthContext,
) -> Result<Vec<AnimValue>, String> {
    let rejected = |reason: String| {
        format!("motionValues.{name} cannot take {}: {reason}", list.to_css().unwrap_or_default())
    };
    let resolved = ListValue {
        items: list.items.iter().map(|item| item.resolved_in(lengths)).collect(),
        slash: list.slash,
    };
    let values = split_list(name, resolved.horizontal(), resolved.vertical()).map_err(rejected)?;
    match props.iter().zip(&values).find(|(prop, value)| !prop.accepts(value)) {
        Some((prop, _)) => Err(rejected(format!("{} takes no such component", prop.name()))),
        None => Ok(values),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;

    use super::*;
    use crate::css::length::LinearLength;
    use crate::style::CornerRadiusValue;

    struct Clock(Instant);

    impl Clock {
        fn at(&self, millis: u64) -> Instant {
            self.0 + Duration::from_millis(millis)
        }
    }

    fn clock() -> Clock {
        Clock(Instant::now())
    }

    fn linear(seconds: f64) -> Value {
        json!({ "duration": seconds, "ease": "linear" })
    }

    fn kinds(frame: &MotionValueFrame) -> Vec<(u32, MotionValueEventKind)> {
        frame.events.iter().map(|event| (event.id, event.kind)).collect()
    }

    #[test]
    fn values_round_trip_through_json() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(0.0, &json!(2.5)).unwrap();
        assert_eq!(store.get(0.0, clock.at(0)).unwrap(), json!(2.5));

        store.create_source(1.0, &json!("50%")).unwrap();
        assert_eq!(store.get(1.0, clock.at(0)).unwrap(), json!("50%"));

        store.create_source(2.0, &json!([1, 2, 3])).unwrap();
        assert_eq!(store.get(2.0, clock.at(0)).unwrap(), json!([1.0, 2.0, 3.0]));

        let shadow = json!({ "offsetX": 0, "offsetY": 2, "blurRadius": 4, "spreadRadius": 0, "color": "#000000" });
        store.create_source(3.0, &json!([shadow])).unwrap();
        let read = store.get(3.0, clock.at(0)).unwrap();
        assert_eq!(read[0]["offsetY"], json!(2.0));
        assert_eq!(read[0]["blurRadius"], json!(4.0));
        assert_eq!(read[0]["inset"], json!(false));

        store.create_source(4.0, &json!("#ff0000")).unwrap();
        assert!(store.get(4.0, clock.at(0)).unwrap().as_str().unwrap().starts_with("rgba("));
    }

    #[test]
    fn values_it_cannot_hold_are_rejected() {
        let mut store = MotionValueStore::default();
        for value in [json!(null), json!(true), json!("auto"), json!("not a colour")] {
            assert!(store.create_source(0.0, &value).is_err(), "{value}");
        }
        assert!(store.create_source(-1.0, &json!(0)).is_err());
        assert!(store.create_source(0.5, &json!(0)).is_err());
        store.create_source(0.0, &json!(0)).unwrap();
        assert!(store.create_source(0.0, &json!(1)).is_err());
    }

    #[test]
    fn animate_takes_the_transition_as_json() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!(0)).unwrap();
        store.animate(1.0, &json!(100), Some(&linear(1.0)), clock.at(0)).unwrap();
        assert_eq!(store.get(1.0, clock.at(250)).unwrap(), json!(25.0));
        assert!(store.is_animating(1.0, clock.at(250)).unwrap());
        assert_eq!(store.velocity(1.0, clock.at(250)).unwrap(), 100.0);
        store.stop(1.0, clock.at(500)).unwrap();
        assert_eq!(store.get(1.0, clock.at(900)).unwrap(), json!(50.0));
        assert!(store
            .animate(1.0, &json!(1), Some(&json!({ "bogus": 1 })), clock.at(900))
            .is_err());
    }

    fn number_at(store: &mut MotionValueStore, id: f64, at: Instant) -> f64 {
        store.get(id, at).unwrap().as_f64().unwrap()
    }

    fn close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-6, "expected {expected}, got {actual}");
    }

    fn keyframes_store() -> (Clock, MotionValueStore) {
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!(0)).unwrap();
        (clock(), store)
    }

    #[test]
    fn keyframes_are_spaced_evenly_or_by_times() {
        let (clock, mut store) = keyframes_store();
        store.animate(1.0, &json!([0, 100, 40]), Some(&linear(1.0)), clock.at(0)).unwrap();
        close(number_at(&mut store, 1.0, clock.at(250)), 50.0);
        close(number_at(&mut store, 1.0, clock.at(500)), 100.0);
        close(number_at(&mut store, 1.0, clock.at(750)), 70.0);
        assert_eq!(number_at(&mut store, 1.0, clock.at(1000)), 40.0);
        assert!(!store.is_animating(1.0, clock.at(1000)).unwrap());

        let timed = json!({ "duration": 1.0, "ease": "linear", "times": [0, 0.25, 1] });
        store.animate(1.0, &json!([0, 100, 40]), Some(&timed), clock.at(2000)).unwrap();
        close(number_at(&mut store, 1.0, clock.at(2125)), 50.0);
        close(number_at(&mut store, 1.0, clock.at(2250)), 100.0);
        close(number_at(&mut store, 1.0, clock.at(2625)), 70.0);
    }

    #[test]
    fn a_leading_null_starts_from_the_value_and_a_later_one_repeats_the_one_before() {
        let (clock, mut store) = keyframes_store();
        store.set(1.0, &json!(10), clock.at(0)).unwrap();
        store.animate(1.0, &json!([null, 50, null, 90]), Some(&linear(3.0)), clock.at(100)).unwrap();
        close(number_at(&mut store, 1.0, clock.at(100)), 10.0);
        close(number_at(&mut store, 1.0, clock.at(600)), 30.0);
        // The held keyframe: 50 to 50 between 1s and 2s.
        close(number_at(&mut store, 1.0, clock.at(1600)), 50.0);
        close(number_at(&mut store, 1.0, clock.at(2600)), 70.0);

        // Without a null the track opens on its own first keyframe.
        store.animate(1.0, &json!([20, 60]), Some(&linear(1.0)), clock.at(5000)).unwrap();
        close(number_at(&mut store, 1.0, clock.at(5000)), 20.0);
        close(number_at(&mut store, 1.0, clock.at(5500)), 40.0);

        assert!(store.animate(1.0, &json!([null]), None, clock.at(6000)).is_err());
        store.animate(1.0, &json!([75]), Some(&linear(1.0)), clock.at(6000)).unwrap();
        assert_eq!(number_at(&mut store, 1.0, clock.at(7000)), 75.0);
    }

    #[test]
    fn an_ease_per_segment_applies_to_its_own_segment() {
        let (clock, mut store) = keyframes_store();
        let eases = json!({ "duration": 1.0, "ease": ["linear", "step-end"] });
        store.animate(1.0, &json!([0, 10, 20]), Some(&eases), clock.at(0)).unwrap();
        close(number_at(&mut store, 1.0, clock.at(250)), 5.0);
        close(number_at(&mut store, 1.0, clock.at(750)), 10.0);
        assert_eq!(number_at(&mut store, 1.0, clock.at(1000)), 20.0);

        let wrong = json!({ "duration": 1.0, "ease": ["linear"] });
        assert!(store.animate(1.0, &json!([0, 10, 20]), Some(&wrong), clock.at(2000)).is_err());
    }

    #[test]
    fn infinity_repeats_for_ever_and_never_completes() {
        let (clock, mut store) = keyframes_store();
        store.listen(1.0, false, true, clock.at(0)).unwrap();
        let looping = json!({ "duration": 1.0, "ease": "linear", "repeat": "Infinity" });
        store.animate(1.0, &json!([0, 100]), Some(&looping), clock.at(0)).unwrap();

        let mut events = Vec::new();
        for second in 0..60u64 {
            let frame = store.frame([], &[], clock.at(second * 1000 + 500));
            assert!(frame.active, "second {second}");
            events.extend(kinds(&frame));
        }
        assert_eq!(events, vec![(1, MotionValueEventKind::AnimationStart)]);
        close(number_at(&mut store, 1.0, clock.at(3_600_250)), 25.0);
        assert!(store.is_animating(1.0, clock.at(3_600_250)).unwrap());
    }

    #[test]
    fn a_single_target_repeats_too() {
        let (clock, mut store) = keyframes_store();
        let looping = json!({ "duration": 1.0, "ease": "linear", "repeat": "Infinity" });
        store.animate(1.0, &json!(100), Some(&looping), clock.at(0)).unwrap();
        close(number_at(&mut store, 1.0, clock.at(2250)), 25.0);
        assert!(store
            .animate(1.0, &json!(100), Some(&json!({ "repeat": "forever" })), clock.at(0))
            .is_err());
        assert!(store
            .animate(1.0, &json!(100), Some(&json!({ "repeat": -1 })), clock.at(0))
            .is_err());
    }

    #[test]
    fn a_counted_repeat_ends_on_the_last_iteration() {
        let (clock, mut store) = keyframes_store();
        store.listen(1.0, false, true, clock.at(0)).unwrap();
        let twice = json!({ "duration": 1.0, "ease": "linear", "repeat": 1, "repeatDelay": 1.0 });
        store.animate(1.0, &json!([0, 100]), Some(&twice), clock.at(0)).unwrap();
        close(number_at(&mut store, 1.0, clock.at(1500)), 100.0);
        close(number_at(&mut store, 1.0, clock.at(2500)), 50.0);
        assert!(store.is_animating(1.0, clock.at(2500)).unwrap());
        let frame = store.frame([], &[], clock.at(4000));
        assert_eq!(
            kinds(&frame),
            vec![
                (1, MotionValueEventKind::AnimationStart),
                (1, MotionValueEventKind::AnimationComplete)
            ]
        );
        assert_eq!(number_at(&mut store, 1.0, clock.at(4000)), 100.0);
    }

    #[test]
    fn reverse_runs_time_backwards_and_mirror_runs_the_keyframes_backwards() {
        let looping = |kind: &str| {
            json!({
                "duration": 1.0,
                "ease": "linear",
                "times": [0, 0.25, 1],
                "repeat": 1,
                "repeatType": kind
            })
        };
        let (clock, mut store) = keyframes_store();
        store.animate(1.0, &json!([0, 100, 40]), Some(&looping("loop")), clock.at(0)).unwrap();
        close(number_at(&mut store, 1.0, clock.at(1125)), 50.0);

        // Time runs back: at 0.25 into the second pass the first sits at 0.75.
        store.animate(1.0, &json!([0, 100, 40]), Some(&looping("reverse")), clock.at(10_000)).unwrap();
        close(number_at(&mut store, 1.0, clock.at(11_250)), 60.0);
        assert_eq!(number_at(&mut store, 1.0, clock.at(12_000)), 0.0);

        // The keyframes run back with the same times: 40, 100, 0.
        store.animate(1.0, &json!([0, 100, 40]), Some(&looping("mirror")), clock.at(20_000)).unwrap();
        close(number_at(&mut store, 1.0, clock.at(21_000)), 40.0);
        close(number_at(&mut store, 1.0, clock.at(21_250)), 100.0);
        assert_eq!(number_at(&mut store, 1.0, clock.at(22_000)), 0.0);
    }

    #[test]
    fn stopping_mid_loop_holds_the_value_and_ends_the_animation() {
        let (clock, mut store) = keyframes_store();
        store.listen(1.0, false, true, clock.at(0)).unwrap();
        let looping = json!({ "duration": 1.0, "ease": "linear", "repeat": "Infinity", "repeatType": "mirror" });
        store.animate(1.0, &json!([0, 100]), Some(&looping), clock.at(0)).unwrap();
        assert_eq!(kinds(&store.frame([], &[], clock.at(1000))), vec![(1, MotionValueEventKind::AnimationStart)]);

        // Second pass, running back down.
        close(number_at(&mut store, 1.0, clock.at(1250)), 75.0);
        store.stop(1.0, clock.at(1250)).unwrap();
        close(number_at(&mut store, 1.0, clock.at(9000)), 75.0);
        assert!(!store.is_animating(1.0, clock.at(9000)).unwrap());
        let frame = store.frame([], &[], clock.at(9016));
        assert_eq!(kinds(&frame), vec![(1, MotionValueEventKind::AnimationComplete)]);
        assert!(!frame.active);

        // The value is free again: a new animation starts from where it stopped.
        store.animate(1.0, &json!([null, 0]), Some(&linear(1.0)), clock.at(10_000)).unwrap();
        close(number_at(&mut store, 1.0, clock.at(10_500)), 37.5);
    }

    #[test]
    fn an_array_is_keyframes_unless_it_is_a_value_the_source_holds() {
        let (clock, mut store) = keyframes_store();
        // A number holds no list, so any array is keyframes.
        store.animate(1.0, &json!([0, 10, 20]), Some(&linear(1.0)), clock.at(0)).unwrap();
        close(number_at(&mut store, 1.0, clock.at(500)), 10.0);

        store.create_source(2.0, &json!([0, 0])).unwrap();
        // A list of numbers is the target of a list-of-numbers source ...
        store.animate(2.0, &json!([10, 20]), Some(&linear(1.0)), clock.at(0)).unwrap();
        assert_eq!(store.get(2.0, clock.at(500)).unwrap(), json!([5.0, 10.0]));
        // ... and a list of lists is its keyframes.
        store.animate(2.0, &json!([[0, 0], [10, 20], null]), Some(&linear(1.0)), clock.at(2000)).unwrap();
        assert_eq!(store.get(2.0, clock.at(2250)).unwrap(), json!([5.0, 10.0]));
        assert_eq!(store.get(2.0, clock.at(2750)).unwrap(), json!([10.0, 20.0]));
        assert!(store
            .animate(2.0, &json!([[0, 0], "#ffffff"]), Some(&linear(1.0)), clock.at(2000))
            .is_err());

        let layer = |blur: f64| {
            json!({ "offsetX": 0, "offsetY": 0, "blurRadius": blur, "spreadRadius": 0, "color": "#000000" })
        };
        store.create_source(3.0, &json!([layer(0.0)])).unwrap();
        store.animate(3.0, &json!([layer(10.0)]), Some(&linear(1.0)), clock.at(0)).unwrap();
        close(store.get(3.0, clock.at(500)).unwrap()[0]["blurRadius"].as_f64().unwrap(), 5.0);
        store
            .animate(3.0, &json!([[layer(0.0)], [layer(20.0)], [layer(10.0)]]), Some(&linear(1.0)), clock.at(2000))
            .unwrap();
        close(store.get(3.0, clock.at(2500)).unwrap()[0]["blurRadius"].as_f64().unwrap(), 20.0);
        close(store.get(3.0, clock.at(2750)).unwrap()[0]["blurRadius"].as_f64().unwrap(), 15.0);
    }

    #[test]
    fn colours_and_percentages_have_keyframes() {
        let (clock, mut store) = keyframes_store();
        store.create_source(2.0, &json!("#000000")).unwrap();
        store.animate(2.0, &json!(["#000000", "#ff0000", "#000000"]), Some(&linear(1.0)), clock.at(0)).unwrap();
        let mid = store.get(2.0, clock.at(500)).unwrap();
        assert!(mid.as_str().unwrap().starts_with("rgba(255.000, 0.000, 0.000"), "{mid}");

        store.create_source(3.0, &json!("0%")).unwrap();
        store.animate(3.0, &json!(["0%", "50%", "100%"]), Some(&linear(1.0)), clock.at(0)).unwrap();
        assert_eq!(store.get(3.0, clock.at(500)).unwrap(), json!("50%"));
        assert!(store.animate(3.0, &json!(["0%", "#fff"]), None, clock.at(0)).is_err());
        // A failed animate leaves the one running.
        assert_eq!(store.get(3.0, clock.at(500)).unwrap(), json!("50%"));
    }

    #[test]
    fn a_percentage_and_a_number_blend_as_a_calc() {
        let (clock, mut store) = keyframes_store();
        store.create_source(2.0, &json!("50%")).unwrap();
        store.animate(2.0, &json!(20), Some(&linear(1.0)), clock.at(0)).unwrap();
        assert_eq!(store.get(2.0, clock.at(500)).unwrap(), json!("calc(25% + 10px)"));
        assert_eq!(store.get(2.0, clock.at(1000)).unwrap(), json!(20.0));
    }

    #[test]
    fn font_variation_settings_are_values_that_blend_per_axis() {
        let (clock, mut store) = keyframes_store();
        store.create_source(2.0, &json!("'wght' 400, 'wdth' 100")).unwrap();
        assert_eq!(store.get(2.0, clock.at(0)).unwrap(), json!("\"wdth\" 100, \"wght\" 400"));
        store
            .animate(2.0, &json!("'wght' 800, 'wdth' 50"), Some(&linear(1.0)), clock.at(0))
            .unwrap();
        assert_eq!(store.get(2.0, clock.at(500)).unwrap(), json!("\"wdth\" 75, \"wght\" 600"));

        // Other axes, and normal, flip halfway.
        store.animate(2.0, &json!("normal"), Some(&linear(1.0)), clock.at(2000)).unwrap();
        assert_eq!(store.get(2.0, clock.at(2499)).unwrap(), json!("\"wdth\" 50, \"wght\" 800"));
        assert_eq!(store.get(2.0, clock.at(2500)).unwrap(), json!("normal"));
        assert!(store.animate(2.0, &json!("#ff0000"), None, clock.at(3000)).is_err());
    }

    #[test]
    fn keyframes_reject_what_the_track_cannot_play() {
        let (clock, mut store) = keyframes_store();
        let spring = json!({ "type": "spring" });
        assert!(store.animate(1.0, &json!([0, 1, 2]), Some(&spring), clock.at(0)).is_err());
        store.animate(1.0, &json!([0, 1]), Some(&spring), clock.at(0)).unwrap();
        assert!(store
            .animate(1.0, &json!([0, 1]), Some(&json!({ "times": [0, 1, 1] })), clock.at(0))
            .is_err());
        assert!(store
            .animate(1.0, &json!([0, 1]), Some(&json!({ "times": [1, 0] })), clock.at(0))
            .is_err());
        assert!(store.animate(1.0, &json!([0, "auto"]), None, clock.at(0)).is_err());
        assert!(store.animate(1.0, &json!([]), None, clock.at(0)).is_err());
    }

    #[test]
    fn a_spring_between_two_keyframes_can_repeat() {
        let (clock, mut store) = keyframes_store();
        let spring = json!({ "type": "spring", "stiffness": 200, "damping": 10, "repeat": "Infinity", "repeatType": "mirror" });
        store.animate(1.0, &json!([0, 100]), Some(&spring), clock.at(0)).unwrap();
        assert!(store.is_animating(1.0, clock.at(600_000)).unwrap());
    }

    #[test]
    fn velocity_is_for_numbers() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!("#000000")).unwrap();
        assert!(store.velocity(1.0, clock.at(0)).is_err());
    }

    #[test]
    fn a_transform_and_a_spring_are_built_from_json() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!(0)).unwrap();
        store
            .create_transform(
                2.0,
                1.0,
                vec![0.0, 100.0],
                &json!(["#000000", "#ffffff"]),
                Some(&json!({ "clamp": false, "ease": "linear" })),
            )
            .unwrap();
        store
            .create_transform(3.0, 1.0, vec![0.0, 100.0], &json!([0, 10]), Some(&json!({ "ease": [0.25, 0.1, 0.25, 1.0] })))
            .unwrap();
        store.set(1.0, &json!(100), clock.at(0)).unwrap();
        assert_eq!(store.get(3.0, clock.at(0)).unwrap(), json!(10.0));

        store.create_follower(4.0, 1.0, &json!({ "stiffness": 300, "damping": 30 }), clock.at(0)).unwrap();
        assert_eq!(store.get(4.0, clock.at(0)).unwrap(), json!(100.0));
        store.set(1.0, &json!(0), clock.at(16)).unwrap();
        // The follower retargets when it is first sampled after the change.
        store.get(4.0, clock.at(16)).unwrap();
        let mid = store.get(4.0, clock.at(48)).unwrap().as_f64().unwrap();
        assert!(mid > 0.0 && mid < 100.0, "{mid}");

        store.create_spring(5.0, &json!(0), &json!({}), clock.at(0)).unwrap();
        store.set(5.0, &json!(10), clock.at(0)).unwrap();
        assert!(store.is_animating(5.0, clock.at(50)).unwrap());
        assert!(store
            .create_spring(6.0, &json!(0), &json!({ "type": "tween" }), clock.at(0))
            .is_err());
    }

    #[test]
    fn an_unbound_unlistened_follower_reads_where_frames_would_have_taken_it() {
        let clock = clock();
        let spring = json!({ "stiffness": 400, "damping": 40 });
        let mut read = MotionValueStore::default();
        let mut framed = MotionValueStore::default();
        for store in [&mut read, &mut framed] {
            store.create_source(1.0, &json!(0)).unwrap();
            store.create_follower(2.0, 1.0, &spring, clock.at(0)).unwrap();
            store.set(1.0, &json!(100), clock.at(0)).unwrap();
        }
        framed.listen(2.0, true, false, clock.at(0)).unwrap();
        for ms in (16..=160).step_by(16) {
            framed.frame([], &[], clock.at(ms));
        }

        let mid = read.get(2.0, clock.at(160)).unwrap().as_f64().unwrap();
        assert!(mid > 1.0 && mid < 100.0, "{mid}");
        assert_eq!(read.get(2.0, clock.at(160)).unwrap(), framed.get(2.0, clock.at(160)).unwrap());
        assert!(read.velocity(2.0, clock.at(160)).unwrap() > 0.0);
        assert!(read.is_animating(2.0, clock.at(160)).unwrap());
        assert_eq!(read.get(2.0, clock.at(60_000)).unwrap(), json!(100.0));
        assert!(!read.is_animating(2.0, clock.at(60_000)).unwrap());
    }

    #[test]
    fn a_listener_hears_changes_once_each() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!(0)).unwrap();
        store.listen(1.0, true, false, clock.at(0)).unwrap();

        assert!(kinds(&store.frame([], &[], clock.at(0))).is_empty());
        store.set(1.0, &json!(5), clock.at(10)).unwrap();
        let frame = store.frame([], &[], clock.at(10));
        assert_eq!(frame.events.len(), 1);
        assert_eq!(frame.events[0].value.as_deref(), Some("5.0"));
        assert!(store.frame([], &[], clock.at(20)).events.is_empty());

        store.listen(1.0, false, false, clock.at(20)).unwrap();
        store.set(1.0, &json!(6), clock.at(30)).unwrap();
        assert!(store.frame([], &[], clock.at(30)).events.is_empty());
    }

    #[test]
    fn animation_start_and_complete_are_found_by_diffing_activity() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!(0)).unwrap();
        store.listen(1.0, false, true, clock.at(0)).unwrap();

        store.animate(1.0, &json!(10), Some(&linear(1.0)), clock.at(0)).unwrap();
        // The clock jumps past the end before the next frame: both are still delivered.
        let frame = store.frame([], &[], clock.at(5000));
        assert_eq!(
            kinds(&frame),
            vec![
                (1, MotionValueEventKind::AnimationStart),
                (1, MotionValueEventKind::AnimationComplete)
            ]
        );
        assert!(!frame.active);
        assert!(store.frame([], &[], clock.at(5016)).events.is_empty());
    }

    #[test]
    fn a_frame_is_active_while_a_bound_or_listened_value_moves() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!(0)).unwrap();
        store.create_source(2.0, &json!(0)).unwrap();
        store.animate(1.0, &json!(10), Some(&linear(1.0)), clock.at(0)).unwrap();
        store.animate(2.0, &json!(10), Some(&linear(1.0)), clock.at(0)).unwrap();

        assert!(!store.frame([], &[], clock.at(100)).active);
        let binding = json!({ "opacity": 1 });
        assert!(store.frame([&binding], &[], clock.at(100)).active);
        assert!(!store.frame([&binding], &[], clock.at(1000)).active);
        store.listen(2.0, true, false, clock.at(500)).unwrap();
        assert!(store.frame([], &[], clock.at(600)).active);
    }

    #[test]
    fn a_held_animation_asks_for_no_frame_and_is_not_complete() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!(0)).unwrap();
        store.listen(1.0, true, true, clock.at(0)).unwrap();
        store.animate(1.0, &json!(10), Some(&linear(1.0)), clock.at(0)).unwrap();
        let binding = json!({ "opacity": 1 });
        let started = store.frame([&binding], &[], clock.at(100));
        assert!(started.active);
        assert_eq!(started.events.iter().map(|event| event.kind).collect::<Vec<_>>(), vec![
            MotionValueEventKind::AnimationStart,
            MotionValueEventKind::Change,
        ]);

        store.pause(1.0, clock.at(400)).unwrap();
        for millis in [400, 1500, 6000] {
            let frame = store.frame([&binding], &[], clock.at(millis));
            assert!(!frame.active, "{millis}: a held value needs no frame");
            assert!(frame.events.iter().all(|event| event.kind != MotionValueEventKind::AnimationComplete));
        }
        assert!(store.is_animating(1.0, clock.at(6000)).unwrap());
        assert_eq!(store.get(1.0, clock.at(6000)).unwrap(), json!(4.0));

        store.play(1.0, clock.at(6000)).unwrap();
        assert!(store.frame([&binding], &[], clock.at(6100)).active);
        let done = store.frame([&binding], &[], clock.at(7000));
        assert!(done.events.iter().any(|event| event.kind == MotionValueEventKind::AnimationComplete));
        assert!(!done.active);
    }

    #[test]
    fn running_an_animation_backwards_to_its_start_completes_it() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!(0)).unwrap();
        store.listen(1.0, false, true, clock.at(0)).unwrap();
        store.animate(1.0, &json!(10), Some(&linear(1.0)), clock.at(0)).unwrap();
        store.frame([], &[], clock.at(100));
        store.set_speed(1.0, -1.0, clock.at(100)).unwrap();
        assert!(store.frame([], &[], clock.at(150)).events.is_empty());
        let end = store.frame([], &[], clock.at(200));
        assert_eq!(end.events.iter().map(|event| event.kind).collect::<Vec<_>>(), vec![
            MotionValueEventKind::AnimationComplete,
        ]);
        assert!(!end.active);
        assert_eq!(store.get(1.0, clock.at(200)).unwrap(), json!(0.0));
    }

    #[test]
    fn bound_values_are_laid_over_a_style() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!(0.5)).unwrap();
        store.create_source(2.0, &json!("#ff0000")).unwrap();
        store.create_source(3.0, &json!(8)).unwrap();
        let binding = json!({ "opacity": 1, "backgroundColor": 2, "padding": 3 });
        store.frame([&binding], &[], clock.at(0));

        let mut style = StyleDesc::default();
        store.apply(&binding, &mut style, &ParentValues::initial());
        assert_eq!(style.opacity, Some(0.5));
        assert!(style.background_color.as_deref().unwrap().starts_with("rgba("));
        assert_eq!(style.padding_top, Some(LinearLength::px(8.0)));
        assert_eq!(style.padding_left, Some(LinearLength::px(8.0)));
    }

    #[test]
    fn a_bound_value_is_clamped_and_a_mismatch_is_skipped() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!(3)).unwrap();
        store.create_source(2.0, &json!("#ff0000")).unwrap();
        let binding = json!({ "opacity": 1 });
        let mismatch = json!({ "opacity": 2 });
        store.frame([&binding, &mismatch], &[], clock.at(0));

        let mut style = StyleDesc::default();
        store.apply(&binding, &mut style, &ParentValues::initial());
        assert_eq!(style.opacity, Some(1.0));

        let mut style = StyleDesc::default();
        store.apply(&mismatch, &mut style, &ParentValues::initial());
        assert_eq!(style.opacity, None);
    }

    #[test]
    fn an_angle_string_is_degrees_for_the_angle_keys_and_no_value_of_another_key() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        for (id, text) in [(1.0, "45deg"), (2.0, "0.5turn"), (3.0, "10grad"), (4.0, "-0.25turn")] {
            store.create_source(id, &json!(text)).unwrap();
        }
        store.create_source(5.0, &json!(1)).unwrap();
        store.create_source(6.0, &json!("1rad")).unwrap();
        assert_eq!(store.get(1.0, clock.at(0)).unwrap(), json!("45deg"));
        assert_eq!(store.get(2.0, clock.at(0)).unwrap(), json!("180deg"));
        assert_eq!(store.get(3.0, clock.at(0)).unwrap(), json!("9deg"));
        assert!(store.create_source(7.0, &json!("45 deg")).is_err());
        assert!(store.create_source(7.0, &json!("45px")).is_ok(), "a length is still a length");

        let binding = json!({ "rotate": 1, "skewX": 2, "rotateZ": 3, "skewY": 4, "x": 6, "opacity": 6 });
        store.frame([&binding], &[], clock.at(0));
        let mut style = StyleDesc::default();
        store.apply(&binding, &mut style, &ParentValues::initial());
        assert_eq!((style.motion_rotate, style.skew_x, style.rotate_z, style.skew_y), (Some(45.0), Some(180.0), Some(9.0), Some(-90.0)));
        assert!(style.x.is_none() && style.opacity.is_none(), "an angle is no length and no opacity");

        let number = json!({ "rotate": 5 });
        store.frame([&number], &[], clock.at(0));
        let mut style = StyleDesc::default();
        store.apply(&number, &mut style, &ParentValues::initial());
        assert_eq!(style.motion_rotate, Some(1.0));
    }

    #[test]
    fn a_number_and_an_angle_mix_in_degrees() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!(0)).unwrap();
        store.animate(1.0, &json!("1turn"), Some(&linear(1.0)), clock.at(0)).unwrap();
        let binding = json!({ "rotate": 1 });
        store.frame([&binding], &[], clock.at(500));
        let mut style = StyleDesc::default();
        store.apply(&binding, &mut style, &ParentValues::initial());
        assert_eq!(style.motion_rotate, Some(180.0));
        assert_eq!(store.get(1.0, clock.at(500)).unwrap(), json!("180deg"));

        store.create_source(2.0, &json!("90deg")).unwrap();
        store.animate(2.0, &json!(270), Some(&linear(1.0)), clock.at(0)).unwrap();
        assert_eq!(store.get(2.0, clock.at(500)).unwrap(), json!("180deg"));
        store.create_source(3.0, &json!("1turn")).unwrap();
        assert!(store.animate(3.0, &json!("red"), Some(&linear(1.0)), clock.at(0)).is_err());
    }

    #[test]
    fn a_transform_string_is_a_value_that_drives_motions_transform() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!("translateX(10px) rotate(45deg)")).unwrap();
        store.create_source(2.0, &json!("none")).unwrap();
        assert_eq!(store.get(2.0, clock.at(0)).unwrap(), json!("none"));
        assert!(store.create_source(3.0, &json!("translateX(10px")).is_err());

        let binding = json!({ "transform": 1, "opacity": 2 });
        store.frame([&binding], &[], clock.at(0));
        let mut style = StyleDesc::default();
        store.apply(&binding, &mut style, &ParentValues::initial());
        let text = style.motion_transform.as_deref().expect("the transform is written");
        assert_eq!(crate::css::transform::parse_transform(text, &LengthContext::initial(0.0, 0.0)), crate::css::transform::parse_transform("translateX(10px) rotate(45deg)", &LengthContext::initial(0.0, 0.0)));
        assert!(style.transform.is_none() && style.opacity.is_none());

        store.animate(1.0, &json!("translateX(110px) rotate(135deg)"), Some(&linear(1.0)), clock.at(0)).unwrap();
        store.frame([&binding], &[], clock.at(500));
        let mut style = StyleDesc::default();
        store.apply(&binding, &mut style, &ParentValues::initial());
        let lengths = LengthContext::initial(0.0, 0.0);
        let at = crate::css::transform::parse_transform(style.motion_transform.as_deref().unwrap(), &lengths).unwrap();
        match at.functions() {
            [gpui::TransformFunction::Translate(x, _), gpui::TransformFunction::Rotate(angle)] => {
                assert!((x.px - 60.0).abs() < 1e-3 && (angle - std::f32::consts::FRAC_PI_2).abs() < 1e-3, "{at:?}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_late_row_samples_only_the_values_the_frame_did_not() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!(0.25)).unwrap();
        let root = json!({ "opacity": 1 });
        store.frame([&root], &[], clock.at(0));

        // Bound after the frame, as a row mounted between two frames binds it.
        store.create_source(2.0, &json!(0.75)).unwrap();
        let row = json!({ "opacity": 2 });
        let mut style = StyleDesc::default();
        store.apply(&row, &mut style, &ParentValues::initial());
        assert_eq!(style.opacity, None, "the frame never sampled it");

        store.set(1.0, &json!(0.5), clock.at(10)).unwrap();
        store.sample_unsampled([&root, &row], &[], clock.at(10));
        store.apply(&row, &mut style, &ParentValues::initial());
        assert_eq!(style.opacity, Some(0.75));
        store.apply(&root, &mut style, &ParentValues::initial());
        assert_eq!(style.opacity, Some(0.25), "the frame's own sample is kept");
    }

    #[test]
    fn bindings_name_real_properties() {
        assert!(parse_binding(&json!({ "display": 1 })).is_err());
        assert!(parse_binding(&json!({ "opacity": "x" })).is_err());
        assert!(parse_binding(&json!([1])).is_err());
        match parse_binding(&json!({ "padding": 1 })).unwrap().as_slice() {
            [Bound::Shorthand { props, .. }] => assert_eq!(props.len(), 4),
            _ => panic!("padding is one shorthand binding"),
        }
    }

    #[test]
    fn a_disposed_value_stops_being_sampled() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!(0)).unwrap();
        store.listen(1.0, true, true, clock.at(0)).unwrap();
        assert!(store.dispose(1.0).unwrap());
        assert!(!store.dispose(1.0).unwrap());
        assert!(store.get(1.0, clock.at(0)).is_err());
        assert!(store.frame([&json!({ "opacity": 1 })], &[], clock.at(0)).events.is_empty());
    }

    #[test]
    fn a_calc_string_is_held_as_its_px_and_percentage() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!("calc(100% - 20px)")).unwrap();
        assert_eq!(store.get(1.0, clock.at(0)).unwrap(), json!("calc(100% - 20px)"));
        store.set(1.0, &json!("calc(50% + 10px)"), clock.at(0)).unwrap();
        assert_eq!(store.get(1.0, clock.at(0)).unwrap(), json!("calc(50% + 10px)"));

        // A length with only one part is the number or the percentage, `0%` included.
        store.set(1.0, &json!("20px"), clock.at(0)).unwrap();
        assert_eq!(store.get(1.0, clock.at(0)).unwrap(), json!(20.0));
        store.set(1.0, &json!("calc(50% + 20px - 50%)"), clock.at(0)).unwrap();
        assert_eq!(store.get(1.0, clock.at(0)).unwrap(), json!(20.0));
        store.set(1.0, &json!("0%"), clock.at(0)).unwrap();
        assert_eq!(store.get(1.0, clock.at(0)).unwrap(), json!("0%"));

        assert!(store.set(1.0, &json!("calc(100% - )"), clock.at(0)).is_err());
        assert!(store.set(1.0, &json!("wide"), clock.at(0)).is_err());
    }

    #[test]
    fn viewport_units_resolve_when_a_value_is_written_and_em_and_rem_stay_unresolved() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.set_viewport(800.0, 600.0);
        store.create_source(1.0, &json!("calc(100% - 2em)")).unwrap();
        assert_eq!(store.get(1.0, clock.at(0)).unwrap(), json!("calc(100% - 2em)"));
        store.set(1.0, &json!("calc(1rem + 50vw)"), clock.at(0)).unwrap();
        assert_eq!(store.get(1.0, clock.at(0)).unwrap(), json!("calc(1rem + 400px)"));
        store.set(1.0, &json!("calc(2 * (10% + 1em))"), clock.at(0)).unwrap();
        assert_eq!(store.get(1.0, clock.at(0)).unwrap(), json!("calc(20% + 2em)"));
        store.set(1.0, &json!("2em"), clock.at(0)).unwrap();
        assert_eq!(store.get(1.0, clock.at(0)).unwrap(), json!("2em"));
    }

    /// The font of an element whose parent has `parent` px text and a root of `root` px.
    fn font_of(parent: f64, root: f64) -> ParentValues {
        let mut lengths = LengthContext::initial(800.0, 600.0);
        lengths = lengths.descend(Some(&LinearLength::px(root)), true);
        let mut font = ParentValues::initial().descend(None);
        font.lengths = lengths.descend(Some(&LinearLength::px(parent)), false);
        font.above_root = false;
        font
    }

    #[test]
    fn em_and_rem_resolve_against_each_element_a_value_is_bound_to() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!("2em")).unwrap();
        store.create_source(2.0, &json!("calc(1rem + 50%)")).unwrap();
        let binding = json!({ "marginLeft": 1, "paddingTop": 2 });
        store.frame([&binding], &[], clock.at(0));
        for (own, root, margin, padding) in [(20.0, 10.0, 40.0, 10.0), (10.0, 10.0, 20.0, 10.0), (10.0, 20.0, 20.0, 20.0)] {
            let mut style = StyleDesc { font_size: Some(LinearLength::px(own)), ..Default::default() };
            store.apply(&binding, &mut style, &font_of(own, root));
            assert_eq!(style.margin_left, Some(crate::style::DimensionValue::Pixels(margin)), "{own}px");
            assert_eq!(
                style.padding_top,
                Some(LinearLength { px: padding, percent: 0.5, ..Default::default() }),
                "{own}px, root {root}px"
            );
        }
    }

    #[test]
    fn em_in_a_bound_font_size_is_of_the_parent_and_in_the_rest_of_the_element() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!("2em")).unwrap();
        store.create_source(2.0, &json!("1.5em")).unwrap();
        let binding = json!({ "marginLeft": 2, "fontSize": 1 });
        store.frame([&binding], &[], clock.at(0));
        let mut style = StyleDesc::default();
        store.apply(&binding, &mut style, &font_of(10.0, 16.0));
        assert_eq!(style.font_size, Some(LinearLength::px(20.0)));
        assert_eq!(style.margin_left, Some(crate::style::DimensionValue::Pixels(30.0)));
    }

    #[test]
    fn a_relative_value_on_a_number_property_resolves_to_the_number() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!("0.05em")).unwrap();
        let binding = json!({ "opacity": 1 });
        store.frame([&binding], &[], clock.at(0));
        let mut style = StyleDesc::default();
        store.apply(&binding, &mut style, &font_of(10.0, 16.0));
        assert_eq!(style.opacity, Some(0.5));
    }

    #[test]
    fn em_and_rem_animate_as_parts_and_mix_with_px_and_percentages() {
        let (clock, mut store) = keyframes_store();
        store.create_source(2.0, &json!("1em")).unwrap();
        store.animate(2.0, &json!("3rem"), Some(&linear(1.0)), clock.at(0)).unwrap();
        assert_eq!(store.get(2.0, clock.at(500)).unwrap(), json!("calc(0.5em + 1.5rem)"));
        store.animate(2.0, &json!("20px"), Some(&linear(1.0)), clock.at(1000)).unwrap();
        assert_eq!(store.get(2.0, clock.at(1500)).unwrap(), json!("calc(1.5rem + 10px)"));
        assert_eq!(store.get(2.0, clock.at(2000)).unwrap(), json!(20.0));
    }

    #[test]
    fn flex_cannot_be_bound() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!(1)).unwrap();
        let binding = json!({ "flex": 1 });
        assert!(parse_binding(&binding).is_err());
        store.frame([&binding], &[], clock.at(0));
        let mut style = StyleDesc::default();
        store.apply(&binding, &mut style, &ParentValues::initial());
        assert_eq!(style.flex_grow, None);
    }

    #[test]
    fn two_calcs_animate_as_the_mix_of_their_parts() {
        let (clock, mut store) = keyframes_store();
        store.create_source(2.0, &json!("calc(100% - 20px)")).unwrap();
        store.animate(2.0, &json!("calc(50% + 10px)"), Some(&linear(1.0)), clock.at(0)).unwrap();
        // calc(75% - 5px) is 295px of 400px.
        assert_eq!(store.get(2.0, clock.at(500)).unwrap(), json!("calc(75% - 5px)"));
        assert_eq!(store.get(2.0, clock.at(1000)).unwrap(), json!("calc(50% + 10px)"));
    }

    #[test]
    fn a_px_length_or_percentage_animates_to_a_calc_and_back() {
        let (clock, mut store) = keyframes_store();
        store.create_source(2.0, &json!("20px")).unwrap();
        store.animate(2.0, &json!("calc(100% - 20px)"), Some(&linear(1.0)), clock.at(0)).unwrap();
        // The px parts cancel (20px to -20px is a 0 midpoint), leaving the percentage.
        assert_eq!(store.get(2.0, clock.at(500)).unwrap(), json!("50%"));
        store.animate(2.0, &json!("50%"), Some(&linear(1.0)), clock.at(1000)).unwrap();
        assert_eq!(store.get(2.0, clock.at(1500)).unwrap(), json!("calc(75% - 10px)"));
        store.animate(2.0, &json!(["calc(100% - 20px)", "calc(50% + 10px)", 40]), Some(&linear(2.0)), clock.at(2000)).unwrap();
        assert_eq!(store.get(2.0, clock.at(3000)).unwrap(), json!("calc(50% + 10px)"));
        assert_eq!(store.get(2.0, clock.at(4000)).unwrap(), json!(40.0));
    }

    #[test]
    fn a_transform_maps_to_calc_outputs() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!(0)).unwrap();
        store
            .create_transform(2.0, 1.0, vec![0.0, 1.0], &json!(["calc(100% - 20px)", "calc(50% + 10px)"]), None)
            .unwrap();
        store.set(1.0, &json!(0.5), clock.at(0)).unwrap();
        assert_eq!(store.get(2.0, clock.at(0)).unwrap(), json!("calc(75% - 5px)"));
        assert!(store
            .create_transform(3.0, 1.0, vec![0.0, 1.0], &json!(["calc(100% - 20px)", "#fff"]), None)
            .is_err());
    }

    #[test]
    fn a_calc_binds_to_a_length_and_is_clamped_where_the_property_is_non_negative() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!("calc(100% - 20px)")).unwrap();
        let binding = json!({ "width": 1, "marginLeft": 1 });
        store.frame([&binding], &[], clock.at(0));
        let mut style = StyleDesc::default();
        store.apply(&binding, &mut style, &ParentValues::initial());
        let calc = LinearLength { px: -20.0, percent: 1.0, ..Default::default() };
        assert_eq!(style.width, Some(crate::style::DimensionValue::Calc(calc)));
        assert_eq!(style.margin_left, Some(crate::style::DimensionValue::Calc(calc)));

        store.set(1.0, &json!("calc(-100% - 20px)"), clock.at(10)).unwrap();
        store.frame([&binding], &[], clock.at(10));
        let mut style = StyleDesc::default();
        store.apply(&binding, &mut style, &ParentValues::initial());
        assert_eq!(style.width, Some(crate::style::DimensionValue::Pixels(0.0)));
    }

    /// The fraction a percentage or the percentage part of a calc reads.
    fn fraction_of(value: &Value) -> f64 {
        let text = value.as_str().unwrap().trim_start_matches("calc(");
        text[..text.find('%').unwrap()].parse::<f64>().unwrap() / 100.0
    }

    /// How far the percentage channel of `from` to `to` is from `to` when the
    /// spring stops, a fraction of the basis.
    fn percent_snap(from: Value, to: Value) -> f64 {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_spring(1.0, &from, &json!({ "stiffness": 200, "damping": 20 }), clock.at(0)).unwrap();
        store.set(1.0, &to, clock.at(0)).unwrap();
        let target = fraction_of(&to);
        let mut last_active = None;
        for millis in 1..20_000 {
            if !store.is_animating(1.0, clock.at(millis)).unwrap() {
                break;
            }
            last_active = Some(clock.at(millis));
        }
        let before = fraction_of(&store.get(1.0, last_active.unwrap()).unwrap());
        (before - target).abs()
    }

    #[test]
    fn a_spring_on_a_percentage_rests_within_a_tenth_of_a_pixel_of_a_1000px_basis() {
        // 0.1px of 1000px is 1e-4 of the basis; the fraction's old threshold was 5e-3.
        let snap = percent_snap(json!("0%"), json!("100%"));
        assert!(snap < 1e-4, "snapped {snap} of the basis");
        let snap = percent_snap(json!("20px"), json!("calc(100% - 20px)"));
        assert!(snap < 1e-4, "snapped {snap} of the basis");
    }

    #[test]
    fn a_multi_value_string_is_held_as_its_components_and_reads_back_as_css() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!("0.5em 0.75em")).unwrap();
        assert_eq!(store.get(1.0, clock.at(0)).unwrap(), json!("0.5em 0.75em"));
        store.set(1.0, &json!("10px / 20px"), clock.at(0)).unwrap();
        assert_eq!(store.get(1.0, clock.at(0)).unwrap(), json!("10px / 20px"));
        store.set(1.0, &json!("calc(100% - 4px)  50% 0"), clock.at(0)).unwrap();
        assert_eq!(store.get(1.0, clock.at(0)).unwrap(), json!("calc(100% - 4px) 50% 0px"));
        store.set(1.0, &json!("10px/5%"), clock.at(0)).unwrap();
        assert_eq!(store.get(1.0, clock.at(0)).unwrap(), json!("10px / 5%"));

        store.create_source(2.0, &json!("red blue")).unwrap();
        let read = store.get(2.0, clock.at(0)).unwrap();
        assert!(read.as_str().unwrap().starts_with("rgba(255.000, 0.000, 0.000, 1.00000) rgba(0.000, 0.000, 255.000"), "{read}");

        // A colour with spaces and a slash is one value, and a lone length keeps its one-value shape.
        store.create_source(3.0, &json!("rgb(0 0 255 / 50%)")).unwrap();
        assert!(store.get(3.0, clock.at(0)).unwrap().as_str().unwrap().starts_with("rgba(0.000, 0.000, 255.000, 0.5"));
        store.create_source(4.0, &json!("12px")).unwrap();
        assert_eq!(store.get(4.0, clock.at(0)).unwrap(), json!(12.0));
        store.set(4.0, &json!("1px 2px"), clock.at(0)).unwrap();
        assert_eq!(store.get(4.0, clock.at(0)).unwrap(), json!("1px 2px"));
    }

    #[test]
    fn a_multi_value_string_that_is_no_list_of_lengths_or_colours_is_rejected() {
        let mut store = MotionValueStore::default();
        for text in ["10px bogus", "10px / red", "/ 10px", "10px /", "1px / 2px / 3px", "red 10px / 20px", "10px 20px solid"] {
            assert!(store.create_source(1.0, &json!(text)).is_err(), "{text}");
        }
    }

    #[test]
    fn two_lists_of_the_same_shape_blend_component_by_component() {
        let (clock, mut store) = keyframes_store();
        store.create_source(2.0, &json!("0.5em 0.75em")).unwrap();
        store.animate(2.0, &json!("1em 1.5em"), Some(&linear(1.0)), clock.at(0)).unwrap();
        assert_eq!(store.get(2.0, clock.at(500)).unwrap(), json!("0.75em 1.125em"));
        assert_eq!(store.get(2.0, clock.at(1000)).unwrap(), json!("1em 1.5em"));

        // Lengths of different units mix as calc() parts, per component.
        store.animate(2.0, &json!("50% calc(100% - 4px)"), Some(&linear(1.0)), clock.at(2000)).unwrap();
        assert_eq!(store.get(2.0, clock.at(2500)).unwrap(), json!("calc(25% + 0.5em) calc(50% + 0.75em - 2px)"));

        store.create_source(3.0, &json!("10px / 20px")).unwrap();
        store.animate(3.0, &json!("30px / 40px"), Some(&linear(1.0)), clock.at(0)).unwrap();
        assert_eq!(store.get(3.0, clock.at(500)).unwrap(), json!("20px / 30px"));

        store.create_source(4.0, &json!("#000000 #ffffff")).unwrap();
        store.animate(4.0, &json!("#ffffff #ffffff"), Some(&linear(1.0)), clock.at(0)).unwrap();
        let mid = store.get(4.0, clock.at(500)).unwrap();
        let mid = mid.as_str().unwrap();
        let (first, second) = mid.split_once(") ").unwrap();
        assert!(first.starts_with("rgba(") && second.starts_with("rgba("), "{mid}");
        assert_ne!(first, second.trim_end_matches(')'));
    }

    #[test]
    fn list_keyframes_blend_per_segment() {
        let (clock, mut store) = keyframes_store();
        store.create_source(2.0, &json!("0px 0px")).unwrap();
        store
            .animate(2.0, &json!(["0px 0px", "10px 20px", null, "30px 0px"]), Some(&linear(3.0)), clock.at(0))
            .unwrap();
        assert_eq!(store.get(2.0, clock.at(500)).unwrap(), json!("5px 10px"));
        assert_eq!(store.get(2.0, clock.at(1500)).unwrap(), json!("10px 20px"));
        assert_eq!(store.get(2.0, clock.at(2500)).unwrap(), json!("20px 10px"));
    }

    #[test]
    fn a_shorter_list_is_filled_out_by_the_shorthand_rule_before_blending() {
        let (clock, mut store) = keyframes_store();
        store.create_source(2.0, &json!("8px")).unwrap();
        store.animate(2.0, &json!("8px 16px"), Some(&linear(1.0)), clock.at(0)).unwrap();
        assert_eq!(store.get(2.0, clock.at(500)).unwrap(), json!("8px 12px"));
        assert_eq!(store.get(2.0, clock.at(1000)).unwrap(), json!("8px 16px"));

        store.animate(2.0, &json!("0px 10px 20px 30px"), Some(&linear(1.0)), clock.at(2000)).unwrap();
        assert_eq!(store.get(2.0, clock.at(2500)).unwrap(), json!("4px 13px 14px 23px"));

        // Three values are top, the sides, bottom; two are the sides alike.
        store.create_source(3.0, &json!("0px 10px 20px")).unwrap();
        store.animate(3.0, &json!("20px 10px"), Some(&linear(1.0)), clock.at(0)).unwrap();
        assert_eq!(store.get(3.0, clock.at(500)).unwrap(), json!("10px 10px 20px"));

        // A radius without a / has the same vertical radii as horizontal.
        store.create_source(4.0, &json!("10px 20px")).unwrap();
        store.animate(4.0, &json!("10px / 30px"), Some(&linear(1.0)), clock.at(0)).unwrap();
        assert_eq!(store.get(4.0, clock.at(500)).unwrap(), json!("10px 15px / 20px 25px"));
    }

    #[test]
    fn lists_that_cannot_be_filled_out_alike_jump_at_the_halfway_point() {
        let (clock, mut store) = keyframes_store();
        store.create_source(2.0, &json!("1px 2px")).unwrap();
        store.animate(2.0, &json!("1px 2px 3px 4px 5px"), Some(&linear(1.0)), clock.at(0)).unwrap();
        assert_eq!(store.get(2.0, clock.at(400)).unwrap(), json!("1px 2px"));
        assert_eq!(store.get(2.0, clock.at(600)).unwrap(), json!("1px 2px 3px 4px 5px"));

        // A colour never blends with a length, a list or not.
        store.create_source(5.0, &json!("1px 2px")).unwrap();
        assert!(store.animate(5.0, &json!("red blue"), None, clock.at(2000)).is_err());
        store.create_source(3.0, &json!("red")).unwrap();
        assert!(store.animate(3.0, &json!("1px 2px"), None, clock.at(0)).is_err());
    }

    #[test]
    fn a_list_is_mapped_by_a_transform_and_chased_by_a_spring() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!(0)).unwrap();
        store
            .create_transform(2.0, 1.0, vec![0.0, 1.0], &json!(["0px 10px", "20px 30px"]), None)
            .unwrap();
        store.set(1.0, &json!(0.5), clock.at(0)).unwrap();
        assert_eq!(store.get(2.0, clock.at(0)).unwrap(), json!("10px 20px"));

        store.create_spring(3.0, &json!("0px 0px"), &json!({ "stiffness": 300, "damping": 30 }), clock.at(0)).unwrap();
        store.set(3.0, &json!("10px 20px"), clock.at(0)).unwrap();
        assert!(store.is_animating(3.0, clock.at(50)).unwrap());
        let mid = store.get(3.0, clock.at(50)).unwrap();
        assert_ne!(mid, json!("0px 0px"));
        assert_eq!(store.get(3.0, clock.at(60_000)).unwrap(), json!("10px 20px"));
    }

    fn applied(store: &mut MotionValueStore, clock: &Clock, binding: Value) -> StyleDesc {
        store.frame([&binding], &[], clock.at(0));
        let mut style = StyleDesc::default();
        store.apply(&binding, &mut style, &ParentValues::initial());
        style
    }

    #[test]
    fn a_list_bound_to_a_shorthand_gives_each_side_its_own_component() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!("0.5em 0.75em")).unwrap();
        let style = applied(&mut store, &clock, json!({ "padding": 1 }));
        assert_eq!(style.padding_top, Some(LinearLength::px(8.0)));
        assert_eq!(style.padding_bottom, Some(LinearLength::px(8.0)));
        assert_eq!(style.padding_left, Some(LinearLength::px(12.0)));
        assert_eq!(style.padding_right, Some(LinearLength::px(12.0)));

        store.set(1.0, &json!("1px 2px 3px"), clock.at(0)).unwrap();
        let style = applied(&mut store, &clock, json!({ "borderWidth": 1 }));
        assert_eq!(style.border_top_width, Some(LinearLength::px(1.0)));
        assert_eq!(style.border_right_width, Some(LinearLength::px(2.0)));
        assert_eq!(style.border_bottom_width, Some(LinearLength::px(3.0)));
        assert_eq!(style.border_left_width, Some(LinearLength::px(2.0)));

    }

    #[test]
    fn a_list_bound_to_margin_and_inset_reads_as_dimensions() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!("8px 25%")).unwrap();
        let style = applied(&mut store, &clock, json!({ "margin": 1, "inset": 1 }));
        assert_eq!(style.margin_top, Some(crate::style::DimensionValue::Pixels(8.0)));
        assert_eq!(style.margin_left, Some(crate::style::DimensionValue::Percentage(0.25)));
        assert_eq!(style.top, Some(crate::style::DimensionValue::Pixels(8.0)));
        assert_eq!(style.right, Some(crate::style::DimensionValue::Percentage(0.25)));
    }

    #[test]
    fn a_list_bound_to_gap_is_the_row_and_the_column() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!("4px 9px")).unwrap();
        let style = applied(&mut store, &clock, json!({ "gap": 1 }));
        assert_eq!(style.row_gap, Some(LinearLength::px(4.0)));
        assert_eq!(style.column_gap, Some(LinearLength::px(9.0)));

        store.set(1.0, &json!("1px 2px 3px"), clock.at(0)).unwrap();
        let style = applied(&mut store, &clock, json!({ "gap": 1 }));
        assert_eq!(style.row_gap, None, "gap takes at most two values");
    }

    #[test]
    fn a_list_bound_to_border_radius_gives_each_corner_both_radii() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!("10px / 20px")).unwrap();
        let style = applied(&mut store, &clock, json!({ "borderRadius": 1 }));
        let corner = |x: f64, y: f64| Some(CornerRadiusValue { x: LinearLength::px(x), y: Some(LinearLength::px(y)) });
        assert_eq!(style.border_top_left_radius, corner(10.0, 20.0));
        assert_eq!(style.border_bottom_right_radius, corner(10.0, 20.0));

        // Clockwise from the top left on both sides of the /.
        store.set(1.0, &json!("1px 2px 3px 4px / 5px 6px 7px 8px"), clock.at(0)).unwrap();
        let style = applied(&mut store, &clock, json!({ "borderRadius": 1 }));
        assert_eq!(style.border_top_left_radius, corner(1.0, 5.0));
        assert_eq!(style.border_top_right_radius, corner(2.0, 6.0));
        assert_eq!(style.border_bottom_right_radius, corner(3.0, 7.0));
        assert_eq!(style.border_bottom_left_radius, corner(4.0, 8.0));

        // Without a / the corners are circular.
        store.set(1.0, &json!("1px 2px"), clock.at(0)).unwrap();
        let style = applied(&mut store, &clock, json!({ "borderRadius": 1 }));
        let circle = |x: f64| Some(CornerRadiusValue { x: LinearLength::px(x), y: None });
        assert_eq!(style.border_top_left_radius, circle(1.0));
        assert_eq!(style.border_top_right_radius, circle(2.0));
        assert_eq!(style.border_bottom_left_radius, circle(2.0));
        assert_eq!(style.border_bottom_right_radius, circle(1.0));
    }

    #[test]
    fn a_list_bound_to_border_color_gives_each_side_its_own_colour() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!("#ff0000 #0000ff")).unwrap();
        let style = applied(&mut store, &clock, json!({ "borderColor": 1 }));
        let red = style.border_top_color.clone().unwrap();
        let blue = style.border_left_color.clone().unwrap();
        assert_ne!(red, blue);
        assert!(red.starts_with("rgba(255.000, 0.000, 0.000") && blue.starts_with("rgba(0.000, 0.000, 255.000"), "{red} {blue}");
        assert_eq!(style.border_bottom_color, Some(red));
        assert_eq!(style.border_right_color, Some(blue));
    }

    #[test]
    fn a_list_a_shorthand_cannot_take_is_skipped() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        let cases = [
            ("padding", json!("#ff0000 #0000ff")),
            ("borderColor", json!("1px 2px")),
            ("padding", json!("1px 2px 3px 4px 5px")),
            ("padding", json!("1px / 2px")),
            ("gap", json!("1px 2px 3px")),
        ];
        for (index, (name, value)) in cases.into_iter().enumerate() {
            let id = index as f64;
            store.create_source(id, &value).unwrap();
            let style = applied(&mut store, &clock, json!({ name: id }));
            assert_eq!(style.padding_top, None, "{name} {value}");
            assert_eq!(style.border_top_color, None, "{name} {value}");
            assert_eq!(style.row_gap, None, "{name} {value}");
        }
    }

    #[test]
    fn a_list_is_no_value_of_a_longhand() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(1.0, &json!("1px 2px")).unwrap();
        let style = applied(&mut store, &clock, json!({ "paddingTop": 1, "width": 1, "opacity": 1 }));
        assert_eq!(style.padding_top, None);
        assert_eq!(style.width, None);
        assert_eq!(style.opacity, None);
    }

    #[test]
    fn an_animating_list_moves_the_sides_it_is_bound_to() {
        let (clock, mut store) = keyframes_store();
        store.create_source(2.0, &json!("0.5em 0.75em")).unwrap();
        let binding = json!({ "padding": 2 });
        store.animate(2.0, &json!("1em 1.5em"), Some(&linear(1.0)), clock.at(0)).unwrap();
        assert!(store.frame([&binding], &[], clock.at(500)).active);
        let mut style = StyleDesc::default();
        store.apply(&binding, &mut style, &ParentValues::initial());
        assert_eq!(style.padding_top, Some(LinearLength::px(12.0)));
        assert_eq!(style.padding_left, Some(LinearLength::px(18.0)));
    }

    #[test]
    fn a_binding_is_due_while_its_value_moves_or_after_a_write_and_not_while_it_rests() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        for id in 0..4 {
            store.create_source(f64::from(id), &json!(0)).unwrap();
        }
        store.create_transform(4.0, 0.0, vec![0.0, 1.0], &json!([0, 10]), None).unwrap();
        let moving = json!({ "opacity": 0 });
        let resting = json!({ "opacity": 1 });
        let mapped = json!({ "opacity": 4 });
        let bindings = [&moving, &resting, &mapped];
        let due = |store: &MotionValueStore| bindings.map(|binding| store.binding_due(binding));

        store.animate(0.0, &json!(1), Some(&linear(1.0)), clock.at(0)).unwrap();
        assert_eq!(due(&store), [true, false, true], "an animation was started, and a value mapped from it follows");
        store.frame(bindings, &[], clock.at(100));
        assert_eq!(due(&store), [true, false, true], "it moves by itself");
        store.frame(bindings, &[], clock.at(900));
        assert_eq!(due(&store), [true, false, true], "the frame that sampled it moving built the elements at the old value");
        store.frame(bindings, &[], clock.at(1100));
        assert_eq!(due(&store), [false, false, false], "it came to rest and every element has its final value");

        store.set(1.0, &json!(0.5), clock.at(1200)).unwrap();
        assert_eq!(due(&store), [false, true, false], "a write");
        store.frame(bindings, &[], clock.at(1216));
        assert_eq!(due(&store), [false, false, false], "a frame that sampled it has built it");

        store.pause(0.0, clock.at(1300)).unwrap();
        assert_eq!(due(&store), [true, false, true], "a control on an animation");
        assert!(MotionValueStore::default().binding_due(&json!({ "nope": 1 })), "a binding that does not parse is not known not to");
    }

    #[test]
    fn moving_values_ask_for_a_frame_only_for_an_element_in_view_and_for_a_tick_otherwise() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(0.0, &json!(0)).unwrap();
        store.create_source(1.0, &json!(0)).unwrap();
        store.listen(1.0, true, false, clock.at(0)).unwrap();
        let bound = json!({ "opacity": 0 });
        let shown = |dormant: bool| [(dormant, &bound)];

        store.frame([&bound], &[], clock.at(0));
        assert_eq!(store.demand(shown(false)), Demand::Idle);

        store.animate(1.0, &json!(1), Some(&linear(1.0)), clock.at(0)).unwrap();
        store.frame([&bound], &[], clock.at(100));
        assert_eq!(store.demand(shown(false)), Demand::Dormant, "a value that is only listened to draws nothing");

        store.animate(0.0, &json!(1), Some(&linear(1.0)), clock.at(100)).unwrap();
        store.frame([&bound], &[], clock.at(200));
        assert_eq!(store.demand(shown(false)), Demand::Frame, "an element in view is bound to a value that moves");
        assert_eq!(store.demand(shown(true)), Demand::Dormant, "and one that is dormant is not");
        assert_eq!(store.demand([]), Demand::Dormant);

        let dormant = store.advance_dormant(clock.at(300));
        assert!(dormant.active);
        assert_eq!(store.demand(shown(false)), Demand::Frame, "it does not change what the last frame found");
        let done = store.advance_dormant(clock.at(1300));
        assert!(!done.active);
        assert!(done.events.iter().any(|event| event.id == 1 && event.kind == MotionValueEventKind::Change), "the events of a listened value come");
        store.frame([&bound], &[], clock.at(1400));
        assert_eq!(store.demand(shown(false)), Demand::Idle);
    }

    /// A store with a source (0) that nobody binds and a watch on it, animated at `now` by `transition`.
    fn watched(change: bool, animation: bool, transition: Value) -> (Clock, MotionValueStore) {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(0.0, &json!(0)).unwrap();
        store.listen(0.0, change, animation, clock.at(0)).unwrap();
        store.animate(0.0, &json!(100), Some(&transition), clock.at(0)).unwrap();
        store.frame([], &[], clock.at(100));
        (clock, store)
    }

    #[test]
    fn a_value_that_nobody_listens_to_needs_no_sample_however_it_moves() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(0.0, &json!(0)).unwrap();
        let bound = json!({ "opacity": 0 });
        store.animate(0.0, &json!(1), Some(&json!({ "duration": 1.0, "ease": "linear", "repeat": "Infinity" })), clock.at(0)).unwrap();
        store.frame([&bound], &[], clock.at(100));
        assert_eq!(store.demand([(true, &bound)]), Demand::Dormant);
        assert_eq!(store.dormant_wake(clock.at(100)), Wake::Never);
    }

    #[test]
    fn a_value_with_a_change_listener_needs_a_sample_in_every_frame_it_moves_in() {
        let (clock, store) = watched(true, false, linear(1.0));
        assert_eq!(store.dormant_wake(clock.at(100)), Wake::NextFrame);
        let (clock, store) = watched(true, true, linear(1.0));
        assert_eq!(store.dormant_wake(clock.at(100)), Wake::NextFrame, "the change listener decides");
    }

    #[test]
    fn a_value_with_only_an_animation_listener_needs_a_sample_when_its_animation_ends() {
        let (clock, mut store) = watched(false, true, linear(1.0));
        assert_eq!(store.dormant_wake(clock.at(100)), Wake::At(clock.at(1000)));
        store.pause(0.0, clock.at(200)).unwrap();
        store.frame([], &[], clock.at(250));
        assert_eq!(store.dormant_wake(clock.at(250)), Wake::Never, "a held animation does not end");
        store.play(0.0, clock.at(300)).unwrap();
        store.set_speed(0.0, 2.0, clock.at(300)).unwrap();
        store.frame([], &[], clock.at(350));
        // 200 ms of the second are played; at twice the pace 800 ms more take 400 ms.
        assert_eq!(store.dormant_wake(clock.at(350)), Wake::At(clock.at(700)));
        let done = store.advance_dormant(clock.at(700));
        assert_eq!(kinds(&done), vec![(0, MotionValueEventKind::AnimationComplete)]);
        assert_eq!(store.dormant_wake(clock.at(700)), Wake::Never);
    }

    #[test]
    fn the_end_of_a_repeat_is_known_and_a_repeat_without_a_count_has_none() {
        let twice = json!({ "duration": 1.0, "ease": "linear", "repeat": 1 });
        let (clock, store) = watched(false, true, twice);
        assert_eq!(store.dormant_wake(clock.at(100)), Wake::At(clock.at(2000)));
        let forever = json!({ "duration": 1.0, "ease": "linear", "repeat": "Infinity" });
        let (clock, store) = watched(false, true, forever);
        assert_eq!(store.dormant_wake(clock.at(100)), Wake::Never);
    }

    #[test]
    fn a_spring_is_sampled_where_it_comes_to_rest() {
        let spring = json!({ "type": "spring", "stiffness": 200, "damping": 10 });
        let (clock, mut store) = watched(false, true, spring);
        let Wake::At(rest) = store.dormant_wake(clock.at(100)) else {
            panic!("a spring that rests within 20 s says when");
        };
        assert!(rest > clock.at(1000), "it is not a frame away");
        let before = store.advance_dormant(rest - Duration::from_millis(1));
        assert!(kinds(&before).is_empty(), "a millisecond early it still moves");
        let done = store.advance_dormant(rest);
        assert_eq!(kinds(&done), vec![(0, MotionValueEventKind::AnimationComplete)]);
        assert_eq!(store.dormant_wake(rest), Wake::Never);
    }

    #[test]
    fn a_spring_that_is_listened_to_for_its_changes_is_sampled_in_every_frame() {
        let spring = json!({ "type": "spring", "stiffness": 200, "damping": 10 });
        let (clock, store) = watched(true, true, spring);
        assert_eq!(store.dormant_wake(clock.at(100)), Wake::NextFrame);
    }

    fn follower_of_a_source(listen_to: f64) -> (Clock, MotionValueStore) {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(0.0, &json!(0)).unwrap();
        store.create_follower(1.0, 0.0, &json!({ "stiffness": 300, "damping": 30 }), clock.at(0)).unwrap();
        store.listen(listen_to, false, true, clock.at(0)).unwrap();
        (clock, store)
    }

    #[test]
    fn a_follower_of_a_value_at_rest_is_sampled_where_it_comes_to_rest() {
        let (clock, mut store) = follower_of_a_source(1.0);
        store.set(0.0, &json!(100), clock.at(100)).unwrap();
        store.frame([], &[], clock.at(116));
        let Wake::At(rest) = store.dormant_wake(clock.at(116)) else {
            panic!("a follower of a value at rest says when it rests");
        };
        assert!(rest > clock.at(400));
        let done = store.advance_dormant(rest);
        assert_eq!(kinds(&done).last(), Some(&(1, MotionValueEventKind::AnimationComplete)));
        assert_eq!(store.dormant_wake(rest), Wake::Never);
    }

    #[test]
    fn a_follower_of_a_loop_is_sampled_in_every_frame_though_the_loop_has_no_end() {
        let (clock, mut store) = follower_of_a_source(1.0);
        let forever = json!({ "duration": 1.0, "ease": "linear", "repeat": "Infinity" });
        store.animate(0.0, &json!(100), Some(&forever), clock.at(100)).unwrap();
        store.frame([], &[], clock.at(116));
        assert_eq!(store.dormant_wake(clock.at(116)), Wake::NextFrame);
        store.frame([], &[], clock.at(5000));
        assert_eq!(store.dormant_wake(clock.at(5000)), Wake::NextFrame);
    }

    #[test]
    fn a_follower_of_a_value_that_moves_is_sampled_in_every_frame() {
        let (clock, mut store) = follower_of_a_source(1.0);
        store.animate(0.0, &json!(100), Some(&linear(1.0)), clock.at(100)).unwrap();
        store.frame([], &[], clock.at(116));
        assert_eq!(store.dormant_wake(clock.at(116)), Wake::NextFrame);
        store.frame([], &[], clock.at(1116));
        assert!(matches!(store.dormant_wake(clock.at(1116)), Wake::At(_)), "once the source rests, the follower's end is known");
    }

    #[test]
    fn a_value_computed_from_an_animation_ends_with_it() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(0.0, &json!(0)).unwrap();
        store.create_transform(1.0, 0.0, vec![0.0, 100.0], &json!([0, 1]), None).unwrap();
        store.listen(1.0, false, true, clock.at(0)).unwrap();
        store.animate(0.0, &json!(100), Some(&linear(1.0)), clock.at(0)).unwrap();
        store.frame([], &[], clock.at(100));
        assert_eq!(store.dormant_wake(clock.at(100)), Wake::At(clock.at(1000)));
    }

    #[test]
    fn a_tick_leaves_the_moving_values_that_only_sleeping_elements_bind_as_they_are() {
        let clock = clock();
        let mut store = MotionValueStore::default();
        store.create_source(0.0, &json!(0)).unwrap();
        store.create_source(1.0, &json!(0)).unwrap();
        let bindings = [(10u64, json!({ "opacity": 0 })), (11, json!({ "opacity": 1 }))];
        let index = BindingIndex::new(bindings.iter().map(|(element, binding)| (*element, binding)));
        let looping = json!({ "duration": 1.0, "ease": "linear", "repeat": "Infinity" });
        for id in [0.0, 1.0] {
            store.animate(id, &json!(1), Some(&looping), clock.at(0)).unwrap();
        }
        store.frame(bindings.iter().map(|(_, binding)| binding), &[], clock.at(0));
        let sampled = |store: &mut MotionValueStore, millis: u64, asleep: &dyn Fn(u64) -> bool| {
            let before = crate::perf_counters::snapshot().motion_values_sampled;
            store.step(clock.at(millis), &index, asleep);
            crate::perf_counters::snapshot().motion_values_sampled - before
        };
        assert_eq!(sampled(&mut store, 16, &|_| false), 2, "both are seen");
        assert_eq!(sampled(&mut store, 32, &|element| element == 11), 1, "one is bound to a sleeper");
        assert_eq!(sampled(&mut store, 48, &|_| true), 0, "none is seen");
        assert_eq!(store.demand([(true, &bindings[0].1), (true, &bindings[1].1)]), Demand::Dormant, "they still move");
        assert_eq!(sampled(&mut store, 64, &|_| false), 2, "and are sampled when seen");
    }
}
