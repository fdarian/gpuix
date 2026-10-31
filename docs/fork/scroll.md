# Scrolling

How a scroller works in the fork: the overflow rules, wheel routing and latching, scrollbars, the default scroll actions of the keyboard, `onScroll`, scroll writes and what they cost, virtual-list scrolling and its box, scroll offsets in device pixels, and scroll anchoring (`overflowAnchor`).

## The scrolling engine

Code: `packages/native/src/scroll.rs` and `scrollbar.rs`; zed `scroll_routing.rs`.

Scrollable overflow follows CSS. `overflow: auto` behaves like `scroll`, `hidden` can be scrolled only from code, `clip` does not scroll, and overflow never changes `display` or wraps children. A scroller's range is the scrollable overflow of all its descendants: every descendant that overflows counts, down to one that clips. An absolute descendant counts only for its containing block, and fixed, anchored and deferred layers never count.

These are readable: `scrollTop`, `scrollLeft`, `scrollWidth`, `scrollHeight`, `clientWidth`, `clientHeight`, `scrollTo` and `scrollBy`. `scrollIntoView({ block, inline, behavior })` goes through every ancestor scroller, and `scrollBehavior: smooth` animates writes. With no arguments `scrollIntoView()` aligns to the top. `overscrollBehavior` is honoured.

Scroll geometry asked of a renderer that lacks it, half of a `scrollTo` pair, and an unknown `behavior` or alignment string throw.

`scrollTo` on a scroller with no box (`display: none` on it or above) does nothing, instant or smooth, and `scrollTop` reads 0, because the DOM ignores a scroll position set on an element without a layout box. The scroller's handle is kept, so its offset is back when it shows again, but `GpuixView::scrollable_handle` and the `SCROLL_HANDLES` sync leave it out. Don't let a hidden scroller's handle back into that sync: poc jumped to the offset of a hidden scroller, and the first retained build animated it, because `SMOOTH_BY_DEFAULT` outlives the frames that do not build the scroller.

## Wheel routing and latching

One wheel event goes to the innermost scroller that can use any of it, and that scroller takes it whole. The rest of the delta is dropped: chaining to the outer scroller happens between gestures, not inside one event. A trackpad gesture, momentum included, stays latched to the scroller that started it. Virtual lists take part in the same routing, taking the wheel from and giving it to the surrounding scrollers between gestures.

The wheel itself is a separate event, `onWheel` (see [events-and-input.md](events-and-input.md)). Its delta sign is positive down, and the simulated wheel (`nativeSimulateScrollWheel`, `dispatchScrollWheel`, the automation `wheel`) takes the same sign. `nativeSimulateScrollWheel` without a phase starts a new gesture.

## Scrollbars

macOS-style scrollbars (drag, track paging) are on by default. Overlay bars are the default; with System Settings "Show scroll bars: Always" (which macOS also picks while a mouse is attached) the bars are classic: they take a 15 px gutter out of the container, so `clientWidth` and the content box shrink. The setting is read from the OS and refreshed on its notification.

`scrollbarWidth` is `"auto" | "thin" | "none"` and `scrollbarColor` is `"auto"` or `"<thumb> <track>"`.

