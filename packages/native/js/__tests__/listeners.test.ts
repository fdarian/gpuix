import { describe, expect, it } from "vitest"
import { captureHandlerKey } from "../events/dispatch.js"
import {
  nativeEventType,
  registerEventListener,
  unregisterEventListener,
} from "../events/listeners.js"
import type { EventHandlerMap } from "../host.js"

function setup() {
  const eventHandlers: EventHandlerMap = new Map()
  const calls: Array<[number, string, boolean]> = []
  const renderer = {
    setEventListener(id: number, eventType: string, hasHandler: boolean) {
      calls.push([id, eventType, hasHandler])
    },
  }
  const noop = () => {}
  return {
    eventHandlers,
    calls,
    add: (id: number, key: string) =>
      registerEventListener(eventHandlers, renderer, id, key, noop),
    remove: (id: number, key: string) =>
      unregisterEventListener(eventHandlers, renderer, id, key),
  }
}

describe("nativeEventType", () => {
  it("strips the capture suffix and nothing else", () => {
    expect(nativeEventType("mousedownCapture")).toBe("mousedown")
    expect(nativeEventType("mousedown")).toBe("mousedown")
    expect(nativeEventType("gotpointercapture")).toBe("gotpointercapture")
    expect(nativeEventType("gotpointercaptureCapture")).toBe("gotpointercapture")
  })
})

describe("native listener registration", () => {
  it("registers a bubble handler once", () => {
    const { add, calls } = setup()
    add(1, "click")
    add(1, "click")
    expect(calls).toEqual([[1, "click", true]])
  })

  it("registers a capture handler on the base type", () => {
    const { add, calls } = setup()
    add(1, captureHandlerKey("click"))
    expect(calls).toEqual([[1, "click", true]])
  })

  it("registers the base type once while both phases are present", () => {
    const { add, calls } = setup()
    add(1, "click")
    add(1, captureHandlerKey("click"))
    expect(calls).toEqual([[1, "click", true]])
  })

  it("keeps listening when only one phase is removed", () => {
    const { add, remove, calls } = setup()
    add(1, "click")
    add(1, captureHandlerKey("click"))
    remove(1, "click")
    expect(calls).toEqual([[1, "click", true]])
    remove(1, captureHandlerKey("click"))
    expect(calls).toEqual([[1, "click", true], [1, "click", false]])
  })

  it("stops listening when the last phase goes, whichever order", () => {
    const { add, remove, calls } = setup()
    add(1, "drop")
    add(1, captureHandlerKey("drop"))
    remove(1, captureHandlerKey("drop"))
    remove(1, "drop")
    expect(calls).toEqual([[1, "drop", true], [1, "drop", false]])
  })

  it("starts again after the listener was removed", () => {
    const { add, remove, calls } = setup()
    add(1, "click")
    remove(1, "click")
    add(1, captureHandlerKey("click"))
    expect(calls).toEqual([[1, "click", true], [1, "click", false], [1, "click", true]])
  })

  it("counts elements and types separately", () => {
    const { add, remove, calls } = setup()
    add(1, "click")
    add(2, "click")
    add(1, "mousedown")
    remove(1, "click")
    expect(calls).toEqual([
      [1, "click", true],
      [2, "click", true],
      [1, "mousedown", true],
      [1, "click", false],
    ])
  })

  it("does nothing when removing a handler that is not registered", () => {
    const { add, remove, calls } = setup()
    remove(1, "click")
    add(1, "click")
    remove(1, captureHandlerKey("click"))
    expect(calls).toEqual([[1, "click", true]])
  })

  it("replaces a handler without telling native again", () => {
    const { eventHandlers, calls } = setup()
    const first = () => {}
    const second = () => {}
    const renderer = { setEventListener: () => calls.push([0, "", false]) }
    registerEventListener(eventHandlers, renderer, 1, "click", first)
    registerEventListener(eventHandlers, renderer, 1, "click", second)
    expect(eventHandlers.get(1)?.get("click")).toBe(second)
    expect(calls).toHaveLength(1)
  })
})
