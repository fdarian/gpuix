/// Selecting, copying and finding text in tables: document order (a positioned cell, a footer group written
/// first) together with the tabs and line breaks Chrome puts between cells and rows.
///
/// The strings are `getSelection().toString()` of headless Chrome (chrome-headless-shell) on the HTML above each
/// test, after `selectAllChildren` of the container, which is what a drag over the whole table gives:
///
/// ```html
/// <div id="root" style="width:400px;font:20px/30px GpuixTestSquare"> ...the case... </div>
/// ```

import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Style = NonNullable<React.ComponentProps<"div">["style"]>

function part(display: string) {
  return function Part({ id, style, children }: { id?: string; style?: Style; children?: React.ReactNode }) {
    return (
      <div testId={id} style={{ display, ...style }}>
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

type TestRoot = ReturnType<typeof createTestRoot>

function boxOf(renderer: TestRoot["renderer"], id: string) {
  const element = renderer.findByTestId(id)
  expect(element, `${id} is mounted`).toBeDefined()
  const bounds = renderer.getElementBounds(element!.id)
  expect(bounds, `${id} has a box`).not.toBeNull()
  return bounds!
}

/** Drags from just inside the top left of `from` to `past` pixels beyond the bottom right of `to`. */
function drag(renderer: TestRoot["renderer"], from: string, to: string, past = 30) {
  const start = boxOf(renderer, from)
  const end = boxOf(renderer, to)
  return renderer.dragSelect(start.x + 1, start.y + 5, end.x + end.width + past, end.y + end.height - 5)
}

function dragBack(renderer: TestRoot["renderer"], from: string, to: string) {
  const start = boxOf(renderer, from)
  const end = boxOf(renderer, to)
  return renderer.dragSelect(end.x + end.width + 30, end.y + end.height - 5, start.x + 1, start.y + 5)
}

const wrap = (ui: React.ReactElement) => <div style={{ width: 400, fontSize: 20, lineHeight: "30px" }}>{ui}</div>

describeNative("tables x selection order", () => {
  /**
   * A cell with `position: relative` (and one with a `z-index`) paints after the cells that follow it, and the
   * text still copies where it sits in the document.
   *
   * ```html
   * <div id="t" style="display:table"><div id="r1" style="display:table-row">
   *   <div id="a" style="display:table-cell;position:relative">aa</div><div id="b" style="display:table-cell">bb</div></div>
   * <div id="r2" style="display:table-row"><div id="c" style="display:table-cell">cc</div>
   *   <div id="d" style="display:table-cell;position:relative;z-index:2">dd</div></div></div>
   * ```
   * selects as `"aa\tbb\ncc\tdd"`.
   */
  it("copies the cells of positioned cells in document order, with tabs and line breaks", () => {
    const ui = wrap(
      <Table id="t">
        <Row id="r1">
          <Cell id="a" style={{ position: "relative" }}>
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
          <Cell id="d" style={{ position: "relative", zIndex: 2 }}>
            <text>dd</text>
          </Cell>
        </Row>
      </Table>
    )
    const { render, renderer, unmount } = createTestRoot()
    try {
      render(ui)
      expect(drag(renderer, "a", "d")).toBe("aa\tbb\ncc\tdd")
      renderer.clearSelection()
      expect(dragBack(renderer, "a", "d")).toBe("aa\tbb\ncc\tdd")
      renderer.clearSelection()
      // From the second cell of the first row to the first of the second: both ends are plain cells, the
      // positioned ones are outside the range.
      expect(drag(renderer, "b", "c", 0)).toBe("bb\ncc")
    } finally {
      unmount()
    }
  })

  /**
   * A footer group written first lays out last, and a header group written second lays out first; the
   * selection keeps the source order.
   *
   * ```html
   * <div style="display:table"><div style="display:table-footer-group"><div style="display:table-row"><div style="display:table-cell">ft</div></div></div>
   * <div style="display:table-header-group"><div style="display:table-row"><div style="display:table-cell">hd</div></div></div>
   * <div style="display:table-row-group"><div style="display:table-row"><div style="display:table-cell">b1</div><div style="display:table-cell">b2</div></div></div></div>
   * ```
   * selects as `"ft\nhd\nb1\tb2"`.
   */
  it("keeps the source order of a footer group written first", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      render(
        wrap(
          <Table id="t">
            <Foot id="tf">
              <Row>
                <Cell id="ft">
                  <text>ft</text>
                </Cell>
              </Row>
            </Foot>
            <Head id="th">
              <Row>
                <Cell id="hd">
                  <text>hd</text>
                </Cell>
              </Row>
            </Head>
            <Body id="tb">
              <Row>
                <Cell id="b1">
                  <text>b1</text>
                </Cell>
                <Cell id="b2">
                  <text>b2</text>
                </Cell>
              </Row>
            </Body>
          </Table>
        )
      )
      // Laid out: hd, b1 b2, ft. The header is the first thing painted, the body ends the range.
      expect(boxOf(renderer, "hd").y).toBeLessThan(boxOf(renderer, "b1").y)
      expect(boxOf(renderer, "b1").y).toBeLessThan(boxOf(renderer, "ft").y)
      expect(drag(renderer, "hd", "b2")).toBe("hd\nb1\tb2")
      renderer.clearSelection()
      // From the end of the header up to the start of the footer, written first: nothing in between in the document.
      expect(dragBack(renderer, "ft", "hd")).toBe("ft\nhd")
    } finally {
      unmount()
    }
  })

  /**
   * A row with `position: relative` and a caption.
   *
   * ```html
   * <div style="display:table"><div style="display:table-caption">cap</div>
   * <div style="display:table-row;position:relative;top:5px"><div style="display:table-cell">x</div><div style="display:table-cell">y</div></div>
   * <div style="display:table-row"><div style="display:table-cell">z</div></div></div>
   * ```
   * selects as `"cap\nx\ty\nz"`.
   */
  it("copies a positioned row and a caption in document order", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      render(
        wrap(
          <Table id="t">
            <Caption id="cap">
              <text>cap</text>
            </Caption>
            <Row id="r1" style={{ position: "relative", top: 5 }}>
              <Cell id="x">
                <text>x</text>
              </Cell>
              <Cell id="y">
                <text>y</text>
              </Cell>
            </Row>
            <Row id="r2">
              <Cell id="z">
                <text>z</text>
              </Cell>
            </Row>
          </Table>
        )
      )
      expect(drag(renderer, "cap", "z")).toBe("cap\nx\ty\nz")
    } finally {
      unmount()
    }
  })

  it("copies the anonymous cells of a block in a positioned row in document order", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      render(
        wrap(
          <Table id="t">
            before
            <Row id="r" style={{ position: "relative" }}>
              <Cell id="a">
                <text>aa</text>
              </Cell>
              <Cell id="b">
                <text>bb</text>
              </Cell>
            </Row>
            after
          </Table>
        )
      )
      expect(drag(renderer, "t", "t", 0)).toBe("before\naa\tbb\nafter")
    } finally {
      unmount()
    }
  })

  /** Find numbers the matches in document order, as the selection does, and the cells in a table are no exception. */
  it("numbers the matches of find in document order when a cell paints late", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      const ui = (activeIndex: number) => (
        <div style={{ width: 400, fontSize: 20 }} highlight={{ query: "x", activeIndex }}>
          <Table>
            <Row>
              <Cell style={{ position: "relative" }}>
                <text>x first</text>
              </Cell>
              <Cell>
                <text>x second</text>
              </Cell>
            </Row>
            <Row>
              <Cell>
                <text>x third</text>
              </Cell>
            </Row>
          </Table>
        </div>
      )
      render(ui(0))
      const hits = renderer.getPaintedHighlights()
      // Painted: the plain cells first, the positioned one last.
      expect(hits.map((hit) => hit.text)).toEqual(["x second", "x third", "x first"])
      expect(hits.map((hit) => hit.active)).toEqual([false, false, true])
      render(ui(1))
      expect(renderer.getPaintedHighlights().map((hit) => hit.active)).toEqual([true, false, false])
      render(ui(2))
      expect(renderer.getPaintedHighlights().map((hit) => hit.active)).toEqual([false, true, false])
    } finally {
      unmount()
    }
  })
})
