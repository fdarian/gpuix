//! The effect of a CSS animation or transition as Web Animations has it: the timing a run plays
//! by, the progress and iteration that timing gives at a local time, and what a `KeyframeEffect`
//! reads of both (`getTiming()`, `getComputedTiming()`, `updateTiming()`).
//!
//! Times are seconds here; the napi layer turns them into the milliseconds of the DOM.

use super::css_animation::{Direction, FillMode, IterationCount};
use super::curve::{ease, parse_timing_function, Ease};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Before,
    Active,
    After,
}

/// Where an effect is at one local time (Web Animations, "calculating the directed progress").
pub(super) struct Evaluation {
    pub(super) phase: Phase,
    /// Zero-based, meaningful in the active phase.
    pub(super) iteration: u64,
    /// How far through the keyframes, 0 to 1, after the direction. `None` when the effect has no
    /// effect at this time: before the delay without `backwards`, or after the end without
    /// `forwards`.
    pub(super) progress: Option<f64>,
}

impl Evaluation {
    /// The progress after the effect's own easing, which is not the keyframes'.
    pub(super) fn eased(mut self, easing: Option<&Ease>) -> Self {
        if let (Some(progress), Some(easing)) = (self.progress, easing) {
            self.progress = Some(ease(progress, easing));
        }
        self
    }
}

/// The timing of one effect, without its easing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Timing {
    pub(super) delay: f64,
    /// Seconds. An `auto` duration is 0.
    pub(super) duration: f64,
    pub(super) iterations: IterationCount,
    pub(super) direction: Direction,
    pub(super) fill: FillMode,
    pub(super) end_delay: f64,
    pub(super) iteration_start: f64,
}

impl Timing {
    pub(super) fn active_duration(&self) -> f64 {
        match self.iterations {
            _ if self.duration == 0.0 => 0.0,
            IterationCount::Finite(count) if count == 0.0 => 0.0,
            IterationCount::Infinite => f64::INFINITY,
            IterationCount::Finite(count) => self.duration * count,
        }
    }

    /// Where the effect ends, in local time: the delay, the active duration and the end delay.
    /// Infinite for an effect that repeats for ever.
    pub(super) fn end_time(&self) -> f64 {
        (self.delay + self.active_duration() + self.end_delay).max(0.0)
    }

    /// `elapsedTime` of an `animationstart`: the part of the active interval a negative delay
    /// already skipped.
    pub(super) fn initial_elapsed(&self) -> f64 {
        (-self.delay).min(self.active_duration()).max(0.0)
    }

    /// `backwards_rate` is a negative playback rate, which gives the boundaries to the phase the
    /// time came from.
    pub(super) fn evaluate(&self, local: f64, backwards_rate: bool) -> Evaluation {
        let active_duration = self.active_duration();
        let end_time = self.end_time();
        let before_boundary = self.delay.min(end_time).max(0.0);
        let after_boundary = (self.delay + active_duration).min(end_time).max(0.0);
        let phase = if local < before_boundary || (backwards_rate && local == before_boundary) {
            Phase::Before
        } else if local > after_boundary || (!backwards_rate && local == after_boundary) {
            Phase::After
        } else {
            Phase::Active
        };

        let backwards_fill = matches!(self.fill, FillMode::Backwards | FillMode::Both);
        let forwards_fill = matches!(self.fill, FillMode::Forwards | FillMode::Both);
        let active_time = match phase {
            Phase::Before => backwards_fill.then(|| (local - self.delay).max(0.0)),
            Phase::Active => Some(local - self.delay),
            Phase::After => forwards_fill.then(|| (local - self.delay).clamp(0.0, active_duration)),
        };
        let Some(active_time) = active_time else {
            return Evaluation { phase, iteration: 0, progress: None };
        };

        let count = match self.iterations {
            IterationCount::Infinite => f64::INFINITY,
            IterationCount::Finite(count) => count,
        };
        let iterations_done = if self.duration == 0.0 {
            if phase == Phase::Before {
                0.0
            } else {
                count
            }
        } else if active_time >= active_duration {
            // Not `active_time / duration`: that can land a hair past the count.
            count
        } else {
            active_time / self.duration
        };
        let overall = iterations_done + self.iteration_start;
        let mut progress = if overall.is_infinite() { self.iteration_start % 1.0 } else { overall % 1.0 };
        let mut iteration = overall.floor();
        if progress == 0.0 && phase != Phase::Before && active_time >= active_duration && count != 0.0 {
            // The end of an iteration is its last frame, not the next one's first.
            progress = 1.0;
            iteration = overall.floor() - 1.0;
        }
        let iteration = iteration.max(0.0) as u64;
        let backwards = match self.direction {
            Direction::Normal => false,
            Direction::Reverse => true,
            Direction::Alternate => iteration % 2 == 1,
            Direction::AlternateReverse => iteration % 2 == 0,
        };
        Evaluation {
            phase,
            iteration,
            progress: Some(if backwards { 1.0 - progress } else { progress }),
        }
    }
}

