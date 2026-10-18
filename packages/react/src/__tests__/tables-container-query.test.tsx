/// Container queries and tables. The boxes are what headless Chrome (chrome-headless-shell) lays out for the
/// HTML above each test (`getBoundingClientRect`, relative to the 300px wide `#root`).
///
/// Chrome does not make a table, an `inline-table`, a row group, a row, a column or a cell a query container
/// (CSS Containment 3: `container-type` does not apply to table boxes and internal table boxes): the style is
/// ignored, with the size containment that comes with it. A caption is a container. Cells and tables are still
/// laid out by the table algorithm, and what is in them is queried in the frame that lays them out.
///
/// Every page has
///
/// ```html
/// <style>
///   .q { width: 30px; height: 10px }
///   @container (min-width: 80px) { .q { width: 70px; height: 12px } }
///   .cq { width: 50cqw; height: 6px }
/// </style>
/// <div id="root" style="width:300px;font:20px/20px GpuixTestSquare"> ...the case... </div>
/// ```

import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Style = NonNullable<React.ComponentProps<"div">["style"]>
type Rect = [number, number, number, number]
type TestRoot = ReturnType<typeof createTestRoot>

const Q = {
  width: 30,
  height: 10,
  "@container": { "(min-width: 80px)": { width: 70, height: 12 } },
} as const satisfies Style
const CQ = { width: "50cqw", height: 6 } as const satisfies Style

function part(display: string) {
  return function Part({ id, style, children }: { id: string; style?: Style; children?: React.ReactNode }) {
    return (
      <div testId={id} style={{ display, ...style }}>
        {children}
      </div>
    )
  }
}

const Table = part("table")
const InlineTable = part("inline-table")
const Row = part("table-row")
const Cell = part("table-cell")
const Caption = part("table-caption")
const Group = part("table-row-group")

function laidOut(testRoot: TestRoot, id: string): Rect {
  testRoot.renderer.dispatchNativeEvents()
  const element = testRoot.renderer.findByTestId(id)
  expect(element, `${id} is mounted`).toBeDefined()
  const found = testRoot.renderer.getElementBounds(element!.id)
  expect(found, `${id} has a box`).not.toBeNull()
  const origin = testRoot.renderer.getElementBounds(testRoot.renderer.findByTestId("root")!.id)!
  return [found!.x - origin.x, found!.y - origin.y, found!.width, found!.height]
}

function expectBoxes(testRoot: TestRoot, want: Record<string, Rect>) {
  for (const [id, expected] of Object.entries(want)) {
    const actual = laidOut(testRoot, id)
    actual.forEach((value, index) => {
      expect(value, `${id} ${["x", "y", "width", "height"][index]}: got ${actual} want ${expected}`).toBeCloseTo(
        expected[index]!,
        1
      )
    })
  }
}

/** Frames the window painted while `change` ran. */
function paintedFrames(testRoot: TestRoot, change: () => void): number {
  const before = testRoot.renderer.getPaintedFrameCount()
  change()
  return testRoot.renderer.getPaintedFrameCount() - before
}

function withRoot(run: (testRoot: TestRoot) => void) {
  const testRoot = createTestRoot()
  try {
    run(testRoot)
  } finally {
    testRoot.unmount()
  }
}

const root = (children: React.ReactNode) => (
  <div testId="root" style={{ width: 300, fontSize: 20 }}>
    {children}
  </div>
)

