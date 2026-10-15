---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

CSS animations. `animation` and its longhands (`animationName`, `animationDuration`, `animationTimingFunction`, `animationDelay`, `animationIterationCount`, `animationDirection`, `animationFillMode`, `animationPlayState`) take CSS syntax, and `defineKeyframes(name, { from, '50%', to })`, exported from `@gpuix/react` and `@gpuix/solid`, is the `@keyframes` analogue, shared by every window in the process. Animations run natively with CSS Animations Level 1 behaviour: iteration counts including `infinite`, directions, fill modes, play state, negative delays, a timing function per keyframe, and a restart when the name changes. They win over the element's style, state styles, CSS transitions and `motion`, and `hover`, `active` and `focusVisible` may declare one. `onAnimationStart`, `onAnimationIteration`, `onAnimationEnd` and `onAnimationCancel` carry `animationName`, `elapsedTime` and `pseudoElement`. Differences from a browser: redefining keyframes affects only animations that start afterwards, a running transition never beats an animation, only the properties `motion` animates can be keyframed, and `animation-composition`, `animation-timeline` and `animation-range` are not supported. An invalid value is an error.
