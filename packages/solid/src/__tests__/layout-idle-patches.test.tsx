import { For, createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import { motion } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { TestRenderer } from "../testing.js"

const patching = process.env.GPUIX_RETAIN_OFF === undefined && process.env.GPUIX_PATCH_OFF === undefined
const linear = { duration: 1, ease: "linear" } as const

type Variant = "none" | "position" | "layout" | "size"

function counters(renderer: TestRenderer) {
  return {
    patched: renderer.getPatchCounters().patchedFrames,
    whole: renderer.getPaintedFrameCount(),
    unpatchable: JSON.parse(renderer.getPatchCounters().unpatchable) as Record<string, number>,
  }
}

function ticks(renderer: TestRenderer, count: number) {
  const before = counters(renderer)
  for (let index = 0; index < count; index++) {
    renderer.clockAdvance(16)
    renderer.dispatchNativeEvents()
    renderer.drawFrame()
  }
  const after = counters(renderer)
  const unpatchable: Record<string, number> = {}
  for (const [name, total] of Object.entries(after.unpatchable)) {
    const gained = total - (before.unpatchable[name] ?? 0)
    if (gained > 0) unpatchable[name] = gained
  }
  return { patched: after.patched - before.patched, whole: after.whole - before.whole, unpatchable }
}

describe.skipIf(!hasNativeTestRenderer)("Solid: an idle layout box (#117)", () => {
  for (const variant of ["none", "position", "layout", "size"] as const satisfies Variant[]) {
    it(`${variant}: ticks are patched while no glide runs, whole while one does, and patched again after`, () => {
      const [moved, setMoved] = createSignal(false)
      const events: string[] = []
      const app = createTestRoot({ width: 600, height: 600 })
      const renderer = app.renderer
      const sizes = variant === "size"
      app.render(() => (
        <div style={{ display: "flex", flexDirection: "column", gap: 8, padding: 16 }}>
          <motion.div
            animate={{ opacity: [1, 0.4, 1] }}
            transition={{ duration: 1, repeat: Number.POSITIVE_INFINITY }}
            style={{ width: 40, height: 40, backgroundColor: "#6B97FF" }}
          />
          <motion.div
            testId="box"
            layout={variant === "none" ? undefined : variant === "layout" ? true : variant}
            transition={linear}
            onLayoutAnimationStart={() => events.push("start")}
            onLayoutAnimationComplete={() => events.push("complete")}
            style={{
              width: sizes && moved() ? 220 : 120,
              height: 24,
              marginLeft: sizes ? 0 : moved() ? 100 : 0,
              backgroundColor: "#999999",
            }}
          />
          <For each={Array.from({ length: 200 })}>
            {() => <div style={{ width: 80, height: 4, backgroundColor: "#cccccc" }} />}
          </For>
        </div>
      ))
      renderer.flush()
      const box = () => renderer.getElementBounds(renderer.findByTestId("box")!.id)!
      const offset = () => (sizes ? box().width - 120 : box().x - x0)
      const x0 = box().x
      const glides = variant !== "none"
      ticks(renderer, 60)

      const idle = ticks(renderer, 150)
      if (patching) expect(idle).toEqual({ patched: 150, whole: 0, unpatchable: {} })

      app.flushSync(() => setMoved(true))
      expect(Math.abs(offset())).toBeCloseTo(glides ? 0 : 100, 0)
      const mid = ticks(renderer, 30)
      if (glides) {
        expect(offset()).toBeGreaterThan(40)
        expect(offset()).toBeLessThan(60)
        expect(mid.patched).toBe(0)
        expect(mid.whole).toBe(30)
        expect(events).toEqual(["start"])
      } else {
        expect(offset()).toBeCloseTo(100, 0)
      }

      ticks(renderer, 45)
      const rest = ticks(renderer, 150)
      expect(offset()).toBeCloseTo(100, 0)
      if (glides) expect(events).toEqual(["start", "complete"])
      if (patching) {
        expect(rest.patched).toBeGreaterThanOrEqual(147)
        expect(rest.whole).toBeLessThanOrEqual(3)
      }

      events.length = 0
      app.flushSync(() => setMoved(false))
      expect(Math.abs(offset())).toBeCloseTo(glides ? 100 : 0, 0)
      ticks(renderer, 30)
      if (glides) {
        expect(offset()).toBeGreaterThan(40)
        expect(offset()).toBeLessThan(60)
      }
      ticks(renderer, 60)
      const settled = ticks(renderer, 100)
      if (patching) expect(settled.patched).toBeGreaterThanOrEqual(98)
      expect(offset()).toBeCloseTo(0, 0)
      app.unmount()
    }, 120_000)
  }
})
