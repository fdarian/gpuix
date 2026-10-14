import { describe, expect, it } from "vitest"
import {
  createMutationQueue,
  flushPendingMutations,
  readBoundingClientRect,
  toBoundingClientRect,
} from "../host.js"
import type { MutationHost, NativeRenderer } from "../host.js"

function recordingRenderer() {
  const batches: unknown[][] = []
  const renderer: MutationHost = {
    applyBatch(json) {
      batches.push(JSON.parse(json))
      return []
    },
  }
  return { renderer, batches }
}

describe("flushPendingMutations", () => {
  it("sends what every queue of the renderer holds, whoever scheduled the flush", () => {
    const { renderer, batches } = recordingRenderer()
    const scheduled: (() => void)[] = []
    const first = createMutationQueue(renderer, undefined, (flush) => scheduled.push(flush))
    const second = createMutationQueue(renderer)
    first.createElement(1, "div")
    second.createElement(2, "div")
    expect(batches).toEqual([])

    flushPendingMutations(renderer)

    expect(batches).toEqual([[["createElement", 1, "div"]], [["createElement", 2, "div"]]])
    expect(first.pending + second.pending).toBe(0)
    scheduled[0]!()
    expect(batches).toHaveLength(2)
  })

  it("leaves another renderer's queue alone", () => {
    const left = recordingRenderer()
    const right = recordingRenderer()
    createMutationQueue(left.renderer).createElement(1, "div")
    createMutationQueue(right.renderer).createElement(1, "div")

    flushPendingMutations(left.renderer)

    expect(left.batches).toHaveLength(1)
    expect(right.batches).toHaveLength(0)
  })
})

describe("readBoundingClientRect", () => {
  it("flushes the queue before asking the renderer", () => {
    const { renderer, batches } = recordingRenderer()
    const reads: number[] = []
    const measuring = Object.assign(renderer, {
      getElementBounds(id: number) {
        reads.push(batches.length)
        return { x: 4, y: 6, width: 10, height: 20 }
      },
    }) as NativeRenderer
    createMutationQueue(measuring).createElement(7, "div")

    const rect = readBoundingClientRect(measuring, 7)

    expect(reads).toEqual([1])
    expect(rect).toEqual({ x: 4, y: 6, width: 10, height: 20, left: 4, top: 6, right: 14, bottom: 26 })
  })

  it("reports a zero rect when the element has no box", () => {
    const { renderer } = recordingRenderer()
    const measuring = Object.assign(renderer, { getElementBounds: () => null }) as NativeRenderer
    expect(readBoundingClientRect(measuring, 1)).toEqual(toBoundingClientRect(null))
    expect(toBoundingClientRect(null).right).toBe(0)
  })

  it("refuses a renderer that cannot measure", () => {
    const { renderer } = recordingRenderer()
    expect(() => readBoundingClientRect(renderer as NativeRenderer, 1)).toThrow(/getElementBounds/)
  })
})
