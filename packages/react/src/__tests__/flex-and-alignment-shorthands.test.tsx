/// `flex`, `flex-flow`, `place-items`, `place-content` and `place-self`, and the grid placement
/// shorthands, in the boxes Chrome lays out (the expected numbers were read from headless
/// Chrome): static, and in a `hover` state.

import React from "react"
import { describe, expect, it } from "vitest"
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
function rect(renderer: Renderer, testId: string, against = "host") {
  const bounds = box(renderer, testId)
  const origin = box(renderer, against)
  return [bounds.x - origin.x, bounds.y - origin.y, bounds.width, bounds.height]
}

/** Moves the pointer to `x`, `y` and lets the hover state settle. */
function moveTo(renderer: Renderer, x: number, y: number) {
  renderer.nativeSimulateMouseMove(x, y)
  renderer.clockFastForward(16)
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

/** Lays `host` out with `items` as its children and returns each item's box in the host. */
function laidOut(host: Style, items: Style[], then?: (renderer: Renderer) => void) {
  let result: number[][] = []
  const { render, renderer, unmount } = createTestRoot()
  try {
    render(
      <div testId="host" style={{ position: "relative", ...host }}>
        {items.map((style, index) => (
          <div key={index} testId={`item${index}`} style={style} />
        ))}
      </div>
    )
    result = items.map((_, index) => rect(renderer, `item${index}`))
    then?.(renderer)
  } finally {
    unmount()
  }
  return result
}

const ROW = { display: "flex", width: 300, height: 100 }

describeNative("flex", () => {
  it("1 shares the space equally whatever the items hold, as its basis is 0%", () => {
    const equal = [
      [0, 0, 150, 10],
      [150, 0, 150, 10],
    ]
    expect(laidOut(ROW, [{ flex: 1, width: 50, height: 10 }, { flex: 1, width: 100, height: 10 }])).toEqual(equal)
    expect(laidOut(ROW, [{ flex: "1", width: 50, height: 10 }, { flex: "1 1 0%", width: 100, height: 10 }])).toEqual(equal)
  })

  it("auto distributes the free space on top of the content size", () => {
    expect(laidOut(ROW, [{ flex: "auto", width: 50, height: 10 }, { flex: "auto", width: 100, height: 10 }])).toEqual([
      [0, 0, 125, 10],
      [125, 0, 175, 10],
    ])
  })

  it("none neither grows nor shrinks, and initial only shrinks", () => {
    const narrow = { display: "flex", width: 100, height: 100 }
    expect(laidOut(narrow, [{ flex: "none", width: 80, height: 10 }, { flex: "none", width: 80, height: 10 }])).toEqual([
      [0, 0, 80, 10],
      [80, 0, 80, 10],
    ])
    expect(laidOut(narrow, [{ flex: "initial", width: 80, height: 10 }, { flex: "initial", width: 80, height: 10 }])).toEqual([
      [0, 0, 50, 10],
      [50, 0, 50, 10],
    ])
  })

  it("takes a grow factor with a basis: '1 100px' and '3 100px' split 300px of free space 1 to 3", () => {
    const wide = { display: "flex", width: 500, height: 100 }
    expect(laidOut(wide, [{ flex: "1 100px", height: 10 }, { flex: "3 100px", height: 10 }])).toEqual([
      [0, 0, 175, 10],
      [175, 0, 325, 10],
    ])
  })

  it("is a basis with a grow and shrink of 1 when it is a lone length", () => {
    expect(laidOut({ display: "flex", width: 500, height: 100 }, [{ flex: "100px", height: 10 }, { flex: "100px", height: 10 }])).toEqual([
      [0, 0, 250, 10],
      [250, 0, 250, 10],
    ])
  })

  it("takes the basis before or after the factors", () => {
    expect(laidOut(ROW, [{ flex: "30px 2", height: 10 }, { flex: "30px 1", height: 10 }])).toEqual([
      [0, 0, 190, 10],
      [190, 0, 110, 10],
    ])
    expect(laidOut(ROW, [{ flex: "2 1 30px", height: 10 }, { flex: "1 1 30px", height: 10 }])).toEqual([
      [0, 0, 190, 10],
      [190, 0, 110, 10],
    ])
  })

  it("takes a grow and a shrink over a 0% basis, and a unitless 0 second is the shrink", () => {
    expect(laidOut(ROW, [{ flex: "2 3", height: 10 }, { flex: "1 1", height: 10 }])).toEqual([
      [0, 0, 200, 10],
      [200, 0, 100, 10],
    ])
    expect(laidOut(ROW, [{ flex: "1 0", width: 70, height: 10 }, { flex: "1 0", height: 10 }])).toEqual([
      [0, 0, 150, 10],
      [150, 0, 150, 10],
    ])
  })

  it("takes a unitless 0 after two factors as the basis: '1 1 0' and '2 1 0' share 300px 1 to 2", () => {
    expect(laidOut(ROW, [{ flex: "1 1 0", height: 10 }, { flex: "2 1 0", height: 10 }])).toEqual([
      [0, 0, 100, 10],
      [100, 0, 200, 10],
    ])
  })

  it("0 does not grow, and takes the basis 0%", () => {
    expect(laidOut(ROW, [{ flex: 0, width: 70, height: 10 }, { flex: 1, height: 10 }])).toEqual([
      [0, 0, 0, 10],
      [0, 0, 300, 10],
    ])
  })

  it("takes a calc() basis", () => {
    expect(laidOut(ROW, [{ flex: "1 1 calc(50% - 10px)", height: 10 }, { flex: "0 0 40px", height: 10 }])).toEqual([
      [0, 0, 260, 10],
      [260, 0, 40, 10],
    ])
  })

  it("shares a column of a definite height by the grow factors", () => {
    expect(
      laidOut(
        { display: "flex", flexDirection: "column", width: 300, height: 200 },
        [{ flex: 1, width: 10 }, { flex: 3, width: 10 }]
      )
    ).toEqual([
      [0, 0, 10, 50],
      [0, 50, 10, 150],
    ])
  })

  it("lets flexGrow, flexShrink and flexBasis win over it", () => {
    expect(laidOut(ROW, [{ flex: 1, flexGrow: 3, height: 10 }, { flex: 1, height: 10 }])).toEqual([
      [0, 0, 225, 10],
      [225, 0, 75, 10],
    ])
    expect(laidOut(ROW, [{ flex: "1 1 50px", flexBasis: 100, height: 10 }, { flex: "1 1 50px", height: 10 }])).toEqual([
      [0, 0, 175, 10],
      [175, 0, 125, 10],
    ])
  })

  it("is an error for a value CSS would drop", () => {
    expect(errorsOf({ flex: "1 2 3" })).toMatch(/invalid flex "1 2 3"/)
    expect(errorsOf({ flex: -1 })).toMatch(/invalid flex -1/)
    expect(errorsOf({ flex: "1 -1" })).toMatch(/invalid flex "1 -1"/)
    expect(errorsOf({ flex: "1 auto 2" })).toMatch(/invalid flex/)
    expect(errorsOf({ flex: "" })).toMatch(/invalid flex/)
    expect(errorsOf({ hover: { flex: "bogus" } })).toMatch(/invalid flex "bogus"/)
  })

  describe("in a hover state", () => {
    const hovered = (flex: Style) => {
      let result: number[][] = []
      const { render, renderer, unmount } = createTestRoot()
      try {
        render(
          <div testId="host" style={{ position: "relative", ...ROW }}>
            <div testId="item0" style={{ flex: 1, height: 10, ...flex }} />
            <div testId="item1" style={{ flex: 1, height: 10 }} />
          </div>
        )
        moveTo(renderer, box(renderer, "item0").x + 5, box(renderer, "item0").y + 5)
        result = [rect(renderer, "item0"), rect(renderer, "item1")]
      } finally {
        unmount()
      }
      return result
    }

    it("replaces the base shorthand, and a base longhand it covers", () => {
      expect(hovered({ flexGrow: 9, hover: { flex: "3 1 0%" } })).toEqual([
        [0, 0, 225, 10],
        [225, 0, 75, 10],
      ])
    })

    it("lets a state longhand win over the base shorthand", () => {
      expect(hovered({ hover: { flexGrow: 3 } })).toEqual([
        [0, 0, 225, 10],
        [225, 0, 75, 10],
      ])
    })
  })
})

describeNative("flexFlow", () => {
  it("column wrap lays the items down and wraps them to a second column", () => {
    const column = { display: "flex", flexFlow: "column wrap", width: 200, height: 100, alignContent: "flex-start" }
    const item = { width: 30, height: 40 }
    expect(laidOut(column, [item, item, item])).toEqual([
      [0, 0, 30, 40],
      [0, 40, 30, 40],
      [30, 0, 30, 40],
    ])
  })

  it("takes the wrap before the direction, and a lone wrap leaves the direction a row", () => {
    const item = { width: 30, height: 40 }
    expect(
      laidOut({ display: "flex", flexFlow: "wrap column", width: 200, height: 100, alignContent: "flex-start" }, [item, item, item])
    ).toEqual([
      [0, 0, 30, 40],
      [0, 40, 30, 40],
      [30, 0, 30, 40],
    ])
    expect(
      laidOut({ display: "flex", flexFlow: "wrap-reverse", width: 100, height: 100, alignContent: "flex-start" }, [
        { width: 60, height: 20 },
        { width: 60, height: 20 },
      ])
    ).toEqual([
      [0, 80, 60, 20],
      [0, 60, 60, 20],
    ])
  })

  it("row-reverse starts from the right", () => {
    expect(
      laidOut({ display: "flex", flexFlow: "row-reverse", width: 300, height: 100 }, [
        { width: 50, height: 20 },
        { width: 50, height: 20 },
      ])
    ).toEqual([
      [250, 0, 50, 20],
      [200, 0, 50, 20],
    ])
  })

  it("lets flexDirection and flexWrap win over it", () => {
    expect(
      laidOut({ display: "flex", flexFlow: "column", flexDirection: "row", width: 300, height: 100 }, [
        { width: 50, height: 20 },
        { width: 50, height: 20 },
      ])
    ).toEqual([
      [0, 0, 50, 20],
      [50, 0, 50, 20],
    ])
  })

  it("is an error for a value CSS would drop", () => {
    expect(errorsOf({ flexFlow: "row row" })).toMatch(/invalid flex-flow "row row"/)
    expect(errorsOf({ flexFlow: "column column-reverse" })).toMatch(/invalid flex-flow/)
    expect(errorsOf({ flexFlow: "sideways" })).toMatch(/invalid flex-flow "sideways"/)
    expect(errorsOf({ flexFlow: "" })).toMatch(/invalid flex-flow/)
  })

  it("is replaced by a state's flexFlow", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      render(
        <div
          testId="host"
          style={{ position: "relative", display: "flex", flexFlow: "row wrap", flexDirection: "row-reverse", width: 300, height: 100, hover: { flexFlow: "column" } }}
        >
          <div testId="item0" style={{ width: 50, height: 20 }} />
          <div testId="item1" style={{ width: 50, height: 20 }} />
        </div>
      )
      expect(rect(renderer, "item0")).toEqual([250, 0, 50, 20])
      moveTo(renderer, box(renderer, "host").x + 5, box(renderer, "host").y + 90)
      expect(rect(renderer, "item0")).toEqual([0, 0, 50, 20])
      expect(rect(renderer, "item1")).toEqual([0, 20, 50, 20])
    } finally {
      unmount()
    }
  })
})

