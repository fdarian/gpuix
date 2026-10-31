# GPUIX on hermes-node

Proof that a GPUIX React app can run on [hermes-node](https://github.com/tmikov/hermes-node) instead of Node or Bun.

The window is real GPUI. React reconciles in Hermes. The native addon is the same `@ghia/native` `.node` file.

```
React CJS bundle  ►  hermes-node  ►  process.dlopen  ►  @ghia/native  ►  GPUI
```

## Run

Needs a **locally rebuilt** `hermes-node`. The v0.0.2 macOS release cannot load any `.node` addon. See [NAPI blocking issues](#napi-blocking-issues).

```bash
bun hermes/build.ts
GPUIX_BACKGROUND=1 hermes-node --no-compile-cache hermes/app.cjs
```

`GPUIX_SCREENSHOT=/tmp/out.png` paints one frame, writes a PNG, and exits.

## Single executable

```bash
hermes-node --build-bundle=hermes/dist/app.bundle hermes/app.cjs
hermes-node --build-exe=hermes/dist/gpuix-hermes \
  --kit=<hermes-node-build>/kit \
  hermes/dist/app.bundle
```

`--build-exe` links a new binary. Native addons still sit beside it. `dlopen` takes a path.

Measured on this machine, Release arm64:

| Artifact | Size |
|---|---|
| `hermes-node` runtime | 11 MB |
| `app.cjs` (React + GPUIX, production) | 453 KB |
| `app.bundle` (Hermes bytecode) | 266 KB |
| `gpuix-hermes` executable | 12 MB |
| `gpuix-native.darwin-arm64.node` sidecar | 22 MB |
| **exe + native** | **33 MB** |

The 22 MB sidecar is the GPUIX native addon. Hermes itself is the 12 MB exe.

## What works

- `process.dlopen` of a napi-rs addon
- `GpuixRenderer` construct, `init`, `applyBatch`, `tick`
- React 19 + `react-reconciler` after a CJS bundle
- Clickable counter (`useState`)
- `--build-bundle` and `--build-exe`

Hermes has no ESM. The app is bundled to CJS. The automation client's `import()` is stubbed.

## NAPI blocking issues

**1. Official macOS release exports zero `napi_*` symbols.**

[hermes-node](https://github.com/tmikov/hermes-node) documents that N-API addons work: `process.dlopen` is wired, and NAPI symbols are exported with `-rdynamic` / `-export_dynamic`. That is true on Linux. It is **not** true of `hermes-node-0.0.2-macos-universal`.

`tools/hermes-node/CMakeLists.txt` also passes `-Wl,-dead_strip` on Apple. The comment says that was never exercised on macOS. On ld64, `-export_dynamic` is not a GC root, so `-dead_strip` drops the whole NAPI C ABI.

Result with the official binary:

```
dlopen(...gpuix-native.darwin-arm64.node):
  symbol not found in flat namespace '_napi_create_function'
```

`nm -gU` on that binary: **0** `_napi_*` exports. A rebuilt binary without `-dead_strip`: **145** `_napi_*` exports, and the same `.node` loads.

A tiny C addon (`hello.node`) fails the same way on the release binary. This is not a GPUIX or napi-rs bug.

**2. Hardened Runtime vs ad-hoc `.node`.**

The release binary is signed `adhoc,runtime`. Loading an ad-hoc linker-signed `.node` fails with:

```
mapping process and mapped file (non-platform) have different Team IDs
```

Resign without the hardened runtime (`codesign --force --sign -`) or build locally. The rebuild here is ad-hoc without `runtime`.

**3. napi-rs `dyn-symbols`.**

napi-rs 3 defaults to `dyn-symbols`: it `dlsym`s NAPI from the host process at load time. If the host exports the C ABI, that works. If it does not, every call prints `Node-API symbol X has not been loaded` and returns a stub. Rebuilding hermes-node with exports is the fix. No GPUIX change needed.

**4. Hermes engine gaps that did not block this app.**

From [Hermes NAPI COMPATIBILITY.md](https://raw.githubusercontent.com/facebook/hermes/static_h/API/napi/COMPATIBILITY.md) and hermes-node docs:

- NAPI v10 is implemented, including thread-safe functions and async work
- No ESM (`import()`, `"type": "module"`)
- No `queueMicrotask` / `performance` globals (polyfilled in the bundle banner)
- No `crypto` / `tls` / `worker_threads` (unused here)
- `eval` is indirect only (unused here)

## Rebuild hermes-node on macOS

```bash
git clone --recurse-submodules https://github.com/tmikov/hermes-node.git
# Drop -Wl,-dead_strip from tools/hermes-node/CMakeLists.txt on APPLE.
cmake -S . -B cmake-build-release -G Ninja \
  -DCMAKE_BUILD_TYPE=Release -DHERMES_ENABLE_TEST_SUITE=OFF
ninja -C cmake-build-release hermes-node
# optional, for --build-exe
ninja -C cmake-build-release hermes-node-kit
```
