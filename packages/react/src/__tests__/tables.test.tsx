/// CSS tables through the React API: `display: table` and its parts, `colSpan`, `rowSpan`, `span`, and the
/// table styles.
///
/// Every layout test compares the border box of each element with the one headless Chrome gives for the HTML above
/// it (`getBoundingClientRect`, relative to the 300px wide container). The boxes inside cells are fixed-size
/// `div`s so no font is involved. Chrome draws `display: table*` on a `div` the way it draws a `<table>`, so the
/// HTML uses table tags where that reads shorter, and `div`s where the test is about a `display` value or about
/// something a table tag cannot say (a `div` inside a table, which an anonymous cell wraps). The page around each
/// case:
///
/// ```html
/// <style>
///   * { box-sizing: border-box; margin: 0; padding: 0; border: 0 solid transparent }
///   table { border-spacing: 0; border-collapse: separate }
///   caption { text-align: left }
///   td, th, thead, tbody, tfoot, tr { vertical-align: baseline }
/// </style>
/// <div id="case" style="position: absolute; left: 0; top: 0; width: 300px"> ...the case... </div>
/// ```
///
/// The numbers between `oracle:begin` and `oracle:end` are written by a script that runs Chrome
/// (`chrome-headless-shell --headless --dump-dom`) on those blocks and reads `getBoundingClientRect` of every element
/// with an `id`. A `<col>` has no `id`: Chrome gives it a box that is not its layout.
///
/// Text in a cell is not compared with Chrome, whose fonts are not gpuix's. A cell with text has no baseline here:
/// it is aligned by the bottom of its content, see the README's Tables section.

import fs from "fs"
import React from "react"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { ResizeObserver } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Style = NonNullable<React.ComponentProps<"div">["style"]>

interface PartProps {
  id: string
  style?: Style
  children?: React.ReactNode
  colSpan?: number
  rowSpan?: number
  span?: number
}

function part(display: string) {
  return function Part({ id, style, children, ...props }: PartProps) {
    return (
      <div testId={id} style={{ display, ...style }} {...props}>
        {children}
      </div>
    )
  }
}

const Table = part("table")
const Row = part("table-row")
const Cell = part("table-cell")
const Caption = part("table-caption")
const Head = part("table-header-group")
const Body = part("table-row-group")
const Foot = part("table-footer-group")
const Col = part("table-column")
const ColGroup = part("table-column-group")

function Box({ id, w, h, style }: { id: string; w: number; h: number; style?: Style }) {
  return <div testId={id} style={{ width: w, height: h, ...style }} />
}

type Rect = [number, number, number, number]

type TestRoot = ReturnType<typeof createTestRoot>

/** The box of the element with this `testId`, as `[x, y, width, height]`. */
function boxOf(renderer: TestRoot["renderer"], id: string): Rect {
  const element = renderer.findByTestId(id)
  expect(element, `${id} is mounted`).toBeDefined()
  const bounds = renderer.getElementBounds(element!.id)
  expect(bounds, `${id} has a box`).not.toBeNull()
  return [bounds!.x, bounds!.y, bounds!.width, bounds!.height]
}

function withRoot(ui: React.ReactElement, width: number, check: (renderer: TestRoot["renderer"]) => void) {
  const { render, renderer, unmount } = createTestRoot()
  try {
    render(<div style={{ width }}>{ui}</div>)
    check(renderer)
  } finally {
    unmount()
  }
}

/** Every box in `want` is where Chrome has it, to a fraction of a pixel. */
function expectBoxes(ui: React.ReactElement, want: Record<string, Rect>, width = 300) {
  withRoot(ui, width, (renderer) => {
    for (const [id, rect] of Object.entries(want)) {
      const got = boxOf(renderer, id)
      rect.forEach((value, index) => {
        expect(got[index], `${id} ${["x", "y", "width", "height"][index]}: got ${got} want ${rect}`).toBeCloseTo(
          value,
          1
        )
      })
    }
  })
}

