---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

Breaking: simulated moves carry the buttons that are held. The test renderer remembers the buttons `nativeSimulateMouseDown` pressed and `nativeSimulateMouseUp` released, and every `nativeSimulateMouseMove`, `dispatchMouseMove` and `nativeSimulateMouseExit` reports them, as a real OS does, so a move after a press no longer looks like a lost release and ends a pointer capture. The `pressedButton` parameter is gone from `nativeSimulateMouseMove`, `dispatchMouseMove` and the automation `mouse.move` (a call that passed one now passes `modifiers` third). `simulateLostMouseUp(button?)` forgets a held button without dispatching anything, for a test of a release the OS never delivered.
