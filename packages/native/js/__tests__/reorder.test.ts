import { describe, expect, it } from "vitest"
import type { EventPayload } from "../../index.js"
import { ReorderController, createReorderDrag, reorderedValues } from "../reorder.js"
import type { ReorderOptions, ReorderSlots } from "../reorder.js"

// Three 20 px slots stacked from 100, with a 4 px gap: centres at 110, 134, 158.
const slots: ReorderSlots<string> = {
  sizes: new Map([
    ["a", 20],
    ["b", 20],
    ["c", 20],
  ]),
  gaps: [4, 4],
  firstStart: 100,
}

describe("reorderedValues", () => {
  it("keeps the order until the centre passes a neighbour's centre", () => {
    expect(reorderedValues(slots, ["a", "b", "c"], "a", 133)).toEqual(["a", "b", "c"])
    expect(reorderedValues(slots, ["a", "b", "c"], "a", 135)).toEqual(["b", "a", "c"])
  })

  it("swaps with every neighbour the centre has passed", () => {
    expect(reorderedValues(slots, ["a", "b", "c"], "a", 200)).toEqual(["b", "c", "a"])
    expect(reorderedValues(slots, ["a", "b", "c"], "c", 0)).toEqual(["c", "a", "b"])
  })

  it("reads the slots of the new order, so unequal sizes do not swap back", () => {
    const uneven: ReorderSlots<string> = {
      sizes: new Map([
        ["a", 40],
        ["b", 10],
      ]),
      gaps: [0],
      firstStart: 0,
    }
    // a's slot centre is 20, b's 45. After the swap b sits at 0..10 (centre 5), a at 10..50 (centre 30).
    expect(reorderedValues(uneven, ["a", "b"], "a", 46)).toEqual(["b", "a"])
    expect(reorderedValues(uneven, ["b", "a"], "a", 46)).toEqual(["b", "a"])
    expect(reorderedValues(uneven, ["b", "a"], "a", 4)).toEqual(["a", "b"])
  })

  it("rejects a value that is not in the order", () => {
    expect(() => reorderedValues(slots, ["a", "b"], "z", 0)).toThrow(/not in values/)
  })
})

function group(order: string[]) {
  const proposals: string[][] = []
  /** Item boxes 20 px tall and 4 px apart from y = 100, in the order the layout currently has; an element id is the item's index in `order`. */
  let layout = order
  const names = [...order, "d"]
  const column = (elementId: number) => ({ x: 0, y: 100 + layout.indexOf(names[elementId]) * 24, width: 50, height: 20 })
  const layOut = (next: string[]) => {
    layout = next
  }
  const items = new Map(order.map((value) => [value, createReorderDrag()]))
  const reorder = new ReorderController<string>()
  order.forEach((value, index) => {
    const drag = items.get(value)
    if (drag === undefined) throw new Error("missing drag")
    reorder.register(value, { elementId: () => index, capturePointer: () => {}, drag })
  })
  const configure = (values: string[], overrides: Partial<ReorderOptions<string>> = {}) =>
    reorder.configure({
      axis: "y",
      values,
      onReorder: (next) => proposals.push(next),
      renderer: { getElementBounds: column },
      ...overrides,
    })
  configure(order)
  return { reorder, items, proposals, configure, layOut }
}

const at = (x: number, y: number, pressedButton?: number): EventPayload => ({ x, y, button: 0, pointerId: 1, pressedButton }) as EventPayload & { pointerId: number }

