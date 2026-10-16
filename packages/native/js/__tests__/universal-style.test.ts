import { describe, expect, it } from "vitest"
import { createMutationQueue } from "../mutations.js"
import { TestRenderer, hasNativeTestRenderer } from "../testing.js"
import { pushUniversalStyle } from "../universal-style.js"
import type { StyleDesc } from "../host.js"

function recordingTarget() {
  const layers: Array<StyleDesc | null> = []
  return {
    layers,
    setUniversalStyle(style: StyleDesc | null) {
      layers.push(style)
    },
  }
}

describe("pushUniversalStyle", () => {
  it("sends the layer of the entries, spread in mount order", () => {
    const renderer = {}
    const target = recordingTarget()
    pushUniversalStyle(renderer, target, { color: "#111111", opacity: 0.5 })
    pushUniversalStyle(renderer, target, { color: "#222222", cursor: "pointer" })
    expect(target.layers).toEqual([
      { color: "#111111", opacity: 0.5 },
      { color: "#222222", opacity: 0.5, cursor: "pointer" },
    ])
  })

  it("spreads top-level properties, so a later entry replaces a nested style whole", () => {
    const renderer = {}
    const target = recordingTarget()
    pushUniversalStyle(renderer, target, { hover: { color: "#111111", opacity: 0.5 } })
    pushUniversalStyle(renderer, target, { hover: { color: "#222222" } })
    expect(target.layers.at(-1)).toEqual({ hover: { color: "#222222" } })
  })

  it("removing one entry leaves the others, and the last removal clears the layer", () => {
    const renderer = {}
    const target = recordingTarget()
    const first = pushUniversalStyle(renderer, target, { color: "#111111", opacity: 0.5 })
    const second = pushUniversalStyle(renderer, target, { color: "#222222" })
    first.remove()
    expect(target.layers.at(-1)).toEqual({ color: "#222222" })
    second.remove()
    expect(target.layers.at(-1)).toBeNull()
    first.remove()
    expect(target.layers).toHaveLength(4)
  })

  it("removing the later entry brings the earlier values back", () => {
    const renderer = {}
    const target = recordingTarget()
    pushUniversalStyle(renderer, target, { color: "#111111" })
    const later = pushUniversalStyle(renderer, target, { color: "#222222" })
    later.remove()
    expect(target.layers.at(-1)).toEqual({ color: "#111111" })
  })

  it("updates an entry in place and does not resend what native already holds", () => {
    const renderer = {}
    const target = recordingTarget()
    const entry = pushUniversalStyle(renderer, target, { color: "#111111" })
    entry.update({ color: "#111111" })
    entry.update({ color: "#333333" })
    entry.update({ color: "#333333" })
    expect(target.layers).toEqual([{ color: "#111111" }, { color: "#333333" }])
  })

  it("keeps one stack per renderer", () => {
    const left = {}
    const right = {}
    const target = recordingTarget()
    pushUniversalStyle(left, target, { color: "#111111" })
    const other = pushUniversalStyle(right, target, { opacity: 0.5 })
    expect(target.layers).toEqual([{ color: "#111111" }, { opacity: 0.5 }])
    other.remove()
    expect(target.layers.at(-1)).toBeNull()
  })
})

