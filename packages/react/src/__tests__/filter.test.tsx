import path from "path"
import type { ReactNode } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { expectScreenshotsEqual, readLogicalPixels, SHOTS_DIR, type Pixels } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

function shoot(name: string, tree: ReactNode) {
  const { render, renderer, unmount } = createTestRoot()
  try {
    render(tree)
    renderer.dispatchNativeEvents()
    const file = path.join(SHOTS_DIR, `filter-${name}.png`)
    renderer.captureScreenshot(file)
    return { file, pixels: readLogicalPixels(file, renderer) }
  } finally {
    unmount()
  }
}

/** A white 200x200 stage, so every sampled pixel has a known backdrop. */
function Stage(props: { children: ReactNode }) {
  return (
    <div style={{ position: "relative", width: 200, height: 200, backgroundColor: "#ffffff" }}>
      {props.children}
    </div>
  )
}

/** A box at (60, 60), 40x40. */
function box(style: Record<string, unknown>) {
  return (
    <Stage>
      <div
        style={{
          position: "absolute",
          left: 60,
          top: 60,
          width: 40,
          height: 40,
          backgroundColor: "#000000",
          ...style,
        }}
      />
    </Stage>
  )
}

function channels(pixels: Pixels, x: number, y: number) {
  const [r, g, b] = pixels.at(x, y)
  return [r, g, b]
}

describeNative("filter", () => {
  it("blur softens an edge that is sharp without it", () => {
    const sharp = shoot("blur-off", box({})).pixels
    const blurred = shoot("blur-on", box({ filter: "blur(4px)" })).pixels
    // Just outside and just inside the left edge.
    expect(sharp.at(57, 80)[0]).toBe(255)
    expect(sharp.at(62, 80)[0]).toBe(0)
    expect(blurred.at(57, 80)[0]).toBeLessThan(235)
    expect(blurred.at(62, 80)[0]).toBeGreaterThan(20)
    // The middle of the box stays dark and the far side stays white.
    expect(blurred.at(80, 80)[0]).toBeLessThan(20)
    expect(blurred.at(20, 20)[0]).toBe(255)
  })

  it("grayscale removes the hue", () => {
    const [r, g, b] = channels(shoot("grayscale", box({ backgroundColor: "#ff0000", filter: "grayscale(1)" })).pixels, 80, 80)
    expect(Math.abs(r! - g!)).toBeLessThanOrEqual(2)
    expect(Math.abs(g! - b!)).toBeLessThanOrEqual(2)
    expect(r).toBeLessThan(120)
    expect(r).toBeGreaterThan(20)
  })

  it("brightness scales the colour", () => {
    const [r, g, b] = channels(
      shoot("brightness", box({ backgroundColor: "#ff8040", filter: "brightness(0.5)" })).pixels,
      80,
      80
    )
    expect(Math.abs(r! - 128)).toBeLessThanOrEqual(4)
    expect(Math.abs(g! - 64)).toBeLessThanOrEqual(4)
    expect(Math.abs(b! - 32)).toBeLessThanOrEqual(4)
  })

  it("a percentage and a chain apply in order", () => {
    const [r, g, b] = channels(
      shoot("chain", box({ backgroundColor: "#ff0000", filter: "invert(100%) brightness(0.5)" })).pixels,
      80,
      80
    )
    // Inverted red is cyan, then halved.
    expect(r).toBeLessThanOrEqual(3)
    expect(Math.abs(g! - 128)).toBeLessThanOrEqual(4)
    expect(Math.abs(b! - 128)).toBeLessThanOrEqual(4)
  })

  it("drop-shadow paints an offset copy of the shape under it", () => {
    const shot = shoot(
      "drop-shadow",
      box({ backgroundColor: "#ff0000", filter: "drop-shadow(10px 10px 0 #0000ff)" })
    ).pixels
    expect(channels(shot, 80, 80)).toEqual([255, 0, 0])
    // Right of and below the box, inside the shadow's offset.
    expect(channels(shot, 105, 105)).toEqual([0, 0, 255])
    // Outside the shadow, and the shadow does not move to the other side.
    expect(channels(shot, 55, 55)).toEqual([255, 255, 255])
    expect(channels(shot, 112, 80)).toEqual([255, 255, 255])
  })

  it("drop-shadow takes currentColor from the element", () => {
    const shot = shoot(
      "drop-shadow-current",
      box({ backgroundColor: "#ff0000", color: "#00ff00", filter: "drop-shadow(10px 10px)" })
    ).pixels
    expect(channels(shot, 105, 105)).toEqual([0, 255, 0])
  })

  it("none and an invalid list both leave the element alone", () => {
    const plain = shoot("plain", box({ backgroundColor: "#336699" }))
    expectScreenshotsEqual(plain.file, shoot("none", box({ backgroundColor: "#336699", filter: "none" })).file)
    expectScreenshotsEqual(
      plain.file,
      shoot("invalid", box({ backgroundColor: "#336699", filter: "blur(4px) sparkle(2)" })).file
    )
  })
})

