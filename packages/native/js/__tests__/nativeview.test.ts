import { describe, expect, it } from "vitest"
import type { DomEvent } from "../events/dispatch.js"
import type { EventHandlerMap } from "../host.js"
import { syncNativeviewListeners } from "../nativeview.js"

const ELEMENT_ID = 7

function setup() {
  const eventHandlers: EventHandlerMap = new Map()
  const listenerCalls: string[] = []
  const renderer = {
    setEventListener(_id: number, type: string, on: boolean) {
      listenerCalls.push(`${type}:${on}`)
    },
  }
  const element = {}
  const sync = (onAttach?: unknown, onDetach?: unknown) =>
    syncNativeviewListeners(eventHandlers, renderer, ELEMENT_ID, onAttach, onDetach)
  const send = (type: "attach" | "detach", nativeHandle?: string) => {
    const handler = eventHandlers.get(ELEMENT_ID)?.get(type)
    if (!handler) throw new Error(`no ${type} handler`)
    handler({ type, elementId: ELEMENT_ID, target: element, nativeHandle } as unknown as DomEvent)
  }
  return { eventHandlers, listenerCalls, sync, send }
}

describe("syncNativeviewListeners", () => {
  it("registers both native listeners for either handler, once", () => {
    const { eventHandlers, listenerCalls, sync } = setup()
    sync(() => {})
    sync(() => {}, () => {})
    sync(undefined, () => {})
    expect([...eventHandlers.get(ELEMENT_ID)!.keys()].sort()).toEqual(["attach", "detach"])
    expect(listenerCalls).toEqual(["attach:true", "detach:true"])
  })

  it("unregisters both when neither is left", () => {
    const { eventHandlers, listenerCalls, sync } = setup()
    sync(() => {}, () => {})
    sync(undefined, undefined)
    expect(eventHandlers.has(ELEMENT_ID)).toBe(false)
    expect(listenerCalls).toEqual(["attach:true", "detach:true", "attach:false", "detach:false"])
  })

  it("rejects a prop that is not a function", () => {
    const { sync } = setup()
    expect(() => sync("nope")).toThrow("onAttach of a <nativeview> must be a function")
  })

  it("hands onAttach the handle as a bigint and pairs onDetach with it", () => {
    const { sync, send } = setup()
    const log: unknown[] = []
    sync((view: bigint) => log.push(view), () => log.push("detach"))
    send("attach", "140737488355328")
    send("detach")
    expect(log).toEqual([140737488355328n, "detach"])
  })

  it("keeps a handle that does not fit a double exact", () => {
    const { sync, send } = setup()
    let seen = 0n
    sync((view: bigint) => (seen = view))
    send("attach", "18446744073709551615")
    expect(seen).toBe(18446744073709551615n)
  })

  it("throws for a missing or non-decimal handle and does not call onAttach", () => {
    const { sync, send } = setup()
    const log: unknown[] = []
    sync((view: bigint) => log.push(view), () => log.push("detach"))
    expect(() => send("attach")).toThrow("no native handle")
    expect(() => send("attach", "0x10")).toThrow("no native handle")
    expect(log).toEqual([])
    send("detach")
    expect(log).toEqual([])
  })

  it("drops an attach that a detach overtook, and calls no onDetach", () => {
    const { sync, send } = setup()
    const log: unknown[] = []
    sync((view: bigint) => log.push(view), () => log.push("detach"))
    send("detach")
    send("attach", "1")
    expect(log).toEqual([])
  })

  it("calls onDetach for every detach when there is no onAttach", () => {
    const { sync, send } = setup()
    const log: unknown[] = []
    sync(undefined, () => log.push("detach"))
    send("attach", "1")
    send("detach")
    expect(log).toEqual(["detach"])
  })

  it("reads the handlers of the latest sync", () => {
    const { sync, send } = setup()
    const log: string[] = []
    sync(() => log.push("old attach"), () => log.push("old detach"))
    sync(() => log.push("new attach"), () => log.push("new detach"))
    send("attach", "1")
    send("detach")
    expect(log).toEqual(["new attach", "new detach"])
  })

  it("still pairs onDetach when onAttach throws", () => {
    const { sync, send } = setup()
    const log: string[] = []
    sync(
      () => {
        throw new Error("boom")
      },
      () => log.push("detach")
    )
    expect(() => send("attach", "1")).toThrow("boom")
    send("detach")
    expect(log).toEqual(["detach"])
  })

  it("throws for a second attach or a second detach", () => {
    const { sync, send } = setup()
    sync(() => {}, () => {})
    send("attach", "1")
    expect(() => send("attach", "1")).toThrow("attached twice")
    send("detach")
    expect(() => send("detach")).toThrow("detached twice")
  })

  it("keeps the state of one element apart from another", () => {
    const first = setup()
    const second = setup()
    const log: string[] = []
    first.sync(() => log.push("first"), () => log.push("first detach"))
    second.sync(() => log.push("second"), () => log.push("second detach"))
    first.send("detach")
    second.send("attach", "1")
    expect(log).toEqual(["second"])
  })
})
