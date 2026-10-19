/**
 * A bounds read after a change that cannot move a box runs no layout pass, as Chrome runs no
 * layout for a transform or an opacity change: the window is dirty, but the boxes published by
 * the last layout are still the boxes. Only a motion value write that drives paint-only
 * properties (`AnimProp::paints_only`) is known not to move one; every other invalidation, and
 * every doubtful write, runs the pass as before.
 *
 * Inline draws are off so that a dirty window stays dirty until `drawFrame`, which is what lets
 * `getLayoutPassCount` see the passes.
 */

import { useState } from "react"
import { describe, expect, it } from "vitest"
import { motion, motionValue, useTransform } from "../index"
import type { MotionValue } from "../index"
import { flushSync } from "../reconciler/reconciler"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Setup = {
  opacity: MotionValue<number>
  color: MotionValue<string>
  width: MotionValue<number>
  left: MotionValue<number>
  source: MotionValue<number>
  setHeight: (height: number) => void
}

function Page(props: { setup: Setup }) {
  const setup = props.setup
  const [height, setHeight] = useState(40)
  setup.setHeight = setHeight
  const derivedWidth = useTransform(setup.source, [0, 1], [30, 130])
  return (
    <div testId="stage" style={{ width: 600, height: 400, flexDirection: "column" }}>
      <motion.div
        testId="paint"
        style={{ width: 80, height, opacity: setup.opacity, backgroundColor: setup.color }}
      />
      <motion.div testId="sized" style={{ width: setup.width, height: 20 }} />
      <motion.div testId="derived" style={{ width: derivedWidth, height: 20 }} />
      <div testId="positioned" style={{ position: "relative", width: 100, height: 30 }}>
        <motion.div testId="moved" style={{ position: "absolute", top: 0, left: setup.left, width: 10, height: 10 }} />
      </div>
      <div
        testId="hoverable"
        style={{ width: 50, height: 30, hover: { width: 120 } }}
      />
    </div>
  )
}

function createSetup() {
  const setup: Setup = {
    opacity: motionValue(1),
    color: motionValue("#ff0000"),
    width: motionValue(60),
    left: motionValue(0),
    source: motionValue(0),
    setHeight: () => {},
  }
  return setup
}

function mount() {
  const setup = createSetup()
  const root = createTestRoot({ width: 640, height: 440 })
  const renderer = root.renderer
  renderer.clockPause()
  renderer.setInlineDraws(false)
  root.render(<Page setup={setup} />)
  renderer.flush()
  // The first reads after a commit do their pass; a frame ends them.
  const ids = Object.fromEntries(
    ["paint", "sized", "derived", "moved", "positioned", "hoverable"].map((testId) => [testId, renderer.findByTestId(testId)!.id])
  )
  renderer.getElementBounds(ids.paint!)
  renderer.drawFrame()
  const passes = () => renderer.getLayoutPassCount()
  const rect = (testId: string) => renderer.getElementBounds(ids[testId]!)
  return { root, renderer, setup, ids, passes, rect }
}

describeNative("a bounds read after a change that moves no box", { timeout: 30_000 }, () => {
  it("runs no pass on a clean window", () => {
    const { root, renderer, passes, rect } = mount()
    try {
      const start = passes()
      const before = rect("paint")
      expect(rect("paint")).toEqual(before)
      expect(passes() - start).toBe(0)
    } finally {
      renderer.setInlineDraws(true)
      root.unmount()
    }
  })

  it("runs no pass after an opacity or a colour write, and the frame still draws", () => {
    const { root, renderer, setup, passes, rect } = mount()
    try {
      const before = rect("paint")
      const start = passes()
      const built = renderer.getBuildCounters().elementsBuilt
      const painted = renderer.getPaintedFrameCount()

      setup.opacity.set(0.4)
      expect(rect("paint"), "after an opacity write").toEqual(before)
      setup.color.set("#0000ff")
      expect(rect("paint"), "after a colour write").toEqual(before)
      expect(passes() - start, "no layout pass").toBe(0)
      expect(renderer.getBuildCounters().elementsBuilt, "nothing built for the reads").toBe(built)
      expect(renderer.getPaintedFrameCount(), "nothing painted for the reads").toBe(painted)

      renderer.drawFrame()
      expect(renderer.getPaintedFrameCount() - painted, "the frame draws once").toBe(1)
      expect(rect("paint")).toEqual(before)
      expect(passes() - start, "and is clean after it").toBe(0)
    } finally {
      renderer.setInlineDraws(true)
      root.unmount()
    }
  })

  it("runs the pass for a write that drives a width, and reads the new width", () => {
    const { root, renderer, setup, passes, rect } = mount()
    try {
      const start = passes()
      setup.width.set(90)
      expect(rect("sized").width).toBe(90)
      expect(rect("sized").width).toBe(90)
      expect(passes() - start, "one pass for both reads").toBe(1)
    } finally {
      renderer.setInlineDraws(true)
      root.unmount()
    }
  })

  it("runs the pass for a write that drives an inset or, through a transform, a width", () => {
    const { root, renderer, setup, passes, rect } = mount()
    try {
      let start = passes()
      setup.left.set(25)
      expect(rect("moved").x - rect("positioned").x).toBe(25)
      expect(passes() - start, "left moves a box").toBe(1)

      renderer.drawFrame()
      start = passes()
      setup.source.set(0.5)
      expect(rect("derived").width, "the value the width is computed from").toBe(80)
      expect(passes() - start).toBe(1)
    } finally {
      renderer.setInlineDraws(true)
      root.unmount()
    }
  })

  it("runs the pass for a JS commit that moves a box after a paint-only write", () => {
    const { root, renderer, setup, passes, rect } = mount()
    try {
      const start = passes()
      setup.opacity.set(0.5)
      flushSync(() => setup.setHeight(70))
      renderer.flush()
      expect(rect("paint").height).toBe(70)
      expect(passes() - start).toBe(1)
    } finally {
      renderer.setInlineDraws(true)
      root.unmount()
    }
  })

  it("runs the pass for a paint-only write that came before a layout write", () => {
    const { root, renderer, setup, passes, rect } = mount()
    try {
      const start = passes()
      setup.opacity.set(0.5)
      setup.width.set(110)
      expect(rect("sized").width).toBe(110)
      expect(passes() - start).toBe(1)
    } finally {
      renderer.setInlineDraws(true)
      root.unmount()
    }
  })

  it("runs the pass for a hover that moves a box after a paint-only write", () => {
    const { root, renderer, setup, passes, rect } = mount()
    try {
      expect(rect("hoverable").width).toBe(50)
      const start = passes()
      setup.opacity.set(0.5)
      const hoverable = rect("hoverable")
      renderer.dispatchMouseMove(hoverable.x + 5, hoverable.y + 5)
      expect(rect("hoverable").width).toBe(120)
      expect(passes() - start).toBe(1)
    } finally {
      renderer.setInlineDraws(true)
      root.unmount()
    }
  })

  it("runs the pass after a frame and a later layout change, as it did", () => {
    const { root, renderer, setup, passes, rect } = mount()
    try {
      setup.opacity.set(0.5)
      renderer.drawFrame()
      const start = passes()
      expect(rect("paint").height).toBe(40)
      expect(passes() - start, "the frame drew the write").toBe(0)
      flushSync(() => setup.setHeight(55))
      renderer.flush()
      expect(rect("paint").height).toBe(55)
      expect(passes() - start).toBe(1)
    } finally {
      renderer.setInlineDraws(true)
      root.unmount()
    }
  })
})
