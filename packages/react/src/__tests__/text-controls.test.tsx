/** `<input>` and `<textarea>`: selection API, `disabled`, `type`, `maxLength`. */
// @ts-nocheck

import React, { useRef, useState } from "react"
import { beforeEach, describe, expect, it } from "vitest"
import type { EventPayload } from "@gpuix/native"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("text control selection", () => {
  let testRoot: ReturnType<typeof createTestRoot>

  beforeEach(() => {
    testRoot = createTestRoot()
  })

  function Field(props: { initial: string; onRef?: (el: unknown) => void; [key: string]: unknown }) {
    const [text, setText] = useState(props.initial)
    const { initial, onRef, ...rest } = props
    return (
      <div style={{ width: 400, height: 100 }}>
        <input
          ref={onRef}
          value={text}
          style={{ width: 300, height: 40 }}
          onChange={(event: EventPayload) => setText(event.value ?? "")}
          {...rest}
        />
        <text>{`Value: ${text}`}</text>
      </div>
    )
  }

  it("select() selects everything and reports no direction", () => {
    let input
    testRoot.render(<Field initial="hello" onRef={(el) => (input = el)} />)
    const id = testRoot.renderer.findByType("input")[0].id

    expect(input.selectionStart).toBe(5)
    expect(input.selectionEnd).toBe(5)
    input.select()
    expect(input.selectionStart).toBe(0)
    expect(input.selectionEnd).toBe(5)
    expect(input.selectionDirection).toBe("none")

    testRoot.renderer.nativeSimulateKeystrokes(id, "x")
    expect(testRoot.renderer.getAllText()).toContain("Value: x")
  })

  it("setSelectionRange() selects a range with a direction", () => {
    let input
    testRoot.render(<Field initial="hello" onRef={(el) => (input = el)} />)
    const id = testRoot.renderer.findByType("input")[0].id

    input.setSelectionRange(1, 4, "backward")
    expect(input.selectionStart).toBe(1)
    expect(input.selectionEnd).toBe(4)
    expect(input.selectionDirection).toBe("backward")

    input.setSelectionRange(1, 4)
    expect(input.selectionDirection).toBe("none")
    input.setSelectionRange(1, 4, "forward")
    expect(input.selectionDirection).toBe("forward")

    testRoot.renderer.nativeSimulateKeystrokes(id, "J")
    expect(testRoot.renderer.getAllText()).toContain("Value: hJo")
  })

  it("clamps the range and collapses it when end is not after start", () => {
    let input
    testRoot.render(<Field initial="hello" onRef={(el) => (input = el)} />)

    input.setSelectionRange(2, 99)
    expect([input.selectionStart, input.selectionEnd]).toEqual([2, 5])
    input.setSelectionRange(4, 1)
    expect([input.selectionStart, input.selectionEnd]).toEqual([1, 1])
    input.setSelectionRange(-3, 2)
    expect([input.selectionStart, input.selectionEnd]).toEqual([0, 2])
  })

  it("counts offsets in UTF-16 code units", () => {
    let input
    testRoot.render(<Field initial={"a\u{1F600}b"} onRef={(el) => (input = el)} />)
    const id = testRoot.renderer.findByType("input")[0].id

    expect(input.selectionEnd).toBe(4)
    input.setSelectionRange(1, 3)
    expect([input.selectionStart, input.selectionEnd]).toEqual([1, 3])
    testRoot.renderer.nativeSimulateKeystrokes(id, "x")
    expect(testRoot.renderer.getAllText()).toContain("Value: axb")
  })

  it("reads and writes the selection on a mounted ref before the first frame", () => {
    function Mount() {
      const ref = useRef(null)
      React.useLayoutEffect(() => {
        ref.current.setSelectionRange(1, 2)
      }, [])
      return <input ref={ref} value="abc" style={{ width: 200, height: 30 }} />
    }
    testRoot.render(<Mount />)
    const input = testRoot.renderer.findByType("input")[0]
    expect(testRoot.renderer.getTextSelection(input.id)).toEqual({
      start: 1,
      end: 2,
      direction: "none",
    })
  })

  it("sees a controlled value update that is not painted yet", () => {
    let input
    const capture = (el) => {
      if (el) input = el
    }
    const field = (value: string) => (
      <input ref={capture} value={value} style={{ width: 200, height: 30 }} />
    )
    testRoot.render(field("ab"))
    testRoot.render(field("abcdef"))
    input.setSelectionRange(3, 6)
    expect([input.selectionStart, input.selectionEnd]).toEqual([3, 6])
  })

  it("works on a textarea", () => {
    let area
    testRoot.render(
      <textarea ref={(el) => (area = el)} value={"one\ntwo"} style={{ width: 200 }} />,
    )
    area.setSelectionRange(2, 5)
    expect([area.selectionStart, area.selectionEnd]).toEqual([2, 5])
    area.select()
    expect(area.selectionEnd).toBe(7)
  })

  it("selects everything from an onFocus handler (select on focus)", () => {
    function SelectOnFocus() {
      const ref = useRef(null)
      const [text, setText] = useState("hello")
      return (
        <div style={{ width: 400, height: 100 }}>
          <input
            ref={ref}
            value={text}
            style={{ width: 300, height: 40 }}
            onFocus={() => ref.current.select()}
            onChange={(event: EventPayload) => setText(event.value ?? "")}
          />
          <text>{`Value: ${text}`}</text>
        </div>
      )
    }
    testRoot.render(<SelectOnFocus />)
    const input = testRoot.renderer.findByType("input")[0]
    testRoot.renderer.focusElement(input.id)
    expect(testRoot.renderer.getTextSelection(input.id)).toEqual({
      start: 0,
      end: 5,
      direction: "none",
    })
    testRoot.renderer.simulateKeystrokes("x")
    expect(testRoot.renderer.getAllText()).toContain("Value: x")
  })

  it("selects everything when a click focuses the field", () => {
    function SelectOnFocus() {
      const ref = useRef(null)
      return (
        <input
          ref={ref}
          value="hello"
          style={{ width: 300, height: 40 }}
          onFocus={() => ref.current.select()}
        />
      )
    }
    testRoot.render(<SelectOnFocus />)
    const input = testRoot.renderer.findByType("input")[0]
    testRoot.renderer.nativeSimulateClick(150, 20)
    expect(testRoot.renderer.getTextSelection(input.id)).toMatchObject({ start: 0, end: 5 })
  })
})

