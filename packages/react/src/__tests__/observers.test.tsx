import { describe, expect, it } from "vitest"
import type {
  IntersectionObserverEntry,
  PublicInstance,
  ResizeObserverEntry,
} from "../index"
import { IntersectionObserver, ResizeObserver } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

function setup() {
  const root = createTestRoot()
  const refs = new Map<string, PublicInstance>()
  const ref = (name: string) => (instance: PublicInstance | null) => {
    if (instance) refs.set(name, instance)
  }
  const element = (name: string) => refs.get(name)!
  return { ...root, ref, element }
}

describeNative("ResizeObserver", () => {
  it("reports the border and content box after observe, once per frame", () => {
    const { render, renderer, ref, element, unmount } = setup()
    try {
      const batches: ResizeObserverEntry[][] = []
      render(
        <div style={{ flexDirection: "column" }}>
          <div
            ref={ref("box")}
            style={{ width: 100, height: 60, padding: 10, borderWidth: 2, borderColor: "#fff" }}
          />
        </div>
      )
      const observer = new ResizeObserver((entries) => batches.push(entries))
      observer.observe(element("box"))
      renderer.dispatchNativeEvents()

      expect(batches).toHaveLength(1)
      const [entry] = batches[0]!
      expect(batches[0]).toHaveLength(1)
      expect(entry!.target).toBe(element("box"))
      expect(entry!.borderBoxSize).toEqual([{ inlineSize: 100, blockSize: 60 }])
      expect(entry!.contentBoxSize).toEqual([{ inlineSize: 76, blockSize: 36 }])
      expect(entry!.contentRect).toMatchObject({ x: 10, y: 10, width: 76, height: 36 })

      renderer.flush()
      renderer.dispatchNativeEvents()
      expect(batches, "an unchanged frame reports nothing").toHaveLength(1)
    } finally {
      unmount()
    }
  })

  it("reports again when the size changes, and honours box: border-box", () => {
    const { render, renderer, ref, element, unmount } = setup()
    try {
      const content: ResizeObserverEntry[] = []
      const border: ResizeObserverEntry[] = []
      const app = (width: number, padding: number) => (
        <div style={{ flexDirection: "column" }}>
          <div ref={ref("a")} style={{ width, height: 40, padding }} />
          <div ref={ref("b")} style={{ width, height: 40, padding }} />
        </div>
      )
      render(app(100, 0))
      const contentObserver = new ResizeObserver((entries) => content.push(...entries))
      const borderObserver = new ResizeObserver((entries) => border.push(...entries))
      contentObserver.observe(element("a"))
      borderObserver.observe(element("b"), { box: "border-box" })
      renderer.dispatchNativeEvents()
      expect([content.length, border.length]).toEqual([1, 1])

      render(app(100, 10))
      renderer.dispatchNativeEvents()
      expect(content.map((entry) => entry.contentRect.width)).toEqual([100, 80])
      expect(border, "padding moves the content box, not the border box").toHaveLength(1)

      render(app(150, 10))
      renderer.dispatchNativeEvents()
      expect(border.map((entry) => entry.borderBoxSize[0].inlineSize)).toEqual([100, 150])
    } finally {
      unmount()
    }
  })

  it("delivers entries for several targets in one callback", () => {
    const { render, renderer, ref, element, unmount } = setup()
    try {
      const batches: ResizeObserverEntry[][] = []
      render(
        <div style={{ flexDirection: "column" }}>
          <div ref={ref("a")} style={{ width: 10, height: 10 }} />
          <div ref={ref("b")} style={{ width: 20, height: 20 }} />
        </div>
      )
      const observer = new ResizeObserver((entries) => batches.push(entries))
      observer.observe(element("a"))
      observer.observe(element("b"))
      renderer.dispatchNativeEvents()

      expect(batches).toHaveLength(1)
      expect(batches[0]!.map((entry) => entry.contentRect.width).sort()).toEqual([10, 20])
    } finally {
      unmount()
    }
  })

  it("stops after unobserve and disconnect", () => {
    const { render, renderer, ref, element, unmount } = setup()
    try {
      const widths: number[] = []
      const app = (width: number) => (
        <div style={{ flexDirection: "column" }}>
          <div ref={ref("a")} style={{ width, height: 10 }} />
          <div ref={ref("b")} style={{ width, height: 10 }} />
        </div>
      )
      render(app(10))
      const observer = new ResizeObserver((entries) => {
        for (const entry of entries) widths.push(entry.contentRect.width)
      })
      observer.observe(element("a"))
      observer.observe(element("b"))
      renderer.dispatchNativeEvents()
      expect(widths).toEqual([10, 10])

      observer.unobserve(element("a"))
      render(app(30))
      renderer.dispatchNativeEvents()
      expect(widths).toEqual([10, 10, 30])

      observer.disconnect()
      render(app(50))
      renderer.dispatchNativeEvents()
      expect(widths).toEqual([10, 10, 30])
    } finally {
      unmount()
    }
  })

  it("reports 0 by 0 when the element stops painting", () => {
    const { render, renderer, ref, element, unmount } = setup()
    try {
      const sizes: number[][] = []
      const app = (visibility: "visible" | "hidden") => (
        <div style={{ flexDirection: "column" }}>
          <div ref={ref("a")} style={{ visibility, width: 40, height: 20 }} />
        </div>
      )
      render(app("visible"))
      new ResizeObserver((entries) => {
        for (const entry of entries) sizes.push([entry.contentRect.width, entry.contentRect.height])
      }).observe(element("a"))
      renderer.dispatchNativeEvents()

      render(app("hidden"))
      renderer.dispatchNativeEvents()
      expect(sizes).toEqual([
        [40, 20],
        [0, 0],
      ])

      render(app("visible"))
      renderer.dispatchNativeEvents()
      expect(sizes.at(-1)).toEqual([40, 20])
    } finally {
      unmount()
    }
  })

  it("reports 0 by 0 once when the element is removed, then stays quiet", () => {
    const { render, renderer, ref, element, unmount } = setup()
    try {
      const sizes: number[][] = []
      const app = (shown: boolean) => (
        <div style={{ flexDirection: "column" }}>
          {shown ? <div ref={ref("a")} style={{ width: 40, height: 20 }} /> : null}
        </div>
      )
      render(app(true))
      const observer = new ResizeObserver((entries) => {
        for (const entry of entries) sizes.push([entry.contentRect.width, entry.contentRect.height])
      })
      observer.observe(element("a"))
      renderer.dispatchNativeEvents()

      render(app(false))
      renderer.dispatchNativeEvents()
      renderer.flush()
      renderer.dispatchNativeEvents()
      expect(sizes).toEqual([
        [40, 20],
        [0, 0],
      ])
    } finally {
      unmount()
    }
  })

  it.each([
    ["input", (ref: ReturnType<typeof setup>["ref"]) => <input ref={ref("a")} style={{ width: 120, height: 30 }} />],
    ["img", (ref: ReturnType<typeof setup>["ref"]) => <img ref={ref("a")} style={{ width: 120, height: 30 }} />],
    ["svg", (ref: ReturnType<typeof setup>["ref"]) => <svg ref={ref("a")} style={{ width: 120, height: 30 }} />],
    ["textarea", (ref: ReturnType<typeof setup>["ref"]) => <textarea ref={ref("a")} style={{ width: 120, height: 30 }} />],
    ["code", (ref: ReturnType<typeof setup>["ref"]) => <code ref={ref("a")} code="let a = 1" language="ts" style={{ width: 120, height: 30 }} />],
    ["markdown", (ref: ReturnType<typeof setup>["ref"]) => <markdown ref={ref("a")} source="# hi" style={{ width: 120, height: 30 }} />],
  ])("observes a <%s>", (_name, build) => {
    const { render, renderer, ref, element, unmount } = setup()
    try {
      const sizes: number[][] = []
      render(<div style={{ flexDirection: "column" }}>{build(ref)}</div>)
      new ResizeObserver((entries) => {
        for (const entry of entries) sizes.push([entry.borderBoxSize[0].inlineSize, entry.borderBoxSize[0].blockSize])
      }).observe(element("a"))
      renderer.dispatchNativeEvents()
      expect(sizes).toEqual([[120, 30]])
    } finally {
      unmount()
    }
  })

  it("rejects a box it cannot observe", () => {
    const { ref, element, unmount, render } = setup()
    try {
      render(<div ref={ref("a")} />)
      const observer = new ResizeObserver(() => {})
      expect(() =>
        observer.observe(element("a"), { box: "device-pixel-content-box" as "content-box" })
      ).toThrow(TypeError)
    } finally {
      unmount()
    }
  })
})

