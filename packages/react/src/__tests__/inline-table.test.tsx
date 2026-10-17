/// `display: inline-table`: a table that sits on a line of text as one unit (an atomic inline), with the
/// first baseline of its first row for a baseline. The expected numbers are from headless Chrome
/// (chrome-headless-shell, never the Chrome app); each case says which HTML it is. The text is
/// GpuixTestSquare at 20px with a line height of 20px, whose ascent is the whole em and whose descent is
/// none, so the strut of a line is 20px above the baseline and nothing below it, and every character is
/// 20px wide. The numbers are relative to the container
/// (`<div id=root style="width:...;font:20px/20px GpuixTestSquare">`). Boxes in the cells are fixed-size
/// `div`s, which have no text baseline: a cell's baseline is the bottom of its content.
///
/// Chrome's `vertical-align` on a table cell is the cell's alignment (top, middle, bottom, baseline) and
/// the line keywords (`sub`, `super`, ...) do nothing to it; on the `inline-table` it is the box's
/// alignment on the line.

import fs from "fs"
import React from "react"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const SQUARE_FONT = fs.readFileSync(new URL("./fixtures/gpuix-test-square.ttf", import.meta.url))

const TEXT = { fontFamily: "GpuixTestSquare", fontSize: 20, lineHeight: "20px", color: "#000000" } as const

type Style = NonNullable<React.ComponentProps<"div">["style"]>
type Rect = { x: number; y: number; w: number; h: number }
const r = (x: number, y: number, w: number, h: number): Rect => ({ x, y, w, h })

/** The names the elements of a render are found by, and their boxes relative to the first. */
function setup() {
  const root = createTestRoot()
  root.renderer.registerFont(SQUARE_FONT)
  const refs = new Map<string, PublicInstance>()
  const ref = (name: string) => (instance: PublicInstance | null) => {
    if (instance) refs.set(name, instance)
  }
  const rect = (name: string): Rect => {
    const element = refs.get(name)
    expect(element, `no element ${name}`).toBeDefined()
    const box = element!.getBoundingClientRect()
    const origin = refs.get("root")!.getBoundingClientRect()
    return { x: box.x - origin.x, y: box.y - origin.y, w: box.width, h: box.height }
  }
  return { ...root, ref, rect, element: (name: string) => refs.get(name)! }
}

function expectRect(actual: Rect, expected: Rect, context: string, tolerance = 0.51) {
  for (const key of ["x", "y", "w", "h"] as const) {
    expect(Math.abs(actual[key] - expected[key]), `${context}: ${key} ${actual[key]} != ${expected[key]}`).toBeLessThanOrEqual(
      tolerance
    )
  }
}

const INLINE_TABLE = { display: "inline-table" } as const

type Names = ReturnType<typeof setup>["ref"]

function Row({ names, id, style, children }: { names: Names; id: string; style?: Style; children: React.ReactNode }) {
  return (
    <div ref={names(id)} style={{ display: "table-row", ...style }}>
      {children}
    </div>
  )
}

/** A cell with one fixed-size box in it, named `b` + the number of the cell. */
function Cell({
  names,
  id,
  box,
  boxName = `b${id.slice(1)}`,
  style,
}: {
  names: Names
  id: string
  box?: [number, number]
  boxName?: string
  style?: Style
}) {
  return (
    <div ref={names(id)} style={{ display: "table-cell", ...style }}>
      {box && <div ref={names(boxName)} style={{ width: box[0], height: box[1] }} />}
    </div>
  )
}

function expectAll(rect: (name: string) => Rect, want: Record<string, Rect>, tolerance?: number) {
  if (process.env.DUMP) console.log(JSON.stringify(Object.fromEntries(Object.keys(want).map((k) => [k, rect(k)]))))
  for (const [name, expected] of Object.entries(want)) expectRect(rect(name), expected, name, tolerance)
}

