import { useEffect, useState } from "react"
import {
  createTextSearchController,
  findRanges,
} from "@ghia/native/host"
import type {
  FindRangesOptions,
  TextSearchOptions,
} from "@ghia/native/host"
import type { Props } from "../types/host.js"

export type { FindRangesOptions, TextSearchOptions }
export { findRanges }

export interface TextSearch {
  props: Pick<Props, "highlight" | "onHighlight">
  total: number
  active: number
  next(): void
  previous(): void
  goTo(index: number): void
}

/** React adapter for the framework-neutral text-search controller. */
export function useTextSearch(options: TextSearchOptions): TextSearch {
  const [controller] = useState(createTextSearchController)
  const [, setRevision] = useState(0)
  useEffect(
    () => controller.subscribe(() => setRevision((value) => value + 1)),
    [controller]
  )
  const snapshot = controller.getSnapshot(options)
  return {
    ...snapshot,
    next: () => controller.next(),
    previous: () => controller.previous(),
    goTo: (index) => controller.goTo(index),
  }
}
