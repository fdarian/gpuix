//! The frame patcher: a tick whose only change is how some units paint (see `Retain::preview`) is
//! drawn by building those units alone and having gpui paint them again in the frame it rendered
//! last (`Window::repaint_in_place`), instead of building the page.
//!
//! A tick is asked for by the frame before it (`GpuixView::request_next_frame`), and gpui calls the
//! patcher at the top of its draw. A tick the patcher does not take is drawn whole in the same
//! draw, which is why it changes nothing a frame would not change itself: the clocks it moves are
//! sampled at an instant and not stepped, so the frame that follows at a later instant finds the
//! state it would have found, and the events the patcher emitted are not emitted again.

use std::collections::BTreeMap;
use std::rc::Rc;

use super::Unpatchable;
use crate::renderer::GpuixView;

/// Whether ticks are patched, and why they were not.
pub(crate) struct Patching {
    /// `GPUIX_PATCH_OFF` and `setPatchEnabled` turn patching off, to time a run without it.
    enabled: bool,
    installed: bool,
    declines: BTreeMap<String, u64>,
}

impl Patching {
    pub(crate) fn new() -> Self {
        Self {
            enabled: std::env::var_os("GPUIX_PATCH_OFF").is_none(),
            installed: false,
            declines: BTreeMap::new(),
        }
    }

    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// How many ticks were not patched by gpuix, by kind (gpui counts its own declines).
    pub(crate) fn declines(&self) -> &BTreeMap<String, u64> {
        &self.declines
    }

    fn decline(&mut self, kind: String) {
        *self.declines.entry(kind).or_default() += 1;
    }
}

impl Unpatchable {
    fn kind(self) -> String {
        match self {
            Unpatchable::NoFrame => "NoFrame".to_owned(),
            Unpatchable::LayoutPass => "LayoutPass".to_owned(),
            Unpatchable::Unsteady => "Unsteady".to_owned(),
            Unpatchable::Gliding => "Gliding".to_owned(),
            Unpatchable::ScrollAnimation => "ScrollAnimation".to_owned(),
            Unpatchable::Edited => "Edited".to_owned(),
            Unpatchable::Everything => "Everything".to_owned(),
            Unpatchable::Sourced => "Sourced".to_owned(),
            Unpatchable::Outside(_) => "Outside".to_owned(),
            Unpatchable::Ended(_) => "Ended".to_owned(),
            Unpatchable::Dormancy => "Dormancy".to_owned(),
            Unpatchable::Refused(refusal) => refusal.key(),
        }
    }
}

/// What a patch needs of the units built for it, once the view is no longer borrowed: gpui paints
/// them, and paints may update the view.
pub(crate) struct Built {
    pub(crate) units: Vec<(gpui::ElementId, gpui::AnyElement)>,
    tapes: Vec<(u64, crate::tape::SharedTape)>,
    stepped: Stepped,
}

/// What the clocks that a tick moved say about the frames to come.
struct Stepped {
    motion: bool,
    dormant_motion: crate::dormant_wake::Wake,
    motion_values: crate::motion::Demand,
}

impl GpuixView {
    /// Makes gpui ask this view to patch the ticks it requests. Once per window.
    pub(crate) fn install_patcher(&mut self, window: &mut gpui::Window, cx: &mut gpui::Context<Self>) {
        if std::mem::replace(&mut self.patch.installed, true) {
            return;
        }
        let view = cx.weak_entity();
        let patcher = move |window: &mut gpui::Window, app: &mut gpui::App| -> bool {
            let Some(view) = view.upgrade() else {
                return false;
            };
            let Some(built) = view.update(app, |view, cx| view.build_patch(window, cx)) else {
                return false;
            };
            let tapes = view.read(app).tapes.clone();
            let scratch = built.tapes;
            let captured = scratch.clone();
            let accepted = move |_: &mut gpui::Window, _: &mut gpui::App| {
                scratch.iter().all(|(id, tape)| tapes.same_as_recorded(*id, tape))
            };
            let Ok(outcome) = window.repaint_in_place(built.units, app, accepted) else {
                return false;
            };
            // A scroller around the units is not painted again, and its range follows the
            // transforms that moved below it.
            let window_id = window.window_handle().window_id();
            let scale = window.scale_factor();
            for change in &outcome.scroll_extents {
                let scrolled = view.read(app).scroll_handles.iter().find(|(_, handle)| **handle == change.handle).map(|(id, _)| *id);
                if let Some(id) = scrolled {
                    crate::automation::republish_scroll_content(window_id, id, change.content, scale);
                }
            }
            // The boxes, and the matrices of the texts and start regions that a press is mapped through,
            // are what a transform that moves changes without a build.
            for (_, tape) in &captured {
                let tape = tape.borrow();
                tape.each(crate::tape::Kind::Bounds, &mut crate::automation::publish_captured_bounds);
                tape.each(crate::tape::Kind::Text, &mut crate::text::publish_captured_text);
                tape.each(crate::tape::Kind::StartRegion, &mut crate::text::publish_captured_start_region);
            }
            let (tree, callback) = {
                let view = view.read(app);
                (view.tree.clone(), view.event_callback.clone())
            };
            let relevance = tree.lock().unwrap().relevance.clone();
            relevance.note_transforms_captured();
            // The canvas that sweeps after a draw is not among the units: a transform written without a build
            // moved the boxes just published, and the elements that skip depending on where they are are asked again.
            if !relevance.is_empty() {
                let handle = window.window_handle();
                let viewport = window.viewport_size();
                app.defer(move |cx| crate::observers::sweep_relevance(handle, viewport, &tree, &callback, cx));
            }
            view.update(app, |view, cx| view.continue_after_patch(window, &built.stepped, cx));
            true
        };
        window.set_frame_patcher(Some(Rc::new(patcher)));
    }

