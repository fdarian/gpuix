/** The `inputType`, `data` and `isComposing` of the per-edit `onChange` event of <input> and <textarea>. */
// @ts-nocheck

import React, { useState } from "react"
import { beforeEach, describe, expect, it } from "vitest"
import { clipboard } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("input event", () => {
  let testRoot: ReturnType<typeof createTestRoot>
  let events: any[]
  let field: any

  const mount = (element: "input" | "textarea", initial = "", props = {}) => {
    events = []
    testRoot = createTestRoot()
    const Tag = element
    function Controlled() {
      const [text, setText] = useState(initial)
      return (
        <div style={{ width: 400, height: 200 }}>
          <Tag
            testId="field"
            ref={(node) => {
              if (node) field = node
            }}
            value={text}
            style={{ width: 300, height: 60 }}
            {...props}
            onChange={(event) => {
              events.push(event)
              props.onChange?.(event)
              setText(event.value)
            }}
          />
        </div>
      )
    }
    testRoot.render(<Controlled />)
  }

  const id = () => testRoot.renderer.findByTestId("field").id
  const press = (keystrokes: string) => testRoot.renderer.nativeSimulateKeystrokes(id(), keystrokes)
  const edits = () => events.map((event) => [event.inputType, event.data, event.value])

  beforeEach(() => {
    mount("input")
  })

  it("typing a character is insertText with the character as data", () => {
    press("a")
    expect(edits()).toEqual([["insertText", "a", "a"]])
    expect(events[0].isComposing).toBe(false)
  })

  it("typing over a selection is insertText", () => {
    mount("input", "hello")
    field.setSelectionRange(1, 4)
    press("x")
    expect(edits()).toEqual([["insertText", "x", "hxo"]])
  })

  it("data is the text that fit under maxLength", () => {
    mount("input", "", { maxLength: 3 })
    press("a b c d")
    expect(edits()).toEqual([
      ["insertText", "a", "a"],
      ["insertText", "b", "ab"],
      ["insertText", "c", "abc"],
    ])
  })

  it("Backspace is deleteContentBackward and Delete deleteContentForward, with null data", () => {
    mount("input", "abcd")
    field.setSelectionRange(2, 2)
    press("Backspace Delete")
    expect(edits()).toEqual([
      ["deleteContentBackward", null, "acd"],
      ["deleteContentForward", null, "ad"],
    ])
  })

  it("Alt+Backspace and Alt+Delete delete a word", () => {
    mount("input", "one two three")
    field.setSelectionRange(7, 7)
    press("Alt+Backspace Alt+Delete")
    expect(edits()).toEqual([
      ["deleteWordBackward", null, "one  three"],
      ["deleteWordForward", null, "one "],
    ])
  })

  it("Cmd+Backspace and Cmd+Delete delete to the line edge", () => {
    mount("input", "abcdef")
    field.setSelectionRange(2, 2)
    press("Meta+Backspace Meta+Delete")
    expect(edits()).toEqual([
      ["deleteSoftLineBackward", null, "cdef"],
      ["deleteSoftLineForward", null, ""],
    ])
  })

  it("a deletion with nothing to delete fires nothing", () => {
    press("Backspace Delete")
    expect(events).toEqual([])
  })

  it("a selection deleted with Backspace is deleteContentBackward", () => {
    mount("input", "hello")
    field.setSelectionRange(1, 4)
    press("Backspace")
    expect(edits()).toEqual([["deleteContentBackward", null, "ho"]])
  })

  it("cut is deleteByCut", () => {
    mount("input", "hello")
    field.setSelectionRange(0, 2)
    press("Meta+x")
    expect(edits()).toEqual([["deleteByCut", null, "llo"]])
  })

  it("paste is insertFromPaste with the pasted text as data", async () => {
    mount("input", "hello")
    await clipboard.writeText("pasted")
    field.setSelectionRange(5, 5)
    press("Meta+v")
    expect(edits()).toEqual([["insertFromPaste", "pasted", "hellopasted"]])
  })

  it("undo is historyUndo and redo historyRedo, with null data", () => {
    press("a b c")
    events.length = 0
    press("Meta+z")
    press("Meta+Shift+z")
    expect(edits()).toEqual([
      ["historyUndo", null, ""],
      ["historyRedo", null, "abc"],
    ])
    expect(events.map((event) => event.isComposing)).toEqual([false, false])
  })

  it("Enter in an <input> fires no input event", () => {
    mount("input", "a")
    press("Enter")
    expect(events).toEqual([])
  })

  it("Enter in a <textarea> is insertLineBreak with null data", () => {
    mount("textarea", "ab")
    field.setSelectionRange(1, 1)
    press("Enter")
    expect(edits()).toEqual([["insertLineBreak", null, "a\nb"]])
  })

  it("a pasted newline in a <textarea> is insertFromPaste with the whole text", async () => {
    mount("textarea")
    await clipboard.writeText("x\ny")
    press("Meta+v")
    expect(edits()).toEqual([["insertFromPaste", "x\ny", "x\ny"]])
  })

  it("setting the value from code fires no input event", () => {
    mount("input", "a")
    field.value = "b"
    press("Backspace")
    expect(edits()).toEqual([["deleteContentBackward", null, ""]])
  })

  describe("composition", () => {
    const compose = (...steps) => testRoot.renderer.simulateComposition(id(), steps)

    it("updates are insertCompositionText with the text so far, composing", () => {
      compose(
        { key: "k", action: "mark", text: "k" },
        { key: "a", action: "mark", text: "か" }
      )
      expect(edits()).toEqual([
        ["insertCompositionText", "k", "k"],
        ["insertCompositionText", "か", "か"],
      ])
      expect(events.map((event) => event.isComposing)).toEqual([true, true])
    })

    it("the commit is insertCompositionText, still composing, before compositionend", () => {
      const order: string[] = []
      mount("input", "", {
        onChange: (event) => {
          order.push(`${event.type} ${event.inputType} ${event.isComposing}`)
        },
        onCompositionEnd: (event) => order.push(`${event.type} ${JSON.stringify(event.data)}`),
      })
      compose(
        { key: "a", action: "mark", text: "あ" },
        { key: "Enter", action: "commit", text: "あ" }
      )
      expect(order).toEqual([
        "change insertCompositionText true",
        "change insertCompositionText true",
        'compositionend "あ"',
      ])
      expect(events[1].data).toBe("あ")
    })

    it("cancelling the composition is deleteCompositionText with null data, before compositionend", () => {
      const order: string[] = []
      mount("input", "", {
        onChange: (event) => {
          order.push(`${event.type} ${event.inputType} ${event.data} ${event.isComposing}`)
        },
        onCompositionEnd: (event) => order.push(event.type),
      })
      compose(
        { key: "a", action: "mark", text: "あ" },
        { key: "Escape", action: "mark", text: "" }
      )
      expect(order).toEqual([
        "change insertCompositionText あ true",
        "change deleteCompositionText null true",
        "compositionend",
      ])
    })

    it("typing after the composition is a plain insertText again", () => {
      compose(
        { key: "a", action: "mark", text: "あ" },
        { key: "Enter", action: "commit", text: "あ" }
      )
      events.length = 0
      press("x")
      expect(edits()).toEqual([["insertText", "x", "あx"]])
      expect(events[0].isComposing).toBe(false)
    })
  })

  describe("number stepper", () => {
    it("reports an empty inputType", () => {
      mount("input", "1", { type: "number" })
      press("ArrowUp")
      expect(edits()).toEqual([["", null, "2"]])
    })
  })
})
