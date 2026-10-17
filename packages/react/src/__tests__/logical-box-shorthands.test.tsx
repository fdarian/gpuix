/// `paddingBlock`, `paddingInline`, `marginBlock`, `marginInline`, `insetBlock`, `insetInline`,
/// `borderBlock` and `borderInline` (block is top and bottom, inline is left and right), and the
/// grammar of `border` and the side borders, in the boxes Chrome lays out (the expected numbers
/// were read from headless Chrome): static, and in a `hover` state.
///
/// Where a CSS cascade is decided by the order the declarations are written in, gpuix has a
/// fixed order instead: `margin`, then `marginBlock` and `marginInline`, then `marginTop` and
/// the other sides. The cases here write the declarations in that order.

import React from "react"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { createTestRoot, hasNativeTestRenderer, type TestRoot } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Renderer = TestRoot["renderer"]
type Style = Record<string, unknown>

function box(renderer: Renderer, testId: string) {
  const element = renderer.findByTestId(testId)
  expect(element, `missing testId ${testId}`).toBeDefined()
  const bounds = renderer.getElementBounds(element!.id)
  expect(bounds, `no painted bounds for ${testId}`).toBeDefined()
  return bounds!
}

/** `[x, y, width, height]` of the element, relative to the box `against`. */
function rect(renderer: Renderer, testId: string, against = "scene") {
  const bounds = box(renderer, testId)
  const origin = box(renderer, against)
  return [bounds.x - origin.x, bounds.y - origin.y, bounds.width, bounds.height]
}

function errorsOf(style: Style) {
  const errors: Error[] = []
  const { render, unmount } = createTestRoot({ onUncaughtError: (error) => errors.push(error) })
  try {
    render(<div style={style} />)
    return errors.map((error) => error.message).join("\n")
  } finally {
    unmount()
  }
}

/** A 300x200 box, 16px text, holding `children`. */
function scene(children: React.ReactNode, style: Style = {}) {
  return (
    <div testId="scene" style={{ position: "relative", width: 300, height: 200, fontSize: 16, ...style }}>
      {children}
    </div>
  )
}

/** Lays `ui` out, and reads `read` before and after the pointer moves into `over`. */
function laidOut<T>(ui: React.ReactElement, read: (renderer: Renderer) => T, hover?: { over: string; at: [number, number] }) {
  const { render, renderer, unmount } = createTestRoot()
  try {
    render(ui)
    const at = read(renderer)
    if (!hover) return { at, on: at }
    const target = box(renderer, hover.over)
    renderer.nativeSimulateMouseMove(target.x + hover.at[0], target.y + hover.at[1])
    renderer.clockFastForward(16)
    return { at, on: read(renderer) }
  } finally {
    unmount()
  }
}

const CHILD = { width: 20, height: 10 }
const childAndHost = (renderer: Renderer) => [rect(renderer, "child"), rect(renderer, "host")]

/** A fit-content flex host holding the 20x10 child. */
function hosted(style: Style) {
  return scene(
    <div testId="host" style={{ display: "flex", width: "fit-content", ...style }}>
      <div testId="child" style={CHILD} />
    </div>
  )
}

