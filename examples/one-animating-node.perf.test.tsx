/**
 * Per-frame cost of one small animating node over a large static tree
 * (issue #37). The shape is the demo's changelog pages: a page of cards and
 * text (about 2000 elements at 60 cards) with one `motion.div` hover
 * highlight moving over it. Nothing but the highlight changes between frames,
 * yet every frame rebuilds, re-lays-out and repaints the whole tree.
 *
 * Variants, each at 0, 30 and 60 cards:
 *   idle          no motion node: the clock moves, nothing changes
 *   motion-top    the highlight animates `top`, so it also moves in layout
 *   motion-opacity  the highlight animates `opacity`, a paint-only property
 *
 * A frame is the window clock moving 1/75 s (the demo's 75 Hz display), which
 * notifies the view and draws once: the same rebuild, layout, prepaint and paint a real
 * frame runs, minus GPU submission. Slope between card counts is the cost per
 * live node (the demo measured about 0.5 ms + 10 us per node).
 *
 * It profiles and has no time budgets; the build counters it prints are guarded. Run it through
 * `bun run test:perf:animating` (NODE_ENV=production, like `test:perf`).
 */

import React from 'react'
import { describe, expect, it } from 'vitest'
import { motion } from '@ghia/react'
import { createTestRoot, hasNativeTestRenderer } from '@ghia/react/testing'
import { countedReasons, formatRetain, formatRetained, formatSummary, retainedDelta, sample, summarize } from './perf-harness'
import { C, CardGrid, LONG_TWEEN } from './perf-fixtures'

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const WINDOW = { width: 1240, height: 780 }
const FRAME_MS = 1000 / 75
const CARD_COUNTS = [0, 30, 60]
const WARMUP = 10
const SAMPLES = 100

type Variant = 'idle' | 'motion-top' | 'motion-opacity'

function Highlight({ variant }: { variant: Variant }) {
  const base = {
    position: 'absolute',
    left: 20,
    top: 20,
    width: 240,
    height: 32,
    borderRadius: 8,
    backgroundColor: '#ffffff',
    opacity: 0.1,
  } as const
  if (variant === 'motion-top') {
    return <motion.div testId="highlight" style={base} initial={{ top: 20 }} animate={{ top: 700 }} transition={LONG_TWEEN} />
  }
  return <motion.div testId="highlight" style={base} initial={{ opacity: 0.05 }} animate={{ opacity: 0.9 }} transition={LONG_TWEEN} />
}

function Page({ cards, variant }: { cards: number; variant: Variant }) {
  return (
    <div
      style={{
        position: 'relative',
        width: WINDOW.width,
        height: WINDOW.height,
        backgroundColor: C.bg,
        overflow: 'hidden',
      }}
    >
      <CardGrid cards={cards} />
      {variant === 'idle' ? null : <Highlight variant={variant} />}
    </div>
  )
}

