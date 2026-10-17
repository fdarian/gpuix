/// `hasHover` and tables: a cell, a row, a caption or the text of an anonymous cell is a descendant of
/// the row, row group and table around it, like any other child, so hovering it restyles them. The
/// pointer finds the part through the table's own layout and the relational state reads the hover
/// chain of the retained tree, in which an anonymous cell is not a node.

import fs from "fs"
import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Style = NonNullable<React.ComponentProps<"div">["style"]>
type Rgba = [number, number, number, number]
const RED: Rgba = [255, 0, 0, 255]
const BLUE: Rgba = [0, 0, 255, 255]
const GREEN: Rgba = [0, 255, 0, 255]
const WHITE: Rgba = [255, 255, 255, 255]

type TestRoot = ReturnType<typeof createTestRoot>

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

let shot = 0

function sample(testRoot: TestRoot, x: number, y: number): Rgba {
  const file = `${SHOTS_DIR}/tables-has-state-${shot++}.png`
  fs.rmSync(file, { force: true })
  testRoot.renderer.captureScreenshot(file)
  return readLogicalPixels(file, testRoot.renderer).at(x, y)
}

function centre(testRoot: TestRoot, id: string): [number, number] {
  testRoot.renderer.dispatchNativeEvents()
  const element = testRoot.renderer.findByTestId(id)
  expect(element, `${id} is mounted`).toBeDefined()
  const box = testRoot.renderer.getElementBounds(element!.id)
  expect(box, `${id} has a box`).not.toBeNull()
  return [box!.x + box!.width / 2, box!.y + box!.height / 2]
}

function hover(testRoot: TestRoot, id: string) {
  const [x, y] = centre(testRoot, id)
  testRoot.renderer.nativeSimulateMouseMove(x, y)
}

function withRoot(tree: React.ReactElement, run: (testRoot: TestRoot) => void) {
  const testRoot = createTestRoot()
  try {
    testRoot.render(tree)
    run(testRoot)
  } finally {
    testRoot.unmount()
  }
}

const box = (id: string, width = 100, height = 40) => <div testId={id} style={{ width, height }} />

