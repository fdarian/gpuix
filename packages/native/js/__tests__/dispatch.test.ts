import { afterEach, describe, expect, it, vi } from "vitest"
import type { EventPayload } from "../../index.js"
import type { EventHandlerMap } from "../host.js"
import { captureHandlerKey, dispatchDomEvent, type DomEvent } from "../events/dispatch.js"
import { dispatchPropagating, document, window } from "../events/window-target.js"

const ROOT = 1
const MIDDLE = 2
const LEAF = 3
const PATH = [LEAF, MIDDLE, ROOT]
const OTHER = 9

const elements = new Map<number, object>([ROOT, MIDDLE, LEAF, OTHER].map((id) => [id, { id }]))
const elementById = (id: number) => elements.get(id)
const element = (id: number): object => {
  const found = elements.get(id)
  if (!found) throw new Error(`no test element ${id}`)
  return found
}

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
  return { root: { eventHandlers, elementById }, calls, listen }
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
    const seen: Array<[object | null, number, object | null]> = []
    const record = (event: DomEvent) => seen.push([event.currentTarget, event.eventPhase, event.target])
    listen(ROOT, captureHandlerKey("click"), record)
    listen(LEAF, "click", record)
    listen(MIDDLE, "click", record)
    const event = payload("click", { path: PATH })
    dispatchDomEvent(root, event)
    expect(seen).toHaveLength(3)
    expect(seen[0][0]).toBe(element(ROOT))
    expect(seen[1][0]).toBe(element(LEAF))
    expect(seen[2][0]).toBe(element(MIDDLE))
    expect(seen.map((entry) => entry[1])).toEqual([1, 2, 3])
    for (const entry of seen) expect(entry[2]).toBe(element(LEAF))
  })

  it("gives the elements, not their ids, as target and relatedTarget, and keeps elementId numeric", () => {
    const { root, listen } = setup()
    let received: DomEvent | undefined
    listen(LEAF, "focusout", (event) => {
      received = event
    })
    dispatchDomEvent(root, payload("focusout", { path: PATH, target: LEAF, relatedTarget: OTHER }))
    expect(received?.target).toBe(element(LEAF))
    expect(received?.relatedTarget).toBe(element(OTHER))
    expect(received?.elementId).toBe(LEAF)
    expect(received?.composedPath()).toEqual([element(LEAF), element(MIDDLE), element(ROOT)])
    expect(received).not.toHaveProperty("path")
  })

  it("has a null relatedTarget when native sent none, or the root has dropped that element", () => {
    const { root, listen } = setup()
    const seen: Array<object | null> = []
    listen(LEAF, "focusin", (event) => seen.push(event.relatedTarget))
    dispatchDomEvent(root, payload("focusin", { path: PATH }))
    dispatchDomEvent(root, payload("focusin", { path: PATH, relatedTarget: 77 }))
    expect(seen).toEqual([null, null])
  })

  it("throws for a path element the root does not have", () => {
    const { root } = setup()
    expect(() => dispatchDomEvent(root, payload("click", { path: [LEAF, 55] }))).toThrow(/element 55/)
  })

  it("exposes the payload fields next to the DOM surface", () => {
    const { root, listen } = setup()
    let received: DomEvent | undefined
    listen(LEAF, "mouseDown", (event) => {
      received = event
    })
    dispatchDomEvent(root, payload("mouseDown", { path: PATH, x: 10, y: 20, button: 2, relatedTarget: OTHER }))
    expect(received).toMatchObject({
      type: "mouseDown",
      eventType: "mouseDown",
      target: element(LEAF),
      elementId: LEAF,
      x: 10,
      y: 20,
      button: 2,
      relatedTarget: element(OTHER),
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
    for (const type of ["mouseOver", "mouseOut", "click", "pointerDown", "focusin", "keydown"]) {
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

  it("does not bubble invalid, but lets a handler cancel it", () => {
    const { root, calls, listen } = setup()
    listen(MIDDLE, "invalid")
    listen(LEAF, "invalid", (event) => event.preventDefault())
    const result = dispatchDomEvent(root, payload("invalid", { path: PATH }))
    expect(calls).toEqual(["3:invalid"])
    expect(result.defaultPrevented).toBe(true)
  })

  it("gives a submit event its FormData and no submitter", () => {
    const { root, listen } = setup()
    let seen: { formData: FormData; submitter: unknown } | undefined
    listen(ROOT, "submit", (event) => {
      seen = event as unknown as { formData: FormData; submitter: unknown }
    })
    dispatchDomEvent(
      root,
      payload("submit", {
        path: PATH,
        formEntries: [
          ["a", "1"],
          ["a", "2"],
          ["b", ""],
        ],
      })
    )
    expect(Array.from(seen!.formData.entries())).toEqual([
      ["a", "1"],
      ["a", "2"],
      ["b", ""],
    ])
    expect(seen!.formData.getAll("a")).toEqual(["1", "2"])
    expect(seen!.submitter).toBeNull()
  })

  it("handles an event without a path at its element only", () => {
    const { root, calls, listen } = setup()
    listen(LEAF, "clickCapture")
    listen(LEAF, "click")
    listen(MIDDLE, "click")
    listen(ROOT, "clickCapture")
    let target: object | null | undefined
    listen(LEAF, "keydown", (event) => {
      target = event.target
    })
    dispatchDomEvent(root, payload("click"))
    dispatchDomEvent(root, payload("keydown"))
    expect(calls).toEqual(["3:clickCapture", "3:click", "3:keydown"])
    expect(target).toBe(element(LEAF))
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

describe("a handler that throws", () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it("does not stop the dispatch, and reports its error with reportError once the dispatch is over", () => {
    const reported: unknown[] = []
    vi.stubGlobal("reportError", (error: unknown) => reported.push(error))
    const { root, calls, listen } = setup()
    const failure = new Error("boom")
    let phaseAfter: number | undefined
    listen(ROOT, captureHandlerKey("click"), () => {
      throw failure
    })
    listen(LEAF, "click", (event) => {
      expect(reported).toEqual([])
      phaseAfter = event.eventPhase
    })
    listen(ROOT, "click")
    const event = payload("click", { path: PATH })
    expect(() => dispatchDomEvent(root, event)).not.toThrow()
    expect(calls).toEqual(["1:clickCapture", "3:click", "1:click"])
    expect(phaseAfter).toBe(2)
    expect(reported).toEqual([failure])
  })

  it("reports every error, and still resets currentTarget and eventPhase", () => {
    const reported: unknown[] = []
    vi.stubGlobal("reportError", (error: unknown) => reported.push(error))
    const { root, listen } = setup()
    const seen: DomEvent[] = []
    listen(LEAF, "click", (event) => {
      seen.push(event)
      throw new Error("first")
    })
    listen(ROOT, "click", () => {
      throw new Error("second")
    })
    dispatchDomEvent(root, payload("click", { path: PATH }))
    expect(reported.map((error) => (error as Error).message)).toEqual(["first", "second"])
    expect(seen[0].currentTarget).toBeNull()
    expect(seen[0].eventPhase).toBe(0)
  })

  it("rethrows asynchronously when there is no reportError", () => {
    vi.stubGlobal("reportError", undefined)
    const queued: Array<() => void> = []
    vi.stubGlobal("queueMicrotask", (task: () => void) => queued.push(task))
    const { root, calls, listen } = setup()
    const failure = new Error("boom")
    listen(LEAF, "click", () => {
      throw failure
    })
    listen(ROOT, "click")
    expect(() => dispatchDomEvent(root, payload("click", { path: PATH }))).not.toThrow()
    expect(calls).toEqual(["3:click", "1:click"])
    expect(queued).toHaveLength(1)
    expect(queued[0]).toThrow(failure)
  })

  it("goes to the root's reportError instead of the global handler, in handler order", () => {
    const global = vi.fn()
    vi.stubGlobal("reportError", global)
    const reported: unknown[] = []
    const { root, listen } = setup()
    listen(LEAF, "click", () => {
      throw new Error("first")
    })
    listen(ROOT, "click", () => {
      throw new Error("second")
    })
    dispatchDomEvent({ ...root, reportError: (error) => reported.push(error) }, payload("click", { path: PATH }))
    expect(reported.map((error) => (error as Error).message)).toEqual(["first", "second"])
    expect(global).not.toHaveBeenCalled()
  })

  it("does not end the dispatch when a window or document listener throws", () => {
    const reported: unknown[] = []
    const { root, calls, listen } = setup()
    const seen: string[] = []
    const windowCapture = () => {
      seen.push("window capture")
      throw new Error("window capture")
    }
    const documentBubble = () => {
      seen.push("document bubble")
      throw new Error("document bubble")
    }
    const windowBubble = () => seen.push("window bubble")
    window.addEventListener("click", windowCapture, { capture: true })
    document.addEventListener("click", documentBubble)
    window.addEventListener("click", windowBubble)
    listen(LEAF, "click")
    try {
      dispatchPropagating(
        { ...root, reportError: (error) => reported.push(error) },
        payload("click", { path: PATH })
      )
    } finally {
      window.removeEventListener("click", windowCapture, { capture: true })
      document.removeEventListener("click", documentBubble)
      window.removeEventListener("click", windowBubble)
    }
    expect(seen).toEqual(["window capture", "document bubble", "window bubble"])
    expect(calls).toEqual(["3:click"])
    expect(reported.map((error) => (error as Error).message)).toEqual(["window capture", "document bubble"])
  })

  it("keeps going past a throwing listener of an event aimed at the window", () => {
    const reported: unknown[] = []
    const seen: string[] = []
    const first = () => {
      seen.push("first")
      throw new Error("first")
    }
    const second = () => seen.push("second")
    window.addEventListener("keydown", first)
    window.addEventListener("keydown", second)
    try {
      dispatchPropagating(
        { eventHandlers: new Map(), elementById, reportError: (error) => reported.push(error) },
        payload("keydown")
      )
    } finally {
      window.removeEventListener("keydown", first)
      window.removeEventListener("keydown", second)
    }
    expect(seen).toEqual(["first", "second"])
    expect(reported).toHaveLength(1)
  })
})

describe("window and document listeners", () => {
  it("see the target element while currentTarget is the window or document", () => {
    const { root, listen } = setup()
    listen(LEAF, "click")
    const seen: Array<[string, object | null, object | null]> = []
    const onWindow = (event: DomEvent) => seen.push(["window", event.target, event.currentTarget])
    const onDocument = (event: DomEvent) => seen.push(["document", event.target, event.currentTarget])
    window.addEventListener("click", onWindow)
    document.addEventListener("click", onDocument)
    try {
      dispatchPropagating(root, payload("click", { path: PATH }))
    } finally {
      window.removeEventListener("click", onWindow)
      document.removeEventListener("click", onDocument)
    }
    expect(seen.map((entry) => entry[0])).toEqual(["document", "window"])
    expect(seen[0][1]).toBe(element(LEAF))
    expect(seen[0][2]).toBe(document)
    expect(seen[1][1]).toBe(element(LEAF))
    expect(seen[1][2]).toBe(window)
  })

  it("see a null target for an event aimed at the window", () => {
    const { root } = setup()
    const seen: Array<[object | null, object | null, object[]]> = []
    const onKey = (event: DomEvent) => seen.push([event.target, event.currentTarget, event.composedPath()])
    window.addEventListener("keydown", onKey)
    try {
      dispatchPropagating(root, payload("keydown"))
    } finally {
      window.removeEventListener("keydown", onKey)
    }
    expect(seen).toHaveLength(1)
    expect(seen[0][0]).toBeNull()
    expect(seen[0][1]).toBe(window)
    expect(seen[0][2]).toEqual([])
  })
})
