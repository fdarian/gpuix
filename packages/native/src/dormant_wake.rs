//! When the animations nobody sees need the window again.
//!
//! Chrome runs an animation on content it does not show without painting for it, and an animation
//! that never reports anything (a loop with no listener) is not run at all until it is visible:
//! its value is a function of the time. So a dormant source says when it next has something to
//! tell, a [`Wake`], and the window asks for no more than the earliest of them.

use std::time::Duration;

use web_time::Instant;

/// When a source of dormant work next needs a sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Wake {
    /// Nothing it does can be told before it is seen or changed again.
    Never,
    /// Every frame: the source cannot say when it is next due.
    NextFrame,
    /// At the first frame at or after this instant.
    At(Instant),
}

impl Wake {
    /// The earlier of two wakes.
    pub(crate) fn merge(self, other: Wake) -> Wake {
        match (self, other) {
            (Wake::NextFrame, _) | (_, Wake::NextFrame) => Wake::NextFrame,
            (Wake::Never, wake) | (wake, Wake::Never) => wake,
            (Wake::At(left), Wake::At(right)) => Wake::At(left.min(right)),
        }
    }

    pub(crate) fn merged(wakes: impl IntoIterator<Item = Wake>) -> Wake {
        wakes.into_iter().fold(Wake::Never, Wake::merge)
    }

    /// `NextFrame` when `due`, which is what a source that has only a yes or no to give says.
    pub(crate) fn next_frame_if(due: bool) -> Wake {
        if due {
            Wake::NextFrame
        } else {
            Wake::Never
        }
    }
}

/// What a wake asks of the window, after it replaced what the holder was waiting for.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Request {
    Nothing,
    /// A frame callback that runs the dormant tick.
    Frame,
    /// A timer for `wait`, kept with `DormantWake::hold` under `at`, that then asks for the frame
    /// callback.
    Timer { at: Instant, wait: Duration },
}

/// The frame callback and the timer that a dormant wake is waiting on.
#[derive(Default)]
pub(crate) struct DormantWake {
    /// A frame callback is queued. It cannot be taken back, and its tick plans the wake again, so
    /// nothing is added while it waits.
    frame_queued: bool,
    timer: Option<Timer>,
}

struct Timer {
    at: Instant,
    /// Dropping it cancels the wait.
    task: gpui::Task<()>,
}

impl DormantWake {
    /// What to ask for to be woken at `wake`, and the timer that `wake` replaces dropped.
    pub(crate) fn plan(&mut self, wake: Wake, now: Instant) -> Request {
        match wake {
            Wake::Never => {
                self.timer = None;
                Request::Nothing
            }
            Wake::At(at) if at > now => {
                if self.frame_queued || self.timer.as_ref().is_some_and(|timer| timer.at == at) {
                    return Request::Nothing;
                }
                // The timer in place is dropped by `hold`, when the new one is made.
                Request::Timer { at, wait: at - now }
            }
            Wake::NextFrame | Wake::At(_) => {
                self.timer = None;
                self.queue_frame()
            }
        }
    }

    /// The frame callback the timer asks for, or nothing if one is queued.
    pub(crate) fn queue_frame(&mut self) -> Request {
        if std::mem::replace(&mut self.frame_queued, true) {
            Request::Nothing
        } else {
            Request::Frame
        }
    }

    /// Keeps the timer for `at`, in place of the one before it.
    pub(crate) fn hold(&mut self, at: Instant, task: gpui::Task<()>) {
        self.timer = Some(Timer { at, task });
    }

    /// The timer is done. Its task is the one running, so it is let go of and not dropped.
    pub(crate) fn timer_fired(&mut self) {
        if let Some(timer) = self.timer.take() {
            timer.task.detach();
        }
    }

    pub(crate) fn frame_ran(&mut self) {
        self.frame_queued = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn after(start: Instant, milliseconds: u64) -> Instant {
        start + Duration::from_millis(milliseconds)
    }

    #[test]
    fn the_earliest_wake_wins() {
        let start = Instant::now();
        let soon = Wake::At(after(start, 10));
        let late = Wake::At(after(start, 500));
        assert_eq!(soon.merge(late), soon);
        assert_eq!(late.merge(soon), soon);
        assert_eq!(Wake::NextFrame.merge(soon), Wake::NextFrame);
        assert_eq!(soon.merge(Wake::NextFrame), Wake::NextFrame);
        assert_eq!(Wake::Never.merge(late), late);
        assert_eq!(late.merge(Wake::Never), late);
        assert_eq!(Wake::Never.merge(Wake::Never), Wake::Never);
    }

    #[test]
    fn a_list_of_wakes_is_merged_from_never() {
        let start = Instant::now();
        assert_eq!(Wake::merged([]), Wake::Never);
        assert_eq!(
            Wake::merged([Wake::At(after(start, 30)), Wake::Never, Wake::At(after(start, 20))]),
            Wake::At(after(start, 20)),
        );
        assert_eq!(Wake::merged([Wake::Never, Wake::NextFrame, Wake::At(start)]), Wake::NextFrame);
    }

    #[test]
    fn a_source_with_a_yes_or_no_is_the_next_frame_or_never() {
        assert_eq!(Wake::next_frame_if(true), Wake::NextFrame);
        assert_eq!(Wake::next_frame_if(false), Wake::Never);
    }

    #[test]
    fn the_next_frame_and_a_past_instant_ask_for_one_frame_callback() {
        let now = Instant::now();
        let mut wake = DormantWake::default();
        assert_eq!(wake.plan(Wake::NextFrame, now), Request::Frame);
        assert_eq!(wake.plan(Wake::NextFrame, now), Request::Nothing, "one is queued");
        wake.frame_ran();
        assert_eq!(wake.plan(Wake::At(now), now), Request::Frame, "due now is the next frame");
        wake.frame_ran();
        assert_eq!(wake.plan(Wake::At(after(now, 5)), after(now, 9)), Request::Frame, "so is overdue");
    }

    #[test]
    fn a_later_instant_asks_for_a_timer_for_the_wait() {
        let now = Instant::now();
        let mut wake = DormantWake::default();
        assert_eq!(wake.plan(Wake::At(after(now, 250)), now), Request::Timer { at: after(now, 250), wait: Duration::from_millis(250) });
    }

    #[test]
    fn a_later_instant_waits_for_the_frame_callback_that_is_queued() {
        let now = Instant::now();
        let mut wake = DormantWake::default();
        assert_eq!(wake.plan(Wake::NextFrame, now), Request::Frame);
        assert_eq!(wake.plan(Wake::At(after(now, 250)), now), Request::Nothing);
        wake.frame_ran();
        assert_eq!(wake.plan(Wake::At(after(now, 250)), now), Request::Timer { at: after(now, 250), wait: Duration::from_millis(250) });
    }

    #[test]
    fn never_asks_for_nothing() {
        let now = Instant::now();
        let mut wake = DormantWake::default();
        assert_eq!(wake.plan(Wake::Never, now), Request::Nothing);
    }

    #[test]
    fn the_timer_asks_for_the_frame_callback_once() {
        let mut wake = DormantWake::default();
        assert_eq!(wake.queue_frame(), Request::Frame);
        assert_eq!(wake.queue_frame(), Request::Nothing);
        wake.frame_ran();
        assert_eq!(wake.queue_frame(), Request::Frame);
    }
}
