import { useState } from "react"
import { describe, expect, it } from "vitest"
import { Reorder } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { TestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const linear = { duration: 1, ease: "linear" } as const

function frame(renderer: TestRenderer, ms: number): void {
  renderer.clockFastForward(ms)
  renderer.flush()
  renderer.dispatchNativeEvents()
}

function boundsOf(renderer: TestRenderer, testId: string) {
  return renderer.getElementBounds(renderer.findByTestId(testId).id)
}

type ListProps = {
  initial: string[]
  axis?: "x" | "y"
  proposals?: string[][]
  accept?: boolean
  onItemMouseDown?: () => void
}

function List(props: ListProps) {
  const [order, setOrder] = useState(props.initial)
  const axis = props.axis ?? "y"
  return (
    <Reorder.Group
      axis={axis}
      values={order}
      onReorder={(next) => {
        props.proposals?.push(next)
        if (props.accept !== false) setOrder(next)
      }}
      style={{ display: "flex", flexDirection: axis === "y" ? "column" : "row" }}
    >
      {order.map((name) => (
        <Reorder.Item
          key={name}
          value={name}
          testId={name}
          transition={linear}
          onMouseDown={props.onItemMouseDown}
          style={{ height: 20, width: 50 }}
        />
      ))}
    </Reorder.Group>
  )
}

function mount(props: ListProps) {
  const { render, renderer } = createTestRoot()
  renderer.clockPause()
  render(<List {...props} />)
  return renderer
}

describeNative("Reorder", () => {
  it("lays the items out like a plain list", () => {
    const renderer = mount({ initial: ["a", "b", "c"] })
    expect(boundsOf(renderer, "a")?.y).toBe(0)
    expect(boundsOf(renderer, "b")?.y).toBe(20)
    expect(boundsOf(renderer, "c")?.y).toBe(40)
  })

  it("moves the grabbed item with the pointer and leaves the others where they are", () => {
    const proposals: string[][] = []
    const renderer = mount({ initial: ["a", "b", "c"], proposals })
    renderer.nativeSimulateMouseDown(10, 10)
    renderer.nativeSimulateMouseMove(10, 18, 0)
    expect(boundsOf(renderer, "a")?.y).toBe(8)
    expect(boundsOf(renderer, "b")?.y).toBe(20)
    expect(proposals).toEqual([])
  })

  it("swaps when the dragged centre passes a neighbour's, and the neighbour glides", () => {
    const proposals: string[][] = []
    const renderer = mount({ initial: ["a", "b", "c"], proposals })
    renderer.nativeSimulateMouseDown(10, 10)
    renderer.nativeSimulateMouseMove(10, 29, 0)
    expect(proposals).toEqual([])
    renderer.nativeSimulateMouseMove(10, 31, 0)
    expect(proposals).toEqual([["b", "a", "c"]])

    // The dragged item stays under the pointer while its slot moves.
    expect(boundsOf(renderer, "a")?.y).toBe(21)
    // b starts from where it was and glides up into a's old slot.
    expect(boundsOf(renderer, "b")?.y).toBeCloseTo(20, 1)
    frame(renderer, 500)
    expect(boundsOf(renderer, "b")?.y).toBeCloseTo(10, 1)
    frame(renderer, 600)
    expect(boundsOf(renderer, "b")?.y).toBe(0)
    expect(boundsOf(renderer, "a")?.y).toBe(21)
  })

  it("lets the item glide to its slot on release", () => {
    const renderer = mount({ initial: ["a", "b", "c"] })
    renderer.nativeSimulateMouseDown(10, 10)
    renderer.nativeSimulateMouseMove(10, 38, 0)
    expect(boundsOf(renderer, "a")?.y).toBe(28)
    renderer.nativeSimulateMouseUp(10, 38, 0)
    expect(boundsOf(renderer, "a")?.y).toBeCloseTo(28, 1)
    frame(renderer, 500)
    expect(boundsOf(renderer, "a")?.y).toBeCloseTo(24, 1)
    frame(renderer, 600)
    expect(boundsOf(renderer, "a")?.y).toBe(20)
  })

  it("carries an item across several neighbours in one drag", () => {
    const proposals: string[][] = []
    const renderer = mount({ initial: ["a", "b", "c"], proposals })
    renderer.nativeSimulateMouseDown(10, 10)
    renderer.nativeSimulateMouseMove(10, 55, 0)
    expect(proposals).toEqual([["b", "c", "a"]])
    renderer.nativeSimulateMouseMove(10, 25, 0)
    expect(proposals.at(-1)).toEqual(["b", "a", "c"])
  })

  it("only proposes: an order the parent does not apply stays put", () => {
    const proposals: string[][] = []
    const renderer = mount({ initial: ["a", "b", "c"], proposals, accept: false })
    renderer.nativeSimulateMouseDown(10, 10)
    renderer.nativeSimulateMouseMove(10, 35, 0)
    renderer.nativeSimulateMouseMove(10, 36, 0)
    expect(proposals).toEqual([["b", "a", "c"]])
    expect(boundsOf(renderer, "b")?.y).toBe(20)
    expect(boundsOf(renderer, "a")?.y).toBe(26)
  })

  it("drags along x for axis x and ignores the other axis", () => {
    const proposals: string[][] = []
    const renderer = mount({ initial: ["a", "b"], axis: "x", proposals })
    expect(boundsOf(renderer, "b")?.x).toBe(50)
    renderer.nativeSimulateMouseDown(10, 10)
    renderer.nativeSimulateMouseMove(70, 40, 0)
    expect(proposals).toEqual([["b", "a"]])
    expect(boundsOf(renderer, "a")?.x).toBe(60)
    expect(boundsOf(renderer, "a")?.y).toBe(0)
  })

  it("keeps the drag after the pointer leaves the item and ends it on release outside", () => {
    const renderer = mount({ initial: ["a", "b", "c"] })
    renderer.nativeSimulateMouseDown(10, 10)
    renderer.nativeSimulateMouseMove(300, 18, 0)
    expect(boundsOf(renderer, "a")?.y).toBe(8)
    renderer.nativeSimulateMouseUp(300, 18, 0)
    frame(renderer, 1100)
    expect(boundsOf(renderer, "a")?.y).toBe(0)
  })

  it("still calls the item's own mouse handlers", () => {
    let presses = 0
    const renderer = mount({ initial: ["a", "b"], onItemMouseDown: () => (presses += 1) })
    renderer.nativeSimulateMouseDown(10, 10)
    expect(presses).toBe(1)
  })
})
