# Observers and content-visibility

How an app is told about boxes (`onLayout`, `ResizeObserver`, `IntersectionObserver`), in which order a frame's reports arrive, and what `content-visibility` does: skipping the content of an element, deciding when it is relevant, and what stays visible to events, the accessibility tree, Tab and find while it is skipped.

## Observers and `onLayout`

`ResizeObserver` and `IntersectionObserver` follow DOM semantics. They read the boxes a pass published, once the frame is presented.

### Reports are read from the published boxes

An element with `onLayout`, a `ResizeObserver` or an `IntersectionObserver` has no paint listener of its own. `observers::frame_sweep` runs after each painted frame and reads the box of every such element from the registry `automation::get_bounds` serves: the border box, the border and padding (`ElementBounds`), and for an intersection the boxes of its clipping ancestors. A replayed subtree republishes the same boxes (see `tape` in [retention.md](retention.md#the-effect-tape)), so an observed element can be replayed and the report is the one a build gives: same draw, same entries, one event per change.

The targets come from `RetainedTree::geometry_ids`, which holds the elements with any of the three listeners, custom elements included. Their clipping ancestors are computed from the tree when its revision moves. An inline span paints in its paragraph's container, which the build that made the paragraph notes (`ObserverReports::note_clip_origin`).

A layout-only pass reports nothing. A forced layout delivers no `ResizeObserver` callbacks in a browser, and the frame that follows reports the change against what was last delivered. `sweep` runs after painted frames only and compares against the sizes last delivered, which a layout-only pass does not move. Don't make a bounds read draw to get its report: a `scrollTop` write after a style change would then deliver `onLayout` ahead of the scroll event of its frame, where Chrome delivers the scroll, then the resize observation, and a forced layout delivers none (`fixtures/scroll-order-oracle.json`).

### The order of a frame's reports

The order is the DOM's rendering steps, stated rather than an effect of how many times each report was deferred. Scroll events come first, then `onLayout` and `ResizeObserver` (gpuix's `onLayout` is `ResizeObserver` for an element, so it is delivered with it), then `IntersectionObserver`, as in "update the rendering": run the scroll steps, then the resize observation loop, then the intersection observation steps.

- `report_scroll` queues into `PaintReports::pending_scrolls` at paint, in paint order (the DOM's is the order the scrolls happened in, which is the same for the scroller one input moves). `observers::sweep` drains the queue first, and a deferred flush drains it when nothing listens for boxes.
- The sweep emits `onLayout` and resize reports in document order (`onLayout` before the resize report of one element), then intersection reports. An element that is gone comes after the ones that are there of its phase.
- The callbacks are ordered on the JS side (`js/observers.ts`) as the specs do. The "gather active observations" loop walks the document's observers in creation order and each observer's targets in the order `observe()` added them. A `ResizeObserver` re-observing a target moves it to the end; `IntersectionObserver.observe` of a target it already has changes nothing. Every resize callback runs before any intersection callback.
- Two frames merged into one delivery keep the entries of one target in the order they arose.

### Removed elements

An element that is removed is reported gone by the next presented frame, not by a layout read. This is kept on purpose: in the DOM a forced layout (`getBoundingClientRect`) does not deliver observations, which wait for the next rendering step. A layout-only pass reports neither an absence nor what is there.

### The root of an `IntersectionObserver`

An element root is its own report. It has the `intersectObserve` listener like a target, and its report carries the border box, the border and the room a classic scrollbar reserves (`Observation::scrollbar_width`, `scrollbar_height`, from `renderer::reserved_scrollbar`, the handle's size as of the last layout). `js/observers.ts` takes the client box from it and reads nothing: the padding box less the scrollbar, which is what Chrome calls `rootBounds` before `rootMargin`. A percentage margin is of that box, width for left and right, height for top and bottom. The clip a target is cut by at an `overflow` ancestor is the same client box (`clip_rects`).

Reports are not applied as they arrive. Each marks its record, and `flushObservations` evaluates the marked records once the frame's reports are all in. A root that moves or resizes in the frame its target changes therefore gives one entry, from the geometry of that frame: a root that moves with no threshold crossed gives none, one that resizes so a ratio changes gives its entry in that frame.

## `content-visibility`

`contentVisibility: "visible" | "auto" | "hidden"` and `containIntrinsicSize`, with `containIntrinsicWidth`, `containIntrinsicHeight` and the `InlineSize`/`BlockSize` spellings, which are the width and the height. The physical longhand beats the logical one, and both beat the shorthand. `hidden` always skips; `auto` skips while the element is not relevant to the user ([below](#auto-relevant-to-the-user)). The keys are in `layout/content_visibility.rs` and `css/contain_intrinsic.rs`; `contain-intrinsic-size` takes `none`, a length, `auto <length>` or `auto none`, a number is px, and the order is width then height.

### Containment

Any value but `visible` contains the element, on every element type (`content_visibility::contained`, a style rewrite applied once the final style of an element is known, and again by `Resolved::of_style` so the walks over the retained tree agree):

- `overflow: clip` on each axis the author left `visible`; an `overflow` that already clips wins.
- A stacking context (`stacking::creates_context`).
- `position: relative` with the insets dropped and no `z-index` (unless the element is an item), when the author's position is static. This makes it the containing block of its `position: absolute` descendants (`containing_block::is_positioned` reads the resolved position).
- Layout containment (`contain_layout`, set by `layout::style::apply`): an independent formatting context and the containing block of its `position: fixed` descendants. The design is in [layout.md](layout.md#layout-containment-of-content-visibility).

Chrome gives `auto` and `hidden` layout containment whether or not the content is skipped (`EffectiveContainment` keys off the computed value). The box is a block formatting context root, so no margin of a child collapses through its top or bottom edge, and it is the containing block of `position: fixed` descendants as well as of absolute ones. Size containment applies `contain-intrinsic-size` after the children are laid out, so a child's percentage height under an `auto`-height skipped box is `auto` (0 if empty; 100 under an authored `height: 200px`, which is definite).

A box with `overflow: clip` on both axes is not a scroll container, so as a flex or grid item it keeps its content-based minimum size (an `overflow: hidden` one shrinks to nothing in a flex column). Every `auto` and `hidden` element has that overflow.

### What skipping does

Skipping applies to host elements `div`, `text`, `label`, `form` and block `span`; another element type is contained and never skipped.

- The element itself is built and painted (background, border, hit area). Its children are not built, planned or counted (`build_host_container`).
- It has size containment on both axes with `contain-intrinsic-size` as the content box (`contain_skipped`): `none` is 0, `auto <length>` is the remembered size once there is one, else the length. Its automatic minimum size as a flex or grid item is nothing (Chrome shrinks a skipped item to its padding).
- Nothing is hidden or forgotten as under `display: none`: the DOM, the transition and animation state and the resolved styles stay.
- No animation or transition below ticks or fires an event while skipped. The timeline goes on: on show the runs sample the instant they find, `currentTime` is where the clock is, and the iterations missed are one `animationiteration`, not replayed.
- A subtree that shows again is built whole. `Retain::skip_content` runs when an element begins to skip and drops the records, `dynamic`, `dormant`, `unbuilt` and `woken` entries of everything below (the layout of content a read forced is kept), so no ancestor is kept in the spine by a loop nobody sees and the dormant clocks stop. The bookkeeping is in [retention.md](retention.md); the dormant clocks are in [patch-frames.md](patch-frames.md#dormant-animations-63).
- `RetainedElement::is_built_by_ancestor` and `box_generation::is_hidden` count an element below a skipped one, so a change there is not a spine of its own. An `<img>` there is loaded like one under `display: none`, by `load_hidden_images`.

A scroller in forced content has a scroll handle. The handles a read sees (`SCROLL_HANDLES`) are synced at the end of a build for the elements that have a box, and `scroll::has_scrolling_box` asked `is_hidden`, which counts a skipped ancestor as built last time. `is_hidden_in_pass` and `is_skipped_by_unforced_ancestor` leave out an ancestor that the pass forces, so the handle of a scroller in forced content is synced and `scrollHeight` is its extent. A write still finds no handle, as the pass that follows a read is not forced.

### Reads of geometry force the skipped ancestors

`getBoundingClientRect`, `getElementBounds` and `getScrollMetrics` build the content they need for the read and skip it again in the next painted frame (Chrome's `DisplayLockUtilities::ScopedForcedUpdate`). The pass costs one layout-only pass, leaves no trace in the element's state and tells nothing: the animations of a forced element are sampled for the boxes and put back, so no `animationstart` fires for a loop that is never shown, and the boxes are those of the animations at the instant of the pass. The mechanism and its cost are in [forced-reads.md](forced-reads.md#reads-inside-skipped-content-88). A read of an element outside any skipped subtree costs the walk up its ancestors and nothing else.

The test renderer's `getBuiltElementBounds` is the old read: the box of the last layout, null for what is not built, which is how tests tell that the content of an element is skipped.

### `auto`: relevant to the user

`content_visibility::Relevance` holds one answer set per tree (`tree.relevance`). An `auto` element builds its content while it is near the viewport or holds the focus, and skips it (as `hidden` does) while it is not.

Near is as in Chrome, measured against headless Chrome and not the spec: the border box outset by 150% of the viewport on each side (1.5 times the width to the left and right, the height above and below), cut by the padding box of every ancestor that clips it and by the viewport, still has an area. An element at the very edge of the margin is not near.

Near is measured in the window, where a transform puts the box: the margin is outset in the element's own space (a scale scales it, a rotation turns it) and the outset box is carried through the element's own transform and its ancestors' to the window, then cut by the window mask and by the clip a general transform keeps (`Window::local_clip_in_window`). Compared in the space it was laid out in, a box under a scale, a rotate or a translation that moved it was skipped on screen or built far away (#116; `content-visibility-transform.test.tsx` against `fixtures/content-visibility-transform-oracle.*`, regenerate with `bun fixtures/content-visibility-transform-oracle.ts`). A transform written without a build (a patched animation tick) moves the boxes just published, so the patch defers `observers::sweep_relevance` when the page has `auto` elements; a page without any pays one `is_empty` check.

Focus: the focused element and its ancestors (`FocusStyles::within`, given to the store at the start of every render) are relevant, so focusing an element inside a skipped one builds what holds it, in the same frame.

**How it is decided.**

- The first answer comes in the frame that builds the element. An `auto` element nothing has answered for is built skipped, and its prepaint listener (`content_visibility::watch`) asks whether it is near, from the bounds it is prepainted at and the content mask (the ancestors' clips and the window). An element in a culled subtree is not near. A subtree that holds an `auto` element is culled only past the margin (`Subtree::reach(proximity_reach)`), and a box that clips both axes drops what is below it from the reach, as the margin stops at its clip.
- A near answer for an element built skipped is `request_redraw_before_present` (the container-query pass, `MAX_PASSES` 4), and the next pass builds it. A page whose `contain-intrinsic-size` is not the size its entries have is laid out again by that pass, and what is near moves: the first draw presents the first answers and the frames after it settle the rest, invisibly, 150% of a viewport away.
- After that the answers come from the sweep that follows each painted frame (`Relevance::sweep`, from `observers::frame_sweep`, which runs when any `auto` element exists though nothing observes). It reads the published boxes with the clips the `IntersectionObserver` uses (`clipping_ancestors`, `clip_rects`) and asks for a frame when an element is to show or skip otherwise than it was built. It asks as a commit does (`invalidate_after_commit`), not with `Window::refresh`, which would turn replay off for every root: a scroll that shows three entries of a page of 190 built 227 subtrees that way, against 6.
- Those changes take effect in the next frame, with no hysteresis, as Chrome's do. The elements that differ are a source of the frame (`Sources::relevance`, `Relevance::pending`), so only the roots above them that are replayed are built, and they are part of the key of the element (`container_key`).

**Remembered size.** The content box (border box less padding and border) of an element that is not skipped is remembered from its prepaint and from the sweep, for `contain-intrinsic-size: auto`. `auto` is forced on both axes of an `auto` element, so a skipped one keeps the size it had, and the declared length (none is 0) is for one never laid out. Any element with `auto` in its `contain-intrinsic-size` remembers, so a `hidden` one that was visible keeps the size it had.

### `contentvisibilityautostatechange`

`onContentVisibilityAutoStateChange` (`event.skipped`; bubbles, cancelable, with a capture twin) is sent for `auto` elements only, after the frame that applied the state (`sweep_relevance`, in document order). It fires once for the first determination (`skipped: false` for an element built whole in the frame that mounted it, `true` for a far one) and then each time the state differs from the one last told. Going from `auto` to `visible` and back forgets what was last told, so the first answer is told again.

**The first frame's events.** For an `auto` element that is there before the first rendering update (in the markup, or added in `DOMContentLoaded`), Chrome tells: on screen, `skipped: true` before the first animation frame callback and `skipped: false` after it; off screen, `skipped: true` only; for an element added after the first frame, one event with its state. A style update before the first lifecycle locks the element, and the first full lifecycle makes the first intersection observation synchronously and unlocks it (`fixtures/content-visibility-auto-oracle.ts`, scene `load`: `event 0 true`, `event 1 true`, `raf 1`, `event 0 false`, `raf 2`, `event 2 false`, `event 3 true`).

gpuix matches that order. `Relevance` knows whether a frame with something in it has been drawn (`drawn`): the first sweep sets it, and so does `frame_sweep` for a frame whose root has content (`RetainedTree::has_content`) and no `auto` element, since the container a renderer mounts into is drawn, empty, before the first element is. The first sweep tells every `auto` element it has not told `skipped: true` first (`Swept::locked`, in document order, each recorded as told `true`), and then the state it is in (`changes`): `false` for the near ones, nothing more for the others. There is no `requestAnimationFrame` here, so both come after the first frame, in Chrome's order and not around a callback. An element that comes after that frame is told once. An app whose window is drawn empty before its tree is mounted tells its elements once, like an element added after the first frame.

### Skipped content in the accessibility tree and in the tab order

Chrome keeps the content of a skipped `auto` element in the accessibility tree (roles and names come from the DOM, no layout is needed, and `aria-labelledby` into it still names) and lets Tab reach a focusable element in it, scrolling to it and making the element relevant. `hidden` content is neither (Chrome 156: `Accessibility.getFullAXTree` with a screen reader on lists every heading and button of the page but those of the `hidden` entry; Tab from the last built button lands on the first button of the skipped entry). Nothing below a skipped element is built here, so both come from the retained tree.

**Accessibility.** With the tree active, `build_host_container` gives a skipped `auto` element the nodes of everything below it (`accessibility/skipped.rs`). `exposed` walks the retained tree in document order (`contents::flatten`, so a flex `order` does not move a node, as in Chrome) and:

- leaves out `display: none`, `aria-hidden` and `inert` subtrees, nodes under `visibility: hidden` (a descendant that is `visible` again is kept), the parts of a paragraph (`is_part_of_paragraph`: an inline `span` has no node when built either) and everything below a nested `hidden` element;
- goes down through a nested `auto` one.

The node of an element is made by the code that makes it for a built element: `apply_accessibility` runs on a throwaway `div`, and the role, `write_a11y_info` and the relations are read back from it (`Element::a11y_role` and the rest). A role, an `aria-*` prop, an `id`, a name from a `<label>` or from a reference, the text of a `<text>` (`describe_text`, shared with the built path, and the same for a skipped `<text>` itself, whose plan is made only for it) and the default roles (`accessibility::default_role`, also read by `build_host_container` and the editor) are never written twice.

The skipped element hosts the nodes with gpui's `a11y_synthetic_children`, nested (`A11ySubtreeBuilder::push_nested`, `push_relation`; the id of a node is the hash of the host's and the element's). It gets a node even with no role of its own, a `Group` (`apply_hosting`), which it has only while it hosts something. The nodes are recorded and replayed with the element; they are made when the tree is active at the build, and gpui's replay key already tells a window whose tree turned on or off. A change below the element moves its `subtree_revision`, and a link that changed (`links::take_changed`) makes its ancestors build, so a name that comes from a label or a reference follows.

**Tab.** gpui moves focus along the tab stops it painted. `focus::step_focus` first asks `focus/skipped.rs::target`:

- The focusable elements (`tree.focusable_ids`, so `inert`, `visibility: hidden` and a disabled control are out already, and a negative `tabIndex` is no stop) that have a skipped `auto` ancestor and no `display: none` or `hidden` one are put in tab order (the `tabIndex` group, then the path of child indices from the root).
- They are ordered with the stop gpui would pick (`Window::next_tab_stop` / `prev_tab_stop`, which only read) and the focused element, and the one that is met first wins, wrapping like gpui does.
- The element has a focus handle already (`sync_focus_handles` makes them for every focusable element, built or not), so `change_focus` focuses it and the events run.
- It is not built, so there is no box to scroll to: `read_after_layout` lays out its skipped ancestors as a read of geometry does, and the scrollers around it scroll to its nearest edge from that box (`scroll_element_into_view_at`). The frame after builds the element, which holds the focus (`wanted_skipped`).

A tree without an `auto` element costs one check per Tab.

### Find in skipped content

Chrome's find buffer (`FindBuffer::ShouldIgnoreContents`) leaves out the content of a locked element that find-in-page cannot activate, which is a `hidden` one, and searches the content of an `auto` element whether it is skipped or not. A match that becomes the active one activates the lock (`ActivateFindInPageMatchRangeIfNeeded`, kept unlocked for two lifecycles) and the page scrolls to it.

- `collect` leaves out what is below a `hidden` host element (`content_visibility::found_in`, the declared value; `same_paragraphs` moves `search_revision` when `content-visibility` changes, so the cached groups follow) and tags each group below an `auto` element with its id (`Group::autos`, innermost first). `matchCount` therefore does not count `hidden` matches.
- `MatchSet` keeps, from that, the `auto` elements around each match (`autos`) and how many matches each element holds (`held`). Skipped `auto` content is in the total.
- Numbering stays in document order. An `auto` element that skips its content announces its matches in its prepaint listener (`watch`, `search::announce_skipped`) as one entry of `count` ordinals at its `DocPos`. `number` (split out of `wash`) sorts that in with the runs that announced theirs, so the built matches after it keep the numbers they have in the document.
- The reveal: numbering notes the `auto` elements around the active match (the element of the skipped entry that holds it, or the `autos` of a painted match). `search::settle_frame` returns them, and `observers::frame_sweep` calls it in the paint of every frame (a frame with no wash to paint, because every match is skipped, would number nothing otherwise) and hands them to `Relevance::set_found`. `wanted_skipped` treats them as it does the focus within: the sweep finds the element wants to build, the frame after builds it, its match paints as the active one, and it stays built for as long as the match is active.

The reveal must last as long as the match is active. One that lasted only the frame that found it would build and skip the element on alternate frames when the active match is far from the viewport.

Rejected: numbering skipped matches by their index among the collected ones alone (`ordinal = offset + index`). The native runs (`<code>`, `<markdown>`, `<diff>`) are numbered between the retained ones, and only the sort by `DocPos` puts them in place; the skipped element has a `DocPos` where its matches have none. Taking the `activeIndex` for the index of a collected match to find the element to reveal fails for the same reason.

### Observers below skipped content

Measured against headless Chrome 156 (`fixtures/content-visibility-observers-oracle.*`; `hidden` and `auto` alike). The content of a skipped element is not gathered.

- **`ResizeObserver`.** It tells nothing of a target in skipped content while it is skipped (no 0 by 0 when it hides), keeps the size it last told, and on show compares the target with that size and fires only if it differs. A target never rendered fires once when it shows. The skipped element itself is a normal box (a `hidden` one reports its `contain-intrinsic-size`).
- **`IntersectionObserver`.** A target in skipped content (`hidden`, and `auto` while it is skipped) is told "not intersecting" with `boundingClientRect`, `intersectionRect` and `rootBounds` all zero. That is its first report (a target never laid out), and when the skip lands (one frame after the host moved away or `content-visibility` changed; the frame that moves the host still reports the real geometry, it is not skipped yet), only if the target was intersecting. While it is skipped nothing is told, whatever moves (the host, the content inside, a `rootMargin` of 4000px that reaches it), and on show a fresh entry comes from the new layout.
- **A read of geometry** forces the content for the read and changes none of this: it delivers nothing and moves no size told.

How `observers::sweep` does it:

- It leaves a target without a box because an ancestor skips (`inside_skipped_content`) alone for the `ResizeObserver`. `display: none` and a removed element are still 0 by 0, as in Chrome.
- For an `IntersectionObserver` it reports a target that is inside skipped content, with a box kept from a layout or none, as gone (`present: false`, the zero entry above) and does not read its box.
- The first probe of an observed element is `probeElementBounds`, `read_after_layout` with `SkippedContent::Leave`: the same pass as `getElementBounds`, but the skipped content stays unbuilt and a target in it has no box, so observing in hidden content builds none of it.

### CSS animations in skipped content

Measured against Chrome 156 (`fixtures/content-visibility-animation-oracle.{html,py,sh,json}`; scenes `q1`..`q3`, `hidden` and `auto`). Chrome tells nothing for a run while its content skips. At the reveal it tells what the run owes, two or three frames later:

- one coalesced `animationiteration` with the `elapsedTime` of the iteration reached;
- `animationstart` and `animationend` of a run that began and ended meanwhile;
- `animationend` and `transitionend`.

The first frame shows the value of the instant. An animation or class added, or a style changed, while skipped starts (its transition) at the reveal from the start. Content skipped from its first frame is styled at the reveal and starts then. `getAnimations()` of a running animation reads the live time and tells nothing. The tests allow gpuix the first three frames after the reveal and none before.

gpuix is as Chrome in these cases:

- A loop skipped 1.35 s tells one `animationiteration` (1.5) and nothing earlier; its first frame is the control's colour and time.
- A finite 300 ms animation and a delayed one that ended while skipped tell `animationend` (0.3), or `animationstart` and `animationend`, at the reveal, with the end value.
- A class added while skipped starts at the reveal from 0.
- A transition started before the skip tells `transitionend` at the reveal with the end value.
- A style change while skipped (no read) starts its transition at the reveal, from 0, also when the reveal comes mid-way.
- Content skipped from load starts its animations at the reveal (`auto`: a host that is far at the first frame).
- `getAnimations()` while skipped reads the live `currentTime` and tells nothing.
- A motion value written at every tick into skipped content shows the last value when it is shown.
- `auto` draws the content one frame after the reveal (it is built in the frame that finds it near); the events still come in the first.

Don't let `animate_style` mark an element as animating opacity unless it stepped it (`steps`). An element built as forced content (a geometry read inside skipped content, `is_forced_by_ancestor`) is sampled and not stepped, so it has no animated style for the frames that skip it, and the `expect("an opacity that runs has the style it runs on")` aborts the process. It reproduced as soon as a scene read the bounds of an element with a looping `opacity` in an `auto` host being scrolled back.

A forced read queues no animation event for a run the element already had and ticks nothing. A read of an element in content gpuix never built styles the skipped hosts around it, as Chrome does (scenes `q1_load_read`, `q2_class_read`, `q6_*`, `q7_*`): its CSS animations are made at the read, wait for the next frame, run with the clock while skipped and are the same objects for every later read and after the reveal, and the DOM hears of their starts, iterations and ends after the reveal. Any direct read of an element styles the whole never-rendered host subtree (rect, `getAnimations()` or the style of an unanimated property alike); the lists of a subtree and of the document do not. A transition a read makes tells `transitionrun` and `transitionstart` at the read, runs on while skipped and tells `transitionend` at the reveal (`q3_inside_read`, `q3_inside_mid_read`). The mechanism is in [forced-reads.md](forced-reads.md#animations-in-forced-content).

## Breaking changes

Each is a change vs upstream gpuix, because the DOM does it that way.

- A frame's reports reach the app in the order of the DOM's rendering steps: scroll events, then `onLayout` and `ResizeObserver`, then `IntersectionObserver`.
- `onLayout` fires on every element an observer works on, custom elements included.
- An element that is removed is reported gone by the next presented frame, never by a layout read.

## Tests

- `retained-observers.test.tsx`: observed elements replayed report as a build does.
- `frame-event-order.test.tsx`: the order of a frame's reports, with the callback order on the JS side.
- `observer-gone-reports.test.tsx` and `forced-layout.test.tsx`: a removed element is reported by the next presented frame, and a forced layout reports nothing.
- `content-visibility-observers.test.tsx`: 39 scenes against `fixtures/content-visibility-observers-oracle.*`, frame for frame. Root of an `IntersectionObserver`: the `io8` scenes, and "evaluates an observer with an explicit root without reading layout" (no layout pass, no `getElementBounds`). `io8b2` (a classic scrollbar on the root, `setScrollbarStyle("classic")`) is Chrome's frame for frame, in a plain frame and under `GPUIX_RETAIN_VERIFY=1` alike: the root takes its gutter in the frame its content starts to scroll ([scroll.md](scroll.md#scrollbars)).
- `content-visibility.test.tsx`: 93 cases against headless Chrome (`fixtures/content-visibility-oracle.*`, regenerate with `sh fixtures/content-visibility-oracle.sh`) of a `hidden` element with a child much taller, in a block, flex row and column, grid, `inline-block`, `inline-flex`, `fit-content`, `min-content`, `max-content` and absolute parent, with padding and border, a definite size, `min`/`max`, every spelling of `containIntrinsicSize`; and `auto` elements on screen (children laid out, an absolutely positioned child placed against the element), at 1/64 px.
  - Pixels: the content clipped at the padding box and not without `contentVisibility`, an `overflow` that clips kept, the background and border of a skipped element painted and hit, a `z-index` inside it stays inside.
  - Counters: 200 rows under a `hidden` element shape, measure and create nothing (2 elements built, 2 layout nodes: the page and the element), and rows added below it neither; the rows are laid out when it shows and gone again when it hides. A loop below it, on screen or out of the window (dormant), fires no event and builds nothing for 3.5 s, and continues from the timeline.
  - 37 `contain-*` cases of `fixtures/content-visibility-cases.json`: margins that stay inside a `hidden`, `auto` or padded target and collapse through a `visible` one; a target's own margins collapsing with its siblings'; a fixed child with `top/left`, insets and `bottom/right` placed in the padding box of a target with a margin, padding and border, and at the viewport under `visible`; `height: 50%` in a block, a flex column and row, and a grid under `hidden` with an intrinsic size, an authored height and padding, and an on-screen `auto`; and the `hidden`-element test that holds `scrollHeight` at 200.
  - Six `hidden-read-*` cases (padding and border, an intrinsic size, a flex row, a percentage width, an absolutely positioned child, and the example of the specification, a child at (11, 22) of 100x200) whose child is read against Chrome at 1/64 px.
- `content-visibility-auto.test.tsx`: eight scenes against headless Chrome (`fixtures/content-visibility-auto-oracle.*`, regenerate with `bun fixtures/content-visibility-auto-oracle.ts`, which drives `chrome-headless-shell` over the DevTools protocol, one page per scene, a page settled before the scene is added since the first frame of a page is told differently). Each scene is a list of steps (mount, scrolls, a focus, a style change) read after the page settles: which elements are skipped, the scroll extent and offset, the size of the boxes, which element has the focus, and what each element was told (`exact` per step where nothing moves under a scroll, the last state told where the layout moves as the entries are laid out).
  - `list` (190 entries of 37 px with an intrinsic size of 40 px, 900x780 window, scrolled to 1000, 4000, 7000 and 0), `exact` (the intrinsic size is right), `bare` (no `contain-intrinsic-size`: `auto` is forced, an entry keeps the size it had), `clipped` (a scroller of 400 px: what clips an entry cuts the margin), `horizontal` (a row scrolled along x), `nested` (24 sections of 20 rows: a row is clipped by its section, so only those of the sections on screen are built), `focus` (an entry far away that holds the focus stays built through scrolls and is skipped when it is blurred), `remember` (a `hidden` element that was visible keeps its size, a never-shown one has the length, a far `auto` one has the length until it is near and then its size).
  - Counters: the first draw of the page of 190 entries with text builds the near ones (`first draw` in the test log), idle frames build nothing; a far element runs and builds nothing below it, and goes on from the timeline when it comes near; the value changed to `visible`, `hidden` and `auto` again. A scroll that shows three of 190 entries beside a sidebar of 60 text rows builds the roots of those entries in the frame after the sweep (6 subtrees, the same with and without the sidebar) and nothing in the frame after that.
  - Forced layout: scenes `forced` (the bars of four of the 190 entries, near and far, read at mount and after a scroll to 4000 and back; the entries stay skipped), `forcedNested` (a read through two `hidden` elements) and `forcedScroller` (a scroller in a `hidden` element and in an `auto` one far below, before and after a scroll: `[0, 0, 40, 200, 40, 50]` both). Counter tests: a read of a far entry creates the 11 nodes and the one line of text of that entry and no more, paints nothing, tells nothing, remembers no size and starts no animation; the frame after it builds and paints what any frame does and not the text of that entry; a read of an element that is built on a clean window runs no pass. The oracle page asks for a frame and checks `checkVisibility({ contentVisibilityAuto: true })` after the read, since about one run in six Chrome had made its first rendering update before the case was on the page.
  - First frame: the oracle's scene `load`; "the first frame of a window" (the three events in Chrome's order, nothing more over idle frames; two elements added later told once each; an element after a frame with other content told once); the scenes of the oracle, which Chrome adds to a page that has settled, are mounted after a first frame of their own; the bubbling test counts the first frame's two events.
- `content-visibility-a11y.test.tsx` and `fixtures/content-visibility-a11y-oracle.*` (regenerate with `bun fixtures/content-visibility-a11y-oracle.ts`): the headings and buttons of the page with a screen reader on against Chrome (40 entries, one `hidden`); the nodes and their order, the role of the entry kept, `aria-hidden`, `display: none`, `visibility`, `hidden` and nested `auto` content, `aria-labelledby` both ways, a change below the entry, one node when the entry is built and when it is skipped again, the tree turned off and on, a skipped `<text>`; Tab and Shift+Tab from the last built button over `auto` entries and over `hidden` ones against the focus order of Chrome, the entry built and the button scrolled to the bottom edge, wrapping, and no stop in `display: none`. Rust: `focus::skipped` (7 unit tests of the choice).
- `content-visibility-find.test.tsx` (5): the total counts 39 of 40 entries, the far ones skipped and the `hidden` one out; text only in the `hidden` entry finds nothing and builds nothing; stepping `activeIndex` to a far entry builds it, paints its match as the active one and keeps it built over six frames, and the entry of a match that stops being active is skipped again; past the `hidden` entry the numbering skips it; scrolled to the end with the first entries skipped, the built matches keep their document-order numbers; a change of `contentVisibility` follows. Rust: `text::search` (3 unit tests: collect, numbering of skipped matches, settle) and `content_visibility` (`set_found`).
- CSS animations in skipped content: `skipped-animation-events-fixture.tsx`, called by `content-visibility.test.tsx` for `hidden` and `content-visibility-auto.test.tsx` for `auto` (the `q1` to `q3` and `q6` scenes x 2 modes, in ticks of 16 ms in the test renderer, the reads of `q1_load_read`, `q2_class_read`, `q3_inside_read` and the `q6` scenes included) and `skipped-animation-scope-fixture.tsx` (the `q7` scenes: which elements of a never-rendered host a read styles); regenerate the oracle with `sh fixtures/content-visibility-animation-oracle.sh` (needs `uv` and the cached headless shell). The test that reads the bounds of an element with a looping `opacity` in skipped content covers the crash above: while skipped, at the reveal before any frame, and after.
- Containment: taffy `containment` (a child margin through a plain and a contained block, a percentage child under a contained auto height in a block, a flex column and a flex row, a stretched and a growing item still filling the contained height); gpui `div` tests (margins of a layout-contained box, a fixed box placed and clipped by a contained ancestor, a positioned box between them skipped, a static contained box containing fixed and absolute boxes); gpuix `containing_block` and `content_visibility`.
- `handler-calls.test.tsx` covers `probeElementBounds`.

The content-visibility tests are also run with `GPUIX_RETAIN_VERIFY=1`.

Deviations from Chrome: [deviations.md](deviations.md#observers-and-content-visibility).
