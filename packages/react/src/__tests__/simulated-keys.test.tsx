/** The test renderer's key helpers speak Playwright's `keyboard.press` grammar. */
// @ts-nocheck

import React, { useState } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const box = { width: 40, height: 20 }

describeNative("simulated keystrokes", () => {
  it("moves focus back on Shift+Tab", () => {
    const testRoot = createTestRoot()
    testRoot.render(
      <div style={{ width: 200, height: 100 }}>
        <div testId="a" tabIndex={0} style={box} />
        <div testId="b" tabIndex={0} style={box} />
      </div>
    )
    const renderer = testRoot.renderer
    const focused = () => renderer.getElement(renderer.getFocusedElementId()).testId
    renderer.focusElement(renderer.findByTestId("b").id)
    renderer.simulateKeystrokes("Shift+Tab")
    expect(focused()).toBe("a")
    renderer.simulateKeystrokes("Tab")
    expect(focused()).toBe("b")
  })

  it("sends keydown then keyup for a press", () => {
    const events: any[] = []
    const testRoot = createTestRoot()
    testRoot.render(
      <div style={{ width: 200, height: 100 }}>
        <div
          testId="target"
          tabIndex={0}
          style={box}
          onKeyDown={(event) => events.push(event)}
          onKeyUp={(event) => events.push(event)}
        />
      </div>
    )
    testRoot.renderer.nativeSimulateKeystrokes(testRoot.renderer.findByTestId("target").id, "Enter")
    expect(events.map((event) => [event.type, event.key])).toEqual([
      ["keydown", "Enter"],
      ["keyup", "Enter"],
    ])
  })

  it("types characters into an input", () => {
    function TextInput() {
      const [text, setText] = useState("")
      return (
        <div style={{ width: 400, height: 100 }}>
          <input value={text} style={{ width: 300, height: 40 }} onChange={(event) => setText(event.value)} />
          <text>{`Value: ${text}`}</text>
        </div>
      )
    }
    const testRoot = createTestRoot()
    testRoot.render(<TextInput />)
    const renderer = testRoot.renderer
    renderer.nativeSimulateKeystrokes(renderer.findByType("input")[0].id, "a Shift+b")
    expect(renderer.getAllText()).toEqual(["Value: aB"])
    renderer.simulateKeystrokes("C Space KeyD Digit1")
    expect(renderer.getAllText()).toEqual(["Value: aBC d1"])
  })

  it("throws for an unknown key and presses nothing", () => {
    const events: any[] = []
    const testRoot = createTestRoot()
    testRoot.render(
      <div style={{ width: 200, height: 100 }}>
        <div testId="target" tabIndex={0} style={box} onKeyDown={(event) => events.push(event.key)} />
      </div>
    )
    const renderer = testRoot.renderer
    renderer.focusElement(renderer.findByTestId("target").id)
    expect(() => renderer.simulateKeystrokes("a enter")).toThrow(/Unknown key "enter"/)
    expect(() => renderer.simulateKeystrokes("cmd-shift-p")).toThrow(/Unknown key/)
    expect(() => renderer.nativeSimulateKeyDown(renderer.findByTestId("target").id, "Bogus")).toThrow(
      /Unknown key "Bogus"/
    )
    expect(events).toEqual([])
  })

  it("sends a lone keydown and keyup", () => {
    const types: string[] = []
    const testRoot = createTestRoot()
    testRoot.render(
      <div style={{ width: 200, height: 100 }}>
        <div
          testId="target"
          tabIndex={0}
          style={box}
          onKeyDown={(event) => types.push(`${event.type}:${event.key}`)}
          onKeyUp={(event) => types.push(`${event.type}:${event.key}`)}
        />
      </div>
    )
    const id = testRoot.renderer.findByTestId("target").id
    testRoot.renderer.nativeSimulateKeyDown(id, "Meta+s")
    testRoot.renderer.nativeSimulateKeyUp(id, "Meta+s")
    expect(types).toEqual(["keydown:s", "keyup:s"])
  })
})