describeNative("backdropFilter", () => {
  /** Vertical black and white stripes, 4px wide, across the stage. */
  function stripes(overlay: Record<string, unknown> | undefined) {
    return (
      <Stage>
        <div style={{ position: "absolute", left: 0, top: 0, width: 200, height: 200, display: "flex", flexDirection: "row" }}>
          {Array.from({ length: 50 }, (_, index) => (
            <div key={index} style={{ width: 4, height: 200, backgroundColor: index % 2 === 0 ? "#000000" : "#ffffff" }} />
          ))}
        </div>
        {overlay && (
          <div style={{ position: "absolute", left: 60, top: 60, width: 80, height: 80, ...overlay }} />
        )}
      </Stage>
    )
  }

  it("blurs what is behind the element and nothing else", () => {
    const sharp = shoot("backdrop-off", stripes(undefined)).pixels
    const blurred = shoot("backdrop-on", stripes({ backdropFilter: "blur(8px)" })).pixels
    // The stripe at x = 96..100 is black: sampled at its centre, inside the overlay.
    expect(sharp.at(98, 100)[0]).toBe(0)
    expect(blurred.at(98, 100)[0]).toBeGreaterThan(90)
    expect(blurred.at(98, 100)[0]).toBeLessThan(170)
    // Outside the overlay the stripes are untouched.
    expect(blurred.at(18, 100)[0]).toBe(0)
    expect(blurred.at(22, 100)[0]).toBe(255)
  })

  it("does not blur the element's own content", () => {
    const shot = shoot(
      "backdrop-content",
      stripes({
        backdropFilter: "blur(8px)",
        backgroundColor: "rgba(255, 0, 0, 0)",
        borderWidth: 0,
      })
    ).pixels
    expect(shot.at(98, 100)[0]).toBeGreaterThan(90)
    const own = shoot(
      "backdrop-own",
      stripes({ backdropFilter: "blur(8px)", backgroundColor: "#ff0000" })
    ).pixels
    expect(channels(own, 98, 100)).toEqual([255, 0, 0])
  })
})

describeNative("opacity as a group", () => {
  it("overlapping opaque children do not show through each other", () => {
    const shot = shoot(
      "group-opacity",
      <Stage>
        <div style={{ position: "absolute", left: 0, top: 0, width: 200, height: 200, opacity: 0.5 }}>
          <div style={{ position: "absolute", left: 40, top: 40, width: 60, height: 60, backgroundColor: "#ff0000" }} />
          <div style={{ position: "absolute", left: 70, top: 70, width: 60, height: 60, backgroundColor: "#0000ff" }} />
        </div>
      </Stage>
    ).pixels
    const single = channels(shot, 120, 120)
    const overlap = channels(shot, 80, 80)
    // Blue at 50% over white.
    for (let index = 0; index < 3; index++) {
      expect(Math.abs(single[index]! - [128, 128, 255][index]!)).toBeLessThanOrEqual(3)
      expect(Math.abs(overlap[index]! - single[index]!)).toBeLessThanOrEqual(1)
    }
    const red = channels(shot, 50, 50)
    expect(Math.abs(red[0]! - 255)).toBeLessThanOrEqual(1)
    expect(Math.abs(red[1]! - 128)).toBeLessThanOrEqual(3)
  })
  it("filter: none in a hover state clears the base filter", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      render(box({ backgroundColor: "#ff0000", filter: "grayscale(1)", hover: { filter: "none" } }))
      renderer.dispatchNativeEvents()
      const before = path.join(SHOTS_DIR, "filter-hover-before.png")
      renderer.nativeSimulateMouseMove(5, 5)
      renderer.captureScreenshot(before)
      const grey = channels(readLogicalPixels(before, renderer), 80, 80)
      expect(Math.abs(grey[0]! - grey[1]!)).toBeLessThanOrEqual(2)
      const after = path.join(SHOTS_DIR, "filter-hover-after.png")
      renderer.nativeSimulateMouseMove(80, 80)
      renderer.captureScreenshot(after)
      const red = channels(readLogicalPixels(after, renderer), 80, 80)
      expect(red[0]).toBe(255)
      expect(red[1]).toBeLessThanOrEqual(4)
    } finally {
      unmount()
    }
  })
})
