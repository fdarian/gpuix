import { createSignal, onCleanup, type Accessor } from "solid-js"
import {
  createTextSearchController,
  observeWindowInsets,
  observeWindowSize,
  readWindowInsets,
  readWindowSize,
  toAppearance,
} from "@gpuix/native/host"
import type {
  Appearance,
  ObserverOptions,
  TextSearchOptions,
  WindowInsets,
  WindowSize,
} from "@gpuix/native/host"
import { useGpuix, useGpuixRequired } from "./root.js"

export function createWindowSize(options: ObserverOptions = {}): Accessor<WindowSize> {
  const renderer = useGpuixRequired()
  const [value, setValue] = createSignal(readWindowSize(renderer))
  onCleanup(observeWindowSize(renderer, setValue, options))
  return value
}

export function createWindowInsets(
  options: ObserverOptions = {}
): Accessor<WindowInsets> {
  const renderer = useGpuixRequired()
  const [value, setValue] = createSignal(readWindowInsets(renderer))
  onCleanup(observeWindowInsets(renderer, setValue, options))
  return value
}

export function createSelectedText(): Accessor<string | null> {
  const context = useGpuix()
  if (!context) throw new Error("createSelectedText must be used inside a GPUIX root")
  const [value, setValue] = createSignal(context.renderer.getSelectedText?.() ?? null)
  onCleanup(context.subscribeSelection(setValue))
  return value
}

export function createAppearance(): Accessor<Appearance> {
  const context = useGpuix()
  if (!context) throw new Error("createAppearance must be used inside a GPUIX root")
  const getAppearance = context.renderer.getAppearance
  if (!getAppearance) throw new Error("This renderer does not report the window appearance")
  const [value, setValue] = createSignal(toAppearance(getAppearance.call(context.renderer)))
  onCleanup(context.subscribeAppearance(setValue))
  return value
}

export function createTextSearch(options: Accessor<TextSearchOptions>) {
  const controller = createTextSearchController()
  const [revision, setRevision] = createSignal(0)
  onCleanup(controller.subscribe(() => setRevision((value) => value + 1)))
  return {
    get props() {
      revision()
      return controller.getSnapshot(options()).props
    },
    get total() {
      revision()
      return controller.getSnapshot(options()).total
    },
    get active() {
      revision()
      return controller.getSnapshot(options()).active
    },
    next: () => controller.next(),
    previous: () => controller.previous(),
    goTo: (index: number) => controller.goTo(index),
  }
}