describeNative("paddingBlock and paddingInline", () => {
  const padded = (style: Style) => laidOut(hosted(style), childAndHost).at

  it("paddingBlock '4px 8px' pads the top and the bottom", () => {
    expect(padded({ paddingBlock: "4px 8px" })).toEqual([
      [0, 4, 20, 10],
      [0, 0, 20, 22],
    ])
  })

  it("takes one value for both ends", () => {
    expect(padded({ paddingBlock: "6px" })).toEqual([
      [0, 6, 20, 10],
      [0, 0, 20, 22],
    ])
    expect(padded({ paddingBlock: 6 })[1]).toEqual([0, 0, 20, 22])
  })

  it("paddingInline '2px 6px' pads the left and the right", () => {
    expect(padded({ paddingInline: "2px 6px" })).toEqual([
      [2, 0, 20, 10],
      [0, 0, 28, 10],
    ])
  })

  it("reads em against the element's font size", () => {
    expect(padded({ paddingInline: "0.5em 1em" })).toEqual([
      [8, 0, 20, 10],
      [0, 0, 44, 10],
    ])
  })

  it("wins over padding on its axis and leaves the other axis to it", () => {
    expect(padded({ padding: 10, paddingInline: 2 })).toEqual([
      [2, 10, 20, 10],
      [0, 0, 24, 30],
    ])
  })

  it("loses to the side longhands", () => {
    expect(padded({ paddingInline: "2px 4px", paddingLeft: 9 })).toEqual([
      [9, 0, 20, 10],
      [0, 0, 33, 10],
    ])
  })

  it("is an error for a value CSS would drop", () => {
    expect(errorsOf({ paddingBlock: "1px 2px 3px" })).toMatch(/one or two values/)
    expect(errorsOf({ paddingInline: "1px auto" })).toMatch(/invalid padding-inline "auto"/)
    expect(errorsOf({ paddingBlock: "" })).toMatch(/one or two values/)
    expect(errorsOf({ hover: { paddingBlock: "1px bogus" } })).toMatch(/invalid padding-block "bogus"/)
  })

  it("is replaced by a state's padding, paddingBlock or side", () => {
    const base = { paddingBlock: 2, paddingInline: 3 }
    const hover = (state: Style) =>
      laidOut(hosted({ ...base, hover: state }), childAndHost, { over: "host", at: [1, 1] }).on[0]
    expect(hover({ paddingBlock: "10px 20px" })).toEqual([3, 10, 20, 10])
    expect(hover({ padding: 5 })).toEqual([5, 5, 20, 10])
    expect(hover({ paddingTop: 7 })).toEqual([3, 7, 20, 10])
  })
})

describeNative("marginBlock and marginInline", () => {
  const placed = (style: Style) =>
    laidOut(
      scene(
        <div testId="host" style={{ display: "flex", width: 300 }}>
          <div testId="child" style={{ width: 100, height: 10, ...style }} />
        </div>
      ),
      childAndHost
    ).at

  it("marginBlock '10px 20px' puts 10px above and 20px below", () => {
    expect(placed({ marginBlock: "10px 20px" })).toEqual([
      [0, 10, 100, 10],
      [0, 0, 300, 40],
    ])
  })

  it("marginInline auto centres a box with a width", () => {
    expect(placed({ marginInline: "auto" })[0]).toEqual([100, 0, 100, 10])
  })

  it("marginInline 'auto 0' pushes it to the right", () => {
    expect(placed({ marginInline: "auto 0" })[0]).toEqual([200, 0, 100, 10])
  })

  it("marginInline '5px 15px' puts 5px on the left", () => {
    expect(placed({ marginInline: "5px 15px" })[0]).toEqual([5, 0, 100, 10])
  })

  it("wins over margin on its axis", () => {
    expect(placed({ margin: 30, marginInline: 7 })).toEqual([
      [7, 30, 100, 10],
      [0, 0, 300, 70],
    ])
  })

  it("loses to the side longhands", () => {
    expect(placed({ marginBlock: "10px 20px", marginTop: 4 })[0]).toEqual([0, 4, 100, 10])
  })

  it("is an error for a value CSS would drop", () => {
    expect(errorsOf({ marginBlock: "0 max-content" })).toMatch(/invalid margin-block "max-content"/)
    expect(errorsOf({ marginInline: "1px 2px 3px" })).toMatch(/one or two values/)
    expect(errorsOf({ hover: { marginInline: "bogus" } })).toMatch(/invalid dimension: bogus/)
  })

  it("is replaced by a state's margin, marginInline or side", () => {
    const base = { marginInline: "auto" }
    const hover = (state: Style) =>
      laidOut(
        scene(
          <div testId="host" style={{ display: "flex", width: 300 }}>
            <div testId="child" style={{ width: 100, height: 10, ...base, hover: state }} />
          </div>
        ),
        childAndHost,
        { over: "child", at: [1, 1] }
      )
    expect(hover({ marginInline: 0 }).at[0]).toEqual([100, 0, 100, 10])
    expect(hover({ marginInline: 0 }).on[0]).toEqual([0, 0, 100, 10])
    expect(hover({ margin: "0 0 0 8px" }).on[0]).toEqual([8, 0, 100, 10])
    expect(hover({ marginLeft: 4 }).on[0]).toEqual([4, 0, 100, 10])
  })
})

