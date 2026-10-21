/// Text has a first baseline in layout: a table cell on the baseline lines up by the first line of its
/// text, an `inline-table` sits on the first baseline of its first row, and flex and grid items with
/// `align-items: baseline` line up by the text in them, as in Chrome.
///
/// `fixtures/text-baseline-oracle.html` is the page and `fixtures/text-baseline-oracle.json` the boxes
/// headless Chrome reports for it (`getBoundingClientRect` relative to the box of the case,
/// `[x, y, width, height]`; regenerate with `fixtures/dsf-oracle.sh text-baseline`), and
/// `fixtures/text-baseline-oracle-x2.json` the same on a display of device scale 2, where the half-leading of a line
/// that is not whole is floored to a device pixel and not a CSS pixel (the `odd-` cases). The text is GpuixTestSquare, every
/// character one em wide with the whole em for the ascent and no descent, so the baseline of a line of
/// text is the half-leading plus the font size below the top of the line.

import fs from "fs"
import path from "path"
import type { ReactElement, ReactNode } from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const SQUARE_FONT = fs.readFileSync(new URL("./fixtures/gpuix-test-square.ttf", import.meta.url))

const TEXT = { fontFamily: "GpuixTestSquare", fontSize: 20, lineHeight: "20px", color: "#000000" } as const

type Rect = [number, number, number, number]

/**
 * The width of a text is its shaped width up to 1/64 of a pixel, and the font's units make 20px of "A"
 * 20.000002, which does not move what follows it by more than a unit; boxes are painted on device pixels. The baselines are
 * vertical: `y` and `height` are compared exactly.
 */
const WIDTH_TOLERANCE = 0.51

type Style = Record<string, number | string>

const SCALES = [1, 2] as const
type Scale = (typeof SCALES)[number]

