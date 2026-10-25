//! The Web Animations controls of CSS animations and CSS transitions: a stable
//! id, `pause()`, `play()`, `cancel()`, `finish()`, `reverse()`, the playback
//! rate and its update, `currentTime`, `startTime`, `playState` and `pending`.
//!
//! [`Playback`] is the animation half of the Web Animations specification
//! (hold time, start time, pending play and pause tasks, pending playback
//! rate, the finished state), as Chrome runs it, minus the effect: what it
//! shows of the effect is told to it as the `end` of the effect. Times are
//! seconds, with the delay inside the effect, so `currentTime` is 0 at the
//! start whatever the delay. The timeline is the clock of the renderer, read
//! at the instant of the last frame: nothing moves between frames, and the
//! tasks a command schedules (`ready`) are run by the next frame that is
//! drawn, as the browser runs them at its next frame.

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use web_time::Instant;

use crate::dormant_wake::Wake;

/// One counter for the process, so an id names one run across every window and
/// both kinds. Ids start at 1.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

static NEXT_SEQ: AtomicU64 = AtomicU64::new(1);

/// A number that orders the events of every engine by when they were made, for the order they
/// reach JS in.
pub(crate) fn next_seq() -> u64 {
    NEXT_SEQ.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Running,
    Paused,
    Finished,
    /// Cancelled: only a [retired](Retired) run says so, a run the engine has
    /// is never idle.
    Idle,
}

impl Status {
    /// The DOM `playState`.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Finished => "finished",
            Self::Idle => "idle",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Animation,
    Transition,
    /// What a script made with `Element.animate()`: an animation of the motion engine that Motion
    /// would hand to the browser, see [`super::script`].
    Script,
}

impl Kind {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Animation => "animation",
            Self::Transition => "transition",
            Self::Script => "script",
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
    /// Milliseconds. Meaningless for an idle run, which has no current time.
    pub(crate) current_time: f64,
    /// Milliseconds where the effect ends, delay included. Infinite for an
    /// animation that repeats forever.
    pub(crate) end_time: f64,
    pub(crate) playback_rate: f64,
    pub(crate) status: Status,
    /// A play or pause is waiting for the next frame.
    pub(crate) pending: bool,
    /// The start time, in milliseconds from `at`: negative for one that began before.
    pub(crate) start_offset: Option<f64>,
    /// The instant the run was read at, the timeline time of everything above.
    pub(crate) at: Instant,
}

impl Info {
    pub(super) fn of(kind: Kind, name: String, clock: &Playback, now: Instant, end: f64) -> Self {
        let status = clock.status(now, end);
        Self {
            id: clock.id(),
            kind,
            name,
            current_time: clock.current_time(now, end).map_or(0.0, |time| time * 1000.0),
            end_time: end * 1000.0,
            playback_rate: clock.rate(),
            status,
            pending: clock.pending(),
            start_offset: clock.start_offset(now).map(|offset| offset * 1000.0),
            at: now,
        }
    }

    /// What a run showed when it was cancelled.
    pub(crate) fn cancelled(mut self) -> Self {
        self.status = Status::Idle;
        self.pending = false;
        self.start_offset = None;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Command {
    Pause,
    Play,
    Cancel,
    Finish,
    Reverse,
    /// Milliseconds.
    SetCurrentTime(f64),
    SetPlaybackRate(f64),
    UpdatePlaybackRate(f64),
    /// Milliseconds from the instant of the command; `None` clears it.
    SetStartTime(Option<f64>),
}

/// What a command refuses with, which JS turns into a `DOMException` of that name.
const INVALID_STATE: &str = "InvalidStateError: ";

fn invalid_state(message: &str) -> String {
    format!("{INVALID_STATE}{message}")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ApiEventKind {
    Finish,
    Cancel,
    /// A play or pause that waited for a frame has run: `ready` resolves.
    Ready,
}

impl ApiEventKind {
    /// The internal event type JS maps to the animation's promises and events.
    pub(crate) fn event_type(self) -> &'static str {
        match self {
            Self::Finish => "webanimationfinish",
            Self::Cancel => "webanimationcancel",
            Self::Ready => "webanimationready",
        }
    }
}

/// A run finished, was cancelled or became ready. `element` is where it ran, `info` what it
/// showed at that moment.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ApiEvent {
    pub(crate) element: u64,
    pub(crate) run: u64,
    pub(crate) kind: ApiEventKind,
    pub(crate) info: Info,
    /// See [`next_seq`].
    pub(crate) seq: u64,
}

impl ApiEvent {
    pub(crate) fn new(element: u64, kind: ApiEventKind, info: Info) -> Self {
        Self { element, run: info.id, kind, info, seq: next_seq() }
    }

    /// The event for a note a clock made.
    pub(super) fn of(element: u64, note: Note, info: Info) -> Self {
        let kind = match note {
            Note::Ready => ApiEventKind::Ready,
            Note::Finish => ApiEventKind::Finish,
            Note::Cancel => ApiEventKind::Cancel,
        };
        let info = if note == Note::Cancel { info.cancelled() } else { info };
        Self::new(element, kind, info)
    }
}

/// The runs whose finish or cancel was reported to JS, as of that moment, until
/// JS has consumed the event.
///
/// Events reach JS after the frame that made them, and an app can read the
/// animation in between; a transition that finished and a run that was
/// cancelled are no longer in their engine then. Each event is counted: JS
/// releases once per event it consumed, and the entry goes with the last.
#[derive(Debug, Default)]
pub(crate) struct Retired(HashMap<u64, Held>);

#[derive(Debug)]
struct Held {
    info: Info,
    /// Events of the run JS has not consumed yet.
    pending: usize,
}

impl Retired {
    pub(crate) fn retire(&mut self, info: Info) {
        match self.0.entry(info.id) {
            Entry::Occupied(mut entry) => {
                entry.get_mut().info = info;
                entry.get_mut().pending += 1;
            }
            Entry::Vacant(entry) => {
                entry.insert(Held { info, pending: 1 });
            }
        }
    }

