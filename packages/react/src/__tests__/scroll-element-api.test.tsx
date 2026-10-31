import fs from "fs"
import path from "path"
import { useLayoutEffect, useRef, useState } from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { readScrollMetrics } from "@ghia/native/host"
import type { PublicInstance } from "../types/host.js"
import { flushSync } from "../reconciler/reconciler.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

function mount() {
  const refs: Record<string, PublicInstance> = {}
  const ref = (name: string) => (instance: PublicInstance | null) => {
    if (instance) refs[name] = instance
  }
  const root = createTestRoot()
  root.render(
    <div
      ref={ref("scroller")}
      testId="scroller"
      style={{ width: 200, height: 100, padding: 10, overflow: "scroll" }}
    >
      <div style={{ width: 500, height: 400, flexShrink: 0 }} />
      <div ref={ref("target")} testId="target" style={{ height: 20 }} />
    </div>
  )
  return { ...root, refs }
}

describeNative("scroll members of a host element", () => {
  it("reads the geometry the DOM reads", () => {
    const { refs, unmount } = mount()
    try {
      const scroller = refs.scroller!
      expect(scroller.clientWidth).toBe(200)
      expect(scroller.clientHeight).toBe(100)
      expect(scroller.scrollWidth).toBe(520)
      expect(scroller.scrollHeight).toBe(440)
      expect(scroller.scrollTop).toBe(0)
      expect(scroller.scrollLeft).toBe(0)
    } finally {
      unmount()
    }
  })

  it("scrolls instantly when scrollTop and scrollLeft are set", () => {
    const { refs, unmount } = mount()
    try {
      const scroller = refs.scroller!
      scroller.scrollTop = 120
      expect(scroller.scrollTop).toBe(120)
      scroller.scrollLeft = 30
      expect(scroller.scrollLeft).toBe(30)
      scroller.scrollTop = 100_000
      expect(scroller.scrollTop, "clamped to the range").toBe(340)
      scroller.scrollTop = -50
      expect(scroller.scrollTop, "never before the start").toBe(0)
    } finally {
      unmount()
    }
  })

  it("scrolls with scrollTo(options), keeping an axis it does not name", () => {
    const { refs, unmount } = mount()
    try {
      const scroller = refs.scroller!
      scroller.scrollTo({ top: 50, left: 20 })
      expect([scroller.scrollLeft, scroller.scrollTop]).toEqual([20, 50])
      scroller.scrollTo({ top: 70 })
      expect([scroller.scrollLeft, scroller.scrollTop]).toEqual([20, 70])
      scroller.scrollTo({ left: 0, behavior: "instant" })
      expect([scroller.scrollLeft, scroller.scrollTop]).toEqual([0, 70])
    } finally {
      unmount()
    }
  })

  it("scrolls with scrollTo(x, y)", () => {
    const { refs, unmount } = mount()
    try {
      const scroller = refs.scroller!
      scroller.scrollTo(15, 25)
      expect([scroller.scrollLeft, scroller.scrollTop]).toEqual([15, 25])
    } finally {
      unmount()
    }
  })

  it("scrolls relative to the current position with scrollBy", () => {
    const { refs, unmount } = mount()
    try {
      const scroller = refs.scroller!
      scroller.scrollTo(10, 10)
      scroller.scrollBy(5, 20)
      expect([scroller.scrollLeft, scroller.scrollTop]).toEqual([15, 30])
      scroller.scrollBy({ top: -10 })
      expect([scroller.scrollLeft, scroller.scrollTop]).toEqual([15, 20])
    } finally {
      unmount()
    }
  })

  it("answers zero for an element native has not painted", () => {
    const { renderer, unmount } = createTestRoot()
    try {
      expect(readScrollMetrics(renderer, 987_654)).toEqual({
        scrollLeft: 0,
        scrollTop: 0,
        scrollWidth: 0,
        scrollHeight: 0,
        clientWidth: 0,
        clientHeight: 0,
      })
    } finally {
      unmount()
    }
  })
})

