import type { JSX } from "solid-js"
import {
  TestRenderer,
  hasNativeTestRenderer,
  type TestWindowOptions,
} from "@gpuix/native/testing"
import { createRoot, type Root } from "./root.js"

export * from "@gpuix/native/testing"

export interface TestRoot {
  root: Root
  renderer: TestRenderer
  render(code: () => JSX.Element): void
  flushSync<Value>(fn: () => Value): Value
  unmount(): void
}

export function createTestRoot(options: TestWindowOptions = {}): TestRoot {
  let root!: Root
  const renderer = new TestRenderer({
    ...options,
    dispatchEvent: (event) => root.dispatch(event),
    dispatchSyncEvent: (event) => root.dispatchSync(event),
  })
  root = createRoot(renderer, {
    onKeyDown: options.onKeyDown,
    onKeyUp: options.onKeyUp,
    onSelectionChange: options.onSelectionChange,
    onAppearanceChange: options.onAppearanceChange,
    tabNavigation: options.tabNavigation,
    keyboardFocusDim: options.keyboardFocusDim,
  })
  return {
    root,
    renderer,
    render(code) {
      root.render(code)
      renderer.flush()
    },
    flushSync(fn) {
      const value = root.flushSync(fn)
      renderer.flush()
      return value
    },
    unmount() {
      root.unmount()
      renderer.flush()
      renderer.dispose()
    },
  }
}

export { TestRenderer, hasNativeTestRenderer }
