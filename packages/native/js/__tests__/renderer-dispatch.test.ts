import { describe, expect, it } from "vitest"
import type { EventPayload } from "../../index.js"
import type { EventHandlerMap, NativeRenderer } from "../host.js"
import { captureHandlerKey, type DomEvent } from "../events/dispatch.js"
import { installElementSource } from "../events/window-target.js"
import { createRendererState } from "../renderer-state.js"

const elements = new Map<number, object>([1, 2, 3, 9].map((id) => [id, { id }]))

function setup() {
  const renderer = {} as NativeRenderer
  installElementSource(renderer, (id) => elements.get(id))
  const state = createRendererState(renderer)
  const eventHandlers: EventHandlerMap = new Map()
  const scheduled: Array<() => void> = []
  const seen: EventPayload[] = []
  state.attach({
    eventHandlers,
    schedule: (run) => scheduled.push(run),
    onEvent: (event) => seen.push(event),
  })
  const calls: string[] = []
  const listen = (id: number, key: string, handler?: (event: DomEvent) => void) => {
    const handlers = eventHandlers.get(id) ?? new Map()
    handlers.set(key, (event: DomEvent) => {
      calls.push(`${id}:${key}:${event.eventType}`)
      handler?.(event)
    })
    eventHandlers.set(id, handlers)
  }
  const flush = () => scheduled.splice(0).forEach((run) => run())
  return { state, listen, calls, scheduled, flush, seen }
}

const event = (eventType: string, fields: Partial<EventPayload> = {}) =>
  ({ elementId: 3, eventType, ...fields }) as EventPayload

describe("renderer dispatch of pointer, mouse and drag events", () => {
  it("runs capture then bubble along the path, batched through schedule", () => {
    const { state, listen, calls, scheduled, flush } = setup()
    listen(1, captureHandlerKey("mousedown"))
    listen(3, "mousedown")
    listen(1, "mousedown")
    expect(state.dispatch(event("mousedown", { path: [3, 2, 1] }))).toBe(true)
    expect(calls).toEqual([])
    expect(scheduled).toHaveLength(1)
    flush()
    expect(calls).toEqual([
      "1:mousedownCapture:mousedown",
      "3:mousedown:mousedown",
      "1:mousedown:mousedown",
    ])
  })

  it("matches the old camelCase names native still emits", () => {
    const { state, listen, calls, flush } = setup()
    listen(3, "mousedown")
    listen(3, "dblclick")
    listen(3, "contextmenu")
    listen(3, "auxclick")
    expect(state.dispatch(event("mouseDown"))).toBe(true)
    expect(state.dispatch(event("auxClick"))).toBe(true)
    expect(state.dispatch(event("contextMenu"))).toBe(true)
    flush()
    expect(calls).toEqual([
      "3:mousedown:mousedown",
      "3:auxclick:auxclick",
      "3:contextmenu:contextmenu",
    ])
  })

  it("reports no handler when nobody on the path listens", () => {
    const { state, listen, scheduled } = setup()
    listen(9, "mousedown")
    expect(state.dispatch(event("mousedown", { path: [3, 2, 1] }))).toBe(false)
    expect(state.dispatch(event("mouseup", { path: [3, 2, 1] }))).toBe(false)
    expect(scheduled).toHaveLength(0)
  })

  it("lets a capture-only ancestor see the event", () => {
    const { state, listen, calls, flush } = setup()
    listen(1, captureHandlerKey("pointerdown"))
    expect(state.dispatch(event("pointerdown", { path: [3, 2, 1] }))).toBe(true)
    flush()
    expect(calls).toEqual(["1:pointerdownCapture:pointerdown"])
  })

  it("ignores an enter below an ancestor that only listens in the bubble phase", () => {
    const { state, listen, scheduled } = setup()
    listen(1, "mouseenter")
    expect(state.dispatch(event("mouseenter", { path: [3, 2, 1] }))).toBe(false)
    expect(scheduled).toHaveLength(0)
  })

  it("hears an enter below an ancestor that captures it, and at the element itself", () => {
    const { state, listen, calls, flush } = setup()
    listen(1, captureHandlerKey("mouseenter"))
    expect(state.dispatch(event("mouseenter", { path: [3, 2, 1] }))).toBe(true)
    flush()
    expect(calls).toEqual(["1:mouseenterCapture:mouseenter"])
    listen(3, "mouseenter")
    expect(state.dispatch(event("mouseenter", { path: [3, 2, 1] }))).toBe(true)
    flush()
    expect(calls.slice(1)).toEqual([
      "1:mouseenterCapture:mouseenter",
      "3:mouseenter:mouseenter",
    ])
  })

  it("hands the original payload to onEvent", () => {
    const { state, listen, flush, seen } = setup()
    listen(3, "click")
    const payload = event("click")
    state.dispatch(payload)
    flush()
    expect(seen).toEqual([payload])
  })

  it("drops an event whose target element the root no longer has", () => {
    const { state, listen, scheduled } = setup()
    listen(1, "click")
    expect(state.dispatch(event("click", { path: [77, 1] }))).toBe(false)
    expect(scheduled).toHaveLength(0)
  })

  it("gives the outside-press event the pressed element as target and the listener as currentTarget", () => {
    const { state, listen, flush } = setup()
    const seen: DomEvent[] = []
    listen(1, "mousedownoutside", (outside) => seen.push(outside))
    state.dispatch(event("mouseDownOutside", { elementId: 1, target: 3, path: [3, 2, 1] }))
    flush()
    expect(seen).toHaveLength(1)
    expect(seen[0].target).toBe(elements.get(3))
    expect(seen[0].currentTarget).toBe(elements.get(1))
    expect(seen[0].elementId).toBe(1)
  })

  it("keeps the gpuix outside-press event off the DOM path", () => {
    const { state, listen, calls, flush, seen } = setup()
    listen(3, "mousedownoutside")
    listen(1, "mousedownoutside")
    // Native delivers it to the listener (1), with the press target (3) in the payload.
    const outside = event("mouseDownOutside", { elementId: 1, target: 3, path: [3, 2, 1] })
    expect(state.dispatch(outside)).toBe(true)
    flush()
    expect(calls).toEqual(["1:mousedownoutside:mousedownoutside"])
    expect(seen).toEqual([outside])
  })

  it("leaves other event types alone", () => {
    const { state, listen, calls, flush } = setup()
    listen(3, "scrollChange")
    expect(state.dispatch(event("scrollChange"))).toBe(true)
    flush()
    expect(calls).toEqual(["3:scrollChange:scrollChange"])
  })
})
