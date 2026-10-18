/// A `<virtual-list>` row that is `display: contents`, and the reads of an element with no box, against
/// headless Chrome (`packages/react/src/__tests__/fixtures/display-contents-oracle.json`).
import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { createSignal, For } from "solid-js"
import { describe, expect, it } from "bun:test"
import type { HostElement } from "../index.js"
import { IntersectionObserver } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const oracle = JSON.parse(
  fs.readFileSync(
    path.resolve(
      path.dirname(fileURLToPath(import.meta.url)),
      "../../../react/src/__tests__/fixtures/display-contents-oracle.json"
    ),
    "utf8"
  )
)

const list = { width: 400, height: 200 } as const
const contents = { display: "contents" } as const
const tall = (height: number) => ({ height, flexShrink: 0 }) as const

function setup(view: () => any) {
  const app = createTestRoot()
  app.render(view)
  const renderer = app.renderer
  const id = (testId: string) => renderer.findByTestId(testId)!.id
  const bounds = (testId: string) => renderer.getElementBounds(id(testId))
  const focused = () => renderer.getElement(renderer.getFocusedElementId()!)?.testId ?? null
  return { app, renderer, id, bounds, focused }
}

function stacked(rows: number, overrides: (row: number) => Record<string, unknown> = () => ({})) {
  return () => (
    <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
      {Array.from({ length: rows }, (_, row) => (
        <div testId={`row-${row}`} style={contents} {...overrides(row)}>
          <div testId={`row-${row}-a`} tabIndex={0} style={tall(20)} />
          <div testId={`row-${row}-b`} tabIndex={0} style={tall(20)} />
        </div>
      ))}
    </virtual-list>
  )
}

