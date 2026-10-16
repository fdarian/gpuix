/**
 * `display: contents` where there is no parent to splice into (the root, a
 * virtual-list row), and the listeners of an element that has no box.
 */
import React from "react"
import { afterEach, beforeEach, describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer, type TestRoot } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("display: contents", () => {
  let testRoot: TestRoot

  beforeEach(() => {
    testRoot = createTestRoot()
  })

  afterEach(() => {
    testRoot.unmount()
  })

  const id = (testId: string) => testRoot.renderer.findByTestId(testId)!.id
  const bounds = (testId: string) => testRoot.renderer.getElementBounds(id(testId))

  describe("as the root", () => {
    it("builds no box and lays its children out as the root's own", () => {
      testRoot.render(
        <div
          testId="root"
          style={{ display: "contents", padding: 30, borderWidth: 4, backgroundColor: "#ff0000", width: 500 }}
        >
          <div testId="a" style={{ width: 50, height: 50 }} />
          <div style={{ display: "contents" }}>
            <div testId="b" style={{ width: 50, height: 50 }} />
          </div>
        </div>
      )

      expect(bounds("root")).toBeNull()
      expect(bounds("a")).toMatchObject({ x: 0, y: 0, width: 50, height: 50 })
      expect(bounds("b")).toMatchObject({ x: 0, y: 50 })
    })

    it("is not a hit target: a pointer over a child hits the child", () => {
      testRoot.render(
        <div testId="root" style={{ display: "contents" }}>
          <div testId="a" style={{ width: 50, height: 50 }} />
        </div>
      )

      const hits = testRoot.renderer.getElementsAt(10, 10)
      expect(hits[0]).toBe(id("a"))
      expect(hits).not.toContain(id("root"))
    })

    it("passes its text style down to its text", () => {
      testRoot.render(
        <div style={{ display: "contents", color: "#ff0000", fontSize: 16, lineHeight: "20px" }}>
          <div testId="box" style={{ width: 200, backgroundColor: "#ffffff" }}>
            redredred
          </div>
        </div>
      )

      const box = bounds("box")!
      const file = `${SHOTS_DIR}/display-contents-root-${process.pid}.png`
      testRoot.renderer.captureScreenshot(file)
      const pixels = readLogicalPixels(file, testRoot.renderer)
      let red = 0
      let dark = 0
      for (let y = Math.floor(box.y); y < Math.ceil(box.y + box.height); y++) {
        for (let x = Math.floor(box.x); x < Math.ceil(box.x + box.width); x++) {
          const [r, g, b] = pixels.at(x, y)
          if (r > 200 && g < 60 && b < 60) red++
          if (r < 60 && g < 60 && b < 60) dark++
        }
      }
      expect(red).toBeGreaterThan(20)
      expect(dark).toBe(0)
    })

    it("still hears the events that pass through it", () => {
      const seen: string[] = []
      testRoot.render(
        <div
          testId="root"
          style={{ display: "contents" }}
          onClick={(event) => seen.push(`click ${event.currentTarget.id === id("root")}`)}
        >
          <div testId="a" style={{ width: 50, height: 50 }} />
        </div>
      )

      testRoot.renderer.nativeSimulateClick(10, 10)

      expect(seen).toEqual(["click true"])
    })
  })

  describe("as a virtual-list row", () => {
    it("builds no box and stacks its children in the row", () => {
      testRoot.render(
        <virtual-list overdraw={0} estimatedItemHeight={40} style={{ width: 400, height: 200 }}>
          {[0, 1].map((row) => (
            <div key={row} testId={`row-${row}`} style={{ display: "contents", padding: 10, backgroundColor: "#ff0000" }}>
              <div testId={`row-${row}-a`} style={{ height: 20, flexShrink: 0 }} />
              <div testId={`row-${row}-b`} style={{ height: 20, flexShrink: 0 }} />
            </div>
          ))}
        </virtual-list>
      )

      expect(bounds("row-0")).toBeNull()
      expect(bounds("row-1")).toBeNull()
      expect(bounds("row-0-a")).toMatchObject({ x: 0, y: 0, width: 400, height: 20 })
      expect(bounds("row-0-b")).toMatchObject({ y: 20, height: 20 })
      expect(bounds("row-1-a")).toMatchObject({ y: 40 })
    })
  })

  describe("listeners of an element with no box", () => {
    function render(handlers: Record<string, (event: any) => void>) {
      testRoot.render(
        <div style={{ width: 200, height: 100 }}>
          <div testId="contents" style={{ display: "contents" }} {...handlers}>
            <div testId="a" tabIndex={0} style={{ width: 50, height: 50 }} />
            <div testId="b" tabIndex={0} style={{ width: 50, height: 50 }} />
          </div>
          <div testId="outside" tabIndex={0} style={{ width: 50, height: 50 }} />
        </div>
      )
    }

    it("fires bubble and capture click listeners with the element as currentTarget and the hit descendant as target", () => {
      const seen: Array<[string, number, number]> = []
      const record = (name: string) => (event: any) => seen.push([name, event.currentTarget.id, event.target.id])
      render({ onClick: record("bubble"), onClickCapture: record("capture") })

      testRoot.renderer.nativeSimulateClick(10, 10)

      expect(seen).toEqual([
        ["capture", id("contents"), id("a")],
        ["bubble", id("contents"), id("a")],
      ])
    })

    it("is the target of click() on itself", () => {
      const seen: Array<[number, number]> = []
      let contents: { click(): void } | undefined
      testRoot.render(
        <div style={{ width: 200, height: 100 }}>
          <div
            ref={(instance) => {
              if (instance) contents = instance as unknown as { click(): void }
            }}
            testId="contents"
            style={{ display: "contents" }}
            onClick={(event) => seen.push([event.currentTarget.id, event.target.id])}
          >
            <div style={{ width: 50, height: 50 }} />
          </div>
        </div>
      )

      contents!.click()
      testRoot.renderer.dispatchNativeEvents()

      expect(seen).toEqual([[id("contents"), id("contents")]])
    })

    it("hears pointer, mouse and key events of its descendants", () => {
      const seen: string[] = []
      const record = (name: string) => () => seen.push(name)
      render({
        onPointerDown: record("pointerdown"),
        onMouseDown: record("mousedown"),
        onKeyDown: record("keydown"),
        onFocus: record("focus"),
        onBlur: record("blur"),
      })

      testRoot.renderer.nativeSimulateClick(10, 10)
      testRoot.renderer.simulateKeystrokes("x")
      testRoot.renderer.focusElement(id("outside"))

      expect(seen).toEqual(["pointerdown", "mousedown", "focus", "keydown", "blur"])
    })

    it("gets one enter when the pointer enters any descendant and one leave when it leaves them all", () => {
      const seen: string[] = []
      const record = (name: string) => () => seen.push(name)
      render({
        onPointerEnter: record("pointerenter"),
        onPointerLeave: record("pointerleave"),
        onMouseEnter: record("mouseenter"),
        onMouseLeave: record("mouseleave"),
      })
      const move = (x: number, y: number) => testRoot.renderer.nativeSimulateMouseMove(x, y)

      move(10, 10)
      expect(seen).toEqual(["pointerenter", "mouseenter"])
      seen.length = 0
      move(10, 70)
      expect(seen, "moving from one descendant to the next stays inside").toEqual([])
      move(10, 120)
      expect(seen).toEqual(["pointerleave", "mouseleave"])
    })
  })
})
