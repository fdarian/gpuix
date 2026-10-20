/// An `@container` block sets an item's `order` as well as its `display`: the flex or grid container sorts its items by the
/// order the blocks of the query container above give them, in the frame that lays the container out, in both directions
/// (a container that shrinks past the query, and one that grows back). Tab order stays in document order.
///
/// The expected positions are from headless Chrome (chrome-headless-shell, never the Chrome app):
/// fixtures/container-order-oracle.html has every case with the `x` of each item from the left of its row at two widths of
/// the query container, and fixtures/container-order-oracle.sh regenerates the json.

import fs from "fs"
import path from "path"
import type { ReactElement } from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Item = { id: string; order?: number; queried?: number; display?: string }
type Case = {
  name: string
  kind: "flex" | "grid" | "contents"
  container: "outside" | "row"
  items: Item[]
  widths: number[]
  results: Record<string, (number | null)[]>
}

const ORACLE: Case[] = JSON.parse(
  fs.readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/container-order-oracle.json"), "utf8")
)

const QUERY = "box (width < 400px)"

function app(spec: Case, width: number): ReactElement {
  const items = spec.items.map((item) => {
    const block: Record<string, unknown> = {}
    if (item.queried !== undefined) block.order = item.queried
    if (item.display) block.display = item.display
    return (
      <div
        key={item.id}
        testId={item.id}
        tabIndex={0}
        style={{
          width: 40,
          height: 20,
          order: item.order,
          "@container": Object.keys(block).length > 0 ? { [QUERY]: block } : undefined,
        }}
      />
    )
  })
  const rowStyle = {
    display: spec.kind === "grid" ? "grid" : "flex",
    gridTemplateColumns: spec.kind === "grid" ? "40px 40px 40px 40px" : undefined,
  }
  if (spec.container === "row") {
    return (
      <div testId="row" style={{ ...rowStyle, width, containerType: "inline-size", containerName: "box" }}>
        {items}
      </div>
    )
  }
  const row = (
    <div testId="row" style={rowStyle}>
      {items}
    </div>
  )
  return (
    <div testId="box" style={{ width, containerType: "inline-size", containerName: "box" }}>
      {spec.kind === "contents" ? <div style={{ display: "contents" }}>{row}</div> : row}
    </div>
  )
}

function setup() {
  const root = createTestRoot({ width: 700, height: 200 })
  const boundsOf = (testId: string) => {
    const element = root.renderer.findByTestId(testId)
    return element ? root.renderer.getElementBounds(element.id) : undefined
  }
  /** The `x` of each item from the left of the row, `null` for an item with no box. */
  const positions = (spec: Case) => {
    const row = boundsOf("row")!
    return spec.items.map((item) => {
      const bounds = boundsOf(item.id)
      return bounds && bounds.width > 0 ? bounds.x - row.x : null
    })
  }
  const paintedFrames = (change: () => void): number => {
    const before = root.renderer.getPaintedFrameCount()
    change()
    return root.renderer.getPaintedFrameCount() - before
  }
  const tabOrder = (count: number) => {
    const visited: (string | null)[] = []
    for (let step = 0; step < count; step++) {
      root.renderer.simulateKeystrokes("Tab")
      visited.push(root.renderer.getElement(root.renderer.getFocusedElementId())?.testId ?? null)
    }
    return visited
  }
  return { ...root, positions, paintedFrames, tabOrder }
}

describeNative("order from an @container block", () => {
  for (const spec of ORACLE) {
    describe(spec.name, () => {
      const [narrow, wide] = spec.widths.map(String)
      const [narrowWidth, wideWidth] = spec.widths

      it("sorts the items in the first frame, narrow and wide", () => {
        for (const [width, key] of [
          [narrowWidth, narrow],
          [wideWidth, wide],
        ] as const) {
          const { render, positions, paintedFrames, unmount } = setup()
          try {
            expect(paintedFrames(() => render(app(spec, width)))).toBe(1)
            expect(positions(spec), `at ${width}`).toEqual(spec.results[key])
          } finally {
            unmount()
          }
        }
      })

      it("sorts them again in the frame that resizes the container, both ways", () => {
        const { render, positions, paintedFrames, unmount } = setup()
        try {
          render(app(spec, wideWidth))
          expect(positions(spec)).toEqual(spec.results[wide])
          for (const [width, key] of [
            [narrowWidth, narrow],
            [wideWidth, wide],
            [narrowWidth, narrow],
          ] as const) {
            expect(paintedFrames(() => render(app(spec, width)))).toBe(1)
            expect(positions(spec), `at ${width}`).toEqual(spec.results[key])
          }
        } finally {
          unmount()
        }
      })

      it("keeps Tab in document order", () => {
        const { render, tabOrder, unmount } = setup()
        try {
          render(app(spec, narrowWidth))
          const ids = spec.items.filter((item) => item.display !== "none").map((item) => item.id)
          expect(tabOrder(ids.length)).toEqual(ids)
        } finally {
          unmount()
        }
      })
    })
  }
})
