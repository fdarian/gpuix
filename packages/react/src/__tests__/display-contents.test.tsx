/**
 * `display: contents` where there is no parent to splice into (the root, a
 * virtual-list row), and the listeners of an element that has no box.
 */
import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"
import React from "react"
import { afterEach, beforeEach, describe, expect, it } from "vitest"
import { defineKeyframes, motion } from "../index"
import { createTestRoot, hasNativeTestRenderer, type TestRoot } from "../testing.js"
import type { PublicInstance } from "../types/host.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

/** What Chrome reports for the same shapes: `fixtures/display-contents-oracle.html`. */
const oracle = JSON.parse(
  fs.readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/display-contents-oracle.json"), "utf8")
)

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const RED = "#ff0000"
const BLUE = "#0000ff"
const LINEAR = { duration: 1, ease: "linear" } as const

defineKeyframes("display-contents-ink", { from: { color: RED }, to: { color: BLUE } })

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
          style={{
            display: "contents",
            padding: 30,
            borderWidth: 4,
            borderStyle: "solid",
            backgroundColor: "#ff0000",
            width: 500,
          }}
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
  describe("what an element with no box lets a reader see", () => {
    const focused = () => testRoot.renderer.getElement(testRoot.renderer.getFocusedElementId())?.testId ?? null

    it("answers a zero rect from getBoundingClientRect, as the root and spliced into a parent", () => {
      const instances: Record<string, PublicInstance> = {}
      const ref = (name: string) => (instance: PublicInstance | null) => {
        if (instance) instances[name] = instance
      }
      testRoot.render(
        <div ref={ref("root")} style={{ display: "contents" }}>
          <div ref={ref("spliced")} style={{ display: "contents" }}>
            <div style={{ width: 50, height: 50 }} />
          </div>
        </div>
      )

      const [x, y, width, height] = oracle.rect
      const zero = { x, y, width, height, top: y, left: x, right: x + width, bottom: y + height }
      expect(instances.root!.getBoundingClientRect()).toEqual(zero)
      expect(instances.spliced!.getBoundingClientRect()).toEqual(zero)
    })

    it("cannot be focused, takes no Tab stop, and leaves its children their own", () => {
      testRoot.render(
        <div style={{ width: 200, height: 100 }}>
          <div testId="contents" tabIndex={0} style={{ display: "contents" }}>
            <div testId="a" tabIndex={0} style={{ width: 50, height: 20 }} />
          </div>
          <div testId="b" tabIndex={0} style={{ width: 50, height: 20 }} />
        </div>
      )

      testRoot.renderer.focusElement(id("contents"))
      expect(focused()).toBe(null)
      expect(oracle.rowFocusable).toBe(false)

      testRoot.renderer.simulateKeystrokes("Tab")
      expect(focused()).toBe("a")
      testRoot.renderer.simulateKeystrokes("Tab")
      expect(focused()).toBe("b")
    })

    it("can be focused again once it stops being display: contents", () => {
      const app = (display: "contents" | "block") => (
        <div style={{ width: 200, height: 100 }}>
          <div testId="element" tabIndex={0} style={{ display, height: 20 }} />
        </div>
      )
      testRoot.render(app("contents"))
      testRoot.renderer.focusElement(id("element"))
      expect(focused()).toBe(null)

      testRoot.render(app("block"))
      testRoot.renderer.focusElement(id("element"))
      expect(focused()).toBe("element")

      testRoot.render(app("contents"))
      testRoot.renderer.flush()
      expect(focused(), "a focused element that loses its box blurs").toBe(null)
    })
  })

  describe("what moves on it reaches its children", () => {
    let shots = 0
    /** The colour of the darkest pixel of the glyphs in a white box: what the text is painted with. */
    const ink = (testId: string) => {
      const box = bounds(testId)!
      const file = `${SHOTS_DIR}/display-contents-ink-${process.pid}-${shots++}.png`
      testRoot.renderer.captureScreenshot(file)
      const pixels = readLogicalPixels(file, testRoot.renderer)
      let darkest = [255, 255, 255]
      for (let y = Math.floor(box.y); y < Math.ceil(box.y + box.height); y++) {
        for (let x = Math.floor(box.x); x < Math.ceil(box.x + box.width); x++) {
          const pixel = pixels.at(x, y)
          if (pixel[1] < darkest[1]) darkest = [pixel[0], pixel[1], pixel[2]]
        }
      }
      return { red: darkest[0], blue: darkest[2] }
    }
    const word = (style: object = {}) => (
      <div testId="word" style={{ width: 300, backgroundColor: "#ffffff", fontSize: 32, fontWeight: "bold", fontFamily: "Menlo", ...style }}>
        WWWW
      </div>
    )
    const after = (ms: number) => {
      testRoot.renderer.clockFastForward(ms)
      testRoot.renderer.dispatchNativeEvents()
    }

    it("follows a motion of color, half way and settled", () => {
      testRoot.renderer.clockPause()
      testRoot.render(
        <motion.div style={{ display: "contents" }} initial={{ color: RED }} animate={{ color: BLUE }} transition={LINEAR}>
          {word()}
        </motion.div>
      )
      expect(ink("word")).toMatchObject({ red: 255, blue: 0 })

      after(500)
      const half = ink("word")
      expect(half.red).toBeGreaterThan(100)
      expect(half.red).toBeLessThan(200)
      expect(half.blue).toBeGreaterThan(100)
      expect(half.blue).toBeLessThan(200)

      after(600)
      expect(ink("word")).toMatchObject({ red: 0, blue: 255 })
    })

    it("follows a motion of color through a wrapper that has nothing to move", () => {
      testRoot.renderer.clockPause()
      testRoot.render(
        <div style={{ display: "contents", color: "#00ff00" }}>
          <motion.div style={{ display: "contents" }} initial={{ color: RED }} animate={{ color: BLUE }} transition={LINEAR}>
            <div style={{ display: "contents" }}>{word()}</div>
          </motion.div>
        </div>
      )
      after(500)
      const half = ink("word")
      expect(half.red).toBeGreaterThan(100)
      expect(half.blue).toBeGreaterThan(100)
      after(600)
      expect(ink("word")).toMatchObject({ red: 0, blue: 255 })
    })

    it("follows a motion of color to the text it holds itself", () => {
      testRoot.renderer.clockPause()
      testRoot.render(
        <div testId="word" style={{ width: 300, backgroundColor: "#ffffff", fontSize: 32, fontWeight: "bold", fontFamily: "Menlo" }}>
          <motion.div style={{ display: "contents" }} initial={{ color: RED }} animate={{ color: BLUE }} transition={LINEAR}>
            WWWW
          </motion.div>
        </div>
      )
      after(500)
      expect(ink("word").red).toBeGreaterThan(100)
      expect(ink("word").blue).toBeGreaterThan(100)
      after(600)
      expect(ink("word")).toMatchObject({ red: 0, blue: 255 })
    })

    it("follows a CSS transition of color that a style change starts", () => {
      testRoot.renderer.clockPause()
      const app = (color: string) => (
        <div style={{ display: "contents", color, transition: "color 1s linear" }}>{word()}</div>
      )
      testRoot.render(app(RED))
      expect(ink("word")).toMatchObject({ red: 255, blue: 0 })

      testRoot.render(app(BLUE))
      after(500)
      const half = ink("word")
      expect(half.red).toBeGreaterThan(100)
      expect(half.red).toBeLessThan(200)
      expect(half.blue).toBeGreaterThan(100)

      after(600)
      expect(ink("word")).toMatchObject({ red: 0, blue: 255 })
    })

    it("follows a keyframes animation of color", () => {
      testRoot.renderer.clockPause()
      testRoot.render(<div style={{ display: "contents", animation: "display-contents-ink 1s linear forwards" }}>{word()}</div>)
      expect(ink("word")).toMatchObject({ red: 255, blue: 0 })

      after(500)
      const half = ink("word")
      expect(half.red).toBeGreaterThan(100)
      expect(half.red).toBeLessThan(200)
      expect(half.blue).toBeGreaterThan(100)

      after(600)
      expect(ink("word")).toMatchObject({ red: 0, blue: 255 })
    })

    it("lays its children out at an animated fontSize", () => {
      testRoot.renderer.clockPause()
      testRoot.render(
        <motion.div style={{ display: "contents" }} initial={{ fontSize: 16 }} animate={{ fontSize: 48 }} transition={LINEAR}>
          <div testId="word" style={{ width: 300 }}>
            WWWW
          </div>
        </motion.div>
      )
      const start = bounds("word")!.height
      after(500)
      const half = bounds("word")!.height
      after(600)
      const end = bounds("word")!.height
      expect(half).toBeGreaterThan(start)
      expect(end).toBeGreaterThan(half)
    })

    it("does nothing with an opacity: it has no box to fade", () => {
      testRoot.renderer.clockPause()
      testRoot.render(
        <motion.div style={{ display: "contents" }} initial={{ opacity: 1 }} animate={{ opacity: 0 }} transition={LINEAR}>
          {word({ color: RED })}
        </motion.div>
      )
      after(500)
      expect(ink("word")).toMatchObject({ red: 255, blue: 0 })
      after(600)
      expect(ink("word")).toMatchObject({ red: 255, blue: 0 })
    })
  })
})
