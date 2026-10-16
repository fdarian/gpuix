import { useContext, createContext } from "react"
import type { MutationRenderer } from "@gpuix/native/host"
import type { NativeRenderer } from "../types/host.js"

export interface GpuixContextValue {
  renderer: NativeRenderer | null
  /** The root's mutation queue, for what must be ordered with its element mutations. */
  mutations: MutationRenderer | null
}

export const GpuixContext = createContext<GpuixContextValue>({
  renderer: null,
  mutations: null,
})

/**
 * Access the GPUIX renderer from within a component
 */
export function useGpuix(): GpuixContextValue {
  return useContext(GpuixContext)
}

/**
 * Access the GPUIX renderer, throwing if not available
 */
export function useGpuixRequired(): NativeRenderer {
  const { renderer } = useGpuix()
  if (!renderer) {
    throw new Error("useGpuixRequired must be used within a GpuixProvider")
  }
  return renderer
}