/// How an `updateTiming()` sets the duration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum DurationUpdate {
    Auto,
    Seconds(f64),
}

/// What `updateTiming()` sets, kept by the run: the properties it names win over the style from
/// then on, the others still follow it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TimingUpdate {
    pub(crate) delay: Option<f64>,
    pub(crate) duration: Option<DurationUpdate>,
    pub(crate) end_delay: Option<f64>,
    pub(crate) fill: Option<FillMode>,
    pub(crate) iteration_start: Option<f64>,
    pub(crate) iterations: Option<IterationCount>,
    pub(crate) direction: Option<Direction>,
    pub(crate) easing: Option<Ease>,
}

impl TimingUpdate {
    /// Reads the milliseconds and names of the DOM's `OptionalEffectTiming`, checked by JS.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn parse(
        delay: Option<f64>,
        duration: Option<DurationUpdate>,
        end_delay: Option<f64>,
        fill: Option<&str>,
        iteration_start: Option<f64>,
        iterations: Option<f64>,
        direction: Option<&str>,
        easing: Option<&str>,
    ) -> Result<Self, String> {
        let type_error = |message: String| format!("{TYPE_ERROR}{message}");
        Ok(Self {
            delay: delay.map(|delay| delay / 1000.0),
            duration: duration.map(|duration| match duration {
                DurationUpdate::Auto => DurationUpdate::Auto,
                DurationUpdate::Seconds(millis) => DurationUpdate::Seconds(millis / 1000.0),
            }),
            end_delay: end_delay.map(|end_delay| end_delay / 1000.0),
            fill: fill
                .map(|fill| match fill {
                    "none" => Ok(FillMode::None),
                    "forwards" => Ok(FillMode::Forwards),
                    "backwards" => Ok(FillMode::Backwards),
                    "both" => Ok(FillMode::Both),
                    other => Err(type_error(format!("`{other}` is not a fill mode"))),
                })
                .transpose()?,
            iteration_start,
            iterations: iterations.map(|count| {
                if count.is_infinite() {
                    IterationCount::Infinite
                } else {
                    IterationCount::Finite(count)
                }
            }),
            direction: direction
                .map(|direction| match direction {
                    "normal" => Ok(Direction::Normal),
                    "reverse" => Ok(Direction::Reverse),
                    "alternate" => Ok(Direction::Alternate),
                    "alternate-reverse" => Ok(Direction::AlternateReverse),
                    other => Err(type_error(format!("`{other}` is not a direction"))),
                })
                .transpose()?,
            easing: easing
                .map(|easing| parse_timing_function(easing).map_err(|error| type_error(format!("easing: {error}"))))
                .transpose()?,
        })
    }

    /// Takes what `later` sets over what this does.
    pub(crate) fn merge(&mut self, later: &Self) {
        fn take<T: Clone>(slot: &mut Option<T>, value: &Option<T>) {
            if value.is_some() {
                *slot = value.clone();
            }
        }
        take(&mut self.delay, &later.delay);
        take(&mut self.duration, &later.duration);
        take(&mut self.end_delay, &later.end_delay);
        take(&mut self.fill, &later.fill);
        take(&mut self.iteration_start, &later.iteration_start);
        take(&mut self.iterations, &later.iterations);
        take(&mut self.direction, &later.direction);
        take(&mut self.easing, &later.easing);
    }

    /// `timing` with what this sets laid over it.
    pub(super) fn over(&self, mut timing: Timing) -> Timing {
        if let Some(delay) = self.delay {
            timing.delay = delay;
        }
        match self.duration {
            Some(DurationUpdate::Auto) => timing.duration = 0.0,
            Some(DurationUpdate::Seconds(duration)) => timing.duration = duration,
            None => {}
        }
        if let Some(end_delay) = self.end_delay {
            timing.end_delay = end_delay;
        }
        if let Some(fill) = self.fill {
            timing.fill = fill;
        }
        if let Some(start) = self.iteration_start {
            timing.iteration_start = start;
        }
        if let Some(iterations) = self.iterations {
            timing.iterations = iterations;
        }
        if let Some(direction) = self.direction {
            timing.direction = direction;
        }
        timing
    }
}

/// What a command refuses with when the DOM would throw a `TypeError`, which JS turns into one.
pub(crate) const TYPE_ERROR: &str = "TypeError: ";

