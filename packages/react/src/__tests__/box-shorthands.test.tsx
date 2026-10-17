/// `padding`, `margin`, `borderWidth`, `gap` and `inset` take CSS's one to four value
/// strings, in the boxes Chrome lays out: static, in a `hover` state, and along an
/// inline span's line.

import React from "react"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { createTestRoot, hasNativeTestRenderer, type TestRoot } from "../testing.js"

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

/** The messages of the errors rendering `style` reports. */
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

function withRoot(ui: React.ReactElement, check: (renderer: Renderer) => void) {
  const { render, renderer, unmount } = createTestRoot()
  try {
    render(ui)
    check(renderer)
  } finally {
    unmount()
  }
}

/** A 300x200 box, 16px text, holding `children`. */
function scene(children: React.ReactNode, style: Style = {}) {
  return (
    <div testId="scene" style={{ width: 300, height: 200, fontSize: 16, ...style }}>
      {children}
    </div>
  )
}

/** A 20x10 child, so the space around it is the box that holds it. */
const CHILD = <div testId="child" style={{ width: 20, height: 10 }} />

describeNative("padding takes one to four values", () => {
  const laidOut = (padding: string | number, style: Style = {}) => {
    let result: number[][] = []
    withRoot(
      scene(
        <div testId="host" style={{ display: "flex", width: "fit-content", padding, ...style }}>
          {CHILD}
        </div>
      ),
      (renderer) => {
        result = [rect(renderer, "child"), rect(renderer, "host")]
      }
    )
    return result
  }

  it("reads em against the element's font size: 0.5em 0.75em at 16px is 8px and 12px", () => {
    const [child, host] = laidOut("0.5em 0.75em")
    expect(child).toEqual([12, 8, 20, 10])
    expect(host).toEqual([0, 0, 44, 26])
  })

  it("fills the missing sides like CSS", () => {
    expect(laidOut("6px")[0]).toEqual([6, 6, 20, 10])
    expect(laidOut("1px 2px 3px")[0]).toEqual([2, 1, 20, 10])
    expect(laidOut("1px 2px 3px")[1]).toEqual([0, 0, 24, 14])
    expect(laidOut("1px 2px 3px 4px")[0]).toEqual([4, 1, 20, 10])
    expect(laidOut("1px 2px 3px 4px")[1]).toEqual([0, 0, 26, 14])
  })

  it("keeps a calc() with spaces in it as one value", () => {
    const [child] = laidOut("calc(1px + 1em) 2px")
    expect(child).toEqual([2, 17, 20, 10])
  })

  it("lets a side longhand win over the shorthand", () => {
    expect(laidOut("1px 2px", { paddingLeft: 9 })[0]).toEqual([9, 1, 20, 10])
  })

  it("is an error for a value CSS would drop", () => {
    expect(errorsOf({ padding: "1px auto" })).toMatch(/invalid padding "auto"/)
    expect(errorsOf({ padding: "1px 12" })).toMatch(/invalid padding "12"/)
    expect(errorsOf({ padding: "1px 2px 3px 4px 5px" })).toMatch(/one to four values/)
    expect(errorsOf({ padding: "" })).toMatch(/one to four values/)
    expect(errorsOf({ hover: { padding: "1px bogus" } })).toMatch(/invalid padding "bogus"/)
  })
})

describeNative("margin takes one to four values, each of which may be auto", () => {
  // The 1px of padding keeps the child's top margin inside the scene instead of collapsing through it.
  const placed = (margin: string, style: Style = {}) => {
    let result: number[] = []
    withRoot(
      scene(<div testId="child" style={{ width: 100, height: 10, margin, ...style }} />, { paddingTop: 1 }),
      (renderer) => {
        result = rect(renderer, "child")
      }
    )
    return result
  }

  it("centres a box with 0 auto", () => {
    expect(placed("0 auto")).toEqual([100, 1, 100, 10])
  })

  it("puts each value on its side", () => {
    expect(placed("10px 20px 30px 40px")).toEqual([40, 11, 100, 10])
    expect(placed("10px 20px")).toEqual([20, 11, 100, 10])
    expect(placed("10px 20px 30px")).toEqual([20, 11, 100, 10])
    expect(placed("0.5em 1em")).toEqual([16, 9, 100, 10])
  })

  it("takes auto on one side only", () => {
    expect(placed("4px 4px 4px auto")).toEqual([196, 5, 100, 10])
  })

  it("lets a side longhand win over the shorthand", () => {
    expect(placed("0 auto", { marginLeft: 7 })).toEqual([7, 1, 100, 10])
  })

  it("is an error for an intrinsic size keyword in any value", () => {
    expect(errorsOf({ margin: "0 max-content" })).toMatch(/invalid margin "max-content"/)
  })
})