describeNative("tables: layout against Chrome", () => {
  /**
   * Columns are as wide as their widest cell, rows as tall as their tallest, and the table is as wide as its
   * columns. A cell's padding and border count.
   *
   * ```html
   * <table id="t">
   *   <tr id="r1">
   *     <td id="a" style="padding:2px"><div id="a1" style="width:30px;height:10px"></div></td>
   *     <td id="b"><div id="b1" style="width:50px;height:20px"></div></td>
   *   </tr>
   *   <tr id="r2">
   *     <td id="c"><div id="c1" style="width:70px;height:15px"></div></td>
   *     <td id="d" style="border-width:3px"><div id="d1" style="width:20px;height:5px"></div></td>
   *   </tr>
   * </table>
   * ```
   */
  it("sizes an auto table by its columns and rows", () => {
    expectBoxes(
      <Table id="t">
        <Row id="r1">
          <Cell id="a" style={{ padding: 2 }}>
            <Box id="a1" w={30} h={10} />
          </Cell>
          <Cell id="b">
            <Box id="b1" w={50} h={20} />
          </Cell>
        </Row>
        <Row id="r2">
          <Cell id="c">
            <Box id="c1" w={70} h={15} />
          </Cell>
          <Cell id="d" style={{ borderWidth: 3, borderStyle: "solid" }}>
            <Box id="d1" w={20} h={5} />
          </Cell>
        </Row>
      </Table>,
      {
        // oracle:begin
        t: [0, 0, 120, 40],
        r1: [0, 0, 120, 22],
        a: [0, 0, 70, 22],
        a1: [2, 10, 30, 10],
        b: [70, 0, 50, 22],
        b1: [70, 0, 50, 20],
        r2: [0, 22, 120, 18],
        c: [0, 22, 70, 18],
        c1: [0, 22, 70, 15],
        d: [70, 22, 50, 18],
        d1: [73, 32, 20, 5],
        // oracle:end
      }
    )
  })

  /**
   * A cell can cover columns and rows.
   *
   * ```html
   * <table id="t">
   *   <tr id="r1">
   *     <td id="a" colspan="2"><div id="a1" style="width:60px;height:10px"></div></td>
   *     <td id="b" rowspan="2" style="vertical-align:top"><div id="b1" style="width:30px;height:30px"></div></td>
   *   </tr>
   *   <tr id="r2">
   *     <td id="c"><div id="c1" style="width:40px;height:10px"></div></td>
   *     <td id="d"><div id="d1" style="width:50px;height:20px"></div></td>
   *   </tr>
   * </table>
   * ```
   */
  it("spans columns with colSpan and rows with rowSpan", () => {
    expectBoxes(
      <Table id="t">
        <Row id="r1">
          <Cell id="a" colSpan={2}>
            <Box id="a1" w={60} h={10} />
          </Cell>
          <Cell id="b" rowSpan={2} style={{ verticalAlign: "top" }}>
            <Box id="b1" w={30} h={30} />
          </Cell>
        </Row>
        <Row id="r2">
          <Cell id="c">
            <Box id="c1" w={40} h={10} />
          </Cell>
          <Cell id="d">
            <Box id="d1" w={50} h={20} />
          </Cell>
        </Row>
      </Table>,
      {
        // oracle:begin
        t: [0, 0, 120, 30],
        r1: [0, 0, 120, 10],
        a: [0, 0, 90, 10],
        a1: [0, 0, 60, 10],
        b: [90, 0, 30, 30],
        b1: [90, 0, 30, 30],
        r2: [0, 10, 120, 20],
        c: [0, 10, 40, 20],
        c1: [0, 20, 40, 10],
        d: [40, 10, 50, 20],
        d1: [40, 10, 50, 20],
        // oracle:end
      }
    )
  })

  /**
   * A `rowspan` of 0 reaches the end of the row group, not the end of the table.
   *
   * ```html
   * <table id="t">
   *   <tbody id="g1">
   *     <tr id="r1">
   *       <td id="a" rowspan="0" style="vertical-align:top"><div id="a1" style="width:20px;height:10px"></div></td>
   *       <td id="b"><div id="b1" style="width:30px;height:20px"></div></td>
   *     </tr>
   *     <tr id="r2"><td id="c"><div id="c1" style="width:30px;height:20px"></div></td></tr>
   *     <tr id="r3"><td id="d"><div id="d1" style="width:30px;height:20px"></div></td></tr>
   *   </tbody>
   *   <tbody id="g2">
   *     <tr id="r4">
   *       <td id="e"><div id="e1" style="width:10px;height:10px"></div></td>
   *       <td id="f"><div id="f1" style="width:10px;height:10px"></div></td>
   *     </tr>
   *   </tbody>
   * </table>
   * ```
   */
  it("reaches the end of the row group with a rowSpan of 0", () => {
    expectBoxes(
      <Table id="t">
        <Body id="g1">
          <Row id="r1">
            <Cell id="a" rowSpan={0} style={{ verticalAlign: "top" }}>
              <Box id="a1" w={20} h={10} />
            </Cell>
            <Cell id="b">
              <Box id="b1" w={30} h={20} />
            </Cell>
          </Row>
          <Row id="r2">
            <Cell id="c">
              <Box id="c1" w={30} h={20} />
            </Cell>
          </Row>
          <Row id="r3">
            <Cell id="d">
              <Box id="d1" w={30} h={20} />
            </Cell>
          </Row>
        </Body>
        <Body id="g2">
          <Row id="r4">
            <Cell id="e">
              <Box id="e1" w={10} h={10} />
            </Cell>
            <Cell id="f">
              <Box id="f1" w={10} h={10} />
            </Cell>
          </Row>
        </Body>
      </Table>,
      {
        // oracle:begin
        t: [0, 0, 50, 70],
        g1: [0, 0, 50, 60],
        r1: [0, 0, 50, 20],
        a: [0, 0, 20, 60],
        a1: [0, 0, 20, 10],
        b: [20, 0, 30, 20],
        b1: [20, 0, 30, 20],
        r2: [0, 20, 50, 20],
        c: [20, 20, 30, 20],
        c1: [20, 20, 30, 20],
        r3: [0, 40, 50, 20],
        d: [20, 40, 30, 20],
        d1: [20, 40, 30, 20],
        g2: [0, 60, 50, 10],
        r4: [0, 60, 50, 10],
        e: [0, 60, 20, 10],
        e1: [0, 60, 10, 10],
        f: [20, 60, 30, 10],
        f1: [20, 60, 10, 10],
        // oracle:end
      }
    )
  })

  /**
   * A column's width, a cell's percentage and a cell's length share the table's width.
   *
   * ```html
   * <table id="t" style="width:300px">
   *   <colgroup><col style="width:60px"><col><col style="width:20%"></colgroup>
   *   <tr id="r1">
   *     <td id="a"><div id="a1" style="width:10px;height:10px"></div></td>
   *     <td id="b" style="width:30%"><div id="b1" style="width:10px;height:10px"></div></td>
   *     <td id="c"><div id="c1" style="width:10px;height:10px"></div></td>
   *   </tr>
   *   <tr id="r2">
   *     <td id="d" style="width:100px"><div id="d1" style="width:10px;height:10px"></div></td>
   *     <td id="e"><div id="e1" style="width:10px;height:10px"></div></td>
   *     <td id="f"><div id="f1" style="width:10px;height:10px"></div></td>
   *   </tr>
   * </table>
   * ```
   */
  it("shares the width between px and percentage columns and cells", () => {
    expectBoxes(
      <Table id="t" style={{ width: 300 }}>
        <ColGroup id="cg">
          <Col id="k1" style={{ width: 60 }} />
          <Col id="k2" />
          <Col id="k3" style={{ width: "20%" }} />
        </ColGroup>
        <Row id="r1">
          <Cell id="a">
            <Box id="a1" w={10} h={10} />
          </Cell>
          <Cell id="b" style={{ width: "30%" }}>
            <Box id="b1" w={10} h={10} />
          </Cell>
          <Cell id="c">
            <Box id="c1" w={10} h={10} />
          </Cell>
        </Row>
        <Row id="r2">
          <Cell id="d" style={{ width: 100 }}>
            <Box id="d1" w={10} h={10} />
          </Cell>
          <Cell id="e">
            <Box id="e1" w={10} h={10} />
          </Cell>
          <Cell id="f">
            <Box id="f1" w={10} h={10} />
          </Cell>
        </Row>
      </Table>,
      {
        // oracle:begin
        t: [0, 0, 300, 20],
        r1: [0, 0, 300, 10],
        a: [0, 0, 150, 10],
        a1: [0, 0, 10, 10],
        b: [150, 0, 90, 10],
        b1: [150, 0, 10, 10],
        c: [240, 0, 60, 10],
        c1: [240, 0, 10, 10],
        r2: [0, 10, 300, 10],
        d: [0, 10, 150, 10],
        d1: [0, 10, 10, 10],
        e: [150, 10, 90, 10],
        e1: [150, 10, 10, 10],
        f: [240, 10, 60, 10],
        f1: [240, 10, 10, 10],
        // oracle:end
      }
    )
  })

  /**
   * A fixed table takes its columns from the first row, whatever the later rows hold.
   *
   * ```html
   * <table id="t" style="table-layout:fixed;width:200px">
   *   <tr id="r1">
   *     <td id="a" style="width:50px"><div id="a1" style="width:90px;height:10px"></div></td>
   *     <td id="b"><div id="b1" style="width:10px;height:10px"></div></td>
   *     <td id="c"><div id="c1" style="width:10px;height:10px"></div></td>
   *   </tr>
   *   <tr id="r2"><td id="d" colspan="3"><div id="d1" style="width:250px;height:10px"></div></td></tr>
   * </table>
   * ```
   */
  it("takes the columns of a fixed table from its first row", () => {
    expectBoxes(
      <Table id="t" style={{ width: 200, tableLayout: "fixed" }}>
        <Row id="r1">
          <Cell id="a" style={{ width: 50 }}>
            <Box id="a1" w={90} h={10} />
          </Cell>
          <Cell id="b">
            <Box id="b1" w={10} h={10} />
          </Cell>
          <Cell id="c">
            <Box id="c1" w={10} h={10} />
          </Cell>
        </Row>
        <Row id="r2">
          <Cell id="d" colSpan={3}>
            <Box id="d1" w={250} h={10} />
          </Cell>
        </Row>
      </Table>,
      {
        // oracle:begin
        t: [0, 0, 200, 20],
        r1: [0, 0, 200, 10],
        a: [0, 0, 50, 10],
        a1: [0, 0, 90, 10],
        b: [50, 0, 75, 10],
        b1: [50, 0, 10, 10],
        c: [125, 0, 75, 10],
        c1: [125, 0, 10, 10],
        r2: [0, 10, 200, 10],
        d: [0, 10, 200, 10],
        d1: [0, 10, 250, 10],
        // oracle:end
      }
    )
  })

  /**
   * Spacing goes between the cells and between them and the table's padding and border, which wrap the grid.
   *
   * ```html
   * <table id="t" style="border-spacing:6px 3px;border-width:2px;padding:1px">
   *   <tr id="r1">
   *     <td id="a"><div id="a1" style="width:30px;height:10px"></div></td>
   *     <td id="b"><div id="b1" style="width:40px;height:10px"></div></td>
   *   </tr>
   *   <tr id="r2">
   *     <td id="c"><div id="c1" style="width:30px;height:20px"></div></td>
   *     <td id="d"><div id="d1" style="width:40px;height:5px"></div></td>
   *   </tr>
   * </table>
   * ```
   */
  it("puts borderSpacing between the cells and around the grid", () => {
    expectBoxes(
      <Table id="t" style={{ borderSpacing: "6px 3px", borderWidth: 2, borderStyle: "solid", padding: 1 }}>
        <Row id="r1">
          <Cell id="a">
            <Box id="a1" w={30} h={10} />
          </Cell>
          <Cell id="b">
            <Box id="b1" w={40} h={10} />
          </Cell>
        </Row>
        <Row id="r2">
          <Cell id="c">
            <Box id="c1" w={30} h={20} />
          </Cell>
          <Cell id="d">
            <Box id="d1" w={40} h={5} />
          </Cell>
        </Row>
      </Table>,
      {
        // oracle:begin
        t: [0, 0, 94, 45],
        r1: [9, 6, 76, 10],
        a: [9, 6, 30, 10],
        a1: [9, 6, 30, 10],
        b: [45, 6, 40, 10],
        b1: [45, 6, 40, 10],
        r2: [9, 19, 76, 20],
        c: [9, 19, 30, 20],
        c1: [9, 19, 30, 20],
        d: [45, 19, 40, 20],
        d1: [45, 34, 40, 5],
        // oracle:end
      }
    )
  })

  /**
   * One length is both axes, `em` is the table's font size, and a table inside a cell inherits the spacing.
   *
   * ```html
   * <table id="t" style="border-spacing:2em 4px;font-size:10px">
   *   <tr id="r1">
   *     <td id="a"><div id="a1" style="width:20px;height:10px"></div></td>
   *     <td id="b">
   *       <table id="n" style="border-spacing:inherit">
   *         <tr id="nr">
   *           <td id="na"><div id="na1" style="width:10px;height:10px"></div></td>
   *           <td id="nb"><div id="nb1" style="width:10px;height:10px"></div></td>
   *         </tr>
   *       </table>
   *     </td>
   *   </tr>
   * </table>
   * <table id="u" style="border-spacing:5px;margin-top:100px">
   *   <tr id="ur"><td id="ua"><div id="ua1" style="width:10px;height:10px"></div></td><td id="ub"><div id="ub1" style="width:10px;height:10px"></div></td></tr>
   * </table>
   * ```
   */
  it("inherits borderSpacing and resolves it where it is declared", () => {
    expectBoxes(
      <>
        <Table id="t" style={{ borderSpacing: "2em 4px", fontSize: 10 }}>
          <Row id="r1">
            <Cell id="a">
              <Box id="a1" w={20} h={10} />
            </Cell>
            <Cell id="b">
              <Table id="n" style={{ fontSize: 30 }}>
                <Row id="nr">
                  <Cell id="na">
                    <Box id="na1" w={10} h={10} />
                  </Cell>
                  <Cell id="nb">
                    <Box id="nb1" w={10} h={10} />
                  </Cell>
                </Row>
              </Table>
            </Cell>
          </Row>
        </Table>
        <Table id="u" style={{ borderSpacing: 5, marginTop: 100 }}>
          <Row id="ur">
            <Cell id="ua">
              <Box id="ua1" w={10} h={10} />
            </Cell>
            <Cell id="ub">
              <Box id="ub1" w={10} h={10} />
            </Cell>
          </Row>
        </Table>
      </>,
      {
        // oracle:begin
        t: [0, 0, 160, 26],
        r1: [20, 4, 120, 18],
        a: [20, 4, 20, 18],
        a1: [20, 8, 20, 10],
        b: [60, 4, 80, 18],
        n: [60, 4, 80, 18],
        nr: [80, 8, 40, 10],
        na: [80, 8, 10, 10],
        na1: [80, 8, 10, 10],
        nb: [110, 8, 10, 10],
        nb1: [110, 8, 10, 10],
        u: [0, 126, 35, 20],
        ur: [5, 131, 25, 10],
        ua: [5, 131, 10, 10],
        ua1: [5, 131, 10, 10],
        ub: [20, 131, 10, 10],
        ub1: [20, 131, 10, 10],
        // oracle:end
      }
    )
  })

  /**
   * Captions go above and below the grid, as wide as the table. Its border and padding wrap the grid alone.
   *
   * ```html
   * <table id="t" style="border-width:3px;padding:2px">
   *   <caption id="cap1"><div id="c1" style="width:20px;height:10px"></div></caption>
   *   <tr id="r1">
   *     <td id="a"><div id="a1" style="width:30px;height:10px"></div></td>
   *     <td id="b"><div id="b1" style="width:50px;height:20px"></div></td>
   *   </tr>
   *   <caption id="cap2" style="caption-side:bottom"><div id="c2" style="width:20px;height:6px"></div></caption>
   * </table>
   * ```
   */
  it("puts captions above and below the grid", () => {
    expectBoxes(
      <Table id="t" style={{ borderWidth: 3, borderStyle: "solid", padding: 2 }}>
        <Caption id="cap1">
          <Box id="c1" w={20} h={10} />
        </Caption>
        <Row id="r1">
          <Cell id="a">
            <Box id="a1" w={30} h={10} />
          </Cell>
          <Cell id="b">
            <Box id="b1" w={50} h={20} />
          </Cell>
        </Row>
        <Caption id="cap2" style={{ captionSide: "bottom" }}>
          <Box id="c2" w={20} h={6} />
        </Caption>
      </Table>,
      {
        // oracle:begin
        t: [0, 0, 90, 46],
        cap1: [0, 0, 90, 10],
        c1: [0, 0, 20, 10],
        r1: [5, 15, 80, 20],
        a: [5, 15, 30, 20],
        a1: [5, 25, 30, 10],
        b: [35, 15, 50, 20],
        b1: [35, 15, 50, 20],
        cap2: [0, 40, 90, 6],
        c2: [0, 40, 20, 6],
        // oracle:end
      }
    )
  })

  /**
   * `caption-side` inherits from the table, and a caption wider than the grid widens the table.
   *
   * ```html
   * <table id="t" style="caption-side:bottom">
   *   <caption id="cap"><div id="c" style="width:120px;height:8px"></div></caption>
   *   <tr id="r1"><td id="a"><div id="a1" style="width:30px;height:10px"></div></td></tr>
   * </table>
   * ```
   */
  it("inherits captionSide from the table", () => {
    expectBoxes(
      <Table id="t" style={{ captionSide: "bottom" }}>
        <Caption id="cap">
          <Box id="c" w={120} h={8} />
        </Caption>
        <Row id="r1">
          <Cell id="a">
            <Box id="a1" w={30} h={10} />
          </Cell>
        </Row>
      </Table>,
      {
        // oracle:begin
        t: [0, 0, 120, 18],
        cap: [0, 10, 120, 8],
        c: [0, 10, 120, 8],
        r1: [0, 0, 120, 10],
        a: [0, 0, 120, 10],
        a1: [0, 0, 30, 10],
        // oracle:end
      }
    )
  })

  /**
   * The header group goes first and the footer group last wherever they are, and they keep their height when the
   * table is taller.
   *
   * ```html
   * <table id="t" style="height:80px">
   *   <tfoot id="f"><tr id="fr"><td id="fa"><div id="fa1" style="width:30px;height:10px"></div></td></tr></tfoot>
   *   <tbody id="b"><tr id="br"><td id="ba"><div id="ba1" style="width:20px;height:10px"></div></td></tr></tbody>
   *   <thead id="h"><tr id="hr"><td id="ha"><div id="ha1" style="width:60px;height:12px"></div></td></tr></thead>
   * </table>
   * ```
   */
  it("moves the header group first and the footer group last", () => {
    expectBoxes(
      <Table id="t" style={{ height: 80 }}>
        <Foot id="f">
          <Row id="fr">
            <Cell id="fa">
              <Box id="fa1" w={30} h={10} />
            </Cell>
          </Row>
        </Foot>
        <Body id="b">
          <Row id="br">
            <Cell id="ba">
              <Box id="ba1" w={20} h={10} />
            </Cell>
          </Row>
        </Body>
        <Head id="h">
          <Row id="hr">
            <Cell id="ha">
              <Box id="ha1" w={60} h={12} />
            </Cell>
          </Row>
        </Head>
      </Table>,
      {
        // oracle:begin
        t: [0, 0, 60, 80],
        f: [0, 70, 60, 10],
        fr: [0, 70, 60, 10],
        fa: [0, 70, 60, 10],
        fa1: [0, 70, 30, 10],
        b: [0, 12, 60, 58],
        br: [0, 12, 60, 58],
        ba: [0, 12, 60, 58],
        ba1: [0, 12, 20, 10],
        h: [0, 0, 60, 12],
        hr: [0, 0, 60, 12],
        ha: [0, 0, 60, 12],
        ha1: [0, 0, 60, 12],
        // oracle:end
      }
    )
  })

  /**
   * A cell aligns its content to the baseline, the top, the middle or the bottom of its row. Content without text
   * has its bottom as its baseline.
   *
   * ```html
   * <table id="t">
   *   <tr id="r1">
   *     <td id="a"><div id="a1" style="width:20px;height:30px"></div></td>
   *     <td id="b" style="padding-top:10px"><div id="b1" style="width:20px;height:10px"></div></td>
   *     <td id="c" style="vertical-align:top"><div id="c1" style="width:20px;height:10px"></div></td>
   *     <td id="d" style="vertical-align:middle"><div id="d1" style="width:20px;height:10px"></div></td>
   *     <td id="e" style="vertical-align:bottom"><div id="e1" style="width:20px;height:10px"></div></td>
   *     <td id="f" style="vertical-align:4px"><div id="f1" style="width:20px;height:10px"></div></td>
   *   </tr>
   * </table>
   * ```
   */
  it("aligns a cell's content by verticalAlign", () => {
    expectBoxes(
      <Table id="t">
        <Row id="r1">
          <Cell id="a">
            <Box id="a1" w={20} h={30} />
          </Cell>
          <Cell id="b" style={{ paddingTop: 10 }}>
            <Box id="b1" w={20} h={10} />
          </Cell>
          <Cell id="c" style={{ verticalAlign: "top" }}>
            <Box id="c1" w={20} h={10} />
          </Cell>
          <Cell id="d" style={{ verticalAlign: "middle" }}>
            <Box id="d1" w={20} h={10} />
          </Cell>
          <Cell id="e" style={{ verticalAlign: "bottom" }}>
            <Box id="e1" w={20} h={10} />
          </Cell>
          <Cell id="f" style={{ verticalAlign: 4 }}>
            <Box id="f1" w={20} h={10} />
          </Cell>
        </Row>
      </Table>,
      {
        // oracle:begin
        t: [0, 0, 120, 30],
        r1: [0, 0, 120, 30],
        a: [0, 0, 20, 30],
        a1: [0, 0, 20, 30],
        b: [20, 0, 20, 30],
        b1: [20, 20, 20, 10],
        c: [40, 0, 20, 30],
        c1: [40, 0, 20, 10],
        d: [60, 0, 20, 30],
        d1: [60, 10, 20, 10],
        e: [80, 0, 20, 30],
        e1: [80, 20, 20, 10],
        f: [100, 0, 20, 30],
        f1: [100, 20, 20, 10],
        // oracle:end
      }
    )
  })

  /**
   * Children of a table or a row that are not table parts share one anonymous cell, which stacks them as blocks.
   * White space between parts makes none.
   *
   * ```html
   * <div id="t" style="display:table">
   *   <div id="x" style="width:30px;height:10px"></div>
   *   <div id="y" style="width:50px;height:20px"></div>
   *   <div id="r" style="display:table-row">
   *     <div id="a" style="display:table-cell"><div id="a1" style="width:20px;height:10px"></div></div>
   *     <div id="p" style="width:40px;height:5px"></div>
   *     <div id="q" style="width:10px;height:7px"></div>
   *   </div>
   * </div>
   * ```
   */
  it("wraps the children of a table and a row that are not parts in one anonymous cell", () => {
    expectBoxes(
      <Table id="t">
        <Box id="x" w={30} h={10} />
        <Box id="y" w={50} h={20} />
        {" "}
        <Row id="r">
          {" "}
          <Cell id="a">
            <Box id="a1" w={20} h={10} />
          </Cell>
          {"\n  "}
          <Box id="p" w={40} h={5} />
          <Box id="q" w={10} h={7} />
        </Row>
      </Table>,
      {
        // oracle:begin
        t: [0, 0, 90, 42],
        x: [0, 0, 30, 10],
        y: [0, 10, 50, 20],
        r: [0, 30, 90, 12],
        a: [0, 30, 50, 12],
        a1: [0, 32, 20, 10],
        p: [50, 30, 40, 5],
        q: [50, 35, 10, 7],
        // oracle:end
      }
    )
  })

  /**
   * Text next to a block in a table is in the same anonymous cell. Lines have a fixed height so no glyph metric
   * decides the numbers.
   *
   * ```html
   * <div id="t" style="display:table;line-height:20px">
   *   <div id="w" style="width:60px">ab</div>
   *   <div id="z" style="width:30px;height:10px"></div>
   * </div>
   * ```
   */
  it("puts text and blocks of a table in the same anonymous cell", () => {
    expectBoxes(
      <Table id="t" style={{ lineHeight: "20px" }}>
        <div testId="w" style={{ width: 60 }}>
          ab
        </div>
        <Box id="z" w={30} h={10} />
      </Table>,
      {
        // oracle:begin
        t: [0, 0, 60, 30],
        w: [0, 0, 60, 20],
        z: [0, 20, 30, 10],
        // oracle:end
      }
    )
  })

  /**
   * A table is a block-level box in flex too: as wide as its columns, and not narrower than they need.
   *
   * ```html
   * <div id="f" style="display:flex;width:130px">
   *   <div id="f1" style="width:50px;height:10px;flex-shrink:0"></div>
   *   <table id="t">
   *     <tr id="r1">
   *       <td id="a"><div id="a1" style="width:30px;height:10px"></div></td>
   *       <td id="b"><div id="b1" style="width:40px;height:10px"></div></td>
   *     </tr>
   *   </table>
   *   <div id="f2" style="width:40px;height:10px;flex-shrink:0"></div>
   * </div>
   * ```
   */
  it("keeps the content width of a table that is a flex item", () => {
    expectBoxes(
      <div testId="f" style={{ display: "flex", width: 130 }}>
        <Box id="f1" w={50} h={10} style={{ flexShrink: 0 }} />
        <Table id="t">
          <Row id="r1">
            <Cell id="a">
              <Box id="a1" w={30} h={10} />
            </Cell>
            <Cell id="b">
              <Box id="b1" w={40} h={10} />
            </Cell>
          </Row>
        </Table>
        <Box id="f2" w={40} h={10} style={{ flexShrink: 0 }} />
      </div>,
      {
        // oracle:begin
        f: [0, 0, 130, 10],
        f1: [0, 0, 50, 10],
        t: [50, 0, 70, 10],
        r1: [50, 0, 70, 10],
        a: [50, 0, 30, 10],
        a1: [50, 0, 30, 10],
        b: [80, 0, 40, 10],
        b1: [80, 0, 40, 10],
        f2: [120, 0, 40, 10],
        // oracle:end
      },
      130
    )
  })

  /**
   * Every `display` value is a part of a table when it is a `div`: groups, captions, columns, `span` on a column.
   *
   * ```html
   * <div id="t" style="display:table;width:200px">
   *   <div id="cap" style="display:table-caption"><div id="cb" style="width:20px;height:10px"></div></div>
   *   <div style="display:table-column-group">
   *     <div style="display:table-column;width:60px"></div>
   *     <div style="display:table-column"></div>
   *   </div>
   *   <div id="f" style="display:table-footer-group">
   *     <div id="fr" style="display:table-row">
   *       <div id="fa" style="display:table-cell"><div id="fa1" style="width:30px;height:10px"></div></div>
   *       <div id="fb" style="display:table-cell"><div id="fb1" style="width:30px;height:10px"></div></div>
   *     </div>
   *   </div>
   *   <div id="h" style="display:table-header-group">
   *     <div id="hr" style="display:table-row">
   *       <div id="ha" style="display:table-cell"><div id="ha1" style="width:90px;height:12px"></div></div>
   *       <div id="hb" style="display:table-cell"><div id="hb1" style="width:10px;height:12px"></div></div>
   *     </div>
   *   </div>
   *   <div id="b" style="display:table-row-group">
   *     <div id="br" style="display:table-row">
   *       <div id="ba" style="display:table-cell"><div id="ba1" style="width:20px;height:14px"></div></div>
   *       <div id="bb" style="display:table-cell"><div id="bb1" style="width:20px;height:14px"></div></div>
   *     </div>
   *   </div>
   * </div>
   * ```
   */
  it("reads every display value of a table part", () => {
    expectBoxes(
      <Table id="t" style={{ width: 200 }}>
        <Caption id="cap">
          <Box id="cb" w={20} h={10} />
        </Caption>
        <ColGroup id="cg">
          <Col id="c1" style={{ width: 60 }} />
          <Col id="c2" />
        </ColGroup>
        <Foot id="f">
          <Row id="fr">
            <Cell id="fa">
              <Box id="fa1" w={30} h={10} />
            </Cell>
            <Cell id="fb">
              <Box id="fb1" w={30} h={10} />
            </Cell>
          </Row>
        </Foot>
        <Head id="h">
          <Row id="hr">
            <Cell id="ha">
              <Box id="ha1" w={90} h={12} />
            </Cell>
            <Cell id="hb">
              <Box id="hb1" w={10} h={12} />
            </Cell>
          </Row>
        </Head>
        <Body id="b">
          <Row id="br">
            <Cell id="ba">
              <Box id="ba1" w={20} h={14} />
            </Cell>
            <Cell id="bb">
              <Box id="bb1" w={20} h={14} />
            </Cell>
          </Row>
        </Body>
      </Table>,
      {
        // oracle:begin
        t: [0, 0, 200, 46],
        cap: [0, 0, 200, 10],
        cb: [0, 0, 20, 10],
        f: [0, 36, 200, 10],
        fr: [0, 36, 200, 10],
        fa: [0, 36, 90, 10],
        fa1: [0, 36, 30, 10],
        fb: [90, 36, 110, 10],
        fb1: [90, 36, 30, 10],
        h: [0, 10, 200, 12],
        hr: [0, 10, 200, 12],
        ha: [0, 10, 90, 12],
        ha1: [0, 10, 90, 12],
        hb: [90, 10, 110, 12],
        hb1: [90, 10, 10, 12],
        b: [0, 22, 200, 14],
        br: [0, 22, 200, 14],
        ba: [0, 22, 90, 14],
        ba1: [0, 22, 20, 14],
        bb: [90, 22, 110, 14],
        bb1: [90, 22, 20, 14],
        // oracle:end
      }
    )
  })

  /**
   * `span` makes one column stand for several, and columns no cell starts in take no width.
   *
   * ```html
   * <table id="t" style="width:300px">
   *   <colgroup><col span="2" style="width:40px"><col></colgroup>
   *   <tr id="r1">
   *     <td id="a"><div id="a1" style="width:10px;height:10px"></div></td>
   *     <td id="b"><div id="b1" style="width:10px;height:10px"></div></td>
   *     <td id="c"><div id="c1" style="width:10px;height:10px"></div></td>
   *     <td id="d"><div id="d1" style="width:10px;height:10px"></div></td>
   *   </tr>
   * </table>
   * ```
   */
  it("makes a column stand for span columns", () => {
    expectBoxes(
      <Table id="t" style={{ width: 300 }}>
        <ColGroup id="cg">
          <Col id="cs" span={2} style={{ width: 40 }} />
          <Col id="cr" />
        </ColGroup>
        <Row id="r1">
          <Cell id="a">
            <Box id="a1" w={10} h={10} />
          </Cell>
          <Cell id="b">
            <Box id="b1" w={10} h={10} />
          </Cell>
          <Cell id="c">
            <Box id="c1" w={10} h={10} />
          </Cell>
          <Cell id="d">
            <Box id="d1" w={10} h={10} />
          </Cell>
        </Row>
      </Table>,
      {
        // oracle:begin
        t: [0, 0, 300, 10],
        r1: [0, 0, 300, 10],
        a: [0, 0, 40, 10],
        a1: [0, 0, 10, 10],
        b: [40, 0, 40, 10],
        b1: [40, 0, 10, 10],
        c: [80, 0, 110, 10],
        c1: [80, 0, 10, 10],
        d: [190, 0, 110, 10],
        d1: [190, 0, 10, 10],
        // oracle:end
      }
    )
  })

  /**
   * A width below what the columns need is raised to it.
   *
   * ```html
   * <table id="t" style="width:50px">
   *   <tr id="r1">
   *     <td id="a"><div id="a1" style="width:30px;height:10px"></div></td>
   *     <td id="b"><div id="b1" style="width:40px;height:10px"></div></td>
   *   </tr>
   * </table>
   * ```
   */
  it("raises a table's width to the least its columns need", () => {
    expectBoxes(
      <Table id="t" style={{ width: 50 }}>
        <Row id="r1">
          <Cell id="a">
            <Box id="a1" w={30} h={10} />
          </Cell>
          <Cell id="b">
            <Box id="b1" w={40} h={10} />
          </Cell>
        </Row>
      </Table>,
      {
        // oracle:begin
        t: [0, 0, 70, 10],
        r1: [0, 0, 70, 10],
        a: [0, 0, 30, 10],
        a1: [0, 0, 30, 10],
        b: [30, 0, 40, 10],
        b1: [30, 0, 40, 10],
        // oracle:end
      }
    )
  })
})