/// What `getTiming()` and `getComputedTiming()` read of a run, in the DOM's milliseconds.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EffectInfo {
    pub(crate) delay: f64,
    /// `None` is `"auto"`.
    pub(crate) duration: Option<f64>,
    pub(crate) end_delay: f64,
    pub(crate) fill: &'static str,
    pub(crate) iteration_start: f64,
    /// Infinite for an effect that repeats for ever.
    pub(crate) iterations: f64,
    pub(crate) direction: &'static str,
    pub(crate) easing: String,
    pub(crate) end_time: f64,
    pub(crate) active_duration: f64,
    /// `None` for an idle run, which has no current time.
    pub(crate) local_time: Option<f64>,
    /// `None` outside the active phase unless a fill holds the effect.
    pub(crate) progress: Option<f64>,
    pub(crate) current_iteration: Option<u64>,
}

impl EffectInfo {
    /// `local` and `backwards_rate` are the current time of the run, in seconds, and whether it
    /// runs backwards; `duration_auto` says the duration was set to `auto`.
    pub(super) fn of(
        timing: &Timing,
        duration_auto: bool,
        easing: Option<&Ease>,
        local: Option<f64>,
        backwards_rate: bool,
    ) -> Result<Self, String> {
        let evaluation = local.map(|local| timing.evaluate(local, backwards_rate).eased(easing));
        let progress = evaluation.as_ref().and_then(|evaluation| evaluation.progress);
        Ok(Self {
            delay: timing.delay * 1000.0,
            duration: (!duration_auto).then_some(timing.duration * 1000.0),
            end_delay: timing.end_delay * 1000.0,
            fill: match timing.fill {
                FillMode::None => "none",
                FillMode::Forwards => "forwards",
                FillMode::Backwards => "backwards",
                FillMode::Both => "both",
            },
            iteration_start: timing.iteration_start,
            iterations: match timing.iterations {
                IterationCount::Infinite => f64::INFINITY,
                IterationCount::Finite(count) => count,
            },
            direction: match timing.direction {
                Direction::Normal => "normal",
                Direction::Reverse => "reverse",
                Direction::Alternate => "alternate",
                Direction::AlternateReverse => "alternate-reverse",
            },
            easing: match easing {
                Some(easing) => easing.to_css()?,
                None => "linear".to_string(),
            },
            end_time: timing.end_time() * 1000.0,
            active_duration: timing.active_duration() * 1000.0,
            local_time: local.map(|local| local * 1000.0),
            progress,
            current_iteration: progress.and(evaluation.map(|evaluation| evaluation.iteration)),
        })
    }
}

