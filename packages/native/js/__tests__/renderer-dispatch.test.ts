import { describe, expect, it } from "vitest"
import type { EventPayload } from "../../index.js"
import type { EventHandlerMap, KeyEvent, NativeRenderer } from "../host.js"
import { captureHandlerKey } from "../events/dispatch.js"
import { createRendererState } from "../renderer-state.js"

function setup() {
  const renderer = {} as NativeRenderer
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
  const listen = (id: number, key: string) => {
    const handlers = eventHandlers.get(id) ?? new Map()
    handlers.set(key, (event: EventPayload) => calls.push(`${id}:${key}:${event.eventType}`))
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

  it("hands the original payload to onEvent", () => {
    const { state, listen, flush, seen } = setup()
    listen(3, "click")
    const payload = event("click")
    state.dispatch(payload)
    flush()
    expect(seen).toEqual([payload])
  })

  it("keeps the gpuix outside-press event off the DOM path", () => {
    const { state, listen, calls, flush, seen } = setup()
    listen(3, "mousedownoutside")
    listen(1, "mousedownoutside")
    // Native delivers it to the listener (1), with the press target (3) in the payload.
    const outside = event("mouseDownOutside", { elementId: 1, target: 3, path: [3, 2, 1] })
    expect(state.dispatch(outside)).toBe(true)
    flush()
    expect(calls).toEqual(["1:mousedownoutside:mouseDownOutside"])
    expect(seen).toEqual([outside])
  })

  it("leaves other event types alone", () => {
    const { state, listen, calls, flush } = setup()
    listen(3, "scrollChange")
    expect(state.dispatch(event("scrollChange"))).toBe(true)
    flush()
    expect(calls).toEqual(["3:scrollChange:scrollChange"])
  })

  it("gives a key event click(), which asks the renderer to click the handler's element", () => {
    const clicked: number[] = []
    const renderer = { clickElement: (id: number) => clicked.push(id) } as unknown as NativeRenderer
    const state = createRendererState(renderer)
    const eventHandlers: EventHandlerMap = new Map()
    state.attach({ eventHandlers })
    eventHandlers.set(5, new Map([["keyDown", (event: EventPayload) => (event as KeyEvent).click()]]))
    state.dispatch({ elementId: 5, eventType: "keyDown", key: "enter" } as EventPayload)
    expect(clicked).toEqual([5])
  })

  it("throws from click() when the renderer cannot click an element", () => {
    const { state, scheduled } = setup()
    const errors: unknown[] = []
    const { eventHandlers } = state.current()!
    eventHandlers.set(5, new Map([["keyDown", (event: EventPayload) => {
      try {
        ;(event as KeyEvent).click()
      } catch (error) {
        errors.push(error)
      }
    }]]))
    state.dispatch({ elementId: 5, eventType: "keyDown" } as EventPayload)
    scheduled.splice(0).forEach((run) => run())
    expect(String(errors[0])).toContain("clickElement")
  })
})
