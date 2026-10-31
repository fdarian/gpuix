# Patch frames and dormant animations

A running animation should cost what it paints, not what the page holds. A *tick* is a frame whose only cause is the clock. When
everything that changed in a tick is how some elements paint, gpuix builds only those elements and gpui paints them again in the
frame already shown (a *patch*). When the animation is on content nobody sees, nothing is built or drawn (*dormant*). This file covers
both, the cases that used to defeat them, and what a patched tick still walks. How the next tick is asked for, and the display-link
pump behind it, is in [frames-and-wakes.md](frames-and-wakes.md#how-js-is-woken-patch-2). The retention layers a patch builds on are in
[retention.md](retention.md).

## A tick that costs the page (#62)

One colour loop on one text, with nothing else changing, cost a tick 0.94 ms on 500 nodes, 5.84 ms on 3000, 30.5 ms on 12000 and
85.8 ms on 30000, though the tick built 2 elements and painted 8 primitives. The same page drawn once with nothing changed (a forced
draw) took 49.2 ms at 30000.

A draw builds a new `Frame`, and everything a frame holds is copied into it once for each retained root it replays: scene
operations, hitboxes, dispatch nodes, element states, line layouts, accessibility nodes, and in gpuix what the tape republishes. The
profile at 30000 nodes was the bookkeeping of replaying 6000 roots: the element-state map and the id compares about 45%, moving
`RetainedState` 17%, the dispatch tree 7%, gpuix's `retained_root` closure and the bounds and text registries 9%. Layout was constant
and scene and paint were under 1%. A cheaper replay is still a step per root, so the page would still pay for what did not change.

So a tick whose only change is how some animations paint is drawn as Chrome repaints the display items that were damaged: the frame
that is shown is kept, the units that moved are built alone and painted again in it.

| nodes | tick drawn whole | tick patched | patching off |
|---|---|---|---|
| 500 | 0.94 ms | 0.22 ms | - |
| 3000 | 5.84 ms | 0.23 ms | 2.51 ms |
| 12000 | 30.5 ms | 0.22 ms | - |
| 30000 | 85.8 ms | 0.24 ms | 36.0 ms |

Test renderer, release addon built without LTO, 60 ticks 16 ms apart, `clockAdvance` between ticks. "Patching off" is the same page
with the cheaper replay of [retention.md](retention.md) and every tick drawn whole. A patched tick builds 1 element, prepaints and
paints 7, paints 8 primitives and replays no root, at every size; drawn whole it built 2, prepainted and painted 15, replayed 100
to 6000 roots and 800 primitives. The forced draw of the 30000 node page that changed nothing went from 49.2 ms to 3.75 ms with the
cheaper replay. An svg stroke dash loop like a button spinner costs 0.079 and 0.099 ms a tick at 500 and 5000 nodes (0.98 and 13.2
with patching off). In a live window with the display link (75 Hz, 8 s at 3000, 12000 and 30000 nodes) every tick after the first
full frame was patched and cost 0.6 to 1.0 ms at every size.

## Units

A subtree that only a running paint-only animation changes is built whole as a *unit*, so that a tick can build it alone.

**What may run on a unit: paint only.** Every running animation can say whether it only changes what its element paints:
`CssAnimations::paints_only` (every property its keyframes animate is in `AnimProp::paints_only`, an SVG attribute counts, and a name
whose keyframes are undefined or were redefined since the run read them does not), `CssTransitions::paints_only`,
`MotionState::paints_only` (its tracks that can move: one of a layout property that rests at `now` counts for nothing, one that is moving, waiting for a measurement or following layout counts) and `MotionValueStore::binding_paints_only`. Dormancy
([below](#dormant-animations-63)) uses the same classification. An animation of a layout property (`width`, an inset, a `layout`
FLIP animation, a motion value that drives a size) is never paint-only: it is built every tick with the spine above it, culled or
shown, and counted as `LayoutAnimation` in `getPatchCounters().unpatchable`. A binding of such a value that rests does not make a
tick of a loop elsewhere whole.

**Which elements become units** (gpuix, `retain.rs`). `Retain::begin_unit` decides at `enter` from the record of the build before:
everything that runs on the element paints only (`Record::paints_only`, noted by `runs_paint_only`), it is eligible, not dormant,
built in a plain context, holds at most `UNIT_MAX_ELEMENTS` (256) elements, and nothing inside it is skipped (`Kept::InUnit`).
`Retain::end_unit` confirms from the build: no exclusion, no scroller, no absolute descendant placed against a block outside it (the
root itself may be absolute: its own containing block is outside by definition), not absent, and nothing lifted out of it unless the
element is a stacking context that takes it in (an effect group, a positioned element with a `z-index`). An element over the cap is
built whole and counted (`unitsOversized`, `TooLarge`). An element that is no unit says why in `Record::refusal` (`Retain::refusal`),
which the preview turns into the `unpatchable` key. The outermost such element on a path is wrapped as
`gpui::retained(id, key of this frame, prebuilt).patchable()`, with `.lift_z(z)` when it is lifted (`unit_root`) and the `Taped` and
`on_replay_translated` wiring of a `retained_root`, and keeps its `FrameEnv` in `Retain::units()`. A layout-only pass and a deferred
build make none; the reference pass of a verified frame does, since after a patch that pass is the frame shown. A unit root keeps the
start of its build (`BuildStart`, one `Inherited` clone) for a patch to build from again, so the bound of the one-animating-node test
`motion-opacity` is `elementsBuilt + retainableRoots + unitsBuilt`.

**A lifted element can be a unit.** Below 1 an opacity makes an effect group, which gpuix lifts at z 0 into the context around it,
and a positioned element is lifted at its z-index, so a skeleton pulse, an opacity loop and the colour loop of a positioned box are
units. gpui patches such a unit when the new element is lifted as the recording was.

**An `<svg>` whose shapes run a paint-only animation is a unit.** The shapes of an `<svg>` are never built, so the svg host notes
what runs on them (`shapes_running`) as bits of its own, and `runs_paint_only` of the host asks every shape below it. A running
document is rasterised in the paint whatever its size ([below](#an-svg-is-culled-like-a-box)), so any `<svg>` is one.

## How gpui patches (`window/patch.rs`)

A frame that draws is a new `Frame` and copies every root it replays, so it costs the page. A tick that changes only how some roots
paint repaints those roots in the frame that is shown and leaves the rest as it is.

- **`retained(..).patchable()`** makes a root leave a `PatchUnit` in a per-frame map keyed by its id when it records and its
  recording is plain: not relocatable, not culled. The unit holds what the replay key lacks of the surroundings: the id stack, text
  styles, rem size, view, content mask, exact clips, offset, opacity, positioned frame, image cache and stacking contexts.
- **`Placement`** says how a root is painted. `InPlace` when no lift is open. `Deferred { paint }` when the root, or an element
  around it, is painted by a stacking context later (a root lifted by `lift_z`, a hole, or prepainted inside a lift that is open, a
  positioned or translucent box around it): the unit keeps a second set of surroundings, captured when the context paints it, and
  the `lift_z` the root declares. A patch prepaints the new element directly, so it is not lifted again. A root that says it is
  lifted under a parent that lifts nothing is painted in place and patched as one.
- **An absolute root is a unit, recorded and never skipped.** Its node is rewritten against a block outside it by
  `settle_containing_blocks`, which a frame that skips the subtree does not run, so gpui keeps such a recording as a source for a
  patch only (`Recorded::root_escapes`; every frame builds the root). `Window::root_escape` names the root's layout node while its
  recording or patch is made, so its own escapes do not count against it under a clip smaller than its block. The patch runs no
  layout pass; a moved block is caught by the bounds key. `end_unit` judges only what is below the root for an escape, so an absolute
  box inside it against the root is fine and an absolute descendant whose containing block is outside the unit still refuses it as
  `Escapes`.
- **`Window::request_tick`** asks for a draw with no view queued and no layout invalidated. A draw for a tick first asks the hook of
  `Window::set_frame_patcher`, which builds the roots the tick changes and calls `Window::repaint_in_place(units, cx, accept)`. That
  restores the surroundings of each unit, lays the new element out onto the nodes the recording has (the counters of a build that
  is skipped must not move), prepaints and paints it into a scratch frame with the recorded hitbox ids handed out in order, and
  compares the result with the recording: the replay key, hitboxes by value, cursors, tab stops, listener, input handler, tooltip and
  dispatch node counts, and the kinds of the operations. Then `accept` runs (the embedder compares its tape). On success the
  operations of the rendered scene are overwritten in place, `Scene::rebuild_from_operations` makes its primitives again from the
  log, and the frame is presented only if an operation changed. A tick that changes no operation presents nothing.
- **Any failure declines.** The element states go back, the layout pass ends, and gpui draws the tick whole in the same draw.
  `PatchDecline` names the reason: pending view, refresh, unknown unit, stale recording, surroundings, layout mismatch, unstable
  layout, effects, hitboxes, registrations, operations, lifts, scroll extent (the transforms that ticked would clamp a scroller's offset or move a thumb), rejected. A wrong patch is a mismatch under the verifier.
- **Counters.** `Window::patch_counters` reports patched frames, patched units and the declines by reason; `frame_work_counters` also counts the elements and primitives a patch painted, and `rasterized_glyphs`, the glyphs rasterized because the atlas missed them: a glyph of a patchable root that the content mask clips away is a pending glyph operation (compared like a sprite, never rasterized until it shows), so a patched tick of a long text behind a clip rasterizes only what shows.
- **A unit that holds the focused element is patched.** The scratch frame sets `Frame::focus`, which used to fail the registration
  check for every such unit; the unit is declined only when it gains or loses the focus.

## The gpuix patcher (`retain/patch.rs`)

Installed once per window from `render`. For a tick it takes `now` from the clock and asks `Retain::preview`, which changes nothing.
A tick is patchable when, since the last frame built whole: only time-driven ids changed (running `dynamic`, motion that runs or
ended, not yet built), each in a unit or dormant; the root revision is the same; no layout-only pass ran; no scroll animation or
scrollbar fade is active; no `layout` box has a glide in flight or events to send (`LayoutFlips::needs_frames`, asked of the live
flips and not recorded); and that frame built no element that follows the clock (`Exclusion::varies_with_time`). An element with no
box (`display: none` or below one, `box_generation::is_hidden`) is skipped as a dormant one is.

The reasons it says no are `Unpatchable`:

| Key | Meaning |
|---|---|
| `NoFrame`, `LayoutPass`, `Unsteady`, `Gliding`, `ScrollAnimation`, `Edited`, `Everything` | no frame to patch, a layout-only pass ran, the last frame held an element that follows the clock, a `layout` glide is in flight or its events are to be sent, a scroll animation or fade runs, the tree was edited, everything changed |
| `Sourced` | the pointer, focus, a scroll, a custom element, a measurement, a link or a command changed |
| `Outside` | a time-driven change outside every unit that is not dormant, on an element that has a box and the last build gives no reason for |
| `Refused` | the element is no unit: `Excluded:<kind>` (`Excluded:fixed` for `position: fixed`; gpui has no root placed by the viewport, `Escape::Viewport` is declined `Surroundings`), `ExcludedBelow:<kind>`, `Escapes`, `Lifts`, `Scrolls`, `LayoutAnimation`, `TooLarge`. What runs on the element is looked at first, so an absolute element whose `top` runs is `LayoutAnimation`, and so is a tick made `Sourced` by the measurement of a layout animation |
| `Ended` | a unit whose element the last frame no longer found a paint-only animation running on, though a source still lists it, as an SVG shape that holds its end value is: a frame built whole would not make it a unit, so it is not repainted as one |
| `Dormancy` | the preview has no unit but elements are about to enter the dormant set or their verdicts moved: the frame settles that ([skipped content](#skipped-content-97)) |

If it says yes, the patcher steps what a frame steps before it builds (`sync_motion_states`, `sync_motion_values`, the clocks of the
dormant animations, their events) and builds each unit with `GpuixView::build_unit` from its kept env at `now`, with the retention
closed (nothing entered, recorded or tapped, `BUILD_EFFECTS` truncated, events drained, `built_at` set, no frame asked for), inside a
tape that captures: `Tape::capturing` notes the writes of a prepaint and paint without making them. gpui's `accept` is
`Tapes::same_as_recorded` for each scratch tape. On success it takes the active flags and asks for the next tick; on any decline it
returns false and gpui draws the tick whole in the same draw.

A tick declines safely: the clocks were sampled at an instant and not stepped, so the frame that follows finds the state it would
have, and each event was emitted once. The frame that ends a build asks for the next one with `request_next_frame`:
`on_next_frame(request_tick)` when units were made (`Retain::has_units`) and patching is on, `request_animation_frame` otherwise or
while a scroll animation runs. `patches_ticks` does not need a unit, so a page with none asks for ticks and counts them. Patching is
off with the retention, and with `GPUIX_PATCH_OFF` or `setPatchEnabled(false)`. The counters are listed in
[testing.md](testing.md#switches-and-counters).

The tick is the only thing a patch can answer. A hover, a React commit, a scroll, a measurement or a click draws a full frame, and
patching resumes at the next tick.

## Cases that used to draw whole

Each of these made every tick of a loop elsewhere on the page a full frame. Found with `getPatchCounters().unpatchable` and
`declines`, `getBuildCounters` and `getFrameWorkCounters`.

### An opacity run is a compositor layer from its start to its end (#81)

A loop on `opacity` used to be lifted into an effect group only below 1, and painted nothing at 0 (the scene was hidden). A loop
through exactly 1 or 0 changed the shape of what its element paints: the tick at 1 flipped the lift and declined the patch
(`Surroundings`), and the tick at 0 dropped the operations (`Operations`), so those ticks and the one after were drawn whole. It was
a bug in retention as well: a record made at opacity 1 (not lifted) and skipped (a dormant loop below the window) was handed to gpui
as a wrapper with no lift, and the build that replaced the replay when the card came into view was lifted at 0.25. zed's
`retained.rs` `request_subtree_layout` ("the root of a retained subtree is lifted as its element says") panicked on a debug addon,
and the release addon kept the stale `None` and painted the element in the wrong place of the stacking context.

In Web Animations an element with a current animation of a property acts as `will-change` of it, and Chrome keeps its compositor
layer. So an element whose `opacity` is animated is an effect group for the whole run, at whatever value it samples.

- **gpuix** (`animate_style`): `StyleDesc::animating_opacity` is set while a CSS animation that is current (active, or before its
  active phase going forward, not one that only fills), a running CSS transition, a `motion` track or a moving motion value targets
  `opacity` (`opacity_runs`). `has_effect_group` reads it, so `lift_z`, `creates_context` and `Resolved` all see an effect group at
  any sampled value.
- **zed**: `Style::has_active_opacity_animation` makes `group_effect` return a group at 1 and keeps the element's scene at 0, so the
  operations keep their shape from one tick to the next. A static opacity is as it was: 1 has no group, 0 paints nothing.
- **A run's start and end are whole frames**, as Chrome re-layers there. `Record::lift_run` says whether a build was in a run, and
  `skippable` refuses a record that disagrees with the run state now (`Kept::LiftRun`), so the wrapper of a skipped root never
  carries a lift that the build replacing it does not. Don't weaken that check: the panic above returns. A state style that changes
  the opacity (`hover`, `focus`, `focusWithin`, `groupHover`, `hasHover`) or a write changes the element's record, so it falls under
  the same rule with or without a transition. `lift_run` is also true while any run of a transform property is current, Motion's included
  ([paint-and-transforms.md](paint-and-transforms.md#retention-and-patched-ticks)).

The ticks inside a run, at 1 and at 0 included, are patched. A native loop and a CSS animation through exactly 1 and exactly 0, at
500 and 5000 nodes, patch at every tick, and nothing is declined or counted unpatchable.

### An `<input>` is retained and its caret is a tick (#74)

A page with an `<input>` or `<textarea>`, focused or not, in view or not, drew every tick of a loop elsewhere whole (`Unsteady`
at the tick rate): 4.0 ms a draw at 7.8k elements, 7.4 ms at 13k. A page with a `<code>`, `<markdown>` or `<diff>` did the same.
The input had no `retain_state`, so it was `Exclusion::CustomElement`, which was in `varies_with_time`, and `Plan::unsteady`
declined every tick of a page that built one. The kind was listed for the input, whose caret follows the clock; `<code>`,
`<markdown>` and `<diff>` have no `retain_state` either but do not follow it (the highlight is made on the frame thread and nothing
of them arrives late). The input's measure was a keyless request, which made every recording of it `UnstableLayout`.

- **The editor is retained like any element.** `TextEditorElement::retain_state` is an epoch (`TextEditorState::changed` counts
  every notify of the editor but the blink's), so what is typed, selected or scrolled rebuilds the input and nothing else does;
  focus and the window going key are already in the planner's focus bits. Its measure is keyed (`request_keyed_measured_layout`,
  `TextEditorState::measure_key`: the shaped text, runs, sizes, rows, the font generation and the entity).
  `TextSystem::font_generation` is public in zed for the key.
- **The caret is the clock's only.** `CustomElement::follows_clock` notes `Dynamic::Blink`, an own, paint-only kind, so a focused
  input is a unit unless a layout animation also runs on it. The blink is a timer aimed at the end of the phase (`arm_blink`, from
  the activity anchor on the executor's clock, not a 500 ms loop) that calls `Window::request_tick` instead of notifying, and the
  caret quad is painted in both phases (in no colour while off) so a patch has the operations of the recording. An unfocused input,
  or one in a window that is not key, has no source at all.
- **A tick of another loop does not build the input.** `CustomElement::clock_due` says whether what follows the clock changed since
  it was painted: the phase's timer sets it and the paint clears it. `Retain::between_changes` leaves an element whose only dynamic
  is `Blink` out of the touched set while it is not due.
- **A caret nobody sees asks for no frame.** The timer is armed by the prepaint of the editor (`TextEditorState::keep_blinking`),
  only while the window is key, the caret is shown and its box meets the content mask and the bounds of the editor; `render` only
  notes the focus coming. An input scrolled back into view arms again in the frame that moves it.
- **`<code>`, `<markdown>` and `<diff>` left `varies_with_time`.** Nothing else read it for custom elements.

A loop beside a focused or unfocused `input`, a `textarea` or an input out of the clip went from 4.0 ms and 27.5% of a core
(`Unsteady` 67.5 a second) to 0.8 to 0.95 ms and 6% at 7.8k elements, the same as the page without an input (0.97 in the same run),
with every tick patched. At 13k elements 7.4 to 0.34 ms; at 1306, 1.16 to 0.34 ms. The blink is a patched frame that builds the input
alone.

### A loop under a positioned or translucent ancestor, or on an absolute element (#75)

A colour or stroke loop on a text or an `<svg>` under a `position: relative`, `absolute` or `opacity < 1` wrapper drew every tick
whole (`UnknownUnit` at the tick rate), and so did a loop on an element that is itself `position: absolute`: 3.1 to 3.9 ms a draw at
7.8k elements, 5.3 to 6 ms at 13k. The cause was in gpui. `capture_patch_unit` recorded a unit only for a static root with no lift
open, and for a lifted root with exactly one open lift in a lifting context, so a root prepainted inside any open lift (gpuix lifts a
positioned box or an opacity group, `lifts_open >= 1` while it is prepainted) recorded nothing, and `try_repaint_in_place` declined
it. For the element that is itself absolute, gpuix refused it as `Escapes`, since gpui kept no root placed by a box outside it. The
fix is `Placement` and `Recorded::root_escapes` above.

Draw ms at 3000 nodes (7805 elements, release addon), before to after: text under `relative` 3.29 to 0.36, under `absolute` 3.10 to
0.40, under `opacity < 1` 3.22 to 0.40, itself `absolute` 4.36 to 0.38; `<svg>` under `relative` 3.44 to 0.23, under `absolute`
3.85 to 0.23. The absolute text was 30% of a core and is 3%. At 5000 nodes (13005 elements) a text under `relative` and one that is
itself `absolute` went from 5.28 to 0.34 ms. A page with a `relative` header wrapper: a colour loop 3.45 to 0.67 ms at 500 nodes,
2.59 to 0.41 at 2000 and 8.89 to 0.43 at 8000 (64% of a core to 3%); the stroke loops likewise. Every tick is patched and `declines`
is empty. A `width` or `top` loop there is still drawn whole, and now says `LayoutAnimation` at the tick rate where it said nothing.

### A motion value is touched when it can have changed (#76)

A motion value bound to a box drew whole for four reasons (a visible paint-only binding already patched; an `<svg>` stroke is reached
through the `color` of the `<svg>` and `currentColor` strokes, since a motion value binds to a style property and never to an
attribute of a shape):

- A box bound to a value that rests was built by every tick of a loop beside it (50 resting boxes: 51 elements built a tick, 1 alone).
- A width, left or top bound to a value that rests made every tick of a loop elsewhere whole (`LayoutAnimation`, 8 of 8).
- A paint-only value bound to a culled element requested a frame every vsync, drawn whole (`NoUnits`, 764 primitives replayed),
  where a CSS loop there asks for none.
- A script's `mv.set(v)` from a `requestAnimationFrame` loop, or from the `change` of another value, was a whole frame at each
  write (`PendingView`).

Every element with a `motionValues` binding carries `Dynamic::MotionValues`, and `Retain::preview` touched all of them at every tick,
moving or resting. The end of a frame asked for a tick whenever any bound value moved (`MotionValueFrame::active`, one tree-wide
bool), which never reached the dormant branch. A write went through `Repaint::MotionWrite` to `notify_paint_only`, which queues the
view.

- **A resting binding is not touched.** `MotionValueStore::binding_due` says whether a binding can show otherwise than the frame
  that built its element did: a value it binds was moving by itself when that frame sampled it (`frame_moving`), or it, or a value
  it is computed from through transforms and followers (`MotionValues::follows_any`), was set, jumped, animated, stopped or
  controlled since (`written`, cleared by `frame`, which a frame built whole and the patcher's `step_clocks` both run). It goes
  through `Sources::clock_due` beside the caret's, and `Retain::between_changes` leaves an element whose only dynamic is `Blink` or
  `MotionValues` out of the touched set while it is not due. One that also runs an animation is touched by every tick.
- **What the values ask of the frames is per element.** `MotionValueStore::demand` is `Frame` when a value bound by an element that
  is not dormant (or read by a drag) moves, `Dormant` when only dormant elements bind it or only a listener reads it, `Idle`
  otherwise. `Dormant` arms the dormant tick when a listener needs one ([wake times](#wake-times-90)).
- **A paint-only write is a tick.** `Repaint::invalidate` asks for one (`Window::request_tick`) when `GpuixView::patches_ticks` and
  `MotionValueStore::writes_paint_only` hold; `animate`, `stop` and the controls take the same road, so the frame that starts and
  the frame that ends a run are patches too. The layout stays current, so a read of bounds before the frame lays nothing out. A
  write that can move a box still invalidates layout and is drawn whole, and with `GPUIX_PATCH_OFF` it notifies as before.

Counted, not timed: 50 resting bound boxes beside a colour loop build what the loop builds alone (1 element a tick, from 51); a loop
beside a resting width, left or top patches every tick; a bound colour, background colour, spring or range transform out of view in
a scroller, or among 5000 nodes below the window, draws no frame in 24 ticks and sends one `change` per tick; a `color` written
every frame at 500 and 5000 nodes is patched at every write with the work of the loop.

### An `<svg>` is culled like a box

Every custom element called `Subtree::block`, so an `<svg>` and everything above it could never be culled, `Dormancy::holds` never
saw it culled, and a stroke loop on a card nobody sees was built every tick. A document over 4096 bytes or 128 x 128 px was
rasterised by a job, which a patched tick would start and then show the image of the tick before, so a running one was
`Exclusion::Svg` and every tick of it was drawn whole. Now:

- **The ink of an `<svg>` is its box.** It draws one image fitted to its box, `overflow: visible` included, so `build_element` takes
  `Subtree::of_element(empty, style)` for it in place of `block` (the box plus what its own style adds) and the element calls
  `cullable`. In a culled subtree its canvas makes no image and starts no job (`SvgPaint::note_culled`; `retain_state` is 1 after
  such a paint, so an `<svg>` never shown is not an unsettled raster).
- **The shapes sleep with it.** A shape is never built, so a running shape used to be named in `Sources::motion`, which `holds`
  could never find culled. `running_motion` names the `<svg>` (`svg_tree::host_of`), and the clocks of its shapes enter and leave
  dormancy with it (`with_shapes`), so `animationiteration`, `animationend` and `getAnimations()` of a shape come at the ticks of a
  shown one.
- **A running document is rasterised in the paint, whatever its size** (`CustomElement::shapes_run`, `Key::in_paint`), so
  `draws_changes_at_once` holds and its ticks are patched. A static large `<svg>` keeps the job. The rasterisation moves from a
  background thread to the frame, which the whole frame it replaces paid in waiting for the job as well.

Per tick, test renderer, release, median of 30, before to after: a 200 x 200 `<svg>` with one running circle 0.53 to 0.11 ms on 500
nodes and 4.30 to 0.12 ms on 5000; 400 x 400 0.73 to 0.29 and 5.85 to 0.28; 400 x 400 with 40 running shapes 4.11 to 3.80 and 7.83
to 3.73; 800 x 800 with 40 shapes 7.36 to 5.94 and 10.97 to 5.91. None was patched before; 39 of 40 ticks are after (the first
records the unit). Below the window or clipped out of a scroller, 200 cards with a stroke loop on an `<svg>` of any size make the
idle tick and the hover frame of 0 cards.

An `<svg>` whose paint would start a job (`Unsettled::Stale`, a document or colour that a paint would start a job for, which a replay
would not) is whole for one frame: its `retain_state` is `None`, so the frame before that paint is unsteady. The job's result arrives
as a frame of its own and the ticks go on after it.

### An `<svg>` waiting for its raster, and motion under `display: none`

A page with a thinking indicator (a text that shimmers and a glyph that morphs its `d`) drew every tick whole (2358 elements, 1240 x
780, 75 Hz, 8 s, release addon: 68 draws a second, none patched, 6.3 ms a draw, 40 to 44% of a core), and the tile of the indicator
alone (47 elements) patched 63 of 68. Turning off either loop alone changed nothing, turning off both drew nothing, so the page held
every tick whole for as long as anything on it ticked. Two things on the page that nothing draws:

- **An `<svg>` that had not painted an image was unsteady.** `SvgElement::retain_state` was `None` (`Exclusion::Svg`, one of the
  kinds that `varies_with_time`, so `plan.unsteady` and `Unpatchable::Unsteady` for every tick) whenever `SvgPaint::paints_in_place`
  was false: before any paint had made an image, while a job ran, with a result in the inbox, and with a big document or a colour
  that the image shown was not. A box with no area (`width: 0`, a big document with `height: 0`) is never painted (`paint_svg`
  returns before `SvgPaint::image`), so its state stayed `None` for as long as it lived and every frame that built the page recorded
  the page as unsteady.
- **Motion under `display: none` was outside every unit.** `Retain::preview` named an element something runs on that no unit holds
  as `Outside`, also when it has no box (`display: none` or below one: no build reaches it and no frame paints it). The closed
  dropdown and the cards that are not shown ran a motion or had been built with a value, so every tick of the page was declined for
  them (`Outside` 64 a second).

The fix:

- **A waiting `<svg>` has a word in its state.** `SvgPaint::unsettled` says which of `Unsettled::{NeverPainted, JobRunning,
  ResultWaiting, Stale}` holds, and `retain_state` gives a word per waiting state (2 never painted, 3 job running, 4 its result
  waiting for the next paint). Nothing a replay of the last paint draws differs while it waits, and the arrival changes the word,
  which `compute_deltas` finds as a changed custom element and builds that `<svg>` once: the arrival draws one frame that is not a
  tick, and ticks go on after it. Only `Stale` stays `None`.
- **An element with no box is not outside a unit.** `Retain::preview` skips it as it skips a dormant one.
- **`getRetainDebug` says why.** `exclusionWhy` names why a built element excluded itself (`CustomElement::unretained_because`; for
  an `<svg>` the `Unsettled` name), and `outside` lists the elements ticks were declined for as outside every unit, with their record,
  their refusal and whether they are hidden or unbuilt (`[d:none]` in their labels).

That page went from 68 whole draws a second (`Outside` 64 a second), 6.1 ms and 42% of a core to 63 of 68 patched (the rest are the
word roll's layout animation, `LayoutAnimation` 4.2 a second, and `Edited` 0.4 a second) at 4.0 to 4.2 ms and 28%. The first draw
that notes a blink after a focus is whole (it cannot also be the unit, counted `Outside`); the blinks after it are patched. The sweep under [what a patched tick still walks](#what-a-patched-tick-still-walks) measured the same page again: across both runs it went from 68 whole frames a second at 6.1 to 6.3 ms and 40 to 44% of a core to 63 patched of 68 at 3.3 to 4.2 ms and 21 to 28%.

### An idle `layout` box (#117)

A looping opacity animation beside a `motion.div` with `layout`, `layout="position"` or `layout="size"` that never moves drew every
tick whole: 0 patched and 150 whole in 150 ticks, against 150 and 0 for the same page without the prop. Two causes, which show as two
reasons:

- **`Unsteady`** (FF's `queued-stack`: 674 in 10 s, 25 to 29% CPU against 9.7%). Every `layout` element built is
  `Exclusion::LayoutAnimation`, and that exclusion was in `varies_with_time`, so the plan the frame recorded was unsteady for as long as
  the box existed, whether or not a glide ran. The glide is not a property of the build: it starts in prepaint, at a frame that finds
  the tree changed and the box moved, and a tick (whose revision is the plan's) can never start one. So a plan no longer carries it.
  `Retain::preview` is told whether a glide is in flight (`LayoutFlips::needs_frames`): a box with a track that has not run out, an
  event not yet sent, or a `dragOffset` (its values reach a box that is not bound to them, and nothing else tells a tick that a drag
  moved). The tick is then `Gliding` and is drawn whole, which builds the box and steps its track. The frame that steps the glide to its
  end puts `layoutAnimationComplete` in the store, and a render sends it, so the next tick is whole as well (`events` not empty), and the
  one after patches. A box with no box of its own (`display: none`) is stepped by nobody and does not count. A box that glides later
  starts as it always did: the commit that moves it is a tree edit (`Edited`), that whole frame builds it (it is excluded, so it is
  never skipped), and its prepaint starts the glide from where it was shown.
- **`NoFrame`** (the repro: 200 bars of 80 by 4 beside the loop). The excluded box makes every ancestor not eligible to be skipped, so
  the page held no root worth skipping (each bar is a root of one element), the back-off took the page for one that gains nothing from
  being recorded, and recorded one frame in nine: the plan was forgotten between them, and the records with it, which the units
  of the next frame are chosen from (`begin_unit`), so the page never had one. An element a tick could patch (something runs on it
  that only paints, and nothing refuses it) is now a candidate as an element a frame could skip is, so the page is recorded
  (`finish_session`). The pages where this shows are the ones that can skip nothing beside a loop; FF's, with large cards, was
  `Unsteady`.

A box that is itself animated (`layout` and `animate` on one element) is not a unit, as before (`Excluded:layout-animation`), and an
ancestor of a `layout` box is not one either (`ExcludedBelow:layout-animation`); only the neighbours stopped paying.

## What a patched tick still walks

A patched tick should cost what its units cost, and one on a page with many motions did not: 4.11 ms on average (median 4.27) against
1.92 (median 1.55) for the tile alone in the same run, where both patch the same units (10.5 built a tick, with 10.5 elements, 69.5
prepainted and painted, 10.5 primitives and 40 layout nodes reused). A sample of the release addon found the rest in three walks
over the page, each in something that did not move.

| Walk | Share of a patched tick | Grows with |
|---|---|---|
| `sync_motion_states` (half of it `parent_inherited` walking each motion's ancestors, the rest comparing each motion's source with its last) and `running_motion` | 26% and 5% | the motions on the page |
| `Scene::rebuild_from_operations` (every primitive of the frame put in the scene's bounds tree again) | 22% (95% with 1000 boxes in view) | the primitives in view |
| `step_clocks` sampling and parsing the bindings of every element that binds a motion value, and `preview` parsing those of every element last built with one (`binding_due`) | 52% with 1000 bound boxes, 18% in `preview` | the bound values on the page |

- **A tick steps the motions that move.** `GpuixView::sync_motions` notes the motions that moved and the `MotionInputs` of what a
  sync reads but the clock: `structure` is a hash of the tree and its revision, appearance, viewport and measurements, beside the
  hover chain, the press chain and the focus-visible element. `step_motions` (a tick) steps only the moving motions while the inputs
  are the same, and all of them when they are not or when a measurement waits for a frame to take it. A motion that moves at a tick
  moved at the one before, since an animation is active from its start (its delay included) until it ends, and only a change of
  what a sync reads starts one. `preview_tick` takes its running motions from the same list. `getBuildCounters().motionsSynced`
  counts the motion elements a sync read.
- **A frame built whole syncs the motions it builds.** `sync_motions_for_build`, called by `render` (whole and layout-only passes),
  does the full sync when `structure` changed. Otherwise it steps what a tick steps, except the moving motions whose element has
  no box in the pass about to be built (under `display: none`, or under a host that skips and that the pass does not force,
  `is_hidden_before_build`), which wait for their wake. Where the build could decide otherwise (a `display` an `@container` folds,
  a skip a read forces) the check says "has a box". A hover, a press or a focus-visible change reaches only the motions whose
  element, or the ancestor that controls their variants, flipped, and syncs those whether they move or not. Hosts a layout-only
  read forces count as built. `build_virtual_child` steps the moving motions of its row and does not sync the page; a gesture
  change in a row's draw is a full sync. A hover over a page of 401 resting motions syncs 1 a frame (2 with a `whileHover`), where
  a whole frame synced all 401.
- **A tick makes its scene again from the draw orders it had.** A draw order is one more than the greatest of those of the bounds
  inserted before it that intersect it, so it is a function of the sequence of bounds a scene took, and a patch changes how
  operations paint, seldom where. A scene keeps the bounds and the order of each insert since `clear` (`order_log`, and the log of
  each group it ended, by the id it has in its build), and `rebuild_from_operations` compares the bounds it inserts with that log:
  while they are the same, in the same sequence, it takes the orders from the log and does not ask the tree (zed
  `Scene::insert_bounds`). A sequence that is not the one reused (something moved, a quad that gained a background and lost its
  strips, a group that faded out) is rebuilt with the tree, after the insert that found it, and the tree a rebuild did not fill is
  made from the log when something is inserted afterwards. `getFrameWorkCounters().boundsInserts` counts the orders worked out with
  a tree. The comparison of what an operation paints with its recording (`PaintOperation::differs_from`) used to format both through
  `Debug` to two strings for every operation of every unit, 8% of a tick of the tile; it formats the recorded one once and writes the
  other through a comparison that stops at the first character that differs, which is early for what changes (a colour, an opacity).
  The answer is the one of comparing the two strings.
- **A tick samples the values that move and parses no binding.** `MotionValueStore::step` samples again what moved by itself when it
  was last sampled (the values elements bind and the ones read for a drag) and the values a write since can have changed, and leaves
  the rest as the frame left them, since only time and a write move a value. A tick does that while the bindings, appearance and
  viewport are the ones the last frame sampled (`GpuixView::step_values`, keyed on the tree and its revision as `step_motions` is),
  and a frame samples them all (`sync_values`). `BindingIndex` holds the bindings parsed once per revision of the tree, by element
  and by value: `value_demand` asks only the elements bound to a value that moves, and `preview` and a frame ask `element_due` of
  it. `getBuildCounters().motionValuesSampled` counts the values sampled for the elements that bind them.

Patched ticks alone: the showcase page went from mean 4.11 to 1.82 ms (median 4.27 to 1.18) against 1.92 (median 1.55) for the tile
alone, so nothing of the page is left in it. At window scale 2, which is what the machine gave for the sweep, the patched tick is 3.05 ms against 3.06 for the tile alone. Over a full sweep at window scale 2 the showcase went from 4.0 to 4.2 ms a draw and 28%
of a core to 3.3 to 3.7 ms and 21 to 22%, and the same page without the shimmer from 2.8 to 3.1 ms and 19 to 20% to 2.0 ms and 13%.
Beside a loop, median per patched tick over 300 ticks: 1000 plain boxes in view 1.87 to 0.125 ms; 1000 resting bound boxes 2.39 to
0.21 ms (none 0.05).

What is left is the cost of the units themselves. A `motion.path` that morphs `d` is a new document every tick: it is parsed again
(usvg `parse_svg`, about 5% of a patched tick of the tile), rasterised in the paint, and the atlas makes a Metal texture for it and
releases the one of the tick before (`PlatformAtlas::get_or_insert_with` and `drop_image`, about 14%, IOGPU calls). The tick is
patched, and costs what the unit does. And what a unit paints is still compared with its tape through strings: the operations stop at
the first differing character, the entries of the tape are still formatted whole (about 14% of a tick of the tile, with the
operations, before the operations changed).

## Dormant animations (#63)

200 cards clipped out of view, each with a colour loop, cost an idle tick 4.86 ms against 0.09 ms for the same page with no loops,
and built 403 elements (0.16 ms with patching and dormancy, against 0.08 ms with none). Chrome paints nothing for them. A hover over a sidebar beside 5000 such cards took 37 to 43 ms against 0.4 to
1.5 ms with none. Two causes: a CSS animation steps only in a build, so every culled animated card was built (2 elements) with the
spine above it each tick, for a result that nothing draws; and a hover frame replays every card of a scroller, at a cost per element
and not per root (about 8 µs for each card nobody sees, 1.2 µs an element, with flat counters; the profile put it in ids that missed
the pointer compare and fell to `SmolStr::eq`, about 30%, the loop that moves nested roots, `Frame::finish` moving the element
states, `reuse_subtree`, and gpuix's tape writing each replayed root into the registries).

Nothing is built or drawn for an animation nobody sees.

- **An element is dormant** when everything that runs on it only changes what it paints and the last frame that reached it walked it
  culled (`ElementBounds::culled`, taken from `Window::is_culling` in the prepaint listener of `capture_layout_box` and written
  through the tape like the rest of the box, so a replayed root republishes it; a verified frame leaves it out of what it compares,
  since a root kept near a scroller's clip is walked culled when it replays an old recording and unculled when it is built whole).
  `Dormancy::holds` decides it from live state: culled, no span, no `layout`, and the animations, transitions, `motion` and
  `motionValues` of the element paint only. An `<svg>` can be culled and so can sleep, with its shapes. `Dynamic::Spliced` keeps the
  children of an animating `display: contents` element out of it. A dormant element is left out of `changed`, so its ancestors leave
  the spine and the root around it replays. A culled recording cannot replay at a place that is not culled (`ReplayKey::culling`),
  so a card that shows again is built late with the clock of that frame, and the colour it shows is the current instant's.
- **A dormant element costs a frame nothing while nothing about it changed.** `Retain::dormant` is a persistent map (element, the
  kinds running on it) kept out of `dynamic`, so `compute_deltas` asks `Dormancy::holds` only of what is not asleep. An element
  sleeps until something can have changed its answer: a build that reaches it (`finish_session` notes it in `woken`, and the next
  frame that begins asks again, so the frame that built it still sees what the frame before found), or what `holds` reads that no
  build of the element tells (`Sources::verdicts`: the keyframes registry's epoch, the tree's count of re-parentings, the viewport),
  which asks every sleeper again. A frame that does not record (retention off or resting, planning off) lets them all go, and a
  destroyed element is let go by `prune`. What `holds` reads of the tree (a span, an SVG element, a `layout` element, a
  `motionValues` binding that moves a box, the shapes an `<svg>` draws) is `PaintStatics`, kept per element while its
  `subtree_revision` and its parent are the same and pruned when the map doubles; a `<text>` in a `<text>` is read every time, since
  what makes it an SVG element is above its parent.
- **Their clocks advance without a build.** `CssAnimations::advance_clock(id, now)` and the same for transitions move the runs to
  `now` as the first half of a frame does (`Run::tick`, shared with `advance`), report iterations, ends and finishes, and note
  whether a run still moves. A dormant run is not ticked until something can be due: `Run::quiet_until` is the next of the start,
  the next iteration boundary, the end and the finish, less a millisecond (an hour for a held clock; nothing for a run going
  backwards or a `play()` waiting for a frame). `CssAnimations::advance_dormant` ticks from a due queue (a heap by `quiet_until`,
  one live entry per element) and from a list of elements to look at in the next tick whatever it says (new, touched by a build or a
  command, a run that cannot say when it is due), so a tick visits the runs whose boundary has come. `animationiteration`,
  `animationend` and the finish events come at the frames a tick of every dormant element would give them. Dormant transitions are
  looked at only while one runs. `Retain::take_dormant_moves` tells the renderer who fell asleep and who woke, so the two stores
  keep their dormant set by `enter_dormant` and `leave_dormant` instead of being given the whole set every frame.
- **A tick that has only dormant work draws and presents nothing.** `GpuixView::dormant_tick` is a `Window::on_next_frame` callback,
  armed when a dormant source has something to tell ([wake times](#wake-times-90)). It moves the runs and transitions to `now`,
  steps native motion, emits the events and sets `built_at`, so `getAnimations()` reads the instant of the tick, and it does not
  notify the view. A patched tick moves the dormant clocks the same way before it builds its units.

A hover over 5000 cards clipped out of view with colour loops went from 37 to 43 ms to 3.0 ms (0.4 ms with no cards), and an idle
tick of them takes 0.00 ms. The remaining hover cost scales with the roots a frame replays, at about 0.5 µs a card; see
[retention.md](retention.md).

**What is dormant is narrower than what is culled.** A span, a `layout` element and a child spliced out of an animating
`display: contents` element are not dormant, and neither is a card in the overscan of a scroller: it is recorded in a region wider
than the clip and is not walked culled, so its loop is built as a visible one's. An element whose `opacity` is animated is not culled
in a scroller either (it is built every tick there as well), so a bound or animated `opacity` is not dormant. A positioned `<svg>`
still blocks culling, so it is never dormant and the offscreen cost of its loop is what it was. An `<input>` still blocks culling:
its key and input handlers are registered by its prepaint and paint, which a culled subtree does not run, so a focused input
scrolled out of view would stop taking text. It is never dormant, and an input outside the clip is prepainted, but it arms no blink
and asks for no frame while its caret cannot be seen.

## Wake times (#90)

A looping `motion` animation on culled content used to run `dormant_tick` at every display frame, and each run synced every motion
on the page (`sync_motions`, `parent_inherited`) though nothing was drawn: about 3.5% CPU per loop. A loop under `display: none` was
worse, since it is not dormant: every tick was a whole frame (60 built frames and 4800 motions synced over 60 ticks of 40 cards). A
culled CSS loop and a culled bound motion value ran a tick at every frame too, and a patched tick beside a visible loop stepped
every culled motion.

Chrome runs an animation on content it does not show without painting for it, and one that reports nothing (a loop with no
listener) is not run at all until it is visible: what it shows is a function of the time. So a dormant source says when it next has
something to tell and the window asks for no more.

- **A dormant source says when it next has something to tell** (`dormant_wake.rs`): a `Wake` is `Never`, `NextFrame` or
  `At(instant)`, merged by the earliest, and `GpuixView::wake_dormant` asks for no more than that. `NextFrame` (and an `At` that has
  passed) queues the frame callback of `dormant_tick`; a later `At` holds an executor timer for the wait that then queues it;
  `Never` drops the timer and asks for nothing. A new wake replaces the timer in place. The end of a frame, of a patch and of a
  dormant tick all go through it (`GpuixView::dormant_clock_wake` merges the CSS animations, transitions and motion values; the
  motions' wake comes from the split below).
- **`MotionState::wake`** is `At(end)` for a timed tween or keyframes (delay and finite repeats counted), `Never` for a repeat
  without a count, and `NextFrame` for a measurement that is pending. A spring that plays once, or a finite number of times,
  says `At(rest)`: the first instant a frame sees it at rest, which a 60 Hz scan of the closed form finds to within one step and a
  bisection of that step pins down (`Span::rests`, kept per track until its keyframes or entry speed change). A spring still moving
  at the end of the 20 s scan has no instant to give and stays `NextFrame`.
- **`CssAnimations::dormant_wake`** is the earliest instant a dormant run starts (the end of its delay) or ends, taken from a heap
  of them (`Sleepers::boundaries`), and its next iteration only for an element whose event path listens for `animationiteration`
  (`path_listens`, which is also what gates the emission; Blink's `RequiresIterationEvents`). Which iterating elements listen is
  asked again when the tree's revision moves. A run that cannot say (a `play()` waiting for a frame, a negative rate, an element
  just built or commanded) is `NextFrame`; a held one, and a loop nobody listens to, `Never`. `Run::events` holds the two instants
  beside `quiet_until`, which still decides which runs a tick looks at. `animationstart` and `animationend` wake whether or not
  anything listens, since the `Animation` objects of `getAnimations()` listen by existing.
- **`CssTransitions::dormant_wake`** is the end of the delay and the end of each dormant running transition (`Playback::wake_at`).
- **`MotionValueStore::dormant_wake`**: a moving value with a `change` listener is `NextFrame` (Motion's `change` follows the
  animation, not the element being seen); one with only an `animation` listener is the end of its animation when that is known
  (`MotionValues::wake`: a tween's clock, a repeat's end, a spring's rest); one nobody listens to is `Never`, whoever binds
  it. A follower (`Node::Spring`) wakes at its own rest while its source is still, and is `NextFrame` while its source moves (a
  loop without a count, which has no wake of its own, included): it retargets with each frame it is sampled in.
  `advance_dormant` only finds the events of the listened values and no longer samples the bound ones: the frame that draws an
  element again does, and a spring follower catches up (`MAX_CATCH_UP_FRAMES`). It leaves `frame_moving`, `frame_values` and
  `written` as the last build left them, which the frame that builds an element after it wakes has to find. `change` still comes at
  the display rate, `animationStart` and `animationComplete` when they happen, and a `finished` promise resolves out of view.
- **A motion with no box sleeps like a dormant one** (`GpuixView::motion_sleeps`: dormant, or hidden by `display: none` or a
  `content-visibility` ancestor), and so does a value bound only to such elements (`value_sleeps`), so neither asks for a frame. CSS
  animations and transitions under `display: none` are cancelled and their state dropped.
- **A tick steps what is awake or due.** `step_motions` splits the motions that move into the ones that are drawn and the ones that
  sleep (`MotionSplit`, taken at the end of a frame and after a step), steps the first, and the second only when the earliest of
  their wakes is due; `MotionStep` returns the awake ones and that wake. `MotionValueStore::step` leaves a moving value bound only
  to sleeping elements as it is (`BindingIndex::bound_only_to`). A frame built whole does the same (above).
- **`getAnimations()` and the commands of an element that is dormant read the clock**, not the instant of the last frame: no frame
  or tick need have come for it for any time (`GpuixView::api_instant`).
- **`getFrameWorkCounters().dormantTicks`** counts the runs of `dormant_tick` (`resetBuildCounters` does not clear it).

Dormant ticks over 60 ticks of 40 culled cards, clipped by an `overflow: hidden` scroller: a motion loop 60 before, 0 after; a CSS
colour or stroke loop with no listener 60 to 0, and the page costs what an empty one costs (painted frames, patched frames,
elements built, motions synced all 0); 20 culled bound motion values with no listener 60 to 0, with a `change` listener one a tick as
before. With an `onAnimationIteration` listener, 200 ticks of 40 loops of 1 s take 200 dormant ticks before and 3 after (one per
iteration), each delivered in the tick that passes its boundary. Culled tweens, CSS animations and CSS transitions of 500 ms end in
the tick that passes 500 ms, with 32 dormant ticks before and at most 2 after. A patched tick of one visible motion loop beside 40
culled motion loops and 40 culled CSS loops syncs 41 motions before and 1 after, the work of the visible loop alone. Under
`display: none`, 60 painted frames and 4800 motions synced before, 0 after. Over 150 ticks of 20 cards, a culled or skipped native
spring takes 40 dormant ticks before and 1 after, a spring motion value with no listener 41 before and 1 after, and a follower of a
value at rest 41 before and 1 after; the completion comes in the tick it does for the same spring in view. A spring with a `change`
listener keeps a tick a frame.

## Skipped content (#97)

Skipped content (`content-visibility: hidden`, or `auto` scrolled far away; see
[observers-and-content-visibility.md](observers-and-content-visibility.md)) costs the window no frame. A page of cards that each
run a `motion.div` loop, a `motion.path` `d` loop and a CSS colour loop draws nothing over 60 ticks, whether the loops started in
skipped content or were running when it began to be skipped: 0 painted frames, 0 patched, 0 dormant ticks, 0 motions synced, the
same as 0 cards. Two things used to cost, and are fixed:

- **A whole frame rebuilt the path to every skipped host.** `running_motion` named a motion by `is_animating` alone, so a motion
  under a host that skips (no box, nothing built) was a running source: `compute_deltas` put it in `changed`, the spine of the frame
  took its host and every ancestor, and a hover frame over a sidebar rebuilt the page around eight skipped hosts instead of
  replaying it (300 elements built over 20 hovers, 120 for cards that run nothing). It now leaves out a motion whose host has no box
  (`is_hidden`: `display: none` or a skipping ancestor; an `<svg>` shape counts as its `<svg>`), so a skipped subtree has no running
  source, as `Retain::skip_content` says. A motion on the skipping host itself is not skipped content and runs as before.
- **A tick that changed nothing drawn was drawn whole.** A paint-only write to a motion value bound inside skipped content requests
  a tick (`Repaint::MotionWrite`); the preview found no unit (the element is skipped), and `build_patch` declined it as `NoUnits`,
  so gpui drew the whole frame: 20 writes, 20 painted frames. A preview with no unit now steps the clocks and calls
  `repaint_in_place` with no root, which builds, lays out and presents nothing: gpui counts it in `PatchCounters::empty_ticks`
  (`getPatchCounters().emptyTicks`), and not in `patchedFrames`. `step_clocks` emits the transition and animation events the units
  would have. A preview with no unit but with elements about to enter the dormant set (or whose verdicts moved) is still drawn
  whole, counted as `Dormancy`, since the frame is what settles that: left to an empty tick, culled transitions never became
  dormant and their `transitionend` never came. `NoUnits` is no longer a key of `unpatchable`; `Dormancy` is.

20 hovers with 8 skipped hosts of looping cards: 300 elements built before, 120 after (the number for hosts that run nothing). 20
writes of a motion value bound in 8 hidden hosts: 20 painted frames before; 0 painted, 0 patched, 0 declines and 20 `emptyTicks`
after.

**An `auto` host inside the margin but clipped away** by an `overflow: hidden` scroller is not skipped content: it is built and culled, so its loops are dormant like those of any culled card. This holds where the host began visible and was scrolled or pushed out above or below the clip. Below the clip the content mask is empty, and an empty mask culls every child tested against it (see [culling](retention.md#culling-clipped-content-38)): 0 painted, 0 patched and 0 declines over 60 ticks, the cost of an empty page.

## Limits

- **A tick that gpui or gpuix cannot patch is a full frame, and says why.** A subtree of more than 256 elements is built whole
  (`unitsOversized`, `TooLarge`). A `position: fixed` element is no unit (`Excluded:fixed`), where an absolute one is. A loop on an
  element that holds an absolute box against a block outside it is `Escapes`. A scroll animation or scrollbar fade, a layout-only
  pass since the last full frame, the pointer, focus, a scroll, a measurement or any change that is not the clock make the tick a
  full frame (`getPatchCounters().unpatchable`). A scroll animation is always asked for as `request_animation_frame` and drawn
  whole. The run of an `opacity` or transform animation starts and ends with a whole frame.
- **A hover frame still scales with the roots it replays**, so culled cards are not free there (see above and
  [retention.md](retention.md)).
- **Layout-driving loops are full frames** ([units](#units)).
- **The glyph `d` morph** costs a parse, a raster and a texture a tick; the tape is compared through strings. Not changed ([what
  a patched tick still walks](#what-a-patched-tick-still-walks)).
- **An `<svg>` whose paint would start a job** is whole for one frame ([`<svg>`](#an-svg-is-culled-like-a-box)).
- **Open, `Unsteady` from FF's showcase.** It stopped reproducing with the same release addon and no change to the build or the page: first `Unsteady` 68 a second, then `Outside` 64 a second. The cause was not found (a different display state of the machine was suspected and not shown). The `<svg>` with no area is the class that reproduces `Unsteady` in a test and is fixed and pinned ([`<svg>` waiting for its raster](#an-svg-waiting-for-its-raster-and-motion-under-display-none)), but it is not proven to be the showcase's cause.
- **Open, FF's skipped ThinkingIndicator cards.** FF reported cards that were skipped and hold a ThinkingIndicator (a `motion.path` `d` loop and a CSS colour loop per letter) taking an idle page from 4 to 6% CPU and 0 whole frames a second to 20 to 23% and about 72 a second. It was not reproduced in the test renderer ([skipped content](#skipped-content-97)); it needs a measurement in a real window.
- **Open, iteration listeners.** They are asked again when the tree's revision moves, so a page that commits at every frame walks the
  path of each iterating dormant element at each.

## Breaking changes

A paint-only animation on content a frame walked culled does not build or draw: its clock, its events and `getAnimations()` move at
ticks that present nothing, as Chrome runs the animations of content it does not show. `animationiteration` and the other events
come at the frames a per-frame tick gives them, and `getAnimations()` reads the instant of the tick.

## Tests

- Transforms in three dimensions patch every tick with no decline of either kind (`getPatchCounters`): `transform-css-patch.test.tsx` for a transition of `rotateY` under a perspective, an animation of 3D lists that do not pair, of the depth of `transform-origin` around a rotation, and of the `perspective` and `perspective-origin` of the parent; `motion-transform.test.tsx` for a Motion tween of `rotateY` whose card hides the face that turns away on the patched ticks (`faces_away` is published with the box, not compared by the tape; [paint-and-transforms.md](paint-and-transforms.md#retention-and-patched-ticks)).
- `animation-frame-work.test.tsx`: at 500 and 5000 nodes every loop kind (keyframes colour, native `motion` colour, colour
  transition, svg stroke dash, `motion` and keyframes opacity, the colour loop of a relative box with and without a `z-index`, loops
  beside an `<input>`, `textarea`, focused input, selection, `<code>`, `<markdown>` and `<diff>`, loops under `relative`,
  `absolute` and `opacity < 1` wrappers and on absolute elements, beside an `<svg>` of no area or waiting for a job) does the same
  work per tick (elements built, prepainted, painted, primitives, retained and layout counters equal) with `patchedFrames` equal to
  the ticks, also with a screen reader on, and the picture equals the frame built whole at the same instant and patching off.
  Hover and a React commit draw a full frame and patching resumes; `animationiteration` comes once per iteration and `currentTime`
  moves per tick across 300 patched ticks; loops through exactly 1 and exactly 0 patch every tick. A `width` loop is drawn whole
  and says `LayoutAnimation`; a fixed text counts `Excluded:fixed`; a loop on an element that holds an absolute box against an outside
  block counts `Escapes`; each refusal on a page with no unit; the containing block moved by a click mid-loop is followed. The
  focused input is built 0 times across 20 ticks within a phase and once across a change of phase; one scrolled out of its scroller
  asks for no frame in 2 s of clock, takes the text typed there and blinks again when scrolled back. A loop beside 400 resting
  `motion.div`s syncs one motion a tick (401 after a resize, then one again), one bound to a colour beside 400 resting values samples
  one value a tick, a colour loop beside 120 boxes in view works out the orders of the loop alone. Motion-value cases: tween,
  spring and paused-then-played tween equal the frame built whole up to the tick the value stops being a source; a write to a
  scroller's colour or to a width is drawn whole. The tests that screenshot between ticks look at every third tick, since the tick
  after a screenshot is whole.
- `layout-idle-patches.test.tsx` (react) and the solid file of the same name: a looping opacity animation beside a box with no `layout`,
  `layout`, `layout="position"` or `layout="size"` (200 bars of 80 by 4 pixels, the page where nothing can be skipped) has 150 of 150
  ticks patched and none whole while the box rests, 30 of 30 whole while a 1 s glide runs (the box a half of the way at 30 ticks, and
  `onLayoutAnimationStart` once), and the ticks patch again once `onLayoutAnimationComplete` has been sent (at most 3 whole of 150); a glide
  after it starts from where the box is shown, also when it retargets one half way. Red before #117 for the three `layout` variants (150 whole, `NoFrame`). `retain.rs`: a layout box leaves a tick alone, `Gliding` says no, a page with a loop and
  nothing to skip is recorded at every frame; `layout_flip.rs`: `needs_frames` from the commit that moved a box until its events are
  sent, and for a box with `dragOffset`. `examples/layout-idle.perf.test.tsx` (`bun run test:perf:layout-idle`) is the guard.
- `offscreen-animation-work.test.tsx`: 200 cards with a stroke loop on a small and on a more than 4 KB `<svg>`, below the window
  and clipped; iterations and ends of five loops of other lengths come at the ticks of a shown one; a card scrolled into view has
  the pixels of a build with retention off; the dormant-tick, wake and skipped-content counts above. "A native spring that nobody sees" and "a spring
  motion value that nobody sees" (culled and skipped places; 20 cards): completion in the tick it has in view with at most two
  dormant ticks and at most one painted frame, none at all with no listener, a tick a frame with a `change` listener, the value of
  the current instant when revealed mid-run, a follower of a loop sampled each tick. "A frame built whole by a hover beside native
  motions that rest" (1 motion synced, 2 with a `whileHover`) and "the rows of a virtual list built beside native motions that
  rest" (the rows sync their own motions, not the page's). Runs on the debug addon (it used to abort at its first motion case).
- `retained-opacity-states.test.tsx` (36 cases): one card of a replaying page goes between 1 and 0.5, 0.5 and 1, and 1 and 0 through
  each state style and a React write, with no transition and with `opacity 240ms linear` stepped on the test clock, and equals
  retention off at every step, plain and under `GPUIX_RETAIN_VERIFY=1`.
- `content-visibility.test.tsx`, `content-visibility-auto.test.tsx`: skipped loops draw nothing over 60 ticks (the `auto` ones wait
  200 ticks, since the scrollbar fades and draws after a scroll); the culled-transition test pins `transitionend` coming.
  The skipped pages sync no motion over a hover (`motionsSynced` 0), a forced read of one card syncs at most 2 (4 under the
  verifier, which runs the pass twice), and "animations in auto content that is near but clipped away by an overflow hidden scroller"
  pins that content scrolled out above the clip, below it from the start and pushed out below it costs what an empty page does.
- gpui `window/patch_tests.rs`: a page of 10 and of 1000 static retained roots beside one patchable root whose colour moves does the
  same work (patch counters, zero replays and builds), the patched scene equals the frame built whole, also in a scrolled scroller,
  with the accessibility tree on, and with the pointer on a hitbox in the root; each decline reason; a tick that changes no
  operation presents nothing; a wrong patch is a mismatch under the verifier; lifted roots alone, between lifted boxes, positioned in
  their parent, and in a root that a frame replays; a lifted unit patched at 0.5, 1, 0, 0.25, 0, 1, 0.5 keeps the operations of
  every frame; a unit that holds the focused element; eight holders registered and repainted in place, with the verifier, a
  scroller, replay and hover over them, an absolute root under each holder with the block moving between ticks, and one that
  escapes the clip around it;
  `a_tick_that_changes_one_root_costs_the_same_whatever_the_page_holds` (`bounds_inserts` 10 for 5 ticks of the unit on 10 and on
  1000 rows).
- gpui `scene::tests`: a scene made again with other colours is the scene painted so (groups, a layer and glyphs included, with no
  order worked out); after something moved it is the scene painted so; a rebuild twice, and an insert after one.
  `cargo test -p gpui --lib --features test-support scene:: patch`: 92 passed.
- `retain.rs` unit tests: the unit choice (outermost, paint-only, size cap, exclusion, scroll, lift, absolute, nothing skipped
  inside), the preview (each decline, dormant, no effect on the next frame), the capturing tape, a patched tick and a tick declined
  after its clocks stepped under the verifier, each refusal and its key, an absolute element and one holding a box against it are
  units, the record made on the other side of a run, a blinking element is a unit unless a layout animation also runs, a caret is
  touched by the tick of its phase and no other, a bound element is touched when due and not otherwise, motion under no box, an
  `<svg>` waiting for its raster, an `<svg>` too big to raster at rest is a unit. `bindings.rs`: `binding_due` across a run, its
  end, a write, a transform, a control and an unparsable binding; `demand`; `advance_dormant` leaving the last frame's findings.
  `svg_paint.rs`: `unsettled`, the in-paint key, the culled note.

Run with `GPUIX_RETAIN_VERIFY=1` as well. Counters and the verifier are in [testing.md](testing.md).
