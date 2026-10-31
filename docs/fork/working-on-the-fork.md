# Working on the fork

How the two repos are laid out, which commit rules keep their history append-only, what a cherry-pick from upstream has to
preserve, how to build, where new code goes, and which platforms have been exercised. The conflict notes are the long part:
each says which invariant a file carries, so a resolution keeps the invariant and not one side of the diff. The procedure for
following upstream is in [syncing-upstream.md](syncing-upstream.md).

## Repos and branches

Two repos, one branch each that matters:

| | `fdarian/ghia` | `fdarian/zed` |
|---|---|---|
| Branch | `main` (GitHub default) | `poc` (GitHub default) |
| Holds | the gpuix code with the fork's commits on top | zed's `gpuix` branch with the fork's commits on top |
| `upstream` remote | `remorses/gpuix`, branch `main` | `remorses/zed`, branch `gpuix` |

Both branches are append-only: nothing is rebased, amended or force-pushed once it is on GitHub. gpuix `main` records the zed
commit it needs as the `zed` gitlink, so rewriting zed `poc` would leave old gpuix commits pinning SHAs that no longer exist.
There is no mirror of either upstream branch; the `upstream` remote is where [syncing-upstream.md](syncing-upstream.md) reads.

`zed/` is a submodule; `.gitmodules` points at `https://github.com/fdarian/zed` on `poc`, and that stays. The submodule's origin
is `fdarian/zed` on GitHub, a fresh worktree gets it with `git submodule update --init zed`, and zed feature branches are
pushed there. `main` in `fdarian/zed` is not part of the fork. Keep `zed/` on local `poc` and land zed changes there; a
feature may live on its own branch in either repo until it lands on `main` or `poc`.

