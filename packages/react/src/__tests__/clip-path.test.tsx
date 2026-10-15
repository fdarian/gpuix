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
    const file = path.join(SHOTS_DIR, `clip-path-${name}.png`)
    renderer.captureScreenshot(file)
    return { file, pixels: readLogicalPixels(file, renderer) }
  } finally {
    unmount()
  }
}

/** A white 200x200 stage with a red 100x100 box at (50, 50). */
function clipped(style: Record<string, unknown>, children?: ReactNode) {
  return (
    <div style={{ position: "relative", width: 200, height: 200, backgroundColor: "#ffffff" }}>
      <div
        style={{
          position: "absolute",
          left: 50,
          top: 50,
          width: 100,
          height: 100,
          backgroundColor: "#ff0000",
          ...style,
        }}
      >
        {children}
      </div>
    </div>
  )
}

type Pixels = ReturnType<typeof shoot>["pixels"]

function expectRed(pixels: Pixels, x: number, y: number) {
  const [r, g, b] = pixels.at(x, y)
  expect(r).toBe(255)
  expect(g).toBeLessThanOrEqual(4)
  expect(b).toBeLessThanOrEqual(4)
}

function expectWhite(pixels: Pixels, x: number, y: number) {
  expect(pixels.at(x, y)).toEqual([255, 255, 255, 255])
}

describeNative("clip-path", () => {
  it("circle() clips the corners and keeps the middle", () => {
    const shot = shoot("circle", clipped({ clipPath: "circle()" })).pixels
    expectRed(shot, 100, 100)
    expectRed(shot, 70, 70)
    expectRed(shot, 100, 54)
    expectRed(shot, 54, 100)
    expectWhite(shot, 53, 53)
    expectWhite(shot, 147, 53)
    expectWhite(shot, 53, 147)
    expectWhite(shot, 147, 147)
  })

  it("circle() takes a radius and a position", () => {
    const shot = shoot("circle-at", clipped({ clipPath: "circle(30px at 25% 75%)" })).pixels
    expectRed(shot, 75, 125)
    expectRed(shot, 75, 105)
    expectWhite(shot, 100, 100)
    expectWhite(shot, 140, 60)
    expectWhite(shot, 75, 155)
  })

  it("a polygon triangle shows only the triangle", () => {
    const shot = shoot("polygon", clipped({ clipPath: "polygon(50% 0, 100% 100%, 0 100%)" })).pixels
    expectRed(shot, 100, 55)
    expectRed(shot, 100, 120)
    expectRed(shot, 56, 145)
    expectRed(shot, 144, 145)
    expectWhite(shot, 56, 56)
    expectWhite(shot, 144, 56)
    expectWhite(shot, 60, 70)
  })

  it("inset() with round hides the inset band and the rounded corners", () => {
    const rounded = shoot("inset-round", clipped({ clipPath: "inset(10px round 20px)" })).pixels
    expectRed(rounded, 100, 100)
    expectRed(rounded, 100, 62)
    expectRed(rounded, 62, 100)
    expectWhite(rounded, 100, 55)
    expectWhite(rounded, 55, 100)
    // The corner pixel is inside the inset rectangle but outside the rounded corner.
    expectWhite(rounded, 62, 62)
    const square = shoot("inset-square", clipped({ clipPath: "inset(10px)" })).pixels
    expectRed(square, 62, 62)
    expectWhite(square, 55, 100)
  })

  it("the reference box chooses what the lengths are relative to", () => {
    const padded = {
      boxSizing: "border-box",
      padding: 20,
      borderWidth: 0,
    }
    const border = shoot("box-border", clipped({ ...padded, clipPath: "inset(0)" })).pixels
    expectRed(border, 55, 100)
    const content = shoot("box-content", clipped({ ...padded, clipPath: "inset(0) content-box" })).pixels
    expectWhite(content, 55, 100)
    expectRed(content, 75, 100)
    const leading = shoot("box-leading", clipped({ ...padded, clipPath: "content-box inset(0)" }))
    expectScreenshotsEqual(shoot("box-content-2", clipped({ ...padded, clipPath: "inset(0) content-box" })).file, leading.file)
  })

  it("a clip path clips the children too", () => {
    const shot = shoot(
      "children",
      clipped(
        { backgroundColor: "#ffffff", clipPath: "circle()" },
        <div style={{ width: 100, height: 100, backgroundColor: "#0000ff" }} />
      )
    ).pixels
    expect(shot.at(100, 100)).toEqual([0, 0, 255, 255])
    expectWhite(shot, 53, 53)
  })

  it("none is byte-identical to no clip path", () => {
    const plain = shoot("plain", clipped({}))
    expectScreenshotsEqual(plain.file, shoot("none", clipped({ clipPath: "none" })).file)
  })

  it("an invalid value and an unsupported one leave the element alone", () => {
    const plain = shoot("plain-2", clipped({}))
    for (const [name, value] of [
      ["url", "url(#clip)"],
      ["path", "path('M0 0 L10 10 Z')"],
      ["box-only", "padding-box"],
      ["bad-circle", "circle(-5px)"],
      ["empty-polygon", "polygon()"],
    ]) {
      expectScreenshotsEqual(plain.file, shoot(`invalid-${name}`, clipped({ clipPath: value })).file)
    }
  })
  it("clipPath: none in a hover state clears the base clip", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      render(clipped({ clipPath: "circle()", hover: { clipPath: "none" } }))
      renderer.dispatchNativeEvents()
      const before = path.join(SHOTS_DIR, "clip-path-hover-before.png")
      renderer.nativeSimulateMouseMove(5, 5)
      renderer.captureScreenshot(before)
      expectWhite(readLogicalPixels(before, renderer), 53, 53)
      const after = path.join(SHOTS_DIR, "clip-path-hover-after.png")
      renderer.nativeSimulateMouseMove(100, 100)
      renderer.captureScreenshot(after)
      expectRed(readLogicalPixels(after, renderer), 53, 53)
    } finally {
      unmount()
    }
  })
})

