/**
 * Keys and focus reach JS synchronously, before native acts on them, so
 * `preventDefault()` has its DOM meaning. Also the `window` and `document`
 * targets and the re-entrant renderer calls a handler may make.
 */
// @ts-nocheck

import React, { useRef, useState } from "react"
import { afterEach, beforeEach, describe, expect, it } from "vitest"
import { document, window } from "@gpuix/native/host"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("synchronous key dispatch", () => {
  let testRoot: ReturnType<typeof createTestRoot>
  const cleanups: Array<() => void> = []

  beforeEach(() => {
    testRoot = createTestRoot()
  })

  afterEach(() => {
    for (const cleanup of cleanups.splice(0)) cleanup()
  })

  /** `window.addEventListener` that the test removes again. */
  function listen(target, type: string, listener, options?) {
    target.addEventListener(type, listener, options)
    cleanups.push(() => target.removeEventListener(type, listener, options))
  }

  function Control(props: { initial: string; area?: boolean; onKeyDown?: Function; onRef: Function }) {
    const [text, setText] = useState(props.initial)
    const Tag = props.area ? "textarea" : "input"
    return (
      <div style={{ width: 400, height: 160 }}>
        <Tag
          ref={props.onRef}
          value={text}
          style={{ width: 300, height: props.area ? 80 : 40 }}
          onKeyDown={props.onKeyDown}
          onChange={(event) => setText(event.value ?? "")}
        />
        <text>{`Value: ${JSON.stringify(text)}`}</text>
      </div>
    )
  }

  function mountControl(initial: string, options: { area?: boolean; onKeyDown?: Function } = {}) {
    let control
    testRoot.render(<Control initial={initial} onRef={(el) => (control = el)} {...options} />)
    const id = testRoot.renderer.findByType(options.area ? "textarea" : "input")[0].id
    return {
      control: () => control,
      press: (keys: string) => testRoot.renderer.nativeSimulateKeystrokes(id, keys),
      value: () => testRoot.renderer.getAllText().find((line) => line.startsWith("Value: ")),
    }
  }

  describe("preventDefault on keydown", () => {
    // Each case: a key that changes the caret or the text when nothing stops it.
    const cases = [
      { key: "left", area: false, initial: "hello", caret: 2 },
      { key: "right", area: false, initial: "hello", caret: 2 },
      { key: "home", area: false, initial: "hello", caret: 2 },
      { key: "end", area: false, initial: "hello", caret: 2 },
      { key: "backspace", area: false, initial: "hello", caret: 2 },
      { key: "enter", area: true, initial: "hello", caret: 2 },
      { key: "up", area: true, initial: "ab\ncd", caret: 4 },
      { key: "down", area: true, initial: "ab\ncd", caret: 1 },
      { key: "x", area: false, initial: "hello", caret: 2 },
      { key: "x", area: true, initial: "hello", caret: 2 },
    ]

    for (const { key, area, initial, caret } of cases) {
      it(`cancels ${key} in a${area ? " textarea" : "n input"}`, () => {
        let prevent = false
        const field = mountControl(initial, {
          area,
          onKeyDown: (event) => {
            if (prevent) event.preventDefault()
          },
        })
        const state = () => [field.control().selectionStart, field.value()]

        // Without preventDefault the key acts, so the case is a real one.
        field.control().setSelectionRange(caret, caret)
        const before = state()
        field.press(key)
        expect(state()).not.toEqual(before)

        prevent = true
        field.control().setSelectionRange(caret, caret)
        const after = state()
        field.press(key)
        expect(state()).toEqual(after)
      })
    }
  })

  it("runs the keydown handler before the editor acts on the key", () => {
    const seen: number[] = []
    const field = mountControl("hello", {
      onKeyDown: () => seen.push(field.control().selectionStart),
    })
    field.control().setSelectionRange(2, 2)
    field.press("left")
    expect(seen).toEqual([2])
    expect(field.control().selectionStart).toBe(1)
  })

  it("lets a handler read the caret, move it and cancel the key", () => {
    const seen: Record<string, unknown> = {}
    const field = mountControl("hello", {
      onKeyDown: (event) => {
        seen.caret = field.control().selectionStart
        field.control().setSelectionRange(4, 4)
        seen.after = field.control().selectionStart
        seen.windowSize = testRoot.renderer.getWindowSize()
        seen.a11y = testRoot.renderer.getA11yTree()
        event.preventDefault()
      },
    })
    field.control().setSelectionRange(2, 2)
    field.press("left")

    expect(seen.caret).toBe(2)
    expect(seen.after).toBe(4)
    expect(seen.windowSize).toEqual(testRoot.renderer.getWindowSize())
    expect(seen.a11y).toBeDefined()
    expect(field.control().selectionStart).toBe(4)
    expect(field.value()).toBe('Value: "hello"')
  })

  it("lets a handler call back into the renderer while the key is dispatched", () => {
    const seen: Record<string, unknown> = {}
    function App() {
      const [count, setCount] = useState(0)
      const other = useRef()
      const field = useRef()
      return (
        <div style={{ width: 400, height: 200 }}>
          <div
            testId="a"
            tabIndex={0}
            style={{ width: 40, height: 20 }}
            onKeyDown={() => {
              seen.focusedAtKey = testRoot.renderer.getElement(testRoot.renderer.getFocusedElementId())?.testId
              seen.selection = field.current.selectionStart
              setCount((value) => value + 1)
              other.current.focus()
              seen.focusedRightAfter = testRoot.renderer.getElement(testRoot.renderer.getFocusedElementId())?.testId
            }}
          />
          <div ref={other} testId="b" tabIndex={0} style={{ width: 40, height: 20 }} />
          <input ref={field} value="hello" onChange={() => {}} style={{ width: 100, height: 30 }} />
          <text>{`count=${count}`}</text>
        </div>
      )
    }
    testRoot.render(<App />)
    const a = testRoot.renderer.findByTestId("a")
    testRoot.renderer.nativeSimulateKeystrokes(a.id, "x")

    expect(seen.focusedAtKey).toBe("a")
    expect(seen.selection).toBe(5)
    expect(seen.focusedRightAfter).toBe("b")
    expect(testRoot.renderer.getAllText()).toContain("count=1")
    const focused = testRoot.renderer.getFocusedElementId()
    expect(testRoot.renderer.getElement(focused)?.testId).toBe("b")
  })

  describe("Tab", () => {
    function Stops(props: { onKeyDown?: Function }) {
      return (
        <div style={{ width: 300, height: 100 }} onKeyDown={props.onKeyDown}>
          <div testId="one" tabIndex={0} style={{ width: 40, height: 20 }} />
          <div testId="two" tabIndex={0} style={{ width: 40, height: 20 }} />
        </div>
      )
    }
    const focused = () =>
      testRoot.renderer.getElement(testRoot.renderer.getFocusedElementId())?.testId ?? null

    it("moves focus unless a handler prevents it", () => {
      let prevent = true
      testRoot.render(<Stops onKeyDown={(event) => prevent && event.preventDefault()} />)
      testRoot.renderer.focusElement(testRoot.renderer.findByTestId("one").id)
      testRoot.renderer.simulateKeystrokes("tab")
      expect(focused()).toBe("one")

      prevent = false
      testRoot.renderer.simulateKeystrokes("tab")
      expect(focused()).toBe("two")
    })

    it("never types a tab into a text field", () => {
      const field = mountControl("ab")
      field.press("tab")
      expect(field.value()).toBe('Value: "ab"')
    })
  })

  describe("window and document", () => {
    function Focusable(props: { onKeyDown?: Function }) {
      return (
        <div style={{ width: 200, height: 100 }}>
          <div testId="target" tabIndex={0} style={{ width: 40, height: 20 }} onKeyDown={props.onKeyDown} />
        </div>
      )
    }
    const press = (keys: string) =>
      testRoot.renderer.nativeSimulateKeystrokes(testRoot.renderer.findByTestId("target").id, keys)

    it("runs window capture, document capture, the element, document, then window", () => {
      const log: string[] = []
      testRoot.render(<Focusable onKeyDown={() => log.push("element")} />)
      listen(window, "keydown", () => log.push("window"))
      listen(document, "keydown", () => log.push("document"))
      listen(window, "keydown", (event) => log.push(`window capture ${event.eventPhase}`), { capture: true })
      listen(document, "keydown", (event) => log.push(`document capture ${event.eventPhase}`), { capture: true })
      press("x")
      expect(log).toEqual(["window capture 1", "document capture 1", "element", "document", "window"])
    })

    it("hears a key with nothing focused", () => {
      const keys: string[] = []
      testRoot.render(<Focusable />)
      listen(window, "keydown", (event) => keys.push(`${event.key}:${event.target}`))
      testRoot.renderer.simulateKeystrokes("x")
      expect(keys).toEqual(["x:undefined"])
    })

    it("stops at stopPropagation, in capture and on the element", () => {
      const log: string[] = []
      let stopAtElement = false
      testRoot.render(
        <Focusable
          onKeyDown={(event) => {
            log.push("element")
            if (stopAtElement) event.stopPropagation()
          }}
        />
      )
      let stopAtWindow = true
      listen(window, "keydown", (event) => {
        log.push("window capture")
        if (stopAtWindow) event.stopPropagation()
      }, { capture: true })
      listen(window, "keydown", () => log.push("window bubble"))
      press("x")
      expect(log).toEqual(["window capture"])

      log.length = 0
      stopAtWindow = false
      stopAtElement = true
      press("x")
      expect(log).toEqual(["window capture", "element"])
    })

    it("honors once, removeEventListener and the root onKeyDown option", () => {
      const log: string[] = []
      testRoot.render(<Focusable />)
      listen(window, "keydown", () => log.push("once"), { once: true })
      const removed = () => log.push("removed")
      window.addEventListener("keydown", removed)
      window.removeEventListener("keydown", removed)
      press("x")
      press("y")
      expect(log).toEqual(["once"])
    })

    it("treats the root onKeyDown option as a window bubble listener", () => {
      const log: string[] = []
      testRoot.root.unmount()
      testRoot = createTestRoot({ onKeyDown: (event) => log.push(`root ${event.key}`) })
      testRoot.render(<Focusable onKeyDown={() => log.push("element")} />)
      listen(window, "keydown", () => log.push("window"))
      press("x")
      expect(log).toEqual(["element", "window", "root x"])
    })

    it("lets preventDefault in a window listener cancel the editor", () => {
      const field = mountControl("hello")
      listen(window, "keydown", (event) => event.preventDefault())
      field.control().setSelectionRange(2, 2)
      field.press("backspace")
      expect(field.value()).toBe('Value: "hello"')
    })
  })
})

