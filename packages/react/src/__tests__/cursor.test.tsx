/// `renderer.getCursor()`: the cursor the window asks the OS for where the pointer is.

import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("getCursor", () => {
  it("follows the pointer over elements and is the arrow elsewhere", () => {
    const testRoot = createTestRoot()
    testRoot.render(
      <div style={{ width: 200, height: 100, flexDirection: "row" }}>
        <div testId="link" style={{ width: 100, height: 100, cursor: "pointer" }} />
        <div testId="resize" style={{ width: 100, height: 100, cursor: "ew-resize" }} />
      </div>
    )
    const { renderer } = testRoot
    const middle = (testId: string) => {
      const box = renderer.getElementBounds(renderer.findByTestId(testId)!.id)!
      return { x: box.x + box.width / 2, y: box.y + box.height / 2 }
    }
    expect(renderer.getCursor()).toBe("default")
    renderer.nativeSimulateMouseMove(middle("link").x, middle("link").y)
    expect(renderer.getCursor()).toBe("pointer")
    renderer.nativeSimulateMouseMove(middle("resize").x, middle("resize").y)
    expect(renderer.getCursor()).toBe("ew-resize")
    renderer.nativeSimulateMouseMove(390, 390)
    expect(renderer.getCursor()).toBe("default")
    testRoot.unmount()
  })
})
