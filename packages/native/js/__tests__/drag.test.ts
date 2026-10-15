import { describe, expect, it } from "vitest"
import type { EventPayload } from "../../index.js"
import type { DomEvent } from "../events/dispatch.js"
import { createDragDispatcher } from "../events/drag.js"
import type { EventHandlerMap } from "../host.js"
import type { HostDataTransfer } from "../events/types.js"

const TARGET = 3
const FILE = { path: "/tmp/a.txt", name: "a.txt", size: 1, type: "text/plain", lastModified: 2 }

function drag(eventType: string, fields: Partial<EventPayload> = {}): EventPayload {
  return { elementId: TARGET, eventType, path: [TARGET], ...fields } as EventPayload
}

const SOURCE = 2

function setup(
  handlers: Record<string, (event: DomEvent) => void>,
  sourceHandlers: Record<string, (event: DomEvent) => void> = {}
) {
  const eventHandlers: EventHandlerMap = new Map([
    [TARGET, new Map(Object.entries(handlers) as [string, never][])],
    [SOURCE, new Map(Object.entries(sourceHandlers) as [string, never][])],
  ])
  let cancelled = 0
  return {
    root: { eventHandlers },
    dispatch: createDragDispatcher(() => cancelled++),
    cancelled: () => cancelled,
  }
}

function transferOf(event: DomEvent): HostDataTransfer {
  return (event as unknown as { dataTransfer: HostDataTransfer }).dataTransfer
}

