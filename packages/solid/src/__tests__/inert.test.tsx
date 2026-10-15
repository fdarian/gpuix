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
    renderer.simulateKeystrokes("tab")
    expect(focused()).toBe("before")
    renderer.simulateKeystrokes("tab")
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
    const parent = Object.values(tree.nodes ?? {}).find((node) =>
      node.children?.some((child) => tree.nodes?.[child]?.aria?.label === "inside")
    )
    expect(parent?.aria?.hidden).toBe(true)
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
})
