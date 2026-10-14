import type { NativeRenderer } from "./host.js"

export interface FocusOptions {
  /** Skip scrolling the element into view. Defaults to false, like the DOM. */
  preventScroll?: boolean
}

/** The focus methods of a DOM element, shared by every adapter's element refs. */
export interface FocusableElement {
  focus(options?: FocusOptions): void
  blur(): void
  /** Only `:focus`, `:focus-visible` and `:focus-within`; any other selector throws. */
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
  if (!options?.preventScroll) renderer.scrollIntoView?.(id)
}

export function blurHostElement(renderer: NativeRenderer, id: number): void {
  required(renderer.blurElement, "blur()").call(renderer, id)
}

export function hostElementMatches(renderer: NativeRenderer, id: number, selector: string): boolean {
  return required(renderer.matchesFocus, "matches()").call(renderer, id, selector)
}