describe.skipIf(!hasNativeTestRenderer)("Solid virtual-list row: display contents", () => {
  describe("listeners", () => {
    it("hears a click on a child, capture before bubble, with currentTarget the row and target the child", () => {
      const seen: Array<[string, number, number]> = []
      const record = (name: string) => (event: any) => seen.push([name, event.currentTarget.id, event.target.id])
      const { renderer, id } = setup(
        stacked(3, (row) => (row === 1 ? { onClick: record("bubble"), onClickCapture: record("capture") } : {}))
      )

      renderer.nativeSimulateClick(10, 50)

      expect(seen).toEqual([
        ["capture", id("row-1"), id("row-1-a")],
        ["bubble", id("row-1"), id("row-1-a")],
      ])
    })

    it("hears pointer, mouse and focus events of its children and not of another row's", () => {
      const seen: string[] = []
      const record = (name: string) => () => seen.push(name)
      const { renderer } = setup(
        stacked(3, (row) =>
          row === 0
            ? {
                onPointerDown: record("pointerdown"),
                onMouseDown: record("mousedown"),
                onFocusIn: record("focusin"),
                onFocusOut: record("focusout"),
                onKeyDown: record("keydown"),
              }
            : {}
        )
      )

      renderer.nativeSimulateClick(10, 70)
      expect(seen).toEqual([])
      renderer.nativeSimulateClick(10, 10)
      renderer.simulateKeystrokes("x")
      renderer.nativeSimulateClick(10, 70)

      expect(seen).toEqual(["pointerdown", "mousedown", "focusin", "keydown", "focusout"])
    })

    it("gets one enter for any child and one leave when the pointer leaves them all", () => {
      const seen: string[] = []
      const record = (name: string) => () => seen.push(name)
      const { renderer } = setup(
        stacked(2, (row) =>
          row === 0 ? { onPointerEnter: record("pointerenter"), onPointerLeave: record("pointerleave") } : {}
        )
      )

      renderer.nativeSimulateMouseMove(10, 10)
      renderer.nativeSimulateMouseMove(10, 30)
      expect(seen).toEqual(["pointerenter"])
      renderer.nativeSimulateMouseMove(10, 50)
      expect(seen).toEqual(["pointerenter", "pointerleave"])
    })

    it("is the target of click() on itself", () => {
      const seen: Array<[number, number]> = []
      let row!: HostElement
      const { renderer, id } = setup(() => (
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          <div
            ref={(host) => (row = host)}
            testId="row"
            style={contents}
            onClick={(event) => seen.push([event.currentTarget.id, event.target.id])}
          >
            <div style={{ height: 20 }} />
          </div>
        </virtual-list>
      ))

      row.click()
      renderer.dispatchNativeEvents()

      expect(seen).toEqual([[id("row"), id("row")]])
    })
  })

  describe("measuring", () => {
    it("measures a row as its children's block extent and ignores the row's own box styles", () => {
      const { bounds } = setup(() => (
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          <For each={[0, 1, 2]}>
            {(row) => (
              <div
                testId={`row-${row}`}
                style={{ display: "contents", height: 500, padding: 30, margin: 30, borderWidth: 9, borderStyle: "solid" }}
              >
                <div testId={`row-${row}-a`} style={tall(30)} />
                <div testId={`row-${row}-b`} style={tall(25)} />
              </div>
            )}
          </For>
        </virtual-list>
      ))

      expect(bounds("row-0-a")).toMatchObject({ x: 0, y: 0, width: 400, height: 30 })
      expect(bounds("row-0-b")).toMatchObject({ y: 30, height: 25 })
      expect(bounds("row-1-a")).toMatchObject({ y: 55 })
      expect(bounds("row-2-a")).toMatchObject({ y: 110 })
    })

    it("gives an empty row no height", () => {
      const { bounds } = setup(() => (
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          <div style={contents}>
            <div testId="a" style={tall(30)} />
          </div>
          <div style={contents} />
          <div style={contents}>
            <div testId="b" style={tall(30)} />
          </div>
        </virtual-list>
      ))

      expect(bounds("a")).toMatchObject({ y: 0 })
      expect(bounds("b")).toMatchObject({ y: 30 })
    })

    it("lets the list scroll through and past empty rows", () => {
      const { renderer, bounds } = setup(() => (
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          {Array.from({ length: 60 }, (_, row) =>
            row % 3 === 1 ? (
              <div style={contents} />
            ) : (
              <div style={contents}>
                <div testId={`row-${row}`} style={tall(30)} />
              </div>
            )
          )}
        </virtual-list>
      ))
      const listId = renderer.findByType("virtual-list")[0]!.id

      renderer.scrollToItem(listId, 30)
      expect(bounds("row-30")).toMatchObject({ y: 0 })
      renderer.scrollToItem(listId, 59)
      const last = bounds("row-59")!
      expect(last.y + last.height).toBe(200)
    })

    it("lays a row out again when it changes between contents and block", () => {
      const [display, setDisplay] = createSignal<"contents" | "block">("block")
      const { app, bounds } = setup(() => (
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          <div testId="row-0" style={{ display: display(), padding: 10 }}>
            <div testId="row-0-a" style={tall(20)} />
          </div>
          <div testId="row-1" style={{ padding: 10 }} />
        </virtual-list>
      ))
      expect(bounds("row-0")).toMatchObject({ y: 0, height: 40 })
      expect(bounds("row-0-a")).toMatchObject({ x: 10, y: 10 })
      expect(bounds("row-1")).toMatchObject({ y: 40 })

      app.flushSync(() => setDisplay("contents"))
      expect(bounds("row-0")).toBeNull()
      expect(bounds("row-0-a")).toMatchObject({ x: 0, y: 0, width: 400, height: 20 })
      expect(bounds("row-1")).toMatchObject({ y: 20 })

      app.flushSync(() => setDisplay("block"))
      expect(bounds("row-0")).toMatchObject({ y: 0, height: 40 })
      expect(bounds("row-0-a")).toMatchObject({ x: 10, y: 10 })
      expect(bounds("row-1")).toMatchObject({ y: 40 })
    })

    it("follows keyed reorders", () => {
      const [order, setOrder] = createSignal([0, 1, 2])
      const { app, bounds } = setup(() => (
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          <For each={order()}>
            {(row) => (
              <div testId={`row-${row}`} style={contents}>
                <div testId={`row-${row}-a`} style={tall(10 * (row + 1))} />
              </div>
            )}
          </For>
        </virtual-list>
      ))
      expect(bounds("row-0-a")).toMatchObject({ y: 0, height: 10 })
      expect(bounds("row-1-a")).toMatchObject({ y: 10, height: 20 })
      expect(bounds("row-2-a")).toMatchObject({ y: 30, height: 30 })

      app.flushSync(() => setOrder([2, 0, 1]))
      expect(bounds("row-2-a")).toMatchObject({ y: 0 })
      expect(bounds("row-0-a")).toMatchObject({ y: 30 })
      expect(bounds("row-1-a")).toMatchObject({ y: 40 })
    })
  })

  describe("reads", () => {
    it("answers a zero rect for getBoundingClientRect, as Chrome does for display: contents", () => {
      let row!: HostElement
      setup(() => (
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          <div ref={(host) => (row = host)} style={contents}>
            <div style={tall(20)} />
          </div>
        </virtual-list>
      ))

      const [x, y, width, height] = oracle.rect
      expect(row.getBoundingClientRect()).toEqual({ x, y, width, height, top: y, left: x, right: x + width, bottom: y + height })
    })

    it("is not in getElementsAt", () => {
      const { renderer, id } = setup(stacked(3))

      const hits = renderer.getElementsAt(10, 50)
      expect(hits[0]).toBe(id("row-1-a"))
      expect(hits).not.toContain(id("row-1"))
    })

    it("never intersects, with a zero rect, in an IntersectionObserver", () => {
      let row!: HostElement
      const entries: Array<{ isIntersecting: boolean; intersectionRatio: number; boundingClientRect: any }> = []
      const { renderer } = setup(() => (
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          <div ref={(host) => (row = host)} style={contents}>
            <div style={tall(20)} />
          </div>
        </virtual-list>
      ))
      new IntersectionObserver((batch) => entries.push(...batch)).observe(row)
      renderer.dispatchNativeEvents()

      const [x, y, width, height] = oracle.io[0].rect
      expect(entries).toHaveLength(1)
      expect(entries[0]).toMatchObject({
        isIntersecting: oracle.io[0].isIntersecting,
        intersectionRatio: oracle.io[0].ratio,
        boundingClientRect: { x, y, width, height },
      })
    })

    it("keeps its role and name in the accessibility tree around its children, as Chrome does", () => {
      const { renderer } = setup(() => (
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          <div style={contents} role="group" aria-label="row">
            <div role="button" aria-label="row-a" style={tall(20)} />
            <div role="button" aria-label="row-b" style={tall(20)} />
          </div>
        </virtual-list>
      ))

      const nodes = renderer.getA11yTree().nodes ?? {}
      const named = (label: string) => Object.entries(nodes).find(([, node]) => node.aria?.label === label)!
      expect(oracle.ax).toEqual(expect.arrayContaining(["row", "row-a", "row-b"]))
      const [, group] = named("row")
      expect(group.aria?.role).toBe("Group")
      expect(group.children).toEqual([named("row-a")[0], named("row-b")[0]])
    })

    it("hides its subtree from the accessibility tree with aria-hidden", () => {
      const { renderer } = setup(() => (
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          <div style={contents} aria-hidden="true">
            <div role="button" aria-label="hidden-child" style={tall(20)} />
          </div>
          <div style={contents}>
            <div role="button" aria-label="shown-child" style={tall(20)} />
          </div>
        </virtual-list>
      ))

      const nodes = Object.values(renderer.getA11yTree().nodes ?? {})
      const named = (label: string) => nodes.find((node) => node.aria?.label === label)!
      expect(named("hidden-child").aria?.hidden).toBe(true)
      expect(named("shown-child").aria?.hidden).toBeFalsy()
      expect(oracle.ax).not.toContain("aria-hidden-child")
    })
  })

  describe("focus", () => {
    it("tabs through the children of contents rows in order and skips a row that has tabIndex itself", () => {
      const { renderer, focused } = setup(stacked(3, (row) => (row === 1 ? { tabIndex: 0 } : {})))

      const walked: Array<string | null> = []
      for (let press = 0; press < 4; press++) {
        renderer.simulateKeystrokes("Tab")
        walked.push(focused())
      }
      expect(walked).toEqual(["row-0-a", "row-0-b", "row-1-a", "row-1-b"])
      renderer.simulateKeystrokes("Shift+Tab")
      expect(focused()).toBe("row-1-a")
    })

    it("cannot be focused itself", () => {
      const { renderer, id, focused } = setup(stacked(2, (row) => (row === 0 ? { tabIndex: 0 } : {})))

      renderer.focusElement(id("row-0"))

      expect(focused()).toBe(null)
      expect(oracle.rowFocusable).toBe(false)
    })

    it("scrolls a focused child's row into view", () => {
      const { renderer, id, bounds } = setup(stacked(50))

      renderer.focusElement(id("row-40-a"))

      expect(bounds("row-40-a")).not.toBeNull()
      expect(renderer.getListScrollTop(renderer.findByType("virtual-list")[0]!.id)![0]).toBeGreaterThan(30)
    })

    it("keeps the focus of a child when its row switches between contents and block", () => {
      const [display, setDisplay] = createSignal<"contents" | "block">("block")
      const { app, renderer, id, focused } = setup(() => (
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          <div style={{ display: display() }}>
            <div testId="child" tabIndex={0} style={tall(20)} />
          </div>
        </virtual-list>
      ))
      renderer.focusElement(id("child"))
      expect(focused()).toBe("child")

      app.flushSync(() => setDisplay("contents"))
      expect(focused()).toBe("child")
      app.flushSync(() => setDisplay("block"))
      expect(focused()).toBe("child")
    })
  })

  describe("what the row passes down", () => {
    it("is skipped, with its subtree, by inert", () => {
      const seen: string[] = []
      const { renderer, focused } = setup(() => (
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          <div style={contents} inert>
            <div tabIndex={0} onClick={() => seen.push("click")} style={tall(20)} />
          </div>
        </virtual-list>
      ))

      renderer.nativeSimulateClick(10, 10)
      renderer.simulateKeystrokes("Tab")

      expect(seen).toEqual([])
      expect(focused()).toBe(null)
    })

    it("hands pointerEvents: none to its children, so a press goes through them", () => {
      const seen: string[] = []
      const { renderer, id } = setup(() => (
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list} onClick={() => seen.push("list")}>
          <div style={{ display: "contents", pointerEvents: "none" }}>
            <div testId="child" onClick={() => seen.push("child")} style={tall(20)} />
          </div>
        </virtual-list>
      ))

      renderer.nativeSimulateClick(10, 10)

      expect(seen).toEqual(["list"])
      expect(renderer.getElementsAt(10, 10)).not.toContain(id("child"))
    })

    it("splices a nested display: contents element into the row", () => {
      const { bounds } = setup(() => (
        <virtual-list overdraw={0} estimatedItemHeight={40} style={list}>
          <div style={contents}>
            <div style={contents}>
              <div testId="a" style={tall(15)} />
              <div style={contents}>
                <div testId="b" style={tall(15)} />
              </div>
            </div>
          </div>
          <div testId="next" style={tall(10)} />
        </virtual-list>
      ))

      expect(bounds("a")).toMatchObject({ y: 0 })
      expect(bounds("b")).toMatchObject({ y: 15 })
      expect(bounds("next")).toMatchObject({ y: 30 })
    })
  })
})