describeNative("clipPath clips the pointer", () => {
  /** Clicks (x, y) and returns the names of the elements that got the click, target first. */
  function clickAt(tree: (record: (name: string) => () => void) => ReactNode, x: number, y: number) {
    const { render, renderer, unmount } = createTestRoot()
    const log: string[] = []
    try {
      render(tree((name) => () => log.push(name)))
      renderer.dispatchNativeEvents()
      renderer.nativeSimulateClick(x, y)
      return log
    } finally {
      unmount()
    }
  }

  /** A clicked 200x200 stage with a clicked, circle-clipped 100x100 box at (50, 50) above a
   *  clicked 200x200 sibling. A child fills the box's top-left corner, outside the circle. */
  const scene = (record: (name: string) => () => void) => (
    <div style={{ position: "relative", width: 200, height: 200 }} onClick={record("stage")}>
      <div style={{ position: "absolute", width: 200, height: 200 }} onClick={record("under")} />
      <div
        style={{
          position: "absolute",
          left: 50,
          top: 50,
          width: 100,
          height: 100,
          backgroundColor: "#ff0000",
          clipPath: "circle()",
        }}
        onClick={record("clipped")}
      >
        <div style={{ width: 30, height: 30, backgroundColor: "#00ff00" }} onClick={record("corner")} />
        <div
          style={{ position: "absolute", left: 40, top: 40, width: 20, height: 20 }}
          onClick={record("centre")}
        />
      </div>
    </div>
  )

  it("an element is hit inside its shape and not outside it", () => {
    expect(clickAt(scene, 100, 100)).toContain("clipped")
    // The corner of the box is outside the circle: the sibling beneath is hit.
    expect(clickAt(scene, 52, 52)).toEqual(["under", "stage"])
  })

  it("the whole subtree is clipped with it", () => {
    expect(clickAt(scene, 100, 100)).toEqual(["centre", "clipped", "stage"])
    // The filled child sits in the corner the circle cuts off.
    expect(clickAt(scene, 60, 60)).toEqual(["under", "stage"])
  })

  it("clipPath: none in a hover state restores the hit area", () => {
    const { render, renderer, unmount } = createTestRoot()
    const log: string[] = []
    try {
      render(
        clipped(
          { clipPath: "circle()", hover: { clipPath: "none" } },
          <div style={{ width: 100, height: 100 }} onClick={() => log.push("child")} />
        )
      )
      renderer.dispatchNativeEvents()
      // Outside the circle, so no hover and no hit on the box or its child.
      renderer.nativeSimulateClick(52, 52)
      expect(log).toEqual([])
      renderer.nativeSimulateMouseMove(100, 100)
      renderer.nativeSimulateClick(52, 52)
      expect(log).toEqual(["child"])
    } finally {
      unmount()
    }
  })
})

describeNative("a rounded overflow clip clips the pointer", () => {
  function clickAt(x: number, y: number) {
    const { render, renderer, unmount } = createTestRoot()
    const log: string[] = []
    try {
      render(
        <div style={{ position: "relative", width: 200, height: 200 }}>
          <div style={{ position: "absolute", width: 200, height: 200 }} onClick={() => log.push("under")} />
          <div
            style={{
              position: "absolute",
              left: 50,
              top: 50,
              width: 100,
              height: 100,
              overflow: "hidden",
              borderRadius: 40,
            }}
          >
            <div style={{ width: 100, height: 100 }} onClick={() => log.push("child")} />
          </div>
        </div>
      )
      renderer.dispatchNativeEvents()
      renderer.nativeSimulateClick(x, y)
      return log
    } finally {
      unmount()
    }
  }

  it("a press in the rounded-away corner falls through to what is underneath", () => {
    expect(clickAt(52, 52)).toEqual(["under"])
    expect(clickAt(148, 148)).toEqual(["under"])
  })

  it("a press inside the arc and along the straight edges reaches the child", () => {
    expect(clickAt(70, 70)).toEqual(["child"])
    expect(clickAt(100, 52)).toEqual(["child"])
  })
})
