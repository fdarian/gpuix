// The order in which one frame's reports reach the app is the DOM's order of the rendering steps: scroll events,
// then ResizeObserver callbacks (`onLayout` is delivered with them), then IntersectionObserver callbacks. Within a
// kind: scroll and `onLayout` events in document order; observer callbacks in the order the observers were created,
// and the entries of one observer in the order its targets were observed.
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { IntersectionObserver, ResizeObserver } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

function run() {
  const { render, renderer, unmount } = createTestRoot({ width: 400, height: 400 })
  const refs = new Map<string, PublicInstance>()
  const ref = (name: string) => (instance: PublicInstance | null) => {
    if (instance) refs.set(name, instance)
  }
  const log: string[] = []
  const names = (entries: { target: PublicInstance }[]) =>
    entries.map((entry) => [...refs].find(([, instance]) => instance === entry.target)![0]).join(" ")
  try {
    // Frames as a window presents them: the test renderer's inline draws would give each change a frame.
    renderer.setInlineDraws(false)
    const frame = () => renderer.drawFrame()
    const page = (changed: boolean) => (
      <div style={{ flexDirection: "column" }}>
        <div
          ref={ref("scroller")}
          testId="scroller"
          onScroll={() => log.push("scroll")}
          style={{ overflow: "auto", width: 100, height: 100 }}
        >
          <div style={{ width: 50, height: 1000 }} />
        </div>
        <div style={{ width: 20, height: changed ? 2000 : 0 }} />
        {["a", "b", "c"].map((name) => (
          <div
            key={name}
            ref={ref(name)}
            onLayout={() => log.push(`layout ${name}`)}
            style={{ width: changed ? 40 : 30, height: 20 }}
          />
        ))}
      </div>
    )
    render(page(false))
    // Created in this order: the first resize observer watches c then a, the second b; the first intersection
    // observer watches b, the second c then a.
    const roFirst = new ResizeObserver((entries) => log.push(`resize first ${names(entries)}`))
    const roSecond = new ResizeObserver((entries) => log.push(`resize second ${names(entries)}`))
    const ioFirst = new IntersectionObserver((entries) => log.push(`intersect first ${names(entries)}`))
    const ioSecond = new IntersectionObserver((entries) => log.push(`intersect second ${names(entries)}`))
    roFirst.observe(refs.get("c")!)
    roFirst.observe(refs.get("a")!)
    roSecond.observe(refs.get("b")!)
    ioFirst.observe(refs.get("b")!)
    ioSecond.observe(refs.get("c")!)
    ioSecond.observe(refs.get("a")!)
    for (let i = 0; i < 3; i++) {
      frame()
      renderer.dispatchNativeEvents()
    }

    log.length = 0
    render(page(true))
    renderer.scrollTo(refs.get("scroller")!.id, 0, 200)
    frame()
    renderer.dispatchNativeEvents()
    return log
  } finally {
    unmount()
  }
}

describeNative("the reports of one frame", () => {
  it("reach the app in the order of the DOM's rendering steps", () => {
    expect(run()).toEqual([
      "scroll",
      "layout a",
      "layout b",
      "layout c",
      "resize first c a",
      "resize second b",
      "intersect first b",
      "intersect second c a",
    ])
  })
})
