import { For, createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import { Reorder } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { TestRenderer } from "../testing.js"

const linear = { duration: 1, ease: "linear" } as const

function frame(renderer: TestRenderer, ms: number): void {
  renderer.clockFastForward(ms)
  renderer.flush()
  renderer.dispatchNativeEvents()
}

function boundsOf(renderer: TestRenderer, testId: string) {
  return renderer.getElementBounds(renderer.findByTestId(testId)!.id)!
}

type ListProps = {
  initial: string[]
  axis?: "x" | "y"
  proposals?: string[][]
  accept?: boolean
  onItemMouseDown?: () => void
  /** Filled with a setter for the order, to change the items from outside. */
  setItems?: { current: ((names: string[]) => void) | undefined }
}

function List(props: ListProps) {
  const [order, setOrder] = createSignal(props.initial)
  if (props.setItems) props.setItems.current = setOrder
  const axis = () => props.axis ?? "y"
  return (
    <Reorder.Group
      axis={axis()}
      values={order()}
      onReorder={(next) => {
        props.proposals?.push(next)
        if (props.accept !== false) setOrder(next)
      }}
      style={{ display: "flex", flexDirection: axis() === "y" ? "column" : "row" }}
    >
      <For each={order()}>
        {(name) => (
          <Reorder.Item
            value={name}
            testId={name}
            transition={linear}
            onMouseDown={props.onItemMouseDown}
            style={{ height: 20, width: 50 }}
          />
        )}
      </For>
    </Reorder.Group>
  )
}

function mountApp(props: ListProps) {
  const app = createTestRoot()
  app.renderer.clockPause()
  app.render(() => <List {...props} />)
  return app
}

function mount(props: ListProps) {
  return mountApp(props).renderer
}

describe.skipIf(!hasNativeTestRenderer)("Solid Reorder", () => {
  it("lays the items out like a plain list", () => {
    const renderer = mount({ initial: ["a", "b", "c"] })
    expect(boundsOf(renderer, "a").y).toBe(0)
    expect(boundsOf(renderer, "b").y).toBe(20)
    expect(boundsOf(renderer, "c").y).toBe(40)
  })

  it("moves the grabbed item with the pointer and leaves the others where they are", () => {
    const proposals: string[][] = []
    const renderer = mount({ initial: ["a", "b", "c"], proposals })
    renderer.nativeSimulateMouseDown(10, 10)
    renderer.nativeSimulateMouseMove(10, 18)
    expect(boundsOf(renderer, "a").y).toBe(8)
    expect(boundsOf(renderer, "b").y).toBe(20)
    expect(proposals).toEqual([])
  })

  it("swaps when the dragged centre passes a neighbour's, and the neighbour glides", () => {
    const proposals: string[][] = []
    const renderer = mount({ initial: ["a", "b", "c"], proposals })
    renderer.nativeSimulateMouseDown(10, 10)
    renderer.nativeSimulateMouseMove(10, 29)
    expect(proposals).toEqual([])
    renderer.nativeSimulateMouseMove(10, 31)
    expect(proposals).toEqual([["b", "a", "c"]])

    expect(boundsOf(renderer, "a").y).toBe(21)
    expect(boundsOf(renderer, "b").y).toBeCloseTo(20, 1)
    frame(renderer, 500)
    expect(boundsOf(renderer, "b").y).toBeCloseTo(10, 1)
    frame(renderer, 600)
    expect(boundsOf(renderer, "b").y).toBe(0)
    expect(boundsOf(renderer, "a").y).toBe(21)
  })

  it("lets the item glide to its slot on release", () => {
    const renderer = mount({ initial: ["a", "b", "c"] })
    renderer.nativeSimulateMouseDown(10, 10)
    renderer.nativeSimulateMouseMove(10, 38)
    expect(boundsOf(renderer, "a").y).toBe(28)
    renderer.nativeSimulateMouseUp(10, 38, 0)
    expect(boundsOf(renderer, "a").y).toBeCloseTo(28, 1)
    frame(renderer, 500)
    expect(boundsOf(renderer, "a").y).toBeCloseTo(24, 1)
    frame(renderer, 600)
    expect(boundsOf(renderer, "a").y).toBe(20)
  })

  it("carries an item across several neighbours in one drag", () => {
    const proposals: string[][] = []
    const renderer = mount({ initial: ["a", "b", "c"], proposals })
    renderer.nativeSimulateMouseDown(10, 10)
    renderer.nativeSimulateMouseMove(10, 55)
    expect(proposals).toEqual([["b", "c", "a"]])
    renderer.nativeSimulateMouseMove(10, 25)
    expect(proposals.at(-1)).toEqual(["b", "a", "c"])
  })

  it("only proposes: an order the parent does not apply stays put", () => {
    const proposals: string[][] = []
    const renderer = mount({ initial: ["a", "b", "c"], proposals, accept: false })
    renderer.nativeSimulateMouseDown(10, 10)
    renderer.nativeSimulateMouseMove(10, 35)
    renderer.nativeSimulateMouseMove(10, 36)
    expect(proposals).toEqual([["b", "a", "c"]])
    expect(boundsOf(renderer, "b").y).toBe(20)
    expect(boundsOf(renderer, "a").y).toBe(26)
  })

  it("drags along x for axis x and ignores the other axis", () => {
    const proposals: string[][] = []
    const renderer = mount({ initial: ["a", "b"], axis: "x", proposals })
    expect(boundsOf(renderer, "b").x).toBe(50)
    renderer.nativeSimulateMouseDown(10, 10)
    renderer.nativeSimulateMouseMove(70, 40)
    expect(proposals).toEqual([["b", "a"]])
    expect(boundsOf(renderer, "a").x).toBe(60)
    expect(boundsOf(renderer, "a").y).toBe(0)
  })

  it("keeps the drag after the pointer leaves the item and ends it on release outside", () => {
    const renderer = mount({ initial: ["a", "b", "c"] })
    renderer.nativeSimulateMouseDown(10, 10)
    renderer.nativeSimulateMouseMove(300, 18)
    expect(boundsOf(renderer, "a").y).toBe(8)
    renderer.nativeSimulateMouseUp(300, 18, 0)
    frame(renderer, 1100)
    expect(boundsOf(renderer, "a").y).toBe(0)
  })

  it("keeps a drag on b going when a lostpointercapture for a's finished drag arrives late", () => {
    const app = mountApp({ initial: ["a", "b", "c"] })
    const renderer = app.renderer
    renderer.nativeSimulateMouseDown(10, 10)
    renderer.nativeSimulateMouseUp(10, 10, 0)
    renderer.nativeSimulateMouseDown(10, 30)
    renderer.nativeSimulateMouseMove(10, 33)
    expect(boundsOf(renderer, "b").y).toBe(23)

    app.root.dispatch({ elementId: renderer.findByTestId("a")!.id, eventType: "lostpointercapture", pointerId: 1 })
    renderer.nativeSimulateMouseMove(10, 35)
    expect(boundsOf(renderer, "b").y).toBe(25)

    renderer.nativeSimulateMouseUp(10, 35, 0)
    frame(renderer, 1100)
    expect(boundsOf(renderer, "b").y).toBe(20)
  })

  it("keeps a second drag of the same item going: the first drag's lostpointercapture comes before its pointerdown", () => {
    const renderer = mount({ initial: ["a", "b", "c"] })
    for (const press of [10, 12]) {
      renderer.nativeSimulateMouseDown(10, press)
      renderer.nativeSimulateMouseMove(10, press + 4)
      expect(boundsOf(renderer, "a").y).toBe(4)
      renderer.nativeSimulateMouseMove(10, press + 6)
      expect(boundsOf(renderer, "a").y).toBe(6)
      renderer.nativeSimulateMouseUp(10, press + 6, 0)
      frame(renderer, 1100)
      expect(boundsOf(renderer, "a").y).toBe(0)
    }
  })

  it("still calls the item's own mouse handlers", () => {
    let presses = 0
    const renderer = mount({ initial: ["a", "b"], onItemMouseDown: () => (presses += 1) })
    renderer.nativeSimulateMouseDown(10, 10)
    expect(presses).toBe(1)
  })

  describe("when the items change mid-drag", () => {
    function setItemsTo(app: ReturnType<typeof mountApp>, setItems: ListProps["setItems"], names: string[]): void {
      app.flushSync(() => setItems?.current?.(names))
      frame(app.renderer, 1100)
    }

    it("keeps dragging when an item is added, and swaps with it once it is measured", () => {
      const proposals: string[][] = []
      const setItems: ListProps["setItems"] = { current: undefined }
      const app = mountApp({ initial: ["a", "b", "c"], proposals, setItems })
      const renderer = app.renderer
      renderer.nativeSimulateMouseDown(10, 10)
      renderer.nativeSimulateMouseMove(10, 18)
      setItemsTo(app, setItems, ["a", "b", "c", "d"])
      renderer.nativeSimulateMouseMove(10, 20)
      renderer.nativeSimulateMouseMove(10, 20)
      expect(boundsOf(renderer, "a").y).toBe(10)
      expect(proposals).toEqual([])
      renderer.nativeSimulateMouseMove(10, 75)
      expect(proposals).toEqual([["b", "c", "d", "a"]])
    })

    it("keeps dragging when an item is removed", () => {
      const proposals: string[][] = []
      const setItems: ListProps["setItems"] = { current: undefined }
      const app = mountApp({ initial: ["a", "b", "c"], proposals, setItems })
      const renderer = app.renderer
      renderer.nativeSimulateMouseDown(10, 10)
      renderer.nativeSimulateMouseMove(10, 18)
      setItemsTo(app, setItems, ["a", "c"])
      renderer.nativeSimulateMouseMove(10, 20)
      renderer.nativeSimulateMouseMove(10, 20)
      renderer.nativeSimulateMouseMove(10, 35)
      expect(proposals).toEqual([["c", "a"]])
    })

    it("ends the drag without a proposal when the dragged item is removed", () => {
      const proposals: string[][] = []
      const setItems: ListProps["setItems"] = { current: undefined }
      const app = mountApp({ initial: ["a", "b", "c"], proposals, setItems })
      const renderer = app.renderer
      renderer.nativeSimulateMouseDown(10, 10)
      renderer.nativeSimulateMouseMove(10, 18)
      setItemsTo(app, setItems, ["b", "c"])
      renderer.nativeSimulateMouseMove(10, 60)
      renderer.nativeSimulateMouseUp(10, 60, 0)
      expect(proposals).toEqual([])
      expect(boundsOf(renderer, "b").y).toBe(0)
    })
  })
})
