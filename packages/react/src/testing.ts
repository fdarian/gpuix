import type { ReactNode } from "react"
import type { EventPayload } from "@ghia/native"
import {
  TestRenderer,
  hasNativeTestRenderer,
  type TestWindowOptions,
} from "@ghia/native/testing"
import { createRoot, flushSync, type Root } from "./reconciler/reconciler.js"
import {
  createRendererState,
  type RootEventHandlers,
  type SyncDispatchResult,
} from "@ghia/native/host"
import { handleGpuixEvent } from "./reconciler/event-registry.js"

export * from "@ghia/native/testing"

export interface TestRoot {
  root: Root
  renderer: TestRenderer
  render: (node: ReactNode) => void
  unmount: () => void
}

export function createTestRoot(
  options: TestWindowOptions & Pick<RootEventHandlers, "onUncaughtError"> = {}
): TestRoot {
  let renderer!: TestRenderer
  renderer = new TestRenderer({
    ...options,
    dispatchEvent: (event: EventPayload) => {
      flushSync(() => handleGpuixEvent(event, renderer))
    },
    dispatchSyncEvent: (event: EventPayload) => {
      let result: SyncDispatchResult | undefined
      flushSync(() => {
        result = createRendererState(renderer).dispatchSync(event)
      })
      if (!result) throw new Error("flushSync did not run the dispatch")
      return result
    },
  })
  const root = createRoot(renderer, {
    onKeyDown: options.onKeyDown,
    onKeyUp: options.onKeyUp,
    onSelectionChange: options.onSelectionChange,
    onAppearanceChange: options.onAppearanceChange,
    tabNavigation: options.tabNavigation,
    keyboardFocusDim: options.keyboardFocusDim,
    onUncaughtError: options.onUncaughtError,
  })
  const render = (node: ReactNode): void => {
    flushSync(() => root.render(node))
    renderer.flush()
  }
  const unmount = (): void => {
    root.unmount()
    renderer.dispose()
  }
  return { root, renderer, render, unmount }
}

export { TestRenderer, hasNativeTestRenderer }
