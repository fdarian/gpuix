---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

Motion values. Put a `motionValue` in `style` where a property `motion` can animate takes a plain value, and the element follows it every frame in Rust, with no render or N-API call per frame. React gets `useMotionValue`, `useTransform`, `useSpring`, `useVelocity` and `useMotionValueEvent`; Solid gets `createMotionValue`, `createTransform`, `createSpring`, `createVelocity` and `createMotionValueEvent`; both export `motionValue`, `animate` and `isMotionValue`. A range-form transform and a spring that follows a value are evaluated natively at the same instant as their input. A function-form transform and a velocity value are computed in JavaScript, so they trail their inputs by a frame. `animate(value, target, transition)` takes the same transition as `motion` and returns `stop`, `cancel`, `complete` and `finished`. `style` on host elements and `motion.*` is typed to accept motion values (`MotionValueStyle`). See "Motion values" in the README.
