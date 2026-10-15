---
'@gpuix/native': minor
'@gpuix/react': minor
'@gpuix/solid': minor
---

`Reorder.Group` and `Reorder.Item`, like Motion's `Reorder`: a list the user reorders by dragging. A held item follows the pointer along `axis` and `onReorder` gets the swapped order once its centre passes a neighbour's, the other items glide to their slots with `layout`, and on release the item glides home. The native `layout` prop takes an internal `dragOffset` of motion values that shifts a box by the pointer and keeps it under the pointer when its slot moves. `@gpuix/solid` also keeps the element of a node that Solid moves (`<For>` reorders destroyed and recreated it, losing pointer capture and layout animations) and no longer places a node before itself at the end of the list.
