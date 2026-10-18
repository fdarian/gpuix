// An observed element that is removed is reported gone by the next presented frame, as the DOM does at its next
// rendering step. A read of layout in between (a bounds read lays out and does not present) delivers nothing, as
// forcing layout does not run the rendering steps.
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { IntersectionObserver, ResizeObserver } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("an observed element that is removed", () => {
  it("is reported by the next presented frame, not by a read of layout", () => {
    const { render, renderer, unmount } = createTestRoot({ width: 400, height: 400 })
    const refs = new Map<string, PublicInstance>()
    const ref = (name: string) => (instance: PublicInstance | null) => {
      if (instance) refs.set(name, instance)
    }
    const log: string[] = []
    try {
      renderer.setInlineDraws(false)
      const page = (withTarget: boolean) => (
        <div style={{ flexDirection: "column" }}>
          <div ref={ref("other")} style={{ width: 10, height: 10 }} />
          {withTarget && <div ref={ref("target")} style={{ width: 30, height: 20 }} />}
        </div>
      )
      render(page(true))
      new ResizeObserver((entries) => log.push(`resize ${entries[0]!.contentRect.width}x${entries[0]!.contentRect.height}`)).observe(
        refs.get("target")!
      )
      new IntersectionObserver((entries) => log.push(`intersect ${entries[0]!.isIntersecting}`)).observe(refs.get("target")!)
      for (let i = 0; i < 3; i++) {
        renderer.drawFrame()
        renderer.dispatchNativeEvents()
      }
      expect(log).toEqual(["resize 30x20", "intersect true"])

      log.length = 0
      render(page(false))
      renderer.getElementBounds(refs.get("other")!.id)
      renderer.dispatchNativeEvents()
      expect(log, "a read of layout does not run the rendering steps").toEqual([])

      renderer.drawFrame()
      renderer.dispatchNativeEvents()
      expect(log).toEqual(["resize 0x0", "intersect false"])
    } finally {
      unmount()
    }
  })
})
