import { useCallback, useMemo, useSyncExternalStore } from "react"
import { REDUCED_MOTION_QUERY, matchMediaFor } from "@ghia/native/host"
import type { MatchMedia } from "@ghia/native/host"
import { useGpuixRequired } from "./use-gpuix.js"

/**
 * `window.matchMedia` bound to the window this component is rendered in. The
 * same function comes back while the window is the same.
 */
export function useMatchMedia(): MatchMedia {
  return matchMediaFor(useGpuixRequired())
}

/** Whether the media query matches, re-rendering when that changes. */
export function useMediaQuery(query: string): boolean {
  const matchMedia = useMatchMedia()
  const list = useMemo(() => matchMedia(query), [matchMedia, query])
  // A new subscribe function every render would unsubscribe and resubscribe.
  const subscribe = useCallback(
    (onChange: () => void) => {
      list.addEventListener("change", onChange)
      return () => list.removeEventListener("change", onChange)
    },
    [list]
  )
  return useSyncExternalStore(subscribe, () => list.matches)
}

/**
 * True when the OS asks for reduced motion. Same contract as Framer Motion's
 * `useReducedMotion`, except it is never `null`: the answer is known on the
 * first render.
 */
export function useReducedMotion(): boolean {
  return useMediaQuery(REDUCED_MOTION_QUERY)
}