const WHITE = [255, 255, 255, 255]
const RED = [255, 0, 0, 255]
const BLUE = [0, 0, 255, 255]
const GREEN = [0, 255, 0, 255]

/** The pixels of the page `ui` paints in a 300px white container. */
function paint(name: string, ui: React.ReactElement, run?: (renderer: TestRoot["renderer"]) => void) {
  const { render, renderer, unmount } = createTestRoot()
  try {
    render(<div style={{ width: 300, backgroundColor: "#ffffff" }}>{ui}</div>)
    run?.(renderer)
    const file = `${SHOTS_DIR}/tables-${name}.png`
    fs.rmSync(file, { force: true })
    renderer.captureScreenshot(file)
    return readLogicalPixels(file, renderer)
  } finally {
    unmount()
  }
}

describeNative("tables: paint", () => {
  /**
   * The wrapper box of a table holds its captions; its background, border and outline surround the grid. The
   * table's `getBoundingClientRect` is the wrapper's, and `border-width` and `padding` wrap the grid alone.
   *
   * ```html
   * <table id="t" style="border-width:4px;border-style:solid;border-color:blue;background:red;outline:2px solid lime">
   *   <caption id="cap"><div id="c" style="width:80px;height:20px"></div></caption>
   *   <tr id="r"><td id="a"><div id="a1" style="width:50px;height:30px"></div></td></tr>
   * </table>
   * <table id="u" style="caption-side:bottom;margin-top:100px;border-width:4px;border-style:solid;border-color:blue;background:red">
   *   <caption id="ucap"><div id="uc" style="width:80px;height:20px"></div></caption>
   *   <tr id="ur"><td id="ua"><div id="ua1" style="width:50px;height:30px"></div></td></tr>
   * </table>
   * ```
   */
  it("paints a table's background, border and outline around its grid, not its captions", () => {
    const ui = (
      <>
        <Table
          id="t"
          style={{
            borderWidth: 4,
            borderStyle: "solid",
            borderColor: "#0000ff",
            backgroundColor: "#ff0000",
            outlineWidth: 2,
            outlineStyle: "solid",
            outlineColor: "#00ff00",
          }}
        >
          <Caption id="cap">
            <Box id="c" w={80} h={20} />
          </Caption>
          <Row id="r">
            <Cell id="a">
              <Box id="a1" w={50} h={30} />
            </Cell>
          </Row>
        </Table>
        <Table
          id="u"
          style={{
            captionSide: "bottom",
            marginTop: 100,
            borderWidth: 4,
            borderStyle: "solid",
            borderColor: "#0000ff",
            backgroundColor: "#ff0000",
          }}
        >
          <Caption id="ucap">
            <Box id="uc" w={80} h={20} />
          </Caption>
          <Row id="ur">
            <Cell id="ua">
              <Box id="ua1" w={50} h={30} />
            </Cell>
          </Row>
        </Table>
      </>
    )
    expectBoxes(
      ui,
      {
        // oracle:begin
        t: [0, 0, 80, 58],
        cap: [0, 0, 80, 20],
        c: [0, 0, 80, 20],
        r: [4, 24, 72, 30],
        a: [4, 24, 72, 30],
        a1: [4, 24, 50, 30],
        u: [0, 158, 80, 58],
        ucap: [0, 196, 80, 20],
        uc: [0, 196, 80, 20],
        ur: [4, 162, 72, 30],
        ua: [4, 162, 72, 30],
        ua1: [4, 162, 50, 30],
        // oracle:end
      }
    )
    const pixels = paint("grid-paint", ui)
    // Over the caption: the page.
    expect(pixels.at(40, 10)).toEqual(WHITE)
    // The outline sits just outside the grid, over the caption's row, and the border starts at the grid.
    expect(pixels.at(40, 19)).toEqual(GREEN)
    expect(pixels.at(40, 21)).toEqual(BLUE)
    expect(pixels.at(40, 40)).toEqual(RED)
    expect(pixels.at(40, 55)).toEqual(BLUE)
    expect(pixels.at(40, 61)).toEqual(WHITE)
    // The second table, 100px below the first, has its caption below: its grid starts at its own top.
    expect(pixels.at(40, 159)).toEqual(BLUE)
    expect(pixels.at(40, 175)).toEqual(RED)
    expect(pixels.at(40, 193)).toEqual(BLUE)
    expect(pixels.at(40, 205)).toEqual(WHITE)
  })

  /**
   * ```html
   * <table id="t" style="empty-cells:hide;border-spacing:4px;width:100px">
   *   <tr id="r">
   *     <td id="a" style="background:blue;width:40px;height:20px"></td>
   *     <td id="b" style="background:blue;width:40px;height:20px"><div id="b1" style="width:10px;height:10px"></div></td>
   *   </tr>
   * </table>
   * ```
   */
  it("leaves an empty cell unpainted with emptyCells hide, and keeps its size", () => {
    const ui = (hide: boolean) => (
      <Table id="t" style={{ emptyCells: hide ? "hide" : "show", borderSpacing: 4, width: 100 }}>
        <Row id="r">
          <Cell id="a" style={{ backgroundColor: "#0000ff", width: 40, height: 20 }} />
          <Cell id="b" style={{ backgroundColor: "#0000ff", width: 40, height: 20 }}>
            {" "}
            <Box id="b1" w={10} h={10} />
          </Cell>
        </Row>
      </Table>
    )
    const hidden = paint("empty-cells-hide", ui(true))
    expect(hidden.at(10, 10)).toEqual(WHITE)
    expect(hidden.at(60, 10)).toEqual(BLUE)
    const shown = paint("empty-cells-show", ui(false))
    expect(shown.at(10, 10)).toEqual(BLUE)
    expectBoxes(ui(true), {
      // oracle:begin
      t: [0, 0, 100, 28],
      r: [4, 4, 92, 20],
      a: [4, 4, 44, 20],
      b: [52, 4, 44, 20],
      b1: [52, 4, 10, 10],
      // oracle:end
    })
  })

  it("clips a cell's content with overflow hidden", () => {
    const ui = (overflow: "hidden" | "visible") => (
      <Table id="t" style={{ width: 100, tableLayout: "fixed" }}>
        <Row id="r">
          <Cell id="a" style={{ width: 50, overflow }}>
            <Box id="a1" w={90} h={10} style={{ backgroundColor: "#ff0000" }} />
          </Cell>
          <Cell id="b" />
        </Row>
      </Table>
    )
    expect(paint("overflow-hidden", ui("hidden")).at(70, 5)).toEqual(WHITE)
    expect(paint("overflow-visible", ui("visible")).at(70, 5)).toEqual(RED)
  })
})

