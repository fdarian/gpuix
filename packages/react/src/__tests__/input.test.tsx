/** End-to-end tests for the native GPUI text editor host elements. */
// @ts-nocheck

import React, { useState } from "react"
import { beforeEach, describe, expect, it } from "vitest"
import type { EventPayload, KeyEvent } from "@gpuix/native"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("native text editors", () => {
  let testRoot: ReturnType<typeof createTestRoot>

  beforeEach(() => {
    testRoot = createTestRoot()
  })

  it("edits text natively and emits the complete value", () => {
    function TextInput() {
      const [text, setText] = useState("")
      return (
        <div style={{ width: 400, height: 100 }}>
          <input
            value={text}
            placeholder="Type here..."
            style={{ width: 300, height: 40 }}
            onChange={(event: EventPayload) => setText(event.value ?? "")}
          />
          <text>{`Value: ${text}`}</text>
        </div>
      )
    }

    testRoot.render(<TextInput />)
    const input = testRoot.renderer.findByType("input")[0]
    testRoot.renderer.nativeSimulateKeystrokes(input.id, "h i")

    expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
      [
        "Value: hi",
      ]
    `)
    expect(testRoot.renderer.getPaintedText()).toContain("hi")
  })

  it("inserts a newline on enter in a textarea", () => {
    function Textarea() {
      const [text, setText] = useState("")
      return (
        <div style={{ width: 400, height: 160 }}>
          <textarea
            value={text}
            placeholder="Write a message..."
            minRows={1}
            maxRows={4}
            style={{ width: 300 }}
            onChange={(event: EventPayload) => setText(event.value ?? "")}
          />
          <text>{`Value: ${JSON.stringify(text)}`}</text>
        </div>
      )
    }

    testRoot.render(<Textarea />)
    const textarea = testRoot.renderer.findByType("textarea")[0]

    testRoot.renderer.nativeSimulateKeystrokes(textarea.id, "h i enter t h e r e")
    expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
      [
        "Value: \"hi\\nthere\"",
      ]
    `)
  })

  it("inserts a newline in the middle of a textarea buffer", () => {
    function Textarea() {
      const [text, setText] = useState("ab")
      return (
        <div style={{ width: 400, height: 160 }}>
          <textarea
            value={text}
            style={{ width: 300 }}
            onChange={(event: EventPayload) => setText(event.value ?? "")}
          />
          <text>{`Value: ${JSON.stringify(text)}`}</text>
        </div>
      )
    }

    testRoot.render(<Textarea />)
    const textarea = testRoot.renderer.findByType("textarea")[0]
    testRoot.renderer.nativeSimulateKeystrokes(textarea.id, "left enter")

    expect(testRoot.renderer.getAllText()).toContain('Value: "a\\nb"')
  })

  it("replaces a textarea selection with a newline and undoes the edit", () => {
    function Textarea() {
      const [text, setText] = useState("abc")
      return (
        <div style={{ width: 400, height: 160 }}>
          <textarea
            value={text}
            style={{ width: 300 }}
            onChange={(event: EventPayload) => setText(event.value ?? "")}
          />
          <text>{`Value: ${JSON.stringify(text)}`}</text>
        </div>
      )
    }

    testRoot.render(<Textarea />)
    const textarea = testRoot.renderer.findByType("textarea")[0]
    testRoot.renderer.nativeSimulateKeystrokes(textarea.id, "shift-left enter")
    expect(testRoot.renderer.getAllText()).toContain('Value: "ab\\n"')

    testRoot.renderer.nativeSimulateKeystrokes(textarea.id, "cmd-z")
    expect(testRoot.renderer.getAllText()).toContain('Value: "abc"')
  })

  it("submits a textarea from an Enter keydown that prevents the newline", () => {
    function Textarea() {
      const [text, setText] = useState("")
      const [submits, setSubmits] = useState(0)
      return (
        <div style={{ width: 400, height: 160 }}>
          <textarea
            value={text}
            minRows={1}
            maxRows={4}
            style={{ width: 300 }}
            onChange={(event: EventPayload) => setText(event.value ?? "")}
            onKeyDown={(event: KeyEvent) => {
              if (event.key !== "Enter" || event.modifiers?.shift) return
              event.preventDefault()
              setSubmits((count) => count + 1)
            }}
          />
          <text>{`Value: ${JSON.stringify(text)}`}</text>
          <text>{`Submits: ${submits}`}</text>
        </div>
      )
    }

    testRoot.render(<Textarea />)
    const textarea = testRoot.renderer.findByType("textarea")[0]

    testRoot.renderer.nativeSimulateKeystrokes(textarea.id, "h i shift-enter t h e r e")
    expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
      [
        "Value: \"hi\\nthere\"",
        "Submits: 0",
      ]
    `)

    testRoot.renderer.nativeSimulateKeystrokes(textarea.id, "enter")
    expect(testRoot.renderer.getAllText()).toContain("Submits: 1")
    expect(testRoot.renderer.getAllText()).toContain('Value: "hi\\nthere"')
  })

  it("switches enter between newline and submit as the keydown handler changes", () => {
    function Textarea({ submit }: { submit?: boolean }) {
      const [text, setText] = useState("ab")
      const [submits, setSubmits] = useState(0)
      return (
        <div style={{ width: 400, height: 160 }}>
          <textarea
            value={text}
            style={{ width: 300 }}
            onChange={(event: EventPayload) => setText(event.value ?? "")}
            onKeyDown={
              submit
                ? (event: KeyEvent) => {
                    if (event.key !== "Enter") return
                    event.preventDefault()
                    setSubmits((count) => count + 1)
                  }
                : undefined
            }
          />
          <text>{`Value: ${JSON.stringify(text)}`}</text>
          <text>{`Submits: ${submits}`}</text>
        </div>
      )
    }

    testRoot.render(<Textarea />)
    const textarea = testRoot.renderer.findByType("textarea")[0]
    testRoot.renderer.nativeSimulateKeystrokes(textarea.id, "left enter")
    expect(testRoot.renderer.getAllText()).toContain('Value: "a\\nb"')
    expect(testRoot.renderer.getAllText()).toContain("Submits: 0")

    testRoot.render(<Textarea submit />)
    testRoot.renderer.nativeSimulateKeystrokes(textarea.id, "enter")
    expect(testRoot.renderer.getAllText()).toContain('Value: "a\\nb"')
    expect(testRoot.renderer.getAllText()).toContain("Submits: 1")

    testRoot.render(<Textarea />)
    testRoot.renderer.nativeSimulateKeystrokes(textarea.id, "enter")
    expect(testRoot.renderer.getAllText()).toContain('Value: "a\\n\\nb"')
    expect(testRoot.renderer.getAllText()).toContain("Submits: 1")
  })

  it("deletes to the start of the line with cmd-backspace", () => {
    function Textarea() {
      const [text, setText] = useState("keep\nhello world")
      return (
        <div style={{ width: 400, height: 160 }}>
          <textarea
            value={text}
            style={{ width: 300 }}
            onChange={(event: EventPayload) => setText(event.value ?? "")}
          />
          <text>{`Value: ${JSON.stringify(text)}`}</text>
        </div>
      )
    }

    testRoot.render(<Textarea />)
    const textarea = testRoot.renderer.findByType("textarea")[0]
    testRoot.renderer.nativeSimulateKeystrokes(textarea.id, "cmd-backspace")

    expect(testRoot.renderer.getAllText()).toContain('Value: "keep\\n"')
  })

  it("deletes to the end of the line with cmd-delete", () => {
    function Textarea() {
      const [text, setText] = useState("keep\nhello world")
      return (
        <div style={{ width: 400, height: 160 }}>
          <textarea
            value={text}
            style={{ width: 300 }}
            onChange={(event: EventPayload) => setText(event.value ?? "")}
          />
          <text>{`Value: ${JSON.stringify(text)}`}</text>
        </div>
      )
    }

    testRoot.render(<Textarea />)
    const textarea = testRoot.renderer.findByType("textarea")[0]
    testRoot.renderer.nativeSimulateKeystrokes(textarea.id, "cmd-left cmd-delete")

    expect(testRoot.renderer.getAllText()).toContain('Value: "keep\\n"')
  })

  it("deletes one complete grapheme", () => {
    function TextInput() {
      const [text, setText] = useState("A🙂")
      return (
        <div style={{ width: 400, height: 100 }}>
          <input
            value={text}
            style={{ width: 300, height: 40 }}
            onChange={(event: EventPayload) => setText(event.value ?? "")}
          />
          <text>{`Value: ${text}`}</text>
        </div>
      )
    }

    testRoot.render(<TextInput />)
    const input = testRoot.renderer.findByType("input")[0]
    testRoot.renderer.nativeSimulateKeystrokes(input.id, "backspace")

    expect(testRoot.renderer.getAllText()).toContain("Value: A")
  })

  it("moves the caret, replaces a selection, and undoes the edit", () => {
    function TextInput() {
      const [text, setText] = useState("ac")
      return (
        <div style={{ width: 400, height: 100 }}>
          <input
            value={text}
            style={{ width: 300, height: 40 }}
            onChange={(event: EventPayload) => setText(event.value ?? "")}
          />
          <text>{`Value: ${text}`}</text>
        </div>
      )
    }

    testRoot.render(<TextInput />)
    const input = testRoot.renderer.findByType("input")[0]
    testRoot.renderer.nativeSimulateKeystrokes(input.id, "left b shift-left X")
    expect(testRoot.renderer.getAllText()).toContain("Value: aXc")

    testRoot.renderer.nativeSimulateKeystrokes(input.id, "cmd-z")
    expect(testRoot.renderer.getAllText()).toContain("Value: abc")
  })

  it("undoes a contiguous typing run as one edit", () => {
    function TextInput() {
      const [text, setText] = useState("")
      return (
        <div style={{ width: 400, height: 100 }}>
          <input
            value={text}
            style={{ width: 300, height: 40 }}
            onChange={(event: EventPayload) => setText(event.value ?? "")}
          />
          <text>{`Value: ${text}`}</text>
        </div>
      )
    }

    testRoot.render(<TextInput />)
    const input = testRoot.renderer.findByType("input")[0]
    testRoot.renderer.nativeSimulateKeystrokes(input.id, "a b c cmd-z")

    expect(testRoot.renderer.getAllText()).toContain("Value: ")
    expect(testRoot.renderer.getAllText()).not.toContain("Value: ab")
  })

  it("does not coalesce typing after 700ms", () => {
    function TextInput() {
      const [text, setText] = useState("")
      return (
        <div style={{ width: 400, height: 100 }}>
          <input
            value={text}
            style={{ width: 300, height: 40 }}
            onChange={(event: EventPayload) => setText(event.value ?? "")}
          />
          <text>{`Value: ${text}`}</text>
        </div>
      )
    }

    testRoot.render(<TextInput />)
    const input = testRoot.renderer.findByType("input")[0]
    testRoot.renderer.nativeSimulateKeystrokes(input.id, "a")
    testRoot.renderer.advanceTime(800)
    testRoot.renderer.nativeSimulateKeystrokes(input.id, "b cmd-z")

    expect(testRoot.renderer.getAllText()).toContain("Value: a")
  })

  it("undoes contiguous backward deletion as one edit", () => {
    function TextInput() {
      const [text, setText] = useState("abcd")
      return (
        <div style={{ width: 400, height: 100 }}>
          <input
            value={text}
            style={{ width: 300, height: 40 }}
            onChange={(event: EventPayload) => setText(event.value ?? "")}
          />
          <text>{`Value: ${text}`}</text>
        </div>
      )
    }

    testRoot.render(<TextInput />)
    const input = testRoot.renderer.findByType("input")[0]
    testRoot.renderer.nativeSimulateKeystrokes(input.id, "backspace backspace cmd-z")

    expect(testRoot.renderer.getAllText()).toContain("Value: abcd")
  })

  it("undoes contiguous forward deletion as one edit", () => {
    function TextInput() {
      const [text, setText] = useState("abcd")
      return (
        <div style={{ width: 400, height: 100 }}>
          <input
            value={text}
            style={{ width: 300, height: 40 }}
            onChange={(event: EventPayload) => setText(event.value ?? "")}
          />
          <text>{`Value: ${text}`}</text>
        </div>
      )
    }

    testRoot.render(<TextInput />)
    const input = testRoot.renderer.findByType("input")[0]
    testRoot.renderer.nativeSimulateKeystrokes(input.id, "cmd-left delete delete cmd-z")

    expect(testRoot.renderer.getAllText()).toContain("Value: abcd")
  })

  // Native binds word motion to alt on macOS and to ctrl everywhere else, the
  // same split every platform's own text fields use, so the test has to ask
  // for the chord this host actually binds.
  it("moves by words with the platform's word chord", () => {
    const word = process.platform === "darwin" ? "alt" : "ctrl"
    function TextInput() {
      const [text, setText] = useState("hello world")
      return (
        <div style={{ width: 400, height: 100 }}>
          <input
            value={text}
            style={{ width: 300, height: 40 }}
            onChange={(event: EventPayload) => setText(event.value ?? "")}
          />
          <text>{`Value: ${text}`}</text>
        </div>
      )
    }

    testRoot.render(<TextInput />)
    const input = testRoot.renderer.findByType("input")[0]
    testRoot.renderer.nativeSimulateKeystrokes(
      input.id,
      `${word}-left X ${word}-right Y`,
    )

    expect(testRoot.renderer.getAllText()).toContain("Value: hello XworldY")
  })

  it("blocks editing when readOnly", () => {
    function TextInput() {
      const [text, setText] = useState("locked")
      return (
        <div style={{ width: 400, height: 100 }}>
          <input
            value={text}
            readOnly
            style={{ width: 300, height: 40 }}
            onChange={(event: EventPayload) => setText(event.value ?? "")}
          />
          <text>{`Value: ${text}`}</text>
        </div>
      )
    }

    testRoot.render(<TextInput />)
    const input = testRoot.renderer.findByType("input")[0]
    testRoot.renderer.nativeSimulateKeystrokes(input.id, "backspace a")

    expect(testRoot.renderer.getAllText()).toContain("Value: locked")
  })

  it("forwards the caret theme to the native editor", () => {
    testRoot.render(
      <input
        autoFocus
        value=""
        theme={{ caret: "#22c55e" }}
        style={{ width: 300, height: 40 }}
      />
    )

    const input = testRoot.renderer.findByType("input")[0]
    expect(input.customProps?.theme).toMatchInlineSnapshot(`
      {
        "caret": "#22c55e",
      }
    `)
  })

  it("applies external value changes", () => {
    function TextInput() {
      const [text, setText] = useState("draft")
      return (
        <div style={{ width: 400, height: 100 }}>
          <input
            value={text}
            placeholder="Empty"
            style={{ width: 300, height: 40 }}
            onChange={(event: EventPayload) => setText(event.value ?? "")}
            onKeyDown={(event: KeyEvent) => {
              if (event.key === "Enter") setText("")
            }}
          />
          <text>{`Value: ${text}`}</text>
        </div>
      )
    }

    testRoot.render(<TextInput />)
    const input = testRoot.renderer.findByType("input")[0]
    testRoot.renderer.nativeSimulateKeystrokes(input.id, "enter")

    expect(testRoot.renderer.getAllText()).toContain("Value: ")
    expect(testRoot.renderer.getPaintedText()).toContain("Empty")
  })

  it("focuses from a real mouse click", () => {
    function TextInput() {
      const [text, setText] = useState("")
      return (
        <div style={{ width: 400, height: 100 }}>
          <input
            value={text}
            style={{ width: 300, height: 40 }}
            onChange={(event: EventPayload) => setText(event.value ?? "")}
          />
          <text>{`Value: ${text}`}</text>
        </div>
      )
    }

    testRoot.render(<TextInput />)
    testRoot.renderer.nativeSimulateClick(250, 20)
    testRoot.renderer.simulateKeystrokes("a")

    expect(testRoot.renderer.getAllText()).toContain("Value: a")
  })

  it("keeps primary click and keyboard events available", () => {
    let click: EventPayload | undefined

    function TextInput() {
      const [clicks, setClicks] = useState(0)
      const [keys, setKeys] = useState(0)
      return (
        <div style={{ width: 400, height: 100 }}>
          <input
            value=""
            style={{ width: 300, height: 40 }}
            onClick={(event) => {
              click = event
              setClicks((count) => count + 1)
            }}
            onKeyDown={() => setKeys((count) => count + 1)}
          />
          <text>{`Events: ${clicks}/${keys}`}</text>
        </div>
      )
    }

    testRoot.render(<TextInput />)
    const input = testRoot.renderer.findByType("input")[0]
    testRoot.renderer.nativeSimulateMouseDown(150, 20, 0)
    testRoot.renderer.nativeSimulateMouseUp(150, 20, 0)
    testRoot.renderer.nativeSimulateKeyDown(input.id, "a")

    expect(testRoot.renderer.getAllText()).toContain("Events: 1/1")
    expect(click).toMatchObject({ button: 0, isRightClick: false })

    testRoot.renderer.nativeSimulateMouseDown(150, 20, 2)
    testRoot.renderer.nativeSimulateMouseUp(150, 20, 2)
    expect(testRoot.renderer.getAllText()).toContain("Events: 1/1")
  })

  it("propagates paste when the clipboard has no text", () => {
    const elementKeys: string[] = []
    const windowKeys: string[] = []

    for (const readOnly of [false, true]) {
      testRoot = createTestRoot({
        onKeyDown: (event) => windowKeys.push(event.key ?? ""),
      })
      testRoot.render(
        <input
          autoFocus
          readOnly={readOnly}
          value=""
          style={{ width: 300, height: 40 }}
          onKeyDown={(event) => elementKeys.push(event.key ?? "")}
        />,
      )
      const input = testRoot.renderer.findByType("input")[0]
      testRoot.renderer.nativeSimulateKeyDown(input.id, "cmd-v")
    }

    expect({ elementKeys, windowKeys }).toMatchInlineSnapshot(`
      {
        "elementKeys": [
          "v",
          "v",
        ],
        "windowKeys": [
          "v",
          "v",
        ],
      }
    `)
  })

  function editorBounds(type: "input" | "textarea") {
    const node = testRoot.renderer.findByType(type)[0]
    expect(node).toBeDefined()
    const bounds = testRoot.renderer.getElementBounds(node.id)
    expect(bounds).not.toBeNull()
    return bounds!
  }

  it("sizes a row from style.lineHeight", () => {
    testRoot.render(
      <textarea value="one" minRows={1} maxRows={8} style={{ width: 300, lineHeight: 30 }} />,
    )
    expect(editorBounds("textarea").height).toBe(30)
  })

  it("multiplies lineHeight by minRows", () => {
    testRoot.render(
      <textarea value="one" minRows={3} maxRows={8} style={{ width: 300, lineHeight: 30 }} />,
    )
    expect(editorBounds("textarea").height).toBe(90)
  })

  it("scales a row from fontSize when lineHeight is unset", () => {
    testRoot.render(<input value="one" style={{ width: 300, fontSize: 28 }} />)
    // GPUI default leading is phi, so 28px * 1.618 rounds to 45.
    expect(editorBounds("input").height).toBe(45)
  })

  it("uses lineHeight on a single-line input", () => {
    testRoot.render(<input value="one" style={{ width: 300, lineHeight: 22 }} />)
    expect(editorBounds("input").height).toBe(22)
  })
})
