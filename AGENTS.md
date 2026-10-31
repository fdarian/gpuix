# AGENTS.md - Alpina Codebase Guide

Alpina builds desktop apps with TypeScript on [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui), Zed's GPU-accelerated UI framework: React or Solid components render into a Rust retained tree that GPUI draws on the GPU. It is `fdarian/alpina`, a fork of `remorses/gpuix`. Alpina is optimized for macOS and Solid; React and other platforms are not widely tested, so do not report them as working.

```
Solid / React (TypeScript)  ->  napi-rs  ->  GPUI (Rust, zed/ submodule)  ->  Metal
```

Read [README.md](./README.md), then [docs/fork/index.md](./docs/fork/index.md): what the fork changes, the principle behind it, and the map of the topic docs. Where `docs/fork/*` and this file disagree, the fork docs win.

## Docs

- [docs/reference.md](./docs/reference.md) - the API contract: every element, prop, event, renderer option, method and behaviour an app author sees. **Document every user-facing change there in the same change**; a changeset does not replace it.
- [docs/fork/index.md](./docs/fork/index.md) - overview of what changed from gpuix, one topic doc per mechanism (events, scroll, layout, text, paint, motion, retention, patch frames, forced reads, observers, webview, nativeview, window), and "Where notes go": behaviour and design go into the topic doc, merged into the section about that mechanism, with no round narration.
- [docs/fork/working-on-the-fork.md](./docs/fork/working-on-the-fork.md) - repos and branches, commit rules, where new code goes, the invariants a cherry-pick has to keep, building. Read before touching `renderer.rs`, `style.rs`, `host.ts` or `zed/`.
- [docs/fork/testing.md](./docs/fork/testing.md) - test commands, Chrome oracles, the test renderer, perf measurement, driving a live window.
- [docs/fork/syncing-upstream.md](./docs/fork/syncing-upstream.md) - the ledger of what is reviewed from `remorses/gpuix` and `remorses/zed`, and the take/adapt/skip procedure.
- [docs/solid.md](./docs/solid.md) - the Solid adapter guide. [docs/serialization-benchmark.md](./docs/serialization-benchmark.md) - why the mutation batch is shaped as it is.

## Rules

