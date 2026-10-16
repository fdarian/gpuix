---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

Breaking: the test renderer behaves like the live one for appearance and window options. `setAppearance` now applies (it only validated before): `getAppearance()` reads it back, `onAppearanceChange` fires, and `light-dark()` and `prefers-color-scheme` follow; `"system"` restores the platform's appearance. `createTestRoot` and `TestRenderer` take the live renderer's `WindowOptions`, opened through the same translation: `title` (read it with the new `getWindowTitle()`), `appearance`, `titlebarTransparent` and the rest take effect where the offscreen window can represent them. `appRegion` therefore applies in a test window only when it was made with `titlebarTransparent: true`; before, every test window counted as frameless. `fullscreen`, `appName`, `appId` and `layerShell` are ignored, the default title is `"GPUIX"` (was `"GPUIX Test"`), and `new TestGpuixRenderer()` takes a `WindowOptions` object instead of `(width, height)`. The README's Testing section lists what each option does.
