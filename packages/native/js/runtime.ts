import { GpuixRenderer } from "@gpuix/native"
import type { EventPayload, WindowOptions } from "@gpuix/native"
import {
  App,
  browserRendererAsTest,
  InProcessBackend,
  liveRendererAsTest,
  serveAutomationStdio,
  type LiveAutomationRenderer,
} from "./automation/client.js"
import { bindClipboard } from "./clipboard.js"
import { bindEyeDropper } from "./eyedropper.js"
import { readFontFiles, type WindowFontsOption } from "./fonts.js"
import { createRendererState, reportSyncErrors } from "./renderer-state.js"

export type { WindowFontsOption }

const DEFAULT_FRAME_MS = 8
const FRAME_WAKE_FALLBACK_MS = 100

export interface FrameLoop {
  stop(): void
}

export interface CreateNativeRendererOptions {
  onEvent?: (event: EventPayload) => void
  onError?: (error: unknown) => void
}

export function createNativeRenderer(
  options: CreateNativeRendererOptions = {}
): GpuixRenderer {
  let renderer!: GpuixRenderer
  renderer = new GpuixRenderer((error, event) => {
    if (error) {
      options.onError?.(error)
      return
    }
    try {
      if (createRendererState(renderer).dispatch(event)) options.onEvent?.(event)
    } catch (dispatchError) {
      options.onError?.(dispatchError)
    }
  })
  bindClipboard(renderer)
  bindEyeDropper(renderer)
  // Keys reach JS here, inside the native call, so preventDefault() can still
  // stop the editor. A throw becomes the error of the call on the stack.
  renderer.setSyncEventHandler(
    reportSyncErrors(
      (event) => createRendererState(renderer).dispatchSync(event),
      options.onError
    )
  )
  const automated =
    typeof process !== "undefined" && process.stdin && !process.stdin.isTTY
  const init = renderer.init.bind(renderer)
  renderer.init = (windowOptions?: WindowOptions & WindowFontsOption | null) => {
    const fonts = readFontFiles(windowOptions?.fonts)
    init(windowOptions)
    for (const font of fonts) renderer.registerFont(font)
    if (automated) enableAutomation(renderer)
  }
  return renderer
}

export interface FrameLoopOptions {
  frameMs?: number
  onTerminated?: () => void
  onError?: (error: unknown) => void
}

export function startFrameLoop(
  renderer: Pick<GpuixRenderer, "requiresTick" | "tick"> &
    Partial<Pick<GpuixRenderer, "setFrameWakeCallback">>,
  options: FrameLoopOptions = {}
): FrameLoop {
  if (!renderer.requiresTick()) return { stop() {} }
  let timer: ReturnType<typeof setTimeout> | undefined
  let stopped = false
  const stop = () => {
    if (stopped) return
    stopped = true
    if (frameWake) renderer.setFrameWakeCallback?.(null)
    if (timer !== undefined) clearTimeout(timer)
    timer = undefined
  }
  const loop = () => {
    if (stopped) return
    if (timer !== undefined) clearTimeout(timer)
    timer = undefined
    const started = performance.now()
    let running = true
    try {
      running = renderer.tick()
    } catch (error) {
      options.onError?.(error)
    }
    if (running === false) {
      stop()
      options.onTerminated?.()
      return
    }
    if (stopped) return
    const delay = frameWake
      ? FRAME_WAKE_FALLBACK_MS
      : Math.max(0, (options.frameMs ?? DEFAULT_FRAME_MS) - (performance.now() - started))
    timer = setTimeout(loop, delay)
  }
  const frameWake = options.frameMs === undefined &&
    renderer.setFrameWakeCallback?.(loop) === true
  loop()
  return { stop }
}

export function enableAutomation(renderer: LiveAutomationRenderer): void {
  serveAutomationStdio(new InProcessBackend(liveRendererAsTest(renderer)))
}

export function installBrowserAutomation(
  renderer: LiveAutomationRenderer,
  existing?: App
): App {
  if (existing) return existing
  return new App(
    new InProcessBackend(browserRendererAsTest(renderer))
  )
}