describeNative("tables: parts are elements", () => {
  it("delivers events to a cell, its row and its table, and an anonymous cell takes no part in the path", () => {
    const log: string[] = []
    const record = (name: string) => () => log.push(name)
    const { render, renderer, unmount } = createTestRoot()
    try {
      render(
        <div style={{ width: 300 }}>
          <Table id="t" style={{ borderSpacing: 2 }} onClick={record("table")}>
            <Row id="r" onClick={record("row")}>
              <Cell id="a" onClick={record("cell")}>
                <Box id="a1" w={40} h={20} />
              </Cell>
            </Row>
            <Box id="loose" w={40} h={20} />
          </Table>
        </div>
      )
      const click = (id: string) => {
        const [x, y, width, height] = boxOf(renderer, id)
        renderer.nativeSimulateClick(x + width / 2, y + height / 2)
      }
      click("a1")
      expect(log).toEqual(["cell", "row", "table"])
      log.length = 0
      click("loose")
      expect(log).toEqual(["table"])
    } finally {
      unmount()
    }
  })

  it("restyles a row and a table while the pointer is over one of their cells", () => {
    const ui = (
      <Table id="t" style={{ hover: { backgroundColor: "#0000ff" } }}>
        <Row id="r1" style={{ hover: { backgroundColor: "#ff0000" } }}>
          <Cell id="a">
            <Box id="a1" w={40} h={20} />
          </Cell>
        </Row>
        <Row id="r2">
          <Cell id="b">
            <Box id="b1" w={40} h={20} />
          </Cell>
        </Row>
      </Table>
    )
    expect(paint("hover-off", ui).at(20, 10)).toEqual(WHITE)
    expect(paint("hover-first", ui, (renderer) => renderer.nativeSimulateMouseMove(20, 10)).at(20, 10)).toEqual(RED)
    // The second row is hovered, and the table behind it, which is a row's ancestor too.
    const second = paint("hover-second", ui, (renderer) => renderer.nativeSimulateMouseMove(20, 30))
    expect(second.at(20, 10)).toEqual(BLUE)
    expect(second.at(20, 30)).toEqual(BLUE)
  })

  it("tabs through cells in document order, not in the order the groups are laid out", () => {
    const { render, renderer, unmount } = createTestRoot({ width: 300, height: 200 })
    try {
      render(
        <Table id="t">
          <Foot id="f">
            <Row id="fr">
              <Cell id="fa" tabIndex={0}>
                <Box id="fa1" w={30} h={10} />
              </Cell>
            </Row>
          </Foot>
          <Body id="b">
            <Row id="br">
              <Cell id="ba" tabIndex={0}>
                <Box id="ba1" w={30} h={10} />
              </Cell>
              <Cell id="bb" tabIndex={0}>
                <Box id="bb1" w={30} h={10} />
              </Cell>
            </Row>
          </Body>
          <Head id="h">
            <Row id="hr">
              <Cell id="ha" tabIndex={0}>
                <Box id="ha1" w={30} h={10} />
              </Cell>
            </Row>
          </Head>
        </Table>
      )
      const focused = () => renderer.getElement(renderer.getFocusedElementId())?.testId ?? null
      const order: (string | null)[] = []
      for (let tab = 0; tab < 4; tab++) {
        renderer.simulateKeystrokes("Tab")
        order.push(focused())
      }
      expect(order).toEqual(["fa", "ba", "bb", "ha"])
      renderer.focusElement(renderer.findByTestId("bb")!.id)
      expect(focused()).toBe("bb")
    } finally {
      unmount()
    }
  })

  it("reports a cell's size to a ResizeObserver when its column grows", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      const sizes: number[] = []
      let target: PublicInstance | null = null
      const table = (width: number) => (
        <Table id="t">
          <Row id="r1">
            <Cell
              id="a"
              ref={(instance: PublicInstance | null) => {
                target = instance
              }}
            >
              <Box id="a1" w={20} h={10} />
            </Cell>
          </Row>
          <Row id="r2">
            <Cell id="b">
              <Box id="b1" w={width} h={10} />
            </Cell>
          </Row>
        </Table>
      )
      render(table(50))
      const observer = new ResizeObserver((entries) => sizes.push(entries[0]!.borderBoxSize[0]!.inlineSize))
      observer.observe(target!)
      renderer.dispatchNativeEvents()
      render(table(90))
      renderer.dispatchNativeEvents()
      expect(sizes).toEqual([50, 90])
    } finally {
      unmount()
    }
  })

  it("orders overlapping cells by z-index like any positioned element", () => {
    const ui = (zIndex?: number) => (
      <Table id="t">
        <Row id="r">
          <Cell id="a" style={{ position: "relative", left: 30, zIndex }}>
            <Box id="a1" w={40} h={20} />
          </Cell>
          <Cell id="b" style={{ position: "relative" }}>
            <Box id="b1" w={40} h={20} />
          </Cell>
        </Row>
      </Table>
    )
    for (const [zIndex, top] of [
      [undefined, "b1"],
      [1, "a1"],
    ] as const) {
      const { render, renderer, unmount } = createTestRoot()
      try {
        render(ui(zIndex))
        // a1 spans x 30-70 once relatively offset, b1 40-80: they overlap at x 50.
        expect(boxOf(renderer, "a1")[0]).toBe(30)
        const hit = renderer.getElementsAt(50, 10)[0]
        expect(hit, `zIndex ${zIndex}`).toBe(renderer.findByTestId(top)!.id)
      } finally {
        unmount()
      }
    }
  })

  it("selects and finds the text of cells, and of an anonymous cell, in document order", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      render(
        <div style={{ width: 400, fontSize: 20 }} highlight={{ query: "a" }}>
          <Table id="t" style={{ lineHeight: 30 }}>
            loose a
            <Row id="r">
              <Cell id="c1">
                <text>alpha</text>
              </Cell>
              <Cell id="c2">
                <text>beta</text>
              </Cell>
            </Row>
          </Table>
        </div>
      )
      const matched = renderer.getPaintedHighlights().map((hit) => hit.text.slice(hit.start, hit.end))
      // "loose a" in the anonymous cell, "alpha" twice and "beta".
      expect(matched).toEqual(["a", "a", "a", "a"])
      const [x1, y1] = boxOf(renderer, "c1")
      const text = renderer.dragSelect(x1 + 1, y1 + 5, 290, y1 + 5)
      expect(text).toContain("alpha")
      expect(text).toContain("beta")
      expect(text!.indexOf("alpha")).toBeLessThan(text!.indexOf("beta"))
    } finally {
      unmount()
    }
  })

  /**
   * Chrome serialises a selection across cells with a tab between the cells of a row and a line break between rows
   * (`getSelection().toString()` of a `<table>` and of `display: table` boxes, and `innerText`), also when the
   * cells are anonymous.
   *
   * ```html
   * <table><tr><td>aa</td><td>bb</td></tr><tr><td>cc</td><td>dd</td></tr></table>
   * ```
   * selects as `"aa\tbb\ncc\tdd"`; a row of two bare cells in a block as `"x\ty"`; `loose` in an anonymous cell
   * before a row as `"loose\naa\tbb"`.
   */
  it("copies the cells of a row with a tab between them and the rows with a line break", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      const copy = (ui: React.ReactElement, from: string, to: string) => {
        render(<div style={{ width: 400, fontSize: 20, lineHeight: 30 }}>{ui}</div>)
        const [x1, y1] = boxOf(renderer, from)
        const [x2, y2, w2, h2] = boxOf(renderer, to)
        return renderer.dragSelect(x1 + 1, y1 + 5, x2 + w2 + 30, y2 + h2 - 5)
      }
      expect(
        copy(
          <Table id="t">
            <Row id="r1">
              <Cell id="a">
                <text>aa</text>
              </Cell>
              <Cell id="b">
                <text>bb</text>
              </Cell>
            </Row>
            <Row id="r2">
              <Cell id="c">
                <text>cc</text>
              </Cell>
              <Cell id="d">
                <text>dd</text>
              </Cell>
            </Row>
          </Table>,
          "a",
          "d"
        )
      ).toBe("aa\tbb\ncc\tdd")
      // Cells that are bare, in the anonymous table of a block.
      expect(
        copy(
          <div>
            <Cell id="a">
              <text>x</text>
            </Cell>
            <Cell id="b">
              <text>y</text>
            </Cell>
          </div>,
          "a",
          "b"
        )
      ).toBe("x\ty")
      // Text in an anonymous cell of the table, then a row.
      expect(
        copy(
          <Table id="t">
            loose
            <Row id="r">
              <Cell id="a">
                <text>aa</text>
              </Cell>
              <Cell id="b">
                <text>bb</text>
              </Cell>
            </Row>
          </Table>,
          "t",
          "b"
        )
      ).toBe("loose\naa\tbb")
    } finally {
      unmount()
    }
  })

  it("transitions the width of a cell, and the table around it follows", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      const table = (width: number) => (
        <Table id="t">
          <Row id="r">
            <Cell id="a" style={{ width, transition: "width 1s linear" }}>
              <Box id="a1" w={10} h={10} />
            </Cell>
          </Row>
        </Table>
      )
      renderer.clockPause()
      render(table(100))
      renderer.dispatchNativeEvents()
      expect(boxOf(renderer, "a")[2]).toBe(100)
      render(table(200))
      renderer.clockFastForward(500)
      renderer.dispatchNativeEvents()
      expect(boxOf(renderer, "a")[2]).toBeCloseTo(150, 0)
      expect(boxOf(renderer, "t")[2]).toBeCloseTo(150, 0)
      renderer.clockFastForward(1000)
      renderer.dispatchNativeEvents()
      expect(boxOf(renderer, "a")[2]).toBe(200)
    } finally {
      unmount()
    }
  })

  it("lays a table out again when colSpan or display change", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      const table = (colSpan: number, display = "table-cell") => (
        <div style={{ width: 300 }}>
          <Table id="t">
            <Row id="r1">
              <Cell id="a" colSpan={colSpan} style={{ display }}>
                <Box id="a1" w={40} h={10} />
              </Cell>
              <Cell id="b">
                <Box id="b1" w={20} h={10} />
              </Cell>
            </Row>
            <Row id="r2">
              <Cell id="c">
                <Box id="c1" w={20} h={10} />
              </Cell>
              <Cell id="d">
                <Box id="d1" w={30} h={10} />
              </Cell>
              <Cell id="e">
                <Box id="e1" w={20} h={10} />
              </Cell>
            </Row>
          </Table>
        </div>
      )
      render(table(1))
      expect([boxOf(renderer, "a")[2], boxOf(renderer, "b")[0]]).toEqual([40, 40])
      render(table(2))
      expect([boxOf(renderer, "a")[2], boxOf(renderer, "b")[0]]).toEqual([50, 50])
      // No longer a cell: the row wraps it in an anonymous one, and a block ignores colSpan.
      render(table(2, "block"))
      expect([boxOf(renderer, "a")[2], boxOf(renderer, "b")[0]]).toEqual([40, 40])
    } finally {
      unmount()
    }
  })

  it("reports a borderSpacing, tableLayout, captionSide, borderCollapse or emptyCells that CSS rejects", () => {
    for (const [style, message] of [
      [{ borderSpacing: "-1px" }, /invalid borderSpacing "-1px"/],
      [{ borderSpacing: "1px 2px 3px" }, /invalid borderSpacing "1px 2px 3px"/],
      [{ borderSpacing: "10%" }, /invalid borderSpacing "10%"/],
      [{ tableLayout: "balanced" }, /invalid tableLayout: "balanced"/],
      [{ captionSide: "left" }, /invalid captionSide: "left"/],
      [{ borderCollapse: "yes" }, /invalid borderCollapse: "yes"/],
      [{ emptyCells: "none" }, /invalid emptyCells: "none"/],
      [{ hover: { borderSpacing: -3 } }, /invalid borderSpacing/],
    ] as const) {
      const errors: Error[] = []
      const { render, unmount } = createTestRoot({ onUncaughtError: (error) => errors.push(error) })
      try {
        render(<Table id="t" style={style as Style} />)
        expect(errors.map((error) => error.message).join("\n"), JSON.stringify(style)).toMatch(message)
      } finally {
        unmount()
      }
    }
  })
})

