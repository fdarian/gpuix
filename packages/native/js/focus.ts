import type { NativeRenderer } from "./host.js"

export interface FocusOptions {
  /** Skip scrolling the element into view. Defaults to false, like the DOM. */
  preventScroll?: boolean
}

/** The focus methods of a DOM element, shared by every adapter's element refs. */
export interface FocusableElement {
  focus(options?: FocusOptions): void
  blur(): void
  /** Only `:focus`, `:focus-visible`, `:focus-within`, `:hover` and `:active`; any other selector throws. */
  matches(selector: string): boolean
}

function required<Method>(method: Method | undefined, name: string): Method {
  if (!method) throw new Error(`This GPUIX renderer does not support ${name}`)
  return method
}

export function focusHostElement(
  renderer: NativeRenderer,
  id: number,
  options?: FocusOptions
): void {
  required(renderer.focusElement, "focus()").call(renderer, id)
  if (!options?.preventScroll) scrollFocusIntoView(renderer, id)
}

/** Tab and Shift+Tab: moves focus along the tab order, then scrolls the new element into view. */
export function moveFocusByTab(renderer: NativeRenderer, backwards: boolean): void {
  const move = backwards ? renderer.focusPrevious : renderer.focusNext
  if (!move) return
  move.call(renderer)
  const id = renderer.getFocusedElementId?.()
  if (id != null) scrollFocusIntoView(renderer, id)
}

/** Browsers scroll a newly focused element to its nearest edge, and only if it is not in view. */
function scrollFocusIntoView(renderer: NativeRenderer, id: number): void {
  renderer.scrollIntoView?.(id, "nearest", "nearest")
}

export function blurHostElement(renderer: NativeRenderer, id: number): void {
  required(renderer.blurElement, "blur()").call(renderer, id)
}

export function hostElementMatches(renderer: NativeRenderer, id: number, selector: string): boolean {
  return required(renderer.matchesFocus, "matches()").call(renderer, id, selector)
}
