# Retention: frames that skip what did not change

A frame used to build every element, lay out every node and paint every quad, whether or not anything changed. Retention lets a frame leave unchanged subtrees alone and put back what they drew in the frame before, so an idle or hover frame costs the roots that changed and not the page. This file covers how that works, the contract it keeps with a full build, and what you must not break when you add a build input.

Animation ticks that repaint only some roots are [patch-frames.md](patch-frames.md). Bounds and scroll reads that lay out without drawing are [forced-reads.md](forced-reads.md).

## Why three layers (#37)

An idle page of 2042 elements cost 16.8 ms (about 8.3 µs an element), and one node animating on it cost the same, 17.0 ms. The same page costs 1.29 ms idle and 1.51 ms with one `opacity` node animating (2 elements built) with retention on ([Measured](#measured)).

gpui consumes its elements every frame: the element arena is cleared and a `Drawable` runs once. A frame that does not build a subtree therefore has to put back what the subtree prepainted and painted in the frame before, and that only works if its layout nodes are still there. So:

1. the layout engine must persist (layer 1);
2. gpui needs an element that can be replayed (layer 2);
3. gpuix has to decide which subtrees a frame may leave unbuilt, and repeat what building them did outside the frame (layer 3).

Each layer is useful alone and was committed and measured on its own.

**The contract.** Apart from the list in [Where a frame differs from a full build](#where-a-frame-differs-from-a-full-build), none of this is behaviour. A frame must be the frame a full build of the same state gives, and where it is not that is a bug. `GPUIX_RETAIN_VERIFY=1` builds every frame that replayed once more and compares ([testing.md](testing.md#the-retention-verifier)); a build input that is in none of the [invalidation rules](#invalidation-rules) is a hole, and verify mode is how holes are found.

## Layer 1: a layout engine that keeps its nodes

zed `crates/gpui/src/taffy.rs` and the vendored taffy. The engine is not cleared between frames, and a request finds its node again by a key (a scope plus its ordinal in that scope) and sets only what differs, so taffy's per-node cache survives for every node a frame leaves alone. Measured leaves (text, image, lists, `<input>`) are keyed by a *measure key* that must hash every input of the measure closure; a key that misses one makes a node keep a size it should not. Nodes are freed at the end of a presented draw by reachability from the roots laid out in it. The vendored taffy patches (each marked `fdarian fork patch`) include the associative measure cache that fixed #36, where nested auto-width columns laid out in exponential time (one level 11 µs, twelve levels 6174 µs). The mechanism, the patches and the engine fuzz are in [layout.md](layout.md#the-layout-engine-persists-across-frames).

Two things in layer 1 matter to the rest of this file. `NodeMeta::requested` keeps the style the element asked for, because gpui rewrites some boxes after the request (width keywords, natural widths, absolute boxes whose containing block is not their parent, window roots), and a request compares with that. And the `layout_generation` of a node moves when the node is laid out with another size, padding, border, scrollbar or content size, which is what a replay checks.

## Layer 2: `gpui::retained`

zed `elements/retained.rs` and `window/retained.rs`. `retained(id, key, build)` wraps a subtree with a key its owner computes from everything the subtree is built from. The owner promises that equal keys mean the subtree draws, lays out and reacts the same, including what its elements read without being passed it (hover, focus, running animations). gpui checks only what it owns.

A frame whose key is the one the frame before recorded does not call `build`. It lays out the nodes the engine kept (the retained element numbers its requests under a scope of its own, so id-less siblings after it keep their ordinals, and it keeps the element states of everything inside it, see [What a replay costs](#what-a-replay-costs)). After layout it replays the recorded prepaint and paint if the surroundings are the recorded ones, and otherwise builds the subtree then, in prepaint, onto the nodes it already has.

**What a replay puts back**: hitboxes and tab stops (by value), the dispatch tree, focus, tooltips, line layouts, element states, the entities the subtree read, accessibility nodes (with relations, focus ids, bounds and action listeners), the scene, listeners, cursor styles and input handlers.

**What must match to replay** (`ReplayKey`):

- the subtree's bounds (so its origin), its unsnapped size and the sub-pixel phase of its unsnapped origin;
- the content mask and exact clips, text style, visibility, element opacity, rem size, scale factor, viewport size, and the ambient transform;
- the positioned frame and element offset, the current view and image cache, whether a11y is active, whether culling and lifting are on, DOM hit testing;
- how many of the contexts around the subtree are stacking contexts and how many of the elements around it are handed to one (a subtree is not replayed after an ancestor starts to be painted from its stacking context);
- the root's `layout_generation`, as recorded.

A mismatch builds the subtree lazily. If its layout requests do not land on the nodes it has (nodes made, styles or children set), it is laid out as a root of its own and not recorded (`LayoutMismatch`). The size and the phase are in the key because the boxes inside snap edge by edge from the root's origin: a root that keeps its snapped box while its origin settles at another fraction (a sibling that animates to a width in percent) has them at other sizes. Two phases are equal within 1/256 of a device pixel, on the circle (0.999 and 0.001 are close): above the rounding of the `f32` sum of offsets that gives the origin, below the 1/64 that sizes measured from text are quantized to.

**What stops a frame from skipping at all** (`request_layout`):

- a draw pass above 1 (container queries draw several), a measuring pass that is not layout-only, the inspector;
- `Window::refresh` (which `set_rem_size` calls when the size changes);
- an entity the subtree read being notified (the view rendering it excepted, since its notify is what draws the frame);
- a different engine or a dead root node;
- no recording painted in the frame that is rendered (and, for a subtree that holds holes or fragments, none of them painted in it);
- a text style, rem size or scale factor around the subtree that is not the recorded one. This is checked here and not only in prepaint because the text style is not in gpuix's key (`white-space`, `text-wrap-*`, `text-box-edge` and the rest reach a text through gpui's inherited `TextStyle`), and a text that wraps differently moves the boxes that layout has placed by the old size by the time prepaint could build it.

**What makes a recording non-reusable** (`NonReusable`, counted by `getRetainedCounters`):

- a box lifted into a stacking context that holds neither a hole nor a fragment for it (`Lift`);
- a box drawn outside its clip (`Escape`);
- a deferred draw; a nested layout tree (a virtual list's rows);
- layout nodes that are new every frame or rewritten (`UnstableLayout`); a root with no layout result; `LayoutMismatch`;
- a redraw request, an autoscroll request, a frame callback, an asset still loading (including image caches and custom sources), a window control area;
- a cached view drawn from its last frame (`CachedView`);
- a paint listener that reports bounds ran with no `on_replay` hook on the element (`Publication`).

**Requirements on the caller.** The element needs an element id of its own that is unique among the elements around it; it is not on the id stack, so wrapping a subtree changes no id below it. The root of the subtree must not escape its clip. If it is lifted, say so with `lift_z`; `cull_outset` forwards how far the subtree inks, for culling.

### Holes and fragments

gpui lifts a positioned element out of its parent into the nearest stacking context, which paints it in z order with the others, so the paint of a root is not one run and a root that lifts something is not replayable by copying its run. Three rules decide what a recording does about the boxes it hands over:

- **Lifts that stay inside the recording are not lifts.** The recording takes a mark of the ambient lift list when it starts and, when it ends, looks only at the entries it added to it, which are the boxes that left the recording for a context outside. A stacking context root whose positioned descendants are all consumed by it adds none, and replays.
- **A hole** is a lifted element that is a retained root of its own (`retained(..).lift_z(z)`). The recording of the root around it keeps a stub for it (its id, z, its ordinal relative to the first box the recording handed over, its hitboxes by value, mask, paint context and image cache), and a replay reserves ordinals in the live lift list, pushes one entry per stub and has the context paint the hole from its own recording. The hole is not asked for alone: the key of the root around it holds the hole's token, so a hole that changes or animates builds the root around it (it stays on the spine). A hole that read entities replays only while they are unchanged, and observes them again.
- **A fragment** is any other lifted element, such as an `absolute` box placed by a positioned box inside the root. While a recording is open, the context records what it paints for the element into a cell (`PlainPaint`: the paint record, the retained subtrees nested in it and the frame serial) shared by every recording open around it (the *owners*: the open recordings whose lift list is the one the element joined). A replay of an owner puts the cell's record at the live ordinal. A cell whose paint is missing or from another frame counts as a missing hole.

Both bracket what they paint with `RetainedPhase::LiftedBegin(rel)` and `LiftedEnd` for each owner's hook, and a replay gives the owner `LiftedPaint(rel)` where the context paints the element (after the owner's `Paint` for `z >= 0`, before it for `z < 0`), which is how gpuix's tape plays the writes the context made for it. A pass that only lays out gives a hole `LayoutPaint` and the owner `Layout`. A recording that holds holes or fragments is never moved by a scroller (`NonRelocatable::Hole`). An ambient lift that is neither is `NonReusable::Lift`.

An `absolute` box whose containing block is inside the subtree used to make every such subtree unstable, because the engine rewrites an absolute box against its containing block after layout. The engine now counts it unstable only when the containing block lies outside the retained subtree (`escapes_subtree`), and gpuix's eligibility is the other half: a root holding an `absolute` box whose containing block is above the root is not eligible.

### Moving a recording: translated replay

A scroll moves the content of a scroller by a vector, and a recording of the content can be put there instead of built again. `retained(..).relocatable(overscan)` asks for that.

- **Recording.** The subtree is prepainted and painted into a region of its own, the content mask around it inflated by `overscan` on each side, into a recorder scene that keeps the operations. It emits them into the frame through `Scene::replay_translated(ops, Δ, clip)`, and a build emits with Δ = 0 through the same path, so the verifier compares like with like. Culling runs against the region, so what the overscan holds is recorded but not painted before it shows. A subtree that is culled where it stands is recorded under the empty mask with no region, and can be put anywhere that is culled too.
- **Δ is exact.** It is the difference of the scroll offsets as they are painted at, snapped to device pixels; it is not what `scrollTop` reads. `Window::layout_bounds` and `layout_unsnapped_bounds` both add the snapped offset, so the bounds an owner keeps are the ones a build reports under a fractional scroll. An atomic inline's origin is an element offset too, rounded half up to a device pixel; its fraction is kept beside the offset (`element_origin_phase`) and only `layout_unsnapped_bounds` adds it back, so a move by whole device pixels keeps it, as it keeps the phase in the key. The rounding of painted geometry (box edges, underline origins, glyph subpixel quantisation, emoji and svg sprites) is translation-covariant: a shift by whole device pixels moves the result by that shift, with the bytes of positive results unchanged. That is exact only at a scale factor that is a power of two (1, 2, 4), where a position shifted by whole device pixels is exactly the shifted position, so any other scale factor does not relocate.
- **When it is legal** (`ReplayKey::relocation`): every field of the replay key is equal but the origin, the content mask, the exact clips and the positioned frame (the unsnapped size and the phase are equal: a move by whole device pixels keeps the phase, and with it the side every edge inside snaps to); the origin differs by exactly Δ; the engine's bounds before the offset and the `layout_generation` are the recorded ones; the region shifted by Δ still covers the clip around the subtree now; and each rounded clip inside the subtree meets the clip around it as it did (`translated(Δ).intersection_is_exact(ambient)`), because painting cuts a rounded corner that a clip crosses in a way a shifted replay does not. The misses are counted as `relocationMisses` (`Place`, `Region`, `Clip`). A rounded row that the edge of its scroller cuts is built.
- **What moves by value**: scene operations (sprites also move their `translation`), hitboxes, accessibility node bounds, and the gpuix tape's geometry writes (each entry is the effect and the offset it was made at). **What moves through a handle**: a `TextLayout`'s bounds and a `ScrollHandle`'s bounds, which a recording registers with `Window::note_relocated(apply, describe)`; `apply` sets the handle to its recorded value plus Δ, so a replay is idempotent. **What has no geometry** and is put back as it is: cursors, tab stops, dispatch, focus, line layouts and element states.
- **Never moved** (`NonRelocatable`): a scale factor that is not a power of two, a hole, `position: sticky`, an escape to a containing block outside the subtree, a position kept past the frame (`Window::note_positioned_capture()`: a drag, a keyboard click, a tooltip's bounds test; a `Canvas` says `keeps_no_position()` when its closures keep none), an input handler, a scrolled scroller inside, a root inside a transform that is more than a translation (the ambient transform is in the replay key, so such a root is built when it differs and is not moved), a listener that reports to its owner when the owner does not take the translation (`on_replay_translated`), and a rounded clip cut by the clip around. Deferred draws and virtual lists are not reusable at all. Two relocatable subtrees do not nest: the inner one is built every frame that builds the outer.

A box that an animation moves by a fraction of a pixel is built and not replayed, since a replay moves by whole device pixels (#65, [layout.md](layout.md#the-164-px-unit)). The tail of a spring moves boxes until it rests, so a page that retargets springs from `onLayout` sends more layout events than before #65. This is open, not addressed.

How the transform stack interacts with replay is in [paint-and-transforms.md](paint-and-transforms.md#retention-and-patched-ticks).

### What a replay costs

A replay is a step per root, not per element inside it. In gpui, replaying an unchanged root costs 3.4 µs plus 7 ns for each element in it, where it was 14.8 µs plus 0.15 µs per element (`state_carry_frame_time`: roots of 100 elements with a nested root per 10, clipped). Five changes, each measured on its own page (50 roots of 100 or of 400 elements), got there:

- **Roots by pointer.** A recording names the retained roots inside it by the id their element state is stored under (`RetainedState` keeps the id of the first map the state was stored with), so a replay finds them by pointer and not by comparing ids element by element (14.8 to 10.6 µs a root at 100). Ids that missed the pointer compare had fallen to `SmolStr::eq`, about 30% of the profile.
- **One map of states.** Element states are one map of the window that outlives the frames (`Window::element_states`), each stamped with the pass that last accessed it and with the retained root that was being built, prepainted or painted when it did (`StateOwner`) (10.6 to 6.5 µs at 100). A frame that is presented drops the states that it neither accessed nor kept (`release_unused_element_states`): a subtree that layout skips marks its owner kept, so a replay copies none of the states of its range to the frame. A state lives as long as its root is replayed or built; a root that leaves the page loses its states and starts over when it comes back. Ranges copied by anything else (a cached view, a deferred draw) stamp the states they name.
- **Placement by origin.** A recording has an `Origin` that says where its prepaint and its paint started and in which frame. The origin of a recording made or replayed inside another is placed within the outer one's, relative to where that one started, so a replay places its own origin and the roots inside it follow (`PrepaintRecord::range_now`, `Recorded::is_placed_in`): a replay does not visit the roots inside the root it replays (21.9 to 12.8 µs at 400). A rolled-back prepaint (`Window::transact`) drops the placement of the recordings it made.
- **Dispatch nodes copied in one pass** (6.3 to 5.4 µs at 100). `DispatchTree::reuse_subtree` copies the nodes of a range in one pass, the parent of each worked out from its place in the range. A range with a node that names a view is still copied a node at a time.
- **Dispatch nodes are small and shared** (12.0 to 5.6 µs at 400). Listeners and key context are `NodeExtras` behind an `Rc`, taken by pointer from the tree of the frame before (a node that gets a listener or a context after it was shared copies them first).

Two smaller costs that the same profile showed: `Frame::finish` moving element states, and the per-window bounds map that every painted or replayed element writes into, which hashes with FxHasher instead of SipHash (0.5 to 1.5 ms at 7000 replayed elements). `collect_garbage` walks the layout trees only when a node was made, left a tree or the roots changed since the last collection.

## Layer 3: gpuix decides what to skip

`retain.rs` and `tape.rs`.

### The entry key

`Retain::enter` runs for every element before it is built. The key has three parts:

- the element's `subtree_revision`, which every mutation to the element or anything below it moves (an event listener change too);
- a fingerprint of `Inherited` hashed by content, not by pointer: group, container and highlight scopes are fresh `Arc`s every frame. A field added to `Inherited` must be added to `inherited_fingerprint`; a size test fails when one is missing;
- the shape: the element's id, its index among its siblings, its parent's shape, whether it is a stacking root or item, an atomic-inline root, the `display: contents` elements it was spliced out of and the style each had when it last changed (`contents::overlay_key`), an anchor scope, the `text-box-trim` an ancestor hands down, the size layout last gave it if it is a query container or has `@container` blocks (`ContainerStore::key`; the sizes of the containers above it are in the `Inherited` fingerprint), plus words the build adds as it reads things the tree cannot show: the accessibility links an element reads (`links_key`: `<label>`, `aria-*` references), the length of the click path, a custom element's `retain_state`.

**What the key cannot see is volatile**: pointer and focus state, scroll offsets, measurements, the selection, running motion, transitions and animations. The build notes what it read (`pointer`, `focus`, `scroll`, `measurement`, `text`, `dynamic`) into a `Vol` kept with the key in the element's *record*. The record also holds the subtree's size, its lift, its culling reach and its build effects. A skipped element does not enter its children, so what a build folds up from them (epoch, token, holes and whether it lifts) is taken from its record: without `Record::lifts` a root with a skipped child that lifts would be recorded as lifting nothing, be relocatable the frame after and be built inline then.

### Frame deltas and the spine

`find_deltas` compares the sources with the frame before: hover and active chains (and the target each moved to or from, for `:has()`), the focus-within chain, the `:focus-visible` element and the keyboard flag, the dim path, scroll offsets, custom `retain_state` values, motions running now or in the frame before, elements whose measurement or links changed, and the elements whose last build was dynamic. Those elements and all their ancestors are the *spine*: the upward closure of the changed ids and `unbuilt` (below). A root is skipped when it is off the spine, so an element on it whose parent is not would be replayed with it frozen; under verify `find_deltas` counts each one as a mismatch (`[retain] element N is on the spine and its parent M is not`). A new selection identity, or a frame with nothing known from before, sets `all`: nothing is skipped.

`find_deltas` is `compute_deltas`, which changes nothing and has peeks of what a frame takes (`Measurements::peek_changed`, `links::peek_changed`, `peek_commanded`), and `commit_deltas`, so a tick can ask what would have changed without taking it. An element whose animations only change what it paints and that the last frame walked culled is *dormant* and is left out of `changed` ([patch-frames.md](patch-frames.md#dormant-animations-63)).

A pass that only lays out is a frame to `find_deltas` (it updates `seen` and drains `Measurements::take_changed` and `links::take_changed`) but does not visit an element under `display: none`. So the spine is not forgotten when it is not built: `Retain::unbuilt` carries every spine element a pass did not build (it is removed in `finish_session` when the element is built, and with the destroyed elements in `prune`), and the next `find_deltas` walks up from it with the changed ids. It carries only elements a build can enter. A `display: contents` element (but its tree's root, which builds as a host) is spliced out of its parent, an `<svg>` paints what is below it as one image, and an inline span or text is a part of its container's paragraph (`RetainedElement::is_built_by_ancestor`): no build enters them, so they would stay in `unbuilt` for good and keep their ancestors on the spine. The spine is a set of its own, closed under parent, which is what makes the walk right in stopping at the first id already in it; seeding it with `unbuilt` as it stands would not be, since the built ancestors of an unbuilt element have left it.

A new selection reaches an element the same way: a `Record` keeps the selection it was built under and `skippable` compares it, since `all` is a property of the pass and not of the elements it visited.

### When a frame skips an element

All of these:

1. planning is on and it is not `all`;
2. the element has a layout node of its own (`boxed`: table rows, row groups and columns do not) and is built in a plain context (no baseline probe, inline line, document host list, containing-height scope, atomic extraction); the root of a subtree that gpui asked to have built late is built, though elements below it may be skipped;
3. nothing is excluded ([below](#eligibility-and-exclusions)), it is not on the spine and no scroller above it moved, except as a root that gpui moves with the scroller ([below](#roots-under-a-scroller-move-with-it));
4. its last build left a record, the record says eligible, it is a root worth the replay, and the key it was entered with is equal to the key the record was entered with.

**A root worth the replay (`MIN_ROOT`).** Replaying a root has a cost of its own, about what building a plain box costs: 2000 childless boxes in a scroller that nothing changed took 1.24 to 1.31 times as long with retention on as off when every box was a root. So a root is skipped when it has at least `MIN_ROOT` (2) elements, or when it is one element that is costly to build: it carries text, or is an image, an `<svg>`, a `<text>` or a custom element. With the rule a scroller of plain, rounded or shadowed boxes costs 1.00 times what it costs with retention off, and a scroller of texts 0.4. Measured on the pages of the fluid-functionalism demo, the texts and icons that are roots of one element are 0.5 to 0.8 ms of the 3 to 6 ms an idle frame costs, which is why the exception exists. A verified run takes every root, and `GPUIX_RETAIN_MIN_ROOT=1` takes every box, to measure.

A skipped element becomes `retained_root`: `gpui::retained(token, build_subtree)` with an `on_replay` hook. The token is a hash of the entry key, the record's epoch and the tokens of the holes in it, so rebuilding an element on its own changes its token and the recording of that build is not mistaken for another. Closing over a `FrameEnv` (inherited state, document path, clock, focus styles, entry key) lets `build_subtree` build the element later, if gpui cannot replay it, exactly as the frame would have.

A subtree that only a running paint-only animation changes can also be built whole as a *unit* while the rest of the page is left alone; that is [patch-frames.md](patch-frames.md#units).

### Eligibility and exclusions

After a build, an element is eligible when nothing in it is excluded and no `absolute` box in it has its containing block above it (`Below::escapes` is folded up from each absolute box and cleared by a positioned box). An element that is itself `absolute` is not eligible for a skip, since gpui rewrites its layout against a block outside it, but it may be a unit. What it lifts into the stacking context around it does not matter: a lifted element that is a root of its own is a hole in the recording of the root around it (`retained(..).lift_z(z)`, never `relocatable`) and any other is a fragment of it ([Holes and fragments](#holes-and-fragments)). Container queries do not exclude: the size a container was last laid out at is in its key, the sizes of the containers above in the fingerprint, and the axes that a skipped reader read are a build effect. The children spliced out of a `display: contents` element are retained, with the wrappers' style in their shape; a subtree gpui builds late under such a root keeps the style the wrappers hand down.

Excluded (`Exclusion`), so the element and every box around it is built:

- a paint reporter (`onScroll` on an element that scrolls; `onLayout`, `ResizeObserver` and `IntersectionObserver` are not one, see [observers-and-content-visibility.md](observers-and-content-visibility.md));
- a `highlight`;
- a custom element that has no `retain_state` (`<code>`, `<markdown>` and `<diff>` hold scrollers whose offset only gpui knows; an `<svg>` whose raster is not settled). `<input>` and `<textarea>` have one, an epoch of everything but their blink, and are retained ([patch-frames.md](patch-frames.md#an-input-is-retained-and-its-caret-is-a-tick-74));
- a virtual list and the rows it builds;
- a table with collapsed borders (`Exclusion::CollapsedTable`, below);
- a `display: contents` element built as an element of its own; `<anchored>`; `position: sticky` or `fixed`;
- a width keyword or a natural width (`UnstableLayout`);
- an `overflow: scroll` or `auto` element. A scroller is a boundary: the box around it is built too, but its children are the roots;
- a `layout` (FLIP) element; an atomic inline.

A running `<svg>` is rasterised in the paint whatever its size, so it is a unit ([patch-frames.md](patch-frames.md#an-svg-is-culled-like-a-box)); `Exclusion::Svg` is left for the one frame before a paint that would start a raster job.

Not excluded, because their scroll offset, links and state are in the key or the record, or the tape republishes what they report: `overflow: hidden`, labels, aria links, settled `<img>` and `<svg>`, `onLayout` and observers, container queries, and lifted and `absolute` boxes that a root contains.

**Tables with collapsed borders.** A table with collapsed borders resolves the winners and widths of its edges when it is built, from the borders its parts were painted with in the last build (`ResolvedStyles::borders`), paints the colours its parts are built with in this frame (`settle_collapsed_borders`), and asks for another pass only when a width or a style moved (#68). Every part is `Exclusion::CollapsedTable` (the parts are under `Inherited::table.collapse`), so neither the parts nor the table around them are ever replayed, whatever transitions or animates in a cell. The table itself is the one unit such a page has: paint-only motion on any part of it makes the table the unit (`Retain::collapsed_table`, `Retain::unit_facts`: the motion of the table and of what is in it, and no exclusion below it but the parts), since it is the table that paints their borders, and a tick builds the table alone. The parts are never units, and a part excluded for another reason (`Retain::exclude` replaces `CollapsedTable` by it) or one that lays out keeps the table from being one. A patched tick still lays the unit out onto its own nodes (one pass, no node made).

An anonymous table has no element and is built by `build_plan`, never through `build_element_in`, so `build_plan` marks the element that builds it as holding the table, and that element is the unit of a paint-only motion on any part, under the rules of every unit (at most 256 elements, no exclusion below it but the parts, nothing that scrolls or lifts out of it). A container too big to be one stays whole and is counted `TooLarge`. Without this, a colour transition on a cell drew every tick whole (`Outside`). The anonymous table's borders are keyed `BoxKey::Anonymous { container, ordinal }` and resolved from the children of the container, so a build of the container alone resolves the same borders. Per tick: 0 patched frames, 458 nodes reused and 472 painted before; 1 unit, 51 nodes reused and 64 painted after.

### What records, and when

Every build of an element writes a new record (key, `Vol`, size, lift, reach, build effects) with the epoch advanced, except a build gpui asked for late in the frame that already built it, which keeps the epoch (the epoch of an element with no record is the frame serial, so a build never takes a token gpui still holds a recording of). gpui records the prepaint and paint of a root whenever it builds it, for layout or late, so a root that failed to replay records again with that build.

**A root built whole is recorded by that frame** (`Retain::exit` returns a `Recording`). `exit` returns the token, lift, culling reach and overscan for a build that `skippable` takes the record of in the frame after (planned, plain, boxed, not prekeyed, outside a unit and a deferred build, no exclusion, not escaped, worth its size, not moved by a scroller nor too big for one, and no motion running in or below it), and `build_element_in` wraps the element it built as `gpui::retained(id, token, built)` (`recorded_root`). So the frame that builds a page or a changed root is the one gpui records, and the frame after replays. If `enter` only wrapped skipped elements, the first build of a page would leave a record that said "skippable under token T", the next frame would skip under T, gpui would have no recording under it and build the root again late, in prepaint, to record it, and only the frame after that would replay. The two passes over the nodes in `finish_session` are `Retain::settle`, run by `exit`, so the epoch, token, holes, escapes, lifts, size and digest of a node exist when its build ends.

Three things a first recording meets that a later one does not: a background image still loading is asked of the loader during the build, outside the recording (a build that left one loading is not recorded); a scroller has no `Scroller` before its first layout, so a provisional one takes the style's axes and the window for a viewport, or its first frame would record its roots where they stand; and zed's `Recorded::is_loose` (a recording made in an overscan no region was made for, as under a rounded clip) is compared as loose by the verifier, with `Window::is_recording_any_relocation` keeping a root nested in the reference build of a verified frame from planning a relocation of its own.

A relocatable root that was built because the move was not legal records again in a region around where it is now. A frame in a back-off stretch records nothing and leaves the records as they were. Records go when the element is destroyed (`prune`) or when planning is turned off or on.

The cost is a first draw that is about 15% dearer: the entries within a scroller's overscan are prepainted and painted to be recorded in the frame that builds them (r11b, 190 entries, 1902 elements in a scroller: 30 to 35 ms, with 735 prepainted and 2587 painted primitives against 303 and 1616), where the second draw used to pay for it. Draw 2 is 14.3 to 1.8 ms (1902 to 12 elements built, 190 subtree builds to 0, 190 hits and one lazy build at the edge of the overscan band), and a text change is 13.9 to 2.1 ms with the frame after replaying it; the culled variant is 2.2 to 1.1 ms.

### The effect tape

A replay puts the scene back but runs none of the listeners and canvases that write gpuix's own per-frame registries, and gpuix resets them at the top of every painted frame. Every write that a retained subtree has to repeat goes through `tape::write` (or `tape::write_in_layout`, for registries a layout-only pass writes too): bounds, selection and start regions, the painted-text log, inline hit paragraphs, `Measurements::by_element`. A `Taped` element around the root of the subtree records the writes of its prepaint and paint. An entry is the write and the offset it was made at, so a moved replay (`on_replay_translated`) shifts the geometry it holds (`ElementBounds::translated`, the start regions).

A replay does not write the tape into the registries. It tells them which lists of writes it puts back (`tape::publish` with a `Live`: the list, how far it is moved, whether a pass that only lays out makes it), and they read it when something asks, so a replay costs the same whatever the subtree holds.

- A tape is an `Rc<Entries>` per phase, a list that is never changed once recorded (a new recording starts a list of its own, so a replay that was not read yet keeps what the subtree wrote then), with writes of a kind (`tape::Kind`) and nested lists by reference. A write belongs to the innermost recording, the subtree around it holds the finished list as `Entry::Nested(list, by)`, and a subtree replayed inside a recorded one is nested the same way, so recording a replay is O(1) too.
- The registries that keep an order (`text::paint::Frame::{registry, start_regions, painted}`, `inline_hit::PARAGRAPHS`) are `tape::Layered`: a write is a slot and a replay is a slot, and the first read after them expands the replays in place, in the order they came, and keeps the result until the next write.
- The painted boxes (`automation::BOUNDS`) answer by element: a write notes the serial of the list it was recorded into (`tape::owner`), and a box is the one the frame wrote or, from `Stored::recorded`, the one a live list recorded moved by its delta at read. Marking a replay live is one insert whatever is nested in it: a list notes the list it was last nested in and by how much (`tape::PARENTS`), and a box is looked up by going up from the list it was recorded into until a live replay is found (`tape::delta_of`). A frame reset bumps a counter and clears the live set instead of walking the map, and stale boxes are purged when the map doubles. There is no eager apply of a relocated replay: the delta is applied at read.
- `Measurements::by_element` is not reset by a frame, so a replay leaves it as the build wrote it (`Kind::Measured` stays on the tape for `same_writes` and the verified frame).

What the stacking context paints for an element the root handed to it is outside the paint of the root: gpui tells the hook `LiftedBegin(rel)` and `LiftedEnd` around the paint of the frame that builds it, and the tape keeps those writes in a slice by `rel` (`Phase::Lifted`); a replay of the root gets `LiftedPaint(rel)` where the context paints the element and plays the slice. A hole plays its own tape (`Paint`, or `LayoutPaint` in a pass that only lays out), and the root around it plays the writes of its prepaint. A pass that only lays out plays the in-layout writes of the slices too.

The one write that depends on what is outside the root is the parent content box in the measurement of the root itself (a percentage that meets `auto` in motion resolves against it, and a sibling or the parent's padding changes it with the root's box and key as they were). The hook republishes it from the current layout (`Window::replayed_root`, `Measurements::republish_parent_content`) before it publishes the tape, and a change marks the root's measurement changed, so the next frame builds it as it would have.

### Build effects

Effects of *building*, not of painting, are `BuildEffect`s kept with the record and applied when a frame skips the element: marking elements as measured (`Measurements::wrapped`), asking for background images (`background_images::REQUESTS`) and the axes of a query container that a style read (`ContainerRead`). The last is what keeps a container honest: after layout it compares its content box with the one the build assumed, for the axes something read, and asks for another pass if they differ; a skipped reader has to say what it read, on the container's scope of this frame, or a container that is resized with the reader's box unchanged would not ask. A reader inside the container's own subtree has no scope to say it to, and does not need to, since its layout did not change.

### Scroll back-off

Recording a frame costs about 5% of building it. A page whose frames find nothing a later frame could skip (a scroller whose rows move every frame, boxes too small to be a subtree) only pays that. After a recorded frame that found no skippable element, the next 1, 2, 4 and 8 frames (capped at 8) are built unrecorded, and one frame in nine looks again. Recording starts again at once when the scrollers stop moving or the element count changes. `GPUIX_RETAIN_VERIFY` turns the back-off off. A scroller whose roots gpui can put where the scroll moved them (below) is not a page with nothing to skip, and is recorded every frame. So is a page whose frame holds an element that a tick can patch (something runs on it that only paints, and nothing refuses it as a [unit](patch-frames.md#units)): the records a unit is chosen from go with a forgotten frame, and an idle `layout` box beside a loop (an excluded box, so that nothing around it was a root worth skipping) was `NoFrame` at every tick but one in nine for that reason (#117).

### Roots under a scroller move with it

A root of at most two viewports (per axis, by its last painted box) inside a scroller, or a clip that script scrolled, is handed to gpui as `retained(..).relocatable(overscan)`, with the overscan half a viewport of the scroller (`Scroller` in `retain.rs`); what gpui does with it is in [Moving a recording](#moving-a-recording-translated-replay). A box that is bigger than two viewports is built and its children are the roots, so a scroller's single content wrapper is not one giant root. The scroller that moved is on the spine, but what is below it is not built when its roots are moved: `moved` rows are a loss only when gpui builds them anyway. `Retain::relocation` compares `lazy_builds` with `relocations` per frame and stops planning on moves for 1, 2, 4 and 8 frames when the builds are four times the moves.

Measured, test renderer, release, one scroll per frame, 200 cards in a 400x300 scroller: 5.1 ms with retention off, 1.0 ms with it on at 3 px a frame (0.19 of the frame), 0.35 of the frame at 400 px and 0.5 at 2000 px. The `clipped` benchmark (`test:perf:clipped`, a scroller of cards that really moves 3 px a frame, #101) measures 1.2 ms against 13 ms (0.09), with no accessibility tree 1.18 against 13.5.

### The colour read set

The key of a root held all of `Inherited`, colours included, so a `color` on the page keyed every root below it as changed though the cards set their own text colour, and past that key gpui's `ReplayKey` holds the whole text style around a root. Colour is now a change only to the roots that read it (`ColorContext`, `color::reads`):

- `ColorContext` keeps where its `current` was declared, `Retain::enter` / `exit` open and close a read frame per built element, and `Record::reads_colors` says whether something in it read a place above it.
- A `color` an element declares cuts the read for what is below it. `currentColor` and `color-mix(.. currentColor ..)` (also in a state, and in a parse-cache hit of a text that names it), text, custom elements, `<svg>` markup, `motion` and bound values read the colour they inherit. The default colour of an outline and a text decoration is lazy: it is not a read for every element.
- `Key::colors` is compared by `skippable`, `Cause::Inherited` and the token only for a record that read it.
- A root that reads none is wrapped `ignoring_text_color` (zed `Retained::ignoring_text_color`): gpui lays it out, prepaints, paints, records and replays it under a colour of its own (`FIXED_TEXT_COLOR`, magenta, so a reader that was missed shows). It is not used for a root that lifts.

On r11b with a `color` changed on the page: 1902 elements built to 2, 17 ms to 1.6 ms (recording every root on first build alone made these frames 60% dearer, since each now recorded every root).

**Only colour and custom properties have a read set.** A change to the lengths, font state, direction, groups, containers or scheme still keys every root below it as changed. A custom property is in the key of a record only when something built in the subtree read one (`Record::reads_custom`: a transform with a `var()`, a `style()` query on the parent), so a `--p` set above a page of rows builds the rows that read it and not the others.

## Invalidation rules

The contract between a build and the skipping. Every row is a way for the frame to stay correct.

### What changes the entry key, so the element is built

- A mutation to the element or anything below it (`subtree_revision`), including attributes, styles, text, children and event listeners.
- A change in anything `Inherited` carries that a descendant reads: colours (only for a root that read a colour above it, `Record::reads_colors`), lengths and font state, selectability, `inert`, `pointer-events: none`, table settings, container and group scopes with their hover, active and focus-within state, the highlight declaration, `appRegion`, `frameless`, the underline offset, the `text-box-trim` handed down, `direction` with the `text-align` it resolves (and what the parent's text style already resolved it to), and the custom properties (`--x`) that `style()` queries read, hashed in sorted order.
- A Web Animations command on the element since the frame before (`Sources::commanded`): `play()` on a transition that had finished, a seek, a rate change. Nothing in the last build says that a run which had ended can move again.
- A change in the element's shape: its place among its siblings, its parent's shape, being a stacking root or item, an accessibility link, the length of the click path, the size of a query container, the `display: contents` wrappers it was spliced out of, the `retain_state` of a custom element.

### Fields the parity test pins, and why each is covered

Pinned by `retained-parity.test.tsx`, run with and without `GPUIX_RETAIN_VERIFY=1`:

- An own, non-inherited style field (`mask-border-*`, the `flex-basis` keywords, `direction` on the element, `:disabled` and the other state styles' bodies, custom properties on a state style) is in the revision and `StyleDesc` equality. A state style that carries `--x` reaches the children through `Inherited::custom`, and a hover on the element is a pointer delta.
- `:disabled`, `:enabled`, `:has(:disabled)` and `:has(:enabled)` read the tree (`RetainedElement::control_disabled`, `ControlStyles`, which is worked out once per frame). A flip is a prop mutation, which moves the revision of the control and of every ancestor, which are exactly the elements `:has()` can name, so there is no delta. Their `PointerStates` flags have bits in `pointer_bits`, which only attributes a rebuild in the counters.
- `Resolved` (`resolved_style.rs`) is written on each build and kept by a replay, which is right because nothing it is made from changed; a destroyed element forgets its record, and a `display: none` build does too. It is made from the focus-folded style, and the focus variants are on the spine (`:focus-within`, `:focus-visible`, the keyboard flag). The gpui element must say the same lift as the record: `apply_focus_variants` decides it again from the folded style (`stacking::apply_folded_lift`), because a variant applied on its own could not see the base `position` or the item flag. `Retain::digest` of a host includes the `Resolved` record, so a verified build sees a stale one.
- `display: contents` wrappers hand a container declaration and custom properties to the children spliced out of them (`contents_levels`); the wrapper's style serial is in the child's `contents_key`, so a change in the wrapper is a change in the key of each spliced child. A wrapper that toggles `display: contents` re-tracks the focusable elements below it (`track_focusable_subtree`).
- Text: `strut_font`, `nowrap`, `TextRun.group`, `UnderlineStyle.decorator` and `.fragment`, `InlineBox.wraps` and `.key` are in the gpui measure key through `TextLayoutOptions`, `TextRun` and `InlineBox`, which hash all their fields, and `LineHeight::Normal` is the line height measured from the text style's font. The sampled insets of a span that moves (`InsetSpec::text_key`) are in it, and a span or element gesture is a pointer delta, with a motion's controller named through `gesture_controller`. The digest of a verified paragraph includes the insets and the decorations. The measure key of a text holds each run's length, font, font size, line height and whether the decoration changes there, not every field of the run ([text.md](text.md#text-measure-keys-and-the-shape-store)).

### What puts an element on the spine

So it and its ancestors are built, and its other children may still be skipped.

- Entering or leaving hover, active, focus-within, focus-visible or the dim path, including the targets `:has(:hover)` reads; the keyboard flag changing.
- A scroll offset changing (the scroller and its ancestors; what is below the scroller is *moved*, see [above](#roots-under-a-scroller-move-with-it)), a scroller appearing or disappearing.
- A custom element's `retain_state` changing; a measurement it reads (`motion/measure.rs`) changing; its accessibility links changing.
- A motion, transition, CSS animation, motion value or inline-span motion running now, and any element whose last build found one running. A dormant element is not one: it sleeps (`Retain::dormant`) until a build reaches it or what `Dormancy::holds` reads that no build tells moves (`Sources::verdicts`: the keyframes registry's epoch, the tree's count of re-parentings, the viewport), and a root that shows again is built with the clock of that frame, since a culled recording cannot replay at a place that is not culled (`ReplayKey::culling`).
- Anything above that a pass which only lays out saw but did not build and a build can enter (`Retain::unbuilt`), the parents of everything on the spine whether or not a build enters them, and a hole that changed or animates, which puts the root around it on the spine through the token.

A tick that patches takes only the time-driven sources of this list (running animations, motion, motion that ended). A delta of any other source (the pointer, focus, a scroll, a custom `retain_state`, a measurement, a link, a command) declines the patch and the tick is drawn whole ([patch-frames.md](patch-frames.md#units)).

### What makes the whole frame build everything

`all`, or no planning:

- The first frame, a new text selection identity, and any call that forgets the records (`setRetainPlanning`, `setRetainEnabled`, `setRetainLift`, `GPUIX_RETAIN_OFF`).
- A draw pass above 1, `Window::refresh` (a rem size change calls it), a measuring pass that is not a layout-only one (`paint_if_dirty` inside input dispatch), the inspector.
- A frame in a back-off stretch, which also records nothing.

**The refresh rule.** `Window::refresh` is for state the planner cannot see, which a key or a delta would have to carry and none does: `set_live_image` and `register_font` (the bytes behind an id), background images (`background_images.rs`), the rem size, and a screenshot. Everything JS does to the tree is seen: a mutation moves a `subtree_revision`, a scroll offset and a running motion are frame deltas. So a commit from JS, a motion write, a programmatic scroll and a read of the automation tree dirty the window with `invalidate_after_commit` (`renderer.rs`), which both renderers use and which calls `cx.notify` and not `refresh`. Don't put `Window::refresh` back on that path: with a refresh, one hover write over 3000 nodes built 5403 elements (about 222 ms under load) instead of 5, and a hover sweep that committed from JS built 616 a frame instead of 7.5. The test renderer goes through the same function, so a `refresh` put back in it shows up in `live-invalidation.test.tsx`. A write that only drives paint-only properties notifies with `notify_paint_only` ([forced-reads.md](forced-reads.md#writes-that-do-not-make-the-next-read-run-a-pass-47)).

**Call sites that still refresh, with no reason found that they must.** They were left alone because nothing showed them safe without it, and each is a place to try `notify` and run the retained suites with `GPUIX_RETAIN_VERIFY=1`: focus changes (`focus.rs` `change_focus`, and the focus, blur and `focusNextWithin` commands in `run_ui_commands`), the window title, the setup of window key events and of selection-change events, the `scrollIntoView` family (`scrollIntoView`, `scrollByKey` and `scroll_focus_into_view`), `custom_elements/text_selection.rs` (`set_text_selection`), the selection listeners in `text/paint.rs`, `custom_elements/svg_paint.rs` (a landed raster), the test helpers in `retain.rs`, and the wasm pointer lock. Each costs a full build of the frame that follows it.

### What makes a skipped subtree build late, in prepaint

- Its `ReplayKey` differs: the subtree moved by something other than a scroll that a relocatable root is put after, was laid out at another unsnapped size or with its origin at another sub-pixel phase (its snapped box can be the same), was clipped differently, or the opacity, text style, scale, viewport or the number of stacking contexts around it changed.
- Its root's `layout_generation` differs, which includes percentage padding resolved against a parent that was resized.
- An entity it read was notified, or an asset it drew was still loading.
- A hole or fragment in it was not painted in the rendered frame.

What makes a recording non-reusable (`NonReusable`) or not movable (`NonRelocatable`) is in [Layer 2](#layer-2-gpuiretained). Those subtrees are built every frame they are asked for, or where they are moved to, and `getRetainedCounters().nonReusable`, `.nonRelocatable` and `.relocationMisses` say why.

### Adding a build input

- State on the element goes through the revision.
- State of the window or the app that the build reads either enters `Inherited` and its fingerprint, becomes a delta in `find_deltas` and a note in the build, or excludes the element.
- A registry the paint writes goes through the tape, with its geometry through the offset.
- A new custom element implements `CustomElement::retain_state`: `None` excludes it, `Some(hash)` of everything it keeps for itself puts that hash in the shape and in the deltas.
- A key that does not hash every input of a measure function makes a layout node keep a size it should not.
- A new gpui element that keeps a position past the frame says so (`note_positioned_capture`, `keeps_no_position`, `note_relocated`), and a new painted geometry rounds translation-covariantly.

## Culling clipped content (#38)

A browser keeps style and layout for content a clip hides and skips its paint. The fork does the same: the build and the layout of a clipped box stay, because JS can read its box, and the prepaint and paint go. A clipped box costs about two thirds of a visible one (culling alone takes a clipped box from 4.9 µs, the same as a visible one, to 2.1 against 3.1 for a visible one); a page that wants better culls in JS (README, "Pannable surfaces must cull"). Skipping build and layout under a fully clipped node stays undone, because the descendants' bounds are readable from JS (`getBoundingClientRect`, scroll metrics, observers); build and layout, not paint, were most of the cost of a hidden menu, and the retained replay is what makes the idle frame cheap.

**gpui decides, from the real layout.** A child that says `.cullable(outset)` (`Element::cull_outset`) is culled when its border box, and the boxes of what overflows it, inflated by `outset` and a pixel, do not touch `content_mask().bounds`. The overflow is the engine's `painted_extent` (memoised per node; descendants of a node that clips an axis contribute nothing on that axis). A subtree with a `fixed` or detached node, or an `absolute` node whose containing block is above it, has no extent and is not culled. Never culled either: a child that is lifted, escapes its clip, is sticky, or has an image cache or a prepaint order; any child while a hitbox captures the pointer; anything inside a culled subtree. A content mask with no width or height culls every child tested against it, the ones at its edge too. A clip that cuts a mask away leaves an empty rectangle at the edge it met (`Bounds::intersect` clamps), and the pixel of slack made content right below an `overflow: hidden` clip touch it. Content clipped away that way is culled, and its animations are dormant (0 painted frames, [patch-frames.md](patch-frames.md#skipped-content-97)).

**gpuix says how far a subtree inks** (`layout/cull.rs`), bottom-up as `build_element` returns: the shadows, outlines, blur and drop shadow of the style and of every state style, and the font size of text plus how far its underlines are put from the baseline and how thick they are (`Inherited::underline_reach`: the largest `text-underline-offset` of the elements above and of the spans of a paragraph). Absolute, fixed and sticky positions, custom elements (an `<svg>` that is not positioned is the exception: its ink is its box), virtual lists, `layout` (FLIP) wrappers, collapsed tables, paragraphs with an atomic or painted inline box, and anything with a cursor, region or focus anchor `block()` the subtree: no ancestor of it is cullable. A box that clips both axes drops what is built below it from the reach. A retained root forwards the reach recorded for its subtree.

**What a culled subtree still does** (`prepaint_culled`, `paint_culled`, under an empty content mask):

- `on_painted`, `on_painted_layout`, `on_prepainted_layout` and `on_children_prepainted` listeners run, so bounds, observers and `onLayout` are as for a painted box.
- The scroll metrics of the scrollers in it and the prepaint overlay of a classic scrollbar (so a scroller outside the clip takes its gutter), tab stops and the focus path, the layout-pass publication, and the positioned frame an escaping descendant is placed by.
- The accessibility nodes, with bounds and relations. The accessibility tree is not a reason to stop culling: macOS turns it on for the first query from any assistive client and never turns it off.
- `StyledText` only sets its bounds; the canvas under a text runs in full, which is where the selection registry, the find ranges, the inline-box bounds and the painted-text log are written, so selecting, searching and `getPaintedText()` see clipped text.

It has no hitbox, listener, tooltip, cursor, style from state or overlay painter, which nothing could reach outside the clip. A retained recording of a culled subtree is made under the empty mask and moves to any place that is culled too.

**The walk of a culled subtree stays.** The walk serves the bounds registry (`getBoundingClientRect`, observers, container queries, `content-visibility`), the text registries (selection, find, painted text), tab stops and the focus path, accessibility nodes and nested scroll metrics. A retained root already skips it on a hover or an idle frame (`culledPrepainted` 0 at 0, 1000 and 5000 cards); only a build that goes through it walks it (18 prepainted and 18 painted per card). Skipping it for a build too would take the walk from about 60 to 75% of a built frame (hover at 5000 cards with retention off, 584 to 232 ms), but the reads and the tab order would break; it needs a lazy or movable registry. Not done.

## What a hover frame costs (#46, #57, #58, #63)

A hover over a 60-row sidebar costs the sidebar, and should not cost the 200 cards beside it, whether they are below the window, in a clipped scroller or hidden behind `opacity: 0`. In a real window with the demo's pages it did: the frame grew with the page. #57 saw one animating text on 3000 nodes as a full rebuild every frame (about 28 ms), and #58 saw a hidden copy of every label cost about 19 µs a text on a rebuilt frame. #63: 200 cards clipped out of view, each with a colour loop, cost an idle tick 4.86 ms against 0.09 ms for the same page with no loops, and built 403 elements; a hover over a sidebar beside 5000 such cards took 37 to 43 ms against 0.4 to 1.5 ms with none. Chrome paints nothing for them.

**Causes, in the order of what they cost.**

- **A refresh.** A JS commit, a motion write, a programmatic scroll and a read of the automation tree called `Window::refresh`, and a refreshing frame replays nothing: one hover write over 3000 nodes built 5403 elements instead of 5. They notify now ([the refresh rule](#what-makes-the-whole-frame-build-everything)). #57 was this and not a rebuild of the page: an animating text builds 2 elements a frame, and a native keyframes animation never refreshed.
- **Walks of every descendant.** The painted extent of a subtree (what culling reads) and the scrollable overflow of a scroller were worked out from the children on every pass: a hover frame on 1000 clipped cards walked all 7000 nodes twice. Both are kept from one pass and frame to the next, stamped with the node's `layout_generation`, its absolute origin and scale, and an epoch that moves when a node becomes or stops being static, escapable or detached, and are worked out again only for a node whose layout changed. The unsnapped overflow that `scrollWidth` reads is kept the same way, in a map of its own. `getLayoutCounters()` counts the walks (`extentWalks`, `overflowWalks`, `unsnappedOverflowWalks`).
- **Replay per element and not per root.** See [What a replay costs](#what-a-replay-costs). The profile of the 5000-card page put it in ids that missed the pointer compare, the loop that moves nested roots, `Frame::finish` moving the element states, `reuse_subtree`, and gpuix's tape writing each replayed root into the registries (about 8 µs for each card nobody sees, 1.2 µs an element, with flat counters).
- **Animations in content nobody sees.** A CSS animation steps only in a build, so every culled animated card was built (2 elements) with the spine above it each tick, for a result nothing draws. That is [dormancy in patch-frames.md](patch-frames.md#dormant-animations-63).
- **Maps per element swept when something left.** The tapes and the CSS animation and transition states of a destroyed element are dropped in the frame after it was destroyed and not in every frame (`GpuixView::swept_at`; the tapes alone were 11% of the hover frame).

Three other causes are not retention and live with their topics: an `opacity: 0` subtree painted into a group that was thrown away ([paint-and-transforms.md](paint-and-transforms.md)), an inherited `color` that re-measured every text, and a line layout cache that forgot ([text.md](text.md)).

**What a hover frame skips now.** Unchanged content that is below the window, clipped or `opacity: 0` is not built, measured, shaped, styled, laid out again, prepainted or painted, and its primitives are not inserted: a page of 200 cards makes the same frame as a page of none, in every counter above and for the a11y tree on and off. What stays is the replay of recorded roots, which scales with the roots a frame replays and not with the cards behind them. Cards below the window are not reached. In a scroller each card is a retained root that is replayed (and, outside the viewport, culled). No second prepaint of a culled root exists: the parent's culled prepaint is the call that reaches the replay.

**What is left.** A hover frame still scales with the roots it replays, now at about 0.5 µs a card: 5000 cards clipped out of view cost 3.0 ms against 0.4 ms with none, with colour loops or without. A replay is a step per root (3.4 µs in gpui's bench), and gpui's `StateOwner::is_kept` is 30% of what is left. On the hover frame at 5000 cards, the replay change alone took the clipped page from 34.6 to 27 ms, the page below the window from 36.0 to 29 ms and the clipped page with colour loops from 42.9 to 37.7 ms (on the zed before the step-per-root changes); the cached tree facts and the clocks looked at only when due took the last to 32.5 ms, the zed changes to 8.5 ms, and the persistent dormant set to 3.05 ms. `compute_deltas` fell from 46% of the hover frame to 0.4%. The numbers are counts and CPU time in the test renderer; the cost in a real window (the display link, the GPU) was not measured.

## Where a frame differs from a full build

Where a frame is not what a full build of the same state draws or reports, or where the pass leaves a gap. These are not Chrome deviations; they are the places retention does not apply or does not match.

**Never retained** (a page made of these is built as before and pays the 4 to 5% of bookkeeping that the [back-off](#scroll-back-off) limits):

- `<code>`, `<markdown>` and `<diff>` until they implement `retain_state` (they no longer make a tick of a loop elsewhere whole);
- an `<svg>` for the one frame before a paint that would start a raster job (`Unsettled::Stale`; one that waits for its raster is retained);
- virtual lists and their rows (a chat page tops out near 60% skipped);
- scrollers themselves and the boxes around them (what is inside a scroller is retained), including a scroller the user scrolls (`overflow: scroll`) and the card around it; elements with an `onScroll` listener that scroll;
- highlights; anchored layers; sticky and fixed boxes; width keywords and natural widths; `layout` (FLIP) elements; a `display: contents` element that is built as an element of its own;
- collapsed tables (a paint-only motion on a part of one is still a patch, with the table, or the container of an anonymous table, as its unit);
- atomic inlines: a text with atomic inline boxes keeps its measure ([text.md](text.md#text-measure-keys-and-the-shape-store)), but it and its boxes are built every frame, about 43 µs each;
- a root that holds an `absolute` box placed from outside it (an escaping absolute), and an element that is itself `absolute` (it is built by every frame, and may be a patch unit).

**Other differences.**

- **Roots are boxes with a layout node.** A table row, row group or column is never a root; a root may be one element, but only one that is costly to build (a plain box is not worth its replay).
- **Scroll frames replay what gpui can move.** A scroller's offset is volatile, and what is below it moves with it. A root that gpui records to be moved is replayed at the new place; one it cannot is built: a root that holds a hole or a fragment, a rounded clip cut through its corner (so the rows that the rounded top and bottom of a scroller cut are built, the rest of the scroller is moved), a root inside a transform that is more than a translation, a box placed by something outside it, a position kept past the frame (a drag, a keyboard click, a tooltip), an input handler, a scrolled scroller inside, a root nested in another root that moves (the inner is built every frame the outer is), and every root at a scale factor that is not a power of two. A page that scrolls costs at most 1.05 times what it cost without retention (guarded), and a list of small rows a fifth (guarded).
- **A scroller that takes its classic gutter builds twice.** In the frame an `overflow: auto` scroller's content starts to scroll, the overlay prepaint calls `request_redraw_before_present` as a flipping container query does, and the draw builds the page again with the gutter before it presents: 2.0 times the elements of a frame whose bar is reserved ([scroll.md](scroll.md#scrollbars)).
- **Layout-driving loops are full frames.** Only animations that change what their element paints are patched or dormant. A `width` or inset loop, a layout (FLIP) animation and a motion value that drives a size are built every tick with the spine above them, culled or shown, and counted as `LayoutAnimation` in `getPatchCounters().unpatchable`. A binding of such a value that rests does not make a tick of a loop elsewhere whole, nor does a `motion.*` size key that rests (see [patch-frames.md](patch-frames.md)).
- **A read on a dirty window still builds the spine** and does layout and prepaint of the page; only `paint_if_dirty` inside dispatch builds every subtree. After a write that drives only paint-only properties it does nothing at all ([forced-reads.md](forced-reads.md#writes-that-do-not-make-the-next-read-run-a-pass-47)).
- **Call sites that refresh** build the whole frame that follows; see [the refresh rule](#what-makes-the-whole-frame-build-everything).
- **A page with `display` transitions that use `allow-discrete`** (`hold_display`) can build a different set of children with an unchanged key. The verify mode finds it only where a test reaches it.
- **A culled subtree draws nothing, so it has no hitbox.** The clip hides it, so that cannot be seen. While an element captures the pointer nothing is culled.
- **Pixels.** Retention against a whole build is exact (verify mode). Against the fork before the pass, a few randomly generated pages differed by 1/255 in some pixels; the cause was not found and it has not been looked at since #41 changed the text layout units, which differ on purpose.
- **A motion value binds no SVG attribute.** It binds style properties; the way to move the stroke of an `<svg>` with one is its `color` with `currentColor` strokes.
- **The first draw costs about 15% more** and **only `color` has an inherited read set**: see [What records, and when](#what-records-and-when) and [The colour read set](#the-colour-read-set).
- **The verifier is weaker where a recording was made in a region**, and its reference build leaves recordings unmovable: see [testing.md](testing.md#the-retention-verifier).
- **A frame that moves the box of a skipped host, or of one above it, ends the shared forced pass**, and a subtree under a content mask that did not move with it is built when something culled inside it is near the mask's edge: see [forced-reads.md](forced-reads.md#what-a-forced-read-costs-83).

## Rejected alternatives

- **Memoising the resolved style per element.** Style resolution is about 3% of a frame; what remained of the build was gpui-side moves of `Div` and `AnyElement`.
- **Excluding frames when the accessibility tree is on**, for culling and for replay. The tree is on for good once any assistive client has asked, so the gate would be no optimisation for those users. Both record and replay the tree's nodes.
- **Skipping with a per-element dirty flag in JS.** The key is computed in Rust from the tree; JS knows nothing about hover, focus or scroll.
- **One recording per scroller, translated as a layer.** A hover or an animation inside the scroller would rebuild all of it. The unit is a retained root, so a change rebuilds its own root, and a root bigger than two viewports is split.
- **Making the replay cheaper and nothing else, for #62.** It stays a step per root: with every cost of #63 in, a tick of the 30000 node page drawn whole still takes 36.0 ms, against 0.24 ms patched ([patch-frames.md](patch-frames.md)).
- **Painting lifted boxes with their parents.** A `position: relative` element with `z-index: auto` is painted at the context's z = 0 layer in tree order (CSS Appendix E), so a recording of the parent cannot hold it in place; holes and fragments keep the context's order exactly.

## Measured

All numbers are CPU p50 per frame on the test renderer with a release addon, three runs for poc and perf on the demo pages and one run for everything else. They are not what a window shows: the measuring session had no display link, so GPU time, presentation and the compositor are not in them. "Poc" is the fork before the pass, "off" is the pass with `GPUIX_RETAIN_OFF=1` (the persistent engine and the cheaper build, nothing skipped), "perf" is everything on. The tables were not re-measured after the `MIN_ROOT` rule, translated replay or lift holes, and the chat and form rows predate `<input>` being retained. How to run them is in [testing.md](testing.md).

**Pages of the fluid-functionalism demo** (the demo's own harness, not in this repo; ms per frame; elements built per frame in parentheses). *Idle* is nothing happening, *sweep* a pointer moving across the page, *scroll* a wheel.

| page (elements) | idle poc | idle off | idle perf | sweep poc / off / perf | scroll poc / off / perf |
|---|---|---|---|---|---|
| introduction (742) | 8.1 | 4.0 (359) | **1.2** (26) | 8.3 / 4.4 / 1.7 | 8.1 / 4.0 / 1.2 |
| showcase (2348) | 15.2 | 8.2 (617) | **3.5** (60) | 15.5 / 9.0 / 4.2 | 15.3 / 8.7 / 3.5 |
| changelog (2338) | 9.9 | 5.4 (451) | **2.2** (60) | 10.2 / 5.7 / 2.8 | 225 (a) / 8.5 / 5.7 |
| command menu (3478) | 13.8 | 7.4 (654) | **2.3** (42) | 14.2 / 8.2 / 2.9 | 14.1 / 7.4 / 2.3 |
| fluid hover (2006) | 10.2 | 5.2 (456) | **1.8** (40) | 10.3 / 5.5 / 2.5 | 18.5 / 9.6 / 3.4 |

The showcase scroll has p95 138 / 28 / 18.9 ms and fluid hover 93 / - / 13.0 ms. (a) Poc drew 20.7 times per frame on that page in the test renderer, which draws a dirty window inline whenever effects flush; a window with a display link draws once. The comparison is of the pass against that artefact, and the other scroll rows are the fair ones.

The changelog and fluid hover pages have an animated svg that moves a pixel a frame, and the roots it moves are rebuilt each frame, legitimately. A scroll frame builds more than an idle one (showcase 258 elements, changelog 319, fluid hover 143); an earlier profile put the p95 spikes on frames where a cull region flips, which the demo does with `display: none` toggles.

**Benchmarks in `examples/`:**

| benchmark | poc | off | perf |
|---|---|---|---|
| #36 nested auto-width columns, depth 12, 8 instances (`depth-col`) | 54.9 ms (6861 µs each) | - | 0.07 ms (6 µs each) |
| #37 page of 2042 elements, idle (`one-animating-node`) | 18.0 ms | 9.2 ms | 1.29 ms |
| #37 the same page, one `opacity` node animating | 18.8 ms | - | 1.51 ms (2 elements built) |
| #38 2000 boxes, visible / clipped / in a scroller (`clipped`; "off" is the shape that measures culling, a11y on) | 10.8 / 10.2 / 10.1 ms | 6.6 / 4.6 / 4.6 ms | - |
| #38 a 16x13 table (639 elements), visible / clipped | 4.7 / 4.0 ms | 4.0 / 2.8 ms | 0.90 / 0.32 ms |
| #37 200 cards in a scroller, 3 px a frame | 10.9 ms a draw (32.7 ms for the 3 draws a flush makes here) | 4.9 to 5.4 ms | 0.86 to 0.98 ms (0.95 a draw in the poc-comparable run) |

**Pages of `retain.perf`** (per frame; hover is the pointer over the page, typing is a text edit):

| page (elements) | idle poc | idle perf | hover poc | hover perf | typing poc | typing perf | skipped idle |
|---|---|---|---|---|---|---|---|
| cards (2042) | 16.5 | 1.31 | 16.8 | 1.98 | - | - | 100% |
| table (593) | 4.33 | 0.58 | 4.80 | 1.34 | - | - | 100% |
| timeline (925) | 7.13 | 1.35 | 8.82 | 2.54 | - | - | 98% |
| mail (344) | 3.05 | 0.69 | 3.89 | 1.04 | 8.75 | 5.12 | 94% |
| chat (1053) | 2.95 | 1.16 | 3.34 | 1.32 | 8.40 | 4.72 | 59% |
| form (68) | 0.75 | 0.39 | 0.78 | 0.42 | 2.59 | 1.90 | 65% |

A chat page is capped by its virtual-list rows and a form by its inputs, which were never retained when this was measured.

**A page of clipped and scrolled-out menus** (`examples/command-menus.perf.test.tsx`, run with `bun run test:perf:menus` in `examples/`; profiling, no budgets). Before the perf pass a frame spent about 70% in build, `request_layout` and taffy and about 27% in prepaint and paint. An idle frame (`renderer.flush()`, nothing changed) took, release build, p50 CPU: 10.6 ms with every menu laid out, 4.1 ms with the far ones hidden, 2.8 ms with all hidden. `GlobalElementId` hashing its id path once, at construction (zed `1f55cd8cd9`), took that to 9.6, 3.7 and 2.7 ms. The perf pass took the idle frame to 0.99, 0.45 and 0.37 ms when retained subtrees first replayed (95% of the elements are in subtrees an idle frame leaves unbuilt).

## Upstreaming

The #36 cache change is upstream's own #1010 (the fork's key adds to it) and is the one clean candidate. The rest is one engine change in taffy and gpui, and a gpuix side that is specific to this fork's element builder; none of it is separable without the persistent engine.

## Tests

- **Spine and deltas**: `layout-pass-deltas.test.tsx` (hover); the unit tests of `retain.rs` (active, focus, measurement, links, selection, the spine and what `unbuilt` carries, the lifts of a skipped root); `retained-spine`, `retained-frame-sources` (what else changes per frame without a prop, three ways around a wrapper) and solid's `retained-cull-wrapper` (the shape of the Fluid Functionalism demo cards) for a motion that runs where the way up to a replayed root is never built (below a `display: contents` wrapper, inside an `<svg>`, in an inline span).
- **Parity**: `retained-parity.test.tsx`, with and without `GPUIX_RETAIN_VERIFY=1`.
- **Refresh rule**: `live-invalidation.test.tsx`.
- **First build**: `retained-first-build.test.tsx` (draw 2 after a root background change builds no card and every card is a hit; a changed card is built once and the frame after replays it with no lazy build; the pictures, text, boxes and accessibility tree equal retention off through six steps; a card beside a running motion is recorded and the spine is not; a rounded, bordered scroller of sections scrolled with the wheel fails on a verifier mismatch).
- **Colour**: `retained-inherited-color` (cards with their own text colour build 0 and replay 12; cards whose text inherits; a `currentColor` border with its own text colour; a nested declaration cutting only below it; `color: currentColor` and a `color-mix` of it not cutting; 15 readers drawn as a build draws them; a hover state), `color::tests` (reads per depth), `retain::tests` (a card that sets its colour is not changed by the page's).
- **Anonymous collapsed table**: `collapsed-border-transition` (every tick patched with one unit, no node created or removed, no root of the page replayed, the same work with 60 and 240 siblings, the colour of ticks 2, 5, 10 and 12 equal to patching off, a container of 200 extra rows whole and counted `TooLarge`, a border widened mid-transition lays out and goes on patched).
- **Content visibility and scroll**: `content-visibility-auto` (the frame that shows three entries builds their roots once and the frames after build none); `retained-scroll` counts relocations under the verifier by scrolling in pairs (verifier off for the frame that records, on for the one that moves; [testing.md](testing.md#the-retention-verifier)). Plain and under `GPUIX_RETAIN_VERIFY=1`, with no `[retained]` mismatch.
- **Hover frame work**: `hover-frame-work.test.tsx` (a 60-row sidebar beside 200 cards below the window or in a scroller, with and without an `opacity: 0` copy of each label, accessibility tree on and off, makes the frames of 0 cards in elements built, measures, shapes, styles, children set, nodes created, extent and overflow walks, prepainted and painted elements and painted primitives; an `opacity: 0` text and box add no primitive and keep their box and are hit; one keyframes text on 3000 nodes builds at most 3 elements a frame; an inherited colour changed over 30 labels measures and shapes nothing; also under `GPUIX_RETAIN_VERIFY=1`).
- **Perf**: the `clipped`, `one-animating-node`, `depth-col` and menus benchmarks in `examples/` ([Measured](#measured)); `timeline.perf.test.tsx`'s budgets are in [testing.md](testing.md).
