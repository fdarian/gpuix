import type { DomEvent, RootEvent } from "./events/dispatch.js"
import type {
  Appearance,
  AppearanceHost,
  EdgeInsets,
  HighlightSpec,
  HostProps,
  NativeRenderer,
  NativeWindowInsets,
  SelectionHost,
  WindowInsetsHost,
  WindowSizeHost,
} from "./host.js"
import { getElementAnimations, type Animation } from "./web-animations.js"

/**
 * `setPointerCapture`, `releasePointerCapture` and `hasPointerCapture` for one
 * host element, as the DOM's `Element` has them. The mouse is `pointerId` 1.
 *
 * Capture applies from the next native pointer input, because the call crosses
 * from JS to the UI thread without waiting for it: calling `setPointerCapture`
 * from a `pointerdown` handler works, the element receives the events after that
 * one, and `gotpointercapture` fires before the first of them. `hasPointerCapture`
 * answers for the request, so it is true right after `setPointerCapture`.
 * Calling it with no button down does nothing. All three throw for a `pointerId`
 * other than 1. For an element that is not in the tree, `setPointerCapture`
 * throws, `releasePointerCapture` does nothing and `hasPointerCapture` is false.
 */
export interface PointerCaptureMethods {
  setPointerCapture(pointerId: number): void
  releasePointerCapture(pointerId: number): void
  hasPointerCapture(pointerId: number): boolean
}

/**
 * What a host ref has beyond props: the pointer capture methods, `click()` and
 * `requestPointerLock()`, like `HTMLElement`'s.
 *
 * `click()` delivers a `click` that no pointer made: `detail` 0, `pointerId`
 * -1, `pointerType` `""`, no position, and it bubbles along the element's path.
 * Throws for an element that is not in the tree.
 *
 * `requestPointerLock()` hides the cursor and freezes it, and from then on
 * every pointer and mouse event goes to this element with a frozen
 * `clientX`/`clientY` and the real motion in `movementX`/`movementY`. It
 * returns at once and the outcome arrives with the next frame, as
 * `pointerlockchange` or `pointerlockerror` at `document`. It fails
 * (`pointerlockerror`) for an element that is not in the tree and when the
 * platform refuses: the window must be active, and only macOS can lock the
 * pointer. Call `renderer.exitPointerLock()` to release it.
 */
export interface ElementMethods extends PointerCaptureMethods {
  click(): void
  requestPointerLock(): void
  /**
   * The CSS transitions, then the CSS animations, of the element that are
   * running, paused or in effect through a fill, in composite order. The same
   * `Animation` object stands for a run every time. See `Animation` for what
   * it has and what is not built. Animations of the motion engine
   * (`motion`, `animate`) are not listed, and there is no `subtree` option.
   * Pending changes are sent first. An element that is not in the tree has none.
   */
  getAnimations(): Animation[]
}

export function createElementMethods(
  renderer: NativeRenderer,
  elementId: number
): ElementMethods {
  const supported = <
    Name extends
      | "setPointerCapture"
      | "releasePointerCapture"
      | "hasPointerCapture"
      | "clickElement"
      | "requestPointerLock"
  >(
    name: Name
  ): NonNullable<NativeRenderer[Name]> => {
    const method = renderer[name]
    if (!method) throw new Error(`This renderer does not support ${name}`)
    return method.bind(renderer) as NonNullable<NativeRenderer[Name]>
  }
  return {
    setPointerCapture: (pointerId) => supported("setPointerCapture")(elementId, pointerId),
    releasePointerCapture: (pointerId) => supported("releasePointerCapture")(elementId, pointerId),
    hasPointerCapture: (pointerId) => supported("hasPointerCapture")(elementId, pointerId),
    click: () => supported("clickElement")(elementId),
    requestPointerLock: () => supported("requestPointerLock")(elementId),
    getAnimations: () => getElementAnimations(renderer, elementId),
  }
}

export interface WindowSize {
  width: number
  height: number
}

export interface ObserverOptions {
  /** Poll interval in milliseconds. Defaults to 100. Set false for one read. */
  intervalMs?: number | false
}

