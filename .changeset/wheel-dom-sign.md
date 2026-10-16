---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

Breaking: the simulated wheel takes the DOM's sign. `nativeSimulateScrollWheel`, `dispatchScrollWheel`, the locator's `wheel()` and `app.mouse.wheel()` treat a positive `deltaY` as scroll down (the content moves up) and a positive `deltaX` as scroll right, like `WheelEvent.deltaY`, the value `onWheel` reports for the same tick. They used GPUI's sign, where a negative `deltaY` scrolled down, so a wheel helper call now needs both deltas negated.