describeNative("scrollIntoView", () => {
  function nestedTarget() {
    const refs: Record<string, PublicInstance> = {}
    const ref = (name: string) => (instance: PublicInstance | null) => {
      if (instance) refs[name] = instance
    }
    const root = createTestRoot()
    root.render(
      <div
        ref={ref("outer")}
        style={{ width: 300, height: 200, overflow: "scroll" }}
      >
        <div style={{ height: 300 }} />
        <div style={{ padding: 10 }}>
          <div
            ref={ref("middle")}
            style={{ width: 280, height: 150, overflow: "scroll" }}
          >
            <div style={{ height: 500 }} />
            <div>
              <div style={{ height: 40 }} />
              <div ref={ref("target")} style={{ width: 30, height: 20 }} />
              <div style={{ height: 200 }} />
            </div>
          </div>
        </div>
        <div style={{ height: 600 }} />
      </div>
    )
    return { ...root, refs }
  }

  it("scrolls every scrollable ancestor to a deeply nested descendant", () => {
    const { refs, unmount } = nestedTarget()
    try {
      refs.target!.scrollIntoView()
      // The target is 540 down in the middle scroller, which sits 310 down in the outer one,
      // so both scroll to put it at the start of the view.
      expect(refs.middle!.scrollTop).toBe(540)
      expect(refs.outer!.scrollTop).toBe(310)
    } finally {
      unmount()
    }
  })

  it("aligns the block with start, center and end", () => {
    const { refs, unmount } = nestedTarget()
    try {
      const middle = refs.middle!
      refs.target!.scrollIntoView({ block: "start" })
      expect(middle.scrollTop).toBe(540)
      refs.target!.scrollIntoView({ block: "end" })
      expect(middle.scrollTop).toBe(540 + 20 - 150)
      refs.target!.scrollIntoView({ block: "center" })
      expect(middle.scrollTop).toBe(540 + 10 - 75)
    } finally {
      unmount()
    }
  })

  it("does nothing for block nearest when the target is in view", () => {
    const { refs, unmount } = nestedTarget()
    try {
      const middle = refs.middle!
      refs.target!.scrollIntoView({ block: "start" })
      const outerTop = refs.outer!.scrollTop
      const middleTop = middle.scrollTop
      refs.target!.scrollIntoView({ block: "nearest" })
      expect(middle.scrollTop).toBe(middleTop)
      expect(refs.outer!.scrollTop).toBe(outerTop)
    } finally {
      unmount()
    }
  })

  it("takes the DOM's boolean argument", () => {
    const { refs, unmount } = nestedTarget()
    try {
      refs.target!.scrollIntoView(false)
      expect(refs.middle!.scrollTop).toBe(540 + 20 - 150)
      refs.target!.scrollIntoView(true)
      expect(refs.middle!.scrollTop).toBe(540)
    } finally {
      unmount()
    }
  })
})

describeNative("scroll reads are never stale", () => {
  it("reads scrollHeight in the commit that mounts the content", () => {
    const { render, unmount } = createTestRoot()
    try {
      let scrollHeight: number | undefined
      function App() {
        const ref = useRef<PublicInstance>(null)
        useLayoutEffect(() => {
          scrollHeight = ref.current!.scrollHeight
        }, [])
        return (
          <div ref={ref} style={{ width: 200, height: 100, overflow: "scroll" }}>
            <div style={{ height: 400, flexShrink: 0 }} />
          </div>
        )
      }
      render(<App />)

      expect(scrollHeight).toBe(400)
    } finally {
      unmount()
    }
  })

  it("reads scrollHeight right after content changed in the same turn", () => {
    const { render, unmount } = createTestRoot()
    try {
      let setRows!: (rows: number) => void
      let scroller: PublicInstance | null = null
      function App() {
        const [rows, set] = useState(2)
        setRows = set
        return (
          <div
            ref={(instance) => {
              scroller = instance
            }}
            style={{ width: 200, height: 100, overflow: "scroll" }}
          >
            {Array.from({ length: rows }, (_, row) => (
              <div key={row} style={{ height: 50, flexShrink: 0 }} />
            ))}
          </div>
        )
      }
      render(<App />)
      expect(scroller!.scrollHeight).toBe(100)

      flushSync(() => setRows(10))
      expect(scroller!.scrollHeight).toBe(500)
      flushSync(() => setRows(4))
      expect(scroller!.scrollHeight).toBe(200)
    } finally {
      unmount()
    }
  })

  it("aligns scrollIntoView by the layout of content added in the same turn", () => {
    const { render, unmount } = createTestRoot()
    try {
      let setRows!: (rows: number) => void
      let scroller: PublicInstance | null = null
      let target: PublicInstance | null = null
      function App() {
        const [rows, set] = useState(1)
        setRows = set
        return (
          <div
            ref={(instance) => {
              scroller = instance
            }}
            style={{ width: 200, height: 100, overflow: "scroll" }}
          >
            {Array.from({ length: rows }, (_, row) => (
              <div key={row} style={{ height: 50, flexShrink: 0 }} />
            ))}
            <div
              ref={(instance) => {
                target = instance
              }}
              style={{ height: 20, flexShrink: 0 }}
            />
          </div>
        )
      }
      render(<App />)

      flushSync(() => setRows(10))
      target!.scrollIntoView({ block: "start" })
      // The target sits 500 down, but the content ends at 520 in a 100 tall view.
      expect(scroller!.scrollTop).toBe(420)
    } finally {
      unmount()
    }
  })
})

