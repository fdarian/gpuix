//! The Web Animations controls of CSS animations and CSS transitions: a stable
//! id, `pause()` and `play()`, `currentTime`, `playbackRate` and `playState`.
//!
//! [`Playback`] is the clock both kinds of run carry. Times are seconds from
//! the start of the animation, with the delay inside the effect, so
//! `currentTime` is 0 at the start whatever the delay. The clock keeps a base
//! time at an anchor instant and advances from it by `rate` while it ticks.
//! Reads and commands happen between frames, at the instant of the last frame
//! (the timeline time), never at `Instant::now`.

use std::sync::atomic::{AtomicU64, Ordering};

use web_time::Instant;

/// One counter for the process, so an id names one run across every window and
/// both kinds. Ids start at 1.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Running,
    Paused,
    Finished,
}

impl Status {
    /// The DOM `playState`. A run that was cancelled is gone, and JS reports it
    /// `idle` from its `cancel` event.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Finished => "finished",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Animation,
    Transition,
}

impl Kind {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Animation => "animation",
            Self::Transition => "transition",
        }
    }
}

/// What the API shows of one run.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Info {
    pub(crate) id: u64,
    pub(crate) kind: Kind,
    /// The `animation-name`, or the CSS name of the transitioned property.
    pub(crate) name: String,
    /// Milliseconds.
    pub(crate) current_time: f64,
    /// Milliseconds where the effect ends, delay included. Infinite for an
    /// animation that repeats forever.
    pub(crate) end_time: f64,
    pub(crate) playback_rate: f64,
    pub(crate) status: Status,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Command {
    Pause,
    Play,
    /// Milliseconds.
    SetCurrentTime(f64),
    SetPlaybackRate(f64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ApiEventKind {
    Finish,
    Cancel,
}

impl ApiEventKind {
    /// The internal event type JS maps to the animation's `finished` promise.
    pub(crate) fn event_type(self) -> &'static str {
        match self {
            Self::Finish => "webanimationfinish",
            Self::Cancel => "webanimationcancel",
        }
    }
}

/// A run finished or was cancelled. `element` is where it ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ApiEvent {
    pub(crate) element: u64,
    pub(crate) run: u64,
    pub(crate) kind: ApiEventKind,
}

#[derive(Clone, Debug)]
pub(super) struct Playback {
    id: u64,
    /// The current time at `anchor`.
    base: f64,
    anchor: Instant,
    rate: f64,
    /// Whether time moves. A style's `animation-play-state: paused` and the
    /// API's `pause()` both clear it.
    ticking: bool,
    /// `play()` was called on a held clock: the next frame starts it, from the
    /// time it holds.
    pending: bool,
    /// `pause()` or `play()` was called: the style's play state no longer
    /// decides whether the clock ticks (CSS Animations 2).
    controlled: bool,
    /// The finish of this stretch of time was reported.
    finish_reported: bool,
}

