import type { NativeRenderer } from "./host.js"

/** `selectionDirection` of an `<input>` or `<textarea>`. */
export type SelectionDirection = "forward" | "backward" | "none"

/**
 * The selection API a ref to an `<input>` or `<textarea>` carries, shaped like
 * the DOM. Offsets are UTF-16 code units. The getters are synchronous.
 */
export interface TextControlSelection {
  /** Select all of the text. Does not move focus. */
  select(): void
  /**
   * Select `start` to `end`. Both clamp to the text, and an `end` at or before
   * `start` collapses the selection to `end`. `direction` defaults to `"none"`.
   */
  setSelectionRange(start: number, end: number, direction?: SelectionDirection): void
  readonly selectionStart: number
  readonly selectionEnd: number
  readonly selectionDirection: SelectionDirection
}

/** What an adapter hands the helpers below for one text control. */
export interface TextControlHost {
  id: number
  nativeRenderer: NativeRenderer
  /** Apply queued mutations, so the element and its latest props exist natively. */
  flushMutations(): void
}

type SelectionNative = Required<
  Pick<NativeRenderer, "selectText" | "setSelectionRange" | "getTextSelection">
>

function native(host: TextControlHost): SelectionNative {
  const renderer = host.nativeRenderer
  if (!renderer.selectText || !renderer.setSelectionRange || !renderer.getTextSelection) {
    throw new Error("This renderer does not support the text selection API")
  }
  host.flushMutations()
  return renderer as SelectionNative
}

export function selectTextControl(host: TextControlHost): void {
  native(host).selectText(host.id)
}

export function setTextControlSelectionRange(
  host: TextControlHost,
  start: number,
  end: number,
  direction?: SelectionDirection
): void {
  native(host).setSelectionRange(host.id, start, end, direction)
}

export function readTextControlSelection(host: TextControlHost): {
  start: number
  end: number
  direction: SelectionDirection
} {
  const selection = native(host).getTextSelection(host.id)
  return {
    start: selection.start,
    end: selection.end,
    direction: selection.direction as SelectionDirection,
  }
}
