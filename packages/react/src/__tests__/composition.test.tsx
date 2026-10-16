/** Input method composition: key flags and the edits of a simulated input method. */
// @ts-nocheck

import React from "react"
import { beforeEach, describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("input method composition", () => {
  let events: any[]
  let renderer: ReturnType<typeof createTestRoot>["renderer"]
  let inputId: number

  beforeEach(() => {
    events = []
    const testRoot = createTestRoot()
    renderer = testRoot.renderer
    const record = (event: any) => events.push(event)
    testRoot.render(
      <div style={{ width: 300, height: 100 }}>
        <input
          testId="field"
          style={{ width: 200, height: 30 }}
          onChange={record}
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
