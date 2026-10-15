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

/** Items 20 px tall, 4 px apart, from y = 100. */
function column(elementId: number) {
  return { x: 0, y: 100 + elementId * 24, width: 50, height: 20 }
}

function group(order: string[]) {
  const proposals: string[][] = []
  const items = new Map(order.map((value) => [value, createReorderDrag()]))
  const reorder = new ReorderController<string>()
  order.forEach((value, index) => {
    const drag = items.get(value)
    if (drag === undefined) throw new Error("missing drag")
    reorder.register(value, { elementId: () => index, drag })
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
  return { reorder, items, proposals, configure }
}

const at = (x: number, y: number, pressedButton?: number): EventPayload => ({ x, y, button: 0, pressedButton }) as EventPayload

describe("ReorderController", () => {
  it("holds the item on press, follows the pointer and lets go on release", () => {
    const { reorder, items } = group(["a", "b", "c"])
    const drag = items.get("a")
    reorder.pointerDown("a", at(10, 105))
    expect(drag?.active.get()).toBe(1)
    reorder.pointerMove(at(10, 115, 0))
    expect(drag?.y.get()).toBe(10)
    expect(drag?.x.get()).toBe(0)
    reorder.pointerUp()
    expect(drag?.active.get()).toBe(0)
    expect(drag?.y.get()).toBe(0)
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
    const { reorder, proposals, configure } = group(["a", "b", "c"])
    reorder.pointerDown("a", at(10, 110))
    reorder.pointerMove(at(10, 140, 0))
    reorder.pointerMove(at(10, 141, 0))
    expect(proposals).toHaveLength(1)
    // Applied: the new order is the base, and a further swap is judged against it.
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

  it("ignores moves with nothing held and presses of other buttons", () => {
    const { reorder, items, proposals } = group(["a", "b"])
    reorder.pointerMove(at(10, 500, 0))
    reorder.pointerDown("a", { x: 10, y: 105, button: 2 } as EventPayload)
    expect(items.get("a")?.active.get()).toBe(0)
    expect(proposals).toEqual([])
  })

  it("lets go of an item that unmounts mid-drag", () => {
    const { reorder, items } = group(["a", "b"])
    const drag = items.get("a")
    const stop = reorder.register("a", { elementId: () => 0, drag: drag ?? createReorderDrag() })
    reorder.pointerDown("a", at(10, 105))
    stop()
    expect(reorder.isDragging("a")).toBe(false)
  })

  it("lets go and throws when an item joins the group mid-drag", () => {
    const { reorder, items, configure } = group(["a", "b"])
    reorder.pointerDown("a", at(10, 105))
    configure(["a", "b", "c"])
    expect(() => reorder.pointerMove(at(10, 120, 0))).toThrow(/joined the group/)
    expect(items.get("a")?.active.get()).toBe(0)
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