export interface WindowInsets extends NativeWindowInsets {
  keyboardTop: number
  keyboardVisible: boolean
  visibleHeight: number
}

const DEFAULT_WINDOW_SIZE: WindowSize = { width: 800, height: 600 }
const ZERO_EDGES: EdgeInsets = { top: 0, right: 0, bottom: 0, left: 0 }

export function readWindowSize(
  renderer: WindowSizeHost | NativeRenderer | null
): WindowSize {
  try {
    const size = renderer?.getWindowSize?.()
    if (size && size.width > 0 && size.height > 0) return { ...size }
  } catch {
    // The platform window can still be opening.
  }
  return DEFAULT_WINDOW_SIZE
}

export function readWindowInsets(
  renderer: (WindowSizeHost & WindowInsetsHost) | NativeRenderer | null
): WindowInsets {
  let size = DEFAULT_WINDOW_SIZE
  let insets: NativeWindowInsets = {
    safeArea: ZERO_EDGES,
    ime: ZERO_EDGES,
    effective: ZERO_EDGES,
  }
  try {
    size = renderer?.getWindowSize?.() ?? size
    insets = renderer?.getWindowInsets?.() ?? insets
  } catch {
    // The platform window can still be opening.
  }
  return {
    ...insets,
    keyboardTop: size.height - insets.ime.bottom,
    keyboardVisible: insets.ime.bottom > 0,
    visibleHeight: size.height - insets.effective.top - insets.effective.bottom,
  }
}

function sameWindowSize(left: WindowSize, right: WindowSize): boolean {
  return left.width === right.width && left.height === right.height
}

function sameEdges(left: EdgeInsets, right: EdgeInsets): boolean {
  return left.top === right.top && left.right === right.right &&
    left.bottom === right.bottom && left.left === right.left
}

function sameWindowInsets(left: WindowInsets, right: WindowInsets): boolean {
  return left.keyboardTop === right.keyboardTop &&
    left.keyboardVisible === right.keyboardVisible &&
    left.visibleHeight === right.visibleHeight &&
    sameEdges(left.safeArea, right.safeArea) &&
    sameEdges(left.ime, right.ime) &&
    sameEdges(left.effective, right.effective)
}

function observe<Value>(
  read: () => Value,
  same: (left: Value, right: Value) => boolean,
  callback: (value: Value) => void,
  options: ObserverOptions
): () => void {
  let current = read()
  callback(current)
  if (options.intervalMs === false) return () => {}
  const timer = setInterval(() => {
    const next = read()
    if (same(current, next)) return
    current = next
    callback(next)
  }, Math.max(16, options.intervalMs ?? 100))
  return () => clearInterval(timer)
}

export function observeWindowSize(
  renderer: WindowSizeHost | NativeRenderer | null,
  callback: (size: WindowSize) => void,
  options: ObserverOptions = {}
): () => void {
  return observe(() => readWindowSize(renderer), sameWindowSize, callback, options)
}

export function observeWindowInsets(
  renderer: (WindowSizeHost & WindowInsetsHost) | NativeRenderer | null,
  callback: (insets: WindowInsets) => void,
  options: ObserverOptions = {}
): () => void {
  return observe(() => readWindowInsets(renderer), sameWindowInsets, callback, options)
}

export function observeSelectedText(
  renderer: SelectionHost,
  subscribe: (listener: (event: RootEvent) => void) => () => void,
  callback: (text: string | null) => void
): () => void {
  callback(renderer.getSelectedText() ?? null)
  return subscribe((event) => callback(event.value ?? null))
}

export function toAppearance(value: string | undefined): Appearance {
  if (value === "light" || value === "dark") return value
  throw new Error(`Expected an appearance of "light" or "dark", got ${String(value)}`)
}

export function observeAppearance(
  renderer: AppearanceHost,
  subscribe: (listener: (event: RootEvent) => void) => () => void,
  callback: (appearance: Appearance) => void
): () => void {
  callback(toAppearance(renderer.getAppearance()))
  return subscribe((event) => callback(toAppearance(event.value)))
}