describe("ReorderController", () => {
  it("holds the item on press, follows the pointer and lets go on release", () => {
    const { reorder, items } = group(["a", "b", "c"])
    const drag = items.get("a")
    reorder.pointerDown("a", at(10, 105))
    expect(drag?.active.get()).toBe(1)
    reorder.pointerMove(at(10, 115, 0))
    expect(drag?.y.get()).toBe(10)
    expect(drag?.x.get()).toBe(0)
    reorder.pointerEnd("a", { pointerId: 1 })
    expect(drag?.active.get()).toBe(0)
    expect(drag?.y.get()).toBe(0)
  })

  it("ignores an end for another item or pointer, such as a late lostpointercapture of an earlier drag", () => {
    const { reorder, items } = group(["a", "b", "c"])
    reorder.pointerDown("a", at(10, 105))
    reorder.pointerEnd("a", { pointerId: 1 })
    reorder.pointerDown("b", at(10, 125))
    reorder.pointerEnd("a", { pointerId: 1 })
    expect(reorder.isDragging("b")).toBe(true)
    expect(items.get("b")?.active.get()).toBe(1)
    reorder.pointerEnd("b", { pointerId: 2 })
    expect(reorder.isDragging("b")).toBe(true)
    reorder.pointerEnd("b", { pointerId: 1 })
    expect(reorder.isDragging("b")).toBe(false)
    expect(items.get("b")?.active.get()).toBe(0)
  })

  it("proposes the new order once the dragged centre passes the next one", () => {
    const { reorder, proposals } = group(["a", "b", "c"])
    // Grab a 5 px below its centre (110): a's centre follows the pointer minus 5.
    reorder.pointerDown("a", at(10, 115))
    reorder.pointerMove(at(10, 138, 0))
    expect(proposals).toEqual([])
    reorder.pointerMove(at(10, 141, 0))
    expect(proposals).toEqual([["b", "a", "c"]])
  })

  it("does not repeat a proposal the parent has not applied yet", () => {
    const { reorder, proposals, configure, layOut } = group(["a", "b", "c"])
    reorder.pointerDown("a", at(10, 110))
    reorder.pointerMove(at(10, 140, 0))
    reorder.pointerMove(at(10, 141, 0))
    expect(proposals).toHaveLength(1)
    // Applied: the new order is the base, and a further swap is judged against it.
    layOut(["b", "a", "c"])
    configure(["b", "a", "c"])
    reorder.pointerMove(at(10, 142, 0))
    expect(proposals).toHaveLength(1)
    reorder.pointerMove(at(10, 170, 0))
    expect(proposals).toEqual([
      ["b", "a", "c"],
      ["b", "c", "a"],
    ])
  })

  it("drags along x for an x axis", () => {
    const proposals: string[][] = []
    const { reorder, items, configure } = group(["a", "b"])
    configure(["a", "b"], {
      axis: "x",
      onReorder: (next) => proposals.push(next),
      renderer: { getElementBounds: (id) => ({ x: id * 60, y: 0, width: 50, height: 20 }) },
    })
    reorder.pointerDown("a", at(25, 10))
    reorder.pointerMove(at(100, 40, 0))
    expect(items.get("a")?.x.get()).toBe(75)
    expect(items.get("a")?.y.get()).toBe(0)
    expect(proposals).toEqual([["b", "a"]])
  })

  it("lets go when a move arrives with no button down", () => {
    const { reorder, items } = group(["a", "b"])
    reorder.pointerDown("a", at(10, 105))
    reorder.pointerMove(at(10, 130))
    expect(items.get("a")?.active.get()).toBe(0)
    expect(reorder.isDragging("a")).toBe(false)
  })

  it("captures the pointer to the item that was pressed", () => {
    const { reorder } = group(["a", "b"])
    const captured: number[] = []
    reorder.register("b", { elementId: () => 1, capturePointer: (pointerId) => captured.push(pointerId), drag: createReorderDrag() })
    reorder.pointerDown("b", at(10, 129))
    expect(captured).toEqual([1])
  })

  it("does not hold the item when the pointer cannot be captured", () => {
    const { reorder } = group(["a", "b"])
    reorder.register("a", {
      elementId: () => 0,
      capturePointer: () => {
        throw new Error("not in the tree")
      },
      drag: createReorderDrag(),
    })
    expect(() => reorder.pointerDown("a", at(10, 105))).toThrow("not in the tree")
    expect(reorder.isDragging("a")).toBe(false)
  })

  it("ignores moves with nothing held and presses of other buttons", () => {
    const { reorder, items, proposals } = group(["a", "b"])
    reorder.pointerMove(at(10, 500, 0))
    reorder.pointerDown("a", { x: 10, y: 105, button: 2, pointerId: 1 } as EventPayload & { pointerId: number })
    expect(items.get("a")?.active.get()).toBe(0)
    expect(proposals).toEqual([])
  })

  it("lets go of an item that unmounts mid-drag", () => {
    const { reorder, items } = group(["a", "b"])
    const drag = items.get("a")
    const stop = reorder.register("a", { elementId: () => 0, capturePointer: () => {}, drag: drag ?? createReorderDrag() })
    reorder.pointerDown("a", at(10, 105))
    stop()
    expect(reorder.isDragging("a")).toBe(false)
  })

  describe("when the items change mid-drag", () => {
    // Each look needs the one before it to agree, so a move is made twice.
    const settle = (reorder: ReorderController<string>, y: number) => {
      reorder.pointerMove(at(10, y, 0))
      reorder.pointerMove(at(10, y, 0))
    }

    it("keeps dragging when an item joins, judging it from its measured slot once it has settled", () => {
      const { reorder, items, proposals, configure, layOut } = group(["a", "b", "c"])
      const added = createReorderDrag()
      reorder.pointerDown("a", at(10, 110))
      reorder.register("d", { elementId: () => 3, capturePointer: () => {}, drag: added })
      layOut(["a", "b", "c", "d"])
      configure(["a", "b", "c", "d"])
      // d is not measured yet: the drag follows the pointer and proposes nothing.
      expect(() => reorder.pointerMove(at(10, 125, 0))).not.toThrow()
      expect(items.get("a")?.active.get()).toBe(1)
      expect(items.get("a")?.y.get()).toBe(15)
      expect(proposals).toEqual([])
      reorder.pointerMove(at(10, 126, 0))
      // d's slot centre is 100 + 3 * 24 + 10 = 182.
      reorder.pointerMove(at(10, 190, 0))
      expect(proposals).toEqual([["b", "c", "d", "a"]])
    })

    it("keeps dragging when an item leaves, and measures the slots that are left", () => {
      const { reorder, items, proposals, configure, layOut } = group(["a", "b", "c"])
      reorder.pointerDown("a", at(10, 110))
      layOut(["a", "c"])
      configure(["a", "c"])
      settle(reorder, 120)
      expect(items.get("a")?.active.get()).toBe(1)
      // c now sits in b's slot, centred at 134.
      reorder.pointerMove(at(10, 140, 0))
      expect(proposals).toEqual([["c", "a"]])
    })

    it("lets go without proposing when the dragged item is removed", () => {
      const { reorder, proposals, configure } = group(["a", "b", "c"])
      reorder.pointerDown("a", at(10, 110))
      configure(["b", "c"])
      expect(() => reorder.pointerMove(at(10, 300, 0))).not.toThrow()
      expect(reorder.isDragging("a")).toBe(false)
      expect(proposals).toEqual([])
    })

    it("lets go without proposing when the dragged item unmounts", () => {
      const { reorder, items, proposals, configure } = group(["a", "b", "c"])
      const stop = reorder.register("a", { elementId: () => 0, capturePointer: () => {}, drag: items.get("a") ?? createReorderDrag() })
      reorder.pointerDown("a", at(10, 110))
      stop()
      configure(["b", "c"])
      reorder.pointerMove(at(10, 300, 0))
      expect(reorder.isDragging("a")).toBe(false)
      expect(proposals).toEqual([])
    })

    it("does not judge from glide positions after a reorder", () => {
      const { reorder, proposals, configure } = group(["a", "b", "c"])
      reorder.pointerDown("a", at(10, 110))
      reorder.pointerMove(at(10, 141, 0))
      expect(proposals).toEqual([["b", "a", "c"]])
      // b and c are still gliding up into their slots, b faster than c: no two looks agree.
      let gliding = 124
      configure(["b", "a", "c"], {
        renderer: {
          getElementBounds: (id) => ({ x: 0, y: id === 1 ? (gliding -= 2) : id === 2 ? 130 : 100, width: 50, height: 20 }),
        },
      })
      for (const y of [142, 143, 150]) reorder.pointerMove(at(10, y, 0))
      // The slots are still the old ones stacked in the new order: c's centre is 158, not its drawn 140.
      expect(proposals).toHaveLength(1)
      reorder.pointerMove(at(10, 159, 0))
      expect(proposals).toEqual([
        ["b", "a", "c"],
        ["b", "c", "a"],
      ])
    })

    it("measures the slots again once a reorder has settled", () => {
      const { reorder, items, proposals, configure } = group(["a", "b", "c"])
      reorder.pointerDown("a", at(10, 110))
      reorder.pointerMove(at(10, 141, 0))
      // Settled in b, a, c order, but 30 px tall: centres at 115, 149 and 183.
      const taller = [1, 0, 2]
      configure(["b", "a", "c"], {
        renderer: { getElementBounds: (id) => ({ x: 0, y: 100 + taller.indexOf(id) * 34, width: 50, height: 30 }) },
      })
      settle(reorder, 142)
      reorder.pointerMove(at(10, 170, 0))
      expect(items.get("a")?.active.get()).toBe(1)
      expect(proposals).toEqual([["b", "a", "c"]])
      reorder.pointerMove(at(10, 184, 0))
      expect(proposals.at(-1)).toEqual(["b", "c", "a"])
    })
  })

  it("says what is missing instead of guessing", () => {
    const { reorder, configure } = group(["a", "b"])
    configure(["a", "b"], { renderer: { getElementBounds: () => null } })
    expect(() => reorder.pointerDown("a", at(10, 105))).toThrow(/not been laid out/)
    configure(["a", "b"], { renderer: null })
    expect(() => reorder.pointerDown("a", at(10, 105))).toThrow(/GPUIX root/)
    configure(["a", "z"])
    expect(() => reorder.pointerDown("a", at(10, 105))).toThrow(/no item is registered/)
  })
})
