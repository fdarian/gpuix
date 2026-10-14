import { describe, expect, it } from "vitest"
import type { EventPayload } from "../../index.js"
import type { EventHandlerMap } from "../host.js"
import { captureHandlerKey, dispatchDomEvent, type DomEvent } from "../events/dispatch.js"

const ROOT = 1
const MIDDLE = 2
const LEAF = 3
const PATH = [LEAF, MIDDLE, ROOT]

function payload(eventType: string, fields: Partial<EventPayload> = {}): EventPayload {
  return { elementId: LEAF, eventType, ...fields } as EventPayload
}

function setup() {
  const eventHandlers: EventHandlerMap = new Map()
  const calls: string[] = []
  const listen = (
    id: number,
    key: string,
    handler: (event: DomEvent) => void = () => {},
    label = `${id}:${key}`
  ) => {
    const handlers = eventHandlers.get(id) ?? new Map()
    handlers.set(key, (event) => {
      calls.push(label)
      handler(event as DomEvent)
    })
    eventHandlers.set(id, handlers)
  }
  return { root: { eventHandlers }, calls, listen }
}

describe("dispatchDomEvent", () => {
  it("runs capture root to target, then the target, then bubble target to root", () => {
    const { root, calls, listen } = setup()
    for (const id of PATH) {
      listen(id, "mouseDownCapture", undefined, `capture ${id}`)
      listen(id, "mouseDown", undefined, `bubble ${id}`)
    }
    dispatchDomEvent(root, payload("mouseDown", { path: PATH }))
    expect(calls).toEqual([
      "capture 1",
      "capture 2",
      "capture 3",
      "bubble 3",
      "bubble 2",
      "bubble 1",
    ])
  })

  it("reports the phase and currentTarget of each call while target stays fixed", () => {
    const { root, listen } = setup()
    const seen: Array<[number | null, number, number]> = []
    const record = (event: DomEvent) => seen.push([event.currentTarget, event.eventPhase, event.target])
    listen(ROOT, captureHandlerKey("click"), record)
    listen(LEAF, "click", record)
    listen(MIDDLE, "click", record)
    const event = payload("click", { path: PATH })
    dispatchDomEvent(root, event)
    expect(seen).toEqual([
      [ROOT, 1, LEAF],
      [LEAF, 2, LEAF],
      [MIDDLE, 3, LEAF],
    ])
  })

  it("exposes the payload fields next to the DOM surface", () => {
    const { root, listen } = setup()
    let received: DomEvent | undefined
    listen(LEAF, "mouseDown", (event) => {
      received = event
    })
    dispatchDomEvent(root, payload("mouseDown", { path: PATH, x: 10, y: 20, button: 2, relatedTarget: 9 }))
    expect(received).toMatchObject({
      type: "mouseDown",
      eventType: "mouseDown",
      target: LEAF,
      elementId: LEAF,
      x: 10,
      y: 20,
      button: 2,
      relatedTarget: 9,
      bubbles: true,
      cancelable: true,
      defaultPrevented: false,
    })
    expect(typeof received?.timeStamp).toBe("number")
    expect(received?.currentTarget, "cleared once dispatch ends").toBeNull()
    expect(received?.eventPhase).toBe(0)
  })

  it("stopPropagation in capture keeps the event from reaching the target and bubble", () => {
    const { root, calls, listen } = setup()
    listen(ROOT, "clickCapture", (event) => event.stopPropagation())
    listen(MIDDLE, "clickCapture")
    listen(LEAF, "click")
    const result = dispatchDomEvent(root, payload("click", { path: PATH }))
    expect(calls).toEqual(["1:clickCapture"])
    expect(result.propagationStopped).toBe(true)
  })

  it("stopPropagation still runs the other handlers of the current element", () => {
    const { root, calls, listen } = setup()
    listen(LEAF, "clickCapture", (event) => event.stopPropagation())
    listen(LEAF, "click")
    listen(MIDDLE, "click")
    dispatchDomEvent(root, payload("click", { path: PATH }))
    expect(calls).toEqual(["3:clickCapture", "3:click"])
  })

  it("stopPropagation in bubble stops at that element", () => {
    const { root, calls, listen } = setup()
    listen(LEAF, "click")
    listen(MIDDLE, "click", (event) => event.stopPropagation())
    listen(ROOT, "click")
    const result = dispatchDomEvent(root, payload("click", { path: PATH }))
    expect(calls).toEqual(["3:click", "2:click"])
    expect(result.propagationStopped).toBe(true)
  })

  it("stopImmediatePropagation also skips the rest of the current element", () => {
    const { root, calls, listen } = setup()
    listen(LEAF, "clickCapture", (event) => event.stopImmediatePropagation())
    listen(LEAF, "click")
    listen(MIDDLE, "click")
    const result = dispatchDomEvent(root, payload("click", { path: PATH }))
    expect(calls).toEqual(["3:clickCapture"])
    expect(result.propagationStopped).toBe(true)
  })

  it("does not report propagationStopped when nothing stopped it", () => {
    const { root, listen } = setup()
    listen(LEAF, "click")
    expect(dispatchDomEvent(root, payload("click", { path: PATH })).propagationStopped).toBe(false)
  })

  it("keeps non-bubbling types out of the bubble phase but still captures them", () => {
    for (const type of [
      "mouseEnter",
      "mouseleave",
      "pointerEnter",
      "pointerleave",
      "focus",
      "blur",
      "load",
      "error",
      "scroll",
    ]) {
      const { root, calls, listen } = setup()
      listen(ROOT, type, undefined, "ancestor bubble")
      listen(ROOT, captureHandlerKey(type), undefined, "ancestor capture")
      listen(LEAF, type, undefined, "target")
      dispatchDomEvent(root, payload(type, { path: PATH }))
      expect(calls, type).toEqual(["ancestor capture", "target"])
    }
  })

  it("bubbles mouseover, mouseout, click, focusin and the rest", () => {
    for (const type of ["mouseOver", "mouseOut", "click", "pointerDown", "focusin", "keyDown"]) {
      const { root, calls, listen } = setup()
      listen(ROOT, type, undefined, "ancestor")
      dispatchDomEvent(root, payload(type, { path: PATH }))
      expect(calls, type).toEqual(["ancestor"])
    }
  })

  it("reports defaultPrevented only when a handler called preventDefault", () => {
    const { root, listen } = setup()
    expect(dispatchDomEvent(root, payload("click", { path: PATH })).defaultPrevented).toBe(false)
    listen(ROOT, "click", (event) => event.preventDefault())
    const result = dispatchDomEvent(root, payload("click", { path: PATH }))
    expect(result.defaultPrevented).toBe(true)
  })

  it("shows defaultPrevented to later handlers", () => {
    const { root, listen } = setup()
    let seen: boolean | undefined
    listen(ROOT, "clickCapture", (event) => event.preventDefault())
    listen(LEAF, "click", (event) => {
      seen = event.defaultPrevented
    })
    dispatchDomEvent(root, payload("click", { path: PATH }))
    expect(seen).toBe(true)
  })

  it("ignores preventDefault on a non-cancelable event", () => {
    const { root, listen } = setup()
    listen(LEAF, "mouseEnter", (event) => event.preventDefault())
    expect(dispatchDomEvent(root, payload("mouseEnter", { path: PATH })).defaultPrevented).toBe(false)
  })

  it("handles an event without a path at its element only", () => {
    const { root, calls, listen } = setup()
    listen(LEAF, "clickCapture")
    listen(LEAF, "click")
    listen(MIDDLE, "click")
    listen(ROOT, "clickCapture")
    let target: number | undefined
    listen(LEAF, "keyDown", (event) => {
      target = event.target
    })
    dispatchDomEvent(root, payload("click"))
    dispatchDomEvent(root, payload("keyDown"))
    expect(calls).toEqual(["3:clickCapture", "3:click", "3:keyDown"])
    expect(target).toBe(LEAF)
  })

  it("skips elements that have no handlers and does nothing when none is registered", () => {
    const { root, calls, listen } = setup()
    expect(dispatchDomEvent(root, payload("click", { path: PATH }))).toEqual({
      defaultPrevented: false,
      propagationStopped: false,
    })
    listen(ROOT, "click")
    dispatchDomEvent(root, payload("click", { path: PATH }))
    expect(calls).toEqual(["1:click"])
  })

  it("rejects a path that contradicts the target", () => {
    const { root } = setup()
    expect(() => dispatchDomEvent(root, payload("click", { path: PATH, target: ROOT }))).toThrow(/targets element 1/)
    expect(() => dispatchDomEvent(root, payload("click", { path: [] }))).toThrow(/empty propagation path/)
  })
})
