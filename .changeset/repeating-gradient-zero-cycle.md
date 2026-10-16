---
'@gpuix/native': patch
---

A repeating gradient whose cycle has no length paints the average colour of its stops when the first and last stop are in different units, as CSS does. `repeating-linear-gradient(red 10px, blue 5%)` on a 200px line is purple; before, only stops in the same unit were averaged and this one drew a hard edge. The cycle is measured in the box, so the same gradient on a 400px line repeats normally.
