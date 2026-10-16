/**
 * A `window` or `document` listener hears every event that reaches the top of
 * the tree, so native must produce it even when nothing on the path listens:
 * the pointer over bare background targets the root element, which has no
 * handler.
 */
import { afterEach, beforeEach, describe, expect, it } from "bun:test"
import { document, window } from "@gpuix/native/host"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

describe.skipIf(!hasNativeTestRenderer)("Solid window and document pointer listeners", () => {
  let app: ReturnType<typeof createTestRoot>

  beforeEach(() => {
    app = createTestRoot()
    app.render(() => <div style={{ width: "100%", height: "100%" }} />)
  })

  afterEach(() => {
    app.unmount()
  })

  const overBackground = () => app.renderer.nativeSimulateMouseMove(300, 300)

  it("hear a pointermove over bare background, until removed", () => {
    const seen: string[] = []
    const listener = (event: any) => seen.push(`${event.type}@${event.clientX},${event.clientY}`)
    window.addEventListener("pointermove", listener)
    overBackground()
    app.renderer.nativeSimulateMouseMove(310, 300)
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
    const listener = (event: any) => seen.push(event.type)
    window.addEventListener("mousedown", listener)
    overBackground()
    expect(seen).toEqual([])
    app.renderer.nativeSimulateMouseDown(300, 300)
    expect(seen).toEqual(["mousedown"])
    window.removeEventListener("mousedown", listener)
  })
})