export interface FindRangesOptions {
  text: string
  query: string
  caseSensitive?: boolean
  wholeWord?: boolean
}

const WORD_CHAR = /[\p{Alphabetic}\p{N}_]/u

function wordCharBefore(text: string, end: number): boolean {
  if (end <= 0) return false
  const low = text.charCodeAt(end - 1)
  const start = low >= 0xdc00 && low <= 0xdfff && end >= 2 ? end - 2 : end - 1
  return WORD_CHAR.test(text.slice(start, end))
}

function wordCharAt(text: string, start: number): boolean {
  const codePoint = text.codePointAt(start)
  return codePoint === undefined ? false : WORD_CHAR.test(String.fromCodePoint(codePoint))
}

function fold(text: string): { folded: string; map: number[] } {
  let folded = ""
  const map: number[] = []
  for (let index = 0; index < text.length;) {
    const char = String.fromCodePoint(text.codePointAt(index) ?? 0)
    const lower = char.toLowerCase()
    for (let unit = 0; unit < lower.length; unit += 1) map.push(index)
    folded += lower
    index += char.length
  }
  map.push(text.length)
  return { folded, map }
}

export function findRanges(options: FindRangesOptions): Array<[number, number]> {
  const { text, query, caseSensitive = false, wholeWord = false } = options
  if (query.length === 0) return []
  const { folded, map } = caseSensitive ? { folded: text, map: null } : fold(text)
  const needle = caseSensitive ? query : query.toLowerCase()
  const ranges: Array<[number, number]> = []
  let from = 0
  for (;;) {
    const at = folded.indexOf(needle, from)
    if (at < 0) break
    from = at + needle.length
    const start = map ? map[at]! : at
    const end = map ? map[from]! : from
    if (start >= end) continue
    if (wholeWord && (wordCharBefore(text, start) || wordCharAt(text, end))) continue
    ranges.push([start, end])
  }
  return ranges
}

export interface TextSearchOptions {
  query: string
  caseSensitive?: boolean
  wholeWord?: boolean
  color?: string
  activeColor?: string
  radius?: number
  matches?: { total: number; indexOffset: number }
}

export interface TextSearchSnapshot {
  props: Pick<HostProps, "highlight" | "onHighlight">
  total: number
  active: number
}

export interface TextSearchController {
  getSnapshot(options: TextSearchOptions): TextSearchSnapshot
  next(): void
  previous(): void
  goTo(index: number): void
  subscribe(callback: () => void): () => void
}

export function createTextSearchController(): TextSearchController {
  let reported = 0
  let requested = 0
  let latest: TextSearchOptions = { query: "" }
  const listeners = new Set<() => void>()
  const emit = () => listeners.forEach((listener) => listener())
  const total = () => latest.query.length === 0 ? 0 : (latest.matches?.total ?? reported)
  const active = () => total() === 0 ? 0 : Math.min(requested, total() - 1)
  const onHighlight = (event: DomEvent) => {
    const next = event.matchCount ?? 0
    if (reported === next) return
    reported = next
    emit()
  }

  return {
    getSnapshot(options) {
      latest = options
      const currentTotal = total()
      const currentActive = active()
      const highlight: HighlightSpec | null = options.query.length === 0 ? null : {
        query: options.query,
        caseSensitive: options.caseSensitive,
        wholeWord: options.wholeWord,
        color: options.color,
        activeColor: options.activeColor,
        radius: options.radius,
        activeIndex: currentActive,
        matchIndexOffset: options.matches?.indexOffset,
      }
      return {
        props: { highlight, onHighlight },
        total: currentTotal,
        active: currentActive,
      }
    },
    next() {
      const count = total()
      if (count === 0) return
      requested = (Math.min(requested, count - 1) + 1) % count
      emit()
    },
    previous() {
      const count = total()
      if (count === 0) return
      requested = (Math.min(requested, count - 1) + count - 1) % count
      emit()
    },
    goTo(index) {
      if (index < 0 || index >= total()) return
      requested = index
      emit()
    },
    subscribe(callback) {
      listeners.add(callback)
      return () => listeners.delete(callback)
    },
  }
}
