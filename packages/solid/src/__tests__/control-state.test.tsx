import { createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import { Button, Select, SelectTrigger } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

describe.skipIf(!hasNativeTestRenderer)("Solid disabled, enabled, hasDisabled and hasEnabled", () => {
  type App = ReturnType<typeof createTestRoot>

  const bounds = (app: App, testId: string) => {
    app.renderer.dispatchNativeEvents()
    return app.renderer.getElementBounds(app.renderer.findByTestId(testId)!.id)!
  }
  const widthOf = (app: App, testId: string) => bounds(app, testId).width

  const WIDTH = 50
  const states = { disabled: { width: 70 }, enabled: { width: 90 } }

  it("styles an input by whether it is disabled, and follows the signal", () => {
    const [disabled, setDisabled] = createSignal(false)
    const app = createTestRoot()
    app.render(() => <input testId="field" disabled={disabled()} style={{ width: WIDTH, height: 20, ...states }} />)
    expect(widthOf(app, "field")).toBe(90)
    setDisabled(true)
    expect(widthOf(app, "field")).toBe(70)
    setDisabled(false)
    expect(widthOf(app, "field")).toBe(90)
    app.unmount()
  })

  it("runs a transition declared in disabled", () => {
    const [disabled, setDisabled] = createSignal(false)
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <input
        testId="field"
        disabled={disabled()}
        style={{ width: WIDTH, height: 20, disabled: { width: 150, transition: "width 1s linear" } }}
      />
    ))
    expect(widthOf(app, "field")).toBe(WIDTH)
    setDisabled(true)
    // The change reaches native with the next frame, which starts the transition.
    expect(widthOf(app, "field")).toBe(WIDTH)
    app.renderer.clockFastForward(500)
    expect(widthOf(app, "field")).toBeGreaterThan(99)
    expect(widthOf(app, "field")).toBeLessThan(101)
    app.renderer.clockFastForward(1000)
    expect(widthOf(app, "field")).toBe(150)
    app.unmount()
  })

  it("styles a Button by its disabled prop", () => {
    const [disabled, setDisabled] = createSignal(false)
    const app = createTestRoot()
    app.render(() => <Button testId="button" disabled={disabled()} style={{ width: WIDTH, height: 20, ...states }} />)
    expect(widthOf(app, "button")).toBe(90)
    setDisabled(true)
    expect(widthOf(app, "button")).toBe(70)
    setDisabled(false)
    expect(widthOf(app, "button")).toBe(90)
    app.unmount()
  })

  it("styles the root and the trigger of a Select", () => {
    const [disabled, setDisabled] = createSignal(false)
    const app = createTestRoot()
    app.render(() => (
      <Select testId="select" disabled={disabled()} style={{ width: WIDTH, height: 20, ...states }}>
        <SelectTrigger testId="trigger" style={{ width: 40, height: 20, ...states }} />
      </Select>
    ))
    expect([widthOf(app, "select"), widthOf(app, "trigger")]).toEqual([90, 90])
    setDisabled(true)
    expect([widthOf(app, "select"), widthOf(app, "trigger")]).toEqual([70, 70])
    app.unmount()
  })

  it("does not match a plain div that has a disabled prop, as in a DOM", () => {
    const app = createTestRoot()
    app.render(() => (
      <div>
        {/* @ts-expect-error a div has no disabled prop */}
        <div testId="plain" disabled style={{ width: WIDTH, height: 20, ...states }} />
        <div testId="holder" style={{ width: WIDTH, height: 20, hasDisabled: { width: 70 }, hasEnabled: { width: 90 } }}>
          {/* @ts-expect-error a div has no disabled prop */}
          <div disabled style={{ width: 10, height: 10 }} />
        </div>
      </div>
    ))
    expect(widthOf(app, "plain")).toBe(WIDTH)
    expect(widthOf(app, "holder")).toBe(WIDTH)
    app.unmount()
  })

  it("matches the ancestors of a control by its state, and not a sibling or the control itself", () => {
    const [disabled, setDisabled] = createSignal(false)
    const app = createTestRoot()
    app.render(() => (
      <div>
        <div testId="holder" style={{ width: WIDTH, height: 20, hasDisabled: { width: 70 }, hasEnabled: { height: 40 } }}>
          <div style={{ width: 80, height: 30 }}>
            <input
              testId="field"
              disabled={disabled()}
              style={{ width: 20, height: 10, hasDisabled: { width: 90 }, hasEnabled: { width: 90 } }}
            />
          </div>
        </div>
        <div testId="sibling" style={{ width: WIDTH, height: 20, hasDisabled: { width: 70 }, hasEnabled: { width: 90 } }} />
      </div>
    ))
    const holder = () => [widthOf(app, "holder"), bounds(app, "holder").height]
    expect(holder()).toEqual([WIDTH, 40])
    setDisabled(true)
    expect(holder()).toEqual([70, 20])
    setDisabled(false)
    expect(holder()).toEqual([WIDTH, 40])
    expect([widthOf(app, "sibling"), widthOf(app, "field")]).toEqual([WIDTH, 20])
    app.unmount()
  })

  it("matches the ancestors of a disabled Button", () => {
    const [disabled, setDisabled] = createSignal(false)
    const app = createTestRoot()
    app.render(() => (
      <div testId="holder" style={{ width: WIDTH, height: 20, hasDisabled: { width: 70 } }}>
        <Button disabled={disabled()} style={{ width: 10, height: 10 }} />
      </div>
    ))
    expect(widthOf(app, "holder")).toBe(WIDTH)
    setDisabled(true)
    expect(widthOf(app, "holder")).toBe(70)
    app.unmount()
  })
})