describeNative("focus events on window", () => {
  let testRoot: ReturnType<typeof createTestRoot>
  const cleanups: Array<() => void> = []

  beforeEach(() => {
    testRoot = createTestRoot()
  })

  afterEach(() => {
    for (const cleanup of cleanups.splice(0)) cleanup()
  })

  function listen(target, type: string, listener, options?) {
    target.addEventListener(type, listener, options)
    cleanups.push(() => target.removeEventListener(type, listener, options))
  }

  const idOf = (testId: string) => testRoot.renderer.findByTestId(testId).id

  function Pair(props: { onFocus?: Function }) {
    return (
      <div testId="outer" style={{ width: 200, height: 100 }} onFocus={props.onFocus}>
        <div testId="a" tabIndex={0} style={{ width: 40, height: 20 }} />
        <div testId="b" tabIndex={0} style={{ width: 40, height: 20 }} />
      </div>
    )
  }

  it("bubbles focusin and focusout with target and relatedTarget", () => {
    const log: string[] = []
    testRoot.render(<Pair />)
    listen(window, "focusin", (event) => log.push(`in ${event.target}<-${event.relatedTarget}`))
    listen(window, "focusout", (event) => log.push(`out ${event.target}->${event.relatedTarget}`))
    testRoot.renderer.focusElement(idOf("a"))
    testRoot.renderer.focusElement(idOf("b"))
    expect(log).toEqual([
      `in ${idOf("a")}<-undefined`,
      `out ${idOf("a")}->${idOf("b")}`,
      `in ${idOf("b")}<-${idOf("a")}`,
    ])
  })

  it("does not bubble focus and blur, but captures them", () => {
    const log: string[] = []
    testRoot.render(<Pair />)
    listen(window, "focus", () => log.push("focus bubble"))
    listen(window, "focus", () => log.push("focus capture"), { capture: true })
    testRoot.renderer.focusElement(idOf("a"))
    expect(log).toEqual(["focus capture"])
  })

  it("stops a focusin at the element that stops it", () => {
    const log: string[] = []
    testRoot.render(<Pair onFocus={(event) => event.stopPropagation()} />)
    listen(window, "focusin", () => log.push("window"))
    testRoot.renderer.focusElement(idOf("a"))
    expect(log).toEqual([])
  })
})