/** Compares the pixels of a block of the page with a map of it: one letter a pixel, `.` for white. */
function expectMap(
  pixels: ReturnType<typeof paint>,
  rows: string[],
  legend: Record<string, number[]>,
  at: [number, number] = [0, 0]
) {
  const letters = Object.entries({ ".": WHITE, ...legend })
  const got = rows.map((row, y) =>
    [...row]
      .map((_, x) => {
        const pixel = pixels.at(at[0] + x, at[1] + y)
        return letters.find(([, color]) => color.every((value, index) => value === pixel[index]))?.[0] ?? "?"
      })
      .join("")
  )
  expect(got.join("\n")).toEqual(rows.join("\n"))
}

const BLACK = [0, 0, 0, 255]
const DARK_GREEN = [0, 128, 0, 255]
const ORANGE = [255, 165, 0, 255]
const YELLOW = [255, 255, 0, 255]
const PURPLE = [128, 0, 128, 255]
const GRAY = [238, 238, 238, 255]

describeNative("tables: border-collapse", () => {
  /**
   * The borders of the cells and the table, the table's 4px border against the 2, 6, 4 and 2px of the cells: the
   * wider border wins each edge, a cell wins a tie with the table (the green one on the left and below), each side of
   * a border holds half of it, and the borders cover the cells' backgrounds and meet the way Chrome joins them: the
   * 6px blue one runs up to the top of the table and over the black one, the others stop at its edge.
   *
   * ```html
   * <table id="t" style="border-collapse:collapse;border:4px solid #000000;background:#eeeeee">
   *   <tr id="r1">
   *     <td id="a" style="border:2px solid #ff0000;background:#ffff00"><div id="a1" style="width:30px;height:10px"></div></td>
   *     <td id="b" style="border:6px solid #0000ff"><div id="b1" style="width:30px;height:10px"></div></td>
   *   </tr>
   *   <tr id="r2">
   *     <td id="c" style="border:4px solid #008000"><div id="c1" style="width:30px;height:10px"></div></td>
   *     <td id="d" style="border:2px solid #ffa500"><div id="d1" style="width:30px;height:10px"></div></td>
   *   </tr>
   * </table>
   * ```
   * The map is the screenshot of that page, 76 by 36 pixels from the corner of the table, `.` white, `g` the table's
   * background, `K` black, `B` blue, `Y` yellow, `G` green. Red and orange lose every edge.
   */
  it("gives each edge the wider border and paints it centred on the line, over the cells", () => {
    const border = (width: number, color: string): Style => ({ border: `${width}px solid ${color}` })
    const ui = (
      <Table id="t" style={{ borderCollapse: "collapse", backgroundColor: "#eeeeee", ...border(4, "#000000") }}>
        <Row id="r1">
          <Cell id="a" style={{ ...border(2, "#ff0000"), backgroundColor: "#ffff00" }}>
            <Box id="a1" w={30} h={10} />
          </Cell>
          <Cell id="b" style={border(6, "#0000ff")}>
            <Box id="b1" w={30} h={10} />
          </Cell>
        </Row>
        <Row id="r2">
          <Cell id="c" style={border(4, "#008000")}>
            <Box id="c1" w={30} h={10} />
          </Cell>
          <Cell id="d" style={border(2, "#ffa500")}>
            <Box id="d1" w={30} h={10} />
          </Cell>
        </Row>
      </Table>
    )
    expectBoxes(ui, {
      // oracle:begin
      t: [0, 0, 76, 36],
      r1: [2, 3, 71, 16],
      a: [2, 3, 35, 16],
      a1: [4, 6, 30, 10],
      b: [37, 3, 36, 16],
      b1: [40, 6, 30, 10],
      r2: [2, 19, 71, 15],
      c: [2, 19, 35, 15],
      c1: [4, 22, 30, 10],
      d: [37, 19, 36, 15],
      d1: [39, 22, 30, 10],
      // oracle:end
    })
    expectMap(
      paint("collapse-conflicts", ui),
      [
      "ggggggggggggggggggggggggggggggggggBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
      "KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
      "KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
      "KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
      "KKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
      "KKKKYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
      "KKKKYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYBBBBBBggggggggggggggggggggggggggggggBBBBBB",
      "KKKKYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYBBBBBBggggggggggggggggggggggggggggggBBBBBB",
      "KKKKYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYBBBBBBggggggggggggggggggggggggggggggBBBBBB",
      "KKKKYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYBBBBBBggggggggggggggggggggggggggggggBBBBBB",
      "KKKKYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYBBBBBBggggggggggggggggggggggggggggggBBBBBB",
      "KKKKYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYBBBBBBggggggggggggggggggggggggggggggBBBBBB",
      "KKKKYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYBBBBBBggggggggggggggggggggggggggggggBBBBBB",
      "KKKKYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYBBBBBBggggggggggggggggggggggggggggggBBBBBB",
      "KKKKYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYBBBBBBggggggggggggggggggggggggggggggBBBBBB",
      "KKKKYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYBBBBBBggggggggggggggggggggggggggggggBBBBBB",
      "KKKKYYYYYYYYYYYYYYYYYYYYYYYYYYYYYYBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
      "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
      "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
      "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
      "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
      "GGGGggggggggggggggggggggggggggggggBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
      "GGGGgggggggggggggggggggggggggggggggGGGGggggggggggggggggggggggggggggggggKKKKg",
      "GGGGgggggggggggggggggggggggggggggggGGGGggggggggggggggggggggggggggggggggKKKKg",
      "GGGGgggggggggggggggggggggggggggggggGGGGggggggggggggggggggggggggggggggggKKKKg",
      "GGGGgggggggggggggggggggggggggggggggGGGGggggggggggggggggggggggggggggggggKKKKg",
      "GGGGgggggggggggggggggggggggggggggggGGGGggggggggggggggggggggggggggggggggKKKKg",
      "GGGGgggggggggggggggggggggggggggggggGGGGggggggggggggggggggggggggggggggggKKKKg",
      "GGGGgggggggggggggggggggggggggggggggGGGGggggggggggggggggggggggggggggggggKKKKg",
      "GGGGgggggggggggggggggggggggggggggggGGGGggggggggggggggggggggggggggggggggKKKKg",
      "GGGGgggggggggggggggggggggggggggggggGGGGggggggggggggggggggggggggggggggggKKKKg",
      "GGGGgggggggggggggggggggggggggggggggGGGGggggggggggggggggggggggggggggggggKKKKg",
      "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKg",
      "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKg",
      "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKg",
      "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKKg"
      ],
      { K: BLACK, B: BLUE, g: GRAY, Y: YELLOW, G: DARK_GREEN }
    )
  })

  /**
   * A cell spanning two rows has one border along its right side for each row: the 4px red of the cell against the
   * 2px blue of the cell beside the first row, and against the 6px green of the second. Where the second row's
   * green meets its edge with the first, it is the widest there and covers the red.
   *
   * ```html
   * <table id="t" style="border-collapse:collapse">
   *   <tr id="r1">
   *     <td id="s" rowspan="2" style="border:4px solid #ff0000"><div id="s1" style="width:20px;height:10px"></div></td>
   *     <td id="p" style="border:2px solid #0000ff"><div id="p1" style="width:20px;height:10px"></div></td>
   *   </tr>
   *   <tr id="r2">
   *     <td id="q" style="border:6px solid #008000"><div id="q1" style="width:20px;height:10px"></div></td>
   *   </tr>
   * </table>
   * ```
   * `R` red, `B` blue, `G` green, 56 by 36 pixels.
   */
  it("gives a cell that spans rows a border for each of them", () => {
    const ui = (
      <Table id="t" style={{ borderCollapse: "collapse" }}>
        <Row id="r1">
          <Cell id="s" rowSpan={2} style={{ border: "4px solid #ff0000" }}>
            <Box id="s1" w={20} h={10} />
          </Cell>
          <Cell id="p" style={{ border: "2px solid #0000ff" }}>
            <Box id="p1" w={20} h={10} />
          </Cell>
        </Row>
        <Row id="r2">
          <Cell id="q" style={{ border: "6px solid #008000" }}>
            <Box id="q1" w={20} h={10} />
          </Cell>
        </Row>
      </Table>
    )
    expectBoxes(ui, {
      // oracle:begin
      t: [0, 0, 56, 36],
      r1: [2, 2, 51, 15],
      s: [2, 2, 25, 31],
      s1: [4, 4, 20, 10],
      p: [27, 2, 26, 15],
      p1: [29, 4, 20, 10],
      r2: [2, 17, 51, 16],
      q: [27, 17, 26, 16],
      q1: [30, 20, 20, 10],
      // oracle:end
    })
    expectMap(
      paint("collapse-rowspan", ui),
      [
      "RRRRRRRRRRRRRRRRRRRRRRRRRRRRR...........................",
      "RRRRRRRRRRRRRRRRRRRRRRRRRRRRRBBBBBBBBBBBBBBBBBBBBBBBBB..",
      "RRRRRRRRRRRRRRRRRRRRRRRRRRRRRBBBBBBBBBBBBBBBBBBBBBBBBB..",
      "RRRRRRRRRRRRRRRRRRRRRRRRRRRRR.......................BB..",
      "RRRR.....................RRRR.......................BB..",
      "RRRR.....................RRRR.......................BB..",
      "RRRR.....................RRRR.......................BB..",
      "RRRR.....................RRRR.......................BB..",
      "RRRR.....................RRRR.......................BB..",
      "RRRR.....................RRRR.......................BB..",
      "RRRR.....................RRRR.......................BB..",
      "RRRR.....................RRRR.......................BB..",
      "RRRR.....................RRRR.......................BB..",
      "RRRR.....................RRRR.......................BB..",
      "RRRR....................GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG",
      "RRRR....................GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG",
      "RRRR....................GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG",
      "RRRR....................GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG",
      "RRRR....................GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG",
      "RRRR....................GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG",
      "RRRR....................GGGGGG....................GGGGGG",
      "RRRR....................GGGGGG....................GGGGGG",
      "RRRR....................GGGGGG....................GGGGGG",
      "RRRR....................GGGGGG....................GGGGGG",
      "RRRR....................GGGGGG....................GGGGGG",
      "RRRR....................GGGGGG....................GGGGGG",
      "RRRR....................GGGGGG....................GGGGGG",
      "RRRR....................GGGGGG....................GGGGGG",
      "RRRR....................GGGGGG....................GGGGGG",
      "RRRR....................GGGGGG....................GGGGGG",
      "RRRR....................GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG",
      "RRRRRRRRRRRRRRRRRRRRRRRRGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG",
      "RRRRRRRRRRRRRRRRRRRRRRRRGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG",
      "RRRRRRRRRRRRRRRRRRRRRRRRGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG",
      "RRRRRRRRRRRRRRRRRRRRRRRRGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG",
      "........................GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG"
      ],
      { R: RED, B: BLUE, G: DARK_GREEN }
    )
  })

  /**
   * `hidden` removes the border of an edge whatever the others say, and `none` loses to any border: the middle cell's
   * hidden borders leave the edges next to it bare (its top and bottom too), and the 2px orange border of the cell
   * beside the 8px `none` ones is the one there.
   *
   * ```html
   * <table id="t" style="border-collapse:collapse">
   *   <tr id="r1">
   *     <td id="a" style="border:4px solid #ff0000"><div id="a1" style="width:12px;height:10px"></div></td>
   *     <td id="b" style="border:6px hidden #00ff00"><div id="b1" style="width:12px;height:10px"></div></td>
   *     <td id="c" style="border:4px solid #0000ff"><div id="c1" style="width:12px;height:10px"></div></td>
   *   </tr>
   *   <tr id="r2">
   *     <td id="d" style="border:2px solid #ffa500"><div id="d1" style="width:12px;height:10px"></div></td>
   *     <td id="e" style="border:8px none #00ff00"><div id="e1" style="width:12px;height:10px"></div></td>
   *     <td id="f"><div id="f1" style="width:12px;height:10px"></div></td>
   *   </tr>
   * </table>
   * ```
   * `R` red, `B` blue, `O` orange, 45 by 30 pixels.
   */
  it("removes the borders of an edge that is hidden and ignores one that is none", () => {
    const ui = (
      <Table id="t" style={{ borderCollapse: "collapse" }}>
        <Row id="r1">
          <Cell id="a" style={{ border: "4px solid #ff0000" }}>
            <Box id="a1" w={12} h={10} />
          </Cell>
          <Cell id="b" style={{ border: "6px hidden #00ff00" }}>
            <Box id="b1" w={12} h={10} />
          </Cell>
          <Cell id="c" style={{ border: "4px solid #0000ff" }}>
            <Box id="c1" w={12} h={10} />
          </Cell>
        </Row>
        <Row id="r2">
          <Cell id="d" style={{ border: "2px solid #ffa500" }}>
            <Box id="d1" w={12} h={10} />
          </Cell>
          <Cell id="e" style={{ border: "8px none #00ff00" }}>
            <Box id="e1" w={12} h={10} />
          </Cell>
          <Cell id="f">
            <Box id="f1" w={12} h={10} />
          </Cell>
        </Row>
      </Table>
    )
    expectBoxes(ui, {
      // oracle:begin
      t: [0, 0, 45, 30],
      r1: [2, 2, 41, 14],
      a: [2, 2, 14, 14],
      a1: [4, 4, 12, 10],
      b: [16, 2, 13, 14],
      b1: [16, 4, 12, 10],
      c: [29, 2, 14, 14],
      c1: [29, 4, 12, 10],
      r2: [2, 16, 41, 13],
      d: [2, 16, 14, 13],
      d1: [3, 18, 12, 10],
      e: [16, 16, 13, 13],
      e1: [17, 18, 12, 10],
      f: [29, 16, 14, 13],
      f1: [29, 18, 12, 10],
      // oracle:end
    })
    expectMap(
      paint("collapse-hidden", ui),
      [
      "RRRRRRRRRRRRRRRR.............BBBBBBBBBBBBBBBB",
      "RRRRRRRRRRRRRRRR.............BBBBBBBBBBBBBBBB",
      "RRRRRRRRRRRRRRRR.............BBBBBBBBBBBBBBBB",
      "RRRRRRRRRRRRRRRR.............BBBBBBBBBBBBBBBB",
      "RRRR.....................................BBBB",
      "RRRR.....................................BBBB",
      "RRRR.....................................BBBB",
      "RRRR.....................................BBBB",
      "RRRR.....................................BBBB",
      "RRRR.....................................BBBB",
      "RRRR.....................................BBBB",
      "RRRR.....................................BBBB",
      "RRRR.....................................BBBB",
      "RRRR.....................................BBBB",
      "RRRRRRRRRRRRRRRRR............BBBBBBBBBBBBBBBB",
      "RRRRRRRRRRRRRRRRR............BBBBBBBBBBBBBBBB",
      "RRRRRRRRRRRRRRRRR............BBBBBBBBBBBBBBBB",
      "RRRRRRRRRRRRRRRRR............BBBBBBBBBBBBBBBB",
      ".OO............OO............................",
      ".OO............OO............................",
      ".OO............OO............................",
      ".OO............OO............................",
      ".OO............OO............................",
      ".OO............OO............................",
      ".OO............OO............................",
      ".OO............OO............................",
      ".OO............OO............................",
      ".OO............OO............................",
      ".OOOOOOOOOOOOOOOO............................",
      ".OOOOOOOOOOOOOOOO............................"
      ],
      { R: RED, B: BLUE, O: ORANGE }
    )
  })

  /**
   * Row groups, rows, columns and column groups have borders too, and take part in the conflicts: the 12px purple top
   * border of the row group beats the table's edge, the 8px red bottom border of the first row beats the cell's
   * black 2px one, the 10px blue one of the second column beats the 6px green of the column group (its left, top and
   * bottom edges are the group's too) where they meet.
   *
   * ```html
   * <table id="t" style="border-collapse:collapse">
   *   <colgroup id="cg" style="border:6px solid #008000"><col id="c1"><col id="c2" style="border:10px solid #0000ff"></colgroup>
   *   <tbody id="g1" style="border-top:12px solid #800080">
   *     <tr id="r1" style="border-bottom:8px solid #ff0000">
   *       <td id="a" style="border:2px solid #000000"><div style="width:20px;height:10px"></div></td>
   *       <td id="b"><div style="width:20px;height:10px"></div></td>
   *     </tr>
   *     <tr id="r2"><td id="c"><div style="width:20px;height:10px"></div></td><td id="d"><div style="width:20px;height:10px"></div></td></tr>
   *   </tbody>
   * </table>
   * ```
   * `G` green, `B` blue, `P` purple, `R` red, 66 by 50 pixels. (The `<col>`s have no box in Chrome that is theirs.)
   */
  it("takes the borders of rows, row groups, columns and column groups into the conflicts", () => {
    const ui = (
      <Table id="t" style={{ borderCollapse: "collapse" }}>
        <ColGroup id="cg" style={{ border: "6px solid #008000" }}>
          <Col id="c1" />
          <Col id="c2" style={{ border: "10px solid #0000ff" }} />
        </ColGroup>
        <Body id="g1" style={{ borderTop: "12px solid #800080" }}>
          <Row id="r1" style={{ borderBottom: "8px solid #ff0000" }}>
            <Cell id="a" style={{ border: "2px solid #000000" }}>
              <Box id="a1" w={20} h={10} />
            </Cell>
            <Cell id="b">
              <Box id="b1" w={20} h={10} />
            </Cell>
          </Row>
          <Row id="r2">
            <Cell id="c">
              <Box id="c3" w={20} h={10} />
            </Cell>
            <Cell id="d">
              <Box id="d1" w={20} h={10} />
            </Cell>
          </Row>
        </Body>
      </Table>
    )
    expectBoxes(ui, {
      // oracle:begin
      t: [0, 0, 66, 50],
      g1: [3, 6, 58, 39],
      r1: [3, 6, 58, 20],
      a: [3, 6, 28, 20],
      b: [31, 6, 30, 20],
      r2: [3, 26, 58, 19],
      c: [3, 26, 28, 19],
      d: [31, 26, 30, 19],
      // oracle:end
    })
    expectMap(
      paint("collapse-parts", ui),
      [
      "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP",
      "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP",
      "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP",
      "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP",
      "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP",
      "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP",
      "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP",
      "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP",
      "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP",
      "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP",
      "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP",
      "PPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPPP",
      "GGGGGG....................BBBBBBBBBB....................BBBBBBBBBB",
      "GGGGGG....................BBBBBBBBBB....................BBBBBBBBBB",
      "GGGGGG....................BBBBBBBBBB....................BBBBBBBBBB",
      "GGGGGG....................BBBBBBBBBB....................BBBBBBBBBB",
      "GGGGGG....................BBBBBBBBBB....................BBBBBBBBBB",
      "GGGGGG....................BBBBBBBBBB....................BBBBBBBBBB",
      "GGGGGG....................BBBBBBBBBB....................BBBBBBBBBB",
      "GGGGGG....................BBBBBBBBBB....................BBBBBBBBBB",
      "GGGGGG....................BBBBBBBBBB....................BBBBBBBBBB",
      "GGGGGG....................BBBBBBBBBB....................BBBBBBBBBB",
      "RRRRRRRRRRRRRRRRRRRRRRRRRRBBBBBBBBBBRRRRRRRRRRRRRRRRRRRRBBBBBBBBBB",
      "RRRRRRRRRRRRRRRRRRRRRRRRRRBBBBBBBBBBRRRRRRRRRRRRRRRRRRRRBBBBBBBBBB",
      "RRRRRRRRRRRRRRRRRRRRRRRRRRBBBBBBBBBBRRRRRRRRRRRRRRRRRRRRBBBBBBBBBB",
      "RRRRRRRRRRRRRRRRRRRRRRRRRRBBBBBBBBBBRRRRRRRRRRRRRRRRRRRRBBBBBBBBBB",
      "RRRRRRRRRRRRRRRRRRRRRRRRRRBBBBBBBBBBRRRRRRRRRRRRRRRRRRRRBBBBBBBBBB",
      "RRRRRRRRRRRRRRRRRRRRRRRRRRBBBBBBBBBBRRRRRRRRRRRRRRRRRRRRBBBBBBBBBB",
      "RRRRRRRRRRRRRRRRRRRRRRRRRRBBBBBBBBBBRRRRRRRRRRRRRRRRRRRRBBBBBBBBBB",
      "RRRRRRRRRRRRRRRRRRRRRRRRRRBBBBBBBBBBRRRRRRRRRRRRRRRRRRRRBBBBBBBBBB",
      "GGGGGG....................BBBBBBBBBB....................BBBBBBBBBB",
      "GGGGGG....................BBBBBBBBBB....................BBBBBBBBBB",
      "GGGGGG....................BBBBBBBBBB....................BBBBBBBBBB",
      "GGGGGG....................BBBBBBBBBB....................BBBBBBBBBB",
      "GGGGGG....................BBBBBBBBBB....................BBBBBBBBBB",
      "GGGGGG....................BBBBBBBBBB....................BBBBBBBBBB",
      "GGGGGG....................BBBBBBBBBB....................BBBBBBBBBB",
      "GGGGGG....................BBBBBBBBBB....................BBBBBBBBBB",
      "GGGGGG....................BBBBBBBBBB....................BBBBBBBBBB",
      "GGGGGG....................BBBBBBBBBB....................BBBBBBBBBB",
      "GGGGGG....................BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
      "GGGGGG....................BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
      "GGGGGGGGGGGGGGGGGGGGGGGGGGBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
      "GGGGGGGGGGGGGGGGGGGGGGGGGGBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
      "GGGGGGGGGGGGGGGGGGGGGGGGGGBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
      "GGGGGGGGGGGGGGGGGGGGGGGGGGBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
      "GGGGGGGGGGGGGGGGGGGGGGGGGGBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
      "GGGGGGGGGGGGGGGGGGGGGGGGGGBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
      "..........................BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
      "..........................BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB"
      ],
      { G: DARK_GREEN, B: BLUE, P: PURPLE, R: RED, K: BLACK }
    )
  })

  /**
   * The table lays out as if it had no spacing, no padding and no radius, and `empty-cells` does not hide an
   * empty cell's borders.
   *
   * ```html
   * <table id="t" style="border-collapse:collapse;border-spacing:10px;empty-cells:hide;padding:10px;border:2px solid #000000;border-radius:8px;background:#eeeeee">
   *   <tr id="r1">
   *     <td id="a" style="border:2px solid #ff0000;background:#ffff00"></td>
   *     <td id="b" style="border:2px solid #0000ff;padding:3px"><div id="b1" style="width:20px;height:10px"></div></td>
   *   </tr>
   * </table>
   * ```
   */
  it("ignores border-spacing, the table's padding and radius and empty-cells", () => {
    const ui = (
      <Table
        id="t"
        style={{
          borderCollapse: "collapse",
          borderSpacing: 10,
          emptyCells: "hide",
          padding: 10,
          border: "2px solid #000000",
          borderRadius: 8,
          backgroundColor: "#eeeeee",
        }}
      >
        <Row id="r1">
          <Cell id="a" style={{ border: "2px solid #ff0000", backgroundColor: "#ffff00" }} />
          <Cell id="b" style={{ border: "2px solid #0000ff", padding: 3 }}>
            <Box id="b1" w={20} h={10} />
          </Cell>
        </Row>
      </Table>
    )
    expectBoxes(ui, {
      // oracle:begin
      t: [0, 0, 32, 20],
      r1: [1, 1, 30, 18],
      a: [1, 1, 2, 18],
      b: [3, 1, 28, 18],
      b1: [7, 5, 20, 10],
      // oracle:end
    })
    const pixels = paint("collapse-ignored", ui)
    // The empty cell has its 2px red border on the left though `empty-cells` is `hide`, where it wins the tie with the
    // table's black one; the table's background is inside the borders, and the blue is the other cell's.
    // Pixels of the Chrome screenshot of the page.
    for (const [x, y, color] of [
      [0, 0, RED],
      [3, 10, RED],
      [4, 10, GRAY],
      [29, 10, GRAY],
      [30, 10, BLUE],
      [31, 19, BLUE],
      [10, 0, BLUE],
      [10, 1, BLUE],
      [10, 2, GRAY],
      [10, 17, GRAY],
      [10, 18, BLUE],
    ] as const) {
      expect(pixels.at(x, y), `(${x}, ${y})`).toEqual(color)
    }
  })

  /**
   * A `double` border of 6px is two lines of 2px with a gap of 2px, a dashed one has gaps along it and a
   * dotted one is not a line. They are centred on the grid line like the solid ones; the thickness is the
   * border's.
   */
  it("paints the style of a border: double, dashed and dotted", () => {
    const ui = (style: string) => (
      <Table id="t" style={{ borderCollapse: "collapse", border: `6px ${style} #0000ff` }}>
        <Row id="r">
          <Cell id="a">
            <Box id="a1" w={60} h={20} />
          </Cell>
        </Row>
      </Table>
    )
    const double = paint("collapse-double", ui("double"))
    const column = (pixels: typeof double, x: number) =>
      [0, 1, 2, 3, 4, 5, 6, 7].map((y) => (pixels.at(x, y).every((value, index) => value === BLUE[index]) ? "B" : "."))
    expect(column(double, 30).join("")).toBe("BB..BB..")
    const dashed = paint("collapse-dashed", ui("dashed"))
    // Along the top border, clear of the corners.
    const dashes = Array.from({ length: 50 }, (_, index) => column(dashed, index + 8)[1])
    expect(dashes).toContain("B")
    expect(dashes).toContain(".")
    // Across the dashes the line is 6 thick, centred on the line at y = 3: rows 0 to 5.
    const solidRows = (pixels: typeof dashed) => {
      const at = Array.from({ length: 50 }, (_, index) => index + 8).find((x) => column(pixels, x)[2] === "B")!
      return column(pixels, at).join("")
    }
    expect(solidRows(dashed)).toBe("BBBBBB..")
    const dotted = paint("collapse-dotted", ui("dotted"))
    const dots = Array.from({ length: 50 }, (_, index) => column(dotted, index + 8)[2])
    expect(dots).toContain("B")
    expect(dots).toContain(".")
  })

  /**
   * A border of an odd width leaves half a pixel more on neither side: the halves are exact, so the grid starts
   * 1.5px in, and a cell is 33 wide around its 30. All four are 3px, so on each edge the cell on the left or
   * above beats the other, and a cell beats the table.
   *
   * ```html
   * <table id="t" style="border-collapse:collapse;border:3px solid #000000">
   *   <tr id="r1">
   *     <td id="a" style="border:3px solid #ff0000"><div id="a1" style="width:30px;height:10px"></div></td>
   *     <td id="b" style="border:3px solid #0000ff"><div id="b1" style="width:30px;height:10px"></div></td>
   *   </tr>
   *   <tr id="r2">
   *     <td id="c" style="border:3px solid #008000"><div id="c1" style="width:30px;height:10px"></div></td>
   *     <td id="d" style="border:3px solid #ffa500"><div id="d1" style="width:30px;height:10px"></div></td>
   *   </tr>
   * </table>
   * ```
   * `R` red, `B` blue, `G` green, `O` orange, 69 by 29 pixels.
   */
  it("splits a border of an odd width into exact halves", () => {
    const border = (color: string): Style => ({ border: `3px solid ${color}` })
    const ui = (
      <Table id="t" style={{ borderCollapse: "collapse", ...border("#000000") }}>
        <Row id="r1">
          <Cell id="a" style={border("#ff0000")}>
            <Box id="a1" w={30} h={10} />
          </Cell>
          <Cell id="b" style={border("#0000ff")}>
            <Box id="b1" w={30} h={10} />
          </Cell>
        </Row>
        <Row id="r2">
          <Cell id="c" style={border("#008000")}>
            <Box id="c1" w={30} h={10} />
          </Cell>
          <Cell id="d" style={border("#ffa500")}>
            <Box id="d1" w={30} h={10} />
          </Cell>
        </Row>
      </Table>
    )
    expectBoxes(ui, {
      // oracle:begin
      // Chrome's boxes are at 1.5 and 34.5: here they snap to a pixel, half toward zero, and the sizes agree.
      t: [0, 0, 69, 29],
      r1: [1, 1, 66, 13],
      a: [1, 1, 33, 13],
      a1: [3, 3, 30, 10],
      b: [34, 1, 33, 13],
      b1: [36, 3, 30, 10],
      r2: [1, 14, 66, 13],
      c: [1, 14, 33, 13],
      c1: [3, 16, 30, 10],
      d: [34, 14, 33, 13],
      d1: [36, 16, 30, 10],
      // oracle:end
    })
    expectMap(
      paint("collapse-odd", ui),
      [
      "RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
      "RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
      "RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
      "RRR..............................RRR..............................BBB",
      "RRR..............................RRR..............................BBB",
      "RRR..............................RRR..............................BBB",
      "RRR..............................RRR..............................BBB",
      "RRR..............................RRR..............................BBB",
      "RRR..............................RRR..............................BBB",
      "RRR..............................RRR..............................BBB",
      "RRR..............................RRR..............................BBB",
      "RRR..............................RRR..............................BBB",
      "RRR..............................RRR..............................BBB",
      "RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
      "RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
      "RRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRRBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
      "GGG..............................GGG..............................OOO",
      "GGG..............................GGG..............................OOO",
      "GGG..............................GGG..............................OOO",
      "GGG..............................GGG..............................OOO",
      "GGG..............................GGG..............................OOO",
      "GGG..............................GGG..............................OOO",
      "GGG..............................GGG..............................OOO",
      "GGG..............................GGG..............................OOO",
      "GGG..............................GGG..............................OOO",
      "GGG..............................GGG..............................OOO",
      "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO",
      "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO",
      "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOOO"
      ],
      { R: RED, B: BLUE, G: DARK_GREEN, O: ORANGE }
    )
  })

  it("paints no border on a row, a row group, a column or a column group of its own", () => {
    const edge: Style = { border: "4px solid #ff0000" }
    const ui = (collapse: boolean) => (
      <Table id="t" style={{ borderCollapse: collapse ? "collapse" : "separate" }}>
        <ColGroup id="cg" style={edge}>
          <Col id="c1" style={edge} />
        </ColGroup>
        <Body id="g" style={edge}>
          <Row id="r" style={edge}>
            <Cell id="a">
              <Box id="a1" w={30} h={10} />
            </Cell>
          </Row>
        </Body>
      </Table>
    )
    // Separated, none of them has a border: the table is as big as its cell and white all over.
    const separate = paint("collapse-off", ui(false))
    expect(separate.at(1, 1)).toEqual(WHITE)
    expect(separate.at(15, 5)).toEqual(WHITE)
    expectBoxes(ui(false), { t: [0, 0, 30, 10], a: [0, 0, 30, 10] })
    // Collapsed, they are the borders of the cell's edges.
    expect(paint("collapse-on", ui(true)).at(1, 1)).toEqual(RED)
  })

  /**
   * Table parts whose parent is not a table get an anonymous table, one for each run of them: the cells `a` and `b`
   * are one table, a block ends it, and the row `r` and the row group `g` after it are another, as wide as the widest
   * of their cells. Another run of cells does not make a column of the first table.
   *
   * ```html
   * <div id="p">
   *   <div id="h" style="height:5px"></div>
   *   <div id="a" style="display:table-cell"><div id="a1" style="width:30px;height:10px"></div></div>
   *   <div id="b" style="display:table-cell"><div id="b1" style="width:20px;height:20px"></div></div>
   *   <div id="m" style="height:7px"></div>
   *   <div id="r" style="display:table-row"><div id="c" style="display:table-cell"><div id="c1" style="width:10px;height:10px"></div></div></div>
   *   <div id="g" style="display:table-row-group"><div id="r2" style="display:table-row"><div id="d" style="display:table-cell"><div id="d1" style="width:40px;height:6px"></div></div></div></div>
   *   <div id="e" style="height:3px"></div>
   * </div>
   * ```
   */
  it("wraps runs of table parts that are not in a table in an anonymous table", () => {
    expectBoxes(
      <div testId="p">
        <Box id="h" w={300} h={5} />
        <Cell id="a">
          <Box id="a1" w={30} h={10} />
        </Cell>
        <Cell id="b">
          <Box id="b1" w={20} h={20} />
        </Cell>
        <Box id="m" w={300} h={7} />
        <Row id="r">
          <Cell id="c">
            <Box id="c1" w={10} h={10} />
          </Cell>
        </Row>
        <Body id="g">
          <Row id="r2">
            <Cell id="d">
              <Box id="d1" w={40} h={6} />
            </Cell>
          </Row>
        </Body>
        <Box id="e" w={300} h={3} />
      </div>,
      {
        // oracle:begin
        p: [0, 0, 300, 51],
        h: [0, 0, 300, 5],
        a: [0, 5, 30, 20],
        a1: [0, 15, 30, 10],
        b: [30, 5, 20, 20],
        b1: [30, 5, 20, 20],
        m: [0, 25, 300, 7],
        r: [0, 32, 40, 10],
        c: [0, 32, 40, 10],
        c1: [0, 32, 10, 10],
        g: [0, 42, 40, 6],
        r2: [0, 42, 40, 6],
        d: [0, 42, 40, 6],
        d1: [0, 42, 40, 6],
        e: [0, 48, 300, 3],
        // oracle:end
      }
    )
  })

  it("leaves a table part alone in a flex container, where it is a flex item, and drops white space between parts", () => {
    expectBoxes(
      <div testId="p" style={{ display: "flex" }}>
        <Cell id="a">
          <Box id="a1" w={30} h={10} />
        </Cell>
        <Cell id="b">
          <Box id="b1" w={20} h={20} />
        </Cell>
      </div>,
      { a: [0, 0, 30, 20], b: [30, 0, 20, 20] }
    )
    expectBoxes(
      <div testId="p">
        <Cell id="a">
          <Box id="a1" w={30} h={10} />
        </Cell>{" "}
        <Cell id="b">
          <Box id="b1" w={20} h={20} />
        </Cell>
      </div>,
      { b: [30, 0, 20, 20] }
    )
  })

  /**
   * The anonymous table inherits `border-collapse`, so cells in a block with it collapse their borders.
   *
   * ```html
   * <div id="p" style="border-collapse:collapse">
   *   <div id="a" style="display:table-cell;border:4px solid #ff0000"><div id="a1" style="width:20px;height:10px"></div></div>
   *   <div id="b" style="display:table-cell;border:2px solid #0000ff"><div id="b1" style="width:20px;height:10px"></div></div>
   * </div>
   * ```
   * `R` red, `B` blue, 52 by 18 pixels.
   */
  it("collapses the borders of cells in an anonymous table when the parent says so", () => {
    const ui = (
      <div testId="p" style={{ borderCollapse: "collapse" }}>
        <Cell id="a" style={{ border: "4px solid #ff0000" }}>
          <Box id="a1" w={20} h={10} />
        </Cell>
        <Cell id="b" style={{ border: "2px solid #0000ff" }}>
          <Box id="b1" w={20} h={10} />
        </Cell>
      </div>
    )
    expectBoxes(ui, {
      // oracle:begin
      a: [2, 2, 24, 14],
      a1: [4, 4, 20, 10],
      b: [26, 2, 23, 14],
      b1: [28, 4, 20, 10],
      // oracle:end
    })
    expectMap(
      paint("collapse-anonymous", ui),
      [
        "RRRRRRRRRRRRRRRRRRRRRRRRRRRR........................",
        "RRRRRRRRRRRRRRRRRRRRRRRRRRRRBBBBBBBBBBBBBBBBBBBBBB..",
        "RRRRRRRRRRRRRRRRRRRRRRRRRRRRBBBBBBBBBBBBBBBBBBBBBB..",
        "RRRRRRRRRRRRRRRRRRRRRRRRRRRR....................BB..",
        "RRRR....................RRRR....................BB..",
        "RRRR....................RRRR....................BB..",
        "RRRR....................RRRR....................BB..",
        "RRRR....................RRRR....................BB..",
        "RRRR....................RRRR....................BB..",
        "RRRR....................RRRR....................BB..",
        "RRRR....................RRRR....................BB..",
        "RRRR....................RRRR....................BB..",
        "RRRR....................RRRR....................BB..",
        "RRRR....................RRRR....................BB..",
        "RRRRRRRRRRRRRRRRRRRRRRRRRRRR....................BB..",
        "RRRRRRRRRRRRRRRRRRRRRRRRRRRRBBBBBBBBBBBBBBBBBBBBBB..",
        "RRRRRRRRRRRRRRRRRRRRRRRRRRRRBBBBBBBBBBBBBBBBBBBBBB..",
        "RRRRRRRRRRRRRRRRRRRRRRRRRRRR........................"
      ],
      { R: RED, B: BLUE }
    )
  })

  it("paints a collapsed table's borders under its opacity", () => {
    const solid = paint(
      "collapse-opaque",
      <Table id="t" style={{ borderCollapse: "collapse", border: "4px solid #0000ff" }}>
        <Row id="r">
          <Cell id="a">
            <Box id="a1" w={20} h={10} />
          </Cell>
        </Row>
      </Table>
    )
    const faded = paint(
      "collapse-faded",
      <Table id="t" style={{ borderCollapse: "collapse", border: "4px solid #0000ff", opacity: 0.5 }}>
        <Row id="r">
          <Cell id="a">
            <Box id="a1" w={20} h={10} />
          </Cell>
        </Row>
      </Table>
    )
    expect(solid.at(10, 1)).toEqual(BLUE)
    const [r, g, b] = faded.at(10, 1)
    expect([r, g].every((value) => value > 100 && value < 160), `blended: ${r},${g},${b}`).toBe(true)
    expect(b).toBe(255)
  })
})
