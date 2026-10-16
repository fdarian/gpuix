import { useLayoutEffect, useRef } from "react"
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

  it("gives a second observer of the same target its own initial entry", () => {
    const { render, renderer, ref, element, unmount } = setup()
    try {
      const first: ResizeObserverEntry[] = []
      const second: ResizeObserverEntry[] = []
      render(<div ref={ref("a")} style={{ width: 10, height: 10 }} />)
      new ResizeObserver((entries) => first.push(...entries)).observe(element("a"))
      renderer.dispatchNativeEvents()
      new ResizeObserver((entries) => second.push(...entries)).observe(element("a"))
      renderer.dispatchNativeEvents()
      expect([first.length, second.length]).toEqual([1, 1])
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

  it("reports 0 by 0 when the element stops having a box", () => {
    const { render, renderer, ref, element, unmount } = setup()
    try {
      const sizes: number[][] = []
      const app = (display: "flex" | "none") => (
        <div style={{ flexDirection: "column" }}>
          <div ref={ref("a")} style={{ display, width: 40, height: 20 }} />
        </div>
      )
      render(app("flex"))
      new ResizeObserver((entries) => {
        for (const entry of entries) sizes.push([entry.contentRect.width, entry.contentRect.height])
      }).observe(element("a"))
      renderer.dispatchNativeEvents()

      render(app("none"))
      renderer.dispatchNativeEvents()
      expect(sizes).toEqual([
        [40, 20],
        [0, 0],
      ])

      render(app("flex"))
      renderer.dispatchNativeEvents()
      expect(sizes.at(-1)).toEqual([40, 20])
    } finally {
      unmount()
    }
  })

  it("keeps reporting the real box of a visibility: hidden element", () => {
    const { render, renderer, ref, element, unmount } = setup()
    try {
      const entries: ResizeObserverEntry[] = []
      const app = (visibility: "visible" | "hidden", width: number) => (
        <div style={{ flexDirection: "column" }}>
          <div
            ref={ref("a")}
            style={{ visibility, width, height: 20, padding: 4, borderWidth: 1, borderColor: "#fff" }}
          />
        </div>
      )
      render(app("hidden", 40))
      new ResizeObserver((batch) => entries.push(...batch)).observe(element("a"))
      renderer.dispatchNativeEvents()

      expect(entries).toHaveLength(1)
      expect(entries[0]!.borderBoxSize).toEqual([{ inlineSize: 40, blockSize: 20 }])
      expect(entries[0]!.contentRect).toMatchObject({ x: 4, y: 4, width: 30, height: 10 })

      render(app("hidden", 60))
      renderer.dispatchNativeEvents()
      expect(entries.at(-1)!.borderBoxSize).toEqual([{ inlineSize: 60, blockSize: 20 }])

      render(app("visible", 60))
      renderer.dispatchNativeEvents()
      expect(entries, "showing it changes no size").toHaveLength(2)
    } finally {
      unmount()
    }
  })

  describe("a state style that changes padding or border", () => {
    // A border-box fixed size keeps the border box still, so only the content
    // box tells the observer.
    const hovered = (
      ref: ReturnType<typeof setup>["ref"],
      style: Record<string, unknown>,
      hover: Record<string, unknown>
    ) => (
      <div style={{ flexDirection: "column" }}>
        <div ref={ref("box")} style={{ width: 100, height: 60, ...style, hover }} />
      </div>
    )

    const pointerOver = (renderer: ReturnType<typeof setup>["renderer"], element: PublicInstance) => {
      const bounds = renderer.getElementBounds(element.id)
      return {
        enter: () => renderer.nativeSimulateMouseMove(bounds.x + 2, bounds.y + 2),
        leave: () => renderer.nativeSimulateMouseMove(bounds.x + 500, bounds.y + 300),
      }
    }

    it("reports the hover padding, and again when the pointer leaves", () => {
      const { render, renderer, ref, element, unmount } = setup()
      try {
        const entries: ResizeObserverEntry[] = []
        render(hovered(ref, { padding: 4 }, { padding: 10 }))
        new ResizeObserver((batch) => entries.push(...batch)).observe(element("box"))
        renderer.dispatchNativeEvents()
        expect(entries).toHaveLength(1)
        expect(entries[0]!.contentRect).toMatchObject({ x: 4, y: 4, width: 92, height: 52 })

        const pointer = pointerOver(renderer, element("box"))
        pointer.enter()
        renderer.dispatchNativeEvents()
        expect(entries).toHaveLength(2)
        expect(entries[1]!.borderBoxSize).toEqual([{ inlineSize: 100, blockSize: 60 }])
        expect(entries[1]!.contentBoxSize).toEqual([{ inlineSize: 80, blockSize: 40 }])
        expect(entries[1]!.contentRect).toMatchObject({ x: 10, y: 10, width: 80, height: 40 })

        pointer.leave()
        renderer.dispatchNativeEvents()
        expect(entries).toHaveLength(3)
        expect(entries[2]!.contentRect).toMatchObject({ x: 4, y: 4, width: 92, height: 52 })
      } finally {
        unmount()
      }
    })

    it("reports the hover padding of an element that has none at rest", () => {
      const { render, renderer, ref, element, unmount } = setup()
      try {
        const entries: ResizeObserverEntry[] = []
        render(hovered(ref, {}, { paddingLeft: 12, paddingTop: 6 }))
        new ResizeObserver((batch) => entries.push(...batch)).observe(element("box"))
        renderer.dispatchNativeEvents()
        expect(entries[0]!.contentRect).toMatchObject({ x: 0, y: 0, width: 100, height: 60 })

        pointerOver(renderer, element("box")).enter()
        renderer.dispatchNativeEvents()
        expect(entries).toHaveLength(2)
        expect(entries[1]!.contentRect).toMatchObject({ x: 12, y: 6, width: 88, height: 54 })
      } finally {
        unmount()
      }
    })

    it("reports the hover border width", () => {
      const { render, renderer, ref, element, unmount } = setup()
      try {
        const entries: ResizeObserverEntry[] = []
        render(hovered(ref, { borderWidth: 1, borderColor: "#fff" }, { borderWidth: 5 }))
        new ResizeObserver((batch) => entries.push(...batch)).observe(element("box"))
        renderer.dispatchNativeEvents()
        expect(entries[0]!.contentRect).toMatchObject({ width: 98, height: 58 })

        pointerOver(renderer, element("box")).enter()
        renderer.dispatchNativeEvents()
        expect(entries).toHaveLength(2)
        expect(entries[1]!.contentRect).toMatchObject({ width: 90, height: 50 })
      } finally {
        unmount()
      }
    })

    it("gives clientLeft the hover border width", () => {
      const { render, renderer, ref, element, unmount } = setup()
      try {
        render(hovered(ref, { borderWidth: 1, borderColor: "#fff" }, { borderWidth: 5 }))
        expect(renderer.getElementBounds(element("box").id).clientLeft).toBe(1)

        pointerOver(renderer, element("box")).enter()
        renderer.flush()
        expect(renderer.getElementBounds(element("box").id).clientLeft).toBe(5)
      } finally {
        unmount()
      }
    })

    it("reports the focusVisible padding, and focusWithin on an ancestor", () => {
      const { render, renderer, ref, element, unmount } = setup()
      try {
        const own: ResizeObserverEntry[] = []
        const outer: ResizeObserverEntry[] = []
        render(
          <div style={{ flexDirection: "column" }}>
            <div
              ref={ref("outer")}
              style={{ width: 100, height: 60, focusWithin: { padding: 8 } }}
            >
              <div
                ref={ref("own")}
                tabIndex={0}
                style={{ width: 40, height: 20, focusVisible: { padding: 5 } }}
              />
            </div>
          </div>
        )
        new ResizeObserver((batch) => own.push(...batch)).observe(element("own"))
        new ResizeObserver((batch) => outer.push(...batch)).observe(element("outer"))
        renderer.dispatchNativeEvents()
        expect([own.length, outer.length]).toEqual([1, 1])

        renderer.simulateKeystrokes("Tab")
        renderer.dispatchNativeEvents()
        expect(own.at(-1)!.contentRect).toMatchObject({ x: 5, y: 5, width: 30, height: 10 })
        expect(outer.at(-1)!.contentRect).toMatchObject({ x: 8, y: 8, width: 84, height: 44 })
        expect([own.length, outer.length]).toEqual([2, 2])
      } finally {
        unmount()
      }
    })

    it("follows a padding transition frame by frame", () => {
      const { render, renderer, ref, element, unmount } = setup()
      try {
        renderer.clockPause()
        const entries: ResizeObserverEntry[] = []
        render(
          <div style={{ flexDirection: "column" }}>
            <div
              ref={ref("box")}
              style={{
                width: 100,
                height: 60,
                padding: 0,
                transition: "padding 1s linear",
                hover: { padding: 20 },
              }}
            />
          </div>
        )
        new ResizeObserver((batch) => entries.push(...batch)).observe(element("box"))
        renderer.dispatchNativeEvents()
        expect(entries).toHaveLength(1)

        pointerOver(renderer, element("box")).enter()
        renderer.clockFastForward(500)
        renderer.dispatchNativeEvents()
        expect(entries.at(-1)!.contentRect.x).toBeCloseTo(10, 0)
        expect(entries.at(-1)!.contentRect.width).toBeCloseTo(80, 0)

        renderer.clockFastForward(1000)
        renderer.dispatchNativeEvents()
        expect(entries.at(-1)!.contentRect).toMatchObject({ x: 20, y: 20, width: 60, height: 20 })
      } finally {
        unmount()
      }
    })
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

  it("reports a visibility: hidden target by its geometry, as intersecting", () => {
    const { render, renderer, ref, element, unmount } = setup()
    try {
      const { entries, callback } = collect()
      render(
        <div style={{ flexDirection: "column", padding: 8 }}>
          <div ref={ref("a")} style={{ visibility: "hidden", width: 50, height: 30 }} />
        </div>
      )
      new IntersectionObserver(callback).observe(element("a"))
      renderer.dispatchNativeEvents()

      expect(entries).toHaveLength(1)
      expect(entries[0]).toMatchObject({
        isIntersecting: true,
        intersectionRatio: 1,
        boundingClientRect: { x: 8, y: 8, width: 50, height: 30 },
      })
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
          <div ref={ref("root")} style={{ position: "relative", width: 100, height: 100, overflow: "hidden" }}>
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

  describe("an element root with a border", () => {
    // The root's border box is 100 by 100 at (20, 20); its padding box is
    // 80 by 80 at (30, 30). The target sits in the 10px border, at (20, 20).
    const app = (
      ref: ReturnType<typeof setup>["ref"],
      target: { top: number; left: number; width: number; height: number }
    ) => (
      <div style={{ flexDirection: "column", padding: 20 }}>
        <div
          ref={ref("root")}
          style={{ position: "relative", width: 100, height: 100, borderWidth: 10, overflow: "hidden" }}
        >
          <div style={{ position: "absolute", ...target }} ref={ref("a")} />
        </div>
      </div>
    )

    it("does not intersect a target that is entirely under the border", () => {
      const { render, renderer, ref, element, unmount } = setup()
      try {
        const { entries, callback } = collect()
        render(app(ref, { top: -10, left: -10, width: 10, height: 10 }))
        new IntersectionObserver(callback, { root: element("root") }).observe(element("a"))
        renderer.dispatchNativeEvents()
        expect(entries).toHaveLength(1)
        expect(entries[0]).toMatchObject({
          isIntersecting: false,
          intersectionRatio: 0,
          boundingClientRect: { x: 20, y: 20, width: 10, height: 10 },
          intersectionRect: { width: 0, height: 0 },
          rootBounds: { x: 30, y: 30, width: 80, height: 80 },
        })
      } finally {
        unmount()
      }
    })

    it("measures the intersection against the padding box", () => {
      const { render, renderer, ref, element, unmount } = setup()
      try {
        const { entries, callback } = collect()
        render(app(ref, { top: -10, left: -10, width: 20, height: 20 }))
        new IntersectionObserver(callback, { root: element("root"), threshold: [0, 0.25, 1] }).observe(
          element("a")
        )
        renderer.dispatchNativeEvents()
        expect(entries).toHaveLength(1)
        expect(entries[0]).toMatchObject({
          isIntersecting: true,
          intersectionRatio: 0.25,
          intersectionRect: { x: 30, y: 30, width: 10, height: 10 },
          rootBounds: { x: 30, y: 30, width: 80, height: 80 },
        })
      } finally {
        unmount()
      }
    })

    it("applies rootMargin to the padding box, percentages included", () => {
      const { render, renderer, ref, element, unmount } = setup()
      try {
        const px = collect()
        const percent = collect()
        render(app(ref, { top: -10, left: -10, width: 10, height: 10 }))
        new IntersectionObserver(px.callback, { root: element("root"), rootMargin: "10px" }).observe(
          element("a")
        )
        new IntersectionObserver(percent.callback, { root: element("root"), rootMargin: "25%" }).observe(
          element("a")
        )
        renderer.dispatchNativeEvents()
        expect(px.entries[0]).toMatchObject({
          isIntersecting: true,
          rootBounds: { x: 20, y: 20, width: 100, height: 100 },
        })
        // 25% of the 80 by 80 padding box is 20px, not 25px of the border box.
        expect(percent.entries[0]).toMatchObject({
          isIntersecting: true,
          rootBounds: { x: 10, y: 10, width: 120, height: 120 },
        })
      } finally {
        unmount()
      }
    })

    it("re-evaluates when only the border changes", () => {
      const { render, renderer, ref, element, unmount } = setup()
      try {
        const { entries, callback } = collect()
        // The root keeps its border box; the target lives outside it, over the
        // spot where the border is.
        const sibling = (borderWidth: number) => (
          <div style={{ flexDirection: "column", padding: 20, position: "relative" }}>
            <div
              ref={ref("root")}
              style={{ width: 100, height: 100, borderWidth, overflow: "hidden" }}
            />
            <div
              ref={ref("a")}
              style={{ position: "absolute", top: 20, left: 20, width: 10, height: 10 }}
            />
          </div>
        )
        render(sibling(10))
        new IntersectionObserver(callback, { root: element("root") }).observe(element("a"))
        renderer.dispatchNativeEvents()
        render(sibling(0))
        renderer.dispatchNativeEvents()
        expect(entries.map((entry) => entry.isIntersecting)).toEqual([false, true])
        expect(entries[1]!.rootBounds).toMatchObject({ x: 20, y: 20, width: 100, height: 100 })
      } finally {
        unmount()
      }
    })
  })

  it("clips by an ancestor's padding box whether or not its border is painted", () => {
    const { render, renderer, ref, element, unmount } = setup()
    try {
      const { entries, callback } = collect()
      render(
        <div style={{ flexDirection: "column", padding: 20 }}>
          <div style={{ position: "relative", width: 100, height: 100, borderWidth: 10, overflow: "hidden" }}>
            <div
              ref={ref("a")}
              style={{ position: "absolute", top: -10, left: -10, width: 10, height: 10 }}
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
          <div style={{ position: "relative", width: 100, height: 100, overflow: "hidden" }}>
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
            <div style={{ position: "relative", width: 100, height: 100, overflow }}>
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

  describe("an absolute or fixed target", () => {
    /** A 100 by 100 clip at (200, 100) holding a dropdown that hangs out of it at (10, 10). */
    const dropdownIn = (
      clip: Record<string, unknown>,
      dropdown: Record<string, unknown>,
      ref: (instance: PublicInstance | null) => void
    ) => (
      <div style={{ position: "relative", width: 600, height: 400, paddingLeft: 200, paddingTop: 100 }}>
        <div style={{ width: 100, height: 100, overflow: "hidden", ...clip }}>
          <div ref={ref} style={{ position: "absolute", left: 10, top: 10, width: 50, height: 50, ...dropdown }} />
        </div>
      </div>
    )

    it.each(["hidden", "scroll", "clip"])(
      "is not clipped by a static overflow: %s ancestor before its containing block",
      (overflow) => {
        const { render, renderer, ref, element, unmount } = setup()
        try {
          const { entries, callback } = collect()
          render(dropdownIn({ overflow }, {}, ref("a")))
          new IntersectionObserver(callback).observe(element("a"))
          renderer.dispatchNativeEvents()
          expect(entries.map((entry) => entry.isIntersecting)).toEqual([true])
          expect(entries[0]!.boundingClientRect).toMatchObject({ x: 10, y: 10, width: 50, height: 50 })
          expect(entries[0]!.intersectionRatio).toBe(1)
        } finally {
          unmount()
        }
      }
    )

    it("is clipped by an overflow ancestor that is its containing block", () => {
      const { render, renderer, ref, element, unmount } = setup()
      try {
        const { entries, callback } = collect()
        // Hangs out of the positioned clip, which spans (200, 100) to (300, 200).
        render(dropdownIn({ position: "relative" }, { left: 150, top: 150 }, ref("a")))
        new IntersectionObserver(callback).observe(element("a"))
        renderer.dispatchNativeEvents()
        expect(entries.map((entry) => entry.isIntersecting)).toEqual([false])
      } finally {
        unmount()
      }
    })

    it.each([
      // Past the static 50 wide clip, which does not reach it, and inside the 100 wide one.
      [60, true],
      // Past the 100 wide clip around the containing block, which clips it.
      [150, false],
    ])("is clipped by the overflow around its containing block, at left %i", (left, visible) => {
      const { render, renderer, ref, element, unmount } = setup()
      try {
        const { entries, callback } = collect()
        render(
          <div style={{ width: 100, height: 100, overflow: "hidden" }}>
            <div style={{ position: "relative", width: 300, height: 300 }}>
              <div style={{ width: 50, height: 50, overflow: "hidden" }}>
                <div ref={ref("a")} style={{ position: "absolute", left, top: 10, width: 30, height: 30 }} />
              </div>
            </div>
          </div>
        )
        new IntersectionObserver(callback).observe(element("a"))
        renderer.dispatchNativeEvents()
        expect(entries.map((entry) => entry.isIntersecting)).toEqual([visible])
      } finally {
        unmount()
      }
    })

    it("is not clipped by a positioned overflow ancestor when it is fixed", () => {
      const { render, renderer, ref, element, unmount } = setup()
      try {
        const { entries, callback } = collect()
        render(dropdownIn({ position: "relative" }, { position: "fixed" }, ref("a")))
        new IntersectionObserver(callback).observe(element("a"))
        renderer.dispatchNativeEvents()
        expect(entries.map((entry) => entry.isIntersecting)).toEqual([true])
      } finally {
        unmount()
      }
    })

    it("follows its containing block's scroll, not a static scroller's", () => {
      const { render, renderer, ref, element, unmount } = setup()
      try {
        const { entries, callback } = collect()
        render(
          <div style={{ position: "relative", width: 400, height: 300 }}>
            <div testId="scroller" style={{ width: 100, height: 100, overflow: "scroll" }}>
              <div style={{ height: 1000 }} />
              <div ref={ref("a")} style={{ position: "absolute", left: 200, top: 10, width: 40, height: 40 }} />
            </div>
          </div>
        )
        new IntersectionObserver(callback).observe(element("a"))
        renderer.dispatchNativeEvents()
        expect(entries.map((entry) => entry.isIntersecting)).toEqual([true])

        renderer.scrollTo(renderer.findByTestId("scroller")!.id, 0, 40)
        renderer.flush()
        renderer.dispatchNativeEvents()
        expect(element("a").getBoundingClientRect().y, "it did not move").toBe(10)
        expect(entries.map((entry) => entry.isIntersecting)).toEqual([true])
      } finally {
        unmount()
      }
    })
  })

  it("gives a second observer of the same target its own initial entry", () => {
    const { render, renderer, ref, element, unmount } = setup()
    try {
      const first = collect()
      const second = collect()
      render(<div ref={ref("a")} style={{ width: 10, height: 10 }} />)
      new IntersectionObserver(first.callback).observe(element("a"))
      renderer.dispatchNativeEvents()
      expect(first.entries).toHaveLength(1)

      new IntersectionObserver(second.callback).observe(element("a"))
      renderer.dispatchNativeEvents()
      expect(second.entries).toHaveLength(1)
      expect(second.entries[0]).toMatchObject({ target: element("a"), isIntersecting: true })
      expect(first.entries, "the first observer is not told again").toHaveLength(1)
    } finally {
      unmount()
    }
  })

  it("gives a second observer an initial entry in the same frame as the first", () => {
    const { render, renderer, ref, element, unmount } = setup()
    try {
      const first = collect()
      const second = collect()
      render(<div ref={ref("a")} style={{ width: 10, height: 10 }} />)
      new IntersectionObserver(first.callback).observe(element("a"))
      new IntersectionObserver(second.callback).observe(element("a"))
      renderer.dispatchNativeEvents()
      expect([first.entries.length, second.entries.length]).toEqual([1, 1])
    } finally {
      unmount()
    }
  })

  it("delivers the initial entries of observers mounted in one commit with one draw", () => {
    const { render, renderer, unmount } = setup()
    try {
      const count = 8
      const batches: IntersectionObserverEntry[][] = []
      let drawsWhileObserving = -1
      function App() {
        const refs = useRef<(PublicInstance | null)[]>([])
        useLayoutEffect(() => {
          const before = renderer.getPaintedFrameCount()
          const observers = refs.current.map((target) => {
            const observer = new IntersectionObserver((entries) => batches.push(entries))
            observer.observe(target!)
            return observer
          })
          drawsWhileObserving = renderer.getPaintedFrameCount() - before
          return () => observers.forEach((observer) => observer.disconnect())
        }, [])
        return (
          <div style={{ flexDirection: "column" }}>
            {Array.from({ length: count }, (_, index) => (
              <div
                key={index}
                ref={(instance) => {
                  refs.current[index] = instance
                }}
                style={{ width: 10, height: 10 }}
              />
            ))}
          </div>
        )
      }
      render(<App />)
      const before = renderer.getPaintedFrameCount()
      renderer.dispatchNativeEvents()
      const drawsAfter = renderer.getPaintedFrameCount() - before

      expect(drawsWhileObserving, "observe() reads no layout").toBe(0)
      expect(drawsAfter, "the batch is probed with one draw").toBeLessThanOrEqual(1)
      expect(batches.flat()).toHaveLength(count)
    } finally {
      unmount()
    }
  })

  it("rejects thresholds and margins that are not valid", () => {
    expect(() => new IntersectionObserver(() => {}, { threshold: 2 })).toThrow(RangeError)
    expect(() => new IntersectionObserver(() => {}, { rootMargin: "2em" })).toThrow(SyntaxError)
    expect(new IntersectionObserver(() => {}, { threshold: [1, 0.25] }).thresholds).toEqual([0.25, 1])
  })
})
