import { describe, expect, it } from "bun:test"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

describe.skipIf(!hasNativeTestRenderer)("Solid groupFocusWithin", () => {
  type App = ReturnType<typeof createTestRoot>

  const widthOf = (app: App, testId: string): number => {
    app.renderer.dispatchNativeEvents()
    return app.renderer.getElementBounds(app.renderer.findByTestId(testId)!.id)!.width
  }
  const focus = (app: App, testId: string) => app.renderer.focusElement(app.renderer.findByTestId(testId)!.id)
  const blur = (app: App, testId: string) => app.renderer.blurElement(app.renderer.findByTestId(testId)!.id)

  it("applies to descendants while an input inside the group is focused, and clears on blur", () => {
    const app = createTestRoot()
    app.render(() => (
      <div style={{ group: true, width: 200, height: 100 }}>
        <input testId="field" style={{ width: 100, height: 20 }} />
        <div testId="follower" style={{ width: 40, height: 20, groupFocusWithin: { width: 80 } }} />
      </div>
    ))
    expect(widthOf(app, "follower")).toBe(40)
    focus(app, "field")
    expect(widthOf(app, "follower")).toBe(80)
    blur(app, "field")
    expect(widthOf(app, "follower")).toBe(40)
    app.unmount()
  })

  it("does not apply while focus is outside the group", () => {
    const app = createTestRoot()
    app.render(() => (
      <div>
        <div style={{ group: true, width: 200, height: 50 }}>
          <div testId="follower" style={{ width: 40, height: 20, groupFocusWithin: { width: 80 } }} />
        </div>
        <input testId="outside" style={{ width: 100, height: 20 }} />
      </div>
    ))
    focus(app, "outside")
    expect(widthOf(app, "follower")).toBe(40)
    app.unmount()
  })

  it("follows the nearest group by name", () => {
    const app = createTestRoot()
    app.render(() => (
      <div style={{ group: "outer", width: 300, height: 200 }}>
        <input testId="outer-field" style={{ width: 100, height: 20 }} />
        <div style={{ group: "inner", width: 200, height: 100 }}>
          <input testId="inner-field" style={{ width: 100, height: 20 }} />
          <div testId="inner-follower" style={{ width: 10, height: 20, groupFocusWithin: { group: "inner", width: 20 } }} />
          <div testId="outer-follower" style={{ width: 10, height: 20, groupFocusWithin: { group: "outer", width: 30 } }} />
        </div>
      </div>
    ))
    focus(app, "outer-field")
    // The outer input is outside "inner", but inside "outer".
    expect([widthOf(app, "inner-follower"), widthOf(app, "outer-follower")]).toEqual([10, 30])
    focus(app, "inner-field")
    expect([widthOf(app, "inner-follower"), widthOf(app, "outer-follower")]).toEqual([20, 30])
    app.unmount()
  })

  it("runs a transition declared in it", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <div style={{ group: true, width: 200, height: 100 }}>
        <input testId="field" style={{ width: 100, height: 20 }} />
        <div
          testId="follower"
          style={{ width: 40, height: 20, groupFocusWithin: { width: 80, transition: "width 1s linear" } }}
        />
      </div>
    ))
    focus(app, "field")
    app.renderer.clockFastForward(500)
    expect(widthOf(app, "follower")).toBeGreaterThan(59)
    expect(widthOf(app, "follower")).toBeLessThan(61)
    app.renderer.clockFastForward(1000)
    expect(widthOf(app, "follower")).toBe(80)
    app.unmount()
  })
})
