---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

CSS transitions. `transition`, `transitionProperty`, `transitionDuration`, `transitionTimingFunction` and `transitionDelay` take CSS syntax with kebab-case property names, and a change in the value of any property `motion` can animate now animates natively, whether it came from a style prop or from a `hover`, `active` or `focusVisible` state style applying or ending. Behaviour follows CSS Transitions Level 1: the first style never transitions, an interrupt starts from the visible value, reversing shortens the way back, a negative delay starts part-way, and removing a property from the list cancels it. Timing functions are `linear`, `ease`, `ease-in`, `ease-out`, `ease-in-out`, `cubic-bezier()`, `steps()`, `step-start`, `step-end` and `linear()`; `motion` accepts the same strings as `ease`, and a `cubic-bezier` may now overshoot. `onTransitionRun`, `onTransitionStart`, `onTransitionEnd` and `onTransitionCancel` carry `propertyName`, `elapsedTime` and `pseudoElement`. An element that declares a transition has its state styles applied by gpuix rather than GPUI's per-element refinements, so they show one frame after the pointer moves; elements without one are unchanged. An invalid value is an error. `transition-behavior: allow-discrete` is not supported.