describe("the mutation queue", () => {
  function queueOver(applied: string[], reject?: (batch: string) => string | undefined) {
    return createMutationQueue({
      applyBatch(json: string) {
        const rejection = reject?.(json)
        if (rejection) throw new Error(rejection)
        applied.push(json)
        return []
      },
      createImageFromBlob: () => {
        throw new Error("unused")
      },
    } as never)
  }

  it("orders the layer with the element mutations", () => {
    const applied: string[] = []
    const queue = queueOver(applied)
    queue.createElement(1, "div")
    queue.setUniversalStyle({ opacity: 0.5 })
    queue.createElement(2, "div")
    queue.setUniversalStyle(null)
    queue.flushMutations()
    expect(JSON.parse(applied[0])).toEqual([
      ["createElement", 1, "div"],
      ["setUniversalStyle", { opacity: 0.5 }],
      ["createElement", 2, "div"],
      ["setUniversalStyle", null],
    ])
  })

  it("drops a layer native rejects and sends the rest, like a rejected setStyle", () => {
    const applied: string[] = []
    const queue = queueOver(applied, (batch) =>
      batch.includes('"setUniversalStyle"')
        ? "Batch op 1 setUniversalStyle parse error: invalid type"
        : undefined
    )
    queue.createElement(1, "div")
    queue.setUniversalStyle({ color: 5 } as never)
    queue.setStyle(1, { opacity: 1 })
    expect(() => queue.flushMutations()).toThrow(/setUniversalStyle parse error/)
    expect(applied.map((batch) => JSON.parse(batch))).toEqual([
      [
        ["createElement", 1, "div"],
        ["setStyle", 1, { opacity: 1 }],
      ],
    ])
    expect(queue.pending).toBe(0)
  })
})

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("setUniversalStyle on the test renderer", () => {
  const apply = (renderer: TestRenderer, ops: unknown[]) => {
    renderer.applyBatch(JSON.stringify(ops))
    renderer.flush()
  }
  const sizeOf = (renderer: TestRenderer, id: number) => {
    const bounds = renderer.getElementBounds(id)
    if (!bounds) throw new Error(`element ${id} has no box`)
    return [bounds.width, bounds.height]
  }

  function mount() {
    const renderer = new TestRenderer({ width: 400, height: 400 })
    apply(renderer, [
      ["createElement", 1, "div"],
      ["setRoot", 1],
      ["setStyle", 1, { display: "flex", alignItems: "flex-start", width: 400, height: 400 }],
      ["createElement", 2, "div"],
      ["appendChild", 1, 2],
      ["createElement", 3, "div"],
      ["appendChild", 1, 3],
      ["setStyle", 3, { width: 70 }],
    ])
    return renderer
  }

  it("styles elements that never had a style, under the ones that did", () => {
    const renderer = mount()
    try {
      expect(sizeOf(renderer, 2)).toEqual([0, 0])
      apply(renderer, [["setUniversalStyle", { width: 50, height: 20, flexShrink: 0 }]])
      expect(sizeOf(renderer, 2)).toEqual([50, 20])
      expect(sizeOf(renderer, 3)).toEqual([70, 20])
    } finally {
      renderer.dispose()
    }
  })

  it("reaches an element mounted later, and a clear restores the own styles", () => {
    const renderer = mount()
    try {
      apply(renderer, [
        ["setUniversalStyle", { width: 50, height: 20, flexShrink: 0 }],
        ["createElement", 4, "div"],
        ["appendChild", 1, 4],
      ])
      expect(sizeOf(renderer, 4)).toEqual([50, 20])
      apply(renderer, [["setUniversalStyle", null]])
      expect(sizeOf(renderer, 2)).toEqual([0, 0])
      expect(sizeOf(renderer, 3)[0]).toBe(70)
      expect(sizeOf(renderer, 4)).toEqual([0, 0])
    } finally {
      renderer.dispose()
    }
  })

  it("rejects an invalid layer with the batch around it and changes nothing", () => {
    const renderer = mount()
    try {
      apply(renderer, [["setUniversalStyle", { width: 50, height: 20, flexShrink: 0 }]])
      expect(() =>
        renderer.applyBatch(
          JSON.stringify([
            ["setStyle", 2, { width: 99 }],
            ["setUniversalStyle", { width: "wide" }],
          ])
        )
      ).toThrow(/Batch op 1 setUniversalStyle parse error/)
      renderer.flush()
      expect(sizeOf(renderer, 2)).toEqual([50, 20])
    } finally {
      renderer.dispose()
    }
  })
})
