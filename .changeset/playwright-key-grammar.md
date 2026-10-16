---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

Breaking: the simulated keyboard speaks the DOM. `simulateKeystrokes`, `nativeSimulateKeystrokes`, `nativeSimulateKeyDown`, `nativeSimulateKeyUp`, the locator's `press()` and the automation `keystrokes`, `keyDown` and `keyUp` calls take Playwright's `keyboard.press` grammar instead of GPUI keystroke syntax: a DOM `key` (`"Enter"`, `"a"`, `"ArrowDown"`) or `code` (`"Space"`, `"KeyA"`), modifiers joined by `+` (`"Shift+Tab"`, `"Meta+a"`, `"ControlOrMeta+Shift+P"`). A sequence is still space-separated. Replace `"enter"` with `"Enter"`, `"down"` with `"ArrowDown"`, `"shift-tab"` with `"Shift+Tab"`, `"cmd-a"` with `"Meta+a"`. Unknown names throw. Each press is now a `keydown` followed by a `keyup` (it was a `keydown` alone), so a test that also called `nativeSimulateKeyUp` after `nativeSimulateKeystrokes` sees two `keyup`s.
