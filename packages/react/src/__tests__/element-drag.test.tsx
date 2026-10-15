/// Dragging an element end to end: the `draggable` prop, the 4px threshold, the
/// session from `dragstart` to `dragend`, and what the dataTransfer lets a
/// handler do. Every test drives the real native dispatcher through the GPU
/// test renderer.

import { describe, it, expect, beforeEach } from "vitest"
import React from "react"
import type { HostDragEvent, HostMouseEvent } from "@gpuix/native"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("draggable elements", () => {
  let testRoot: ReturnType<typeof createTestRoot>
  let log: string[]

  beforeEach(() => {
    testRoot = createTestRoot()
    log = []
  })

  const press = (x: number, y: number) => testRoot.renderer.nativeSimulateMouseDown(x, y)
  const move = (x: number, y: number) => testRoot.renderer.nativeSimulateMouseMove(x, y, 0)
  const release = (x: number, y: number) => testRoot.renderer.nativeSimulateMouseUp(x, y)

  const accept = (event: HostDragEvent) => event.preventDefault()

  /** Source at y 0..100, target at y 100..200, a 100x100 gap-free column. */
  function column(props: {
    source?: Record<string, unknown>
    target?: Record<string, unknown>
    root?: Record<string, unknown>
    sourceChild?: React.ReactNode
  }) {
    return (
      <div
        style={{ width: 400, height: 400, display: "flex", flexDirection: "column" }}
        {...props.root}
      >
        <div style={{ width: 100, height: 100 }} draggable {...props.source}>
          {props.sourceChild}
        </div>
        <div style={{ width: 100, height: 100 }} {...props.target} />
      </div>
    )
  }

  const note = (name: string) => (event: { type: string }) => {
    log.push(`${name}:${event.type}`)
  }

  describe("starting", () => {
    it("is not a drag until the pointer moves more than 4px from the press", () => {
      testRoot.render(
        column({
          source: {
            onDragStart: note("source"),
            onPointerMove: note("source"),
          },
        }),
      )

      press(50, 50)
      move(54, 50)
      move(52, 53)
      expect(log).toEqual(["source:pointermove", "source:pointermove"])

      move(55, 50)
      expect(log.slice(2)).toEqual(["source:dragstart"])
    })

    it("does not start without the draggable prop, and a plain click still clicks", () => {
      testRoot.render(
        column({
          source: { draggable: undefined, onDragStart: note("source"), onClick: note("source") },
        }),
      )

      press(50, 50)
      move(90, 90)
      release(90, 90)
      expect(log).toEqual(["source:click"])

      press(50, 50)
      release(50, 50)
      expect(log).toEqual(["source:click", "source:click"])
    })

    it("stops being draggable when the prop goes away", () => {
      function App({ draggable }: { draggable: boolean }) {
        return column({ source: { draggable, onDragStart: note("source") } })
      }
      testRoot.render(<App draggable />)
      testRoot.render(<App draggable={false} />)

      press(50, 50)
      move(90, 90)

      expect(log).toEqual([])
    })

    it("takes the nearest draggable ancestor of the press target as the source", () => {
      const seen: { target: number; currentTarget: number | null }[] = []
      testRoot.render(
        column({
          source: {
            onDragStart: (event: HostDragEvent) =>
              seen.push({ target: event.target, currentTarget: event.currentTarget }),
          },
          sourceChild: <div style={{ width: 60, height: 60 }} />,
        }),
      )
      const inner = testRoot.renderer.findByType("div").find((div) => div.style.width === 60)!
      const outer = testRoot.renderer.findByType("div").find((div) => div.style.width === 100)!

      press(30, 30)
      move(90, 30)

      expect(seen).toEqual([{ target: outer.id, currentTarget: outer.id }])
      expect(inner.id).not.toBe(outer.id)
    })

    it("lets a draggable child be its own source inside a draggable parent", () => {
      const sources: number[] = []
      testRoot.render(
        <div style={{ width: 200, height: 200 }} draggable onDragStart={(event) => sources.push(event.target)}>
          <div
            style={{ width: 100, height: 100 }}
            draggable
            onDragStart={(event) => {
              sources.push(event.target)
              event.stopPropagation()
            }}
          />
        </div>,
      )
      const child = testRoot.renderer.findByType("div").find((div) => div.style.width === 100)!

      press(50, 50)
      move(100, 50)

      expect(sources).toEqual([child.id])
    })

    it("cancels the pointer first, then starts the drag on the source", () => {
      const record = (event: HostMouseEvent) => log.push(event.type)
      testRoot.render(
        column({
          root: {
            onPointerCancel: record,
            onPointerOut: record,
            onPointerLeave: record,
            onDragStart: record,
            onDrag: record,
            onDragEnter: record,
            onDragOver: record,
          },
        }),
      )

      move(50, 50)
      press(50, 50)
      move(60, 50)

      expect(log).toEqual([
        "pointercancel",
        "pointerout",
        "pointerleave",
        "dragstart",
        "drag",
        "dragenter",
        "dragover",
      ])
    })

    it("fires no pointer, mouse or click event for the rest of the press", () => {
      const record = (event: HostMouseEvent) => log.push(event.type)
      testRoot.render(
        column({
          root: {
            onPointerMove: record,
            onMouseMove: record,
            onPointerOver: record,
            onMouseOver: record,
            onPointerOut: record,
            onMouseOut: record,
            onPointerEnter: record,
            onMouseEnter: record,
            onPointerLeave: record,
            onMouseLeave: record,
            onPointerUp: record,
            onMouseUp: record,
            onClick: record,
            onDragOver: record,
            onDragEnd: record,
          },
        }),
      )

      move(50, 50)
      press(50, 50)
      log.length = 0
      move(60, 50)
      move(60, 150)
      move(61, 150)
      release(61, 150)

      expect(log.filter((type) => !type.startsWith("drag"))).toEqual(["pointerout", "pointerleave", "mouseout", "mouseleave"])
      log.length = 0
      move(70, 150)
      expect(log).toEqual([
        "pointerover",
        "pointerenter",
        "mouseover",
        "mouseenter",
        "pointermove",
        "mousemove",
      ])
    })
  })

  describe("moving", () => {
    it("sends drag to the source, then the boundary events and dragover to what is under it", () => {
      testRoot.render(
        column({
          source: {
            onDragStart: note("source"),
            onDrag: note("source"),
            onDragEnter: note("source"),
            onDragOver: note("source"),
            onDragLeave: note("source"),
          },
          target: {
            onDragEnter: note("target"),
            onDragOver: note("target"),
            onDragLeave: note("target"),
          },
        }),
      )

      press(50, 50)
      move(60, 50)
      log.length = 0
      move(61, 50)
      move(61, 150)

      expect(log).toEqual([
        "source:drag",
        "source:dragover",
        "source:drag",
        "target:dragenter",
        "source:dragleave",
        "target:dragover",
      ])
    })
  })

  describe("dropping", () => {
    it("delivers the data dragstart stored to the drop, and nowhere before", () => {
      const seen: Record<string, string> = {}
      testRoot.render(
        column({
          source: {
            onDragStart: (event: HostDragEvent) => {
              event.dataTransfer!.setData("text/plain", "payload")
              event.dataTransfer!.setData("application/json", '{"id":1}')
              seen.start = event.dataTransfer!.getData("text/plain")
            },
            onDragEnd: (event: HostDragEvent) => {
              seen.end = event.dataTransfer!.getData("text/plain")
            },
          },
          target: {
            onDragEnter: (event: HostDragEvent) => {
              seen.enter = event.dataTransfer!.getData("text/plain")
            },
            onDragOver: (event: HostDragEvent) => {
              seen.over = event.dataTransfer!.getData("text/plain")
              event.preventDefault()
            },
            onDrop: (event: HostDragEvent) => {
              seen.drop = event.dataTransfer!.getData("text/plain")
              seen.json = event.dataTransfer!.getData("application/json")
              seen.types = event.dataTransfer!.types.join()
            },
          },
        }),
      )

      press(50, 50)
      move(60, 50)
      move(60, 150)
      release(60, 150)

      expect(seen).toEqual({
        start: "payload",
        enter: "",
        over: "",
        drop: "payload",
        json: '{"id":1}',
        types: "text/plain,application/json",
        end: "",
      })
    })

    it("fires drop and then dragend at the source, with the effect the target chose", () => {
      testRoot.render(
        column({
          source: {
            onDragStart: (event: HostDragEvent) => {
              event.dataTransfer!.effectAllowed = "copyMove"
            },
            onDragEnd: (event: HostDragEvent) =>
              log.push(`end:${event.target === event.currentTarget}:${event.dataTransfer!.dropEffect}`),
          },
          target: {
            onDragOver: (event: HostDragEvent) => {
              event.preventDefault()
              event.dataTransfer!.dropEffect = "move"
            },
            onDrop: (event: HostDragEvent) => log.push(`drop:${event.dataTransfer!.dropEffect}`),
          },
        }),
      )

      press(50, 50)
      move(60, 150)
      release(60, 150)

      expect(log).toEqual(["drop:move", "end:true:move"])
    })

    it("fires dragend with dropEffect none when the target did not accept", () => {
      testRoot.render(
        column({
          source: { onDragEnd: (event: HostDragEvent) => log.push(`end:${event.dataTransfer!.dropEffect}`) },
          target: {
            onDragOver: () => {},
            onDragLeave: note("target"),
            onDrop: note("target"),
          },
        }),
      )

      press(50, 50)
      move(60, 150)
      release(60, 150)

      expect(log).toEqual(["target:dragleave", "end:none"])
    })

    it("fires dragend with dropEffect none when released over nothing that listens", () => {
      testRoot.render(
        column({ source: { onDragEnd: (event: HostDragEvent) => log.push(`end:${event.dataTransfer!.dropEffect}`) } }),
      )

      press(50, 50)
      move(60, 150)
      release(300, 300)

      expect(log).toEqual(["end:none"])
    })

    it("refuses an effect the source does not allow", () => {
      testRoot.render(
        column({
          source: {
            onDragStart: (event: HostDragEvent) => {
              event.dataTransfer!.effectAllowed = "copy"
            },
            onDragEnd: (event: HostDragEvent) => log.push(`end:${event.dataTransfer!.dropEffect}`),
          },
          target: {
            onDragOver: (event: HostDragEvent) => {
              event.preventDefault()
              event.dataTransfer!.dropEffect = "move"
            },
            onDrop: note("target"),
          },
        }),
      )

      press(50, 50)
      move(60, 150)
      release(60, 150)

      expect(log).toEqual(["end:none"])
    })

    it("is not a click, on the source or anywhere else", () => {
      testRoot.render(
        column({
          root: { onClick: note("root"), onMouseUp: note("root") },
          target: { onDragOver: accept },
        }),
      )

      press(50, 50)
      move(60, 150)
      release(60, 150)

      expect(log).toEqual([])
    })

    it("hides the data in the events that are not dragstart or drop", () => {
      const seen: string[] = []
      const read = (event: HostDragEvent) =>
        seen.push(`${event.type}:${event.dataTransfer!.getData("text/plain")}`)
      testRoot.render(
        column({
          source: {
            onDragStart: (event: HostDragEvent) => {
              event.dataTransfer!.setData("text/plain", "secret")
              read(event)
            },
            onDrag: read,
            onDragEnd: read,
          },
          target: {
            onDragEnter: read,
            onDragOver: (event: HostDragEvent) => {
              read(event)
              event.preventDefault()
              event.dataTransfer!.setData("text/plain", "overwritten")
            },
            onDragLeave: read,
            onDrop: read,
          },
        }),
      )

      press(50, 50)
      move(60, 150)
      move(60, 151)
      release(60, 151)

      expect(seen).toEqual([
        "dragstart:secret",
        "drag:",
        "dragenter:",
        "dragover:",
        "drag:",
        "dragover:",
        "drop:secret",
        "dragend:",
      ])
    })
  })

  describe("ending without a drop", () => {
    it("is cancelled by Escape: dragleave, then dragend with dropEffect none, and no drop", () => {
      testRoot.render(
        column({
          source: {
            tabIndex: 0,
            onDragEnd: (event: HostDragEvent) => log.push(`end:${event.dataTransfer!.dropEffect}`),
          },
          target: {
            onDragOver: accept,
            onDragLeave: (event: HostDragEvent) =>
              log.push(`leave:${event.relatedTarget === undefined ? "nowhere" : "element"}`),
            onDrop: note("target"),
          },
        }),
      )
      const source = testRoot.renderer.findByType("div").find((div) => div.style.width === 100 && div.events.has("dragend"))!

      press(50, 50)
      move(60, 150)
      testRoot.renderer.nativeSimulateKeyDown(source.id, "escape")
      expect(log).toEqual(["leave:nowhere", "end:none"])

      release(60, 150)
      move(70, 150)
      expect(log).toEqual(["leave:nowhere", "end:none"])
    })

    it("is cancelled by preventDefault in dragstart: no more drag events, pointer events return", () => {
      const record = (event: HostMouseEvent) => log.push(event.type)
      testRoot.render(
        column({
          root: { onPointerMove: record, onPointerUp: record, onClick: record },
          source: {
            onDragStart: (event: HostDragEvent) => {
              log.push("dragstart")
              event.preventDefault()
            },
            onDrag: record,
            onDragEnd: record,
          },
          target: { onDragEnter: record, onDragOver: record, onDrop: record },
        }),
      )

      press(50, 50)
      move(60, 50)
      move(60, 150)
      move(61, 150)
      release(61, 150)

      expect(log).toEqual(["dragstart", "pointermove", "pointermove", "pointerup"])
    })

    it("lets the next drag start normally after a cancelled one", () => {
      let cancel = true
      testRoot.render(
        column({
          source: {
            onDragStart: (event: HostDragEvent) => {
              log.push(`start:${cancel}`)
              if (cancel) event.preventDefault()
            },
            onDragEnd: note("source"),
          },
        }),
      )

      press(50, 50)
      move(60, 50)
      move(61, 50)
      release(61, 50)
      cancel = false
      press(50, 50)
      move(60, 50)
      release(60, 50)

      expect(log).toEqual(["start:true", "start:false", "source:dragend"])
    })
  })
})
