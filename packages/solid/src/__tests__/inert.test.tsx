import { createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const box = { width: 40, height: 20 }

function setup(view: () => any) {
  const app = createTestRoot({ width: 300, height: 200 })
  app.render(view)
  const renderer = app.renderer
  const focused = () => renderer.getElement(renderer.getFocusedElementId()!)?.testId ?? null
  const idOf = (testId: string) => renderer.findByTestId(testId)!.id
  return { app, renderer, focused, idOf }
}

describe.skipIf(!hasNativeTestRenderer)("Solid inert", () => {
  it("skips the subtree when tabbing and ignores focus()", () => {
    const { renderer, focused, idOf } = setup(() => (
      <div style={{ width: 200, height: 150 }}>
        <div testId="before" tabIndex={0} style={box} />
        <div inert style={{ width: 100, height: 60 }}>
          <div testId="inside" tabIndex={0} style={box} />
          <input testId="field" style={box} />
        </div>
        <div testId="after" tabIndex={0} style={box} />
      </div>
    ))
    renderer.simulateKeystrokes("Tab")
    expect(focused()).toBe("before")
    renderer.simulateKeystrokes("Tab")
    expect(focused()).toBe("after")

    renderer.focusElement(idOf("inside"))
    renderer.flush()
    expect(focused()).toBe("after")
    renderer.focusElement(idOf("field"))
    renderer.flush()
    expect(focused()).toBe("after")
  })

  it("blurs the focused element when an ancestor becomes inert, and restores focusability", () => {
    const log: string[] = []
    const [inert, setInert] = createSignal(false)
    const { app, renderer, focused, idOf } = setup(() => (
      <div style={{ width: 200, height: 100 }}>
        <div inert={inert()} style={{ width: 100, height: 60 }}>
          <div testId="inside" tabIndex={0} style={box} onBlur={() => log.push("blur")} />
        </div>
      </div>
    ))
    renderer.focusElement(idOf("inside"))
    expect(focused()).toBe("inside")

    app.flushSync(() => setInert(true))
    renderer.dispatchNativeEvents()
    expect(focused()).toBe(null)
    expect(log).toEqual(["blur"])

    app.flushSync(() => setInert(false))
    renderer.dispatchNativeEvents()
    renderer.focusElement(idOf("inside"))
    expect(focused()).toBe("inside")
  })

  it("hides the subtree from the accessibility tree", () => {
    const { renderer } = setup(() => (
      <div inert style={{ width: 120, height: 40 }}>
        <div role="button" aria-label="inside" style={box} />
      </div>
    ))
    const tree = renderer.getA11yTree()
    const nodes = Object.values(tree.nodes ?? {})
    const parent = nodes.find((node) =>
      node.children?.some((child) => tree.nodes?.[child]?.aria?.label === "inside")
    )
    expect(parent?.aria?.hidden).toBe(true)
    const inside = nodes.find((node) => node.aria?.label === "inside")
    expect(inside?.aria?.hidden).toBe(true)
  })

  it("does not deliver clicks to the subtree, and lets them reach what is behind it", () => {
    const log: string[] = []
    const { renderer } = setup(() => (
      <div style={{ width: 200, height: 100, position: "relative" }}>
        <div
          onClick={() => log.push("behind")}
          style={{ width: 100, height: 60, backgroundColor: "#123456" }}
        />
        <div
          inert
          onClick={() => log.push("inert")}
          style={{ position: "absolute", left: 0, top: 0, width: 100, height: 60, backgroundColor: "#654321" }}
        >
          <div
            testId="inner"
            onClick={() => log.push("inner")}
            style={{ width: 100, height: 60, pointerEvents: "auto" }}
          />
        </div>
      </div>
    ))
    renderer.nativeSimulateClick(50, 30)
    expect(log).toEqual(["behind"])
  })

  it("makes the rows of a virtual list inert when an ancestor becomes inert after they were built, and restores them", () => {
    const log: string[] = []
    const [inert, setInert] = createSignal(false)
    const { app, renderer, focused, idOf } = setup(() => (
      <div inert={inert()} style={{ width: 200, height: 100 }}>
        <virtual-list overdraw={0} estimatedItemHeight={20} style={{ width: 200, height: 100 }}>
          {Array.from({ length: 20 }, (_, index) => (
            <div
              testId={`row-${index}`}
              tabIndex={0}
              role="button"
              aria-label={`row ${index}`}
              onClick={() => log.push(`click ${index}`)}
              style={{ width: 200, height: 20, flexShrink: 0 }}
            />
          ))}
        </virtual-list>
      </div>
    ))
    const row = idOf("row-1")
    // Focusing a row scrolls it to the top of the list, so the point is read again each time.
    const at = (): [number, number] => {
      const bounds = renderer.getElementBounds(row)!
      return [bounds.x + bounds.width / 2, bounds.y + bounds.height / 2]
    }
    const hidden = (label: string) => {
      const nodes = Object.values(renderer.getA11yTree().nodes ?? {}) as any[]
      const node = nodes.find((node) => node.aria?.label === label)
      if (node === undefined) throw new Error(`no accessibility node labelled ${label}`)
      return node.aria?.hidden === true
    }

    expect(renderer.getElementsAt(...at())).toContain(row)
    expect(hidden("row 1")).toBe(false)
    renderer.nativeSimulateClick(...at())
    expect(log).toEqual(["click 1"])
    renderer.focusElement(row)
    expect(focused()).toBe("row-1")

    app.flushSync(() => setInert(true))
    renderer.flush()
    expect(renderer.getElementsAt(...at())).not.toContain(row)
    expect(hidden("row 1")).toBe(true)
    expect(focused()).toBe(null)
    renderer.focusElement(row)
    renderer.flush()
    expect(focused()).toBe(null)
    renderer.nativeSimulateClick(...at())
    expect(log).toEqual(["click 1"])

    app.flushSync(() => setInert(false))
    renderer.flush()
    expect(renderer.getElementsAt(...at())).toContain(row)
    expect(hidden("row 1")).toBe(false)
    renderer.nativeSimulateClick(...at())
    expect(log).toEqual(["click 1", "click 1"])
    renderer.focusElement(row)
    expect(focused()).toBe("row-1")
  })
})