/// One keyframe as `getKeyframes()` shows it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct KeyframeInfo {
    pub(crate) offset: f64,
    pub(crate) easing: String,
    pub(crate) composite: &'static str,
    /// The camelCase property and its computed value, as Chrome writes it.
    pub(crate) values: Vec<(String, String)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timing() -> Timing {
        Timing {
            delay: 0.0,
            duration: 1.0,
            iterations: IterationCount::Finite(1.0),
            direction: Direction::Normal,
            fill: FillMode::None,
            end_delay: 0.0,
            iteration_start: 0.0,
        }
    }

    #[test]
    fn the_end_of_an_effect_counts_the_delay_the_iterations_and_the_end_delay() {
        let timing = Timing { delay: 0.5, iterations: IterationCount::Finite(2.0), end_delay: 0.25, ..timing() };
        assert_eq!(timing.active_duration(), 2.0);
        assert_eq!(timing.end_time(), 2.75);
        assert_eq!(Timing { iterations: IterationCount::Infinite, ..timing }.end_time(), f64::INFINITY);
        assert_eq!(Timing { duration: 0.0, iterations: IterationCount::Infinite, ..timing }.active_duration(), 0.0);
    }

    #[test]
    fn a_negative_end_delay_ends_the_effect_early_and_never_before_the_start() {
        let timing = Timing { end_delay: -0.25, ..timing() };
        assert_eq!(timing.end_time(), 0.75);
        assert_eq!(Timing { end_delay: -5.0, ..timing }.end_time(), 0.0);
    }

    #[test]
    fn the_phases_follow_the_boundaries_and_the_direction_the_time_came_from() {
        let timing = Timing { delay: 0.5, ..timing() };
        assert_eq!(timing.evaluate(0.25, false).phase, Phase::Before);
        assert_eq!(timing.evaluate(0.5, false).phase, Phase::Active);
        assert_eq!(timing.evaluate(0.5, true).phase, Phase::Before);
        assert_eq!(timing.evaluate(1.5, false).phase, Phase::After);
        assert_eq!(timing.evaluate(1.5, true).phase, Phase::Active);
    }

    #[test]
    fn a_fill_holds_the_effect_outside_the_active_phase_and_none_leaves_it_without() {
        let both = Timing { delay: 0.5, fill: FillMode::Both, ..timing() };
        assert_eq!(both.evaluate(0.0, false).progress, Some(0.0));
        assert_eq!(both.evaluate(9.0, false).progress, Some(1.0));
        let none = Timing { delay: 0.5, ..timing() };
        assert_eq!(none.evaluate(0.0, false).progress, None);
        assert_eq!(none.evaluate(9.0, false).progress, None);
    }

    #[test]
    fn the_progress_of_a_direction_alternates_by_the_iteration() {
        let timing = Timing { iterations: IterationCount::Finite(3.0), direction: Direction::Alternate, ..timing() };
        assert_eq!(timing.evaluate(0.25, false).progress, Some(0.25));
        assert_eq!(timing.evaluate(1.25, false).progress, Some(0.75));
        assert_eq!(timing.evaluate(1.25, false).iteration, 1);
        assert_eq!(timing.evaluate(2.25, false).progress, Some(0.25));
    }

    #[test]
    fn the_end_of_the_last_iteration_is_its_last_frame() {
        let timing = Timing { iterations: IterationCount::Finite(2.0), fill: FillMode::Forwards, ..timing() };
        let end = timing.evaluate(2.0, false);
        assert_eq!(end.progress, Some(1.0));
        assert_eq!(end.iteration, 1);
    }

    #[test]
    fn an_iteration_start_begins_part_way_through_an_iteration() {
        let timing = Timing { iteration_start: 0.5, fill: FillMode::Both, ..timing() };
        assert_eq!(timing.evaluate(0.0, false).progress, Some(0.5));
        assert_eq!(timing.evaluate(0.5, false).progress, Some(0.0));
        assert_eq!(timing.evaluate(0.5, false).iteration, 1);
        // The end of the active interval, at 1.5 iterations, is half an iteration in.
        assert_eq!(timing.evaluate(9.0, false).progress, Some(0.5));
    }

    #[test]
    fn an_end_delay_is_after_the_active_phase_with_no_effect_unless_filled() {
        let timing = Timing { end_delay: 0.5, ..timing() };
        assert_eq!(timing.evaluate(1.2, false).phase, Phase::After);
        assert_eq!(timing.evaluate(1.2, false).progress, None);
        assert_eq!(Timing { fill: FillMode::Forwards, ..timing }.evaluate(1.2, false).progress, Some(1.0));
    }

    #[test]
    fn an_update_lays_only_what_it_sets_over_the_timing() {
        let update = TimingUpdate {
            duration: Some(DurationUpdate::Seconds(2.0)),
            iterations: Some(IterationCount::Infinite),
            ..Default::default()
        };
        let timing = update.over(Timing { delay: 0.25, ..timing() });
        assert_eq!(timing.duration, 2.0);
        assert_eq!(timing.delay, 0.25);
        assert_eq!(timing.iterations, IterationCount::Infinite);
        let mut kept = update.clone();
        kept.merge(&TimingUpdate { duration: Some(DurationUpdate::Auto), ..Default::default() });
        assert_eq!(kept.duration, Some(DurationUpdate::Auto));
        assert_eq!(kept.iterations, Some(IterationCount::Infinite));
    }

    #[test]
    fn an_update_in_milliseconds_is_in_seconds_and_a_bad_name_is_a_type_error() {
        let update =
            TimingUpdate::parse(Some(250.0), Some(DurationUpdate::Seconds(1500.0)), None, Some("both"), None, None, None, None).unwrap();
        assert_eq!(update.delay, Some(0.25));
        assert_eq!(update.duration, Some(DurationUpdate::Seconds(1.5)));
        assert_eq!(update.fill, Some(FillMode::Both));
        let error = TimingUpdate::parse(None, None, None, Some("sideways"), None, None, None, None).unwrap_err();
        assert!(error.starts_with(TYPE_ERROR), "{error}");
        let error = TimingUpdate::parse(None, None, None, None, None, None, None, Some("nope")).unwrap_err();
        assert!(error.starts_with(TYPE_ERROR), "{error}");
    }

    #[test]
    fn what_a_run_reports_has_no_progress_or_iteration_without_a_current_time() {
        let info = EffectInfo::of(&timing(), false, None, None, false).unwrap();
        assert_eq!((info.local_time, info.progress, info.current_iteration), (None, None, None));
        assert_eq!((info.end_time, info.active_duration, info.easing.as_str()), (1000.0, 1000.0, "linear"));
        let info = EffectInfo::of(&timing(), false, None, Some(0.5), false).unwrap();
        assert_eq!((info.local_time, info.progress, info.current_iteration), (Some(500.0), Some(0.5), Some(0)));
    }

    #[test]
    fn an_effect_easing_shapes_the_progress_it_reports() {
        let easing = parse_timing_function("ease-in").unwrap();
        let info = EffectInfo::of(&timing(), false, Some(&easing), Some(0.5), false).unwrap();
        assert!((info.progress.unwrap() - 0.315).abs() < 0.001, "{:?}", info.progress);
        assert_eq!(info.easing, "ease-in");
    }
}
