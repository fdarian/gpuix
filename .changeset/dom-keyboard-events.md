---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

Breaking: keyboard events are DOM-shaped. `event.key` now holds DOM `KeyboardEvent.key` values instead of GPUI key names: `"Enter"`, `"Escape"`, `"Tab"`, `"Backspace"`, `"Delete"`, `"ArrowUp"`/`"ArrowDown"`/`"ArrowLeft"`/`"ArrowRight"` (was `"up"`...), `"Home"`, `"End"`, `"PageUp"`, `"PageDown"`, `" "` for space (was `"space"`), `"F1"`...`"F35"`, modifier keys as `"Shift"`/`"Control"`/`"Alt"`/`"Meta"`, and printable keys as the character produced, with shift case (`shift-a` is `"A"`). Unknown keys are `"Unidentified"`. Replace comparisons such as `event.key === "enter"` with `event.key === "Enter"`. Key events also gain `code` (the physical key, `"KeyA"`, `"Digit1"`, `"ArrowLeft"`, derived assuming a US QWERTY layout for printable keys; `""` when unknown), `repeat` (replaces `isHeld`, which stays), `isComposing` (always `false`: a key feeding an IME composition never reaches GPUI), and `shiftKey` / `ctrlKey` / `altKey` / `metaKey` next to `modifiers`. The input syntax of `simulateKeystrokes`, `nativeSimulateKeyDown` and `press()` is unchanged (GPUI keystroke syntax: `enter`, `cmd-shift-p`).