- **Behave like the DOM.** Events, focus, scrolling, layout, paint and motion follow DOM and CSS semantics under the browser's names (`onPointerDown`, `tabIndex`, `position: sticky`). No gpuix-only rule, no DOM behaviour hidden behind an opt-in, and no compatibility shim: breaking changes are taken. A deviation is acceptable only where GPUI cannot build the DOM model; it goes in [deviations.md](./docs/fork/deviations.md), the single list, and is linked from `docs/reference.md` at the feature.
- **New code goes in new modules** (`events/`, `layout/`, `css/`, `motion/`, `scroll.rs`, `js/events/`). `renderer.rs`, `style.rs` and `js/host.ts` get wiring only, because they conflict on every upstream pick.
- **Built-in components follow [Base UI](https://base-ui.com/react/components/select)** (`select`, `combobox`, `tooltip` and any new primitive in `@gpuix/react` and `@gpuix/solid`): the same split between Root data and children. `items` on `Select` Root is only a label lookup for `SelectValue` while the popup is closed; keyboard navigation and clicks read the mounted `SelectItem` children. Do not walk `child.type`, and do not require `items` for the menu to work.
- **Framework-neutral TypeScript belongs in `@gpuix/native`,** not in an adapter. React and Solid keep only scheduling, context, JSX integration and components; never copy shared runtime code back into one.
- **macOS only.** Windows, Linux and wasm paths exist but were not compiled by the people who wrote them; the gaps are in [deviations.md](./docs/fork/deviations.md#other-platforms). Do not spend work on them.

## Alpina is a thin layer on GPUI

**Read the GPUI source before you write native code.** `zed/crates/gpui` is checked out here, and `gpui::div`, `gpui::Window` and `gpui::ListState` are the real API; Alpina translates a tree into calls on them. The `zed/` submodule is not upstream GPUI (it carries DOM hit testing, scroll routing, sticky, containing blocks, grid templates, effect groups, `draw_if_dirty`, a persistent layout engine, retained subtrees, culling), so GPUI docs and `zed-industries/zed` issues describe something close but not identical. The source in `zed/crates/gpui` is the truth.

Do not invent behaviour on top of GPUI. When an element needs something GPUI does not do, in order:

1. Find the GPUI API that already does it (search `zed/crates/gpui` for the symbol).
2. Search `zed-industries/zed` issues and PRs, before you debug GPUI, write a feature that needs a new GPUI API, or patch the fork. The answer is often a bug already reported, an open PR (wait, then bump), a merged PR (bump the submodule), or a closed issue the Zed team declined (plan a fork-only fix). Use the `--repo` and `--match` flags: `gh search issues --repo zed-industries/zed --include-prs --limit 30 'TransformationMatrix'`, `--match title` to find the feature rather than every mention, `gh search code --repo zed-industries/zed --language Rust ...`. `repo:` or `in:title` inside the query string fails or silently drops the repo filter. Search real symbol names, not concepts. Record the outcome (issue and PR URLs) in the PR body so nobody repeats the search.
3. Fix it in the `zed/` submodule as an ordinary GPUI change (see Repos), and bump the gitlink. Make a change that alters behaviour for every user opt-in per window, the way DOM hit testing is (`Window::set_dom_hit_testing`, default off), which keeps it small to review and possible to send upstream.
4. Only then add Alpina code.

**Never paper over GPUI in `packages/native`.** A workaround that re-applies state after GPUI computed it, patches a value GPUI owns, or reaches around a GPUI invariant breaks on the next submodule bump and is very hard to debug. When one is unavoidable, a comment states what GPUI does, why that is not what Alpina needs, and which GPUI call makes it safe. Prefer the smallest translation.

A "PR to Zed" means upstream [`zed-industries/zed`](https://github.com/zed-industries/zed) `main`, which is not a fork fix. Never open one from this checkout or switch `zed/` to another branch for it: `zed/` is what the addon builds against. Make a separate worktree from `zed-industries/zed` `main` (the zed clone's `upstream` remote is `remorses/zed`, so add upstream Zed as its own remote), commit there, and cherry-pick onto `poc` after the PR merges.

## Repos and tooling

- **Branches.** `fdarian/alpina` has `main` as its default branch; `zed/` is the submodule `fdarian/zed` on `poc`. Both are **append-only**: no rebasing, amending or force-pushing once pushed, because `main` pins zed SHAs by gitlink. `.gitmodules` stays `fdarian/zed` on `poc`. Keep `zed/` on local `poc`, never detached (switch back after `git submodule update` or a pointer update); a fresh worktree gets it with `git submodule update --init zed`. Land a zed change on `poc` and push it first, then `git add zed` in the commit that needs it; never pin a commit that is not on `fdarian/zed`. Details: [working-on-the-fork.md](./docs/fork/working-on-the-fork.md#repos-and-branches).
- **pnpm installs, Bun runs.** Dependencies come from `pnpm install`; scripts, tests and apps run with Bun (`bun run`, `bun test`, `bun --hot`). Run a package's suite with `bun run test`: react, native and examples are vitest, so a bare `bun test` is the wrong runner there ([testing.md](./docs/fork/testing.md#test-commands) has the commands).
- **Build.** `bun run build` in `packages/native` builds the addon; after a change in `packages/react`, `bun run build` there too (`examples/` load its `dist`). **Finish with a release build:** scratch apps link this checkout, and a leftover debug addon looks like lag. No hot reload for the native half, so rebuild and restart. Fresh machines need the Metal compiler once: `xcodebuild -downloadComponent MetalToolchain`.
- **Rust toolchain.** `rust-toolchain.toml` must match `zed/rust-toolchain.toml`; update it when the submodule moves or GPUI may not compile.
- **Generated files.** `packages/native/index.js`, `index.cjs` and `index.d.ts` are written by `napi build` and tracked; the `.node` is not. Edit the Rust `#[napi]` item and rebuild, never the output. `index.js` is ESM, `index.cjs` exists for the Hermes CJS runtime, and both loaders sit beside the `.node`.
- **Solid apps** use `jsx: "preserve"`, `jsxImportSource: "@gpuix/solid"` and the `@gpuix/solid/preload` in `bunfig.toml`; do not add Vite or a wrapper CLI ([docs/solid.md](./docs/solid.md)).
- **Releases.** A user-visible change adds a changeset with `pnpm changeset`; merging the `Version Packages` PR publishes.
- **Syncing with upstream** is by commit, see [syncing-upstream.md](./docs/fork/syncing-upstream.md); do not rebase or merge an upstream branch.

## Where the code is

A change reaches the screen like this: the adapter queues host mutations, `applyBatch` (`BatchOp` in `renderer.rs`) applies them to the Rust `RetainedTree` (`retained_tree.rs`), and every GPUI frame `GpuixView::render` walks that tree through `build_element` into GPUI elements. No element tree is serialized from JS. Events return as `EventPayload` (`element_tree.rs`), the one napi struct for every event type. [docs/reference.md](./docs/reference.md#architecture) has the picture. `build_element` resolves a style in a fixed order that [layout.md](./docs/fork/layout.md) lists; read it before adding a style stage.

| Area | `packages/native/src/` | `packages/native/js/` | `zed/crates/gpui/src/` |
|---|---|---|---|
| Pointer events, hover, drag | `events/` (`dispatcher`, `listeners`, `state`, `sync_dispatch`, `element_drag`, `file_drag`, `pointer_lock`) | `events/` (`dispatch`, `event-props`, `drag`) | `window.rs` (`Hitbox`, `dom_hit_test`) |
| Keyboard, focus, forms, a11y | `events/keyboard.rs`, `focus.rs`, `forms.rs`, `accessibility*` | `events/keyboard-defaults.ts`, `focus.ts`, `forms.ts`, `aria.ts` | `_accessibility.rs` |
| Scrolling | `scroll.rs`, `scrollbar.rs` | `scroll.ts` | `scroll_routing.rs`, `elements/div.rs`, `elements/list.rs`, `sticky.rs` |
| Layout | `layout/`, `custom_elements/anchored.rs` | | `taffy.rs`, `taffy/`, `third_party/taffy` (persistent engine), `elements/anchored.rs` |
| Paint and style | `style.rs`, `css/`, `color.rs`, `text/inline.rs`, `custom_elements/svg_*.rs` | `svg.ts` | `effects.rs`, `scene.rs`, `style.rs`, `crates/gpui_apple` |
| Native views | `custom_elements/webview/`, `platform/webview_macos.rs` | `webview.ts` | `composition.rs`, `composition/`, `crates/gpui_macos/src/window/composition.rs` |
| Motion | `motion/` | `motion-*.ts`, `keyframes.ts`, `layout-animation.ts`, `match-media.ts` | |
| Text, selection, search | `text/`, `syntax/`, `markdown/`, `diff/` | `text-selection.ts` | `elements/atomic_inline.rs`, `text_system/inline.rs` |
| Bounds, observers, automation | `automation.rs`, `observers.rs`, `resolved_style.rs` | `bounds.ts`, `observers.ts`, `automation/` | `window.rs` (`draw_if_dirty`, `publish_layout`) |
| Skipped subtrees, culling | `retain.rs`, `tape.rs`, `layout/cull.rs`, `perf_counters.rs` | `testing.ts` (counters and switches) | `elements/retained.rs`, `window/retained.rs` |

Adapters: `packages/react/src` (`reconciler/`, `components/`, `hooks/`) and `packages/solid/src` (`host.ts`, `primitives.ts`, `components/`). Also `examples/` (demos, perf guards and regression tests), `hermes/`, `docs/`.

## Gotchas

- **Bounds are recorded in paint,** never prepaint, and anything new a read can observe is published by `publish_layout` in a layout-only pass: [layout.md](./docs/fork/layout.md#reading-boxes-back-45), [forced-reads.md](./docs/fork/forced-reads.md#publication).
- **Retained subtrees replay what did not change.** A new build input, color read or style field has to reach the invalidation rules, or a replay shows a stale frame: [retention.md](./docs/fork/retention.md), and the retention verifier in [testing.md](./docs/fork/testing.md#the-retention-verifier).
- **The desktop pump** (`@gpuix/native/runtime`) wakes on main-run-loop work and display-link demand, so an idle window costs nothing. `tick()` must stay non-blocking so Node and Bun I/O run: [frames-and-wakes.md](./docs/fork/frames-and-wakes.md).
- **Every element needs a host-derived GPUI id,** or hover, scroll offset and the accessibility node silently vanish: [events-and-input.md](./docs/fork/events-and-input.md#the-dispatcher).
- **A macOS menu item owns its shortcut.** Never add an Edit menu carrying `⌘C/V/X/A`: [window.md](./docs/fork/window.md#the-macos-menu-bar).
- **Layout numbers** live in `Theme::metrics`, not in Rust constants: [working-on-the-fork.md](./docs/fork/working-on-the-fork.md#where-new-code-goes).
- **`<virtual-list>`** builds rows on `cx.processor` re-entry; never capture the root render's guard: [scroll.md](./docs/fork/scroll.md#virtual-lists).
- **Hot reload.** `bun --hot` remounts React and drops `useState`; Fast Refresh runs only through `scripts/web.ts` in the browser. Never run `bun run clean` in `packages/react` while that server is up.
- **Profile a release addon** with a script, not the live window: [testing.md](./docs/fork/testing.md#measuring-performance).
- **Checking your own work in a live window:** pass `focus: false`, drive it through the automation API (never `usecomputer`, `screencapture` or desktop clicks), and for "open it so I can look" leave a `bun --hot` session running: [testing.md](./docs/fork/testing.md#driving-a-live-window).
- **Ported code** (`text/`, `syntax/`, `markdown/`, `diff/`, `theme.rs`, `code.rs`, `diff.rs`) comes from Comet, see `THIRD_PARTY_NOTICES.md`; read the original before fixing one.
