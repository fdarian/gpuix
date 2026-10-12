---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

`motion` transitions accept `type: "spring"`, described by `stiffness`,
`damping` and `mass` or by `duration` and `bounce`, with optional `restDelta`
and `restSpeed`. Springs are evaluated in closed form natively, so they do not
depend on the frame rate. When a target changes mid-flight the new segment
starts from the current position and speed, so interrupted springs (and a tween
interrupted by a spring) bend instead of restarting from rest. Output is
clamped for `opacity` and sizes because springs overshoot. Tween transitions are
unchanged. `@gpuix/react` also exports `MotionTweenTransition` and
`MotionSpringTransition`.