describeNative("inset takes one to four values", () => {
  const placed = (inset: string | number, style: Style = {}) => {
    let result: number[] = []
    withRoot(
      scene(<div testId="child" style={{ position: "absolute", inset, ...style }} />, { position: "relative" }),
      (renderer) => {
        result = rect(renderer, "child")
      }
    )
    return result
  }

  it("is 10px from the top and bottom and 20px from the sides for 10px 20px", () => {
    expect(placed("10px 20px")).toEqual([20, 10, 260, 180])
  })

  it("takes one, three and four values", () => {
    expect(placed(0)).toEqual([0, 0, 300, 200])
    expect(placed("5px")).toEqual([5, 5, 290, 190])
    expect(placed("1px 2px 3px")).toEqual([2, 1, 296, 196])
    expect(placed("1px 2px 3px 4px")).toEqual([4, 1, 294, 196])
  })

  it("lets auto leave a side to the box's size", () => {
    expect(placed("auto 10px 20px auto", { width: 50, height: 30 })).toEqual([240, 150, 50, 30])
  })

  it("lets top, right, bottom and left win over it", () => {
    expect(placed("10px 20px", { left: 0, bottom: 50 })).toEqual([0, 10, 280, 140])
  })

  it("is an error for an intrinsic size keyword", () => {
    expect(errorsOf({ inset: "10px fit-content" })).toMatch(/invalid inset "fit-content"/)
  })
})

describeNative("gap takes a row and a column", () => {
  const wrapped = (gap: string | number, style: Style = {}) => {
    let result: number[][] = []
    withRoot(
      scene(
        <div testId="flow" style={{ display: "flex", flexWrap: "wrap", width: 100, gap, ...style }}>
          <div testId="a" style={{ width: 40, height: 10 }} />
          <div testId="b" style={{ width: 40, height: 10 }} />
          <div testId="c" style={{ width: 40, height: 10 }} />
        </div>
      ),
      (renderer) => {
        result = ["a", "b", "c"].map((id) => rect(renderer, id).slice(0, 2))
      }
    )
    return result
  }

  it("is 4px between rows and 8px between columns for 4px 8px", () => {
    expect(wrapped("4px 8px")).toEqual([
      [0, 0],
      [48, 0],
      [0, 14],
    ])
  })

  it("is both gaps for one value", () => {
    expect(wrapped("6px")).toEqual([
      [0, 0],
      [46, 0],
      [0, 16],
    ])
  })

  it("reads em against the element's font size", () => {
    expect(wrapped("0.25em 0.5em")).toEqual([
      [0, 0],
      [48, 0],
      [0, 14],
    ])
  })

  it("lets rowGap and columnGap win over it", () => {
    expect(wrapped("4px 8px", { columnGap: 2, rowGap: 0 })).toEqual([
      [0, 0],
      [42, 0],
      [0, 10],
    ])
  })

  it("is an error for three values and for auto", () => {
    expect(errorsOf({ gap: "1px 2px 3px" })).toMatch(/one or two values/)
    expect(errorsOf({ gap: "1px auto" })).toMatch(/invalid gap "auto"/)
  })
})

describeNative("borderWidth takes one to four values", () => {
  const bordered = (borderWidth: string | number, style: Style = {}) => {
    let result: number[][] = []
    withRoot(
      scene(
        <div testId="host" style={{ display: "flex", width: "fit-content", borderWidth, borderStyle: "solid", ...style }}>
          {CHILD}
        </div>
      ),
      (renderer) => {
        result = [rect(renderer, "child"), rect(renderer, "host")]
      }
    )
    return result
  }

  it("gives each side its own width for 1px 2px 3px 4px", () => {
    const [child, host] = bordered("1px 2px 3px 4px")
    expect(child).toEqual([4, 1, 20, 10])
    expect(host).toEqual([0, 0, 26, 14])
  })

  it("fills the missing sides like CSS", () => {
    expect(bordered("2px 6px")[1]).toEqual([0, 0, 32, 14])
    expect(bordered("1px 2px 3px")[0]).toEqual([2, 1, 20, 10])
  })

  it("takes thin, medium and thick per side", () => {
    const [child, host] = bordered("thin medium thick")
    expect(child).toEqual([3, 1, 20, 10])
    expect(host).toEqual([0, 0, 26, 16])
  })

  it("reads em against the element's font size", () => {
    expect(bordered("0.25em 0.5em")[0]).toEqual([8, 4, 20, 10])
  })

  it("lets a side longhand win over it", () => {
    expect(bordered("1px 2px 3px 4px", { borderLeftWidth: 10 })[0]).toEqual([10, 1, 20, 10])
  })

  it("is an error for a percentage and for five values", () => {
    expect(errorsOf({ borderWidth: "1px 10%" })).toMatch(/invalid border width "10%"/)
    expect(errorsOf({ borderWidth: "1px 1px 1px 1px 1px" })).toMatch(/one to four values/)
  })
})

