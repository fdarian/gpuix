/**
 * A `window` or `document` listener hears every event that reaches the top of
 * the tree, so native must produce it even when nothing on the path listens.
 * The pointer over bare background targets the root element, and a move inside
 * the window but outside the root's box has no target at all: it is aimed at
 * the window, as a key with nothing focused is.
 */
import React from "react"
import { afterEach, beforeEach, describe, expect, it } from "vitest"
import { document, window } from "@ghia/native/host"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("window and document pointer listeners", () => {
  let testRoot: ReturnType<typeof createTestRoot>
  let rootMoves: string[]

  beforeEach(() => {
    rootMoves = []
    testRoot = createTestRoot()
    testRoot.render(<div style={{ width: 200, height: 200 }} onPointerMove={() => rootMoves.push("root")} />)
  })

  afterEach(() => {
    testRoot.unmount()
  })

  const overRootBackground = () => testRoot.renderer.nativeSimulateMouseMove(100, 100)
  const outsideRoot = () => testRoot.renderer.nativeSimulateMouseMove(600, 500)

  it("hear a pointermove over bare background, until removed", () => {
    const seen: string[] = []
    const listener = (event) => seen.push(`${event.type}@${event.clientX},${event.clientY}`)
    window.addEventListener("pointermove", listener)
    overRootBackground()
    testRoot.renderer.nativeSimulateMouseMove(110, 100)
    expect(seen).toEqual(["pointermove@100,100", "pointermove@110,100"])

    window.removeEventListener("pointermove", listener)
    overRootBackground()
    expect(seen).toHaveLength(2)
  })

  it("hear a mousemove on document, in the capture phase too", () => {
    const seen: string[] = []
    const bubble = () => seen.push("bubble")
    const capture = () => seen.push("capture")
    document.addEventListener("mousemove", bubble)
    document.addEventListener("mousemove", capture, { capture: true })
    overRootBackground()
    expect(seen).toEqual(["capture", "bubble"])

    document.removeEventListener("mousemove", bubble)
    document.removeEventListener("mousemove", capture, { capture: true })
    overRootBackground()
    expect(seen).toHaveLength(2)
  })

  it("hear a move inside the window but outside the root, with no target", () => {
    const seen: Array<{ type: string; target: unknown; path: unknown }> = []
    const listener = (event) => seen.push({ type: event.type, target: event.target, path: event.composedPath() })
    window.addEventListener("pointermove", listener)
    outsideRoot()
    expect(seen).toEqual([{ type: "pointermove", target: null, path: [] }])
    expect(rootMoves).toEqual([])

    window.removeEventListener("pointermove", listener)
    outsideRoot()
    expect(seen).toHaveLength(1)
  })

  it("hear a document mousemove outside the root, and the root hears nothing", () => {
    const seen: string[] = []
    const listener = (event) => seen.push(`${event.type}:${event.target}`)
    document.addEventListener("mousemove", listener)
    outsideRoot()
    expect(seen).toEqual(["mousemove:null"])
    expect(rootMoves).toEqual([])
    document.removeEventListener("mousemove", listener)
  })

  it("hear only the types they listen for", () => {
    const seen: string[] = []
    const listener = (event) => seen.push(event.type)
    window.addEventListener("mousedown", listener)
    overRootBackground()
    expect(seen).toEqual([])
    testRoot.renderer.nativeSimulateMouseDown(100, 100)
    expect(seen).toEqual(["mousedown"])
    window.removeEventListener("mousedown", listener)
  })
})
