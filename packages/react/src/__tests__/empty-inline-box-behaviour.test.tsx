/// What an empty `<span>` with inline margin, border or padding is besides a box in the layout (the oracle in
/// `empty-inline-box.test.tsx` has the boxes): it paints its background and border, is the target of the pointer
/// over its fragment and not outside it, reports `getClientRects`, is no text for copy, accessibility or find, and
/// is a box again when its style changes.

import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const TEXT = { width: 300, fontSize: 20, lineHeight: "20px", color: "#000000", backgroundColor: "#ffffff" } as const
const BOX = { paddingLeft: 10, paddingRight: 10, backgroundColor: "#00ff00" } as const

describeNative("an empty inline box that takes space along the line", () => {
  function setup(node: React.ReactNode) {
    const root = createTestRoot({ width: 300, height: 80 })
    root.render(node)
    const bounds = (id: string) => root.renderer.getElementBounds(root.renderer.findByTestId(id)!.id)!
    return { ...root, bounds }
  }

  it("paints its background and border over its fragment, as tall as the content area", () => {
    const { renderer, bounds, unmount } = setup(
      <div style={TEXT}>
        <span testId="x" style={{ ...BOX, borderLeft: "2px solid #ff0000" }} />
      </div>
    )
    try {
      const box = bounds("x")
      expect(box.width).toBe(22)
      expect(box.height).toBeGreaterThan(10)
      const file = `${SHOTS_DIR}/empty-inline-box-paint.png`
      renderer.captureScreenshot(file)
      const pixels = readLogicalPixels(file, renderer)
      const y = Math.floor(box.y + box.height / 2)
      expect(pixels.at(Math.floor(box.x + 1), y), "the border").toEqual([255, 0, 0, 255])
      expect(pixels.at(Math.floor(box.x + 12), y), "the padding").toEqual([0, 255, 0, 255])
      expect(pixels.at(Math.floor(box.x + 21), y), "the right padding").toEqual([0, 255, 0, 255])
      expect(pixels.at(Math.floor(box.x + 23), y), "beyond the box").toEqual([255, 255, 255, 255])
    } finally {
      unmount()
    }
  })

  it("is the target of a click over its fragment only", () => {
    const seen: string[] = []
    const { renderer, bounds, unmount } = setup(
      <div style={TEXT} onClick={() => seen.push("div")}>
        <text style={{ fontFamily: "Menlo" }}>
          AA
          <span testId="x" style={BOX} onClick={() => seen.push("span")} />
          BB
        </text>
      </div>
    )
    try {
      const box = bounds("x")
      const y = box.y + box.height / 2
      renderer.nativeSimulateClick(box.x + box.width / 2, y)
      expect(seen).toEqual(["span", "div"])
      seen.length = 0
      renderer.nativeSimulateClick(box.x - 3, y)
      renderer.nativeSimulateClick(box.x + box.width + 3, y)
      expect(seen).toEqual(["div", "div"])
    } finally {
      unmount()
    }
  })

  it("reports its box to getBoundingClientRect, and nothing for a box that takes no space", () => {
    const refs = new Map<string, any>()
    const ref = (name: string) => (instance: any) => instance && refs.set(name, instance)
    const { unmount } = setup(
      <div style={TEXT}>
        <text style={{ fontFamily: "Menlo" }}>
          AA
          <span ref={ref("x")} style={BOX} />
          BB
          <span ref={ref("none")} style={{ paddingTop: 4 }} />
        </text>
      </div>
    )
    try {
      expect(refs.get("x").getBoundingClientRect().width).toBe(20)
      expect(refs.get("x").getBoundingClientRect().height).toBeGreaterThan(10)
      expect(refs.get("none").getBoundingClientRect()).toMatchObject({ width: 0, height: 0 })
    } finally {
      unmount()
    }
  })

  it("is no text: selection copies the text around it, and the accessible name has none of it", () => {
    const { renderer, bounds, unmount } = setup(
      <div style={TEXT} role="note" testId="note">
        <text testId="t" style={{ fontFamily: "Menlo" }}>
          AA
          <span style={BOX} />
          BB
        </text>
      </div>
    )
    try {
      const box = bounds("t")
      expect(renderer.dragSelect(box.x + 1, box.y + 5, box.x + box.width - 1, box.y + 5)).toBe("AABB")
      const tree = JSON.stringify(renderer.getA11yTree())
      expect(tree).toContain("AABB")
      expect(tree.toLowerCase()).not.toContain("ufffc")
      expect(tree).not.toContain("\uFFFC")
    } finally {
      unmount()
    }
  })

  it("becomes a box again when its padding appears, and a phantom when it goes", () => {
    const root = createTestRoot({ width: 300, height: 80 })
    try {
      const render = (padding: number) =>
        root.render(
          <div style={TEXT}>
            <text style={{ fontFamily: "Menlo" }}>
              AA
              <span testId="x" style={{ paddingLeft: padding, paddingRight: padding }} />
              BB
            </text>
          </div>
        )
      const box = () => {
        const element = root.renderer.findByTestId("x")
        return element ? root.renderer.getElementBounds(element.id) : null
      }
      render(0)
      expect(box()).toBeNull()
      const narrow = root.renderer.getElementBounds(root.renderer.findByTestId("x")?.id ?? 0)
      expect(narrow).toBeNull()
      render(7)
      expect(box()?.width).toBe(14)
      render(0)
      expect(box()).toBeNull()
    } finally {
      root.unmount()
    }
  })
})
