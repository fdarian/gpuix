/** Keyboard events carry DOM `KeyboardEvent` values, not GPUI key names. */
// @ts-nocheck

import React from "react"
import { beforeEach, describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("DOM-shaped key events", () => {
  let events: any[]
  let press: (keystrokes: string) => void
  let renderer: ReturnType<typeof createTestRoot>["renderer"]

  beforeEach(() => {
    events = []
    const testRoot = createTestRoot()
    renderer = testRoot.renderer
    testRoot.render(
      <div style={{ width: 200, height: 100 }}>
        <div
          testId="target"
          tabIndex={0}
          style={{ width: 40, height: 20 }}
          onKeyDown={(event) => events.push(event)}
          onKeyUp={(event) => events.push(event)}
        />
      </div>
    )
    const id = renderer.findByTestId("target").id
    press = (keystrokes) => renderer.nativeSimulateKeystrokes(id, keystrokes)
  })

  /** `[key, code]` of each keydown, in order. */
  const keys = () =>
    events.filter((event) => event.type === "keydown").map((event) => [event.key, event.code])

  it("names named keys the way the DOM does", () => {
    press("Enter Escape Tab Backspace Delete Space ArrowDown ArrowLeft Home End PageUp F5")
    expect(keys()).toEqual([
      ["Enter", "Enter"],
      ["Escape", "Escape"],
      ["Tab", "Tab"],
      ["Backspace", "Backspace"],
      ["Delete", "Delete"],
      [" ", "Space"],
      ["ArrowDown", "ArrowDown"],
      ["ArrowLeft", "ArrowLeft"],
      ["Home", "Home"],
      ["End", "End"],
      ["PageUp", "PageUp"],
      ["F5", "F5"],
    ])
  })

  it("reports the produced character as key and the physical key as code", () => {
    press("a Shift+a 1 ;")
    // A raw key event has no typed character, so shift alone decides the symbol.
    renderer.nativeSimulateKeyDown(renderer.findByTestId("target").id, "Shift+1")
    expect(keys()).toEqual([
      ["a", "KeyA"],
      ["A", "KeyA"],
      ["1", "Digit1"],
      [";", "Semicolon"],
      ["!", "Digit1"],
    ])
  })

  it("sets the modifier booleans next to modifiers", () => {
    press("Meta+s")
    const [down] = events
    expect(down.key).toBe("s")
    expect(down.metaKey).toBe(true)
    expect(down.modifiers.cmd).toBe(true)
    expect([down.shiftKey, down.ctrlKey, down.altKey]).toEqual([false, false, false])

    events.length = 0
    press("Control+Alt+Shift+x")
    expect([events[0].shiftKey, events[0].ctrlKey, events[0].altKey, events[0].metaKey]).toEqual([
      true,
      true,
      true,
      false,
    ])
  })

  it("reports repeat and isComposing", () => {
    const id = renderer.findByTestId("target").id
    renderer.nativeSimulateKeystrokes(id, "x")
    renderer.nativeSimulateKeyDown(id, "x", true)
    renderer.nativeSimulateKeyUp(id, "x")
    const downs = events.filter((event) => event.type === "keydown")
    expect(downs.map((event) => event.repeat)).toEqual([false, true])
    expect(events.every((event) => event.isComposing === false)).toBe(true)
    expect(events.find((event) => event.type === "keyup").repeat).toBe(false)
  })
})
