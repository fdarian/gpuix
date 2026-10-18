/**
 * The boxes inside an atomic inline (an `inline-block` in a text) are laid out by a private layout
 * engine, not the window's. A layout-only pass (a bounds or scroll read on a dirty window) queues
 * the publishing of their bounds (`Interactivity::publish_layout`) and runs it after the tree is
 * prepainted, against the window's engine, with a layout id of the atomic inline's engine. A key
 * of one engine is a node of the other when both have a node there (the wrong insets, silently),
 * and an invalid key when the window's engine has no such node (`invalid SlotMap key used`, which
 * aborts the process).
 *
 * Run this file alone: the panic aborts the worker, and the tests after it do not report.
 */

import React from "react"
import { describe, expect, it } from "vitest"
import { render, resetRender } from "../reconciler/renderer"
import { hasNativeTestRenderer, TestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const TEXT = { fontSize: 16, lineHeight: "20px", fontFamily: "Menlo", color: "#000000" } as const

/** `rows` sizes the window's layout engine, `cells` the atomic inline's: the key of a cell is a
 *  live node of the window only while the window has at least as many nodes. */
function Page(props: { name: string; rows: number; cells: number; width?: number }) {
  const rows = Array.from({ length: props.rows }, (_, index) => (
    <div key={index} style={{ height: 24, padding: 3, borderWidth: 1, borderStyle: "solid", borderColor: "#888" }}>
      <div style={{ width: 30 + index, height: 10 }} />
    </div>
  ))
  const cells = Array.from({ length: props.cells }, (_, index) => (
    <div key={index} testId={index === 0 ? `${props.name}-atomic-child` : undefined} style={{ width: 2 + index, height: 1 }} />
  ))
  return (
    <div testId={`${props.name}-root`} style={{ width: props.width ?? 420, height: 600, flexDirection: "column" }}>
      <div testId={`${props.name}-scroller`} style={{ width: 300, height: 120, overflow: "scroll" }}>
        {rows}
      </div>
      <text style={{ ...TEXT, width: 360 }}>
        {"Before the box "}
        <div
          testId={`${props.name}-atomic`}
          style={{
            display: "inline-block",
            width: 90,
            height: 40,
            padding: 5,
            borderWidth: 3,
            borderStyle: "solid",
            borderColor: "#00f",
          }}
        >
          {cells}
        </div>
        {" and after it"}
      </text>
    </div>
  )
}

type Shape = { rows: number; cells: number }

const WINDOW_NODES_OUTNUMBER_CELLS: Shape = { rows: 40, cells: 2 }
const CELLS_OUTNUMBER_WINDOW_NODES: Shape = { rows: 2, cells: 40 }

function withWindow(run: (renderer: TestRenderer) => void) {
  const renderer = new TestRenderer()
  renderer.setInlineDraws(false)
  try {
    run(renderer)
  } finally {
    resetRender()
    renderer.dispose()
  }
}

function mount(renderer: TestRenderer, name: string, shape: Shape, width?: number) {
  render(<Page name={name} {...shape} width={width} />, { renderer })
  renderer.flush()
}

/** The way the demo remounts: `render` again on the same renderer unmounts the previous root. */
function presentAfterRemount(renderer: TestRenderer, shape: Shape, remount: boolean): string {
  mount(renderer, "first", shape)
  renderer.drawFrame()
  if (!remount) return "first"
  mount(renderer, "second", shape)
  renderer.drawFrame()
  return "second"
}

function boxes(renderer: TestRenderer, name: string) {
  const bounds = (suffix: string) => {
    const element = renderer.findByTestId(`${name}-${suffix}`)
    expect(element, `${name}-${suffix}`).toBeDefined()
    return renderer.getElementBounds(element!.id)
  }
  return { atomic: bounds("atomic"), atomicChild: bounds("atomic-child"), scroller: bounds("scroller") }
}

function scrollMetrics(renderer: TestRenderer, name: string) {
  return renderer.getScrollMetrics(renderer.findByTestId(`${name}-scroller`)!.id)
}

describeNative("a layout-only read around an atomic inline", () => {
  for (const remount of [false, true]) {
    const when = remount ? "after a remount and a presented frame" : "after a presented frame"

    it(`publishes the insets of an atomic inline as a painted frame does, ${when}`, { timeout: 60_000 }, () => {
      withWindow((renderer) => {
        const name = presentAfterRemount(renderer, WINDOW_NODES_OUTNUMBER_CELLS, remount)
        mount(renderer, name, WINDOW_NODES_OUTNUMBER_CELLS, 421)
        const passes = renderer.getLayoutPassCount()
        const painted = renderer.getPaintedFrameCount()
        const laidOut = boxes(renderer, name)
        expect(renderer.getLayoutPassCount() - passes, "the reads shared a layout-only pass").toBe(1)
        expect(renderer.getPaintedFrameCount(), "the reads painted nothing").toBe(painted)

        renderer.drawFrame()
        const drawn = boxes(renderer, name)
        expect(drawn.atomic.clientLeft, "the border of the atomic inline").toBe(3)
        expect(laidOut).toEqual(drawn)
      })
    })

    it(`does not abort on a scroll read of a window with fewer nodes than the atomic inline, ${when}`, { timeout: 60_000 }, () => {
      withWindow((renderer) => {
        const name = presentAfterRemount(renderer, CELLS_OUTNUMBER_WINDOW_NODES, remount)
        mount(renderer, name, CELLS_OUTNUMBER_WINDOW_NODES, 421)
        const laidOut = scrollMetrics(renderer, name)
        renderer.drawFrame()
        expect(laidOut).toEqual(scrollMetrics(renderer, name))
      })
    })

    it(`does not abort on a bounds read of a window with fewer nodes than the atomic inline, ${when}`, { timeout: 60_000 }, () => {
      withWindow((renderer) => {
        const name = presentAfterRemount(renderer, CELLS_OUTNUMBER_WINDOW_NODES, remount)
        mount(renderer, name, CELLS_OUTNUMBER_WINDOW_NODES, 421)
        const laidOut = boxes(renderer, name)
        renderer.drawFrame()
        expect(laidOut).toEqual(boxes(renderer, name))
      })
    })
  }
})
