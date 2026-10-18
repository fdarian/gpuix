// @ts-nocheck
/**
 * The parent content box a replayed root measured. A height that animates from a percentage to
 * `auto` starts from that percentage in px, a share of the content box of the parent that the
 * paint before recorded (`Measurements`). The content box changes while the root's own box and
 * key stay as they were: the page around a card gains padding below it.
 *
 * Run with GPUIX_RETAIN_VERIFY=1 too: gpui then compares what a replayed root wrote (the tape) with a build.
 */

import React from "react"
import { expect, it } from "vitest"
import { motion } from "../index"
import { describeNative, setup } from "./retained-harness.js"

const TIMEOUT = 30_000
const LINEAR = { duration: 1, ease: "linear" } as const
const PAGE_HEIGHT = 400
const PADDING = 200
/** The mover is as tall as this while its percentage is more, before and after the padding. */
const MAX_HEIGHT = 80
const NATURAL_HEIGHT = 40

describeNative("the parent content box of a replayed root", { timeout: TIMEOUT }, () => {
  function Block({ index }: { index: number }) {
    return (
      <div testId={`block-${index}`} style={{ flexShrink: 0, height: 30, backgroundColor: "#335577" }}>
        <div style={{ height: 10, backgroundColor: "#223344" }}>
          <text>{`block ${index}`}</text>
        </div>
      </div>
    )
  }

  /** A page `padding` taller on the inside than it is: the mover is a share of what is left. */
  function page(padding: number, height: string) {
    return (
      <div
        testId="page"
        style={{ display: "flex", flexDirection: "column", height: PAGE_HEIGHT, width: 300, paddingBottom: padding }}
      >
        <motion.div
          testId="mover"
          initial={{ height: "50%" }}
          animate={{ height }}
          transition={LINEAR}
          style={{ flexShrink: 0, maxHeight: MAX_HEIGHT, overflow: "hidden", backgroundColor: "#335577" }}
        >
          <div style={{ height: 20, backgroundColor: "#223344" }}>
            <text>mover</text>
          </div>
          <div style={{ height: 20, backgroundColor: "#445566" }} />
        </motion.div>
        {Array.from({ length: 5 }, (_, index) => (
          <Block key={index} index={index} />
        ))}
      </div>
    )
  }

  function heightHalfWayToAuto(retention: boolean) {
    const test = setup(page(0, "50%"))
    try {
      test.renderer.setRetainPlanning(retention)
      test.renderer.clockPause()
      test.settle()
      const replays = test.counters().replays
      const mover = test.bounds("mover")
      expect(mover.height).toBe(MAX_HEIGHT)

      test.render(page(PADDING, "50%"))
      test.settle()
      // The mover is where it was, as tall as it was: nothing of its own changed.
      expect(test.bounds("mover")).toEqual(mover)
      if (retention) expect(test.counters().replays, "the mover is replayed").toBeGreaterThan(replays)

      test.render(page(PADDING, "auto"))
      test.renderer.flush()
      test.renderer.clockFastForward(500)
      const height = test.bounds("mover").height
      expect(test.counters().mismatches, test.renderer.getRetentionMismatches().join("\n")).toBe(0)
      return height
    } finally {
      test.unmount()
    }
  }

  it("is the one the page has now when a height starts from a percentage to auto", () => {
    const built = heightHalfWayToAuto(false)
    // Half of what is left of the page (400 - 200) starts it, and it is half way to the natural height.
    expect(built).toBeCloseTo(((PAGE_HEIGHT - PADDING) / 2 + NATURAL_HEIGHT) / 2, 0)
    expect(heightHalfWayToAuto(true)).toBeCloseTo(built, 0)
  })
})