describeNative("inline-table", () => {
  it("sits on the baseline of its first row, whose cells sit on the baseline by their bottom", () => {
    // <div id=root style="width:300px;font:20px/20px GpuixTestSquare"><span id=t1>AA</span>
    //   <div id=t style="display:inline-table"><div id=r1 style="display:table-row">
    //     <div id=c1 style="display:table-cell"><div id=b1 style="width:30px;height:30px"></div></div>
    //     <div id=c2 style="display:table-cell"><div id=b2 style="width:20px;height:10px"></div></div></div>
    //   <div id=r2 style="display:table-row">
    //     <div id=c3 style="display:table-cell"><div id=b3 style="width:30px;height:40px"></div></div>
    //     <div id=c4 style="display:table-cell"></div></div></div><span id=t2>AA</span></div>
    // (written on one line in the page: white space between the parts would make no difference)
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ width: 300, ...TEXT }}>
          <span ref={ref("t1")}>AA</span>
          <div ref={ref("t")} style={INLINE_TABLE}>
            <Row names={ref} id="r1">
              <Cell names={ref} id="c1" box={[30, 30]} />
              <Cell names={ref} id="c2" box={[20, 10]} />
            </Row>
            <Row names={ref} id="r2">
              <Cell names={ref} id="c3" box={[30, 40]} />
              <Cell names={ref} id="c4" />
            </Row>
          </div>
          <span ref={ref("t2")}>AA</span>
        </div>
      )
      expectAll(rect, {
        root: r(0, 0, 300, 70),
        t1: r(0, 10, 40, 20),
        t: r(40, 0, 50, 70),
        r1: r(40, 0, 50, 30),
        c1: r(40, 0, 30, 30),
        b1: r(40, 0, 30, 30),
        c2: r(70, 0, 20, 30),
        b2: r(70, 20, 20, 10),
        r2: r(40, 30, 50, 40),
        c3: r(40, 30, 30, 40),
        b3: r(40, 30, 30, 40),
        c4: r(70, 30, 20, 40),
        t2: r(90, 10, 40, 20),
      })
    } finally {
      unmount()
    }
  })

  it("the cells' vertical-align is the cell's alignment, and the baseline cell decides the table's baseline", () => {
    // <div id=root style="width:300px;font:20px/20px GpuixTestSquare"><span id=t1>AA</span>
    //   <div id=t style="display:inline-table"><div id=r1 style="display:table-row">
    //     <div id=c1 style="display:table-cell;vertical-align:top"><div id=b1 style="width:30px;height:10px"></div></div>
    //     <div id=c2 style="display:table-cell;vertical-align:middle"><div id=b2 style="width:20px;height:10px"></div></div>
    //     <div id=c3 style="display:table-cell;vertical-align:bottom"><div id=b3 style="width:20px;height:10px"></div></div>
    //     <div id=c4 style="display:table-cell;vertical-align:baseline"><div id=b4 style="width:20px;height:30px"></div></div>
    //   </div></div><span id=t2>AA</span></div>
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ width: 300, ...TEXT }}>
          <span ref={ref("t1")}>AA</span>
          <div ref={ref("t")} style={INLINE_TABLE}>
            <Row names={ref} id="r1">
              <Cell names={ref} id="c1" box={[30, 10]} style={{ verticalAlign: "top" }} />
              <Cell names={ref} id="c2" box={[20, 10]} style={{ verticalAlign: "middle" }} />
              <Cell names={ref} id="c3" box={[20, 10]} style={{ verticalAlign: "bottom" }} />
              <Cell names={ref} id="c4" box={[20, 30]} style={{ verticalAlign: "baseline" }} />
            </Row>
          </div>
          <span ref={ref("t2")}>AA</span>
        </div>
      )
      expectAll(rect, {
        root: r(0, 0, 300, 30),
        t1: r(0, 10, 40, 20),
        t: r(40, 0, 90, 30),
        b1: r(40, 0, 30, 10),
        b2: r(70, 10, 20, 10),
        b3: r(90, 20, 20, 10),
        b4: r(110, 0, 20, 30),
        t2: r(130, 10, 40, 20),
      })
    } finally {
      unmount()
    }
  })

  it("a first row with no cell on the baseline has its bottom for the baseline", () => {
    // <div id=root style="width:300px;font:20px/20px GpuixTestSquare"><span id=t1>AA</span>
    //   <div id=t style="display:inline-table"><div id=r1 style="display:table-row">
    //     <div id=c1 style="display:table-cell;vertical-align:top"><div id=b1 style="width:30px;height:10px"></div></div>
    //     <div id=c2 style="display:table-cell;vertical-align:bottom"><div id=b2 style="width:20px;height:20px"></div></div></div>
    //   <div id=r2 style="display:table-row"><div id=c3 style="display:table-cell"><div id=b3 style="width:30px;height:40px"></div></div></div>
    //   </div><span id=t2>AA</span></div>
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ width: 300, ...TEXT }}>
          <span ref={ref("t1")}>AA</span>
          <div ref={ref("t")} style={INLINE_TABLE}>
            <Row names={ref} id="r1">
              <Cell names={ref} id="c1" box={[30, 10]} style={{ verticalAlign: "top" }} />
              <Cell names={ref} id="c2" box={[20, 20]} style={{ verticalAlign: "bottom" }} />
            </Row>
            <Row names={ref} id="r2">
              <Cell names={ref} id="c3" box={[30, 40]} />
            </Row>
          </div>
          <span ref={ref("t2")}>AA</span>
        </div>
      )
      expectAll(rect, {
        root: r(0, 0, 300, 60),
        t1: r(0, 0, 40, 20),
        t: r(40, 0, 50, 60),
        c1: r(40, 0, 30, 20),
        b2: r(70, 0, 20, 20),
        b3: r(40, 20, 30, 40),
        t2: r(90, 0, 40, 20),
      })
    } finally {
      unmount()
    }
  })

  it("the baseline is below the caption, border, padding and spacing, and the margin is around the box", () => {
    // <div id=root style="width:300px;font:20px/20px GpuixTestSquare"><span id=t1>AA</span>
    //   <div id=t style="display:inline-table;margin:4px 6px 8px 2px;border-spacing:3px;border:2px solid black;padding:1px">
    //     <div id=cap style="display:table-caption;height:12px"></div>
    //     <div id=r1 style="display:table-row"><div id=c1 style="display:table-cell"><div id=b1 style="width:30px;height:20px"></div></div>
    //     <div id=c2 style="display:table-cell"><div id=b2 style="width:20px;height:10px"></div></div></div></div><span id=t2>AA</span></div>
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ width: 300, ...TEXT }}>
          <span ref={ref("t1")}>AA</span>
          <div
            ref={ref("t")}
            style={{
              ...INLINE_TABLE,
              marginTop: 4,
              marginRight: 6,
              marginBottom: 8,
              marginLeft: 2,
              borderSpacing: 3,
              border: "2px solid black",
              padding: 1,
            }}
          >
            <div ref={ref("cap")} style={{ display: "table-caption", height: 12 }} />
            <Row names={ref} id="r1">
              <Cell names={ref} id="c1" box={[30, 20]} />
              <Cell names={ref} id="c2" box={[20, 10]} />
            </Row>
          </div>
          <span ref={ref("t2")}>AA</span>
        </div>
      )
      expectAll(rect, {
        root: r(0, 0, 300, 56),
        t1: r(0, 22, 40, 20),
        t: r(42, 4, 65, 44),
        cap: r(42, 4, 65, 12),
        r1: r(48, 22, 53, 20),
        c1: r(48, 22, 30, 20),
        b2: r(81, 32, 20, 10),
        t2: r(113, 22, 40, 20),
      })
    } finally {
      unmount()
    }
  })

  it("is aligned on the line by its own vertical-align, as an inline-block is", () => {
    // <div id=root style="width:300px;font:20px/20px GpuixTestSquare"><span id=t1>AA</span>
    //   <div id=t style="display:inline-table;vertical-align:KEYWORD"><div id=r1 style="display:table-row">
    //     <div id=c1 style="display:table-cell"><div id=b1 style="width:30px;height:50px"></div></div></div></div>
    //   <span id=t2>AA</span></div>
    // The row of Chrome numbers is [root height, y of t1], the table is 50 tall at y=0. middle, sub and
    // super move the text by a fraction of a pixel in Chrome, which gpui puts at its own x-height.
    const cases: [string, number, number][] = [
      ["top", 50, 0],
      ["bottom", 50, 30],
      ["text-top", 50, 0],
      ["text-bottom", 50, 30],
      ["sub", 50, 25],
    ]
    for (const [align, height, textY] of cases) {
      const { render, ref, rect, unmount } = setup()
      try {
        render(
          <div ref={ref("root")} style={{ width: 300, ...TEXT }}>
            <span ref={ref("t1")}>AA</span>
            <div ref={ref("t")} style={{ ...INLINE_TABLE, verticalAlign: align }}>
              <Row names={ref} id="r1">
                <Cell names={ref} id="c1" box={[30, 50]} />
              </Row>
            </div>
            <span ref={ref("t2")}>AA</span>
          </div>
        )
        expectAll(rect, {
          root: r(0, 0, 300, height),
          t1: r(0, textY, 40, 20),
          t: r(40, 0, 30, 50),
          t2: r(70, textY, 40, 20),
        })
      } finally {
        unmount()
      }
    }
  })

  it("wraps with the text as one unit, and a table that does not fit starts a line", () => {
    // <div id=root style="width:100px;font:20px/20px GpuixTestSquare">
    //   <div id=ta style="display:inline-table"><div style="display:table-row"><div id=ca style="display:table-cell"><div id=ba style="width:60px;height:20px"></div></div></div></div>
    //   <div id=tb style="display:inline-table"><div style="display:table-row"><div id=cb style="display:table-cell"><div id=bb style="width:50px;height:30px"></div></div></div></div>
    //   <span id=t3>A</span></div>
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ width: 100, ...TEXT }}>
          <div ref={ref("ta")} style={INLINE_TABLE}>
            <div style={{ display: "table-row" }}>
              <Cell names={ref} id="ca" box={[60, 20]} />
            </div>
          </div>
          <div ref={ref("tb")} style={INLINE_TABLE}>
            <div style={{ display: "table-row" }}>
              <Cell names={ref} id="cb" box={[50, 30]} />
            </div>
          </div>
          <span ref={ref("t3")}>A</span>
        </div>
      )
      expectAll(rect, {
        root: r(0, 0, 100, 50),
        ta: r(0, 0, 60, 20),
        ba: r(0, 0, 60, 20),
        tb: r(0, 20, 50, 30),
        bb: r(0, 20, 50, 30),
        t3: r(50, 30, 20, 20),
      })
    } finally {
      unmount()
    }
  })

  it("a percentage width is of the paragraph, and a table wider than the line overflows it on a line of its own", () => {
    // <div id=root style="width:200px;font:20px/20px GpuixTestSquare">
    //   <div id=ta style="display:inline-table;width:50%"><div style="display:table-row">
    //     <div id=ca style="display:table-cell"><div id=ba style="width:30px;height:20px"></div></div>
    //     <div id=cb style="display:table-cell"><div id=bb style="width:30px;height:20px"></div></div></div></div>
    //   <div id=tb style="display:inline-table"><div style="display:table-row">
    //     <div id=cc style="display:table-cell"><div id=bc style="width:250px;height:10px"></div></div></div></div></div>
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ width: 200, ...TEXT }}>
          <div ref={ref("ta")} style={{ ...INLINE_TABLE, width: "50%" }}>
            <div style={{ display: "table-row" }}>
              <Cell names={ref} id="ca" box={[30, 20]} />
              <Cell names={ref} id="cb" box={[30, 20]} />
            </div>
          </div>{" "}
          <div ref={ref("tb")} style={INLINE_TABLE}>
            <div style={{ display: "table-row" }}>
              <Cell names={ref} id="cc" box={[250, 10]} />
            </div>
          </div>
        </div>
      )
      expectAll(rect, {
        root: r(0, 0, 200, 40),
        ta: r(0, 0, 100, 20),
        ca: r(0, 0, 50, 20),
        ba: r(0, 0, 30, 20),
        cb: r(50, 0, 50, 20),
        bb: r(50, 0, 30, 20),
        tb: r(0, 30, 250, 10),
        bc: r(0, 30, 250, 10),
      })
    } finally {
      unmount()
    }
  })

  it("shrinks to the line, wrapping the text of its cells, down to their min-content width", () => {
    // <div id=root style="width:60px;font:20px/20px GpuixTestSquare"><div id=t style="display:inline-table">
    //   <div style="display:table-row"><div id=c1 style="display:table-cell">AB CD</div></div></div></div>
    // Chrome: the table is 60 wide (max-content 100, min-content 40) and 40 tall; at 30px it would be 40 wide.
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ width: 60, ...TEXT }}>
          <div ref={ref("t")} style={INLINE_TABLE}>
            <div style={{ display: "table-row" }}>
              <div ref={ref("c1")} style={{ display: "table-cell" }}>
                AB CD
              </div>
            </div>
          </div>
        </div>
      )
      expectAll(rect, { root: r(0, 0, 60, 40), t: r(0, 0, 60, 40), c1: r(0, 0, 60, 40) }, 1.01)
    } finally {
      unmount()
    }
  })

  it("is moved by its relative offsets without moving the text", () => {
    // <div id=root style="width:300px;font:20px/20px GpuixTestSquare"><span id=t1>AA</span>
    //   <div id=t style="display:inline-table;position:relative;left:10px;top:5px"><div id=r1 style="display:table-row">
    //   <div id=c1 style="display:table-cell"><div id=b1 style="width:30px;height:30px"></div></div></div></div><span id=t2>AA</span></div>
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ width: 300, ...TEXT }}>
          <span ref={ref("t1")}>AA</span>
          <div ref={ref("t")} style={{ ...INLINE_TABLE, position: "relative", left: 10, top: 5 }}>
            <Row names={ref} id="r1">
              <Cell names={ref} id="c1" box={[30, 30]} />
            </Row>
          </div>
          <span ref={ref("t2")}>AA</span>
        </div>
      )
      expectAll(rect, {
        root: r(0, 0, 300, 30),
        t1: r(0, 10, 40, 20),
        t: r(50, 5, 30, 30),
        b1: r(50, 5, 30, 30),
        t2: r(70, 10, 40, 20),
      })
    } finally {
      unmount()
    }
  })

  it("is a block-level table where the parent lays out items, or in a table part, which wraps it in a cell", () => {
    // <div id=root style="width:300px;display:flex;flex-direction:column;font:20px/20px GpuixTestSquare">
    //   <div id=t style="display:inline-table"><div style="display:table-row"><div id=c1 style="display:table-cell">
    //   <div id=b1 style="width:30px;height:30px"></div></div></div></div></div>
    // Chrome: the flex item is a table that is stretched, 300x30.
    const flex = setup()
    try {
      flex.render(
        <div ref={flex.ref("root")} style={{ width: 300, display: "flex", flexDirection: "column", ...TEXT }}>
          <div ref={flex.ref("t")} style={INLINE_TABLE}>
            <div style={{ display: "table-row" }}>
              <Cell names={flex.ref} id="c1" box={[30, 30]} />
            </div>
          </div>
        </div>
      )
      expectAll(flex.rect, { root: r(0, 0, 300, 30), t: r(0, 0, 300, 30), c1: r(0, 0, 300, 30), b1: r(0, 0, 30, 30) })
    } finally {
      flex.unmount()
    }

    // <div id=root style="width:300px;font:20px/20px GpuixTestSquare"><div id=t style="display:table">
    //   <div id=r style="display:table-row"><div id=c1 style="display:table-cell"><div id=b1 style="width:30px;height:30px"></div></div>
    //   <div id=nt style="display:table"><div style="display:table-row"><div id=nc style="display:table-cell"><div id=nb style="width:10px;height:10px"></div></div></div></div>
    //   <div id=it style="display:inline-table"><div style="display:table-row"><div id=ic style="display:table-cell"><div id=ib style="width:10px;height:10px"></div></div></div></div>
    //   </div></div></div>
    // Chrome: a table (inline or not) in a row is not a part of it: an anonymous cell wraps both, 10px wide.
    const row = setup()
    try {
      row.render(
        <div ref={row.ref("root")} style={{ width: 300, ...TEXT }}>
          <div ref={row.ref("t")} style={{ display: "table" }}>
            <Row names={row.ref} id="r">
              <Cell names={row.ref} id="c1" box={[30, 30]} />
              <div ref={row.ref("nt")} style={{ display: "table" }}>
                <div style={{ display: "table-row" }}>
                  <Cell names={row.ref} id="nc" boxName="nb" box={[10, 10]} />
                </div>
              </div>
              <div ref={row.ref("it")} style={INLINE_TABLE}>
                <div style={{ display: "table-row" }}>
                  <Cell names={row.ref} id="ic" boxName="ib" box={[10, 10]} />
                </div>
              </div>
            </Row>
          </div>
        </div>
      )
      expectAll(row.rect, {
        root: r(0, 0, 300, 50),
        t: r(0, 0, 40, 50),
        r: r(0, 0, 40, 50),
        c1: r(0, 0, 30, 50),
        b1: r(0, 0, 30, 30),
        nt: r(30, 20, 10, 10),
        nb: r(30, 20, 10, 10),
        it: r(30, 40, 10, 10),
        ib: r(30, 40, 10, 10),
      })
    } finally {
      row.unmount()
    }
  })
})
