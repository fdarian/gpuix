/// `content-visibility: auto` boxes under, and with, a transform (#116): whether a box is near the viewport is decided on the
/// box as the viewport sees it, which is the box through every transform around it and its own (the margin of 150% of the
/// viewport is added in the box's own space, and scaled and turned with it), cut by what clips it as the viewport sees that.
/// A box tested where layout put it is skipped on screen, or built and painted far away.
///
/// `fixtures/content-visibility-transform-cases.ts` has the cases, `fixtures/content-visibility-transform-oracle.html` is the
/// page and `fixtures/content-visibility-transform-oracle.ts` drives it and writes `fixtures/content-visibility-transform-oracle.json`
/// (regenerate with `bun fixtures/content-visibility-transform-oracle.ts`).

import fs from "fs"
import path from "path"
import type { CSSProperties, ReactNode } from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { animate, motion, motionValue } from "../index"
import { createTestRoot, hasNativeTestRenderer, type TestRenderer } from "../testing.js"
import { CASES, PAGE, type Node } from "./fixtures/content-visibility-transform-cases"
import { verifying } from "./retained-harness.js"
import { elapse } from "./skipped-animations-fixture.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const FIXTURES = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures")
const ORACLE = JSON.parse(fs.readFileSync(path.join(FIXTURES, "content-visibility-transform-oracle.json"), "utf8")) as Record<
  string,
  { skipped: string[]; told: Record<string, boolean> }
>

type Told = Map<string, boolean>

function Boxes({ nodes, told }: { nodes: Node[]; told: Told }): ReactNode {
  return nodes.map((node, index) => (
    <div
      key={index}
      testId={node.name ? `box-${node.name}` : undefined}
      style={node.style as CSSProperties}
      onContentVisibilityAutoStateChange={node.name ? (event) => told.set(node.name!, event.skipped) : undefined}
    >
      {node.name ? <div testId={`probe-${node.name}`} style={{ position: "absolute", left: 0, top: 0, width: 10, height: 10 }} /> : null}
      {node.children ? <Boxes nodes={node.children} told={told} /> : null}
    </div>
  ))
}

const Page = ({ children }: { children: ReactNode }) => (
  <div style={{ position: "relative", width: PAGE.width, height: PAGE.height, overflow: "hidden" }}>{children}</div>
)

function namesOf(nodes: Node[]): string[] {
  return nodes.flatMap((node) => [...(node.name ? [node.name] : []), ...namesOf(node.children ?? [])])
}

function idsOf(renderer: TestRenderer): Map<string, number> {
  const ids = new Map<string, number>()
  for (const element of renderer.findByType("div")) if (element.testId) ids.set(element.testId, element.id)
  return ids
}

const skippedOf = (renderer: TestRenderer, names: string[]) => {
  const ids = idsOf(renderer)
  return names.filter((name) => !renderer.getBuiltElementBounds(ids.get(`probe-${name}`)!)).sort()
}

/** Draws until a frame changes nothing the test can see. */
function settle(renderer: TestRenderer, names: string[]): number {
  let last = ""
  for (let frames = 1; frames <= 24; frames++) {
    renderer.drawFrame()
    renderer.dispatchNativeEvents()
    const now = skippedOf(renderer, names).join()
    if (now === last) return frames
    last = now
  }
  throw new Error("the scene did not settle in 24 frames")
}

/** Draws the window's first frame, with something else in it, so that what is added after it is not in the first frame. */
function drawFirstFrame(root: ReturnType<typeof createTestRoot>, renderer: TestRenderer) {
  root.render(
    <Page>
      <div style={{ height: 1 }} />
    </Page>
  )
  renderer.flush()
  renderer.drawFrame()
  renderer.dispatchNativeEvents()
}

