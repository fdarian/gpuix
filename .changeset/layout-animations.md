---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

Layout animations on `motion.div`. `layout` (`true`, `"position"`, `"size"`) glides a box to its new layout position instead of jumping, `layoutId` shares the glide between elements so a highlight can move from one tab to the next, and `LayoutGroup` scopes the ids (`${groupId}-${layoutId}`, nested groups join). The glide uses `transition.layout`, then the component's or `MotionConfig`'s `transition`, then a 0.45 s tween, follows `MotionConfig` `reducedMotion`, and is reported by `onLayoutAnimationStart` and `onLayoutAnimationComplete`. Only position animates: a size change snaps, and an element that held a `layoutId` is not hidden when another takes it. `animate` is now optional on `motion.*`, and `transition` accepts a `layout` entry.
