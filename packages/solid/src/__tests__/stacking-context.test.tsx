import { createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

describe.skipIf(!hasNativeTestRenderer)("zIndex across stacking contexts (solid)", () => {
  const scene = (clicks: string[], group: Record<string, unknown>) => (
    <div testId="root" style={{ width: 400, height: 400 }}>
      <div testId="group" style={{ width: 200, height: 200, ...group }}>
        <div
          testId="inner"
          onClick={() => clicks.push("inner")}
          style={{ position: "absolute", zIndex: 100, left: 50, top: 50, width: 100, height: 100 }}
        />
      </div>
      <div
        testId="later"
        onClick={() => clicks.push("later")}
        style={{ position: "absolute", zIndex: 2, left: 100, top: 100, width: 200, height: 200 }}
      />
    </div>
  )

  const topAt = (app: ReturnType<typeof createTestRoot>, x: number, y: number) =>
    app.renderer.getElement(app.renderer.getElementsAt(x, y)[0])?.testId

  it("lets a z-indexed descendant of an earlier sibling take the click above a later sibling", () => {
    const clicks: string[] = []
    const app = createTestRoot()
    try {
      app.render(() => scene(clicks, {}))
      app.renderer.nativeSimulateClick(120, 120)
      expect(clicks).toEqual(["inner"])
      expect(topAt(app, 120, 120)).toBe("inner")
    } finally {
      app.unmount()
    }
  })

  it("traps the z-index inside an element with opacity below 1, and follows a change", () => {
    const clicks: string[] = []
    const [opacity, setOpacity] = createSignal(0.5)
    const app = createTestRoot()
    try {
      app.render(() => scene(clicks, { get opacity() { return opacity() } }))
      app.renderer.nativeSimulateClick(120, 120)
      expect(clicks).toEqual(["later"])
      expect(topAt(app, 120, 120)).toBe("later")

      app.flushSync(() => setOpacity(1))
      app.renderer.nativeSimulateClick(120, 120)
      expect(clicks).toEqual(["later", "inner"])
      expect(topAt(app, 120, 120)).toBe("inner")
    } finally {
      app.unmount()
    }
  })
})
