import path from "path"
import type { ReactNode } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { expectScreenshotsEqual, readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

function shoot(name: string, tree: ReactNode) {
  const { render, renderer, unmount } = createTestRoot()
  try {
    render(tree)
    renderer.dispatchNativeEvents()
    const file = path.join(SHOTS_DIR, `mask-${name}.png`)
    renderer.captureScreenshot(file)
    return { file, pixels: readLogicalPixels(file, renderer) }
  } finally {
    unmount()
  }
}

/** A white 200x200 stage with a 100x200 red column at (50, 0) inside a masked box. */
function masked(style: Record<string, unknown>) {
  return (
    <div style={{ position: "relative", width: 200, height: 200, backgroundColor: "#ffffff" }}>
      <div style={{ position: "absolute", left: 50, top: 0, width: 100, height: 200, ...style }}>
        <div style={{ width: 100, height: 200, backgroundColor: "#ff0000" }} />
      </div>
    </div>
  )
}

const FADE = "linear-gradient(to bottom, transparent, black 20px, black 80%, transparent)"

/** Asserts the pixel is the unmasked red, within the rounding of a mask's alpha. */
function expectRed(pixels: ReturnType<typeof shoot>["pixels"], x: number, y: number) {
  const [r, g, b] = pixels.at(x, y)
  expect(r).toBe(255)
  expect(g).toBeLessThanOrEqual(4)
  expect(b).toBeLessThanOrEqual(4)
}

describeNative("mask", () => {
  it("a scroll-fade mask fades the top and bottom edge of the content", () => {
    const shot = shoot("fade", masked({ maskImage: FADE })).pixels
    // Fully hidden at the very top, half way up the fade, then solid.
    expect(shot.at(100, 0)[1]).toBeGreaterThan(235)
    const half = shot.at(100, 10)
    expect(half[1]).toBeGreaterThan(90)
    expect(half[1]).toBeLessThan(170)
    expectRed(shot, 100, 30)
    expectRed(shot, 100, 100)
    expectRed(shot, 100, 150)
    // The bottom fade starts at 80% of the height.
    const bottom = shot.at(100, 180)
    expect(bottom[1]).toBeGreaterThan(90)
    expect(bottom[1]).toBeLessThan(170)
    expect(shot.at(100, 199)[1]).toBeGreaterThan(235)
    // Outside the element nothing changes.
    expect(shot.at(20, 100)).toEqual(shot.at(20, 5))
  })

  it("the shorthand and the longhands paint the same", () => {
    const longhand = shoot("fade-longhand", masked({ maskImage: FADE }))
    expectScreenshotsEqual(longhand.file, shoot("fade-shorthand", masked({ mask: FADE })).file)
  })

  it("a no-repeat mask shows only the tile and hides the rest", () => {
    const shot = shoot(
      "tile",
      masked({
        maskImage: "linear-gradient(black, black)",
        maskSize: "40px 40px",
        maskPosition: "20px 30px",
        maskRepeat: "no-repeat",
      })
    ).pixels
    expectRed(shot, 50 + 40, 30 + 20)
    expectRed(shot, 50 + 22, 30 + 2)
    // Left of, above, right of and below the tile are hidden.
    expect(shot.at(50 + 10, 30 + 20)[1]).toBe(255)
    expect(shot.at(50 + 40, 30 - 10)[1]).toBe(255)
    expect(shot.at(50 + 80, 30 + 20)[1]).toBe(255)
    expect(shot.at(50 + 40, 30 + 60)[1]).toBe(255)
  })

  it("repeat-x tiles along one axis only", () => {
    const shot = shoot(
      "repeat-x",
      masked({
        maskImage: "linear-gradient(black, black)",
        maskSize: "20px 20px",
        maskRepeat: "repeat-x",
      })
    ).pixels
    expectRed(shot, 50 + 5, 10)
    expectRed(shot, 50 + 95, 10)
    expect(shot.at(50 + 5, 60)[1]).toBe(255)
  })

  it("maskClip limits the mask to a box and hides the rest", () => {
    const shot = shoot(
      "clip",
      masked({
        maskImage: "linear-gradient(black, black)",
        padding: 20,
        borderWidth: 0,
        maskClip: "content-box",
        maskOrigin: "content-box",
        boxSizing: "border-box",
      })
    ).pixels
    expect(shot.at(50 + 5, 100)[1]).toBe(255)
    expect(shot.at(50 + 50, 100)[1]).toBeLessThan(255)
  })

  it("none is byte-identical to no mask", () => {
    const plain = shoot("plain", masked({}))
    expectScreenshotsEqual(plain.file, shoot("none-image", masked({ maskImage: "none" })).file)
    expectScreenshotsEqual(plain.file, shoot("none-shorthand", masked({ mask: "none" })).file)
  })

  it("an invalid value leaves the element alone", () => {
    const plain = shoot("plain-2", masked({}))
    expectScreenshotsEqual(plain.file, shoot("invalid", masked({ maskImage: "linear-gradient(black)" })).file)
    expectScreenshotsEqual(plain.file, shoot("invalid-shorthand", masked({ mask: "red linear-gradient(black, transparent)" })).file)
  })

  it("maskImage: none in a hover state clears the base mask", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      // The red fill is on the hovered box itself: a filled child would take the
      // hover from it.
      render(
        <div style={{ position: "relative", width: 200, height: 200, backgroundColor: "#ffffff" }}>
          <div
            style={{
              position: "absolute",
              left: 50,
              top: 0,
              width: 100,
              height: 200,
              backgroundColor: "#ff0000",
              maskImage: FADE,
              hover: { maskImage: "none" },
            }}
          />
        </div>
      )
      renderer.dispatchNativeEvents()
      const before = path.join(SHOTS_DIR, "mask-hover-before.png")
      renderer.nativeSimulateMouseMove(5, 100)
      renderer.captureScreenshot(before)
      expect(readLogicalPixels(before, renderer).at(100, 0)[1]).toBeGreaterThan(235)
      const after = path.join(SHOTS_DIR, "mask-hover-after.png")
      renderer.nativeSimulateMouseMove(100, 100)
      renderer.captureScreenshot(after)
      expectRed(readLogicalPixels(after, renderer), 100, 0)
    } finally {
      unmount()
    }
  })
})
