/**
 * Cost of clipped and off-screen content (issue #38). A box outside an `overflow: hidden` or
 * scroll clip is not prepainted or painted: gpui skips the subtree, which only reports the bounds,
 * sizes, scroll metrics and tab stops that JS can read. Build and layout still run for it, as in a
 * browser, so a clipped box costs less than a visible one but not nothing. Only `display: none`
 * costs nothing.
 *
 * Two groups, each a frame of `renderer.flush()` (rebuild, layout, prepaint
 * and paint of an unchanged tree):
 *
 *   boxes   N 20x20 boxes in a wrapping flex row (perf-micro `misc`), N = 0,
 *           500, 2000, in a 400x300 container that is
 *             hidden   overflow: hidden, about 80% of the boxes outside the clip
 *             scroll   overflow: auto, the same
 *             none     display: none, the cost the clip does not have
 *             visible  no clip, 1180 px wide, every box inside the window
 *           Per-box cost is (p50 at N - p50 at 0) / N.
 *   table   the scroll-area demo: a 16 x 13 table (about 430 elements) in a
 *           320 x 240 box that is hidden, scroll, none, or `visible` (the box
 *           grows to the table).
 *
 * It logs the numbers and guards two things: that the clipped boxes are skipped (counted, so noise
 * cannot hide a regression), and that a frame with them is cheaper than the same frame with
 * every box visible, by a margin well inside what it measures. Run it through
 * `bun run test:perf:clipped` (NODE_ENV=production, like `test:perf`).
 */

import React from 'react'
import { describe, expect, it } from 'vitest'
import { createTestRoot, hasNativeTestRenderer } from '@gpuix/react/testing'
import { formatSummary, sample, summarize, type Summary } from './perf-harness'

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const WINDOW = { width: 1240, height: 1100 }
const WARMUP = 10
const SAMPLES = 60
const BOX_COUNTS = [0, 500, 2000]
const COLUMNS = 16
const ROWS = 13

type Clip = 'hidden' | 'scroll' | 'none' | 'visible'

function clipStyle(clip: Clip, width: number, height: number): React.CSSProperties {
  switch (clip) {
    case 'hidden':
      return { width, height, overflow: 'hidden' }
    case 'scroll':
      return { width, height, overflowY: 'auto', overflowX: 'auto' }
    case 'none':
      return { width, height, display: 'none' }
    case 'visible':
      return {}
  }
}

function Boxes({ count, clip }: { count: number; clip: Clip }) {
  return (
    <div
      style={{
        ...clipStyle(clip, 400, 300),
        display: clip === 'none' ? 'none' : 'flex',
        flexDirection: 'row',
        flexWrap: 'wrap',
        alignContent: 'flex-start',
        width: clip === 'visible' ? 1180 : 400,
      }}
    >
      {Array.from({ length: count }, (_, index) => (
        <div key={index} style={{ width: 20, height: 20, margin: 2, backgroundColor: '#8a8a8a' }} />
      ))}
    </div>
  )
}

function Table({ clip }: { clip: Clip }) {
  return (
    <div style={{ ...clipStyle(clip, 320, 240), border: '1px solid #2a2a2e' }}>
      {Array.from({ length: ROWS }, (_, row) => (
        <div key={row} style={{ display: 'flex', flexDirection: 'row' }}>
          {Array.from({ length: COLUMNS }, (_, column) => (
            <div
              key={column}
              style={{ width: 80, height: 28, flexShrink: 0, padding: 6, borderBottomWidth: 1, borderColor: '#2a2a2e', borderStyle: 'solid' }}
            >
              <text style={{ fontSize: 12, color: '#f2f2f3' }}>{`r${row} c${column}`}</text>
            </div>
          ))}
        </div>
      ))}
    </div>
  )
}

function frame(node: React.ReactNode): { summary: Summary; elements: number; culled: number } {
  const root = createTestRoot(WINDOW)
  try {
    // An app without a screen reader, which gpui skips clipped content for.
    root.renderer.setA11yActive(false)
    root.render(<div style={{ width: WINDOW.width, height: WINDOW.height }}>{node}</div>)
    const summary = summarize(sample(() => root.renderer.flush(), WARMUP, SAMPLES))
    const before = root.renderer.getCulledSubtreeCount()
    root.renderer.flush()
    return { summary, elements: root.renderer.getRetainedElementCount(), culled: root.renderer.getCulledSubtreeCount() - before }
  } finally {
    root.unmount()
  }
}

/** What a clipped frame may cost against the same frame unclipped. Measured: 0.66 to 0.70. */
const CLIPPED_OVER_VISIBLE = 0.85

describeNative('clipped and off-screen content', () => {
  it('boxes: per-box cost by clip', () => {
    const empty = frame(<Boxes count={0} clip="hidden" />)
    const largest: Partial<Record<Clip, ReturnType<typeof frame>>> = {}
    for (const clip of ['visible', 'hidden', 'scroll', 'none'] as const) {
      for (const count of BOX_COUNTS) {
        if (count === 0) continue
        const run = frame(<Boxes count={count} clip={clip} />)
        const perBoxUs = ((run.summary.cpuP50 - empty.summary.cpuP50) / count) * 1000
        console.log(
          `[clipped.perf] boxes ${clip} N=${count} elements=${run.elements} skipped=${run.culled} per-box=${perBoxUs.toFixed(2)}us | ${formatSummary(run.summary)}`,
        )
        if (count === Math.max(...BOX_COUNTS)) largest[clip] = run
      }
    }
    expect(empty.elements).toBeGreaterThan(0)

    const visible = largest.visible!
    expect(visible.culled).toBe(0)
    for (const clip of ['hidden', 'scroll'] as const) {
      const run = largest[clip]!
      // 400 x 300 holds about 12 x 10 of the 24 px cells: the rest is outside.
      expect(run.culled, `${clip}: the boxes outside the clip are skipped`).toBeGreaterThan(Math.max(...BOX_COUNTS) * 0.8)
      const ratio = (run.summary.cpuP50 - empty.summary.cpuP50) / (visible.summary.cpuP50 - empty.summary.cpuP50)
      expect(ratio, `${clip} costs ${ratio.toFixed(2)} of visible per box`).toBeLessThan(CLIPPED_OVER_VISIBLE)
    }
  }, 300_000)

  it('table: the scroll-area demo shape', () => {
    const runs: Partial<Record<Clip, ReturnType<typeof frame>>> = {}
    for (const clip of ['visible', 'hidden', 'scroll', 'none'] as const) {
      const run = frame(<Table clip={clip} />)
      runs[clip] = run
      console.log(`[clipped.perf] table ${COLUMNS}x${ROWS} ${clip} elements=${run.elements} skipped=${run.culled} | ${formatSummary(run.summary)}`)
      expect(run.elements).toBeGreaterThan(0)
    }
    expect(runs.visible!.culled).toBe(0)
    for (const clip of ['hidden', 'scroll'] as const) {
      expect(runs[clip]!.culled, `${clip}: the rows outside the clip are skipped`).toBeGreaterThan(0)
      const ratio = runs[clip]!.summary.cpuP50 / runs.visible!.summary.cpuP50
      expect(ratio, `${clip} costs ${ratio.toFixed(2)} of visible`).toBeLessThan(CLIPPED_OVER_VISIBLE)
    }
  }, 300_000)
})