const oracleOf = (file: string): Record<string, Rect> =>
  JSON.parse(fs.readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures", file), "utf8"))

const ORACLE: Record<Scale, Record<string, Rect>> = {
  1: oracleOf("text-baseline-oracle.json"),
  2: oracleOf("text-baseline-oracle-x2.json"),
}

function chrome(scale: Scale, id: string): Rect {
  const box = ORACLE[scale][id]
  expect(box, `no oracle box ${id}`).toBeDefined()
  return box ?? [0, 0, 0, 0]
}

type Names = (name: string) => (instance: PublicInstance | null) => void

/** Renders `inner` in the container of the case `scope`, at both scales, and compares the boxes of `names` with Chrome's. */
function check(scope: string, containerStyle: Style, inner: (ref: Names) => ReactElement, names: string[]) {
  for (const scale of SCALES) {
    checkAt(scale, scope, containerStyle, inner, names)
  }
}

function checkAt(
  scale: Scale,
  scope: string,
  containerStyle: Style,
  inner: (ref: Names) => ReactElement,
  names: string[]
) {
  const test = createTestRoot()
  test.renderer.setWindowScaleFactor(scale)
  test.renderer.registerFont(SQUARE_FONT)
  const refs = new Map<string, PublicInstance>()
  const ref: Names = (name) => (instance) => {
    if (instance) refs.set(name, instance)
  }
  const rect = (name: string): Rect => {
    test.renderer.dispatchNativeEvents()
    const element = refs.get(name)
    expect(element, `no element ${name}`).toBeDefined()
    const box = element?.getBoundingClientRect() ?? { x: 0, y: 0, width: 0, height: 0 }
    const origin = refs.get("root")?.getBoundingClientRect() ?? { x: 0, y: 0 }
    return [box.x - origin.x, box.y - origin.y, box.width, box.height]
  }
  try {
    test.render(
      <div ref={ref("root")} style={{ width: 300, ...TEXT, ...containerStyle }}>
        {inner(ref)}
      </div>
    )
    if (process.env.DUMP)
      console.log(scope, JSON.stringify(Object.fromEntries(["root", ...names].map((name) => [name, rect(name)]))))
    for (const name of ["root", ...names]) {
      const expected = chrome(scale, name === "root" ? scope : `${scope}.${name}`)
      const actual = rect(name)
      for (const [ix, key] of ["x", "y", "w", "h"].entries()) {
        expect(
          Math.abs((actual[ix] ?? 0) - (expected[ix] ?? 0)),
          `${scope}.${name} (scale ${scale}): ${key} ${actual[ix]} != ${expected[ix]}`
        ).toBeLessThanOrEqual(key === "y" || key === "h" ? 0.51 / scale : WIDTH_TOLERANCE)
      }
    }
  } finally {
    test.unmount()
  }
}

const BIG = { fontSize: 40, lineHeight: "40px" } as const

function Part({ display, id, ref, style, children }: { display: string; id: string; ref: Names; style?: Style; children?: ReactNode }) {
  return (
    <div ref={ref(id)} style={{ display, ...style }}>
      {children}
    </div>
  )
}

const Table = (props: { id: string; ref: Names; style?: Style; children?: ReactNode }) => <Part display="table" {...props} />
const Row = (props: { id: string; ref: Names; style?: Style; children?: ReactNode }) => (
  <Part display="table-row" {...props} />
)
const Cell = (props: { id: string; ref: Names; style?: Style; children?: ReactNode }) => (
  <Part display="table-cell" {...props} />
)
const Letter = ({ id, ref }: { id: string; ref: Names }) => <span ref={ref(id)}>A</span>

describeNative("text baselines in table cells", () => {
  it("line up cells of different font sizes and line heights by the baseline of their text", () => {
    check(
      "cell-sizes",
      {},
      (ref) => (
        <Table id="t" ref={ref}>
          <Row id="r" ref={ref}>
            <Cell id="c1" ref={ref}>
              <Letter id="s1" ref={ref} />
            </Cell>
            <Cell id="c2" ref={ref} style={BIG}>
              <Letter id="s2" ref={ref} />
            </Cell>
            <Cell id="c3" ref={ref} style={{ fontSize: 30, lineHeight: "60px" }}>
              <Letter id="s3" ref={ref} />
            </Cell>
          </Row>
        </Table>
      ),
      ["t", "r", "c1", "s1", "c2", "s2", "c3", "s3"]
    )
  })

  it("use the first line of a cell with several", () => {
    check(
      "cell-lines",
      {},
      (ref) => (
        <Table id="t" ref={ref}>
          <Row id="r" ref={ref}>
            <Cell id="c1" ref={ref} style={{ whiteSpace: "pre" }}>
              <span ref={ref("s1")}>{"A\nA\nA"}</span>
            </Cell>
            <Cell id="c2" ref={ref} style={BIG}>
              <Letter id="s2" ref={ref} />
            </Cell>
          </Row>
        </Table>
      ),
      ["t", "r", "c1", "s1", "c2", "s2"]
    )
  })

  it("use the first line of the first block, below the padding and margin above it", () => {
    check(
      "cell-blocks",
      {},
      (ref) => (
        <Table id="t" ref={ref}>
          <Row id="r" ref={ref}>
            <Cell id="c1" ref={ref} style={{ padding: 5 }}>
              <div ref={ref("d1")} style={{ marginTop: 7, paddingTop: 3 }}>
                <Letter id="s1" ref={ref} />
              </div>
              <div ref={ref("d2")}>
                <Letter id="s2" ref={ref} />
              </div>
            </Cell>
            <Cell id="c2" ref={ref} style={BIG}>
              <Letter id="s3" ref={ref} />
            </Cell>
          </Row>
        </Table>
      ),
      ["t", "r", "c1", "d1", "s1", "d2", "s2", "c2", "s3"]
    )
  })

  it("put a cell without text on the baseline by its bottom, next to cells with text", () => {
    check(
      "cell-mixed",
      {},
      (ref) => (
        <Table id="t" ref={ref}>
          <Row id="r" ref={ref}>
            <Cell id="c1" ref={ref}>
              <Letter id="s1" ref={ref} />
            </Cell>
            <Cell id="c2" ref={ref}>
              <div ref={ref("b2")} style={{ width: 30, height: 30 }} />
            </Cell>
            <Cell id="c3" ref={ref} style={BIG}>
              <Letter id="s3" ref={ref} />
            </Cell>
          </Row>
        </Table>
      ),
      ["t", "r", "c1", "s1", "c2", "b2", "c3", "s3"]
    )
  })

  it("leave cells aligned to the top or the bottom where they are", () => {
    check(
      "cell-aligns",
      {},
      (ref) => (
        <Table id="t" ref={ref}>
          <Row id="r" ref={ref}>
            <Cell id="c1" ref={ref} style={{ verticalAlign: "top" }}>
              <Letter id="s1" ref={ref} />
            </Cell>
            <Cell id="c2" ref={ref} style={BIG}>
              <Letter id="s2" ref={ref} />
            </Cell>
            <Cell id="c3" ref={ref} style={{ verticalAlign: "bottom" }}>
              <Letter id="s3" ref={ref} />
            </Cell>
          </Row>
        </Table>
      ),
      ["t", "r", "c1", "s1", "c2", "s2", "c3", "s3"]
    )
  })

  it("give each row a baseline of its own", () => {
    check(
      "cell-rows",
      {},
      (ref) => (
        <Table id="t" ref={ref}>
          <Row id="r1" ref={ref}>
            <Cell id="c1" ref={ref}>
              <Letter id="s1" ref={ref} />
            </Cell>
            <Cell id="c2" ref={ref} style={BIG}>
              <Letter id="s2" ref={ref} />
            </Cell>
          </Row>
          <Row id="r2" ref={ref}>
            <Cell id="c3" ref={ref} style={BIG}>
              <Letter id="s3" ref={ref} />
            </Cell>
            <Cell id="c4" ref={ref}>
              <Letter id="s4" ref={ref} />
            </Cell>
          </Row>
        </Table>
      ),
      ["t", "r1", "c1", "s1", "c2", "s2", "r2", "c3", "s3", "c4", "s4"]
    )
  })
})

describeNative("an inline-table with text", () => {
  const INLINE_TABLE = { display: "inline-table" } as const

  it("sits on the baseline of the text in its first row", () => {
    check(
      "inline-table",
      {},
      (ref) => (
        <>
          <span ref={ref("t1")}>AA</span>
          <Table id="t" ref={ref} style={INLINE_TABLE}>
            <Row id="r1" ref={ref}>
              <Cell id="c1" ref={ref} style={{ fontSize: 30, lineHeight: "30px" }}>
                <Letter id="s1" ref={ref} />
              </Cell>
              <Cell id="c2" ref={ref}>
                <Letter id="s2" ref={ref} />
              </Cell>
            </Row>
            <Row id="r2" ref={ref}>
              <Cell id="c3" ref={ref} style={BIG}>
                <Letter id="s3" ref={ref} />
              </Cell>
            </Row>
          </Table>
          <span ref={ref("t2")}>AA</span>
        </>
      ),
      ["t1", "t", "r1", "c1", "s1", "c2", "s2", "r2", "c3", "s3", "t2"]
    )
  })

  it("has the text of its first row below the caption, border, padding and spacing", () => {
    check(
      "inline-table-frame",
      {},
      (ref) => (
        <>
          <span ref={ref("t1")}>AA</span>
          <Table
            id="t"
            ref={ref}
            style={{ ...INLINE_TABLE, border: "2px solid #000000", padding: 1, borderSpacing: 3 }}
          >
            <div ref={ref("cap")} style={{ display: "table-caption", height: 12 }} />
            <Row id="r1" ref={ref}>
              <Cell id="c1" ref={ref} style={BIG}>
                <Letter id="s1" ref={ref} />
              </Cell>
            </Row>
          </Table>
          <span ref={ref("t2")}>AA</span>
        </>
      ),
      ["t1", "t", "cap", "r1", "c1", "s1", "t2"]
    )
  })

  it("sits on the bottom of its first row when no cell of it is on the baseline", () => {
    check(
      "inline-table-bottom",
      {},
      (ref) => (
        <>
          <span ref={ref("t1")}>AA</span>
          <Table id="t" ref={ref} style={INLINE_TABLE}>
            <Row id="r1" ref={ref}>
              <Cell id="c1" ref={ref} style={{ ...BIG, verticalAlign: "top" }}>
                <Letter id="s1" ref={ref} />
              </Cell>
            </Row>
          </Table>
          <span ref={ref("t2")}>AA</span>
        </>
      ),
      ["t1", "t", "r1", "c1", "s1", "t2"]
    )
  })
})

describeNative("align-items: baseline in a flex row", () => {
  const FLEX = { display: "flex", alignItems: "baseline" } as const

  it("lines items up by their text, and a box without text by its bottom", () => {
    check(
      "flex",
      FLEX,
      (ref) => (
        <>
          <div ref={ref("a")}>A</div>
          <div ref={ref("b")} style={BIG}>
            A
          </div>
          <div ref={ref("c")} style={{ padding: 10, fontSize: 30, lineHeight: "30px" }}>
            A
          </div>
          <div ref={ref("d")} style={{ width: 20, height: 20 }} />
        </>
      ),
      ["a", "b", "c", "d"]
    )
  })

  it("uses the first line of an item with several", () => {
    check(
      "flex-lines",
      FLEX,
      (ref) => (
        <>
          <div ref={ref("a")} style={{ whiteSpace: "pre" }}>
            {"A\nA"}
          </div>
          <div ref={ref("b")} style={BIG}>
            A
          </div>
        </>
      ),
      ["a", "b"]
    )
  })

  it("lines up text that is not in a box of its own", () => {
    check(
      "flex-raw",
      FLEX,
      (ref) => (
        <>
          A
          <div ref={ref("b")} style={BIG}>
            A
          </div>
        </>
      ),
      ["b"]
    )
  })

  it("counts the margin of a block in the item above the baseline", () => {
    check(
      "flex-block",
      FLEX,
      (ref) => (
        <>
          <div ref={ref("a")}>
            <div ref={ref("a1")} style={{ marginTop: 7 }}>
              <Letter id="s1" ref={ref} />
            </div>
          </div>
          <div ref={ref("b")} style={BIG}>
            A
          </div>
        </>
      ),
      ["a", "a1", "s1", "b"]
    )
  })

  it("counts the half-leading of the line above the baseline", () => {
    check(
      "flex-leading",
      FLEX,
      (ref) => (
        <>
          <div ref={ref("a")} style={{ lineHeight: "60px" }}>
            A
          </div>
          <div ref={ref("b")} style={BIG}>
            A
          </div>
        </>
      ),
      ["a", "b"]
    )
  })

  it("gives a flex row the baseline of its first item", () => {
    check(
      "flex-nested",
      FLEX,
      (ref) => (
        <>
          <div ref={ref("a")}>A</div>
          <div ref={ref("inner")} style={{ display: "flex", alignItems: "flex-start" }}>
            <div ref={ref("ia")} style={BIG}>
              A
            </div>
            <div ref={ref("ib")} style={{ width: 20, height: 20 }} />
          </div>
        </>
      ),
      ["a", "inner", "ia", "ib"]
    )
  })

  it("gives a flex column the baseline of its first item", () => {
    check(
      "flex-column",
      FLEX,
      (ref) => (
        <>
          <div ref={ref("a")}>A</div>
          <div ref={ref("col")} style={{ display: "flex", flexDirection: "column" }}>
            <div ref={ref("c1")} style={BIG}>
              A
            </div>
            <div ref={ref("c2")}>A</div>
          </div>
        </>
      ),
      ["a", "col", "c1", "c2"]
    )
  })

  it("gives a table the baseline of its first row", () => {
    check(
      "flex-table",
      FLEX,
      (ref) => (
        <>
          <div ref={ref("a")}>A</div>
          <Table id="t" ref={ref}>
            <Row id="r" ref={ref}>
              <Cell id="c1" ref={ref} style={BIG}>
                <Letter id="s1" ref={ref} />
              </Cell>
            </Row>
          </Table>
        </>
      ),
      ["a", "t", "c1", "s1"]
    )
  })

  it("gives a grid the baseline of its first row", () => {
    check(
      "flex-grid",
      FLEX,
      (ref) => (
        <>
          <div ref={ref("a")}>A</div>
          <div ref={ref("g")} style={{ display: "grid", gridTemplateColumns: "40px" }}>
            <div ref={ref("g1")} style={BIG}>
              A
            </div>
          </div>
        </>
      ),
      ["a", "g", "g1"]
    )
  })

  it("gives a grid the baseline of its first item below the margin of the item", () => {
    check(
      "flex-grid-margin",
      FLEX,
      (ref) => (
        <>
          <div ref={ref("a")}>A</div>
          <div ref={ref("g")} style={{ display: "grid", gridTemplateColumns: "40px" }}>
            <div ref={ref("g1")} style={{ ...BIG, marginTop: 5, paddingBottom: 10 }}>
              A
            </div>
          </div>
        </>
      ),
      ["a", "g", "g1"]
    )
  })

  it("clamps the baseline of an item that clips to its box", () => {
    check(
      "flex-clip",
      FLEX,
      (ref) => (
        <>
          <div ref={ref("a")} style={{ overflow: "hidden", height: 10 }}>
            A
          </div>
          <div ref={ref("b")} style={BIG}>
            A
          </div>
        </>
      ),
      ["a", "b"]
    )
  })

  it("lines up each line of a wrapping row by itself", () => {
    check(
      "flex-wrap",
      { ...FLEX, flexWrap: "wrap", width: 70 },
      (ref) => (
        <>
          <div ref={ref("a")}>A</div>
          <div ref={ref("b")} style={BIG}>
            A
          </div>
          <div ref={ref("c")}>A</div>
          <div ref={ref("d")} style={BIG}>
            A
          </div>
        </>
      ),
      ["a", "b", "c", "d"]
    )
  })

  it("moves only the items with align-self: baseline", () => {
    check(
      "flex-self",
      { display: "flex", alignItems: "center", height: 60 },
      (ref) => (
        <>
          <div ref={ref("a")} style={{ alignSelf: "baseline" }}>
            A
          </div>
          <div ref={ref("b")} style={{ ...BIG, alignSelf: "baseline" }}>
            A
          </div>
          <div ref={ref("c")}>A</div>
        </>
      ),
      ["a", "b", "c"]
    )
  })
})

describeNative("align-items: baseline in a grid", () => {
  const GRID = { display: "grid", gridTemplateColumns: "50px 50px 50px", alignItems: "baseline" } as const

  it("lines up the items of a row by their text", () => {
    check(
      "grid",
      GRID,
      (ref) => (
        <>
          <div ref={ref("a")}>A</div>
          <div ref={ref("b")} style={BIG}>
            A
          </div>
          <div ref={ref("c")} style={{ padding: 10, fontSize: 30, lineHeight: "30px" }}>
            A
          </div>
          <div ref={ref("d")} style={BIG}>
            A
          </div>
          <div ref={ref("e")}>A</div>
          <div ref={ref("f")} style={{ padding: 5 }}>
            A
          </div>
        </>
      ),
      ["a", "b", "c", "d", "e", "f"]
    )
  })

  it("lines up a box without text by its bottom, and text with a taller line", () => {
    check(
      "grid-mixed",
      GRID,
      (ref) => (
        <>
          <div ref={ref("a")} style={{ width: 20, height: 20 }} />
          <div ref={ref("b")} style={{ lineHeight: "60px" }}>
            A
          </div>
          <div ref={ref("c")}>A</div>
        </>
      ),
      ["a", "b", "c"]
    )
  })

  it("moves only the items with align-self: baseline", () => {
    check(
      "grid-self",
      { ...GRID, alignItems: "start" },
      (ref) => (
        <>
          <div ref={ref("a")} style={{ alignSelf: "baseline" }}>
            A
          </div>
          <div ref={ref("b")} style={{ ...BIG, alignSelf: "baseline" }}>
            A
          </div>
          <div ref={ref("c")}>A</div>
        </>
      ),
      ["a", "b", "c"]
    )
  })
})

describeNative("an inline-flex with text", () => {
  it("sits on the baseline of its first item", () => {
    check(
      "inline-flex",
      {},
      (ref) => (
        <>
          <span ref={ref("t1")}>AA</span>
          <div ref={ref("f")} style={{ display: "inline-flex", padding: 3 }}>
            <div ref={ref("i1")} style={BIG}>
              A
            </div>
          </div>
          <span ref={ref("t2")}>AA</span>
        </>
      ),
      ["t1", "f", "i1", "t2"]
    )
  })
})

describeNative("a half-leading that is not whole", () => {
  const ODD = { lineHeight: "45px" } as const

  it("is floored to a device pixel in the cells of a row", () => {
    check(
      "odd-cells",
      {},
      (ref) => (
        <Table id="t" ref={ref}>
          <Row id="r" ref={ref}>
            <Cell id="c1" ref={ref} style={{ lineHeight: "25px" }}>
              <Letter id="s1" ref={ref} />
            </Cell>
            <Cell id="c2" ref={ref} style={ODD}>
              <Letter id="s2" ref={ref} />
            </Cell>
            <Cell id="c3" ref={ref} style={BIG}>
              <Letter id="s3" ref={ref} />
            </Cell>
          </Row>
        </Table>
      ),
      ["t", "r", "c1", "c2", "c3"]
    )
  })

  it("is floored to a device pixel in a flex row and a grid on the baseline", () => {
    const items = (ref: Names) => (
      <>
        <div ref={ref("a")} style={{ lineHeight: "25px" }}>
          A
        </div>
        <div ref={ref("b")} style={ODD}>
          A
        </div>
        <div ref={ref("c")} style={BIG}>
          A
        </div>
      </>
    )
    check("odd-flex", { display: "flex", alignItems: "baseline" }, items, ["a", "b", "c"])
    check(
      "odd-grid",
      { display: "grid", gridTemplateColumns: "50px 50px 50px", alignItems: "baseline" },
      items,
      ["a", "b", "c"]
    )
  })

  it("is floored to a device pixel in an inline-flex and an inline-table on the baseline", () => {
    check(
      "odd-inline-flex",
      {},
      (ref) => (
        <>
          <span ref={ref("t1")}>AA</span>
          <div ref={ref("f")} style={{ display: "inline-flex", padding: 3 }}>
            <div ref={ref("i1")} style={ODD}>
              A
            </div>
          </div>
          <span ref={ref("t2")}>AA</span>
        </>
      ),
      ["t1", "f", "i1", "t2"]
    )
    check(
      "odd-inline-table",
      { lineHeight: "25px" },
      (ref) => (
        <>
          <span ref={ref("t1")}>AA</span>
          <Part display="inline-table" id="t" ref={ref}>
            <Row id="r" ref={ref}>
              <Cell id="c1" ref={ref} style={ODD}>
                <Letter id="s1" ref={ref} />
              </Cell>
            </Row>
          </Part>
          <span ref={ref("t2")}>AA</span>
        </>
      ),
      ["t1", "t", "c1", "s1", "t2"]
    )
  })
})
