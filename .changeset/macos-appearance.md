---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

The window follows the system light or dark setting, and an app can force it.
`render()` takes `appearance: "system" | "light" | "dark"` (default
`"system"`), and the renderer has `setAppearance()` and `getAppearance()`, which
returns the effective `"light"` or `"dark"`. `onAppearanceChange` is a
window-level callback on `render()` and `createRoot()` that fires when the
effective appearance changes; `observeAppearance` (React) and
`createAppearance` (Solid) wrap it. An unknown appearance string throws.
`appearance` and `setAppearance` apply on macOS and are ignored on Windows and
Linux. The default theme of `<code>`, `<diff>` and `<markdown>` is unchanged.
