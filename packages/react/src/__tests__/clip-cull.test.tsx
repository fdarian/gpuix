// @ts-nocheck
/**
 * Content outside an ancestor's clip is not prepainted or painted (issue #38), and everything JS
 * can read of it is still there: boxes, observers, scroll metrics, container sizes, tab order.
 *
 * `getCulledSubtreeCount` says that something was skipped, so a test of what stays true of a
 * skipped subtree cannot pass by nothing having been skipped. The test window pretends a screen
 * reader is connected, so the accessibility tree is built here too.
 */

import fs from "fs"
import React from "react"
import { describe, expect, it } from "vitest"
import { IntersectionObserver, ResizeObserver } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rgba = [number, number, number, number]
const RED: Rgba = [255, 0, 0, 255]
const BLUE: Rgba = [0, 0, 255, 255]
const WHITE: Rgba = [255, 255, 255, 255]

const ROW = 40
const ROWS = 10
/** 100 high: rows 0 and 1 are inside, row 2 is cut by the clip, rows 3 to 9 are outside. */
const CLIP = { width: 200, height: 100 }

function setup(element: React.ReactElement, size = { width: 300, height: 400 }) {
  const root = createTestRoot(size)
  const refs = new Map()
  const ref = (name: string) => (instance) => {
    if (instance) refs.set(name, instance)
  }
  root.render(typeof element === "function" ? element(ref) : element)
  const culled = () => root.renderer.getCulledSubtreeCount()
  /** How many subtrees one more frame skips. */
  const skippedPerFrame = () => {
    const before = culled()
    root.renderer.flush()
    return culled() - before
  }
  return { ...root, refs, ref, culled, skippedPerFrame, element: (name: string) => refs.get(name) }
}

const rows = (ref, extra: (index: number) => object = () => ({})) =>
  Array.from({ length: ROWS }, (_, index) => (
    <div
      key={index}
      ref={ref(`row${index}`)}
      testId={`row${index}`}
      style={{ width: 150, height: ROW, flexShrink: 0, backgroundColor: "#335577", ...extra(index) }}
    />
  ))

const clip = (ref, children, style = {}) => (
  <div style={{ ...CLIP, overflow: "hidden", ...style }}>{children}</div>
)

function sample(testRoot, name: string, x: number, y: number): Rgba {
  const file = `${SHOTS_DIR}/clip-cull-${name}.png`
  fs.rmSync(file, { force: true })
  testRoot.renderer.captureScreenshot(file)
  return readLogicalPixels(file, testRoot.renderer).at(x, y)
}

