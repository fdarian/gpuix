---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

`AnimatePresence` takes `mode` and `custom`. `mode="wait"` renders an entering child only after every exit is done, `mode="popLayout"` takes a leaving child out of flow (`position: absolute` at the box it last painted inside its parent, read with `getElementBounds`) so its siblings reflow at once, and `"sync"` stays the default. `custom` is handed to the `exit` of every leaving child, which may now be a function of it, as `exit={(custom) => target}`; a `motion.*` also takes a `custom` prop. In Solid an exiting child stays at its place in the list while it leaves, instead of moving to the end, which recreated its node and dropped its completion handler when siblings were still mounted.
