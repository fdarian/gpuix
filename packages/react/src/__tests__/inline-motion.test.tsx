/// The colours of an inline box run CSS transitions and keyframe animations as an
/// element's do. A span has no element to animate, so its paragraph samples them
/// each frame and paints the result: these tests read the pixels it paints.

import fs from "fs"
import path from "path"
import React from "react"
import { describe, expect, it } from "vitest"
import { defineKeyframes } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const LINE = 20
const TEXT = { fontSize: 16, lineHeight: `${LINE}px`, fontFamily: "Menlo", color: "#000000" } as const
const PAGE = { width: 300, height: 120, padding: 20, backgroundColor: "#ffffff" } as const

type Rgb = [number, number, number]

defineKeyframes("inline-motion-wash", {
  from: { backgroundColor: "#ff0000" },
  to: { backgroundColor: "#0000ff" },
})

describeNative("transitions and animations of an inline box's colours", () => {
  function setup(render: (state: string) => React.ReactNode, initial: string) {
    const testRoot = createTestRoot({ width: 300, height: 120 })
    const renderer = testRoot.renderer
    fs.mkdirSync(SHOTS_DIR, { recursive: true })
    const file = path.join(SHOTS_DIR, "inline-motion.png")
    renderer.clockPause()
    testRoot.render(render(initial))
    const paragraph = () => renderer.getElementBounds(renderer.findByTestId("p")!.id)!
    const pixels = () => {
      renderer.dispatchNativeEvents()
      renderer.captureScreenshot(file)
      return readLogicalPixels(file, renderer)
    }
    return {
      testRoot,
      renderer,
      paragraph,
      set: (state: string) => testRoot.render(render(state)),
      /** The pixel `dx` and `dy` logical pixels from the paragraph's top left corner. */
      at: (dx: number, dy: number): Rgb => {
        const box = paragraph()
        return pixels().at(Math.floor(box.x + dx), Math.floor(box.y + dy)).slice(0, 3) as Rgb
      },
      /** The pixel of the paragraph that is the furthest from the white page: the ink. */
      ink: (): Rgb => {
        const box = paragraph()
        const shot = pixels()
        let best: Rgb = [255, 255, 255]
        let bestDistance = 0
        for (let y = Math.floor(box.y); y < box.y + box.height; y++) {
          for (let x = Math.floor(box.x); x < box.x + box.width; x++) {
            const pixel = shot.at(x, y).slice(0, 3) as Rgb
            const distance = pixel.reduce((sum, channel) => sum + (255 - channel), 0)
            if (distance > bestDistance) {
              best = pixel
              bestDistance = distance
            }
          }
        }
        return best
      },
    }
  }

  /** Between red and blue, and far from both. */
  const isBetween = (pixel: Rgb) => {
    expect(pixel[0], `${pixel}`).toBeGreaterThan(70)
    expect(pixel[0], `${pixel}`).toBeLessThan(190)
    expect(pixel[2], `${pixel}`).toBeGreaterThan(70)
    expect(pixel[2], `${pixel}`).toBeLessThan(190)
    expect(pixel[1], `${pixel}`).toBeLessThan(10)
  }

  const colours = { red: "#ff0000", blue: "#0000ff" } as const

  it("fades the background of a <mark> from the old colour to the new one", () => {
    const page = setup(
      (colour) => (
        <div style={PAGE}>
          <text testId="p" style={TEXT}>
            <span
              style={{
                paddingLeft: 12,
                backgroundColor: colours[colour as keyof typeof colours],
                transition: "background-color 1s linear",
              }}
            >
              aaaa
            </span>{" "}
            tail
          </text>
        </div>
      ),
      "red"
    )
    const before = page.paragraph()
    expect(page.at(4, 10)).toEqual([255, 0, 0])

    page.set("blue")
    page.renderer.clockFastForward(500)
    const middle = page.at(4, 10)
    isBetween(middle)
    expect(middle).not.toEqual([255, 0, 0])
    expect(middle).not.toEqual([0, 0, 255])
    // The legacy colours blend in sRGB, as an element's do.
    expect(Math.abs(middle[0] - 128)).toBeLessThan(6)
    expect(page.paragraph(), "a colour moves nothing").toEqual(before)

    page.renderer.clockFastForward(1000)
    expect(page.at(4, 10)).toEqual([0, 0, 255])
    page.testRoot.unmount()
  })

  it("transitions the text colour", () => {
    const page = setup(
      (colour) => (
        <div style={PAGE}>
          <text testId="p" style={{ ...TEXT, fontSize: 64, lineHeight: "70px" }}>
            <span style={{ color: colours[colour as keyof typeof colours], transition: "color 1s linear" }}>
              MM
            </span>
          </text>
        </div>
      ),
      "red"
    )
    expect(page.ink()).toEqual([255, 0, 0])
    page.set("blue")
    page.renderer.clockFastForward(500)
    isBetween(page.ink())
    page.renderer.clockFastForward(1000)
    expect(page.ink()).toEqual([0, 0, 255])
    page.testRoot.unmount()
  })

  it("transitions the colour of a text decoration", () => {
    // White text on a white page: the underline is all there is to see.
    const page = setup(
      (colour) => (
        <div style={PAGE}>
          <text testId="p" style={{ ...TEXT, color: "#ffffff" }}>
            <span
              style={{
                textDecorationLine: "underline",
                textDecorationColor: colours[colour as keyof typeof colours],
                transition: "text-decoration-color 1s linear",
              }}
            >
              aaaaaaaa
            </span>
          </text>
        </div>
      ),
      "red"
    )
    expect(page.ink()).toEqual([255, 0, 0])
    page.set("blue")
    page.renderer.clockFastForward(500)
    isBetween(page.ink())
    page.renderer.clockFastForward(1000)
    expect(page.ink()).toEqual([0, 0, 255])
    page.testRoot.unmount()
  })

  it("runs a keyframe animation on the colour of a span", () => {
    const page = setup(
      () => (
        <div style={PAGE}>
          <text testId="p" style={TEXT}>
            <span style={{ paddingLeft: 12, animation: "inline-motion-wash 1s linear" }}>aaaa</span> tail
          </text>
        </div>
      ),
      "start"
    )
    expect(page.at(4, 10)).toEqual([255, 0, 0])
    page.renderer.clockFastForward(500)
    isBetween(page.at(4, 10))
    page.renderer.clockFastForward(250)
    const later = page.at(4, 10)
    expect(later[2], "the animation goes on to blue").toBeGreaterThan(130)
    expect(later[0]).toBeLessThan(120)
    page.testRoot.unmount()
  })

  it("transitions the colour of the border of an inline box, all sides or one", () => {
    const page = setup(
      (state) => (
        <div style={PAGE}>
          <text testId="p" style={TEXT}>
            <span
              style={{
                borderWidth: 4,
                borderStyle: "solid",
                borderColor: state === "red" ? colours.red : colours.blue,
                borderLeftColor: colours.red,
                transition: "border-color 1s linear",
              }}
            >
              aaaa
            </span>
          </text>
        </div>
      ),
      "red"
    )
    // The top border runs along the whole box, above the content area of the line.
    const top = () => page.at(30, -2)
    expect(top()).toEqual([255, 0, 0])
    page.set("blue")
    page.renderer.clockFastForward(500)
    isBetween(top())
    expect(page.at(1.5, 10), "a side that declares its own colour does not move").toEqual([255, 0, 0])
    page.renderer.clockFastForward(1000)
    expect(top()).toEqual([0, 0, 255])
    page.testRoot.unmount()
  })

  it("shows a new colour at once when the span declares no transition", () => {
    const page = setup(
      (colour) => (
        <div style={PAGE}>
          <text testId="p" style={TEXT}>
            <span style={{ paddingLeft: 12, backgroundColor: colours[colour as keyof typeof colours] }}>aaaa</span>
          </text>
        </div>
      ),
      "red"
    )
    expect(page.at(4, 10)).toEqual([255, 0, 0])
    page.set("blue")
    expect(page.at(4, 10)).toEqual([0, 0, 255])
    page.testRoot.unmount()
  })

  it("reports the transition on the span, bubbling to the paragraph's container", () => {
    const seen: Array<[string, string]> = []
    let span: unknown
    const testRoot = createTestRoot({ width: 300, height: 120 })
    const renderer = testRoot.renderer
    const tree = (colour: string) => (
      <div
        style={PAGE}
        onTransitionEnd={(event) => seen.push([event.propertyName, String(event.target === span)])}
      >
        <text testId="p" style={TEXT}>
          <span
            ref={(instance) => {
              if (instance) span = instance
            }}
            style={{ backgroundColor: colour, transition: "background-color 1s linear" }}
          >
            aaaa
          </span>
        </text>
      </div>
    )
    renderer.clockPause()
    testRoot.render(tree(colours.red))
    testRoot.render(tree(colours.blue))
    renderer.clockFastForward(1500)
    renderer.dispatchNativeEvents()
    expect(seen).toEqual([["background-color", "true"]])
    testRoot.unmount()
  })

  it("does not move the layout properties of a span", () => {
    const testRoot = createTestRoot({ width: 300, height: 120 })
    const renderer = testRoot.renderer
    const events: string[] = []
    const tree = (padding: number) => (
      <div style={PAGE} onTransitionRun={(event) => events.push(event.propertyName)}>
        <text testId="p" style={TEXT}>
          <span style={{ paddingLeft: padding, transition: "all 1s linear" }}>aaaa</span> tail
        </text>
      </div>
    )
    renderer.clockPause()
    testRoot.render(tree(0))
    const narrow = renderer.getElementBounds(renderer.findByTestId("p")!.id)!
    testRoot.render(tree(40))
    renderer.clockFastForward(500)
    renderer.dispatchNativeEvents()
    const halfway = renderer.getElementBounds(renderer.findByTestId("p")!.id)!
    expect(halfway.width).toBe(narrow.width)
    expect(events).toEqual([])
    testRoot.unmount()
  })
})