describeNative("<input disabled>", () => {
  let testRoot: ReturnType<typeof createTestRoot>

  beforeEach(() => {
    testRoot = createTestRoot()
  })

  function Form(props: { disabled: boolean; log: string[] }) {
    const [text, setText] = useState("locked")
    return (
      <div style={{ width: 400, height: 200, flexDirection: "column" }}>
        <div tabIndex={0} testId="before" style={{ width: 50, height: 20 }} />
        <input
          disabled={props.disabled}
          value={text}
          testId="field"
          style={{ width: 300, height: 40 }}
          onChange={(event: EventPayload) => setText(event.value ?? "")}
          onClick={() => props.log.push("click")}
          onFocus={() => props.log.push("focus")}
          onBlur={() => props.log.push("blur")}
          onKeyDown={() => props.log.push("keydown")}
        />
        <div tabIndex={0} testId="after" style={{ width: 50, height: 20 }} />
        <text>{`Value: ${text}`}</text>
      </div>
    )
  }

  const focusedTestId = () =>
    testRoot.renderer.getElement(testRoot.renderer.getFocusedElementId())?.testId

  it("cannot be focused by code, a click or Tab", () => {
    const log: string[] = []
    testRoot.render(<Form disabled log={log} />)
    const field = testRoot.renderer.findByTestId("field")
    const bounds = testRoot.renderer.getElementBounds(field.id)

    testRoot.renderer.focusElement(field.id)
    expect(focusedTestId()).not.toBe("field")

    testRoot.renderer.nativeSimulateClick(bounds.x + 20, bounds.y + 20)
    expect(focusedTestId()).not.toBe("field")

    testRoot.renderer.focusElement(testRoot.renderer.findByTestId("before").id)
    testRoot.renderer.simulateKeystrokes("tab")
    expect(focusedTestId()).toBe("after")
    expect(log).toEqual([])
  })

  it("takes no editing, and no click or key events", () => {
    const log: string[] = []
    testRoot.render(<Form disabled log={log} />)
    const field = testRoot.renderer.findByTestId("field")
    const bounds = testRoot.renderer.getElementBounds(field.id)

    testRoot.renderer.nativeSimulateClick(bounds.x + 20, bounds.y + 20)
    testRoot.renderer.simulateKeystrokes("x backspace")
    expect(testRoot.renderer.getAllText()).toContain("Value: locked")
    expect(log).toEqual([])
  })

  it("is excluded from the tab order and gives focus up when it becomes disabled", () => {
    const log: string[] = []
    testRoot.render(<Form disabled={false} log={log} />)
    const field = testRoot.renderer.findByTestId("field")
    testRoot.renderer.focusElement(field.id)
    expect(focusedTestId()).toBe("field")
    expect(log).toContain("focus")

    testRoot.render(<Form disabled log={log} />)
    testRoot.renderer.dispatchNativeEvents()
    expect(focusedTestId()).not.toBe("field")
    expect(testRoot.renderer.getFocusedElementId()).toBeNull()
    expect(log).toContain("blur")
  })
})