describe.skipIf(!hasNativeTestRenderer)("Solid display: contents without a box", () => {
  it("answers a zero rect from getBoundingClientRect, as the root and spliced into a parent", () => {
    let root!: HostElement
    let spliced!: HostElement
    setup(() => (
      <div ref={(host) => (root = host)} style={{ display: "contents" }}>
        <div ref={(host) => (spliced = host)} style={{ display: "contents" }}>
          <div style={{ width: 50, height: 50 }} />
        </div>
      </div>
    ))

    const [x, y, width, height] = oracle.rect
    const zero = { x, y, width, height, top: y, left: x, right: x + width, bottom: y + height }
    expect(root.getBoundingClientRect()).toEqual(zero)
    expect(spliced.getBoundingClientRect()).toEqual(zero)
  })

  it("cannot be focused, takes no Tab stop, and leaves its children their own", () => {
    const { renderer, id, focused } = setup(() => (
      <div style={{ width: 200, height: 100 }}>
        <div testId="contents" tabIndex={0} style={{ display: "contents" }}>
          <div testId="a" tabIndex={0} style={{ width: 50, height: 20 }} />
        </div>
        <div testId="b" tabIndex={0} style={{ width: 50, height: 20 }} />
      </div>
    ))

    renderer.focusElement(id("contents"))
    expect(focused()).toBe(null)

    renderer.simulateKeystrokes("Tab")
    expect(focused()).toBe("a")
    renderer.simulateKeystrokes("Tab")
    expect(focused()).toBe("b")
  })

  it("can be focused again once it stops being display: contents, and blurs when it becomes one", () => {
    const [display, setDisplay] = createSignal<"contents" | "block">("contents")
    const { app, renderer, id, focused } = setup(() => (
      <div style={{ width: 200, height: 100 }}>
        <div testId="element" tabIndex={0} style={{ display: display(), height: 20 }} />
      </div>
    ))
    renderer.focusElement(id("element"))
    expect(focused()).toBe(null)

    app.flushSync(() => setDisplay("block"))
    renderer.focusElement(id("element"))
    expect(focused()).toBe("element")

    app.flushSync(() => setDisplay("contents"))
    renderer.flush()
    expect(focused()).toBe(null)
  })
})