describeNative("placeItems, placeContent and placeSelf", () => {
  const GRID = { display: "grid", width: 300, height: 100 }
  const CHILD = { width: 50, height: 20 }

  it("placeItems 'center end' centres the item in its row and puts it at the end of its column", () => {
    expect(laidOut({ ...GRID, placeItems: "center end" }, [CHILD])).toEqual([[250, 40, 50, 20]])
    expect(laidOut({ ...GRID, placeItems: "end center" }, [CHILD])).toEqual([[125, 80, 50, 20]])
  })

  it("placeItems with one value sets both", () => {
    expect(laidOut({ display: "flex", width: 300, height: 100, placeItems: "center" }, [CHILD, { width: 50, height: 40 }])).toEqual([
      [0, 40, 50, 20],
      [50, 30, 50, 40],
    ])
    expect(laidOut({ ...GRID, placeItems: "center" }, [CHILD])).toEqual([[125, 40, 50, 20]])
  })

  it("placeContent 'center end' places the tracks", () => {
    expect(laidOut({ ...GRID, gridTemplateColumns: "50px", placeContent: "center end" }, [CHILD])).toEqual([[250, 40, 50, 20]])
  })

  it("placeContent 'space-between center' spaces the lines and centres each", () => {
    expect(
      laidOut({ display: "flex", flexWrap: "wrap", width: 300, height: 100, placeContent: "space-between center" }, [
        { width: 100, height: 20 },
        { width: 100, height: 20 },
        { width: 150, height: 20 },
      ])
    ).toEqual([
      [50, 0, 100, 20],
      [150, 0, 100, 20],
      [75, 80, 150, 20],
    ])
  })

  it("placeSelf places one item", () => {
    expect(laidOut(GRID, [{ ...CHILD, placeSelf: "end center" }])).toEqual([[125, 80, 50, 20]])
    expect(laidOut(GRID, [{ ...CHILD, placeSelf: "center" }])).toEqual([[125, 40, 50, 20]])
  })

  it("lets the align and justify longhands win", () => {
    expect(laidOut(GRID, [{ ...CHILD, placeSelf: "center", alignSelf: "start", justifySelf: "end" }])).toEqual([[250, 0, 50, 20]])
    expect(laidOut({ ...GRID, placeItems: "center", alignItems: "start" }, [CHILD])).toEqual([[125, 0, 50, 20]])
  })

  it("is an error for a value CSS would drop", () => {
    expect(errorsOf({ placeItems: "left" })).toMatch(/invalid place-items "left"/)
    expect(errorsOf({ placeItems: "space-between" })).toMatch(/invalid place-items "space-between"/)
    expect(errorsOf({ placeContent: "auto" })).toMatch(/invalid place-content/)
    expect(errorsOf({ placeSelf: "center end start" })).toMatch(/invalid place-self/)
    expect(errorsOf({ placeSelf: "" })).toMatch(/invalid place-self/)
    expect(errorsOf({ hover: { placeItems: "bogus" } })).toMatch(/invalid place-items "bogus"/)
  })

  it("is replaced by a state's placeItems", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      render(
        <div testId="host" style={{ position: "relative", ...GRID, placeItems: "start", alignItems: "end", hover: { placeItems: "center end" } }}>
          <div testId="item0" style={CHILD} />
        </div>
      )
      expect(rect(renderer, "item0")).toEqual([0, 80, 50, 20])
      moveTo(renderer, box(renderer, "host").x + 5, box(renderer, "host").y + 5)
      expect(rect(renderer, "item0")).toEqual([250, 40, 50, 20])
    } finally {
      unmount()
    }
  })
})

