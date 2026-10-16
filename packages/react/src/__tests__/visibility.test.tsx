// @ts-nocheck
/** `visibility: hidden`: keeps its box, paints nothing, inherits, takes no pointer or focus. */

import fs from "node:fs"
import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const RED: [number, number, number, number] = [255, 0, 0, 255]
const BLUE: [number, number, number, number] = [0, 0, 255, 255]

function setup(element: React.ReactElement) {
  const root = createTestRoot({ width: 300, height: 200 })
  root.render(element)
  const { renderer } = root
  const focused = () => renderer.getElement(renderer.getFocusedElementId())?.testId ?? null
  const idOf = (testId: string) => renderer.findByTestId(testId).id
  return { root, renderer, focused, idOf }
}

function pixelsOf(renderer, name: string) {
  const file = `${SHOTS_DIR}/${name}.png`
  fs.mkdirSync(SHOTS_DIR, { recursive: true })
  if (fs.existsSync(file)) fs.unlinkSync(file)
  renderer.captureScreenshot(file)
  return readLogicalPixels(file, renderer)
}

describeNative("visibility: hidden", () => {
  describe("paint", () => {
    it("paints nothing of its own, and its descendants inherit that", () => {
      const { renderer } = setup(
        <div style={{ flexDirection: "row", width: 300, height: 200, backgroundColor: "#ffffff" }}>
          <div style={{ visibility: "hidden", width: 100, height: 100, backgroundColor: "#ff0000" }}>
            <div style={{ width: 50, height: 50, backgroundColor: "#0000ff" }} />
          </div>
        </div>,
      )
      const pixels = pixelsOf(renderer, "visibility-hidden-subtree")
      expect(pixels.at(80, 80)).toEqual([255, 255, 255, 255])
      expect(pixels.at(20, 20)).toEqual([255, 255, 255, 255])
    })

    it("lets a descendant with visibility: visible paint, on the hidden element's box", () => {
      const { renderer } = setup(
        <div style={{ flexDirection: "row", width: 300, height: 200, backgroundColor: "#ffffff" }}>
          <div style={{ flexDirection: "column", visibility: "hidden", width: 100, height: 100, backgroundColor: "#ff0000" }}>
            <div style={{ width: 50, height: 50, backgroundColor: "#00ff00" }} />
            <div style={{ visibility: "visible", marginLeft: 50, width: 50, height: 50, backgroundColor: "#0000ff" }} />
          </div>
        </div>,
      )
      const pixels = pixelsOf(renderer, "visibility-visible-descendant")
      // Blue sits at x 50..100, y 50..100: the first child is 50 high, so the second starts below it.
      expect(pixels.at(75, 75)).toEqual(BLUE)
      expect(pixels.at(80, 20)).toEqual([255, 255, 255, 255])
      expect(pixels.at(20, 20), "the sibling above it inherits hidden").toEqual([255, 255, 255, 255])
    })

    it("paints again when it is shown", () => {
      const app = (visibility: "hidden" | "visible") => (
        <div style={{ flexDirection: "row", width: 300, height: 200, backgroundColor: "#ffffff" }}>
          <div style={{ visibility, width: 100, height: 100, backgroundColor: "#ff0000" }} />
        </div>
      )
      const { root, renderer } = setup(app("hidden"))
      expect(pixelsOf(renderer, "visibility-toggle-hidden").at(50, 50)).toEqual([255, 255, 255, 255])
      root.render(app("visible"))
      expect(pixelsOf(renderer, "visibility-toggle-visible").at(50, 50)).toEqual(RED)
    })
  })

  describe("pointer", () => {
    it("lets the pointer through to what lies under it, with no hover", () => {
      const log: string[] = []
      const { renderer } = setup(
        <div style={{ width: 300, height: 200 }}>
          <div
            style={{ position: "absolute", left: 0, top: 0, width: 100, height: 100 }}
            onClick={() => log.push("under")}
            onPointerEnter={() => log.push("under:enter")}
          />
          <div
            style={{ position: "absolute", left: 0, top: 0, width: 100, height: 100, visibility: "hidden" }}
            onClick={() => log.push("hidden")}
            onPointerEnter={() => log.push("hidden:enter")}
          />
        </div>,
      )

      renderer.nativeSimulateMouseMove(50, 50)
      renderer.nativeSimulateClick(50, 50)

      expect(log).toEqual(["under:enter", "under"])
    })

    it("reaches a visible descendant, and the hidden ancestor still hears the bubbling event", () => {
      const log: string[] = []
      const { renderer } = setup(
        <div style={{ width: 300, height: 200 }}>
          <div
            style={{ flexDirection: "column", visibility: "hidden", width: 100, height: 100 }}
            onClick={() => log.push("hidden")}
          >
            <div style={{ width: 40, height: 40 }} onClick={() => log.push("inherited")} />
            <div
              style={{ visibility: "visible", width: 40, height: 40 }}
              onClick={() => log.push("visible")}
            />
          </div>
        </div>,
      )

      renderer.nativeSimulateClick(20, 20)
      expect(log, "the first child inherits hidden").toEqual([])

      renderer.nativeSimulateClick(20, 60)
      expect(log).toEqual(["visible", "hidden"])
    })

    it("is left out of getElementsAt until a descendant shows again", () => {
      const { renderer, idOf } = setup(
        <div style={{ width: 300, height: 200 }}>
          <div testId="hidden" style={{ flexDirection: "column", visibility: "hidden", width: 100, height: 100 }}>
            <div testId="inherited" style={{ width: 40, height: 40 }} />
            <div testId="visible" style={{ visibility: "visible", width: 40, height: 40 }} />
          </div>
        </div>,
      )

      expect(renderer.getElementsAt(20, 20)).not.toContain(idOf("hidden"))
      expect(renderer.getElementsAt(20, 20)).not.toContain(idOf("inherited"))
      expect(renderer.getElementsAt(20, 60)[0]).toBe(idOf("visible"))
      expect(renderer.getElementsAt(20, 60)).not.toContain(idOf("hidden"))
    })
  })

  describe("focus and accessibility", () => {
    it("is skipped by Tab, and a visible descendant is not", () => {
      const { renderer, focused } = setup(
        <div style={{ width: 300, height: 200 }}>
          <div testId="first" tabIndex={0} style={{ width: 40, height: 20 }} />
          <div testId="hidden" tabIndex={0} style={{ visibility: "hidden", width: 40, height: 20 }}>
            <div testId="inherited" tabIndex={0} style={{ width: 40, height: 20 }} />
            <div testId="visible" tabIndex={0} style={{ visibility: "visible", width: 40, height: 20 }} />
          </div>
          <div testId="last" tabIndex={0} style={{ width: 40, height: 20 }} />
        </div>,
      )

      const order: (string | null)[] = []
      for (let press = 0; press < 3; press++) {
        renderer.simulateKeystrokes("Tab")
        order.push(focused())
      }
      expect(order).toEqual(["first", "visible", "last"])
    })

    it("ignores focus() and takes no click focus", () => {
      const { renderer, focused, idOf } = setup(
        <div style={{ width: 300, height: 200 }}>
          <div testId="hidden" tabIndex={0} style={{ visibility: "hidden", width: 40, height: 20 }} />
        </div>,
      )

      renderer.focusElement(idOf("hidden"))
      renderer.flush()
      expect(focused()).toBe(null)
      renderer.nativeSimulateClick(20, 10)
      expect(focused()).toBe(null)
    })

    it("blurs a focused element when it hides, and may be focused again when shown", () => {
      const app = (visibility: "hidden" | "visible") => (
        <div style={{ width: 300, height: 200 }}>
          <div testId="field" tabIndex={0} style={{ visibility, width: 40, height: 20 }} />
        </div>
      )
      const { root, renderer, focused, idOf } = setup(app("visible"))
      renderer.focusElement(idOf("field"))
      expect(focused()).toBe("field")

      root.render(app("hidden"))
      renderer.flush()
      expect(focused()).toBe(null)

      root.render(app("visible"))
      renderer.flush()
      renderer.focusElement(idOf("field"))
      expect(focused()).toBe("field")
    })

    it("has no accessibility node", () => {
      const { renderer } = setup(
        <div style={{ width: 300, height: 200 }}>
          <div role="button" aria-label="shown" style={{ width: 40, height: 20 }} />
          <div role="button" aria-label="hidden" style={{ visibility: "hidden", width: 40, height: 20 }} />
        </div>,
      )

      const labels = Object.values(renderer.getA11yTree().nodes ?? {}).map((node) => node.aria?.label)
      expect(labels).toContain("shown")
      expect(labels).not.toContain("hidden")
    })
  })
})
