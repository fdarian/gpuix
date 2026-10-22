/// `overflowAnchor` and scroll anchoring against Chrome: a scroller that is scrolled away from its start keeps what it shows
/// where it is when the layout moves it, by moving the offset by what the anchor (the first box in view) moved. Where the
/// offset is, which row is first in view and where, and in which frame the scroll events run, step by step, in the scenes of
/// `fixtures/scroll-anchoring-scenes.ts`.
///
/// `fixtures/scroll-anchoring-oracle.html` is the page, `fixtures/scroll-anchoring-oracle.ts` drives it and writes
/// `fixtures/scroll-anchoring-oracle.json` (regenerate with `bun fixtures/scroll-anchoring-oracle.ts [scene...]`).

import fs from "fs"
import path from "path"
import type { CSSProperties, ReactNode } from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer, type TestRenderer } from "../testing.js"
import { verifying } from "./retained-harness.js"
import { SCENES, type Op, type SceneDef, type Style } from "./fixtures/scroll-anchoring-scenes"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const FIXTURES = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures")

type Read = { top: number; first: string | null; rowTop: number | null }
type OracleStep = Read & { step: string; events: number[]; forced?: Read }
const ORACLE: Record<string, OracleStep[]> = JSON.parse(fs.readFileSync(path.join(FIXTURES, "scroll-anchoring-oracle.json"), "utf8"))

const UNIT = 1 / 64
const FRAMES = 6

type Row = { id: string; height: string; style: Style }
type Model = { rows: Row[]; scroller: Style; wrapper: Style | null }

const styled = (base: Style, patch: Style): Style => {
  const merged = { ...base, ...patch }
  for (const key of Object.keys(merged)) if (merged[key] === "") delete merged[key]
  return merged
}

const modelOf = (scene: SceneDef): Model => ({
  rows: Array.from({ length: scene.rows }, (_, index) => ({
    id: `r${index}`,
    height: scene.rowHeight,
    style: { ...scene.rowStyle, ...scene.rowStyles?.[`r${index}`] },
  })),
  scroller: scene.scrollerStyle ?? {},
  wrapper: scene.wrapper ?? null,
})

const edited = (model: Model, op: Exclude<Op, { op: "scrollTo" } | { op: "scrollBy" }>, scene: SceneDef): Model => {
  if (op.op === "insert") {
    const rows = [...model.rows]
    const at = op.before === null ? rows.length : rows.findIndex((row) => row.id === op.before)
    if (at < 0) throw new Error(`no row ${op.before}`)
    rows.splice(at, 0, { id: op.id, height: op.height, style: { ...scene.rowStyle, ...op.style } })
    return { ...model, rows }
  }
  if (op.op === "remove") return { ...model, rows: model.rows.filter((row) => row.id !== op.id) }
  if (op.id === "scroller") return { ...model, scroller: styled(model.scroller, op.style) }
  if (op.id === "wrapper") {
    if (!model.wrapper) throw new Error("the scene has no wrapper")
    return { ...model, wrapper: styled(model.wrapper, op.style) }
  }
  return { ...model, rows: model.rows.map((row) => (row.id === op.id ? { ...row, style: styled(row.style, op.style) } : row)) }
}

function Page({ scene, model, onScroll }: { scene: SceneDef; model: Model; onScroll: () => void }) {
  const rows = model.rows.map((row) =>
    scene.child ? (
      <div key={row.id} testId={row.id} style={row.style as CSSProperties}>
        <div style={{ height: row.height }} />
      </div>
    ) : (
      <div key={row.id} testId={row.id} style={{ height: row.height, ...row.style } as CSSProperties} />
    )
  )
  return (
    <div style={{ width: 600, height: 500 }}>
      <div
        testId="scroller"
        onScroll={onScroll}
        style={{ display: "block", boxSizing: "border-box", overflow: "auto", width: 300, height: 300, ...model.scroller } as CSSProperties}
      >
        {model.wrapper ? (
          <div testId="wrapper" style={{ display: "block", boxSizing: "border-box", ...model.wrapper } as CSSProperties}>
            {rows}
          </div>
        ) : (
          rows
        )}
      </div>
    </div>
  )
}

function idsOf(renderer: TestRenderer): Map<string, number> {
  const ids = new Map<string, number>()
  for (const element of renderer.findByType("div")) if (element.testId) ids.set(element.testId, element.id)
  return ids
}

/** What the page shows: the offset, and the first row that is in view with its top from the top of the scroller. */
function look(renderer: TestRenderer, model: Model): Read {
  const ids = idsOf(renderer)
  const scroller = ids.get("scroller")
  if (scroller === undefined) throw new Error("no scroller")
  const metrics = renderer.getScrollMetrics(scroller)
  const box = renderer.getElementBounds(scroller)
  if (!metrics || !box) throw new Error("the scroller has no boxes")
  for (const row of model.rows) {
    const id = ids.get(row.id)
    const rect = id === undefined ? null : renderer.getElementBounds(id)
    if (rect && rect.height > 0 && rect.y + rect.height > box.y && rect.y < box.y + box.height) {
      return { top: metrics[1] ?? 0, first: row.id, rowTop: rect.y - box.y }
    }
  }
  return { top: metrics[1] ?? 0, first: null, rowTop: null }
}

