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
GPUI fires on_click on the element
       │
       ▼
Rust closure calls emit_event_full(callback, 3, "click", {x, y, ...})
       │
       ▼
Desktop ThreadsafeFunction / browser callback sends EventPayload
       │
       ▼
JS event registry: eventHandlers.get(3)?.get("click")?.(payload)
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
| `titlebarTransparent` | boolean | Hide the native titlebar so the app draws chrome under the traffic lights |
| `windowBackground` | `"opaque"` (default), `"transparent"`, `"blurred"` | Window fill. `"blurred"` is the macOS vibrancy backdrop |
| `trafficLightX` / `trafficLightY` | pixels | Traffic-light origin. The chat example uses `(16, 17)` |
| `appearance` | `"system"` (default), `"light"`, `"dark"` | Force the app light or dark, or follow the OS. See [Appearance](#appearance) |
| `transparent` | boolean | Same as `windowBackground: "transparent"` when that option is unset |
| `appName` | string | Name inside the macOS `Hide X` and `Quit X` items. Defaults to `title` |
| `focus` | boolean, default `true` | `false` opens the window behind the active app, like `open -g` |
| `show` | boolean, default `true` | `false` opens the window hidden. Call `activateWindow()` to reveal it |
| `fonts` | string[] | Font files (`.ttf`, `.otf`) read and passed to [`registerFont()`](#custom-fonts) right after the window opens, so `fontFamily: "Inter"` resolves without a system-wide install. GPUIX ships no fonts: the app supplies its own files. Paths resolve against the process's current working directory, so pass absolute ones, e.g. `fileURLToPath(new URL("./fonts/Inter-Regular.ttf", import.meta.url))`. An unreadable path makes `render()` throw. Variable fonts only render their default instance, so pass one static file per weight |

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

`TestRenderer.getAppearance()` reports the system appearance and
`setAppearance` only validates its argument. The test window is a real
offscreen platform window without a run loop, so it never receives an
appearance change and `onAppearanceChange` does not fire under test.

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
the window of the most recently mounted root and throws before any root is
mounted. Outside an adapter, `matchMediaFor(renderer)` from `@gpuix/native/host`
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

The renderer has no resize event, so while a `change` listener is attached to a
query that reads the size, the window size is sampled every 100 ms
(`MEDIA_SIZE_POLL_MS`). Queries that do not read the size cost nothing while idle,
and a `MediaQueryList` with no listener is not subscribed to anything.

Under test, the OS settings start as "no preference" regardless of the machine.
`TestRenderer.setMediaFeatures({ reducedMotion: true })` changes them and
delivers the `change` events. `prefers-color-scheme` reports the system
appearance and cannot be flipped, for the reason given under Appearance.

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

On **macOS**, `startFrameLoop` calls `renderer.tick()` when GPUI's display link
queues a frame, instead of polling every 8 ms. Each tick drains only ready AppKit
events and Core Foundation sources, then returns without blocking JavaScript I/O.
Wakes coalesce while JavaScript is busy. A 100 ms fallback timer pumps events
when no display wake arrives, such as when the window is hidden or occluded.
Input waits for the next pump, not a separate input wake. This improves frame
pacing; it does not eliminate idle CPU use while the display link is running.

Pass `{ frameMs }` to `startFrameLoop` to use fixed-rate timer polling instead.
Call `.stop()` on the returned handle to clear the wake callback and timer.
For manual pumping, `renderer.setFrameWakeCallback(() => renderer.tick())`
registers a weak callback after `init()`. It returns `true` on macOS and `false`
on other platforms. Pass `null` to unregister; an already queued callback may
still arrive. Manual callers must also pump when the display link is stopped.

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

Set **`initial={false}`** (or leave `initial` out) when the element must mount at
its first `animate` target. Later `animate` changes still transition normally. If
a target changes while motion is active, the next transition starts from the
current visible value, so reversing an animation does not jump.

A property that `initial` does not name starts from the element's own `style`
value, as Motion for React reads it from the DOM, and otherwise from the CSS
initial value (`0` for padding, margin and radii, `1` for `opacity`,
`transparent` for `backgroundColor`, the element's `color` for `borderColor` and
`outlineColor`). An unset `width`, `height`, `minWidth`, `minHeight`,
`maxWidth` or `maxHeight` starts from the size layout gave the element, read
back from the previous frame, so the clock starts on the first frame that has it.
Other values CSS leaves to layout or inheritance (`top` unset, `fontSize`,
`color`) have no start yet, so that property begins at its target.

### Targets and timing

Any `StyleDesc` longhand whose value is a number, a length, a colour or a shadow
list can be animated, in `animate`, `initial` and `exit`:

| Target | Range or unit |
|---|---|
| `width`, `height`, `minWidth`, `minHeight`, `maxWidth`, `maxHeight` | pixels or `%`, zero or greater |
| `top`, `right`, `bottom`, `left` | pixels |
| `marginTop`, `marginRight`, `marginBottom`, `marginLeft`, `outlineOffset` | pixels, may be negative |
| `paddingTop` ... `paddingLeft`, `rowGap`, `columnGap`, `borderTopWidth` ... `borderLeftWidth`, the four `border*Radius` corners, `outlineWidth`, `scrollbarWidth`, `fontSize`, `lineHeight`, `flexGrow`, `flexShrink`, `flexBasis`, `aspectRatio` | zero or greater |
| `opacity` | `0` through `1` |
| `fontWeight` | `1` through `1000`, or a keyword such as `"bold"` |
| `backgroundColor`, `color`, `borderColor`, `outlineColor`, `scrollbarColor`, `selectionColor` | any colour GPUIX parses |
| `boxShadow` | one shadow or a list |

`padding`, `margin`, `borderWidth`, `borderRadius` and `gap` are shorthands that
expand to their longhands, so `animate={{ borderRadius: 8 }}` also overrides a
`borderTopLeftRadius` in `style`. When a description names a shorthand and one of
its longhands, the longhand wins.

Naming a discrete property (`display`, `position`, ...) or an unknown key is an
error, not a silent no-op.

**Layout-resolved lengths.** The sizes in the first row move between units, and
`height` also moves to or from `"auto"`:

- `px` to `%` and back runs in px, with the percentage resolved against the
  parent's content box (`width`, `minWidth` and `maxWidth` against its width, the
  height ones against its height). The percentage follows the parent if it
  resizes mid-flight, and the style settles on the unit you asked for. A root
  element has no parent, so it resolves against the window's content size, as the
  initial containing block does in a document.
- `height: "auto"` is the height the content wants. The target follows the
  content while it grows (a spring keeps its velocity), the end is rounded up to
  a whole device pixel so nothing clips, and the style goes back to a literal
  `auto` when the animation settles. `height: 0` to `"auto"` and back is an
  accordion. An element that is not painted (inside a `display: none` subtree)
  has nothing to measure and jumps to its target.
- `width: "auto"` is an error: the width of the content needs a second layout
  pass to measure. Name a pixel width instead.

**Colours** interpolate in Oklab with premultiplied alpha, like CSS
`color-mix(in oklab, ...)`, so a fade to `transparent` keeps its hue and a red
to blue move does not pass through a dull purple. Overshoot is clamped to sRGB.

**Box shadows** animate per layer (offset, blur, spread and colour). When the
lists differ in length, the shorter one is padded with `0 0 0 0 transparent`
layers, as CSS does. If a layer's `inset` differs between the two lists, the
shadow switches at the halfway point instead of interpolating.

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
| `restDelta` | `0.5`, or `0.005` when the move is under 5 | Distance from the target, in the property's unit, under which the spring may settle |
| `restSpeed` | `2`, or `0.01` when the move is under 5 | Speed, in units per second, under which the spring may settle |

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
`motion.polyline`, `motion.polygon` and `motion.g` animate the attributes of a
shape inside an `<svg>`. They take the same `initial`, `animate`, `exit` and
`transition` as `motion.div`, including per-key transitions, springs and
`AnimatePresence`, but the keys are SVG attributes instead of style.

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
| `fill`, `stroke` | Colours, interpolated in Oklab with premultiplied alpha like `backgroundColor`. `none` counts as transparent |
| `cx`, `cy`, `r`, `rx`, `ry`, `x`, `y`, `width`, `height`, `x1`, `y1`, `x2`, `y2` | Numbers in user units |
| `d` | Path data. Both paths are made absolute first; with the same commands every number is interpolated, otherwise the value switches halfway through the progress (or when a spring passes halfway) |

A key that `initial` does not name starts from the element's own attribute, and
then from the SVG default (`fill` black, `stroke` transparent, opacities 1,
`strokeWidth` 1). `r`, `width` and similar have no default, so an animation of
one that the element does not declare jumps to its target. Naming a key that is
not in the table, or a style such as `backgroundColor`, is an error.

Because the `<svg>` is rasterised as one image, an animated shape paints a new
image every frame. The cost is one atlas entry per frame until the rasteriser
replaces the image in place.

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
custom exit that decides when the retained child can unmount.

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
`custom` that returns one. `useIsPresent()` and `usePresence()` work in React. In
Solid, `AnimatePresence` drives the exit from outside the child, so a descendant
component reads them as present.

`motion.div` accepts **`onMotionComplete`**. It runs when the current native
target settles. If React changes the target before an older completion reaches
JavaScript, the stale completion is ignored.

### Motion values

A **motion value** holds a number, a colour string, a list of numbers or
box shadows that changes over time. Put one in `style` where a property
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
  `motion` (tweens and springs, in seconds) and a `target`. The returned
  controls have `stop()`, `cancel()` (back to where it started), `complete()` (to
  the target), `finished` and `then`; the promise never rejects. The target may
  be a list of keyframes (`animate(x, [null, 100, 0], { repeat: Infinity })`, see
  [Keyframes](#keyframes)); `complete()` goes to the last one. A repeating
  animation never finishes on its own. `animate(element, ...)` is not
  supported.
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

Reduced, the position and size properties jump to their target instead of
animating: `width`, `height`, `minWidth`, `minHeight`, `maxWidth`, `maxHeight`,
`top`, `right`, `bottom`, `left`, the margins, the paddings, `gap`, `rowGap`,
`columnGap` and `flexBasis`. Opacity, colours, shadows, radii, borders and the
rest still animate with their transition. The jump still counts as the
animation settling: `onMotionComplete` fires, and `AnimatePresence` exits
complete. Flipping the setting while an animation runs finishes the reduced
properties at once.

`animate()` on a motion value follows the same rule when the value is bound,
through `style` of a `motion.*` component under the config, to one of those
properties: the value jumps and `finished` resolves. A value bound to opacity,
bound to a plain `div`, or not bound to any element animates normally.

CSS `transition` is not affected, as in the DOM. `useReducedMotionConfig()`
(`createReducedMotionConfig()` in Solid, returning an accessor) reports whether
the nearest `MotionConfig` is reduced right now, for animation you drive
yourself. [Layout animations](#layout-animations) follow the setting: a reduced
`layout` or `layoutId` box jumps to its new position.

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
  while it is held and until it arrives; GPUIX has no `zIndex`, so this is done
  by drawing it last.
- A drag follows the pointer after it leaves the item: the item captures the
  pointer when pressed, as any element listening for both `onMouseDown` and
  `onMouseMove` does. A release outside the window is not seen; the next move
  with no button down ends the drag.

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

A container with `overflow: "scroll"` or `"auto"` is natively scrollable. GPUI handles scroll physics, clipping, and offset persistence. Scrollbars are overlays, so the two values differ in nothing. `"hidden"` clips and is a scroll container that only code scrolls: `scrollTop`, `scrollTo` and `scrollIntoView` move it, the wheel and the keys do not, as in a browser. `"clip"` only clips, and `"visible"` (the default) does neither. `overflowX` and `overflowY` set one axis.

Overflow follows CSS. It never changes `display` or wraps the children: a scroller lays out exactly as it would without `overflow`. A child wider than the scroller overflows it and can be scrolled to. A scroller can scroll as far as its children, their padding, and its own padding and border reach, measured from where the padding box starts. Content that fits never scrolls, and neither does an empty scroller.

Plain scroll containers still build every child. Use `<virtual-list>` below when the collection can grow large.

### The wheel

The wheel goes to the innermost scroller under the pointer that can still scroll in the direction of the delta. Each axis is independent: a scroller takes the part of the delta it can use, and the rest goes to the scroller around it. A scroller at its limit hands the whole delta on, which is how a list inside a page lets you carry on scrolling the page. Nested scrollers work. The exception is `<virtual-list>`, which takes every tick under the pointer itself and neither receives nor hands on a delta, so keep other scrollers out of it and it out of other scrollers.

`overscrollBehavior` (and `overscrollBehaviorX` / `Y`) decides what happens to the delta a scroller cannot use. `"auto"`, the default, passes it on. `"contain"` and `"none"` keep it, so the page behind does not move.

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
| `scrollTop`, `scrollLeft` | Read, or set for an instant `scrollTo`. Clamped to the scrollable range. |
| `scrollWidth`, `scrollHeight` | The content's extent, never less than the client size. |
| `clientWidth`, `clientHeight` | The padding box. Overlay scrollbars take nothing from it. |
| `scrollTo({ top, left, behavior })`, `scrollTo(x, y)` | An axis you leave out stays where it is. |
| `scrollBy({ top, left, behavior })`, `scrollBy(dx, dy)` | Relative to the current position. |
| `scrollIntoView({ block, inline, behavior })` | `block` defaults to `"start"` and `inline` to `"nearest"`; each takes `"start"`, `"center"`, `"end"` or `"nearest"`. `scrollIntoView(false)` aligns the end. |

`scrollIntoView` works for a descendant at any depth and scrolls every scrollable ancestor, outermost last, as the DOM does. It aligns by the element's laid-out box, not by its index among its siblings. Inside a `<virtual-list>` it scrolls to the row that holds the element and aligns that row.

`behavior` is `"instant"`, `"smooth"` or `"auto"`. With `"auto"`, or nothing, a scroller follows its `scrollBehavior` style. `scrollBehavior: "smooth"` makes `scrollTo` and `scrollIntoView` animate over about 300 to 450 ms (longer for longer distances), firing `onScroll` each frame. A wheel tick, another `scrollTo` or a drag ends the animation. An explicit `behavior` always wins.

The same calls exist on the renderer for code that holds an id. `scrollTo` takes a negative offset there, the way GPUI keeps it:

```tsx
renderer.scrollTo(elementId, x, y, behavior?)             // set the offset directly
renderer.scrollToItem(elementId, index)                    // a virtual list's row
renderer.scrollIntoView(elementId, block?, inline?, behavior?)
renderer.getScrollOffset(elementId)                        // [x, y], or null
renderer.getScrollMetrics(elementId)                       // [scrollLeft, scrollTop, scrollWidth, scrollHeight, clientWidth, clientHeight]
```

### Sticky positioning

`position: "sticky"` with `top`, `right`, `bottom` and `left` works as in CSS. The element lays out like a relative one and its insets do not move its siblings. Then it is moved to stay inside the scrollport of its nearest scroll container (the window, if none scrolls around it), that far from the edges its insets name, and never out of its parent's content box. A header sticks at the top of the scroller while its section is in view, and the next section's header pushes it out as the section ends. A sticky element also sticks sideways with `left` or `right`.

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

Its insets are pixels. `hidden` and `scroll` containers hold sticky elements the same way, and the geometry (`getElementBounds`, hit testing, `onLayout`) is the stuck position.

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

A row can contain nested `<div>`, `<text>`, `<markdown>`, `<code>`, `<diff>`, `<input>`, and `<textarea>` elements. Focusable rows stay active when they move offscreen, so keyboard input and native editor state are preserved. Those children must not scroll: a `<virtual-list>` takes every wheel tick under the pointer itself and does not hand any to the scrollers around it or inside it (see [Scrolling](#scrolling)).

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

`scrollTo`, `scrollToItem`, and `getScrollOffset` all support virtual lists.

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
skips them for you.

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

An inline box can set `color`, `backgroundColor` (a `<mark>`-style wash),
`fontWeight`, `fontStyle`, `fontFamily`, `textDecoration` (`underline` or `line-through`),
`fontVariantNumeric` and `fontFeatureSettings`. A box inherits from the box
around it, as in CSS.

- A `<text>` anywhere else is a block, and a block child ends a paragraph.
- A nested `<text>` that needs to react is a block too, because an inline box
  cannot take events yet: one with an event listener, a `tabIndex`, a `hover`,
  `active`, `focusVisible`, `groupHover` or `groupActive` style, or a `group`.
  It splits the paragraph around it. A `<span>` is always inline, so give it
  none of these; use a `<text>` when the run needs to be clickable.
- In a `display: flex` or `grid` container each run of raw strings is one item
  and every `<span>` or nested `<text>` is an item of its own, with its content as
  a paragraph.
- Whitespace-only text (space, tab, newline, CR, form feed; not a no-break space)
  makes no box, and neither does a child with `display: none`.
- Text is not collapsed: spaces and newlines inside a string are painted as written.

Not supported inside a paragraph: `fontSize` on a run (GPUI's text runs have no
size), padding, border and margin on an inline box, `fontStyle` (there is no
`fontStyle` style yet), and events or hit testing on a `<span>`. A `<span>` with
a style that needs a box of its own (`position: absolute`, `display: block`,
`flex` or `grid`) is laid out as a block instead.

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
submits its [form](#forms), if it has one. There is no `onSubmit` on `<input>`
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
the row uses GPUI's default leading, so a larger `fontSize` grows the box.
Pass `lineHeight` to set the row in pixels. `minRows` and `maxRows` multiply
that height. An explicit `height` still overrides both.

```tsx
<textarea
  value={draft}
  minRows={1}
  maxRows={8}
  style={{ fontSize: 14, lineHeight: 20 }}
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
shows no caret, takes no editing, and gets no `click` or `keyDown` events. Its
text is painted in the theme's muted colour (`theme.textMuted`), and it is
reported as disabled to assistive technology.

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
gets a macOS-style overlay scrollbar. It is hidden at rest and appears when the scroll offset
changes for any reason (the wheel, `scrollTo`, a key, a drag). It fades out after about a second
without movement, over 200 ms. While the pointer is over the bar it stays, the thumb thickens
from 7 to 11 px, and a faint track shows behind it.

The thumb drags, and that scrolls the container and fires `onScroll`. Pressing the track pages
toward the pointer, by 90% of the viewport. The bar's hitbox exists only where a bar is visible,
so at rest nothing blocks the content under the edge. The bar is an overlay: it never changes
what the container can scroll, and it sits on the right edge, and the bottom for sideways
scrolling, leaving the corner free when both show.

Two CSS properties style it:

```tsx
<div style={{ height: 300, overflow: 'auto', scrollbarWidth: 'thin', scrollbarColor: '#8888 #0001' }}>
  {rows}
</div>
```

| Property | |
|---|---|
| `scrollbarWidth` | `"auto"` (default, 7 px, 11 under the pointer), `"thin"` (4 px, 7 under the pointer), or `"none"`, which draws no bar and leaves the container scrollable. |
| `scrollbarColor` | `"auto"` (default), or `"<thumb> <track>"`: two CSS colours, the thumb and the faint track that shows under the pointer. A value that is not two colours is ignored, as in CSS. |

By default the thumb is 50% black on a light window and 50% white on a dark one, following the
system appearance. A list's bar comes from the heights the list has measured (and
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
along the control's own path), like the DOM. Nothing happens when the click
lands on the control itself, when a handler cancelled the label's `click` with
`preventDefault()`, when the press began elsewhere than where the release
landed (the click then targets a common ancestor, which has no label in its
path), or when the control is `disabled` or `inert`. `preventDefault()` is known
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

- **Implicit submission.** Enter in an `<input>` submits its form. A `keydown`
  handler that calls `preventDefault()` stops it. Enter in a `<textarea>` always
  inserts a newline.
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
`tabIndex`. **A key or focus listener does not make an element focusable.**
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
| `tabIndex={0}` | Joins the tab order, in document order |
| `tabIndex={n}`, `n > 0` | Tabbed first, in ascending order of `n` (document order among equal values), then the `0`s |
| `tabIndex={-1}` | Focusable by click, `focus()` or `focusElement`, but never a Tab stop |
| `autoFocus` | Takes focus once, when its native focus handle is created |
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
`:focus-visible` (see [Focus styles](#focus-styles)). Removing a focused element
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
`window` alone and `event.target` is `undefined`.

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
`"ArrowLeft"`), derived from GPUI's key name with a US QWERTY layout, so on
other layouts a printable key's `code` follows the US position of the
character GPUI reports, and it is `""` when nothing maps. `repeat` is true for
auto-repeat, `isComposing` is always `false` (a key that feeds an IME
composition is consumed by the platform before GPUI sees it), and `shiftKey`,
`ctrlKey`, `altKey` and `metaKey` mirror `modifiers`. The `simulateKeystrokes`
and `press()` test helpers still take GPUI keystroke syntax (`enter`,
`cmd-shift-p`).

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

On macOS, a key sent to an active input method (IME composition) reaches it
before GPUI, so `preventDefault()` cannot cancel composed text.

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

### Tab and Escape are defaults

**Tab** and **Shift+Tab** move focus through the tab order, like a browser. The
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

### Calling the renderer from a handler

Key and focus handlers run in the middle of a native call, and may still use the
renderer: `focusElement`, `scrollTo`, `getScrollOffset`, the clipboard,
`promptForPaths`, `getWindowSize`, `getA11yTree` and the `setWindow*` calls all
run against the window being dispatched. A handler that throws is reported like
any other event error and the next handler still runs.

### Imperative focus

`focusNext()` and `focusPrevious()` map directly to GPUI's
`window.focus_next()` and `window.focus_prev()`.
`focusNextWithin(id)` / `focusPreviousWithin(id)` wrap inside that subtree.
`getFocusedElementId()` returns the host id, or `null`. Browser focus requests made
while WebGPU opens are queued and applied after the first focus handles exist. If
several requests arrive before that render, the latest request wins.

Element refs have the DOM focus methods (React instances and Solid
`HostElement`):

```tsx
const buttonRef = useRef(null)

buttonRef.current.focus()                       // scrolls it into view
buttonRef.current.focus({ preventScroll: true })
buttonRef.current.blur()                        // no-op unless it is focused
buttonRef.current.matches(':focus-visible')

<div ref={buttonRef} tabIndex={-1}>Focused on demand</div>
```

`matches()` supports `:focus`, `:focus-visible` and `:focus-within`; any other
selector throws. Removing `tabIndex` removes the element from the focus order and
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
size itself from the trigger with `width`, `minWidth`, `maxWidth`, `height`,
`minHeight` or `maxHeight` set to `"anchor-size(width)"` or
`"anchor-size(height)"`, resolved natively (the child is transparent for the
one frame before the trigger is first measured). And
`constrainToAvailable` caps the layer's `maxWidth` / `maxHeight` to that room
natively on every layout, with no JS round trip. An explicit `anchor` corner
keeps the older corner-based placement.

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

A filled child of a click target (switch thumb, radio dot, check icon) needs no
special care: a click on it bubbles to the parent's `onClick` like any child's.
Give it `pointerEvents: "none"` only if you want the pointer to treat it as part
of the parent.

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
sends in a microtask, React at commit) and the window is laid out and painted
synchronously if anything changed, so the first read after a mount, or a read
right after a style change in the same turn, answers for the tree as it is. An
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

`renderer.getElementBounds(id)` is the raw read on the live `GpuixRenderer` and
on the test renderer. It does the same flush and draw, and returns `null`
instead of a zero rect when the element has no box. `<input>` reports its border
box like every other element. The browser (wasm) build still answers from the
last painted frame.

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
`borderBoxSize`, `contentBoxSize`) and the observer. The first callback follows
`observe()` for an element that is not 0 by 0, then one per change of the
observed box (`"content-box"`, the default, or `"border-box"`). The content box
is the border box minus the element's own `padding` and `borderWidth`; `hover`
and `active` overrides of those are not counted. An element that stops painting
(`visibility: "hidden"`, a virtual list row outside its window) reports 0 by 0.
`device-pixel-content-box` throws a `TypeError`.

`IntersectionObserver` takes `{ root, rootMargin, threshold }`. `root` is a host
element or `null` for the window. Entries have `isIntersecting`,
`intersectionRatio`, `boundingClientRect`, `intersectionRect`, `rootBounds`,
`time` and `target`. Each target gets an entry after `observe()` (an element
that never paints reports not intersecting) and then one per threshold crossing
or `isIntersecting` flip. The intersection is the target's border box clipped by
every ancestor's `overflow` between it and the root, then by the root.
`rootMargin` takes 1 to 4 lengths in `px` or `%`, and with no `root` it can reach
past the window edge, which does not clip the target.

Callbacks run once per batch of a frame's reports, after the frame, so state set
in one paints a frame later. Observing an element that is not mounted yet is
allowed, as in the DOM: it reports once it has a box, and an `IntersectionObserver`
first reports it as not intersecting. An element that is removed behaves like one
that stopped painting: `ResizeObserver` reports 0 by 0 and `IntersectionObserver`
reports `isIntersecting: false`, once, and nothing after that. Call `unobserve` or
`disconnect` to release it.

Observers work on `<div>`, `<text>`, `<input>`, `<textarea>`, `<img>`, `<svg>`,
`<anchored>`, `<code>`, `<diff>` and `<markdown>`. `<virtual-list>` itself does not
report; its rows do, and a row outside the window reports as not painting.

### Find what is under a point

`getElementsAt(x, y)` returns the ids of the elements whose last painted box
holds a window point, topmost first, like the DOM's `elementsFromPoint`. It
works on the live `GpuixRenderer` and on the test renderer.

```tsx
const [topmost] = renderer.getElementsAt?.(event.x, event.y) ?? []
```

It answers from the last frame, so an element that did not paint (hidden, or
outside a virtual list's window) is never listed. `overflow: hidden` and
`scroll` clip what lies outside them, `pointer-events: none` is skipped (and
inherited, until a child sets `auto`), and a deferred
[`<anchored>`](#overlay-menus) layer comes first, ordered by `priority`,
because gpui paints it after the tree and outside the clip of its ancestors.
Beyond that, tree order is paint order: gpuix has no z-index.

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
| `Backdrop` | Press closes the dialog |
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

The payload is a normal `EventPayload`. `value` is the joined selected text.

Selection works because each painted text element registers itself into a
per-frame registry in **paint order**, which is document order. A drag anchored
in one element resolves against that registry into per-element spans: partial in
the anchor and head, whole for everything between.

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

The raster is made once per size, colour and document. A box that changes size
every frame (a transition, a window drag) keeps drawing the last raster,
stretched to fit, and is rasterised again 100 ms after the size stops changing;
the first change after a pause is sharp at once. Rasterising happens on the
paint thread, so a very large SVG costs a frame when its size changes.

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
`polyline`, `polygon` and `g`. They are valid only inside `<svg>` (anywhere else
is an error), and they are not layout boxes: the `<svg>` paints them all as one
image, so they take no `style` and no events.

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

The rasteriser still keeps only each pixel's coverage and tints it with
`style.color`, so `fill` and `stroke` colours change opacity, not hue. Animate
them with [`motion.path`](#animate-svg-shapes).

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
| Context menu | `onContextMenu` | as `onClick`, on the secondary button **press** |
| Mouse down / up | `onMouseDown`, `onMouseUp` | the mouse fields below, `button`, `detail` |
| Mouse move | `onMouseMove` | the mouse fields below, `pressedButton`, `movementX`, `movementY` |
| Mouse enter / leave | `onMouseEnter`, `onMouseLeave` | `relatedTarget` — do not bubble, fire per element in DOM order |
| Mouse over / out | `onMouseOver`, `onMouseOut` | `relatedTarget` — bubble |
| Pointer events | `onPointerDown`, `onPointerMove`, `onPointerUp`, `onPointerEnter`, `onPointerLeave`, `onPointerOver`, `onPointerOut` | the mouse fields, plus `pointerId` (1) and `pointerType` (`"mouse"`); `onPointerMove` also has `movementX` and `movementY`. `onPointerCancel` fires on the press target when a `draggable` drag starts |
| Drag target | `onDragEnter`, `onDragOver`, `onDragLeave`, `onDrop` | the mouse fields, `dataTransfer` — for files dragged in from the OS and for `draggable` elements; `relatedTarget` on enter and leave. All bubble |
| Drag source | `onDragStart`, `onDrag`, `onDragEnd` | the mouse fields, `dataTransfer` — on a `draggable` element. All bubble |
| Click outside | `onMouseDownOutside` | the mouse fields below, `button` — `target` is the pressed element |
| Key down | `onKeyDown` | `key`, `code`, `keyChar`, `repeat`, `isComposing`, `shiftKey`, `ctrlKey`, `altKey`, `metaKey`, `modifiers` |
| Key up | `onKeyUp` | `key`, `code`, `keyChar`, `repeat`, `isComposing`, `shiftKey`, `ctrlKey`, `altKey`, `metaKey`, `modifiers` |
| Focus | `onFocus` | `focusVisible`. See [Focus events](#focus-events) |
| Blur | `onBlur` | `focusVisible` |
| Focus in | `onFocusIn` (Solid; React's `onFocus`) | `focusVisible`. Bubbles |
| Focus out | `onFocusOut` (Solid; React's `onBlur`) | `focusVisible`. Bubbles |
| Wheel | `onWheel`, `onWheelCapture` | `deltaX`, `deltaY`, `deltaMode`, `clientX`, `clientY`, `offsetX`, `offsetY`, `buttons`, `precise`, `touchPhase`, `modifiers`. Bubbles, cancelable. See [The wheel](#the-wheel) |
| Scroll | `onScroll` | `scrollTop`, `scrollLeft` — scroll containers, not `<virtual-list>` |
| Layout | `onLayout` | `width`, `height`, `x`, `y` — `<div>` and `<text>` only. See also `ResizeObserver` and `IntersectionObserver` under [Measure an element](#measure-an-element) |
| Change | `onChange` | `value` — `<input>` and `<textarea>` only |
| Submit | `onSubmit` | `formData`, `submitter` — `<form>`, bubbling. See [Forms](#forms) |
| Toggle file | `onToggleFile` | `value` (file path) — `<diff>` only |
| Show more | `onShowMore` | `value` (hidden line count) — `<diff>` only |
| Line click | `onLineClick` | `value`, `oldLine`, `newLine` — `<diff>` only |
| Link click | `onLinkClick` | `value` (URL) — `<markdown>` only |
| Selection change | `onSelectionChange` | `value` (joined selected text) — window-level on `render()` |
| Appearance change | `onAppearanceChange` | `value` (`"light"` or `"dark"`) — window-level on `render()` |
| Pointer lock change | `onPointerLockChange` | `target` (the locked element, absent once released) — window-level on `render()` |
| Pointer lock error | `onPointerLockError` | `target` (the element that was asked) — window-level on `render()` |

Every pointer, mouse and drag handler also has a capture-phase twin:
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
`target` and `currentTarget` (element ids), `eventPhase`, `bubbles`,
`cancelable`, `defaultPrevented`, `stopPropagation()`,
`stopImmediatePropagation()` and `preventDefault()`, plus `clientX`/`clientY`,
`offsetX`/`offsetY`, `button`, `buttons`, `detail`, `relatedTarget`,
`pointerId`, `pointerType` and `modifiers` where the event has them.
`x`/`y` are the same as `clientX`/`clientY`. `offsetX`/`offsetY` are relative
to the target's padding edge. Native only produces an event type when the
target or one of its ancestors listens for it (the drag events below are the exception, except `drag`). Capture
handlers run from the root down to the target, then the target's handlers,
then bubble handlers back up. `mouseenter`, `mouseleave`, `pointerenter` and
`pointerleave` do not bubble. Event types are the lowercase DOM names
(`event.type === "pointerdown"`).

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
and can be set to `"none"`. The cursor badge the OS draws cannot reflect whether
the drop will be accepted. Unlike other pointer events, these four are sent to
JS whether or not something listens, since JS needs every `dragover` to know
whether the drop was accepted.

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
  `dropEffect` `"none"`. Escape still reaches the window's key handlers and layers.
- `event.preventDefault()` in `onDragStart` cancels the drag. The drag has
  already started when JS runs, so a move or two may already have been
  sent; JS drops them, and pointer events carry on from the next input.
- A drag does not start while another element holds pointer capture. If the source itself
  holds it, it is released (`lostpointercapture`) as the drag starts.

`event.dataTransfer` is one object for the whole drag, in DOM protected mode:
`setData`, `getData`, `clearData`, `types`, `effectAllowed` and `dropEffect`.
`setData` and `effectAllowed` work in `dragstart` only. `getData` works in
`dragstart` and `drop` and returns `""` in every other event, `dragover` included.
`setDragImage` is accepted and does nothing: GPUI has no OS drag, so there is no
drag image and nothing follows the pointer. The drag stays inside the window: it
cannot drop on another app, and a drag from another app is a file drag.

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

**`onLayout`** is `ResizeObserver` for an element. It fires after the first paint
that has the listener and again each time the element's **size** changes, with
its border box: `width` and `height`, and `x` and `y` in window pixels. Moving
without resizing, as a row does when a list scrolls, fires nothing. Removing the
listener and adding it back reports the size again. An element with
`visibility: "hidden"` is not painted, so it reports nothing until it is shown.

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
`select` that is `disabled`, or on an element inside one, as in HTML.
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

The call crosses from JS to the UI thread without waiting for it, so the capture
**applies from the next native pointer event**, not within the handler that asked
for it. `hasPointerCapture` answers for the request, so it is `true` right after
`setPointerCapture`. Calling `setPointerCapture` with no button down does
nothing, as in Chrome. `renderer.setPointerCapture(elementId, pointerId)`,
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

render(<App />, {
  onPointerLockChange(event) {
    setLocked(event.target !== undefined) // the locked element's id, absent once released
  },
  onPointerLockError(event) {
    console.warn('could not lock', event.target)
  },
})
```

While the lock is held, pointer and mouse events target the lock element, as
with capture, wherever the mouse is, and `clientX`/`clientY` stay at the
position the pointer had when the lock began. The motion is in `movementX` and
`movementY`. `renderer.exitPointerLock()` releases it, and
`renderer.pointerLockElement()` returns the locked element's id or `null`.
`renderer.requestPointerLock(elementId)` is the same call as the ref's.

Like the DOM's, the request is asynchronous. It returns at once and the next
frame applies it, so `pointerLockElement()` is still `null` right after the
call. The outcome arrives as one of two **window-level** callbacks on
`render()` / `createRoot()`, the same attachment as `onAppearanceChange`, because
in the DOM they fire at `document`:

- `onPointerLockChange` fires when the lock element changes: the lock was
  granted, moved to another element, or ended. The event's `target` is the
  locked element, absent once the lock is gone. A lock ends by
  `exitPointerLock()`, when the window loses focus (the platform releases it),
  and when the locked element leaves the tree.
- `onPointerLockError` fires when a request fails: the element is not in the
  tree, or the platform refused. `target` is the element that was asked.

Asking again for the element that already holds the lock does nothing. Asking
for another one moves the lock without releasing the cursor.

Only macOS can lock the pointer (`CGAssociateMouseAndMouseCursorPosition`), and
only for the active window. On Windows and Linux every request fails with
`onPointerLockError`.

`movementX` and `movementY` are on every `pointermove` and `mousemove`, locked or
not: how far the pointer moved since the previous move, in logical pixels, from
the platform's own relative motion. Required in the types of `onMouseMove` and
`onPointerMove` (`HostMouseMoveEvent`, `HostPointerMoveEvent`); other pointer
events do not carry them. They are `0` where the platform reports no relative
motion (everything but macOS).

Under test the offscreen window is never active, so a real lock is always
refused. `TestRenderer` runs a simulated platform instead: `requestPointerLock`
and `exitPointerLock` draw the frame they ask for and deliver the events,
`simulatePointerLockGranting(false)` makes the platform refuse, and
`simulatePointerLockLost()` drops the lock as a deactivation does. The
simulated platform reports no motion unless a move says so:
`nativeSimulateMouseMove(x, y, pressedButton, modifiers, { x: 4, y: -3 })`.

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
> - **Few shorthand values.** `padding` takes numbers and `margin` takes a
>   number, a percentage or `"auto"`, so CSS strings like `"0 16px"` or
>   `calc()` are ignored. `border`, `borderColor` and `boxShadow` do take CSS
>   strings, see below.
> - **`boxShadow` takes the CSS string or a structured object.** See below.
> - **No `<button>`.** Use `<div onClick>` with `cursor: "pointer"`.
> - **Do not nest `<text>` in `<text>`.** Adjacent `<text>` siblings merge
>   into one line. A `<text>` child of another `<text>` is a nested div.
> - **`<input>` has no default inner padding.** Set `padding` on the input
>   style, or pad the parent wrapper. The input clips to its own
>   `borderRadius` automatically.

**Layout:** `display` (`"flex"` | `"grid"` | `"block"` | `"none"` | `"contents"`), `flexDirection` (`"row"` | `"row-reverse"` | `"column"` | `"column-reverse"`), `flexWrap` (`"nowrap"` | `"wrap"` | `"wrap-reverse"`), `flexGrow`, `flexShrink`, `flexBasis`, `order`, `alignItems` (`"start"` | `"center"` | `"end"` | `"stretch"` | `"baseline"`), `alignSelf`, `alignContent`, `justifyContent` (adds `"space-between"`, `"space-around"` and `"space-evenly"`), `justifyItems`, `justifySelf`, `gap`, `rowGap`, `columnGap`. Grid: see [Grid](#grid)

**Sizing:** `width`, `height`, `minWidth`, `minHeight`, `maxWidth`, `maxHeight` — accepts pixels (number) or percentages (string like `"100%"`). `aspectRatio` is a number or a CSS ratio (`"16 / 9"`, `"1"`) and sizes whichever axis has no definite size

**Spacing:** `padding`, `paddingTop/Right/Bottom/Left`, `margin`, `marginTop/Right/Bottom/Left` — margins take pixels, a percentage of the containing block's width, or `"auto"` (which absorbs free space in flex layouts, so `marginLeft: "auto"` pushes an item to the end)

**Position:** `position` (`"static"` (default) | `"relative"` | `"absolute"` | `"fixed"` | `"sticky"`), `top`, `right`, `bottom`, `left` (pixels, percentages or `"auto"`). An absolute box resolves its insets, percentage sizes and margins against the padding box of its nearest non-static ancestor (the window if there is none; a sticky ancestor counts) and keeps its static position on an `auto` axis. `"fixed"` resolves against the window. `zIndex` orders positioned siblings, and a sticky element is positioned. See [Sticky positioning](#sticky-positioning) for `"sticky"`

**Visual:** `background`, `backgroundColor`, `backgroundImage`, `backgroundSize`, `backgroundPosition`, `backgroundRepeat`, `backgroundOrigin`, `backgroundClip`, `color`, `colorScheme`, `opacity`, `cursor`, `pointerEvents`, `borderRadius`, `borderTopLeftRadius`, `borderTopRightRadius`, `borderBottomLeftRadius`, `borderBottomRightRadius`, `borderWidth`, `borderTopWidth`, `borderRightWidth`, `borderBottomWidth`, `borderLeftWidth`, `borderColor`, `borderTopColor`, `borderRightColor`, `borderBottomColor`, `borderLeftColor`, `borderStyle` (`"solid"` | `"dashed"`), `border`, `borderTop`, `borderRight`, `borderBottom`, `borderLeft`, `boxShadow`, `filter`, `backdropFilter`, `mask`, `maskImage`, `maskSize`, `maskPosition`, `maskRepeat`, `maskOrigin`, `maskClip`, `clipPath`, `outlineWidth`, `outlineColor`, `outlineOffset`

### Borders

Widths, colours and corners can differ per side:

```tsx
<div style={{ borderBottomWidth: 2, borderBottomColor: '#89b4fa' }} />
<div style={{ borderWidth: 4, borderColor: 'red blue' }} />   {/* top/bottom red, left/right blue */}
<div style={{ border: '1px solid rgba(0,0,0,.2)', borderTop: '2px dashed currentColor' }} />
```

`borderColor` takes one to four colours with the CSS meaning. `border` and
`borderTop` / `borderRight` / `borderBottom` / `borderLeft` take
`<width> <style> <color>` in any order, with `solid` or `dashed` as the style.
No style means no border, a missing colour is `currentColor`, and a missing width
is 3px (`medium`; `thin` is 1px and `thick` 5px). Other line styles (`dotted`,
`double`, ...) make the value invalid, so it is ignored. `border` is applied
first, then the side shorthands, then the longhands (`borderWidth`,
`borderTopColor`, ...). The line style is one for the whole element, because GPUI
draws one: a `dashed` anywhere makes every side dashed.

### Layout like CSS

- **Text nodes.** An empty text node produces no box, and neither does a
  white-space-only one that sits directly in a `div`, so `{" "}` and the empty
  placeholders Solid creates for `<Show>` and `<For>` no longer add a flex or
  grid item (and a `gap`). Inside a `<text>` white space is kept.
- **`display: "none"`** builds no box for the element or its descendants, and
  leaves no gap. **`display: "contents"`** also builds no box: its children are
  spliced, recursively, into the parent's child list, so they are the parent's
  flex or grid items (and are sorted by `order` with them). The element's own box
  styles (size, padding, background, border) do not apply, its inheritable text
  style (`color`, `fontSize`, `fontFamily`, `fontWeight`, `textAlign`,
  `lineHeight`, `whiteSpace`, `cursor`, `visibility`, `userSelect`) passes to the
  children, and `getElementBounds` returns `null` for it. Its own pointer
  handlers never fire, as it has no hitbox. The `inline-*` values and `table`
  are not supported (the layout engine has no such box types).
- **Static position.** An unpositioned box ignores `top`/`left` and is not a
  containing block. Known difference from CSS: an `overflow: "hidden"` static
  ancestor still clips an absolute descendant whose containing block is further
  up, and a scroller scrolls it.
- **`<img>`** is a replaced element. A missing dimension comes from the other
  one and the image's intrinsic ratio (or `aspectRatio`), a percentage of an
  indefinite parent counts as `auto`, and with neither set it takes its
  intrinsic size. `maxWidth` keeps the proportions.
- **Paint order.** Within a parent, in-flow children paint first, then children
  with a `position` other than `static`, in tree order. Hit testing follows
  paint order, so an `absolute` overlay listed before an in-flow sibling is on top
  of it. `zIndex` (an integer, positioned children only) puts a child below the
  in-flow ones when negative and above them when positive. Each parent is its
  own stacking context: `zIndex` orders siblings only, never against cousins.
  Tab order stays in document order.
- **`order`** stable-sorts the children of a flex or grid container. Layout and
  paint follow it; tab order stays in document order.

#### Grid

`gridTemplateColumns` and `gridTemplateRows` take a CSS track list: `px`, `%`
and `fr` units, `auto`, `min-content`, `max-content`, `minmax()`,
`fit-content()`, `repeat(n | auto-fill | auto-fit, ...)`, `[line names]` and
`none`. A bare number `n` still means `repeat(n, minmax(0, 1fr))` (with
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
tested; on Linux (X11 and Wayland) the default cursor stays visible.

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

`backgroundImage` takes a comma list of CSS gradients, the first on top:
`linear-gradient()`, `radial-gradient()`, `conic-gradient()` and their
`repeating-` forms, with any number of stops, colour hints, `in srgb` / `in
oklab`, and `none`. Colours are any [colour form](#colors), including
`currentColor`. `backgroundSize`, `backgroundPosition`, `backgroundRepeat`
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

Longhands win over the shorthand. Limits:
- `url(...)` images are valid but not drawn: that layer is skipped and the others
  still paint.
- Interpolation spaces other than `srgb` and `oklab` skip the layer.
- `backgroundOrigin` and `backgroundClip` move the gradient layers only. The
  background *colour* fills the border box, or the glyphs under `text` (below).
- `backgroundAttachment` has no meaning here and is ignored in the shorthand.

#### Gradient text

`backgroundClip: 'text'` (or `text` in the `background` shorthand) paints a layer
only through the glyphs of the element and its descendants, nested inline spans
included. The gradient is laid out on the element's box, as in CSS, so it spans
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
lengths are `px` (or a bare `0`), `inset` and the colour may lead or trail the
lengths, the colour can be any [colour form](#colors) and defaults to
`currentColor`, and `none` paints nothing. An invalid string paints no shadow.

```tsx
<div style={{ boxShadow: '0 1px 2px rgba(0,0,0,.2), inset 0 0 0 1px red' }} />
```

The structured form's fields are `offsetX`, `offsetY`, `blurRadius`, `spreadRadius`,
`color`, and `inset`, which draws inside the border box:

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

`filter` and `backdropFilter` take the CSS function list: `blur()`, `brightness()`,
`contrast()`, `grayscale()`, `hue-rotate()`, `invert()`, `opacity()`, `saturate()`,
`sepia()` and `drop-shadow()`, applied in order, or `none`. Amounts are numbers or
percentages, lengths are `px` (or a bare `0`), and angles are `deg`, `rad`, `turn`
or `grad`. A `drop-shadow()` colour can be any [colour form](#colors) and defaults
to `currentColor`. An invalid string is ignored, as in CSS, and `url()` references
are not supported. `filter` applies to the element and everything in it, and
`backdropFilter` to what is painted behind it, so give the element a translucent
background for the effect to show. `opacity` also composites the element as a
group, so overlapping children of a half-transparent parent do not show through
each other.

```tsx
<div style={{ filter: 'blur(4px) brightness(1.2)' }} />
<div style={{ backdropFilter: 'blur(12px)', backgroundColor: 'rgba(255,255,255,0.4)' }} />
<div style={{ filter: 'drop-shadow(2px 4px 6px rgba(0,0,0,.4))' }} />
```

`maskImage` multiplies the element and everything in it by the alpha of a comma
list of gradients (the same forms as `backgroundImage`, first on top), so
`transparent` hides and any opaque colour shows. `maskSize`, `maskPosition`,
`maskRepeat`, `maskOrigin` and `maskClip` are comma lists that repeat to cover the
layers, and `mask` is the shorthand (layers, `position / size`, repeat, and origin
and clip boxes). The longhands win over it. `maskOrigin` and `maskClip` start at
`border-box`, not at the `padding-box` a background starts from. A layer that does
not cover an area leaves it hidden, so a non-repeating mask hides what it does not
reach. A mask composites the element as a group, like `opacity` and `filter`.

```tsx
// Fade the top and bottom edge of a scroller.
<div style={{ overflow: 'scroll', maskImage: 'linear-gradient(to bottom, transparent, black 20px, black 80%, transparent)' }} />
<div style={{ mask: 'radial-gradient(circle, black 60%, transparent 61%) center / 24px 24px' }} />
```

Limits: `url(...)` masks are valid but not drawn, so that layer is skipped.
`maskMode` and `maskComposite` have no props, and only the alpha of a gradient is
used. `no-clip` and `margin-box` are not supported, so a value using them is
ignored. `fill-box` is `content-box` and `stroke-box` and `view-box` are
`border-box`.

`clipPath` clips the element and everything in it to a basic shape: `inset()`
(one to four insets and `round` radii), `circle()` and `ellipse()` (a radius,
`closest-side` or `farthest-side`, and `at <position>`), `polygon()` (an optional
`nonzero` or `evenodd`, then points), or `none`. Lengths are `px` or `%`. Write a
`border-box` (the default), `padding-box` or `content-box` before or after the
shape to choose what the lengths are relative to. An invalid value is ignored, as
in CSS. A clip path composites the element as a group, like `opacity` and `mask`.

```tsx
<div style={{ clipPath: 'circle(50% at 50% 50%)' }} />
<div style={{ clipPath: 'polygon(50% 0, 100% 100%, 0 100%)' }} />
<div style={{ clipPath: 'inset(8px round 12px)' }} />
```

Limits: `url()`, `path()`, `rect()` and `xywh()` are not supported, and neither is a
box with no shape, so a value using them is ignored. `margin-box` and `stroke-box`
and `view-box` are `border-box`, and `fill-box` is `content-box`. `inset()` rounds
its corners with circles, so the vertical radii after `/` in `round` are dropped.
Clip path does not clip hit testing yet: a click on the clipped-away part of the
element still reaches it.

**Overflow:** `overflow`, `overflowX`, `overflowY` — `"clip"` clips content, `"hidden"` clips and scrolls only from code, `"scroll"` and `"auto"` create a native scrollable container with persistent scroll state. `overscrollBehavior`, `overscrollBehaviorX`, `overscrollBehaviorY` (`"auto"` | `"contain"` | `"none"`) and `scrollBehavior` (`"auto"` | `"smooth"`) — see [Scrolling](#scrolling)

**Text:** `fontSize`, `fontFamily`, `fontWeight`, `fontStyle` (`"normal"` | `"italic"` | `"oblique"`, inherited), `textAlign`, `lineHeight`, `whiteSpace`, `textOverflow`, `lineClamp`, `textDecoration` (`"underline"` | `"line-through"` | `"none"`)

`fontVariantNumeric` (`"tabular-nums"`, `"proportional-nums"`, `"lining-nums"`, `"oldstyle-nums"`, `"slashed-zero"`, `"diagonal-fractions"`, `"stacked-fractions"`, `"ordinal"`, `"normal"`, one keyword per group, combined with spaces) and `fontFeatureSettings` (`'"tnum" 1, "liga" 0'`) turn OpenType features on or off. Both inherit, and a descendant that sets one keeps the other. When both name a feature, `fontFeatureSettings` wins. The font needs the feature, and neither prop is available inside `hover`, `active` or `focusVisible`.

`fontFamily` resolves installed fonts. To use your own, pass the files in the `fonts` window option or call [`registerFont()`](#custom-fonts). Variable fonts render only their default instance, so use one static file per weight. The family name must match the font's family name exactly.

**Selection:** `userSelect` (`"text"` | `"none"`), `selectionColor` — both inherit down the tree

**Focus:** `focusVisible`, `focusWithin`. See [Focus styles](#focus-styles)

**Ancestor state:** `group`, `groupHover`, `groupActive`. See [Group state styles](#group-state-styles)

### Group state styles

There is no stylesheet, so an element cannot react to a hovered ancestor with a
selector. Mark the ancestor with `group` and style the descendants with
`groupHover` and `groupActive`, like Tailwind's `group-hover:`:

```tsx
<div style={{ group: true, padding: 12, backgroundColor: '#fff' }}>
  <text style={{ color: '#666', groupHover: { color: '#000' } }}>Title</text>
  <div style={{ opacity: 0, groupHover: { opacity: 1 }, groupActive: { backgroundColor: '#eee' } }} />
</div>
```

`group: true` is the default group, which an unnamed `groupHover` follows. Name a
group (`group: 'card'`) and point at it with `groupHover: { group: 'card',
backgroundColor: '#eee' }` when groups nest: the style follows the nearest
ancestor with that name. `groupHover` applies while the pointer is over the
group or anything inside it, `groupActive` while the mouse is down inside it.
Each element takes one `groupHover` and one `groupActive`. The group's state is
the pointer dispatcher's, the same that drives `hover` and `active`, and a
group applies to its descendants, not to its own element. They fold in with the
element's own states, later winning: base, `groupHover`, `hover`, `groupActive`,
`active`. So a `color` set by a group state reaches the descendants'
`currentColor` like one set by `hover`. `groupFocusWithin` is not available.

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
`<diff>`, `<img>`, `<svg>` and the editors. The one exception is
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
    focusVisible: { outlineWidth: 2, outlineColor: '#89b4fa', outlineOffset: 2 },
  }}
/>
```

`focusVisible` needs a **focusable** element: `tabIndex`, `<input>`,
`<textarea>`, or a primitive such as `Button`.

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
  focusVisible: { outlineWidth: 2, outlineColor: '#89b4fa' }, // when focused
}} />                                                         // dims otherwise
<div tabIndex={0} style={{ keyboardFocusDim: false }} />      // never dims
```

**Turn the dim off for the whole window.** Pass `keyboardFocusDim: false` to
`render()` (or `createTestRoot()`), then style focus yourself with
`focusVisible`.

```tsx
render(<App />, { keyboardFocusDim: false })
```

**Outline, not border.** `outlineWidth`, `outlineColor` and `outlineOffset` draw
a line outside the border box, like CSS `outline`. It takes **no layout space**,
so a ring you add with `focusVisible` moves nothing. A negative offset draws it
inside. It follows `borderRadius`. A parent with `overflow: "hidden"` clips it,
like in a browser.

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
  `transitionDuration`, `transitionTimingFunction` and `transitionDelay` win over
  the shorthand, and shorter lists repeat to the length of `transitionProperty`.
  Every property `motion` can animate (see [Targets and timing](#targets-and-timing))
  can transition. Shorthand names (`padding`, `margin`, `border-radius`, `inset`,
  `flex`, ...) cover their longhands. A valid CSS name gpuix does not animate
  (`transform`) is accepted and matches nothing. A camelCase name, a bad time or
  timing function, or an unparsable value is rejected when the style is set, like
  any other invalid style
- **Timing functions.** `linear`, `ease`, `ease-in`, `ease-out`, `ease-in-out`,
  `cubic-bezier()` (it may overshoot), `steps(n, jump-start | jump-end |
  jump-none | jump-both | start | end)`, `step-start`, `step-end` and `linear()`
  stop lists. `motion` accepts the same strings as `ease`
- **What starts one.** Each frame the element's style, with the state styles that
  apply folded in, is compared with the previous frame's. A changed value starts a
  transition when its duration plus delay is above zero. The first style an
  element mounts with never transitions. A property that is unset starts from its
  CSS initial value (`opacity` 1, padding `0`); one CSS leaves to layout (`auto`,
  an unset `width`) changes instantly
- **`px` and `%`.** A `width`, `height`, `minWidth`, `minHeight`, `maxWidth` or
  `maxHeight` that moves between a pixel length and a percentage transitions in
  px, with the percentage resolved against the parent's content box (the window's
  content size for a root). It follows the parent while it runs and settles on the
  literal target unit. Two percentages interpolate as percentages
- **`auto` heights.** As in a browser, a change between a length and `auto` is
  discrete unless `interpolateSize: 'allow-keywords'` is set on the element or an
  ancestor (it inherits, default `'numeric-only'`). That covers `height` to or from
  `auto` or unset, and an unset `minHeight` / `maxHeight` (CSS computes them to
  `auto` / `none`). With it set, the transition starts from the laid-out height and
  runs to the content's height, follows the content while it grows, and hands back
  to `auto` at the end. `width` to or from `auto` stays discrete. `motion` animates
  to `height: 'auto'` whatever `interpolateSize` is, since that is Framer's
  behaviour
- **Interrupts.** A change mid-flight starts from the visible value, not the old
  target. Returning to where the transition began shortens the way back in
  proportion, as CSS specifies. Changing `transition*` does not touch a running
  transition, but dropping its property from `transitionProperty` cancels it. A
  negative delay starts part-way through
- **State styles.** `hover`, `active` and `focusVisible` are folded into the style
  that is compared, so they animate in and out. A `transition` set inside `hover`
  is the one used while entering hover; the base one is used leaving it. An
  element that declares a transition has its state styles applied by gpuix instead
  of GPUI's per-element refinements, so they show one frame after the pointer
  moves. Elements without a transition behave as before
- **`motion`.** A property `animate` names belongs to `motion` and is not
  transitioned
- **Events.** `onTransitionRun`, `onTransitionStart` (after the delay),
  `onTransitionEnd` and `onTransitionCancel` carry `propertyName` (`"width"`,
  `"background-color"`), `elapsedTime` in seconds and `pseudoElement: ""`. They
  fire on the transitioning element only and do not bubble yet

Not supported: `transition-behavior: allow-discrete`, so `display` and other
discrete properties never transition; and the one transition-start case where an
element's first style has no `transition` and a later style adds one together with
a changed value (the change applies instantly; the next one animates).

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
  value, plus `animationTimingFunction`, which eases from that keyframe to the
  next. Blocks at the same offset merge, the later key winning. A property a
  definition leaves out of `from` or `to` starts or ends at the element's own
  value, as in CSS. A bad definition throws from `defineKeyframes` and keeps the
  earlier one
- **One registry for the process.** `@keyframes` is document-global, so a
  definition reaches every window and test root, including ones created later.
  Define keyframes once at module scope
- **Properties.** `animation` is a comma list of `<name> <duration> <timing
  function> <delay> <iteration count> <direction> <fill mode> <play state>` in any
  order: the first time is the duration, the second the delay. `animationName`,
  `animationDuration`, `animationTimingFunction`, `animationDelay`,
  `animationIterationCount`, `animationDirection`, `animationFillMode` and
  `animationPlayState` win over the shorthand, and shorter lists repeat to the
  length of `animationName`. The timing functions are the ones
  [CSS transitions](#css-transitions) take. An invalid value is rejected when the
  style is set. A name with no keyframes animates nothing, as in a browser
- **Behaviour.** CSS Animations Level 1: `infinite` and fractional iteration
  counts, `reverse`, `alternate` and `alternate-reverse`, `backwards` / `forwards`
  / `both` fill, `paused` (freezes where it is and resumes from there), a negative
  delay that starts part-way through, and a restart when `animationName` changes
- **State styles.** `hover`, `active` and `focusVisible` may declare `animation`.
  It starts when the state applies and is cancelled when it ends
- **Cascade.** Animations win over the element's style, its state styles, running
  CSS transitions and `motion`, as animations do over normal declarations in CSS
- **Events.** `onAnimationStart` (after the delay), `onAnimationIteration` (each
  iteration after the first, not at the end), `onAnimationEnd` and
  `onAnimationCancel` carry `animationName`, `elapsedTime` in seconds and
  `pseudoElement: ""`. `onAnimationCancel` fires when the animation is removed or
  its name changes before it ended; one that already ended is not cancelled. They
  fire on the animating element only and do not bubble yet

Differences from a browser:

- Redefining a name affects animations that start afterwards. One already running
  keeps the keyframes it started with, where a browser re-resolves it live
- A running CSS transition does not beat an animation on the same property. The
  animation always wins, and a transition never starts from a value an animation
  drove, because transitions compare the style beneath the animations
- Only the properties `motion` can animate are animatable, so `transform` and
  discrete properties such as `display` cannot be keyframed
- `animation-composition`, `animation-timeline` and `animation-range` are not
  supported
- `animation-duration: auto` is `0s`
- A size keyframe against `auto` (or an unset size) is discrete unless
  `interpolateSize: 'allow-keywords'` applies, as for transitions

> **Note: `white-space: pre` is not supported.** GPUI's text system only has `normal` (wraps) and `nowrap` (single line). To preserve newlines like HTML `<pre>`, split your text on `\n` in React and render each line as a separate `<text>` element in a flex column:
>
> ```tsx
> <div style={{ display: 'flex', flexDirection: 'column', fontFamily: 'Menlo' }}>
>   {code.split('\n').map((line, i) => (
>     <text key={i} style={{ whiteSpace: 'nowrap' }}>{line}</text>
>   ))}
> </div>
> ```

> **Note: GPUI defaults text color to black, not white.** Unlike CSS, GPUI does not inherit `color` from parent elements. Every `<text>` element that doesn't set an explicit `color` style will render as black — invisible on dark backgrounds. Always set `color` on your text elements or on a parent `<div>` (which applies `text_color` to all children in that subtree via GPUI's `Styled` trait).

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
focused editor contents. `press('enter')` sends one key. `waitFor()` polls until
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

A drag sends **interpolated moves**, not one jump, because snapping, live
previews, and per-move commits only appear when the pointer travels. Pass
`steps` to control how many, and `offset` to press away from the center.

```ts
await app.getByTestId('clip-7').dragBy(120, 0, { steps: 6 })
await app.getByTestId('clip-7-trim-end').dragTo(app.getByTestId('clip-8'))
await app.mouse.drag({ x: 240, y: 500 }, { x: 700, y: 620 })
```

Every mouse call takes **`modifiers`** in the same syntax as `press('cmd-a')`,
so cmd-wheel zoom, shift-click range selection, and alt-drag duplication are all
testable:

```ts
await app.getByTestId('canvas').wheel(0, 120, { modifiers: 'cmd' })
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
await app.getByTestId('composer').press('enter')
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
renderer.nativeSimulateKeystrokes('enter')

// Inspect results
const events = renderer.drainNativeEvents()
renderer.captureScreenshot('/tmp/test.png')
const text = renderer.getAllText()
```

`createTestRoot({ width, height, fonts })` sizes the offscreen window and registers font files with `registerFont()`, the same as the `fonts` window option.

A file drag is simulated in steps, each a native event that reaches your
handlers. `nativeSimulateFileDragEnter(x, y, paths)` starts it,
`nativeSimulateFileDragMove(x, y)` moves it, and it ends with
`nativeSimulateFileDragSubmit(x, y)` (a drop), `nativeSimulateFileDragExit()`
(the files left) or both, then `nativeSimulateFileDragEnd()`.
`nativeSimulateFileDrop(x, y, paths)` is the whole sequence of a drop: enter,
submit, exit, end. The paths must exist, since native reads their metadata.

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
- [x] Keyboard events (keyDown, keyUp) with focus management
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
