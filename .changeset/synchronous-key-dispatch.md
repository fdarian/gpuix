---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

Keyboard events follow the DOM: one `keydown` and one `keyup` per keystroke, dispatched synchronously with capture, target and bubble phases, so `preventDefault()` cancels the key before the editor acts on it.

```tsx
import { window } from '@gpuix/react' // or '@gpuix/solid'

window.addEventListener('keydown', (event) => {
  if (event.key === 'k' && event.modifiers?.cmd) {
    event.preventDefault()
    openPalette()
  }
}, { capture: true })
```

- `preventDefault()` on `keydown` cancels the editor action (caret moves, delete, Enter) and the text the key would insert, as well as the Tab and Escape defaults. This holds on macOS and the test renderer. On Windows and Linux events still reach JavaScript through a queue, so only the Tab and Escape defaults can be cancelled. On macOS, text composed through an active input method cannot be cancelled
- `window` and `document` are exported by `@gpuix/react` and `@gpuix/solid` (and `@gpuix/native/host`) with `addEventListener(type, listener, { capture, once })` and `removeEventListener`. They are not installed as globals. The order is window capture, document capture, the element path, document bubble, window bubble. The `render({ onKeyDown, onKeyUp })` options are window bubble listeners
- A handler can call back into the renderer (`focusElement`, `scrollTo`, the clipboard, `promptForPaths`, `getWindowSize`, `getA11yTree`, `setWindow*`) while the key is dispatched
- A mouse press on something not focusable blurs the focused element. `preventDefault()` in `onMouseDown` keeps focus

Breaking:

- An element's `onKeyDown` and `onKeyUp` no longer fire once per ancestor on the focus path. They fire once, as the event bubbles, and `event.target` is the focused element (`undefined` when nothing is focused)
- The separate window key event is gone: `windowKeyDown`, `windowKeyUp` and `keystrokeId` are removed from the event payload. Listen with `window.addEventListener('keydown', ...)` or the `render({ onKeyDown })` option
- Native no longer runs a GPUI action first and only then calls `onKeyDown`. A key bound to an action now reaches `onKeyDown` before the action runs
