// The order in which one frame's reports reach the app is the DOM's order of the rendering steps: scroll events,
// then ResizeObserver callbacks (`onLayout` is delivered with them), then IntersectionObserver callbacks. Within a
// kind: scroll and `onLayout` events in document order; observer callbacks in the order the observers were created,
// and the entries of one observer in the order its targets were observed.
import fs from "fs"
import path from "path"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { IntersectionObserver, ResizeObserver } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

function run(scenario: "scroll then change" | "write while dirty" = "scroll then change") {
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
    if (scenario === "scroll then change") {
      // The scroll comes first: a scroll write after a style change forces the layout, and its `onLayout`
      // reports would go out ahead of the frame's scroll event.
      renderer.scrollTo(refs.get("scroller")!.id, 0, 200)
      render(page(true))
    } else {
      // A `scrollTop` write while the style change is pending lays out to clamp, as a browser does; that
      // layout reports nothing, the frame does (Chrome: `fixtures/scroll-order-oracle.json`).
      render(page(true))
      refs.get("scroller")!.scrollTop = 200
      renderer.dispatchNativeEvents()
      log.push("after the write")
    }
    frame()
    renderer.dispatchNativeEvents()
    return log
  } finally {
    unmount()
  }
}

const frameReports = [
  "layout a",
  "layout b",
  "layout c",
  "resize first c a",
  "resize second b",
  "intersect first b",
  "intersect second c a",
]

describeNative("the reports of one frame", () => {
  it("reach the app in the order of the DOM's rendering steps", () => {
    expect(run()).toEqual(["scroll", ...frameReports])
  })

  it("come after the scroll of a write that laid out the page first, and a forced layout reports nothing", () => {
    // Chrome: a `scrollTop` write after a style change delivers nothing, then the frame runs the scroll event
    // and the resize observation.
    const oracle = JSON.parse(
      fs.readFileSync(
        path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/scroll-order-oracle.json"),
        "utf8"
      )
    ).cases.styleThenScrollTopWrite
    expect(oracle.syncAfterWrite).toEqual([])
    expect(oracle.afterFrames.filter((entry: string) => entry !== "raf")).toEqual(["scroll", "resize target"])
    expect(run("write while dirty")).toEqual(["after the write", "scroll", ...frameReports])
  })
})
