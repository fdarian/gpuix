---
'@gpuix/native': patch
'@gpuix/react': patch
'@gpuix/solid': patch
---

`navigator.clipboard` is declared in the shared types, so a project without the DOM lib in its `tsconfig` (for example `lib: ["ESNext"]`) type-checks it. With the DOM lib the declaration is identical to its own.
