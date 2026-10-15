---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

`MotionConfig` for React and Solid sets a default `transition` for the `motion.*` components below it and a `reducedMotion` setting (`"never"` by default, `"always"`, or `"user"` to follow `prefers-reduced-motion`). Reduced, `width`, `height`, min/max sizes, insets, margins, paddings, gaps and `flexBasis` jump to their target instead of animating, `onMotionComplete` still fires, and opacity, colours, shadows and radii keep animating. `animate()` on a motion value jumps when the value is bound to one of those properties on a `motion.*` element under a reduced config. CSS transitions are not affected. `useReducedMotionConfig()` (React) and `createReducedMotionConfig()` (Solid) report the resolved setting, and the motion descriptor takes `reducedMotion: true` natively. See "MotionConfig and reduced motion" in the README.
