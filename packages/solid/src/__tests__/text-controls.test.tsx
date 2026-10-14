import { createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { HostElement } from "../host.js"

describe.skipIf(!hasNativeTestRenderer)("Solid text controls", () => {
  it("selects with select() and setSelectionRange() through the element", () => {
    let field!: HostElement
    const [text, setText] = createSignal("hello")
    const app = createTestRoot()
    app.render(() => (
      <input
        ref={field}
        testId="field"
        value={text()}
        style={{ width: 300, height: 40 }}
        onChange={(event) => setText(event.value ?? "")}
      />
    ))
    expect([field.selectionStart, field.selectionEnd]).toEqual([5, 5])

    field.select()
    expect([field.selectionStart, field.selectionEnd, field.selectionDirection]).toEqual([
      0,
      5,
      "none",
    ])

    field.setSelectionRange(1, 4, "backward")
    expect([field.selectionStart, field.selectionEnd, field.selectionDirection]).toEqual([
      1,
      4,
      "backward",
    ])

    app.renderer.nativeSimulateKeystrokes(field.id, "J")
    expect(text()).toBe("hJo")
  })

  it("selects on focus", () => {
    let field!: HostElement
    const app = createTestRoot()
    app.render(() => (
      <input
        ref={field}
        value="hello"
        style={{ width: 300, height: 40 }}
        onFocus={() => field.select()}
      />
    ))
    app.renderer.focusElement(field.id)
    expect([field.selectionStart, field.selectionEnd]).toEqual([0, 5])
  })

  it("rejects the selection API on other elements", () => {
    let box!: HostElement
    const app = createTestRoot()
    app.render(() => <div ref={box} style={{ width: 10, height: 10 }} />)
    expect(() => box.select()).toThrow("only valid on <input> and <textarea>")
  })

  it("keeps a disabled input out of focus and editing", () => {
    let field!: HostElement
    const [disabled, setDisabled] = createSignal(true)
    const [text, setText] = createSignal("locked")
    const app = createTestRoot()
    app.render(() => (
      <input
        ref={field}
        disabled={disabled()}
        value={text()}
        style={{ width: 300, height: 40 }}
        onChange={(event) => setText(event.value ?? "")}
      />
    ))
    app.renderer.focusElement(field.id)
    expect(app.renderer.getFocusedElementId()).toBeNull()
    app.renderer.simulateKeystrokes("x")
    expect(text()).toBe("locked")

    app.flushSync(() => setDisabled(false))
    app.renderer.focusElement(field.id)
    expect(app.renderer.getFocusedElementId()).toBe(field.id)
    app.renderer.simulateKeystrokes("x")
    expect(text()).toBe("lockedx")
  })

  it("steps a number input up to max", () => {
    const [text, setText] = createSignal("1")
    let field!: HostElement
    const app = createTestRoot()
    app.render(() => (
      <input
        ref={field}
        type="number"
        max={2}
        value={text()}
        style={{ width: 300, height: 40 }}
        onChange={(event) => setText(event.value ?? "")}
      />
    ))
    app.renderer.nativeSimulateKeystrokes(field.id, "up up")
    expect(text()).toBe("2")
  })
})