/// What the scroll members make of a value that is not a finite number, against `fixtures/scroll-order-oracle.html`
/// in headless Chrome (regenerate with `bun fixtures/scroll-order-oracle.ts`): the scroller there is 100 by 100 over
/// 500 by 1000 of content (`scroll()` is not an element member here), starts at (left 0, top 50) and reads `[scrollTop, scrollLeft]` after each write. NaN and
/// both infinities are 0 for every setter, `scrollTo` and `scroll` option or argument (the other axis stays), and a
/// `scrollBy` of one is a move of 0.
describeNative("scroll offsets that are not finite numbers", () => {
  const oracle: { nonFinite: Record<string, [number, number]> } = JSON.parse(
    fs.readFileSync(
      path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/scroll-order-oracle.json"),
      "utf8"
    )
  )
  const writes: Record<string, (scroller: PublicInstance, value: unknown) => void> = {
    "scrollTop = $": (scroller, value) => void (scroller.scrollTop = value as number),
    "scrollLeft = $": (scroller, value) => void (scroller.scrollLeft = value as number),
    "scrollTo({top: $})": (scroller, value) => scroller.scrollTo({ top: value as number }),
    "scrollTo({left: $})": (scroller, value) => scroller.scrollTo({ left: value as number }),
    "scrollTo({top: $, left: $})": (scroller, value) => scroller.scrollTo({ top: value as number, left: value as number }),
    "scrollTo($, $)": (scroller, value) => scroller.scrollTo(value as number, value as number),
    "scrollTo($, 10)": (scroller, value) => scroller.scrollTo(value as number, 10),
    "scrollTo(10, $)": (scroller, value) => scroller.scrollTo(10, value as number),
    "scrollBy({top: $})": (scroller, value) => scroller.scrollBy({ top: value as number }),
    "scrollBy({left: $})": (scroller, value) => scroller.scrollBy({ left: value as number }),
    "scrollBy($, $)": (scroller, value) => scroller.scrollBy(value as number, value as number),
    "scrollBy($, 10)": (scroller, value) => scroller.scrollBy(value as number, 10),
    "scrollBy(10, $)": (scroller, value) => scroller.scrollBy(10, value as number),
  }
  const values: [string, unknown][] = [
    ["NaN", NaN],
    ["Infinity", Infinity],
    ["-Infinity", -Infinity],
  ]
  const cases = values.flatMap(([label, value]) =>
    Object.entries(writes).map(([template, write]) => ({
      name: template.replaceAll("$", label),
      write: (scroller: PublicInstance) => write(scroller, value),
    }))
  )
  cases.push(
    { name: "scrollTop = undefined", write: (scroller) => void (scroller.scrollTop = undefined as never) },
    { name: "scrollTop = null", write: (scroller) => void (scroller.scrollTop = null as never) },
    { name: "scrollTop = '20abc'", write: (scroller) => void (scroller.scrollTop = "20abc" as never) },
    { name: "scrollTop = '20'", write: (scroller) => void (scroller.scrollTop = "20" as never) }
  )

  it("are read as Chrome reads them", () => {
    const refs: Record<string, PublicInstance> = {}
    const root = createTestRoot()
    try {
      root.render(
        <div
          ref={(instance) => {
            if (instance) refs.scroller = instance
          }}
          style={{ width: 100, height: 100, overflow: "auto" }}
        >
          <div style={{ width: 500, height: 1000, flexShrink: 0 }} />
        </div>
      )
      const scroller = refs.scroller!
      const found: Record<string, [number, number]> = {}
      for (const scenario of cases) {
        scroller.scrollTop = 50
        scroller.scrollLeft = 0
        scenario.write(scroller)
        found[scenario.name] = [scroller.scrollTop, scroller.scrollLeft]
      }
      expect(found).toEqual(Object.fromEntries(cases.map((scenario) => [scenario.name, oracle.nonFinite[scenario.name]])))
    } finally {
      root.unmount()
    }
  })
})