zed vendors taffy 0.13.0 in `third_party/taffy`, wired in by `[patch.crates-io]` in both `zed/Cargo.toml` and
`packages/native/Cargo.toml`. Keep both entries pointing at the vendored copy: the layout features need the fork's patches (see
[Layout and scrolling](#layout-and-scrolling)).

## Commits

One commit per behaviour, with its tests and its docs: a feature commit carries its `docs/reference.md` text and the
matching `docs/fork/` text, as ordinary changes. `AGENTS.md` changes only when the code map or a rule every contributor needs does. There is no fork commit and no required order.

- **A commit that needs a zed change bumps the gitlink itself.** Land the zed commit on `poc` and push it first, then make the
  gpuix commit with `git add zed`. A zed commit that is not on `fdarian/zed` leaves a `main` nobody else can check out.
- **Do not bump the gitlink by accident.** `git add zed` and `git commit -a` stage whatever `zed/` is checked out at; check
  `git diff zed` before a commit that is not about zed.
- **Never rewrite what is pushed.** A fix to an earlier commit is a new commit.
- **`.gitmodules` stays `fdarian/zed` on `poc`.**

## Resolving conflicts

These are the invariants a cherry-pick from upstream has to preserve; resolve each conflict by keeping them, then
`git cherry-pick --continue`. Four files collect conflicts from every area: `packages/native/src/renderer.rs` (wiring for
each area's JS bindings, written three times: macOS, wasm and the Windows/Linux command path), `packages/native/js/host.ts`,
`docs/reference.md` and `AGENTS.md`.

- **Generated files.** The `index.d.ts`, `index.js` and `index.cjs` in `packages/native` are never resolved by hand: keep
  the fork's file and regenerate with `bun run build` in `packages/native`.
- **An upstream change** the fork already carries in its own form is skipped, not merged; the ledger says so.
- **Fork-owned files.** Many files below are the fork's alone, so a conflict there is upstream editing the code around them.
  The resolution keeps the invariant named, not one side.
- **`AGENTS.md`.** Keep the fork's file: it is Ghia's own orientation, and the topic docs hold the behaviour. Take from the
  pick only a line that is still true here.
- **`.gitmodules`.** A conflict means upstream changed the submodule URL or branch; keep `fdarian/zed` on `poc`.
- **`@ghia/` in a picked commit** becomes `@ghia/` (see [syncing-upstream.md](syncing-upstream.md)).

The notes below are grouped by the topic doc that describes the behaviour; read that doc for why the invariant exists.

### Frames and wakes

See [frames-and-wakes.md](frames-and-wakes.md).

- **Present log (zed).** `GPUIX_PRESENT_LOG` is a debug log in `gpui_apple`'s `metal_renderer.rs`. If upstream restructures
  how drawables are presented, re-add the log around the present call.
- **`flush_effects` guard (zed).** If `GpuiMode` or the inline draw block moved, keep the draw under
  `cfg(any(test, feature = "test-support", feature = "bench"))` and add the `self.mode.is_test()` check. If upstream gated
  this some other way, drop the commit.
- **Pump (gpuix).** Upstream owns `startFrameLoop`, `setFrameWakeCallback` and the zed frame hook; take its side of
  `packages/native/js/runtime.ts` and `src/renderer.rs`. Keep on top of it the exported `FrameLoopOptions`, `windowClosed` (it
  releases the sync key handler when the last window closes) and the Solid `frameLoop` option, and keep `frame-loop.test.ts`
  passing against upstream's `setFrameWakeCallback` return value. The other end of the pump is the fork's: the hook is zed's
  `set_run_loop_wake_hook` (upstream's `set_frame_wake_hook` is gone from the fork), `tick()` calls `run_loop_pumped()` after it
  clears `pending`, and `startFrameLoop` has no fallback timer on the wake path. Re-apply those three on top of upstream's
  side, or the idle window wakes at the display rate again. Without zed's NSEventThread observer
  (`gpui_macos::run_loop_wake`) it also never wakes on input.

### Window

See [window.md](window.md).

- **Appearance and fonts (gpuix).** Appearance conflicts are in `js/host.ts` (the `NativeRenderer` method pick-list must keep
  `setAppearance` and `getAppearance`), `src/renderer.rs` and `src/test_renderer.rs`. The `fonts` option lives in
  `js/fonts.ts`, `js/runtime.ts` (`init` wrapper) and `js/testing.ts`; it has no Rust. If upstream adds its own font option,
  drop the commit.
- **Window drag regions.**
  - gpuix: `src/events/app_region.rs`; `events/listeners.rs` (the press and release listeners: the region check comes before
    the dispatcher sees a press, and Windows must call `prevent_default` and return without stopping it); `renderer.rs`
    (`Inherited` carries `app_region` and `frameless`, and `to_gpui_window_options` sets `app_owns_titlebar_drag`);
    `events/state_styles.rs` (`overlay_fields!` lists `app_region`).
  - zed: `window.rs` (`window_control_area_at` and the platform hook that calls it, `pressed_mouse_buttons`, the cursor rule
    `drag_area_overrides_cursor`), `div.rs` (`window_client_area`, `WindowRegion`), and `gpui_macos` `start_window_move` (the
    state lock must be dropped before the call into AppKit). Every caller of `start_window_move` passes `cx`.

### Events and input

See [events-and-input.md](events-and-input.md).

- **Pointer events.**
  - gpuix: `renderer.rs` (the per-element mouse listeners and `wire_standard_events` are gone; keep them gone, not re-added by
    an upstream change), `src/element_tree.rs` (`EventPayload` fields), `js/mutations.ts` (`EVENT_PROPS`, capture props),
    `js/renderer-state.ts` (focus and forms rewrote it too), `src/events/` and `js/events/`. Upstream edits to hover or click
    wiring belong in the dispatcher.
  - zed: `window.rs` (`Hitbox` parent tracking, `hit_test`, `dom_hit_test`, `clipped_out`), `div.rs` (hitbox insertion,
    `Style::hit_clip_path`), `HitboxBehavior::PassThrough`, and the cursor and pointer-lock code in `gpui_macos`. DOM mode must
    stay opt-in per window.
- **Focus, keyboard, forms, a11y.** `src/focus.rs`, `src/forms.rs`, `src/accessibility/` (`aria.rs`, `label.rs`),
  `src/events/sync_dispatch.rs` and `keyboard.rs`, `js/focus.ts`, `js/forms.ts`, `js/events/keyboard-defaults.ts`, and
  `renderer-state.ts` and `EventPayload` again. In zed: `_accessibility.rs` and the `PathPromptOptions` / `KeystrokeEvent`
  changes. If upstream adds its own `tabIndex` or key-name handling, the DOM key names (`"Enter"`) win.
- **State styles.** `overlay_fields!` in `src/events/state_styles.rs` is described under [Paint and style](#paint-and-style);
  the `has*` fold order lives in `resolve_pointer_states`, and restyle gating in `events/listeners.rs` and `dispatcher.rs`
  counts `has*`. Anything a build publishes must be idempotent, since a pass can be dropped.
- **Observers.** `onLayout` and the observers are read from the published boxes: see
  [Forced reads, observers and content visibility](#forced-reads-observers-and-content-visibility).

### Layout and scrolling

See [layout.md](layout.md) and [scroll.md](scroll.md).

- **Layout and scroll code.** gpuix: `src/scroll.rs`, `scrollbar.rs`, `src/layout/` (`box_generation.rs`: keep `display: none`
  handling and drop the text-node branch if the inline builder owns it), `custom_elements/anchored.rs`, and `style.rs`
  (`DimensionValue` margins and insets). zed: `scroll_routing.rs`, the scroll listener in `div.rs`,
  `taffy/containing_block.rs`, `grid_template.rs`, `sticky.rs`, `elements/anchored.rs`, `list.rs`. Wheel candidates go
  through `scroll_routing::is_candidate`, which rejects `PassThrough` hitboxes: keep that.
- **Tables (gpuix).** `src/layout/table.rs` (roles, anonymous cells and tables), `layout/table_grid.rs` and
  `layout/collapse.rs` (the two must keep the placement of zed's `compute/table/structure.rs`), `css/table.rs`.
  `layout/display.rs` is the one place that knows the `inline-*` values and their blockification. `layout/contents.rs` keeps
  the inherited table styles (`borderSpacing` and the rest) through `display: contents`, and `box_generation.rs` reads
  `display()` for `display: none`.
- **Container queries and `:has()` (gpuix).** `src/layout/container.rs` and `css/container_query.rs`; `css/length.rs`
  (`LengthContext` carries the container boxes); `css/custom_properties.rs`; `events/state_styles.rs` (see
  [Paint and style](#paint-and-style) for `overlay_fields!`).
- **Vendored taffy** (zed `third_party/taffy`) is the biggest conflict surface. An upstream taffy bump means re-applying the
  fork's patches there.
  - Table and leaf code: `compute/table/`, `compute/leaf.rs`, `style/mod.rs`, `tree/traits.rs`, `tree/taffy_tree.rs`, the
    grid alignment files.
  - `tree/cache.rs`: measure entries stay fully associative, and the key keeps the sizing mode, the margin flags and the
    `modes` byte. If upstream lands taffy #1010, keep the fork's key.
  - `tree/taffy_tree.rs`: a style equal to the node's changes nothing, `mark_dirty` visits every ancestor, `set_children`
    tolerates stale parents, `remove` and `clear` drop the context, the dirty and layout generations, `mark_reached`,
    `set_unrounded_layout` bumps the `layout_generation` of every ancestor on a box that differs and stamps `box_changed_at`.
  - `compute/flexbox.rs`: the known dimensions are floored by padding and border, as block and grid do.
  - `tree/layout.rs` (`differs_inside`), `tree/traits.rs` (the measure function takes the `RunMode`), `compute/block.rs` (the
    float height rule).
  - `engine_fuzz_tests.rs` must pass on seeds 1 to 20000 after any of these (`GPUI_ENGINE_FUZZ_SEEDS`).
  - **Relayout boundaries** (`tree/taffy_tree.rs` `mark_dirty`, `is_relayout_boundary`, `SavedLayout`, `compute_layout`): the
    boundary list is Blink's, and `compute_layout` lays pending boundaries out with the input they were last placed with, so
    any rewrite of `PerformLayout` has to keep storing it (and drop it on a hidden layout). The generations above a boundary
    still move; if an upstream change makes `mark_dirty` stop without bumping them, retained subtrees and request signatures
    go stale. A boundary whose answer changed must fall back to the chain to the root. `relayout_boundary_tests.rs` (a
    differential fuzz) and the gpui engine fuzz (seeds 1 to 20000, which draws boundaries) are the check; floats are out of
    the fuzz.
- **The gpui layout engine** (zed `taffy.rs`) must stay persistent. Requests that have a scope take a node by key and compare
  before they set. `NodeMeta::requested` is the style the element asked for, which every rewriter (`intrinsic.rs`,
  `natural.rs`, `containing_block.rs`, forced and stretched widths) must read and restore. `end_frame` is the only collector
  of a presented draw. A new request path must either be keyed or count as unstable. `escapes_subtree` decides whether an
  absolute box makes a retained subtree unstable.
- **In gpui** `window.rs` (the draw pass loop), `elements/div.rs` (prepainted-layout listeners, the table painting hook),
  `elements/text.rs`, `text_system/` and the platform shapers, `style.rs`, `taffy.rs`, `view.rs` also collect layout-feature
  conflicts.
- **Scroll anchoring** (zed `elements/scroll_anchoring.rs` and its state on `ScrollHandleState`): the anchor is picked in
  `before_layout`, called by `compute_layout_in` before the layout is computed and never at the end of a prepaint; `step` only
  compensates, after the offset is clamped; an offset other than the one the step left is an explicit scroll and drops the
  anchor; the suppression flags set in `reuse_node` are cleared at `end_pass`. Gpuix's `scroll_reporter` reports nothing in a
  layout-only pass. `scroll-anchoring.test.tsx` (the Chrome scenes) is the check.
- **Layout-only passes** (zed `window.rs`): `build_next_frame` passes (`draw_pass`, `refreshing`, `layout_pass`, `measuring`),
  `draw_if_dirty` / `paint_if_dirty`, `publish_layout`, the recording and replay of the frame's index ranges, and
  `lies_outside_the_mask`. A layout-only pass must not swap frames or record a frame (it leaves layout records only). See
  [forced-reads.md](forced-reads.md).
- **Culling** (gpuix `layout/cull.rs`): new elements that draw outside their box or are placed by something other than their
  parent call `block()`. The zed side is under [Retention](#retention).

### Text

See [text.md](text.md).

- **Atomic inlines (gpuix).** `src/text/atomic.rs`, `text/inline.rs` (the planner), `text/paragraph.rs`, `text/paint.rs`,
  `css/vertical_align.rs`. The planner and the table role read `RetainedElement::display()`, never `style.display`, or an
  `@container` display switch stops working.
- **Selection order (gpuix).** `text/selection.rs`, `text/search.rs`, `text/paint.rs` (`DocPos`, `SelectableText::path`).
- **Text units (zed `elements/text.rs`, `text_system/`).** Text is laid out in 1/64 px and snapped when painted;
  `chrome_layout_units` has the Chrome numbers.
- **Measure keys (gpuix `text/inline_insets.rs`, `text/atomic.rs`).** The measure key of a text holds the insets' key
  (`InsetSpec::text_key`) and the atomic inlines' digest.

### Paint and style

See [paint-and-transforms.md](paint-and-transforms.md).

- **Files.** gpuix: `src/css/`, `src/color.rs`, `src/text/inline.rs` and the `<svg>` code under `custom_elements/`
  (`svg_paint.rs`), plus `style.rs`. zed: `scene.rs`, `effects.rs`, `style.rs` (`border_colors`, `ClipPathValue`), and the
  `gpui_apple` Metal pipeline.
- **`overlay_fields!` in `src/events/state_styles.rs` must list every `StyleDesc` field.** It is a struct literal, so a field
  missing from it fails to compile, and every area that adds a style field (layout, paint, motion, drag regions) touches it.
  It also lists `container_type`, `container_name`, `container` and the `@container` blocks, and layers `custom_properties`.
- **A change to `Quad` needs the shader and the Rust struct to move together,** on every backend, or the stride checks fail.
  `Underline` is a GPU struct too: change the Rust struct, `shaders.metal`, `shaders.wgsl`, `shaders_webgl.wgsl` (stride and
  loader) and `shaders.hlsl` together.
- **`StyleDesc::differs_only_in_paint`** (`style.rs`) and **`AnimProp::paints_only`** are the paint-only key list. A style
  field that is not copied over the clone is a layout key, which is the safe side. A field added to the list must not move or
  size a box, change a stacking context or a clip, or start a motion, or a bounds read after the write answers from a stale
  layout. `batch_tests` in `renderer.rs` pins the list.

### Motion

See [motion.md](motion.md).

- **Springs (gpuix).** Conflicts are in `src/motion/`, `js/host.ts` and the React `motionKey`. `MotionState::sample` returns
  position, velocity and `active` together; keep `sync` taking the sampled velocity, not just the visible style.
- **Motion proper.** `src/motion/`, `js/motion-*.ts`, `keyframes.ts`, `presence.ts`, `reorder.ts`, `layout-animation.ts`,
  `match-media.ts`, the React `motionKey` and the Solid host. The zed commit is `on_painted_layout` and content-size exposure.
  Hover and active come from `PointerState`; do not reintroduce a probe element above a hitbox.

### Retention

See [retention.md](retention.md). Every file here is the fork's.

- **zed `elements/retained.rs`, `window/retained.rs`:** the conditions to skip, the `ReplayKey` and the `NonReusable` reasons.
  A new `Window` field that a prepaint or paint reads belongs in the key; a new thing that a replay cannot repeat belongs in
  `NonReusable`, and one that a moved replay cannot repeat in `NonRelocatable`.
- **zed relocation** (`window/retained.rs` `Relocation` and `ReplayKey::relocation`, `scene.rs` `replay_translated` and
  `translate`, `window.rs` `snap_bounds`, `layout_bounds`, the underline, glyph, emoji and svg quantisation, `util.rs`
  `round_to_device_pixel_down`): the rounding is translation-covariant, and an upstream change to how painted geometry is
  snapped has to keep a shift by whole device pixels moving the result by the same shift. `relocation_tests.rs` and the fuzz
  (`RETAINED_FUZZ_OFF`) are the check.
- **zed holes and fragments** (`window/retained.rs` `LiftRecord`, `PlainPaint`, `lifted_holes_since`, `replay_lifts`;
  `window.rs` `begin_lift`, `finish_lift`, `begin_lift_paint`, `paint_lifted`): a change to the stacking context's lift list
  must keep ordinals reserved for a replay, the owners of an entry and the `Lifted*` phases to the hooks. `hole_tests.rs` and
  `fragment_tests.rs`.
- **zed `elements/div.rs`, `element.rs`:** `prepaint_unlifted_child` (culling) and the `*_culled` methods. The culled paths
  must keep publishing bounds, scroll metrics, tab stops and a11y nodes. Anything that keeps a position past the frame calls
  `note_positioned_capture`.
- **gpuix `retain.rs`:** `inherited_fingerprint` lists every `Inherited` field (a size test fails otherwise);
  `compute_deltas` lists every source of volatile state, and `preview` declines a patch for any source it does not name as
  time-driven; `Exclusion` is the list of what a replay cannot repeat; `Scroller` and `Relocation` decide what a scroller
  moves; `worth` is the `MIN_ROOT` rule. If upstream adds a build input, it goes in one of those or in the tape.
- **gpuix `tape.rs` and its call sites** (`tape::write` in `automation.rs`, `text/paint.rs`, `text/inline_hit.rs` and
  `motion/measure.rs`; `tape::note_build` in `motion/measure.rs` and `background_images.rs`): a registry write in a paint
  listener goes through `tape::write`, and an effect of building goes through `note_build`, or it is lost when a frame skips
  the subtree. The phases (`Prepaint`, `Paint`, `Layout`, `Lifted(rel)`) follow gpui's `RetainedPhase`; the hook map is in
  `renderer.rs`' `retained_root`, and an entry is the write and its offset. A replay does not write the registries: it
  publishes the lists it puts back (`tape::publish`) and a registry reads them when asked (`tape::Layered` for those that
  keep an order, `tape::owner` and `delta_of` for the painted boxes), so a new registry has to be read that way or be written
  on replay at a cost per element.
- **gpuix `renderer.rs`:** the `retain.*` notes between `enter` and `exit` in `build_element_unpumped` record what a build
  read, and `retained_root` / `build_subtree` / `FrameEnv` must hold everything a late build needs of the build around it.
  `set_retention_whole_frame(false)` stays. Keep `slot.rs` borrows: they are the 1.6 KB moves the build avoids.
- **Layout records** (zed `elements/retained.rs` `LayoutRecord`, `window/retained.rs`, `window.rs`): a layout record is
  skipped with only under the conditions of a painted recording (same token, no notified entity, node alive, same engine) plus
  the stamps that say the box its tape holds is still the last one written: `rendered_serial` (a draw ends every record),
  `refresh_epoch` (`Window::refresh` ends the ones before it) and `reference_passes` (a verify pass builds inline). A pass
  that is abandoned, or whose prepaint a `List` rolls back, must never run the last publication, or its records would be
  skipped with. A layout tape never holds a painted tape (`layout_painted_skips`: `tape::PARENTS` would be corrupted), and the
  painted `Recorded` is never touched by it. gpuix `tape.rs` `Tape::laid_out` / `TapeStore::laid_out` and `tape::LAID_OUT` in
  the serial keep the two sets of boxes apart in `automation::Stored`. `layout_record_tests.rs` and
  `forced-layout-incremental.test.tsx`, plain and under the verifier, are the check.

### Patch frames

See [patch-frames.md](patch-frames.md).

- **zed `window/patch.rs` and `Window::request_tick`:** a patch must decline for anything a recording cannot repeat, and
  change nothing it declined (the element states go back, the layout pass ends). A new `Window` field that a prepaint or
  paint reads belongs in the `PatchUnit` surroundings as well as in the `ReplayKey`. `Window::element_states` (`StateOwner`,
  the pass stamps) and `Origin` (`range_now`) are what makes a replay a step per root: a path that copies states or visits
  nested roots per element undoes it. `patch_tests.rs` and the retained fuzz under the verifier are the check.
- **gpuix `retain/patch.rs`, and `begin_unit`, `end_unit` and `preview` in `retain.rs`:** a unit builds alone only from what
  its kept `FrameEnv` holds, and a write that bypasses the tape (`report_scroll`) makes an element with a scroller inside no
  unit. A new source of volatile state is a reason in `Unpatchable`, and `Dormancy::holds` must read nothing a build of the
  element does not tell or `Sources::verdicts` does not move.

### Forced reads, observers and content visibility

See [forced-reads.md](forced-reads.md) and [observers-and-content-visibility.md](observers-and-content-visibility.md).

- **Observers (gpuix `observers.rs`, `automation.rs`, `retained_tree.rs` `geometry_ids`, `js/observers.ts`):** `onLayout` and
  the observers are read from the published boxes in `frame_sweep`, in the order scroll, `onLayout` and `ResizeObserver`, then
  `IntersectionObserver`. A paint listener that reports geometry for them would exclude the element from retention again.
- **`RelationRegistry`** (`accessibility/relations.rs`, on `RetainedTree`): `links::compute` reads only what it lists. A prop,
  id, role or text mutation that `compute` starts to read has to reach `reads`, `prop_changed`, `text_changed` or
  `restructured`, or the links stay stale, and `compute` never walks `tree.elements` (`relationVisits`).
- **`Relevance`** (`layout/content_visibility.rs`): `ForcedRead` holds the `gpui::LayoutStamp` read after the forced pass's
  trailing invalidation, the `HostPlace` of each forced host and the clocks of transform writes and layout animations. A new
  invalidation that can move a box has to go through gpui's invalidator (not `notify_paint_only`), or a read shares a pass that
  is stale, and so does a box that a frame moves without a taffy box (`TaffyTree::box_generation` and the per-node
  `box_changed_at` are set in `set_unrounded_layout`; a layout written anywhere else would not count). A new paint-only
  write that can change a transform must reach `note_transform_write` (`assign_style` does it for a style, the motion-value
  write for a bound key), or a read answers with the transform from before it. `animate_style` rewinds the animation and
  transition state of an element a read forces: keep any state a new animation kind adds in `rewind_point`.
- **Skipped-content nodes** (`accessibility/skipped.rs`) come from `apply_accessibility` on a throwaway `div`; do not write a
  role or a name there that the built path does not also write. `focus/skipped.rs` and `text/search.rs`
  (`announce_skipped`, `settle_frame`) follow the same rule: the retained tree, not the built one, is the source.

### Webview and window composition

See [webview.md](webview.md). The zed side is new files plus small hunks in shared ones (`git -C zed log --oneline` lists them: the
`gpui: native surfaces` and `gpui_apple`, `gpui_macos` composition commits, the `PlatformWindow::composition_frame(s)` recorder,
the keyboard/press commit and the transaction-flush commit). If upstream's zed PR #62379 lands, drop them and keep only the
invariants below.

- **Files that are the fork's alone** (a conflict there is upstream editing around them): `crates/gpui/src/composition.rs` and
  `composition/{tree,present,hit,tests}.rs`, `crates/gpui/examples/native_surface.rs`, `crates/gpui_apple/src/composition.rs`,
  `crates/gpui_macos/src/window/composition.rs`, `crates/gpui_macos/tests/native_surface.rs`; in gpuix
  `src/custom_elements/webview/`, `src/platform/webview_{macos,pulse,url}.rs`, `js/webview.ts`, `examples/webview-e2e/`.
- **`scene.rs`.** `PaintOperation::NativeSurface` must be recorded (`self.record(...)` in `insert_native_surface`) and replayed in
  both the plain replay and the translated one (`translate` of the placeholder, then `clip_mask` when the replay is clipped), and
  printed in the operation log (the hitbox is left out: a new build inserts it with a new id). `native_surfaces` is sorted by
  order in `finish`. A placeholder inside an effect group goes to `pending_native_surfaces` and takes its order when the
  *outermost* group closes. `batches_in(OrderRange)` is `batches()` over `partition_point` slices; the iterators' `*_start` must
  stay offsets into the whole vectors. Re-apply: if upstream restructures the scene's vectors or the batch iterator, redo
  `within` per vector, and keep `batches()` equal to `batches_in(ALL)`.
- **`window.rs`.** Four hooks: the `composition: WindowComposition` field; `crate::composition::present(self)` where the window
  hands its finished scene to the platform (it must stay the only place, and a window with no surface must still call
  `platform_window.draw`); `Hitbox::parent_id`; `snap_bounds` and `snapped_content_mask` `pub(crate)`; and the cursor rule
  (`!pointer_over_native_surface`) where the window sets the cursor. Re-apply: if upstream moves the draw hand-off, put `present`
  at the new place; if hitboxes lose their parent link, `overlay_hitboxes` cannot exclude the page's ancestors and a layer would
  shut the page out of the pointer.
- **`platform.rs`.** `PlatformWindow::draw_composed` (default: `draw` the whole scene), `create_native_surface` (default: error),
  and under `test-support` `composition_frame(s)`; `platform/test/window.rs` records composed frames and surface events. A new
  platform window implementation compiles through the defaults.
- **`gpui_macos` `window/composition.rs`** and its hooks in `window.rs`. Invariants: the window class has `sendEvent:` routed by
  `register_window_input` (and `COMPOSITION_INPUT_IVAR` set once a surface exists, so other windows pay nothing); a pointer event
  bound for a native container is shown to GPUI first and marked `pre_dispatched_event` so `dispatch_view_event` returns `None` the
  second time; `native_buttons`, `gpui_buttons` and `withheld_buttons` pin a button's recipient from press to release; the
  press is withheld only when GPUI *stopped* it, not on `default_prevented`; wheels never enter `send_event`; views are sorted in
  place (`sortSubviewsUsingFunction`), never removed and re-added; `draw_composed` presents with the transaction when anything
  changed and flushes `CATransaction` after the frame **outside** the state lock; `performKeyEquivalent:` of the window view
  defers to `-[NSView performKeyEquivalent:]` while `keyboard_is_native`; `reset_cursor_rects` uses `cursor_regions`;
  `scale_factor_changed`, `drawable_size_changed`, `window_closed` and `set_presents_with_transaction` are called wherever the
  window sizes, closes or changes how it presents (every direct `renderer.set_presents_with_transaction` was replaced).
  Re-apply: after an upstream change to the window view's event methods, check `handle_view_event` still returns the dispatch
  result (`dispatch_view_event`), that every `set_presents_with_transaction` goes through the composition's, and run
  `native_surface`.
- **`gpui_apple`.** `MetalRenderer::draw_orders(scene, range)` draws the primitives and effect groups in a range of orders and
  clears the rest (an empty range presents a clear overlay); `new_layer(device, transparent)` and `new_internal` are
  `pub(crate)` for `new_overlay_renderer`, which shares the base renderer's device, sprite atlas and colourspace (a tile in one
  atlas is valid in every layer). The group queue takes the range of groups (`group_indices`). Re-apply: if upstream changes how
  a frame is encoded, `draw_orders` must still draw one range and clear the target when it is empty.
- **Focus (gpuix).** `focus.rs` carries `PageMove` through `observe_focus_in` -> `settle_focus`; `step_focus` records the Tab
  direction (`tab_step`) before settling. `GpuixWebView` answers nil from `nextValidKeyView` / `previousValidKeyView` and calls the
  element's `FocusExit`. `<webview>` clears the keyboard focus dim in its own render, not in `apply_focus_styles`.
- **Tests that catch a bad re-apply:** zed `cargo test -p gpui --lib --features test-support composition`, `-p gpui_apple`, `-p
  gpui_macos --test native_surface` (a real, unoccluded window), then the `webview.test.tsx` files and the e2e
  ([testing.md](testing.md#the-webview-e2e)).

### Tests

See [testing.md](testing.md). The test renderer (`test_renderer.rs`, `js/testing.ts`) keeps the switches in
[Switches and counters](testing.md#switches-and-counters), and `packages/native/index.d.ts` is regenerated, not merged.

## Building

The addon is one build with `test-support`, as upstream:

```bash
cd packages/native
bun run build         # release addon, same flags as upstream's `bun run build`
bun run build:debug   # debug addon; never use it to judge frame time
```

The release profile sets `strip = "none"`: rustc's post-link `llvm-strip` drops the linker's 4-byte pad, so the string table
offset is 4 mod 8 whenever the indirect-symbol count is odd, and macOS 27 dyld refuses to load the addon. Finish with a
release build: scratch apps link this checkout, and a leftover debug addon looks like lag. Set `GPUIX_PRESENT_LOG=<file>` to
append when each frame reaches the screen to that file. There is no separate test addon (see the rejected two-addon design in
[frames-and-wakes.md](frames-and-wakes.md#no-inline-draws-in-real-windows-patch-1)), so `TestGpuixRenderer` is in the same
file.

After a reconciler change in `packages/react`, run `bun run build` there. The `examples/` apps and `bun --hot chat.tsx` load
`packages/react/dist`, while its vitest suite loads `src`, so a fix can pass one and fail the other. Never run `bun run clean`
there while the web dev server (`bun scripts/web.ts`) is up: the folder is in the server's module graph, and removing it
corrupts the registry until the server restarts (every page load fails with `Failed to load bundled module
'packages/react/dist/index.js'`). `build` is plain `tsc` for that reason and the wipe is a separate script; clean first, then
start the server.

Some machines need workarounds that do not belong in the fork: no `metal` compiler (enable `gpui_macos/runtime_shaders`), or
an Xcode license that has not been accepted (point `DEVELOPER_DIR` at the Command Line Tools). The demo that uses this fork
keeps its own build script for them.

## CI and releases

`ci.yml` runs on pushes to `main` and on pull requests (not for changes that only touch Markdown, `docs/`, `LICENSE`, `NOTICE` or
`.changeset/`) as one `check` job on `macos-latest`: the native build, the Solid and React builds, then `bun run test` in
`packages/native`, `packages/solid`, `packages/react` and `examples`. A failing suite does not skip the ones after it. The `*.perf.test.tsx` files and the e2e apps are not run.
There is no format job and no Turbo: a format commit over upstream-derived code would conflict with every pick.

A user-visible change adds a changeset with `pnpm changeset`. Pushing it to `main` runs `release.yml`, where the changesets
action opens or updates the `Version Packages` PR. `@ghia/native`, `@ghia/solid` and `@ghia/react` are `fixed`: they share a
version. Merging that PR runs the action again: `bun run ci:release` builds the addon and both adapters on the runner and
`changeset publish` publishes the three packages with provenance, through npm trusted publishing (OIDC, no token). Each
package's `prepublishOnly` refuses to run without `CI`, so nothing is published from a laptop by accident.

`@ghia/native` ships its addon in its own tarball (`gpuix-native.darwin-arm64.node`) and has no platform packages; only
`aarch64-apple-darwin` is built.

The first publish of a package cannot use OIDC, because a trusted publisher can only be added to a package that exists on npm.
Publish each package once by hand from a checkout of the version commit, after building the three (`pnpm publish` turns
`workspace:^` into the version, `npm publish` does not):

```bash
CI=true pnpm --filter @ghia/native --filter @ghia/solid --filter @ghia/react publish --access public --no-git-checks
```

Then on npmjs.com add a trusted publisher to each package (Settings, Publishing): repository `fdarian/ghia`, workflow
`release.yml`, branch `main`. `repository.url` in each `package.json` must stay `git+https://github.com/fdarian/ghia.git`, or
the publish is rejected.

## Where new code goes

New code lives in new modules (`packages/native/src/events/`, `scroll.rs`, `layout/`, `css/`, `motion/`,
`packages/native/js/events/` and so on). `renderer.rs`, `style.rs` and `host.ts` get wiring only, which keeps picking from
upstream tractable.

- **Layout numbers live in `Theme::metrics`, not in a Rust `const`.** A new number that decides layout goes on `Metrics` with a
  default, into `MetricsOverride` and into the `GpuixMetrics` TypeScript interface, so a design tweak is a re-render and not a
  native rebuild. It also goes into `Metrics::hash_diff_layout_into` if it changes the height of a `<diff>` row, since a changed
  hash resets the list; the markdown and code metrics stay out of it because a reset drops the scroll anchor.
- **Framework-neutral TypeScript belongs in `@ghia/native`, not in an adapter.** `@ghia/native/host` owns host contracts,
  mutation batching, renderer-local ids, event routing, observers and text search. `testing` and `automation` own the shared
  test wrapper and automation client. None of these subpaths loads the `.node` addon at import time except `testing`, which
  constructs the native test class. React and Solid keep framework scheduling, context, JSX integration and components.
- **The mutation queue carries raw objects.** `setStyle` and `setCustomProp` take raw JSON values; encoding either before the
  outer batch is stringified adds a second parse and turns a string such as `"top"` into nested JSON.
- **Ported code.** `text/`, `syntax/`, `markdown/`, `diff/`, `theme.rs`, `custom_elements/code.rs`, `custom_elements/diff.rs` and
  the caret blink in `custom_elements/input.rs` come from [Comet](https://github.com/zeronsh/comet) (MIT); each file names its
  original in its header and `THIRD_PARTY_NOTICES.md` has the table. Read the original first when fixing one: it usually says
  why the code is shaped as it is.

`docs/reference.md` is the API contract, as `AGENTS.md` requires: behaviour is documented there, and `docs/fork/` holds the
design and the reasons.

Native views are an exception to "new code in new modules" only at the edges: the zed side of `<webview>` is the new `composition*` modules plus the small hunks listed under [Webview and window composition](#webview-and-window-composition), and nothing else in gpuix knows about a native surface.

## Platforms

macOS is the platform that was exercised. Several features have a macOS-only path (Metal effects, pointer lock, the
synchronous dispatch that lets `preventDefault` cancel a native default, the OS reduced-motion read). The Windows, Linux and
wasm paths were written but not compiled by the people who wrote them, and the zed shader edits for them were never built or
rendered. The gaps, and what closing each would take, are listed in [deviations.md](deviations.md#other-platforms).

The wasm build (`bun run build:web`) needs nightly Rust and `wasm-bindgen`.

The paint changes touched zed crates outside `gpui` (`editor`, `ui`, `terminal_view`, `onboarding`, `keymap_editor`), and those
were not built. `cargo check -p editor -p ui -p terminal_view` would; it matters only to someone building Zed from this
submodule.