describeNative("content outside a clip", () => {
  it("is skipped, with every box still readable", () => {
    const { skippedPerFrame, element, unmount } = setup((ref) => clip(ref, rows(ref)))
    try {
      // Rows 3 to 9 are outside the clip.
      expect(skippedPerFrame()).toBeGreaterThanOrEqual(7)
      const rects = Array.from({ length: ROWS }, (_, index) => element(`row${index}`).getBoundingClientRect())
      expect(rects.map((rect) => rect.y)).toEqual(Array.from({ length: ROWS }, (_, index) => index * ROW))
      expect(rects.every((rect) => rect.width === 150 && rect.height === ROW)).toBe(true)
    } finally {
      unmount()
    }
  })

  it("is not skipped when the clip is not a clip", () => {
    const { skippedPerFrame, unmount } = setup((ref) => clip(ref, rows(ref), { overflow: "visible" }))
    try {
      expect(skippedPerFrame()).toBe(0)
    } finally {
      unmount()
    }
  })

  it("keeps a box-shadowed child that is outside the clip but casts into it", () => {
    const shadowed = (ref) =>
      clip(
        ref,
        <>
          <div style={{ width: 150, height: 106, flexShrink: 0 }} />
          <div
            ref={ref("lit")}
            style={{ width: 150, height: 40, flexShrink: 0, backgroundColor: "#ffffff", boxShadow: "0 -20px 0 0 #ff0000" }}
          />
        </>,
        { backgroundColor: "#ffffff" }
      )
    const { skippedPerFrame, ...root } = setup(shadowed)
    try {
      // The child starts at y=106, below the clip at 100, and its shadow covers 86 to 126.
      expect(skippedPerFrame()).toBe(0)
      expect(sample(root, "shadow-in", 50, 93)).toEqual(RED)
      expect(sample(root, "shadow-in", 50, 80)).toEqual(WHITE)
    } finally {
      root.unmount()
    }
  })

  it("is skipped once its shadow cannot reach the clip", () => {
    const { skippedPerFrame, unmount } = setup((ref) =>
      clip(
        ref,
        <>
          <div style={{ width: 150, height: 130, flexShrink: 0 }} />
          <div
            style={{ width: 150, height: 40, flexShrink: 0, backgroundColor: "#ffffff", boxShadow: "0 -20px 0 0 #ff0000" }}
          />
        </>
      )
    )
    try {
      expect(skippedPerFrame()).toBeGreaterThan(0)
    } finally {
      unmount()
    }
  })

  it("keeps an absolute child that a box outside the clip places inside it", () => {
    const placed = (ref) =>
      clip(
        ref,
        <>
          <div style={{ width: 150, height: 140, flexShrink: 0 }} />
          <div style={{ width: 150, height: 40, flexShrink: 0, position: "static" }}>
            <div style={{ position: "absolute", top: 10, left: 10, width: 30, height: 30, backgroundColor: "#0000ff" }} />
          </div>
        </>,
        { position: "relative", backgroundColor: "#ffffff" }
      )
    const root = setup(placed)
    try {
      expect(sample(root, "absolute", 20, 20)).toEqual(BLUE)
      expect(sample(root, "absolute", 60, 20)).toEqual(WHITE)
    } finally {
      root.unmount()
    }
  })

  it("is painted when it is scrolled into view, and skipped again when it leaves", () => {
    const { renderer, skippedPerFrame, element, unmount, ...root } = setup((ref) => (
      <div ref={ref("scroller")} testId="scroller" style={{ ...CLIP, overflow: "auto", backgroundColor: "#ffffff" }}>
        {rows(ref, (index) => ({ backgroundColor: index === 5 ? "#ff0000" : "#335577" }))}
      </div>
    ))
    try {
      expect(skippedPerFrame()).toBeGreaterThanOrEqual(7)
      // Row 5 is 200 to 240; the scroller shows 100 high.
      expect(sample({ renderer }, "scroll-out", 20, 20)).not.toEqual(RED)
      element("scroller").scrollTop = 160
      renderer.flush()
      // Row 5 is at 40 to 80 in the view now.
      expect(sample({ renderer }, "scroll-in", 20, 60)).toEqual(RED)
      expect(element("row5").getBoundingClientRect().y).toBe(5 * ROW - 160)
      element("scroller").scrollTop = 0
      expect(skippedPerFrame()).toBeGreaterThanOrEqual(7)
      expect(sample({ renderer }, "scroll-back", 20, 60)).not.toEqual(RED)
    } finally {
      unmount()
    }
  })

  it("keeps hover, press and click on the siblings inside the clip", () => {
    const clicks: string[] = []
    const { renderer, skippedPerFrame, unmount, ...root } = setup((ref) =>
      clip(
        ref,
        rows(ref, (index) => ({ hover: { backgroundColor: "#ff0000" } })).map((row, index) =>
          React.cloneElement(row, { onClick: () => clicks.push(`row${index}`) })
        )
      )
    )
    try {
      expect(skippedPerFrame()).toBeGreaterThan(0)
      renderer.nativeSimulateMouseMove(20, ROW + 10)
      renderer.flush()
      expect(sample({ renderer }, "hover", 20, ROW + 10)).toEqual(RED)
      expect(sample({ renderer }, "hover", 20, 10)).not.toEqual(RED)
      renderer.nativeSimulateClick(20, ROW + 10)
      renderer.nativeSimulateClick(20, 10)
      expect(clicks).toEqual(["row1", "row0"])
    } finally {
      unmount()
    }
  })

  it("keeps Tab working into a focusable that is skipped, and scrolls it into view", () => {
    const { renderer, skippedPerFrame, refs, unmount } = setup((ref) => (
      <div testId="scroller" ref={ref("scroller")} style={{ ...CLIP, overflow: "scroll" }}>
        {Array.from({ length: 6 }, (_, index) => (
          <div key={index} testId={`i${index}`} tabIndex={0} style={{ width: 150, height: ROW, flexShrink: 0 }} />
        ))}
      </div>
    ))
    try {
      expect(skippedPerFrame()).toBeGreaterThan(0)
      const focused = () => renderer.getElement(renderer.getFocusedElementId())?.testId ?? null
      renderer.simulateKeystrokes("Tab")
      expect(focused()).toBe("i0")
      // i3 starts at 120, below the 100 clip, and so do i4 and i5.
      renderer.simulateKeystrokes("Tab Tab Tab")
      expect(focused()).toBe("i3")
      expect(refs.get("scroller").scrollTop).toBeGreaterThan(0)
      // Focus the scroller's way back too.
      renderer.simulateKeystrokes("Shift+Tab")
      expect(focused()).toBe("i2")
    } finally {
      unmount()
    }
  })

  it("keeps an input that is outside the clip focusable by Tab and scrolls it into view", () => {
    const { renderer, refs, unmount } = setup((ref) => (
      <div ref={ref("scroller")} style={{ ...CLIP, overflow: "scroll" }}>
        <div style={{ width: 150, height: 160, flexShrink: 0 }} />
        <input testId="field" style={{ width: 150, height: 30, flexShrink: 0 }} />
      </div>
    ))
    try {
      renderer.simulateKeystrokes("Tab")
      expect(renderer.getElement(renderer.getFocusedElementId())?.testId).toBe("field")
      expect(refs.get("scroller").scrollTop).toBeGreaterThan(0)
    } finally {
      unmount()
    }
  })

  it("keeps the scroll metrics of a scroller in a skipped subtree", () => {
    const { skippedPerFrame, refs, unmount } = setup((ref) =>
      clip(
        ref,
        <>
          <div style={{ width: 150, height: 150, flexShrink: 0 }} />
          <div style={{ width: 150, height: 60, flexShrink: 0 }}>
            <div ref={ref("inner")} style={{ width: 100, height: 50, overflow: "auto" }}>
              <div style={{ width: 300, height: 200, flexShrink: 0 }} />
            </div>
          </div>
        </>
      )
    )
    try {
      expect(skippedPerFrame()).toBeGreaterThan(0)
      const inner = refs.get("inner")
      expect([inner.clientWidth, inner.clientHeight]).toEqual([100, 50])
      expect([inner.scrollWidth, inner.scrollHeight]).toEqual([300, 200])
      inner.scrollTop = 80
      expect(inner.scrollTop).toBe(80)
    } finally {
      unmount()
    }
  })

  it("reports to a ResizeObserver, and again when it resizes", () => {
    const sizes: number[] = []
    const app = (ref, height: number) =>
      clip(
        ref,
        <>
          <div style={{ width: 150, height: 150, flexShrink: 0 }} />
          <div ref={ref("target")} style={{ width: 150, height, flexShrink: 0 }} />
        </>
      )
    const { renderer, render, refs, skippedPerFrame, ref, unmount } = setup((ref) => app(ref, 30))
    try {
      expect(skippedPerFrame()).toBeGreaterThan(0)
      new ResizeObserver((entries) => sizes.push(...entries.map((entry) => entry.contentRect.height))).observe(
        refs.get("target")
      )
      renderer.dispatchNativeEvents()
      expect(sizes).toEqual([30])
      render(app(ref, 45))
      renderer.dispatchNativeEvents()
      expect(sizes).toEqual([30, 45])
    } finally {
      unmount()
    }
  })

  it("reports to an IntersectionObserver as not intersecting, then as intersecting once scrolled in", () => {
    const entries: boolean[] = []
    const { renderer, skippedPerFrame, refs, unmount } = setup((ref) => (
      <div ref={ref("scroller")} style={{ ...CLIP, overflow: "auto" }}>
        <div style={{ width: 150, height: 150, flexShrink: 0 }} />
        <div ref={ref("target")} style={{ width: 150, height: 30, flexShrink: 0 }} />
      </div>
    ))
    try {
      expect(skippedPerFrame()).toBeGreaterThan(0)
      new IntersectionObserver((batch) => entries.push(...batch.map((entry) => entry.isIntersecting))).observe(
        refs.get("target")
      )
      renderer.dispatchNativeEvents()
      expect(entries).toEqual([false])

      refs.get("scroller").scrollTop = 100
      renderer.flush()
      renderer.dispatchNativeEvents()
      expect(entries).toEqual([false, true])

      refs.get("scroller").scrollTop = 0
      renderer.flush()
      renderer.dispatchNativeEvents()
      expect(entries).toEqual([false, true, false])
    } finally {
      unmount()
    }
  })

  it("keeps the size a container query reads from a skipped container", () => {
    const { skippedPerFrame, refs, unmount } = setup((ref) =>
      clip(
        ref,
        <>
          <div style={{ width: 150, height: 150, flexShrink: 0 }} />
          <div style={{ width: 160, height: 40, flexShrink: 0, containerType: "inline-size" }}>
            <div
              ref={ref("child")}
              style={{ width: 20, height: 20, "@container": { "(min-width: 150px)": { width: 77 } } }}
            />
          </div>
        </>
      )
    )
    try {
      expect(skippedPerFrame()).toBeGreaterThan(0)
      expect(refs.get("child").getBoundingClientRect().width).toBe(77)
    } finally {
      unmount()
    }
  })

  it("keeps a skipped button in the accessibility tree, with its box and relations", () => {
    const buttons = (ref) =>
      clip(
        ref,
        <>
          <div style={{ width: 150, height: 150, flexShrink: 0 }} />
          <div
            ref={ref("clipped")}
            id="clipped.button"
            role="button"
            aria-label="Clipped"
            aria-describedby="clipped.hint"
            tabIndex={0}
            onClick={() => {}}
            style={{ width: 120, height: 30, flexShrink: 0 }}
          >
            <text style={{ fontSize: 12 }}>inside the button</text>
          </div>
          <div id="clipped.hint" role="note" aria-label="Hint" style={{ width: 50, height: 10, flexShrink: 0 }} />
        </>
      )
    const { skippedPerFrame, refs, ...root } = setup(buttons)
    try {
      expect(skippedPerFrame()).toBeGreaterThan(0)
      const tree = root.renderer.getA11yTree()
      const nodes = Object.entries(tree.nodes ?? {})
      const find = (authorId: string) => {
        const found = nodes.find(([, node]) => node.aria?.author_id === authorId)
        if (!found) throw new Error(`no a11y node with author id ${authorId}`)
        return found
      }
      const [, button] = find("clipped.button")
      const [hintKey] = find("clipped.hint")
      expect(button.aria?.role).toBe("Button")
      expect(button.aria?.label).toBe("Clipped")
      expect(button.aria?.on_action).toContain("Click")
      expect(button.aria?.described_by).toEqual([hintKey])

      const scale = tree.frame?.scale_factor ?? 1
      const rect = refs.get("clipped").getBoundingClientRect()
      expect(rect.y).toBe(150)
      expect(button.bounds).toEqual({
        x0: rect.x * scale,
        y0: rect.y * scale,
        x1: (rect.x + rect.width) * scale,
        y1: (rect.y + rect.height) * scale,
      })
    } finally {
      root.unmount()
    }
  })
})
