/// What `Reorder.Group` and `Reorder.Item` mean to the adapters, shared by
/// React and Solid: a pointer drag on an item moves it along the group's axis
/// through the renderer's layout offset, and swaps it with a neighbour once
/// its centre passes the neighbour's.
///
/// Pointer events are read from the item the user grabbed, which relies on
/// the renderer's implicit capture: an element with both `onMouseDown` and
/// `onMouseMove` keeps receiving moves and the release after the pointer
/// leaves it.

import type { ElementBounds, EventPayload } from "../index.js"
import type { LayoutDrag } from "./layout-animation.js"
import { motionValue } from "./motion-value.js"

export type ReorderAxis = "x" | "y"

export type ReorderOptions<Value> = {
  /** The axis items are laid out and dragged along. */
  axis: ReorderAxis
  /** The current order. Every value needs a registered item. */
  values: readonly Value[]
  /** Called with the order a drag proposes. The group never reorders by itself: pass the result back as `values`. */
  onReorder: (values: Value[]) => void
  /** Where item boxes are read from, when a drag starts. */
  renderer: { getElementBounds(elementId: number): ElementBounds | null } | undefined
}

export type ReorderItem = {
  /** The element id of the item's box, once it has one. */
  elementId(): number | undefined
  drag: LayoutDrag
}

export function createReorderDrag(): LayoutDrag {
  return { x: motionValue(0), y: motionValue(0), active: motionValue(0) }
}

/** The slots the items had when the drag began: their sizes on the axis and the spaces between consecutive slots. */
export type ReorderSlots<Value> = {
  sizes: Map<Value, number>
  gaps: number[]
  firstStart: number
}

type Held<Value> = ReorderSlots<Value> & {
  value: Value
  /** The pointer on the group's axis when it pressed. */
  pressed: number
  /** The pointer's distance from the item's centre when it pressed. */
  grab: number
}

function sameOrder<Value>(left: readonly Value[], right: readonly Value[]): boolean {
  return left.length === right.length && left.every((value, index) => value === right[index])
}

function axisSpan(bounds: ElementBounds, axis: ReorderAxis): { start: number; size: number } {
  return axis === "y" ? { start: bounds.y, size: bounds.height } : { start: bounds.x, size: bounds.width }
}

function sizeOf<Value>(slots: ReorderSlots<Value>, value: Value): number {
  const size = slots.sizes.get(value)
  if (size === undefined) throw new Error("Reorder.Group: an item joined the group while another was being dragged")
  return size
}

/** The slots are stacked in `order`, so the answer does not wait for the layout to follow a swap. */
function centreOfSlot<Value>(slots: ReorderSlots<Value>, order: readonly Value[], position: number): number {
  let start = slots.firstStart
  for (let index = 0; index < position; index++) {
    start += sizeOf(slots, order[index]) + slots.gaps[index]
  }
  return start + sizeOf(slots, order[position]) / 2
}

/**
 * The order after dragging `value` until its centre is at `centre`: past a
 * neighbour's centre it takes that neighbour's place.
 */
export function reorderedValues<Value>(
  slots: ReorderSlots<Value>,
  order: readonly Value[],
  value: Value,
  centre: number
): Value[] {
  const next = [...order]
  let position = next.indexOf(value)
  if (position === -1) throw new Error("Reorder.Group: the dragged value is not in values")
  const swap = (from: number, to: number) => {
    const moved = next[from]
    next[from] = next[to]
    next[to] = moved
  }
  while (position < next.length - 1 && centre > centreOfSlot(slots, next, position + 1)) {
    swap(position, position + 1)
    position += 1
  }
  while (position > 0 && centre < centreOfSlot(slots, next, position - 1)) {
    swap(position, position - 1)
    position -= 1
  }
  return next
}

export class ReorderGroup<Value> {
  readonly #items = new Map<Value, ReorderItem>()
  #options: ReorderOptions<Value> | undefined
  #held: Held<Value> | undefined
  /** The order last handed to `onReorder`, until `values` changes, so a move that proposes it again says nothing. */
  #proposed: readonly Value[] | undefined

