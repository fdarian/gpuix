---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

`render()`, `renderer.init()` and `createTestRoot()` accept `fonts`, a list of font file paths. Each file is read and passed to `registerFont()` as soon as the renderer exists, so `fontFamily: "Inter"` resolves without installing the font system-wide and the same font checks apply. An unreadable path throws before any window opens. Variable fonts only render their default instance, so pass one static file per weight.