describeNative("hasHover and tables", () => {
  it("restyles the row of a hovered cell, and no other row", () => {
    // Two rows of two 100x40 cells; the first row paints under its cells.
    const tree = (
      <div style={{ width: 300 }}>
        <Table id="t" style={{ borderSpacing: 0 }}>
          <Row id="r1" style={{ backgroundColor: "#ff0000", hasHover: { backgroundColor: "#0000ff" } }}>
            <Cell id="a">{box("a1")}</Cell>
            <Cell id="b">{box("b1")}</Cell>
          </Row>
          <Row id="r2" style={{ backgroundColor: "#ff0000", hasHover: { backgroundColor: "#0000ff" } }}>
            <Cell id="c">{box("c1")}</Cell>
            <Cell id="d">{box("d1")}</Cell>
          </Row>
        </Table>
      </div>
    )
    withRoot(tree, (testRoot) => {
      testRoot.renderer.nativeSimulateMouseMove(280, 200)
      expect(sample(testRoot, 20, 20)).toEqual(RED)
      hover(testRoot, "b1")
      expect(sample(testRoot, 20, 20), "the first row").toEqual(BLUE)
      expect(sample(testRoot, 20, 60), "the second row").toEqual(RED)
      hover(testRoot, "c1")
      expect(sample(testRoot, 20, 20)).toEqual(RED)
      expect(sample(testRoot, 20, 60)).toEqual(BLUE)
      testRoot.renderer.nativeSimulateMouseMove(280, 200)
      expect(sample(testRoot, 20, 60)).toEqual(RED)
    })
  })

  it("restyles a row group and the table, and a cell's own hover wins over what it is under", () => {
    const tree = (
      <div style={{ width: 300 }}>
        <Table
          id="t"
          style={{ borderSpacing: 10, backgroundColor: "#ff0000", hasHover: { backgroundColor: "#0000ff" } }}
        >
          <Row id="r1">
            <Cell id="a" style={{ backgroundColor: "#ffffff", hover: { backgroundColor: "#00ff00" } }}>
              {box("a1")}
            </Cell>
          </Row>
        </Table>
      </div>
    )
    withRoot(tree, (testRoot) => {
      // The table's own background shows in the spacing around the cell, at (5, 5).
      testRoot.renderer.nativeSimulateMouseMove(280, 200)
      expect(sample(testRoot, 5, 5)).toEqual(RED)
      expect(sample(testRoot, 30, 30)).toEqual(WHITE)
      hover(testRoot, "a1")
      expect(sample(testRoot, 5, 5), "the table has a hovered cell").toEqual(BLUE)
      expect(sample(testRoot, 30, 30), "the cell is hovered itself").toEqual(GREEN)
    })
  })

  it("restyles the table when the text of an anonymous cell, or a caption, is hovered", () => {
    const tree = (
      <div style={{ width: 300, fontSize: 20 }}>
        <Table
          id="t"
          style={{ borderSpacing: 0, padding: 10, backgroundColor: "#ff0000", hasHover: { backgroundColor: "#0000ff" } }}
        >
          <Caption id="cap">{box("cap1", 80, 20)}</Caption>
          <div testId="loose" style={{ width: 80, height: 20 }} />
          <Row id="r">
            <Cell id="a">{box("a1", 80, 20)}</Cell>
          </Row>
        </Table>
      </div>
    )
    withRoot(tree, (testRoot) => {
      testRoot.renderer.nativeSimulateMouseMove(290, 300)
      expect(sample(testRoot, 3, 40)).toEqual(RED)
      hover(testRoot, "loose")
      expect(sample(testRoot, 3, 40), "a block in an anonymous cell").toEqual(BLUE)
      testRoot.renderer.nativeSimulateMouseMove(290, 300)
      expect(sample(testRoot, 3, 40)).toEqual(RED)
      hover(testRoot, "cap1")
      expect(sample(testRoot, 3, 40), "the caption").toEqual(BLUE)
    })
  })

  it("does not restyle a row while the pointer is on the spacing around its cells", () => {
    const tree = (
      <div style={{ width: 300 }}>
        <Table id="t" style={{ borderSpacing: "20px 0" }}>
          <Row id="r1" style={{ backgroundColor: "#ff0000", hasHover: { backgroundColor: "#0000ff" } }}>
            <Cell id="a">{box("a1")}</Cell>
          </Row>
        </Table>
      </div>
    )
    withRoot(tree, (testRoot) => {
      // The pointer on the 20px of spacing left of the cell is on the table, which is no descendant of the row.
      hover(testRoot, "a1")
      expect(sample(testRoot, 30, 20)).toEqual(BLUE)
      testRoot.renderer.nativeSimulateMouseMove(10, 20)
      expect(sample(testRoot, 30, 20)).toEqual(RED)
    })
  })

  it("restyles the paragraph around an inline-table while one of its cells is hovered", () => {
    const tree = (
      <div
        style={{ width: 300, height: 80, backgroundColor: "#ff0000", fontSize: 20, hasHover: { backgroundColor: "#0000ff" } }}
      >
        AA
        <div style={{ display: "inline-table" }}>
          <Row id="r">
            <Cell id="a">{box("a1", 30, 30)}</Cell>
          </Row>
        </div>
      </div>
    )
    withRoot(tree, (testRoot) => {
      testRoot.renderer.nativeSimulateMouseMove(280, 70)
      expect(sample(testRoot, 250, 70)).toEqual(RED)
      hover(testRoot, "a1")
      expect(sample(testRoot, 250, 70)).toEqual(BLUE)
      testRoot.renderer.nativeSimulateMouseMove(280, 70)
      expect(sample(testRoot, 250, 70)).toEqual(RED)
    })
  })
})