describeNative("<input type>", () => {
  let testRoot: ReturnType<typeof createTestRoot>

  beforeEach(() => {
    testRoot = createTestRoot()
  })

  it("paints a bullet per character for a password and reports the raw value", () => {
    const changes: string[] = []
    function Password() {
      const [text, setText] = useState("")
      return (
        <input
          type="password"
          value={text}
          style={{ width: 300, height: 40 }}
          onChange={(event: EventPayload) => {
            changes.push(event.value)
            setText(event.value ?? "")
          }}
        />
      )
    }
    testRoot.render(<Password />)
    const input = testRoot.renderer.findByType("input")[0]
    testRoot.renderer.nativeSimulateKeystrokes(input.id, "s e c r e t")

    expect(changes.at(-1)).toBe("secret")
    const painted = testRoot.renderer.getPaintedText()
    expect(painted).toContain("••••••")
    expect(painted.join("")).not.toContain("secret")
  })

  it("keeps editing a password at the right place", () => {
    function Password() {
      const [text, setText] = useState("abc")
      return (
        <div>
          <input
            type="password"
            value={text}
            style={{ width: 300, height: 40 }}
            onChange={(event: EventPayload) => setText(event.value ?? "")}
          />
          <text>{`Value: ${text}`}</text>
        </div>
      )
    }
    testRoot.render(<Password />)
    const input = testRoot.renderer.findByType("input")[0]
    testRoot.renderer.nativeSimulateKeystrokes(input.id, "left left X")
    expect(testRoot.renderer.getAllText()).toContain("Value: aXbc")
  })

  it("steps a number with the arrow keys within min and max and emits change", () => {
    const changes: string[] = []
    function Quantity() {
      const [text, setText] = useState("8")
      return (
        <input
          type="number"
          min={0}
          max={10}
          step={1}
          value={text}
          style={{ width: 300, height: 40 }}
          onChange={(event: EventPayload) => {
            changes.push(event.value)
            setText(event.value ?? "")
          }}
        />
      )
    }
    testRoot.render(<Quantity />)
    const input = testRoot.renderer.findByType("input")[0]
    testRoot.renderer.nativeSimulateKeystrokes(input.id, "up up up")
    expect(changes).toEqual(["9", "10"])
    testRoot.renderer.nativeSimulateKeystrokes(input.id, "down down")
    expect(changes).toEqual(["9", "10", "9", "8"])
  })

  it("steps by a decimal step", () => {
    const changes: string[] = []
    testRoot.render(
      <input
        type="number"
        step="0.1"
        value="0.2"
        style={{ width: 300, height: 40 }}
        onChange={(event: EventPayload) => changes.push(event.value)}
      />,
    )
    const input = testRoot.renderer.findByType("input")[0]
    testRoot.renderer.nativeSimulateKeystrokes(input.id, "up")
    expect(changes).toEqual(["0.3"])
  })

  it("lets a number input hold text that is not a number", () => {
    const changes: string[] = []
    function Free() {
      const [text, setText] = useState("")
      return (
        <input
          type="number"
          value={text}
          style={{ width: 300, height: 40 }}
          onChange={(event: EventPayload) => {
            changes.push(event.value)
            setText(event.value ?? "")
          }}
        />
      )
    }
    testRoot.render(<Free />)
    const input = testRoot.renderer.findByType("input")[0]
    testRoot.renderer.nativeSimulateKeystrokes(input.id, "1 e x")
    expect(changes.at(-1)).toBe("1ex")
  })

  it("does not step a text input, a readOnly number or a textarea", () => {
    const changes: string[] = []
    const onChange = (event: EventPayload) => changes.push(event.value)
    testRoot.render(
      <div>
        <input type="text" value="4" style={{ width: 100, height: 30 }} onChange={onChange} />
        <input type="number" readOnly value="4" style={{ width: 100, height: 30 }} onChange={onChange} />
        <textarea type="number" value="4" style={{ width: 100 }} onChange={onChange} />
      </div>,
    )
    for (const input of testRoot.renderer.findByType("input")) {
      testRoot.renderer.nativeSimulateKeystrokes(input.id, "up")
    }
    testRoot.renderer.nativeSimulateKeystrokes(testRoot.renderer.findByType("textarea")[0].id, "up")
    expect(changes).toEqual([])
  })

  it("exposes type=search as a search field and a password as a password field", () => {
    testRoot.render(
      <div>
        <input type="search" value="" style={{ width: 100, height: 30 }} />
        <input type="password" value="hunter2" style={{ width: 100, height: 30 }} />
        <input type="email" value="a@b.c" style={{ width: 100, height: 30 }} />
      </div>,
    )
    testRoot.renderer.flush()
    const tree = testRoot.renderer.getA11yTree()
    const nodes = Object.values(tree.nodes ?? {}).map((node) => node.aria)
    const roles = nodes.map((aria) => aria?.role)
    expect(roles).toContain("SearchInput")
    expect(roles).toContain("PasswordInput")
    expect(roles).toContain("TextInput")
    expect(JSON.stringify(tree)).not.toContain("hunter2")
  })
})

