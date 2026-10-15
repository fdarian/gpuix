/// What `Reorder.Group` and `Reorder.Item` mean to the adapters, shared by
/// React and Solid: a pointer drag on an item moves it along the group's axis
/// through the renderer's layout offset, and swaps it with a neighbour once
/// its centre passes the neighbour's.
///
/// The adapters wire DOM pointer events on the item the user grabbed:
/// `pointerdown` calls `pointerDown`, which captures the pointer to the item
/// (`setPointerCapture`), so `pointermove` keeps reaching it after the pointer
/// leaves its box; `pointerup`, `pointercancel` and `lostpointercapture` all
/// call `pointerEnd`. Capture from JS applies from the next native pointer
/// input, and `pointerdown` runs synchronously on macOS, so the first move
/// after the press is already delivered to the item. Where the press is
/// dispatched later (Windows, Linux) a move that came in before the handler
/// ran goes to whatever is under the pointer, and the drag starts at the next.

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
  /** Where item boxes are read from, when a drag starts and again as the order changes during it. */
  renderer: { getElementBounds?: (elementId: number) => ElementBounds | null } | null
}

export type ReorderItem = {
  /** The element id of the item's box, once it has one. */
  elementId(): number | undefined
  /** Routes the pointer's events to the item until it is released. Throws while the item is not in the tree. */
  capturePointer(pointerId: number): void
  drag: LayoutDrag
}

/** The `pointerdown` that starts a drag. */
export type ReorderPress = EventPayload & { pointerId: number }

export function createReorderDrag(): LayoutDrag {
  return { x: motionValue(0), y: motionValue(0), active: motionValue(0) }
}

/** The slots the items had when the drag began: their sizes on the axis and the spaces between consecutive slots. */
export type ReorderSlots<Value> = {
  sizes: Map<Value, number>
  gaps: number[]
  firstStart: number
}

/** What one look at the items showed: their sizes, and where every item but the held one starts. */
type Reading<Value> = {
  values: readonly Value[]
  sizes: Map<Value, number>
  starts: Map<Value, number>
}

type Held<Value> = {
  value: Value
  pointerId: number
  /** The pointer on the group's axis when it pressed. */
  pressed: number
  /** The pointer's distance from the item's centre when it pressed. */
  grab: number
  slots: ReorderSlots<Value>
  /** The order `slots` were measured for. The slots are measured again at a move once `values` is another order. */
  measuredFor: readonly Value[]
  /** The last look at the items made for a new order, which is only trusted once the next look agrees with it. */
  reading: Reading<Value> | undefined
}

function sameOrder<Value>(left: readonly Value[], right: readonly Value[]): boolean {
  return left.length === right.length && left.every((value, index) => value === right[index])
}

function axisSpan(bounds: ElementBounds, axis: ReorderAxis): { start: number; size: number } {
  return axis === "y" ? { start: bounds.y, size: bounds.height } : { start: bounds.x, size: bounds.width }
}

