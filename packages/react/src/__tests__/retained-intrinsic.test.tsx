// @ts-nocheck
/**
 * A box with an intrinsic width keyword (`min-width: min-content`) is measured under another style
 * than its own and asked for again with the result. When nothing under it changed that must leave
 * the layout of the roots around it as it was: a retained root is replayed only while its layout
 * is the one it was recorded under, and one that is laid out again each frame is built again each
 * frame, with the roots nested in it.
 *
 * Run with GPUIX_RETAIN_VERIFY=1 too: gpui then compares every replayed root with a build.
 */

import React from "react"
import { expect, it } from "vitest"
import { Card, CARDS, describeNative, Page, setup, verifying } from "./retained-harness.js"

const TIMEOUT = 30_000
const FRAMES = 6

describeNative("boxes with an intrinsic width keyword in retained cards", { timeout: TIMEOUT }, () => {
  /**
   * The box with the keywords is held to 40px by its parent and holds a root of its own: the probe lays the root out
   * 50px wide (the max-content of the box), and the layout after it 40px wide.
   */
  const keywordCard = (index: number) => (
    <Card
      key={index}
      index={index}
      extra={
        <div style={{ width: 40, height: 20 }}>
          <div style={{ width: "fit-content", maxWidth: "100%", height: 20 }}>
            <div testId={`inner-${index}`} style={{ display: "flex", height: 20 }}>
              <div style={{ flexShrink: 0, width: 30, height: 20, backgroundColor: "#ff00ff" }} />
              <div style={{ flexShrink: 0, width: 20, height: 20, backgroundColor: "#00ffff" }} />
            </div>
          </div>
        </div>
      }
    />
  )

  it("are not laid out again by a frame that changed nothing, and leave the roots in them replayed", () => {
    const page = setup(<Page card={keywordCard} />)
    try {
      page.renderer.clockPause()
      page.settle()
      const layout = page.renderer.getLayoutCounters()
      const retained = page.counters()
      for (let frame = 0; frame < FRAMES; frame++) page.renderer.flush()
      const after = page.renderer.getLayoutCounters()
      const counters = page.counters()
      expect(page.bounds("inner-3").width).toBe(40)
      expect(counters.replays - retained.replays, "roots replayed").toBeGreaterThanOrEqual(FRAMES * CARDS)
      if (!verifying) expect(counters.lazyBuilds - retained.lazyBuilds, "roots built when gpui found their layout changed").toBe(0)
      expect(after.stylesSet - layout.stylesSet, "styles layout gave nodes").toBe(0)
      page.finish()
    } finally {
      page.unmount()
    }
  })

  it("are laid out again when their content changes", () => {
    const wide = (width: number) => (index: number) =>
      (
        <Card
          key={index}
          index={index}
          extra={
            <div style={{ display: "flex", width: 140, height: 20 }}>
              <div testId={`keyword-${index}`} style={{ minWidth: "min-content", height: 20 }}>
                <div style={{ width: index === 3 ? width : 50, height: 20 }} />
              </div>
            </div>
          }
        />
      )
    const page = setup(<Page card={wide(50)} />)
    try {
      page.renderer.clockPause()
      page.settle()
      expect(page.bounds("keyword-3").width).toBe(50)
      page.render(<Page card={wide(90)} />)
      page.settle()
      expect(page.bounds("keyword-3").width).toBe(90)
      expect(page.bounds("keyword-4").width).toBe(50)
      page.finish()
    } finally {
      page.unmount()
    }
  })
})