describeNative("container queries and tables", () => {
  /**
   * A container around a table: a cell is queried against it, and a `cq` unit in the cell is of it. The table
   * shrinks to what the cell asks for, so a flip of the query changes the table in the same frame.
   *
   * ```html
   * <div id="ct" style="container-type:inline-size;width:200px"><div id="t" style="display:table;border-spacing:0">
   *   <div id="r1" style="display:table-row"><div id="c1" style="display:table-cell">
   *     <div id="q1" class="q"></div><div id="cq1" class="cq"></div></div></div></div></div>
   * ```
   */
  it("queries a cell against the container around the table, in the one frame, wide, mid and narrow", () => {
    const cases: [number, Record<string, Rect>][] = [
      [200, { ct: [0, 0, 200, 18], t: [0, 0, 100, 18], c1: [0, 0, 100, 18], q1: [0, 0, 70, 12], cq1: [0, 12, 100, 6] }],
      [100, { ct: [0, 0, 100, 18], t: [0, 0, 70, 18], c1: [0, 0, 70, 18], q1: [0, 0, 70, 12], cq1: [0, 12, 50, 6] }],
      [60, { ct: [0, 0, 60, 16], t: [0, 0, 30, 16], c1: [0, 0, 30, 16], q1: [0, 0, 30, 10], cq1: [0, 10, 30, 6] }],
    ]
    const app = (width: number) =>
      root(
        <div testId="ct" style={{ containerType: "inline-size", width }}>
          <Table id="t" style={{ borderSpacing: 0 }}>
            <Row id="r1">
              <Cell id="c1">
                <div testId="q1" style={Q} />
                <div testId="cq1" style={CQ} />
              </Cell>
            </Row>
          </Table>
        </div>
      )
    withRoot((testRoot) => {
      expect(paintedFrames(testRoot, () => testRoot.render(app(200)))).toBe(1)
      expectBoxes(testRoot, cases[0]![1])
      // The container is resized and the table follows in the frame that lays it out, both ways.
      for (const [width, boxes] of [cases[1]!, cases[2]!, cases[0]!, cases[2]!] as const) {
        expect(paintedFrames(testRoot, () => testRoot.render(app(width)))).toBe(1)
        expectBoxes(testRoot, boxes)
      }
    })
  })

  /**
   * A container in a cell is as wide as the cell the table algorithm made: the column is as wide as the widest
   * cell in it. The container's own content asks for no width (size containment), so the column's width is the
   * other cell's, and the query sees the content box of the container in the first frame.
   *
   * ```html
   * <div id="t" style="display:table;border-spacing:0"><div id="r1" style="display:table-row">
   *   <div id="c1" style="display:table-cell;padding:5px"><div id="ct" style="container-type:inline-size">
   *     <div id="q1" class="q"></div><div id="cq1" class="cq"></div></div></div></div>
   * <div id="r2" style="display:table-row"><div id="c2" style="display:table-cell"><div id="b" style="width:150px;height:20px"></div></div></div></div>
   * ```
   */
  it("sizes a container in a cell by the column, and queries it in the frame that lays the column out", () => {
    const app = (column: number) =>
      root(
        <Table id="t" style={{ borderSpacing: 0 }}>
          <Row id="r1">
            <Cell id="c1" style={{ padding: 5 }}>
              <div testId="ct" style={{ containerType: "inline-size" }}>
                <div testId="q1" style={Q} />
                <div testId="cq1" style={CQ} />
              </div>
            </Cell>
          </Row>
          <Row id="r2">
            <Cell id="c2">
              <div testId="b" style={{ width: column, height: 20 }} />
            </Cell>
          </Row>
        </Table>
      )
    const wide: Record<string, Rect> = {
      t: [0, 0, 150, 48],
      c1: [0, 0, 150, 28],
      ct: [5, 5, 140, 18],
      q1: [5, 5, 70, 12],
      cq1: [5, 17, 70, 6],
      c2: [0, 28, 150, 20],
    }
    const narrow: Record<string, Rect> = {
      t: [0, 0, 60, 46],
      c1: [0, 0, 60, 26],
      ct: [5, 5, 50, 16],
      q1: [5, 5, 30, 10],
      cq1: [5, 15, 25, 6],
      c2: [0, 26, 60, 20],
    }
    withRoot((testRoot) => {
      expect(paintedFrames(testRoot, () => testRoot.render(app(150)))).toBe(1)
      expectBoxes(testRoot, wide)
      expect(paintedFrames(testRoot, () => testRoot.render(app(60)))).toBe(1)
      expectBoxes(testRoot, narrow)
      expect(paintedFrames(testRoot, () => testRoot.render(app(150)))).toBe(1)
      expectBoxes(testRoot, wide)
    })
  })

  /**
   * A block in an anonymous cell of the table is under the container too.
   *
   * ```html
   * <div id="ct" style="container-type:inline-size;width:200px"><div id="t" style="display:table;border-spacing:0">
   *   <div id="cqa" class="cq"></div>
   *   <div id="r1" style="display:table-row"><div id="c1" style="display:table-cell"><div id="q1" class="q"></div></div></div></div></div>
   * ```
   */
  it("a cq unit and a query in an anonymous cell see the container around the table", () => {
    withRoot((testRoot) => {
      testRoot.render(
        root(
          <div testId="ct" style={{ containerType: "inline-size", width: 200 }}>
            <Table id="t" style={{ borderSpacing: 0 }}>
              <div testId="cqa" style={CQ} />
              <Row id="r1">
                <Cell id="c1">
                  <div testId="q1" style={Q} />
                </Cell>
              </Row>
            </Table>
          </div>
        )
      )
      expectBoxes(testRoot, {
        ct: [0, 0, 200, 18],
        t: [0, 0, 100, 18],
        cqa: [0, 0, 100, 6],
        r1: [0, 6, 100, 12],
        q1: [0, 6, 70, 12],
      })
    })
  })

  /**
   * A cell, a row, a row group and a table with `container-type` cannot be size contained (a cell is as wide as
   * its content), so a size query under them is unknown. They are still the container a query finds, which
   * `container-hosts.test.tsx` checks against Chrome with a container above.
   *
   * ```html
   * <div id="t" style="display:table;border-spacing:0"><div id="k" style="display:table-cell;container-type:inline-size">
   *   <div id="q1" class="q" style="height:5px"></div><div id="bx" style="width:100px;height:5px"></div></div></div>
   * ```
   * with `table-row` and `table-row-group` for `k` (in the table, around the cell), and `inline-table` and
   * `table` for the table `t`. The same: `q1` stays 30 wide, `k` and `t` are 100 wide.
   */
  it("does not size contain a cell, a row, a row group and a table", () => {
    const contained = { containerType: "inline-size" } as const
    const inside = (
      <>
        <div testId="q1" style={{ ...Q, height: 5 }} />
        <div testId="bx" style={{ width: 100, height: 5 }} />
      </>
    )
    const cases: [string, React.ReactElement][] = [
      [
        "cell",
        <Table id="t" style={{ borderSpacing: 0 }}>
          <Cell id="k" style={contained}>
            {inside}
          </Cell>
        </Table>,
      ],
      [
        "row",
        <Table id="t" style={{ borderSpacing: 0 }}>
          <Row id="k" style={contained}>
            <Cell id="c">{inside}</Cell>
          </Row>
        </Table>,
      ],
      [
        "row group",
        <Table id="t" style={{ borderSpacing: 0 }}>
          <Group id="k" style={contained}>
            <Row id="r">
              <Cell id="c">{inside}</Cell>
            </Row>
          </Group>
        </Table>,
      ],
      [
        "table",
        <Table id="t" style={{ borderSpacing: 0, ...contained }}>
          <Row id="r">
            <Cell id="k">{inside}</Cell>
          </Row>
        </Table>,
      ],
      [
        "inline-table",
        <InlineTable id="t" style={{ borderSpacing: 0, ...contained }}>
          <Row id="r">
            <Cell id="k">{inside}</Cell>
          </Row>
        </InlineTable>,
      ],
    ]
    for (const [name, ui] of cases) {
      withRoot((testRoot) => {
        testRoot.render(root(ui))
        const t = laidOut(testRoot, "t")
        expect(t[2], `${name}: the table is as wide as its content`).toBeCloseTo(100, 1)
        expect(laidOut(testRoot, "k")[2], `${name}: k`).toBeCloseTo(100, 1)
        expect(laidOut(testRoot, "q1")[2], `${name}: the query finds no size`).toBeCloseTo(30, 1)
      })
    }
  })

  /**
   * A caption is a container, and size contained: it is no wider than nothing, so a table with only a caption
   * is 0 wide, and the query against that fails.
   *
   * ```html
   * <div id="t" style="display:table;border-spacing:0"><div id="k" style="display:table-caption;container-type:inline-size">
   *   <div id="q1" class="q" style="height:5px"></div><div id="bx" style="width:100px;height:5px"></div></div></div>
   * ```
   * gives `t` and `k` 0 wide and `q1` 30; with a `width:200px` table and a row under it the caption is 200 wide, its
   * `q1` 70 wide and its `cq1` 100.
   */
  it("makes a caption a container", () => {
    withRoot((testRoot) => {
      testRoot.render(
        root(
          <Table id="t" style={{ borderSpacing: 0 }}>
            <Caption id="k" style={{ containerType: "inline-size" }}>
              <div testId="q1" style={{ ...Q, height: 5 }} />
              <div testId="bx" style={{ width: 100, height: 5 }} />
            </Caption>
          </Table>
        )
      )
      expectBoxes(testRoot, { t: [0, 0, 0, 10], k: [0, 0, 0, 10], q1: [0, 0, 30, 5], bx: [0, 5, 100, 5] })
    })
    withRoot((testRoot) => {
      testRoot.render(
        root(
          <Table id="t" style={{ borderSpacing: 0, width: 200 }}>
            <Caption id="cap" style={{ containerType: "inline-size" }}>
              <div testId="q1" style={Q} />
              <div testId="cq1" style={CQ} />
            </Caption>
            <Row id="r1">
              <Cell id="c1">
                <div testId="b" style={{ width: 20, height: 5 }} />
              </Cell>
            </Row>
          </Table>
        )
      )
      expectBoxes(testRoot, {
        t: [0, 0, 200, 23],
        cap: [0, 0, 200, 18],
        q1: [0, 0, 70, 12],
        cq1: [0, 12, 100, 6],
        r1: [0, 18, 200, 5],
        b: [0, 18, 20, 5],
      })
    })
  })
})
