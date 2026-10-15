import { createComponent, createContext, useContext, type JSX } from "solid-js"
import type { EventPayload } from "@gpuix/native"
import {
  createMutationQueue,
  createRendererState,
  unregisterEventHandlers,
} from "@gpuix/native/host"
import { toAppearance } from "@gpuix/native/host"
import type {
  Appearance,
  NativeRenderer,
  RendererRootBinding,
  RootEventHandlers,
} from "@gpuix/native/host"
import { HostRoot, removeHostNode } from "./host.js"
import { universalRender } from "./universal.js"

export interface GpuixContextValue {
  renderer: NativeRenderer
  subscribeSelection(callback: (text: string | null) => void): () => void
  subscribeAppearance(callback: (appearance: Appearance) => void): () => void
}

export const GpuixContext = createContext<GpuixContextValue>()

export function useGpuix(): GpuixContextValue | undefined {
  return useContext(GpuixContext)
}

export function useGpuixRequired(): NativeRenderer {
  const value = useGpuix()
  if (!value) throw new Error("useGpuixRequired must be used inside a GPUIX root")
  return value.renderer
}

export interface Root {
  render(code: () => JSX.Element): void
  flush(): void
  flushSync<Value>(fn: () => Value): Value
  dispatch(event: EventPayload): boolean
  unmount(): void
}

export function createRoot(
  renderer: NativeRenderer,
  rootHandlers: RootEventHandlers = {}
): Root {
  const state = createRendererState(renderer)
  const eventHandlers = new Map()
  const mutations = createMutationQueue(renderer, (ids) => {
    for (const id of ids) unregisterEventHandlers(eventHandlers, id)
  }, (flush) => {
    queueMicrotask(() => {
      try {
        flush()
      } catch (error) {
        rootHandlers.onUncaughtError?.(
          error instanceof Error ? error : new Error(String(error)),
          {}
        )
      }
    })
  })
  const container = new HostRoot({
    mutations,
    nativeRenderer: renderer,
    eventHandlers,
    allocateId: () => ++state.ids.nextElementId,
  })
  let binding: RendererRootBinding | undefined
  let dispose: (() => void) | undefined
  const selectionListeners = new Set<(text: string | null) => void>()
  const onSelectionChange = (event: EventPayload, currentRenderer: NativeRenderer) => {
    rootHandlers.onSelectionChange?.(event, currentRenderer)
    for (const listener of selectionListeners) listener(event.value ?? null)
  }
  const syncSelection = () => {
    if (!binding) return
    renderer.setWindowSelectionChange?.(
      Boolean(rootHandlers.onSelectionChange) || selectionListeners.size > 0,
      binding.windowSelectionEventId
    )
  }

  const appearanceListeners = new Set<(appearance: Appearance) => void>()
  const onAppearanceChange = (event: EventPayload, currentRenderer: NativeRenderer) => {
    rootHandlers.onAppearanceChange?.(event, currentRenderer)
    const appearance = toAppearance(event.value)
    for (const listener of appearanceListeners) listener(appearance)
  }
  const syncAppearance = () => {
    if (!binding) return
    renderer.setWindowAppearanceChange?.(
      Boolean(rootHandlers.onAppearanceChange) || appearanceListeners.size > 0,
      binding.windowAppearanceEventId
    )
  }

  const attach = () => {
    binding = state.attach({
      eventHandlers,
      onWindowKeyDown: rootHandlers.onKeyDown,
      onWindowKeyUp: rootHandlers.onKeyUp,
      onSelectionChange,
      onAppearanceChange,
      onPointerLockChange: rootHandlers.onPointerLockChange,
      onPointerLockError: rootHandlers.onPointerLockError,
      tabNavigation: rootHandlers.tabNavigation,
      onEvent: rootHandlers.onEvent,
    })
    renderer.setWindowKeyEvents?.(
      // Always on: the Tab and Escape defaults run on the window event.
      true,
      Boolean(rootHandlers.onKeyUp),
      binding.windowKeyEventId
    )
    // Sent on every attach, so a remount without the option restores the default.
    mutations.setKeyboardFocusDim(rootHandlers.keyboardFocusDim !== false)
    syncSelection()
    syncAppearance()
  }

  const detach = () => {
    if (!binding?.detach()) return
    renderer.setWindowKeyEvents?.(false, false, binding.windowKeyEventId)
    renderer.setWindowSelectionChange?.(false, binding.windowSelectionEventId)
    renderer.setWindowAppearanceChange?.(false, binding.windowAppearanceEventId)
    binding = undefined
  }

  attach()
  const flush = () => mutations.flushMutations()
  const clear = () => {
    dispose?.()
    dispose = undefined
    const mounted = container.children.at(0)
    if (mounted) removeHostNode(container, mounted)
    flush()
    eventHandlers.clear()
  }

  return {
    render(code) {
      if (dispose) {
        clear()
        detach()
        attach()
      }
      dispose = universalRender(
        () => createComponent(GpuixContext.Provider, {
          value: {
            renderer,
            subscribeSelection(callback) {
              selectionListeners.add(callback)
              callback(renderer.getSelectedText?.() ?? null)
              syncSelection()
              return () => {
                selectionListeners.delete(callback)
                syncSelection()
              }
            },
            subscribeAppearance(callback) {
              appearanceListeners.add(callback)
              syncAppearance()
              return () => {
                appearanceListeners.delete(callback)
                syncAppearance()
              }
            },
          },
          get children() {
            return code()
          },
        }),
        container
      )
      flush()
    },
    flush,
    flushSync(fn) {
      try {
        return fn()
      } finally {
        flush()
      }
    },
    dispatch(event) {
      try {
        return state.dispatch(event)
      } finally {
        flush()
      }
    },
    unmount() {
      clear()
      detach()
    },
  }
}