function fromSource(eventType: string, fields: Partial<EventPayload> = {}): EventPayload {
  return { elementId: SOURCE, eventType, path: [SOURCE], ...fields } as EventPayload
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
    expect(() => dispatch(root, drag("dragover"))).toThrow(/before the drag started/)
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
    expect(() => dispatch(root, drag("dragover"))).toThrow(/before the drag started/)
  })

  it("makes the next target cancel a dragover of its own before a drop reaches it", () => {
    const calls: string[] = []
    const { root, dispatch } = setup(
      { dragover: (event) => event.preventDefault() },
      { drop: (event) => calls.push(`drop ${event.type}`), dragleave: () => calls.push("dragleave") }
    )
    dispatch(root, drag("dragenter", { files: [FILE] }))
    dispatch(root, drag("dragover"))
    dispatch(root, fromSource("dragenter", { relatedTarget: TARGET }))
    dispatch(root, drag("dragleave", { relatedTarget: SOURCE }))
    dispatch(root, fromSource("drop"))
    expect(calls).toEqual(["dragleave"])
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

describe("element drag dispatcher", () => {
  it("lets dragstart fill the dataTransfer and hides it from every other event until the drop", () => {
    const seen: Record<string, string> = {}
    const { root, dispatch } = setup(
      {
        dragover: (event) => {
          seen.over = transferOf(event).getData("text/plain")
          event.preventDefault()
        },
        drop: (event) => {
          seen.drop = transferOf(event).getData("text/plain")
          seen.dropTypes = transferOf(event).types.join()
          seen.drop2 = transferOf(event).getData("TEXT")
        },
      },
      {
        dragstart: (event) => {
          const data = transferOf(event)
          data.setData("text/plain", "hello")
          data.effectAllowed = "move"
          seen.start = data.getData("text/plain")
        },
        dragend: (event) => {
          seen.end = transferOf(event).getData("text/plain")
        },
      }
    )
    dispatch(root, fromSource("dragstart"))
    dispatch(root, drag("dragenter"))
    dispatch(root, drag("dragover"))
    dispatch(root, drag("drop"))
    dispatch(root, fromSource("dragend"))
    expect(seen).toEqual({
      start: "hello",
      over: "",
      drop: "hello",
      dropTypes: "text/plain",
      drop2: "hello",
      end: "",
    })
  })

  it("keeps setData and effectAllowed read-only outside dragstart", () => {
    let transfer: HostDataTransfer | undefined
    const { root, dispatch } = setup(
      {
        dragover: (event) => {
          transfer = transferOf(event)
          transfer.setData("text/plain", "late")
          transfer.effectAllowed = "copy"
          transfer.clearData()
        },
      },
      { dragstart: (event) => transferOf(event).setData("text/plain", "early") }
    )
    dispatch(root, fromSource("dragstart"))
    dispatch(root, drag("dragover"))
    expect(transfer!.effectAllowed).toBe("uninitialized")
    expect(transfer!.types).toEqual(["text/plain"])
  })

  it("delivers a drop after a cancelled dragover and reports its effect in dragend", () => {
    const log: string[] = []
    const { root, dispatch } = setup(
      {
        dragover: (event) => {
          event.preventDefault()
          transferOf(event).dropEffect = "move"
        },
        drop: (event) => log.push(`drop:${transferOf(event).dropEffect}`),
      },
      {
        dragstart: (event) => {
          transferOf(event).effectAllowed = "copyMove"
        },
        dragend: (event) => log.push(`end:${transferOf(event).dropEffect}`),
      }
    )
    dispatch(root, fromSource("dragstart"))
    dispatch(root, drag("dragover"))
    dispatch(root, drag("drop"))
    dispatch(root, fromSource("dragend"))
    expect(log).toEqual(["drop:move", "end:move"])
  })

  it("reports none in dragend when the drop was refused", () => {
    const log: string[] = []
    const { root, dispatch } = setup(
      {
        dragover: () => {},
        drop: () => log.push("drop"),
        dragleave: (event) => log.push(event.type),
      },
      { dragend: (event) => log.push(`end:${transferOf(event).dropEffect}`) }
    )
    dispatch(root, fromSource("dragstart"))
    dispatch(root, drag("dragover"))
    dispatch(root, drag("drop"))
    dispatch(root, fromSource("dragend"))
    expect(log).toEqual(["dragleave", "end:none"])
  })

  it("reports none in dragend after a cancel, and a dragleave to nowhere does not end the drag", () => {
    const log: string[] = []
    const { root, dispatch } = setup(
      { dragover: (event) => event.preventDefault() },
      { dragend: (event) => log.push(`end:${transferOf(event).dropEffect}`) }
    )
    dispatch(root, fromSource("dragstart"))
    dispatch(root, drag("dragover"))
    dispatch(root, drag("dragleave"))
    dispatch(root, fromSource("dragend"))
    expect(log).toEqual(["end:none"])
    expect(() => dispatch(root, drag("dragover"))).toThrow(/before the drag started/)
  })

  it("refuses an effect the source did not allow", () => {
    const log: string[] = []
    const { root, dispatch } = setup(
      {
        dragover: (event) => {
          event.preventDefault()
          transferOf(event).dropEffect = "link"
        },
        drop: () => log.push("drop"),
      },
      { dragstart: (event) => (transferOf(event).effectAllowed = "copy") }
    )
    dispatch(root, fromSource("dragstart"))
    dispatch(root, drag("dragover"))
    dispatch(root, drag("drop"))
    expect(log).toEqual([])
  })

  it("starts each dragover from the effect the source's effectAllowed implies", () => {
    const effects: string[] = []
    const { root, dispatch } = setup(
      { dragover: (event) => effects.push(transferOf(event).dropEffect) },
      { dragstart: (event) => (transferOf(event).effectAllowed = "move") }
    )
    dispatch(root, fromSource("dragstart"))
    dispatch(root, drag("dragover"))
    expect(effects).toEqual(["move"])
  })

  it("cancels the drag when dragstart is prevented and ignores what native had already sent", () => {
    const log: string[] = []
    const { root, dispatch, cancelled } = setup(
      { dragover: () => log.push("over"), drop: () => log.push("drop") },
      {
        dragstart: (event) => event.preventDefault(),
        dragend: () => log.push("end"),
      }
    )
    dispatch(root, fromSource("dragstart"))
    expect(cancelled()).toBe(1)
    dispatch(root, fromSource("drag"))
    dispatch(root, drag("dragenter"))
    dispatch(root, drag("dragover"))
    dispatch(root, drag("drop"))
    dispatch(root, fromSource("dragend"))
    expect(log).toEqual([])

    // The next drag is fresh.
    dispatch(root, fromSource("dragstart"))
    expect(cancelled()).toBe(2)
  })

  it("does not cancel a drag whose dragstart was not prevented", () => {
    const { root, dispatch, cancelled } = setup({}, { dragstart: () => {} })
    dispatch(root, fromSource("dragstart"))
    expect(cancelled()).toBe(0)
  })

  it("a drag of files after an element drag gets its own dataTransfer", () => {
    let transfer: HostDataTransfer | undefined
    const { root, dispatch } = setup({
      dragenter: (event) => (transfer = transferOf(event)),
    })
    dispatch(root, fromSource("dragstart"))
    dispatch(root, fromSource("dragend"))
    dispatch(root, drag("dragenter", { files: [FILE] }))
    expect(transfer!.files).toEqual([FILE])
  })
})