describeNative("the shorthands in a state style", () => {
  /** Lays out `ui`, moves the pointer `dx`, `dy` into `over`, and reads `read` before and after. */
  function hovered(ui: React.ReactElement, over: string, point: [number, number], read: (renderer: Renderer) => number[][]) {
    let result: { at: number[][]; on: number[][] } | undefined
    withRoot(ui, (renderer) => {
      const at = read(renderer)
      const target = box(renderer, over)
      renderer.nativeSimulateMouseMove(target.x + point[0], target.y + point[1])
      renderer.clockFastForward(16)
      result = { at, on: read(renderer) }
    })
    return result!
  }

  const host = (rest: Style, hover: Style) =>
    scene(
      <div testId="host" style={{ display: "flex", width: 120, height: 90, ...rest, hover }}>
        {CHILD}
      </div>
    )
  const childAndHost = (renderer: Renderer) => [rect(renderer, "child"), rect(renderer, "host")]

  it("lays out padding 0.5em 0.75em on hover", () => {
    const { at, on } = hovered(host({}, { padding: "0.5em 0.75em" }), "host", [100, 80], childAndHost)
    expect(at[0]).toEqual([0, 0, 20, 10])
    expect(on[0]).toEqual([12, 8, 20, 10])
  })

  it("lays out borderWidth 1px 2px 3px 4px on hover", () => {
    const { at, on } = hovered(
      host({ borderStyle: "solid", borderWidth: 0 }, { borderWidth: "1px 2px 3px 4px" }),
      "host",
      [100, 80],
      childAndHost
    )
    expect(at[0]).toEqual([0, 0, 20, 10])
    expect(on[0]).toEqual([4, 1, 20, 10])
  })

  it("resets the base longhands a state shorthand covers", () => {
    const { at, on } = hovered(host({ paddingLeft: 30 }, { padding: "5px 6px" }), "host", [100, 80], childAndHost)
    expect(at[0]).toEqual([30, 0, 20, 10])
    expect(on[0]).toEqual([6, 5, 20, 10])
  })

  it("lays out margin 0 auto on hover", () => {
    const { at, on } = hovered(
      scene(
        <div testId="parent" style={{ width: 150, height: 40 }}>
          <div testId="child" style={{ width: 100, height: 10, hover: { margin: "0 auto" } }} />
        </div>
      ),
      "child",
      [50, 5],
      (renderer) => [rect(renderer, "child")]
    )
    expect(at[0]).toEqual([0, 0, 100, 10])
    expect(on[0]).toEqual([25, 0, 100, 10])
  })

  it("lays out inset 10px 20px on hover", () => {
    const { at, on } = hovered(
      scene(
        <div testId="parent" style={{ position: "relative", width: 200, height: 100 }}>
          <div
            testId="child"
            style={{ position: "absolute", top: 0, left: 0, width: 100, height: 50, hover: { inset: "10px 20px" } }}
          />
        </div>
      ),
      "child",
      [50, 30],
      (renderer) => [rect(renderer, "child")]
    )
    expect(at[0]).toEqual([0, 0, 100, 50])
    // The width and height are set, so the left and top insets win over the right and bottom.
    expect(on[0]).toEqual([20, 10, 100, 50])
  })

  it("lays out gap 4px 8px on hover", () => {
    const { at, on } = hovered(
      scene(
        <div testId="flow" style={{ display: "flex", flexWrap: "wrap", width: 100, hover: { gap: "4px 8px" } }}>
          <div testId="a" style={{ width: 40, height: 10 }} />
          <div testId="b" style={{ width: 40, height: 10 }} />
          <div testId="c" style={{ width: 40, height: 10 }} />
        </div>
      ),
      "flow",
      [5, 5],
      (renderer) => ["a", "b", "c"].map((id) => rect(renderer, id).slice(0, 2))
    )
    expect(at).toEqual([
      [0, 0],
      [40, 0],
      [0, 10],
    ])
    expect(on).toEqual([
      [0, 0],
      [48, 0],
      [0, 14],
    ])
  })
})

describeNative("the shorthands on an inline span", () => {
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

  it("counts each side of a padding, margin and border width shorthand", () => {
    expect(shift({ padding: "0 0.5em" })).toBeCloseTo(16, 1)
    expect(shift({ padding: "0 1px 0 4px" })).toBeCloseTo(5, 1)
    expect(shift({ margin: "0 4px 0 6px" })).toBeCloseTo(10, 1)
    expect(shift({ margin: "1px 3px" })).toBeCloseTo(6, 1)
    expect(shift({ borderWidth: "0 1px 0 2px", borderStyle: "solid" })).toBeCloseTo(3, 1)
  })
})
