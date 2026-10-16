---
'@gpuix/native': patch
'@gpuix/react': patch
'@gpuix/solid': patch
---

`keydown` and `keyup` carry the legacy `keyCode` and `which` with the values Chrome on macOS reports: 65-90 for letters, 48-57 for the digit row, 13 for Enter, 37-40 for the arrows, 112+ for the function keys, 91 and 93 for the left and right Command keys, 96-105 and 106-111 for the keypad, the OEM numbers 186-192 and 219-222 for punctuation, and 229 for a key the input method took (`key` `"Process"`). Letters and punctuation follow the character the layout types, with the physical key's US-layout number as the fallback for a layout like Cyrillic. There is no `keypress` event, so no `charCode`.
