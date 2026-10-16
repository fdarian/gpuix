/**
 * A `window` or `document` listener hears every event that reaches the top of
 * the tree, so native must produce it even when nothing on the path listens.
 * The pointer over bare background targets the root element, and a move inside
 * the window but outside the root's box has no target at all: it is aimed at
 * the window, as a key with nothing focused is.
 */
import { afterEach, beforeEach, describe, expect, it } from "bun:test"
import { document, window } from "@gpuix/native/host"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

describe.skipIf(!hasNativeTestRenderer)("Solid window and document pointer listeners", () => {
  let app: ReturnType<typeof createTestRoot>
  let rootMoves: string[]

  beforeEach(() => {
    rootMoves = []
    app = createTestRoot()
    app.render(() => <div style={{ width: 200, height: 200 }} onPointerMove={() => rootMoves.push("root")} />)
  })

  afterEach(() => {
    app.unmount()
  })

  const overRootBackground = () => app.renderer.nativeSimulateMouseMove(100, 100)
  const outsideRoot = () => app.renderer.nativeSimulateMouseMove(600, 500)

  it("hear a pointermove over bare background, until removed", () => {
    const seen: string[] = []
    const listener = (event: any) => seen.push(`${event.type}@${event.clientX},${event.clientY}`)
    window.addEventListener("pointermove", listener)
    overRootBackground()
    app.renderer.nativeSimulateMouseMove(110, 100)
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
    const listener = (event: any) => seen.push({ type: event.type, target: event.target, path: event.path })
    window.addEventListener("pointermove", listener)
    outsideRoot()
    expect(seen).toEqual([{ type: "pointermove", target: undefined, path: undefined }])
    expect(rootMoves).toEqual([])

    window.removeEventListener("pointermove", listener)
    outsideRoot()
    expect(seen).toHaveLength(1)
  })

  it("hear a document mousemove outside the root, and the root hears nothing", () => {
    const seen: string[] = []
    const listener = (event: any) => seen.push(`${event.type}:${event.target}`)
    document.addEventListener("mousemove", listener)
    outsideRoot()
    expect(seen).toEqual(["mousemove:undefined"])
    expect(rootMoves).toEqual([])
    document.removeEventListener("mousemove", listener)
  })

  it("hear only the types they listen for", () => {
    const seen: string[] = []
    const listener = (event: any) => seen.push(event.type)
    window.addEventListener("mousedown", listener)
    overRootBackground()
    expect(seen).toEqual([])
    app.renderer.nativeSimulateMouseDown(100, 100)
    expect(seen).toEqual(["mousedown"])
    window.removeEventListener("mousedown", listener)
  })
})
