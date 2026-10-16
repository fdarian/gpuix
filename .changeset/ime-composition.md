---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

Input method composition on macOS follows Chrome. A key the input method takes first reaches `onKeyDown` as a `"Process"` key before the input method gets it (`preventDefault()` cannot stop the input method), and `isComposing` on `onKeyDown` and `onKeyUp` is true while text is marked. `<input>` and `<textarea>` fire `onCompositionStart`, `onCompositionUpdate` and `onCompositionEnd` (with `…Capture` twins, `data`, bubbling, not cancelable) before the `onChange` of the edit. `TestRenderer.simulateComposition(id, steps)` runs a composition in tests. Windows and Linux still report `isComposing` false and fire no composition events.
