import { createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import type { HostElement, IntersectionObserverEntry, ResizeObserverEntry } from "../index.js"
import { IntersectionObserver, ResizeObserver } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

describe.skipIf(!hasNativeTestRenderer)("ResizeObserver", () => {
  it("reports the content box, then each change of it", () => {
    const [width, setWidth] = createSignal(100)
    let box!: HostElement
    const batches: ResizeObserverEntry[][] = []
    const app = createTestRoot()
    app.render(() => (
      <div style={{ flexDirection: "column" }}>
        <div ref={(host) => (box = host)} style={{ width: width(), height: 40, padding: 10 }} />
      </div>
    ))
    const observer = new ResizeObserver((entries) => batches.push(entries))
    observer.observe(box)
    app.renderer.dispatchNativeEvents()

    expect(batches).toHaveLength(1)
    expect(batches[0]![0]!.target).toBe(box)
    expect(batches[0]![0]!.contentRect).toMatchObject({ x: 10, y: 10, width: 80, height: 20 })
    expect(batches[0]![0]!.borderBoxSize).toEqual([{ inlineSize: 100, blockSize: 40 }])

    app.renderer.flush()
    app.renderer.dispatchNativeEvents()
    expect(batches).toHaveLength(1)

    app.flushSync(() => setWidth(160))
    app.renderer.dispatchNativeEvents()
    expect(batches.map((entries) => entries[0]!.contentRect.width)).toEqual([80, 140])

    observer.disconnect()
    app.flushSync(() => setWidth(200))
    app.renderer.dispatchNativeEvents()
    expect(batches).toHaveLength(2)
  })

  it("can be started from a ref, before the element is mounted", async () => {
    const batches: ResizeObserverEntry[][] = []
    const observer = new ResizeObserver((entries) => batches.push(entries))
    const app = createTestRoot()
    app.render(() => (
      <div style={{ flexDirection: "column" }}>
        <div ref={(host) => observer.observe(host)} style={{ width: 30, height: 20 }} />
      </div>
    ))
    await Promise.resolve()
    app.renderer.flush()
    app.renderer.dispatchNativeEvents()

    expect(batches).toHaveLength(1)
    expect(batches[0]![0]!.contentRect).toMatchObject({ width: 30, height: 20 })
  })
})

describe.skipIf(!hasNativeTestRenderer)("IntersectionObserver", () => {
  it("reports crossings of the window edge", () => {
    const [top, setTop] = createSignal(5000)
    let target!: HostElement
    const entries: IntersectionObserverEntry[] = []
    const app = createTestRoot()
    app.render(() => (
      <div style={{ flexDirection: "column" }}>
        <div
          ref={(host) => (target = host)}
          style={{ position: "absolute", top: top(), left: 0, width: 50, height: 30 }}
        />
      </div>
    ))
    new IntersectionObserver((batch) => entries.push(...batch)).observe(target)
    app.renderer.dispatchNativeEvents()
    expect(entries.map((entry) => entry.isIntersecting)).toEqual([false])

    app.flushSync(() => setTop(100))
    app.renderer.dispatchNativeEvents()
    expect(entries.map((entry) => entry.isIntersecting)).toEqual([false, true])
    expect(entries[1]).toMatchObject({
      intersectionRatio: 1,
      boundingClientRect: { y: 100, width: 50, height: 30 },
    })
  })

  it("uses the part of the target an ancestor leaves visible", () => {
    const entries: IntersectionObserverEntry[] = []
    let target!: HostElement
    const app = createTestRoot()
    app.render(() => (
      <div style={{ flexDirection: "column" }}>
        <div style={{ height: 100, width: 100, overflow: "hidden" }}>
          <div ref={(host) => (target = host)} style={{ height: 100, width: 100, marginTop: -50 }} />
        </div>
      </div>
    ))
    new IntersectionObserver((batch) => entries.push(...batch), { threshold: [0, 0.5, 1] }).observe(target)
    app.renderer.dispatchNativeEvents()

    expect(entries.map((entry) => entry.intersectionRatio)).toEqual([0.5])
  })
})
