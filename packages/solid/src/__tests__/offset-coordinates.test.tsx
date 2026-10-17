/**
 * `offsetX` and `offsetY` through the public handlers: the pointer relative to the padding edge
 * of the event's target, as the DOM reports them. A handler on an ancestor reads the same numbers
 * as the target's own, because the offset is the target's, not `currentTarget`'s.
 */

import { describe, expect, it } from "bun:test"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

type Offset = { type: string; offsetX: number | undefined; offsetY: number | undefined }
type Handled = { offsetX?: number; offsetY?: number }

function record(seen: Offset[], type: string) {
  return (event: Handled) => seen.push({ type, offsetX: event.offsetX, offsetY: event.offsetY })
}

/** A 4px border and 10px padding around the target: its padding edge is 4px inside. */
const TARGET = {
  position: "absolute",
  left: 100,
  top: 80,
  width: 200,
  height: 120,
  borderWidth: 4,
  borderStyle: "solid",
  borderColor: "#ffffff",
  padding: 10,
} as const

describe.skipIf(!hasNativeTestRenderer)("Solid offsetX and offsetY", () => {
  it("are filled on a bordered, padded target by pointer, mouse and click handlers", () => {
    const seen: Offset[] = []
    const app = createTestRoot()
    app.render(() => (
      <div style={{ width: 600, height: 400 }}>
        <div
          style={TARGET}
          onPointerMove={record(seen, "pointermove")}
          onMouseMove={record(seen, "mousemove")}
          onPointerDown={record(seen, "pointerdown")}
          onClick={record(seen, "click")}
        />
      </div>
    ))

    app.renderer.nativeSimulateMouseMove(140, 110)
    app.renderer.nativeSimulateClick(140, 110)

    // From the padding edge: 140 - 100 - 4 and 110 - 80 - 4.
    expect(seen.map((entry) => entry.type)).toEqual(
      expect.arrayContaining(["pointermove", "mousemove", "pointerdown", "click"])
    )
    for (const entry of seen) expect(entry).toMatchObject({ offsetX: 36, offsetY: 26 })
  })

  it("are the target's on a child, for the child's handler and its ancestor's", () => {
    const child: Offset[] = []
    const parent: Offset[] = []
    const app = createTestRoot()
    app.render(() => (
      <div style={{ width: 600, height: 400 }}>
        <div
          style={TARGET}
          onPointerMove={record(parent, "pointermove")}
          onMouseMove={record(parent, "mousemove")}
          onClick={record(parent, "click")}
        >
          <div
            style={{ width: 80, height: 50, borderWidth: 2, borderStyle: "solid", borderColor: "#ffffff" }}
            onPointerMove={record(child, "pointermove")}
            onMouseMove={record(child, "mousemove")}
            onClick={record(child, "click")}
          />
        </div>
      </div>
    ))

    // The child's border box starts at (114, 94): its padding edge is 2 in.
    app.renderer.nativeSimulateMouseMove(130, 112)
    app.renderer.nativeSimulateClick(130, 112)

    expect(child.length).toBeGreaterThanOrEqual(3)
    for (const entry of child) expect(entry).toMatchObject({ offsetX: 14, offsetY: 16 })
    expect(parent.length).toBeGreaterThanOrEqual(3)
    for (const entry of parent) expect(entry).toMatchObject({ offsetX: 14, offsetY: 16 })
  })
})
