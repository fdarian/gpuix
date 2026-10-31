// @ts-nocheck
/**
 * A `layout` box that no glide is running on costs a tick nothing (#117). One looping opacity animation
 * beside a `motion.div` with `layout`, `layout="position"` or `layout="size"` that does not move is
 * patched at every tick, as the same page with no `layout` prop is. While a glide really runs the
 * ticks are whole frames, and so is the one after it ends (the frame that sends `onLayoutAnimationComplete`),
 * and the page patches again after that. A box that glides again glides from where it was shown.
 */

import React from "react"
import { describe, expect, it } from "vitest"
import { motion } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip
const patching = process.env.GPUIX_RETAIN_OFF === undefined && process.env.GPUIX_PATCH_OFF === undefined
const TIMEOUT = 120_000

const linear = { duration: 1, ease: "linear" } as const
const SIBLINGS = 200

type Variant = "none" | "position" | "layout" | "size"

function Page(props: { variant: Variant; moved: boolean; events?: string[] }) {
  const sizes = props.variant === "size"
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8, padding: 16 }}>
      <motion.div
        animate={{ opacity: [1, 0.4, 1] }}
        transition={{ duration: 1, repeat: Number.POSITIVE_INFINITY }}
        style={{ width: 40, height: 40, backgroundColor: "#6B97FF" }}
      />
      <motion.div
        testId="box"
        layout={props.variant === "none" ? undefined : props.variant === "layout" ? true : props.variant}
        transition={linear}
        onLayoutAnimationStart={() => props.events?.push("start")}
        onLayoutAnimationComplete={() => props.events?.push("complete")}
        style={{
          width: sizes && props.moved ? 220 : 120,
          height: 24,
          marginLeft: sizes ? 0 : props.moved ? 100 : 0,
          backgroundColor: "#999999",
        }}
      />
      {Array.from({ length: SIBLINGS }, (_, index) => (
        <div key={index} style={{ width: 80, height: 4, backgroundColor: "#cccccc" }} />
      ))}
    </div>
  )
}

function counters(renderer) {
  return {
    patched: renderer.getPatchCounters().patchedFrames,
    whole: renderer.getPaintedFrameCount(),
    unpatchable: JSON.parse(renderer.getPatchCounters().unpatchable) as Record<string, number>,
  }
}

/** Draws `count` ticks of 16 ms as the display link does, and returns what they were. */
function ticks(renderer, count: number, each?: (index: number) => void) {
  const before = counters(renderer)
  for (let index = 0; index < count; index++) {
    renderer.clockAdvance(16)
    renderer.dispatchNativeEvents()
    renderer.drawFrame()
    each?.(index)
  }
  const after = counters(renderer)
  const unpatchable: Record<string, number> = {}
  for (const [name, total] of Object.entries(after.unpatchable)) {
    const gained = total - (before.unpatchable[name] ?? 0)
    if (gained > 0) unpatchable[name] = gained
  }
  return { patched: after.patched - before.patched, whole: after.whole - before.whole, unpatchable }
}

const boundsOf = (renderer) => renderer.getElementBounds(renderer.findByTestId("box").id)

describeNative("an idle layout box (#117)", { timeout: TIMEOUT }, () => {
  for (const variant of ["none", "position", "layout", "size"] as const) {
    it(`${variant}: ticks are patched while no glide runs, whole while one does, and patched again after`, () => {
      const events: string[] = []
      const { render, renderer } = createTestRoot({ width: 600, height: 600 })
      render(<Page variant={variant} moved={false} events={events} />)
      renderer.flush()
      ticks(renderer, 60)

      const idle = ticks(renderer, 150)
      if (patching) expect(idle, "idle").toEqual({ patched: 150, whole: 0, unpatchable: {} })

      const x0 = boundsOf(renderer).x
      const w0 = boundsOf(renderer).width
      const before = counters(renderer)
      render(<Page variant={variant} moved={true} events={events} />)
      renderer.flush()
      const glides = variant !== "none"
      const moved = variant === "size" ? boundsOf(renderer).width - w0 : boundsOf(renderer).x - x0
      const target = variant === "size" ? 100 : 100
      // The box is shown where it was, and the layout under it is already at its end.
      expect(Math.abs(moved), "shown where it was").toBeCloseTo(glides ? 0 : target, 0)

      const mid = ticks(renderer, 30)
      const half = variant === "size" ? boundsOf(renderer).width - w0 : boundsOf(renderer).x - x0
      if (glides) {
        // 31 frames of 16 ms is about half of a 1 s linear glide.
        expect(half, "halfway").toBeGreaterThan(target * 0.4)
        expect(half, "halfway").toBeLessThan(target * 0.6)
        expect(mid.patched, "no tick is patched while a glide runs").toBe(0)
        expect(mid.whole).toBe(30)
        expect(events).toEqual(["start"])
      } else {
        expect(half).toBeCloseTo(target, 0)
        expect(events).toEqual([])
      }

      ticks(renderer, 45)
      // The frame that ends the glide and the one that sends its event are whole, a second after.
      const rest = ticks(renderer, 150)
      expect(Math.abs(variant === "size" ? boundsOf(renderer).width - w0 : boundsOf(renderer).x - x0)).toBeCloseTo(target, 0)
      if (glides) expect(events).toEqual(["start", "complete"])
      if (patching) {
        expect(rest.patched, "patched again after the glide").toBeGreaterThanOrEqual(147)
        expect(rest.whole).toBeLessThanOrEqual(3)
      }
      expect(before.patched).toBeLessThanOrEqual(counters(renderer).patched)

      // A box that glides later starts from where it is shown, even half way through another glide.
      events.length = 0
      render(<Page variant={variant} moved={false} events={events} />)
      renderer.flush()
      const back = variant === "size" ? boundsOf(renderer).width - w0 : boundsOf(renderer).x - x0
      expect(Math.abs(back), "shown where it was").toBeCloseTo(glides ? target : 0, 0)
      ticks(renderer, 30)
      const returning = variant === "size" ? boundsOf(renderer).width - w0 : boundsOf(renderer).x - x0
      if (glides) {
        expect(returning).toBeGreaterThan(target * 0.4)
        expect(returning).toBeLessThan(target * 0.6)
      }
      ticks(renderer, 60)
      const settled = ticks(renderer, 100)
      if (patching) expect(settled.patched, "patched again after the second glide").toBeGreaterThanOrEqual(98)
      expect(boundsOf(renderer).x).toBeCloseTo(x0, 0)
      expect(boundsOf(renderer).width).toBeCloseTo(w0, 0)
    })
  }

  it("a glide that is retargeted half way goes on from where the box is shown", () => {
    const { render, renderer } = createTestRoot({ width: 600, height: 600 })
    render(<Page variant="layout" moved={false} />)
    renderer.flush()
    ticks(renderer, 60)
    const x0 = boundsOf(renderer).x
    render(<Page variant="layout" moved={true} />)
    renderer.flush()
    ticks(renderer, 30)
    const half = boundsOf(renderer).x - x0
    render(<Page variant="layout" moved={false} />)
    renderer.flush()
    expect(boundsOf(renderer).x - x0, "continues from where it was shown").toBeCloseTo(half, 0)
    ticks(renderer, 80)
    expect(boundsOf(renderer).x).toBeCloseTo(x0, 0)
    if (patching) expect(ticks(renderer, 60).patched).toBeGreaterThanOrEqual(58)
  })
})
