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

  it("a press that names a code reports that code, and the US character on it", () => {
    press("KeyA Shift+KeyA Digit1 Shift+Digit1 Control+KeyC ShiftLeft NumpadEnter Enter Numpad1 NumpadAdd")
    expect(keys()).toEqual([
      ["a", "KeyA"],
      ["A", "KeyA"],
      ["1", "Digit1"],
      ["!", "Digit1"],
      ["c", "KeyC"],
      ["Shift", "ShiftLeft"],
      ["Enter", "NumpadEnter"],
      ["Enter", "Enter"],
      ["1", "Numpad1"],
      ["+", "NumpadAdd"],
    ])
  })

  it("keyup carries the code of the key that went down", () => {
    press("Shift+KeyQ")
    expect(events.map((event) => [event.type, event.key, event.code])).toEqual([
      ["keydown", "Q", "KeyQ"],
      ["keyup", "Q", "KeyQ"],
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

  /** `[keyCode, which]` of each keydown, in order. */
  const legacyCodes = () =>
    events.filter((event) => event.type === "keydown").map((event) => [event.keyCode, event.which])

  it("carries Chrome's legacy keyCode and which, equal, on keydown and keyup", () => {
    press("Enter KeyA a Shift+a 1 Shift+1 Space Tab Escape Backspace Delete ArrowLeft ArrowDown F1 F12")
    expect(legacyCodes()).toEqual(
      [13, 65, 65, 65, 49, 49, 32, 9, 27, 8, 46, 37, 40, 112, 123].map((code) => [code, code])
    )
    const keyups = events.filter((event) => event.type === "keyup")
    expect(keyups.map((event) => [event.keyCode, event.which])).toEqual(legacyCodes())
  })

  it("uses the OEM numbers for punctuation and the 96-111 range for the keypad", () => {
    press("; = , - . / ` [ \\ ] ' Numpad0 Numpad9 NumpadMultiply NumpadAdd NumpadSubtract NumpadDecimal NumpadDivide NumpadEnter")
    expect(legacyCodes().map(([code]) => code)).toEqual([
      186, 187, 188, 189, 190, 191, 192, 219, 220, 221, 222, 96, 105, 106, 107, 109, 110, 111, 13,
    ])
  })

  it("tells the left Command key (91) from the right one (93) and numbers the other modifiers", () => {
    press("ShiftLeft ControlRight AltLeft MetaLeft MetaRight CapsLock Shift Meta")
    expect(legacyCodes().map(([code]) => code)).toEqual([16, 17, 18, 91, 93, 20, 16, 91])
  })

  it("keeps the code of the key with a modifier held", () => {
    press("Shift+a Control+c Meta+Shift+KeyC")
    expect(legacyCodes().map(([code]) => code)).toEqual([65, 67, 67])
    expect(events[0].key).toBe("A")
  })
})