  /** Takes the group's latest props. Call it whenever they change. */
  configure(options: ReorderOptions<Value>): void {
    if (this.#options !== undefined && !sameOrder(this.#options.values, options.values)) this.#proposed = undefined
    this.#options = options
  }

  /** Makes `item` the one that `value` stands for. Returns what undoes it. */
  register(value: Value, item: ReorderItem): () => void {
    this.#items.set(value, item)
    return () => {
      if (this.#items.get(value) !== item) return
      this.#items.delete(value)
      if (this.#held?.value === value) this.#held = undefined
    }
  }

  isDragging(value: Value): boolean {
    return this.#held?.value === value
  }

  /** The pointer went down on the item for `value`. */
  pointerDown(value: Value, event: EventPayload): void {
    if (event.button !== undefined && event.button !== 0) return
    this.#release()
    const options = this.#configured()
    const pointer = this.#axisPoint(event)
    const item = this.#item(value)
    const held = this.#measure(options, value, pointer)
    item.drag.x.jump(0)
    item.drag.y.jump(0)
    item.drag.active.jump(1)
    this.#held = held
    this.#proposed = undefined
  }

  /** The pointer moved. Does nothing unless an item is held. */
  pointerMove(event: EventPayload): void {
    const held = this.#held
    if (held === undefined) return
    // A release the window never saw: the button is no longer down.
    if (event.pressedButton !== 0) {
      this.#release()
      return
    }
    const options = this.#configured()
    const pointer = this.#axisPoint(event)
    const drag = this.#item(held.value).drag
    ;(options.axis === "y" ? drag.y : drag.x).set(pointer - held.pressed)

    const next = reorderedValues(held, options.values, held.value, pointer - held.grab)
    if (sameOrder(next, options.values)) return
    if (this.#proposed !== undefined && sameOrder(next, this.#proposed)) return
    this.#proposed = next
    options.onReorder(next)
  }

  /** The pointer went up: the held item glides to its slot. */
  pointerUp(): void {
    this.#release()
  }

  #release(): void {
    const held = this.#held
    if (held === undefined) return
    this.#held = undefined
    const item = this.#items.get(held.value)
    if (item === undefined) return
    // `active` first: once it is off the offsets are no longer read.
    item.drag.active.jump(0)
    item.drag.x.jump(0)
    item.drag.y.jump(0)
  }

  #configured(): ReorderOptions<Value> {
    if (this.#options === undefined) throw new Error("Reorder.Group: the group has not been configured")
    return this.#options
  }

  #item(value: Value): ReorderItem {
    const item = this.#items.get(value)
    if (item === undefined) throw new Error("Reorder.Item: no item is registered for this value")
    return item
  }

  #axisPoint(event: EventPayload): number {
    const point = this.#configured().axis === "y" ? event.y : event.x
    if (point === undefined) throw new Error("Reorder.Item: the pointer event has no position")
    return point
  }

  #measure(options: ReorderOptions<Value>, value: Value, pointer: number): Held<Value> {
    if (options.renderer === undefined) throw new Error("Reorder.Group needs a GPUIX root")
    const sizes = new Map<Value, number>()
    const starts: number[] = []
    for (const key of options.values) {
      const elementId = this.#item(key).elementId()
      const bounds = elementId === undefined ? null : options.renderer.getElementBounds(elementId)
      if (bounds === null) throw new Error("Reorder.Item: an item has not been laid out yet, so it cannot be dragged")
      const span = axisSpan(bounds, options.axis)
      sizes.set(key, span.size)
      starts.push(span.start)
    }
    const index = options.values.indexOf(value)
    if (index === -1) throw new Error("Reorder.Item: the value is not in the group's values")
    const slots: ReorderSlots<Value> = { sizes, gaps: [], firstStart: starts[0] }
    slots.gaps = starts.slice(1).map((start, at) => start - (starts[at] + sizeOf(slots, options.values[at])))
    return {
      ...slots,
      value,
      pressed: pointer,
      grab: pointer - (starts[index] + sizeOf(slots, value) / 2),
    }
  }
}
