import { createSignal } from "solid-js"
import { afterEach, describe, expect, it } from "bun:test"
import { document, window } from "@gpuix/native/host"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

describe.skipIf(!hasNativeTestRenderer)("Solid synchronous key dispatch", () => {
  const cleanups: Array<() => void> = []

  afterEach(() => {
    for (const cleanup of cleanups.splice(0)) cleanup()
  })

  function listen(
    target: typeof window,
    type: string,
    listener: (event: any) => void,
    options?: { capture?: boolean; once?: boolean }
  ) {
    target.addEventListener(type, listener, options)
    cleanups.push(() => target.removeEventListener(type, listener, options))
  }

  function mountFocusable(onKeyDown?: (event: any) => void) {
    const app = createTestRoot()
    app.render(() => (
      <div style={{ width: 200, height: 100 }}>
        <div testId="target" tabIndex={0} style={{ width: 40, height: 20 }} onKeyDown={onKeyDown} />
      </div>
    ))
    const id = app.renderer.findByTestId("target")!.id
    return { app, press: (keys: string) => app.renderer.nativeSimulateKeystrokes(id, keys) }
  }

  it("runs window capture, document capture, the element, document, then window", () => {
    const log: string[] = []
    const { press } = mountFocusable(() => log.push("element"))
    listen(window, "keydown", () => log.push("window"))
    listen(document, "keydown", () => log.push("document"))
    listen(window, "keydown", (event) => log.push(`window capture ${event.eventPhase}`), { capture: true })
    listen(document, "keydown", (event) => log.push(`document capture ${event.eventPhase}`), { capture: true })
    press("x")
    expect(log).toEqual(["window capture 1", "document capture 1", "element", "document", "window"])
  })

  it("stops at stopPropagation, in capture and on the element", () => {
    const log: string[] = []
    let stopAtElement = false
    const { press } = mountFocusable((event) => {
      log.push("element")
      if (stopAtElement) event.stopPropagation()
    })
    let stopAtWindow = true
    listen(
      window,
      "keydown",
      (event) => {
        log.push("window capture")
        if (stopAtWindow) event.stopPropagation()
      },
      { capture: true }
    )
    listen(window, "keydown", () => log.push("window bubble"))
    press("x")
    expect(log).toEqual(["window capture"])

    log.length = 0
    stopAtWindow = false
    stopAtElement = true
    press("x")
    expect(log).toEqual(["window capture", "element"])
  })

  it("cancels an input action with preventDefault on keydown", () => {
    const [text, setText] = createSignal("hello")
    const app = createTestRoot()
    let prevent = false
    app.render(() => (
      <input
        testId="field"
        value={text()}
        style={{ width: 300, height: 40 }}
        onKeyDown={(event) => {
          if (prevent) event.preventDefault()
        }}
        onChange={(event) => setText(event.value ?? "")}
      />
    ))
    const field = app.renderer.findByTestId("field")!
    const press = (keys: string) => app.renderer.nativeSimulateKeystrokes(field.id, keys)

    press("Backspace")
    expect(text()).toBe("hell")

    prevent = true
    press("Backspace")
    press("x")
    expect(text()).toBe("hell")
  })

  it("lets preventDefault in a window listener cancel the editor", () => {
    const [text, setText] = createSignal("hello")
    const app = createTestRoot()
    app.render(() => (
      <input
        testId="field"
        value={text()}
        style={{ width: 300, height: 40 }}
        onChange={(event) => setText(event.value ?? "")}
      />
    ))
    listen(window, "keydown", (event) => event.preventDefault())
    app.renderer.nativeSimulateKeystrokes(app.renderer.findByTestId("field")!.id, "Backspace")
    expect(text()).toBe("hello")
  })
})
