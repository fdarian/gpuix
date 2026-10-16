/**
 * A `window` or `document` listener hears every event that reaches the top of
 * the tree, so native must produce it even when nothing on the path listens:
 * the pointer over bare background targets the root element, which has no
 * handler.
 */
import React from "react"
import { afterEach, beforeEach, describe, expect, it } from "vitest"
import { document, window } from "@gpuix/native/host"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("window and document pointer listeners", () => {
  let testRoot: ReturnType<typeof createTestRoot>

  beforeEach(() => {
    testRoot = createTestRoot()
    testRoot.render(<div style={{ width: "100%", height: "100%" }} />)
  })

  afterEach(() => {
    testRoot.unmount()
  })

  const overBackground = () => testRoot.renderer.nativeSimulateMouseMove(300, 300)

  it("hear a pointermove over bare background, until removed", () => {
    const seen: string[] = []
    const listener = (event) => seen.push(`${event.type}@${event.clientX},${event.clientY}`)
    window.addEventListener("pointermove", listener)
    overBackground()
    testRoot.renderer.nativeSimulateMouseMove(310, 300)
    expect(seen).toEqual(["pointermove@300,300", "pointermove@310,300"])

    window.removeEventListener("pointermove", listener)
    overBackground()
    expect(seen).toHaveLength(2)
  })

  it("hear a mousemove on document, in the capture phase too", () => {
    const seen: string[] = []
    const bubble = () => seen.push("bubble")
    const capture = () => seen.push("capture")
    document.addEventListener("mousemove", bubble)
    document.addEventListener("mousemove", capture, { capture: true })
    overBackground()
    expect(seen).toEqual(["capture", "bubble"])

    document.removeEventListener("mousemove", bubble)
    document.removeEventListener("mousemove", capture, { capture: true })
    overBackground()
    expect(seen).toHaveLength(2)
  })

  it("hear only the types they listen for", () => {
    const seen: string[] = []
    const listener = (event) => seen.push(event.type)
    window.addEventListener("mousedown", listener)
    overBackground()
    expect(seen).toEqual([])
    testRoot.renderer.nativeSimulateMouseDown(300, 300)
    expect(seen).toEqual(["mousedown"])
    window.removeEventListener("mousedown", listener)
  })
})