describeNative("insetBlock and insetInline", () => {
  const placed = (style: Style) =>
    laidOut(scene(<div testId="child" style={{ position: "absolute", ...style }} />), (renderer) => rect(renderer, "child")).at

  it("insetBlock '10px 20px' is 10px from the top and 20px from the bottom", () => {
    expect(placed({ insetBlock: "10px 20px", insetInline: "5px 15px" })).toEqual([5, 10, 280, 170])
  })

  it("takes one value for both ends", () => {
    expect(placed({ insetBlock: "10px", insetInline: 0 })).toEqual([0, 10, 300, 180])
  })

  it("lets auto leave an end to the box's size", () => {
    expect(placed({ insetBlock: "auto 10px", insetInline: "auto 20px", width: 50, height: 30 })).toEqual([230, 160, 50, 30])
  })

  it("wins over inset on its axis", () => {
    expect(placed({ inset: 0, insetInline: "40px 0" })).toEqual([40, 0, 260, 200])
  })

  it("loses to top, right, bottom and left", () => {
    expect(placed({ insetBlock: "10px 20px", top: 50, insetInline: 0 })).toEqual([0, 50, 300, 130])
  })

  it("is an error for a value CSS would drop", () => {
    expect(errorsOf({ insetBlock: "10px fit-content" })).toMatch(/invalid inset-block "fit-content"/)
    expect(errorsOf({ insetInline: "1px 2px 3px" })).toMatch(/one or two values/)
  })

  it("is replaced by a state's inset, insetBlock or side", () => {
    const hover = (state: Style) =>
      laidOut(
        scene(
          <div testId="child" style={{ position: "absolute", insetBlock: "10px 20px", insetInline: 0, hover: state }} />
        ),
        (renderer) => rect(renderer, "child"),
        { over: "child", at: [10, 20] }
      ).on
    expect(hover({ insetBlock: "30px 40px" })).toEqual([0, 30, 300, 130])
    expect(hover({ inset: 5 })).toEqual([5, 5, 290, 190])
    expect(hover({ bottom: 0 })).toEqual([0, 10, 300, 190])
  })
})

describeNative("borderBlock and borderInline", () => {
  const bordered = (style: Style) => laidOut(hosted(style), childAndHost).at

  it("borderBlock '3px solid red' borders the top and the bottom", () => {
    expect(bordered({ borderBlock: "3px solid red" })).toEqual([
      [0, 3, 20, 10],
      [0, 0, 20, 16],
    ])
  })

  it("borderInline '5px dashed red' borders the left and the right", () => {
    expect(bordered({ borderInline: "5px dashed red" })).toEqual([
      [5, 0, 20, 10],
      [0, 0, 30, 10],
    ])
  })

  it("takes the width, style and colour in any order, and thin for a width", () => {
    expect(bordered({ borderBlock: "dotted thin" })).toEqual([
      [0, 1, 20, 10],
      [0, 0, 20, 12],
    ])
  })

  it("has no width without a style, as the style is none", () => {
    expect(bordered({ borderBlock: "3px red" })[1]).toEqual([0, 0, 20, 10])
  })

  it("wins over border on its axis", () => {
    expect(bordered({ border: "1px solid red", borderInline: "6px solid red" })).toEqual([
      [6, 1, 20, 10],
      [0, 0, 32, 12],
    ])
    expect(bordered({ border: "2px solid red", borderBlock: "none" })).toEqual([
      [2, 0, 20, 10],
      [0, 0, 24, 10],
    ])
  })

  it("loses to a side shorthand and to a side longhand", () => {
    expect(bordered({ borderBlock: "4px solid red", borderTop: "1px solid red" })).toEqual([
      [0, 1, 20, 10],
      [0, 0, 20, 15],
    ])
    expect(bordered({ borderBlock: "4px solid red", borderTopWidth: 9 })).toEqual([
      [0, 9, 20, 10],
      [0, 0, 20, 23],
    ])
  })

  it("paints each axis in its own colour", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      render(<div style={{ width: "fit-content" }}><div style={{ display: "flex", width: "fit-content", borderInline: "5px solid #ff0000", borderBlock: "3px solid #0000ff" }}><div style={{ width: 40, height: 40 }} /></div></div>)
      const file = `${SHOTS_DIR}/logical-border-colours.png`
      renderer.captureScreenshot(file)
      const pixels = readLogicalPixels(file, renderer)
      expect(pixels.at(2, 25)).toEqual([255, 0, 0, 255])
      expect(pixels.at(30, 1)).toEqual([0, 0, 255, 255])
      expect(pixels.at(30, 44)).toEqual([0, 0, 255, 255])
      expect(pixels.at(47, 25)).toEqual([255, 0, 0, 255])
    } finally {
      unmount()
    }
  })

  it("is replaced by a state's border, borderBlock or side shorthand", () => {
    const hover = (state: Style) =>
      laidOut(hosted({ borderBlock: "3px solid red", borderInline: "2px solid red", hover: state }), childAndHost, {
        over: "host",
        at: [4, 4],
      }).on
    expect(hover({ borderBlock: "6px solid red" })[0]).toEqual([2, 6, 20, 10])
    expect(hover({ border: "1px solid red" })[0]).toEqual([1, 1, 20, 10])
    expect(hover({ borderTop: "8px solid red" })[0]).toEqual([2, 8, 20, 10])
  })
})