describeNative("content-visibility: auto under a transform against Chrome", { timeout: 120_000 }, () => {
  it("has the cases of the oracle", () => {
    expect(Object.keys(ORACLE).sort()).toEqual(CASES.map((item) => item.name).sort())
  })

  for (const item of CASES) {
    it(`skips what Chrome skips: ${item.name}`, () => {
      const told: Told = new Map()
      const root = createTestRoot(PAGE)
      const renderer = root.renderer
      try {
        renderer.setInlineDraws(false)
        drawFirstFrame(root, renderer)
        const names = namesOf(item.nodes)
        root.render(
          <Page>
            <Boxes nodes={item.nodes} told={told} />
          </Page>
        )
        renderer.flush()
        const expected = ORACLE[item.name]!
        // The frame that builds the boxes answers by where they are when they are prepainted; the frames after it by the boxes painted.
        renderer.drawFrame()
        renderer.dispatchNativeEvents()
        const first = skippedOf(renderer, names)
        settle(renderer, names)
        const settled = skippedOf(renderer, names)
        expect(settled, `${item.name}: skipped once settled`).toEqual(expected.skipped)
        expect(Object.fromEntries([...told].sort()), `${item.name}: what each box was last told`).toEqual(expected.told)
        expect(first, `${item.name}: skipped by the frame that builds them`).toEqual(expected.skipped)
        if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
      } finally {
        root.unmount()
      }
    })
  }
})

describeNative("a transform that is written moves a box into the margin and out of it", { timeout: 120_000 }, () => {
  /** A box laid out 100px from the top of the page, 100px high, under a holder translated by `y`. */
  const holder = (transform: CSSProperties["transform"], told: Told) => (
    <Page>
      <div testId="holder" style={{ position: "absolute", left: 0, top: 0, width: PAGE.width, height: PAGE.height, transform }}>
        <Boxes nodes={[{ name: "box", style: { position: "absolute", left: 100, top: 100, width: 100, height: 100, contentVisibility: "auto" } }]} told={told} />
      </div>
    </Page>
  )

  it("when the style changes: skipped at 3000px, built at 1800px, skipped again at 2000px", () => {
    const told: Told = new Map()
    const root = createTestRoot(PAGE)
    const renderer = root.renderer
    try {
      renderer.setInlineDraws(false)
      drawFirstFrame(root, renderer)
      const at = (y: number) => {
        root.render(holder(`translate(0px, ${y}px)`, told))
        renderer.flush()
        settle(renderer, ["box"])
        elapse(renderer, 0)
        elapse(renderer, 0)
        return skippedOf(renderer, ["box"]).length === 1
      }
      expect(at(3000), "3000px").toBe(true)
      expect(at(1800), "1800px: the box is 1900px down").toBe(false)
      expect(at(1860), "1860px: 1960px down, past the margin").toBe(true)
      expect(at(0), "0px").toBe(false)
      expect([...told]).toEqual([["box", false]])
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    } finally {
      root.unmount()
    }
  })

  it("when a motion value moves the holder, with no layout change", () => {
    const told: Told = new Map()
    const y = motionValue(3000)
    const root = createTestRoot(PAGE)
    const renderer = root.renderer
    try {
      renderer.setInlineDraws(false)
      renderer.clockPause()
      drawFirstFrame(root, renderer)
      root.render(
        <Page>
          <motion.div testId="holder" style={{ position: "absolute", left: 0, top: 0, width: PAGE.width, height: PAGE.height, y }}>
            <Boxes nodes={[{ name: "box", style: { position: "absolute", left: 100, top: 100, width: 100, height: 100, contentVisibility: "auto" } }]} told={told} />
          </motion.div>
        </Page>
      )
      renderer.flush()
      settle(renderer, ["box"])
      const skipped = () => skippedOf(renderer, ["box"]).length === 1
      expect(skipped(), "at 3000px").toBe(true)
      const run = (to: number) => {
        const controls = animate(y, to, { duration: 1, ease: "linear" })
        const seen: string[] = []
        for (let step = 0; step < 10; step++) {
          elapse(renderer, 100)
          // A change of what is near takes effect in the frame after the one that found it.
          elapse(renderer, 0)
          elapse(renderer, 0)
          seen.push(`${Math.round(y.get())}:${skipped() ? "skipped" : "built"}`)
        }
        controls.stop()
        return seen
      }
      const expected = (values: number[]) => values.map((value) => `${value}:${100 + value >= 1950 ? "skipped" : "built"}`)
      expect(run(0), "moving in").toEqual(expected([2700, 2400, 2100, 1800, 1500, 1200, 900, 600, 300, 0]))
      expect(run(3000), "moving out").toEqual(expected([300, 600, 900, 1200, 1500, 1800, 2100, 2400, 2700, 3000]))
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    } finally {
      root.unmount()
    }
  })
})
