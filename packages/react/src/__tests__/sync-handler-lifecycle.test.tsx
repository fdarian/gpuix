/**
 * Every renderer owns its sync key handler: disposing one test root leaves the
 * others' keys cancellable.
 */
// @ts-nocheck

import React, { useState } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

function Field(props: { onKeyDown: Function }) {
  const [text, setText] = useState("hello")
  return (
    <div style={{ width: 400, height: 80 }}>
      <input
        value={text}
        style={{ width: 300, height: 40 }}
        onKeyDown={props.onKeyDown}
        onChange={(event) => setText(event.value ?? "")}
      />
      <text>{`Value: ${JSON.stringify(text)}`}</text>
    </div>
  )
}

describeNative("sync key handler ownership", () => {
  it("keeps cancelling keys in one test root after another is unmounted", () => {
    const first = createTestRoot()
    first.render(<Field onKeyDown={() => {}} />)

    const second = createTestRoot()
    second.render(<Field onKeyDown={(event) => event.preventDefault()} />)
    const id = second.renderer.findByType("input")[0].id
    const value = () =>
      second.renderer.getAllText().find((line) => line.startsWith("Value: "))

    first.unmount()

    second.renderer.nativeSimulateKeystrokes(id, "x")
    expect(value()).toBe('Value: "hello"')
    second.unmount()
  })

  it("stops serving the window once the test root is disposed", () => {
    const root = createTestRoot()
    root.render(<Field onKeyDown={(event) => event.preventDefault()} />)
    const id = root.renderer.findByType("input")[0].id

    root.renderer.dispose()
    expect(() => root.renderer.nativeSimulateKeystrokes(id, "x")).toThrow(/disposed/)
  })
})
