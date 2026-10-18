import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("scrollTo on a scroller under display: none", () => {
  // An element with no box has nothing to scroll: the DOM ignores the call, whichever the behavior.
  for (const [name, style, behavior] of [
    ["a smooth scroller", { scrollBehavior: "smooth" }, undefined],
    ["an instant scroller", {}, undefined],
    ["an explicit smooth scroll", {}, "smooth"],
    ["an explicit instant scroll", { scrollBehavior: "smooth" }, "instant"],
  ] as const) {
    it(`does nothing for ${name}`, () => {
      const page = (hidden: boolean) => (
        <div style={{ display: hidden ? "none" : "block" }}>
          <div testId="scroller" style={{ width: 200, height: 100, overflow: "auto", ...style }}>
            <div style={{ height: 1000 }} />
          </div>
        </div>
      )
      const { render, renderer, unmount } = createTestRoot()
      try {
        render(page(false))
        const id = renderer.findByTestId("scroller")!.id
        renderer.clockPause()
        render(page(true))
        renderer.flush()
        renderer.scrollTo(id, 0, 300, behavior)
        render(page(false))
        renderer.flush()
        for (const wait of [0, 100, 1000]) {
          renderer.clockFastForward(wait)
          renderer.flush()
          expect(renderer.getScrollOffset(id)![1]).toBe(0)
        }
      } finally {
        unmount()
      }
    })
  }
})