    pub(crate) fn get(&self, id: u64) -> Option<&Info> {
        self.0.get(&id).map(|held| &held.info)
    }

    /// One event of `id` was consumed. Releasing an id with no entry is
    /// nothing: JS releases for every event, listed or not.
    pub(crate) fn release(&mut self, id: u64) {
        let Some(held) = self.0.get_mut(&id) else {
            return;
        };
        held.pending -= 1;
        if held.pending == 0 {
            self.0.remove(&id);
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }
}

/// A play or pause that waits for the next frame to run, the Web Animations "pending play task"
/// and "pending pause task".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Task {
    Play,
    Pause,
}

/// Something the clock did that the animation's promises and events are told of, in the order
/// it happened, see [`Playback::take_notes`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Note {
    /// A pending task ran: `ready` resolves.
    Ready,
    /// The animation became finished: the `finish` event and the `finished` promise.
    Finish,
    /// The animation became idle: the `cancel` event.
    Cancel,
}

/// Seconds from `earlier` to `later`, negative when `later` is the earlier of the two.
fn between(later: Instant, earlier: Instant) -> f64 {
    match later.checked_duration_since(earlier) {
        Some(duration) => duration.as_secs_f64(),
        None => -earlier.saturating_duration_since(later).as_secs_f64(),
    }
}

/// The animation of a run, as the specification has it. The timeline is the renderer's clock; a
/// time on it is seconds from `epoch`, the instant the clock was made.
#[derive(Clone, Debug)]
pub(super) struct Playback {
    id: u64,
    epoch: Instant,
    rate: f64,
    /// Set by `updatePlaybackRate` while a task waits, and by `reverse()`; the task applies it.
    pending_rate: Option<f64>,
    /// The current time while it does not follow the timeline.
    hold: Option<f64>,
    /// The time on the timeline at which the current time is 0.
    start: Option<f64>,
    task: Option<Task>,
    /// The current time as the last update of the finished state saw it.
    previous: Option<f64>,
    /// The finished state a `finish` event was made for.
    finished: bool,
    /// `pause()`, `play()` or `reverse()` was called: the style's `animation-play-state` no
    /// longer decides whether the animation is paused (CSS Animations 2).
    controlled: bool,
    style_paused: bool,
    /// The DOM `transitionend` of this stretch of time was reported.
    end_reported: bool,
    notes: Vec<Note>,
}

