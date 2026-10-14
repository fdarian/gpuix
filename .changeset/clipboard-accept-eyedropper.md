---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

Platform APIs that follow the web. `clipboard.writeText()` and `readText()` are the text subset of `navigator.clipboard`, installed there when the runtime has none (`TestRenderer` has its own in-memory clipboard). `promptForPaths({ accept })` filters the picker with the HTML `accept` syntax on macOS and Linux. `EyeDropper` (`new EyeDropper().open({ signal })` resolving `{ sRGBHex }`) is defined on macOS only. The native methods are `writeClipboardText`, `readClipboardText`, `supportsEyeDropper` and `openEyeDropper`. See "Clipboard", "File picker" and "EyeDropper" in the README.
