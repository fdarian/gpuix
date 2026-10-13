import type { JSX } from "solid-js"
import { GpuixRenderer, type WindowOptions } from "@gpuix/native"
import type { NativeRenderer, RootEventHandlers } from "@gpuix/native/host"
import {
  createNativeRenderer,
  installBrowserAutomation,
  startFrameLoop,
  type FrameLoop,
  type FrameLoopOptions,
  type WindowFontsOption,
} from "@gpuix/native/runtime"
import { createRoot, type Root } from "./root.js"

const RENDER_HOST = Symbol.for("@gpuix/solid/render-host")

interface RenderSlot {
  renderer?: NativeRenderer
  root?: Root
  loop?: FrameLoop
}

export interface RenderOptions
  extends WindowOptions,
    WindowFontsOption,
    RootEventHandlers {
  renderer?: NativeRenderer
  debugFrameOverlay?: "hidden" | "minimal" | "full"
  /**
   * How the native event loop is pumped. On macOS the default is paced by the
   * display link; `frameMs` opts into polling with a timer instead.
   */
  frameLoop?: Pick<FrameLoopOptions, "frameMs">
}

export function createRenderer(
  onEvent?: (event: import("@gpuix/native").EventPayload) => void
): GpuixRenderer {
  return createNativeRenderer({
    onEvent,
    onError: (error) => console.error("[gpuix-solid] native event error", error),
  })
}

function slot(): RenderSlot {
  const global = globalThis as typeof globalThis & { [RENDER_HOST]?: RenderSlot }
  return global[RENDER_HOST] ??= {}
}

/** Mount Solid. Later calls under `bun --hot` remount on the same window. */
export function render(code: () => JSX.Element, options: RenderOptions = {}): Root {
  const {
    renderer: injected,
    debugFrameOverlay,
    frameLoop,
    onEvent,
    onKeyDown,
    onKeyUp,
    onSelectionChange,
    onAppearanceChange,
    tabNavigation,
    keyboardFocusDim,
    onUncaughtError,
    ...windowOptions
  } = options
  const host = slot()
  if (!host.renderer) {
    host.renderer = injected ?? createNativeRenderer({
      onError(error) {
        onUncaughtError?.(
          error instanceof Error ? error : new Error(String(error)),
          {}
        )
      },
    })
    if (!injected && host.renderer instanceof GpuixRenderer) {
      host.renderer.init(windowOptions)
    }
  }
  host.root?.unmount()
  host.root = createRoot(host.renderer, {
    onEvent,
    onKeyDown,
    onKeyUp,
    onSelectionChange,
    onAppearanceChange,
    tabNavigation,
    keyboardFocusDim,
    onUncaughtError,
  })
  if (!injected && host.renderer instanceof GpuixRenderer && !host.loop) {
    host.loop = startFrameLoop(host.renderer, {
      ...frameLoop,
      onTerminated: () => {
        if (typeof process !== "undefined") process.exit(0)
      },
      onError: (error) => console.error("[gpuix-solid] frame error", error),
    })
  }
  if (
    typeof window !== "undefined" &&
    host.renderer instanceof GpuixRenderer &&
    !Reflect.has(globalThis, "gpuix")
  ) {
    Reflect.set(globalThis, "gpuix", installBrowserAutomation(host.renderer))
  }
  if (debugFrameOverlay) host.renderer.setDebugFrameOverlay?.(debugFrameOverlay)
  host.root.render(code)
  return host.root
}

export function resetRender(): void {
  const global = globalThis as typeof globalThis & { [RENDER_HOST]?: RenderSlot }
  const host = global[RENDER_HOST]
  host?.loop?.stop()
  host?.root?.unmount()
  delete global[RENDER_HOST]
}