    /// Whether the frames of what runs are asked for as ticks, which are patched when they can be
    /// and counted under the reason when they cannot. A page with no unit asks for them too: a tick
    /// that is drawn whole there would otherwise leave nothing to say why.
    pub(crate) fn patches_ticks(&self) -> bool {
        self.patch.enabled
    }

    /// Asks for the next frame of an animation that runs. A tick is drawn by patching the frame
    /// before when it can be, and whole when it cannot (a scroll animation never is patched).
    pub(crate) fn request_next_frame(&self, window: &mut gpui::Window, scroll_animating: bool) {
        if self.patches_ticks() && !scroll_animating {
            window.on_next_frame(|window, _| window.request_tick());
        } else {
            window.request_animation_frame();
        }
    }

    /// Steps what a frame steps before it builds, at `now`, and builds the units the tick changes.
    /// None if the tick cannot be patched: what was stepped stays, for the frame built whole in
    /// its place to find at its own instant. A tick that changes nothing that is drawn (every change
    /// is in content that is skipped, or asleep) has no unit, and gpui counts it as an empty tick.
    pub(crate) fn build_patch(&mut self, window: &mut gpui::Window, cx: &mut gpui::Context<Self>) -> Option<Built> {
        if !self.patch.enabled {
            return None;
        }
        let now = self.clock.now();
        let ids = match self.preview_tick(window, now) {
            Ok(ids) => ids,
            Err(reason) => {
                if let Unpatchable::Outside(id) = reason {
                    self.retain.debug_outside(id);
                }
                self.patch.decline(reason.kind());
                return None;
            }
        };
        let stepped = self.step_clocks(window, now);
        let mut units = Vec::with_capacity(ids.len());
        let mut tapes = Vec::with_capacity(ids.len());
        for id in ids {
            let Some(unit) = self.build_unit(id, now, window, cx) else {
                self.patch.decline("NotAUnit".to_owned());
                return None;
            };
            tapes.push((id, unit.tape));
            units.push((unit.id, unit.element));
        }
        Some(Built { units, tapes, stepped })
    }

    /// What a frame does to the clocks before it builds: motion and motion values are stepped, the
    /// clocks of the dormant animations move, and the events of all of them are emitted.
    fn step_clocks(&mut self, window: &mut gpui::Window, now: web_time::Instant) -> Stepped {
        let tree_arc = self.tree.clone();
        let tree = tree_arc.lock().unwrap();
        let callback = self.event_callback.clone();
        self.note_pass(window);
        self.frame_instant = Some(now);
        self.css_animations.advance_dormant(now);
        self.css_transitions.advance_dormant(now);
        let step = if tree.motion_ids.is_empty() {
            crate::renderer::MotionStep {
                awake: Vec::new(),
                asleep: crate::dormant_wake::Wake::Never,
                settled: Vec::new(),
            }
        } else {
            let focus_styles = self.focus.styles(&tree, tree.keyboard_focus_dim());
            let pointer_state = self.pointer.clone();
            let pointer = pointer_state.borrow();
            self.step_motions(&tree, window, now, &focus_styles, &pointer)
        };
        self.motion_values
            .set_appearance(crate::renderer::color_scheme_of(window.appearance()));
        let motion_values = self.step_values(&tree, window, now);
        crate::renderer::emit_motion_settled(&callback, &tree, &step.settled);
        crate::renderer::emit_motion_value_events(&callback, &motion_values.events);
        // The units built after this emit these too, but a tick with none must not leave the end of
        // a transition or an iteration that was due for the frame that follows.
        self.emit_css_events(&callback, &tree);
        Stepped {
            motion: !step.awake.is_empty(),
            dormant_motion: step.asleep,
            motion_values: self.value_demand(&tree, window),
        }
    }

    /// Asks for the frames that what still runs after a patch needs, as the end of a frame does.
    fn continue_after_patch(&mut self, window: &mut gpui::Window, stepped: &Stepped, cx: &mut gpui::Context<Self>) {
        let running = self.css_transitions.take_active() | self.css_animations.take_active();
        if running || stepped.motion || stepped.motion_values == crate::motion::Demand::Frame {
            self.request_next_frame(window, false);
        } else {
            let tree_arc = self.tree.clone();
            let tree = tree_arc.lock().unwrap();
            let wake = self.dormant_clock_wake(&tree).merge(stepped.dormant_motion);
            self.wake_dormant(wake, window, cx);
        }
    }
}