impl Playback {
    pub(super) fn start(now: Instant, ticking: bool) -> Self {
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            base: 0.0,
            anchor: now,
            rate: 1.0,
            ticking,
            pending: false,
            controlled: false,
            finish_reported: false,
        }
    }

    pub(super) fn id(&self) -> u64 {
        self.id
    }

    pub(super) fn rate(&self) -> f64 {
        self.rate
    }

    /// The time at `now` if nothing stopped it at an end of the effect. Event
    /// times are found from it.
    pub(super) fn unbounded(&self, now: Instant) -> f64 {
        if self.ticking {
            self.base + self.rate * now.saturating_duration_since(self.anchor).as_secs_f64()
        } else {
            self.base
        }
    }

    /// The current time: running time stops at `end` going forward and at 0
    /// going back, unless it was put beyond that by a seek.
    pub(super) fn current(&self, now: Instant, end: f64) -> f64 {
        let time = self.unbounded(now);
        if !self.ticking {
            return time;
        }
        if self.rate > 0.0 {
            time.min(end.max(self.base))
        } else if self.rate < 0.0 {
            time.max(self.base.min(0.0))
        } else {
            time
        }
    }

    /// When `time` was or will be, for ordering the events of different runs.
    pub(super) fn instant_at(&self, time: f64, now: Instant) -> Instant {
        if !self.ticking || self.rate == 0.0 {
            return now;
        }
        let behind = (self.unbounded(now) - time) / self.rate;
        if behind <= 0.0 {
            return now;
        }
        std::time::Duration::try_from_secs_f64(behind)
            .ok()
            .and_then(|behind| now.checked_sub(behind))
            .unwrap_or(now)
    }

    pub(super) fn is_finished(&self, now: Instant, end: f64) -> bool {
        let time = self.current(now, end);
        (self.rate > 0.0 && time >= end) || (self.rate < 0.0 && time <= 0.0)
    }

    pub(super) fn status(&self, now: Instant, end: f64) -> Status {
        if !self.ticking && !self.pending {
            Status::Paused
        } else if self.is_finished(now, end) {
            Status::Finished
        } else {
            Status::Running
        }
    }

    /// Whether the window needs a frame to carry this clock on: it moves toward
    /// an end, or `play()` waits for a frame to start it.
    pub(super) fn wants_frame(&self, now: Instant, end: f64) -> bool {
        self.pending || (self.ticking && self.rate != 0.0 && !self.is_finished(now, end))
    }

    /// True once per finish: when the run turns finished, until it is not.
    pub(super) fn take_finish(&mut self, now: Instant, end: f64) -> bool {
        let finished = self.status(now, end) == Status::Finished;
        let report = finished && !self.finish_reported;
        self.finish_reported = finished;
        report
    }

    /// A `play()` waiting for a frame starts at that frame's `now`.
    pub(super) fn resolve_pending(&mut self, now: Instant) {
        if self.pending {
            self.pending = false;
            self.ticking = true;
            self.anchor = now;
        }
    }

    /// The style's `animation-play-state`. It has no say once the API took over.
    pub(super) fn follow_style(&mut self, ticking: bool, now: Instant, end: f64) {
        if self.controlled || self.ticking == ticking {
            return;
        }
        if self.ticking {
            self.base = self.current(now, end);
            self.ticking = false;
        } else {
            self.anchor = now;
            self.ticking = true;
        }
    }

    pub(super) fn apply(&mut self, command: Command, now: Instant, end: f64) -> Result<(), String> {
        match command {
            Command::Pause => self.pause(now, end),
            Command::Play => return self.play(now, end),
            Command::SetCurrentTime(millis) => {
                if !millis.is_finite() {
                    return Err(format!("currentTime must be finite, got {millis}"));
                }
                self.base = millis / 1000.0;
                self.anchor = now;
            }
            Command::SetPlaybackRate(rate) => {
                if !rate.is_finite() {
                    return Err(format!("playbackRate must be finite, got {rate}"));
                }
                self.base = self.current(now, end);
                self.anchor = now;
                self.rate = rate;
            }
        }
        Ok(())
    }

    fn pause(&mut self, now: Instant, end: f64) {
        self.controlled = true;
        self.pending = false;
        if self.ticking {
            self.base = self.current(now, end);
            self.ticking = false;
        }
    }

    /// Restarts a finished run from its far end, as the Web Animations `play()`
    /// does, and starts a held one at the next frame.
    fn play(&mut self, now: Instant, end: f64) -> Result<(), String> {
        self.controlled = true;
        let rewind = self.is_finished(now, end);
        if rewind && self.rate < 0.0 && end.is_infinite() {
            return Err("cannot play an animation of infinite length backwards from its end".to_string());
        }
        if rewind {
            self.base = if self.rate > 0.0 { 0.0 } else { end };
            self.anchor = now;
            self.ticking = false;
        }
        if !self.ticking {
            self.pending = true;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn clock() -> (Playback, Instant) {
        let origin = Instant::now();
        (Playback::start(origin, true), origin)
    }

    fn at(origin: Instant, millis: u64) -> Instant {
        origin + Duration::from_millis(millis)
    }

    #[test]
    fn ids_are_distinct_per_clock_and_stay_with_it() {
        let (first, origin) = clock();
        let (second, _) = clock();
        assert_ne!(first.id(), second.id());
        let mut moved = first.clone();
        moved.apply(Command::SetPlaybackRate(2.0), at(origin, 10), 5.0).unwrap();
        assert_eq!(moved.id(), first.id());
    }

    #[test]
    fn time_runs_from_the_start_and_stops_at_the_end() {
        let (clock, origin) = clock();
        assert_eq!(clock.current(at(origin, 0), 2.0), 0.0);
        assert_eq!(clock.current(at(origin, 500), 2.0), 0.5);
        assert_eq!(clock.current(at(origin, 5000), 2.0), 2.0);
        assert_eq!(clock.unbounded(at(origin, 5000)), 5.0);
        assert_eq!(clock.status(at(origin, 500), 2.0), Status::Running);
        assert_eq!(clock.status(at(origin, 2000), 2.0), Status::Finished);
    }

    #[test]
    fn pause_holds_the_time_at_once() {
        let (mut clock, origin) = clock();
        clock.apply(Command::Pause, at(origin, 300), 2.0).unwrap();
        assert_eq!(clock.status(at(origin, 300), 2.0), Status::Paused);
        assert_eq!(clock.current(at(origin, 300), 2.0), 0.3);
        assert_eq!(clock.current(at(origin, 900), 2.0), 0.3);
        assert!(!clock.wants_frame(at(origin, 900), 2.0));
    }

    #[test]
    fn play_waits_for_a_frame_and_continues_from_the_held_time() {
        let (mut clock, origin) = clock();
        clock.apply(Command::Pause, at(origin, 300), 2.0).unwrap();
        clock.apply(Command::Play, at(origin, 600), 2.0).unwrap();
        // Pending: running as far as the state goes, still holding the time.
        assert_eq!(clock.status(at(origin, 600), 2.0), Status::Running);
        assert_eq!(clock.current(at(origin, 700), 2.0), 0.3);
        assert!(clock.wants_frame(at(origin, 700), 2.0));
        clock.resolve_pending(at(origin, 800));
        assert_eq!(clock.current(at(origin, 800), 2.0), 0.3);
        assert_eq!(clock.current(at(origin, 1000), 2.0), 0.5);
        assert!(!clock.pending);
    }

    #[test]
    fn seek_sets_the_time_at_once_and_keeps_the_play_state() {
        let (mut clock, origin) = clock();
        clock.apply(Command::SetCurrentTime(1500.0), at(origin, 100), 2.0).unwrap();
        assert_eq!(clock.current(at(origin, 100), 2.0), 1.5);
        assert_eq!(clock.current(at(origin, 300), 2.0), 1.7);

        clock.apply(Command::Pause, at(origin, 300), 2.0).unwrap();
        clock.apply(Command::SetCurrentTime(250.0), at(origin, 400), 2.0).unwrap();
        assert_eq!(clock.current(at(origin, 400), 2.0), 0.25);
        assert_eq!(clock.current(at(origin, 900), 2.0), 0.25);
        assert_eq!(clock.status(at(origin, 900), 2.0), Status::Paused);
    }

    #[test]
    fn a_seek_past_the_end_stays_there() {
        let (mut clock, origin) = clock();
        clock.apply(Command::SetCurrentTime(9000.0), at(origin, 0), 2.0).unwrap();
        assert_eq!(clock.current(at(origin, 500), 2.0), 9.0);
        assert_eq!(clock.status(at(origin, 500), 2.0), Status::Finished);
    }

    #[test]
    fn a_rate_scales_time_and_changing_it_keeps_the_time_continuous() {
        let (mut clock, origin) = clock();
        clock.apply(Command::SetPlaybackRate(2.0), at(origin, 0), 10.0).unwrap();
        assert_eq!(clock.current(at(origin, 500), 10.0), 1.0);
        clock.apply(Command::SetPlaybackRate(0.5), at(origin, 500), 10.0).unwrap();
        assert_eq!(clock.current(at(origin, 500), 10.0), 1.0);
        assert_eq!(clock.current(at(origin, 1500), 10.0), 1.5);
        assert_eq!(clock.rate(), 0.5);
    }

    #[test]
    fn a_negative_rate_runs_back_and_finishes_at_zero() {
        let (mut clock, origin) = clock();
        clock.apply(Command::SetCurrentTime(1000.0), at(origin, 0), 2.0).unwrap();
        clock.apply(Command::SetPlaybackRate(-1.0), at(origin, 0), 2.0).unwrap();
        assert_eq!(clock.current(at(origin, 400), 2.0), 0.6);
        assert!(!clock.is_finished(at(origin, 400), 2.0));
        assert_eq!(clock.current(at(origin, 3000), 2.0), 0.0);
        assert_eq!(clock.unbounded(at(origin, 3000)), -2.0);
        assert!(clock.is_finished(at(origin, 3000), 2.0));
        assert_eq!(clock.status(at(origin, 3000), 2.0), Status::Finished);
        assert!(!clock.wants_frame(at(origin, 3000), 2.0));
    }

    #[test]
    fn a_rate_of_zero_never_moves_or_finishes() {
        let (mut clock, origin) = clock();
        clock.apply(Command::SetPlaybackRate(0.0), at(origin, 200), 2.0).unwrap();
        assert_eq!(clock.current(at(origin, 9000), 2.0), 0.2);
        assert_eq!(clock.status(at(origin, 9000), 2.0), Status::Running);
        assert!(!clock.wants_frame(at(origin, 9000), 2.0));
    }

    #[test]
    fn finish_is_reported_once_until_the_time_leaves_the_end() {
        let (mut clock, origin) = clock();
        assert!(!clock.take_finish(at(origin, 1000), 2.0));
        assert!(clock.take_finish(at(origin, 2000), 2.0));
        assert!(!clock.take_finish(at(origin, 3000), 2.0));
        clock.apply(Command::SetCurrentTime(0.0), at(origin, 3000), 2.0).unwrap();
        assert!(!clock.take_finish(at(origin, 3500), 2.0));
        assert!(clock.take_finish(at(origin, 5000), 2.0));
    }

    #[test]
    fn a_paused_clock_at_the_end_is_paused_not_finished() {
        let (mut clock, origin) = clock();
        clock.apply(Command::Pause, at(origin, 0), 2.0).unwrap();
        clock.apply(Command::SetCurrentTime(2000.0), at(origin, 0), 2.0).unwrap();
        assert_eq!(clock.status(at(origin, 0), 2.0), Status::Paused);
        assert!(!clock.take_finish(at(origin, 0), 2.0));
    }

    #[test]
    fn play_on_a_finished_clock_rewinds_to_the_start() {
        let (mut clock, origin) = clock();
        assert_eq!(clock.status(at(origin, 3000), 2.0), Status::Finished);
        clock.apply(Command::Play, at(origin, 3000), 2.0).unwrap();
        assert_eq!(clock.current(at(origin, 3000), 2.0), 0.0);
        assert_eq!(clock.status(at(origin, 3000), 2.0), Status::Running);
        clock.resolve_pending(at(origin, 3100));
        assert_eq!(clock.current(at(origin, 3600), 2.0), 0.5);
    }

    #[test]
    fn play_on_a_finished_clock_running_back_rewinds_to_the_end() {
        let (mut clock, origin) = clock();
        clock.apply(Command::SetPlaybackRate(-1.0), at(origin, 0), 2.0).unwrap();
        assert_eq!(clock.status(at(origin, 100), 2.0), Status::Finished);
        clock.apply(Command::Play, at(origin, 100), 2.0).unwrap();
        clock.resolve_pending(at(origin, 200));
        assert_eq!(clock.current(at(origin, 200), 2.0), 2.0);
        assert_eq!(clock.current(at(origin, 700), 2.0), 1.5);
    }

    #[test]
    fn play_running_back_from_an_infinite_end_is_an_error() {
        let (mut clock, origin) = clock();
        clock.apply(Command::SetPlaybackRate(-1.0), at(origin, 0), f64::INFINITY).unwrap();
        assert!(clock.apply(Command::Play, at(origin, 0), f64::INFINITY).is_err());
    }

    #[test]
    fn a_running_clock_that_is_played_keeps_going() {
        let (mut clock, origin) = clock();
        clock.apply(Command::Play, at(origin, 500), 2.0).unwrap();
        assert!(!clock.pending);
        assert_eq!(clock.current(at(origin, 1000), 2.0), 1.0);
    }

    #[test]
    fn the_style_play_state_decides_until_the_api_takes_over() {
        let (mut clock, origin) = clock();
        clock.follow_style(false, at(origin, 400), 2.0);
        assert_eq!(clock.status(at(origin, 900), 2.0), Status::Paused);
        assert_eq!(clock.current(at(origin, 900), 2.0), 0.4);
        clock.follow_style(true, at(origin, 1000), 2.0);
        assert_eq!(clock.current(at(origin, 1500), 2.0), 0.9);

        clock.apply(Command::Pause, at(origin, 1500), 2.0).unwrap();
        clock.follow_style(true, at(origin, 1600), 2.0);
        assert_eq!(clock.status(at(origin, 2000), 2.0), Status::Paused);
        assert_eq!(clock.current(at(origin, 2000), 2.0), 0.9);

        clock.apply(Command::Play, at(origin, 2000), 2.0).unwrap();
        clock.resolve_pending(at(origin, 2100));
        clock.follow_style(false, at(origin, 2200), 2.0);
        assert_eq!(clock.status(at(origin, 2300), 2.0), Status::Running);
        assert_eq!(clock.current(at(origin, 2300), 2.0), 1.1);
    }

    #[test]
    fn event_times_follow_the_rate() {
        let (mut clock, origin) = clock();
        clock.apply(Command::SetPlaybackRate(2.0), at(origin, 0), 10.0).unwrap();
        // Time 1.0 is reached at 500 ms; the frame is at 800 ms (time 1.6).
        assert_eq!(clock.instant_at(1.0, at(origin, 800)), at(origin, 500));
        assert_eq!(clock.instant_at(2.0, at(origin, 800)), at(origin, 800));
    }
}
