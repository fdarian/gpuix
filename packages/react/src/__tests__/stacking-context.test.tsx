/// `zIndex` orders a positioned element in the nearest stacking context, whatever static boxes sit
/// between, as CSS does. Every test drives the real paint and the real hit test through the GPU
/// test renderer; `getElementsAt` has to list what the click went to first.

import type { CSSProperties, ReactNode } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("zIndex across stacking contexts", () => {
  function setup() {
    const root = createTestRoot()
    const clicks: string[] = []
    const on = (name: string) => () => clicks.push(name)
    const nameOf = (id: number) => root.renderer.getElement(id)?.testId
    /** The test ids under the point, topmost first. */
    const hitsAt = (x: number, y: number) =>
      root.renderer
        .getElementsAt(x, y)
        .map(nameOf)
        .filter((name): name is string => name !== undefined)
    /** Clicks, and says which element took it: its own handler runs before any ancestor's. */
    const clickAt = (x: number, y: number) => {
      clicks.length = 0
      root.renderer.nativeSimulateClick(x, y)
      return clicks[0]
    }
    /** The labels the boxes drew, in paint order. */
    const painted = () => {
      root.renderer.flush()
      return root.renderer.getPaintedText()
    }
    /** Starts from the real hit test and checks the listing names the same element first. */
    const agree = (x: number, y: number, expected: string) => {
      expect(clickAt(x, y), `the click at ${x}, ${y}`).toBe(expected)
      expect(hitsAt(x, y)[0], `getElementsAt at ${x}, ${y}`).toBe(expected)
    }
    return { ...root, on, hitsAt, clickAt, painted, agree }
  }

  type Handlers = ReturnType<typeof setup>["on"]

  /** A 100 box that listens for clicks and paints its test id. */
  const box = (on: Handlers, testId: string, style: CSSProperties = {}, children?: ReactNode) => (
    <div testId={testId} onClick={on(testId)} style={{ width: 100, height: 100, ...style }}>
      {testId}
      {children}
    </div>
  )

  const at = (left: number, top: number, extra: CSSProperties = {}): CSSProperties => ({
    position: "absolute",
    left,
    top,
    ...extra,
  })

  it("paints and hits a z-indexed descendant of an earlier sibling above a later sibling", () => {
    const { render, renderer, on, agree, painted, hitsAt, unmount } = setup()
    try {
      render(
        <div testId="root" style={{ width: 400, height: 400 }}>
          <div testId="a" onClick={on("a")} style={{ width: 200, height: 200 }}>
            {box(on, "b", at(120, 120, { zIndex: 5 }))}
          </div>
          <div testId="c" onClick={on("c")} style={{ width: 200, height: 200, marginTop: -100, marginLeft: 100 }}>
            c
          </div>
        </div>
      )
      renderer.dispatchNativeEvents()

      // b and c overlap from 120 to 220; c is in a different subtree and comes later.
      agree(150, 150, "b")
      agree(250, 250, "c")
      agree(50, 50, "a")
      expect(hitsAt(150, 150)).toEqual(["b", "c", "a", "root"])
      expect(painted().filter((label) => label === "b" || label === "c")).toEqual(["c", "b"])
    } finally {
      unmount()
    }
  })

  it("keeps a static box with a zIndex in tree order", () => {
    const { render, renderer, on, agree, unmount } = setup()
    try {
      render(
        <div style={{ width: 400, height: 400 }}>
          {box(on, "b", { zIndex: 5 })}
          {box(on, "c", { marginTop: -100, marginLeft: 50 })}
        </div>
      )
      renderer.dispatchNativeEvents()

      agree(75, 50, "c")
    } finally {
      unmount()
    }
  })

  describe("an element composited as a whole is a context of its own", () => {
    /** The z=100 `inner` sits in `group`, the later `later` has z=2 outside it. */
    function scene(on: Handlers, group: CSSProperties) {
      return (
        <div testId="root" style={{ width: 400, height: 400 }}>
          <div testId="group" style={{ width: 200, height: 200, ...group }}>
            {box(on, "inner", at(50, 50, { zIndex: 100 }))}
          </div>
          {box(on, "later", at(100, 100, { zIndex: 2, width: 200, height: 200 }))}
        </div>
      )
    }

    it.each<[string, CSSProperties]>([
      ["opacity below 1", { opacity: 0.5 }],
      ["a filter", { filter: "grayscale(0.5)" }],
      ["a backdrop filter", { backdropFilter: "blur(2px)" }],
      ["a mask image", { maskImage: "linear-gradient(black, black)" }],
      ["a clip path", { clipPath: "inset(0px)" }],
      ["position: fixed", { position: "fixed", left: 0, top: 0 }],
      ["position: sticky", { position: "sticky", top: 0 }],
      ["a zIndex on a positioned box", { position: "relative", zIndex: 1 }],
    ])("traps a zIndex of 100 inside %s", (_name, group) => {
      const { render, renderer, on, agree, hitsAt, unmount } = setup()
      try {
        render(scene(on, group))
        renderer.dispatchNativeEvents()

        // inner covers 50 to 150, later 100 to 300: they overlap at 120.
        agree(120, 120, "later")
        expect(hitsAt(120, 120).slice(0, 2)).toEqual(["later", "inner"])
        agree(70, 70, "inner")
      } finally {
        unmount()
      }
    })

    it.each<[string, CSSProperties]>([
      ["opacity 1", { opacity: 1 }],
      ["no filter", { filter: "none" }],
      ["a filter that is not valid", { filter: "bogus(1)" }],
      ["a positioned box with zIndex auto", { position: "relative" }],
      ["a zIndex on a static box", { zIndex: 1 }],
    ])("lets a zIndex of 100 out of %s", (_name, group) => {
      const { render, renderer, on, agree, hitsAt, unmount } = setup()
      try {
        render(scene(on, group))
        renderer.dispatchNativeEvents()

        agree(120, 120, "inner")
        expect(hitsAt(120, 120).slice(0, 2)).toEqual(["inner", "later"])
      } finally {
        unmount()
      }
    })

    it("paints the trapped element with its group", () => {
      const { render, renderer, on, painted, unmount } = setup()
      try {
        render(scene(on, { opacity: 0.5 }))
        renderer.dispatchNativeEvents()

        expect(painted().filter((label) => label === "inner" || label === "later")).toEqual(["inner", "later"])
      } finally {
        unmount()
      }
    })
  })

  it("keeps tree order across parents at an equal zIndex, and zIndex 0 is auto", () => {
    const { render, renderer, on, agree, unmount } = setup()
    try {
      render(
        <div style={{ width: 400, height: 400 }}>
          <div style={{ width: 10, height: 10 }}>
            {box(on, "early", at(0, 0, { zIndex: 1 }))}
            {box(on, "auto-early", at(150, 0))}
            {box(on, "zero-early", at(0, 150, { zIndex: 0 }))}
          </div>
          <div style={{ width: 10, height: 10 }}>
            {box(on, "late", at(50, 50, { zIndex: 1 }))}
            {box(on, "zero-late", at(150, 50, { zIndex: 0 }))}
            {box(on, "auto-late", at(50, 150))}
          </div>
        </div>
      )
      renderer.dispatchNativeEvents()

      agree(75, 75, "late")
      // Auto and 0 are the same level, so the later one is above.
      agree(175, 75, "zero-late")
      agree(75, 175, "auto-late")
    } finally {
      unmount()
    }
  })

  it("orders a higher zIndex in the earlier parent above the later parent's", () => {
    const { render, renderer, on, agree, unmount } = setup()
    try {
      render(
        <div style={{ width: 400, height: 400 }}>
          <div style={{ width: 10, height: 10 }}>{box(on, "high", at(0, 0, { zIndex: 3 }))}</div>
          <div style={{ width: 10, height: 10 }}>{box(on, "low", at(50, 50, { zIndex: 1 }))}</div>
        </div>
      )
      renderer.dispatchNativeEvents()

      agree(75, 75, "high")
    } finally {
      unmount()
    }
  })

  it("puts a negative zIndex above the background of its context and below its in-flow content", () => {
    const { render, renderer, on, agree, hitsAt, painted, unmount } = setup()
    try {
      render(
        <div testId="root" onClick={on("root")} style={{ width: 400, height: 400, backgroundColor: "#202020" }}>
          {box(on, "flow", { backgroundColor: "#ff0000" })}
          {box(on, "behind", at(50, 50, { zIndex: -1 }))}
        </div>
      )
      renderer.dispatchNativeEvents()

      agree(75, 75, "flow")
      agree(125, 125, "behind")
      expect(hitsAt(75, 75)).toEqual(["flow", "behind", "root"])
      expect(painted().filter((label) => label === "flow" || label === "behind")).toEqual(["behind", "flow"])
    } finally {
      unmount()
    }
  })

  it("keeps a negative zIndex above the background of a context that is not the root", () => {
    const { render, renderer, on, agree, hitsAt, unmount } = setup()
    try {
      render(
        <div testId="root" style={{ width: 400, height: 400 }}>
          {box(on, "ctx", { position: "relative", zIndex: 1, width: 200, height: 200, backgroundColor: "#202020" }, [
            <div key="flow" testId="flow" onClick={on("flow")} style={{ width: 100, height: 100 }} />,
            <div key="behind" testId="behind" onClick={on("behind")} style={at(50, 50, { zIndex: -1, width: 100, height: 100 })} />,
          ])}
          {box(on, "next", { width: 200, height: 200, marginTop: -100 })}
        </div>
      )
      renderer.dispatchNativeEvents()

      // ctx is lifted above the later, in-flow sibling, so its negative child is too.
      expect(hitsAt(75, 75).slice(0, 3)).toEqual(["flow", "behind", "ctx"])
      agree(125, 125, "behind")
    } finally {
      unmount()
    }
  })

  it("lifts a z-indexed element out of a positioned parent whose zIndex is auto", () => {
    const { render, renderer, on, agree, hitsAt, unmount } = setup()
    try {
      const tree = (parent: CSSProperties) => (
        <div testId="root" style={{ width: 400, height: 400 }}>
          <div testId="parent" style={{ position: "relative", width: 200, height: 200, ...parent }}>
            {box(on, "deep", at(50, 50, { zIndex: 5 }))}
          </div>
          {box(on, "mid", at(100, 100, { zIndex: 2, width: 200, height: 200 }))}
        </div>
      )
      render(tree({}))
      renderer.dispatchNativeEvents()
      agree(120, 120, "deep")
      expect(hitsAt(120, 120).slice(0, 2)).toEqual(["deep", "mid"])

      // With a zIndex of its own the parent traps it, and its 1 is below mid's 2.
      render(tree({ zIndex: 1 }))
      renderer.dispatchNativeEvents()
      agree(120, 120, "mid")
    } finally {
      unmount()
    }
  })

  it("orders z-indexed elements inside an <anchored> layer", () => {
    const { render, renderer, on, agree, hitsAt, unmount } = setup()
    try {
      render(
        <div testId="root" style={{ width: 600, height: 600 }}>
          <anchored position={{ x: 200, y: 200 }} anchor="topLeft" deferred priority={1}>
            <div testId="pop" style={{ position: "relative", width: 150, height: 150 }}>
              <div testId="early" style={{ width: 100, height: 100 }}>
                {box(on, "deep", at(50, 50, { zIndex: 3, width: 80, height: 80 }))}
              </div>
              {box(on, "late", { marginTop: -50, marginLeft: 50 })}
            </div>
          </anchored>
          {box(on, "over", at(0, 0, { zIndex: 99, width: 100, height: 100 }))}
        </div>
      )
      renderer.dispatchNativeEvents()

      // deep and late overlap from 250 to 330, deep is a descendant of the earlier sibling.
      agree(280, 280, "deep")
      expect(hitsAt(280, 280).slice(0, 2)).toEqual(["deep", "late"])
      agree(50, 50, "over")
    } finally {
      unmount()
    }
  })

  it("orders z-indexed elements inside a virtual-list row, and stops them at the row", () => {
    const { render, renderer, on, agree, hitsAt, unmount } = setup()
    try {
      render(
        <virtual-list overdraw={0} estimatedItemHeight={100} style={{ width: 300, height: 300 }}>
          <div key={0} testId="row0" style={{ position: "relative", width: 300, height: 100, flexShrink: 0 }}>
            <div testId="early" style={{ width: 100, height: 60 }}>
              {box(on, "deep", at(20, 20, { zIndex: 3, width: 60, height: 60 }))}
              {box(on, "spill", at(150, 80, { zIndex: 9, width: 60, height: 60 }))}
            </div>
            {box(on, "late", { height: 60, marginTop: -30, marginLeft: 30 })}
          </div>
          <div key={1} testId="row1" onClick={on("row1")} style={{ width: 300, height: 100, flexShrink: 0 }}>
            row1
          </div>
        </virtual-list>
      )
      renderer.dispatchNativeEvents()

      // Inside the row deep is above late, which is later and in flow.
      agree(60, 60, "deep")
      expect(hitsAt(60, 60).slice(0, 2)).toEqual(["deep", "late"])
      // The row is a layout root: spill sticks out of it into row 1 and stays below it.
      agree(170, 110, "row1")
    } finally {
      unmount()
    }
  })

  it("keeps tab order in document order with a lifted element between two others", () => {
    const { render, renderer, unmount } = setup()
    try {
      render(
        <div>
          <div testId="a" tabIndex={0} style={{ width: 20, height: 20 }} />
          <div style={{ width: 40, height: 40 }}>
            <div testId="b" tabIndex={0} style={{ position: "relative", zIndex: 5, width: 20, height: 20 }} />
            <div testId="behind" tabIndex={0} style={{ position: "absolute", zIndex: -1, width: 20, height: 20 }} />
          </div>
          <div testId="c" tabIndex={0} style={{ width: 20, height: 20 }} />
        </div>
      )
      const focused = () => renderer.getElement(renderer.getFocusedElementId())?.testId
      const order: (string | undefined)[] = []
      for (let press = 0; press < 4; press++) {
        renderer.simulateKeystrokes("Tab")
        order.push(focused())
      }
      expect(order).toEqual(["a", "b", "behind", "c"])
      renderer.simulateKeystrokes("Shift+Tab")
      expect(focused()).toBe("behind")
    } finally {
      unmount()
    }
  })

  it("lifts a positioned <virtual-list>, which is not a div, above a later in-flow sibling", () => {
    const { render, renderer, on, painted, unmount } = setup()
    try {
      render(
        <div testId="root" style={{ width: 400, height: 400 }}>
          <div style={{ width: 200, height: 200 }}>
            <virtual-list style={at(0, 0, { width: 100, height: 100 })}>
              <div style={{ height: 100, flexShrink: 0 }}>listed</div>
            </virtual-list>
          </div>
          {box(on, "later", { width: 200, height: 200, marginTop: -200 })}
        </div>
      )
      renderer.dispatchNativeEvents()

      expect(painted().filter((label) => label === "listed" || label === "later")).toEqual(["later", "listed"])
    } finally {
      unmount()
    }
  })
})