describeNative("the border shorthand grammar", () => {
  const bordered = (style: Style) => laidOut(hosted(style), childAndHost).at

  it("takes the width, style and colour in any order", () => {
    for (const border of ["solid red 3px", "red solid 3px", "3px red solid", "solid 3px red"]) {
      expect(bordered({ border }), border).toEqual([
        [3, 3, 20, 10],
        [0, 0, 26, 16],
      ])
    }
  })

  it("is medium wide with a style and no width", () => {
    expect(bordered({ border: "red solid" })[1]).toEqual([0, 0, 26, 16])
  })

  it("has no width without a style, and none or 0 clears it", () => {
    for (const border of ["2px", "2px red", "none", "0", "0 solid red"]) {
      expect(bordered({ border })[1], border).toEqual([0, 0, 20, 10])
    }
  })

  it("takes thin, a colour function and a length of any unit", () => {
    expect(bordered({ border: "thin dashed" })[1]).toEqual([0, 0, 22, 12])
    expect(bordered({ border: "rgb(0 0 0 / 50%) 3px solid" })[1]).toEqual([0, 0, 26, 16])
    expect(bordered({ border: "1em solid" })[0]).toEqual([16, 16, 20, 10])
    expect(bordered({ border: "calc(1px + 1em) solid" })[0]).toEqual([17, 17, 20, 10])
  })

  it("lets a side shorthand override one side", () => {
    expect(bordered({ borderTop: "2px dotted blue", borderLeft: "4px solid red" })).toEqual([
      [4, 2, 20, 10],
      [0, 0, 24, 12],
    ])
  })
})

describeNative("the logical shorthands on an inline span", () => {
  const TEXT = { fontSize: 16, lineHeight: "20px", fontFamily: "Menlo", color: "#000000" } as const

  /** How far the text after a span moves for the span's `space`. */
  const shift = (space: Style) => {
    const afterX = (style: Style) => {
      const refs = new Map<string, PublicInstance>()
      const { render, unmount } = createTestRoot({ width: 300, height: 100 })
      try {
        render(
          <div style={{ width: 400 }}>
            <text style={TEXT}>
              aa <span style={style}>bb</span>
              <span ref={(instance: PublicInstance | null) => instance && refs.set("after", instance)}>cc</span>
            </text>
          </div>
        )
        return refs.get("after")!.getBoundingClientRect().x
      } finally {
        unmount()
      }
    }
    return afterX(space) - afterX({})
  }

  it("counts the inline axis of padding and margin, and takes nothing from the block axis", () => {
    expect(shift({ paddingInline: "2px 6px" })).toBeCloseTo(8, 1)
    expect(shift({ marginInline: "4px 6px" })).toBeCloseTo(10, 1)
    expect(shift({ paddingBlock: 10, marginBlock: 10 })).toBeCloseTo(0, 1)
    expect(shift({ borderInline: "3px solid red" })).toBeCloseTo(6, 1)
    expect(shift({ padding: 5, paddingInline: "1px 2px" })).toBeCloseTo(3, 1)
    expect(shift({ marginInline: "4px 6px", marginLeft: 1 })).toBeCloseTo(7, 1)
  })
})
