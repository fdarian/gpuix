import { describe, expect, it } from "vitest"
import type { EventPayload } from "../../index.js"
import type { DomEvent } from "../events/dispatch.js"
import { createFileDragDispatcher } from "../events/file-drag.js"
import type { EventHandlerMap } from "../host.js"
import type { HostDataTransfer } from "../events/types.js"

const TARGET = 3
const FILE = { path: "/tmp/a.txt", name: "a.txt", size: 1, type: "text/plain", lastModified: 2 }

function drag(eventType: string, fields: Partial<EventPayload> = {}): EventPayload {
  return { elementId: TARGET, eventType, path: [TARGET], ...fields } as EventPayload
}

function setup(handlers: Record<string, (event: DomEvent) => void>) {
  const eventHandlers: EventHandlerMap = new Map([
    [TARGET, new Map(Object.entries(handlers) as [string, never][])],
  ])
  return { root: { eventHandlers }, dispatch: createFileDragDispatcher() }
}

describe("file drag dispatcher", () => {
  it("delivers a drop after a cancelled dragover and leaves the drag", () => {
    const calls: string[] = []
    const { root, dispatch } = setup({
      dragover: (event) => {
        calls.push(event.type)
        event.preventDefault()
      },
      drop: (event) => calls.push(event.type),
      dragleave: (event) => calls.push(event.type),
    })
    dispatch(root, drag("dragenter", { files: [FILE] }))
    dispatch(root, drag("dragover"))
    dispatch(root, drag("drop"))
    expect(calls).toEqual(["dragover", "drop"])
    expect(() => dispatch(root, drag("dragover"))).toThrow(/before any dragenter/)
  })

  it("turns a drop nobody accepted into the dragleave of the DOM", () => {
    const calls: string[] = []
    const { root, dispatch } = setup({
      dragover: () => {},
      drop: (event) => calls.push(event.type),
      dragleave: (event) => calls.push(event.type),
    })
    dispatch(root, drag("dragenter", { files: [FILE] }))
    dispatch(root, drag("dragover"))
    dispatch(root, drag("drop"))
    expect(calls).toEqual(["dragleave"])
  })

  it("refuses a drop whose dropEffect the handler set to none", () => {
    const calls: string[] = []
    const { root, dispatch } = setup({
      dragover: (event) => {
        event.preventDefault()
        ;(event as unknown as { dataTransfer: HostDataTransfer }).dataTransfer.dropEffect = "none"
      },
      drop: (event) => calls.push(event.type),
    })
    dispatch(root, drag("dragenter", { files: [FILE] }))
    dispatch(root, drag("dragover"))
    dispatch(root, drag("drop"))
    expect(calls).toEqual([])
  })

  it("keeps the drag across a dragleave to another element, and ends it on one to nowhere", () => {
    const { root, dispatch } = setup({ dragover: (event) => event.preventDefault() })
    dispatch(root, drag("dragenter", { files: [FILE] }))
    dispatch(root, drag("dragleave", { relatedTarget: 4 }))
    dispatch(root, drag("dragover"))
    dispatch(root, drag("dragleave"))
    expect(() => dispatch(root, drag("dragover"))).toThrow(/before any dragenter/)
  })

  it("gives a read-only dataTransfer", () => {
    let seen: HostDataTransfer | undefined
    const { root, dispatch } = setup({
      dragenter: (event) => {
        seen = (event as unknown as { dataTransfer: HostDataTransfer }).dataTransfer
      },
    })
    dispatch(root, drag("dragenter", { files: [FILE] }))
    seen!.setData("text/plain", "x")
    expect(seen!.getData("text/plain")).toBe("")
    expect(seen!.files).toEqual([FILE])
    expect(Object.isFrozen(seen!.files)).toBe(true)
  })
})