describeNative("IntersectionObserver", () => {
  const collect = () => {
    const entries: IntersectionObserverEntry[] = []
    return { entries, callback: (batch: IntersectionObserverEntry[]) => entries.push(...batch) }
  }

  it("reports an initial entry for a visible target, against the window", () => {
    const { render, renderer, ref, element, unmount } = setup()
    try {
      const { entries, callback } = collect()
      render(
        <div style={{ flexDirection: "column", padding: 8 }}>
          <div ref={ref("a")} style={{ width: 50, height: 30 }} />
        </div>
      )
      new IntersectionObserver(callback).observe(element("a"))
      renderer.dispatchNativeEvents()

      expect(entries).toHaveLength(1)
      const size = renderer.getWindowSize()
      expect(entries[0]).toMatchObject({
        target: element("a"),
        isIntersecting: true,
        intersectionRatio: 1,
        boundingClientRect: { x: 8, y: 8, width: 50, height: 30 },
        intersectionRect: { x: 8, y: 8, width: 50, height: 30 },
        rootBounds: { x: 0, y: 0, width: size.width, height: size.height },
      })
      expect(entries[0]!.time).toBeGreaterThan(0)
    } finally {
      unmount()
    }
  })

  it("reports a target that is outside the window as not intersecting, then when it enters", () => {
    const { render, renderer, ref, element, unmount } = setup()
    try {
      const { entries, callback } = collect()
      const app = (top: number) => (
        <div style={{ flexDirection: "column" }}>
          <div ref={ref("a")} style={{ position: "absolute", top, left: 0, width: 50, height: 30 }} />
        </div>
      )
      render(app(5000))
      new IntersectionObserver(callback).observe(element("a"))
      renderer.dispatchNativeEvents()
      expect(entries.map((entry) => entry.isIntersecting)).toEqual([false])
      expect(entries[0]!.intersectionRatio).toBe(0)
      expect(entries[0]!.boundingClientRect.top).toBe(5000)

      render(app(5001))
      renderer.dispatchNativeEvents()
      expect(entries, "moving while still outside is not a crossing").toHaveLength(1)

      render(app(100))
      renderer.dispatchNativeEvents()
      expect(entries.map((entry) => entry.isIntersecting)).toEqual([false, true])
    } finally {
      unmount()
    }
  })

  it("fires on threshold crossings only, using the part clipped by an ancestor", () => {
    const { render, renderer, ref, element, unmount } = setup()
    try {
      const { entries, callback } = collect()
      const app = (marginTop: number) => (
        <div style={{ flexDirection: "column" }}>
          <div style={{ height: 100, width: 100, overflow: "hidden" }}>
            <div ref={ref("a")} style={{ height: 100, width: 100, marginTop }} />
          </div>
        </div>
      )
      render(app(-50))
      new IntersectionObserver(callback, { threshold: [0, 0.5, 1] }).observe(element("a"))
      renderer.dispatchNativeEvents()
      expect(entries.map((entry) => entry.intersectionRatio)).toEqual([0.5])

      render(app(-45))
      renderer.dispatchNativeEvents()
      expect(entries, "0.55 is still between 0.5 and 1").toHaveLength(1)

      render(app(-80))
      renderer.dispatchNativeEvents()
      expect(entries.map((entry) => entry.intersectionRatio)).toEqual([0.5, 0.2])

      render(app(-100))
      renderer.dispatchNativeEvents()
      expect(entries.at(-1)).toMatchObject({ isIntersecting: false, intersectionRatio: 0 })
    } finally {
      unmount()
    }
  })

  it("grows the window by rootMargin", () => {
    const { render, renderer, ref, element, unmount } = setup()
    try {
      const margin = collect()
      const plain = collect()
      render(
        <div style={{ flexDirection: "column" }}>
          <div
            ref={ref("a")}
            style={{ position: "absolute", top: 10, left: -150, width: 100, height: 20 }}
          />
        </div>
      )
      new IntersectionObserver(plain.callback).observe(element("a"))
      const withMargin = new IntersectionObserver(margin.callback, { rootMargin: "200px" })
      withMargin.observe(element("a"))
      renderer.dispatchNativeEvents()

      expect(withMargin.rootMargin).toBe("200px 200px 200px 200px")
      expect(plain.entries.map((entry) => entry.isIntersecting)).toEqual([false])
      expect(margin.entries.map((entry) => entry.isIntersecting)).toEqual([true])
      expect(margin.entries[0]!.rootBounds.left).toBe(-200)
    } finally {
      unmount()
    }
  })

  it("tests against a root element", () => {
    const { render, renderer, ref, element, unmount } = setup()
    try {
      const { entries, callback } = collect()
      const app = (top: number) => (
        <div style={{ flexDirection: "column", padding: 20 }}>
          <div ref={ref("root")} style={{ width: 100, height: 100, overflow: "hidden" }}>
            <div style={{ position: "absolute", top, left: 0, width: 40, height: 40 }} ref={ref("a")} />
          </div>
        </div>
      )
      render(app(10))
      new IntersectionObserver(callback, { root: element("root") }).observe(element("a"))
      renderer.dispatchNativeEvents()
      expect(entries).toHaveLength(1)
      expect(entries[0]).toMatchObject({
        isIntersecting: true,
        intersectionRatio: 1,
        rootBounds: { x: 20, y: 20, width: 100, height: 100 },
      })

      render(app(150))
      renderer.dispatchNativeEvents()
      expect(entries.map((entry) => entry.isIntersecting)).toEqual([true, false])
    } finally {
      unmount()
    }
  })

  it("reports not intersecting once when the target is removed", () => {
    const { render, renderer, ref, element, unmount } = setup()
    try {
      const { entries, callback } = collect()
      const app = (shown: boolean) => (
        <div style={{ flexDirection: "column" }}>
          {shown ? <div ref={ref("a")} style={{ width: 20, height: 20 }} /> : null}
        </div>
      )
      render(app(true))
      new IntersectionObserver(callback).observe(element("a"))
      renderer.dispatchNativeEvents()
      render(app(false))
      renderer.dispatchNativeEvents()
      renderer.flush()
      renderer.dispatchNativeEvents()
      expect(entries.map((entry) => entry.isIntersecting)).toEqual([true, false])
      expect(entries[1]!.boundingClientRect.width).toBe(0)
    } finally {
      unmount()
    }
  })

  it("clips by an ancestor even when rootMargin reaches past it", () => {
    const { render, renderer, ref, element, unmount } = setup()
    try {
      const { entries, callback } = collect()
      render(
        <div style={{ flexDirection: "column" }}>
          <div style={{ width: 100, height: 100, overflow: "hidden" }}>
            <div
              ref={ref("a")}
              style={{ position: "absolute", top: 150, left: 0, width: 40, height: 40 }}
            />
          </div>
        </div>
      )
      new IntersectionObserver(callback, { rootMargin: "100px" }).observe(element("a"))
      renderer.dispatchNativeEvents()
      // Inside the window and its margin, but the clip hides all of it.
      expect(entries.map((entry) => entry.isIntersecting)).toEqual([false])
    } finally {
      unmount()
    }
  })

  it("sees a target past the window edge through rootMargin when nothing clips it", () => {
    const { render, renderer, ref, element, unmount } = setup()
    try {
      const { entries, callback } = collect()
      const { height } = renderer.getWindowSize()
      render(
        <div style={{ flexDirection: "column" }}>
          <div
            ref={ref("a")}
            style={{ position: "absolute", top: height + 50, left: 0, width: 40, height: 40 }}
          />
        </div>
      )
      new IntersectionObserver(callback, { rootMargin: "100px" }).observe(element("a"))
      renderer.dispatchNativeEvents()
      expect(entries.map((entry) => entry.isIntersecting)).toEqual([true])
    } finally {
      unmount()
    }
  })

  it("takeRecords drains the queue, and disconnect stops reports", () => {
    const { render, renderer, ref, element, unmount } = setup()
    try {
      const { entries, callback } = collect()
      render(<div ref={ref("a")} style={{ width: 10, height: 10 }} />)
      const observer = new IntersectionObserver(callback)
      observer.observe(element("a"))
      renderer.flush()
      renderer.dispatchNativeEvents()
      expect(entries).toHaveLength(1)
      expect(observer.takeRecords()).toEqual([])
      observer.disconnect()
      render(<div ref={ref("a")} style={{ width: 10, height: 10, marginLeft: 5000 }} />)
      renderer.dispatchNativeEvents()
      expect(entries).toHaveLength(1)
    } finally {
      unmount()
    }
  })

  it.each(["auto", "clip", "scroll", "hidden"])(
    "clips by an ancestor with overflow: %s",
    (overflow) => {
      const { render, renderer, ref, element, unmount } = setup()
      try {
        const { entries, callback } = collect()
        render(
          <div style={{ flexDirection: "column" }}>
            <div style={{ width: 100, height: 100, overflow }}>
              <div
                ref={ref("a")}
                style={{ position: "absolute", top: 150, left: 0, width: 40, height: 40 }}
              />
            </div>
          </div>
        )
        new IntersectionObserver(callback).observe(element("a"))
        renderer.dispatchNativeEvents()
        expect(entries.map((entry) => entry.isIntersecting)).toEqual([false])
      } finally {
        unmount()
      }
    }
  )

  it("rejects thresholds and margins that are not valid", () => {
    expect(() => new IntersectionObserver(() => {}, { threshold: 2 })).toThrow(RangeError)
    expect(() => new IntersectionObserver(() => {}, { rootMargin: "2em" })).toThrow(SyntaxError)
    expect(new IntersectionObserver(() => {}, { threshold: [1, 0.25] }).thresholds).toEqual([0.25, 1])
  })
})
