/**
 * The clipboard API against the native test window. That window has its own
 * in-memory clipboard, so nothing here touches the system pasteboard.
 */
// @ts-nocheck

import React, { useState } from "react"
import { beforeEach, describe, expect, it } from "vitest"
import type { EventPayload } from "@gpuix/native"
import { clipboard } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip
const paste = "ControlOrMeta+v"
const copy = "ControlOrMeta+c"
const selectAll = "ControlOrMeta+a"

describeNative("clipboard", () => {
  let testRoot: ReturnType<typeof createTestRoot>

  beforeEach(() => {
    testRoot = createTestRoot()
  })

  it("is installed as navigator.clipboard and starts empty", async () => {
    expect(navigator.clipboard).toBe(clipboard)
    expect(await navigator.clipboard.readText()).toBe("")
  })

  it("round-trips text, including non-ASCII", async () => {
    await clipboard.writeText("héllo 🌍")
    expect(await clipboard.readText()).toBe("héllo 🌍")
    await clipboard.writeText("")
    expect(await clipboard.readText()).toBe("")
  })

  function Field() {
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

  it("is what an <input> pastes", async () => {
    await clipboard.writeText("pasted")
    testRoot.render(<Field />)
    const input = testRoot.renderer.findByType("input")[0]
    testRoot.renderer.nativeSimulateKeyDown(input.id, paste)
    expect(testRoot.renderer.getAllText()).toContain("Value: pasted")
  })

  it("is what an <input> copies to", async () => {
    testRoot.render(<Field />)
    const input = testRoot.renderer.findByType("input")[0]
    testRoot.renderer.nativeSimulateKeystrokes(input.id, "a b c")
    testRoot.renderer.nativeSimulateKeyDown(input.id, selectAll)
    testRoot.renderer.nativeSimulateKeyDown(input.id, copy)
    expect(await clipboard.readText()).toBe("abc")
  })
})
