---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

Focus follows the DOM. Only an enabled `<input>`/`<textarea>` or an element with a `tabIndex` is focusable; a key, `onFocus` or `onBlur` listener no longer makes an element focusable (breaking: give such elements a `tabIndex`). Positive `tabIndex` values are now tabbed first in ascending order, then the `0`s. A focus change dispatches `blur`, `focusout`, `focus`, `focusin` in that order from one observer instead of one gpui subscription per element, so `blur(old)` always precedes `focus(new)`. Solid has `onFocus`/`onBlur` (non-bubbling) and `onFocusIn`/`onFocusOut` (bubbling); React's `onFocus`/`onBlur` bubble like React DOM's. Focus events carry `focusVisible`. `focusVisible` styling follows the browser `:focus-visible` heuristic (text fields always, keyboard focus, script focus inherited) instead of the live input device, and `keyboardFocusDim` uses the same state. New `focusWithin` state style. Element refs gain `focus({ preventScroll })`, `blur()` and `matches(':focus' | ':focus-visible' | ':focus-within')`.
