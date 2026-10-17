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
 * It profiles, it does not guard: no budgets. Run it through
 * `bun run test:perf:animating` (NODE_ENV=production, like `test:perf`).
 */

import React from 'react'
import { describe, expect, it } from 'vitest'
import { motion } from '@gpuix/react'
import { createTestRoot, hasNativeTestRenderer } from '@gpuix/react/testing'
import { formatSummary, sample, summarize } from './perf-harness'
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
          const frames = sample(
            // The clock moves and notifies the view, which draws once: one frame.
            () => renderer.clockFastForward(FRAME_MS),
            WARMUP,
            SAMPLES,
          )
          const drawn = renderer.getDebugFrameOverlayStats().frames - draws
          const highlight = renderer.findByTestId('highlight')
          const highlightY = highlight ? ` highlight-y=${renderer.getElementBounds(highlight.id)!.y.toFixed(1)}` : ''
          console.log(
            `[animating.perf] ${variant}${highlightY} cards=${cards} elements=${elements} ` +
              `draws/frame=${(drawn / (WARMUP + SAMPLES)).toFixed(2)} | ${formatSummary(summarize(frames))}`,
          )
          expect(elements).toBeGreaterThan(0)
        } finally {
          root.unmount()
        }
      }
    }, 300_000)
  }
})
