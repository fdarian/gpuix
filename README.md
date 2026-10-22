> **This is fdarian's proof-of-concept fork of [remorses/gpuix](https://github.com/remorses/gpuix).**
> It lowers scroll and input latency (a guard so `test-support` builds do not draw inline, a display-link frame pump), adds spring transitions, and makes events, focus, layout, scrolling, paint and motion follow DOM/CSS semantics.
> It is rebased on upstream regularly and is not published. See [FORK.md](./FORK.md).

# GPUIX

**React and Solid** for [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui), Zed's GPU UI framework.

Write a React or Solid tree in TypeScript. GPUIX paints it with Metal, DirectX, or Vulkan. No Electron. No web view.

`useState` and JSX still apply. Layout, text, and input go through GPUI, not the DOM.

![A glass dashboard over a painted sky](./docs/images/mail-app.jpg)

Everything above is GPUIX: the glass window, the cards, the schedule,
and native text.

## Quickstart

Create an app from the official example. The command downloads only
`example-app/` and installs its dependencies. There is no repository clone,
native build, or Rust toolchain.

```bash
bunx @gpuix/cli new my-app
cd my-app
bun run dev
```

`@gpuix/react` pulls the native renderer for your platform. Edit `app.tsx` and
the running window remounts on save. Click and keyboard handlers switch to the
new tree without recreating the window.

Using Solid 1 instead? Start with the [Solid quick start](#solid-quick-start).

### Build from scratch

Install the packages directly when you do not want the example app:

```bash
bun add --exact @gpuix/react @gpuix/native react
bun add -d @types/react typescript
```

**Pin your adapter and `@gpuix/native` to the same exact version.** GPUIX is
still pre-1.0, so a new release can break either package. The adapter pulls
`@gpuix/native` with a version range, and that range can install a newer native
binary under an older adapter. Add both as direct dependencies. Upgrade them
together.

### 1. Point TypeScript at the GPUIX JSX types

**`jsxImportSource` is required.** Without it TypeScript uses DOM types, so
`<virtual-list>`, `<markdown>`, `<code>` and `style.hover` all fail to
typecheck.

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "jsx": "react-jsx",
    "jsxImportSource": "@gpuix/react",
    "strict": true,
    "skipLibCheck": true,
    "noEmit": true
  }
}
```

### 2. Write the entry file

End the file with `render()`. That call creates the window, mounts React, and
starts the frame loop.

```tsx
import { useState } from 'react'
import { render } from '@gpuix/react'

function App() {
  const [count, setCount] = useState(0)
  return (
    <div style={{ padding: 24, backgroundColor: '#1a1a1a', height: '100%' }}>
      <div
        onClick={() => setCount((c) => c + 1)}
        style={{
          padding: 12,
          borderRadius: 8,
          cursor: 'pointer',
          backgroundColor: '#232323',
          hover: { backgroundColor: '#2c2c2c' },
        }}
      >
        <text style={{ color: '#e2e2e2' }}>Count: {count}</text>
      </div>
    </div>
  )
}

render(<App />, { title: 'My App', width: 800, height: 600 })
```

> [!IMPORTANT]
> **Give every `<text>` a `color`.** GPUI does not inherit `color` from a
> parent, so text with no color paints **black** and disappears on a dark
> surface.

### 3. Run it

```bash
bun --hot app.tsx
```

For hot reload during development, use `bun --hot` rather than plain `bun`.
A save then remounts React on the same window instead of opening a second one.

Bun is not required to run the renderer. GPUIX uses Node-API native bindings,
and React apps also run on Node.js, as [`disktree`](./disktree/) does. For
Node.js, compile TSX to JavaScript first or use a Node-compatible TSX loader.
The `--hot` option and same-window hot-remount workflow described below are
Bun-specific; running an app on Node.js does not provide Bun's hot reload.

### 4. Ship a binary

```bash
bun build --compile app.tsx --outfile dist/app
./dist/app
```

The binary carries the renderer, so it runs with no Bun and no Node install.

For a smaller ship set, run the same React app on
[hermes-node](./website/src/guides/hermes.mdx) instead of Bun. That path is
**12 MB** plus a **22 MB** native sidecar. The steps are in that guide.

### 5. Wrap it in an app with an icon

A raw Mach-O has no Dock icon. Use
[cargo-packager](https://github.com/crabnebula-dev/cargo-packager) to wrap the
binary. Config: [Config](https://docs.rs/cargo-packager/latest/cargo_packager/config/struct.Config.html).
CLI: [docs.rs/cargo-packager](https://docs.rs/cargo-packager/latest/cargo_packager/).

```bash
cargo install cargo-packager --locked
```

Build an `.icns` from a 1024 PNG, then pack. Pass the `.icns`, not a 1024 PNG.
cargo-packager rejected a 1024 PNG with `No matching IconType`.

```bash
mkdir AppIcon.iconset
sips -z 16 16 icon-1024.png --out AppIcon.iconset/icon_16x16.png
sips -z 32 32 icon-1024.png --out AppIcon.iconset/icon_16x16@2x.png
sips -z 32 32 icon-1024.png --out AppIcon.iconset/icon_32x32.png
sips -z 64 64 icon-1024.png --out AppIcon.iconset/icon_32x32@2x.png
sips -z 128 128 icon-1024.png --out AppIcon.iconset/icon_128x128.png
sips -z 256 256 icon-1024.png --out AppIcon.iconset/icon_128x128@2x.png
sips -z 256 256 icon-1024.png --out AppIcon.iconset/icon_256x256.png
sips -z 512 512 icon-1024.png --out AppIcon.iconset/icon_256x256@2x.png
sips -z 512 512 icon-1024.png --out AppIcon.iconset/icon_512x512.png
sips -z 1024 1024 icon-1024.png --out AppIcon.iconset/icon_512x512@2x.png
iconutil -c icns AppIcon.iconset -o AppIcon.icns
```

**Bun** (one binary):

```json
{
  "productName": "My App",
  "version": "0.1.0",
  "identifier": "dev.example.app",
  "binariesDir": "dist",
  "outDir": "bundle",
  "binaries": [{ "path": "app", "main": true }],
  "icons": ["AppIcon.icns"],
  "formats": ["app"]
}
```

```bash
cargo packager --release --config packager.json
open "bundle/My App.app"
```

**Hermes** needs the `.node` next to the exe. List it as a **second binary**,
not a resource. Resources go in `Contents/Resources`. `dlopen` looks in
`Contents/MacOS`.

```json
{
  "binaries": [
    { "path": "gpuix-hermes", "main": true },
    { "path": "gpuix-native.darwin-arm64.node", "main": false }
  ]
}
```

`formats` is the host OS only:

| OS | `formats` | Output |
|---|---|---|
| macOS | `"app"`, then `"dmg"` | `.app`, optional `.dmg` |
| Windows | `"nsis"` | setup `.exe` |
| Linux | `"appimage"` | `.AppImage` |

On this machine the Bun chat `.app` is **82 MB**. The Hermes counter `.app`
is **34 MB**.

### 6. Auto-update

Packaging does **not** turn on updates. The running app calls
`checkUpdate` on `@gpuix/native`. HTTP uses the same `reqwest_client` as
`<img>`. There is no second native addon.

Host on **GitHub Releases**. Create the release first. CI packs on each OS,
signs, and uploads the bundle plus its `.sig`. The app hits
`https://github.com/OWNER/REPO/releases/latest`. That rewrites to
`https://api.github.com/repos/OWNER/REPO/releases/latest`. The updater
reads `tag_name` and `assets`, then GETs the sibling `{name}.sig`.

Sign once:

```bash
cargo packager signer generate
```

Store the private key and its password as repo secrets
`CARGO_PACKAGER_SIGN_PRIVATE_KEY` and
`CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD`. Put the **public** key in the app.

```tsx
import { checkUpdate } from '@gpuix/native'
import { render } from '@gpuix/react'
import { App } from './app'

async function maybeUpdate() {
  const update = await checkUpdate('0.1.0', {
    endpoints: ['https://github.com/OWNER/REPO/releases/latest'],
    pubkey: '<public key from signer generate>',
  })
  if (update) await update.downloadAndInstall()
}

maybeUpdate()
render(<App />)
```

`https://github.com/OWNER/REPO` is the same endpoint.

Packager only builds the **host** OS. Run it on macOS, Linux, and Windows.
`--release` is the packager profile (look in `binariesDir` for a release
binary). It is not `cargo build --release`. Signing is automatic when those
two env vars are set. Source:
[cargo-packager CLI](https://docs.rs/cargo-packager/latest/cargo_packager/).

With `productName: "My App"`, `version: "0.1.0"`, and
`binaries: [{ "path": "app", "main": true }]`, packager writes:

| OS | `formats` | Files in `outDir` (`bundle/`) |
|---|---|---|
| macOS | `"app"` | `My App.app`, then on sign `My App.app.tar.gz` + `My App.app.tar.gz.sig` |
| Linux | `"appimage"` | `app_0.1.0_x86_64.AppImage` + `.sig` |
| Windows | `"nsis"` | `app_0.1.0_x64-setup.exe` + `.sig` |

The macOS updater wants the **`.app.tar.gz`**, not the `.app` and not a
`.dmg`. Packager tars the `.app` only when it signs. Linux and Windows names
use the **binary stem** (`app`), not `productName`. NSIS arch is `x64`, not
`x86_64`. A missing sibling `.sig` is an error.

Create the GitHub release yourself, then pack and upload. `--clobber`
replaces an asset if CI retries. Do **not** upload a feed JSON.

```bash
# macOS
bun build --compile app.tsx --outfile dist/app
cargo packager --release --config packager.json
gh release upload v0.1.0 \
  "bundle/My App.app.tar.gz" \
  "bundle/My App.app.tar.gz.sig" \
  --clobber

# Linux
bun build --compile app.tsx --outfile dist/app
cargo packager --release --config packager.json
gh release upload v0.1.0 \
  bundle/app_0.1.0_x86_64.AppImage \
  bundle/app_0.1.0_x86_64.AppImage.sig \
  --clobber

# Windows
bun build --compile app.tsx --outfile dist/app.exe
cargo packager --release --config packager.json
gh release upload v0.1.0 \
  bundle/app_0.1.0_x64-setup.exe \
  bundle/app_0.1.0_x64-setup.exe.sig \
  --clobber
```

`downloadAndInstall()` replaces the packaged files. It does **not** relaunch.
Quit after it returns, or the next start uses the new app.

HTTPS is in the native crate. This works on **Bun** and **hermes-node**. It
does not exist in the browser wasm build. The repo must be **public**, or
GitHub will 404 the API. Optional: put a Cloudflare cache in front of
`api.github.com`. Same GitHub JSON. Not a custom schema.

### Start from the example app

[`example-app/`](https://github.com/remorses/gpuix/tree/main/example-app) is a complete todo app in one file, with `dev`,
`build`, `web:dev` and `typecheck` scripts already wired. Create a copy with
`bunx @gpuix/cli new my-app`.

![The GPUIX todo example app](./docs/images/todo-app.png)

### Shell completions

Install completions for the `gpuix` command:

```bash
bun add -g @gpuix/cli
gpuix completions install
```

## Examples

| Example | Run | What it shows |
|---|---|---|
| **todo** | `bun run dev` in [`example-app/`](https://github.com/remorses/gpuix/tree/main/example-app) | The starting point: one file, a `<virtual-list>`, a native `<input>`, and an animated sidebar |
| **blurred window** | `bun run blurred-window` | A macOS frosted-glass surface using GPUI's native vibrancy backdrop and transparent titlebar |
| **chat** | `bun --hot chat.tsx` | A GPUIX app: transparent titlebar, animated sidebar, per-thread transcripts, demo replies, composer, `<markdown>` |
| **timeline** | `bun --hot timeline.tsx` | A video-editor timeline: clip dragging, edge trimming with snapping, playhead scrubbing, marquee selection, zoom under the pointer, and a two-axis pan with a frozen ruler and track column |
| **mail** | `bun --hot mail.tsx` | A Superhuman-style mail client: three panes, thread list, and a Framer newsletter |
| **disktree** | `npx disktree [dir]`, source in [`disktree/`](https://github.com/remorses/gpuix/tree/main/disktree) | A port of [tobi/disktree](https://github.com/tobi/disktree) published to npm: scans a folder or the whole disk on worker threads, draws a translucent treemap on a frosted window, and ranks what could go |
| **native-text** | `bun --hot native-text.tsx` | The three native text components with a tab switcher |
| **counter** | `bun --hot counter.tsx` | The smallest possible app: state, events, hover |
| **diff** | `bun --hot diff.tsx` | A diff viewer composed from `<div>` and `<text>` in JS, for comparison |
| **web** | `bun run web` from the repository root | The ChatGPT example rendered in a browser canvas with WebGPU |

The todo app lives in [`example-app/`](https://github.com/remorses/gpuix/tree/main/example-app) and is meant to be copied with `bunx @gpuix/cli new`.
The rest live in [`examples/`](https://github.com/remorses/gpuix/tree/main/examples). Those `bun --hot` commands need a clone of this repo and a local native build. They will not run against the published packages alone.

Or download a standalone **chat** build from the [GitHub release](https://github.com/remorses/gpuix/releases). No Bun or Rust install is required.

```bash
tar -xzf example-chat-aarch64-apple-darwin.tar.gz
./example-chat-aarch64-apple-darwin
```

The archive keeps the executable bit, so there is no `chmod` step. macOS may still block the unsigned binary the first time. Right-click the file, choose **Open**, and confirm.

On Windows, download `example-chat-x86_64-pc-windows-msvc.exe` for x64 or
`example-chat-aarch64-pc-windows-msvc.exe` for ARM64 and double-click it. On Linux,
the file is `example-chat-x86_64-unknown-linux-gnu.tar.gz`.

The chat and disktree desktop examples keep the native Windows title bar for
dragging, minimizing, maximizing, and closing the window. Other platforms keep
their existing custom chrome.

disktree reveals the selection in Finder on macOS or Explorer on Windows.
On Linux it opens the selected folder, or a file's parent folder, with
`xdg-open`. A file-manager launch failure appears in the selection panel
without stopping the app.

The web example bundles the same React app and reconciler as the desktop chat
example. wasm-bindgen exposes mutations and event callbacks to the existing
retained tree and `GpuixView`, which run through GPUI's browser platform.

The web build needs nightly Rust and the matching wasm-bindgen CLI:

```bash
rustup toolchain install nightly --component rust-src --target wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.127 --locked
bun run web
```

The generated Wasm uses shared memory, so the page must be cross-origin
isolated. Production servers must send these headers on the **top-level
document**:

```http
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Embedder-Policy: require-corp
```

`require-corp` then constrains **cross-origin** subresources, which must supply
their own CORS or `Cross-Origin-Resource-Policy`. Serve the JavaScript and the
Wasm from the same origin as the document and nothing else is needed.

`bun run web` rebuilds the Wasm only when `packages/native/wasm` is missing.
After a Rust change, force it:

```bash
bun scripts/web.ts --rebuild
```

#### Hot reload in the browser

`bun run web` serves the example through Bun's frontend dev server, so an edit
to `examples/chat.tsx` arrives as a **React Fast Refresh** update. Components
swap in place and `useState` survives, which means the composer text, the
sidebar selection, and the scroll position all stay where they were. The GPUI
canvas is never re-created and the ~19 MB Wasm module is never re-fetched.

Fast Refresh only applies to a module whose exports are all components. Edit
anything else, such as the entry file, and Bun reloads the page instead. Both
paths are correct; the reload is only slower.

The Wasm half is a **singleton and must never re-evaluate**.
`WebGpuixRenderer::init` fails with `GPUIX web is already running` once its
thread-local app exists, and GPUI's browser platform appends its own canvas to
`<body>`. What protects it is not that it lives in `node_modules`; Bun bundles
it into the same client registry as your app. It is that Bun re-runs only the
**changed** module and then walks upward through its importers, so an unchanged
dependency stays evaluated and cached. Two rules follow:

- do not call `import.meta.hot.accept("./your-app", ...)` in the entry file.
  Bun runs an importer's dependency-accept callback **even when the imported
  module already self-accepted**, so that callback would remount the tree on top
  of a successful refresh and throw away every `useState`
- keep the `@gpuix/native` import in a module that can never become a Refresh
  boundary and is never explicitly accepted

The chat example puts a virtualized `<diff>` and a GFM table inside an assistant
turn, inside a scrolling transcript:

![A diff and a markdown table inside a chat turn](./docs/images/chat-diff.png)

Markdown, code and a virtualized diff in one frame:

![Markdown, code and diff rendered together](./docs/images/showcase.png)

## Architecture

GPUIX bridges React and Solid to GPUI using a **shared mutation-based runtime**.
Desktop apps use napi-rs; browser apps load the same Rust renderer through
wasm-bindgen. Each framework adapter collects changed elements into one atomic
mutation batch. Rust applies that batch to a retained element tree that GPUI
reads each frame.

```
┌─────────────────────────────────────────────────────────────────┐
│  React or Solid (JavaScript)                                    │
│                                                                 │
│  function App() {                                               │
│    const [count, setCount] = useState(0)                        │
│    return (                                                     │
│      <div style={{ display: 'flex', gap: 8 }}>                  │
│        <div onClick={() => setCount(c => c + 1)}>               │
│          Count: {count}                                         │
│        </div>                                                   │
│      </div>                                                     │
│    )                                                            │
│  }                                                              │
└─────────────────────────────────────────────────────────────────┘
                    │ napi desktop / wasm-bindgen browser
                    │ applyBatch([
                    │   ["createElement", 1, "div"],
                    │   ["setStyle", 1, {...}],
                    │   ["setRoot", 1]
                    │ ])
                    ▼
┌─────────────────────────────────────────────────────────────────┐
│  Rust host bridge                                               │
│                                                                 │
│  RetainedTree ── stores elements, styles, event flags           │
│       │                                                         │
│       ▼  each GPUI frame                                        │
│  GpuixView::render() → build_element() → GPUI elements          │
└─────────────────────────────────────────────────────────────────┘
                    │
                    ▼
┌─────────────────────────────────────────────────────────────────┐
│  GPUI                                                           │
│                                                                 │
│  Metal, DirectX, Vulkan, or browser WebGPU / WebGL2             │
│  Flexbox layout via Taffy                                       │
└─────────────────────────────────────────────────────────────────┘
```

## Why This Works

GPUI is an **immediate-mode** UI framework — it rebuilds the entire element tree every frame. Instead of fighting this, GPUIX embraces it:

1. The React or Solid adapter detects a state change and queues host mutations (`createElement`, `setStyle`, `appendChild`, etc.)
2. `applyBatch()` validates and applies the complete commit to the Rust **RetainedTree**
3. On each GPUI frame, `GpuixView::render()` walks the RetainedTree and calls `build_element()` to produce ephemeral GPUI elements
4. GPUI lays them out (Taffy flexbox) and renders to the GPU
5. Only **changed elements** cross the FFI boundary. The framework adapter sends minimal mutations

React and Solid use the same IDs, mutation queue, event routing, testing API,
automation client, observers, and text-search matcher. Their framework-specific
schedulers and component contexts stay in the adapter packages.

## Mutation API

The mutation surface between JS and Rust is one atomic method. Desktop uses napi and the browser uses wasm-bindgen:

```ts
type MutationHost = Pick<GpuixRenderer, "applyBatch">
```

`createMutationQueue` only needs that. Window size, focus, and selection use smaller host types (`WindowSizeHost`, `SelectionHost`). `NativeRenderer` is `MutationHost` plus those methods, so a fake can implement `applyBatch` alone.

Element IDs are plain numbers generated by an incrementing counter in JS. React may abandon work in concurrent render mode, so GPUIX keeps new host nodes in JS until React places the accepted subtree during commit. Only then are its mutations added to the batch. `applyBatch()` applies that accepted commit atomically and marks the Rust view dirty for the next frame.

## Event Flow

Events travel from GPUI back to React through a `ThreadsafeFunction` on desktop
and a wasm-bindgen callback in the browser.

```
User clicks element id=3
       │
       ▼
GPUI delivers the mouse input; the native dispatcher hit-tests it
and works out the target and its path (3, then its ancestors)
       │
       ▼
Rust calls emit_event_full(callback, 3, "click", {x, y, path, ...})
       │
       ▼
Desktop ThreadsafeFunction / browser callback sends EventPayload
       │
       ▼
JS dispatchDomEvent turns the ids into the adapter's elements (target,
currentTarget, relatedTarget) and walks the path: capture handlers from the
outermost ancestor down, then the target's, then bubble handlers back up,
reading eventHandlers.get(id)?.get("click")
       │
       ▼
React handler runs: onClick={() => setCount(c => c + 1)}
       │
       ▼
State update triggers re-render → reconciler sends mutations back to Rust
```

Event handlers are stored in a JS-side registry keyed by `(elementId, eventType)`. Rust only knows **whether** an element has a listener (via `setEventListener`), not the closure itself — the actual handler lives in JS.

## Packages

- **`@gpuix/native`**: Rust bindings plus the framework-neutral TypeScript host runtime. It owns host types, mutation batching, renderer state, event routing, observers, native testing, and automation.
- **`@gpuix/native/host`**: host contracts, mutation helpers, renderer ownership, window observers, selection observation, and text search. Importing it does not load the `.node` addon.
- **`@gpuix/native/testing`**: the shared `TestRenderer` over the real `TestGpuixRenderer`.
- **`@gpuix/native/automation`**: the shared automation protocol, client, locators, and process launcher.
- **`@gpuix/react`**: the React reconciler and React components. It preserves its existing exports and re-exports shared testing, automation, search, and observer APIs.
- **`@gpuix/solid`**: the Solid 1 universal renderer, Solid primitives, motion, Select, Combobox, Tooltip, Bun preload, and build plugin.
- **`@gpuix/cli`** — `gpuix new` downloads `example-app/`, sets its published React dependency, and installs it as a standalone project.

Pin the selected adapter and `@gpuix/native` to the **same exact version**.
GPUIX is still pre-1.0. Breaking changes can land before v1. Upgrade them
together.

### Solid quick start

Install Solid and the official Solid adapter:

```bash
bun add --exact @gpuix/solid @gpuix/native solid-js
```

Use Solid's preserved JSX. The preload compiles application `.tsx` and `.jsx`
files for Solid's universal renderer and selects the reactive client runtime.

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "jsx": "preserve",
    "jsxImportSource": "@gpuix/solid",
    "strict": true,
    "skipLibCheck": true,
    "noEmit": true
  }
}
```

```toml
preload = ["@gpuix/solid/preload"]
```

```tsx
import { createSignal } from 'solid-js'
import { render } from '@gpuix/solid'

function App() {
  const [count, setCount] = createSignal(0)
  return (
    <div style={{ padding: 24, backgroundColor: '#1a1a1a', height: '100%' }}>
      <div onClick={() => setCount((value) => value + 1)}>
        <text style={{ color: '#e2e2e2' }}>Count: {count()}</text>
      </div>
    </div>
  )
}

render(() => <App />, { title: 'Solid GPUIX', width: 800, height: 600 })
```

Run it directly. No wrapper command or Vite configuration is required.

```bash
bun app.tsx
bun --hot app.tsx
```

For production `Bun.build`, pass the exported plugin:

```ts
import solidPlugin from '@gpuix/solid/bun-plugin'

await Bun.build({
  entrypoints: ['./app.tsx'],
  target: 'bun',
  outdir: './dist',
  plugins: [solidPlugin],
})
```

`@gpuix/solid` targets stable Solid 1.9. Its peer range is `>=1.9 <2`.

The full Solid adapter API is in the [Solid guide](./website/src/guides/solid.mdx).

## Building

This section is for **working on GPUIX itself**. To build an app with it, see
[Quickstart](#quickstart) instead. Installing the packages needs no Rust
toolchain and no submodule.

### Prerequisites

1. Rust toolchain
2. Node.js 18+
3. Xcode with Metal Toolchain (macOS)

The `cli/` and `disktree/` builds use Node.js to set executable permissions on
Unix. Windows needs no `chmod` command.

```bash
# Install Metal Toolchain if needed
xcodebuild -downloadComponent MetalToolchain

# Install dependencies
bun install

# Check out the pinned GPUI fork
git submodule update --init --recursive

# Build native package
cd packages/native
bun run build

# Build React package
cd ../react
bun run build

# Build Solid package
cd ../solid
bun run build

# Run example (use tmux for long-running sessions)
cd ../../examples
bun --hot counter.tsx
```

### Windows ARM64 source builds

**Windows builds cover x64 and ARM64.** CI builds ARM64 on GitHub's
`windows-11-arm` runner, tests the native addon with ARM64 Node.js and Bun,
and includes it in the native npm packages and standalone chat release assets.
Use an ARM64 runtime to load the ARM64 addon; the loader selects the package
from the runtime's architecture, not the machine's architecture.

In Visual Studio Installer, install **Desktop development with C++**, the
ARM64 MSVC tools, a Windows SDK, C++ Clang tools, and CMake. Open a developer
PowerShell with those tools available. [ring requires Clang for Windows ARM64](https://github.com/briansmith/ring/blob/main/BUILDING.md).

If `clang` is not on `PATH`, add Visual Studio's ARM64 LLVM directory:

```powershell
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vs = & $vswhere -latest -products '*' -property installationPath
$llvm = Join-Path $vs 'VC\Tools\Llvm\ARM64\bin'
if (!(Test-Path "$llvm\clang.exe")) { throw 'Install the ARM64 Clang tools in Visual Studio Installer' }
$env:PATH = "$llvm;$env:PATH"

node -p 'process.arch' # must print arm64
bun -p 'process.arch'  # must print arm64
clang --version
cmake --version

rustup target add aarch64-pc-windows-msvc
$env:CARGO_BUILD_TARGET = 'aarch64-pc-windows-msvc'
bun install
Push-Location packages/native
bun run build
Pop-Location
Push-Location packages/react
bun run build
Pop-Location
Push-Location packages/solid
bun run build
Pop-Location
```

Run these commands from a repository checkout with the pinned `zed/` submodule
already initialized, as in the build steps above. Matching runtime and addon
architectures is required; renaming an x64 binary does not make it ARM64.

## Usage

```tsx
import React, { useState } from 'react'
import { render } from '@gpuix/react'

function App() {
  const [count, setCount] = useState(0)
  return (
    <div style={{ display: 'flex', gap: 8, padding: 16 }}>
      <div
        style={{ backgroundColor: '#3b82f6', borderRadius: 8, padding: 12, cursor: 'pointer' }}
        onClick={() => setCount(c => c + 1)}
      >
        <div style={{ color: '#ffffff' }}>Count: {count}</div>
      </div>
    </div>
  )
}

render(<App />, {
  title: 'My App',
  width: 800,
  height: 600,
  titlebarTransparent: true,
  windowBackground: 'blurred',
  trafficLightX: 16,
  trafficLightY: 17,
})
```

`render()` creates the native window, mounts React, and starts the frame loop.
The red traffic-light button quits the process. Start the app again from the
terminal.

| Option | Values | Purpose |
|---|---|---|
| `titlebarTransparent` | boolean | Hide the native titlebar so the app draws chrome under the traffic lights. The window then drags only from the boxes that set [`appRegion: "drag"`](#window-drag-regions) |
| `windowBackground` | `"opaque"` (default), `"transparent"`, `"blurred"` | Window fill. `"blurred"` is the macOS vibrancy backdrop |
| `trafficLightX` / `trafficLightY` | pixels | Traffic-light origin. The chat example uses `(16, 17)` |
| `appearance` | `"system"` (default), `"light"`, `"dark"` | Force the app light or dark, or follow the OS. See [Appearance](#appearance) |
| `transparent` | boolean | Same as `windowBackground: "transparent"` when that option is unset |
| `appName` | string | Name inside the macOS `Hide X` and `Quit X` items. Defaults to `title` |
| `focus` | boolean, default `true` | `false` opens the window behind the active app, like `open -g` |
| `show` | boolean, default `true` | `false` opens the window hidden. Call `activateWindow()` to reveal it |
| `x` / `y` | CSS px | Where the window's top-left opens, in the global space [`screen`](#displays) reports: origin at the primary display's top-left, y down, so a display to the left of or above it is negative. The window opens on the display that contains the point, else the nearest. Both are needed, otherwise the window is centred on the primary display. With `fullscreen: true` it goes fullscreen on that display. As in Electron |
| `fonts` | string[] | Font files (`.ttf`, `.otf`) read and passed to [`registerFont()`](#custom-fonts) right after the window opens, so `fontFamily: "Inter"` resolves without a system-wide install. GPUIX ships no fonts: the app supplies its own files. Paths resolve against the process's current working directory, so pass absolute ones, e.g. `fileURLToPath(new URL("./fonts/Inter-Regular.ttf", import.meta.url))`. An unreadable path makes `render()` throw. On macOS a variable font follows `fontWeight` and `fontVariationSettings`; elsewhere it only renders its default instance, so pass one static file per weight |

Call it again after a save and it remounts the tree on the same window.

### The macOS menu bar

GPUIX installs the application menu bar for you, so a fresh app already answers
`⌘Q`, `⌘H`, `⌥⌘H`, `⌘M`, and `⌘W`. Without it `NSApp.mainMenu` is nil, macOS
paints an empty menu bar, and those shortcuts do not exist at all: AppKit only
provides them through menu items.

```
Apple    <executable>             Window
         ├ Services               ├ (AppKit window tiling)
         ├ Hide <appName>   ⌘H    ├ Minimize          ⌘M
         ├ Hide Others     ⌥⌘H    ├ Zoom
         ├ Show All               ├ Close Window      ⌘W
         └ Quit <appName>   ⌘Q    └ (open windows)
```

**`appName` does not set the title of the application menu.** macOS takes that
from the executable, so `bun app.tsx` shows `bun` during development and a
`bun build --compile` binary shows its own file name. Only a real `.app` bundle
changes it. `appName` reaches the items inside the menu, and nothing else.

There is **no Edit menu**, on purpose. A menu key equivalent is consumed by
AppKit before the window sees the key event, so an Edit menu carrying `⌘C`
would take the keystroke away from text selection and from `<input>`.

Use **`render()`**, not `createRenderer()`, in the app entry. `bun --hot`
re-runs the whole file on save. `createRenderer()` plus `init()` would then
build a second host. `render()` is idempotent: the first call owns the window,
later calls only remount React.

`createRenderer()`, `createRoot()`, and `startFrameLoop()` stay public for
tests and custom hosts. Pass `{ renderer }` into `render()` when you already
have one.

**One renderer drives one root.** A renderer owns one window, one native root
id, and one event map, so `createRoot()` throws if that renderer already has a
mounted root. Call `unmount()` on the first root before you create another;
`render()` already does that for you.

### Background launch

`focus: false` opens the window **without taking focus**. The app you were
typing in keeps the caret and the active titlebar. `show: false` goes further
and opens no window at all, so the process runs with a live React tree and
nothing on screen.

```tsx
render(<App />, { title: 'Notes', focus: false })
```

**Turn this on whenever a coding agent runs your app.** An agent that starts
the app to check its work will otherwise yank the window in front of whatever
you are doing, mid-sentence, once per iteration. With `focus: false` the agent
still gets a real GPU-rendered window it can screenshot and click, and you keep
your editor. See [Let an agent drive the app](#let-an-agent-drive-the-app).

`activateWindow()` brings the window forward and focuses it. It is the only way
to reveal a `show: false` window. Reach it from any component with
`useGpuixRequired()`:

```tsx
import { useGpuixRequired } from '@gpuix/react'

function Reveal() {
  const renderer = useGpuixRequired()
  return <div onClick={() => renderer.activateWindow?.()}>Show</div>
}
```

Outside React, call it on the renderer that `createRenderer()` returned.

### Window controls

The desktop renderer exposes GPUI's native minimize, zoom, and fullscreen
operations. Reach them through `useGpuixRequired()` or the renderer returned by
`createRenderer()`.

```tsx
function WindowControls() {
  const renderer = useGpuixRequired()
  return (
    <div style={{ display: 'flex', gap: 8 }}>
      <div onClick={() => renderer.minimizeWindow?.()}>Minimize</div>
      <div onClick={() => renderer.zoomWindow?.()}>Zoom</div>
      <div onClick={() => renderer.toggleFullscreen?.()}>Fullscreen</div>
    </div>
  )
}
```

`minimizeWindow()`, `zoomWindow()`, and `toggleFullscreen()` work on macOS,
Windows, Linux, and FreeBSD. `zoomWindow()` uses the platform's native zoom or
maximize operation. These methods are not available in the browser renderer.

### Window drag regions

With `titlebarTransparent` there is no titlebar to grab, so the page says which
boxes move the window. `appRegion: "drag"` on the bar, `"no-drag"` on the
controls inside it:

```tsx
<div
  style={{
    appRegion: 'drag',
    height: 40,
    display: 'flex',
    flexDirection: 'row',
    alignItems: 'center',
    paddingLeft: 80, // clears the traffic lights
  }}
>
  <text>Inbox</text>
  <div style={{ flexGrow: 1 }} />
  <div style={{ appRegion: 'no-drag' }} onClick={compose}>Compose</div>
</div>
```

The value **inherits**, like `pointerEvents`, so the text and the spacer above
drag too, and a `"drag"` inside a `"no-drag"` inside a `"drag"` works. It does
nothing without `titlebarTransparent`: a window with its native titlebar drags
from that.

Regions follow Electron's `-webkit-app-region`, not hit testing. Every element
with a value contributes its layout box, in tree order, and the **last box that
holds the point wins**: `"drag"` adds, `"no-drag"` cuts. Z-order,
`pointerEvents`, `opacity`, `overflow` and `borderRadius` are ignored, so an
overlay without a value that covers part of the bar does not stop it dragging
there, and a rounded or clipped child still drags over its full box.

Two cases have no region. A `<virtual-list>`'s own box registers none, so
a `"drag"` on it marks nothing itself: its rows do, by inheritance, so the
region is the rows' boxes and not the list's. And a fullscreen window has no
drag regions at all, as in Electron: nothing drags, and the cursor and hover
are the page's.

An inline `<span>` laid out as part of a paragraph's text has no box of its
own, so its region is one rectangle per line it wraps over: the border box of
each line fragment. It inherits and cuts like any other box, so a `"drag"` span
in a `"no-drag"` paragraph drags on its text, on every line, and nowhere else.

What the page sees inside a drag region:

- **A press never arrives.** No `pointerdown`, `mousedown`, `mouseup` or
  `click`, no focus change, no `onMouseDownOutside` elsewhere. It moves the
  window, and a press that starts in a `"no-drag"` box is the page's as usual.
- **The pointer is outside the window.** Over a region with no button held,
  hover leaves and no move events fire; leaving it brings hover back. The
  cursor is the arrow whatever `cursor` says. The wheel still passes.
- **Right click reaches the page on macOS**, and so does ctrl-click, so a bar can
  have its own context menu. Only the left button moves the window.
- **A double click does what the titlebar does.** On macOS that is the user's
  "Double-click a window's title bar" setting (zoom, minimise or nothing); on
  Linux it toggles maximize.

| Platform | Support |
|---|---|
| macOS | Native. The move starts on the press, with no threshold |
| Windows | Native, and the OS owns the whole region: it drags, maximizes on double click and shows the system menu on right click, none of which the page sees |
| Linux | Implemented the same way, right click opens the window menu. Not exercised on a real Linux window yet |

[`examples/titlebar.tsx`](https://github.com/remorses/gpuix/blob/main/examples/titlebar.tsx)
is the whole pattern: a drag bar with a button and an input cut out of it.
To test a press in a region, see [Window actions in tests](#window-actions-in-tests).

### Displays

`screen` lists the displays, as Electron's does. It works before the window
opens, so an app can pick where to open.

```ts
import { screen } from '@gpuix/react'

screen.getAllDisplays()    // every display, in the order the OS lists them
screen.getPrimaryDisplay() // the one with the menu bar
```

A `Display` has `id`, `label` (the OS's name for it), `bounds` and `workArea`
(`bounds` without the menu bar and Dock), each a `{ x, y, width, height }` in the
global CSS px space of the `x` / `y` window options, `size` and `workAreaSize`
(`{ width, height }` of those two), `scaleFactor` (device pixels per CSS px),
`rotation` (0, 90, 180 or 270), and `internal` (the display built into the
machine). The primary display's `bounds` start at `(0, 0)`.

Both functions work on macOS only and throw elsewhere.

To open on the second display, centred in its work area:

```tsx
import { render, screen } from '@gpuix/react'

const primary = screen.getPrimaryDisplay()
const target = screen.getAllDisplays().find((d) => d.id !== primary.id) ?? primary
const width = 800
const height = 600

render(<App />, {
  width,
  height,
  x: target.workArea.x + (target.workArea.width - width) / 2,
  y: target.workArea.y + (target.workArea.height - height) / 2,
})
```

### Appearance

By default the window follows the system light or dark setting. `appearance`
overrides it for the whole app:

```tsx
render(<App />, {
  appearance: 'dark', // 'system' (default) | 'light' | 'dark'
  onAppearanceChange(event) {
    setScheme(event.value) // 'light' | 'dark'
  },
})
```

Change it later, or read what the window is using right now, through the
renderer:

```tsx
function ThemeToggle() {
  const renderer = useGpuixRequired()
  return (
    <div onClick={() => renderer.setAppearance?.('light')}>
      Light (now {renderer.getAppearance?.()})
    </div>
  )
}
```

`setAppearance('system')` goes back to following the OS. `getAppearance()` is the
**effective** appearance, always `"light"` or `"dark"`, even while following the
system. GPUI's vibrant variants are reported as plain `"light"` / `"dark"`. An
unknown string throws, both in the `appearance` option and in `setAppearance`.

`onAppearanceChange` is a **window-level** callback on `render()` / `createRoot()`,
the same attachment as `onSelectionChange`. `event.value` is `"light"` or
`"dark"`. It fires when the effective appearance changes, whether the user
flipped the OS setting or the app called `setAppearance`. It does not fire for
the initial value, so read `getAppearance()` first, or use
`observeAppearance(renderer, subscribe, callback)` from `@gpuix/react`, which
does both: it calls `callback` with the current value and then on every event
that `subscribe` delivers. In Solid, `createAppearance()` returns an accessor.

macOS applies `appearance` and `setAppearance`. On Windows and Linux they are
accepted and ignored, and `getAppearance()` reports GPUI's value for the
window. GPUIX does not change the default theme of `<code>`, `<diff>` or
`<markdown>` with the appearance; pass those your own colors.

`TestRenderer` does what the live renderer does: the `appearance` option and
`setAppearance` apply to the test window, `getAppearance()` reads it back,
`onAppearanceChange` fires, and `light-dark()` and `prefers-color-scheme` follow.
`setAppearance('system')` restores the platform's appearance, and a test window
made without an `appearance` starts there. The override belongs to the platform,
not to the window (on macOS it is the process's `NSApplication.appearance`), so a
test that forces one and does not make another window leaves it set for the rest
of that process.

### matchMedia

`matchMedia(query)` works as it does in a browser: it returns a `MediaQueryList`
with `matches`, `media`, `addEventListener('change', ...)`,
`removeEventListener`, `onchange`, and the legacy `addListener` /
`removeListener`. A `change` event is a `MediaQueryListEvent` with `matches` and
`media`, and it fires only when `matches` flips.

```tsx
import { useMediaQuery, useReducedMotion } from '@gpuix/react'

function Sidebar() {
  const wide = useMediaQuery('(min-width: 800px)')
  const reduceMotion = useReducedMotion() // true when the OS asks for less motion
  return wide ? <FullSidebar animate={!reduceMotion} /> : <Rail />
}
```

A query is evaluated against the window it is asked in, so each adapter hands you
one bound to the window your component is rendered in:

| | React | Solid |
|---|---|---|
| Bound `matchMedia` | `useMatchMedia()` | `createMatchMedia()` |
| Boolean that tracks a query | `useMediaQuery(query)` | `createMediaQuery(query)` (the query may be a signal) |
| OS reduced motion | `useReducedMotion()` | `createReducedMotion()` |

Code with no component to ask can import `matchMedia` from the adapter. It uses
the window of the most recently mounted root that is still mounted, and throws
when no root is mounted. Outside an adapter, `matchMediaFor(renderer)` from `@gpuix/native/host`
binds to a renderer.

Supported syntax is Media Queries Level 4: `and`, `or`, `not`, `only`, comma
lists, parentheses, and range syntax such as `(400px <= width < 800px)`. The
media types `all` and `screen` match, `print` and `speech` do not.

| Feature | Value in GPUIX |
|---|---|
| `width`, `height`, `aspect-ratio`, `orientation` | The window's content size. `min-` / `max-` prefixes and ranges work. Lengths take `px`, `em`, `rem` (16px), `pt`, `pc`, `in`, `cm`, `mm`, `Q`, `vw`, `vh`, `vmin`, `vmax`. |
| `prefers-color-scheme` | The window's effective appearance, the same value as `getAppearance()`. |
| `prefers-reduced-motion` | The OS "Reduce motion" setting. |
| `prefers-contrast` | `more` when the OS "Increase contrast" setting is on, otherwise `no-preference`. |
| `forced-colors` | Always `none`. |
| `hover`, `any-hover` | Always `hover`. |
| `pointer`, `any-pointer` | Always `fine`. |
| `color`, `monochrome` | `8` and `0`. |

A query item that does not parse, or names a feature or value GPUIX does not
know, is `not all`: it never matches and `media` reports `not all`. The other
items of a comma list still count.

The OS settings are read on macOS (`NSWorkspace` accessibility display options)
and change events arrive from its notification. Windows, Linux and FreeBSD have
no OS read yet and report no preference. `prefers-color-scheme` changes arrive
on every desktop platform that reports appearance changes.

Resizing the window re-evaluates the size queries (`width`, `height`,
`aspect-ratio`, `orientation`) with no timer: the renderer reports each change of
the window's content size, and `change` fires when a list's result flips. A
`MediaQueryList` with no listener is not subscribed to anything.

Under test, the OS settings start as "no preference" regardless of the machine.
`TestRenderer.setMediaFeatures({ reducedMotion: true })` changes them and
delivers the `change` events, and `TestRenderer.setWindowSize(width, height)`
resizes the window the same way. `TestRenderer.setWindowScaleFactor(2)` gives it
the scale of a retina display, for the frames after it. `prefers-color-scheme` reports the system
appearance and cannot be flipped, for the reason given under Appearance.
The "Show scroll bars" setting (see [Scrollbars](#scrollbars)) works the same way:
`TestRenderer.setScrollbarStyle("classic" | "overlay")`.

### File picker

`promptForPaths()` opens the operating system's file picker. It resolves with
the selected absolute paths, or `null` when the user cancels.

```tsx
import { useGpuixRequired } from '@gpuix/react'

function AttachFiles() {
  const renderer = useGpuixRequired()

  const attach = async () => {
    const paths = await renderer.promptForPaths?.({
      files: true,
      multiple: true,
      prompt: 'Attach',
    })
    if (paths) console.log(paths)
  }

  return <div onClick={attach}>Attach files</div>
}
```

With no options, the picker selects one file. Set `directories: true` to select
directories instead. macOS can select files and directories in one prompt;
Windows and Linux require one kind per prompt. Invalid options and platform
picker failures reject. The browser method rejects because browsers return
`File` objects, not operating-system paths. An injected custom renderer can
omit this optional capability.

On the current Windows GPUI backend, the system dialog does not distinguish
cancellation from other `IFileDialog::Show` errors, so both resolve with
`null`. This needs a fix in GPUI before GPUIX can report those errors.

`accept` filters the selectable files with the HTML `accept` syntax:
extensions, MIME types and `type/*` wildcards, separated by commas and
case-insensitive.

```tsx
await renderer.promptForPaths?.({ accept: '.png,.jpg,image/*,application/pdf' })
```

A file matches when it matches any entry, `*/*` matches everything, and
directories are never filtered. A token that is neither `.ext` nor
`type/subtype` rejects the prompt, so a typo cannot silently widen the picker.
macOS and Linux apply the filter. Windows ignores `accept` for now, because
its GPUI backend has no filter support and a MIME type needs an extension
table there; filter the returned paths yourself.

### Clipboard

`clipboard` is the text subset of `navigator.clipboard`. Both methods return
promises, as in the DOM.

```tsx
import { clipboard } from '@gpuix/react' // or '@gpuix/solid'

await clipboard.writeText('copied')
const text = await clipboard.readText() // '' when the clipboard has no text
```

When a renderer is created and the runtime has no `navigator.clipboard`,
GPUIX installs `clipboard` there so code written for the web runs unchanged. An existing
`navigator.clipboard` is never replaced. It always talks to the renderer
created last. Only text is supported, not images or `ClipboardItem`s; the
browser build leaves the browser's own `navigator.clipboard` alone.

`@gpuix/native` also declares `Navigator.clipboard` globally, so a project whose
`tsconfig` has no DOM lib (`lib: ["ESNext"]`) type-checks `navigator.clipboard`
too. With the DOM lib the declaration is identical to its own and merges.

`TestRenderer` has its own in-memory clipboard, so a test can
`writeText()` then paste into an `<input>`, or copy out of one and
`readText()`, without touching the system pasteboard.

### EyeDropper

On macOS, `EyeDropper` is defined globally and works as in the browser:

```tsx
if (typeof EyeDropper !== 'undefined') {
  const controller = new AbortController()
  const { sRGBHex } = await new EyeDropper().open({ signal: controller.signal })
  // '#ff8000'
}
```

`open()` shows the system magnifier and resolves with a lowercase `#rrggbb` in
sRGB. It rejects with an `AbortError` when the user presses Escape, and with
`signal.reason` when `signal` aborts. Windows and Linux do not define
`EyeDropper`, like a browser without it. It is a global only; nothing is
imported.

macOS cannot dismiss the magnifier from code. After an abort the picker stays
on screen until the user clicks or presses Escape, and `open()` rejects with an
`InvalidStateError` until then. The test renderer cannot run it, so test your
handling with a stub `EyeDropper`.

| Platform | `focus: false` | `show: false` |
|---|---|---|
| macOS | window orders in front without becoming key, like `open -g` | honored |
| Windows | `SW_SHOWNOACTIVATE` | honored |
| Linux | **ignored**, the window opens focused | **ignored** |

The process still gets a **Dock icon** on macOS. GPUI sets the regular
activation policy, so there is no menu-bar-agent mode yet. For a real
background daemon, run the app from a `launchd` agent in
`~/Library/LaunchAgents/`; launchd never activates the process.

### Custom fonts

`registerFont()` makes a font that is not installed on the machine available to
`fontFamily`. It takes the bytes of a **TrueType or OpenType** file (`.ttf` or
`.otf`).

```tsx
import { readFileSync } from 'node:fs'
import { useGpuixRequired } from '@gpuix/react'

function LoadFonts() {
  const renderer = useGpuixRequired()
  useEffect(() => {
    renderer.registerFont?.(readFileSync('./Inter-Regular.ttf'))
  }, [renderer])
  return <div style={{ fontFamily: 'Inter' }}>Hello</div>
}
```

The `fonts` window option is a convenience over this: `render(<App />, { fonts: [path] })`
and `createTestRoot({ fonts: [path] })` read each file and call `registerFont()` with its
bytes as soon as the renderer exists, so the same checks apply. An unreadable path throws
naming the path, before any window opens. Relative paths resolve against the process's
current working directory.

Text that already named the family repaints with the font once it is
registered, so there is no need to register before the first render. Register
every weight and style of a family you use; text picks the closest one.

The font must have family, full and PostScript names and a glyph for `"m"`.
GPUI aborts the process on a font without the names, and silently drops a font
without the glyph, so `registerFont()` throws for both and for bytes that are
not a font. Fonts load into the process and last until it exits.

WOFF and WOFF2 are not supported; convert them to TrueType first. The browser
renderer does not have this method, use `FontFace` there.

### Let an agent drive the app

Make focus opt-in through the environment, so a human run behaves normally and
an agent run stays out of the way:

```tsx
render(<App />, {
  title: 'Notes',
  focus: process.env.GPUIX_BACKGROUND !== '1',
})
```

```bash
bun app.tsx                      # you: window comes to the front
GPUIX_BACKGROUND=1 bun app.tsx   # agent: window opens behind your editor
```

`launch()` passes `env` straight through, so an agent script sets it once and
every screenshot, click, and assertion runs on a window that never interrupts
you:

```ts
import { launch } from '@gpuix/react/automation'

const app = await launch({
  command: 'bun',
  args: ['app.tsx'],
  env: { GPUIX_BACKGROUND: '1' },
})

await app.getByTestId('bump').waitFor()
await app.getByTestId('bump').click()
await app.screenshot({ path: 'tmp/after-click.png' })
await app.close()
```

Focus is the only thing that changes. **Automation does not need focus.**
`click()` hits the last painted bounds and `screenshot()` reads the GPU
surface, so both work while the window sits behind your editor, and even on a
`show: false` window that is not on screen at all.

```
  agent ──►  launch({ env: { GPUIX_BACKGROUND: '1' } })
                │
                ▼
           GPU window renders and paints without activation
                │
                ├──►  getByTestId(..).click()   ✓  hits the last painted bounds
                ├──►  screenshot({ path })      ✓  reads the GPU surface
                ├──►  fill() / press()          ✓  uses the live input pipeline
                └──►  close()

  you   ──►  keep typing, your editor stays frontmost the whole time
```

`fill()` and `press()` use the live GPUI window input pipeline. They work
without activating the desktop window. **Linux ignores `focus`**, so an agent
there still gets a focused window.

Prefer `createTestRoot()` when you can. It opens **no window at all**, so
nothing can steal focus and keyboard input works. Reach for `launch()` plus
`focus: false` when the check needs a real window, real GPU paint, or a real
process.

### flushSync

The root is a **concurrent root**, so React commits in a later microtask.
`flushSync` forces the render and the commit to finish before it returns, the
same as in `react-dom`.

```tsx
import { flushSync } from '@gpuix/react'

flushSync(() => setSidebarOpen(true))
```

It flushes **React only**, down to one `applyBatch` call. After it returns the
native retained tree is up to date, including styles and text.

It does **not** wait for GPUI. Layout and paint still happen on the next frame,
exactly like the browser paints after a DOM mutation. To see pixels, wait a
frame in the app, or call `renderer.flush()` in a test.

Use it when an ordering bug depends on the commit landing first: an unmount
before a remount, or a state change before you feed the next event.

## Debug frame overlay

GPUI paints frame-time stats into the window after layout. The overlay is not
a React element. A React FPS label would update every frame and cause more work.

```tsx
render(<App />, { title: 'My App', debugFrameOverlay: 'full' })
```

| Mode | What you see |
|---|---|
| `hidden` | nothing (default) |
| `minimal` | last draw time, e.g. `8.3 MS` |
| `full` | `CUR`, `1%`, `10%`, `MAX`, `FRAMES` |

Or call the renderer:

```ts
renderer.setDebugFrameOverlay('full')
renderer.cycleDebugFrameOverlay()
renderer.resetDebugFrameOverlayStats()
renderer.getDebugFrameOverlay() // 'hidden' | 'minimal' | 'full'
renderer.getDebugFrameOverlayStats()
// { currentMs, p90Ms, p99Ms, maxMs, frames, samples }
```

`p90Ms` is the overlay **10%** line. `p99Ms` is the **1%** line. Those are the slow tail.

The overlay shows **draw time**, not FPS. `8.3 MS` is about 120 Hz.

The chat example has a regression test for this: `examples/chat.perf.test.tsx`. It times mount, wheel draw, and sidebar clicks. It asserts p95, not every frame.

The default example suite excludes this hardware-timing test so shared CI runner variance does not fail functional checks. Run it explicitly on the target Mac:

On macOS, `THROTTLE=utility` restarts the process under `taskpolicy -c utility`. That pins work to E-cores. It is an **M1/M2 Air CPU** proxy, not Chrome 6x. GPU and RAM stay fast. `THROTTLE=background` is slower.

```bash
cd examples
THROTTLE=utility bun run test:perf
THROTTLE=utility bun --hot chat.tsx
```

## Hot reload

### 1. End the file with `render()`

```tsx
import { render } from '@gpuix/react'

function App() {
  return <div style={{ padding: 16 }}>hello</div>
}

render(<App />, { title: 'My App', width: 800, height: 600 })
```

Do **not** call `createRenderer()` or `init()` in this file. `bun --hot` re-runs
the whole entry on save. A second `init()` would open a second window.

### 2. Start the app with `bun --hot`

Prefer **`bun --hot`** over a plain `bun` or `tsx` run. Without `--hot`, a
save starts a second process. With it, `render()` remounts React on the same
window.

```bash
bun --hot app.tsx
```

### 3. Save the file

```
save .tsx  ►  bun re-evaluates the entry  ►  render() remounts React
                     │
                     ▼
              GpuixRenderer, window, GPU stay
```

The first `render()` creates the native host and stores it on `globalThis`.
Each save unmounts the React tree and mounts a new one on that same host.

**Stays:** window, GPU device, native `.node` addon, GPUI scroll physics.

**Resets:** `useState`, focus, React event handlers.

This is a remount, not React Refresh. Keeping hook state needs Bun to inject
`$RefreshReg$` during `--hot`. That transform exists on
`bun build --react-fast-refresh` only. Tracked in
[oven-sh/bun#40179](https://github.com/oven-sh/bun/issues/40179).

Native `.node` edits still need a rebuild. See [Developing the Rust side](#developing-the-rust-side).

On **macOS**, `startFrameLoop` calls `renderer.tick()` when the main run loop has
work, instead of polling every 8 ms: an input event, a block on the main queue
(which includes a display-link frame step) or a run-loop timer. The display link
runs only while a window asks for frames, so an idle window gets no wakes and
arms no timer. Each tick drains only ready AppKit events and Core Foundation
sources, then returns without blocking JavaScript I/O. Wakes coalesce while
JavaScript is busy, and work that arrived meanwhile wakes the next tick. Input
wakes JavaScript as it arrives, once per batch of events, not at the next vsync.

A visible window that is not the key window animates at the display rate. A
window that is occluded or minimised gets no frames; animations keep time and
catch up when it shows.

Pass `{ frameMs }` to `startFrameLoop` to use fixed-rate timer polling instead.
Call `.stop()` on the returned handle to clear the wake callback.
For manual pumping, `renderer.setFrameWakeCallback(() => renderer.tick())`
registers a weak callback after `init()`. It returns `true` on macOS and `false`
on other platforms. Pass `null` to unregister; an already queued callback may
still arrive. The callback fires when the main run loop has work, not only for
frames, and fires again after the next `tick()`, so a manual caller needs no
timer.

A **runtime throw does not freeze the window.** The frame loop catches errors from
`tick()`, native event callbacks catch throws from React handlers, and `render()`
installs `uncaughtException` / `unhandledRejection` listeners so bun stays alive.
The window shows the stack and a **Reload** button that remounts the last
`render()` tree. Save under `bun --hot` also remounts. The process does not
exit.

An error thrown by an **event handler** (an element's, or a `window` or `document`
listener's) does not stop the dispatch: the remaining handlers still run. The error
goes to the renderer's `onError` option when there is one, and otherwise is reported
as uncaught (`reportError`), like a DOM listener's. A simulate call in the test
renderer never rethrows it, so a test that expects the error passes `onError`.

On **Windows and Linux**, GPUI runs its normal blocking native event loop on one
dedicated Rust UI thread. `tick()` does not pump that loop. It only reports
whether the UI thread is still inside `Platform::run`. `startFrameLoop` still
creates a JavaScript timer so last-window-close can return false and `render()`
can `process.exit`, matching macOS. All platforms use GPUI's native platform,
window, renderer, input, scroll, clipboard, keyboard, and IME implementations.
The embedded macOS run-loop extension comes from the pinned GPUIX fork. CI runs
the full React and example test suites through DirectX on Windows.

> [!IMPORTANT]
> On macOS, never drive `tick()` from a `setImmediate` loop. That spins at tens of thousands of
> ticks per second and burns **73% CPU on a completely idle app**, versus **1%** when
> paced.

## Native animations

Use **`motion.div`** to animate from an initial style to a target style. React
sends the target once. Rust calculates intermediate values and requests GPUI
frames until the transition finishes, without a React render or N-API call for
each frame.

### Animate a target

```tsx
import { motion } from '@gpuix/react'

function WelcomeCard() {
  return (
    <motion.div
      initial={{ width: 0, opacity: 0 }}
      animate={{ width: 320, opacity: 1 }}
      transition={{ duration: 0.25, ease: 'easeOut' }}
      style={{ overflow: 'hidden' }}
    >
      <text style={{ color: '#ffffff' }}>Welcome</text>
    </motion.div>
  )
}
```

Set **`initial={false}`** when the element must mount at its `animate` target, the
last value of a keyframe list, with nothing animating. Leave `initial` out and the
element mounts as it is and animates to `animate`, as Motion for React does: a
plain value runs from the element's own value, a [keyframe list](#keyframes) from
its first keyframe, so `animate={{ opacity: [0, 1, 0] }}` with
`transition={{ repeat: Infinity }}` pulses from the first frame. An
`AnimatePresence` with `initial={false}` blocks the mount animation of the
children it first renders, and an element that mounts later animates as usual.
Later `animate` changes still transition normally. If
a target changes while motion is active, the next transition starts from the
current visible value, so reversing an animation does not jump.

A property that `initial` does not name starts from the element's own `style`
value, as Motion for React reads it from the DOM, and otherwise from the CSS
initial value (`0` for padding, margin and radii, `1` for `opacity`,
`transparent` for `backgroundColor`, the element's `color` for the border colours,
`outlineColor` and `textDecorationColor`), or the inherited value for `color`: the parent's colour, or the
scheme's black or white at the root. An unset `width`, `height`, `minWidth`,
`minHeight`, `maxWidth` or `maxHeight` starts from the size layout gave the
element, read back from the previous frame. The measurement only fixes the
numbers: the clock starts at the style change, as for any other animation, so a
measurement that arrives frames later starts the animation that far in.

An unset border width (`borderTopWidth` and the other three sides) starts from the
width a `border` shorthand gives, or `medium` (`3px`), as in Chrome, and so does an
unset `outlineWidth`. The side's style does not enter: a `none` or `hidden` side lays
out with no border whatever its width animates to, but the animation under it already
runs from `3px`, so `animate={{ borderTopWidth: 7 }}` is `5px` halfway with or without a
`borderStyle`.

Where CSS computes an unset property to a keyword there is nothing to blend from:
`aspectRatio` is `auto` and `lineHeight` is `normal` (or a length, for a `motion`, which
takes a number only), and as in CSS those begin at their target. The unset `top`, `right`, `bottom`, `left` and `flexBasis` are `auto` too,
but `auto` is a value of its own there: it flips with a length, see below.
`fontSize` and `fontWeight` are inherited, so an unset one starts from the
computed value: the parent's (`fontSize: "2em"` computes against it as well),
or the initial `16px` and `400` at the root. So are `textUnderlineOffset`, `lineHeight`
(a number, which is what a `motion` takes), `visibility`, `borderSpacing` and the
other table properties, and `pointerEvents`: an unset one starts from the parent's.

### Targets and timing

Any `StyleDesc` longhand whose value is a number, a length, a colour or a shadow
list can be animated, in `animate`, `initial` and `exit`:

| Target | Range or unit |
|---|---|
| `width`, `height`, `minWidth`, `minHeight`, `maxWidth`, `maxHeight` | any length, zero or greater, or `"auto"` (see below) |
| `top`, `right`, `bottom`, `left` | any length, may be negative, or `"auto"` (discrete, see below) |
| `marginTop`, `marginRight`, `marginBottom`, `marginLeft` | any length, may be negative, or `"auto"` (discrete, see below) |
| `paddingTop` ... `paddingLeft` | any length, zero or greater |
| `flexBasis` | any length, zero or greater, or `"auto"` or `"content"` (discrete, see below) |
| `rowGap`, `columnGap`, the four `border*Radius` corners | any length, zero or greater |
| `borderTopWidth` ... `borderLeftWidth`, `outlineWidth`, `fontSize` | any length without a `%` (a `fontSize` `%` is of the parent's, so px), zero or greater |
| `outlineOffset` | any length without a `%`, may be negative |
| `scrollbarWidth`, `lineHeight` (a number), `flexGrow`, `flexShrink`, `aspectRatio` | numbers, zero or greater |
| `opacity` | `0` through `1` |
| `zIndex` | an integer: it blends as a real and rounds half up, as in Chrome (`0` to `10` is `3` a quarter in and `8` at three quarters); an unset one is `auto`, which does not blend |
| `borderSpacing` | one or two lengths without a `%`, zero or greater; both blend, and layout is given whole pixels (Chrome keeps the spacing in a `short`, so `2.5px` is `2px`) |
| `verticalAlign` | a length or a percentage of the line height (a length and a percentage blend as a `calc()`); a keyword (`baseline`, `sub`, `top`, ...) is discrete against a length and another keyword |
| `fontWeight` | `1` through `1000`, or a keyword such as `"bold"` |
| `fontVariationSettings` | `'"wght" 650, "wdth" 90'`, or `normal` |
| `textUnderlineOffset` | `auto`, or any length or percentage, may be negative |
| `textDecorationThickness` | `auto`, `from-font`, or any length or percentage; a negative one paints a device pixel, as zero does |
| `backgroundColor`, `color`, `borderColor`, `borderTopColor` ... `borderLeftColor`, `outlineColor`, `textDecorationColor`, `scrollbarColor`, `selectionColor` | any colour GPUIX parses |
| `boxShadow` | one shadow or a list |

`padding`, `margin`, `inset`, `borderWidth`, `borderColor`, `borderRadius`, `gap` and `flex` are shorthands that
expand to their longhands, so `animate={{ borderRadius: 8 }}` also overrides a
`borderTopLeftRadius` in `style`. When a description names a shorthand and one of
its longhands, the one written later wins, as in CSS. `flex` is `flexGrow`, `flexShrink`
and `flexBasis`, as CSS animates it, written as it is in a style (`"1 1 0px"`, a lone
number `n` is `n 1 0%`): `"1 1 0px"` to `"3 1 100px"` is `"2 1 50px"` halfway. (The logical
shorthands such as `paddingBlock` are not targets: animate the sides. A motion value cannot be
bound to `flex`: bind the three longhands.)

A shorthand written with several values gives each side, axis or corner its own
value, and each animates on its own:

```tsx
<motion.div
  initial={{ padding: '4px 8px', borderRadius: '10px 20px / 5px' }}
  animate={{ padding: '12px 24px 12px 8px', borderRadius: '50%' }}
/>
```

A corner moves both its radii, so `"50%"` is an ellipse on a wide box and
`"10px / 20px"` to `"30px / 40px"` is `"20px / 30px"` halfway. Keyframes of such
strings split frame by frame.

**`auto` margins, insets and `flexBasis`.** CSS cannot interpolate `auto` with a length,
so a pair of them is discrete: `marginLeft: "auto"` to `100` is `auto` until the eased progress
reaches a half and `100px` after, in `animate`, keyframes and CSS animations, and a CSS
transition runs it only under `transition-behavior: allow-discrete` (otherwise it jumps).
`flexBasis: "content"` is a keyword of the same kind, discrete against a length and against `auto`.
An unset margin is `0`, which blends, and an unset inset or `flexBasis` is `auto`, which flips.
`margin: "0 auto"` gives each side its own.

Naming a discrete property (`display`, `position`, `overflowX`, `pointerEvents`, `tableLayout`, ...) or an
unknown key is an error, not a silent no-op; a CSS transition runs them (see
[CSS transitions](#css-transitions)).

**Lengths.** A length is its px and its percentage, whatever unit it is written
in, and two lengths blend part by part, as Chrome blends them: `calc(100% - 20px)`
to `calc(50% + 10px)` is `calc(75% - 5px)` halfway, and `20px` to `50%` is
`calc(10px + 25%)`. `em`, `rem` and the viewport units are px by then, from the
element's font size and the window (a motion value keeps `em` and `rem` until it is
applied, see [Motion values](#motion-values)). A `calc()` that comes out negative is clamped,
and a target that is negative whatever the basis is an error, as is a `%` where CSS
takes none (the border widths and the outline). A spring or a retarget keeps its
speed on each part. The percentage stays a share of its basis, so a parent that
resizes mid-animation is followed exactly and nothing is measured. The basis is
the containing block, or the border box for a corner radius. `height` and `width`
also move to or from `"auto"`:

- The sizes, insets, margins, padding and `flexBasis` resolve a `%` against the
  parent's content box, along the axis CSS uses: `width`, `minWidth`, `maxWidth`,
  `left`, `right`, all four margins and all four paddings against its width
  (`marginTop` and `paddingTop` too, as in CSS), `height`, `minHeight`,
  `maxHeight`, `top` and `bottom` against its height, and `flexBasis` against the
  main size of the flex container (its height in a `column`, its width otherwise).
  The percentage follows the parent if it resizes mid-flight, and the style
  settles on the unit you asked for. A root element has no parent, so it resolves
  against the window's content size, as the initial containing block does in a
  document. An absolute box resolves against its nearest positioned ancestor's
  padding box in layout, but motion measures the parent's content box, so the two
  differ by the padding of that parent (and by the ancestor, when the parent is
  not positioned).
- A `%` `height` follows CSS when the parent's height is not definite: it behaves
  as `auto`, so the animation runs to the content height (and the style settles on
  the `%`, which lays out as `auto`). A height is definite when it is in px, a `%`
  that itself resolves, an absolute box with `top` and `bottom`, or a flex item
  its definite container sizes (the main size of a column, the stretched cross
  size of a single-line row). Grid items are never counted as definite, so a `%`
  height in a grid cell with fixed rows runs to the content height. An absolute
  or fixed box always resolves its `%` height. `minHeight` and `maxHeight` keep
  their share of the parent's height.
- `height: "auto"` is the height the content wants. The target follows the
  content while it grows (a spring keeps its velocity), the end is rounded up to
  a whole device pixel so nothing clips, and the style goes back to a literal
  `auto` when the animation settles. `height: 0` to `"auto"` and back is an
  accordion. An element that is not painted (inside a `display: none` subtree)
  has nothing to measure and jumps to its target.
- `width: "auto"` is the same for the width the content wants: `width: 0` to
  `"auto"` and back is a horizontal accordion. It animates through `motion`, CSS
  transitions and CSS animations. Transitions and animations need
  `interpolateSize: "allow-keywords"`, as for `height`, and `animate` is not gated
  by it. The content's width needs an extra layout of the element's subtree, so
  an element that can animate `width` to or from `auto` pays one more layout per
  frame, and the natural width is measured one frame after the element is first
  painted. The clock still starts at the style change.

Only `auto` and an unset start (see above) need a measurement; every other
length, `calc()` and mixed unit included, is exact from the first frame.

**Colours** interpolate with premultiplied alpha, so a fade to `transparent`
keeps its hue. The space depends on who animates. In `motion` (`animate`,
springs, motion values) every pair blends in Oklab, like
`color-mix(in oklab, ...)`: a red to blue move does not pass through a dull
purple. CSS transitions and CSS animations follow CSS Color 4 instead: when both
colours of a pair are written in a legacy syntax (hex, a name including
`transparent`, `rgb()`, `rgba()`, `hsl()`, `hsla()`) they blend in sRGB, so
`red` to `blue` passes through `#800080`; any other pair (`oklch()`, `lab()`,
`oklab()`, `hwb()`, `color()`, `color-mix()`, or a legacy colour against one of
those) blends in Oklab. Each pair of neighbouring keyframes picks its own space,
`light-dark()` has the syntax of the side it picks, and `currentColor` counts as
a modern syntax. A transition interrupted midway continues in the space its
current colour was blended in. Overshoot is clamped to sRGB.
Targets and keyframes take any colour a style does, and they resolve in the
element's colour context: `currentColor` is the element's own `color` (the
parent's for `color` itself), `light-dark()` follows its `colorScheme`, and
`color-mix()` and `color()` work too. A new `color`, `colorScheme` or window
appearance resolves them again and the track runs to the new colour. CSS
animation keyframes and `<svg>` `fill` and `stroke` do the same. A CSS transition
judges a change by computed values, as browsers do: `light-dark()` computes to
the colour its scheme picks, so a flip of `colorScheme` or of the window
appearance starts a transition on a property that has one (or restarts a running
one whose end it changes, from the value showing); a running transition whose end
colour is the same after the flip is left alone. `currentColor` computes to
itself, so a new `color` starts no transition of its own on a `currentColor`
property: it follows the colour showing each frame, including a `color`
transition.
`hover` and `active` colours of an ancestor are not part of that context. A
motion value is not tied to one element, so its colours resolve as on a root with
`colorScheme: "light dark"` and keep what they resolved to.

**Box shadows** animate per layer (offset, blur, spread and colour). When the
lists differ in length, the shorter one is padded with `0 0 0 0 transparent`
layers, as CSS does. If a layer's `inset` differs between the two lists, the
shadow switches at the halfway point instead of interpolating.

**`fontVariationSettings`** blends per axis between two values that name the same
axes (in any order). Any other pair, `normal` against a list included, is discrete
and flips at the halfway point. `animate`, keyframes and `@keyframes` flip with no
opt-in; a CSS transition runs the flip only with `allow-discrete`
(`transition: 'font-variation-settings 1s allow-discrete'`). A keyframe track of three or more
values flips as a whole when any two of its values differ. An unset start is the
inherited value. A `<span>` animates it too, by `motion.span`, a CSS transition or a CSS animation: the
paragraph is laid out again each frame.

**`textUnderlineOffset` and `textDecorationThickness`** blend between two lengths (a percentage is of `1em`
and blends with px as a `calc()`), on an element and on a `<span>`. `auto` and `from-font` are no length, so a
pair that has one flips at the halfway point, and a CSS transition runs that flip without `allow-discrete`, as
Chrome's does for these two. An unset thickness starts from `auto`, which is not inherited. An unset offset starts
from the offset it inherits, which is `auto` when no ancestor sets one. The painted thickness is a whole number of
device pixels (see [Inline text](#inline-text)), so an animated one steps from line to line, and a negative
thickness (which an animation can pass through) paints one device pixel, as zero does.

The **transition** uses seconds, like Motion for React:

| Option | Default | Values |
|---|---:|---|
| `duration` | `0.3` | Non-negative seconds |
| `delay` | `0` | Non-negative seconds |
| `ease` | `"easeOut"` | `"linear"`, `"ease"`, `"easeIn"`, `"easeOut"`, `"easeInOut"`, `[x1, y1, x2, y2]`, or a CSS timing function string (`"ease-in"`, `"steps(4, jump-start)"`, `"linear(0, 0.25 75%, 1)"`) |

**Per-property transitions** use Motion's shape. A property uses
`transition[property]`, then `transition.default`, then the curve fields at the
top level, then the 0.3 s `easeOut` tween. Each level replaces the next as a
whole, it is never merged, and every property keeps its own `delay`.

```tsx
<motion.div
  animate={{ opacity: 1, backgroundColor: '#3b82f6', paddingLeft: 24 }}
  transition={{
    duration: 0.3,
    opacity: { duration: 0.15, ease: 'linear' },
    paddingLeft: { type: 'spring', stiffness: 300, damping: 24 },
  }}
/>
```

A key in `transition` that is neither a curve field, `default`, nor an animatable
property is an error. Changing the target or transition of one property
restarts only that property, from where it is now; the others keep running.
A property left out of a later `animate` keeps running toward its old target.

Variants and shared layout animations are not available yet. See
[Keyframes](#keyframes) for lists of values.
**Exit** uses `AnimatePresence`, like Motion for React.

### Springs

Set **`type: "spring"`** to animate with a damped spring instead of a timed
curve. Rust evaluates the spring analytically each frame, so it does not depend
on the frame rate, and it needs no React render or N-API call per frame.

```tsx
<motion.div
  animate={{ top: row * 32 }}
  transition={{ type: 'spring', duration: 0.16, bounce: 0 }}
/>
```

Describe a spring one of two ways. Mixing the two forms is an error.

| Form | Options | Defaults |
|---|---|---|
| Physics | `stiffness`, `damping`, `mass` | `100`, `10`, `1` |
| Duration | `duration` (seconds to settle, `0.01` to `10`), `bounce` (`0` to `1`) | `0.8`, `0.3` |

`bounce: 0` is critically damped, so it settles without overshoot; larger values
oscillate longer. The duration form is converted to stiffness and damping with
the same formula as Motion's `spring()`. `stiffness`, `damping`, and `mass` must
be greater than `0`; `sqrt(stiffness / mass)` and `damping / (2 * mass)` must be
at most `1e6`.

These apply to every spring:

| Option | Default | Values |
|---|---:|---|
| `delay` | `0` | Non-negative seconds. The element holds its start until the delay passes |
| `restDelta` | `0.5`, or `0.005` when the move is under 5 | Distance from the target, in the property's unit, under which the spring may settle. The percentage part of a length is in percentage points, and its defaults are the tight ones |
| `restSpeed` | `2`, or `0.01` when the move is under 5 | Speed, in units per second, under which the spring may settle (percentage points per second for the percentage part) |

`ease` is not accepted with `type: "spring"`, and the spring options are not
accepted without it.

A spring **keeps its speed when the target changes**. Moving `top` from one row
to the next while it is still travelling bends the path instead of stopping and
restarting, which is what makes a hover highlight glide between rows. A tween
that is interrupted by a spring hands over its current speed as well. A spring
interrupted by a tween starts the tween from rest.

Springs overshoot, so the applied value is clamped to the property's range
(`opacity` to `0..1`, lengths that cannot be negative to zero or greater, colours
to sRGB), while the spring itself keeps the unclamped value. Margins, insets and
shadow offsets are not clamped. Targets are still validated as before. A colour
or shadow spring carries its speed per channel when the target changes. The motion settles, and `onMotionComplete` fires, once
every animated value is within `restDelta` and `restSpeed` of its target.

`AnimatePresence` exit works with springs: the child leaves after the spring
settles. Transform properties such as scale and translate are not animatable
yet, so an enter that scales an element cannot be expressed.

### Keyframes

A property in `animate` or `exit` takes a list of values instead of one. It
moves through them in order over one `duration`, spread evenly.

```tsx
<motion.div
  animate={{ width: [0, 120, 80], opacity: [0, 1, 0.6] }}
  transition={{ duration: 1, ease: 'linear' }}
/>
```

A `null` stands for the keyframe before it, or for the value the property
starts from when it comes first: `width: [null, 100, 60]` leaves from wherever
the element is. A list of one value is that value. The same list works in
`exit`, which leaves `AnimatePresence` once the last keyframe plays, and on
`motion.path`, `motion.circle` and the other SVG shapes
(`strokeDasharray: ['4', '2']` is two keyframes).

| Option | Default | Values |
|---|---:|---|
| `times` | evenly spaced | One number per keyframe, from `0` to `1` and ascending. Keyframes before the first or after the last time hold their value |
| `ease` | `"easeOut"` | One easing for every segment, or a list with one per segment (one fewer than the keyframes) |
| `repeat` | `0` | Extra iterations after the first. `Infinity` loops for ever and `onMotionComplete` never fires |
| `repeatType` | `"loop"` | `"loop"` plays every iteration from the first keyframe, `"reverse"` plays odd iterations backwards in time (their easing runs backwards too), `"mirror"` plays them through the keyframes in reverse order with the same timing and easing |
| `repeatDelay` | `0` | Seconds held on the last value between iterations |

`duration` is the length of one iteration. A `times` or `ease` list in a shared
transition (the top level or `default`) applies to the properties whose
keyframes it fits and is left out for the others, which use the rest of the
transition (an `ease` list left out becomes the default ease): in `animate={{ width: [0, 50, 100], height: 10 }}`
with `times: [0, 0.2, 1]`, only `width` follows the times. To give a property
its own, name it: `transition={{ duration: 1, width: { times: [0, 0.2, 1] } }}`.
That list must fit the property's keyframes: one of the wrong length makes the
description invalid, so the element does not animate and the renderer logs a
warning. A spring animates between two keyframes only and takes `repeat` but not
`times` or an `ease` list.

```tsx
<motion.div
  animate={{ opacity: [0.4, 1] }}
  transition={{ duration: 0.8, ease: 'easeInOut', repeat: Infinity, repeatType: 'mirror' }}
/>
```

A list is keyframes unless it is itself one value of the property: a shadow
list, or a list of numbers for `strokeDasharray`. Keyframes of those are a list
of lists. `animate(value, [..], transition)` on a [motion value](#motion-values)
takes the same lists and transition fields.

### Animate SVG shapes

`motion.path`, `motion.circle`, `motion.ellipse`, `motion.rect`, `motion.line`,
`motion.polyline`, `motion.polygon`, `motion.g`, `motion.image`, `motion.text`,
`motion.tspan`, `motion.feGaussianBlur`, `motion.feOffset`, `motion.feFlood` and
`motion.feDropShadow` animate the attributes of a shape inside an `<svg>`. They
take the same `initial`, `animate`, `exit` and `transition` as `motion.div`,
including per-key transitions, springs and `AnimatePresence`, but the keys are
SVG attributes instead of style.

```tsx
<svg viewBox="0 0 100 100" width={100} height={100} style={{ color: '#5ca9ff' }}>
  <motion.path
    d="M10 50 L90 50"
    stroke="#000"
    strokeWidth={8}
    initial={{ pathLength: 0 }}
    animate={{ pathLength: 1 }}
    transition={{ duration: 1, ease: 'easeInOut' }}
  />
</svg>
```

| Key | Value |
|---|---|
| `pathLength`, `pathOffset`, `pathSpacing` | Numbers, Framer's normalised draw: `pathLength` is the stroked fraction of the outline (default 1), `pathOffset` moves where the stroke starts, `pathSpacing` is the gap after it (default 1, one dash). They set the dash for you, so do not combine them with `strokeDasharray` |
| `strokeDasharray`, `strokeDashoffset` | A number, a list or `"4 2"`, and a number. Lists of different lengths are repeated to a common length |
| `strokeWidth`, `opacity`, `fillOpacity`, `strokeOpacity` | Numbers. Opacities are `0..1` |
| `fill`, `stroke`, `floodColor` | Colours, interpolated like `backgroundColor` (premultiplied alpha, Oklab in `motion`). `none` counts as transparent |
| `floodOpacity` | A number, `0..1` |
| `cx`, `cy`, `r`, `rx`, `ry`, `x`, `y`, `width`, `height`, `x1`, `y1`, `x2`, `y2` | Numbers in user units |
| `stdDeviation`, `dx`, `dy`, `refX`, `refY`, `fontSize`, `letterSpacing` | Numbers in user units. `stdDeviation` blurs by one number on both axes |
| `d` | Path data. Both paths are made absolute first; with the same commands every number is interpolated, otherwise the value switches halfway through the progress (or when a spring passes halfway) |

A key that `initial` does not name starts from the element's own attribute, and
then from the SVG default (`fill` black, `stroke` transparent, opacities 1,
`strokeWidth` 1, `stdDeviation` and `dx` 0). `r`, `width`, `fontSize` and similar have no default, so an animation of
one that the element does not declare jumps to its target. Naming a key that is
not in the table, or a style such as `backgroundColor`, is an error.

#### CSS keyframes on SVG properties

[CSS animations](#css-animations) animate SVG properties too, without `motion`:
a shape's `style` takes `animation` (and its longhands), and `defineKeyframes`
takes the SVG properties CSS animates. This is the way to write a stroke that
draws itself in:

```tsx
defineKeyframes('draw', { from: { strokeDashoffset: 180 }, to: { strokeDashoffset: 0 } })

<svg viewBox="0 0 200 100" width={200} height={100}>
  <path
    d="M10 50 L190 50"
    stroke="#000"
    strokeWidth={4}
    strokeDasharray={180}
    style={{ animation: 'draw 1s ease-out forwards' }}
    onAnimationEnd={() => console.log('drawn')}
  />
</svg>
```

- **Properties.** `fill`, `stroke`, `strokeWidth`, `strokeDasharray`,
  `strokeDashoffset`, `strokeOpacity`, `strokeMiterlimit`, `fillOpacity`,
  `opacity`, `floodColor`, `floodOpacity`, `stopColor`, `stopOpacity`,
  `fontSize`, `letterSpacing`, `cx`, `cy`, `r`, `rx`, `ry`, `x`, `y`, `width`,
  `height` and `d` (path data, or `path("...")` as in CSS), each on the elements
  a browser applies it to. A value is a number in user units (a `"12px"` string
  is read as 12), a colour, a dash list or path data, as for `motion`.
  `pathLength`, `pathOffset` and `pathSpacing` are Framer's draw keys, and `x1`
  to `y2`, `stdDeviation`, `dx`, `dy`, `refX` and `refY` are attributes and not
  CSS properties: `defineKeyframes` refuses them. A name that is both a style and
  an SVG property (`opacity`, `width`, `height`, `fontSize`) is the SVG one on a
  shape and the style on any other element; a property with no meaning on the
  element it reaches (`r` on a `<div>`, `backgroundColor` on a `<circle>`) is
  ignored, as a browser computes it and never paints it
- **Where it runs.** On a shape, a group or any other element in the `<svg>`
  (`style` takes the `animation` properties only, as the shape is painted by its
  `<svg>`), and on the `<svg>` itself. The `<svg>`'s values are written on its
  root, so the shapes inherit them as in CSS (`fill`, `stroke`, `strokeWidth`,
  the dash properties and `fontSize` inherit; a shape's own attribute wins over
  an inherited value). A shape's animation beats its attribute, its group's
  value and its own `motion`. A missing `from` or `to` is the value the shape
  has without the animation: its attribute, else the nearest ancestor's for an
  inherited property (`fontSize` falls back to the font the `<svg>` inherits,
  16px by default), else CSS's initial value. That is `0` for `strokeDashoffset`,
  `cx`, `cy`, `r`, `x`, `y` and `letterSpacing`, `1` for `strokeWidth` and the
  opacities, `4` for `strokeMiterlimit`, black for `fill`, `stopColor` and
  `floodColor`, and `none` for `stroke`, `strokeDasharray` and `d`. `rx`,
  `ry`, `width` and `height` are `auto`
- **Events and Web Animations.** `onAnimationStart`, `onAnimationEnd` and the
  rest fire from the shape and bubble, and `getAnimations()` on its ref lists its
  `CSSAnimation`s, which can be paused, played and sought
- **Paint.** A paint of `none` against a colour swaps halfway, as in Chrome (it
  does not fade, as in `motion`). A property whose value is `none` or `auto`
  (a missing endpoint of `strokeDasharray`, `d`, `rx`, `ry`, `width` or
  `height` included) is discrete against one that has a number, a dash list or
  a path. Dash lists of different lengths repeat to a common length, and
  paths blend when their commands match and swap halfway otherwise

Not built: CSS transitions of SVG properties (a changed prop jumps; use `motion`
or an animation), and the attributes that are not CSS properties above.

Because the `<svg>` is rasterised as one image, an animated shape paints a new
image every frame. The `<svg>` keeps the image it last painted and drops it from
the texture atlas when it makes the next one, so an animation holds one atlas
entry per `<svg>`, not one per frame. The cost is one rasterisation per frame, in the
frame for a small `<svg>` and on a background thread for a large one.

### Gestures and variants

Every `motion.*` (`motion.div`, `motion.span` and the SVG shapes) takes Framer's
gesture props, and answers to them natively, with no render per hover or press.

```tsx
<motion.div
  style={{ width: 100, height: 40, backgroundColor: '#ff0000' }}
  whileHover={{ width: 200, backgroundColor: '#0000ff' }}
  whileTap={{ scale: 0.95, transition: { type: 'spring' } }}
  transition={{ duration: 0.3 }}
  onHoverStart={(event, info) => console.log(info.point)}
/>
```

- **`whileHover`, `whileTap`, `whileFocus`, `whileInView`.** Each is a target (a
  style object with an optional `transition` for its own animation, or variant
  names) that the element animates to while the gesture is on it, and back from
  when it ends, to its `animate` value or, if that does not name the property, to
  its `style`. Hover is `:hover` (the pointer is over the element or what it
  holds), tap is a primary press that began on it, whatever is under the pointer
  when it is released, and focus is `:focus-visible`: a mouse press does not
  match, Tab does, and the element needs a `tabIndex` to take focus. When several
  apply to a property the order of priority is Framer's, lowest first: `animate`,
  `whileInView`, `whileFocus`, `whileHover`, `whileTap`. An element that is leaving
  `AnimatePresence` answers to none of them: what they moved goes back as the exit
  takes over (Framer lets the active gestures go on with the properties `exit` does
  not name).
- **`whileInView`** is on while any of the element is in the viewport. It watches
  the element with an `IntersectionObserver` and `viewport` takes Framer's options:
  `once`, `root` (a ref), `margin` (the `rootMargin`) and `amount` (`"some"`,
  `"all"` or a fraction). `onViewportEnter(entry)` and `onViewportLeave(entry)`
  are called as it enters and leaves (never leaves with `once`).
- **Callbacks.** `onHoverStart` and `onHoverEnd` (`pointerenter` and
  `pointerleave`), `onTapStart` (a primary press on the element), `onTap` (the
  press ends on the element or what it holds) and `onTapCancel` (it ends
  elsewhere, or is cancelled), each called with `(event, { point })`, `point`
  being the pointer's window position. They run beside the `onPointer*` handlers
  you write, not instead of them. Framer's keyboard tap (Enter on a focused
  element, which also adds `tabindex="0"` to an element with `whileTap`) is not
  built.
- **`variants`** name targets: `initial="hidden" animate="shown"`, a list of names
  (later ones win), a function of `custom` as a variant, and a `transition` inside
  a variant, which the animation to that variant plays with in place of the
  component's. A name that a component with `variants` does not define throws; a
  component with none only hands its names down.
- **Names go down.** A component whose `initial`, `animate`, `exit` or `while*` is
  a name controls variants, and hands those names to the `motion.*` components in
  it that have `variants` and do not control any themselves. They take the names
  they leave out: `initial` and `animate` as they change, `exit` while the
  component leaves, and a `while*` while the controlling component's gesture is
  on, whatever the pointer is over: hover the parent and the child with the same
  names moves. A name that a child's `variants` do not define is skipped. A
  child's own `while*` wins over the name it was handed, and answers to its own
  gesture. A component that writes a name of its own, or `inherit={false}`, breaks
  the chain; a name in an object (`animate={{ ... }}`) does not. Framer's
  `staggerChildren`, `delayChildren` and `when` are not built: every child animates
  at once.
- **SVG shapes** take `variants` and the names handed down to them, so an icon
  moves when the button around it is hovered. A shape takes no pointer or focus
  event of its own (the `<svg>` is one image), so its own `whileHover`,
  `whileTap` and `whileFocus` never turn on; `whileInView` and the viewport
  callbacks need a box a shape does not report.

### Animate an inline span

`motion.span` animates what an inline `<span>` shows inside text, with the
`initial`, `animate`, `exit`, `transition` and `variants` of Framer's
`motion.span`, and answers to `whileHover`, `whileTap`, `whileFocus` and
`whileInView` (see [Gestures and variants](#gestures-and-variants)). It is available in `@gpuix/react` and `@gpuix/solid`, and the type
of its props is `MotionSpanProps`.

```tsx
<text style={{ color: '#e2e2e2' }}>
  Saved{' '}
  <motion.span
    initial={{ backgroundColor: '#ffe58a00', color: '#e2e2e2' }}
    animate={{ backgroundColor: '#ffe58a66', color: '#ffffff' }}
    whileHover={{ paddingLeft: 8, paddingRight: 8, borderRadius: 4 }}
    transition={{ duration: 0.4 }}
  >
    3 files
  </motion.span>
</text>
```

A span is text that its paragraph paints, not a box of its own, so it animates
what CSS lets an inline box show and gpuix's inline model draws: `opacity`, the
colours (`color`, `backgroundColor`, `textDecorationColor`, `outlineColor`,
`borderColor` and the four `border*Color` sides), the font of its text
(`fontSize`, `fontWeight`, `fontVariationSettings`, `lineHeight`, `verticalAlign`), its `padding`,
`margin` and `borderWidth` (with their sides), `borderRadius` (and its corners),
`outlineWidth` and `outlineOffset`, and `textUnderlineOffset` and
`textDecorationThickness`. They blend as on a `motion.div`, and run natively
without a render per frame, in the order CSS uses (the `style`, then CSS
transitions, then `motion` and motion values, then CSS animations). `initial` is
on the first frame, so nothing flashes the `animate` value first. A key of any
other property (`width`, `display`, `flexGrow`, ...) in `initial`, `animate`,
`exit`, a `while*` target or a variant **throws and names the property**, and
TypeScript rejects it. `opacity` fades the box as a group (see
[Painting an inline box](#painting-an-inline-box)).

The padding, margin, border widths and font of a span take space along the line,
so while one of them moves the paragraph is laid out again every frame: the text
after the span moves, a line that no longer fits wraps and the paragraph grows,
as in a browser. The colours, opacity, radii and outline do not change the layout.
(Letter and word spacing and `textShadow` are not drawn on an inline box, so there
is nothing to animate.)

- **Gestures and variants** are those of every `motion.*` (see
  [Gestures and variants](#gestures-and-variants)); a span's hover, tap and
  focus are the states of the `hover`, `active` and `focusVisible` entries of its
  `style`, with no entry needed, and focus needs a `tabIndex`.
- **`exit` inside `AnimatePresence`** works as on a `div`: the span stays
  mounted until its exit has finished, and is then removed. Without an `exit` it
  is removed at once.
- **Motion values in `style`.** A motion value in the `style` of a span (of a
  `motion.span` or a plain `<span>`) drives any of the properties above, from the
  same native sample as on an element. Binding one to any other property throws and
  names it.
- **Transforms, layout and drag have no effect.** A transform does not apply to a
  non-atomic inline box in CSS, so the transform keys (`x`, `y`, `scale`, `rotate`,
  `skew`, ... in `animate`, a target or `style`) are accepted and ignored, and so
  are `layout`, `layoutId`, `layoutDependency`, `onLayoutAnimation*`, `whileDrag` and
  Framer's `drag*` props: a layout animation and a drag move a box with a transform,
  which a span cannot show, so no layout animation runs and none of its callbacks is
  called. (`draggable` and the DOM `onDrag*` events are the ordinary ones.) A
  `Reorder.Item` rendered `as` a span renders and does not reorder.
- A span that is a flex or grid item, or has `display: block`, is laid out as an
  element and paints its colours the same way, but still takes only the keys above.

### Animate a sidebar

Animate an **outer clipping container** and keep the inner sidebar at a fixed
width. This reveals or hides the content without reflowing its text on every
frame.

```tsx
import { motion } from '@gpuix/react'
import type { ReactNode } from 'react'

function SidebarFrame({
  collapsed,
  children,
}: {
  collapsed: boolean
  children: ReactNode
}) {
  const sidebarWidth = 252
  const dividerWidth = 1

  return (
    <motion.div
      initial={false}
      animate={{ width: collapsed ? 0 : sidebarWidth + dividerWidth }}
      transition={{ duration: 0.2, ease: 'easeOut' }}
      style={{
        display: 'flex',
        flexDirection: 'row',
        height: '100%',
        flexShrink: 0,
        overflow: 'hidden',
      }}
    >
      <div style={{ width: sidebarWidth, height: '100%', flexShrink: 0 }}>
        {children}
      </div>
      <div style={{ width: dividerWidth, height: '100%', flexShrink: 0 }} />
    </motion.div>
  )
}
```

The **chat example** uses this pattern. The sidebar remains mounted while its
outer width moves between `253` and `0` pixels.

### Animate unmount

A `motion.div` with **`exit`** only leaves after that target finishes, and only
when it is a child of **`AnimatePresence`**. Without `AnimatePresence`, React
destroys the node on the same commit.

```tsx
import { AnimatePresence, motion } from '@gpuix/react'

function Toast({ show }: { show: boolean }) {
  return (
    <AnimatePresence>
      {show ? (
        <motion.div
          key="toast"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.2, ease: 'easeOut' }}
        >
          <text>Saved</text>
        </motion.div>
      ) : null}
    </AnimatePresence>
  )
}
```

Give every child a **unique `key`** when more than one child can leave. Set
**`initial={false}`** on `AnimatePresence` to skip enter on the first paint.
A child with no `exit` is removed without a tween. Exit still completes when
the target already matches, when the node is outside a virtual list's painted
window, or when invalid runtime data makes the native target unusable.

`AnimatePresence` also accepts **`onExitComplete`**, which runs after every
leaving child is gone. `useIsPresent()` reports whether a custom descendant is
leaving. `usePresence()` returns the same status plus `safeToRemove`, for a
custom exit that decides when the retained child can unmount. Any component
below a child of `AnimatePresence` can call them, at any depth: while the child
leaves, `isPresent` is false there, and a child whose descendants called
`usePresence()` stays until each of them has called `safeToRemove` (and until its
own `exit` animation, if it has one, is done). A caller that never calls
`safeToRemove` keeps the child for good; unmounting it counts as done. Outside an
`AnimatePresence` the status is always present.

In Solid both return an accessor (`usePresence()` is `[isPresent, safeToRemove]`
with `isPresent: () => boolean`). Solid disposes the effects of whatever a `Show`,
a `For` item or `{cond && <Item/>}` stops rendering, so inside an
`AnimatePresence` each component runs in a scope of its own that is kept until
the child it sits in is removed: effects and `onCleanup` of a leaving child's
descendants run on until then, and `useIsPresent()` can be read in an effect or
in JSX. A component that renders nothing is placed in the child by the element
written around it. Elements and effects written directly in a `Show` or `For`
callback (outside a component) are not kept: they stop updating when the child
leaves, as before.

```tsx
import { usePresence } from '@gpuix/react'

function DeferredRemoval() {
  const [isPresent, safeToRemove] = usePresence()

  return (
    <div onClick={() => !isPresent && safeToRemove?.()}>
      <text>{isPresent ? 'Ready' : 'Click to remove'}</text>
    </div>
  )
}
```

#### `mode`, `custom` and `popLayout`

**`mode`** decides how entering and leaving children overlap.

| `mode` | Behaviour |
| --- | --- |
| `"sync"` (default) | Entering and leaving children animate together. |
| `"wait"` | A child that enters while another is leaving renders only after every exit is done, then `onExitComplete` runs. Children that stay keep rendering. |
| `"popLayout"` | A leaving child is taken out of flow at once, so its siblings reflow while it fades. |

```tsx
<div style={{ display: 'flex', flexDirection: 'column' }}>
  <AnimatePresence mode="popLayout">
    {items.map((item) => (
      <motion.div
        key={item.id}
        layout
        exit={{ opacity: 0 }}
        transition={{ duration: 0.2 }}
      >
        <text>{item.label}</text>
      </motion.div>
    ))}
  </AnimatePresence>
</div>
```

`popLayout` reads the box the child last painted, and its parent's, through
`getElementBounds`, then sets `position: 'absolute'` with `top`, `left`, `width`
and `height` relative to the parent. As in CSS, an absolute child is placed
against its nearest positioned ancestor, so give the parent `position:
'relative'`. The parent's border is not subtracted. The child must take `style` (a `motion.*` or a host
element) and, in React, a `ref`; a custom component that spreads neither is not
pinned. Add `layout` to the siblings, as above, to glide them into the gap
instead of jumping. Bring a leaving child back and it returns to flow.

A window has one root element, so put `AnimatePresence` inside a `div` when more
than one child can be present.

**`custom`** is handed to the `exit` of every child that leaves, which may be a
function. The `custom` of `AnimatePresence` is the one read when the child
leaves, so it can change in the same render that removes the child. A
`motion.*` can also take `custom` itself, which is used when `AnimatePresence`
has none. An `exit` function is only called inside an `AnimatePresence`.

```tsx
<AnimatePresence custom={direction}>
  {page ? (
    <motion.div
      key={page}
      initial={false}
      animate={{ opacity: 1 }}
      exit={(direction) => ({ opacity: direction === 'left' ? 0.2 : 0.8 })}
    />
  ) : null}
</AnimatePresence>
```

Variants do not exist in GPUIX, so `exit` is either a target or a function of
`custom` that returns one.

`motion.div` accepts **`onMotionComplete`**. It runs when the current native
target settles. If React changes the target before an older completion reaches
JavaScript, the stale completion is ignored.

### Motion values

A **motion value** holds a number, a colour string, a CSS length string, a
multi-value string, a list of numbers or box shadows that changes over time. Put one in `style` where a property
`motion` can animate takes a plain value, and the element follows it every
frame in Rust, with no render and no N-API call per frame. `motion.div` accepts
them too. A bound value is applied after `animate` and `transition`.

```tsx
import { animate, useMotionValue, useTransform } from '@gpuix/react'

function Meter() {
  const x = useMotionValue(0)
  const width = useTransform(x, [0, 1], [40, 240])
  const opacity = useTransform(x, [0, 1], [0.3, 1])
  return (
    <div
      style={{ width, opacity, height: 12, backgroundColor: '#4ade80' }}
      onClick={() => animate(x, 1, { duration: 0.4, ease: 'ease-out' })}
    />
  )
}
```

```tsx
import { animate, createMotionValue, createTransform } from '@gpuix/solid'

function Meter() {
  const x = createMotionValue(0)
  const width = createTransform(x, [0, 1], [40, 240])
  const opacity = createTransform(x, [0, 1], [0.3, 1])
  return (
    <div
      style={{ width, opacity, height: 12, backgroundColor: '#4ade80' }}
      onClick={() => animate(x, 1, { duration: 0.4, ease: 'ease-out' })}
    />
  )
}
```

| | React | Solid |
|---|---|---|
| A value you write | `useMotionValue(initial)` | `createMotionValue(initial)` |
| Mapped through ranges | `useTransform(value, inputRange, outputRange, options?)` | `createTransform(value, inputRange, outputRange, options?)` |
| Computed in JS | `useTransform(value, fn)`, `useTransform(values, fn)`, `useTransform(() => ...)` | `createTransform(value, fn)`, `createTransform(values, fn)`, `createTransform(() => ...)` |
| Spring that follows a value, or has its own target | `useSpring(valueOrNumber, config?)` | `createSpring(valueOrNumber, config?)` |
| Speed in units per second | `useVelocity(value)` | `createVelocity(value)` |
| Listen to a value | `useMotionValueEvent(value, event, callback)` | `createMotionValueEvent(value, event, callback)` |
| Outside a component | `motionValue(initial)`, `animate(value, target, transition?)`, `isMotionValue(value)` | the same |

A value made with a hook or primitive lives as long as the component (React) or
the owner (Solid). `motionValue(initial)` has no owner: it keeps its state in
JavaScript and gets a native node while an element, a listener or a derived value
needs it, then gives it back. A value drives one window at a time.

- **Reading and writing.** `get()` reads the value at the current frame, so it
  is right even in the frame an animation started. `set(v)` writes it and leaves
  a running animation alone, as in Motion; `stop()` ends the animation, and
  `jump(v)` writes the value and ends it. To take over mid-flight, call `stop()`
  and then `set(v)`: the next animation starts from where the value is.
  `isAnimating()` and `getVelocity()` (numbers only) read the current state.
- **`animate(value, target, transition?)`** takes the same `transition` as
  `motion` (tweens and springs, in seconds) and a `target`. The target may be a
  list of keyframes (`animate(x, [null, 100, 0], { repeat: Infinity })`, see
  [Keyframes](#keyframes)). `animate(element, ...)` is not supported. It returns
  Motion's playback controls (typed `AnimationPlaybackControlsWithThen`), which
  work on the animation natively, so a paused one costs nothing:
  - `pause()` holds the value where it is and `play()` goes on from there. The
    animation stays in effect, `isAnimating()` is true and no frame is asked for
    until it plays. `play()` on a finished animation starts it again.
  - `time` (seconds since the delay ended, up to the end of the last repeat) and
    `speed` (2 is twice as fast, a negative speed runs backwards and finishes at
    the first keyframe, 0 holds) read and assign. `duration` is one iteration, a
    spring's from the time it takes to settle (`Infinity` if it does not in 20 s),
    and `iterationDuration` adds the delay.
  - `state` is `"running"`, `"paused"`, `"finished"` or `"idle"`.
  - `stop()` keeps the value and ends the controls for good. `cancel()` takes the
    value back to the first keyframe, and a later `play()` runs it again from
    there. `complete()` jumps to the last keyframe (the first, backwards).
    `animate()` on the same value replaces the animation: the old controls go
    idle and cannot be played again.
  - `finished` and `then` resolve when the animation finishes by itself or
    `complete()` ends it, with a new promise when `play()` starts a finished one
    again. After `stop()`, `cancel()` or a replacement they stay pending, and they
    never reject.
  - Controls used before the value is bound to a window apply when the animation
    starts; only `duration` and `iterationDuration` throw, as they are not known
    until then.

  Where it differs from Motion: the `time` setter reads back as set, where Motion
  counts the delay in the time it is given; a `speed` other than 0 after 0 runs
  the animation again without `play()`; `isAnimating()` is false after `stop()`
  and `cancel()` of the controls (Motion leaves it true) and true while a seeked
  or replayed animation runs; `time` stops at the end of the last repeat. A
  paused animation holds its value's node, so stop it when its component goes
  (`useEffect(() => () => controls.stop(), [])`). `startTime` and
  `attachTimeline` are not built.
- **Lengths.** A motion value reads `"20px"`, `"50%"`, `"2em"`, `"calc(100% - 20px)"`
  and the other [length](#lengths) syntaxes as a px part and a percentage part, and
  animates between any two lengths as the mix of the parts: `"20px"` to `"50%"` is
  `"calc(10px + 25%)"` halfway. A binding to `width` follows its parent's size exactly.
  `get()` reads a length back as px, a percentage, a font-relative length or a `calc()`.
  The motion value is not tied to an element, so `em` and `rem` stay multiples of the
  font size and resolve where the value is applied, against each bound element: `em` is
  that element's computed `font-size` (the parent's, for `fontSize` itself) and `rem` the
  root's, so `"2em"` bound to `marginLeft` is `40px` on a `20px` element and `20px` on a
  `10px` one, and `get()` reads `"2em"`. `vw`, `vh`, `vmin` and `vmax` resolve against the
  window when the value is written.
- **Multi-value strings.** `"0.5em 0.75em"`, `"10px / 20px"` or `"red blue"` is held
  and blended component by component. Bound to a shorthand (`padding`, `margin`,
  `inset`, `gap`, `borderWidth`, `borderColor`, `borderRadius`), each side, axis or
  corner follows its own component. Two strings with different numbers of values that
  a shorthand can fill out alike animate instead of jumping: `"8px"` to `"8px 16px"`
  moves the left and right from 8 to 16 and leaves top and bottom at 8. Counts no
  shorthand could pair (`"1px 2px"` against five values) flip at the halfway point.
- **Transforms.** The output range holds numbers, colours or shadows, and the
  input range may descend or repeat a value, which makes a step. `clamp: false`
  extrapolates, and `ease` is one easing or one per segment. A bad range is an
  error when the value is first bound.
- **Springs.** `useSpring(source)` chases another value and carries its speed;
  given a number it has its own target, which `set` moves. The config is
  `stiffness`, `damping` and `mass`, or `duration` and `bounce`, as for a
  `motion` spring.
- **Events.** `change` arrives with the frame that sampled it, `animationStart`
  and `animationComplete` when an animation begins and ends. Only values with a
  listener report to JavaScript. React runs the callback in a batch; Solid runs
  it inside `batch`.

**Not native-linked.** A function-form transform (`useTransform(value, fn)`,
`createTransform(() => ...)`) and `useVelocity`/`createVelocity` are computed in
JavaScript: they listen to `change` events, so they trail their inputs by a
frame and only update while they are bound. A range-form transform and a spring
that follows a value are evaluated in Rust at the same instant as their input, so
two elements driven by transforms of one value never drift apart. A function-form
transform tracks the motion values it reads with `get()`, not React state or
Solid signals.

### MotionConfig and reduced motion

`MotionConfig` sets defaults for every `motion.*` component below it. A nested
`MotionConfig` overrides only the props it sets.

```tsx
import { MotionConfig, motion } from '@gpuix/react'

<MotionConfig transition={{ type: 'spring', duration: 0.4 }} reducedMotion="user">
  <motion.div animate={{ width: open ? 240 : 40, opacity: open ? 1 : 0.5 }} />
</MotionConfig>
```

- **`transition`** is used by a `motion.*` component that has no `transition` of
  its own. A component's own `transition` replaces it whole, it is not merged.
- **`reducedMotion`** is `"never"` (the default, as in Motion for React),
  `"always"`, or `"user"`, which follows `prefers-reduced-motion` and changes
  with the OS setting (see [`matchMedia`](#matchmedia)).

Reduced, `width`, `height`, `top`, `right`, `bottom` and `left` jump to their
target instead of animating, and so do [layout animations](#layout-animations).
Nothing else changes: padding, margin, `gap`, min and max sizes, `flexBasis`,
opacity, colours, shadows, radii and borders still animate with their
transition. This is Motion for React's rule, less the transforms GPUI does not
have. The jump still counts as the animation settling: `onMotionComplete`
fires, and `AnimatePresence` exits complete. Flipping the setting while an
animation runs finishes the jumping properties at once.

`animate()` on a motion value follows the same rule when the value is bound,
through `style` of a `motion.*` component under the config, to `width`,
`height`, `top`, `right`, `bottom` or `left`: the value jumps and `finished`
resolves. Motion for React does not reduce a bare `animate(value)`; gpuix does,
so a bound size follows the setting. A value bound to padding or opacity, bound
to a plain `div`, or not bound to any element animates normally.

CSS `transition` is not affected, as in the DOM. `useReducedMotionConfig()`
(`createReducedMotionConfig()` in Solid, returning an accessor) reports whether
the nearest `MotionConfig` is reduced right now, for animation you drive
yourself.

### Layout animations

`layout` on a `motion.div` makes the box glide to its new position when layout
moves it, instead of jumping: a sibling pushed down by a growing accordion, a
list that reorders. `layoutId` shares a glide between elements.

```tsx
import { LayoutGroup, motion } from '@gpuix/react'

function Tabs({ tabs, active, onSelect }) {
  return (
    <LayoutGroup id="tabs">
      <div style={{ display: 'flex' }}>
        {tabs.map((tab) => (
          <div key={tab} onClick={() => onSelect(tab)} style={{ width: 100, height: 28 }}>
            {tab === active && (
              <motion.div
                layoutId="highlight"
                transition={{ type: 'spring', duration: 0.4 }}
                style={{ width: 100, height: 28, bg: '#3b82f6' }}
              />
            )}
          </div>
        ))}
      </div>
    </LayoutGroup>
  )
}

<div style={{ display: 'flex', flexDirection: 'column' }}>
  {order.map((item) => (
    <motion.div key={item} layout />   // glides when `order` changes
  ))}
</div>
```

`@gpuix/solid` takes the same props.

- **`layout`** is `true`, `"position"`, `"size"` or `false`. Where the box
  would land, it starts from where it was last drawn, with the speed it had,
  and eases to the layout position. Hit testing follows what is drawn. A box
  inside another `layout` box is not moved twice, and scrolling is not read as
  a move.
- **`layoutId`** hands the glide to the next element with the same id: an
  element that appears with an id some other element held starts from where
  that one was last shown. It implies `layout`; `layout={false}` turns both
  off. An element that unmounts is remembered for one frame, so the newcomer
  has to mount in the same commit or the next one.
- **`transition`** is `transition.layout` if set, else the `transition` of the
  component or of the nearest [`MotionConfig`](#motionconfig-and-reduced-motion),
  else a 0.45 s cubic-bezier tween. `transition.layout` does not affect
  `animate`, and the rest of `transition` does not need to apply to it:
  `transition={{ duration: 0.2, layout: { type: 'spring' } }}`. A layout
  transition cannot repeat.
- **`onLayoutAnimationStart`** and **`onLayoutAnimationComplete`** fire when a
  glide begins and when it reaches the layout position. A glide replaced by a
  newer one does not complete. Both are reported one frame after the glide
  starts or ends.
- **`LayoutGroup`** with an `id` prefixes the `layoutId` of every `motion.*`
  below it as `${id}-${layoutId}`, so two groups can reuse an id without
  sharing a glide. Nested groups join their ids with `-`: group `a` inside group
  `b` scopes `h` as `b-a-h`. A `LayoutGroup` without an `id` changes nothing.
- **`MotionConfig`** with `reducedMotion` that is reduced makes the box jump.

Differences from Motion for React:

- Only position glides. A size change snaps, because GPUIX has no transform to
  scale a box with and re-laying out every frame would flash. `layout="size"`
  therefore does nothing on its own; with a `layoutId` it still moves the box.
  `"preserve-aspect"` is not supported.
- The element that held a `layoutId` is not hidden when another takes the id.
  If both stay mounted the newcomer glides from the old one, and the old one
  stays where it is. Unmount it or set its `opacity` yourself, as in the tabs
  example above.
- Layout animations start automatically; `LayoutGroup` does not need to
  re-render siblings to notice a change.

### Reorder

`Reorder.Group` and `Reorder.Item` make a list the user reorders by dragging,
like Motion's `Reorder`. The group holds the order; the items follow the pointer
and glide into place with [`layout`](#layout-animations).

```tsx
import { useState } from 'react'
import { Reorder } from '@gpuix/react'

function Tasks() {
  const [tasks, setTasks] = useState(['Write', 'Review', 'Ship'])
  return (
    <Reorder.Group axis="y" values={tasks} onReorder={setTasks}>
      {tasks.map((task) => (
        <Reorder.Item key={task} value={task} style={{ height: 32, bg: '#27272a' }}>
          <text>{task}</text>
        </Reorder.Item>
      ))}
    </Reorder.Group>
  )
}
```

`@gpuix/solid` takes the same props; render the items with `<For>` inside the
group.

- **`Reorder.Group`** takes `values` (the current order), `onReorder(values)`
  and `axis` (`"y"` by default, or `"x"`). `as` sets the host element type
  (`"div"` by default). It lays its children out like any `div`, so give it
  `display: 'flex'` and a `flexDirection` that matches `axis`.
- **`Reorder.Item`** takes a `value` from the group's `values`, `as`, and the
  props of a `motion.div` (`transition`, `animate`, `style`, mouse handlers).
  Its `layout` is always on. Its own `onMouseDown`, `onMouseMove` and
  `onMouseUp` still run.
- While an item is held it follows the pointer along `axis`. When its centre
  passes a neighbour's centre, `onReorder` gets the new order. The group does
  not reorder by itself: pass the order back as `values`, and the other items
  glide to their slots. Skipping several neighbours at once proposes the final
  order once.
- On release the item glides from where it was to its slot with the item's
  layout transition (`transition.layout`, then `transition`, then the 0.45 s
  default). Under `reducedMotion` it jumps. The item is drawn above the others
  while it is held and until it arrives, whatever `zIndex` they have: it is
  drawn last, after the rest of the window.
- A drag follows the pointer after it leaves the item: the item calls
  [`setPointerCapture`](#pointer-capture) on `pointerdown`, and the drag ends on
  `pointerup`, `pointercancel` or `lostpointercapture`. On Windows and Linux,
  where `pointerdown` reaches JavaScript after native has moved on, a move that
  arrives before the handler runs is not captured. A release outside the window is not seen; the next
  move with no button down ends the drag.

Differences from Motion for React:

- Item slots are read from the layout when the press happens, so a drag that
  starts while the items are still gliding from an earlier reorder is judged
  from where they are drawn, not where they will settle. After the order
  changes during a drag, the slots are read again at a move once the boxes
  hold still, so a reorder or an item added or removed mid-drag does not end
  the drag. Removing the held item ends it without calling `onReorder`.
- `dragListener`, `dragControls` and `Reorder.Item`'s `drag` props are not
  supported.

### Capture exact frames

The [automation API](#automation) can freeze the native motion clock and render
specific timestamps. This avoids timer sleeps and gives CI the same frames on
every run.

```tsx
import { connectTest } from '@gpuix/react/automation'
import { createTestRoot } from '@gpuix/react/testing'
import { ChatApp } from './chat'

const { render, renderer } = createTestRoot()
render(<ChatApp />)
const app = await connectTest(renderer)

const startedAt = await app.clock.pause()
await app.getByTestId('sidebar-collapse').click()

await app.captureFrames('review/sidebar', [
  startedAt,
  startedAt + 50,
  startedAt + 100,
  startedAt + 150,
  startedAt + 200,
])

await app.clock.resume()
```

## Scrolling

A container with `overflow: "scroll"` or `"auto"` is natively scrollable. GPUI handles scroll physics, clipping, and offset persistence. The two values differ only when scrollbars take layout space (see [Scrollbars](#scrollbars)): `"scroll"` always reserves the bar and `"auto"` only once there is something to scroll. `"hidden"` clips and is a scroll container that only code scrolls: `scrollTop`, `scrollTo` and `scrollIntoView` move it, the wheel and the keys do not, as in a browser. `"clip"` only clips and is no scroll container, and `"visible"` (the default) does neither. `overflowX` and `overflowY` set one axis. As in CSS, `"visible"` beside `"hidden"`, `"scroll"` or `"auto"` computes to `"auto"`, so both axes clip; beside `"clip"` it stays visible, and only the clipping axis is cut (the clip is then not rounded by `borderRadius`).

Overflow follows CSS. It never changes `display` or wraps the children: a scroller lays out exactly as it would without `overflow`. A child wider than the scroller overflows it and can be scrolled to. A scroller can scroll as far as its descendants' boxes, and its own padding and border, reach, measured from where the padding box starts. This is CSS scrollable overflow: a descendant counts down to one that clips itself (`overflow` other than `visible`), whose box counts but whose own overflow does not. An absolute descendant counts only when the scroller is its containing block, so give the scroller a `position` for that. `position: fixed` boxes and anchored or deferred layers (popovers, tooltips) never count. Content that fits never scrolls, and neither does an empty scroller.

Plain scroll containers still build every child. Use `<virtual-list>` below when the collection can grow large.

### The wheel

The wheel goes to the innermost scroller under the pointer that can still scroll in the direction of the delta. One wheel event scrolls one scroller: the first one, innermost out, that can use any of it takes the whole event on both axes, and what it cannot use is dropped, not passed to the scroller around it in the same event. A scroller at its limit takes nothing, so the event goes to the next one up. That is how a list inside a page lets you carry on scrolling the page: the next gesture (a new trackpad gesture, a mouse wheel after half a second or a pointer move) finds the list at its end and scrolls the page. Nested scrollers work. `<virtual-list>` takes part like any scroller, along the vertical axis only: an event it can scroll vertically is its own, horizontal delta included, and a scroller inside one of its rows is served first.

`overscrollBehavior` (and `overscrollBehaviorX` / `Y`) decides what happens to the delta a scroller cannot use. `"auto"`, the default, lets the next gesture reach the scroller around it. `"contain"` and `"none"` keep it, so the page behind does not move.

A trackpad gesture stays on the scroller that took its first delta, momentum included, so a fling does not jump to the parent when the inner list runs out. A mouse wheel does the same until it pauses for half a second or the pointer moves. `onWheel` listeners on every element under the pointer still run, whichever scroller moved.

`wheel` is one DOM event per wheel input, aimed at the element under the pointer and run along its path: `onWheelCapture` from the outermost ancestor down, then `onWheel` from the target up, and `stopPropagation()` works. Pointer capture does not redirect it, as in a browser; a pointer lock does. It runs before any scroller takes the delta, so `scrollTop` read in a handler is the position before this input. Calling `preventDefault()` in either phase cancels the scroll that input would cause, in every scroller and text input. Two deviations from the DOM:

- It cancels that input only. A browser that sees the first wheel of a gesture cancelled cancels the whole gesture; here the next input scrolls unless its own handler cancels it, so a handler that wants a gesture cancelled calls `preventDefault()` on each input.
- It needs the handler to run before the scroll, which only holds on macOS and in the test renderer. On Windows and Linux the event reaches JavaScript through a queue after native scrolled, so `preventDefault()` has no effect there.

`scroll` does not follow the wheel's path: it stays on the scroller that moved and does not bubble, like the DOM's element `scroll`.

```tsx
<div style={{ height: 300, overflow: 'auto' }}>
  <div style={{ height: 120, overflow: 'auto', overscrollBehavior: 'contain' }}>
    {/* scrolls itself, and never scrolls the outer list from its ends */}
  </div>
  {items}
</div>
```

```tsx
function ScrollableList() {
  return (
    <div style={{ height: 300, overflow: 'auto' }}>
      {items.map((item, i) => (
        <div key={i} style={{ height: 60, padding: 12 }}>
          {item.name}
        </div>
      ))}
    </div>
  )
}
```

`overflow: "scroll"` scrolls both axes at once from a single diagonal gesture, like a browser. A flex column stretches its children to the cross axis, so a two-axis container needs its rows to state a width. Without one there is nothing to pan on **X**:

```tsx
<div style={{ width: 260, height: 220, overflow: 'scroll', display: 'flex', flexDirection: 'column' }}>
  {rows.map((row) => (
    <div key={row.id} style={{ display: 'flex', width: 810, flexShrink: 0 }}>
      {row.cells}
    </div>
  ))}
</div>
```

Flex items shrink as in CSS, so a row of items in a flex scroller needs `flexShrink: 0` or a definite width to overflow.

### Events

`onWheel` is the DOM `wheel`: every tick over the element or a descendant, before any scroller applies it, with `deltaX`, `deltaY`, `deltaMode`, `clientX`, `clientY`, `offsetX`, `offsetY`, `buttons` and `modifiers`. `deltaY` is positive for down, and `deltaMode` is always `0`: a mouse wheel's lines are converted to pixels at 20 px a line. `touchPhase` and `precise` say how the tick arrived.

`onScroll` is the DOM `scroll`: it fires whenever the scroll offset changes, whatever moved it: the wheel, a scrollbar drag, the keyboard, `scrollTo`, `scrollIntoView`, a smooth animation, or content that shrank under the offset. The payload is `{ scrollTop, scrollLeft }` of the target, and it does not bubble. The position at the first paint is not reported, only movement after it. It does not fire for `<virtual-list>`; use `onVisibleRange`.

### Keyboard

Scrolling keys are a default action, like Tab: they run after every `onKeyDown` handler and do nothing if one called `event.preventDefault()`. The arrow keys scroll 40 px, PageUp, PageDown and Space (Shift+Space goes back) 90% of the client size, and Home and End go to the ends. They scroll the innermost scrollable ancestor of the focused element that can move that way, then the one around it. With nothing focused they scroll the scroller under the pointer. Ctrl, Alt and Cmd turn them off.

A focused `<input>` or `<textarea>` keeps its keys, and Space on a focused element with `onClick` activates it instead of scrolling. A handler that wants an arrow key for itself, like a listbox moving a highlight, must call `preventDefault()`, as in the DOM.

### Reading and moving a scroller

A host ref has the DOM's scroll members. Reads come from the last layout.

```tsx
function ProgrammaticScroll() {
  const list = useRef<PublicInstance>(null)
  const lastItem = useRef<PublicInstance>(null)

  return (
    <>
      <div ref={list} style={{ height: 200, overflow: 'auto' }}>
        {items.map((item, i) => (
          <div key={i} ref={i === items.length - 1 ? lastItem : undefined}>
            {item}
          </div>
        ))}
      </div>
      <div onClick={() => lastItem.current?.scrollIntoView({ block: 'end', behavior: 'smooth' })}>
        Jump to last
      </div>
      <div onClick={() => list.current?.scrollTo({ top: 0 })}>Back to top</div>
    </>
  )
}
```

| Member | |
|---|---|
| `scrollTop`, `scrollLeft` | Read, or set for an instant `scrollTo`. Clamped to the scrollable range, which ends where Chrome's does: the extent (with the padding after the content) and the client size, each rounded half up to a device pixel, apart from where the box sits. At scale 1 the farthest offset is `scrollHeight - clientHeight`; at scale 2 it can be half a pixel more, and `scrollTo(0, scrollHeight - clientHeight)` stops short of it, as in Chrome. A write lays out only what is pending, then clamps against that layout, so any number of writes in a task cost no layout, and a write after a style change in the same task is clamped to the new extent. |
| `scrollWidth`, `scrollHeight` | The content's extent, never less than the client size. A whole number: the extent as layout measured it, rounded half up, as Chrome does. |
| `clientWidth`, `clientHeight` | The padding box less the scrollbar. Overlay scrollbars take nothing from it; classic ones (see [Scrollbars](#scrollbars)) take 15 px, or 11 with `scrollbarWidth: "thin"`. A whole number: the size as layout measured it, rounded half up, as Chrome does, so a box 33.328125 px wide has a `clientWidth` of 33. A table with `border-collapse: collapse` is the exception, as in Chrome: its borders are the outer halves of its cells' borders, which `clientLeft` and `clientTop` report (rounded half up) but which `clientWidth` and `clientHeight` do not take off, so they are the whole border box. Its cells' client size leaves their own halves. |
| `scrollTo({ top, left, behavior })`, `scrollTo(x, y)` | An axis you leave out stays where it is. A value that is not a finite number is 0, as in Chrome: `NaN`, `Infinity` and `-Infinity` (and `undefined` or a non-numeric string to a `scrollTop` or `scrollLeft` write) scroll that axis to the start. |
| `scrollBy({ top, left, behavior })`, `scrollBy(dx, dy)` | Relative to the current position. A non-finite amount moves 0. |
| `scrollIntoView({ block, inline, behavior })` | `block` defaults to `"start"` and `inline` to `"nearest"`; each takes `"start"`, `"center"`, `"end"` or `"nearest"`. `scrollIntoView(false)` aligns the end. |

`scrollIntoView` works for a descendant at any depth and scrolls every scrollable ancestor, outermost last, as the DOM does. It aligns by the element's laid-out box, not by its index among its siblings. Inside a `<virtual-list>` it scrolls to the row that holds the element and aligns that row.

`behavior` is `"instant"`, `"smooth"` or `"auto"`. With `"auto"`, or nothing, a scroller follows its `scrollBehavior` style. `scrollBehavior: "smooth"` makes `scrollTo` and `scrollIntoView` animate over about 300 to 450 ms (longer for longer distances), firing `onScroll` each frame. A wheel tick, another `scrollTo` or a drag ends the animation. An explicit `behavior` always wins. An element under `display: none` has no box to scroll, so `scrollTo` does nothing on it, whatever the behavior, and its `scrollTop` reads 0, as in the DOM.

The same calls exist on the renderer for code that holds an id. They speak the DOM's scroll positions, the same numbers as `element.scrollLeft` and `scrollTop` and the `onScroll` event (zero or more, a larger `top` is further down), on the test renderer, the live renderer and the automation protocol alike:

```tsx
renderer.scrollTo(elementId, left, top, behavior?)         // scroll to a position
renderer.scrollToItem(elementId, index)                    // a virtual list's row
renderer.scrollIntoView(elementId, block?, inline?, behavior?)
renderer.getScrollOffset(elementId)                        // [scrollLeft, scrollTop], or null
renderer.getScrollMetrics(elementId)                       // [scrollLeft, scrollTop, scrollWidth, scrollHeight, clientWidth, clientHeight]
```

### Sticky positioning

`position: "sticky"` with `top`, `right`, `bottom` and `left` works as in CSS. The element lays out like a relative one and its insets do not move its siblings. Then it is moved to stay inside the scrollport of its nearest scroll container less that container's padding on every side (the window, if none scrolls around it), that far from the edges its insets name, and never out of its parent's content box. A header sticks at the top of the scroller while its section is in view, and the next section's header pushes it out as the section ends. A sticky element also sticks sideways with `left` or `right`.

```tsx
<div style={{ height: 300, overflow: 'auto' }}>
  {sections.map((section) => (
    <div key={section.id}>
      <div style={{ position: 'sticky', top: 0, height: 28, backgroundColor: '#222' }}>
        {section.title}
      </div>
      {section.rows}
    </div>
  ))}
</div>
```

Its insets are pixels, or percentages of that scrollport less its padding. `hidden` and `scroll` containers hold sticky elements the same way, and the geometry (`getElementBounds`, hit testing, `onLayout`) is the stuck position.

### Panes that must move together

A native scroll container cannot drive a **frozen header**. GPUI moves the
container on the wheel frame, and the JavaScript callback that would move the
header arrives a frame later, so the header tears away during a fast pan.

When two panes must stay locked to the pixel, own the offset in React: put one
`onWheel` listener on a non-scrolling parent, keep `scrollX` and `scrollY` in
state, and translate each pane's content with an absolutely positioned wrapper.
Zed does the same; the editor owns its scroll position and paints the gutter and
the text from it.

```tsx
function Pane({ offsetX, children }: { offsetX: number; children: React.ReactNode }) {
  return (
    <div style={{ flexGrow: 1, minWidth: 0, overflow: 'hidden', position: 'relative' }}>
      {/* An empty positioned box still takes hits, so opt it out. */}
      <div style={{ position: 'absolute', left: -offsetX, top: 0, pointerEvents: 'none' }}>
        {children}
      </div>
    </div>
  )
}
```

Keep the moving subtree in a `memo` component whose props do not change during a
pan. The wheel then costs a handful of style mutations, not one per row. The
[timeline example](./examples/timeline.tsx) does this for a ruler, a track
column, and a clip grid.

## Virtual lists

Use `<virtual-list>` for **long, variable-height collections** such as message lists. React and Rust retain every row, but GPUI only builds, lays out, and paints rows near the viewport.

```tsx
function MessageList({ messages }: { messages: Message[] }) {
  return (
    <virtual-list
      alignment="bottom"
      followTail
      estimatedItemHeight={180}
      style={{ flexGrow: 1, minHeight: 0 }}
    >
      {messages.map((message) => (
        <Message key={message.id} message={message} />
      ))}
    </virtual-list>
  )
}
```

The list needs a **bounded height** or bounded flex space. Its direct children are rows and can contain any GPUIX host or custom element.

The list paints its own box style like a `<div>`: `backgroundColor`, borders, `borderRadius`, `boxShadow`, `opacity`, `filter`, `mask` and `clipPath` apply to it and its rows, and a `borderRadius` or `clipPath` cuts what the pointer can hit.

| Prop | Default | Purpose |
|---|---:|---|
| `alignment` | `"top"` | Use `"bottom"` for chat-style initial positioning |
| `followTail` | `false` | Follow appended rows until the user scrolls away |
| `overdraw` | `512` | Extra pixels built outside the viewport |
| `estimatedItemHeight` | none | Height hint for unmeasured rows. **Required** with `itemCount` |

### How virtualization works

**React reconciliation stays normal.** The complete keyed child list crosses the mutation protocol and remains in Rust's retained tree. GPUIX defers only the expensive GPUI element construction, layout, and paint work.

```text
React Fiber + Rust RetainedTree    all row IDs, props, text, and events
                 │
                 ▼
          GPUI ListState          row count and measured height cache
                 │
                 ▼ visible indexes plus overdraw
          cx.processor            re-enters GpuixView after root render
                 │
                 ▼
          fresh BuildCtx          builds only the requested React subtree
                 │
                 ▼
       GPUI layout and paint      visible rows only
```

### Row heights

**Rows do not need equal heights, and you do not need to know them.** GPUI measures a row when it enters the viewport. `estimatedItemHeight` is a **hint for rows nothing has measured yet**, not a size contract.

```text
index:     0        1        2        3        4        5        6        7
       ┌────────┬────────┬────────┬────────┬────────┬────────┬────────┬────────┐
       │  hint  │  hint  │measured│measured│measured│  hint  │  hint  │  hint  │
       │  220px │  220px │  184px │  512px │   96px │  220px │  220px │  220px │
       └────────┴────────┴────────┴────────┴────────┴────────┴────────┴────────┘
           ▲                          ▲                          ▲
           │                          │                          │
     estimate only         real, variable heights          estimate only
                          (viewport plus overdraw)
```

The sum of that height cache is the scroll length, so a rough estimate only affects **scrollbar accuracy** before a row is visited. The measured height replaces the estimate automatically, and the scrollbar converges as you scroll.

When a retained descendant changes, GPUIX marks its direct row for remeasurement, so a streaming row grows correctly. Appending, removing, or reordering keyed rows keeps measurements for rows whose IDs did not change.

`estimatedItemHeight` is optional in children mode, where every row exists and can be measured. It is **required** with `itemCount`, because React never mounts the rows outside the window and native has no element to measure. Those indexes render as an empty box of the estimated height until React mounts the real row.

### Row boundaries

Each **direct host child** is one virtual row. Give every row a stable React key and one host root:

```tsx
<virtual-list style={{ height: 500 }}>
  {messages.map((message) => (
    <div key={message.id} style={{ paddingBottom: 24 }}>
      <Message message={message} />
    </div>
  ))}
</virtual-list>
```

A row can contain nested `<div>`, `<text>`, `<markdown>`, `<code>`, `<diff>`, `<input>`, and `<textarea>` elements. Focusable rows stay active when they move offscreen, so keyboard input and native editor state are preserved. A scroller inside a row takes the wheel first. When it is at its limit the list takes the event, and when the list is at its limit the scrollers around it do (see [Scrolling](#scrolling)).

A row can be `display: "contents"`. It has no box, so its `height`, `padding`, `margin`, `border` and `backgroundColor` do nothing, and the list measures it as its children stacked as a block: children 30 and 25 px tall make a 55 px row, and a row with no children (or only `display: "none"` ones) is 0 px. A browser has no such list, so this is the list's own definition. Its text style, `inert`, `pointerEvents` and `visibility` pass down to the children, and its own listeners hear them (see [Layout like CSS](#layout-like-css)). It can be keyed, reordered and switched to or from a block like any row, and Tab walks its children, never the row.

### Chat tail behavior

Combine `alignment="bottom"` and `followTail` for a chat thread:

```tsx
<virtual-list
  alignment="bottom"
  followTail
  estimatedItemHeight={220}
  style={{ flexGrow: 1, minHeight: 0 }}
>
  {turns.map((turn) => (
    <ChatTurn key={turn.id} turn={turn} />
  ))}
</virtual-list>
```

The list follows new rows while the user is at the bottom. Scrolling upward pauses tail following. Returning to the bottom enables it again. A streaming final row is remeasured as its content grows.

### Scroll anchoring

The list is anchored on a **row index**, not on a pixel offset. In children mode React reconciles by key, so that index still lands on the same row after a prepend: the rows already on screen stay exactly where they are. A browser does the same, and calls it scroll anchoring.

One exception, also copied from the browser: a top-aligned list that is scrolled to the **very top** stays at the top, so a prepended row is visible.

```text
scrolled down                          pinned to the top
┌──────────────────┐                   ┌──────────────────┐
│ new row  (above) │  ◄── inserted     │ new row          │  ◄── inserted, visible
├──────────────────┤                   ├──────────────────┤
│ ░░ viewport ░░░░ │  stays put        │ ░░ viewport ░░░░ │  follows the insert
│ ░░░░░░░░░░░░░░░░ │                   │ ░░░░░░░░░░░░░░░░ │
└──────────────────┘                   └──────────────────┘
```

That is what a todo list or a feed wants: `setItems((current) => [fresh, ...current])` puts the new row on screen. A history pane that loads older pages while the user reads should use `alignment="bottom"` instead, so a page load never moves the text.

**With `itemCount`, the app owns the correction.** There is no key to reconcile against, so the index is all there is. Prepending shifts every row down one slot, and the anchor keeps pointing at the old number, so the content slides by exactly the number of rows you inserted. Move `windowStart` by the same amount:

```tsx
const prepend = (fresh: Row) => {
  setRows((current) => [fresh, ...current])
  // The anchor is an index. One new row above the window means every existing
  // row moved down one, so the window has to move with it.
  setWindowStart((start) => (start === 0 ? 0 : start + 1))
}
```

Leave `windowStart` at `0` alone; the list is pinned to the top there and the new row should be visible.

### Programmatic scrolling

Use a ref to call the same renderer scroll methods as a plain scroll container:

```tsx
function Results({ rows }: { rows: Result[] }) {
  const renderer = useGpuixRequired()
  const listRef = useRef<{ id: number } | null>(null)

  const reveal = (index: number) => {
    if (listRef.current) {
      renderer.scrollToItem?.(listRef.current.id, index)
    }
  }

  return (
    <>
      <virtual-list ref={listRef} style={{ height: 400 }}>
        {rows.map((row) => (
          <ResultRow key={row.id} row={row} />
        ))}
      </virtual-list>
      <div onClick={() => reveal(rows.length - 1)}>Reveal latest</div>
    </>
  )
}
```

`scrollTo`, `scrollToItem`, and `getScrollOffset` all support virtual lists. A list's
`scrollTop` is its pixel offset into the content, from estimated row heights while
rows are unmeasured; `getListScrollTop` below is exact. A write to it is scroll-only, as on a
`div`: the list clamps it from its last layout and the next frame builds the rows at the new offset. Its largest
offset is the rule of a `div`'s, and `scrollHeight` and `clientHeight` are the rows with their padding and the
list as laid out. Rows are measured on the device pixel, so a row with a fractional height differs from
Chrome's, and a `<virtual-list>` records no bounds of its own: its `getBoundingClientRect()` is zeros.

On a virtual list, `scrollToItem` takes an optional **pixel offset** and the
list reports its logical anchor:

```tsx
renderer.scrollToItem(listId, index, offsetInItem)  // offset in px, may be negative
renderer.getListScrollTop(listId)  // [itemIndex, offsetInItemPx, viewportHeightPx] or null
```

A **negative offset anchors the viewport top above the row**, and the next
layout resolves it against real measured heights. That is the tool for
infinite-scroll history: while the reader waits in a loading row, read
`getListScrollTop`, commit the fetched page, then re-anchor on the message
that was under the loading row with a negative offset. The message stays at
the same pixel while the new rows are measured above it —
`examples/infinite-chat.tsx` is the worked example.

An `itemIndex` equal to the item count is gpui's **at-end sentinel**: a
bottom-aligned list resting at its very end. A reader waiting at a trailing
loading row usually sits there, and the viewport height in the same tuple is
what converts that into a position relative to the trailing rows
(`EDGE_HEIGHT - viewportHeight` in the example).

Virtual-list `scrollToItem` calls are applied on the **next render, after
that frame's child splice**, so an index computed against a just-committed
child list is never shifted twice.

### Performance model

| Work | Plain scroll container | `<virtual-list>` children | `<virtual-list>` + `itemCount` |
|---|---|---|---|
| React Fiber nodes | All rows | All rows | Visible window |
| Rust retained nodes | All rows | All rows | Visible window |
| GPUI row construction | All rows | Visible rows plus overdraw | Visible rows plus overdraw |
| Layout and paint | All rows | Visible rows plus overdraw | Visible rows plus overdraw |
| Height metadata | None | One lightweight entry per row | One lightweight entry per logical row |

The children form still creates every React child, so a 10,000-row `turns.map` is slow to mount. Pass `itemCount` and `windowStart` and render only that slice to mount a window too. Collections with millions of rows still need application-level paging or a data-owning native element.

React releases temporary child-placement lists after a subtree mounts. A
5,000-turn chat retained 45.6 MiB of JS heap instead of 47.7 MiB; windowed
lists avoid those host nodes altogether.

Native host elements keep their event and custom-prop maps only when needed.
On a 75,000-element chat, the retained Rust tree used 33.6 MB instead of
44.5 MB. This changes memory use, not the event or prop API.

### Keep scroll fast

A wheel event notifies the window view. GPUI then rebuilds the **visible**
rows and Taffy lays them out again. Draw time is the cost of those rows, not
the length of the list.

Put a long list on `<virtual-list>`. Keep `overdraw` near one extra
viewport. Put fat content in one native node (`<markdown>`, `<code>`, `<diff>`),
not a tree of React spans.

The host `<virtual-list>` still retains every React child. Pass `itemCount`,
`estimatedItemHeight` and `windowStart`, then render only that window, so mount
does not create every row. Native ignores `itemCount` when the estimate is
missing, so a jump cannot collapse unmounted rows to height 0.

There is **no `VirtualList` wrapper component**. The window is app state:
only the app knows when it must widen, for example when a filter grows
`itemCount` without any scroll. Keep `start` in `useState`, move it from
`onVisibleRange`, and slice around it.

```tsx
const WINDOW = 40

const Transcript = memo(function Transcript({ turns }: { turns: Turn[] }) {
  const [start, setStart] = useState(0)
  const end = Math.min(turns.length, start + WINDOW)
  return (
    <virtual-list
      itemCount={turns.length}
      windowStart={start}
      estimatedItemHeight={220}
      style={{ flexGrow: 1, minHeight: 0 }}
      onVisibleRange={(event) =>
        setStart(Math.max(0, Math.floor(event.startIndex ?? 0) - WINDOW / 4))
      }
    >
      {turns.slice(start, end).map((turn) => (
        <ChatTurn key={turn.id} turn={turn} />
      ))}
    </virtual-list>
  )
})

function ChatApp() {
  const [collapsed, setCollapsed] = useState(false)
  const [turns, setTurns] = useState(initialTurns)
  return (
    <div style={{ display: 'flex', flexDirection: 'row', height: '100%' }}>
      <Sidebar collapsed={collapsed} onCollapse={() => setCollapsed(true)} />
      <Transcript turns={turns} />
      <Composer onSend={(text) => setTurns((current) => [...current, { text }])} />
    </div>
  )
}
```

`turns` is a new array only when a message arrives. Sidebar and draft updates
leave that reference alone, so `memo` skips the map. The chat example uses
this pattern.

Turn on `debugFrameOverlay: 'full'` while you scroll. The overlay is **draw
time**. `8.3 MS` is about 120 Hz.

### Pannable surfaces must cull

`<virtual-list>` is the only thing that virtualizes. A surface where **you** own
the offset — a timeline, a node graph, a map — places its children absolutely,
so GPUI builds and lays out **every** retained child on every frame. Nothing
skips that for you. GPUI does skip the prepaint and paint of in-flow content
outside an `overflow` clip, as a browser does, but a clipped box is still built
and laid out, because script can read its box, and costs about two thirds of a
visible one. Content placed with `position: absolute` is never skipped. For a list of
boxes in flow, [`contentVisibility`](#skipping-content-with-contentvisibility) skips the
build and layout of the ones far from the window.

`memo` and culling fix different halves, and only one of them is the draw:

```
memo(Layer)  ►  cuts React work and the applyBatch mutations
cull in JS   ►  cuts GPUI build, Taffy layout, and paint
```

You already know the offset, so the visible window is a `useMemo` away:

```tsx
const visible = useMemo(() => {
  const from = scrollX / pxPerSecond
  const to = (scrollX + viewportWidth) / pxPerSecond
  return clips.filter((clip) => clip.start <= to && clip.start + clip.duration >= from)
}, [clips, scrollX, pxPerSecond, viewportWidth])
```

The timeline example measures both, on 3,259 clips across 26 tracks:

| Wheel pan, one full frame | p50 |
|---|---|
| Culled | **7.7 ms** |
| `memo` only, no culling | **92 ms** |

> [!IMPORTANT]
> A perf sample must include `renderer.flush()`. Without it you time the React
> update and none of the GPUI build, layout, and paint that follows. The
> `memo`-only number above looks like **0.6 ms** if you forget.

### Skipping content with `contentVisibility`

For a long list that you do not virtualize, `contentVisibility` is CSS
`content-visibility`: `"auto"` skips the content of an element while it is not near the
window and does not hold the focus, `"hidden"` always skips it, and `"visible"` (the
default) never does. A skipped element is built and painted itself (background, border,
hit area) and builds, lays out, shapes and paints nothing inside it. Unlike `display:
none` nothing is forgotten: the subtree's state, styles, transitions and animations stay,
and it is built whole when it shows again. Only a `div`, `text`, `label`, `form` and a
block `span` skip; any other element has the containment below and nothing more.

```tsx
<div style={{ contentVisibility: 'auto', containIntrinsicSize: 'auto 40px' }}>
  <Entry />
</div>
```

A skipped element has the size `containIntrinsicSize` gives: `none` (0), a length or
`auto <length>` per axis, written as one value for both axes or width then height (a
number is px), with the longhands `containIntrinsicWidth`, `containIntrinsicHeight`,
`containIntrinsicInlineSize` and `containIntrinsicBlockSize`. With `auto`, the size the
element had when it was last built is used, and the length until then. Give an `auto`
entry a size close to its real one: the page scrolls by what the skipped entries claim,
and the extent changes as they are built.

<details>
<summary>What `auto` and `hidden` do to the element, and what is near</summary>

- An element with `auto` or `hidden` is contained, skipped or not: `overflow: clip` on
  each axis you left `visible`, a stacking context, an independent formatting context (no
  margin of its first or last child collapses through it), and the containing block of its
  `position: absolute` and `position: fixed` descendants. A percentage `height` of a
  child of a skipped block with an `auto` height is `auto`, whatever
  `containIntrinsicSize` makes the box.
- Near is the border box outset by 150% of the window (its width at the sides, its
  height above and below), cut by every ancestor that clips it. The very edge is not near.
  The first answer comes in the frame that builds the element, which draws again to
  build what is near; a later change takes effect in the next frame, with no hysteresis.
- `focus()` on an element inside a skipped one builds what holds it in the same frame.
  Skipped content is not in the accessibility tree and search does not find it.
- `onContentVisibilityAutoStateChange` (and `…Capture`) fires on an `auto` element
  after the frame that skipped it or showed it, once for the first answer and then when
  it changes. `skipped` is in the payload. It bubbles and is cancelable, and
  `preventDefault()` does nothing.
- A geometry read of an element inside a skipped subtree (`getBoundingClientRect`,
  `getElementBounds`, `getScrollMetrics`) lays the subtree out for real and returns true
  boxes; the subtree stays skipped. The read costs a layout pass, and every read lays
  out again.
- An animation below a skipped element is not built or drawn and fires no event; it
  goes on from the clock when the element shows.
- A percentage height of a child of a skipped element resolves against the intrinsic
  size, where Chrome gives `auto`.

</details>

## Inline text

Text lays out like the DOM's inline formatting. Raw strings and `<span>` are
inline, and so is a `<text>` inside a `<text>` or `<span>`. A run of consecutive
inline children is one paragraph: one string that wraps across its runs, selects
and copies as one line, and is searched by `highlight` as one piece of text.

```tsx
<text style={{ color: '#e2e2e2' }}>
  Saved <span style={{ color: '#7ee787', fontWeight: 700 }}>3 files</span> in{' '}
  <span style={{ backgroundColor: '#ffe58a55' }}>2.1s</span>
</text>
```

An inline box can set `color`, `fontWeight`, `fontStyle`, `fontFamily`,
`fontSize`, `lineHeight`, `textDecoration` (see below), `fontVariantNumeric`,
`fontFeatureSettings` and `fontVariationSettings`; its `backgroundColor` (a `<mark>`-style wash), border and
`borderRadius`, painted round each line of its text (see [Painting an inline box](#painting-an-inline-box)); and
an `outline` (see [Focus on an inline box](#focus-on-an-inline-box)). A box inherits from the box
around it, as in CSS. A `display: contents` span inside a span adds no box: its
children join the paragraph and inherit from it.

`textDecoration` is the CSS shorthand (`"underline line-through"`,
`"underline wavy red 2px"`), and `textDecorationLine`, `textDecorationStyle`,
`textDecorationColor` and `textDecorationThickness` are its longhands, and one
written after it wins over it. A line is drawn over the whole box in the colour its own box had, so a
descendant that changes `color` does not recolour it, and a descendant cannot
take it off, `none` included. Lines accumulate, as in CSS: a nested box, inline or
block, that declares a line the box around it already draws gets its own beside
the outer one, each with its own style, colour and thickness, so `underline wavy
red` inside `underline` draws both. Each is drawn where its kind sits, so two
lines of one kind with the same thickness and style overlap exactly.

Where a line is, follows Chrome. An underline is drawn at the baseline of the box that
declared it, level under text that a nested box raises or lowers. A line-through is
centred two thirds of the ascent of the declaring box's font below the top of the text
it crosses, and an overline sits on the top of that text with its bottom edge there and
the thickness growing upward, so each goes up and down with the text it is drawn over
(a raised or bigger nested box has its own piece at its own height), and takes its
thickness from the declaring box. Underlines and overlines are painted under the glyphs
and a line-through over them. Where a nested box declares a line again, the outer box's
is under the inner one's.

A declaration gives every line it names one style, one colour and one
thickness, as in CSS. The styles are `solid`, `double` (two lines a thickness
apart), `dotted` (round dots), `dashed` and `wavy`, on `underline`, `line-through`
and `overline` alike. `textDecorationThickness` (also the last component of the
shorthand) is `auto`, which is a tenth of the font size as in Chrome, `from-font`,
which is the font's own underline thickness and `auto` for a font without one, or a length: `px`, the
absolute units, `em`, `rem`, the viewport units, `calc()`, or a percentage, which is
a percentage of `1em` of the element's font size (`"10%"` is `"0.1em"`). It is
resolved at the element that declares it, with that element's own font size, and
an inline box declaring it with its own `fontSize` uses that size. The lines a
descendant inherits keep the thickness resolved there; a descendant with a bigger
font does not thicken them. A negative length is valid and paints one device pixel, as
Chrome does for it and for zero; a bare non-zero number is invalid and ignored, like in
every other length in a style string. Dots and dashes
are laid out as Chrome does, in device pixels: a dash is three thicknesses long
with a gap of two (two long with a gap of one from a thickness of 3px), the gaps
stretch or shrink so that the last dash ends where the line does, and a line
shorter than two dashes is solid. A dotted line under four device pixels thick is
square marks one thickness long and two apart, which are not fitted; a thicker one
is round dots with the first and the last centred half a thickness in from the ends,
and the gaps fitted between them. A pattern starts over with every **text
fragment**, which is a text node in one inline box on one row: an outer dashed
underline starts again after a nested box and between two text nodes, and on every
wrapped row, and the spaces that hang past the end of a row are not drawn under.
(A paragraph in which no `<span>` restyles anything is one fragment: see FORK.md.) `auto` and `from-font` stay fractional; a
solid or double line floors them to a device pixel (at least one) when it paints,
and a dotted or dashed one rounds them. A length is rounded to the nearest device
pixel, a half up, and is at least one, `0px` included, as in Chrome: `1.5px` is two
rows on a display of scale 1 and three on one of scale 2, and `0.3px` is one.

`textUnderlineOffset` is inherited and sets how far below the alphabetic baseline
the top edge of an underline sits. `auto` (the default) leaves a gap of half the
thickness, rounded up, and at least a pixel, as Chrome does. A length (a number is
px; `"0.25em"`, `calc()`) replaces that gap, so `0` puts the line on the baseline and
a negative one above it. A percentage is of `1em`. The box that declares the
underline decides: its own value or the one it inherits, and a descendant's offset
does not move the line of an ancestor. `em` resolves where the value is set and
descendants inherit the pixels; a percentage is of the font size of the box that
draws the line. It applies to underlines only, it animates (see
[Targets and timing](#targets-and-timing)), and a `focusVisible` style that declares an
underline takes the inherited offset.

An underline is drawn at the baseline of the box that declared it, and the offset is
measured from there: text inside it that is raised or lowered with `verticalAlign`
(`sub`, `super`, a length, `text-top`, ...) or set in another size stays on the one
line, as in Chrome, and a box that is raised itself carries its underline up with
it. The underline of the paragraph's own container is on the baseline of the row.
Chrome interrupts an underline where it crosses the ink of a glyph
(`text-decoration-skip-ink: auto`); GPUIX draws it whole.

- A `<text>` anywhere else is a block, and a block child ends a paragraph.
- A nested `<text>` with a `tabIndex` is a block too: it is an element of its own
  with the focus handle, and it splits the paragraph around it. A `<span>` stays
  inline whatever its `tabIndex`, see [Focus on an inline box](#focus-on-an-inline-box).
- In a `display: flex` or `grid` container each run of raw strings is one item
  and every `<span>` or nested `<text>` is an item of its own, with its content as
  a paragraph.
- A paragraph that is only collapsible white space (space, tab, newline, CR,
  form feed; not a no-break space) makes no box, and neither does a child with
  `display: none`.
- White space is processed as in CSS, see [White space](#white-space).

A box's `fontSize` is relative to the box around it (`em`, `%`; `rem` is the
root's) and its text is shaped at that size; a box without one inherits. Its
`lineHeight` follows CSS: a number is a multiple of the box's own font size, so a
bigger span in a `lineHeight: 1.5` paragraph has a taller line, and a length is
resolved in the box that declares it and inherited as px. A row is as tall as the
CSS line box: the container's line height around its font, and each run with
its own size or line height adds its line height around the same baseline. A
wrapped row breaks where the sized text actually ends.

A `<span>` with a style that needs a box of its own (`position: absolute`,
`display: block`, `flex` or `grid`) is laid out as a block instead. `display:
inline-block`, `inline-flex` and `inline-grid` keep it on its line, as a box of its own,
see [Inline-block, inline-flex, inline-grid and inline-table](#inline-block-inline-flex-inline-grid-and-inline-table).

### Line height

A line is as tall as Chrome on macOS makes it. The ascent, descent and line gap of a
font are each rounded to a whole device pixel at the size it is used on the window's
display, so a font's lines do not scale with its size, and Times, Helvetica and Courier get a
taller ascent, as in Chrome. `lineHeight: 'normal'`, which is also what an element that sets
none has, is the three together (Helvetica at 16px is 18px, Menlo 19px at scale 1; Menlo 20px is
24px at scale 1 and 23px at scale 2, where the metrics are 37 and 9 device pixels). A number or a
length is laid out in sixty-fourths of a device pixel, so `lineHeight: 1.2` at 12px is 14.390625px and not
14 or 15 on a 1x display and 14.3984375px on a 2x one. A number is cut to that unit and a length or a
percentage is rounded to it, as in Chrome (`lineHeight: 1.15` at 13px is 14.9375px at 1x, `14.95px` and `115%`
are 14.953125px). A text is as wide as its advance ceiled to the same unit, which is 1/64 px at 1x and
1/128 px at 2x. What a line has over the font's ascent and descent is shared around them, the half above
rounded down to a whole device pixel: a 24px line around a 19px content area of Menlo 16px has a
baseline at 17px on a 1x display and at 17.5px on a 2x one, as in Chrome. A window that moves to a
display of another scale shapes its text again. Boxes are snapped to the pixel grid when they are painted, so a stack of such lines is as
tall as the sum of its lines; `getBoundingClientRect()` reports the layout and not the snapped box, so one box
reports its height in sixty-fourths of a pixel.

The strut of a line is the element's own font and line height. A run in another weight,
family or size is a box of its own on the line, and the line is as tall as the strut and
every box together need, so a bold span in a regular paragraph can make its line a pixel
taller. `normal` on a span is the height of the span's font.

### Space around an inline box

`marginLeft`, `marginRight`, `paddingLeft`, `paddingRight`, `borderLeftWidth` and
`borderRightWidth` on a `<span>` or a nested `<text>` take space along the line, as
on a CSS inline box: the text after the box moves right by their sum, the line
and the container that wraps around it grow by it, and wrapping counts it, so a
word followed by the closing padding of its span moves to the next line when the
padding does not fit. The shorthands `margin`, `padding`, `borderWidth`, `border`,
`borderLeft` and `borderRight` set the same sides, and of a shorthand and a longhand
it covers the one written later wins. `hover`, `active` and the other state styles change the space too.

```tsx
<text>
  Press <span style={{ paddingLeft: 6, paddingRight: 6, marginRight: 4 }}>Enter</span> to go on
</text>
```

- Only the inline axis takes space. A vertical margin does nothing, and a vertical
  padding or border moves no line: it reaches above and below the text, over the
  lines around it, as in CSS (see [Painting an inline box](#painting-an-inline-box)).
- A span that wraps has its leading space (margin, border and padding) on its first
  line and its trailing space on its last, as `box-decoration-break: slice` does.
  The first line of the box can be the start of a wrapped line; the leading space
  goes with it. The lines in between have none.
- Boxes nested in each other add up: an inner box that starts where its parent
  does has the parent's leading space first, then its own.
- A box is its border box: `getBoundingClientRect()`, the `onLayout` and observer
  reports, the `outline` ring, `offsetX` and `offsetY`, and what the pointer is on
  (events, hover, `cursor`, a press that focuses) start after its margin and end
  before it. A pointer in the margin is on the boxes around the span, or on the
  container; in the border and the padding it is on the span.
- A text selection or a search match that starts or ends on the first or last
  glyph of a span leaves the space off the end of its wash. A selection that runs
  across a span washes the space inside it, so it has no holes.
- The lengths are CSS [lengths](#lengths). `em` is the span's own font size (its
  `fontSize` if it or a box around it sets one, else the container's), and a
  percentage margin or padding is of the width the paragraph is laid out in, which is the containing block's width when the
  paragraph fills its container. While the paragraph is measured for its content
  (`width: "min-content"`, a flex item without a width) a percentage counts as 0, as
  CSS treats it in intrinsic sizes.
- Deviations: an `auto` or `anchor-size()` margin counts as 0, as `auto` does on an
  inline box in CSS, a negative `marginLeft` or `marginRight` pulls the text as in
  CSS, and a negative padding or border width counts as 0 (a `calc()` of a padding
  is clamped as a whole). A border side needs a line style, as on a `div`: without
  one its width is 0. The space of a span that holds no text left
  after white space processing, or none of whose glyphs are painted, is lost: it
  has no glyph to attach to. A span cut by `text-overflow` or `lineClamp` at its
  end loses its space with the cut.

### Painting an inline box

A `<span>` or nested `<text>` paints its `backgroundColor`, its border (`border`,
`borderWidth`, `borderColor` and the per-side forms) and its `borderRadius` (and
the four corner longhands) around each line of its text, as a CSS inline box does
by default (`box-decoration-break: slice`). They are painted behind the glyphs, by
the same code that paints an element's, so a `hover` or `focusVisible` style can
change them, a nested box paints over the box around it, and a selection or a
search match lies over them.

```tsx
<text style={{ lineHeight: 28 }}>
  Press{' '}
  <span style={{ paddingLeft: 6, paddingRight: 6, paddingTop: 2, paddingBottom: 2,
    backgroundColor: '#2a2a35', border: '1px solid #44445a', borderRadius: 4 }}>
    Enter
  </span>{' '}
  to go on
</text>
```

- **A box is as tall as the content area of its font, not the line.** Its border
  box is the font's ascent and descent, centred in the line as the glyphs are, plus
  the `paddingTop` / `paddingBottom` and the `borderTopWidth` / `borderBottomWidth`
  of the span. A `lineHeight` taller than the font leaves the background short of
  the line box, and a `lineHeight` shorter than the font lets the background reach
  out of it. `getBoundingClientRect().height` is that height. A vertical padding or
  border moves no line, so it overlaps the lines above and below, and the
  container's height does not change.
- **A box that wraps is cut at the break.** Each line of the span is painted as a
  fragment: the left border and the rounded left corners are on the first fragment
  only, the right border and the rounded right corners on the last only, and the
  top and bottom border run along every fragment. The side where a fragment is cut
  is open, with no border and a square corner. The lines in between have neither
  side. The padding follows the same rule, because it is only on the first and
  last lines (see [Space around an inline box](#space-around-an-inline-box)).
  `box-decoration-break: clone` is not available.
- The border is read like a `div`'s: a side needs a [line style](#borders) to have
  any width (`borderWidth` and `borderColor` alone draw nothing), a missing
  `borderColor` is `currentColor` for a shorthand and clear for `borderWidth` alone,
  and every line style is drawn, on each side as it is declared. A radius is the one a `div` has, elliptical and in any length, and a
  corner is the corner longhand, else `borderRadius`.
- The `outline` ring follows the corners of the fragment. The pointer, the cursor
  and the rect that `ResizeObserver` and `IntersectionObserver` report use the same
  border box, so a point in the padding above the text of a span is on the span and
  a point in the line box above the padding is not.
- Only `backgroundColor` paints on an inline box: `backgroundImage` and gradients,
  `boxShadow` and `filter` are not read there. The metrics are the
  line's, so a span in a font whose ascent and descent differ from the rest of the
  line has the line's content area, not its own.
- **`opacity` is a compositing group.** A box with an `opacity` below one paints its
  background, border, outline, glyphs and text decorations, and what the boxes inside it
  paint, opaque into a layer that is faded as a whole, as CSS does: the glyphs over the
  box's own background do not show it through twice, and a box inside another is faded
  by both. Like a stacking context it paints after the text around it, so it lies over
  what that text overlaps, and the group follows the box across the lines it wraps over.
  `display: contents` has no box, so its `opacity` does nothing. Selection and find
  washes on its text are painted under the group. An atomic inline inside the box is an
  element of its own: `opacity` on the box does not fade it.
- **What a box shows transitions and animates.** `color`, `backgroundColor`,
  `textDecorationColor`, `outlineColor`, `outlineWidth`, `outlineOffset`, the border
  colours and widths, the radii, `opacity`, the padding and margins, `fontSize`,
  `fontWeight`, `fontVariationSettings`, `lineHeight`, `verticalAlign`, `textUnderlineOffset` and
  `textDecorationThickness` take part in [CSS transitions](#css-transitions) and
  [CSS animations](#css-animations) as an element's do, natively and without a
  render per frame, with the same timing, delay, `transition-behavior` and
  interpolation (Oklab, or sRGB for legacy colours). A
  `<mark style={{ backgroundColor, transition: 'background-color 200ms' }}>` fades
  when its colour changes, and a keyframe animation on a span can pulse it. The
  padding, margin, border widths and font take space along the line: while one of them
  moves the paragraph is laid out again every frame, and the text after the span moves
  with it. A property a span does not have (`width`, `display`, ...) changes at once and
  starts no event. `onTransitionRun` and the other transition and
  animation events fire with the span as `target`. A property written only inside the
  `border`, `borderTop` or `textDecoration` shorthand is not a target, as for an
  element: write the longhand. `motion` runs on a span
  as [`motion.span`](#animate-an-inline-span).

### Events on inline boxes

A `<span>`, and a nested `<text>`, take the pointer events of an element
(`onClick`, `onPointerDown`, `onMouseEnter`, `onContextMenu`, `onDragStart`, ...)
and stay inline. An inline box has no box of its own, so the pointer is on it when
it is on one of its glyphs, and the span is then the event's `target`. The path
runs from the span through the spans around it to the paragraph's container and
its ancestors, so the container's `onClick` hears a click on the span as it
bubbles up, and `stopPropagation()` on the span keeps it from there.

```tsx
<text onClick={() => console.log('line')}>
  Read the <span onClick={(event) => open(event.target)} style={{ color: '#7ee787' }}>docs</span> first
</text>
```

- Every span under the pointer is the target, listening or not: a click on a
  span with no handler reaches the text's `onClick` with the span as `target`.
- A span that wraps is on both lines. The blank area is not on it: after the end
  of a line, below the last line, and the padding of the container belong to the
  container.
- Moving between two spans of a paragraph sends `mouseleave` and `mouseenter` (and
  `mouseout` and `mouseover`) to those spans only. The container is still hovered,
  so it hears nothing.
- `hover`, `active`, `groupHover` and `groupActive` on a span change the run it
  paints: `color`, `backgroundColor`, `fontWeight`, `fontFamily`, `textDecoration`
  and the other properties an inline box can set, and they can ring it with an
  `outline` (see [Focus on an inline box](#focus-on-an-inline-box)). A
  `groupHover` follows the `group` of the container or of an element above it.
- `pointerEvents: "none"` and `inert` on a span, or above it, keep the pointer
  off it, as on an element.
- `cursor` on a span shows over its border box, as in a browser: it inherits, so the
  innermost box around the glyph that sets one wins, and a glyph in no box that
  sets one keeps the container's cursor (`hover` and `active` can carry it too).
  A `display: contents` span has no box but its descendants inherit its cursor.
  A span that wraps shows it on every line it covers, and, like events, not on the
  blank area of the lines. A paragraph whose container has `pointerEvents: "none"`
  or is `inert` shows none.
- A press on a span still starts a text selection, unless a `mousedown` handler
  calls `preventDefault()`.
- `offsetX` and `offsetY` are measured from the padding edge of the span's first line,
  as for an element: after its margin and its left border (see
  [Space around an inline box](#space-around-an-inline-box)).
- `click()` on a span's ref, or `renderer.clickElement(id)`, clicks it by id, with the same path.

### Measure and observe an inline box

A span has no box of its own, so its `getBoundingClientRect()` is the union of the
rects of its lines, in window coordinates: the smallest rect that holds every line
it covers. Each is a line of its border box, so the margin of the span is outside
it and its padding and border are in (see
[Space around an inline box](#space-around-an-inline-box)). A span on one line is
as wide as its glyphs and as tall as the content area of its font plus its
vertical padding and border, which is not the line height (see
[Painting an inline box](#painting-an-inline-box)). A span that wraps is as tall as the lines
it covers, from the top of the first line's content area to the bottom of the last one's,
and it spans from the leftmost
start to the rightmost end of its lines, so it also covers the part of those lines
that is not the span's. There is no `getClientRects()`. A space that hangs at a
wrap belongs to the line it ends. A span with no text left (only collapsible white
space), `display: none` or `visibility: hidden` has no box and returns a zero
rect, as in the DOM, and `renderer.getElementBounds(id)` returns `null`.

`ResizeObserver` and `IntersectionObserver` take a span. A `ResizeObserver` reports
the union's size, and the span's content box is its border box: the padding and
border of the span are not taken off it. An `IntersectionObserver` intersects the
union with the clips of the paragraph's container and its ancestors, so a span that wraps counts the gap in
its rect as visible. A span that stops painting is reported as 0 by 0 and not
intersecting.

### Focus on an inline box

A `<span>` with a `tabIndex` is focusable like any element, and stays inline:

```tsx
<text>
  Read the <span tabIndex={0} onKeyDown={(event) => event.key === 'Enter' && open()}>docs</span> first
</text>
```

- It is in the tab order at its place in the document, between the focusables
  before and after the paragraph and in tree order among the spans of the
  paragraph, a span around another before it. `tabIndex={n}` with `n > 0` and
  `-1` mean what they mean on a `div`.
- `focus()`, `blur()`, `document.activeElement` and `matches(':focus')`,
  `':focus-visible'` and `':focus-within'` (of the container and its ancestors)
  work on its ref. `focus`, `blur`, `focusin` and `focusout` have the span as
  `target`, the element on the other side of the move as `relatedTarget`, and
  `focusVisible` as for an element. Tab and Shift+Tab move focus into and out of
  it, and scroll it into view by the rect of its lines.
- A press on its glyphs, padding or border focuses it, the innermost focusable span
  if they are nested, even when the paragraph is inside a focusable element. A
  focusable `<span>` with `tabIndex={-1}` is focused by a press or `focus()`, and
  Tab skips it. A press beside the glyphs (the blank area of a line, the padding of
  the container, the margin of the span) is a press on the container. A
  `mousedown` handler that calls `preventDefault()` keeps the focus where it was.
- `inert` and removing `tabIndex` work as on an element: the span leaves the tab
  order and gives up focus.
- `role` and `aria-*` on the span are exposed with it. The accessibility node
  has no size, so a screen reader finds it at the top-left of the paragraph, not
  over its glyphs.
- A span with no text left, or with `display: none`, has no box and no tab stop.
  Like a `display: none` element, it still takes `focus()`, and its `focusVisible`
  and `focusWithin` styles are not painted.

**Focus styles on a span.** `focusVisible` (while the span matches
`:focus-visible`) and `focusWithin` (while it or a span inside it has focus) are
style variants of a span like of any element, and `focusVisible` wins over
`focusWithin`, which wins over `hover` and `active`. The properties an inline box
can set change its run, and `outline` (or `outlineWidth`, `outlineStyle`,
`outlineColor` and `outlineOffset`) draws a focus ring. The base style, `hover` and
`active` can declare an outline too:

```tsx
<span tabIndex={0} style={{ focusVisible: { outline: '2px solid #89b4fa', outlineOffset: 2 } }}>
  docs
</span>
```

- The ring is drawn around each line fragment of the span's text, so a span that
  wraps has one ring on each line, not one box round both. `outlineOffset` moves
  it out from the fragment (a negative one draws it inside), it takes no layout
  space, and it is painted over the text of the span, as an outline is. It is cut
  by the clip of the paragraph (an `overflow` above it) and nothing else.
- The ring of a nested span is its own: focusing the span inside another one
  rings the inner span only, unless the outer one declares `focusWithin`.
- An outline needs a style, as on an element: `outlineWidth` and `outlineColor`
  alone draw nothing (see [Outline](#outline)). The ring follows the span's
  `borderRadius`.
- **Deviation: a span is never dimmed.** While a control has keyboard focus the
  other focusables dim ([Focus styles](#focus-styles)). A focused span starts that
  dim for the elements, like any control, but a span is not dimmed when another
  control has focus, and `keyboardFocusDim` is not read on one: a dim is an
  opacity, and a span has no element to set it on.

### White space

`whiteSpace` (`normal`, `nowrap`, `pre`, `pre-wrap`, `pre-line`, `break-spaces`) is inherited, so
it can be set on a container, on a `<span>` or on a `<text>`. It decides what is
left of the spaces, tabs and newlines of a string before it is painted, and
`getPaintedText()`, search, selection and copy all see that text.

| | Spaces and tabs | Newline | Wraps |
|---|---|---|---|
| `normal` (default) | collapse | a space | yes |
| `nowrap` | collapse | a space | no |
| `pre-line` | collapse | line break | yes |
| `pre-wrap` | kept | line break | yes |
| `break-spaces` | kept | line break | yes, after every space |
| `pre` | kept | line break | no |

- A run of collapsible spaces becomes one space, and the collapsing crosses text
  nodes and span edges: `a{" "}<span>{" "}b</span>` paints `a b`, with the space
  in the first run.
- A collapsible space at the start of a paragraph, at the end of one, before a
  block child and next to a line break is removed.
- A newline in `pre`, `pre-wrap`, `break-spaces` and `pre-line` is a forced break. One final
  newline ends the last line and does not start another, as in a browser.
- The no-break space (U+00A0) is content and never collapses.
- Text with nothing but collapsible white space makes no box in a `normal`,
  `nowrap` or `pre-line` block, and none in a flex or grid container whatever
  its `whiteSpace`.
- `getByText` and `findByText` compare text with its white space collapsed and
  trimmed, as Testing Library does, but they read the string nodes, so a match
  never crosses a span.

```tsx
<text style={{ whiteSpace: 'pre', fontFamily: 'Menlo' }}>{source}</text>
```

#### Tab stops

A tab that `pre`, `pre-wrap` and `break-spaces` keep stays a tab character in the painted text,
so search, selection and copy see `\t`, and it advances to the next tab stop.
`tabSize` sets how far apart the stops are. It is inherited and is 8 by default.

```tsx
<text style={{ whiteSpace: 'pre', tabSize: 4 }}>{"name\tvalue"}</text>
<text style={{ whiteSpace: 'pre', tabSize: '40px' }}>{"name\tvalue"}</text>
```

- A number is that many spaces, each as wide as a space in the font of the run
  that holds the tab. A string is a CSS [length](#lengths) without a percentage:
  `px`, the absolute units, `em`, `rem`, `vw`, `vh`, `vmin`, `vmax` and `calc()` of
  them (`em` is the container's font size). `ch`, a percentage and a negative value
  throw, like any style that cannot be parsed.
- The stops are measured from the start of the line of text the tab is on, not
  from the box: a newline starts a new count, and so does a soft wrap. Where a
  line breaks therefore decides where its tabs end, and a tab that pushes a word
  past the edge sends the word to the next line.
- A tab to a stop it already sits on goes to the next one, so a tab always has
  width (except with `tabSize: 0`, which makes it empty).
- Only a container sets it. A `tabSize` on a `<span>` or on a `<text>` inside a
  paragraph does nothing, because a paragraph is one string with one tab size.
  A `<text>` that is a block of its own, and a `<div>`, set it for their text.
- The text of `<code>` and `<diff>` inherits it too, so a tab there gets a stop
  where it used to get the font's own tab.

#### Hanging spaces

In `normal`, `pre-line` and `pre-wrap` the spaces and tabs at the end of a line
hang, as in CSS: they are not measured, so they neither wrap a line nor take part
in aligning it.

- A space that does not fit stays at the end of its line instead of wrapping. A
  run of spaces at the edge makes no line of its own, and the next line starts
  at the first character after it.
- `textAlign: 'center'` and `'right'` ignore them. That holds for a space where
  the line wraps and, in `pre-wrap`, for the spaces at the end of the text or
  before a line break.
- A hanging space is still there: it is selected, copied and found by search, and
  a selection or highlight over it reaches past the edge of the box.
- The size of a box that shrinks to its text still contains the spaces at the end
  of its last line.
- `pre` and `nowrap` do not hang anything: in `pre` the spaces are content and
  count in the alignment. Nor does `break-spaces`, see below.

#### Break spaces

`break-spaces` is `pre-wrap` where the spaces and tabs always take up room: a line
may break after every one of them, and none hangs.

```tsx
<text style={{ whiteSpace: 'break-spaces', width: 120 }}>{"a      b"}</text>
```

- A space that does not fit moves to the next line and starts it, so a run of
  spaces breaks between any two of them, and a line of nothing but spaces wraps.
- The spaces at the end of a line are measured: they count when the line is fitted
  and `textAlign: 'center'` and `'right'` include them, as do the spaces at the end
  of the text or before a line break.
- A word that does not fit still overflows or breaks inside as `overflowWrap` says.
  With the default, `normal`, the space after it stays at the end of its line.

#### Breaking words

`wordBreak` and `overflowWrap` are inherited and decide where a line may break
inside a word, and what happens to a word that is longer than its line. They need
a value of `whiteSpace` that wraps, and only a container sets them: a `<span>` or
`<text>` inside a paragraph uses the paragraph's, because it is one string.

| `overflowWrap` | A word that does not fit |
|---|---|
| `normal` (default) | overflows its line, and the line breaks at the next place there is one |
| `break-word` | is cut where it overflows, if no earlier place on its line will do |
| `anywhere` | as `break-word` |

| `wordBreak` | Breaks |
|---|---|
| `normal` (default) | where Chrome breaks: after spaces and hyphens, between CJK characters, and wherever the Unicode line breaking rules allow (see below) |
| `break-all` | also between any two letters, so a line is filled to its edge even when a space comes earlier |
| `keep-all` | not between two letters or numbers, CJK characters included, which behave like the letters of a word |
| `break-word` | as `normal` with `overflowWrap: 'anywhere'`, whatever `overflowWrap` says |

```tsx
<text style={{ width: 120, overflowWrap: 'break-word' }}>{"https://example.com/a-very-long-address"}</text>
<text style={{ width: 120, wordBreak: 'break-all' }}>{"aaaa bbbb cccc"}</text>
```

- Without `overflowWrap` a long word, a URL or an identifier is not broken: it
  stretches past the edge of its box, as it does in a browser.
- The places a line may break are Chrome's: Blink's table for ASCII and Latin-1 pairs, then the
  Unicode line breaking algorithm (UAX #14, ICU's `line-break: auto`) for the rest. A line
  breaks after a space, a hyphen (U+002D, U+2010, U+2013) or a `?`, and before `(`, `<`, `[` and
  `{` only after punctuation, so `art(x)` and `a/b` do not break and `a/b?c` breaks after the `?`.
  A hyphen breaks in front of a letter, a quote, `(` or a symbol such as `%`, and not in front
  of `.`, `,`, `)`, `!`, `?`, `/`, `:`, `;`, `]`, `}` or `$`; a `?` is the same, and does not
  break in front of a quote either. In front of a digit a hyphen breaks after a letter or a digit
  (`ABCD-1234`, `2026-10-19`) and not in a minus sign (`-5`, `a -5`). U+2011 (the non-breaking
  hyphen) and the no-break space are glue. Thai, Lao, Khmer and Burmese are split into words
  with a dictionary. `width: 'min-content'` of a text is its longest piece.
- `break-all` does not break before closing punctuation (`)`, `!`, `,`...), after
  opening punctuation or next to a no-break space, and a space still hangs at the
  edge as it does without it.
- `keep-all` keeps letters and numbers together, CJK characters among them, but not the Thai
  family of scripts; a space, a hyphen and punctuation are still places to break. Without
  one a run of them overflows, unless `overflowWrap` cuts it.
- `<input>` and `<textarea>` ignore both and break a long word at the edge, as a browser's
  `textarea` does.
- A value CSS does not have throws, like any style that cannot be parsed.
- Text laid out by GPUI itself, as Zed does, keeps breaking such a word at the
  edge: its `TextStyle::overflow_wrap` defaults to `BreakWord`, and `Normal` is
  what gpuix sets for all of its text.

Deviations, where GPUI's text layout cannot build the DOM model:

- In `normal` and `pre-line` a space at a soft wrap stays in the text, hanging,
  where CSS removes it, so selection and copy see it.
- The Unicode line breaking rules come from ICU4X, Chrome's from ICU, and they agree on
  everything but a few rare sequences: a closing or opening quote next to a small kana or the
  prolonged sound mark (`”ぁ`), and the word boundaries of Lao and Khmer, whose dictionaries differ.
- `hyphens` and `line-break` are not styles: a soft hyphen (U+00AD) is a place to break and
  draws no hyphen where it does, and the strictness is Chrome's `auto`.

#### Balanced and pretty lines

`textWrap` is the inherited shorthand of `textWrapMode` (`wrap`, `nowrap`) and
`textWrapStyle` (`auto`, `balance`, `pretty`, `stable`): a mode, a style, or both in
either order (`"balance"`, `"nowrap"`, `"nowrap pretty"`). What it leaves out is the
longhand's initial value, so `"balance"` also turns wrapping on. Of the shorthand and a
longhand on the same element the one written later wins, and in a state style the
shorthand resets the base's `textWrapMode` and `textWrapStyle`. A value CSS does not have throws.
The mode is inherited by the text of a `<span>` and can be set on one; the style is read from the
container of the paragraph alone, as Chrome does, so a `textWrapStyle` on a `<span>` does nothing (it is
inherited by a block inside it).

```tsx
<text style={{ width: 320, textWrap: 'balance' }}>{title}</text>
<text style={{ width: 320, textWrap: 'pretty' }}>{paragraph}</text>
```

- **`balance`** evens out the lines of a paragraph of at most 6 lines (each stretch
  of text between forced line breaks counts as one paragraph), without making more
  lines than `auto` does.
- **`pretty`** keeps the last line from being a single short word (under a third of
  the line), by breaking the last four lines at most differently. A paragraph whose
  last line is longer is left as `auto` has it.
- The text is as wide as it is with `auto`: only the breaks move, so a box that
  shrinks to its text keeps its width and `textAlign` aligns within it. `stable` is
  `auto`.
- Neither applies to text with a tab, `whiteSpace: 'break-spaces'` or `lineClamp`,
  or where a word is wider than its line, as in Chromium. The breaker approximates
  Chromium's, so the lines can differ from a browser's.
- **`textWrapMode: 'nowrap'`** keeps the text on one line and wins over the wrapping
  part of `whiteSpace` when both are declared on the same element. It leaves what
  `whiteSpace` does to spaces and newlines alone, so `whiteSpace: 'pre'` with
  `textWrapMode: 'wrap'` is `pre-wrap`.
- **On a `<span>`** the mode (or the wrapping part of its `whiteSpace`, which `textWrapMode` and
  `textWrap` on the same span win over) decides where a line may break in its text, and a nested
  span inherits it. A line does not break inside a `nowrap` span, so a span that does not fit moves to the
  next line whole, and a long word in it overflows; a `wrap` span in a `nowrap` block breaks
  normally, and the block wraps for it. Where a line may break at the edge of a span follows
  Chrome: a space belongs to the box it is in, and the word before it lets the line break after it
  too, unless a `nowrap` box starts at the space; between two letters (`wordBreak: 'break-all'`,
  CJK) the box around both lets it, or either side.

#### Truncating text

`textOverflow: 'ellipsis'` cuts the text with `…` after the last glyph that fits.
It cuts the text as it is laid out, so the lines it keeps are the lines the text
wraps to without the cut:

- With `lineClamp: n` the text keeps its first `n` lines, each of them as the
  paragraph wraps it, and the `n`th ends in `…`. A newline is a line.
- Without `lineClamp` a line that wraps is cut to its first line, and with
  `whiteSpace: 'nowrap'` or `'pre'` each line of the text is cut at the edge of
  the box on its own.
- Tab stops, the spaces that hang at a soft wrap, `break-spaces`, `wordBreak` and
  `overflowWrap` take part as they do in the layout. A word that `overflowWrap:
  'normal'` leaves to overflow its line does so, unless it is on the last line
  that is kept, where it is cut at a glyph.
- The spaces and ASCII punctuation just before the `…` are dropped (`Hello,`
  becomes `Hello…`), where a browser keeps them.

`textOverflow: 'ellipsis-start'` still decides where to cut by the width of each
character along one line, so a tab counts as the font's own width of one, not as a
tab stop, and `lineClamp` is a line of `n` times the width.

### Trimming text boxes

A heading in a padded box has more space above its capitals and below its baseline
than the padding says, because a line is as tall as the font's ascent, descent and
line height. `textBoxTrim` cuts that space off a block container, so the padding is
the space that shows.

```tsx
<div style={{ padding: 8, backgroundColor: '#313244', textBox: 'trim-both cap alphabetic' }}>
  <text>Title</text>
</div>
```

- **`textBoxTrim`** (`none`, `trim-start`, `trim-end`, `trim-both`; not inherited).
  `trim-start` cuts the space above the first line of text down to the over edge,
  `trim-end` the space below the last line down to the under edge. The container
  gets shorter by what was cut and what follows moves up. Its padding and border stay:
  the trim is inside them. Ink the edge leaves out (a tall letter above `cap`) overflows.
- **`textBoxEdge`** (inherited, `auto` is `text`) is the metric it cuts to,
  `<over> <under>`. Over is `text` (the font's ascent: only the half-leading above is cut), `cap`
  (cap height) or `ex` (x-height). Under is `text` (the descent) or `alphabetic` (the
  baseline). One value is both where it can be, otherwise the under edge is `text`:
  `"cap"` is `"cap text"` and `"ex"` is `"ex text"`. Chrome rejects a lone `cap` or `ex`
  today; the spec allows it.
- **`textBox`** is the shorthand: `normal`, or a trim and an edge in either order. A
  trim left out is `trim-both`, not `none`, and an edge left out is `auto`. A longhand
  written after it wins over it. An invalid value is ignored.
- **Which lines.** Only the first and last line of text are trimmed. The trim goes
  down through block children to their first or last line, and stops at one with
  padding or a border on that side, or with no line of text to start with. An absolutely
  positioned child is not a first or last child. A flex or grid container trims
  nothing and passes nothing down. `none` on a descendant does not take off a trim
  from above.
- The metrics are those of the font of the block that holds the line. A trimmed
  box is as tall as the metric to the nearest 1/64 of a pixel, as in Chrome, and its edges
  are painted on device pixels.
- **Baselines.** The baseline of a trimmed text is counted from the top of its trimmed
  box, as in Chrome, so a cap-trimmed label in an `inline-block`, `inline-flex` or
  `inline-grid` sits on the baseline of the line around it, and a trimmed flex or grid item
  with `alignItems: 'baseline'` is on the baseline its row shares.

### Inline-block, inline-flex, inline-grid and inline-table

`display: 'inline-block'`, `'inline-flex'`, `'inline-grid'` and `'inline-table'` make an element an
atomic inline: a box that sits on a line of text as one unit, wraps with the text
around it, and is an ordinary element inside (a block, a flex container, a grid
container or a [table](#tables)). It is a `<div>`, `<span>`, `<text>` or any other element; the
paragraph it is in lays it out, and everything else about it is the element's own.

```tsx
<text>
  Status{' '}
  <div style={{ display: 'inline-flex', gap: 4, padding: 2, backgroundColor: '#1f6feb' }}>
    <svg source={dot} style={{ width: 8, height: 8 }} />
    <span>online</span>
  </div>{' '}
  since <span style={{ verticalAlign: 'super', fontSize: 12 }}>3</span> days
</text>
```

- **Where it is atomic.** In a block container (and inside the spans of a
  paragraph) each `inline-*` child is one unit of the paragraph, with a margin box
  that takes the line the way a word does: `margin` is around it, the line is as
  tall as it needs, and the text around it wraps before and after it. An `inline-*`
  child with no text around it still sits on a line, with the strut of the
  container's font: a lone 30px box is 30px plus the descender gap tall, as in a
  browser. Where the parent lays out items (a `flex` or `grid` container, `inline-flex` and
  `inline-grid` included) or the child is `position: absolute` or `fixed`, `display` is
  blockified as CSS Display 3 says: `inline-block` is `block`, `inline-flex` is `flex`,
  `inline-grid` is `grid`, `inline-table` is `table`. A table or an `inline-table` that is a child of a table, row
  group or row is not a part of it: it is content of a cell, which an anonymous cell wraps.
- **Width.** `width: auto` is shrink-to-fit: the content's width, but at most the
  width the paragraph is laid out in and at least its min-content width. A `width`
  in `%` (or a `calc()` with one) is of the paragraph's width, and a percentage on a
  child of an `auto` box is of the width the box came to. A `minWidth` or `maxWidth` in `%`
  is of the paragraph's width too, and `width: 'fit-content'` under a `maxWidth: '50%'` hugs
  its content up to half of it. `margin` in `%` is of the paragraph's
  width as well, and an `auto` margin is 0.
- **Relative position.** `position: relative` moves the box by `left` or `right` and `top` or `bottom`, and
  nothing else on the line moves with it. A percentage `left` or `right` is of the width of the block
  that holds the line, and a `top` or `bottom` of its height when that is definite (a `height`, a
  percentage of one that is, a flex item the container sizes) and `auto` otherwise, as in a browser.
- **Baseline.** An `inline-block` sits on the baseline of the last line box of its
  in-flow content, or on its bottom margin edge when it has none or its `overflow` is
  `hidden`, `scroll` or `auto` (`clip` keeps the baseline). An `inline-flex` or `inline-grid` sits on the first
  baseline of its container: the first item's (in `order`), the first line of its text
  or, for an item with no text, the bottom of its border box; with no items, the bottom margin edge. An `inline-table`
  sits on the first baseline of its table: that of its first row (below the captions above it, the border, the padding and the
  spacing), or the bottom of that row when none of its cells is on the baseline. Absolutely positioned
  content has no baseline, and a box inside another atomic inline is its own.
- **It is an element.** `getBoundingClientRect()`, `onLayout`, `ResizeObserver` and
  `IntersectionObserver` report its border box (with the fraction of its place in the line, as Chrome keeps
  it; it is painted at the device pixel that place rounds half up to), pointer events, `hover` and `active`
  styles and hit testing see it where it is painted (the span around it is its parent for events and
  `:hover`), it takes `tabIndex`, focus and `focusVisible`, `scrollIntoView` scrolls
  to it, its text is selected, copied and found by `highlight`, `transition`, `animate` and
  `motion` run on it, and `display: 'none'` removes it. Order, `key`s and reordering are the
  tree's.
- **Text decoration and colour.** A `textDecoration` of the container or of a span around the box
  is not drawn under it or into it, as in CSS. It inherits the text style of the boxes around it (`color`, `fontSize`,
  `fontFamily`, `lineHeight`...) like any child.

#### Vertical alignment

`verticalAlign` places an inline box on its line, on a `<span>` or on an atomic
inline: `baseline` (the default), `sub` and `super` (lowered by a fifth of the
font size of the box around it plus 1px, raised by a third plus 1px, as Chrome does),
`text-top` and `text-bottom` (the top and bottom of the content area of the font of the
box around it), `middle` (half the x-height of that font above its baseline), `top` and `bottom` (the top and bottom
of the line box, which grows to hold the box), a length (raises the box by it, a negative one lowers it) and a percentage
(of the box's own `lineHeight`, which an `inline-block` inherits from the boxes around it
like any text style). A nested box is aligned to the box around it, and the shifts add up. A line is as tall as
CSS 10.8 makes it: the container's strut and every box on the baseline around the shared
baseline, then the `top` and `bottom` ones. A span has its strut (its font's ascent and descent with the
half-leading of its `lineHeight`) in every line it has something on, whatever it holds: a span with a
`lineHeight` of its own around nothing but a box makes the line as tall as that strut. The bounds, the background, the border and the
outline of a span that is raised or lowered move with its text. `verticalAlign` on a table cell is
the alignment of the cell in its row (`baseline`, `top`, `middle`, `bottom`, see [Tables](#tables)), and an
`inline-table` is aligned on its line by its own `verticalAlign` like any other atomic inline.

#### Deviations

- A text is as wide as its advance up to 1/64 of a device pixel (1/128 px at 2x), as in Chrome, and boxes are laid out in
  fractions of a pixel: a length you write keeps its fraction, cut toward zero to 1/64 of a device pixel as
  Chrome's `LayoutUnit` cuts it (`100.4px` is `100.390625`, `-10.01px` is `-10` at 1x and `-10.0078125` at 2x, and the
  result of a `calc()` is cut, not its terms). The edges of a box snap, half up, to the device pixel it is painted on, and a box with a size is
  never snapped to none, so `33.3px`, `33.3px` and `33.4px` side by side paint 33, 34 and 33 pixels with no gap, and a
  border width is snapped as a border width (floored to whole device pixels, a width under one is one pixel). Painted
  positions are snapped but `getBoundingClientRect` is not, it reports the layout in 1/64 of a device pixel as Chrome
  does. A percentage is cut the same way wherever layout resolves it, tables included (`25%` of 101.296875 is
  25.3125), and a flex item's share of the free space is rounded to that unit the way Chrome rounds it. A few
  cases are still a 64th off: a table row that takes the rest of the table's height is short, `top: 50%` on a
  relatively positioned box in a block reads 0, the leftover of `minmax(%, 1fr)` next to a `1fr` track, the
  `align-content: stretch` of a wrapping flex container at 2x, and a flex item centred in its container (its offset
  is not truncated, so a centred label can be a 64th off). A line of several runs is ceiled once where Chrome
  ceils each run, and a min-content word that kerns with the space before it is up to a unit wider.
- Inside a span with `verticalAlign: top` or `bottom` nothing can be aligned on
  its own: what it holds goes with it. The `getBoundingClientRect()` of a span that holds nothing but atomic
  inlines is its line box, since it has no text to take the content area of its font from (a browser
  reports that content area).
- An `absolute` descendant of an atomic inline is positioned against the atomic inline (it is a layout root),
  and the box is its own stacking context. A percentage `height` inside it sees an `auto` height, which CSS
  does for a shrink-to-fit box too.
- `textOverflow` and `lineClamp` ignore a paragraph with an atomic inline in it: cutting the text would cut its
  placeholder, so it overflows.
- The text in an atomic inline is a text of its own for [selection](#text-selection) and [search](#text-highlighting-and-search),
  and it falls where the box sits in the paragraph, as in a browser: a drag across the box selects the text
  before it, the box's text and the text after it, in that order; the copied text reads `a b c` for
  `a <box>b</box> c` (the placeholder is left out, and the text of a block in the box starts a line of its own,
  as the DOM does); and the matches of a `highlight` are numbered in the same order. A match cannot cross the box.
- An `inline-block`, `inline-flex` or `inline-grid` box is a [query container](#container-queries) like any other box (`containerType` on it works, and an
  `inline-size` one without a `width` is as wide as its padding and border), and the boxes inside it or inside a
  container around its paragraph read `@container` blocks and `cq` units in the frame that lays them out. Hovering
  or pressing something in it restyles the `hasHover` and `hasActive` of its ancestors, the paragraph's element
  and its own `has*` styles included.

## Text input

`<input>` and `<textarea>` use GPUI's platform input handler. They support a
native caret, text selection, IME composition, clipboard actions, undo/redo,
grapheme-safe deletion and mouse positioning.

```tsx
<textarea
  value={draft}
  placeholder="Ask anything"
  minRows={1}
  maxRows={8}
  onChange={(event) => setDraft(event.value ?? '')}
/>

<textarea
  value={draft}
  onChange={(event) => setDraft(event.value ?? '')}
  onKeyDown={(event) => {
    if (event.key !== 'Enter' || event.modifiers?.shift) return
    event.preventDefault() // no newline
    send(draft)
  }}
/>
```

`Enter` inserts a newline in a `<textarea>`; to send on Enter instead, handle it
in `onKeyDown` and call `preventDefault()`, as above. Enter in an `<input>`
submits its [form](#forms), if it has one and no other `<input>` shares it. There is no `onSubmit` on `<input>`
or `<textarea>`. The editor updates natively first, then
reports the complete value to React.
`value` changes can replace the native content, but keeping the same prop value
does not reject an edit like a browser-controlled input.

The focused caret stays solid during edits and then blinks every 500ms while
idle. It stops scheduling repaint frames on blur or while the window is
inactive. Override its colour through the shared native theme:

```tsx
<input theme={{ caret: '#22c55e' }} />
```

When the clipboard has no text, `Cmd+V` or `Ctrl+V` continues to `onKeyDown`
instead of disappearing inside the editor. Applications can then handle an
image-only or file-only clipboard themselves. Copied files also propagate even
when the operating system includes their paths as fallback text. Mixed text and
image clipboard content still pastes its text.

**`fontSize` and `lineHeight`** in `style` size each row. Without `lineHeight`,
the row is `normal`, the height of the font (see [Line height](#line-height)), so a
larger `fontSize` grows the box.
Pass `lineHeight` to set the row (`1.4` times the font size, or a length like
`"20px"`). `minRows` and `maxRows` multiply
that height. An explicit `height` still overrides both.

```tsx
<textarea
  value={draft}
  minRows={1}
  maxRows={8}
  style={{ fontSize: 14, lineHeight: "20px" }}
  onChange={(event) => setDraft(event.value ?? '')}
/>
```

### Selection, `disabled`, `type` and `maxLength`

A ref to an `<input>` or `<textarea>` has the DOM selection API. Offsets are
UTF-16 code units, and the getters are synchronous.

```tsx
const ref = useRef<TextControlInstance>(null) // Solid: a HostElement

<input ref={ref} value={value} onFocus={() => ref.current?.select()} />

ref.current.setSelectionRange(1, 4, 'backward') // direction: 'forward' | 'backward' | 'none'
ref.current.selectionStart // 1
ref.current.selectionEnd // 4
ref.current.selectionDirection // 'backward'
```

`select()` selects all the text and `setSelectionRange(start, end)` clamps both
ends to the text; an `end` at or before `start` collapses the selection to
`end`. Neither moves focus. A selection made from code reports
`selectionDirection` `'none'` until the user changes it, and so does Select All
(`Cmd+A`, triple click). A call right after a
controlled `value` update sees the new value, even before the next frame.
Select-on-focus works with `onFocus`, for keyboard focus and for a click.

`disabled` works like the DOM. The control is not focusable and not a tab stop,
a click does not focus it, and focus leaves it when it becomes disabled. It
shows no caret, takes no editing, and gets no `click` or `keydown` events. Its
text is painted in the theme's muted colour (`theme.textMuted`), and it is
reported as disabled to assistive technology. The `disabled` and `enabled`
[state styles](#form-control-state-disabled-enabled) follow it.

`type` on an `<input>` is `'text'` (the default), `'password'`, `'number'`,
`'email'`, `'search'`, `'tel'` or `'url'`. `type` on a `<textarea>` is ignored.

- `'password'` paints one bullet per character. `onChange` and `value` carry
  the real text. Copy and cut do nothing, double click selects all, and the
  value is not exposed to assistive technology (the role is `PasswordInput`).
- `'number'` makes `ArrowUp` and `ArrowDown` step the value by `step` (default
  1), clamped to `min` and `max`, and report it through `onChange`. Text that
  is not a number steps from 0. Typing is not restricted and `onChange`
  reports the raw text, so `'1e'` stays `'1e'`. There is no spinner. `min`,
  `max` and `step` are ignored for other types. It is exposed to assistive
  technology as a spin button with its numeric value, `min`, `max` and `step`.
- `'search'` is exposed to assistive technology as a search field.
- `'email'`, `'tel'` and `'url'` behave as `'text'`.

`maxLength` cuts what the user types or pastes so the text stays within that
many UTF-16 code units. A `value` set from code is kept whole even when it is
longer; the user can then only delete. An edit with no room for what it inserts
is rejected whole, so typing over a selection in a value that is already too
long leaves the text and the selection as they were.

### Scrollbars

Every container with scrollable `overflow` (`"scroll"` or `"auto"`), and every `<virtual-list>`,
gets a macOS-style scrollbar, in whichever style the system's "Show scroll bars" setting asks
for. macOS also switches to the second style while a mouse is attached, as a browser's bars do.

**Automatically or When scrolling** (overlay bars). The bar is hidden at rest and appears when the
scroll offset changes for any reason (the wheel, `scrollTo`, a key, a drag). It fades out after
about a second without movement, over 200 ms. While the pointer is over the bar it stays, a faint
track shows behind it, and the thumb thickens from 7 to 11 px over 150 ms. The bar is an overlay:
it takes no layout space and never changes what the container can scroll.

**Always** (classic bars). The bar is always there, a solid track and a thumb that neither fades nor
thickens, in a 15 px gutter (11 with `scrollbarWidth: "thin"`) that the container reserves out of its
own box, as in a browser. `clientWidth` and `clientHeight`, the content box that percentages and
`flex` lay children out in, and the range that `scrollIntoView` and the scrolling keys work against
all shrink by the gutter, while `getBoundingClientRect` still reports the border box. The vertical
bar takes width and the horizontal bar takes height, and the corner is filled when both show.
`overflow: "scroll"` always reserves its gutter and shows an empty track when nothing scrolls;
`overflow: "auto"` reserves it once the content scrolls, which takes one more frame to settle.
`scrollbarWidth: "none"` takes no room. A `<virtual-list>` cannot give up room: its classic bar is
always visible but sits over its rows. The setting is read when the window is built and again when
it changes. Under test the window starts with overlay bars regardless of the machine, and
`TestRenderer.setScrollbarStyle("classic")` switches it (`"overlay"` switches back).

On both styles the thumb drags, and that scrolls the container and fires `onScroll`. Pressing the
track pages toward the pointer, by 90% of the viewport. The bar's hitbox exists only where a bar is
visible, so an overlay bar at rest blocks nothing under the edge. The bar sits on the right edge,
and the bottom for sideways scrolling.

Two CSS properties style it:

```tsx
<div style={{ height: 300, overflow: 'auto', scrollbarWidth: 'thin', scrollbarColor: '#8888 #0001' }}>
  {rows}
</div>
```

| Property | |
|---|---|
| `scrollbarWidth` | `"auto"` (default; overlay: 7 px, 11 under the pointer; classic: a 15 px gutter), `"thin"` (overlay: 4 px, 7 under the pointer; classic: an 11 px gutter), or `"none"`, which draws no bar, takes no room and leaves the container scrollable. |
| `scrollbarColor` | `"auto"` (default), or `"<thumb> <track>"`: two CSS colours, the thumb and the track (faint under the pointer on an overlay bar, solid on a classic one). A value that is not two colours is ignored, as in CSS. |

By default the thumb is 50% black on a light window and 50% white on a dark one, following the
system appearance, and a classic track is a solid light or dark grey. A list's bar comes from the heights the list has measured (and
`estimatedItemHeight` for rows it has not reached), the same totals it scrolls by, so its length
settles as rows are measured. `padding` on a `<virtual-list>` insets its rows on all four sides,
and the bar stays at the list's own edge.

`scrollbarWidth` used to be a number of pixels and `scrollbarColor` a single thumb colour that
turned the bar on. Both are replaced by the CSS forms above, and the bar is on by default.

### Give memory back when idle

A JavaScript engine collects on allocation pressure, so after a burst of work
(the first mount, opening a large screen) the heap stays at its peak until the
next burst. `collectGarbageWhenIdle()` runs one full collection once the app has
stopped committing for a while, then waits for the next burst.

```tsx
import { collectGarbageWhenIdle } from '@gpuix/react'

const stop = collectGarbageWhenIdle() // checks every 5s, collects after two quiet checks
```

It uses `Bun.gc(true)`, or `gc()` where Node runs with `--expose-gc`, and does
nothing in a runtime that has neither. Quiet means no mutation batch reached a
renderer and the process is under half a core. The batch count decides, not CPU
alone, because a focused text field blinks its caret in native code and redraws
twice a second. Pass `activity`, `collect`, `checkMs`, `quietChecks` or
`busyCpu` to change what it watches or how often.

### Input in a search pill

`<input>` has **no default inner padding** and paints text at the top of its
box. A single-line input vertically centers its text when given extra height.
Set `padding` on the input style or on a parent wrapper. When the input has
`borderRadius`, text clips to the rounded shape automatically.

```tsx
<div style={{
  display: 'flex',
  flexDirection: 'row',
  alignItems: 'center',
  height: 32,
  paddingLeft: 10,
  paddingRight: 4,
  borderRadius: 16,
  backgroundColor: '#1a1a22',
  borderWidth: 1,
  borderStyle: 'solid',
  borderColor: '#ffffff14',
}}>
  <input
    value={query}
    onChange={(e) => setQuery(e.value ?? '')}
    style={{ flexGrow: 1, minWidth: 0, fontSize: 13, color: '#e8e8ed' }}
  />
</div>
```

## Accessibility

GPUI talks to the **macOS AX tree**, Windows UIA, and Linux AT-SPI through
AccessKit. GPUIX maps React props onto that API. A node is in the tree only
when it has **both** a GPUI id (always set) and a **role**.

Prop names match React DOM. Role **values** are ARIA tokens, not AccessKit
PascalCase. `"none"` and `"presentation"` produce no node.

```tsx
<div
  id="notes.delete"
  role="button"
  aria-label="Delete note"
  aria-description="Removes this note"
  onClick={remove}
>
  Delete
</div>
```

`id` is the DOM `id`. It becomes the node's author id (`AXIdentifier` / UIA
AutomationId), and it is what `htmlFor` and the id-reference props below
resolve against. It replaces the old gpuix-only `aria-id`, which is gone.

Any `aria-*` prop is accepted. Booleans may be written as `true` or `"true"`;
a value the attribute does not allow is ignored. These map to AccessKit:

| Prop | AccessKit |
| ---- | --------- |
| `aria-label`, `aria-description`, `aria-placeholder` | label, description, placeholder |
| `aria-valuetext` | string value |
| `aria-valuenow`, `aria-valuemin`, `aria-valuemax` | numeric value, min, max |
| `aria-checked`, `aria-pressed` | toggled (`true`, `false`, `"mixed"`) |
| `aria-expanded`, `aria-selected` | expanded, selected |
| `aria-disabled`, `aria-required`, `aria-readonly`, `aria-multiselectable`, `aria-busy`, `aria-atomic`, `aria-modal` | the matching flag |
| `aria-hidden` | hidden: the element and its subtree leave the tree; `inert` does the same |
| `aria-invalid`, `aria-live`, `aria-current`, `aria-haspopup`, `aria-orientation`, `aria-sort`, `aria-autocomplete` | the matching enum |
| `aria-multiline` | `role="textbox"` becomes a multiline text input |
| `aria-level`, `aria-posinset`, `aria-setsize` | level, position in set, size of set |
| `aria-rowindex`, `aria-colindex`, `aria-rowcount`, `aria-colcount`, `aria-rowspan`, `aria-colspan` | the matching table property, passed through unchanged |
| `aria-rowindextext`, `aria-colindextext`, `aria-roledescription`, `aria-keyshortcuts`, `aria-braillelabel`, `aria-brailleroledescription` | the matching string |
| `aria-labelledby`, `aria-describedby`, `aria-controls`, `aria-owns`, `aria-details`, `aria-flowto`, `aria-errormessage`, `aria-activedescendant` | relations, see below |

`aria-relevant`, `aria-grabbed` and `aria-dropeffect` are accepted and ignored:
AccessKit has no equivalent.

The relation props hold element `id`s, space-separated (`aria-errormessage` and
`aria-activedescendant` take one). They resolve against the DOM `id` of nodes
in the accessibility tree, after the whole frame is built, so the target may
come later in the tree. An `id` that matches no element is dropped. A role-less
element that one of these props names gets a `group` node so it can be the
target, named by its text content, as the DOM names an element from content.
`aria-activedescendant` on a focused container also reports the target as the
focused node. `aria-hidden` on an element with no role still works: the element
gets a hidden `group` node to carry it.

### `<label>`

`<label htmlFor="email">` names the `<input>` or `<textarea>` whose `id` is
`"email"`. Without `htmlFor`, the label names the first `<input>` or
`<textarea>` inside it. The control's accessible name comes from the label
(AccessKit `labelled_by`), unless the control has its own `aria-label` or
`aria-labelledby`. A control can have several labels. Clicking a label focuses
its control and dispatches a `click` on it (same position and `detail`, bubbling
along the control's own path), like the DOM. The nearest label on the click's
path is the one that activates, so a click inside a nested label reaches only
the inner label's control. Nothing happens when the click lands on the control
itself or inside it, on other interactive content inside the label (`input`,
`textarea`, `select`, `button`), when a handler cancelled the label's `click` with
`preventDefault()`, when the press began elsewhere than where the release
landed (the click then targets a common ancestor, which has no label in its
path), or when the control is `disabled` or `inert`. `label.click()` on a ref and
the VoiceOver Press on a label activate its control the same way. `preventDefault()` is known
on macOS and in the test renderer; on Windows and Linux the event is queued, so
a label always activates its control there.

```tsx
<label htmlFor="email">Email</label>
<input id="email" value={email} onChange={onChange} />
```

Native defaults, so common elements are not silent:

| Element       | Default role            | Name / value                         |
| ------------- | ----------------------- | ------------------------------------ |
| `<text>`      | `Label`                 | content as `aria-valuetext`          |
| `<input>`     | `TextInput`             | `value` and `placeholder`            |
| `<textarea>`  | `MultilineTextInput`    | `value` and `placeholder`            |
| `<img>`       | `Image`                 | `alt` as `aria-label`                |
| `<label>`     | `Label`                 | its text as `aria-valuetext`         |
| `<form>`      | `Form`                  |                                      |

An explicit `role` wins over those defaults. A clickable `div` is **not** a
button until you set `role="button"`. `onClick` registers AccessKit `Click`,
so VoiceOver Press fires the same JS `click` handler, as a DOM click no pointer
made: see [Click without a pointer](#click-without-a-pointer).

The browser / wasm renderer has no AccessKit adapter. These props are
no-ops there.

## Forms

`<form>` groups `<input>` and `<textarea>` controls and submits them together.
A control belongs to its nearest `form` ancestor, wherever it sits below it.
`name` is the key a control is submitted under, and `required` stops the
submission while the control is empty.

```tsx
<form
  onSubmit={(event) => {
    // A real FormData: [["email", "a@b.c"], ["notes", "hi"]]
    save(Object.fromEntries(event.formData))
  }}
>
  <input name="email" required />
  <textarea name="notes" />
</form>
```

- **Implicit submission.** Enter in an `<input>` submits its form when the form
  has at most one `<input>`. GPUIX has no submit button (a `div` with
  `role="button"` is not one), so no form has a default button, and HTML's
  fallback applies: a form with two or more `<input>`s does not submit on Enter,
  whether they are disabled, read-only or of any `type`, and a nested form's
  inputs do not count. Call `requestSubmit()` from a button's `onClick` for
  those. A `keydown` handler that calls `preventDefault()` stops it. A
  `<textarea>` does not count, and Enter in it always inserts a newline.
- **Validation.** Each `required` control with an empty value gets a
  non-bubbling, cancelable `invalid` event (`onInvalid`). The first one whose
  handlers did not call `preventDefault()` takes focus, and the form is not
  submitted. A `disabled` or `readOnly` control is not validated.
- **`submit`.** Cancelable and bubbling, aimed at the form. `event.formData`
  holds the `name` and `value` of every named, enabled control in document
  order. `event.submitter` is `null`: there is no submit button. Nothing
  navigates, so `preventDefault()` has no default to stop.
- **`requestSubmit()`** on the form ref (a React ref or the Solid element)
  validates, then submits, and delivers the events before it returns.

Both events are sent synchronously on macOS, so a handler can call back into the
renderer. On Windows and Linux they are queued like other events.

## Focus and keyboard navigation

Focus is a **native GPUI concept**. GPUIX connects stable React element IDs to
persistent `gpui::FocusHandle` values, so focus survives React rerenders:

```text
React <div tabIndex={0}>
            │
            ▼
Retained element ID ► persistent gpui::FocusHandle ► keyboard/action dispatch
            ▲
            │
      React rerenders
```

### What is focusable

Like the DOM: an enabled `<input>` or `<textarea>`, or any element with a
`tabIndex`, a `<span>` in a paragraph included (see
[Focus on an inline box](#focus-on-an-inline-box)). **A key or focus listener does not make an element focusable.**
`onKeyDown` on a parent still hears the keys of a focused descendant, by
bubbling, but a `div` that only declares `onKeyDown` never takes focus. Give it a
`tabIndex` (`-1` is enough when it should not be a tab stop).

```tsx
<div

```tsx
<div
  tabIndex={0}
  onFocus={() => setActive(true)}
  onBlur={() => setActive(false)}
  onKeyDown={(event) => {
    if (event.key === 'Enter') submit()
  }}
>
  Submit
</div>
```

| Prop | Behavior |
|---|---|
| `tabIndex={0}` | Joins the tab order, in document order: a flex or grid `order` moves what is painted, not where Tab goes |
| `tabIndex={n}`, `n > 0` | Tabbed first, in ascending order of `n` (document order among equal values), then the `0`s |
| `tabIndex={-1}` | Focusable by click, `focus()` or `focusElement`, but never a Tab stop |
| `autoFocus` | Takes focus once, when it mounts: in the commit that attaches it, as in React DOM. Windows and Linux focus it on the next frame |
| `disabled` (`<input>`, `<textarea>`) | Not focusable. Focus leaves it when it becomes disabled |
| `inert` | Nothing in the subtree is focusable; focus leaves it when an ancestor becomes inert. See below |

### `inert`

`<div inert>` takes its whole subtree out of interaction, like the DOM attribute:

- nothing inside is focusable (no Tab stop, `focus()` does nothing), and focus
  leaves a descendant when an ancestor becomes inert;
- the subtree has no accessibility nodes and its text cannot be selected;
- it takes no pointer input: no click, hover, mouse or wheel listener fires, and
  clicks reach whatever is behind it;
- `<input>` and `<textarea>` inside behave as `disabled`.

A descendant cannot opt back in with `tabIndex`, `pointerEvents: "auto"` or
`userSelect: "text"`. An `overflow: "scroll"` container inside still scrolls
under the wheel.

### Focus events

A focus change dispatches in DOM order: `blur` then `focusout` on the element
losing focus, then `focus` then `focusin` on the element gaining it. `focus` and
`blur` do not bubble. `focusin` and `focusout` bubble to ancestors that listen.
Every event carries `focusVisible`, whether the element matches
`:focus-visible` (see [Focus styles](#focus-styles)), and `relatedTarget`: the
element gaining focus on `blur` and `focusout`, the one losing it on `focus` and
`focusin`, `null` when focus came from or went to nothing. Both `target` and
`relatedTarget` are elements, the objects a `ref` gives, so
`event.relatedTarget === menuRef.current` works. Removing a focused element
from the tree fires nothing, like the DOM.

The props follow each framework's web behaviour:

| | `focus` | `blur` | `focusin` | `focusout` |
|---|---|---|---|---|
| Solid | `onFocus` | `onBlur` | `onFocusIn` | `onFocusOut` |
| React | none | none | `onFocus` | `onBlur` |

React DOM listens to `focusin` and `focusout` for `onFocus` and `onBlur`, so in
`@gpuix/react` those props bubble and there is no `onFocusIn` or `onFocusOut`.
`@gpuix/solid` is plain DOM.

### Keyboard events

Each keystroke is **one** `keydown`, and one `keyup` on release, like the DOM.
It targets the focused element, runs capture, target and bubble through its
ancestors, then reaches `document` and `window`. With nothing focused it goes to
`window` alone and `event.target` is `null` (GPUIX has no `body` or `html`
element to stand in).

Handlers run synchronously, before the editor acts on the key, so
`preventDefault()` has its DOM meaning:

```tsx
<input
  onKeyDown={(event) => {
    console.log(event.key, event.code, event.shiftKey, event.repeat)
    if (event.key === 'Enter') event.preventDefault() // no submit
  }}
/>
```

Key events are DOM-shaped. `event.key` holds `KeyboardEvent.key` values:
`"Enter"`, `"Escape"`, `"Tab"`, `"Backspace"`, `"Delete"`, `"ArrowUp"`...,
`"Home"`, `"End"`, `"PageUp"`, `"PageDown"`, `" "` for space, `"F1"`...`"F35"`,
`"Shift"`/`"Control"`/`"Alt"`/`"Meta"`, and the character produced for
printable keys, with shift case (`"A"`, `"!"`). Anything else is
`"Unidentified"`. `event.code` is the physical key (`"KeyA"`, `"Digit1"`,
`"ArrowLeft"`, `"NumpadEnter"`). On macOS both come from the keyboard as in
Chrome: `code` from the key's position, whatever the layout, and `key` from the
character the active layout types there with shift and option applied (control
and command are ignored). On a Cyrillic layout Ctrl+C reports `key` `"с"` and
`code` `"KeyC"`, on AZERTY the key that types `a` reports `"KeyQ"`, and
Option+s reports `"ß"`. Elsewhere `code` is derived from GPUI's key name with
a US QWERTY layout, so on other layouts a printable key's `code` follows the US
position of the character GPUI reports, and it is `""` when nothing maps.
`repeat` is true for
auto-repeat, `isComposing` is true while an input method composition is open
(see [Input method composition](#input-method-composition)), and `shiftKey`,
`ctrlKey`, `altKey` and `metaKey` mirror `modifiers`. The `simulateKeystrokes`
and `press()` test helpers take these same names (`"Enter"`,
`"Shift+Tab"`), see [Simulating keys](#simulating-keys).

The legacy `event.keyCode` and `event.which` are on `keydown` and `keyup`, equal
to each other, with the numbers Chrome on macOS reports: `65`-`90` for letters
(`"a"` and `"A"` are both `65`), `48`-`57` for the digit row, `13` for Enter and
NumpadEnter, `8` Backspace, `9` Tab, `27` Escape, `32` Space, `37`-`40` the
arrows, `33`/`34` PageUp/PageDown, `35`/`36` End/Home, `45` Insert, `46`
Delete, `112`-`123` F1-F12 (`124`+ for F13 and up), `16` Shift, `17` Control,
`18` Alt, `91` left Command and `93` right Command, `20` CapsLock, `96`-`105`
Numpad0-9, `106` `107` `109` `110` `111` for the keypad's `*` `+` `-` `.` `/`,
and for punctuation `186` `;`, `187` `=`, `188` `,`, `189` `-`, `190` `.`,
`191` `/`, `192` `` ` ``, `219` `[`, `220` `\`, `221` `]`, `222` `'`. A letter or
punctuation key takes its number from the character the layout types on it
(Dvorak's `o` in the S position is `79`), and the digit row and keypad from the
physical key; a layout with no Latin letters (Cyrillic) falls back to the
US-layout number of the physical key. A key an input method took (`key`
`"Process"`) is `229`, and a key Chrome gives no number (Fn) is `0`. They are
legacy: prefer `key` and `code`. There is no `keypress` event, so there is no
`charCode`.

The `simulateKeystrokes` and `press()` test helpers take these same names
(`"Enter"`, `"Shift+Tab"`), see [Simulating keys](#simulating-keys).

An element still needs a `tabIndex` to be focused, so a bare `div` with
`onKeyDown` only hears keys that bubble up from a focused descendant.

| Call | Effect |
|---|---|
| `event.preventDefault()` | Cancels the key's default: the editor action (caret move, delete, Enter), the text it would insert, Tab's focus move, Escape closing a layer |
| `event.stopPropagation()` | Skips the rest of the path, `document` and `window`. The default still runs |
| `event.stopImmediatePropagation()` | Also skips the remaining handlers on the current target |
| `event.defaultPrevented` | True after any earlier handler of this keystroke prevented it |

How far `preventDefault()` reaches depends on the platform:

| | Cancels editor actions and typed text |
|---|---|
| macOS, test renderer | Yes. Handlers run inside the native key dispatch |
| Windows, Linux | No. Events reach JavaScript through a queue after native acted on the key. `preventDefault()` still cancels the Tab and Escape defaults |

On macOS, a key sent to an active input method reaches it before GPUI, so
`preventDefault()` cannot cancel composed text.

### Input method composition

On macOS an input method (Japanese, Chinese, Korean, dead keys) composes text in
an `<input>` or `<textarea>`, and the events follow Chrome's:

- A key the input method takes is a `keydown` with `key` `"Process"` (its
  `code` is the physical key, `keyChar` is absent), sent before the input
  method sees it. `isComposing` is false for the key that starts a composition
  and true for the keys after it. `preventDefault()` has no effect on it: the
  input method gets the key anyway. A key the input method hands back (an arrow
  with no candidate window) follows as an ordinary `keydown`, with
  `isComposing` true while text is still marked.
- `compositionstart` (`data` is the text the composition takes over, usually
  `""`), then `compositionupdate` (`data` is the composition's current text) for
  every change, then `compositionend` (`data` is the final text, `""` when the
  composition was cancelled) on commit. `compositionstart` and `compositionupdate`
  come before the `change` of their edit, `compositionend` after the `change` of
  the final one, as in Chrome. They are aimed at the input, bubble, and cannot be
  cancelled. `onCompositionStart`, `onCompositionUpdate` and `onCompositionEnd`
  have `…Capture` twins.
- A `keyup` while text is marked has `isComposing` true. It keeps its real
  `key`.

Windows and Linux report `isComposing` false and fire no composition events.
A value an app sets while a composition is open ends it without a
`compositionend`. The `change` of a composition's edits has `inputType`
`insertCompositionText` and `isComposing` true, see
[The change event](#the-change-event).
In the test renderer, [`simulateComposition`](#simulating-keys) runs the
sequence.

### The change event

`onChange` fires once per edit of the text of an `<input>` or `<textarea>`, which is
what the DOM calls `input` (there is no `onInput`, and no `beforeinput`). Besides
`value` it carries the fields of a DOM `InputEvent`, filled as Chrome fills them:

| Edit | `inputType` | `data` |
|---|---|---|
| A typed character, or text from an input method that is not composing | `insertText` | the text |
| Paste | `insertFromPaste` | the pasted text |
| Enter in a `<textarea>` (an `<input>` fires nothing) | `insertLineBreak` | `null` |
| Each change of a composition, and its commit | `insertCompositionText` | the composition's text |
| A composition cancelled | `deleteCompositionText` | `null` |
| Backspace, Delete | `deleteContentBackward`, `deleteContentForward` | `null` |
| Alt+Backspace, Alt+Delete | `deleteWordBackward`, `deleteWordForward` | `null` |
| Cmd+Backspace, Cmd+Delete | `deleteSoftLineBackward`, `deleteSoftLineForward` | `null` |
| Cut | `deleteByCut` | `null` |
| Undo, redo | `historyUndo`, `historyRedo` | `null` |
| The number stepper (ArrowUp, ArrowDown) | `""` | `null` |

Typing over a selection is `insertText` and deleting a selection is the delete type
of its key. `data` is the text that fit under `maxLength`. `isComposing` is true for
every edit of a composition, the commit included: it comes before `compositionend`.
Setting `value` from code fires nothing. `data` is `null` rather than absent when
the edit inserted no text. The handler's event is typed `HostInputEvent`.

### `window` and `document`

Listen above the element tree with the `window` and `document` objects the
adapters export. They are `EventTarget`-shaped and are not installed as
globals, so libraries do not mistake the app for a browser:

```tsx
import { document, window } from '@gpuix/react' // or '@gpuix/solid'

function onKey(event) {
  if (event.key === 'k' && event.modifiers?.cmd) openPalette()
}
window.addEventListener('keydown', onKey, { capture: true })
window.removeEventListener('keydown', onKey, { capture: true })
```

Types are the DOM's lowercase names: `keydown`, `keyup`, `focusin`, `focusout`,
and `focus` and `blur` (which do not bubble, so only capture listeners hear
them). One event runs window capture, document capture, the element path,
document bubble, window bubble. The `onKeyDown` and `onKeyUp` options of
`render()` are window bubble listeners that also receive the renderer.

A `window` or `document` listener hears every event that bubbles to the top of
the tree (pointer, mouse, click, wheel), whether or not any element on the path
listens. `window.addEventListener('pointermove', fn)` fires while the pointer is
over bare background, where the event's `target` is the root element. A
`pointermove` or `mousemove` inside the window but outside the root element's box
has no element under it: it is aimed at the window, like a key with nothing
focused, so `event.target` is `null` and only the `window` and `document`
listeners run (no element handler, the root's included). Nothing fires while the
pointer is outside the window or over an `appRegion` drag area. Native produces
only the types that have a listener on `window`, `document` or an element on the
path, so adding the first listener of a type is what turns it on.

`document.activeElement` is the focused element, the same object a `ref` gives
(a React instance, a Solid `HostElement`), and `null` when nothing is focused.
It is current inside a `mousedown` handler, where it is still the element the
press is about to take focus from. With several windows (one root each), it is
the focused element of the window that heard a focus event last.

### Tab and Escape are defaults

**Tab** and **Shift+Tab** move focus through the tab order, like a browser, and
scroll the newly focused element into view: every scrolling ancestor moves to the
nearest edge, and only if the element is not already in view. The
default runs after every handler of that keystroke, so any handler can cancel
it:

```tsx
<div
  tabIndex={0}
  onKeyDown={(event) => {
    if (event.key !== 'Tab') return
    event.preventDefault() // this Tab stays here
    insertIndent()
  }}
/>
```

Tab never types a tab character into `<input>` or `<textarea>`, also like a
browser. An editor that wants one calls `preventDefault()` and inserts it. Turn
the default off for the whole window with `render(<App />, { tabNavigation: false })`.

Escape closes the top layer, see
[Escape closes the top layer](#escape-closes-the-top-layer). Neither default
runs for a key held with Ctrl, Alt or Cmd.

### A press moves focus

A mouse press on something that is not focusable blurs the focused element, as
in a browser. `preventDefault()` in an `onMouseDown` keeps focus where it is,
which is how a Combobox item can be pressed while its input stays focused.
`onPointerDown` does not keep focus, as in browsers. On Windows and Linux the
press reaches JavaScript through a queue after native has already moved focus,
so `preventDefault()` in `onMouseDown` has no effect there.

### A cancelled `pointerdown` hides the press's mouse events

`preventDefault()` in an `onPointerDown` keeps the mouse events of that press from
firing, as in browsers (Pointer Events, "prevent mouse events"): no `mousedown`, no
`mousemove` while the button is held, and no `mouseup`, until the button is released.
`pointermove`, `pointerup`, `click` and the others still fire, and so do `contextmenu`
and `onMouseDownOutside`. The focus move of the press is not cancelled (only
`onMouseDown` does that), and `mouseover`, `mouseout`, `mouseenter` and `mouseleave`
are never held back. The rule belongs to the press, not to a button: a second button
pressed while the first is down joins it, and the next press starts afresh. A
`mousemove` with no button held is unaffected. It works on macOS and in the test
renderer; on Windows and Linux the press reaches JavaScript through a queue, so the
mouse events are not held back.

### Calling the renderer from a handler

Key and focus handlers run in the middle of a native call, and may still use the
renderer: `focusElement`, `scrollTo`, `getScrollOffset`, the clipboard,
`promptForPaths`, `getWindowSize`, `getA11yTree` and the `setWindow*` calls all
run against the window being dispatched. A handler that throws is reported like
any other event error and the next handler still runs.

`mousedown`, `pointerdown`, `mouseup`, `pointerup`, `click` and `wheel` handlers
run the same way on macOS and in the test renderer. The input does not hold the
root view, so a handler can read anything (`getFocusedElementId`, `matches()`,
`getBoundingClientRect()` after a change made earlier in the same handler,
scroll metrics, motion values) and gets the current value, and a write
(`focusElement`, selection, motion-value writes, `setImage`, `setWindowTitle`)
applies at once. A press moves focus after its `mousedown` handlers have run, so
focus reads in them still name the element that held it before the press. Only
the calls that start another input, or drive the test app itself
(`nativeSimulate*`, `advanceTime`, `captureScreenshot`), cannot be made from a
handler.

### Imperative focus

`focus()`, `blur()`, `focusNext()`, `focusPrevious()` and the `within` variants fire
their `blur`, `focusout`, `focus` and `focusin` events before they return, on
macOS and in the test renderer, so a handler that moves focus hears them before
its next line (a focus handler may move focus again). The focus a mouse press, an
`autoFocus` mount and a frame cause is delivered the same way, and
`document.activeElement` already names the new element: after the `mousedown`
handlers and their `preventDefault()`, before the press returns; inside the commit
that mounts the element; before the frame that noticed it returns. On Windows and
Linux all of them are queued.

A `mousedown` handler that mounts an `autoFocus` element (a popup opened on
press) therefore loses its focus to the pressed element, as on the web: the press
takes focus after the handlers. Call `preventDefault()` on the `mousedown` to keep
it where the popup put it.

`focusNext()` and `focusPrevious()` move along GPUI's tab order and scroll the new
focus into view like Tab does: every scrolling ancestor moves to the nearest edge,
and only if the element is not already in view.
`focusNextWithin(id)` / `focusPreviousWithin(id)` wrap inside that subtree and do
not scroll. `getFocusedElementId()` returns the host id, or `null`. Browser focus requests made
while WebGPU opens are queued and applied after the first focus handles exist. If
several requests arrive before that render, the latest request wins.

Element refs have the DOM focus methods (React instances and Solid
`HostElement`):

```tsx
const buttonRef = useRef(null)

buttonRef.current.focus()                       // scrolls it to the nearest edge, if it is out of view
buttonRef.current.focus({ preventScroll: true })
buttonRef.current.blur()                        // no-op unless it is focused
buttonRef.current.matches(':focus-visible')

<div ref={buttonRef} tabIndex={-1}>Focused on demand</div>
```

`matches()` supports `:focus`, `:focus-visible`, `:focus-within`, `:hover` and
`:active`; any other selector throws. Removing `tabIndex` removes the element from the focus order and
blurs it if it was focused.

Programmatic focus is the one place `:focus-visible` is inherited: `focus()`
shows the ring when the element focus leaves was showing one. Before any key or
mouse press it shows the ring too, so a script-focused button on a fresh window
has one, as in Chrome. After a mouse press, `focus()` from a mouse-focused
element shows none.

## Headless controls

The built-in controls are **unstyled primitives**, not a fixed component
library. Use them like Radix primitives in shadcn: import a primitive namespace,
wrap and style it in a local file, then import those local components throughout
the app.

```text
@gpuix/react/select ► components/ui/select.tsx ► application screens
  native behavior       local styles/variants       product-specific use
```

Each primitive has a dedicated namespace entry point:

| Import | Main parts |
|---|---|
| `@gpuix/react/button` | `Button`, `buttonProps` |
| `@gpuix/react/select` | `Root`, `Trigger`, `Value`, `Content`, `Item` |
| `@gpuix/react/combobox` | `Root`, `Input`, `Content`, `List`, `Item`, `Empty` |
| `@gpuix/react/tooltip` | `Provider`, `Root`, `Trigger`, `Content` |
| `@gpuix/react/dialog` | `Root`, `Trigger`, `Portal`, `Backdrop`, `Popup`, `Title`, `Description`, `Close` |
| `@gpuix/react/floating` | `FloatingLayer`, `renderSlot` |

### Build a local Select

Create `components/ui/select.tsx`. This file is application code, so it can be
copied and changed without waiting for GPUIX to add a theme option:

```tsx
import * as React from 'react'
import * as SelectPrimitive from '@gpuix/react/select'

export const Select = SelectPrimitive.Root
export const SelectValue = SelectPrimitive.Value
export const SelectGroup = SelectPrimitive.Group

export const SelectTrigger = React.forwardRef<
  React.ElementRef<typeof SelectPrimitive.Trigger>,
  SelectPrimitive.SelectTriggerProps
>(({ style, ...props }, ref) => (
  <SelectPrimitive.Trigger
    ref={ref}
    {...props}
    style={(state) => ({
      width: 220,
      height: 36,
      padding: 8,
      backgroundColor: state.open ? '#334155' : '#1e293b',
      borderRadius: 8,
      ...(typeof style === 'function' ? style(state) : style),
    })}
  />
))

export const SelectContent = React.forwardRef<
  React.ElementRef<typeof SelectPrimitive.Content>,
  SelectPrimitive.SelectContentProps
>(({ style, ...props }, ref) => (
  <SelectPrimitive.Content
    ref={ref}
    sideOffset={6}
    {...props}
    style={{
      width: 220,
      maxHeight: 240,
      overflowY: 'scroll',
      padding: 4,
      backgroundColor: '#0f172a',
      borderRadius: 8,
      ...style,
    }}
  />
))

export const SelectItem = React.forwardRef<
  React.ElementRef<typeof SelectPrimitive.Item>,
  SelectPrimitive.SelectItemProps
>(({ style, ...props }, ref) => (
  <SelectPrimitive.Item
    ref={ref}
    {...props}
    style={(state) => ({
      padding: 8,
      opacity: state.disabled ? 0.4 : 1,
      backgroundColor: state.highlighted
        ? '#334155'
        : state.selected
          ? '#1e3a5f'
          : '#0f172a',
      ...(typeof style === 'function' ? style(state) : style),
    })}
  />
))
```

Pass **`items`** on `Root` when `SelectValue` should show a label while the
menu is closed. Keyboard nav reads the mounted `SelectItem` children. A styled
wrapper around `Item` is fine. Without `items`, `SelectValue` shows the raw
value.

```tsx
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from './components/ui/select'

const models = [
  { value: 'sonnet', label: 'Sonnet' },
  { value: 'opus', label: 'Opus' },
]

<Select items={models} value={model} onValueChange={setModel}>
  <SelectTrigger>
    <SelectValue placeholder="Select a model" />
  </SelectTrigger>
  <SelectContent>
    <SelectGroup>
      {models.map((item) => (
        <SelectItem key={item.value} value={item.value}>
          {item.label}
        </SelectItem>
      ))}
    </SelectGroup>
  </SelectContent>
</Select>
```

The trigger is a tab stop, also with `asChild`, unless the part or its child
sets its own `tabIndex`. Opening the Select focuses its content. `Up`, `Down`,
`Ctrl+P`, `Ctrl+N`, `Enter`, and `Escape` control the menu. Escape goes through
the [layer stack](#escape-closes-the-top-layer). The popup is modal
like Base UI: Tab does not leave it. Closing with the keyboard or a selection
restores focus to the trigger. A press outside closes it and leaves focus where
the press put it. Disabled items are skipped.

GPUI does not bubble clicks. Use `asChild` when a styled row paints the item
fill, so that row becomes the real hit target:

```tsx
<SelectItem value="opus" asChild>
  <MenuRow>Claude Opus 4.6</MenuRow>
</SelectItem>
```

The child must forward its ref and host props. `ComboboxItem` supports the same
pattern.

### Style Combobox and Tooltip the same way

Start their local files from namespace imports too:

```tsx
// components/ui/combobox.tsx
import * as ComboboxPrimitive from '@gpuix/react/combobox'

// components/ui/tooltip.tsx
import * as TooltipPrimitive from '@gpuix/react/tooltip'
```

The application still uses compound components, not one large configuration
object:

```tsx
<ComboboxPrimitive.Root items={['Next.js', 'SvelteKit', 'Astro']}>
  <ComboboxPrimitive.Input style={{ width: 220, height: 36, padding: 8 }} />
  <ComboboxPrimitive.Content style={{ width: 220 }}>
    <ComboboxPrimitive.Empty>No frameworks found.</ComboboxPrimitive.Empty>
    <ComboboxPrimitive.List>
      {(item) => (
        <ComboboxPrimitive.Item key={item} value={item}>
          {item}
        </ComboboxPrimitive.Item>
      )}
    </ComboboxPrimitive.List>
  </ComboboxPrimitive.Content>
</ComboboxPrimitive.Root>
```

```tsx
<TooltipPrimitive.Provider delayDuration={350}>
  <TooltipPrimitive.Root>
    <TooltipPrimitive.Trigger asChild>
      <div tabIndex={0} style={{ padding: 8 }}>Copy</div>
    </TooltipPrimitive.Trigger>
    <TooltipPrimitive.Content side="top" sideOffset={6}>
      Copy message
    </TooltipPrimitive.Content>
  </TooltipPrimitive.Root>
</TooltipPrimitive.Provider>
```

Combobox uses the native input for text editing, IME, clipboard, and focus.
Focus leaving the input closes the popup, so a Tab that moves focus closes it and
a prevented Tab keeps it open. Combobox and Tooltip triggers
are tab stops like the Select trigger. Tooltip `asChild` preserves the child ref
and merges trigger behavior into that host element. The child's own handlers
run first. All floating content uses GPUI's deferred `anchored()` layer,
snaps inside the window, and sits above the controls behind it, so those controls
are not hit where the content is.

### Overlay menus

Menus, tooltips, and dialogs must use **`SelectContent`**, **`ComboboxContent`**,
or `<anchored deferred>`. Those paint in a later pass, on top of
`<virtual-list>` and the rest of the page.

A `position: "absolute"` card that overflows out of the composer sits **under**
the virtual list. The list paints after the composer, so you still see the
markdown through the menu, and clicks hit the text behind it.

```tsx
<Select items={[{ value: 'flash', label: 'DeepSeek V4 Flash' }]} value={model} onValueChange={setModel}>
  <div style={{ position: 'relative' }}>
    <SelectTrigger>
      <SelectValue />
    </SelectTrigger>
    <SelectContent side="top" sideOffset={4} style={{ backgroundColor: '#232323' }}>
      <SelectItem value="flash">DeepSeek V4 Flash</SelectItem>
    </SelectContent>
  </div>
</Select>
```

Give every overlay an **opaque** fill (`#232323`, not `#23232399`).
`FloatingLayer` defaults to `#1A1A1A`. Item rows should use the same solid
color, or a solid hover color. A `#00000000` child on a blurred window punches
through Metal to the desktop.

A raw `<anchored>` with no fill in its style paints `#1A1A1A`. Set
`backgroundColor: "transparent"` to paint nothing, as `Dialog.Portal` does.

`<anchored>` placement follows floating-ui. With `side`, `align`, `gap` and
`offset` it opens on `side` of its parent (the trigger). When that side runs
out of room in the window it opens on the opposite side with the same `gap`,
or on the side with more room if neither fits (`fit="flip"`, the default;
`fit="snap"` never flips), then slides along the other axis to stay inside the
window, `snapMargin` away from its edges. `align="center"` centers on the
trigger. `onPlacement` fires with `{ side, align, availableWidth,
availableHeight, anchorWidth, anchorHeight }` (whole pixels, the last two being
the trigger's border box) when any of them changes. Placement is recomputed on
every layout, so the side changes live as the trigger scrolls, the window
resizes or the layer's content changes size. A direct child of `<anchored>` can
size or offset itself from the trigger with `width`, `minWidth`, `maxWidth`,
`height`, `minHeight`, `maxHeight`, any `margin*` or `top` / `right` / `bottom`
/ `left` set to `"anchor-size(width)"` or `"anchor-size(height)"`, resolved
natively (a trigger that has already painted is measured from that frame, so
the child is right in its first frame; under a trigger that has not, the child
is transparent for the one frame before the trigger is first measured). And
`constrainToAvailable` caps the layer's `maxWidth` / `maxHeight` to that room
natively on every layout, with no JS round trip. An explicit `anchor` corner
keeps the older corner-based placement.

An `<anchored>` lays its children out as a box with the `display` its style gives
it (`flex`, `grid`, `block`, with `flexDirection`, `gap`, `gridTemplateColumns` and
the rest), and its children are the items of that box: a `zIndex` on a child of
a `flex` or `grid` one stacks as it does on any item, without a `position`. With no
`display` it is a flex column; with `display: "flex"` and no `flexDirection` it is a
row, as in CSS.

`fill="window"` makes an `<anchored>` cover the whole window, like
`Dialog.Portal`. Native reads the viewport size every frame, so the layer
follows a resize in the same frame. It ignores `position`, `side`, `align`,
`anchor`, `offset`, and `fit`.

```tsx
<anchored fill="window" style={{ backgroundColor: 'transparent' }}>
  <div style={{ position: 'absolute', top: 0, right: 0, bottom: 0, left: 0 }} />
</anchored>
```

`FloatingLayer` copies uniform and per-corner border radii to its anchored
surface, so rounded Select, Combobox, and Tooltip content does not show square
corners behind it. It also puts `visibility` and `opacity` on that outer surface
so the fallback fill follows them without multiplying nested opacity.
`pointerEvents: "none"` makes the anchored layer transparent to the pointer.
Backgrounds, borders,
shadows, overflow, and layout remain on the inner content to avoid double paint
or changed popup geometry.

### Which element gets a pointer event

GPUIX hit-tests like the DOM. The pointer's **target** is the topmost element
under it, whatever that element paints or has a listener for, and the event
then runs along the target's ancestors in the tree you wrote: capture from the
root down, the target, bubble back up. A `div` with no fill, a transparent
overlay and an absolutely positioned card are all targets where they are, and
none of them hides its ancestors, which still get the event as bubbling and
are still hovered. A card positioned outside its parent's box is still in the
parent's subtree, so the parent's `onMouseEnter` fires when the pointer moves
onto the card.

An `<anchored>` layer paints in a later pass, over the page, but its events
bubble to the element it is declared under, as a React portal's do. The layer
is a target where it paints, so it is hit before what it covers.

Set **`pointerEvents: "none"`** to make an element never the target. The pointer
reaches whatever is under it. It **inherits**: a descendant of a `none` element
is also not a target until one sets **`pointerEvents: "auto"`**, which is how a
clickable control sits inside a `none` layer. An element with `none` stays in
the path of an event whose target is one of its descendants, so its own
listeners still hear it.

Text is hit by its glyphs. A paragraph is one painted string, so the topmost
element over it is the `<text>` or `<div>` that holds it, and the inline `<span>`
under a glyph is the target instead (see [Events on inline boxes](#events-on-inline-boxes)).

A filled child of a click target (switch thumb, radio dot, check icon) needs no
special care: a click on it bubbles to the parent's `onClick` like any child's.
Give it `pointerEvents: "none"` only if you want the pointer to treat it as part
of the parent.

A box's own rounded corners are excluded from hit testing, as in a browser: a
pointer over the rounded-away corner of a `borderRadius` box does not hit it
and reaches what is underneath, whatever its `overflow`. The cut is the box's
own: a child that covers the corner is still hit through its own box.
`overflow` other than `visible` also cuts the box's content at the radius, and
`clipPath` clips the box and everything inside it.

> [!IMPORTANT]
> This changes three rules from earlier versions. A filled or absolutely
> positioned `div` no longer blocks what is behind it from hover: it is only the
> target where it paints. `pointerEvents: "auto"` no longer stops the wheel. And
> `<anchored>` has no `occlude` prop: set `pointerEvents: "none"` to let the
> pointer through. The wheel itself is unchanged.

### Portals

`createPortal(children, target)` mounts `children` as the children of an element
that is already mounted, wherever the calling component sits in the tree. Use it
for a toast layer, a drag preview, or any content that belongs to another part of
the window. Context still flows from the caller, as in React DOM.

Take `target` from a `ref` and render the portal once it is set. A portal into an
element that has not mounted has nowhere to attach.

```tsx
import { createPortal, type PublicInstance } from '@gpuix/react'

function Screen() {
  const [layer, setLayer] = useState<PublicInstance | null>(null)
  return (
    <>
      <div ref={setLayer} style={{ position: 'absolute', top: 0, right: 0, bottom: 0, left: 0 }} />
      <Sidebar>{layer && createPortal(<Toast />, layer)}</Sidebar>
    </>
  )
}
```

The portal's children are appended after whatever the target already holds, and
React keeps them in key order among themselves. `target` must belong to the same
root as the component that calls `createPortal`.

For floating menus that must paint above a `<virtual-list>`, use
[`<anchored>`](#overlay-menus) rather than a portal: a portal changes where
elements live in the tree, not the order gpui paints them in.

### Measure an element

`element.getBoundingClientRect()` returns the element's border box in window
coordinates, like the DOM: `{ x, y, width, height, top, left, right, bottom }`.
It is never stale. Mutations your framework has queued are sent first (Solid
sends in a microtask, React at commit) and the window is laid out synchronously
if anything changed, without painting, as a browser forces layout. The first
read after a mount, or a read right after a style change in the same turn,
answers for the tree as it is, and the next frame draws once however many reads
came before it. A layout a read forces does not advance time: it samples
animations, transitions and motion values at the instant of the last draw, as a
browser's animation clock stands still between frames, so a handler that reads
layout from a `change` event cannot make the animation it listens to run ahead.
A scroll offset write does not make a read of `scrollTop` or `scrollHeight` lay
out (a read of bounds does, the children moved). An
element with no box (not mounted, not painted) returns a zero
rect, as in the DOM.

```tsx
const ref = useRef<PublicInstance>(null)
useLayoutEffect(() => {
  const { width } = ref.current!.getBoundingClientRect()
}, [])
```

Two reads in one turn see the same frame: a window with nothing pending does not
redraw, so the offset between a parent and a child stays constant under a moving
ancestor.

A read right after a write that cannot move a box lays nothing out, as in Chrome. A
motion value that drives only `opacity`, `boxShadow` or a colour (`color`,
`backgroundColor`, a border, outline, text decoration, scrollbar or selection colour) is
such a write, so reading a rect on every frame of a fade costs a read and not a layout. A
value that drives a size or an inset, or that a transform or a follower computes one from,
and every state commit, hover, scroll or resize, make the next read lay the window out.

The rect is the layout, as in Chrome, and not the box that is painted: `x`, `y`, `width`
and `height` are fractions of a pixel, cut down to a multiple of 1/64 of a device pixel (1/128 px at
2x) as a browser's `LayoutUnit` is (a `width: "33.3333%"` of 100 px is 33.328125, a `width: 100.4` is
100.390625, and a box after a text starts where the text ends, say 69.3125). Painting, hit testing and the pointer's
`clientX`, `offsetX` and `offsetY` use the box snapped to device pixels and are
unchanged. `ResizeObserver` sizes, `IntersectionObserver` rects, `onLayout`, the
bounds of a `<span>` and `getElementsAt` read the same boxes. The integer members are
rounded as Chrome rounds them: `clientWidth`, `clientHeight`, `scrollWidth` and
`scrollHeight` (see [Scrolling](#scrolling)) are the padding box and the scrollable overflow
rounded half up, `clientLeft` and `clientTop` of `getElementBounds` are the border
widths, which are whole device pixels, rounded half up,
and `scrollTop` and `scrollLeft` are as they were. Deviations: a few cases of a
percentage, a flex share or a text are still a unit off Chrome's (listed under the deviations of
[inline-block, inline-flex, inline-grid and inline-table](#inline-block-inline-flex-inline-grid-and-inline-table));
the scroll offset an element is placed
at is the one it is painted at, which is a whole device pixel, so a box inside a scroller
that is scrolled by a fraction is where it is painted and not where a browser puts it; and
a box that is not a scroller reports its padding box as `clientWidth` and the same number as
`scrollWidth`, so content that overflows it is not counted.

`renderer.getElementBounds(id)` is the raw read on the live `GpuixRenderer` and
on the test renderer. It does the same flush and layout, and returns `null`
instead of a zero rect when the element has no box. `<input>` reports its border
box like every other element. The browser (wasm) build still answers from the
last painted frame. An element inside a `contentVisibility` subtree that is skipped
gets its true box too (see [Skipping content](#skipping-content-with-contentvisibility)).

### Observe size and visibility

`ResizeObserver` and `IntersectionObserver` follow the DOM. Export them from
`@gpuix/react` or `@gpuix/solid` and pass a host element (a React `ref`, a Solid
`ref`) where the DOM takes an `Element`. They are not installed as globals.

```tsx
const observer = new ResizeObserver((entries) => {
  for (const entry of entries) setWidth(entry.contentRect.width)
})
useEffect(() => {
  observer.observe(ref.current!, { box: "border-box" })
  return () => observer.disconnect()
}, [])
```

`ResizeObserver` calls back with `entries` (`target`, `contentRect`,
`borderBoxSize`, `contentBoxSize`) and the observer. The sizes are fractions of a
pixel, as layout has them (see [Measure an element](#measure-an-element)); so are
the rects of an `IntersectionObserver` entry. The first callback follows
`observe()` for an element that is not 0 by 0, then one per change of the
observed box (`"content-box"`, the default, or `"border-box"`). The content box
is the border box minus the element's laid-out `padding` and border, so `em`, `calc()`
and a `hover`, `active` or focus style (or a transition) that changes them are exact and
report too. An element that has no box
(`display: "none"`, a virtual list row outside its window) reports 0 by 0. A
`visibility: "hidden"` element keeps its box and reports its size.
`device-pixel-content-box` throws a `TypeError`.

`IntersectionObserver` takes `{ root, rootMargin, threshold }`. `root` is a host
element or `null` for the window. Entries have `isIntersecting`,
`intersectionRatio`, `boundingClientRect`, `intersectionRect`, `rootBounds`,
`time` and `target`. Each target gets an entry after `observe()` (an element
that never paints reports not intersecting) and then one per threshold crossing
or `isIntersecting` flip. As in Chrome, an intersection under the first threshold
reports `isIntersecting: false` (with its `intersectionRatio`), so `threshold: 0.5`
means half of the target shows. The intersection is the target's border box clipped by
every ancestor's `overflow` between it and the root, then by the root. For an
`absolute` target those are the ancestors from its containing block up, and a
`fixed` one is clipped by none. The `overflow` and `position` are the ones each
ancestor is painted with, so a `hover` style that makes an ancestor `relative`
with `overflow: hidden` clips its absolute targets while it applies. As in the DOM, an element root and an `overflow` ancestor clip at their padding box: a
target that lies entirely under their border does not intersect, and `rootBounds`
is the root's padding box.
`rootMargin` takes 1 to 4 lengths in `px` or `%` (a percentage is of the padding
box), and with no `root` it can reach past the window edge, which does not clip
the target. The test is geometry only, so
a `visibility: "hidden"` target still intersects.

Callbacks run once per batch of a frame's reports, after the frame, so state set
in one paints a frame later. A frame's reports arrive in the order of the DOM's
rendering steps: `onScroll` events first, then `onLayout` handlers and the
`ResizeObserver` callbacks (`onLayout` is `ResizeObserver` for an element), then the
`IntersectionObserver` callbacks. `onScroll` and `onLayout` run in document order
(`onScroll` in paint order). Observer callbacks run in the order the observers were
created, and the entries of one observer are in the order its targets were observed
(`observe()` again moves a `ResizeObserver` target to the end), as the specs say.
Observations are made when a frame is presented, never by a read of layout:
`getBoundingClientRect`, a `scrollTop` write and the other reads lay out and deliver nothing, and the change they laid out is reported by the next frame, after that frame's `onScroll` event, as the DOM runs its scroll steps before the resize observations. `observe()` reads no layout: every observation
started since the last frame is probed together in the next step, so any number
of observers started by one commit cost one draw. Each observer gets its own
initial entry for a target, also when another observer already watches it. Observing an element that is not mounted yet is
allowed, as in the DOM: it reports once it has a box, and an `IntersectionObserver`
first reports it as not intersecting. An element that is removed behaves like one
that lost its box: `ResizeObserver` reports 0 by 0 and `IntersectionObserver`
reports `isIntersecting: false`, once, and nothing after that. The report comes with the
next frame that is presented, not with a layout read in between. Call `unobserve` or
`disconnect` to release it.

Observers work on `<div>`, `<text>`, `<input>`, `<textarea>`, `<img>`, `<svg>`,
`<anchored>`, `<code>`, `<diff>` and `<markdown>`. `<virtual-list>` itself does not
report; its rows do, and a row outside the window reports as not painting.

### Find what is under a point

`getElementsAt(x, y)` returns the ids of the elements whose last box (as laid out, not
snapped; see [Measure an element](#measure-an-element))
holds a window point, topmost first, like the DOM's `elementsFromPoint`. It
works on the live `GpuixRenderer` and on the test renderer.

```tsx
const [topmost] = renderer.getElementsAt?.(event.x, event.y) ?? []
```

It answers from the last frame, so an element that did not paint (outside a
virtual list's window) is never listed, and neither is one with
`visibility: "hidden"` (a descendant with `visibility: "visible"` is). The
styles it reads (`visibility`, `pointerEvents`, `overflow`, `position`, `zIndex`,
`opacity` and the other effects that make a stacking context, `display`) are the
ones each element was painted with, so a `hover`, `active` or focus state, a
group or `has*` state, an `@container` block, a transition, a CSS animation or a
`motion` value that changes them counts, as `document.elementsFromPoint` does.
A deferred [`<anchored>`](#overlay-menus) layer is listed too, below what it holds.
`overflow: hidden` and
`scroll` clip what lies outside them (not an `absolute` or `fixed` descendant whose
containing block is outside), `pointer-events: none` is skipped (and
inherited, until a child sets `auto`), and a deferred
[`<anchored>`](#overlay-menus) layer comes first, ordered by `priority`,
because gpui paints it after the tree and outside the clip of its ancestors.
Beyond that, the order is the paint order of [stacking contexts](#layout-like-css):
a positioned element, a flex or grid item with a `zIndex` and a translucent one
are listed by `zIndex` in the nearest context, however many static boxes sit
between, and everything else in tree order.

### Button

GPUIX has no native `<button>`, so a `div` with `onClick` is not reachable with
Tab and ignores the keyboard. `Button` is the [Base UI Button](https://base-ui.com/react/components/button):

```tsx
import { Button } from '@gpuix/react/button'

<Button onClick={save} disabled={saving} style={(state) => ({ opacity: state.disabled ? 0.5 : 1 })}>
  Save
</Button>
```

| Behavior | Detail |
|---|---|
| Tab stop | `tabIndex` 0, `role="button"` |
| `onClick` | Press, **Enter** on key down (not on repeat), **Space** on key up. A key clicks the element, so the handler gets a [click without a pointer](#click-without-a-pointer) |
| `disabled` | No `onClick`, leaves the Tab order |
| `focusableWhenDisabled` | Stays in the Tab order while disabled, for a busy "Saving…" button |
| `asChild` | Merges the behavior into your own element |
| `style` | Object, or a function of `{ disabled }` |

`buttonProps(behavior, renderer)` returns the same props for your own parts; `renderer` is
`useGpuixRequired()`, and it delivers the key's click.
`Dialog.Trigger` and `Dialog.Close` are built on it.

### Dialog

Same parts as [Base UI Dialog](https://base-ui.com/react/components/dialog):

```tsx
import * as Dialog from '@gpuix/react/dialog'

<Dialog.Root>
  <Dialog.Trigger>Settings</Dialog.Trigger>
  <Dialog.Portal>
    <Dialog.Backdrop style={{ backgroundColor: '#00000080' }} />
    <Dialog.Popup style={{ width: 420, padding: 16, backgroundColor: '#232323' }}>
      <Dialog.Title>Settings</Dialog.Title>
      <Dialog.Close>Done</Dialog.Close>
    </Dialog.Popup>
  </Dialog.Portal>
</Dialog.Root>
```

| Part | Behavior |
|---|---|
| `Root` | `open`, `defaultOpen`, `onOpenChange`, `modal` (default `true`), `disablePointerDismissal` |
| `Trigger` | Tab stop. Click, Enter, or Space opens |
| `Portal` | Full-window deferred layer (`<anchored fill="window">`). Follows a resize in the same frame. Paints over `<virtual-list>`. Centers its children by default. Modal: blocks clicks and the wheel behind it |
| `Backdrop` | A click closes the dialog. The press and the release both land on it, so a press that starts or ends in the popup does not |
| `Popup` | Moves focus in on open and out on close. Modal: Tab and Shift+Tab stay inside |
| `Close` | Tab stop. Click, Enter, or Space closes |

`initialFocus` and `finalFocus` on the Popup choose where focus goes, like
Base UI:

```tsx
<Dialog.Popup initialFocus={searchRef} finalFocus={composerRef}>
```

| Value | `initialFocus` (open) | `finalFocus` (close) |
|---|---|---|
| unset / `true` | The popup itself, so the first Tab enters it | The trigger, else the element focused when the popup opened |
| ref or element | That element | That element |
| `false` | Focus stays | Focus stays |
| function | Returns one of the above. `null` means the default | Same |

The default for `initialFocus` differs from Base UI, which picks the first
tabbable element. GPUI's tab order exists only after the popup has painted,
so GPUIX focuses the popup and lets the first Tab walk that order.

A dialog opened from app state, with no `Trigger`, still returns focus: the
Popup records the focused element before it mounts.

`initialFocus` beats an `autoFocus` inside the popup. Pass the field as
`initialFocus` instead.

Nested dialogs follow the layer stack. When both open in one update, the inner
one takes focus. When both close in one update, focus goes to the outer one's
return target.

A Select or Tooltip inside the Popup opens above it, and Escape closes it first.

### Escape closes the top layer

Every open Dialog popup, Select, Combobox, and Tooltip is on one **layer stack**
per window. Escape closes only the most recently opened layer, even when nothing
is focused. It is a default action, like Tab, so any `onKeyDown` can keep it
open:

```tsx
<Dialog.Popup
  onKeyDown={(event) => {
    if (event.key === 'Escape' && dirty) event.preventDefault()
  }}
/>
```

A custom overlay joins the same stack with `DismissableLayer`. Mount it only
while the overlay is open:

```tsx
import { DismissableLayer } from '@gpuix/react' // or '@gpuix/solid'

{open && (
  <DismissableLayer onEscapeKeyDown={() => setOpen(false)}>
    <anchored deferred>{/* overlay */}</anchored>
  </DismissableLayer>
)}
```

A layer mounted inside another is always above it, even when both open in the
same commit. `DismissableLayer` also takes `initialFocus()` and
`finalFocus(previous)`, which return an element id or `null`. The stack calls
them in stack order, so only the top layer takes focus on open.

Framework-free code uses `pushDismissLayer(renderer, layer, { previousFocus })`
with a `parent` field, and calls the returned function when the layer closes.

### Trap Tab inside a custom panel

`Dialog.Popup` already does this. For your own panel, prevent the default Tab,
then wrap inside it with `focusNextWithin` / `focusPreviousWithin`.

```tsx
function onKeyDown(event: KeyEvent) {
  if (event.key !== 'Tab' || !panel) return
  event.preventDefault()
  if (event.modifiers?.shift) renderer.focusPreviousWithin?.(panel.id)
  else renderer.focusNextWithin?.(panel.id)
}

<div ref={setPanel} onKeyDown={onKeyDown}>
  <div tabIndex={0} autoFocus>Ok</div>
  <div tabIndex={0}>Cancel</div>
</div>
```

## Text selection

Every text GPUIX paints is **selectable and copyable**, including text inside
`<code>`, `<diff>` and `<markdown>`. A drag that starts in a heading and ends
inside a fenced code block selects everything between; Cmd+C copies it joined in
document order.

There is nothing to opt into. A tap does not select. Only a drag does.
To opt *out* — toolbars, buttons, line-number gutters — set
`userSelect: "none"`, which inherits like the CSS property:

```tsx
<div style={{ userSelect: 'none' }}>
  <text>toolbar label, never selected</text>
</div>
```

![Text selected across markdown blocks](./docs/images/selection.png)

Read the selection from the renderer, or react when it changes:

```tsx
render(<App />, {
  onSelectionChange(event) {
    setCopied(event.value ?? '')
  },
})

renderer.getSelectedText()   // joined text, or null
renderer.clearSelection()
```

`onSelectionChange` is a **window-level** callback on `render()` / `createRoot()`,
the same attachment as `onKeyDown`. Text selection is app-wide, not per element.
It fires once when the selected ranges change, including a clear to empty
(`value` is then omitted). An unchanged frame does not fire.

The event is a `RootEvent`, an event with no propagation. `value` is the joined selected text.

Selection works because each painted text element registers itself into a
per-frame registry. A drag anchored in one element resolves against that
registry into per-element spans: partial in the anchor and head, whole for
everything between.

As in a browser, the range, the wash and the copied text follow **document
order**: the order of the elements in the tree, not the order they paint in.
A `position: relative` element, a `zIndex` or a flex `order` moves what is
painted on top, not where the text sits in the selection. Only the hit test
follows paint order, so a press lands on the topmost text under the pointer.

<details>
<summary>Why not one big text element, like Zed?</summary>

Zed's markdown selects continuously because its whole document is a single
element over one text model. GPUIX renders a *tree* of text elements, so it
rebuilds that continuity at paint time instead. The mechanism is ported from
[Comet](https://github.com/zeronsh/comet) (MIT), which faced the same problem.
</details>

## Text highlighting and search

The **`highlight` prop** paints a background wash behind matched text. Put it on
any element and it applies to that element's subtree, so the root searches the
window and a container searches only that container.

```tsx
<div highlight={{ query: 'fox' }}>
  <text>the quick brown fox</text>
</div>
```

It reaches `<text>`, `<code>`, `<markdown>` and `<diff>` with no extra props,
because every string GPUIX paints goes through the same funnel.

### A find bar

`useTextSearch` owns the cursor and the count. `next` and `previous` are plain
event handlers, so nothing here needs an effect.

```tsx
import { useTextSearch } from '@gpuix/react'

function Find() {
  const [query, setQuery] = useState('')
  const search = useTextSearch({ query })

  return (
    <div style={{ display: 'flex', flexDirection: 'column', flex: 1 }}>
      <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
        <input value={query} onChange={(e) => setQuery(e.value ?? '')} />
        <text>{search.total === 0 ? 'No results' : `${search.active + 1}/${search.total}`}</text>
        <div onClick={search.previous}><text>↑</text></div>
        <div onClick={search.next}><text>↓</text></div>
      </div>

      <div {...search.props} style={{ flex: 1 }}>
        <Transcript />
      </div>
    </div>
  )
}
```

### Explicit ranges

When you already have offsets, from an LSP range or your own model, pass them
instead of a query. They are `[start, end)` in **UTF-16 code units**, the units
`indexOf` and `RegExp.exec` return.

```tsx
<div highlight={{ ranges: [[6, 11]], color: '#f43f5e55' }}>
  <text>Hello {name}!</text>
</div>
```

A pair that splits a surrogate pair is **rejected**, never snapped. Ranges index
retained text only; native elements build their strings in Rust, so use `query`
for those.

### Options

| field | meaning |
|---|---|
| `query` | substring to match, case-insensitive by default |
| `caseSensitive` | exact case only |
| `wholeWord` | neither neighbour may be alphanumeric or `_` |
| `ranges` | explicit `[start, end)` UTF-16 pairs |
| `color` / `activeColor` | any CSS colour; defaults come from the theme |
| `activeIndex` | which match gets `activeColor`, for a find cursor |
| `matchIndexOffset` | matches before this subtree; only for virtualized content |
| `radius` | corner radius of the wash, default 2 |

Pass an **array** to paint several at once, for example search matches plus a
persistent mention tint. Later entries draw on top.

### Matching rules

Matches are **non-overlapping** and leftmost-first. Case-insensitive matching
uses Unicode **lowercasing**, not full case folding, so `ﬀ` does not match `ff`.
A word boundary is any code point that is not Unicode Alphabetic, a digit,
or `_`.

Matches are numbered in **document order**, as browser find numbers them, so
`activeIndex` does not move when a positioned element paints late.

A match never crosses a line, exactly like browser find. It **does** cross the
several host nodes React creates for one interpolated line, which matters more
than it sounds:

```tsx
// React makes 3 host text nodes here. `Hello Tommy` still matches.
<div highlight={{ query: 'Hello Tommy' }}>
  <text>Hello {name}!</text>
</div>
```

The nearest declaration wins, so a nested `highlight` replaces its ancestor's
for that subtree.

**`userSelect: "none"` does not opt out of search.** A browser still finds that
text, so GPUIX still highlights it. Only element chrome, a code gutter or a diff
file header, is excluded.

<details>
<summary>Searching a virtual list</summary>

`<virtual-list>` never builds off-screen rows, so native can only see the
mounted window. Two things follow, and both are the app's job because the app
owns the row data.

**Count the matches yourself** with `findRanges`, which runs the same algorithm
as the native matcher on a string you give it.

**Say where your window starts**, as a count of **matches** above it, not a row
index. Without it native numbers the mounted rows from zero, `activeIndex` means
"the nth visible match", and the find cursor lands on the wrong row.

Both numbers travel together in `matches`, because supplying one without the
other is always wrong.

```tsx
import { findRanges, useTextSearch } from '@gpuix/react'

// One entry per row, so a prefix sum gives both numbers.
const perRow = useMemo(
  () => rows.map((row) => findRanges({ text: row.text, query }).length),
  [rows, query],
)

const search = useTextSearch({
  query,
  matches: {
    total: perRow.reduce((n, count) => n + count, 0),
    indexOffset: perRow.slice(0, windowStart).reduce((n, count) => n + count, 0),
  },
})

// search.next() moves the cursor; you do the scrolling
listRef.current.scrollToItem(rowOfMatch(search.active))
```

`findRanges` matches the native algorithm for the **same** string. Call it on
the same logical lines native paints: adjacent text nodes of one parent are one
line, and `<markdown>` paints inline runs rather than its source.
</details>

<details>
<summary>Why a wash and not gpui's HighlightStyle</summary>

`HighlightStyle.background_color` is painted natively by gpui, but only with
square corners, and it cannot report the boxes it drew. GPUIX paints quads from
`range_rects`, the same helper selection and inline-code pills use, so a
soft-wrapped match is one box per visual row and `getPaintedHighlights()` can
assert the geometry without a screenshot. Zed's own editor paints search
highlights manually for the same reason.
</details>

## Native text components

Three elements render text with Syntect syntax highlighting computed in
Rust. Colours come from a theme prop, so a late-arriving highlight recolours runs
without ever changing layout.

### `<code>`

A syntax-highlighted code block. One row per line at an exact line height, so the
block's height is known before highlighting runs.

It paints **no surface of its own**: no fill, border, radius, padding or language
header. `style` is the surface, so the card look is yours.

```tsx
<code
  code={source}
  language="typescript"        // or path="src/app.ts" to detect from extension
  showLineNumbers
  style={{
    padding: 12,
    borderRadius: 10,
    borderWidth: 1,
    borderStyle: 'solid',
    borderColor: '#ffffff1f',
    backgroundColor: '#ffffff09',
  }}
/>
```

![A syntax-highlighted code block](./docs/images/code.png)

`fontFamily`, `fontSize`, `fontWeight`, `lineHeight` and `color` in `style` beat
the theme. Rows are a fixed height, so `fontSize` alone scales that height by the
theme's ratio; pass `lineHeight` to set it exactly.

The theme's `fontSans` and `fontMono` set the default families. For a custom font, load
the file with the `fonts` window option (see [Usage](#usage)).

Two things stay owned by the element: lines **never wrap**, and the block is its
own horizontal scroller. A long line pans on a horizontal wheel inside it, so
`whiteSpace` and `overflowX` in `style` do nothing.

For a language header, or any other chrome, wrap it in a `<div>` you own:

```tsx
<div style={{ display: 'flex', flexDirection: 'column', borderRadius: 10, overflow: 'hidden' }}>
  <div style={{ padding: 6, backgroundColor: '#ffffff09' }}>
    <text style={{ fontSize: 12, color: '#a3a3a3' }}>{language}</text>
  </div>
  <code code={source} language={language} style={{ padding: 12, minWidth: 0 }} />
</div>
```

`<markdown>` is different: it keeps its own fenced-block card, because a document
renderer owns its layout. Tune that card with the `mdCode*` metrics.

### `<diff>`

A unified diff viewer. It **flows** with its parent by default, so a parent
list can be the only scroller. Collapsing a file removes its rows rather than hiding
them, so a collapsed 10k-line file costs one row.

Use `maxLines` to keep a long patch short. Show more fires `onShowMore`. Clear
`maxLines` in that handler to reveal the rest.

Pass `scroll` and a **bounded height** only for a dedicated full-window viewer.
That path uses GPUI's `list()` and virtualizes. Do not nest it inside another
scroller. See [Scrolling](#scrolling).

```tsx
<diff
  patch={unifiedPatch}
  wordDiff                     // highlight only the tokens that changed
  maxLines={open ? undefined : 24}
  collapsedPaths={['pnpm-lock.yaml']}
  onShowMore={() => setOpen(true)}
  onToggleFile={(e) => toggle(e.value)}
  onLineClick={(e) => console.log(e.oldLine, e.newLine, e.value)}
/>
```

![A unified diff with word-level highlights](./docs/images/diff.png)

### `<markdown>`

GitHub-flavoured markdown: headings, lists, tables, block quotes, fenced code,
strikethrough, task lists, and autolinked bare URLs.

```tsx
<markdown source={readme} onLinkClick={(e) => open(e.value)} />
```

![Markdown with headings, lists, a table and a code fence](./docs/images/markdown.png)

### Theming

All three take the same optional `theme` prop. Every field layers on top of the
built-in dark theme, so overriding one token leaves the rest alone.

```tsx
<code
  code={source}
  language="rust"
  theme={{
    appearance: 'dark',        // or 'light'
    accent: '#7c86ff',
    syntax: { keyword: '#f38ba8', string: '#a6e3a1' },
  }}
/>
```

**Layout numbers live in the theme too**, under `metrics`. Row heights, gutter
widths, paddings and the heading scale are props, not Rust constants, so tuning
the design is a React re-render and never a native rebuild.

```tsx
<diff
  patch={patch}
  theme={{
    metrics: {
      diffLineHeight: 26,
      diffGutterWidth: 48,
      mdHeadingSizes: [24, 19, 16, 14],
    },
  }}
/>
```

When `scroll` is on, `<diff>` virtualizes from these numbers without measuring,
so changing `diffLineHeight` also re-sizes the scroll model.

The same three components, retuned entirely from `metrics` with no rebuild:

![The components with enlarged metrics](./docs/images/metrics.png)

Languages bundled: Rust, TypeScript, TSX, JavaScript, JSX, Python, Go, JSON,
Bash, TOML, YAML, Markdown, HTML, CSS, C.

## Supported Elements

| Element         | Description                                      |
|-----------------|--------------------------------------------------|
| `div`           | Container with flexbox layout                    |
| `text`          | Text content, selectable                         |
| `code`          | Syntax-highlighted code block                    |
| `diff`          | Unified diff viewer. Flows by default            |
| `markdown`      | GitHub-flavoured markdown                        |
| `input`         | Native single-line text editor                   |
| `textarea`      | Native multiline, auto-growing text editor       |
| `virtual-list`  | Long collections; only visible rows are built    |
| `img`           | Local, data URL, or http(s) raster or SVG images  |
| `svg`           | Tintable monochrome SVG icons from source or disk |
| `anchored`      | Positioned overlay                               |
| `canvas`        | Custom drawing (planned)                         |

## Images and icons

`<img>` takes a **filesystem path, `file://` URL, data URL, `blob:` URL, or
http(s) URL**. Pass a path or `file://` URL for local files, encode in-memory
bytes as base64 or wrap them in a `Blob`, or pass a remote URL and let GPUI
fetch it.

### `<img>`

`<img>` paints through GPUI's image element. It loads **PNG, JPEG, WebP, GIF,
SVG, BMP, TIFF, ICO, and Netpbm** from disk, data URLs, or http(s). SVG here is a
full-colour image, not a tintable icon.

```tsx
<img
  src={fileURLToPath(new URL('./photo.png', import.meta.url))}
  objectFit="cover"
  style={{ width: 240, height: 140, borderRadius: 12 }}
/>
```

```tsx
const src = `data:image/png;base64,${Buffer.from(pngBytes).toString('base64')}`

<img src={src} style={{ width: 240, height: 140 }} />
```

```tsx
<img
  src="https://example.com/avatar.png"
  objectFit="cover"
  style={{ width: 48, height: 48, borderRadius: 24 }}
/>
```

Set **both** `width` and `height`. GPUI fetches and decodes on a background
task. The tree does not wait. Without a definite size the box is empty until
decode, then jumps to the bitmap size.

Data URLs support every image format listed above. Base64 and percent-encoded
payloads are accepted. Base64 is read as browsers read it: white space is
ignored, padding may be left off, and the payload may itself be percent-encoded.
A `file://` URL's query and fragment are not part of the path. Remote URLs use
the same GPUI image cache as disk files. They are not written to a temp file.

`objectFit` matches CSS: `"contain"` (default), `"cover"`, `"fill"`,
`"scaleDown"`, or `"none"`. An empty `src` or a failed load shows a fallback
placeholder instead of crashing. A URL that is still loading paints an empty
box of the declared size. There is no spinner.

`onLoad` and `onError` fire once per `src` change, like the DOM events. The
`onLoad` payload carries `naturalWidth` and `naturalHeight` in pixels; `onError`
carries the reason in `value`. An empty `src` fires neither. As in a browser,
an `<img>` that `display: none` keeps out of the layout (itself or an ancestor)
still loads and fires them. Removing `src` clears the image.

```tsx
<img
  src={url}
  onLoad={(event) => setSize([event.naturalWidth, event.naturalHeight])}
  onError={(event) => console.warn('image failed:', event.value)}
/>
```

A `blob:` URL from `URL.createObjectURL(new Blob([bytes]))` works in this
process: the JS layer fetches the blob and hands the bytes to `setImage`. Revoke
the URL only after `onLoad`; a revoked URL fires `onError`. Blob URLs are not
files, so the box is empty until the fetch resolves, and it needs a runtime where
`fetch` accepts blob URLs (Bun does).

`borderRadius` clips the bitmap. GPUI paints the image with those corner
radii. A parent `overflow: "hidden"` wrapper does **not** clip an `<img>`
child. Put the radius on the image.

```tsx
<img
  src={avatarUrl}
  objectFit="cover"
  style={{ width: 32, height: 32, borderRadius: 16 }}
/>
```

### Live images from a buffer

A data URL still works, but it base64-encodes the bytes into the mutation JSON.
For a waveform, a canvas dump, or any frame you already have in memory, push
**raw bytes** through the `<img>` ref. That call skips JSON.

`setImage` takes encoded **PNG, JPEG, WebP, GIF, SVG, BMP, TIFF, ICO, or
Netpbm**. `setImagePixels` takes packed **RGBA** by default. Prefer pixels for
a live waveform. There is no PNG encode, and no JSON.

Pass `{ format: 'bgra' }` when your source already produces **BGRA**. That is
GPUI's native order, so the upload skips a full per-pixel swizzle. ffmpeg
(`-pix_fmt bgra`), VideoToolbox, and node-canvas `toBuffer('raw')` all produce
it.

```ts
img.current?.setImagePixels(width, height, rgba)
img.current?.setImagePixels(width, height, bgra, { format: 'bgra' })
```

Do not compress frames before the call. A PNG or JPEG encode costs far more
than the copy it saves.

Call either from `useLayoutEffect` after mount. A later React `src` **change**
overwrites the pixels. Alpha is straight, not premultiplied.

There is **no density argument**. `width` and `height` on `setImagePixels` are
bitmap pixels. `style.width` and `style.height` are the layout box. On a retina
display, upload **2x** (or `devicePixelRatio`) the box size so GPUI does not
stretch one logical pixel into four screen pixels.

```tsx
import { createCanvas } from 'canvas'
import { useLayoutEffect, useRef } from 'react'
import type { ImgInstance } from '@gpuix/react'

function Waveform({ samples }: { samples: Float32Array }) {
  const img = useRef<ImgInstance>(null)

  useLayoutEffect(() => {
    const width = 1600
    const height = 160
    const canvas = createCanvas(width, height)
    const ctx = canvas.getContext('2d')
    ctx.fillStyle = '#1a1a2e'
    ctx.fillRect(0, 0, width, height)
    ctx.strokeStyle = '#5ca9ff'
    ctx.lineWidth = 2
    ctx.beginPath()
    for (let x = 0; x < samples.length; x++) {
      const y = height / 2 - samples[x]! * (height / 2 - 8)
      if (x === 0) ctx.moveTo(x, y)
      else ctx.lineTo(x, y)
    }
    ctx.stroke()
    img.current?.setImagePixels(width, height, canvas.toBuffer('raw'), {
      format: 'bgra',
    })
  }, [samples])

  return <img ref={img} objectFit="fill" style={{ width: 800, height: 80 }} />
}
```

node-canvas `toBuffer('raw')` is **BGRA** on little-endian machines (every
Apple Silicon, x86, and ARM64 desktop), with no row padding. It is
**premultiplied**, and GPUIX expects straight alpha. That is identical only for
opaque pixels, which is why the example fills the background first. For a
transparent canvas, use `getImageData().data` with the default `'rgba'` format.

The [waveform example](./examples/waveform.tsx) writes BGRA by hand, at 2x.

### `<svg>`

`<svg>` rasterises the document with resvg in **full colour**, at the window's
device resolution, so fills, strokes, gradients and opacity all paint. Raw
`source` works on desktop and in the browser. Desktop apps can also use a local
`src` path. The drawing keeps its aspect ratio inside the box, centred, like
`preserveAspectRatio="xMidYMid meet"`.

The raster is made once per size, colour and document. A small SVG (up to about 4 KB of
markup, drawn at up to about 128 x 128 device pixels) is rasterised while the frame paints,
so an icon is there on its first paint. A bigger one is rasterised on a background thread,
one job at a time per `<svg>`, and never holds up a frame. Until the job lands the `<svg>`
keeps drawing its previous raster, stretched to fit the box, and draws nothing on its first
paint. The view repaints when the new raster is ready, and a size, colour or document that
changed again while a job ran is rasterised once the job is done, skipping the ones in between.
The test renderer waits for these jobs before `render()` and `flush()` return.

`currentColor` in the SVG is the element's computed `color`, inherited like in
the DOM, so `style.color` recolours an icon drawn with `fill="currentColor"` and a
`color` on an ancestor reaches it too. An SVG that sets its own `color` on the
root keeps it. The default `color` is black, white under a dark `colorScheme`, and
an element with no fill paints black, as in a browser.

For application icons, prefer **raw SVG source**. It works with both GPUIX
targets and lets a bundler embed each icon in the JavaScript bundle. Use `src`
only for a desktop app that intentionally ships loose asset files.

`src` is a filesystem path **or** a `data:image/svg+xml,…` URL. Vitest and some
Bun `import … with { type: 'file' }` bindings emit the data URL. GPUIX decodes
both.

Give the element a `width` and `height`: an `<svg>` has no intrinsic size here.
Earlier versions drew a monochrome mask tinted `#e2e2e2` when no `color` was set;
now the SVG paints its own colours, so icons that used `fill="#000"` and relied
on `style.color` to tint them need `fill="currentColor"`.

#### Shapes as children

`<svg>` also takes DOM-style children, so an icon or a chart can be written as
JSX instead of a markup string: `path`, `circle`, `ellipse`, `rect`, `line`,
`polyline`, `polygon`, `g`, `image` and `text`, and what they refer to by `id`:
`defs`, `linearGradient`, `radialGradient`, `stop`, `clipPath`, `mask`,
`symbol`, `use`, `pattern`, `marker` and `filter` (`<rect fill="url(#fade)" />`,
`clipPath="url(#left)"`, `<use href="#bar" />`). They are valid only inside
`<svg>` (anywhere else is an error; `<text>` is the plain text element outside
one), and they are not layout boxes: the `<svg>` paints them all as one image,
so they take no layout or paint `style` and no pointer events. Their `style` takes
[CSS animations](#css-keyframes-on-svg-properties) only, and the animation events fire from them.

- **`pattern`** tiles its children over a `fill="url(#id)"`: `patternUnits`,
  `patternContentUnits`, `patternTransform`, `x`, `y`, `width`, `height`,
  `viewBox`, `preserveAspectRatio` and `href`.
- **`marker`** is drawn at the vertices of a `path`, `line`, `polyline` or
  `polygon` that sets `markerStart`, `markerMid` or `markerEnd` to
  `url(#id)`. The `marker` prop sets all three that are not set themselves.
  It takes `markerWidth`, `markerHeight`, `markerUnits`, `refX`, `refY`,
  `orient`, `viewBox` and `preserveAspectRatio`.
- **`filter`** holds the `fe*` primitives (`feBlend`, `feColorMatrix`,
  `feComponentTransfer` with `feFuncR/G/B/A`, `feComposite`,
  `feConvolveMatrix`, `feDiffuseLighting` and `feSpecularLighting` with
  `feDistantLight`, `fePointLight` or `feSpotLight`, `feDisplacementMap`,
  `feDropShadow`, `feFlood`, `feGaussianBlur`, `feImage`, `feMerge` with
  `feMergeNode`, `feMorphology`, `feOffset`, `feTile`, `feTurbulence`) and is
  applied with the `filter` prop on a shape or `g`: `filter="url(#id)"`. Give
  it `filterUnits`, `primitiveUnits` and a region (`x`, `y`, `width`,
  `height`). This is the SVG attribute, so `filter` on an `<svg>` child is not
  the CSS `filter` style. Chain primitives with `in`, `in2` and `result`.
- **`text`**, `tspan` and `textPath` take their string children as the text,
  and `x`, `y`, `dx`, `dy`, `rotate`, `textLength`, `lengthAdjust`,
  `fontFamily`, `fontSize`, `fontWeight`, `fontStyle`, `textAnchor`,
  `dominantBaseline`, `letterSpacing`, `wordSpacing` and `textDecoration`. White
  space collapses as in a browser unless `xmlSpace="preserve"`. Text is drawn
  with the installed system fonts; an unknown `fontFamily` falls back to the
  default sans-serif.
- **`image`** draws `href` into `x`, `y`, `width` and `height`
  (`preserveAspectRatio` as in a browser). `href` is a `data:` URL, a
  `file://` URL or a file path. Unlike `<img src>`, a `blob:` or `http(s):`
  URL is not loaded: it is dropped with a warning, because the `<svg>`
  rasterises synchronously.

```tsx
<svg viewBox="0 0 24 24" width={24} height={24} style={{ color: '#b4b4b4' }}>
  <g fill="none" stroke="#000" strokeWidth={2} strokeLinecap="round">
    <circle cx={11} cy={11} r={7} />
    <path d="M16 16 L21 21" />
  </g>
</svg>
```

Props are camelCase, as in React DOM, and are written as kebab-case SVG
attributes (`strokeWidth` becomes `stroke-width`). `transform` is an SVG
transform list passed through as a string, and `strokeDasharray` takes a
number, a list or a string. `width` and `height` size the element unless
`style` sets its own; `viewBox` is the coordinate system the children draw in.
When both children and `source` or `src` are given, the children win.

The DOM `pathLength` attribute works as in a browser: it declares the length
that `strokeDasharray` and `strokeDashoffset` are measured in, so
`<path pathLength={1} strokeDasharray="0.5 1" />` strokes half of the outline.
GPUIX measures the shape itself and writes absolute dash values, because the
rasteriser does not support the attribute.

`fill` and `stroke` keep their own colours, so animating them with
[`motion.path`](#animate-svg-shapes) changes hue. They take any colour a style
does: `light-dark()`, `color-mix()` and `color()` resolve in the `<svg>`'s colour
context and reach the rasteriser as `rgba()`.

#### Bun

Use Bun's [`text` loader](https://bun.sh/docs/bundler/loaders#text). The import
is a string containing the complete SVG, and `bun build` embeds it in the
bundle.

```tsx
import searchSvg from './assets/icons/search.svg' with { type: 'text' }

<svg
  source={searchSvg}
  style={{ width: 16, height: 16, color: '#b4b4b4' }}
/>
```

The chat example builds every sidebar and composer icon from raw SVG source this
way.

#### Node.js

For supported Node.js releases, read the icon once relative to the module. A
`URL` keeps the path correct across operating systems and avoids `__dirname`.

```tsx
import { readFileSync } from 'node:fs'

const searchSvg = readFileSync(
  new URL('./assets/icons/search.svg', import.meta.url),
  'utf8',
)

<svg
  source={searchSvg}
  style={{ width: 16, height: 16, color: '#b4b4b4' }}
/>
```

Node.js also has [text modules](https://nodejs.org/api/esm.html#text-modules),
but they currently require `--experimental-import-text`. Prefer
[`readFileSync`](https://nodejs.org/api/fs.html#fsreadfilesyncpath-options) until
text imports no longer need a runtime flag.

## Supported Events

| Event | Props | Payload fields |
|-------|-------|----------------|
| Click | `onClick` | the mouse fields below, `detail` — primary button only. Fires on the nearest common ancestor of the press and release targets |
| Aux click | `onAuxClick` | as `onClick` — non-primary buttons |
| Double click | `onDoubleClick` | as `onClick`, after the click whose `detail` is 2 |
| Context menu | `onContextMenu` | as `onClick`, on the secondary button **press**, and on macOS on a ctrl+primary press (`button` 0) |
| Mouse down / up | `onMouseDown`, `onMouseUp` | the mouse fields below, `button`, `detail` — not fired for a press whose `pointerdown` was cancelled |
| Mouse move | `onMouseMove` | the mouse fields below, `pressedButton`; `movementX` and `movementY` carry the motion. Not fired while a press whose `pointerdown` was cancelled is held |
| Mouse enter / leave | `onMouseEnter`, `onMouseLeave` | `relatedTarget` — do not bubble, fire per element in DOM order |
| Mouse over / out | `onMouseOver`, `onMouseOut` | `relatedTarget` — bubble |
| Pointer events | `onPointerDown`, `onPointerMove`, `onPointerUp`, `onPointerEnter`, `onPointerLeave`, `onPointerOver`, `onPointerOut` | the mouse fields, plus `pointerId` (1) and `pointerType` (`"mouse"`); `onPointerMove` carries the motion in `movementX` and `movementY`, which are `0` on every other mouse and pointer event. `onPointerCancel` fires on the press target when a `draggable` drag starts. As in the DOM, `pointerdown` fires when the first button goes down and `pointerup` when the last comes up; a button pressed or released in between is a `pointermove` with that `button` and the updated `buttons` (the mouse events still fire for every button). Boundary events come as all four pointer ones, then all four mouse ones |
| Drag target | `onDragEnter`, `onDragOver`, `onDragLeave`, `onDrop` | the mouse fields, `dataTransfer` — for files dragged in from the OS and for `draggable` elements; `relatedTarget` on enter and leave. All bubble |
| Drag source | `onDragStart`, `onDrag`, `onDragEnd` | the mouse fields, `dataTransfer` — on a `draggable` element. All bubble |
| Click outside | `onMouseDownOutside` | the mouse fields below, `button` — `target` is the pressed element, `currentTarget` the listening one |
| Key down | `onKeyDown` | `key`, `code`, `keyChar`, `repeat`, `isComposing`, `shiftKey`, `ctrlKey`, `altKey`, `metaKey`, `modifiers` |
| Key up | `onKeyUp` | `key`, `code`, `keyChar`, `repeat`, `isComposing`, `shiftKey`, `ctrlKey`, `altKey`, `metaKey`, `modifiers` |
| Composition | `onCompositionStart`, `onCompositionUpdate`, `onCompositionEnd` | `data` — bubble from the `<input>` or `<textarea>` being composed in, not cancelable. See [Input method composition](#input-method-composition) |
| Focus | `onFocus` | `focusVisible`, `relatedTarget`. See [Focus events](#focus-events) |
| Blur | `onBlur` | `focusVisible`, `relatedTarget` |
| Focus in | `onFocusIn` (Solid; React's `onFocus`) | `focusVisible`, `relatedTarget`. Bubbles |
| Focus out | `onFocusOut` (Solid; React's `onBlur`) | `focusVisible`, `relatedTarget`. Bubbles |
| Wheel | `onWheel`, `onWheelCapture` | `deltaX`, `deltaY`, `deltaMode`, `clientX`, `clientY`, `offsetX`, `offsetY`, `buttons`, `precise`, `touchPhase`, `modifiers`. Bubbles, cancelable. See [The wheel](#the-wheel) |
| Scroll | `onScroll` | `scrollTop`, `scrollLeft` — scroll containers, not `<virtual-list>` |
| Content visibility | `onContentVisibilityAutoStateChange` | `skipped` — an element with `contentVisibility: "auto"` began or stopped skipping its content. Bubbles, cancelable, has an `…Capture` twin. See [Skipping content with `contentVisibility`](#skipping-content-with-contentvisibility) |
| Layout | `onLayout` | `width`, `height`, `x`, `y` — any element a `ResizeObserver` works on (`<div>`, `<text>`, `<input>`, `<img>`, `<svg>` and the other built-in elements). See also `ResizeObserver` and `IntersectionObserver` under [Measure an element](#measure-an-element) |
| Change | `onChange` | `value`, `inputType`, `data`, `isComposing` — `<input>` and `<textarea>` only, once per edit like the DOM `input`. See [The change event](#the-change-event) |
| Submit | `onSubmit` | `formData`, `submitter` — `<form>`, bubbling. See [Forms](#forms) |
| Toggle file | `onToggleFile` | `value` (file path) — `<diff>` only |
| Show more | `onShowMore` | `value` (hidden line count) — `<diff>` only |
| Line click | `onLineClick` | `value`, `oldLine`, `newLine` — `<diff>` only |
| Link click | `onLinkClick` | `value` (URL) — `<markdown>` only |
| Selection change | `onSelectionChange` | `value` (joined selected text) — window-level on `render()` |
| Appearance change | `onAppearanceChange` | `value` (`"light"` or `"dark"`) — window-level on `render()` |

Every pointer, mouse, drag and composition handler also has a capture-phase twin:
`onClickCapture`, `onPointerDownCapture`, `onDropCapture`, and so on.
`onMouseDownOutside` is gpuix's own event and has none.

`onMouseDownOutside` fires after `pointerdown` and `mousedown` of a press, for any button, on every element
that listens for it and does not contain the pressed element. A press that hits no element at all is outside
every listener, the root's included, and its `target` is the root. "Contain" follows the logical tree, so a press
inside an `<anchored>` popup declared under an element is inside that element, and a press on an overlay that
only paints above it is outside. The handler runs at the listening element alone: it does not bubble, and
`target` is the element that was pressed. A press the dispatcher retargets, such as one on a pointer-captured
element, reports the captured element.

Events are DOM-shaped and propagate like the DOM. A handler receives
`target`, `currentTarget` and `relatedTarget`, `composedPath()`, `eventPhase`,
`bubbles`, `cancelable`, `defaultPrevented`, `stopPropagation()`,
`stopImmediatePropagation()` and `preventDefault()`, plus `clientX`/`clientY`,
`offsetX`/`offsetY`, `button`, `buttons`, `detail`, `pointerId`, `pointerType`
and `modifiers` where the event has them.

`target`, `currentTarget` and `relatedTarget` are **elements**, for every
event: the same object a `ref` gives in that adapter (React's host instance,
Solid's element), so they compare with `===` and a handler can call
`event.target.getBoundingClientRect()`. During the `window` and `document`
phases `currentTarget` is that object. `target` is `null` only for an event
aimed at the window (a key with nothing focused, a pointer move outside the
root), and `relatedTarget` is `null` when there is no other element. A native
element id is still there as `event.elementId`, for the renderer methods that
take one (`renderer.scrollTo(elementId, ...)`). `composedPath()` lists the
elements the event travels through, target first and the root last, without
`window` and `document`.
`x`/`y` are the same as `clientX`/`clientY`. `offsetX`/`offsetY` are relative
to the target's padding edge, read from its laid-out border. Native only produces an event type when the
target or one of its ancestors listens for it (the drag events below are the exception, except `drag`). Capture
handlers run from the root down to the target, then the target's handlers,
then bubble handlers back up. `mouseenter`, `mouseleave`, `pointerenter` and
`pointerleave` do not bubble. Event types are the lowercase DOM names
(`event.type === "pointerdown"`), and so are the keyboard and focus events
(`keydown`, `keyup`, `focus`, `blur`, `focusin`, `focusout`).

**`onFileDrop` is removed.** Files dragged in from Finder or the OS are DOM
drag events, delivered along the path under the pointer:

- `dragenter` goes to the element the files moved onto, then `dragleave` to the
  one they left (`relatedTarget` is the other element). `dragover` follows on
  every move. When the files leave the window, `dragleave` goes to the last
  target with no `relatedTarget`.
- A drop on the window outside every element targets the root.
- A drag fires no pointer or mouse event, as in a browser: no `pointermove`,
  `mouseover`, `mouseup` or `click`, and nothing is hovered.
- `drop` is delivered only if the last `dragover` was cancelled with
  `event.preventDefault()` and left `dataTransfer.dropEffect` other than
  `"none"`. Otherwise the target gets `dragleave` instead, as in the DOM.
  Every `dragover` handler on the path decides, so accept the drop where the
  files should land:

```tsx
<div
  onDragOver={(event) => event.preventDefault()}
  onDrop={(event) => {
    for (const file of event.dataTransfer?.files ?? []) {
      console.log(file.path, file.name, file.size, file.type, file.lastModified)
    }
  }}
/>
```

`event.dataTransfer` is one object for the whole drag. `files` entries carry
`path` (absolute), `name`, `size` in bytes, `type` and `lastModified` in
milliseconds since the epoch. `type` is a MIME type guessed from the extension,
or `""`. A file that is not valid UTF-8 or cannot be read is left out, and a
drag with no readable file fires no event. `types` is `["Files"]`; `getData`
returns `""` and `setData` does nothing, as in a browser while a drag is
protected. `dropEffect` starts as `"copy"` on each `dragenter` and `dragover`
and can be set to `"none"`, `"move"` or `"link"`. On macOS the `dragover` also
answers the app the files come from: the drag cursor shows the `dropEffect` of a
cancelled `dragover`, and a drop the page would not take (not cancelled, or
`"none"`) is refused at the OS, so the files slide back. The effect is limited
to what the source allows (holding a modifier narrows it), and a drag with no
readable file is refused. Unlike other pointer events, these four are sent to
JS whether or not something listens, since JS needs every `dragover` to know
whether the drop was accepted. They are also delivered synchronously on macOS,
where the handler runs inside the OS drag callback; a handler that takes long
holds up the drag cursor.

**Dragging an element.** `draggable` makes an element a drag source, like the DOM attribute. A primary press
on it, or on anything inside it that is not itself `draggable`, followed by a move of
more than 4px (logical pixels, from the press) starts a drag:

```tsx
<div
  draggable
  onDragStart={(event) => {
    event.dataTransfer?.setData('text/plain', 'clip-7')
    event.dataTransfer!.effectAllowed = 'move'
  }}
  onDragEnd={(event) => console.log(event.dataTransfer?.dropEffect)}
/>
<div
  onDragOver={(event) => event.preventDefault()}
  onDrop={(event) => move(event.dataTransfer?.getData('text/plain'))}
/>
```

- The press target gets `pointercancel` and the pointer leaves it
  (`pointerout`, `pointerleave`, `mouseout`, `mouseleave`). `dragstart` follows
  at the source, the nearest `draggable` element from the press target up. From then
  until the drag ends no pointer or mouse event fires, nothing is hovered, and
  the release is not a click.
- Each move sends `drag` to the source, then the target events of a file drag
  to what is under the pointer: `dragenter`, `dragleave` (`relatedTarget`
  is the other element) and `dragover`. Dropping on the window outside every element targets the root.
- The release sends `drop` to the target, then `dragend` to the source. `drop`
  is delivered under the rules of a file drag: only if the last `dragover` was
  cancelled with `preventDefault()` and `dropEffect` is not `"none"` and is allowed by
  `effectAllowed`; otherwise the target gets `dragleave` instead. `dragend` reports
  `dropEffect`: the effect the drop took, or `"none"` if nothing was dropped.
- Escape cancels the drag: `dragleave` at the target, then `dragend` at the source with
  `dropEffect` `"none"`. As in Chrome the key is swallowed: no `keydown` for that Escape reaches
  an element, `window` or `document` listener, and no layer (a dialog) closes on it. Escape
  with no drag in progress is an ordinary key.
- `event.preventDefault()` in `onDragStart` cancels the drag. The drag has
  already started when JS runs, so a move or two may already have been
  sent; JS drops them, and pointer events carry on from the next input.
- A drag does not start while another element holds pointer capture. If the source itself
  holds it, it is released (`lostpointercapture`) as the drag starts.

`event.dataTransfer` is one object for the whole drag, in DOM protected mode:
`setData`, `getData`, `clearData`, `types`, `effectAllowed` and `dropEffect`.
`setData` and `effectAllowed` work in `dragstart` only. `getData` works in
`dragstart` and `drop` and returns `""` in every other event, `dragover` included.
`setDragImage` is accepted and does nothing: gpuix does not draw a drag image, so
nothing follows the pointer. The drag stays inside the window: it cannot drop on
another app, and a drag from another app is a file drag. See FORK.md for what a
drag image would take.

**`onWheel`** reports wheel input: the delta, before any scroller has applied it,
signed like the DOM (`deltaY` is positive for down). **`onScroll`** reports the
result, after any cause: the wheel, a scrollbar drag, the keyboard, `scrollTo`,
`scrollIntoView`, a smooth animation, or content that shrank under the offset.
`scrollTop` and `scrollLeft` are pixels from the start, as on the DOM element. The
offset at the first paint is the baseline and is not reported. Use
`onVisibleRange` on a `<virtual-list>`. See [Scrolling](#scrolling).

```tsx
<div
  style={{ height: 300, overflow: 'scroll' }}
  onScroll={(event) => setStuck(event.scrollTop > 80)}
>
  {rows}
</div>
```

**`onLayout`** is `ResizeObserver` for an element, so it works on every element a
`ResizeObserver` does, built-in ones such as `<input>` and `<img>` included. It fires after the first paint
that has the listener and again each time the element's **size** changes, with
its border box: `width` and `height`, and `x` and `y` in window pixels. Moving
without resizing, as a row does when a list scrolls, fires nothing. Removing the
listener and adding it back reports the size again. An element with
`display: "none"` has no box, so it reports nothing until it is shown. One with
`visibility: "hidden"` keeps its box and reports it.

```tsx
<div onLayout={(event) => setWidth(event.width ?? 0)} style={{ flex: 1 }} />
```

The event arrives after the frame that laid the element out, so a handler that
sets state paints the new size one frame later. Read it for decisions such as
which layout to render, not to position something against the element in the
same frame.

Only a focusable element holds a GPUI `FocusHandle`: an enabled input or
textarea, or an element with a `tabIndex`. A key or focus listener alone does not
make a `div` focusable; add `tabIndex` (see
[What is focusable](#what-is-focusable)).

`onMouseLeave` also fires when the pointer leaves the **window** without a final
move over the element, and the element's `hover` style ends with it.

### Click without a pointer

The VoiceOver Press and Enter or Space on a `Button` (`buttonProps`) deliver a
`click` like `HTMLElement.click()` does: `detail` 0, `pointerId` -1, `pointerType`
`""`, `buttons` 0, no position, and a `target` and `path`, so it bubbles to the
ancestors' `onClick`. A host ref has the same `click()`, and
`renderer.clickElement(elementId)` is the same call on the renderer. In your own
`onKeyDown` or `onKeyUp`, call `click()` on the ref of the element that listens.
`click()` and the VoiceOver Press do nothing on a `button`, `input`, `textarea` or
`select` that is `disabled`, or on an element inside one, as in HTML. A disabled
`Button` or `SelectTrigger` is one too (see [Form control state](#form-control-state-disabled-enabled)).
Every mouse and pointer event type has `pointerId` and `pointerType` as required
fields: `1` and `"mouse"` for the mouse.

### Pointer capture

Nothing captures the pointer on its own. Listening for `onMouseDown` and
`onMouseMove` on one element does not keep the moves coming once the pointer
leaves it: that element hears only what happens over it, as in the DOM. To keep
a gesture alive past the element's edge, past its parent, and past the window,
capture the pointer on the press with `setPointerCapture`, like HTML
[`setPointerCapture`](https://developer.mozilla.org/en-US/docs/Web/API/Element/setPointerCapture):

```tsx
const handle = useRef<PublicInstance>(null)

<div
  ref={handle}
  style={{ cursor: 'grab', active: { cursor: 'grabbing' } }}
  onPointerDown={(e) => handle.current?.setPointerCapture?.(e.pointerId)}
  onPointerMove={moveDrag}
  onPointerUp={endDrag}
/>
```

`onMouseDown` works the same way; its event carries `pointerId` too. A full-window
overlay mounted on the press cannot replace the capture: the overlay does not
exist yet when the press happens, and a release past the window edge is lost.
Only the capturing element receives moves while the gesture runs, and only the
hovered element receives them otherwise, so the cost is one event per pointer
move. A press on a child is still a click on that child: listening for
`onMouseDown` and `onMouseMove` swallows nothing.

A host ref has `setPointerCapture(pointerId)`, `releasePointerCapture(pointerId)`
and `hasPointerCapture(pointerId)`, as DOM elements do. The mouse is pointer
`1`; any other `pointerId` throws for all three. `setPointerCapture` also throws
for an element that is not in the tree, while `releasePointerCapture` on one does
nothing and `hasPointerCapture` returns `false`.

While an element holds the capture, pointer and mouse events target it, with the
event path built from it, wherever the pointer is. `:hover` and the boundary
events follow, as in Chrome: the chain the pointer was in gets `pointerout` and
`pointerleave`, the capturing element gets `pointerover` and `pointerenter`.
`gotpointercapture` fires before the first event the capture changes. The capture
ends once the last button is up, in the Pointer Events order: `pointerup`, then
`lostpointercapture`, then `mouseup`, then `click`. Removing the element ends the
capture with no event.

The call is in force when it returns: `hasPointerCapture` is `true` right after
`setPointerCapture`, and the next native pointer input is already aimed at the
element. `gotpointercapture` (and `lostpointercapture` after a release) fires
with that input, before its other events, which is when the DOM fires it. On
Windows and Linux a `pointerdown` handler runs after native has moved on, so a
move that arrives before it is not captured. Calling `setPointerCapture` with no
button down does nothing, as in Chrome. `renderer.setPointerCapture(elementId, pointerId)`,
`releasePointerCapture` and `hasPointerCapture` are the same calls on the
renderer.

### Pointer lock

`requestPointerLock()` on a host ref hides the cursor and freezes it, so a
drag-to-adjust number, a camera or a canvas pan can keep turning however far
the mouse travels, like HTML
[`requestPointerLock`](https://developer.mozilla.org/en-US/docs/Web/API/Element/requestPointerLock):

```tsx
const dial = useRef<PublicInstance>(null)

<div
  ref={dial}
  onMouseDown={() => dial.current?.requestPointerLock?.()}
  onMouseMove={(e) => turn(e.movementX)}
/>

document.addEventListener('pointerlockchange', () => {
  setLocked(document.pointerLockElement !== null) // the locked element, null once released
})
document.addEventListener('pointerlockerror', () => {
  console.warn('could not lock')
})
```

While the lock is held, pointer and mouse events target the lock element, as
with capture, wherever the mouse is, and `clientX`/`clientY` stay at the
position the pointer had when the lock began. The motion is in `movementX` and
`movementY`. `renderer.exitPointerLock()` releases it, and
`renderer.pointerLockElement()` returns the locked element's id or `null`.
`renderer.requestPointerLock(elementId)` is the same call as the ref's.

On macOS the lock is taken inside the call, from an input handler too:
`renderer.pointerLockElement()` and `document.pointerLockElement` are the
element as soon as `requestPointerLock()` returns, and the next pointer event is
already aimed at it. (The DOM sets `pointerLockElement` a task later.) The
outcome arrives afterwards, as events, so a handler never hears it inside the
call that asked. Elsewhere the next frame applies the request.

`pointerlockchange` and `pointerlockerror` are DOM events at `document`: listen
with `document.addEventListener('pointerlockchange', ...)`, the `document` and
`window` objects `@gpuix/react` and `@gpuix/solid` export. `event.target` is
`document`, as in the DOM, and they run the window propagation (window capture,
document capture, document bubble, window bubble), bubble and cannot be
cancelled. The element is `document.pointerLockElement`, like
`document.activeElement`; an error names none.

- `pointerlockchange` fires when the lock element changes: the lock was
  granted, moved to another element, or ended. `document.pointerLockElement` is
  the new lock element, `null` once the lock is gone. A lock ends by
  `exitPointerLock()`, when the window loses focus (the platform releases it),
  and when the locked element leaves the tree.
- `pointerlockerror` fires when a request fails: the element is not in the
  tree, or the platform refused.

Asking again for the element that already holds the lock does nothing. Asking
for another one moves the lock without releasing the cursor.

Only macOS can lock the pointer (`CGAssociateMouseAndMouseCursorPosition`), and
only for the active window. On Windows and Linux every request fails with
`pointerlockerror`.

`movementX` and `movementY` are on every pointer, mouse, wheel and drag event.
On `pointermove` and `mousemove`, locked or not, they are how far the pointer
moved since the previous move, in logical pixels, from the platform's own
relative motion; on every other event they are `0`, as in the DOM. They are
`0` on a move too where the platform reports no relative motion (everything but
macOS). They are required fields of `HostMouseEvent`.

Under test the offscreen window is never active, so a real lock is always
refused. `TestRenderer` runs a simulated platform instead: `requestPointerLock`
and `exitPointerLock` take the lock in the call, from a handler too, and deliver the events,
`simulatePointerLockGranting(false)` makes the platform refuse, and
`simulatePointerLockLost()` drops the lock as a deactivation does. The
simulated platform reports no motion unless a move says so:
`nativeSimulateMouseMove(x, y, modifiers, { x: 4, y: -3 })`.

`onClick` fires on primary-button mouse-up. Use **`onAuxClick`** for the others,
and read `event.isRightClick`. `onMouseDown` and `onMouseUp` see every
button through `event.button` (`0` left, `1` middle, `2` right).

## Supported Styles

CSS-like styling via the `style` prop:

```tsx
<div style={{
  display: 'flex',
  flexDirection: 'column',
  gap: 8,
  padding: 16,
  backgroundColor: '#3b82f6',
  borderRadius: 8,
}}>
  <div style={{ color: '#ffffff', fontSize: 18 }}>
    Hello GPUI!
  </div>
</div>
```

> [!IMPORTANT]
> **GPUIX styles look like CSS but are not CSS.** A few differences trip
> everyone up on the first project:
>
> - **`div` is block, not flex.** Set `display: "flex"` before using
>   `flexDirection`, `gap`, `alignItems`, or `alignSelf`. Without it those
>   props are silently ignored.
> - **A flex child that must shrink needs `minWidth: 0`.** Same rule as CSS,
>   but easier to miss because there is no browser DevTools to inspect.
> - **`position` is `static` unless you set it**, as in CSS. `top`/`left` do
>   nothing on a static box, and a static box is not the reference for
>   `position: "absolute"` children: they resolve against the nearest
>   `relative`, `absolute` or `fixed` ancestor. Put `position: "relative"` on
>   the parent of anything you place with `top`/`right`/`bottom`/`left`.
> - **Of a shorthand and a longhand it covers, the key written later wins**, as in
>   CSS and React DOM. `{ paddingLeft: 4, padding: 8 }` pads the left by 8 and
>   `{ padding: 8, paddingLeft: 4 }` by 4. See [Shorthands](#shorthands).
> - **`boxShadow` takes the CSS string or a structured object.** See below.
> - **No `<button>`.** Use `<div onClick>` with `cursor: "pointer"`.
> - **Do not nest `<text>` in `<text>`.** Adjacent `<text>` siblings merge
>   into one line. A `<text>` child of another `<text>` is a nested div.
> - **`<input>` has no default inner padding.** Set `padding` on the input
>   style, or pad the parent wrapper. The input clips to its own
>   `borderRadius` automatically.

**Layout:** `display` (`"flex"` | `"grid"` | `"block"` | `"inline-block"` | `"inline-flex"` | `"inline-grid"` | `"none"` | `"contents"` | the [table](#tables) values, `"inline-table"` among them; the `inline-*` values are [atomic inlines](#inline-block-inline-flex-inline-grid-and-inline-table)), `verticalAlign` (`"baseline"` | `"sub"` | `"super"` | `"text-top"` | `"text-bottom"` | `"middle"` | `"top"` | `"bottom"` | a length | a percentage of the line height; see [Vertical alignment](#vertical-alignment)), `flex`, `flexFlow`, `flexDirection` (`"row"` | `"row-reverse"` | `"column"` | `"column-reverse"`), `flexWrap` (`"nowrap"` | `"wrap"` | `"wrap-reverse"`), `flexGrow`, `flexShrink`, `flexBasis`, `order`, `alignItems` (`"start"` | `"center"` | `"end"` | `"stretch"` | `"baseline"`), `alignSelf`, `alignContent`, `justifyContent` (adds `"space-between"`, `"space-around"` and `"space-evenly"`), `justifyItems`, `justifySelf`, `placeItems`, `placeContent`, `placeSelf`, `gap` (one or two lengths), `rowGap`, `columnGap`. Grid: see [Grid](#grid). The shorthands are in [Shorthands](#shorthands)

**Sizing:** `width`, `height`, `minWidth`, `minHeight`, `maxWidth`, `maxHeight` — accepts pixels (number), any [length](#lengths) or `calc()` (string like `"100%"`, `"20rem"`, `"calc(100% - 2em)"`) or `"auto"`. `width`, `minWidth`, `maxWidth` and `flexBasis` also take the [intrinsic keywords](#intrinsic-widths) `min-content`, `max-content` and `fit-content`, and `flexBasis` `content`. `aspectRatio` is a number or a CSS ratio (`"16 / 9"`, `"1"`) and sizes whichever axis has no definite size

**Spacing:** `padding`, `paddingBlock`, `paddingInline`, `paddingTop/Right/Bottom/Left`, `margin`, `marginBlock`, `marginInline`, `marginTop/Right/Bottom/Left` — pixels or any [length](#lengths); `padding` and `margin` take one to four values (`"0 auto"`, see [Shorthands](#shorthands)). A percentage is of the containing block's width, and margins also take `"auto"` (which absorbs free space in flex layouts, so `marginLeft: "auto"` pushes an item to the end)

**Position:** `position` (`"static"` (default) | `"relative"` | `"absolute"` | `"fixed"` | `"sticky"`), `top`, `right`, `bottom`, `left` (pixels, any [length](#lengths) or `"auto"`), and `inset`, `insetBlock` and `insetInline` for them (`inset: 0` fills the containing block). An absolute box resolves its insets, percentage sizes and margins against the padding box of its nearest non-static ancestor (the window if there is none; a sticky ancestor counts) and keeps its static position on an `auto` axis. `"fixed"` resolves against the window. `zIndex` (an integer) orders a positioned element, or a flex or grid item, in its stacking context; it does nothing on another static box, and a sticky element is positioned. See [Paint order and stacking contexts](#layout-like-css) and [Sticky positioning](#sticky-positioning) for `"sticky"`

**Visual:** `background`, `backgroundColor`, `backgroundImage`, `backgroundSize`, `backgroundPosition`, `backgroundRepeat`, `backgroundOrigin`, `backgroundClip`, `color`, `colorScheme`, `opacity`, `cursor`, `pointerEvents`, `appRegion` (`"drag"` | `"no-drag"`, see [Window drag regions](#window-drag-regions)), `borderRadius`, `borderTopLeftRadius`, `borderTopRightRadius`, `borderBottomLeftRadius`, `borderBottomRightRadius`, `borderWidth` (one to four values), `borderTopWidth`, `borderRightWidth`, `borderBottomWidth`, `borderLeftWidth`, `borderColor`, `borderTopColor`, `borderRightColor`, `borderBottomColor`, `borderLeftColor`, `borderStyle`, `borderTopStyle`, `borderRightStyle`, `borderBottomStyle`, `borderLeftStyle`, `border`, `borderBlock`, `borderInline`, `borderTop`, `borderRight`, `borderBottom`, `borderLeft`, `boxShadow`, `filter`, `backdropFilter`, `mask`, `maskImage`, `maskSize`, `maskPosition`, `maskRepeat`, `maskOrigin`, `maskClip`, `maskMode`, `maskComposite`, `maskBorder`, `maskBorderSource`, `maskBorderSlice`, `maskBorderWidth`, `maskBorderOutset`, `maskBorderRepeat`, `maskBorderMode`, `clipPath`, `outline`, `outlineWidth`, `outlineStyle`, `outlineColor`, `outlineOffset` (see [Outline](#outline))

### Lengths

Every length-typed style field takes a number (pixels) or a CSS string: `px`,
`in`, `cm`, `mm`, `q`, `pt`, `pc`, `%`, `em`, `rem`, `vw`, `vh`, `vmin`, `vmax`, or
a `calc()` that adds and subtracts any of them and multiplies or divides by a
number. A length and a `%` mixed in one `calc()` works wherever the property has a
basis to resolve the percentage against. Invalid strings throw from `setStyle`
like any other invalid value. A bare number written as a string (`"12"`) is
accepted only on the sizes (`width`, `height`, `min*`, `max*`) and, like them, the
margins and insets; every other field needs a unit or a JS number.

```tsx
<div style={{ width: 'calc(100% - 2rem)', padding: '0.5em', gap: '1vw', fontSize: '1.25rem' }} />
```

The fields:

- Sizes and offsets: `width`, `height`, `minWidth`, `minHeight`, `maxWidth`,
  `maxHeight`, the margins, `top`, `right`, `bottom`, `left`, the paddings, `gap`,
  `rowGap`, `columnGap`, and `flexBasis` (which also takes `auto` and `content`)
- Lines: `borderWidth` and the four side widths (no `%`; `thin`, `medium` and
  `thick` work), `outlineWidth` and `outlineOffset` (no `%`), and the widths in
  the `border` and `outline` shorthands
- Text: `fontSize` and `lineHeight`
- Paint: `boxShadow` offsets, blur and spread (no `%`), the lengths inside
  `filter`, gradients, `backgroundPosition` and `backgroundSize`, `mask*` and the
  `clipPath` shapes
- Grid: the track sizes of `gridTemplateColumns`, `gridTemplateRows`,
  `gridAutoColumns` and `gridAutoRows`
- Tables: `borderSpacing` (one or two lengths, no `%`, nothing negative)

`borderRadius` and the corner radii are not in this list.

What a unit is relative to:

- **`em`** is the element's own computed font size. In `fontSize` itself it is the
  parent's, and so is a `%` there.
- **`rem`** is the root element's computed font size, 16px unless the root sets
  `fontSize`. The root's own `rem` is the initial size.
- **`vw`, `vh`, `vmin`, `vmax`** are the window's size, and are resolved again on
  resize.
- **`cqw`, `cqh`, `cqi`, `cqb`, `cqmin`, `cqmax`** are fractions of the content box of
  the nearest ancestor [query container](#container-queries), and follow it when it
  resizes. With no such container they are the window's size, like `vw` and `vh`.
- A `hover`, `active` or group `fontSize` changes the element's own `em`, and so
  its `em` lengths. A `focusVisible` or `focusWithin` `fontSize` changes the
  element's own text only and not the `em` of its children, the same as `color`.
- Two nested `display: contents` ancestors that set `fontSize` in `em` do not
  compound.

`lineHeight` is CSS `line-height`. A number is a unitless multiplier of the
element's font size (`1.5`), a string is a length, a percentage of the font size
or a `calc()` (`"24px"`, `"150%"`, `"1.5em"`), and `"normal"` is the font's
default leading. A multiplier inherits as a multiplier, so each child gets its own
multiple, and a length inherits as pixels.

Limits: a `calc()` that comes out negative clamps to 0 for `fontSize` and for grid
track sizes. A repeating gradient whose stop position is a `calc()` of a length and
a `%` skips the special case for a zero-length repeat.

### Shorthands

A shorthand is its own style field. Of a shorthand and a longhand it covers the key
written later wins, as in CSS. The ones below take CSS's several-value syntax.

```tsx
<div style={{ display: 'flex', flex: '1 0 200px', padding: '8px 12px', margin: '0 auto', gap: '4px 8px' }} />
```

| Shorthand | Takes |
|---|---|
| `padding`, `margin`, `inset`, `borderWidth` | One to four values in CSS order: `"8px"` is all sides, `"8px 12px"` top and bottom then left and right, `"1px 2px 3px"` top, left and right, bottom, `"1px 2px 3px 4px"` top, right, bottom, left. `margin` and `inset` take `auto` too. `borderWidth` takes `thin`, `medium` and `thick` and no `%` |
| `gap` | One or two lengths: `"4px 8px"` is the gap between rows, then between columns |
| `paddingBlock`, `paddingInline`, `marginBlock`, `marginInline`, `insetBlock`, `insetInline` | One value for both ends or `"<start> <end>"`. Block is top and bottom, inline is left and right: gpuix only lays out horizontally. The start of an inline pair is the left, and the right where the element's `direction` is `rtl` |
| `borderBlock`, `borderInline` | Written like `border` (`"1px solid red"`), for top and bottom or left and right (both sides get the one value, so `direction` does not matter) |
| `flex` | A number `n` is `n 1 0%`. `none` is `0 0 auto`, `auto` is `1 1 auto`, `initial` is `0 1 auto`. Otherwise `<grow> <shrink>? \|\| <basis>`: `"2 3"`, `"2 100px"`, `"100px 2"`, and a lone basis (`"100px"`, `"50%"`, `"fit-content"`) is `1 1 <basis>`. The basis is what `flexBasis` takes: `"1 1 content"` is a basis of the content size |
| `flexFlow` | `"<direction> \|\| <wrap>"` in either order (`"column wrap"`); a part left out is `row` or `nowrap` |
| `placeItems`, `placeContent`, `placeSelf` | `"<align> <justify>"`; one value is both (`"center"`, `"center end"`). Each value is one the longhand takes, so `placeItems: "left"` throws as in CSS |
| `outline` | See [Outline](#outline) |

- **The later key wins.** `{ paddingLeft: 4, padding: 8 }` pads the left by 8 and
  `{ padding: 8, paddingLeft: 4 }` by 4, whatever the shorthand covers: `paddingBlock`
  and `paddingInline` between `padding` and the sides, `border` over `borderBlock`, the
  side shorthands and `borderWidth`, and so on for every shorthand in this page. The
  same holds in each state style, in the keys a `UniversalStyle` layer sets again, and
  in the `animate`, `initial`, `exit`, variant and `transition` maps of `motion`.
- **In a state style** (`hover`, `focusVisible`, ...) a shorthand replaces the base
  style's longhands it covers: `hover: { padding: 12 }` over a base `paddingLeft: 4`
  pads 12 on the left. A state's longhand only changes its own side.
- **Numbers.** A JS number is px. A bare number written inside a string reads as px
  only in `margin` and `inset` (`margin: "0 8"`), like their longhands; elsewhere
  it needs a unit, except `0`. A `calc()` is one value however many spaces it holds.
  A bad count, a bad value, or `auto` where the property takes none throws when
  the style is set. A numeric `border: 1` throws too: write `"1px solid"`.
- **Percentages** in `padding` and `margin` are of the containing block's width on
  every side, as in CSS.
- **`flex` and the longhands.** `flexGrow`, `flexShrink` and `flexBasis` win over
  `flex`. In a column whose height is `auto`, `flex: 1` takes an explicit `height` on
  the item as its basis, where Chrome uses the content.
- **Animation.** The multi-value shorthands animate, each side, axis or corner on
  its own (see [Targets and timing](#targets-and-timing)). `flex` is not a `motion`
  target: animate `flexGrow`, `flexShrink` and `flexBasis`, which `transition: flex`
  covers.

### Intrinsic widths

`width`, `minWidth`, `maxWidth` and `flexBasis` take `min-content`, `max-content`,
`fit-content` and `fit-content(<length-percentage>)`, with the CSS Sizing
meaning. `minWidth: "fit-content"` is `min-content` and `maxWidth: "fit-content"`
is `max-content`. Text wraps at its widest unbreakable stretch for `min-content`, found by the
rules the text breaks lines by: its longest word, without the spaces that hang
after it, or one letter with `overflowWrap: 'anywhere'` or `wordBreak: 'break-all'`
(`overflowWrap: 'break-word'` does not shrink it, as in CSS), and the tab stops
and the space of the inline boxes in it count. A text in a flex item therefore
does not shrink below its longest word without `minWidth: 0`.

```tsx
<div style={{ display: 'flex' }}>
  <div style={{ width: 'fit-content', maxWidth: '50%' }}>a label that wraps</div>
</div>
```

`fit-content` is the preferred width `min(max-content, max(min-content, available width))`
(in a flex column, the container's content width less the margins of an item that stretches, whatever
margin the container itself has; an item aligned to a side is not narrowed by its own margin), and then `minWidth` and `maxWidth` apply to it, `minWidth` winning, as in CSS. A
`maxWidth` under the min-content width is therefore the width (the content overflows), and a
`maxWidth` in `%` over the max-content width does not widen the box: it hugs its content.
A child with `width: '100%'` does not count in the box's content sizes (a percentage of a
width that depends on the child is `auto` there), and gets the final width of the box
afterwards, so a `100%` child of a box that hugs its other content is as wide as they are.

A layout pre-pass measures the width keywords (and `fit-content(<limit>)` as a `flexBasis`), so each node
with one costs extra layouts of its subtree every frame, and nesting multiplies it. Use them where the content
decides the width, not on every box of a long list. The `flexBasis` keywords are read by the flex layout itself
and cost no pre-pass. A `height` keyword behaves as
`auto`, since the block axis of CSS Sizing has no intrinsic size to ask for.
The margins and the insets do not take the keywords.

`flexBasis` takes the same keywords, and `content`, and they win over `width` (and over a basis
length set next to them). They size the flex item from its content, as in Chrome:

- In a `row` or `row-reverse` container, where the main axis is the inline one, `max-content` and
  `min-content` are those content sizes, `content` is `max-content` whatever the item's `width` is (it
  still counts as the item's automatic minimum size and for sizing a container that hugs its
  items), and `fit-content` is `min(max-content, max(min-content, the container's content width less
  the item's margins))`. `minWidth` and `maxWidth` then clamp the basis as they clamp any other.
  An item with `flexShrink: 0` is therefore as wide as the container at most. In a container that is
  sized by its items, with no width to fit, `fit-content` is `max-content` (or `min-content` when the
  container is sized under that).
- A `width: "fit-content"` on a row item with no `flexBasis` of its own is that same size as the
  basis, and no limit: the item still grows and shrinks from it.
- In a `column` container the main axis is the block one, and every keyword is `content`: the height
  of the item's content at the width it gets, not its `height`. In a parent that is not a flex
  container the keywords do nothing.

```tsx
<div style={{ display: 'flex', width: 300 }}>
  {/* 300px wide at most, the text's own width below that */}
  <div style={{ flexBasis: 'fit-content', flexShrink: 0 }}>some text that can wrap</div>
</div>
```

A later `auto` or definite value in a `hover` or focus style replaces a keyword,
and `width: "auto"` in a state style replaces the base `width`, as in CSS.

### Borders

Widths, colours and corners can differ per side:

```tsx
<div style={{ borderBottomWidth: 2, borderBottomStyle: 'solid', borderBottomColor: '#89b4fa' }} />
<div style={{ borderWidth: 4, borderStyle: 'solid', borderColor: 'red blue' }} />   {/* top/bottom red, left/right blue */}
<div style={{ border: '1px solid rgba(0,0,0,.2)', borderTop: '2px dashed currentColor' }} />
```

`borderColor` and `borderStyle` take one to four values with the CSS meaning
(`"dotted dashed"` is dotted top and bottom, dashed left and right), and
`borderTopStyle`, `borderRightStyle`, `borderBottomStyle` and `borderLeftStyle`
set one side. `border` and `borderTop` / `borderRight` / `borderBottom` /
`borderLeft` take `<width> <style> <color>` in any order, and `borderBlock` and
`borderInline` the same for the top and bottom or the left and right. A missing colour is
`currentColor`, and a missing width is 3px (`medium`; `thin` is 1px and `thick`
5px). A width is any [length](#lengths) without a `%`, so `border: '0.1em solid red'`
works. `border` is applied first, then `borderBlock` and `borderInline`, then the side shorthands, then `borderStyle`,
then the longhands (`borderWidth`, `borderTopColor`, `borderTopStyle`, ...). An
invalid value, such as a style that is not in the list below, is ignored.

As in CSS, a side's style starts as `none`, and a `none` or `hidden` side has no
width: nothing is drawn and no space is taken. So a border needs a style, from
`borderStyle`, a side's `borderTopStyle` or a shorthand: `{ borderWidth: 1,
borderColor: 'red' }` and `{ border: '1px red' }` draw nothing, and `{ borderWidth:
1, borderStyle: 'solid', borderColor: 'red' }` does. The other way round, a side
with a visible style and no width is `medium` (3px). Only the sides that name a
style have a border, so `{ borderBottomWidth: 2, borderBottomStyle: 'solid' }` is a
bottom border and the other sides stay at 0. A border on one side of a box with no
content on that axis, such as a `borderTop` hairline on an empty `div`, paints its
whole width. A motion animation or a
[CSS transition](#css-transitions) of a border width shows the width a side has,
so a side with no style stays at 0.

The line styles are drawn like Chrome draws them, each side with its own:

- `solid`, and `dashed` (dashes two widths long with a gap of one, spread to
  fit the side).
- `dotted`: round dots one width across and two apart. They are laid out along
  each side and, on a rounded corner, round it.
- `double`: two lines with a gap between them, a third of the width each,
  rounded to whole device pixels with the rest as the gap. A border under three
  device pixels wide is drawn solid.
- `groove` and `ridge`: the border is split along its middle into two halves
  that are shaded in opposite tones, the outer half like `inset` and the inner
  half like `outset` for a `groove`, and the other way round for a `ridge`.
- `inset` and `outset`: the colour is darkened on two sides and lightened on the
  other two, with Blink's formula, so `outset` has the colour at the top and left
  and a darker shade at the bottom and right, and `inset` the reverse. A colour
  that is already as dark as it gets is not darkened, and black and white take
  Chrome's fixed greys. The transparency of the colour is kept.
- `none` and `hidden` take the side's width, whatever `borderWidth` says: it is
  0 and nothing is drawn. A side no declaration gives a style is `none` too, and
  so is `border: "2px red"`.

Where two sides with different styles meet at a corner, each style is drawn up
to the line that already separates their colours. The pattern of a dotted or
dashed side follows the border's width on that side.

#### Rounded corners

`borderRadius` takes a number (px) or the CSS string: one to four [lengths](#lengths)
or percentages for top-left, top-right, bottom-right and bottom-left (the `margin`
expansion), then optionally `/` and one to four vertical radii. The four corner
longhands (`borderTopLeftRadius` ...) take a number or `"<horizontal>
[<vertical>]"` and win over the shorthand when written after it.

```tsx
<div style={{ borderRadius: 8 }} />
<div style={{ borderRadius: '10px 20px 30px 40px' }} />
<div style={{ borderRadius: '50%' }} />                    {/* an ellipse on a wide box */}
<div style={{ borderRadius: '10px 20px / 5px' }} />
<div style={{ borderRadius: '50% / 10px 20px' }} />
<div style={{ borderTopLeftRadius: '50% 20px' }} />
<div style={{ borderRadius: '0.5em' }} />                   {/* em, rem, vw, vh, vmin and vmax work */}
<div style={{ borderRadius: 'calc(1em + 10%)' }} />         {/* a length and a percentage in one */}
```

A percentage is of the border box, the width for a horizontal radius and the
height for a vertical one, and is resolved when the box is laid out. A single
percentage rounds both axes by it, so `"50%"` on a 200x100 box is an ellipse;
`"50px"` is a circle. A zero in either radius makes the corner square, and radii
that add up to more than a side are scaled down together (CSS Backgrounds 3
section 5.5). Lengths are any [length](#lengths), including a `calc()` that mixes a
length and a percentage: the length part resolves against the element's font size,
the root's and the window when the style is applied, and the percentage stays per
axis. A radius that is negative whatever the units are, or invalid, in a state style
too, throws instead of being ignored; one that only comes out negative
(`calc(1em - 20px)`) is zero. The radius cuts the background,
the border, the shadow, the `overflow` clip and the hit area. The `round` of
`clipPath` `inset()`, `rect()` and `xywh()` takes the same `a / b` form.

A radius in `motion` is still a number: it animates a circular radius in px, and
a style that sets a percentage or elliptical corner has no number to start from,
so that animation starts at `0`.

### Layout like CSS

- **Text nodes.** An empty text node produces no box, and neither does a
  white-space-only one that sits directly in a `div`, so `{" "}` and the empty
  placeholders Solid creates for `<Show>` and `<For>` no longer add a flex or
  grid item (and a `gap`). White space inside text is processed as in CSS, see
  [White space](#white-space).
- **`display: "none"`** builds no box for the element or its descendants, and
  leaves no gap. **`display: "contents"`** also builds no box: its children are
  spliced, recursively, into the parent's child list, so they are the parent's
  flex or grid items (and are sorted by `order` with them). The element's own box
  styles (size, padding, background, border) do not apply, its inheritable text
  style (`color`, `fontSize`, `fontFamily`, `fontWeight`, `textAlign`,
  `lineHeight`, `whiteSpace`, `tabSize`, `cursor`, `visibility`, `userSelect`) passes to the
  children, as it shows in each frame: a `motion`, a CSS transition or a keyframes
  animation of one of those on the element moves the children's text, and one of a
  property that needs a box (`opacity`, `backgroundColor`, a size) does nothing. `getElementBounds` returns `null` for it. As a root, or as a
  `<virtual-list>` row (a list lays out one measured item per row), it has no
  parent to splice into, so its children lay out in an anonymous block in its
  place. Its own listeners still hear every event that passes through it, with
  `currentTarget` set to it: a click on a child, the pointer entering or leaving
  any of its descendants (`mouseenter` and `pointerenter` fire once for the
  whole group), keys and focus inside it, and `click()` on it. It is never the
  target of a pointer event, as it has no box to hit, and it is not focusable
  (Chrome skips it too, `tabIndex` or not; its children keep their own Tab
  stops). `getBoundingClientRect()` is a zero rect and an `IntersectionObserver`
  reports it as not intersecting with a zero rect, as in Chrome. As a root or a
  `<virtual-list>` row it keeps its `role` and `aria-*` in the accessibility tree
  around its children, and `aria-hidden` hides them; spliced into a parent it has
  no element to carry a node, so they are ignored there (Chrome keeps the node).
  `inline-block`, `inline-flex`
  and `inline-grid` are [atomic inlines](#inline-block-inline-flex-inline-grid-and-inline-table);
  `inline-table` is an atomic inline whose inside is a `table` (see [Tables](#tables)).
- **`visibility: "hidden"`** keeps the layout box and every report of it
  (`getBoundingClientRect`, `onLayout`, `ResizeObserver`, `IntersectionObserver`),
  and paints nothing of its own: no background, border, shadow, outline or text.
  The value inherits, so the descendants are hidden too until one sets
  `visibility: "visible"`, which paints on its own box. A hidden element is never
  the target of the pointer (events go to what lies under it, with no hover), cannot
  be focused (Tab skips it, `focus()` is ignored and a focused element blurs when it
  hides), has no accessibility node, and is left out of `getElementsAt`.
  A `visible` descendant is all of those again, and an event on it still bubbles
  through the hidden ancestors.
- **Static position.** An unpositioned box ignores `top`/`left` and is not a
  containing block. As in CSS, an `absolute` box is clipped and scrolled only by
  the ancestors at or inside its containing block, so a static
  `overflow: "hidden"` card or scroller between them does neither, and a `fixed`
  box is clipped by the window alone and scrolls with nothing. Give the card a
  `position` to make it the containing block and clip its dropdown. A
  `position` that only a state style sets (`hover`, `active`, `focusVisible`,
  `focusWithin`, the group variants) escapes the same way while the state
  applies, and a state that sets it back to `static` puts the clip back.
  `getElementsAt`, `scrollIntoView` and `IntersectionObserver` follow the position as painted,
  which includes one an `@container` block, a transition or a motion value sets. An `opacity`,
  `filter`, `mask` or `clipPath` group composites an escaped box with the rest
  of the group, so a card with `opacity` on a clipped scroller no longer cuts
  its dropdown at the scroller's edge. Known difference: a `filter` also lets
  what it spreads of the card past that edge.
- **`<img>`** is a replaced element. A missing dimension comes from the other
  one and the image's intrinsic ratio (or `aspectRatio`), a percentage of an
  indefinite parent counts as `auto`, and with neither set it takes its
  intrinsic size. `maxWidth` keeps the proportions.
- **Paint order and stacking contexts.** A stacking context paints its own
  background, then the lifted descendants with a negative `zIndex`, then its
  in-flow content in tree order, then the other lifted descendants by
  `zIndex` (`auto` is 0) and tree order. A lifted element belongs to the nearest
  stacking context around it, whatever static boxes sit between, so a `zIndex`
  on a card's descendant can reach over the next sibling of the card. Hit
  testing follows paint order, and tab order stays in document order. Lifted
  are: a positioned element (any `position` but `static`); a flex or grid item
  with a `zIndex`, whatever its `position` (`zIndex` applies to an item without
  one, but not to any other static box, and `auto` leaves it in tree order);
  and an element composited as a whole (`opacity` below 1, `filter`,
  `backdropFilter`, `maskImage`, `clipPath`), which paints in the 0 layer with
  the positioned `auto` and 0 elements, in tree order, though it is not
  positioned (it takes its `zIndex` where one applies). Layout is unaffected.
  These form a context: the window root, a positioned element with a `zIndex`
  (not `auto`), `position: fixed` and `sticky`, a flex or grid item with a
  `zIndex` (not `auto`), and anything composited as a whole. A `zIndex` inside
  one cannot rise above what is outside it. An `<anchored>` layer and each row
  of a `<virtual-list>` is a context of its own too, and nothing is lifted out
  of a row: a row's content never rises above the next row. The children of a
  `display: contents` element are items of the container around it, and so are
  the children of an `<anchored>` (see [Overlay menus](#overlay-menus)). The
  other custom elements (`<input>`, `<textarea>`, `<img>`, `<svg>`, `<code>`,
  `<diff>`, `<markdown>`, `<virtual-list>`) draw themselves and do not lay
  their children out as a flex or grid container, so a child of one is no item
  and takes no `zIndex` without a `position`.
  There is no `transform`. `getElementsAt` and the stacking contexts it models
  read the style an element was last painted with, so an `opacity`, `zIndex`,
  `visibility`, `pointerEvents`, `overflow` or `position` that comes from a
  state style, an `@container` block, a transition, a CSS animation or a motion
  value moves the element between layers there, as in a browser. An element
  that was never painted (a virtual list row outside the window) has its declared
  style. Known difference: Chrome gives an element a stacking context for as
  long as an `opacity` animation or transition is on it, even at the instant the
  `opacity` is still 1; gpuix does once it is below 1.
- **`order`** stable-sorts the children of a flex or grid container. Layout and
  paint follow it; tab order stays in document order.
- **Baselines.** Text has a first baseline: that of its first line. A flex or grid
  item with `alignItems` or `alignSelf: "baseline"`, a `baseline`-aligned
  [table cell](#tables) and an [`inline-flex`, `inline-grid` or `inline-table`](#inline-block-inline-flex-inline-grid-and-inline-table)
  on its line all line up by their text, as in a browser, and a box with no text
  uses the bottom of its border box. A flex or grid container's own baseline is its
  first item's.

#### Grid

`gridTemplateColumns` and `gridTemplateRows` take a CSS track list: any
[length](#lengths) (`px`, `%`, `em`, `rem`, `vw`, `calc()`, ...), `fr` units, `auto`, `min-content`, `max-content`, `minmax()`,
`fit-content()`, `repeat(n | auto-fill | auto-fit, ...)`, `[line names]` and
`none`. `fit-content()` takes a length or a `%`, not a `calc()` that mixes the
two. `gridAutoColumns` and `gridAutoRows` take the same track sizes. A bare number `n` still means `repeat(n, minmax(0, 1fr))` (with
`gridColumnMin` / `gridRowMin` to pick the minimum). A track list that does not
parse makes the batch throw, like any other invalid style value.

```tsx
<div style={{
  display: 'grid',
  gridTemplateColumns: '[sidebar] 240px [main] repeat(auto-fill, minmax(120px, 1fr))',
  gridTemplateAreas: '"head head" "side main"',
  gridAutoRows: 'minmax(40px, auto)',
}}>
  <div style={{ gridArea: 'head' }} />
  <div style={{ gridColumn: 'sidebar / span 2', gridRow: '2' }} />
</div>
```

Also: `gridAutoColumns`, `gridAutoRows`, `gridAutoFlow` (`"row"`, `"column"`,
`"dense"`), `gridTemplateAreas` (quoted rows, `.` for an empty cell),
`gridArea`, `gridColumn`, `gridRow`, `gridColumnStart/End`, `gridRowStart/End`
(a line number, negative counted from the end, `"span n"`, a line or area name),
`justifyItems` and `justifySelf`.

#### Tables

`display` takes the CSS table values: `table`, `table-row-group`,
`table-header-group`, `table-footer-group`, `table-row`, `table-cell`,
`table-caption`, `table-column` and `table-column-group`. They are the layout of
CSS Tables 3 as Chrome draws it, on any `div`, `text` or `span`:

```tsx
<div style={{ display: 'table', borderSpacing: 4 }}>
  <div style={{ display: 'table-caption' }}>Totals</div>
  <div style={{ display: 'table-row' }}>
    <div style={{ display: 'table-cell' }}>Rent</div>
    <div style={{ display: 'table-cell', verticalAlign: 'top' }} rowSpan={2}>
      1,200
    </div>
  </div>
  <div style={{ display: 'table-row' }}>
    <div style={{ display: 'table-cell' }}>Food</div>
  </div>
</div>
```

- **`inline-table`.** `display: 'inline-table'` is a table that sits on a line of text as one unit, as an
  [atomic inline](#inline-block-inline-flex-inline-grid-and-inline-table) does: it shrinks to fit (a `width` in `%` is
  of the paragraph, and it is never narrower than its columns' least widths), wraps with the text around it and
  sits on the baseline of its first row. Where the parent lays out items it is a `table`.
- **Props.** `colSpan` and `rowSpan` on a `table-cell`, `span` on a `table-column`
  or `table-column-group`, as the DOM names them. A number or a string of digits;
  `colSpan` and `span` are at least 1 and at most 1000, a `rowSpan` of `0` reaches
  the end of its row group and the most is 65534. Anything else is `1`.
- **Styles.** `borderSpacing` is one length for both axes or two, horizontal then
  vertical (`"4px 8px"`, `2`, `"1em"`; any [length](#lengths) but a `%`, and
  nothing negative). `tableLayout` is `"auto"` or `"fixed"`: a fixed table takes its
  columns from its `table-column`s and the cells of its first row, and needs a
  `width`, else it is auto. `captionSide` is `"top"` or `"bottom"`. `emptyCells`
  `"hide"` leaves a cell with nothing in it, or only white space and `display:
  none`, without its background, border and shadow. `borderSpacing`, `captionSide`,
  `borderCollapse` and `emptyCells` inherit as in CSS (a table inside a cell has
  the spacing of the table around it), `tableLayout` does not. `verticalAlign` on
  a `table-cell` is `"top"`, `"middle"`, `"bottom"` or `"baseline"` (the default);
  any other value is `"baseline"` there. An invalid value throws.
- **Layout.** The table is shrink-to-fit, or `max(width, least width of the
  columns)` with a `width`. Column widths come from the cells' content, their
  `width` (px or `%`) and the `table-column`s'; columns no cell starts in take no
  width; a spanning cell's extra width is shared out as Chrome does. Rows are as
  tall as their tallest cell, a spanning cell's extra height is shared between its
  rows, and a table with a `height` grows its body rows, not the header and footer
  groups. A `table-header-group` goes before the other row groups and a
  `table-footer-group` after them, wherever they are in the source (focus and
  selection keep the source order). `table-caption`s go above or below the grid and
  are as wide as the table. `border` and `padding` of the table wrap the grid
  alone, and the cell spacing goes around and between the cells. A table is a
  block-level box in flex and grid as it is in CSS.
- **Painting.** The table's background, border, `boxShadow` and `outline` surround
  its grid, not its captions, and `getBoundingClientRect` of the table includes
  them (Chrome draws and measures it the same). `opacity`, `filter`, `mask` and
  `clipPath` apply to the whole box.
- **Anonymous cells.** Consecutive children of a table, a row group or a row that
  are not table parts (a block, a `span`, text that is not white space only) are
  wrapped in one anonymous cell, as in CSS 2.1 section 17.2.1, so they stack in one
  cell. The cell has no element, no style and no id: it inherits the text style,
  and an event on its content bubbles to the row or table. Cells directly in a
  table or a row group form an implicit row. White space between parts makes
  nothing, and neither does a `position: absolute` or `fixed` child, which the
  table places by its insets. Children of a `table-column` are not built, and a
  `table-column-group` builds only its `table-column` children.
- **Anonymous tables.** A row, row group, cell, caption, column or column group whose
  parent is not a table (a plain `div`, a `table-cell`) gets an anonymous table around
  it, one for each run of consecutive ones, as in CSS 2.1 section 17.2.1: bare cells
  form a row, the table shrinks to its content, and white space between the parts
  makes nothing. Text or a block ends the run. The table has no element or style of its
  own and inherits `borderSpacing` and `borderCollapse` from its parent. In a `flex` or
  `grid` container, or with `position: absolute` or `fixed`, a part is a plain item and
  is not wrapped.
- **Collapsed borders.** With `borderCollapse: "collapse"` (inherited) the borders of
  the table, its cells, rows, row groups, columns and column groups share the edges of
  the grid, and each edge has one: `hidden` removes it, otherwise the wider wins, then
  the more elaborate style (`double`, `solid`, `dashed`, `dotted`, `ridge`, `outset`,
  `groove`, `inset`; `none` loses to any), then the closer box (cell, row, row group,
  column, column group, table), then the one on the left or above (CSS 2.1 section
  17.6.2.1; the one on the right in a `direction: rtl` table, where the first column is
  the rightmost). A border is centred on its grid line: each cell and the table get half of
  the widest border along each side as their border width, exactly (a 3px border leaves
  1.5px on each side, as in Chrome), so the table's border box holds the outer half of
  its outer borders, and `borderSpacing`, the table's `padding` and `borderRadius`, and
  `emptyCells` do nothing. Cells, rows, groups and the table paint none of their own
  borders; the table paints each edge once, after its cells, so a border covers their
  backgrounds, in the border's colour and style, and where borders cross the strongest
  covers the others and the others run under it. Rows, groups and columns paint no
  border at all when the borders are separate. A part's border is the one it is painted with, so a
  transition, a CSS animation, `motion` or a state style on the border of a cell, row, group or column
  moves its neighbours' share of the edge every frame. The parts are built after the table. Which
  border wins an edge and how wide it is are settled when the table is built, and the colour of an
  edge is the one its owner is painted with in the frame, so a `border-color` transition on a part
  is drawn in step, builds nothing else (the table's siblings are replayed) and is a patched tick
  with the table as its unit. A table that finds a part painted with another width or style than it
  shared the edges by is built again before the frame is presented.
- **Copying.** A selection across cells copies as `getSelection().toString()` does in Chrome:
  a tab between the cells of a row and a line break between rows and between the blocks of
  one cell (the cells of an anonymous table and the anonymous cells included).
- **Parts are elements.** Cells, rows, groups and captions take events, `hover`
  and `active` styles, focus (`tabIndex`), `overflow`, `zIndex`, transitions and
  motion, `ResizeObserver`, `getBoundingClientRect`, selection and search like any
  box. A row or group paints its own background below its cells'.

Differences from Chrome:

- A column no cell starts in joins the one before it in the layout, and the collapsed borders
  predict that: a table with `tableLayout: "fixed"` and a `width` keeps them, so one whose
  `width` is a percentage of a parent with no width draws no collapsed borders.
- Rows and cells with `rowSpan`s that leave rows without a cell of their own and no height
  can share a spanning cell's height out differently.
- A table that is a flex item narrower than its least width reports that least width but is
  positioned for the width it asked for; the maximum width of a table with columns that add up
  to 100% next to columns without a percentage is what the columns ask for, where Chrome
  makes it unbounded.
- An absolutely positioned child of a table is placed by its insets: `margin: auto` does
  not centre it.
- Sums are floats, where Chrome counts in 1/64 of a pixel (a percentage is cut to it, a share is not), so widths
  shared out can differ in the second decimal, and a row that takes the rest of the table's height is a 64th
  short; the painted box is snapped to the device pixel.
- `mask` and `clipPath` reference the whole box, with the captions.
- An element that paints nothing (`visibility: hidden`) counts as content in a cell for
  `emptyCells`.
- Copying a selection across cells puts a tab between the cells of a row and a line break
  between rows, as Chrome does, but after a cell that ends in a block (a `div` of its own,
  not text) Chrome drops the tab and this does not.
- The table styles are not animatable by `motion` or a keyframe, which take no discrete property.
  `borderSpacing`, `borderCollapse`, `captionSide` and `emptyCells` that an element leaves unset start
  from the value it inherits, in a CSS transition as in Chrome.
- A `table-*` display on a custom element (`<img>`, `<input>`, `<svg>`, ...) lays out as a
  block with no table role.

#### Direction

`direction: 'rtl'` (inherited, `'ltr'` by default) makes the inline axis run right to left, as in CSS.
Everything that lays out along it mirrors: the main axis of a `row` flex container (and what
`justifyContent`, `alignItems` and `marginLeft: 'auto'` do on it), the columns of a grid and where an item
sits in its area, the columns of a table (the first column is the rightmost, a cell that spans columns is where
they are, the border of a `collapse` table that is on the end of a cell is its left one), a block narrower than
its parent, which sits at the right edge, a `position: absolute` box with no `left` or `right`, and the side a
vertical scrollbar takes. Physical properties do not swap: `paddingLeft`, `borderRight`, `left` stay where they
say. The logical ones follow the direction: the start of `marginInline`, `paddingInline` and `insetInline` is the
right in an rtl element (the left in one that goes back to `ltr`), also for a `<span>`, which takes the
direction of its paragraph's container.

```tsx
<div style={{ direction: 'rtl', width: 300 }}>
  <div style={{ display: 'flex' }}>
    <div style={{ width: 50 }}>first, at the right edge</div>
    <div style={{ width: 60 }}>second, to its left</div>
  </div>
  <div style={{ textAlign: 'start' }}>starts at the right</div>
</div>
```

`textAlign`'s `start` (the default) and `end` are the right and the left of the element's own `direction`, and a
child that declares another `direction` gets its own side, as it does in CSS. Positions and hit tests of text,
which the selection, the highlights and the bounds of a `<span>` are made of, follow `textAlign`
(`'center'` and `'right'` included).

Differences from Chrome:

- The writing direction of a paragraph is the one CoreText finds in its text (the first strong character), not
  the `direction` of the element, so a Latin sentence in an rtl block puts its punctuation as it does in an ltr
  one, and the order of mixed-direction text is CoreText's. Wrapping and hit tests inside a right-to-left run are
  approximate.
- `direction` is not animatable.

### Cursors

`cursor` takes the CSS keyword. An unlisted keyword is ignored, like any other
invalid style value.

| Group | Keywords |
|---|---|
| Pointing | `default`, `auto`, `pointer`, `context-menu`, `not-allowed`, `no-drop` |
| Text | `text`, `vertical-text`, `crosshair` |
| Dragging | `grab`, `grabbing`, `move`, `all-scroll`, `alias`, `copy` |
| Hidden | `none` |
| Resizing | `col-resize`, `row-resize`, `ew-resize`, `ns-resize`, `nwse-resize`, `nesw-resize`, `n-resize`, `e-resize`, `s-resize`, `w-resize`, `ne-resize`, `nw-resize`, `se-resize`, `sw-resize` |

```tsx
<div style={{ cursor: 'grab', active: { cursor: 'grabbing' } }} />
<div style={{ cursor: 'col-resize' }} />
<div style={{ cursor: 'none' }} />
```

`cursor: 'none'` hides the cursor while it is over the element, as in CSS. It is
implemented on macOS. The Windows backend hides it too but has not been
tested; on Linux (X11 and Wayland) the default cursor stays visible. On macOS the
hide belongs to its window: it ends when the pointer leaves the window, the
window stops being the key window or it closes, and it never hides the cursor
over another window.

### Colors

Every color-bearing style field accepts the same string grammar. GPUIX native
uses `csscolorparser` 0.8.3 and accepts:

- named colors and `transparent`;
- 3/4/6/8-digit hex, with or without `#`;
- `rgb()` / `rgba()`, `hsl()` / `hsla()`, `hwb()` / `hwba()`, and
  `hsv()` / `hsva()`;
- `lab()`, `lch()`, `oklab()`, and `oklch()`;
- `none` components and the parser's limited relative-color `from` / `calc()`
  forms.

Standard comma and modern space/slash alpha forms work. Values are converted
to hard-clipped sRGB before GPUI paints them. Invalid strings are ignored for
that property; they do not reject the full style object.

### Gradients and background layers

`backgroundImage` takes a comma list of CSS gradients and [`url()`
images](#image-backgrounds), the first on top: `linear-gradient()`,
`radial-gradient()`, `conic-gradient()` and their `repeating-` forms, with any
number of stops, colour hints, `in srgb` / `in oklab`, and `none`. Colours are
any [colour form](#colors), including `currentColor`. `backgroundSize`, `backgroundPosition`, `backgroundRepeat`
(including `space` and `round`), `backgroundOrigin` and `backgroundClip` are comma
lists that repeat to cover the layers, as in CSS. `background` is the shorthand:
layers, `position / size`, repeat, origin and clip boxes, and a final colour.

```tsx
<div
  style={{
    backgroundColor: '#0d1117',
    backgroundImage:
      'radial-gradient(circle at 20% 20%, #ffffff33, transparent 40%), linear-gradient(to right in oklab, #7c3aed, #06b6d4)',
    backgroundSize: '100% 100%, 24px 24px',
    backgroundRepeat: 'no-repeat, repeat',
    borderRadius: 12,
  }}
/>

<div style={{ background: 'repeating-linear-gradient(45deg, #0001 0 8px, transparent 8px 16px) #fff' }} />
```

A longhand written after the shorthand wins over it. Limits:
- `image-set()` is valid but not drawn: that layer is skipped and the others still
  paint.
- Interpolation spaces other than `srgb` and `oklab` skip the layer.
- `backgroundOrigin` and `backgroundClip` move the layers only. The background
  *colour* fills the border box, or the glyphs under `text` (below).
- `backgroundAttachment` has no meaning here and is ignored in the shorthand.

#### Image backgrounds

`url()` is a layer like a gradient: in `backgroundImage` or anywhere in a
`background` layer, mixed with gradients, with the same `position / size`, repeat,
origin and clip. The address can be bare, `"double"` or `'single'` quoted (a
quoted one can hold `)`, `,` and spaces, and `\"` escapes), with space inside the
parentheses. A bare `url(a.png)` layer takes the CSS initial values.

```tsx
<div style={{ backgroundImage: 'url(/assets/dots.png)', backgroundSize: '16px', backgroundRepeat: 'round' }} />
<div style={{ background: 'url("hero.jpg") center / cover no-repeat, linear-gradient(#0008, #0000)' }} />
<div style={{ backgroundImage: `url(${URL.createObjectURL(blob)})` }} />
```

The address is read as an [`<img>` `src`](#img) is, and loads the same formats: a
path (relative ones to the process's working directory, there is no document base
URL), a `file://` URL, `data:`, `http:` / `https:`, and `blob:` (see below). The
image's own size is its natural size in CSS px (an SVG's `width` and `height`),
which `auto`, `cover`, `contain` and `round` work from. An animated image draws its
first frame.

A layer paints nothing until its image has loaded, and nothing if it cannot (a
missing file, a failed request, bad data; the reason is logged once as a warning),
while the other layers and the colour paint. A `data:` address is decoded when the
element is built, so it paints in the first frame; a path, `file:` and `http:`
image paints from the frame after its load ends. The address is the identity: a
new one loads the new image, and an element built again with the same one shares
it. An address in a state style (`hover`) is read when that state first applies.
At most 128 images are kept; the ones not asked for longest are dropped and loaded
again if a layer wants them. A layer draws at most 4096 tiles, so a tiny image
repeated over a huge box stops early.

`blob:` URLs only exist in the JS process, so `URL.createObjectURL(...)` is
fetched there once per URL, wherever a style, its state styles included, names
it, and the bytes are handed to native. A revoked URL paints nothing. This needs a
renderer that has `setStyleImage`: the native one and the test renderer do.

Limits: `image-set()` is not drawn. An inline `<span>` box paints
`backgroundColor` only. A `url()` in `mask` and `maskImage` loads the same way
(see `maskImage`, below).

#### Gradient text

`backgroundClip: 'text'` (or `text` in the `background` shorthand) paints a layer
(an `url()` image too) only through the glyphs of the element and its descendants,
nested inline spans included. The gradient is laid out on the element's box, as in CSS, so it spans
the whole block even when the text is shorter. The background colour follows the
last layer's clip, so with `text` there it is confined to the glyphs too. Set
`color: 'transparent'` to hide the text's own fill.

```tsx
<div
  style={{
    backgroundImage: 'linear-gradient(to right, #f43f5e, #6366f1)',
    backgroundClip: 'text',
    color: 'transparent',
  }}
>
  <text style={{ fontSize: 48, fontWeight: 800 }}>Gradient text</text>
</div>
```

A box with no text paints nothing for a `text`-clipped layer. A `background`
given as the object-form gradient is not clipped.

#### The object form

`background` also accepts GPUI's native **two-stop linear gradient** object.
Angles follow CSS: `0` points up and values increase clockwise. Stop positions
use `0` to `1`.

```tsx
<div
  style={{
    background: {
      type: 'linear-gradient',
      angle: 90,
      stops: [
        { color: '#7c3aed', position: 0 },
        { color: '#06b6d4', position: 1 },
      ],
      colorSpace: 'oklab',
    },
    borderRadius: 12,
  }}
/>
```

`colorSpace` is optional and defaults to `"srgb"`. GPUI also supports
`"oklab"`. For radial, conic, repeating and many-stop gradients use
`backgroundImage`.

`hsv()`, `hsva()`, and `hwba()` are parser extensions rather than CSS Color 4
standard functions. Platform/dynamic colors and numeric color integers are not
accepted.

Every colour prop (`color`, `backgroundColor`, gradient stops, `borderColor`,
`outlineColor`, `boxShadow` colours, `scrollbarColor`, `selectionColor`, the
tint of an `<svg>`) also takes the CSS forms that depend on context. They nest
freely:

| Form | Meaning |
|---|---|
| `currentColor` | The element's computed `color`, inherited like in CSS. The default `color` is black under a light `colorScheme` and white under a dark one (CSS `CanvasText`). |
| `light-dark(a, b)` | `a` under a light `colorScheme`, `b` under a dark one. |
| `color-mix(in oklab, a 30%, b)` | Mixes in `srgb`, `srgb-linear`, `display-p3`, `a98-rgb`, `prophoto-rgb`, `rec2020`, `xyz`, `xyz-d50`, `xyz-d65`, `lab`, `lch`, `oklab`, `oklch`, `hsl` or `hwb`. Polar spaces take `shorter` (default), `longer`, `increasing` or `decreasing` `hue`. |
| `color(display-p3 1 0 0)` | The same RGB and XYZ spaces, converted to sRGB and clipped to its gamut. |

`colorScheme` is inherited and takes the CSS values: `"normal"` (light, the
default), `"light"`, `"dark"`, `"light dark"` (follows the window appearance, see
[`getAppearance()`](#appearance) and updates when it changes) and `"only light"` /
`"only dark"`. It picks the side of `light-dark()`, and it sets the default text
colour like CSS `CanvasText`: black under a light scheme and white under a dark
one, until an element declares a `color`. A window with no `colorScheme` is
light, so its default text stays black even when the system is dark.

```tsx
<div
  style={{
    colorScheme: 'light dark',
    color: 'light-dark(#1f2328, #e6edf3)',
    borderWidth: 1,
    borderStyle: 'solid',
    borderColor: 'color-mix(in srgb, currentColor 20%, transparent)',
    backgroundColor: 'light-dark(#fff, #0d1117)',
  }}
/>
```

A `hover` or `active` style that sets `color` changes `currentColor` for that
element's own colours, but not for its descendants: they are built before the
pointer state is known.

Theme values can use the same modern grammar:

```tsx
const theme = {
  surface: 'oklch(18% 0.02 260)',
  accent: 'oklch(67.3% 0.182 276.935)',
  text: 'oklch(96% 0 0)',
}

<div style={{ backgroundColor: theme.surface, borderColor: theme.accent }}>
  <text style={{ color: theme.text }}>Hello GPUIX!</text>
</div>
```

Limited relative-color forms can derive a new color from a base value:

```tsx
<div
  style={{
    backgroundColor: '#bad455',
    borderColor: 'oklch(from #bad455 calc(l - 0.15) calc(c * 0.7) h)',
  }}
/>
```

`boxShadow` accepts the CSS string, one structured shadow, or an array of them,
painted in the order given. The string is parsed into the same layers:
lengths are any [length](#lengths) without a `%` (or a bare `0`), `inset` and the colour may lead or trail the
lengths, the colour can be any [colour form](#colors) and defaults to
`currentColor`, and `none` paints nothing. An invalid string paints no shadow.

```tsx
<div style={{ boxShadow: '0 1px 2px rgba(0,0,0,.2), inset 0 0 0 1px red' }} />
```

The structured form's fields are `offsetX`, `offsetY`, `blurRadius`, `spreadRadius`,
`color`, and `inset`. As in CSS, a shadow that is not `inset` is only painted outside
the border box, so a translucent `backgroundColor` shows the page behind the element
and not the shadow, and an `inset` shadow is only painted inside the padding box,
never over the border:

```tsx
<div
  style={{
    boxShadow: {
      offsetX: 0,
      offsetY: 4,
      blurRadius: 12,
      spreadRadius: 0,
      color: '#00000033',
    },
  }}
/>

<div
  style={{
    boxShadow: [
      { offsetX: 0, offsetY: 1, blurRadius: 2, spreadRadius: 0, color: '#0000004d' },
      { offsetX: 0, offsetY: 0, blurRadius: 0, spreadRadius: 1, color: '#ffffff1a', inset: true },
    ],
  }}
/>
```

`filter`, `backdropFilter`, `mask`, `clipPath` and an `opacity` below 1 on an element
with more than one painted part are drawn as an offscreen group, which only the
macOS (Metal) renderer has. On Windows and Linux the subtree paints flat and only
`opacity` applies.

`filter` and `backdropFilter` take the CSS function list: `blur()`, `brightness()`,
`contrast()`, `grayscale()`, `hue-rotate()`, `invert()`, `opacity()`, `saturate()`,
`sepia()` and `drop-shadow()`, applied in order, or `none`. Amounts are numbers or
percentages, lengths are any [length](#lengths) without a `%` (or a bare `0`), and angles are `deg`, `rad`, `turn`
or `grad`. A `drop-shadow()` colour can be any [colour form](#colors) and defaults
to `currentColor`. An invalid string is ignored, as in CSS, and `url()` references
are not supported. `filter` applies to the element and everything in it, and
`backdropFilter` to what is painted behind it, so give the element a translucent
background for the effect to show. `opacity` also composites the element as a
group, so overlapping children of a half-transparent parent do not show through
each other.

An `opacity` of `0` paints nothing, in the frame that builds it and in one that replays
it, and costs a hover frame nothing while it does not change: a design system that keeps
an `opacity: 0` copy of each label to hold its width pays for layout and not for paint. The
element is still laid out and has its box, `getBoundingClientRect()` reads it, it is hit
tested, its listeners and cursor run, and its text is selectable and found by find, as in
Chrome. A visible element inside it stays hidden, and an element after it shows as usual.

```tsx
<div style={{ filter: 'blur(4px) brightness(1.2)' }} />
<div style={{ backdropFilter: 'blur(12px)', backgroundColor: 'rgba(255,255,255,0.4)' }} />
<div style={{ filter: 'drop-shadow(2px 4px 6px rgba(0,0,0,.4))' }} />
```

`maskImage` multiplies the element and everything in it by a comma list of
layers, first on top: gradients (the same forms as `backgroundImage`) and `url()`
images. With the default `maskMode` a layer's alpha is what counts, so `transparent`
hides and any opaque colour shows. `maskSize`, `maskPosition`,
`maskRepeat`, `maskOrigin` and `maskClip` are comma lists that repeat to cover the
layers, and `mask` is the shorthand (layers, `position / size`, repeat, origin
and clip boxes, a mode and a composite keyword). A longhand written after it wins over it.
`maskOrigin` and `maskClip` start at
`border-box`, not at the `padding-box` a background starts from. A layer that does
not cover an area leaves it hidden, so a non-repeating mask hides what it does not
reach. A mask composites the element as a group, like `opacity` and `filter`.

```tsx
// Fade the top and bottom edge of a scroller.
<div style={{ overflow: 'scroll', maskImage: 'linear-gradient(to bottom, transparent, black 20px, black 80%, transparent)' }} />
<div style={{ mask: 'radial-gradient(circle, black 60%, transparent 61%) center / 24px 24px' }} />
<div style={{ mask: 'url(/assets/blob.svg) center / contain no-repeat' }} />
```

A `url()` mask layer is written and loaded like a [`url()` background](#image-backgrounds):
the same quoting, the same sources (a path, `file://`, `data:`, `http(s):` and
`blob:`), and the same natural size, tiling and caching. Until its image has loaded
a layer is transparent, and it stays transparent if the load fails (a missing file,
bad data), as in CSS. So an element masked by an image that is not there is hidden
whole, not drawn unmasked; with several layers the missing one combines like any
other empty layer. A `data:` image is ready in the first frame, so a mask does not
flash.

`maskMode` is a comma list of `alpha`, `luminance` and `match-source` (the
default), one entry per layer. `alpha` reads a layer's alpha channel, `luminance`
its luminance scaled by its alpha, so black hides and white shows. `match-source`
is `alpha` for a gradient and an image alike.

`maskComposite` is a comma list of `add` (the default), `subtract`, `intersect` and
`exclude`, one entry per layer: how that layer combines with all the layers below
it. `add` lays it over them, `subtract` keeps it only where they are not,
`intersect` only where they are, and `exclude` where exactly one of the two is. The
bottom layer has nothing below it, so its entry is ignored. Both lists repeat to
cover the layers, and a list with an invalid entry is ignored, like the other mask
longhands. The `mask` shorthand takes a mode and a composite keyword in any layer,
in any order among the other parts, and resets both longhands.

```tsx
// Punch the shape of an icon out of a rectangle.
<div style={{ maskImage: 'linear-gradient(#000, #000), url(/assets/icon.svg)', maskComposite: 'exclude' }} />
<div style={{ mask: 'url(/assets/photo-mask.png) luminance center / cover no-repeat' }} />
```

`maskBorder` draws a nine-slice of an image around the border box and multiplies
the element by it, like another mask layer: the top one, which adds to the
`maskImage` layers, so the element shows where either lets it through. It is for
frames, ticket edges and torn-paper borders that a gradient cannot draw.

- `maskBorderSource`: one gradient or `url()` image, or `none`. A gradient has no
  size of its own, so it is as large as the border image area, and `maskBorderSlice`
  counts pixels of that. An image that has not loaded masks the element out, like a
  `maskImage` layer.
- `maskBorderSlice`: one to four numbers (pixels of the image) or percentages, in the
  order of `margin`, that cut the image into nine pieces. The corners go to the corners
  and the edges to the edges, and the middle is drawn only if `fill` comes before or
  after the numbers. It starts at `0`: a source alone draws nothing. A number past the
  image is the image.
- `maskBorderWidth`: one to four lengths, percentages of the border image area,
  numbers (times the element's border width on that side) or `auto`, which is the size
  of the slice and what it starts as. Edges that together are wider than the area all
  shrink by the same factor.
- `maskBorderOutset`: one to four lengths or numbers (times the border width) that grow
  the border image area past the border box. The element is hidden outside the area,
  so a child that sticks out past the outset is cut off.
- `maskBorderRepeat`: one or two of `stretch` (the default), `repeat`, `round` and
  `space`, for the top and bottom edges and the left and right ones. `repeat` tiles an
  edge with whole slices at the size the edge's width scales them to, centred, and cuts
  the ones at the ends. `round` rescales them to a whole number that fits. `space` keeps
  their size and divides what is left into equal gaps around and between them, and draws
  nothing if not even one fits. The middle repeats the same way, scaled like the top
  edge across and the left edge down.
- `maskBorderMode`: `alpha` (the default) reads the image's alpha channel, `luminance`
  its luminance scaled by its alpha.
- `maskBorder` is the shorthand, `<source> || <slice> [ / <width>? [ / <outset> ]? ]? ||
  <repeat> || <mode>`, and resets what it leaves out. A longhand written after it wins
  over it, as with `mask`. The `mask` shorthand resets `maskBorder` and its longhands, as CSS Masking says
  (`{ maskBorder: ..., mask: 'none' }` has no border, `{ mask: 'none', maskBorder: ... }` has one); Chrome,
  which only has `-webkit-mask-box-image`, leaves that alone.

```tsx
<div style={{ maskBorder: 'url(/assets/torn.png) 30 / 30px round' }} />
// Solid but for a soft edge: the corner and edge slices of a gradient, with the middle filled.
<div style={{ maskBorderSource: 'radial-gradient(black 60%, transparent)', maskBorderSlice: '20% fill' }} />
```

Chrome only has the prefixed `-webkit-mask-box-image-*` forms of these, which read the
image by alpha only, imply `fill` when written in the shorthand and start `slice` at
`0 fill`; the unprefixed names here follow the CSS Masking spec instead. A slice is
drawn from the image's own pixels, so a vector (SVG) source that is scaled up well past
its size has soft edges where Chrome's are sharp. A gradient source is laid out for the scaled
size of each slice instead of being warped with it: a gradient along an axis comes out
as Chrome's, but a radial one keeps its circle and an angled one its angle where Chrome
stretches them with the slice. Past 4096 tiles in one edge nothing is drawn for it.

Limits: `image-set()` is not drawn (that layer is skipped, as in `backgroundImage`).
The legacy `-webkit-mask-composite` keywords (`source-over`,
`source-in`, ...) are not supported. `no-clip` and `margin-box` are not supported, so
a value using them is ignored. `fill-box` is `content-box` and `stroke-box` and
`view-box` are `border-box`. Masks, like the other effect groups, are drawn only on
macOS.

`clipPath` clips the element and everything in it to a basic shape: `inset()`
(one to four insets and `round` radii, with `/` for the vertical ones), `rect()`
and `xywh()` (a box, with the same `round`), `circle()` and `ellipse()` (a radius, `closest-side` or
`farthest-side`, and `at <position>`), `polygon()` (an optional `nonzero` or
`evenodd`, then points), `path()` (an optional fill rule, then SVG path data in a
string), or `none`. Lengths are any [length](#lengths) or `%`. Write a `border-box` (the default),
`padding-box` or `content-box` before or after the shape to choose what the
lengths are relative to. An invalid value is ignored, as in CSS. A clip path
composites the element as a group, like `opacity` and `mask`.

```tsx
<div style={{ clipPath: 'circle(50% at 50% 50%)' }} />
<div style={{ clipPath: 'polygon(50% 0, 100% 100%, 0 100%)' }} />
<div style={{ clipPath: 'inset(8px round 12px)' }} />
<div style={{ clipPath: 'rect(auto 80% 50% 10px round 8px)' }} />
<div style={{ clipPath: 'xywh(10% 10px 50% 40px)' }} />
<div style={{ clipPath: "path(evenodd, 'M0 0 H100 V100 H0 Z M25 25 H75 V75 H25 Z')" }} />
```

`rect()` and `xywh()` measure from the top left of the reference box: in
`rect(<top> <right> <bottom> <left>)` the right and bottom are distances from the
left and top edges, unlike `inset()`, and each of the four may be `auto` (`0` for
top and left, `100%` for right and bottom). `xywh(<x> <y> <width> <height>)`
takes a non-negative width and height. The far edge is the sum of the near edge
and the size, so `xywh(10px 0 50% 5px)` ends at `calc(10px + 50%)`.

`path()` takes the SVG path data `d` accepts (all commands, relative or absolute,
arcs included), in `px` from the top left of the reference box. Curves are
flattened to within 0.25px, with a limit of 4096 vertices for the whole path: a
path that would need more is flattened more coarsely, step by step up to 4px, and
is invalid if even that is too many. Each subpath is closed and the fill rule
applies to all of them together, so `nonzero` leaves a subpath that winds the same
way as the one around it filled, and `evenodd` makes it a hole. A path with no
subpath that has an area clips everything.

Limits: `url()` is not supported, and neither is a box with no shape, so a value
using them is ignored. `margin-box` and `stroke-box` and `view-box` are
`border-box`, and `fill-box` is `content-box`. `inset()`, `rect()` and `xywh()`
round their corners with circles, so the vertical radii after `/` in `round` are
dropped. A string in `path()` has no escapes.

The shape also clips hit testing, as in CSS: a pointer over the clipped-away part
hits what is underneath instead, and the element's descendants are cut the same
way, overflow included.

**Overflow:** `overflow`, `overflowX`, `overflowY` — `"clip"` clips content and is no scroll container (a flex or grid item of it keeps its content-based minimum size; of a `"hidden"` one it shrinks to 0), `"hidden"` clips and scrolls only from code, `"scroll"` and `"auto"` create a native scrollable container with persistent scroll state. `overscrollBehavior`, `overscrollBehaviorX`, `overscrollBehaviorY` (`"auto"` | `"contain"` | `"none"`) and `scrollBehavior` (`"auto"` | `"smooth"`) — see [Scrolling](#scrolling)

**Containment:** `contentVisibility` (`"visible"` (default) | `"auto"` | `"hidden"`) skips the content of an element and `containIntrinsicSize` (`none`, a length or `auto <length>` per axis; longhands `containIntrinsicWidth`, `containIntrinsicHeight`, `containIntrinsicInlineSize`, `containIntrinsicBlockSize`) sizes it while it is skipped — see [Skipping content with `contentVisibility`](#skipping-content-with-contentvisibility). Neither inherits.

**Text:** `fontSize`, `fontFamily`, `fontWeight`, `fontStyle` (`"normal"` | `"italic"` | `"oblique"`, inherited), `WebkitFontSmoothing` (`"auto"` | `"antialiased"` | `"subpixel-antialiased"`, inherited; see below), `textAlign` (`"start"` | `"end"` | `"left"` | `"right"` | `"center"`, inherited), `direction` (`"ltr"` | `"rtl"`, inherited; see [Direction](#direction)), `lineHeight` (a number multiplies the font size, a string is a length, see [Lengths](#lengths)), `whiteSpace` (`"normal"` | `"nowrap"` | `"pre"` | `"pre-wrap"` | `"pre-line"` | `"break-spaces"`, inherited), `tabSize` (a number of spaces or a length such as `"40px"`, inherited, default 8; see [White space](#white-space)), `wordBreak` (`"normal"` | `"break-all"` | `"keep-all"` | `"break-word"`, inherited) and `overflowWrap` (`"normal"` | `"break-word"` | `"anywhere"`, inherited; see [Breaking words](#breaking-words)), `textOverflow`, `lineClamp`, `textDecoration` (`"underline line-through"`, `"underline wavy red"`...), `textDecorationLine`, `textDecorationStyle`, `textDecorationColor`, `textDecorationThickness`, `textUnderlineOffset` (see [Inline text](#inline-text)), `textWrap`, `textWrapMode` and `textWrapStyle` (see [Balanced and pretty lines](#balanced-and-pretty-lines)), `textBox`, `textBoxTrim` and `textBoxEdge` (see [Trimming text boxes](#trimming-text-boxes))

`fontVariantNumeric` (`"tabular-nums"`, `"proportional-nums"`, `"lining-nums"`, `"oldstyle-nums"`, `"slashed-zero"`, `"diagonal-fractions"`, `"stacked-fractions"`, `"ordinal"`, `"normal"`, one keyword per group, combined with spaces) and `fontFeatureSettings` (`'"tnum" 1, "liga" 0'`) turn OpenType features on or off. Both inherit, and a descendant that sets one keeps the other. When both name a feature, `fontFeatureSettings` wins. The font needs the feature, and neither prop is available inside `hover`, `active` or `focusVisible`.

`fontVariationSettings` (`'"wght" 650, "wdth" 90'`, or `normal`) sets the axes of a variable font, on macOS. It inherits, works on a `<span>` (per run) and on inputs, and is not available inside state styles. A tag is four characters and its value a number, clamped to the font's range for that axis; an axis the font does not have is ignored, and a tag named twice takes its last value. `fontWeight` already sets `wght` on a variable font, to any weight and not only the named ones (`600` is 600, not the 590 semibold instance), and a `wght` in `fontVariationSettings` wins over it. A value CSS does not have is ignored, as an invalid declaration is, and `fontFeatureSettings` and `fontVariantNumeric` are: the element keeps what it inherits. It animates, on a `<span>` too: see [Targets and timing](#targets-and-timing).

`WebkitFontSmoothing` (`"auto"` | `"antialiased"` | `"subpixel-antialiased"`) is CSS `-webkit-font-smoothing` as Chrome has it on macOS. `"antialiased"` draws glyphs as plain coverage, the same ink for dark text and for light text, which is what a web page sets on `body` to keep its type from looking heavy; `"auto"` (the default) is CoreGraphics' font smoothing, which thickens the strokes, light text most, made into a mask for the colour of the text as Chrome does (the system's "font smoothing" preference still turns it off). On macOS `antialiased` and `auto` have the ink of Chrome's `antialiased` and `auto` (equal to 0.2 % for black on white and white on black, 0.7 % for a grey on a near black, measured on Inter at 14 and 24 px, at scale 1 and 2); `auto` has 9-17 % more ink than `antialiased` for dark text and 22-42 % more for light text. `"subpixel-antialiased"` is `"auto"`, as on current macOS, which draws no subpixel text. It inherits, works on a `<span>` (per run), on inputs and through `display: contents`, and can be set in `hover`, `active` and `focusVisible`. A value CSS does not have is ignored, as an invalid declaration is: the element keeps what it inherits. It does not animate. It is the first vendor-prefixed key, written as React writes it, with a capital `W`.

`fontFamily` resolves installed fonts. To use your own, pass the files in the `fonts` window option or call [`registerFont()`](#custom-fonts). On macOS a variable font follows `fontWeight` and `fontVariationSettings` along its axes; elsewhere it renders its default instance, so use one static file per weight. The family name must match the font's family name exactly.

**Selection:** `userSelect` (`"text"` | `"none"`), `selectionColor` — both inherit down the tree

**Focus:** `focusVisible`, `focusWithin`. See [Focus styles](#focus-styles)

**Ancestor state:** `group`, `groupHover`, `groupActive`, `groupFocusWithin`. See [Group state styles](#group-state-styles)

**Descendant state:** `hasHover`, `hasActive`, `hasFocus`, `hasFocusVisible`, `hasDisabled`, `hasEnabled`. See [Relational state (`:has()`)](#relational-state-has)

**Form control state:** `disabled`, `enabled`. See [Form control state](#form-control-state-disabled-enabled)

**Container queries:** `containerType`, `containerName`, `container`, `"@container"`. See [Container queries](#container-queries)

**Every element:** `<UniversalStyle>`, a `* { ... }` layer under all own styles. See [Universal style](#universal-style)

### Group state styles

There is no stylesheet, so an element cannot react to a hovered ancestor with a
selector. Mark the ancestor with `group` and style the descendants with
`groupHover`, `groupActive` and `groupFocusWithin`, like Tailwind's
`group-hover:`, `group-active:` and `group-focus-within:`:

```tsx
<div style={{ group: true, padding: 12, backgroundColor: '#fff' }}>
  <text style={{ color: '#666', groupHover: { color: '#000' } }}>Title</text>
  <div style={{ opacity: 0, groupHover: { opacity: 1 }, groupActive: { backgroundColor: '#eee' } }} />
  <input style={{ borderColor: '#ccc' }} />
  <text style={{ color: '#666', groupFocusWithin: { color: '#06f' } }}>Editing</text>
</div>
```

`group: true` is the default group, which an unnamed `groupHover` follows. Name a
group (`group: 'card'`) and point at it with `groupHover: { group: 'card',
backgroundColor: '#eee' }` when groups nest: the style follows the nearest
ancestor with that name. `groupHover` applies while the pointer is over the
group or anything inside it, `groupActive` while the mouse is down inside it,
`groupFocusWithin` while the group or anything inside it has focus. Each element
takes one of each. The pointer states are the pointer dispatcher's, the same that
drives `hover` and `active`, and the focus state is the one behind `focusWithin`.
A group applies to its descendants, not to its own element. They fold in with the
element's own states, later winning: base, `groupHover`, `hover`, `groupActive`,
`active`, `groupFocusWithin`, then `focusWithin` and `focusVisible`, which the
focus code applies after the fold. Focus states sit above the pointer ones. The
[`has*` keys](#relational-state-has) and the
[control states](#form-control-state-disabled-enabled) fold in between these. So a
`color` set by a group state reaches the descendants' `currentColor` like one set
by `hover`.

### Relational state (`:has()`)

An element can also react to what its **descendants** are doing, which is what
`:has()` does in CSS. Six nested style objects, like `hover`:

| Key | CSS | Applies while |
|---|---|---|
| `hasHover` | `:has(:hover)` | a descendant is under the pointer |
| `hasActive` | `:has(:active)` | the mouse is down on a descendant |
| `hasFocus` | `:has(:focus)` | a descendant has focus |
| `hasFocusVisible` | `:has(:focus-visible)` | a descendant matches `:focus-visible` |
| `hasDisabled` | `:has(:disabled)` | a descendant is a [disabled form control](#form-control-state-disabled-enabled) |
| `hasEnabled` | `:has(:enabled)` | a descendant is an enabled form control |

```tsx
<div
  style={{
    borderWidth: 1,
    borderColor: '#ccc',
    hasFocus: { borderColor: '#06f' },        // an input inside has focus
    hasHover: { backgroundColor: '#f6f6f6' }, // the pointer is over a child
  }}
>
  <input />
  <div tabIndex={0}>Save</div>
</div>
```

The match is on a **strict descendant**: the element being hovered, pressed or
focused itself does not count, so `hasHover` is not `hover`. `hover` also applies
while a child is hovered, because it applies to each ancestor of the target; use
`hasHover` when you need to tell "over the element" from "over something inside
it". Likewise `hasFocus` differs from `focusWithin`, which also applies to the
focused element. A descendant is one in the logical tree, the same chain that
drives `hover` and `focusWithin`: a [portal](#portals) child counts for the
element it is mounted under, wherever it is painted. In an `inert` subtree
`hasHover`, `hasActive` and `hasFocus` do nothing, as `hover` does not, since
nothing inside it can be hovered, pressed or focused.

They fold in with the element's own states. An element's own state beats what
its descendants cause, and that beats a group. Later wins: base, `groupHover`,
`hasHover`, `hover`, `groupActive`, `hasActive`, `active`, `groupFocusWithin`,
`hasFocus`, `focusWithin`, `hasFocusVisible`, `focusVisible`; `hasEnabled`,
`hasDisabled`, `enabled` and `disabled` come after `active`, before the focus
ones. The ancestor restyles in the same frame the pointer or focus moves, or a
control is enabled or disabled. Everything a state style
can hold works: layout, `position`, `color` (which reaches the descendants'
`currentColor`), [transitions](#css-transitions) and
[animations](#css-animations) declared inside them, and
[`<UniversalStyle>`](#universal-style) refinements. They work on every element
that `hover` does, including an inline `<span>`: a span restyles while a span
inside it is hovered.

There is no stylesheet, so this is not the whole of `:has()`. It cannot express:

- An arbitrary selector. `:has(.error)`, `:has(img)` and `:has(+ x)` need a
  selector engine; the six keys cover the pseudo-classes gpuix tracks
- A direct child only (`:has(> x)`). The match is on any descendant
- `:has(:checked)` and the like. There is no `checked` state style, so there is no
  `hasChecked` either: see [`:checked`](#form-control-state-disabled-enabled).
  Derive it in your component and pass the style
- Anything about a sibling or an ancestor of the descendant. Only the six states
  of a descendant are tracked

### Form control state (`:disabled`, `:enabled`)

`disabled` and `enabled` are nested style objects like `hover`: `:disabled` and
`:enabled`. Their `has*` counterparts, `hasDisabled` and `hasEnabled`
(`:has(:disabled)`, `:has(:enabled)`), are [relational](#relational-state-has):
they apply to a strict ancestor of such a control.

```tsx
<div style={{ hasDisabled: { opacity: 0.6 } }}>
  <input
    disabled={saving}
    style={{ borderColor: '#ccc', disabled: { backgroundColor: '#eee' }, enabled: { borderColor: '#999' } }}
  />
  <Button disabled={saving} style={{ disabled: { opacity: 0.5 } }}>Save</Button>
</div>
```

What counts as a form control is what gpuix has: an `<input>`, a `<textarea>`,
and the components that stand in for a `<button>` or a `<select>`: `Button`, the
triggers of `Dialog`, `Select` and `Combobox`, and the root of a `Select`. A
control is `:enabled` unless it is `disabled`. A plain `<div disabled>` is not a
control and matches neither, as in a DOM (`disabled` is not forwarded to a `div`),
and nothing else is: there is no `<fieldset>`, so no control is disabled by its
ancestor. A custom component that wants the same passes `buttonProps(...)`, which
carries the marker.

They fold in with the pointer states, after `active`: a disabled control keeps
its `disabled` look while it is hovered or pressed, which still apply to it as
`:hover` does in CSS. Unlike `hover`, they also apply in an `inert` subtree, where
`:disabled` still matches. A change to `disabled` restyles the control and the
ancestors that declare `hasDisabled` or `hasEnabled` in the same frame.
[Transitions](#css-transitions), [animations](#css-animations) and
[`<UniversalStyle>`](#universal-style) refinements work in them as in `hover`.

**`:checked` is not available.** It needs a checkable element, and gpuix has none:
every `<input type>` is text-like, and there is no checkbox, radio or `<option>`
to be checked or selected. So there is no `checked` or `hasChecked` key; derive the
state in your component and pass the style, as for a custom checkbox built from a
`div`. It can be added together with a native checkbox control.

### Container queries

A component often has to look different when the space it is given is small, not
when the window is. CSS container queries do that, and so do these styles. Mark an
ancestor as a **container**, then style its descendants with `"@container"` blocks
whose keys are queries about the container's size, and with `cq` units that are
fractions of it:

```tsx
<div style={{ containerType: 'inline-size', containerName: 'card' }}>
  <div
    style={{
      display: 'flex',
      flexDirection: 'column',
      gap: '4cqw', // 4% of the card's width
      '@container': {
        '(min-width: 400px)': { flexDirection: 'row' },
        'card (400px <= width < 800px) and (orientation: landscape)': { gap: 24 },
        'not (min-width: 200px)': { display: 'none' },
      },
    }}
  >
    ...
  </div>
</div>
```

| Style | |
|---|---|
| `containerType` | `"normal"` (default, not a container), `"inline-size"` (queries read the width) or `"size"` (the width and the height) |
| `containerName` | names a query can pick the container by: `"card"`, `"card panel"`, `"none"` |
| `container` | `"<name> [ / <type> ]"`, both in one (`"card / inline-size"`); a type left out is `"normal"`; a longhand written after it wins |
| `"@container"` | an object of `query: style`, in written order |
| `"--name"` | a custom property, which inherits and which `style()` queries read ([below](#style-and-custom-properties)) |

**Which container answers.** A query is about the **nearest ancestor container**,
never the element itself, and measures its **content box**: the border box less
padding, border and a scrollbar gutter. Start the query with a name
(`"card (min-width: 400px)"`) to ask the nearest container that has it instead; a
name alone (`"card"`) is true when there is one. A feature reads one axis or both:
`width` and `inline-size` need a container of type `inline-size` or `size`, and
`height`, `block-size`, `aspect-ratio` and `orientation` need `size`. **One
container answers the whole query**, as in a browser: the nearest one that contains
every axis the query's size features read, so `(max-width: 60px) and (min-height:
0px)` is asked of the nearest `size` container for both, and a `(min-height:
300px)` under an `inline-size` container looks past it to a `size` one above. A
query with no such container, or whose container has not been laid out yet, is
false. A `style()` query with no size feature and no name asks the parent element,
see [`style()` and custom properties](#style-and-custom-properties).

**The grammar** is CSS Containment 3:

- Features: `width`, `height`, `inline-size`, `block-size` (lengths), `aspect-ratio`
  (`16/9`, `1.5`) and `orientation` (`portrait` | `landscape`; portrait when the
  height is at least the width). Written `(feature: value)`, with `min-` and `max-`
  on the first five, as a boolean `(width)` (not zero), or as a range:
  `(width >= 400px)`, `(400px <= width < 800px)`, `(aspect-ratio > 1/1)`
- Lengths: px, `em` (the **container's** font size), `rem`, the viewport units,
  `calc()`; no `%`
- `and`, `or`, `not`, and parentheses. `and` and `or` cannot be mixed without
  parentheses, and `not` takes one condition in parentheses
- `style()` is [below](#style-and-custom-properties). A feature this does not know,
  `scroll-state()`, and a normal property inside `style()` are *unknown*. An unknown
  result is false, so `not` of one is false too. Text that is not a query at all
  (unbalanced parentheses, `and` mixed with `or`) makes the whole style invalid, like
  an invalid `width`

**How blocks apply.** Matching blocks fold into the element's style in the order
they are written, a later one winning over an earlier one that sets the same
property, and a shorthand resetting its longhands as it does in `hover`. They fold
in **before** the state styles, so `hover`, `active`, `focusVisible` and the others
win over a block, as `:hover` beats a plain rule inside `@container` by
specificity. A block may carry state keys of its own, which merge into the style's:

```tsx
style={{
  color: '#444',
  hover: { color: '#000' },
  '@container': { '(min-width: 400px)': { color: '#06f', hover: { color: '#04c' } } },
}}
```

is `#06f` in a wide container, `#04c` when it is hovered, and `#000` when hovered in
a narrow one. A block cannot hold another `"@container"`, and a state style
(`hover`, ...) cannot hold one. Everything else a style holds works in a block:
layout, `display: none`, `color` for `currentColor`, transitions and animations
(a flip starts the transition), and `<UniversalStyle>` blocks, which come before the
element's own. Blocks do not apply to an inline `<span>`: put the style on a `<text>`, or
use a container unit. An [`inline-block`](#inline-block-inline-flex-inline-grid-and-inline-table) span is a box of its own and
takes them.

**Container units.** `cqw` and `cqi` are 1% of the width of the nearest ancestor
container that contains the width, `cqh` and `cqb` 1% of the height of the nearest
one that contains the height (a `size` container), `cqmin` and `cqmax` 1% of the
smaller and larger of those two. They work in every length: sizes, padding, `gap`,
`fontSize`, `lineHeight`, border widths, shadows, gradients, grid tracks, `calc()`.
With no container for an axis they fall back to the small viewport, the window's
size. A container's own `width: "50cqw"` is about the container above it.

**The container's box.** `containerType: "inline-size"` contains the inline axis: the
element's content-based width is only its padding and border, as if it had no
children. That is what makes the query possible (a box that grows to fit what a
query shows could never settle), and it is visible: an `inline-size` container with
no `width` in a flex row, an absolutely positioned box or a column flex container
with `alignItems: "flex-start"` is as wide as its padding and border, whatever it
holds, so give it a `width`, `flexGrow` or `alignSelf: "stretch"`. A block-level
container fills its parent as any block does. `"size"` contains the height too, so
a container with no `height` is as tall as its padding and border. `minWidth`,
`maxWidth` and the like still clamp. A container also lays its content out in a
formatting context of its own, so the margins of its children do not collapse
through it. That is all it does to its box, as in the browser (CSS Containment 3
gives `container-type` size and style containment, not layout containment): it is
not the containing block of its `position: absolute` and `fixed` descendants, which
resolve against the positioned ancestor outside it, and it makes no stacking context,
so a `zIndex` inside it competes with the boxes outside.

**The style is right in the frame that lays the container out.** Layout knows a
container's size only after the tree has been built from the style the size picks,
so the build reads the size the last layout recorded. After layout and prepaint,
each container compares the size it was just laid out at with the one the build
assumed, for the axes a query or a unit read, and when they differ the window
**builds the frame again before presenting it**: the pass is dropped before paint
and the build runs from the new size, up to four passes in all (one more for each
level of containers whose size another container's query changes; a mount at depth
two settles in three). Only the presented pass paints, so `onLayout`, the observers,
bounds, painted text, selection and accessibility only ever see the final frame, and
a mount or a resize that flips a query never shows the old style for a frame. A
transition does not start from a guess: a style that reads a container is not
transitioned until every container above it has been laid out once. If the limit is
reached with the sizes still disagreeing (two containers that flip each other, as
`min-width` rules can) the last pass is presented and the window draws once more, so
such a loop costs a frame per step, never a hang.

The cost is paid only by a frame in which a container's size changed in an axis
something read: each extra pass builds, lays out and prepaints the whole tree but
does not paint. A frame in which no container changed costs one comparison per
container that something reads, and no extra pass. A tree with no container, and one
with containers nobody reads, pays nothing. On 500 containers and 4.5k elements
(`examples/container-queries.perf.test.tsx`, `bun run test:perf:cq`) a steady frame
costs what the same tree without containers costs (about 40 ms of CPU in the test
renderer, within noise), and a frame in which every container crosses a query takes
about 1.9 to 2 times a steady frame, which is what a restyle from the app costs
too, since the app has to render and the frame draws afterwards.

#### `style()` and custom properties

A key that starts with `--` is a CSS custom property, in a style, a state style or an
`"@container"` block. They inherit, and `style()` queries read them:

```tsx
<div style={{ '--density': 'compact', '--columns': 3 }}>
  <div
    style={{
      padding: 12,
      '@container': {
        'style(--density: compact)': { padding: 4 },
        'style(--columns > 2) and (min-width: 400px)': { flexDirection: 'row' },
      },
    }}
  />
</div>
```

- **Values** are text. A string is trimmed, and its `/* comments */` and a trailing
  `!important` are dropped; a number is its decimal text (`3` is `"3"`); an empty
  string, `null` and `undefined` declare nothing, as React DOM removes the property
  for them. Anything else is an error. Names are case sensitive.
- **They inherit.** An element computes its own declarations over its parent's.
  `"initial"` clears one (it has no value below), `"inherit"` and `"unset"` keep the
  parent's. A state style (`hover`, ...) or a matching `"@container"` block that sets
  one changes it for the element and everything below it while it applies.
- **`style(--name: value)`** is true when the container computes `--name` to exactly
  that text: `style(--theme: dark)` does not match `Dark`, `style(--space: a  b)` does not
  match `a b`, and `10px` is not `10PX`. A trailing `!important` and comments in the
  query are ignored. **`style(--name)`** is true when the container has a value for
  it (`0` is one), **`style(--name: initial)`** when it has none.
  **`style(--n > 5px)`**, `<`, `>=`, `<=`, `=` and the two sided
  `style(0 < --n <= 1)` order a number, a length (`em`, `rem` and the viewport units
  resolve against the container's font) or a percentage against a value of the same
  kind; text, a missing property or another kind is unknown.
- **They combine** inside, `style((--a: 1) and (--b: 2))`, `style(not (--a: 1))`,
  `or`, and outside, `style(--a: 1) and style(--b: 2)`, `not style(--a: 1)`, and
  with size features, `(min-width: 400px) and style(--a: 1)`. A declaration list written
  without parentheses (`style(--a: 1; --b: 2)`) is invalid and unknown.
- **Which container.** Every element is a style container, so with no name and no size
  feature a `style()` query asks the **parent element**, whatever its `containerType`;
  with a name, the nearest element that has it, and a `containerName` with no
  `containerType` is enough (a name alone is also a container a name-only query,
  `"@container card"`, finds); with a size feature in the same query, the one container
  that answers the size feature (see above), which then answers the `style()` part too.
  The element's own custom properties are never what its own query reads.
- **A normal property in `style()`** (`style(color: red)`, `style(display: block)`) is
  unknown, as in Chrome, which supports custom properties only.

Differences from the browser:

- `scroll-state()` is unknown (false), and `container-type: scroll-state` does not exist.
  `var()` is not substituted into other properties: a custom property is only read by
  `style()`. `style(--x: inherit)`, `unset` and `revert` are unknown, there is no `@property`
  registration (every value is text, and none transitions or animates), and a custom
  property cannot be set to `""` (that declares nothing)
- Which elements are containers follows CSS. A `div`, `text`, `label`, `form`, an atomic
  `span` (`inline-block`, `inline-flex`, `inline-grid`) and a table caption are size
  contained as `containerType` says, and so are a [`<virtual-list>`](#virtual-lists) (its rows are
  queried against it, and they follow its size in the frame that lays it out), an `<img>`
  and an `<svg>`. An image or an `<svg>` has nothing inside to query, so for them the type is
  only size containment: it takes the natural size, and the natural aspect ratio, away on the
  contained axes, so an `<img>` with `containerType: "size"` and no width is 0 wide and 0
  tall. An `<input>` and a `<textarea>` ignore `containerType`, as they do in Chrome
- An element that cannot be size contained is still a container a query finds: an inline
  `<span>` that is not atomic, a `display: contents` element, a [table](#tables) (and an
  `inline-table`), a row group, a row, a column, a column group and a cell. A size feature
  asked of it is unknown, so the query is false and **so is its negation**, and the lookup
  does not go on to a container above it. A name still matches, `style()` reads its custom
  properties, and a container unit (`cqw`) looks past it to the one above. A cell is laid
  out by the table algorithm and is as wide as its content whatever it declares
- Not containers at all: `<anchored>`, `<code>`, `<markdown>`, `<diff>`, and `<input>` (it
  has no children). `<virtual-list>` takes no `@container` blocks itself (it takes no
  `hover` either), and neither does an inline `<span>`
- A block is evaluated where the element is built, and what the walks over the tree read
  (hit testing, stacking, observers) is the style as it was painted, blocks included.
  The paragraph planner and the table role read `display` before the element is built, so
  `display` is settled from the blocks first (`none`, `block`, `inline`, `inline-block`,
  `inline-flex`, `inline-grid`, and `flex` or `grid` for the container itself, which also
  decides whether `order` sorts its children and whether the `zIndex` of a static child
  stacks) and what part of a table it is (`table`, `inline-table` and the other
  `table-*` values, which also decide what a table, row group or row wraps in an anonymous
  cell, and what a block wraps in an anonymous table): the parent is rebuilt when a block
  changes one. A block's `order` is settled the same way, before the parent sorts its items, so
  an item moves in the frame that resizes the container. `display: contents` is not taken from a block, since the parent splices a
  child in before it is built. An element a block makes `inline` is a span of its parent's
  text, which takes no blocks, so its other blocks stop applying while it is
- There are no vertical writing modes (`writing-mode`, `text-orientation`), so `cqi` is
  `cqw`, `cqb` is `cqh` and `inline-size` is `width`. The vendored taffy lays out
  horizontal-tb only (its alignment code and flexbox and grid carry TODOs for the
  other modes) and gpui has no `writing-mode` style or vertical text, so it is a
  rewrite of the layout and text engines, not something to wire
- Sizes are snapped to sixty-fourths of a pixel, the unit browsers lay out in

### Hover and active

`hover` and `active` are **nested style objects**, like CSS `:hover` and
`:active`. Native applies them with no JavaScript round trip. `hover` applies to
the element under the pointer **and to each of its ancestors**; `active` to the
element the primary button went down on and its ancestors, until the release.

```tsx
<div
  style={{
    backgroundColor: '#313244',
    borderRadius: 8,
    padding: 12,
    hover: { backgroundColor: '#45475a' },
    active: { backgroundColor: '#585b70' },
  }}
>
  Press
</div>
```

Nesting is one level deep. A `hover` object cannot contain another `hover` or
`active`.

They work on **every** element, including `<text>`, `<code>`, `<markdown>`,
`<diff>`, `<img>`, `<svg>`, the editors and an inline `<span>` (see
[Events on inline boxes](#events-on-inline-boxes)). The one exception is
`<virtual-list>`, whose `style` type rejects them: gpui's list has no
interactive identity to hold a hovered or pressed state, so put them on a
wrapping `<div>`.

### Focus styles

`focusVisible` and `focusWithin` are nested style objects, like `hover`.
`focusVisible` applies while the element matches CSS `:focus-visible`;
`focusWithin` while it or any descendant has focus, like `:focus-within`, on
focusable and plain elements alike. There is no separate `focus` key.

`:focus-visible` follows the browser heuristic, tracked per focus change rather
than from the live input device:

- A text field (`<input>`, `<textarea>`) always matches while focused, mouse or
  keyboard
- Focus moved by Tab (or any keyboard input) matches
- Focus moved by a mouse press does not match, until a key goes down while the
  element is focused
- Focus moved by script (`focus()`, `autoFocus`) matches only if the element it
  left matched

```tsx
<div
  tabIndex={0}
  style={{
    borderRadius: 8,
    backgroundColor: '#313244',
    focusVisible: { outline: '2px solid #89b4fa', outlineOffset: 2 },
  }}
/>
```

`focusVisible` needs a **focusable** element: `tabIndex`, `<input>`,
`<textarea>`, or a primitive such as `Button`. A `<span>` with a `tabIndex`
takes both variants too, see [Focus on an inline box](#focus-on-an-inline-box).

**Default: everything else dims.** GPUIX draws no ring. While a control
(`Button`, a `tabIndex` div) has keyboard focus, every **other** focusable
element renders at 40% of its opacity. The focused one stays as it is, and you
see at a glance everything Tab can reach.

```
Tab  ► [ Save ]  (dim New)  (dim Search)  (dim input)
```

- It follows `:focus-visible`, so it lasts while that control stays focused,
  and ends when focus moves to a control that does not match (a mouse press on
  another control, for example). Moving the mouse does not end it
- A focused `<input>` or `<textarea>` dims nothing: typing is keyboard input
  too, and the caret already shows focus
- The focused element's ancestors never dim, because opacity covers the subtree
- `focusVisible` and the dim are **independent**. An element with a ring from
  `focusVisible` still dims while another control has focus
- `style.keyboardFocusDim: false` keeps one element at full opacity.
  `Select.Content` and `Dialog.Popup` set it

```tsx
<div tabIndex={0} style={{
  focusVisible: { outline: '2px solid #89b4fa' },             // when focused
}} />                                                         // dims otherwise
<div tabIndex={0} style={{ keyboardFocusDim: false }} />      // never dims
```

**Turn the dim off for the whole window.** Pass `keyboardFocusDim: false` to
`render()` (or `createTestRoot()`), then style focus yourself with
`focusVisible`.

```tsx
render(<App />, { keyboardFocusDim: false })
```

### Outline

`outline` draws a line outside the border box, like CSS `outline`. It takes **no
layout space**, so a ring you add with `focusVisible` moves nothing. It follows
`borderRadius`, and a parent with `overflow: "hidden"` clips it, like in a browser.

```tsx
<div style={{ outline: '2px solid #89b4fa', outlineOffset: 2 }} />
<div style={{ outlineStyle: 'dashed', outlineColor: 'red' }} />   // 3px dashed red
```

- **A style is required.** As in CSS, `outlineStyle` is `none` until `outline` or
  `outlineStyle` gives one, so `outlineWidth` and `outlineColor` alone draw
  nothing. With a style, the width defaults to `medium` (3px) and the colour to
  `currentColor`.
- **`outline`** is `<width> <style> <colour>` in any order, each part at most
  once. A part it leaves out is that default. `hidden` throws. `outlineWidth`,
  `outlineStyle` and `outlineColor` win over it when written after it.
  `outlineOffset` is not part of it.
- **Styles.** `solid`, `dashed`, `dotted`, `double`, `groove`, `ridge`, `inset` and
  `outset`, as for [borders](#borders). `auto` is accepted and paints as `solid`.
- Width and offset take any [length](#lengths) without a `%`; a negative offset
  draws the outline inside.
- **State styles.** A state's `outline` replaces the base's width, style and
  colour (not the offset). A state's `outlineWidth`, `outlineStyle` or
  `outlineColor` changes only its own part, so `focusVisible: { outlineColor: 'blue' }`
  recolours the base's outline.

### CSS transitions

`transition` and its longhands take CSS syntax with kebab-case property names,
as in a DOM inline style. When the value of an animatable property changes, the
element animates from the value it showed to the new one, natively, with no React
render per frame.

```tsx
<div
  style={{
    backgroundColor: '#313244',
    transition: 'background-color 150ms ease-out, opacity .2s 50ms',
    hover: { backgroundColor: '#45475a' },
  }}
/>
```

- **Properties.** `transition` is a comma list of `<property> <duration> <timing
  function> <delay>` in any order, plus `all` and `none`. `transitionProperty`,
  `transitionDuration`, `transitionTimingFunction`, `transitionDelay` and
  `transitionBehavior` win over the shorthand when written after it, and shorter lists repeat to the
  length of `transitionProperty`.
  Every property `motion` can animate (see [Targets and timing](#targets-and-timing))
  can transition. Shorthand names (`padding`, `margin`, `border-radius`, `inset`,
  `flex`, ...) cover their longhands. A valid CSS name gpuix does not animate
  (`transform`) is accepted and matches nothing. A camelCase name, a bad time or
  timing function, or an unparsable value is rejected when the style is set, like
  any other invalid style
- **Inline boxes.** A `<span>` transitions what an inline box shows too, and only that (see
  [Painting an inline box](#painting-an-inline-box))
- **Discrete properties.** `display`, `visibility`, `position`, `overflow`, `overflowX`,
  `overflowY`, `pointerEvents`, `tableLayout`, `borderCollapse`, `captionSide`, `emptyCells`,
  `flexDirection`, `flexWrap`, `alignItems`, `alignSelf`, `alignContent`, `justifyContent`,
  `justifyItems` and `justifySelf` have no in-between values, so they change at once unless the
  transition says `allow-discrete`, as in CSS Transitions Level 2:
  `transition: 'display 1s allow-discrete'`, or `transitionBehavior` (a list of
  `normal` and `allow-discrete`). `all` covers them only with `allow-discrete`.
  The value flips at the halfway point of the eased progress, after the delay.
  `display` is the exception: the side that is not `none` shows for the whole
  duration, so `block` to `none` stays visible until the end and then goes away
  (the exit animation pattern), and `visibility` is `visible` for the whole duration
  when either end is. A `boxShadow` change between lists that differ in `inset`
  flips the same way. The events are the ones other properties fire.
  `visibility` is the exception to `allow-discrete`: as in CSS it transitions without it, and
  `motion`'s `animate` and keyframes take it (`visible`, `hidden`, `collapse`).
  A `verticalAlign` keyword against a length or another keyword is discrete the same way.
  `borderSpacing` and `zIndex` blend, as the table above says. A transition of an inherited
  property that is unset (`borderSpacing`, `borderCollapse`, `captionSide`, `emptyCells`,
  `pointerEvents`, `visibility`, `textUnderlineOffset`, a unitless `lineHeight`) starts from
  the value the element inherits, which is the initial one when no ancestor sets it.
- **Timing functions.** `linear`, `ease`, `ease-in`, `ease-out`, `ease-in-out`,
  `cubic-bezier()` (it may overshoot), `steps(n, jump-start | jump-end |
  jump-none | jump-both | start | end)`, `step-start`, `step-end` and `linear()`
  stop lists. `motion` accepts the same strings as `ease`
- **What starts one.** Each frame the element's style, with the state styles that
  apply folded in, is compared with the previous frame's. A changed value starts a
  transition when its duration plus delay is above zero, using the `transition*`
  of the style after the change, so a change that also adds the transition (or a
  duration) runs. The first style an
  element mounts with never transitions. A property that is unset starts from its
  CSS computed value: the initial one (`opacity` 1, padding `0`, `boxShadow`
  none, `backgroundColor` transparent) or the inherited one (`color` is the
  parent's colour, so `color` unset to `color: 'blue'` runs from it, and a
  `color` dropped runs back to it). One whose computed value is a keyword CSS
  does not blend with a number, length or colour changes instantly: an unset
  `aspectRatio` (`auto`), `lineHeight` (`normal`), `scrollbarWidth` and an unset `width`.
  An unset `top`, `right`, `bottom`, `left` or `flexBasis` is `auto`, and a margin
  `auto`, which is discrete against a length: it changes instantly unless the
  transition has `transition-behavior: allow-discrete`, and then flips at the halfway
  point. An unset border or outline width is `medium` (or a `border` shorthand's width)
  whatever the style is, so a `borderStyle` that appears starts no `border-width`
  transition. `fontSize`
  and `fontWeight` are inherited too, and so are `textUnderlineOffset`, `visibility`, `pointerEvents`,
  `borderSpacing` and the other table properties and a unitless `lineHeight`: an unset one runs
  from the parent's computed value. `zIndex` is not animatable
- **Lengths.** Lengths blend as a px part and a percentage part, whatever unit
  they are written in (the Lengths paragraph of [Targets and timing](#targets-and-timing)): `px` to `%`, `em`,
  `rem`, the viewport units and `calc()` all transition, from and to each other.
  A `width`, `height`, `minWidth`, `minHeight`, `maxWidth`, `maxHeight`, `top`,
  `right`, `bottom`, `left`, margin, padding or `flexBasis` resolves the
  percentage against the parent's content box (the window's content size for a
  root), along the axis CSS uses: the width for `left`, `right`, every margin and
  padding side, the height for `top` and `bottom`, and for `flexBasis` the main
  axis of the flex container. It follows the parent while it runs and settles on
  the literal target unit
- **`auto` sizes.** As in a browser, a change between a length and `auto` is
  discrete unless `interpolateSize: 'allow-keywords'` is set on the element or an
  ancestor (it inherits, default `'numeric-only'`). That covers `height` and
  `width` to or from `auto` or unset, and an unset `minHeight` / `maxHeight` (CSS
  computes them to `auto` / `none`). With it set, the transition starts from the
  laid-out size and runs to the content's size, follows the content while it
  grows, and hands back to `auto` at the end. Its clock starts at the style
  change, not at the frame that has the measurement. A `width` that animates costs an
  extra layout per frame (see [Intrinsic widths](#intrinsic-widths)). `motion`
  animates to `height: 'auto'` and `width: 'auto'` whatever `interpolateSize` is,
  since that is Framer's behaviour
- **Multi-value shorthands.** `padding: '4px 8px'`, `gap: '4px 8px'`,
  `borderRadius: '10px / 20px'` and the other shorthands of
  [Targets and timing](#targets-and-timing) transition per side, axis or corner, in
  CSS transitions and in keyframes
- **Interrupts.** A change mid-flight starts from the visible value, not the old
  target. Returning to where the transition began shortens the way back in
  proportion, as CSS specifies. Changing `transition*` does not touch a running
  transition, but dropping its property from `transitionProperty` cancels it. A
  negative delay starts part-way through
- **State styles.** `hover`, `active`, `focusVisible`, `groupHover`,
  `groupActive`, `groupFocusWithin`, `hasHover`, `hasActive`, `hasFocus`, `hasFocusVisible`, `disabled`,
  `enabled`, `hasDisabled` and `hasEnabled` are folded
  into the style that is compared, so they animate in and out. A `transition` set inside `hover` is the one used while entering hover; the
  base one is used leaving it. An
  element that declares a transition has its state styles applied by gpuix instead
  of GPUI's per-element refinements, so they show one frame after the pointer
  moves. Elements without a transition behave as before
- **`display: none`.** An element that is not rendered has no transitions: going
  `display: none` cancels the running ones (`onTransitionCancel`), for its
  descendants too, and a style changed while it is hidden does not transition when
  it shows again, as in a browser. The exception is a `display` transition with
  `allow-discrete` going to `none`: the element and its descendants stay rendered
  until it ends. Going from `none` has no transition, and so no events, because
  nothing was rendered to start from; there is no `@starting-style`, and
  `motion`'s `initial` is the way to animate an element in
- **`motion`.** A property `animate` names belongs to `motion` and is not
  transitioned
- **CSS animations.** A property a running [CSS animation](#css-animations)
  drives, or fills with `forwards` / `both`, starts no transition when its
  declared value changes: the value before the change and the one after it are
  both the animated one. When the animation ends the declared value shows, with
  no transition from what the animation showed, and a change declared after it
  ended transitions as usual. A transition already running on the property when
  the animation starts keeps beating it, and a declared change cancels it
- **Events.** `onTransitionRun`, `onTransitionStart` (after the delay),
  `onTransitionEnd` and `onTransitionCancel` carry `propertyName` (`"width"`,
  `"background-color"`), `elapsedTime` in seconds and `pseudoElement: ""`. They
  bubble like the DOM (`target` is the transitioning element) and cannot be
  cancelled; `onTransitionEndCapture` and the other `…Capture` props listen in the
  capture phase

### CSS animations

`animation` and its longhands take CSS syntax, as in a DOM inline style. There is
no stylesheet to hold `@keyframes`, so `defineKeyframes` is its analogue. The
animation then runs natively, with no React render per frame.

```tsx
import { defineKeyframes } from '@gpuix/react' // or '@gpuix/solid'

defineKeyframes('pulse', {
  from: { opacity: 1 },
  '50%': { opacity: 0.4, animationTimingFunction: 'ease-in-out' },
  to: { opacity: 1 },
})

<div
  style={{
    width: 120,
    height: 8,
    backgroundColor: '#89b4fa',
    animation: 'pulse 1.2s linear infinite',
  }}
  onAnimationIteration={(event) => console.log(event.animationName)}
/>
```

- **`defineKeyframes(name, frames)`.** Keys are `from`, `to`, a percentage
  (`'50%'`) or a comma list (`'0%, 50%'`). Values are the properties `motion` can
  animate (see [Targets and timing](#targets-and-timing)), each with a single
  value (and the [SVG properties](#css-keyframes-on-svg-properties), for
  `<svg>` and the shapes in it), plus `animationTimingFunction`, which eases from that keyframe to the
  next. Blocks at the same offset merge, the later key winning. A property a
  definition leaves out of `from` or `to` starts or ends at the element's own
  value, as in CSS, and that value is read every frame, so it follows the
  declared style as it changes. Redefining a name changes the animations already running on
  it from the next frame, and they keep their start time, iteration, direction
  and fill. A bad definition throws from `defineKeyframes` and keeps the earlier
  one
- **One registry for the process.** `@keyframes` is document-global, so a
  definition reaches every window and test root, including ones created later.
  Define keyframes once at module scope
- **Properties.** `animation` is a comma list of `<name> <duration> <timing
  function> <delay> <iteration count> <direction> <fill mode> <play state>` in any
  order: the first time is the duration, the second the delay. `animationName`,
  `animationDuration`, `animationTimingFunction`, `animationDelay`,
  `animationIterationCount`, `animationDirection`, `animationFillMode` and
  `animationPlayState` win over the shorthand when written after it, and shorter lists repeat to the
  length of `animationName`. The timing functions are the ones
  [CSS transitions](#css-transitions) take. An invalid value is rejected when the
  style is set. A name with no keyframes animates nothing, as in a browser
- **Behaviour.** CSS Animations Level 1: `infinite` and fractional iteration
  counts, `reverse`, `alternate` and `alternate-reverse`, `backwards` / `forwards`
  / `both` fill, `paused` (freezes where it is and resumes from there), a negative
  delay that starts part-way through, and a restart when `animationName` changes
- **State styles.** `hover`, `active`, `focusVisible`, `groupHover`,
  `groupActive`, `groupFocusWithin`, `hasHover`, `hasActive`, `hasFocus`, `hasFocusVisible`, `disabled`,
  `enabled`, `hasDisabled` and `hasEnabled` may declare
  `animation`. It starts when the state applies and is
  cancelled when it ends
- **`display: none`.** Going `display: none` cancels the element's animations and
  those of its descendants, and they start over when it shows again
- **Cascade.** Animations win over the element's style, its state styles and
  `motion`, as animations do over normal declarations in CSS. A running CSS
  transition wins over an animation on the same property, as in CSS Cascade 4:
  the animation shows through again when the transition ends
- **Events.** `onAnimationStart` (after the delay), `onAnimationIteration` (each
  iteration after the first, not at the end), `onAnimationEnd` and
  `onAnimationCancel` carry `animationName`, `elapsedTime` in seconds and
  `pseudoElement: ""`. `onAnimationCancel` fires when the animation is removed or
  its name changes before it ended; one that ran out before that frame ends
  (`onAnimationEnd`) and is not cancelled. They
  bubble like the DOM (`target` is the animating element) and cannot be
  cancelled; each has an `on…Capture` twin

Differences from a browser:

- A missing `from` or `to` of a style whose own value gpuix cannot read from
  the element's style (an inherited one such as `fontSize`) holds the nearest
  keyframe instead of using the computed value
- A property the keyframes name counts as driven by the animation for as long as
  it runs or fills, so a declared change on it starts no transition even where
  the animation cannot produce a value for it (it waits for layout)
- Only the properties `motion` can animate are animatable, so `transform` and
  discrete properties such as `display` cannot be keyframed. The SVG properties
  CSS animates are, on [SVG shapes](#css-keyframes-on-svg-properties)
- `animation-composition`, `animation-timeline` and `animation-range` are not
  supported
- `animation-duration: auto` is `0s`
- A size keyframe against `auto` (or an unset size) is discrete unless
  `interpolateSize: 'allow-keywords'` applies, as for transitions
- A keyframe length written with `em`, `rem`, `vw`, `calc()` or a mix of a length
  and a `%` is an error that names the property, as for `motion`

#### Web Animations

`element.getAnimations()` on a ref returns the element's running CSS transitions,
then its CSS animations, as `CSSTransition` and `CSSAnimation` objects. A
transition is listed in the order of the style changes that started them, then by
property name; animations in the order of `animation-name`. An animation is listed
while it is before or in its active phase, running back from its end, or held by
a fill (`forwards`, `both`): one that is paused at the end of its effect without a
fill is not listed, as in Chrome. The same object stands for the same run on every
call.

```tsx
const [animation] = ref.current.getAnimations()
animation.pause()
animation.currentTime = 250 // the next frame paints it
await animation.finished // resolves with the animation
```

- **Members.** `pause()`, `play()`, `currentTime` (milliseconds, read and set,
  `null` when idle), `playbackRate` (read and set, negative allowed), `playState`
  (`idle`, `running`, `paused`, `finished`), `finished`, `animationName` on a
  `CSSAnimation` and `transitionProperty` (`background-color`) on a `CSSTransition`
- **Time.** `currentTime` is 0 at the start whatever the delay, which lives inside
  the effect, and reads the time of the last frame. `pause()` holds it at once,
  `play()` continues from it at the next frame and starts a finished animation
  over (from the end when the rate is negative), and setting it seeks at once.
  After `pause()` or `play()`, `animationPlayState` changes are ignored
- **`finished`.** Resolves with the animation. It is a new promise once the
  animation leaves `finished` again. Cancelling (the name leaves `animation`, a
  transition is replaced, the element goes) rejects the promise that was held with
  an `AbortError` and leaves the animation `idle`
- **A finished transition** keeps `playState: 'finished'` and its end as
  `currentTime`, and drops out of `getAnimations()`, as in Chrome. It can be
  played, paused, sought and given a rate again until the style moves off its end
  value, which cancels it (`idle`). One run back to its start at a negative rate
  stays `finished` and listed, showing the value it started from, until the style
  changes. Commands on a cancelled animation throw an `InvalidStateError`
- **Not built.** `cancel()`, `finish()`, `reverse()`, `updatePlaybackRate()`,
  `ready`, `effect`, `timeline`, `startTime`, `onfinish` and `oncancel`;
  `getAnimations({ subtree: true })` and `document.getAnimations()`. Animations of
  `motion`, `animate` and motion values are not listed. `Animation`, `CSSAnimation`
  and `CSSTransition` are exported from `@gpuix/react` and `@gpuix/solid`; they
  cannot be constructed by an app

> **Note: GPUI defaults text color to black, not white.** Unlike CSS, GPUI does not inherit `color` from parent elements. Every `<text>` element that doesn't set an explicit `color` style will render as black — invisible on dark backgrounds. Always set `color` on your text elements or on a parent `<div>` (which applies `text_color` to all children in that subtree via GPUI's `Styled` trait).

### Universal style

A theme cross-fade needs `transition: 'background-color 300ms, color 300ms'` on
every element, and nothing in your tree owns every element. `<UniversalStyle>` is
the CSS `* { ... }` rule: a style layer under the own style of every element.

```tsx
import { UniversalStyle } from '@gpuix/react' // or '@gpuix/solid'

<UniversalStyle style={{ transition: 'background-color 300ms, color 300ms' }} />
```

It renders nothing, applies while it is mounted and follows its `style` prop. Put
it anywhere in the tree (the layer is the window's, not its subtree's). The same
layer is `setUniversalStyle(style | null)` on the mutation queue, for code that
does not use a component.

- **Lowest precedence.** An element's own style and its state styles win
  property by property. Shorthands and longhands interplay as they do for
  `hover` over its base: an own `padding` replaces a universal `paddingTop`, an
  own `transition` replaces the universal one, and a colour, filter or other CSS
  string that is invalid in the element's own style falls through to the layer's
- **Every element.** Including ones mounted later and ones that never set a
  `style`. A string child is a text node, not an element, and gets nothing; a
  `<text>` element does
- **State refinements.** The layer may carry `hover`, `active`, `focusVisible`,
  `focusWithin`, `groupHover`, `groupActive`, `groupFocusWithin`, `hasHover`, `hasActive`, `hasFocus`,
  `hasFocusVisible`, `disabled`, `enabled`, `hasDisabled` and `hasEnabled`, like `*:hover`. Where the element declares the same refinement the two merge, the
  element's winning per property; where only one does, that one is used
- **Not inherited.** It is a declaration on each element, as `* {}` is, so a
  property that does not inherit does not start to
- **Stacking.** Several `<UniversalStyle>` at once are spread over each other in
  mount order, top-level properties only: a later one replaces a shared
  property, and a nested `hover` object whole. A property a later one sets again
  goes after the earlier layers' keys, so its `paddingLeft` beats an earlier
  `padding`, and its `padding` an earlier `paddingLeft`. Unmounting one leaves the others
- **One restyle.** A change of the layer restyles every element once, in one
  frame, with no per-element JavaScript. The result is the same as having set
  every element's style to its new combined one
- **Validation.** The style is checked exactly like `style` on an element. An
  invalid one rejects the batch it is in and nothing changes

The CSS transition rules hold, which is what makes the cross-fade work. With the
layer's `transition` in place, an element whose `light-dark()` colour flips with
`colorScheme` or the window's appearance (`renderer.setAppearance`) fades,
though it never declared a transition. A layer that adds the `transition` and
moves a colour in the same change runs it, as a style that does so would.

```tsx
<UniversalStyle style={{ transition: 'background-color 300ms, color 300ms' }} />
<div style={{ colorScheme: 'light dark', backgroundColor: 'light-dark(#fff, #111)' }}>
  <text>Fades when the appearance flips</text>
</div>
```

Costs and limits:

- A layer with a `transition` makes every element track transitions, which costs
  a little per element per frame. Name the properties as above instead of `all`
- It is one layer. There are no selectors, no specificity and no cascade between
  rules, so it cannot say "every `<text>`" or "every child of a card"
- An element that had no style when the layer first gave it a `transition` has
  no earlier style to fade from, so a colour the same change gives it appears at
  once. Later changes fade

## Automation

Mark elements with **`testId`**, then drive them like Playwright. The same
client works in vitest, inside browser pages, and against a child process.
Mouse actions use the normal GPUI input path in all three hosts.

```tsx
<div testId="sidebar-collapse" onClick={onCollapse}>‹</div>
<textarea testId="composer" value={draft} onChange={...} />
<div testId="send" onClick={onSend}>↑</div>
```

```ts
import { createTestRoot } from '@gpuix/react'
import { connectTest } from '@gpuix/react/automation'
import { ChatApp } from './chat'

const { render, renderer } = createTestRoot()
render(<ChatApp />)
const app = await connectTest(renderer)

await app.screenshot({ path: 'open.png' })

await app.clock.pause()
await app.getByTestId('sidebar-collapse').click()
await app.clock.fastForward(200)
await app.screenshot({ path: 'collapsed.png' })

await app.getByTestId('composer').fill('hello gpuix')
await app.getByTestId('send').click()
await app.screenshot({ path: 'sent.png' })
```

That is the chat example. The real test lives in
[`examples/chat.test.tsx`](https://github.com/remorses/gpuix/blob/main/examples/chat.test.tsx).

```
createTestRoot()          browser render()          launch({ command, args })
       │                         │                              │
       ▼                         ▼                              ▼
connectTest(renderer)      globalThis.gpuix                child stdin / stdout
       │                         │                              │
       └─────────────────────────┴──► App / Locator ◄───────────┘
                                  click, fill, query, clock
```

### Browser apps

Every browser render installs the automation `App` as **`globalThis.gpuix`**.
It is always available after `render()` returns. No setup flag or separate
transport is required.

```ts
await page.evaluate(async () => {
  await globalThis.gpuix
    .getByTestId('sidebar-collapse')
    .click()

  await globalThis.gpuix
    .getByTestId('composer')
    .fill('hello from Playwriter')

  await globalThis.gpuix.clock.pause()
  await globalThis.gpuix.clock.fastForward(200)
})
```

The browser global supports locators, input, tree and text queries, bounds,
selection, scrolling, focus, and clock control. Browser pages cannot write an
arbitrary local screenshot path. Use the controlling browser tool for that:

```ts
await page.screenshot({ path: 'review/chat.png', scale: 'css' })
```

Bounds come back in **canvas pixels**, not CSS pixels, because that is the
coordinate space GPUI lays out in. On a 2x display a locator at `x: 44` sits at
CSS `x: 22`. Convert before handing a rectangle to a browser tool:

```ts
const scale = await page.evaluate(() => {
  const canvas = document.querySelector('canvas')!
  return canvas.width / canvas.clientWidth
})
const { bounds } = await page.evaluate(() =>
  globalThis.gpuix.getByText('New Task').waitFor(),
)
await page.screenshot({
  scale: 'css',
  clip: {
    x: bounds.x / scale,
    y: bounds.y / scale,
    width: bounds.width / scale,
    height: bounds.height / scale,
  },
})
```

Do not read `window.devicePixelRatio` for this. An automation tool can override
the viewport scale factor after GPUI has already sized its canvas, and then the
two disagree.

### Locators

| Call | Matches |
|---|---|
| `app.getByTestId('send')` | The `testId` prop |
| `app.getByText('New chat')` | A node's own text |
| `app.getByType('textarea')` | The host element type |
| `locator.getByText('...')` | A descendant of another locator |

`click()` hits the center of the last painted bounds. `fill(text)` replaces the
focused editor contents. `press('Enter')` presses one key, or several separated by spaces, in the
[key grammar](#simulating-keys). `waitFor()` polls until
exactly one match exists. `textContent()` returns the node's own text plus every
descendant's, like DOM `textContent`.

### Mouse, wheel, and drag

| Call | What it does |
|---|---|
| `locator.hover()` | Moves the pointer to the center, so hover styles and tooltips fire |
| `locator.wheel(dx, dy)` | One wheel event over the center |
| `locator.dragBy(dx, dy)` | Presses on the center, travels, releases |
| `locator.dragTo(target)` | Same, ending on another locator or a `{ x, y }` point |
| `app.mouse.move / down / up / click` | Raw pointer input in window coordinates |
| `app.mouse.wheel(target, dx, dy)` | A wheel over a point or a locator |
| `app.mouse.drag(from, to)` | A drag between two points, two locators, or a mix |

Wheel deltas have the DOM's sign, the one `onWheel` reports in `deltaY`:
positive `dy` scrolls down (the content moves up) and positive `dx` scrolls
right. `renderer.nativeSimulateScrollWheel(x, y, dx, dy)` and
`renderer.dispatchScrollWheel(...)` on the test renderer take the same sign.

A drag sends **interpolated moves**, not one jump, because snapping, live
previews, and per-move commits only appear when the pointer travels. Pass
`steps` to control how many, and `offset` to press away from the center.

```ts
await app.getByTestId('clip-7').dragBy(120, 0, { steps: 6 })
await app.getByTestId('clip-7-trim-end').dragTo(app.getByTestId('clip-8'))
await app.mouse.drag({ x: 240, y: 500 }, { x: 700, y: 620 })
```

Every mouse call takes **`modifiers`** as a hyphenated string (`'cmd'`,
`'cmd-shift'`, `'alt'`), so cmd-wheel zoom, shift-click range selection, and alt-drag duplication are all
testable:

```ts
await app.getByTestId('canvas').wheel(0, -120, { modifiers: 'cmd' })
await app.getByTestId('clip-8').click({ modifiers: 'shift' })
```

`click()` needs painted bounds. **Every element that accepts `testId` records
them**, including `<img>`, `<svg>` and `<anchored>`. An `<anchored>` reports the
box of the overlay itself, not of the trigger it is anchored to, so `click()`
lands on the menu even when it is deferred and snapped back inside the window.

`<virtual-list>` is the exception, and it takes no `testId`. gpui's list is not
an interactive element, so it has nothing to record a box against. Put the
locator on a wrapping `<div>`.

### Screenshots and clock

`app.screenshot({ path })` writes the current GPU frame as a PNG.

`app.clock.pause()`, `set(ms)`, and `fastForward(ms)` freeze native motion time.
Use that to capture a sidebar animation at known timestamps:

```ts
const startedAt = await app.clock.pause()
await app.getByTestId('sidebar-collapse').click()
await app.captureFrames('review/sidebar', [
  startedAt,
  startedAt + 100,
  startedAt + 200,
])
```

### Live apps

`launch({ command, args })` starts the app and speaks the same commands
over stdin as SSE `data:` lines. The app listens only when stdin is a **pipe**,
so a normal terminal run is unchanged. Lines without a `data:` prefix are
ignored; `console.log` cannot break a message.

```ts
import { launch } from '@gpuix/react/automation'

const app = await launch({
  command: 'bun',
  args: ['examples/chat.tsx'],
  env: { GPUIX_BACKGROUND: '1' },
})
await app.getByTestId('composer').fill('hello')
await app.getByTestId('composer').press('Enter')
await app.getByText('hello').waitFor()
await app.screenshot({ path: 'live.png' })
await app.close()
```

Every live-app check must set `GPUIX_BACKGROUND=1`, and the app entry must map
that flag to `focus: false`. On macOS and Windows, automation uses the real
window input and paint pipelines without making the window active, so taking
the user's keyboard has no test benefit. Linux currently ignores `focus`.

`fill()` and `press()` dispatch through the live GPUI window input pipeline, so
native `<input>` and `<textarea>` elements receive GPUI's keyboard and IME
handling instead of a test-only input path.

## Testing

The locators above sit on a **GPU-backed test renderer** (`TestGpuixRenderer`).
It runs the same `GpuixView`, `build_element()`, `apply_styles()`, and event
handlers as production. Test windows are positioned offscreen and rendered by
Metal on macOS or DirectX on Windows. The methods below are the lower-level API
when a locator is not enough.

| Platform | Test renderer | PNG capture |
|---|---|---|
| macOS | Metal | Yes |
| Windows | DirectX | Yes |
| Linux | Not yet | Waiting for GPUI's wgpu headless renderer |

```ts
import { createTestRoot } from '@gpuix/react/testing'

const { root, renderer } = createTestRoot()

root.render(<MyComponent />)
renderer.flush()  // triggers GpuixView::render() on the native GPU

// Simulate events through GPUI's native input pipeline
renderer.nativeSimulateClick(50, 50)
renderer.simulateKeystrokes('Enter')

// Inspect results
const events = renderer.drainNativeEvents()
renderer.captureScreenshot('/tmp/test.png')
const text = renderer.getAllText()
```

`createTestRoot(options)` takes the same window options as a live window (`WindowOptions`), plus `fonts` to register font files, and opens the offscreen window with them through the same translation. The size defaults to 1280x800, not the live 800x600, and `focus` to `false`. What the window does with each option:

| Option | In the test window |
|---|---|
| `width`, `height` | The window size (`getWindowSize()`). |
| `title` | The platform window's title, read with `getWindowTitle()`. `"GPUIX"` when unset. |
| `appearance` | Applied before the window opens; see [Appearance](#appearance). |
| `titlebarTransparent` | The window counts as frameless, so [`appRegion`](#window-drag-regions) applies and a press in a drag region is recorded (below). Without it `appRegion` does nothing, as in a window with a titlebar. |
| `transparent`, `windowBackground` | Set on the native window. |
| `trafficLightX`, `trafficLightY`, `resizable`, `minWidth`, `minHeight` | Set on the native window, which the test renderer cannot observe: it has no way to resize the window or to read the traffic lights back, so a test sees no difference. |
| `show` | `false` never orders the window front. |
| `focus` | `true` makes the offscreen window key and takes the OS's keyboard focus, which is why it defaults to `false`. Focus events come from `setWindowActive`, not from this. |
| `fullscreen` | **Ignored.** A fullscreen window would leave its offscreen position. |
| `x`, `y` | **Ignored.** The test window stays offscreen. |
| `appName`, `appId`, `layerShell` | **Ignored.** The test window has no application menu, and `appId` and `layerShell` are Linux only. |

Simulated input is dispatched the way a window's own input callback does it, so a
handler it runs can read and write the renderer: `getFocusedElementId()`,
`matches()`, `getBoundingClientRect()` and `document.activeElement` answer
current values inside an `onMouseDown`, and a `DismissableLayer` (Dialog,
Popover) can open from one. Only another `nativeSimulate*` call, `advanceTime` or
`captureScreenshot` throws there. The test window is one per thread: making a
`TestRenderer` tears down the previous one. Call `renderer.dispose()` when a test is done with its renderer (`createTestRoot().unmount()` does, after unmounting the root); native calls that need the window throw afterwards. A renderer nobody disposes is leaked when the thread exits, which is safe but keeps the window until then.

### Simulating keys

`simulateKeystrokes`, `nativeSimulateKeystrokes`, `nativeSimulateKeyDown` and
`nativeSimulateKeyUp` on the test renderer, `press()` on a live-app locator and
`keystrokes` / `keyDown` / `keyUp` in the automation protocol all take the
grammar of Playwright's `keyboard.press`, the same names a handler reads back
as `event.key` and `event.code`. A press is a DOM `key` (`"Enter"`, `"a"`,
`"A"`, `"ArrowDown"`, `"Escape"`, `"Tab"`, `"Backspace"`, `"F2"`, `"PageUp"`) or
a `code` (`"Space"`, `"KeyA"`, `"Digit1"`, `"ShiftLeft"`, `"NumpadEnter"`,
`"Numpad1"`), with modifiers joined by `+`
(`"Shift+Tab"`, `"Meta+a"`, `"Control+Shift+P"`, `"Alt+ArrowLeft"`).
`"ControlOrMeta"` is Meta on macOS and Control elsewhere. Names are
case-sensitive, and one that is not a key throws before anything is pressed:
GPUI's own syntax (`"enter"`, `"cmd-shift-p"`) is not accepted.

```ts
renderer.simulateKeystrokes('Shift+Tab')              // one press
renderer.nativeSimulateKeystrokes(id, 'h i Enter')    // a sequence, spaces between presses
renderer.nativeSimulateKeystrokes(id, 'ControlOrMeta+a Backspace')
```

A sequence is space-separated, so the space bar is `"Space"`. Each press is a
`keydown` then a `keyup`, and a key that types a character inserts it into a
focused `<input>` unless a handler cancels the `keydown`: `"a Shift+b"` types
`aB`, and `"A"` types `A` without Shift held, as in Playwright. A held
Control, Alt or Meta types nothing. `nativeSimulateKeyDown(id, press)` and
`nativeSimulateKeyUp(id, press)` send one half of a single press. A press that
names a `code` (`"KeyA"`, `"NumpadEnter"`) reports exactly that `code`, and for
a character key the US character on it (`"a"`, `"A"` with Shift), whatever
layout the machine has; a press that names a key (`"a"`, `"Enter"`) derives
`code` from it for a US QWERTY layout. The browser (wasm) automation path
passes the names through to the page without checking them.

`simulateComposition(id, steps)` focuses an `<input>` or `<textarea>` and runs an
input method composition in it, one step per key the input method takes. A step
is `{ key, action: 'mark', text }` (the composition becomes `text`),
`{ key, action: 'commit', text }` (it ends with `text`) or
`{ key, action: 'unmark' }` (it ends with the text it has). Each step sends the
`"Process"` `keydown`, the composition events and the `change` of the edit, then
the `keyup` of that key, as in [Input method composition](#input-method-composition).
It throws when `id` is not a focusable text input.

```ts
renderer.simulateComposition(id, [
  { key: 'k', action: 'mark', text: 'k' },
  { key: 'a', action: 'mark', text: 'か' },
  { key: 'Enter', action: 'commit', text: 'か' },
])
```

A file drag is simulated in steps, each a native event that reaches your
handlers. `nativeSimulateFileDragEnter(x, y, paths)` starts it,
`nativeSimulateFileDragMove(x, y)` moves it, and it ends with
`nativeSimulateFileDragSubmit(x, y)` (a drop), `nativeSimulateFileDragExit()`
(the files left) or both, then `nativeSimulateFileDragEnd()`.
`nativeSimulateFileDragEnter` and `nativeSimulateFileDragMove` return the
operation macOS would answer the dragging app with: `"copy"`, `"move"` or
`"link"` for a cancelled `dragover`, `"none"` for a refused drop.
`nativeSimulateFileDrop(x, y, paths)` is the whole sequence of a drop: enter,
submit, exit, end. The paths must exist, since native reads their metadata.

### Simulating the mouse

The test renderer remembers the buttons held by `nativeSimulateMouseDown` and
not yet released by `nativeSimulateMouseUp`, and every `nativeSimulateMouseMove`
and `nativeSimulateMouseExit` reports them (`pressedButton`, and the pointer
capture and drag rules that read it), as a real OS puts the held buttons on
every move. A drag is a down, moves, and an up: there is no `pressedButton`
argument. `nativeSimulateMouseMove(x, y, modifiers, movement)` and
`dispatchMouseMove(x, y, modifiers, movement)` take the modifiers and the
motion the platform reports. `nativeSimulateClick` is a down and an up, so it
leaves its button released.

A release that never arrives (the pointer let go outside the window and the OS
sent nothing) is `simulateLostMouseUp(button?)`: the renderer forgets that
button as held and dispatches nothing, so the next move carries no button and
the page sees a lost release, which cancels the press. `button` is `0` (left,
the default), `1` or `2`. The live automation client does the same:
`app.mouse.move` reports what `app.mouse.down` left held.

`renderer.getCursor()` is the CSS keyword of the cursor the window asks the OS for
where the pointer last was, `"default"` when nothing asks. A mouse move updates
it. The offscreen test window is not the active window, so the OS is never told:
this reads what GPUI resolved. Keywords that name one cursor read back as one of
them (`"pointer"`, `"grabbing"`, `"nwse-resize"`, `"nesw-resize"`, `"not-allowed"`).

### Testing native elements

`getAllText()` only sees `<text>` nodes in the retained tree. `<code>`, `<diff>`
and `<markdown>` paint their text inside GPUI, so use `getPaintedText()`, which
returns every string painted in the last frame in paint order:

```ts
root.render(<code code={'a\nb'} language="ts" />)
expect(renderer.getPaintedText()).toEqual(['a', 'b'])
```

Selection has its own helper. Listeners are registered during **paint**, so
`dragSelect` flushes between every step; calling `simulateMouseDown` / `Move` /
`Up` by hand without those flushes selects nothing:

```ts
expect(renderer.dragSelect(20, 30, 900, 300)).toBe('first line\nsecond line')
```

A highlight is a **quad**, so no amount of `getPaintedText()` will show it. Use
`getPaintedHighlights()`, which reports the matched range in UTF-16 units plus
the boxes it actually drew, one per visual row:

```ts
root.render(
  <div highlight={{ query: 'quick' }}>
    <text>the quick brown fox</text>
  </div>,
)
const [hit] = renderer.getPaintedHighlights()
expect(hit.text.slice(hit.start, hit.end)).toBe('quick')
expect(hit.rects).toHaveLength(1)
```

### Window actions in tests

The test window is offscreen and cannot move, so a press in an
[`appRegion: "drag"`](#window-drag-regions) box is recorded instead. A window
counts as frameless, and `appRegion` applies, when it was made with
`titlebarTransparent: true`, as live.
`takeSimulatedWindowActions()` returns what the window was asked to do since the
last call, oldest first: `"move"`, `"titlebarDoubleClick"` (macOS), `"zoom"` or
`"showMenu"` (Linux):

```ts
const { renderer } = createTestRoot({ titlebarTransparent: true })
// render a page with an appRegion: "drag" bar at the top
renderer.nativeSimulateMouseDown(300, 20)
renderer.nativeSimulateMouseUp(300, 20)
expect(renderer.takeSimulatedWindowActions()).toEqual(['move'])
```

The page got no event from that press. Windows hands the region to the OS, so
nothing is recorded there.

### Frame cost in tests

A frame does not build a subtree whose inputs did not change; GPUI replays what
it drew the frame before. The result is the frame a full build gives, and the
test renderer lets you check that and count what a frame did.

What a hover frame does follows from that. Content that did not change and is not
shown (below the window, outside an `overflow` clip, or `opacity: 0`) is not built,
measured, shaped, laid out again, prepainted or painted, so a pointer moving over a
sidebar costs the sidebar whether the page beside it holds 20 cards or 200. What a
frame still pays per subtree is its replay: cards below the window are not reached,
but in a scroller each card is a recorded subtree that is replayed, so a frame costs a
little for each card the scroller holds. A state commit from JS, a motion write, a
programmatic scroll and a read of the automation tree keep this: they ask for a frame
and do not force a full rebuild of it. Focus changes, `scrollIntoView`, the window
title, text selection set from JS, a live image, a font and a background image still
do.

A running animation costs the page what the animation costs. When all that changed
since the last frame is the clock, and everything that runs only changes how an
element paints (a colour, an opacity, a shadow, an `<svg>` stroke), the tick builds
the elements that animate alone and paints them again in the frame already shown, so
one loop on a page of 30000 nodes costs about what it does on a page of 500. An
animation on content that is not shown (below the window, clipped by an `overflow`) is
not built or drawn at all, and its events and `getAnimations()` still advance. A tick
that moves a box (a `width` or inset loop, a `layout` element, a big `<svg>`), or that
follows any other change (the pointer, focus, a scroll, a commit), is a full frame.
Counters you read off the renderer:

- `getBuildCounters()` (`resetBuildCounters()` to start over): elements built
  and skipped, the style and inherited-state copies the build made, and the
  subtrees built whole as units of an animation (`unitsBuilt`, `unitsOversized`
  for those over 256 elements).
- `getLayoutCounters()`: layout nodes made, found again and freed, styles and
  child lists set, passes, and measure calls, since the window was made, the
  text system's lines shaped, wrapped layouts built, line layout hits and
  `shapeCacheHits` (lines the window's shape store gave back: text shaped more than
  two frames ago and mounted again is not shaped again), and the
  subtrees whose extent for culling and whose scrollable overflow were worked out
  again (`extentWalks`, `overflowWalks`, `unsnappedOverflowWalks`).
- `getFrameWorkCounters()`: elements prepainted and painted by themselves (those in
  a culled subtree apart), and the scene primitives that painting inserted against
  the ones a replay put in again. A replayed subtree counts none of the elements
  under it. Subtract two reads to see what one hover frame did to the content that
  did not change.
- `getRetainedCounters()`: subtrees GPUI skipped, replayed and had to build late,
  how many a scroller moved without building them (`relocations`), and why a
  recording could not be replayed or moved.
- `getPatchCounters()`, also on the live `GpuixRenderer` so a window with the display
  link can tell a patched tick from one drawn whole: `patchedFrames` and
  `patchedUnits`, and two JSON objects keyed by reason, `declines` (ticks GPUI drew
  whole after trying to patch them) and `unpatchable` (ticks gpuix did not try to
  patch, such as `Sourced` for a pointer move and `Edited` for a commit). A loop
  that is patched has `patchedFrames` equal to its ticks.
- `getBuiltElementBounds(id)` (test renderer only): the box of the last layout, `null`
  for an element that is not built. `getElementBounds` lays out a skipped
  `contentVisibility` subtree to answer; this one does not, so a test uses it to tell
  what is built.
- `getCulledSubtreeCount()`: subtrees skipped for lying outside their clip. A test
  of what a clipped box still reports (its box, observers, tab order) reads it to
  be sure something was skipped.
- `getLayoutPassCount()` and `getPaintedFrameCount()`: layout-only passes (a
  bounds read) and real paints. The test window draws a dirty window whenever
  effects flush, which a display link never does, so `setInlineDraws(false)`
  stops that and `drawFrame()` ends a frame the way the display does.

To measure a tick, pause the clock (`clockPause()`), call `setInlineDraws(false)`, and
move the clock with `clockAdvance(ms)` before each `drawFrame()`. `clockFastForward(ms)`
also tells the window, so the frame after it is built whole and never a tick.

`setA11yActive(false)` removes the pretend screen reader the test window starts
with. Set `GPUIX_RETAIN_OFF=1` (or call `setRetainEnabled(false)` on one window)
to build every frame in full, for timing, `GPUIX_PATCH_OFF=1` (or
`setPatchEnabled(false)` on one window) to draw every tick of an animation whole
instead of patching it, and
`GPUIX_RETAIN_VERIFY=1` to build each frame that replayed a second time, whole,
and compare the two: a difference is printed as `[retained]` and fails
`unmount()`. Run your suite with it on after a change to a custom element.

### Assert numbers, not pixels

For a stateful surface, paint the state you want to assert into a **readout**
element and read it with `textContent()`. A screenshot tells you that something
changed; a readout tells you what, and the failure message names the number.

```tsx
<text testId="readout">{`x=${scrollX} y=${scrollY} zoom=${zoom} sel=${selected}`}</text>
```

```ts
const readout = await app.getByTestId('readout').textContent()
expect(readout).toBe('x=140 y=60 zoom=24 sel=clip-7')
```

Every test in [`examples/timeline.test.tsx`](./examples/timeline.test.tsx) works
this way, including the drag, trim, snap, and zoom gestures. Keep the screenshot
as well, for a human to look at after the run.

Screenshots land in `packages/react/screenshots/` and `examples/screenshots/`,
both gitignored, so they can be inspected after a run without adding a binary
diff to every commit. The curated set the README links to lives in
`docs/images/` and is regenerated with:

```bash
bun scripts/screenshots.ts
```

## Developing the Rust side

JS remount is covered above. There is **no hot reload for the native half**,
and there cannot be: `require()` of a `.node` file calls `process.dlopen`, Node
has no matching unload, and the live state (GPUI's platform, GPU device, open
window, UI thread, and selection registry) stays inside the loaded library. A
second load would create independent native state while the first library
remains loaded.

The rebuild is fast enough that it does not matter. Measured on an M-series Mac
after touching one file:

| Step | Time |
|---|---|
| `cargo check --lib` | 1.5s |
| `cargo build --lib` | 4.9s |
| `bun run build:debug` (napi) | ~2s |
| One vitest screenshot file | ~2s |

`bun run dev` wires that into a loop: it watches `packages/native/src`,
rebuilds, and re-renders the screenshot tests. **Rust edit to fresh PNGs is
about 4 seconds.**

```bash
bun run dev                      # rebuild, re-render the showcase screenshots
bun scripts/dev.ts --shots diff  # only tests matching "diff"
bun scripts/dev.ts --app native-text   # rebuild, restart an example app
```

Screenshot mode is the better default. Open
`packages/react/screenshots/showcase.png` in Preview.app, which reloads on
write, and unlike a live window the PNG can also be read by an agent.

Two things avoid the rebuild entirely:

- **Content** already lives in props. Change `patch` or `source` and the next
  frame shows it.
- **Design numbers** live in `theme.metrics`. Tuning a row height or heading
  scale is a React re-render.

The test renderer uses `VisualTestAppContext` with a `TestDispatcher` for deterministic scheduling. Event simulation goes through GPUI's coordinate-based hit testing and dispatch — not synthetic JS events.

## Status

- [x] React reconciler with mutation-based protocol
- [x] Atomic `applyBatch()` mutation transport through napi-rs and wasm-bindgen
- [x] RetainedTree (Rust-side element storage)
- [x] Style mapping (CSS properties → GPUI style methods)
- [x] Mouse events (click, mouseDown, mouseUp, mouseMove, mouseEnter, mouseLeave)
- [x] Click outside (`onMouseDownOutside`)
- [x] Scroll wheel events with delta and touch phase
- [x] Scrollable containers (`overflow: "scroll"`) with persistent scroll state
- [x] Programmatic scroll API (`scrollTo`, `scrollToItem`, `scrollIntoView`, `getScrollOffset`)
- [x] Keyboard events (keydown, keyup) with focus management
- [x] Focus/blur events with automatic FocusHandle creation
- [x] GPU-backed test renderer with screenshot capture
- [x] Standalone build (pinned GPUI platform dependencies)
- [x] Native text input and multiline textarea
- [x] Image and SVG elements (`<img>`, `<svg>`), plus `setImage` / `setImagePixels` on `<img>` refs
- [x] Virtual lists (`<virtual-list>`)
- [x] Native text components (`<code>`, `<diff>`, `<markdown>`)
- [x] Cross-element text selection
- [x] Text highlighting and search (`highlight`, `useTextSearch`)
- [x] Headless Select, Combobox, and Tooltip
- [x] Native `hover` and `active` styles
- [x] Window title (`setWindowTitle`)
- [x] Light and dark appearance (`appearance`, `setAppearance`, `getAppearance`, `onAppearanceChange`)
- [x] Native window controls (`minimizeWindow`, `zoomWindow`, `toggleFullscreen`)
- [x] Window chrome (`titlebarTransparent`, `windowBackground`, traffic-light position)
- [x] macOS menu bar with the standard shortcuts (`appName`)
- [ ] App-declared menus and menu callbacks
- [x] Background launch (`focus`, `show`, `activateWindow`)
- [x] Last window close quits the process
- [x] Runtime errors keep the macOS window alive and show a stack overlay
- [x] Debug frame overlay (`debugFrameOverlay` / `setDebugFrameOverlay`)
- [ ] Canvas element
- [ ] Multiple windows
- [x] JS remount under `bun --hot` (`render()` keeps the native window)
- [ ] React Refresh during `bun --hot` (needs a Bun runtime transform)
- [ ] Hot reload of the native `.node` addon. `bun run dev` rebuilds and restarts. Native modules cannot unload.
- [x] Native `motion.div` transitions with deterministic frame capture
- [x] `AnimatePresence` exit transitions for `motion.div`

## Documentation

See [AGENTS.md](https://github.com/remorses/gpuix/blob/main/AGENTS.md) for detailed architecture, communication flow, and contributing guide.

## License

[Apache-2.0](https://github.com/remorses/gpuix/blob/main/LICENSE)
