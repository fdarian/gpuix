/**
 * Content outside an ancestor's clip is not prepainted or painted (issue #38), and what JS can
 * read of it stays true. gpui skips a subtree only without a screen reader, which the test window
 * pretends to have, so these roots turn it off.
 */
import { createSignal, For } from "solid-js"
import { describe, expect, it } from "bun:test"
import type { HostElement, IntersectionObserverEntry, ResizeObserverEntry } from "../index.js"
import { IntersectionObserver, ResizeObserver } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const ROW = 40
const ROWS = 10
const CLIP = { width: 200, height: 100 }

function mount(code: () => unknown) {
  const app = createTestRoot({ width: 300, height: 400 })
  app.renderer.setA11yActive(false)
  app.render(code as never)
  /** How many subtrees one more frame skips. */
  const skippedPerFrame = () => {
    const before = app.renderer.getCulledSubtreeCount()
    app.renderer.flush()
    return app.renderer.getCulledSubtreeCount() - before
  }
  return { ...app, skippedPerFrame }
}

describe.skipIf(!hasNativeTestRenderer)("content outside a clip", () => {
  it("is skipped, with every box still readable", () => {
    const rows: HostElement[] = []
    const app = mount(() => (
      <div style={{ ...CLIP, overflow: "hidden" }}>
        <For each={Array.from({ length: ROWS }, (_, index) => index)}>
          {(index) => (
            <div ref={(host) => (rows[index] = host)} style={{ width: 150, height: ROW, flexShrink: 0 }} />
          )}
        </For>
      </div>
    ))
    expect(app.skippedPerFrame()).toBeGreaterThanOrEqual(7)
    expect(rows.map((row) => row.getBoundingClientRect().y)).toEqual(
      Array.from({ length: ROWS }, (_, index) => index * ROW)
    )
    app.unmount()
  })

  it("follows a signal that moves it into the clip", () => {
    const [height, setHeight] = createSignal(150)
    let target!: HostElement
    const app = mount(() => (
      <div style={{ ...CLIP, overflow: "hidden" }}>
        <div style={{ width: 150, height: height(), flexShrink: 0 }} />
        <div ref={(host) => (target = host)} style={{ width: 150, height: 30, flexShrink: 0 }} />
      </div>
    ))
    expect(app.skippedPerFrame()).toBeGreaterThan(0)
    expect(target.getBoundingClientRect().y).toBe(150)
    app.flushSync(() => setHeight(40))
    expect(target.getBoundingClientRect().y).toBe(40)
    expect(app.skippedPerFrame()).toBe(0)
    app.unmount()
  })

  it("reports to the observers as not intersecting, then intersecting once scrolled in", () => {
    const intersections: boolean[] = []
    const sizes: number[] = []
    let scroller!: HostElement
    let target!: HostElement
    const app = mount(() => (
      <div ref={(host) => (scroller = host)} style={{ ...CLIP, overflow: "auto" }}>
        <div style={{ width: 150, height: 150, flexShrink: 0 }} />
        <div ref={(host) => (target = host)} style={{ width: 150, height: 30, flexShrink: 0 }} />
      </div>
    ))
    expect(app.skippedPerFrame()).toBeGreaterThan(0)
    new IntersectionObserver((batch: IntersectionObserverEntry[]) =>
      intersections.push(...batch.map((entry) => entry.isIntersecting))
    ).observe(target)
    new ResizeObserver((batch: ResizeObserverEntry[]) =>
      sizes.push(...batch.map((entry) => entry.contentRect.height))
    ).observe(target)
    app.renderer.dispatchNativeEvents()
    expect(intersections).toEqual([false])
    expect(sizes).toEqual([30])

    scroller.scrollTop = 100
    app.renderer.flush()
    app.renderer.dispatchNativeEvents()
    expect(intersections).toEqual([false, true])
    app.unmount()
  })

  it("keeps Tab working into a focusable that is skipped, and scrolls it into view", () => {
    let scroller!: HostElement
    const app = mount(() => (
      <div ref={(host) => (scroller = host)} style={{ ...CLIP, overflow: "scroll" }}>
        <For each={[0, 1, 2, 3, 4, 5]}>
          {(index) => <div testId={`i${index}`} tabIndex={0} style={{ width: 150, height: ROW, flexShrink: 0 }} />}
        </For>
      </div>
    ))
    expect(app.skippedPerFrame()).toBeGreaterThan(0)
    const focused = () => app.renderer.getElement(app.renderer.getFocusedElementId())?.testId ?? null
    app.renderer.simulateKeystrokes("Tab Tab Tab Tab")
    expect(focused()).toBe("i3")
    expect(scroller.scrollTop).toBeGreaterThan(0)
    app.unmount()
  })
})
