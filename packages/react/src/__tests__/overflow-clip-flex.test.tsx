/// `overflow: clip` clips and nothing more: a box with it on both axes is no scroll container, so as a flex item it keeps
/// the content-based minimum size it has with `overflow: visible`, where `hidden` and `scroll` let it shrink to nothing.
/// Heights are headless Chrome's for the same page (30 items of 37px in a column of 780px).

import type { CSSProperties } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const HEIGHT = 780
const ITEMS = 30

function Column({ overflow }: { overflow: string }) {
  return (
    <div style={{ width: 900, height: HEIGHT, display: "flex", flexDirection: "column" }}>
      {Array.from({ length: ITEMS }, (_, index) => (
        <div
          key={index}
          testId={`item-${index}`}
          style={{ display: "flex", flexDirection: "column", padding: 8, gap: 4, overflow } as CSSProperties}
        >
          <div style={{ height: 16, flexShrink: 0 }} />
          <div style={{ height: 1, flexShrink: 0 }} />
        </div>
      ))}
    </div>
  )
}

describeNative("overflow: clip on a flex item", () => {
  it.each([
    ["visible", 37],
    ["clip", 37],
    ["hidden", HEIGHT / ITEMS],
    ["scroll", HEIGHT / ITEMS],
  ])("%s: an item of a column that is too short for them is %d px", (overflow, expected) => {
    const root = createTestRoot({ width: 900, height: HEIGHT })
    try {
      root.render(<Column overflow={overflow} />)
      root.renderer.drawFrame()
      const item = root.renderer.findByTestId("item-0")!
      expect(root.renderer.getElementBounds(item.id)!.height).toBeCloseTo(expected, 1)
    } finally {
      root.unmount()
    }
  })
})
