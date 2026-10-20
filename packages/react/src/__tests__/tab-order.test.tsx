/// Tab order is document order, whatever flex and grid `order` do to the layout and the paint order: `tabIndex` above
/// zero first (ascending, document order among equals), then the zeros in the order of the document.
///
/// The expected ids are from headless Chrome (chrome-headless-shell, never the Chrome app) pressing Tab, then Shift+Tab
/// back from the last stop: fixtures/tab-order-oracle.html has every case and its tree, and tab-order-oracle.sh the
/// playwright driver that regenerates tab-order-oracle.json.

import fs from "fs"
import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Node = { id?: string; tabIndex?: number; style?: Record<string, unknown>; children?: Node[] }
type Case = { name: string; tree: Node; tab: string[]; shiftTab: string[] }

const ORACLE: Case[] = JSON.parse(
  fs.readFileSync(new URL("./fixtures/tab-order-oracle.json", import.meta.url), "utf8")
)

function build(node: Node, key = 0): React.ReactElement {
  return React.createElement(
    "div",
    { key, testId: node.id, tabIndex: node.tabIndex, style: node.style },
    ...(node.children ?? []).map((child, index) => build(child, index))
  )
}

function setup(tree: Node) {
  const root = createTestRoot({ width: 300, height: 200 })
  root.render(<div style={{ padding: 10 }}>{build(tree)}</div>)
  const renderer = root.renderer
  const focused = () => renderer.getElement(renderer.getFocusedElementId())?.testId ?? null
  return { root, renderer, focused }
}

/** The ids Tab visits `count` times from a fresh window, then the ids Shift+Tab visits back from the last stop. */
function walk(tree: Node, count: number) {
  const { renderer, focused } = setup(tree)
  const tab: (string | null)[] = []
  for (let step = 0; step < count; step++) {
    renderer.simulateKeystrokes("Tab")
    tab.push(focused())
  }
  const shiftTab = [tab[tab.length - 1]]
  for (let step = 1; step < count; step++) {
    renderer.simulateKeystrokes("Shift+Tab")
    shiftTab.push(focused())
  }
  return { tab, shiftTab }
}

describeNative("tab order", () => {
  for (const oracle of ORACLE) {
    it(`follows the document in ${oracle.name}`, () => {
      const walked = walk(oracle.tree, oracle.tab.length)
      expect(walked.tab).toEqual(oracle.tab)
      expect(walked.shiftTab).toEqual(oracle.shiftTab)
    })
  }

  it("starts Shift+Tab from the last stop in the document", () => {
    const oracle = ORACLE.find((candidate) => candidate.name === "flex row, order 3 2 1")!
    const { renderer, focused } = setup(oracle.tree)
    renderer.simulateKeystrokes("Shift+Tab")
    expect(focused()).toBe(oracle.tab[oracle.tab.length - 1])
  })

  it("follows the document again after a change of `order`", () => {
    const stop = (id: string, order: number) => ({ id, tabIndex: 0, style: { width: 40, height: 20, order } })
    const row = (...orders: number[]): Node => ({
      style: { display: "flex" },
      children: orders.map((order, index) => stop("abc"[index], order)),
    })
    const root = createTestRoot({ width: 300, height: 200 })
    const renderer = root.renderer
    const visit = () => {
      const visited: (string | null)[] = []
      for (let step = 0; step < 3; step++) {
        renderer.simulateKeystrokes("Tab")
        visited.push(renderer.getElement(renderer.getFocusedElementId())?.testId ?? null)
      }
      return visited
    }
    root.render(build(row(3, 2, 1)))
    expect(visit()).toEqual(["a", "b", "c"])
    // Moving every item to the other end of the row leaves the document, and Tab, where they were.
    root.render(build(row(1, 2, 3)))
    expect(visit()).toEqual(["a", "b", "c"])
    root.render(build(row(0, -1, 2)))
    expect(visit()).toEqual(["a", "b", "c"])
  })
})
