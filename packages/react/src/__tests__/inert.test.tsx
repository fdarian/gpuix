// @ts-nocheck
/** `inert`: no focus, no accessibility nodes, no selection, no pointer input. */

import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const box = { width: 40, height: 20 }

function setup(element: React.ReactElement) {
  const root = createTestRoot({ width: 300, height: 200 })
  root.render(element)
  const { renderer } = root
  const focused = () => renderer.getElement(renderer.getFocusedElementId())?.testId ?? null
  const idOf = (testId: string) => renderer.findByTestId(testId).id
  const center = (testId: string) => {
    const bounds = renderer.getElementBounds(idOf(testId))
    return [bounds.x + bounds.width / 2, bounds.y + bounds.height / 2]
  }
  return { root, renderer, focused, idOf, center }
}

function Panel({ inert }: { inert: boolean }) {
  return (
    <div style={{ width: 200, height: 150 }}>
      <div testId="before" tabIndex={0} style={box} />
      <div testId="panel" inert={inert} style={{ width: 100, height: 60 }}>
        <div testId="inside" tabIndex={0} style={box} />
        <input testId="field" style={box} />
      </div>
      <div testId="after" tabIndex={0} style={box} />
    </div>
  )
}

describeNative("inert", () => {
  it("skips the subtree when tabbing", () => {
    const { renderer, focused } = setup(<Panel inert />)
    renderer.simulateKeystrokes("Tab")
    expect(focused()).toBe("before")
    renderer.simulateKeystrokes("Tab")
    expect(focused()).toBe("after")
    renderer.simulateKeystrokes("Shift+Tab")
    expect(focused()).toBe("before")
  })

  it("makes focus() a no-op for an element inside, input included", () => {
    const { renderer, focused, idOf } = setup(<Panel inert />)
    renderer.focusElement(idOf("inside"))
    renderer.flush()
    expect(focused()).toBe(null)
    renderer.focusElement(idOf("field"))
    renderer.flush()
    expect(focused()).toBe(null)
  })

  it("is focusable again once inert is removed", () => {
    const { root, renderer, focused, idOf } = setup(<Panel inert />)
    root.render(<Panel inert={false} />)
    renderer.flush()
    renderer.focusElement(idOf("inside"))
    expect(focused()).toBe("inside")
  })

  it("blurs the focused element when an ancestor becomes inert", () => {
    const log: string[] = []
    const { root, renderer, focused, idOf } = setup(
      <div style={{ width: 200, height: 100 }}>
        <div testId="panel" style={{ width: 100, height: 60 }}>
          <div testId="inside" tabIndex={0} style={box} onBlur={() => log.push("blur")} />
        </div>
      </div>
    )
    renderer.focusElement(idOf("inside"))
    expect(focused()).toBe("inside")

    root.render(
      <div style={{ width: 200, height: 100 }}>
        <div testId="panel" inert style={{ width: 100, height: 60 }}>
          <div testId="inside" tabIndex={0} style={box} onBlur={() => log.push("blur")} />
        </div>
      </div>
    )
    renderer.dispatchNativeEvents()
    expect(focused()).toBe(null)
    expect(log).toEqual(["blur"])
  })

  it("keeps a descendant out of focus despite its own tabIndex", () => {
    const { renderer, focused } = setup(
      <div style={{ width: 200, height: 100 }}>
        <div inert style={{ width: 100, height: 60 }}>
          <div testId="deep" tabIndex={0} style={box} />
        </div>
      </div>
    )
    renderer.simulateKeystrokes("Tab")
    expect(focused()).toBe(null)
  })

  it("hides the subtree from the accessibility tree", () => {
    const { renderer } = setup(
      <div inert style={{ width: 120, height: 40 }}>
        <div role="button" aria-label="inside" style={box} />
      </div>
    )
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
    function Page() {
      return (
        <div style={{ width: 200, height: 100, position: "relative" }}>
          <div testId="behind" onClick={() => log.push("behind")} style={{ width: 100, height: 60, backgroundColor: "#123456" }} />
          <div
            inert
            style={{ position: "absolute", left: 0, top: 0, width: 100, height: 60, backgroundColor: "#654321" }}
            onClick={() => log.push("inert")}
          >
            <div testId="inner" onClick={() => log.push("inner")} style={{ width: 100, height: 60, pointerEvents: "auto" }} />
          </div>
        </div>
      )
    }
    const { renderer } = setup(<Page />)
    renderer.nativeSimulateClick(50, 30)
    expect(log).toEqual(["behind"])
  })

  it("is not an elementsFromPoint hit", () => {
    const { renderer, idOf } = setup(
      <div style={{ width: 200, height: 100 }}>
        <div inert style={{ width: 100, height: 60 }}>
          <div testId="inner" style={{ width: 100, height: 60, pointerEvents: "auto" }} />
        </div>
      </div>
    )
    expect(renderer.getElementsAt(10, 10)).not.toContain(idOf("inner"))
  })

  it("does not run hover handlers", () => {
    const log: string[] = []
    const hoverable = (inert: boolean) => (
      <div style={{ width: 200, height: 100 }}>
        <div inert={inert} onMouseEnter={() => log.push("enter")} style={{ width: 100, height: 60 }} />
      </div>
    )
    const { root, renderer } = setup(hoverable(true))
    renderer.nativeSimulateMouseMove(50, 30)
    expect(log).toEqual([])

    root.render(hoverable(false))
    renderer.nativeSimulateMouseMove(150, 90)
    renderer.nativeSimulateMouseMove(50, 30)
    expect(log).toContain("enter")
  })
  it("lets clicks, presses and hover reach content underneath an inert overlay", () => {
    const log: string[] = []
    const { renderer } = setup(
      <div style={{ width: 200, height: 100, position: "relative" }}>
        <div
          testId="content"
          onMouseEnter={() => log.push("enter")}
          onMouseDown={() => log.push("down")}
          onClick={() => log.push("click")}
          style={{ width: 100, height: 60 }}
        />
        <div
          inert
          testId="overlay"
          onMouseEnter={() => log.push("overlay-enter")}
          onMouseDown={() => log.push("overlay-down")}
          onClick={() => log.push("overlay-click")}
          style={{ position: "absolute", left: 0, top: 0, width: 100, height: 60 }}
        />
      </div>
    )
    renderer.nativeSimulateClick(50, 30)
    expect(log).toEqual(["enter", "down", "click"])
  })

  it("keeps pointerEvents auto inside an inert subtree non-interactive", () => {
    const log: string[] = []
    const { renderer, idOf } = setup(
      <div style={{ width: 200, height: 100 }}>
        <div inert style={{ width: 100, height: 60 }}>
          <div style={{ width: 100, height: 60, pointerEvents: "none" }}>
            <div
              testId="auto"
              onMouseEnter={() => log.push("enter")}
              onMouseDown={() => log.push("down")}
              onClick={() => log.push("click")}
              style={{ width: 100, height: 60, pointerEvents: "auto" }}
            />
          </div>
        </div>
      </div>
    )
    renderer.nativeSimulateClick(50, 30)
    expect(log).toEqual([])
    expect(renderer.getElementsAt(50, 30)).not.toContain(idOf("auto"))
  })

  describe("a virtual list under an ancestor that becomes inert after its rows were built", () => {
    function List({ inert, log }: { inert: boolean; log: string[] }) {
      return (
        <div testId="shell" inert={inert} style={{ width: 200, height: 100 }}>
          <virtual-list overdraw={0} estimatedItemHeight={20} style={{ width: 200, height: 100 }}>
            {Array.from({ length: 20 }, (_, index) => (
              <div
                key={index}
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
      )
    }

    const hidden = (renderer, label: string) => {
      const nodes = Object.values(renderer.getA11yTree().nodes ?? {})
      const node = nodes.find((node) => node.aria?.label === label)
      if (node === undefined) throw new Error(`no accessibility node labelled ${label}`)
      return node.aria?.hidden === true
    }

    it("takes the rows out of hit testing, focus and the accessibility tree, and puts them back", () => {
      const log: string[] = []
      const { root, renderer, focused, idOf, center } = setup(<List inert={false} log={log} />)
      const row = idOf("row-1")
      // Focusing a row scrolls it to the top of the list, so the point is read again each time.
      const at = () => center("row-1")
      expect(renderer.getElementsAt(...at())).toContain(row)
      expect(hidden(renderer, "row 1")).toBe(false)
      renderer.nativeSimulateClick(...at())
      expect(log).toEqual(["click 1"])
      renderer.focusElement(row)
      expect(focused()).toBe("row-1")

      root.render(<List inert log={log} />)
      renderer.flush()
      expect(renderer.getElementsAt(...at())).not.toContain(row)
      expect(hidden(renderer, "row 1")).toBe(true)
      expect(focused()).toBe(null)
      renderer.focusElement(row)
      renderer.flush()
      expect(focused()).toBe(null)
      renderer.nativeSimulateClick(...at())
      expect(log).toEqual(["click 1"])

      root.render(<List inert={false} log={log} />)
      renderer.flush()
      expect(renderer.getElementsAt(...at())).toContain(row)
      expect(hidden(renderer, "row 1")).toBe(false)
      renderer.nativeSimulateClick(...at())
      expect(log).toEqual(["click 1", "click 1"])
      renderer.focusElement(row)
      expect(focused()).toBe("row-1")
    })
  })
})
