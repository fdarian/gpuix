---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

Keyframes for `motion.*` and motion values. A property in `animate` or `exit` (style or SVG attribute) takes a list, `opacity: [0, 1, 0.6]`, with `null` standing for the previous keyframe or the starting value. The transition gains `times`, an `ease` list with one easing per segment, `repeat` (`Infinity` loops for ever and `onMotionComplete` never fires), `repeatType` (`"loop"`, `"reverse"`, `"mirror"`) and `repeatDelay`; springs animate two keyframes and take `repeat`. `animate(value, [..], transition)` on a motion value takes the same. `MotionStyle` and `SvgMotionStyle` accept keyframe lists (`MotionKeyframes`), and `wireMotionTransition` in `@gpuix/native/host` sends `repeat: Infinity` as the string the renderer accepts. See "Keyframes" in the README.