impl Playback {
    /// A run as the style engine makes it: played, which waits for the next frame to start it,
    /// and paused right after when the style says so. `end` is where its effect ends.
    pub(super) fn start(now: Instant, paused: bool, end: f64) -> Self {
        let mut clock = Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            epoch: now,
            rate: 1.0,
            pending_rate: None,
            hold: Some(0.0),
            start: None,
            task: Some(Task::Play),
            previous: None,
            finished: false,
            controlled: false,
            style_paused: paused,
            end_reported: false,
            notes: Vec::new(),
        };
        if paused {
            clock.pause(now, end).expect("pausing an animation that has a time cannot fail");
        }
        clock.notes.clear();
        clock
    }

    pub(super) fn id(&self) -> u64 {
        self.id
    }

    pub(super) fn rate(&self) -> f64 {
        self.rate
    }

    /// A play or pause waits for a frame.
    pub(super) fn pending(&self) -> bool {
        self.task.is_some()
    }

    /// A play waits for a frame: the animation has no start time yet, so nothing it would tell
    /// of its effect is told.
    pub(super) fn pending_play(&self) -> bool {
        self.task == Some(Task::Play)
    }

    pub(super) fn pending_pause(&self) -> bool {
        self.task == Some(Task::Pause)
    }

    /// Cancelled: no current time and nothing pending.
    pub(super) fn is_idle(&self) -> bool {
        self.hold.is_none() && self.start.is_none() && self.task.is_none()
    }

    /// Whether the time follows the timeline.
    pub(super) fn ticking(&self) -> bool {
        self.hold.is_none() && self.start.is_some()
    }

    fn at(&self, now: Instant) -> f64 {
        between(now, self.epoch)
    }

    fn effective_rate(&self) -> f64 {
        self.pending_rate.unwrap_or(self.rate)
    }

    /// The time the effect is at, seconds from the start of its delay, without the stop an end
    /// puts on it: a track that goes on past the end of its effect is sampled by it. `None` for an
    /// idle clock.
    pub(super) fn time(&self, now: Instant) -> Option<f64> {
        self.hold.or_else(|| self.calculated(now))
    }

    /// The time the start time gives, however long ago the clock was held at an end, or the held time of a clock with
    /// no start time.
    pub(super) fn run_time(&self, now: Instant) -> Option<f64> {
        self.calculated(now).or(self.hold)
    }

    /// The time from the start time, whatever holds it.
    fn calculated(&self, now: Instant) -> Option<f64> {
        self.start.map(|start| (self.at(now) - start) * self.rate)
    }

    /// The current time: the held one, or the one the timeline gives, which stops at an end of
    /// the effect for a clock that ran into it since the last frame.
    pub(super) fn current_time(&self, now: Instant, end: f64) -> Option<f64> {
        if let Some(hold) = self.hold {
            return Some(hold);
        }
        let time = self.calculated(now)?;
        Some(if self.rate > 0.0 {
            time.min(end)
        } else if self.rate < 0.0 {
            time.max(0.0)
        } else {
            time
        })
    }

    /// The current time of a run that is not idle.
    pub(super) fn current(&self, now: Instant, end: f64) -> f64 {
        self.current_time(now, end).expect("a clock that is not idle has a current time")
    }

    /// The start time, in seconds from `now`.
    pub(super) fn start_offset(&self, now: Instant) -> Option<f64> {
        self.start.map(|start| start - self.at(now))
    }

    /// The Web Animations play state.
    pub(super) fn status(&self, now: Instant, end: f64) -> Status {
        let current = self.current_time(now, end);
        if current.is_none() && self.task.is_none() {
            return Status::Idle;
        }
        if self.task == Some(Task::Pause) || (self.start.is_none() && self.task != Some(Task::Play)) {
            return Status::Paused;
        }
        let rate = self.effective_rate();
        match current {
            Some(time) if (rate > 0.0 && time >= end) || (rate < 0.0 && time <= 0.0) => Status::Finished,
            _ => Status::Running,
        }
    }

    pub(super) fn is_finished(&self, now: Instant, end: f64) -> bool {
        self.status(now, end) == Status::Finished
    }

    /// When `time` was or will be, for ordering the events of different runs.
    pub(super) fn instant_at(&self, time: f64, now: Instant) -> Instant {
        // A clock at an end holds its time and keeps the start time that got it there.
        let Some(start) = self.start.filter(|_| self.task.is_none()) else {
            return now;
        };
        if self.rate == 0.0 {
            return now;
        }
        let behind = self.at(now) - (start + time / self.rate);
        if behind <= 0.0 {
            return now;
        }
        std::time::Duration::try_from_secs_f64(behind)
            .ok()
            .and_then(|behind| now.checked_sub(behind))
            .unwrap_or(now)
    }

    /// When the clock reaches `time` by itself, going forward. A held clock never does; one that
    /// waits for a frame, or runs back, cannot say.
    pub(super) fn wake_at(&self, time: f64, now: Instant) -> Wake {
        if self.task.is_some() {
            return Wake::NextFrame;
        }
        let Some(current) = self.calculated(now).filter(|_| self.hold.is_none()) else {
            return Wake::Never;
        };
        if self.rate == 0.0 {
            return Wake::Never;
        }
        if self.rate < 0.0 {
            return Wake::NextFrame;
        }
        let wait = (time - current) / self.rate;
        if wait <= 0.0 {
            return Wake::At(now);
        }
        std::time::Duration::try_from_secs_f64(wait)
            .ok()
            .and_then(|wait| now.checked_add(wait))
            .map_or(Wake::Never, Wake::At)
    }

    /// Whether the window needs a frame to carry this clock on: it moves toward an end, or a
    /// play or pause waits for a frame to run.
    pub(super) fn wants_frame(&self, now: Instant, end: f64) -> bool {
        self.task.is_some() || (self.ticking() && self.rate != 0.0 && !self.is_finished(now, end))
    }

    /// What the clock did since the last call, in order.
    pub(super) fn take_notes(&mut self) -> Vec<Note> {
        std::mem::take(&mut self.notes)
    }

    /// True once per finish: when the run turns finished, until it is not. The DOM's
    /// `transitionend` is told by it, apart from the `finish` event of the animation.
    pub(super) fn take_end(&mut self, now: Instant, end: f64) -> bool {
        let finished = self.is_finished(now, end);
        let report = finished && !self.end_reported;
        self.end_reported = finished;
        report
    }

    /// The effect now ends at `end`, which `updateTiming()` moved: the finished state is looked at
    /// again against it.
    pub(super) fn retime(&mut self, now: Instant, end: f64) {
        self.update_finished(now, end, false);
        self.start_if_finished(now, end);
        if !self.is_finished(now, end) {
            self.end_reported = false;
        }
    }

    /// Notes the finish of a clock that ran into its end by itself, at a frame.
    pub(super) fn settle(&mut self, now: Instant, end: f64) {
        self.update_finished(now, end, false);
    }

    /// The pending play or pause runs, at the frame whose instant is `now`.
    pub(super) fn resolve_pending(&mut self, now: Instant, end: f64) {
        let Some(task) = self.task else {
            return;
        };
        let ready = self.at(now);
        match task {
            Task::Play => {
                if let Some(hold) = self.hold {
                    self.apply_pending_rate();
                    self.start = Some(if self.rate == 0.0 { ready } else { ready - hold / self.rate });
                    if self.rate != 0.0 {
                        self.hold = None;
                    }
                } else if let (Some(start), Some(_)) = (self.start, self.pending_rate) {
                    let to_match = (ready - start) * self.rate;
                    self.apply_pending_rate();
                    if self.rate == 0.0 {
                        self.hold = Some(to_match);
                    }
                    self.start = Some(if self.rate == 0.0 { ready } else { ready - to_match / self.rate });
                }
            }
            Task::Pause => {
                if let (Some(start), None) = (self.start, self.hold) {
                    self.hold = Some((ready - start) * self.rate);
                }
                self.apply_pending_rate();
                self.start = None;
            }
        }
        self.task = None;
        self.notes.push(Note::Ready);
        self.update_finished(now, end, false);
    }

    fn apply_pending_rate(&mut self) {
        if let Some(rate) = self.pending_rate.take() {
            self.rate = rate;
        }
    }

    /// The specification's "update an animation's finished state", with Chrome's reading of the
    /// time that a seek leaves: a time put beyond an end by a seek stays there.
    fn update_finished(&mut self, now: Instant, end: f64, did_seek: bool) {
        let unconstrained = if did_seek { self.hold.or_else(|| self.calculated(now)) } else { self.calculated(now) };
        if let (Some(time), Some(_), None) = (unconstrained, self.start, self.task) {
            let rate = self.effective_rate();
            if rate > 0.0 && time >= end {
                self.hold = Some(if did_seek { time } else { self.previous.map_or(end, |previous| previous.max(end)) });
            } else if rate < 0.0 && time <= 0.0 {
                self.hold = Some(if did_seek { time } else { self.previous.map_or(0.0, |previous| previous.min(0.0)) });
            } else if rate != 0.0 {
                if let (true, Some(hold)) = (did_seek, self.hold) {
                    self.start = Some(self.at(now) - hold / rate);
                }
                self.hold = None;
            }
        }
        self.previous = self.current_time(now, end);
        let finished = self.is_finished(now, end);
        if finished && !self.finished {
            self.notes.push(Note::Finish);
        }
        self.finished = finished;
    }

    /// A time on the clock itself, as `currentTime` sets it: no task is told of it.
    fn silently_set(&mut self, now: Instant, time: f64) {
        if self.hold.is_some() || self.start.is_none() || self.rate == 0.0 {
            self.hold = Some(time);
        } else {
            self.start = Some(self.at(now) - time / self.rate);
        }
        self.previous = None;
    }

    fn set_current_time(&mut self, time: f64, now: Instant, end: f64) {
        self.silently_set(now, time);
        if self.task == Some(Task::Pause) {
            self.resolve_pending(now, end);
        }
        self.update_finished(now, end, true);
        self.start_if_finished(now, end);
    }

    /// A play that waits for a frame, left finished by a seek or a rate, is done at once:
    /// Chrome has nothing to wait for then.
    fn start_if_finished(&mut self, now: Instant, end: f64) {
        if self.task == Some(Task::Play) && self.is_finished(now, end) {
            self.resolve_pending(now, end);
        }
    }

    fn set_playback_rate(&mut self, rate: f64, now: Instant, end: f64) {
        self.pending_rate = None;
        let previous = self.current_time(now, end);
        self.rate = rate;
        if let Some(previous) = previous {
            self.set_current_time(previous, now, end);
        }
    }

    fn update_playback_rate(&mut self, rate: f64, now: Instant, end: f64) -> Result<(), String> {
        let before = self.status(now, end);
        self.pending_rate = Some(rate);
        if self.task.is_some() {
            return Ok(());
        }
        if matches!(before, Status::Idle | Status::Paused) || self.current_time(now, end).is_none() {
            self.apply_pending_rate();
            return Ok(());
        }
        if before == Status::Finished {
            self.apply_pending_rate();
            if rate == 0.0 {
                self.start = Some(self.at(now));
            }
            self.update_finished(now, end, false);
            return Ok(());
        }
        self.play(false, now, end)
    }

    fn set_start_time(&mut self, offset: Option<f64>, now: Instant, end: f64) {
        let previous = self.current_time(now, end);
        self.apply_pending_rate();
        self.start = offset.map(|offset| self.at(now) + offset / 1000.0);
        if self.start.is_some() {
            self.hold = None;
        } else if previous.is_some() {
            self.hold = previous;
        }
        self.task = None;
        self.update_finished(now, end, true);
    }

    fn play(&mut self, auto_rewind: bool, now: Instant, end: f64) -> Result<(), String> {
        let aborted_pause = self.task == Some(Task::Pause);
        let rate = self.effective_rate();
        let current = self.current_time(now, end);
        let seek = if auto_rewind && rate > 0.0 && current.is_none_or(|time| time < 0.0 || time >= end) {
            Some(0.0)
        } else if auto_rewind && rate < 0.0 && current.is_none_or(|time| time <= 0.0 || time > end) {
            if end.is_infinite() {
                return Err(invalid_state("Cannot play an animation of infinite length backwards from its end"));
            }
            Some(end)
        } else if rate == 0.0 && current.is_none() {
            Some(0.0)
        } else {
            None
        };
        if let Some(seek) = seek {
            self.hold = Some(seek);
        }
        if self.hold.is_some() {
            self.start = None;
        }
        self.task = None;
        if self.hold.is_none() && seek.is_none() && !aborted_pause && self.pending_rate.is_none() {
            return Ok(());
        }
        self.task = Some(Task::Play);
        self.update_finished(now, end, false);
        Ok(())
    }

    fn pause(&mut self, now: Instant, end: f64) -> Result<(), String> {
        if self.task == Some(Task::Pause) || self.status(now, end) == Status::Paused {
            return Ok(());
        }
        if self.current_time(now, end).is_none() {
            if self.rate >= 0.0 {
                self.hold = Some(0.0);
            } else if end.is_infinite() {
                return Err(invalid_state("Cannot pause an animation of infinite length backwards from its end"));
            } else {
                self.hold = Some(end);
            }
        }
        // The time is held at once, where the specification holds it when the frame comes: a
        // frame is the instant the clock reads, so the two are the same time.
        if self.hold.is_none() {
            self.hold = self.current_time(now, end);
        }
        self.task = Some(Task::Pause);
        self.update_finished(now, end, false);
        Ok(())
    }

    fn cancel(&mut self, now: Instant, end: f64) {
        if self.status(now, end) == Status::Idle {
            return;
        }
        if self.task.take().is_some() {
            self.apply_pending_rate();
        }
        self.hold = None;
        self.start = None;
        self.previous = None;
        self.finished = false;
        self.notes.push(Note::Cancel);
    }

    fn finish(&mut self, now: Instant, end: f64) -> Result<(), String> {
        let rate = self.effective_rate();
        if rate == 0.0 {
            return Err(invalid_state("Cannot finish an animation with a playback rate of 0"));
        }
        if rate > 0.0 && end.is_infinite() {
            return Err(invalid_state("Cannot finish an animation with an infinite target effect end"));
        }
        self.apply_pending_rate();
        let limit = if self.rate > 0.0 { end } else { 0.0 };
        self.silently_set(now, limit);
        if self.start.is_none() {
            self.start = Some(self.at(now) - limit / self.rate);
        }
        if self.task == Some(Task::Pause) {
            self.hold = None;
        }
        self.task = None;
        self.update_finished(now, end, true);
        Ok(())
    }

    fn reverse(&mut self, now: Instant, end: f64) -> Result<(), String> {
        let original = self.pending_rate;
        // `0.0 -` and not a negation: a rate of 0 reverses to 0, not -0.
        self.pending_rate = Some(0.0 - self.effective_rate());
        let outcome = self.play(true, now, end);
        if outcome.is_err() {
            self.pending_rate = original;
        }
        outcome
    }

    /// The style's `animation-play-state`. It has no say once the API took over.
    pub(super) fn follow_style(&mut self, paused: bool, now: Instant, end: f64) {
        if self.controlled || self.style_paused == paused {
            return;
        }
        self.style_paused = paused;
        let outcome = if paused { self.pause(now, end) } else { self.play(true, now, end) };
        if let Err(error) = outcome {
            log::warn!("animation-play-state cannot be followed: {error}");
        }
    }

    pub(super) fn apply(&mut self, command: Command, now: Instant, end: f64) -> Result<(), String> {
        let outcome = self.run(command, now, end);
        // A finish that a command undid and redid before any frame saw it is a new finish.
        if !self.is_finished(now, end) {
            self.end_reported = false;
        }
        outcome
    }

    fn run(&mut self, command: Command, now: Instant, end: f64) -> Result<(), String> {
        match command {
            Command::Pause => {
                self.controlled = true;
                self.pause(now, end)
            }
            Command::Play => {
                self.controlled = true;
                self.play(true, now, end)
            }
            Command::Reverse => {
                self.controlled = true;
                self.reverse(now, end)
            }
            Command::Cancel => {
                self.cancel(now, end);
                Ok(())
            }
            Command::Finish => self.finish(now, end),
            Command::SetCurrentTime(millis) => {
                if !millis.is_finite() {
                    return Err(format!("currentTime must be finite, got {millis}"));
                }
                self.set_current_time(millis / 1000.0, now, end);
                Ok(())
            }
            Command::SetPlaybackRate(rate) => {
                if !rate.is_finite() {
                    return Err(format!("playbackRate must be finite, got {rate}"));
                }
                self.set_playback_rate(rate, now, end);
                Ok(())
            }
            Command::UpdatePlaybackRate(rate) => {
                if !rate.is_finite() {
                    return Err(format!("playbackRate must be finite, got {rate}"));
                }
                self.update_playback_rate(rate, now, end)
            }
            Command::SetStartTime(offset) => {
                if offset.is_some_and(|offset| !offset.is_finite()) {
                    return Err("startTime must be finite".to_string());
                }
                self.set_start_time(offset, now, end);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    const END: f64 = 2.0;

    /// A clock started at `origin` and already running, with the effect ending at `END`.
    fn clock() -> (Playback, Instant) {
        let origin = Instant::now();
        let mut clock = Playback::start(origin, false, END);
        clock.resolve_pending(origin, END);
        clock.take_notes();
        (clock, origin)
    }

    fn at(origin: Instant, millis: u64) -> Instant {
        origin + Duration::from_millis(millis)
    }

    fn apply(clock: &mut Playback, command: Command, now: Instant) {
        clock.apply(command, now, END).unwrap();
    }

    /// The current time, to the nanosecond: the clock adds up seconds as floats.
    fn time(clock: &Playback, now: Instant) -> f64 {
        (clock.current(now, END) * 1e9).round() / 1e9
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
    fn a_new_clock_waits_for_a_frame_to_start() {
        let origin = Instant::now();
        let mut clock = Playback::start(origin, false, END);
        assert!(clock.pending());
        assert!(clock.pending_play());
        assert_eq!(clock.status(origin, END), Status::Running);
        assert_eq!(clock.current_time(at(origin, 300), END), Some(0.0), "it holds 0 until a frame starts it");
        assert_eq!(clock.start_offset(origin), None);
        clock.resolve_pending(at(origin, 100), END);
        assert!(!clock.pending());
        assert_eq!(clock.take_notes(), [Note::Ready]);
        assert_eq!(clock.current(at(origin, 100), END), 0.0);
        assert_eq!(clock.current(at(origin, 600), END), 0.5);
        assert_eq!(clock.start_offset(at(origin, 600)), Some(-0.5));
    }

    #[test]
    fn a_paused_style_starts_a_clock_with_a_pause_waiting_for_the_frame() {
        let origin = Instant::now();
        let mut clock = Playback::start(origin, true, END);
        assert_eq!(clock.status(origin, END), Status::Paused);
        assert!(clock.pending_pause());
        clock.resolve_pending(at(origin, 50), END);
        assert_eq!(clock.status(at(origin, 900), END), Status::Paused);
        assert_eq!(clock.current(at(origin, 900), END), 0.0);
        assert_eq!(clock.start_offset(origin), None);
    }

    #[test]
    fn time_runs_from_the_start_and_stops_at_the_end() {
        let (clock, origin) = clock();
        assert_eq!(time(&clock, at(origin, 0)), 0.0);
        assert_eq!(time(&clock, at(origin, 500)), 0.5);
        assert_eq!(time(&clock, at(origin, 5000)), 2.0);
        assert_eq!(clock.status(at(origin, 500), END), Status::Running);
        assert_eq!(clock.status(at(origin, 2000), END), Status::Finished);
    }

    #[test]
    fn a_clock_that_runs_reaches_a_time_at_the_instant_it_says() {
        let (mut clock, origin) = clock();
        assert_eq!(clock.wake_at(2.0, at(origin, 500)), Wake::At(at(origin, 2000)));
        assert_eq!(clock.wake_at(2.0, at(origin, 2500)), Wake::At(at(origin, 2500)), "past is now");
        clock.apply(Command::SetPlaybackRate(2.0), at(origin, 1000), 10.0).unwrap();
        // At 1 s of time, 3 s to go at twice the pace takes 1.5 s.
        assert_eq!(clock.wake_at(4.0, at(origin, 1000)), Wake::At(at(origin, 2500)));
        assert_eq!(clock.wake_at(f64::INFINITY, at(origin, 1000)), Wake::Never, "it never gets there");
    }

    #[test]
    fn a_clock_that_is_held_or_cannot_say_has_no_time_it_reaches() {
        let (mut clock, origin) = clock();
        apply(&mut clock, Command::Pause, at(origin, 300));
        assert_eq!(clock.wake_at(2.0, at(origin, 300)), Wake::NextFrame, "the pause waits for a frame");
        clock.resolve_pending(at(origin, 350), END);
        assert_eq!(clock.wake_at(2.0, at(origin, 400)), Wake::Never);
        apply(&mut clock, Command::Play, at(origin, 600));
        assert_eq!(clock.wake_at(2.0, at(origin, 600)), Wake::NextFrame, "it waits for a frame");
        clock.resolve_pending(at(origin, 700), END);
        apply(&mut clock, Command::SetPlaybackRate(-1.0), at(origin, 800));
        assert_eq!(clock.wake_at(2.0, at(origin, 800)), Wake::NextFrame, "it runs back");
    }

    #[test]
    fn pause_holds_the_time_at_once_and_the_frame_lets_go_of_the_start_time() {
        let (mut clock, origin) = clock();
        apply(&mut clock, Command::Pause, at(origin, 300));
        assert_eq!(clock.status(at(origin, 300), END), Status::Paused);
        assert!(clock.pending());
        assert_eq!(time(&clock, at(origin, 300)), 0.3);
        assert_eq!(time(&clock, at(origin, 900)), 0.3);
        assert!(clock.start_offset(at(origin, 300)).is_some(), "the start time goes with the frame");
        clock.resolve_pending(at(origin, 320), END);
        assert_eq!(clock.start_offset(at(origin, 320)), None);
        assert_eq!(time(&clock, at(origin, 900)), 0.3);
        assert!(!clock.wants_frame(at(origin, 900), END));
    }

    #[test]
    fn play_waits_for_a_frame_and_continues_from_the_held_time() {
        let (mut clock, origin) = clock();
        apply(&mut clock, Command::Pause, at(origin, 300));
        clock.resolve_pending(at(origin, 300), END);
        apply(&mut clock, Command::Play, at(origin, 600));
        // Pending: running as far as the state goes, still holding the time.
        assert_eq!(clock.status(at(origin, 600), END), Status::Running);
        assert_eq!(time(&clock, at(origin, 700)), 0.3);
        assert!(clock.wants_frame(at(origin, 700), END));
        clock.resolve_pending(at(origin, 800), END);
        assert_eq!(time(&clock, at(origin, 800)), 0.3);
        assert_eq!(time(&clock, at(origin, 1000)), 0.5);
        assert!(!clock.pending());
    }

    #[test]
    fn seek_sets_the_time_at_once_and_keeps_the_play_state() {
        let (mut clock, origin) = clock();
        apply(&mut clock, Command::SetCurrentTime(1500.0), at(origin, 100));
        assert_eq!(time(&clock, at(origin, 100)), 1.5);
        assert_eq!(time(&clock, at(origin, 300)), 1.7);

        apply(&mut clock, Command::Pause, at(origin, 300));
        apply(&mut clock, Command::SetCurrentTime(250.0), at(origin, 400));
        // Seeking a pause that waits for a frame is the frame: the pause is done.
        assert!(!clock.pending());
        assert_eq!(time(&clock, at(origin, 400)), 0.25);
        assert_eq!(time(&clock, at(origin, 900)), 0.25);
        assert_eq!(clock.status(at(origin, 900), END), Status::Paused);
    }

    #[test]
    fn a_seek_past_the_end_stays_there() {
        let (mut clock, origin) = clock();
        apply(&mut clock, Command::SetCurrentTime(9000.0), at(origin, 0));
        assert_eq!(time(&clock, at(origin, 500)), 9.0);
        assert_eq!(clock.status(at(origin, 500), END), Status::Finished);
    }

    #[test]
    fn a_seek_of_a_play_that_waits_for_a_frame_is_still_waiting_unless_it_finishes() {
        let origin = Instant::now();
        let mut clock = Playback::start(origin, false, END);
        apply(&mut clock, Command::SetCurrentTime(300.0), origin);
        assert!(clock.pending());
        assert_eq!(time(&clock, origin), 0.3);
        apply(&mut clock, Command::SetCurrentTime(2000.0), origin);
        assert!(!clock.pending(), "it is finished, which has nothing to wait for");
        assert_eq!(clock.status(origin, END), Status::Finished);
        assert!(clock.start_offset(origin).is_some());
        assert_eq!(clock.take_notes(), [Note::Finish, Note::Ready]);
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
        apply(&mut clock, Command::SetCurrentTime(1000.0), at(origin, 0));
        apply(&mut clock, Command::SetPlaybackRate(-1.0), at(origin, 0));
        assert_eq!(time(&clock, at(origin, 400)), 0.6);
        assert!(!clock.is_finished(at(origin, 400), END));
        assert_eq!(time(&clock, at(origin, 3000)), 0.0);
        assert!(clock.is_finished(at(origin, 3000), END));
        assert_eq!(clock.status(at(origin, 3000), END), Status::Finished);
        assert!(!clock.wants_frame(at(origin, 3000), END));
    }

    #[test]
    fn a_rate_of_zero_never_moves_or_finishes() {
        let (mut clock, origin) = clock();
        apply(&mut clock, Command::SetPlaybackRate(0.0), at(origin, 200));
        assert_eq!(time(&clock, at(origin, 9000)), 0.2);
        assert_eq!(clock.status(at(origin, 9000), END), Status::Running);
        assert!(!clock.wants_frame(at(origin, 9000), END));
    }

    #[test]
    fn a_finish_is_noted_once_until_the_time_leaves_the_end() {
        let (mut clock, origin) = clock();
        clock.update_finished(at(origin, 1000), END, false);
        assert!(clock.take_notes().is_empty());
        clock.update_finished(at(origin, 2000), END, false);
        assert_eq!(clock.take_notes(), [Note::Finish]);
        clock.update_finished(at(origin, 3000), END, false);
        assert!(clock.take_notes().is_empty());
        apply(&mut clock, Command::SetCurrentTime(0.0), at(origin, 3000));
        assert!(clock.take_notes().is_empty());
        clock.update_finished(at(origin, 3500), END, false);
        clock.update_finished(at(origin, 5000), END, false);
        assert_eq!(clock.take_notes(), [Note::Finish]);
    }

    #[test]
    fn a_finish_undone_and_redone_before_a_frame_is_noted_again() {
        let (mut clock, origin) = clock();
        clock.update_finished(at(origin, 2000), END, false);
        clock.take_notes();
        apply(&mut clock, Command::SetCurrentTime(500.0), at(origin, 2000));
        apply(&mut clock, Command::SetCurrentTime(2000.0), at(origin, 2000));
        assert_eq!(clock.take_notes(), [Note::Finish]);
    }

    #[test]
    fn a_command_that_leaves_a_finished_clock_finished_notes_nothing_new() {
        let (mut clock, origin) = clock();
        clock.update_finished(at(origin, 2000), END, false);
        clock.take_notes();
        apply(&mut clock, Command::SetPlaybackRate(2.0), at(origin, 2000));
        assert!(clock.take_notes().is_empty());
    }

    #[test]
    fn a_paused_clock_at_the_end_is_paused_not_finished() {
        let (mut clock, origin) = clock();
        apply(&mut clock, Command::Pause, at(origin, 0));
        apply(&mut clock, Command::SetCurrentTime(2000.0), at(origin, 0));
        assert_eq!(clock.status(at(origin, 0), END), Status::Paused);
        assert!(clock.take_notes().iter().all(|note| *note != Note::Finish));
    }

    #[test]
    fn play_on_a_finished_clock_rewinds_to_the_start() {
        let (mut clock, origin) = clock();
        assert_eq!(clock.status(at(origin, 3000), END), Status::Finished);
        clock.update_finished(at(origin, 3000), END, false);
        apply(&mut clock, Command::Play, at(origin, 3000));
        assert_eq!(time(&clock, at(origin, 3000)), 0.0);
        assert_eq!(clock.status(at(origin, 3000), END), Status::Running);
        clock.resolve_pending(at(origin, 3100), END);
        assert_eq!(time(&clock, at(origin, 3600)), 0.5);
    }

    #[test]
    fn play_on_a_finished_clock_running_back_rewinds_to_the_end() {
        let (mut clock, origin) = clock();
        apply(&mut clock, Command::SetPlaybackRate(-1.0), at(origin, 0));
        assert_eq!(clock.status(at(origin, 100), END), Status::Finished);
        apply(&mut clock, Command::Play, at(origin, 100));
        clock.resolve_pending(at(origin, 200), END);
        assert_eq!(time(&clock, at(origin, 200)), 2.0);
        assert_eq!(time(&clock, at(origin, 700)), 1.5);
    }

    #[test]
    fn play_running_back_from_an_infinite_end_is_an_error() {
        let (mut clock, origin) = clock();
        clock.apply(Command::SetPlaybackRate(-1.0), at(origin, 0), f64::INFINITY).unwrap();
        let error = clock.apply(Command::Play, at(origin, 0), f64::INFINITY).unwrap_err();
        assert!(error.starts_with(INVALID_STATE), "{error}");
    }

    #[test]
    fn a_running_clock_that_is_played_keeps_going() {
        let (mut clock, origin) = clock();
        apply(&mut clock, Command::Play, at(origin, 500));
        assert!(!clock.pending());
        assert_eq!(time(&clock, at(origin, 1000)), 1.0);
    }

    #[test]
    fn the_style_play_state_decides_until_the_api_takes_over() {
        let (mut clock, origin) = clock();
        clock.follow_style(true, at(origin, 400), END);
        clock.resolve_pending(at(origin, 400), END);
        assert_eq!(clock.status(at(origin, 900), END), Status::Paused);
        assert_eq!(time(&clock, at(origin, 900)), 0.4);
        clock.follow_style(false, at(origin, 1000), END);
        clock.resolve_pending(at(origin, 1000), END);
        assert_eq!(time(&clock, at(origin, 1500)), 0.9);

        apply(&mut clock, Command::Pause, at(origin, 1500));
        clock.resolve_pending(at(origin, 1500), END);
        clock.follow_style(false, at(origin, 1600), END);
        clock.follow_style(true, at(origin, 1600), END);
        assert_eq!(clock.status(at(origin, 2000), END), Status::Paused);
        assert_eq!(time(&clock, at(origin, 2000)), 0.9);

        apply(&mut clock, Command::Play, at(origin, 2000));
        clock.resolve_pending(at(origin, 2100), END);
        clock.follow_style(true, at(origin, 2200), END);
        assert_eq!(clock.status(at(origin, 2300), END), Status::Running);
        assert_eq!(time(&clock, at(origin, 2300)), 1.1);
    }

    #[test]
    fn event_times_follow_the_rate() {
        let (mut clock, origin) = clock();
        clock.apply(Command::SetPlaybackRate(2.0), at(origin, 0), 10.0).unwrap();
        // Time 1.0 is reached at 500 ms; the frame is at 800 ms (time 1.6).
        assert_eq!(clock.instant_at(1.0, at(origin, 800)), at(origin, 500));
        assert_eq!(clock.instant_at(2.0, at(origin, 800)), at(origin, 800));
    }

    #[test]
    fn cancel_makes_the_clock_idle_and_play_brings_it_back_from_the_start() {
        let (mut clock, origin) = clock();
        apply(&mut clock, Command::SetPlaybackRate(-1.0), at(origin, 0));
        clock.take_notes();
        apply(&mut clock, Command::Cancel, at(origin, 100));
        assert_eq!(clock.status(at(origin, 100), END), Status::Idle);
        assert!(clock.is_idle());
        assert_eq!(clock.current_time(at(origin, 100), END), None);
        assert_eq!(clock.rate(), -1.0, "the rate is the animation's, not the run's");
        assert_eq!(clock.take_notes(), [Note::Cancel]);
        apply(&mut clock, Command::Cancel, at(origin, 100));
        assert!(clock.take_notes().is_empty(), "cancelling an idle animation does nothing");
        apply(&mut clock, Command::Play, at(origin, 100));
        assert_eq!(time(&clock, at(origin, 100)), 2.0, "it runs back from its end");
        assert!(clock.pending());
    }

    #[test]
    fn finish_goes_to_the_end_the_rate_runs_toward_and_refuses_what_never_ends() {
        let (mut clock, origin) = clock();
        apply(&mut clock, Command::Finish, at(origin, 100));
        assert_eq!(time(&clock, at(origin, 100)), 2.0);
        assert_eq!(clock.take_notes(), [Note::Finish]);
        apply(&mut clock, Command::SetPlaybackRate(-1.0), at(origin, 100));
        apply(&mut clock, Command::SetCurrentTime(900.0), at(origin, 100));
        apply(&mut clock, Command::Finish, at(origin, 100));
        assert_eq!(time(&clock, at(origin, 100)), 0.0);

        apply(&mut clock, Command::SetPlaybackRate(0.0), at(origin, 100));
        let error = clock.apply(Command::Finish, at(origin, 100), END).unwrap_err();
        assert!(error.starts_with(INVALID_STATE), "{error}");
        apply(&mut clock, Command::SetPlaybackRate(1.0), at(origin, 100));
        assert!(clock.apply(Command::Finish, at(origin, 100), f64::INFINITY).is_err());
    }

    #[test]
    fn reverse_flips_the_rate_when_the_frame_comes_and_a_play_at_the_start_goes_to_the_end() {
        let (mut clock, origin) = clock();
        apply(&mut clock, Command::SetCurrentTime(300.0), at(origin, 0));
        apply(&mut clock, Command::Reverse, at(origin, 0));
        assert_eq!(clock.rate(), 1.0, "the rate is pending");
        assert!(clock.pending());
        clock.resolve_pending(at(origin, 10), END);
        assert_eq!(clock.rate(), -1.0);
        // The time ran on at the old rate until the frame.
        assert_eq!(time(&clock, at(origin, 10)), 0.31);
        assert_eq!(time(&clock, at(origin, 110)), 0.21);

        let (mut fresh, origin) = self::clock();
        apply(&mut fresh, Command::Reverse, at(origin, 0));
        assert_eq!(time(&fresh, at(origin, 0)), 2.0);
    }

    #[test]
    fn update_playback_rate_waits_for_the_frame_of_a_running_clock_and_keeps_its_time() {
        let (mut clock, origin) = clock();
        apply(&mut clock, Command::SetCurrentTime(400.0), at(origin, 0));
        apply(&mut clock, Command::UpdatePlaybackRate(2.0), at(origin, 0));
        assert_eq!(clock.rate(), 1.0);
        assert!(clock.pending());
        clock.resolve_pending(at(origin, 50), END);
        assert_eq!(clock.rate(), 2.0);
        assert_eq!(time(&clock, at(origin, 50)), 0.45);
        assert_eq!(time(&clock, at(origin, 100)), 0.55);
    }

    #[test]
    fn update_playback_rate_is_at_once_for_a_paused_or_idle_clock() {
        let (mut clock, origin) = clock();
        apply(&mut clock, Command::Pause, at(origin, 0));
        clock.resolve_pending(at(origin, 0), END);
        apply(&mut clock, Command::UpdatePlaybackRate(3.0), at(origin, 0));
        assert_eq!(clock.rate(), 3.0);
        assert!(!clock.pending());
        apply(&mut clock, Command::Cancel, at(origin, 0));
        apply(&mut clock, Command::UpdatePlaybackRate(-2.0), at(origin, 0));
        assert_eq!(clock.rate(), -2.0);
    }

    #[test]
    fn setting_the_rate_cancels_an_update_that_waits() {
        let (mut clock, origin) = clock();
        apply(&mut clock, Command::SetCurrentTime(400.0), at(origin, 0));
        apply(&mut clock, Command::UpdatePlaybackRate(2.0), at(origin, 0));
        apply(&mut clock, Command::SetPlaybackRate(3.0), at(origin, 0));
        assert_eq!(clock.rate(), 3.0);
        clock.resolve_pending(at(origin, 10), END);
        assert_eq!(clock.rate(), 3.0);
    }

    #[test]
    fn a_start_time_sets_the_time_from_the_timeline_and_none_holds_it() {
        let (mut clock, origin) = clock();
        apply(&mut clock, Command::SetStartTime(Some(-300.0)), at(origin, 1000));
        assert_eq!(time(&clock, at(origin, 1000)), 0.3);
        apply(&mut clock, Command::SetStartTime(Some(300.0)), at(origin, 1000));
        assert_eq!(time(&clock, at(origin, 1000)), -0.3);
        apply(&mut clock, Command::SetStartTime(None), at(origin, 1000));
        assert_eq!(clock.status(at(origin, 1000), END), Status::Paused);
        assert_eq!(time(&clock, at(origin, 5000)), -0.3);
        apply(&mut clock, Command::SetStartTime(Some(-2000.0)), at(origin, 1000));
        assert_eq!(time(&clock, at(origin, 1000)), 2.0);
        assert_eq!(clock.status(at(origin, 1000), END), Status::Finished);
    }

    #[test]
    fn a_start_time_ends_the_wait_of_a_play() {
        let origin = Instant::now();
        let mut clock = Playback::start(origin, false, END);
        apply(&mut clock, Command::SetStartTime(Some(-300.0)), origin);
        assert!(!clock.pending());
        assert_eq!(time(&clock, origin), 0.3);
    }

    fn info(id: u64, status: Status, current_time: f64) -> Info {
        Info {
            id,
            kind: Kind::Transition,
            name: "width".to_string(),
            current_time,
            end_time: 1000.0,
            playback_rate: 1.0,
            status,
            pending: false,
            start_offset: None,
            at: Instant::now(),
        }
    }

    #[test]
    fn a_retired_run_answers_until_its_events_are_released() {
        let mut retired = Retired::default();
        assert!(retired.get(7).is_none());
        retired.retire(info(7, Status::Finished, 1000.0));
        assert_eq!(retired.get(7).unwrap().status, Status::Finished);
        retired.release(7);
        assert!(retired.get(7).is_none());
        assert_eq!(retired.len(), 0);
    }

    #[test]
    fn a_run_retired_twice_keeps_the_last_word_until_both_are_released() {
        let mut retired = Retired::default();
        retired.retire(info(7, Status::Finished, 1000.0));
        retired.retire(info(7, Status::Finished, 1000.0).cancelled());
        assert_eq!(retired.get(7).unwrap().status, Status::Idle);
        retired.release(7);
        assert_eq!(retired.get(7).unwrap().status, Status::Idle, "the cancel is still on its way");
        retired.release(7);
        assert!(retired.get(7).is_none());
    }

    #[test]
    fn releasing_a_run_that_was_never_retired_does_nothing() {
        let mut retired = Retired::default();
        retired.release(3);
        retired.retire(info(4, Status::Finished, 1000.0));
        retired.release(3);
        assert_eq!(retired.len(), 1);
    }
}
