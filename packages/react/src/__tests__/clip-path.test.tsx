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

  it("path() fills the polygon its data draws", () => {
    // The upper right half of the box, the points being px from its top left.
    const triangle = shoot("path-triangle", clipped({ clipPath: "path('M0 0 L100 0 L100 100 Z')" })).pixels
    expectRed(triangle, 140, 60)
    expectRed(triangle, 120, 100)
    expectWhite(triangle, 60, 140)
    expectWhite(triangle, 80, 120)
    const halfDisc = shoot("path-arc", clipped({ clipPath: "path('M0 50 A50 50 0 0 1 100 50 Z')" })).pixels
    expectRed(halfDisc, 100, 70)
    expectRed(halfDisc, 100, 56)
    expectWhite(halfDisc, 100, 130)
    expectWhite(halfDisc, 53, 53)
    const curve = shoot("path-curve", clipped({ clipPath: "path('M0 100 C0 0 100 0 100 100 Z')" })).pixels
    expectRed(curve, 100, 130)
    expectWhite(curve, 55, 60)
    expectWhite(curve, 145, 60)
  })

  it("path() fills its subpaths under the fill rule, with no seam between them", () => {
    const rings = "M0 0 H100 V100 H0 Z M25 25 H75 V75 H25 Z"
    const nonzero = shoot("path-nonzero", clipped({ clipPath: `path(nonzero, '${rings}')` })).pixels
    expectRed(nonzero, 100, 100)
    // On the diagonal from the first corner of one square to the first corner of the next, where
    // an edge drawn between them would leave a half-covered line.
    expectRed(nonzero, 60, 60)
    const evenodd = shoot("path-evenodd", clipped({ clipPath: `path(evenodd, '${rings}')` })).pixels
    expectWhite(evenodd, 100, 100)
    expectRed(evenodd, 60, 60)
    expectRed(evenodd, 140, 140)
    // Two separate squares: the bridge between them paints nothing.
    const apart = shoot("path-apart", clipped({ clipPath: "path('M0 0 H30 V30 H0 Z M70 70 H100 V100 H70 Z')" })).pixels
    expectRed(apart, 60, 60)
    expectRed(apart, 140, 140)
    expectWhite(apart, 100, 100)
    expectWhite(apart, 140, 60)
    expectWhite(apart, 60, 140)
  })

  it("path() is relative to the reference box", () => {
    const padded = { boxSizing: "border-box", padding: 20, borderWidth: 0 }
    const shot = shoot("path-content-box", clipped({ ...padded, clipPath: "path('M0 0 H20 V20 H0 Z') content-box" })).pixels
    // The content box starts at (70, 70).
    expectRed(shot, 80, 80)
    expectWhite(shot, 60, 60)
    expectWhite(shot, 95, 95)
  })

  it("xywh() is a position and a size from the top left", () => {
    const shot = shoot("xywh", clipped({ clipPath: "xywh(25% 10px 50% 30px)" })).pixels
    // x 75..125, y 60..90.
    expectRed(shot, 100, 75)
    expectRed(shot, 80, 62)
    expectWhite(shot, 70, 75)
    expectWhite(shot, 130, 75)
    expectWhite(shot, 100, 55)
    expectWhite(shot, 100, 95)
    const rounded = shoot("xywh-round", clipped({ clipPath: "xywh(10px 10px 80px 80px round 40px)" })).pixels
    expectRed(rounded, 100, 100)
    expectRed(rounded, 100, 62)
    expectWhite(rounded, 62, 62)
  })

  it("rect() measures right and bottom from the top left, unlike inset()", () => {
    const shot = shoot("rect", clipped({ clipPath: "rect(10px 90px 90% 10px)" })).pixels
    // x 60..140, y 60..140.
    expectRed(shot, 65, 65)
    expectRed(shot, 135, 135)
    expectWhite(shot, 55, 100)
    expectWhite(shot, 145, 100)
    expectWhite(shot, 100, 145)
    const auto = shoot("rect-auto", clipped({ clipPath: "rect(auto 50% auto auto)" })).pixels
    expectRed(auto, 70, 100)
    expectWhite(auto, 130, 100)
    // An inset of the same numbers is a different box.
    const inset = shoot("rect-inset", clipped({ clipPath: "inset(10px 90px 90% 10px)" })).pixels
    expectWhite(inset, 100, 100)
  })

  it("an invalid value and an unsupported one leave the element alone", () => {
    const plain = shoot("plain-2", clipped({}))
    for (const [name, value] of [
      ["url", "url(#clip)"],
      ["path", "path('M0 0 L10')"],
      ["rect", "rect(0 10px 10px)"],
      ["xywh", "xywh(0 0 -5px 5px)"],
      ["xywh-mixed-units", "xywh(10px 0 50% 5px)"],
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

describeNative("path(), rect() and xywh() clip the pointer", () => {
  /** Clicks (x, y) on a clicked 100x100 box at (50, 50) clipped to `clipPath`, above a clicked sibling. */
  function clickAt(clipPath: string, x: number, y: number) {
    const { render, renderer, unmount } = createTestRoot()
    const log: string[] = []
    try {
      render(
        <div style={{ position: "relative", width: 200, height: 200 }}>
          <div style={{ position: "absolute", width: 200, height: 200 }} onClick={() => log.push("under")} />
          <div
            style={{ position: "absolute", left: 50, top: 50, width: 100, height: 100, backgroundColor: "#ff0000", clipPath }}
            onClick={() => log.push("clipped")}
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

  it("a press outside the path falls through to what is underneath", () => {
    const triangle = "path('M0 0 L100 0 L100 100 Z')"
    expect(clickAt(triangle, 140, 60)).toEqual(["child", "clipped"])
    expect(clickAt(triangle, 60, 140)).toEqual(["under"])
  })

  it("the fill rule decides what a hole in the path hits", () => {
    const rings = "M0 0 H100 V100 H0 Z M25 25 H75 V75 H25 Z"
    expect(clickAt(`path(nonzero, '${rings}')`, 100, 100)).toEqual(["child", "clipped"])
    expect(clickAt(`path(evenodd, '${rings}')`, 100, 100)).toEqual(["under"])
    expect(clickAt(`path(evenodd, '${rings}')`, 60, 60)).toEqual(["child", "clipped"])
  })

  it("rect() and xywh() cut the hit area", () => {
    expect(clickAt("rect(10px 90px 90% 10px)", 100, 100)).toEqual(["child", "clipped"])
    expect(clickAt("rect(10px 90px 90% 10px)", 145, 100)).toEqual(["under"])
    expect(clickAt("xywh(25% 10px 50% 30px)", 100, 75)).toEqual(["child", "clipped"])
    expect(clickAt("xywh(25% 10px 50% 30px)", 100, 110)).toEqual(["under"])
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

const SVG_DATA_URL = `data:image/svg+xml;base64,${Buffer.from(
  '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="#00aa00"/></svg>'
).toString("base64")}`

describeNative("clipPath clips the pointer on every kind of element", () => {
  type Box = Record<string, unknown>
  type Factory = (box: Box, onClick: () => void) => ReactNode

  /** What the 100x100 box at (50, 50) is: every host element that has a box of its own. */
  const elements: Array<[string, Factory]> = [
    ["div", (box, onClick) => <div style={box} onClick={onClick} />],
    ["scrolling div", (box, onClick) => (
      <div style={{ ...box, overflow: "scroll" }} onClick={onClick}>
        <div style={{ width: 100, height: 300 }} />
      </div>
    )],
    ["text", (box, onClick) => <text style={box} onClick={onClick}>clipped text</text>],
    ["img", (box, onClick) => <img src={SVG_DATA_URL} style={box} onClick={onClick} />],
    ["img without a source", (box, onClick) => <img style={box} onClick={onClick} />],
    ["svg", (box, onClick) => (
      <svg viewBox="0 0 10 10" style={box} onClick={onClick}>
        <rect width="10" height="10" fill="#00aa00" />
      </svg>
    )],
    ["input", (box, onClick) => <input style={box} onClick={onClick} />],
    ["textarea", (box, onClick) => <textarea style={box} onClick={onClick} />],
    ["markdown", (box, onClick) => <markdown source={"# Title\n\nBody"} style={box} onClick={onClick} />],
    ["code", (box, onClick) => <code code={"const a = 1"} style={box} onClick={onClick} />],
    ["diff", (box, onClick) => <diff patch="" style={box} onClick={onClick} />],
    ["anchored", (box, onClick) => (
      <div style={{ width: 1, height: 1 }}>
        <anchored
          position={{ x: 50, y: 50 }}
          anchor="topLeft"
          style={{ width: box.width, height: box.height, clipPath: box.clipPath }}
          onClick={onClick}
        >
          <div style={{ width: 100, height: 100 }} />
        </anchored>
      </div>
    )],
    ["virtual-list", (box, onClick) => (
      <virtual-list overdraw={0} estimatedItemHeight={40} style={box}>
        {Array.from({ length: 5 }, (_, index) => (
          <div key={index} style={{ height: 40, flexShrink: 0 }} onClick={onClick} />
        ))}
      </virtual-list>
    )],
  ]

  /** Clicks (x, y) on a stage that holds `element`, circle-clipped, over a clicked sibling. */
  function clickAt(factory: Factory, x: number, y: number) {
    const { render, renderer, unmount } = createTestRoot()
    const log: string[] = []
    try {
      render(
        <div style={{ position: "relative", width: 200, height: 200 }}>
          <div style={{ position: "absolute", width: 200, height: 200 }} onClick={() => log.push("under")} />
          {factory(
            { position: "absolute", left: 50, top: 50, width: 100, height: 100, clipPath: "circle()" },
            () => log.push("clipped")
          )}
        </div>
      )
      renderer.flush()
      renderer.dispatchNativeEvents()
      renderer.nativeSimulateClick(x, y)
      return log
    } finally {
      unmount()
    }
  }

  for (const [name, factory] of elements) {
    it(`${name}: a press in the corner the circle cuts off falls through`, () => {
      expect(clickAt(factory, 52, 52)).toEqual(["under"])
      expect(clickAt(factory, 148, 148)).toEqual(["under"])
    })

    it(`${name}: a press inside the circle hits it`, () => {
      expect(clickAt(factory, 100, 100)[0]).toBe("clipped")
    })
  }

  it("a virtual list paints clipped, with its rows", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      render(
        <div style={{ position: "relative", width: 200, height: 200, backgroundColor: "#ffffff" }}>
          <virtual-list
            overdraw={0}
            estimatedItemHeight={40}
            style={{
              position: "absolute",
              left: 50,
              top: 50,
              width: 100,
              height: 100,
              backgroundColor: "#ff0000",
              clipPath: "circle()",
            }}
          >
            <div style={{ height: 20, flexShrink: 0, backgroundColor: "#0000ff" }} />
            <div style={{ height: 80, flexShrink: 0 }} />
          </virtual-list>
        </div>
      )
      renderer.dispatchNativeEvents()
      const file = path.join(SHOTS_DIR, "clip-path-virtual-list.png")
      renderer.captureScreenshot(file)
      const pixels = readLogicalPixels(file, renderer)
      // The first row is cut by the circle at the top left, and the list background shows below it.
      expect(pixels.at(100, 60)).toEqual([0, 0, 255, 255])
      expect(pixels.at(53, 53)).toEqual([255, 255, 255, 255])
      expectRed(pixels, 100, 120)
      expect(pixels.at(53, 147)).toEqual([255, 255, 255, 255])
    } finally {
      unmount()
    }
  })

  it("a virtual list's scrollbar is cut with it", () => {
    const { render, renderer, unmount } = createTestRoot()
    const log: string[] = []
    try {
      render(
        <div style={{ position: "relative", width: 200, height: 200 }}>
          <div style={{ position: "absolute", width: 200, height: 200 }} onClick={() => log.push("under")} />
          <virtual-list
            testId="list"
            overdraw={0}
            estimatedItemHeight={40}
            style={{ position: "absolute", left: 50, top: 50, width: 100, height: 100, clipPath: "circle()" }}
          >
            {Array.from({ length: 10 }, (_, index) => (
              <div key={index} style={{ height: 40, flexShrink: 0 }} />
            ))}
          </virtual-list>
        </div>
      )
      renderer.dispatchNativeEvents()
      renderer.nativeSimulateScrollWheel(100, 100, 0, 40)
      // The bar runs along the right edge of the list, which the circle cuts away near the top.
      renderer.nativeSimulateClick(144, 54)
      expect(log).toEqual(["under"])
    } finally {
      unmount()
    }
  })
})

