import { createEffect, onCleanup, type JSX } from "solid-js"
import { pushUniversalStyle } from "@gpuix/native/host"
import type { StyleDesc, UniversalStyleEntry } from "@gpuix/native/host"
import { useGpuix } from "../root.js"

export interface UniversalStyleProps {
  style: StyleDesc
}

/**
 * Renders nothing. Puts `style` under every element's own style while mounted,
 * as a CSS `* { ... }` rule would: the lowest precedence, so an element's own
 * style and state styles win per property. Several mounted at once are spread
 * over each other, later ones winning, and unmounting one leaves the others.
 */
export function UniversalStyle(props: UniversalStyleProps): JSX.Element {
  const gpuix = useGpuix()
  if (!gpuix) throw new Error("UniversalStyle must be used inside a GPUIX root")
  let entry: UniversalStyleEntry | undefined
  createEffect(() => {
    const style = props.style
    if (entry) entry.update(style)
    else entry = pushUniversalStyle(gpuix.renderer, gpuix.mutations, style)
  })
  onCleanup(() => entry?.remove())
  return undefined
}
