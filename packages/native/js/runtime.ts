import * as native from "@gpuix/native"
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
import { flushPendingMutations } from "./bounds.js"
import {
  registerKeyframesHost,
  registerKeyframesValidator,
  unregisterKeyframesHost,
} from "./keyframes.js"
import { readFontFiles, type WindowFontsOption } from "./fonts.js"
import { createRendererState, reportSyncErrors } from "./renderer-state.js"

export type { WindowFontsOption }

const DEFAULT_FRAME_MS = 8
const FRAME_WAKE_FALLBACK_MS = 100

// The browser entry has no `validateKeyframes`, so it is read off the namespace.
if (typeof native.validateKeyframes === "function") {
  registerKeyframesValidator(native.validateKeyframes)
}

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
  // A read must see mutations a framework has queued but not sent yet (Solid
  // sends in a microtask), and native draws pending changes before answering.
  const getElementBounds = renderer.getElementBounds.bind(renderer)
  renderer.getElementBounds = (id) => {
    flushPendingMutations(renderer)
    return getElementBounds(id)
  }
  const getElementsAt = renderer.getElementsAt.bind(renderer)
  renderer.getElementsAt = (x, y) => {
    flushPendingMutations(renderer)
    return getElementsAt(x, y)
  }
  const automated =
    typeof process !== "undefined" && process.stdin && !process.stdin.isTTY
  const init = renderer.init.bind(renderer)
  const tick = renderer.tick.bind(renderer)
  renderer.init = (windowOptions?: WindowOptions & WindowFontsOption | null) => {
    const fonts = readFontFiles(windowOptions?.fonts)
    init(windowOptions)
    for (const font of fonts) renderer.registerFont(font)
    // The web renderer has no motion engine, so nothing to define keyframes on.
    if (typeof renderer.defineKeyframes === "function") registerKeyframesHost(renderer)
    if (automated) enableAutomation(renderer)
  }
  renderer.tick = () => {
    const running = tick()
    if (!running) unregisterKeyframesHost(renderer)
    return running
  }
  return renderer
}

export interface FrameLoopOptions {
  frameMs?: number
  onTerminated?: () => void
  onError?: (error: unknown) => void
}

type FrameLoopRenderer = Pick<GpuixRenderer, "requiresTick" | "tick"> &
  Partial<Pick<GpuixRenderer, "setFrameWakeCallback" | "setSyncEventHandler">>

/** The last window closed: the sync handler captures the renderer, so it has to be
 *  released for the renderer to be collectable. */
function windowClosed(renderer: FrameLoopRenderer, options: FrameLoopOptions): void {
  renderer.setSyncEventHandler?.(null)
  options.onTerminated?.()
}

export function startFrameLoop(
  renderer: FrameLoopRenderer,
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
      windowClosed(renderer, options)
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
