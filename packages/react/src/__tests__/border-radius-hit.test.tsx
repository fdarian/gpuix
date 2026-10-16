import type { ReactNode } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("a rounded box without overflow clipping misses the pointer at its corners", () => {
  /** Clicks (x, y) and returns the names of the elements that got the click, target first. */
  function clickAt(tree: (record: (name: string) => () => void) => ReactNode, x: number, y: number) {
    const { render, renderer, unmount } = createTestRoot()
    const log: string[] = []
    try {
      render(tree((name) => () => log.push(name)))
      renderer.dispatchNativeEvents()
      renderer.nativeSimulateClick(x, y)
      return log
    } finally {
      unmount()
    }
  }

  /** A 100x100 box at (50, 50) with `overflow` visible, above a sibling that fills the stage. */
  const scene = (record: (name: string) => () => void) => (
    <div style={{ position: "relative", width: 200, height: 200 }}>
      <div style={{ position: "absolute", width: 200, height: 200 }} onClick={record("under")} />
      <div
        style={{ position: "absolute", left: 50, top: 50, width: 100, height: 100, borderRadius: 40 }}
        onClick={record("rounded")}
      />
    </div>
  )

  it("a click at the rounded-away corner reaches what is underneath", () => {
    expect(clickAt(scene, 52, 52)).toEqual(["under"])
    expect(clickAt(scene, 148, 148)).toEqual(["under"])
  })

  it("a click inside the arc and along the straight edges hits the box", () => {
    expect(clickAt(scene, 70, 70)).toEqual(["rounded"])
    expect(clickAt(scene, 100, 52)).toEqual(["rounded"])
    expect(clickAt(scene, 52, 100)).toEqual(["rounded"])
  })

  it("a child spilling past the corner is hit through its own box and bubbles to the parent", () => {
    const withChild = (record: (name: string) => () => void) => (
      <div style={{ position: "relative", width: 200, height: 200 }}>
        <div style={{ position: "absolute", width: 200, height: 200 }} onClick={record("under")} />
        <div
          style={{ position: "absolute", left: 50, top: 50, width: 100, height: 100, borderRadius: 40 }}
          onClick={record("rounded")}
        >
          <div style={{ position: "absolute", left: -10, top: -10, width: 30, height: 30 }} onClick={record("spill")} />
        </div>
      </div>
    )
    expect(clickAt(withChild, 55, 55)).toEqual(["spill", "rounded"])
    expect(clickAt(withChild, 148, 52)).toEqual(["under"])
  })

  it("the corner is rounded per corner radius", () => {
    const perCorner = (record: (name: string) => () => void) => (
      <div style={{ position: "relative", width: 200, height: 200 }}>
        <div style={{ position: "absolute", width: 200, height: 200 }} onClick={record("under")} />
        <div
          style={{
            position: "absolute",
            left: 50,
            top: 50,
            width: 100,
            height: 100,
            borderTopLeftRadius: 40,
          }}
          onClick={record("rounded")}
        />
      </div>
    )
    expect(clickAt(perCorner, 52, 52)).toEqual(["under"])
    expect(clickAt(perCorner, 148, 52)).toEqual(["rounded"])
    expect(clickAt(perCorner, 148, 148)).toEqual(["rounded"])
  })
})
