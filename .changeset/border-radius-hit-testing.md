---
'@gpuix/native': minor
---

A box's own `borderRadius` cuts its hit area, as in a browser. A pointer over a rounded-away corner no longer hits the box (for `onClick`, `onPointerEnter`, hover, the wheel) and reaches what is underneath, even with `overflow` visible. Children are still hit through their own boxes, so one that covers the corner of a rounded parent is hit there.
