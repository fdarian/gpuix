import { useLayoutEffect, useRef } from "react"
import { pushUniversalStyle } from "@gpuix/native/host"
import type { StyleDesc, UniversalStyleEntry } from "@gpuix/native/host"
import { useGpuix } from "../hooks/use-gpuix.js"

export interface UniversalStyleProps {
  style: StyleDesc
}

/**
 * Renders nothing. Puts `style` under every element's own style while mounted,
 * as a CSS `* { ... }` rule would: the lowest precedence, so an element's own
 * style and state styles win per property. Several mounted at once are spread
 * over each other, later ones winning, and unmounting one leaves the others.
 */
export function UniversalStyle(props: UniversalStyleProps): null {
  const gpuix = useGpuix()
  const renderer = gpuix.renderer
  const mutations = gpuix.mutations
  if (!renderer || !mutations) {
    throw new Error("UniversalStyle must be used inside a GPUIX root")
  }
  const entry = useRef<UniversalStyleEntry | null>(null)

  // Native only sees a batch that is flushed, and a layout effect runs after the
  // commit's own flush.
  useLayoutEffect(() => {
    const mounted = pushUniversalStyle(renderer, mutations, props.style)
    entry.current = mounted
    const unmount = () => {
      entry.current = null
      mounted.remove()
      mutations.flushMutations()
    }
    try {
      mutations.flushMutations()
    } catch (error) {
      // React runs no cleanup for an effect that threw, and native rejected the style.
      entry.current = null
      mounted.remove()
      throw error
    }
    return unmount
  }, [renderer, mutations])

  useLayoutEffect(() => {
    entry.current?.update(props.style)
    mutations.flushMutations()
  }, [props.style])

  return null
}