function closeTo(label: string, seen: number | null, expected: number | null) {
  if (seen === null || expected === null) expect.soft(seen, label).toBe(expected)
  else expect.soft(Math.abs(seen - expected), `${label}: ${seen} / Chrome's ${expected}`).toBeLessThanOrEqual(UNIT)
}

/**
 * A scroll event of gpuix is reported by the frame that paints the offset (`observers::sweep`), and only when that offset
 * differs from the last one painted. So one for a write of the offset runs a frame after the one Chrome runs it in, which
 * queues it with the write, an adjustment is queued by the frame that lays it out in both, events painted in one frame are
 * one event, and an offset that is written away and compensated back in one frame is none.
 */
function framesInGpuix(events: number[], writes: boolean, moved: boolean): number[] {
  if (!moved) return []
  return [...new Set(events.map((frame) => (writes && frame === 1 ? 2 : frame)))]
}

/** Scenes of Chrome's behaviour that gpuix does not follow. */
const KNOWN_DEVIATIONS: Record<string, string> = {
  // After a second change of `top` Chrome compensates nothing in the next layout, as if the flag of the box stayed set until
  // it is laid out, which a relative box does not need to be.
  anchorTopStale: "a stale scroll-anchor-disabling flag",
}

function compare(label: string, seen: Read, expected: Read) {
  closeTo(`${label}: scrollTop`, seen.top, expected.top)
  expect.soft(seen.first, `${label}: the first row in view`).toBe(expected.first)
  closeTo(`${label}: its top`, seen.rowTop, expected.rowTop)
}

describeNative("scroll anchoring against Chrome", { timeout: 120_000 }, () => {
  it("has the scenes of the oracle", () => {
    expect(Object.keys(ORACLE).sort()).toEqual(Object.keys(SCENES).sort())
    for (const [name, scene] of Object.entries(SCENES)) expect(scene.steps.length + 1, name).toBe(ORACLE[name]?.length)
  })

  for (const [name, scene] of Object.entries(SCENES)) {
    const check = KNOWN_DEVIATIONS[name] ? it.fails : it
    check(`moves the offset as Chrome does, and tells it in the same frame: ${name}`, () => {
      const root = createTestRoot({ width: 600, height: 500 })
      const renderer = root.renderer
      let frame = 0
      let events: number[] = []
      let model = modelOf(scene)
      let previousTop = 0
      const show = (node: ReactNode) => root.render(node)
      const draw = () => show(<Page scene={scene} model={model} onScroll={() => events.push(frame)} />)
      try {
        renderer.clockPause()
        renderer.setInlineDraws(false)
        draw()
        renderer.drawFrame()
        const scrollerId = () => {
          const id = idsOf(renderer).get("scroller")
          if (id === undefined) throw new Error("no scroller")
          return id
        }
        const offset = () => renderer.getScrollMetrics(scrollerId())?.[1] ?? 0

        const run = (label: string, ops: Op[], forced: boolean | undefined, expected: OracleStep) => {
          const writes = ops.some((op) => op.op === "scrollTo" || op.op === "scrollBy")
          events = []
          frame = 1
          let dirty = false
          for (const op of ops) {
            if (op.op === "scrollTo" || op.op === "scrollBy") {
              if (dirty) draw()
              dirty = false
              const y = op.op === "scrollTo" ? (op.y === "max" ? 1e7 : op.y) : offset() + op.dy
              renderer.scrollTo(scrollerId(), 0, y)
            } else {
              model = edited(model, op, scene)
              dirty = true
            }
          }
          if (dirty) draw()
          if (expected.forced) compare(`${name}, ${label}, read in the task`, look(renderer, model), expected.forced)
          for (frame = 1; frame <= FRAMES; frame++) {
            renderer.clockFastForward(1000 / 60)
            renderer.dispatchNativeEvents()
            renderer.drawFrame()
          }
          renderer.dispatchNativeEvents()
          const seen = look(renderer, model)
          compare(`${name}, ${label}`, seen, expected)
          expect
            .soft(events, `${name}, ${label}: the frames the scroll events run in`)
            .toEqual(framesInGpuix(expected.events, writes, seen.top !== previousTop))
          previousTop = seen.top
        }

        const oracle = ORACLE[name]
        if (!oracle) throw new Error(`no oracle for ${name}`)
        const expectedOf = (index: number) => {
          const expected = oracle[index]
          if (!expected) throw new Error(`${name} has no step ${index} in the oracle`)
          return expected
        }
        run("mounted", [{ op: "scrollTo", y: scene.scrollTo }], false, expectedOf(0))
        scene.steps.forEach((step, index) => run(step.name, step.ops, step.forced, expectedOf(index + 1)))
        if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
      } finally {
        renderer.setInlineDraws(true)
        root.unmount()
      }
    })
  }
})