describeNative("grid placement shorthands", () => {
  const GRID = {
    display: "grid",
    width: 300,
    height: 90,
    gridTemplateColumns: "100px 100px 100px",
    gridTemplateRows: "30px 30px 30px",
  }

  it("gridArea '1 / 2 / 3 / 4' is rows 1 to 3 and columns 2 to 4", () => {
    expect(laidOut(GRID, [{ gridArea: "1 / 2 / 3 / 4" }])).toEqual([[100, 0, 200, 60]])
  })

  it("gridArea takes fewer lines: 2 / 1 and 2", () => {
    expect(laidOut(GRID, [{ gridArea: "2 / 1" }])).toEqual([[0, 30, 100, 30]])
    expect(laidOut(GRID, [{ gridArea: "2" }])).toEqual([[0, 30, 100, 30]])
  })

  it("gridColumn and gridRow take a line, a span, and a pair", () => {
    expect(laidOut(GRID, [{ gridColumn: "2 / span 2", gridRow: 3 }])).toEqual([[100, 60, 200, 30]])
    expect(laidOut(GRID, [{ gridColumn: "span 2" }])).toEqual([[0, 0, 200, 30]])
    expect(laidOut(GRID, [{ gridRow: "2 / 4", gridColumn: 3 }])).toEqual([[200, 30, 100, 60]])
    expect(laidOut(GRID, [{ gridRow: "-2 / -1", gridColumn: -2 }])).toEqual([[200, 60, 100, 30]])
  })

  it("names lines in the pair and in the area", () => {
    const named = {
      ...GRID,
      gridTemplateColumns: "[a] 100px [b] 100px [c] 100px [d]",
      gridTemplateRows: "[r1] 30px [r2] 30px [r3] 30px [r4]",
    }
    expect(laidOut(named, [{ gridColumn: "b / d", gridRow: "r2" }])).toEqual([[100, 30, 200, 30]])
    expect(laidOut(named, [{ gridArea: "r2 / b / r4 / d" }])).toEqual([[100, 30, 200, 60]])
    expect(laidOut(named, [{ gridColumn: "c / span 1", gridRow: "r1" }])).toEqual([[200, 0, 100, 30]])
  })

  it("names an area", () => {
    const areas = { ...GRID, gridTemplateAreas: '"h h h" "s m m" "s f f"' }
    expect(laidOut(areas, [{ gridArea: "m" }, { gridArea: "s" }, { gridArea: "f" }])).toEqual([
      [100, 30, 200, 30],
      [0, 30, 100, 60],
      [100, 60, 200, 30],
    ])
  })

  it("is an error for a line CSS would drop", () => {
    expect(errorsOf({ gridArea: "1 / 2 / 3 / 4 / 5" })).toMatch(/invalid grid-area/)
    expect(errorsOf({ gridColumn: "span 0" })).toMatch(/invalid grid line/)
    expect(errorsOf({ gridRow: "0" })).toMatch(/invalid grid line/)
  })
})