describeNative('one animating node over a large static tree', () => {
  for (const variant of ['idle', 'motion-top', 'motion-opacity'] as const) {
    it(`${variant}: per-frame cost by tree size`, () => {
      for (const cards of CARD_COUNTS) {
        const root = createTestRoot(WINDOW)
        try {
          root.render(<Page cards={cards} variant={variant} />)
          const renderer = root.renderer
          renderer.clockPause()
          const elements = renderer.getRetainedElementCount()
          const draws = renderer.getDebugFrameOverlayStats().frames
          renderer.resetBuildCounters()
          const layoutBefore = renderer.getLayoutCounters()
          const retainedBefore = renderer.getRetainedCounters()
          const frames = sample(
            // The clock moves and notifies the view, which draws once: one frame.
            () => renderer.clockFastForward(FRAME_MS),
            WARMUP,
            SAMPLES,
          )
          const drawn = renderer.getDebugFrameOverlayStats().frames - draws
          const built = renderer.getBuildCounters()
          const layoutAfter = renderer.getLayoutCounters()
          const layoutPerFrame = (key: keyof typeof layoutAfter) =>
            ((layoutAfter[key] - layoutBefore[key]) / (WARMUP + SAMPLES)).toFixed(1)
          const perFrame = (count: number) => (count / (WARMUP + SAMPLES)).toFixed(1)
          const highlight = renderer.findByTestId('highlight')
          const highlightY = highlight ? ` highlight-y=${renderer.getElementBounds(highlight.id)!.y.toFixed(1)}` : ''
          console.log(
            `[animating.perf] ${variant}${highlightY} cards=${cards} elements=${elements} ` +
              `draws/frame=${(drawn / (WARMUP + SAMPLES)).toFixed(2)} ` +
              `built/frame=${perFrame(built.elementsBuilt)} style-clones/frame=${perFrame(built.styleDescClones)} ` +
              `inherited-clones/frame=${perFrame(built.inheritedClones)} ` +
              `layout-nodes-new/frame=${layoutPerFrame('nodesCreated')} reused/frame=${layoutPerFrame('nodesReused')} ` +
              `layout-passes/frame=${layoutPerFrame('passes')} styles-set/frame=${layoutPerFrame('stylesSet')} children-set/frame=${layoutPerFrame('childrenSet')} measures/frame=${layoutPerFrame('measures')} | ${formatSummary(summarize(frames))}`,
          )
          console.log(
            `[animating.perf]   ${formatRetain(built)} | ${formatRetained(retainedDelta(renderer.getRetainedCounters(), retainedBefore), WARMUP + SAMPLES)}`,
          )
          expect(elements).toBeGreaterThan(0)
          // The work counters do not depend on the clock: a frame of a static tree copies one
          // Inherited per element it builds, one for each subtree it leaves to gpui, and one for each
          // unit it builds whole (the start of its build, which a patch builds it again from), and no
          // style except the animated element's own. The highlight is a unit once it is an
          // `absolute` element whose loop paints only (`motion-opacity`), not when it moves a box.
          expect(built.inheritedClones).toBeLessThanOrEqual(
            built.elementsBuilt + built.retainableRoots + built.unitsBuilt,
          )
          expect(built.styleDescClones / (WARMUP + SAMPLES)).toBeLessThanOrEqual(variant === 'idle' ? 0 : 1)
          // Every built element is counted once, as in a retainable subtree or under the reason it is not.
          // `GPUIX_RETAIN_OFF` stops the counting, to time the build without it.
          // A page with no subtree worth skipping (the empty one) stops being recorded for a while,
          // and the frames it is not recorded in count nothing.
          if (process.env.GPUIX_RETAIN_OFF === undefined) {
            const counted = countedReasons(built)
            if (cards > 0) expect(counted).toBe(built.elementsBuilt + built.elementsSkipped)
            else expect(counted).toBeLessThanOrEqual(built.elementsBuilt + built.elementsSkipped)
          }
        } finally {
          root.unmount()
        }
      }
    }, 300_000)
  }

  it('a frame with one node animating costs a fraction of building the page, and builds a fraction of its elements', () => {
    const root = createTestRoot(WINDOW)
    try {
      root.render(<Page cards={60} variant="idle" />)
      const renderer = root.renderer
      renderer.clockPause()
      const elements = renderer.getRetainedElementCount()
      const frameCost = () =>
        summarize(sample(() => renderer.clockFastForward(FRAME_MS), WARMUP, SAMPLES)).cpuP50

      // Every frame builds every element: the cost the page had before frames could skip anything.
      renderer.setRetainPlanning(false)
      const building = frameCost()

      renderer.setRetainPlanning(true)
      root.render(<Page cards={60} variant="motion-opacity" />)
      renderer.clockPause()
      for (let frame = 0; frame < WARMUP; frame++) renderer.clockFastForward(FRAME_MS)
      renderer.resetBuildCounters()
      const animating = frameCost()
      const built = renderer.getBuildCounters()
      const perFrame = built.elementsBuilt / (WARMUP + SAMPLES)
      console.log(
        `[animating.perf] guard elements=${elements} building=${building.toFixed(2)}ms animating=${animating.toFixed(2)}ms ` +
          `ratio=${(animating / building).toFixed(2)} built/frame=${perFrame.toFixed(1)}`,
      )
      // About a tenth today; the bound leaves room for a noisy machine and not for a frame that builds the page.
      expect(animating).toBeLessThan(building * 0.4)
      expect(perFrame).toBeLessThan(elements * 0.05)
      expect(renderer.getRetainedCounters().replays).toBeGreaterThan(0)
    } finally {
      root.unmount()
    }
  }, 300_000)
})