describeNative("maxLength", () => {
  let testRoot: ReturnType<typeof createTestRoot>

  beforeEach(() => {
    testRoot = createTestRoot()
  })

  it("cuts typed input to fit", () => {
    function Limited() {
      const [text, setText] = useState("")
      return (
        <div>
          <input
            maxLength={3}
            value={text}
            style={{ width: 300, height: 40 }}
            onChange={(event: EventPayload) => setText(event.value ?? "")}
          />
          <text>{`Value: ${text}`}</text>
        </div>
      )
    }
    testRoot.render(<Limited />)
    const input = testRoot.renderer.findByType("input")[0]
    testRoot.renderer.nativeSimulateKeystrokes(input.id, "a b c d e")
    expect(testRoot.renderer.getAllText()).toContain("Value: abc")
  })

  it("keeps a longer value set from code, and still allows deleting", () => {
    function Limited() {
      const [text, setText] = useState("abcdef")
      return (
        <div>
          <input
            maxLength={3}
            value={text}
            style={{ width: 300, height: 40 }}
            onChange={(event: EventPayload) => setText(event.value ?? "")}
          />
          <text>{`Value: ${text}`}</text>
        </div>
      )
    }
    testRoot.render(<Limited />)
    const input = testRoot.renderer.findByType("input")[0]
    expect(testRoot.renderer.getAllText()).toContain("Value: abcdef")
    testRoot.renderer.nativeSimulateKeystrokes(input.id, "x")
    expect(testRoot.renderer.getAllText()).toContain("Value: abcdef")
    testRoot.renderer.nativeSimulateKeystrokes(input.id, "backspace")
    expect(testRoot.renderer.getAllText()).toContain("Value: abcde")
  })

  it("lets a selection be replaced up to the limit", () => {
    function Limited() {
      const [text, setText] = useState("ab")
      return (
        <div>
          <input
            maxLength={3}
            value={text}
            style={{ width: 300, height: 40 }}
            onChange={(event: EventPayload) => setText(event.value ?? "")}
          />
          <text>{`Value: ${text}`}</text>
        </div>
      )
    }
    testRoot.render(<Limited />)
    const input = testRoot.renderer.findByType("input")[0]
    testRoot.renderer.nativeSimulateKeystrokes(input.id, "cmd-a x y z")
    expect(testRoot.renderer.getAllText()).toContain("Value: xyz")
  })
})