describeNative("mouse press focus", () => {
  let testRoot: ReturnType<typeof createTestRoot>

  beforeEach(() => {
    testRoot = createTestRoot()
  })

  const focused = () =>
    testRoot.renderer.getElement(testRoot.renderer.getFocusedElementId())?.testId ?? null

  function Scene(props: { onMouseDown?: Function }) {
    return (
      <div style={{ width: 300, height: 200 }}>
        <div testId="field" tabIndex={0} style={{ width: 100, height: 40 }} />
        <div
          testId="plain"
          style={{ width: 100, height: 40 }}
          onMouseDown={props.onMouseDown}
        />
      </div>
    )
  }

  const press = (testId: string) => {
    const bounds = testRoot.renderer.getElementBounds(testRoot.renderer.findByTestId(testId).id)
    testRoot.renderer.nativeSimulateClick(bounds.x + 5, bounds.y + 5)
  }

  it("blurs the focused element when a press lands on something not focusable", () => {
    testRoot.render(<Scene />)
    testRoot.renderer.focusElement(testRoot.renderer.findByTestId("field").id)
    expect(focused()).toBe("field")
    press("plain")
    expect(focused()).toBe(null)
  })

  it("keeps focus when mousedown is prevented", () => {
    testRoot.render(<Scene onMouseDown={(event) => event.preventDefault()} />)
    testRoot.renderer.focusElement(testRoot.renderer.findByTestId("field").id)
    press("plain")
    expect(focused()).toBe("field")
  })

  /** `plain` and `field` side by side, under a wrapper that takes the handlers. */
  function Wrapped(props: Record<string, Function>) {
    return (
      <div testId="wrapper" style={{ width: 300, height: 200 }} {...props}>
        <div testId="field" tabIndex={0} style={{ width: 100, height: 40 }} />
        <div testId="plain" style={{ width: 100, height: 40 }} />
      </div>
    )
  }

  it("keeps focus when an ancestor prevents mousedown", () => {
    testRoot.render(<Wrapped onMouseDown={(event) => event.preventDefault()} />)
    testRoot.renderer.focusElement(testRoot.renderer.findByTestId("field").id)
    press("plain")
    expect(focused()).toBe("field")
  })

  it("keeps focus when a capture handler prevents mousedown", () => {
    testRoot.render(<Wrapped onMouseDownCapture={(event) => event.preventDefault()} />)
    testRoot.renderer.focusElement(testRoot.renderer.findByTestId("field").id)
    press("plain")
    expect(focused()).toBe("field")
  })

  it("does not move focus onto a focusable that mousedown was prevented on", () => {
    testRoot.render(<Wrapped onMouseDown={(event) => event.preventDefault()} />)
    press("field")
    expect(focused()).toBe(null)
  })

  it("moves focus when only pointerdown is prevented, as in the DOM", () => {
    testRoot.render(<Wrapped onPointerDown={(event) => event.preventDefault()} />)
    testRoot.renderer.focusElement(testRoot.renderer.findByTestId("field").id)
    press("plain")
    expect(focused()).toBe(null)
  })

  it("focuses an input on mousedown unless it was prevented on an ancestor", () => {
    const Scene = (props: { prevent: boolean }) => (
      <div
        style={{ width: 300, height: 100 }}
        onMouseDown={props.prevent ? (event) => event.preventDefault() : undefined}
      >
        <input testId="text" style={{ width: 200, height: 40 }} />
      </div>
    )
    testRoot.render(<Scene prevent={true} />)
    press("text")
    expect(focused()).toBe(null)
    testRoot.render(<Scene prevent={false} />)
    press("text")
    expect(focused()).toBe("text")
  })

  it("focuses a focusable ancestor of a filled child", () => {
    testRoot.render(
      <div style={{ width: 300, height: 200 }}>
        <div testId="holder" tabIndex={0} style={{ width: 100, height: 60 }}>
          <div testId="filled" style={{ width: 100, height: 60, backgroundColor: "#334155" }} />
        </div>
      </div>
    )
    press("filled")
    expect(focused()).toBe("holder")
  })
})
