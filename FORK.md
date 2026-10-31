# fdarian/gpuix, a proof-of-concept fork

This is a long-lived proof of concept of what [remorses/gpuix](https://github.com/remorses/gpuix) could be if scrolling and input latency on macOS were a goal, and if its elements behaved like the web DOM. It is not published to npm and nothing here is a request to merge. It is rebased on upstream regularly, so commit hashes change; [docs/fork/working-on-the-fork.md](docs/fork/working-on-the-fork.md) has the procedure.

The fork has no users, so breaking changes are taken and nothing is shimmed behind a compatibility flag. It is macOS-only: the Windows, Linux and wasm paths exist but were not compiled by the people who wrote them, and the gaps are recorded in [docs/fork/deviations.md](docs/fork/deviations.md#other-platforms).

It started as a demo that renders a large syntax-highlighted diff with `@gpuix/solid` and scrolled visibly worse than Zed itself. Porting a component library built for the web then showed that the remaining gaps were not about speed: hover, focus, scrolling, clipping and animation behaved differently from the DOM, and each component had to work around it. The fork closes those gaps and makes frames cheap enough for large pages.

## The principle: DOM and CSS semantics

The fork follows one rule: **behave like the web DOM and CSS.** Anything a web developer can do works the same way here, with DOM semantics and DOM-shaped names (`onPointerDown`, `tabIndex`, `scrollIntoView`, `position: sticky`). The work is not allowed to add gpuix-only behaviour, or to keep the old default and put DOM behaviour behind an opt-in. A deviation is accepted only where the DOM model cannot be built on GPUI, and each one is listed in [docs/fork/deviations.md](docs/fork/deviations.md) with its reason.

Why this and not a compatibility layer: the fork's consumer is a port of web components. Every place gpuix differed from the DOM (an element that blocks hover on its parent, a `scroll` event that meant wheel, a `position` that defaulted to `absolute`) became a workaround in the component, and the workarounds made the port expensive. Upstream tests that asserted the non-DOM behaviour were rewritten to the DOM behaviour instead of being kept alive.

Consequences that apply to every area:

- **Breaking changes are taken, not shimmed.** Each topic doc lists its breaking changes under "Breaking changes", so an app author can find what to edit.
- **New code lives in new modules** (`packages/native/src/events/`, `scroll.rs`, `layout/`, `css/`, `motion/`, `packages/native/js/events/` and so on). `renderer.rs`, `style.rs` and `host.ts` get wiring only, which keeps the rebase onto upstream tractable.
- **The README is the API contract**, as upstream's `AGENTS.md` requires: it documents the behaviour an app author sees. The docs here record why it is that way.
- **macOS is the platform that was exercised.** Several features have a macOS-only path (`<webview>`, Metal effects, pointer lock, the synchronous dispatch that lets `preventDefault` cancel a native default, the OS reduced-motion read), and the shader edits for other platforms were never built or rendered. Each gap, with what closing it would take, is in [Other platforms](docs/fork/deviations.md#other-platforms).

## What the fork changes

Patches 1 and 2 fix two of the three causes of the slow scrolling. Upstream landed the fix for the third (hidden bounds canvases, [remorses/gpuix `d8cf51a`](https://github.com/remorses/gpuix/commit/d8cf51a), `track_host_bounds`), so the fork's own on-demand bounds patch was dropped in favour of it.

| Area | What it does | Doc |
|---|---|---|
| Patch 1: no inline draws in real windows | Wheel events and `applyBatch`/`scrollTo` stop paying for a full synchronous draw (zed `test-support`). | [frames-and-wakes.md](docs/fork/frames-and-wakes.md) |
| Patch 2: display-link frame pump | GPUI is serviced by the display, not by an 8 ms JS timer. JS is woken only when the main run loop has work, input wakes an idle window, and an idle or non-key window costs no wakes and is not throttled. | [frames-and-wakes.md](docs/fork/frames-and-wakes.md) |
| Patch 3: spring transitions | `motion` gets springs and carries velocity across a retarget. | [motion.md](docs/fork/motion.md) |
| Events and input | One DOM hit test, capture/target/bubble dispatch, pointer capture, click family, drag and drop, pointer lock; DOM focus order and `tabIndex`, DOM keyboard events, form controls, `aria-*`, `inert`, clipboard, file picker, `EyeDropper`; group and relational (`:has()`) state styles; inline span listeners, cursor and focus. | [events-and-input.md](docs/fork/events-and-input.md) |
| Layout | Never-stale bounds, CSS positioning and `zIndex`, grid, tables, atomic inlines, container queries, the box shorthands (`padding: "0 8px"`, `flex`, `place*`), floating-ui placement; linear nested-column layout, a layout engine that persists across frames, relayout boundaries, `content-visibility` layout containment, text and bounds in 1/64 px. | [layout.md](docs/fork/layout.md) |
| Text | Inline text and spans, white space, line boxes (`line-height: normal` over the fonts a line falls back to), wrapping where Chrome's breaker breaks (integer advance sums, line ends shaped again, soft hyphens), `textWrap` and `textBox`, decorations, font variations, smoothing and features, Chrome's fallback fonts, advances and CJK punctuation spacing, glyph positions, text that is not measured again for a colour. | [text.md](docs/fork/text.md) |
| Paint and transforms | Rounded clipping, gradients, filters, masks, `clipPath`, group opacity, CSS colours, full-colour `<svg>`, outlines with a style; CSS `transform`, `translate`, `rotate`, `scale` and `transformOrigin` in two and three dimensions (`perspective`, `perspectiveOrigin`, `transformStyle`, `backfaceVisibility`), drawn through every primitive and hit and measured through the matrix. | [paint-and-transforms.md](docs/fork/paint-and-transforms.md) |
| Scroll | Nested scrolling with scrollbars, wheel routing and latching, `onScroll`, virtual lists, scroll offsets in device pixels, scroll anchoring (`overflowAnchor`). | [scroll.md](docs/fork/scroll.md) |
| Motion | Colour and per-property transitions, keyframes and transitions (SVG properties included), CSS `animation`, motion values with Motion's controls, layout animations, Web Animations (`getAnimations()` with playback, effects, subtree and document lists, and Motion's own animations), `matchMedia`, reduced motion; Motion's transform keys (depth keys included) and CSS transitions and animations of the transform and perspective properties. | [motion.md](docs/fork/motion.md) |
| Retention and patch frames | A frame replays the subtrees that did not change (and moves them with a scroll) instead of building them, content clipped away is culled, and a tick of paint-only animations repaints only the roots it changes. Animations nobody can see cost nothing. | [retention.md](docs/fork/retention.md), [patch-frames.md](docs/fork/patch-frames.md) |
| Forced reads | A bounds or scroll read after a change costs the dirty chain: layout-only passes that do not draw, paint-only writes that lay nothing out, reads that share a pass. | [forced-reads.md](docs/fork/forced-reads.md) |
| Observers and `content-visibility` | `onLayout`, `ResizeObserver` and `IntersectionObserver` with Chrome's report order; `content-visibility` with containment, accessibility nodes, Tab, find and first-frame events for skipped content. | [observers-and-content-visibility.md](docs/fork/observers-and-content-visibility.md) |
| `<webview>` | A web page in a native view composed into the window between GPUI layers: a placeholder in the scene cuts the frame into GPUI and native layers at its draw order, so popovers, scroll containers and opacity work around it. Electron's `<webview>` API on a system `WKWebView`, focus and Tab in and out, macOS only (zed `composition`). | [webview.md](docs/fork/webview.md) |
| `<nativeview>` | A `div` with an AppKit view from another library in it, between GPUI layers: a pointer (`onAttach` / `onDetach`) is the whole boundary, so the library links no GPUI. Clipped by CSS `overflow`, GPUI keeps the pointer unless `nativeEvents`, macOS only (zed `composition`). | [nativeview.md](docs/fork/nativeview.md) |
| Window features | Following the macOS appearance, a `fonts` window option, `appRegion` drag regions for frameless windows, `x` / `y` window placement with Electron's `screen` displays. | [window.md](docs/fork/window.md) |

## Documentation map

| File | Holds |
|---|---|
| [README.md](README.md) | The API contract: the behaviour an app author sees. |
| [docs/fork/working-on-the-fork.md](docs/fork/working-on-the-fork.md) | Repos and branches, the commit-stack rules, syncing upstream, resolving conflicts (the re-apply checklist), building, where new code goes, platforms and CI. |
| [docs/fork/frames-and-wakes.md](docs/fork/frames-and-wakes.md) | Patches 1 and 2, how JS is woken (run-loop watch, input observer), windows that are not key, idle cost and memory footprint. |
| [docs/fork/window.md](docs/fork/window.md) | Appearance, the `fonts` option, `appRegion` drag regions, window placement and `screen` displays. |
| [docs/fork/events-and-input.md](docs/fork/events-and-input.md) | Pointer events, focus, keyboard, forms, accessibility, platform APIs, state styles from pointer and focus, inline span listeners. |
| [docs/fork/layout.md](docs/fork/layout.md) | Positioning, stacking, grid, tables, container queries, shorthands, lengths, intrinsic widths, flex rows sized by their items, `flow-root`, the persistent layout engine and its taffy cache, relayout boundaries, layout containment, fractional units, baselines. |
| [docs/fork/text.md](docs/fork/text.md) | Inline text and spans, atomic inlines, white space, line boxes, wrapping and line breaking (the fit test, soft hyphens, overflowing rows), text-box trim, decorations, fonts (fallback advances, script runs, `text-spacing-trim`), glyph positions, text measure keys. |
| [docs/fork/paint-and-transforms.md](docs/fork/paint-and-transforms.md) | Clipping and borders, backgrounds, effect groups and opacity, colours, `<svg>`/`<img>`, masks, `clip-path`, outline, quad cost; CSS transforms, 3D included. |
| [docs/fork/scroll.md](docs/fork/scroll.md) | The scrolling engine, wheel routing, scrollbars, `onScroll`, scroll writes, virtual lists, scroll anchoring. |
| [docs/fork/motion.md](docs/fork/motion.md) | Springs, transitions, keyframes, CSS animations, motion values, layout animations, Web Animations objects, `matchMedia`, reduced motion. |
| [docs/fork/retention.md](docs/fork/retention.md) | The three layers that skip what did not change, replay, holes and fragments, relocation under scroll, keys and eligibility, tapes, invalidation rules, culling, where a frame differs from a full build. |
| [docs/fork/patch-frames.md](docs/fork/patch-frames.md) | Animation ticks: patch frames and units, what a tick may change, dormant animations and their wake times, the cases that draw whole. |
| [docs/fork/forced-reads.md](docs/fork/forced-reads.md) | Layout-only passes for bounds and scroll reads, reads inside dispatch, what a forced read costs, reads inside skipped content. |
| [docs/fork/observers-and-content-visibility.md](docs/fork/observers-and-content-visibility.md) | `onLayout`, `ResizeObserver`, `IntersectionObserver`, report order; `content-visibility` and what it does to events, accessibility, Tab and find. |
| [docs/fork/webview.md](docs/fork/webview.md) | `<webview>` and native window composition: the placeholder and layer cut, geometry, input and focus handoff, the wake timer, the zed modules and how they differ from zed PR #62379, tests. |
| [docs/fork/nativeview.md](docs/fork/nativeview.md) | `<nativeview>` for library authors: `onAttach` / `onDetach` and the lifetime rule, a minimal library, geometry, `overflow` clipping, pointer and focus, nested views, tests. |
| [docs/fork/testing.md](docs/fork/testing.md) | Test commands, Chrome oracles, the test renderer, the `<webview>` e2e, switches and counters, the test clock, the retention verifier, fuzzers, perf measurement and known failing budgets. |
| [docs/fork/deviations.md](docs/fork/deviations.md) | Every known deviation from Chrome, the DOM, Electron and Motion, one section per topic, plus the other-platform gaps. |

## Where notes go

FORK.md and `docs/fork/` belong to the fork commit, the last one in the stack (see [working-on-the-fork.md](docs/fork/working-on-the-fork.md)). The docs hold the why; the README holds the behaviour.

- **Behaviour, design, why and warnings** go in the topic doc a reader of that area opens, merged into the section about that mechanism. Do not append a new dated section.
- **Deviations** from Chrome, the DOM, Electron or Motion go only in [deviations.md](docs/fork/deviations.md), under the topic. The topic docs link there and do not repeat the list.
- **Breaking changes** go in the topic's "Breaking changes" section, **tests** in its "Tests" section.
- **Repo, sync and conflict notes** go in [working-on-the-fork.md](docs/fork/working-on-the-fork.md).
- **No round narration** and no "Round N" headings: history lives in git and the review log. An issue number can stay once as a tag on a heading or a first sentence.
- **A new topic file** only when an area fits none of the above; add it to the map here.