function sizeOf<Value>(slots: { sizes: Map<Value, number> }, value: Value): number {
  const size = slots.sizes.get(value)
  if (size === undefined) throw new Error("Reorder.Group: a value has no measured slot")
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

function sameReading<Value>(left: Reading<Value>, right: Reading<Value>): boolean {
  if (!sameOrder(left.values, right.values)) return false
  const same = (a: Map<Value, number>, b: Map<Value, number>) =>
    a.size === b.size && [...a].every(([key, value]) => b.get(key) === value)
  return same(left.sizes, right.sizes) && same(left.starts, right.starts)
}

/**
 * The slots a reading shows. The held item is drawn where the pointer put it,
 * so its own slot is read off its neighbours instead: halfway between them, or
 * one gap beyond the only neighbour it has on that side. `undefined` when no
 * gap can be told.
 */
function slotsOf<Value>(reading: Reading<Value>, held: Value, previous: ReorderSlots<Value>): ReorderSlots<Value> | undefined {
  const order = reading.values
  if (order.length === 1) return { sizes: reading.sizes, gaps: [], firstStart: previous.firstStart }
  const size = (index: number) => sizeOf(reading, order[index])
  // The held item's entry is a placeholder, replaced below.
  const starts = order.map((value) => {
    if (value === held) return 0
    const start = reading.starts.get(value)
    if (start === undefined) throw new Error("Reorder.Group: a reading has no start for a value")
    return start
  })
  const end = (index: number) => starts[index] + size(index)
  const position = order.indexOf(held)
  const last = order.length - 1
  if (position > 0 && position < last) {
    starts[position] = end(position - 1) + (starts[position + 1] - end(position - 1) - size(position)) / 2
  } else {
    const gap =
      order.length === 2
        ? previous.gaps[0]
        : position === 0
          ? starts[2] - end(1)
          : starts[last - 1] - end(last - 2)
    if (gap === undefined) return undefined
    starts[position] = position === 0 ? starts[1] - gap - size(0) : end(last - 1) + gap
  }
  const gaps = order.slice(1).map((_, index) => starts[index + 1] - end(index))
  return { sizes: reading.sizes, gaps, firstStart: starts[0] }
}

export class ReorderController<Value> {
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
  pointerDown(value: Value, event: ReorderPress): void {
    if (event.button !== undefined && event.button !== 0) return
    this.#release()
    const options = this.#configured()
    const pointer = this.#axisPoint(event)
    const item = this.#item(value)
    const held = this.#measure(options, value, pointer, event.pointerId)
    item.capturePointer(event.pointerId)
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

    if (!options.values.includes(held.value)) {
      this.#release()
      return
    }
    this.#remeasure(held, options)
    // An item that joined has no slot until it has been measured.
    if (options.values.some((value) => !held.slots.sizes.has(value))) return
    const next = reorderedValues(held.slots, options.values, held.value, pointer - held.grab)
    if (sameOrder(next, options.values)) return
    if (this.#proposed !== undefined && sameOrder(next, this.#proposed)) return
    this.#proposed = next
    options.onReorder(next)
  }

  /**
   * The press ended (`pointerup`, `pointercancel` or `lostpointercapture`) on
   * the item for `value`: the held item glides to its slot. An event for another
   * item or pointer is ignored, because a `lostpointercapture` can arrive after
   * a later drag has begun and must not end that one.
   */
  pointerEnd(value: Value, event: { pointerId: number }): void {
    const held = this.#held
    if (held === undefined || held.value !== value || held.pointerId !== event.pointerId) return
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

  #boundsReader(options: ReorderOptions<Value>): (elementId: number) => ElementBounds | null {
    const renderer = options.renderer
    if (renderer === null) throw new Error("Reorder.Group needs a GPUIX root")
    const getElementBounds = renderer.getElementBounds
    if (getElementBounds === undefined) throw new Error("Reorder.Group needs a renderer with getElementBounds")
    return (elementId) => getElementBounds.call(renderer, elementId)
  }

  /**
   * Measures the slots again once `values` is another order than they were
   * measured for: a reorder was applied, or an item joined or left. Items are
   * drawn where they glide to, not where they will settle, so a look is only
   * trusted once the next one agrees with it; until then the slots are the
   * old ones, stacked in the new order. The held item's own box is not read,
   * as it is drawn under the pointer.
   */
  #remeasure(held: Held<Value>, options: ReorderOptions<Value>): void {
    if (sameOrder(held.measuredFor, options.values)) return
    const reading = this.#read(held.value, options)
    const previous = held.reading
    held.reading = reading
    if (reading === undefined || previous === undefined || !sameReading(previous, reading)) return
    const slots = slotsOf(reading, held.value, held.slots)
    if (slots === undefined) return
    held.slots = slots
    held.measuredFor = [...options.values]
    held.reading = undefined
  }

  /** `undefined` while an item has no box yet. */
  #read(held: Value, options: ReorderOptions<Value>): Reading<Value> | undefined {
    const getBounds = this.#boundsReader(options)
    const sizes = new Map<Value, number>()
    const starts = new Map<Value, number>()
    for (const value of options.values) {
      const elementId = this.#items.get(value)?.elementId()
      const bounds = elementId === undefined ? null : getBounds(elementId)
      if (bounds === null) return undefined
      const span = axisSpan(bounds, options.axis)
      sizes.set(value, span.size)
      if (value !== held) starts.set(value, span.start)
    }
    return { values: [...options.values], sizes, starts }
  }

  #measure(options: ReorderOptions<Value>, value: Value, pointer: number, pointerId: number): Held<Value> {
    const getBounds = this.#boundsReader(options)
    const sizes = new Map<Value, number>()
    const starts: number[] = []
    for (const key of options.values) {
      const elementId = this.#item(key).elementId()
      const bounds = elementId === undefined ? null : getBounds(elementId)
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
      value,
      pointerId,
      pressed: pointer,
      grab: pointer - (starts[index] + sizeOf(slots, value) / 2),
      slots,
      measuredFor: [...options.values],
      reading: undefined,
    }
  }
}
