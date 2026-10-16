---
'@gpuix/native': patch
'@gpuix/react': patch
'@gpuix/solid': patch
---

On macOS, `event.code` is the physical key and `event.key` is the character the keyboard layout types there, as in Chrome. The key in the C position reports `code` `"KeyC"` on a Cyrillic layout, the key that types `a` on AZERTY reports `"KeyQ"`, Option+s reports `key` `"ß"`, and the numpad Enter reports `"NumpadEnter"`. The test renderer's key presses accept more codes (`"ShiftLeft"`, `"NumpadEnter"`, `"Numpad1"`) and a press that names a code reports that exact `code`.
