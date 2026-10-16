/** Input method composition: key flags and the edits of a simulated input method. */
// @ts-nocheck

import React from "react"
import { beforeEach, describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("input method composition", () => {
  let events: any[]
  let preventKeyDown = false
  let renderer: ReturnType<typeof createTestRoot>["renderer"]
  let inputId: number

  beforeEach(() => {
    events = []
    preventKeyDown = false
    const testRoot = createTestRoot()
    renderer = testRoot.renderer
    const record = (event: any) => events.push(event)
    testRoot.render(
      <div style={{ width: 300, height: 100 }}>
        <input
          testId="field"
          style={{ width: 200, height: 30 }}
          onChange={record}
          onKeyDown={(event) => {
            record(event)
            if (preventKeyDown) event.preventDefault()
          }}
          onKeyUp={record}
        />
      </div>
    )
    inputId = renderer.findByTestId("field").id
  })

  const compose = (...steps: Parameters<typeof renderer.simulateComposition>[1]) =>
    renderer.simulateComposition(inputId, steps)

  it("edits the input with each step and says on keyup whether it is still composing", () => {
    compose(
      { key: "k", action: "mark", text: "k" },
      { key: "a", action: "mark", text: "か" },
      { key: "Enter", action: "commit", text: "か" }
    )
    expect(events.filter((event) => event.type === "change").map((event) => event.value)).toEqual([
      "k",
      "か",
      "か",
    ])
    expect(
      events.filter((event) => event.type === "keyup").map((event) => [event.key, event.isComposing])
    ).toEqual([
      ["k", true],
      ["a", true],
      ["Enter", false],
    ])
  })

  const keyDowns = () =>
    events
      .filter((event) => event.type === "keydown")
      .map((event) => [event.key, event.code, event.isComposing])

  it("a key the input method takes is a Process keydown, composing once a composition is open", () => {
    compose(
      { key: "k", action: "mark", text: "k" },
      { key: "a", action: "mark", text: "か" },
      { key: "Enter", action: "commit", text: "か" }
    )
    expect(keyDowns()).toEqual([
      ["Process", "KeyK", false],
      ["Process", "KeyA", true],
      ["Process", "Enter", true],
    ])
    expect(events.filter((event) => event.type === "keydown").map((event) => event.keyChar)).toEqual([
      undefined,
      undefined,
      undefined,
    ])
  })

  it("the keydown comes before the edit it causes and the keyup after", () => {
    compose({ key: "a", action: "mark", text: "あ" })
    expect(events.map((event) => event.type)).toEqual(["keydown", "change", "keyup"])
  })

  it("preventDefault on a Process keydown does not stop the input method", () => {
    preventKeyDown = true
    compose(
      { key: "a", action: "mark", text: "あ" },
      { key: "Enter", action: "commit", text: "あ" }
    )
    expect(keyDowns().map(([key]) => key)).toEqual(["Process", "Process"])
    expect(events.filter((event) => event.type === "change").map((event) => event.value)).toEqual([
      "あ",
      "あ",
    ])
    expect(renderer.getPaintedText()).toContain("あ")
  })

  it("a key typed after the composition ended is an ordinary keydown", () => {
    compose({ key: "a", action: "mark", text: "あ" }, { key: "Enter", action: "commit", text: "あ" })
    events.length = 0
    renderer.nativeSimulateKeystrokes(inputId, "x")
    expect(keyDowns()).toEqual([["x", "KeyX", false]])
  })

  it("a keyup with nothing marked is not composing", () => {
    renderer.nativeSimulateKeystrokes(inputId, "x")
    expect(events.filter((event) => event.type === "keyup").map((event) => event.isComposing)).toEqual([
      false,
    ])
  })

  it("unmarking keeps the text and ends the composition", () => {
    compose(
      { key: "a", action: "mark", text: "あ" },
      { key: "Tab", action: "unmark" }
    )
    expect(
      events.filter((event) => event.type === "keyup").map((event) => [event.key, event.isComposing])
    ).toEqual([
      ["a", true],
      ["Tab", false],
    ])
    expect(renderer.getPaintedText()).toContain("あ")
  })

  it("composing needs a focused text input", () => {
    const outside = () =>
      renderer.simulateComposition(renderer.findByType("div")[0].id, [
        { key: "a", action: "mark", text: "あ" },
      ])
    expect(outside).toThrow(/No text input is focused/)
  })
})
