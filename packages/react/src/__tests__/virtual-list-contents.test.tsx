/**
 * A `<virtual-list>` row that is `display: contents`: it generates no box, its
 * children stack in the row's slot, and its own listeners still hear them.
 */
import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"
import React from "react"
import { afterEach, beforeEach, describe, expect, it } from "vitest"
import { IntersectionObserver, ResizeObserver } from "../index"
import { createTestRoot, hasNativeTestRenderer, type TestRoot } from "../testing.js"
import type { PublicInstance } from "../types/host.js"

/** What Chrome reports for the same shapes: `fixtures/display-contents-oracle.html`. */
const oracle = JSON.parse(
  fs.readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/display-contents-oracle.json"), "utf8")
)

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const list = { width: 400, height: 200 } as const
const contents = { display: "contents" } as const

describeNative("virtual-list row: display contents", () => {
  let testRoot: TestRoot

  beforeEach(() => {
    testRoot = createTestRoot()
  })

  afterEach(() => {
    testRoot.unmount()
  })

  const id = (testId: string) => testRoot.renderer.findByTestId(testId)!.id
  const bounds = (testId: string) => testRoot.renderer.getElementBounds(id(testId))
  const focused = () => testRoot.renderer.getElement(testRoot.renderer.getFocusedElementId())?.testId ?? null

  function stacked(rows: number, overrides: (row: number) => Record<string, unknown> = () => ({})) {
    return (
      <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
        {Array.from({ length: rows }, (_, row) => (
          <div key={row} testId={`row-${row}`} style={contents} {...overrides(row)}>
            <div testId={`row-${row}-a`} tabIndex={0} style={{ height: 20, flexShrink: 0 }} />
            <div testId={`row-${row}-b`} tabIndex={0} style={{ height: 20, flexShrink: 0 }} />
          </div>
        ))}
      </virtual-list>
    )
  }

  describe("listeners", () => {
    it("hears a click on a child, capture before bubble, with currentTarget the row and target the child", () => {
      const seen: Array<[string, number, number]> = []
      const record = (name: string) => (event: any) => seen.push([name, event.currentTarget.id, event.target.id])
      testRoot.render(stacked(3, (row) => (row === 1 ? { onClick: record("bubble"), onClickCapture: record("capture") } : {})))

      testRoot.renderer.nativeSimulateClick(10, 50)

      expect(seen).toEqual([
        ["capture", id("row-1"), id("row-1-a")],
        ["bubble", id("row-1"), id("row-1-a")],
      ])
    })

    it("hears pointer, mouse and focus events of its children and not of another row's", () => {
      const seen: string[] = []
      const record = (name: string) => () => seen.push(name)
      testRoot.render(
        stacked(3, (row) =>
          row === 0
            ? {
                onPointerDown: record("pointerdown"),
                onMouseDown: record("mousedown"),
                onFocus: record("focus"),
                onBlur: record("blur"),
                onKeyDown: record("keydown"),
              }
            : {}
        )
      )

      testRoot.renderer.nativeSimulateClick(10, 70)
      expect(seen, "a press in row 1 is not row 0's").toEqual([])
      testRoot.renderer.nativeSimulateClick(10, 10)
      testRoot.renderer.simulateKeystrokes("x")
      testRoot.renderer.nativeSimulateClick(10, 70)

      expect(seen).toEqual(["pointerdown", "mousedown", "focus", "keydown", "blur"])
    })

    it("gets one enter for any child and one leave when the pointer leaves them all", () => {
      const seen: string[] = []
      const record = (name: string) => () => seen.push(name)
      testRoot.render(
        stacked(2, (row) =>
          row === 0 ? { onPointerEnter: record("pointerenter"), onPointerLeave: record("pointerleave") } : {}
        )
      )
      const move = (y: number) => testRoot.renderer.nativeSimulateMouseMove(10, y)

      move(10)
      expect(seen).toEqual(["pointerenter"])
      move(30)
      expect(seen, "from one child to the next stays inside").toEqual(["pointerenter"])
      move(50)
      expect(seen).toEqual(["pointerenter", "pointerleave"])
    })

    it("is the target of click() on itself", () => {
      const seen: Array<[number, number]> = []
      let row: { click(): void } | undefined
      testRoot.render(
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          <div
            ref={(instance) => {
              if (instance) row = instance as unknown as { click(): void }
            }}
            testId="row"
            style={contents}
            onClick={(event) => seen.push([event.currentTarget.id, event.target.id])}
          >
            <div style={{ height: 20 }} />
          </div>
        </virtual-list>
      )

      row!.click()
      testRoot.renderer.dispatchNativeEvents()

      expect(seen).toEqual([[id("row"), id("row")]])
    })
  })

  describe("measuring", () => {
    it("measures a row as its children's block extent and ignores the row's own box styles", () => {
      testRoot.render(
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          {[0, 1, 2].map((row) => (
            <div
              key={row}
              testId={`row-${row}`}
              style={{ display: "contents", height: 500, padding: 30, margin: 30, borderWidth: 9, borderStyle: "solid" }}
            >
              <div testId={`row-${row}-a`} style={{ height: 30, flexShrink: 0 }} />
              <div testId={`row-${row}-b`} style={{ height: 25, flexShrink: 0 }} />
            </div>
          ))}
        </virtual-list>
      )

      expect(bounds("row-0-a")).toMatchObject({ x: 0, y: 0, width: 400, height: 30 })
      expect(bounds("row-0-b")).toMatchObject({ y: 30, height: 25 })
      expect(bounds("row-1-a")).toMatchObject({ y: 55 })
      expect(bounds("row-2-a")).toMatchObject({ y: 110 })
    })

    it("gives an empty row no height", () => {
      testRoot.render(
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          <div key="a" testId="row-a" style={contents}>
            <div testId="a" style={{ height: 30, flexShrink: 0 }} />
          </div>
          <div key="empty" testId="row-empty" style={contents} />
          <div key="b" testId="row-b" style={contents}>
            <div testId="b" style={{ height: 30, flexShrink: 0 }} />
          </div>
        </virtual-list>
      )

      expect(bounds("a")).toMatchObject({ y: 0 })
      expect(bounds("b")).toMatchObject({ y: 30 })
    })

    it("lays a row out again when it changes between contents and block", () => {
      const app = (display: "contents" | "block") => (
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          {[0, 1].map((row) => (
            <div key={row} testId={`row-${row}`} style={{ display: row === 0 ? display : "block", padding: 10 }}>
              <div testId={`row-${row}-a`} style={{ height: 20, flexShrink: 0 }} />
            </div>
          ))}
        </virtual-list>
      )
      testRoot.render(app("block"))
      expect(bounds("row-0")).toMatchObject({ y: 0, height: 40 })
      expect(bounds("row-0-a")).toMatchObject({ x: 10, y: 10 })
      expect(bounds("row-1")).toMatchObject({ y: 40 })

      testRoot.render(app("contents"))
      expect(bounds("row-0")).toBeNull()
      expect(bounds("row-0-a")).toMatchObject({ x: 0, y: 0, width: 400, height: 20 })
      expect(bounds("row-1")).toMatchObject({ y: 20, height: 40 })

      testRoot.render(app("block"))
      expect(bounds("row-0")).toMatchObject({ y: 0, height: 40 })
      expect(bounds("row-0-a")).toMatchObject({ x: 10, y: 10 })
      expect(bounds("row-1")).toMatchObject({ y: 40 })
    })

    it("follows keyed reorders", () => {
      const app = (order: number[]) => (
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          {order.map((row) => (
            <div key={row} testId={`row-${row}`} style={contents}>
              <div testId={`row-${row}-a`} style={{ height: 10 * (row + 1), flexShrink: 0 }} />
            </div>
          ))}
        </virtual-list>
      )
      testRoot.render(app([0, 1, 2]))
      expect(bounds("row-0-a")).toMatchObject({ y: 0, height: 10 })
      expect(bounds("row-1-a")).toMatchObject({ y: 10, height: 20 })
      expect(bounds("row-2-a")).toMatchObject({ y: 30, height: 30 })

      testRoot.render(app([2, 0, 1]))
      expect(bounds("row-2-a")).toMatchObject({ y: 0 })
      expect(bounds("row-0-a")).toMatchObject({ y: 30 })
      expect(bounds("row-1-a")).toMatchObject({ y: 40 })
    })

    it("scrolls to a row and reaches its children", () => {
      testRoot.render(stacked(50))
      const listId = testRoot.renderer.findByType("virtual-list")[0]!.id

      testRoot.renderer.scrollToItem(listId, 30)

      expect(bounds("row-30-a")).toMatchObject({ y: 0, height: 20 })
      expect(bounds("row-30-b")).toMatchObject({ y: 20 })
      expect(testRoot.renderer.getListScrollTop(listId)).toEqual([30, 0, 200])
    })
  })

  describe("reads", () => {
    it("answers a zero rect for getBoundingClientRect, as Chrome does for display: contents", () => {
      let row: PublicInstance | undefined
      testRoot.render(
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          <div ref={(instance) => void (row = instance ?? row)} style={contents}>
            <div style={{ height: 20, flexShrink: 0 }} />
          </div>
        </virtual-list>
      )

      const [x, y, width, height] = oracle.rect
      expect(row!.getBoundingClientRect()).toEqual({ x, y, width, height, top: y, left: x, right: x + width, bottom: y + height })
    })

    it("is not in getElementsAt: a point over a child hits the child", () => {
      testRoot.render(stacked(3))

      const hits = testRoot.renderer.getElementsAt(10, 50)
      expect(hits[0]).toBe(id("row-1-a"))
      expect(hits).not.toContain(id("row-1"))
      expect(oracle.fromPoint.slice(0, 2)).toEqual(["a", "scroller"])
    })

    it("never intersects, with a zero rect, in an IntersectionObserver", () => {
      let row: PublicInstance | undefined
      const entries: Array<{ isIntersecting: boolean; intersectionRatio: number; boundingClientRect: any }> = []
      testRoot.render(
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          <div ref={(instance) => void (row = instance ?? row)} style={contents}>
            <div style={{ height: 20, flexShrink: 0 }} />
          </div>
        </virtual-list>
      )
      new IntersectionObserver((batch) => entries.push(...batch)).observe(row!)
      testRoot.renderer.dispatchNativeEvents()

      expect(entries).toHaveLength(1)
      const [x, y, width, height] = oracle.io[0].rect
      expect(entries[0]).toMatchObject({
        isIntersecting: oracle.io[0].isIntersecting,
        intersectionRatio: oracle.io[0].ratio,
        boundingClientRect: { x, y, width, height },
      })
    })

    it("keeps its role and name in the accessibility tree around its children, as Chrome does", () => {
      testRoot.render(
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          <div style={contents} role="group" aria-label="row">
            <div role="button" aria-label="row-a" style={{ height: 20, flexShrink: 0 }} />
            <div role="button" aria-label="row-b" style={{ height: 20, flexShrink: 0 }} />
          </div>
        </virtual-list>
      )

      const nodes = testRoot.renderer.getA11yTree().nodes ?? {}
      const named = (label: string) => Object.entries(nodes).find(([, node]) => node.aria?.label === label)!
      expect(oracle.ax).toEqual(expect.arrayContaining(["row", "row-a", "row-b"]))
      const [, group] = named("row")
      expect(group.aria?.role).toBe("Group")
      expect(group.children).toEqual([named("row-a")[0], named("row-b")[0]])
    })

    it("hides its subtree from the accessibility tree with aria-hidden", () => {
      testRoot.render(
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          <div style={contents} aria-hidden="true">
            <div role="button" aria-label="hidden-child" style={{ height: 20, flexShrink: 0 }} />
          </div>
          <div style={contents}>
            <div role="button" aria-label="shown-child" style={{ height: 20, flexShrink: 0 }} />
          </div>
        </virtual-list>
      )

      const nodes = Object.values(testRoot.renderer.getA11yTree().nodes ?? {})
      const named = (label: string) => nodes.find((node) => node.aria?.label === label)!
      expect(named("hidden-child").aria?.hidden).toBe(true)
      expect(named("shown-child").aria?.hidden).toBeFalsy()
      expect(oracle.ax).not.toContain("aria-hidden-child")
    })
  })

  describe("focus", () => {
    it("tabs through the children of contents rows in order and skips a row that has tabIndex itself", () => {
      testRoot.render(
        stacked(3, (row) => (row === 1 ? { tabIndex: 0, "aria-label": "row-1" } : {}))
      )

      const walked: Array<string | null> = []
      for (let press = 0; press < 4; press++) {
        testRoot.renderer.simulateKeystrokes("Tab")
        walked.push(focused())
      }
      expect(walked).toEqual(["row-0-a", "row-0-b", "row-1-a", "row-1-b"])
      testRoot.renderer.simulateKeystrokes("Shift+Tab")
      expect(focused()).toBe("row-1-a")
    })

    it("cannot be focused itself", () => {
      testRoot.render(stacked(2, (row) => (row === 0 ? { tabIndex: 0 } : {})))

      testRoot.renderer.focusElement(id("row-0"))

      expect(focused()).toBe(null)
      expect(oracle.rowFocusable).toBe(false)
    })

    it("scrolls a focused child's row into view", () => {
      testRoot.render(stacked(50))

      testRoot.renderer.focusElement(id("row-40-a"))

      expect(bounds("row-40-a")).not.toBeNull()
      expect(testRoot.renderer.getListScrollTop(testRoot.renderer.findByType("virtual-list")[0]!.id)![0]).toBeGreaterThan(30)
    })

    it("keeps the focus of a child when its row switches between contents and block", () => {
      const app = (display: "contents" | "block") => (
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          <div style={{ display }}>
            <div testId="child" tabIndex={0} style={{ height: 20, flexShrink: 0 }} />
          </div>
        </virtual-list>
      )
      testRoot.render(app("block"))
      testRoot.renderer.focusElement(id("child"))
      expect(focused()).toBe("child")

      testRoot.render(app("contents"))
      expect(focused()).toBe("child")
      testRoot.render(app("block"))
      expect(focused()).toBe("child")
    })
  })

  describe("what the row passes down", () => {
    it("is skipped, with its subtree, by inert", () => {
      const seen: string[] = []
      testRoot.render(
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          <div style={contents} inert>
            <div testId="inert-child" tabIndex={0} onClick={() => seen.push("click")} style={{ height: 20, flexShrink: 0 }} />
          </div>
        </virtual-list>
      )

      testRoot.renderer.nativeSimulateClick(10, 10)
      testRoot.renderer.simulateKeystrokes("Tab")

      expect(seen).toEqual([])
      expect(focused()).toBe(null)
    })

    it("hands pointerEvents: none to its children, so a press goes through them", () => {
      const seen: string[] = []
      testRoot.render(
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list} onClick={() => seen.push("list")}>
          <div style={{ display: "contents", pointerEvents: "none" }}>
            <div testId="child" onClick={() => seen.push("child")} style={{ height: 20, flexShrink: 0 }} />
          </div>
        </virtual-list>
      )

      testRoot.renderer.nativeSimulateClick(10, 10)

      expect(seen).toEqual(["list"])
      expect(testRoot.renderer.getElementsAt(10, 10)).not.toContain(id("child"))
    })

    it("hands visibility: hidden to its children", () => {
      testRoot.render(
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          <div style={{ display: "contents", visibility: "hidden" }}>
            <div testId="hidden" style={{ height: 20, flexShrink: 0 }} />
          </div>
        </virtual-list>
      )

      expect(testRoot.renderer.getElementsAt(10, 10)).not.toContain(id("hidden"))
      expect(bounds("hidden")).toMatchObject({ height: 20 })
    })

    it("hands its text style to text and inline children", () => {
      testRoot.render(
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          <div testId="row" style={{ display: "contents", fontSize: 20, lineHeight: "30px" }}>
            plain text <span testId="span">and a span</span>
          </div>
          <div testId="next" style={{ height: 10, flexShrink: 0 }} />
        </virtual-list>
      )

      expect(bounds("span")).toMatchObject({ height: 23 })
      expect(bounds("next")).toMatchObject({ y: 30 })
    })

    it("splices a nested display: contents element into the row", () => {
      testRoot.render(
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          <div style={contents}>
            <div style={contents}>
              <div testId="a" style={{ height: 15, flexShrink: 0 }} />
              <div style={contents}>
                <div testId="b" style={{ height: 15, flexShrink: 0 }} />
              </div>
            </div>
          </div>
          <div testId="next" style={{ height: 10, flexShrink: 0 }} />
        </virtual-list>
      )

      expect(bounds("a")).toMatchObject({ y: 0 })
      expect(bounds("b")).toMatchObject({ y: 15 })
      expect(bounds("next")).toMatchObject({ y: 30 })
    })

    it("has no height when every child is display: none", () => {
      testRoot.render(
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          <div style={contents}>
            <div style={{ display: "none", height: 50 }} />
          </div>
          <div testId="next" style={{ height: 10, flexShrink: 0 }} />
        </virtual-list>
      )

      expect(bounds("next")).toMatchObject({ y: 0 })
    })
  })

  describe("empty rows", () => {
    it("lets the list scroll through and past them", () => {
      testRoot.render(
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          {Array.from({ length: 60 }, (_, row) =>
            row % 3 === 1 ? (
              <div key={row} style={contents} />
            ) : (
              <div key={row} style={contents}>
                <div testId={`row-${row}`} style={{ height: 30, flexShrink: 0 }} />
              </div>
            )
          )}
        </virtual-list>
      )
      const listId = testRoot.renderer.findByType("virtual-list")[0]!.id

      testRoot.renderer.scrollToItem(listId, 30)
      expect(bounds("row-30")).toMatchObject({ y: 0 })
      testRoot.renderer.scrollToItem(listId, 59)
      const last = bounds("row-59")!
      expect(last.y + last.height).toBe(200)
    })
  })

  describe("observers", () => {
    it("reports no box for the row to a ResizeObserver, like display: none", () => {
      let row: PublicInstance | undefined
      const batches: unknown[] = []
      testRoot.render(
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          <div ref={(instance) => void (row = instance ?? row)} style={contents}>
            <div style={{ height: 20, flexShrink: 0 }} />
          </div>
        </virtual-list>
      )
      new ResizeObserver((entries) => batches.push(entries)).observe(row!)
      testRoot.renderer.dispatchNativeEvents()

      // Chrome delivers one 0 by 0 entry at observe(); the DOM spec, and gpuix, report nothing for 0 by 0.
      expect(oracle.ro.contents).toEqual([[0, 0, 0, 0]])
      expect(batches).toEqual([])
    })

    it("observes a child of the row against the list's clip", () => {
      const entries: Array<{ isIntersecting: boolean }> = []
      let child: PublicInstance | undefined
      testRoot.render(
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          <div style={contents}>
            <div ref={(instance) => void (child = instance ?? child)} style={{ height: 20, flexShrink: 0 }} />
          </div>
        </virtual-list>
      )
      new IntersectionObserver((batch) => entries.push(...batch)).observe(child!)
      testRoot.renderer.dispatchNativeEvents()

      expect(entries).toMatchObject([{ isIntersecting: true }])
    })
  })
})
