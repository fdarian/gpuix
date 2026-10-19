---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

A `motion.*` without an `initial` animates on mount, as Motion for React does: a keyframe list such as `animate={{ opacity: [0, 1, 0] }}` with `repeat: Infinity` plays from its first keyframe, and a plain value runs from the element's own value. `initial={false}` still mounts at the target, the last keyframe of a list, with nothing animating. A repeating loop shows its last frame, not its first, at the instant an iteration ends, as Motion does.