**An `overflow: auto` scroller takes its classic gutter in the frame its content starts to scroll.** Whether an `auto` axis scrolls is known only after layout, in the overlay prepaint (`scrollbar.rs`). When the gutter it reserved is out of step with that (`settling`), the prepaint calls `Window::request_redraw_before_present` and the next pass of the same draw reserves it, as Blink lays out again when a bar appears (the relayout loop in `BlockNode::Layout`, `block_node.cc`). Only the presented pass publishes bounds, observers and `onLayout`. The loop is the one container queries use and shares its cap ([layout.md](layout.md#container-queries), `MAX_PASSES` 4): a draw that has used its passes presents with the old gutter and the next frame has the new one (`container::last_pass`). Two rules keep it finite and Chrome's:

- A gutter added in a draw stays for the rest of that draw (Blink freezes a bar that appeared during a layout, `block_node.cc:606`). Content that scrolls only without the bar and fits with it therefore keeps the bar, as in Chrome, and the passes of one draw cannot take it away again.
- A draw that ends with the gutter it began with resets the flip count. Without that, the cap across frames would end such content with no bar after two frames.

A frame that flips builds the scroller's tree twice (2.0 times the elements of a frame whose bar is already reserved); a frame with no flip builds once. A virtual list's classic bar overlays its rows instead of taking layout space (see [deviations.md](deviations.md#scroll)).

## Keyboard scrolling

Arrow, Page, Home, End and Space scroll the nearest scroller as a default action. `preventDefault` stops it. The built-in Select and Combobox do so; an app's own listbox has to as well, or the keys scroll the page under it. Space on a focused element with `onClick` activates it instead of scrolling.

## `onScroll`

`onScroll` fires on any offset change (wheel, scrollbar drag, keys, `scrollTo`, anchoring) and does not bubble. `scroll_reporter` reports nothing in a layout-only pass, so the event of a write runs in the frame after the one that laid it out, as in Chrome, and a write after a mutation fires once. A frame reports the offset it paints when that differs from the last painted one.

## Scroll writes (#70)

A scroll write must not make the whole window stale; a spring writing `scrollTop` every frame is the case it protects.

- **A write that does not move the scroller does nothing.** `scroll::scroll_to` and `scroll_element_to` return whether the scroller moved (or will, for a smooth scroll), and the callers invalidate only then. It still ends a smooth scroll in flight, as `ScrollableArea::InitiateScrollAnimation` does for a no-op offset, and a smooth scroll to where the scroller already is starts none. The layout the write needs when the tree is dirty still runs.
- **A `div` scroller notifies with `App::notify_scroll_only`.** The window keeps a second record, `scroll_metrics_current_at`, next to the layout's. A scroll write and a paint-only write keep it current, and `Window::draw_if_scroll_metrics_dirty` (used by `getScrollMetrics` and by the setter's layout before its clamp) runs no pass while it holds. A read of bounds uses `draw_if_dirty` and the layout record instead, so it lays out: the children moved with the offset. The passes behind these reads are in [forced-reads.md](forced-reads.md).
- **`scroll_to` lays out what is dirty, then clamps** with `scroll::scroll_to` to `ScrollHandle::max_offset` and writes. It used to rely on the next layout to clamp. A `scrollTo` that names both axes reads nothing back.
- **A `<virtual-list>` offset write is scroll-only too.** The list clamps it from its last layout. Before its first layout it keeps the offset, and the first layout holds it to the rows. The next frame builds the rows at the new offset.
- **The test renderer's `scrollTo` does not flush.** It used to flush (notify and draw) before and after, which made every scroll write a layout invalidation that only the test harness had.

A written offset has a range: every maximum offset of a list (scrollbar range, averaged range, wheel, write) is `div.rs`'s `max_scroll_offset` of the rows with their padding, and the fill test of `layout_items` rounds the same way, so no layout pulls back an offset the range allows.

## Scroll offsets are device pixels

`scrollTop = 10.5` used to read back 10.5 while a div painted it at 10 (`pixel_snap` rounds a tie toward zero) and a list kept the fraction and painted at 11. Chrome (`FractionalScrollOffsetsForWebAPI` off) snaps the offset to a device pixel, half up, before it clamps and stores it, and reads that back. The fork does the same, at the scale of the scroller itself:

- gpuix, `scroll.rs`: `scroll_to` (a div, from `scrollTop`, `scrollTo`, `scrollBy`, `scrollIntoView`, a key or the scrollbar) and `scroll_list_to` round the target half up to a device pixel of the scroller's own scale (`ScrollHandle::scale_factor`, `ListState::scale_factor`, kept from the last layout) before they clamp. `dom_position` rounds the other way for `scrollLeft`, `scrollTop`, `getScrollOffset` and `getScrollMetrics`, and so for a wheel or a trackpad that left the offset between two device pixels. A scroller that was not laid out has no scale and is read as stored.
- zed: a div hands its children `snap_scroll_offset` of its offset, so what is painted is what reads back (`pixel_snap` and the element offset are unchanged). `ListDomScroll::top` is the device pixel.
- The frames of a smooth scroll in between are fractions and read rounded.
- The range is `scrollHeight - clientHeight`, each rounded to a device pixel, which a div's `max_offset` already was (writes past the maximum of a list that rounds up stop at 61 where the layout's 61.8 would say 62).

Examples at 1x and 2x: 10.5 reads 11 at 1x; 10.4 and 10.25 read 10 at 1x and 10.5 at 2x (10.25 rounds up there); `scrollTo(12.5, 7.25)` reads 13, 7 at 1x and 12.5, 7.5 at 2x; `scrollBy(0, 0.5)` three times reads 1, 2, 3 at 1x, because it adds to the position as it reads back.

## Virtual lists

A `<virtual-list>` scrolls with the same routing and the same device-pixel rule as a div, with these differences in how it reads and builds.

**Reads.** `scrollTop` reads the list's offset, and the metrics come from the `ListState` (`ListState::dom_scroll`): `scrollTop`, the rows with their padding as `scrollHeight`, the list as laid out as the client size. The largest offset of a list is the rule of a `div` (`max_scroll_offset`), and a scroll value that is not a finite number is 0. A write is scroll-only (see Scroll writes).

**The box is the padding box.** A list used to take its border box for its client box: rows were placed at the origin plus the padding but not the border, laid out and clipped against the border box's height, and hit-tested through it, and `clientWidth`, `clientHeight`, the largest `scrollTop` and the end of `visibleRange` came from it. Now, as `ScrollExtent` in `div.rs` does for a div:

- `List::prepaint` takes the padding box (`padding_box`: the unsnapped and the snapped box less `layout_insets(..).border`) as the list's client box.
- Rows start at the unsnapped origin plus border plus padding, and are clipped and hit-tested at the snapped padding box (`ListPrepaintState::viewport`, what `overflow_mask` clips a div to).
- `ScrollClient` (the range, `fills_viewport`, `dom_scroll().client`) and `viewport_bounds` are the padding box, and the width a row gets is the padding box less the left and right padding.
- The `visibleRange` end is the bottom of the padding box (`scroll - padding.top + height`).

**Rows in the top padding are built.** Rows were built from the row the scroll offset is in, so a row that ends in the top padding of a scrolled list (the padding box starts `padding-top` above the scroll offset in the rows' coordinates) was not built, though Chrome paints it. The build walk starts at the row holding `max(0, scroll - padding.top)` (`first_visible_row`, no earlier than the scroll top's row, and only when the scroll top is less than the padding into its row). The rows above the scroll top that show in the top padding are built, painted and counted by `visibleRange`. `LayoutItemsResponse::first_row_offset` is how far the scroll top is below the first row built, and places the rows. A list that does not fill its viewport walks upward until the row above the first would be out of the top padding too (`rendered_height < available_height + padding.top`), so a bottom-aligned list at its end builds the row in its top padding. The overdraw is counted from the scroll top as before, so a list without top padding builds the rows it did.

**The rows are clipped, painted and hit through the rounded padding box.** `Style::padding_box_mask` (zed) is the clip of a div that hides its overflow on both axes: the padding box with its corners rounded by the border-box radii less the adjacent border width on each axis. `overflow_mask` builds its rounded case from it, and the list takes it for both the clip of its rows and their hit mask, so a row does not fill the corner of a list with a `border-radius`. The viewport, the scroll range and the row positions are the padding box's, unchanged.

Limits that matter when sizing a list (the 512 px overdraw, placeholder rows, the border ring) are in [deviations.md](deviations.md#scroll).

## Scroll anchoring (#82)

A scroller that was scrolled away from its start used to show other content when the layout above the view changed: a row prepended, a row above that grew or was removed, a `content-visibility: auto` entry above that took its real height when it was scrolled to. Chrome moves the offset by what moved the first box in view (CSS scroll anchoring). A list that loads older pages above the reader, or a feed virtualised by CSS, needs it. The design follows Chrome's `scroll_anchor.cc`.

**zed, `elements/scroll_anchoring.rs`.** `Style::overflow_anchor` (`OverflowAnchor { Auto, None }`, `Styled::overflow_anchor`).

- The anchor is the first box of the scroller's content, in layout order, that is fully in view, else the first partly in view with nothing better inside.
- Skipped as candidates: a box with `overflow-anchor: none` (and its subtree), a zero-size or `display: none` box, a box not in view, a fixed box, a sticky box with an inset, and an absolute box whose containing block is outside the scroller.
- After the layout the offset moves by `round(new block start) - round(saved block start)`, clamped, and the saved start is not measured again.
- The anchor is dropped when the offset differs from the one the step left, when the anchor left the scroller, and when the scroller is at offset 0.
- The taffy engine flags a node whose insets, margin, padding, size, min/max size, position or `contain-intrinsic-size` changed in the pass. A flagged node between the anchor and the scroller, with a nonzero delta, drops the anchor without scrolling.
- It runs in layout-only passes too, so a forced read sees the new offset.

**The pick is before the layout.** `Interactivity::request_layout` notes a tracked scroller in the engine, and `compute_layout_in` calls `ScrollAnchoring::before_layout` for each one before it computes the layout, while the nodes hold the boxes of the pass before and the tree and styles of this one (Blink's `NotifyBeforeLayout`). Don't move the pick to the end of the previous prepaint: a scroll in the frame of a prepend (every wheel frame), a removed or hidden anchor, a `none` set on the anchor, and a scroller turned on in the frame of the change would then compensate nothing. A node created in the pass has no box and is not picked.

**gpuix.** `overflowAnchor: "auto" | "none"` (`css/overflow_anchor.rs`; an invalid value is ignored; not inherited, not animatable; a state style overlays it) is given to every box by `layout::style::apply`, since a descendant needs it to be left out as a candidate.

Known gaps against Chrome are in [deviations.md](deviations.md#scroll).

## Transforms

A transformed box inside a scroller changes what `scrollIntoView` aligns (through every scroller around it, each in its own space), how far the scroller scrolls, and where an autoscroll's edge zone is. A patched tick of an animated transform moves the range too, and `scrollWidth` and `scrollHeight` follow it. Under a `perspective` a box counts where its flattened matrix puts it; a `preserve-3d` chain composes level by level. Those rules are in [paint-and-transforms.md](paint-and-transforms.md#geometry); what the patch still declines is in the [deviations](deviations.md#paint-and-transforms).

## Breaking changes

For an app written against upstream gpuix:

- A static `overflow` ancestor no longer clips or scrolls an absolute box. Give the ancestor a `position` to clip it.
- `onScroll` is now the offset-change event, the wheel handler is `onWheel`, and `onScrollChange` is removed.
- The wheel delta sign flipped: positive is down. The simulated wheel takes that sign too, and without a phase it starts a new gesture.
- `overflowX: "scroll"` no longer forces flex and `min-width: 0` or wraps children. Children need an explicit width or `flexShrink: 0` to overflow. The vertical-wheel-onto-x remap is gone, and a horizontal-only scroller no longer sizes its children automatically.
- `scrollIntoView()` with no arguments aligns to the top.
- Arrow, Page, Home/End and Space scroll the nearest scroller unless `preventDefault`ed. App listboxes must `preventDefault` them (see Keyboard scrolling).
- Scrollbars show by default. With the classic style they take layout space, so `clientWidth` and the content box shrink.
- A wheel event no longer chains to the outer scroller in the same event. A test that expects the outer scroller to move sends a second gesture (`"started"` phase).
- A scroller's range counts every descendant that overflows, not only direct children.
- Scroll geometry on a renderer that lacks it, half of a `scrollTo` pair, and an unknown `behavior` or alignment string throw.
- `scrollTo` on a scroller with no box does nothing, and a smooth `scrollBehavior` no longer animates it.
- Scroll offsets read back as device pixels: `scrollTop = 10.5` reads 11 at 1x.

## Tests

- `scroll-write-layout.test.tsx`: a spring's `change` handler writing `scrollTop` for 30 frames runs no layout-only pass and at most one layout a frame; 50 writes with nothing dirty run none and clamp to the maximum; a write after a style change in the same task lays out once and clamps to the new extent; a bounds read after a write sees the scrolled position; a layout pass between frames fires no `change` event, and the box it lays out has the last frame's width. gpui `test_a_scroll_write_only_makes_the_boxes_stale`.
- `scroll-anchoring.test.tsx`: 32 scenes of `fixtures/scroll-anchoring-scenes.ts` against headless Chrome (`fixtures/scroll-anchoring-oracle.*`; regenerate with `bun fixtures/scroll-anchoring-oracle.ts [scene...]`). Each step is compared on `scrollTop`, the first row in view with its top, and the frames the `scroll` events run in. Scenes: rows inserted above, inside, below and at the end; resized, collapsed (`height: 0`, `display: none`) and removed above; a partly visible anchor; fractional heights (50.4 to 50.6 is a pixel); margin, top, height and padding of the anchor, its parent and the scroller; `none` on the scroller, the first row and a parent, and toggled in the frame of a change; offset 0; a write before and after the mutation in one task; the end of the range and a clamp; a removed, a hidden and a vanished view; forced reads; content-visibility lists of 60 entries scrolled to the end and back in steps of 150, 60 and 400 px. zed `scroll_anchoring` (18 tests); `css::overflow_anchor` and the state-style overlay in gpuix.
- `scroll-fraction-oracle` (10 scenes by 2 scales: `scrollTop`, `scrollLeft`, `scrollTo(x, y)` and `scrollBy` with fractions, negative writes, writes past a range whose ends are fractions, a bordered and padded list) and `scroll-fraction.test.tsx`, which renders every scene as a div, and as a `<virtual-list>` when it scrolls along y only. It compares `scrollLeft`, `scrollTop`, `getScrollOffset`, the box of the list and the boxes of its rows after each operation, and the device-pixel rows that rows with backgrounds paint (scenes without a border or padding). gpui `a_scroller_between_device_pixels_paints_at_the_position_that_reads_back`, `test_bounds_for_item_are_where_the_rows_are_placed`; native `scroll.rs` `a_scroll_position_is_rounded_half_up_to_a_device_pixel`, `a_written_offset_is_the_one_that_reads_back`.
- `virtual-list-oracle`: 25 scenes by 2 scales: the padding box of each, the rows of the bordered ones, and the 5 rounded ones, whose probes at 0.85 and 1.1 times each corner ellipse of the padding box are compared with what Chrome paints and hits. The row scenes are `bordered with text rows`, `borders of each side`, `a fractional border` (1.5 px: 1 device px at 1x, 3 at 2x), `a row in the top padding`, `... with a fractional padding`, `... of text rows`, `a bordered row ...` and `... below a border of each side`.
- `scrollbar-gutter-oracle` (headless Chrome's `clientWidth`, `clientHeight`, scroll size and rects after one change, with `::-webkit-scrollbar` bars; regenerate with `sh fixtures/scrollbar-gutter-oracle.sh`) and `scrollbar-gutter-frame.test.tsx`: what one `drawFrame` after the change leaves, and the frames after it, for growth down, across and both ways, content that fits only with the bar, a scroller below the fold of a clip, and a nested pair; the observers (a `ResizeObserver` on the child, an `IntersectionObserver` rooted at the scroller, frame for frame as `content-visibility-observers.test.tsx` `io8b2`); elements built by a flip frame (2x) and by a frame with no flip (1x); an unchanged component whose ancestor's change makes its scroller scroll.
- `virtual-list-rows.test.tsx` compares `clientWidth`, `clientHeight`, `scrollHeight` and every row with a device pixel in view in the padding box, and the device-pixel rows that rows with backgrounds paint inside the padding box. gpui `list.rs`: `test_a_list_scrolls_in_its_padding_box` (client box, range, viewport, row origin and width with an asymmetric border at 1x and 2x), `test_a_row_in_the_top_padding_is_built`, `test_a_row_above_the_padding_box_is_not_built`, `test_a_bottom_aligned_list_builds_the_row_in_its_top_padding` and `test_the_visible_range_starts_in_the_top_padding`. The rounded scenes of `virtual-list-oracle` (5 by 2 scales: a radius, with a border, with a border and padding, elliptical percent radii, a radius for each corner) are read at points just inside (0.85) and outside (1.1) of each corner ellipse by `document.elementFromPoint` and by sampling the pixels under a pointer; gpui `a_list_clips_and_hits_its_rows_through_its_rounded_corners` and `a_bordered_list_rounds_the_clip_of_its_rows_by_the_radius_less_the_border` (the painted clip radius and the hit chain at the corners).

Deviations from Chrome in this area: [deviations.md](deviations.md#scroll).
