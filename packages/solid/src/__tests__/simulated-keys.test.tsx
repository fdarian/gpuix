import { createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const box = { width: 40, height: 20 }

describe.skipIf(!hasNativeTestRenderer)("Solid simulated keystrokes", () => {
  it("moves focus back on Shift+Tab", () => {
    const app = createTestRoot()
    app.render(() => (
      <div style={{ width: 200, height: 100 }}>
        <div testId="a" tabIndex={0} style={box} />
        <div testId="b" tabIndex={0} style={box} />
      </div>
    ))
    const renderer = app.renderer
    const focused = () => renderer.getElement(renderer.getFocusedElementId()!)?.testId
    renderer.focusElement(renderer.findByTestId("b")!.id)
    renderer.simulateKeystrokes("Shift+Tab")
    expect(focused()).toBe("a")
    renderer.simulateKeystrokes("Tab")
    expect(focused()).toBe("b")
  })

  it("sends keydown then keyup for a press", () => {
    const events: { type: string; key?: string }[] = []
    const app = createTestRoot()
    app.render(() => (
      <div style={{ width: 200, height: 100 }}>
        <div
          testId="target"
          tabIndex={0}
          style={box}
          onKeyDown={(event) => events.push(event)}
          onKeyUp={(event) => events.push(event)}
        />
      </div>
    ))
    app.renderer.nativeSimulateKeystrokes(app.renderer.findByTestId("target")!.id, "Enter")
    expect(events.map((event) => [event.type, event.key])).toEqual([
      ["keydown", "Enter"],
      ["keyup", "Enter"],
    ])
  })

  it("types characters into an input", () => {
    const [text, setText] = createSignal("")
    const app = createTestRoot()
    app.render(() => (
      <input testId="field" value={text()} style={{ width: 300, height: 40 }} onChange={(event) => setText(event.value ?? "")} />
    ))
    app.renderer.nativeSimulateKeystrokes(app.renderer.findByTestId("field")!.id, "a Shift+b")
    expect(text()).toBe("aB")
  })

  it("throws for an unknown key and presses nothing", () => {
    const keys: string[] = []
    const app = createTestRoot()
    app.render(() => (
      <div testId="target" tabIndex={0} style={box} onKeyDown={(event) => keys.push(event.key ?? "")} />
    ))
    const renderer = app.renderer
    renderer.focusElement(renderer.findByTestId("target")!.id)
    expect(() => renderer.simulateKeystrokes("a enter")).toThrow(/Unknown key "enter"/)
    expect(() => renderer.simulateKeystrokes("cmd-shift-p")).toThrow(/Unknown key/)
    expect(keys).toEqual([])
  })
})
