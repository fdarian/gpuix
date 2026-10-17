/**
 * Cost of clipped and off-screen content (issue #38). GPUI skips nothing that
 * lies outside an `overflow: hidden` or scroll clip: such a box costs about as
 * much as a visible one (the demo measured 4.4 vs 4.6 us per box). Only
 * `display: none` is skipped.
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
 * It profiles, it does not guard: no budgets. Run it through
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

function frame(node: React.ReactNode): { summary: Summary; elements: number } {
  const root = createTestRoot(WINDOW)
  try {
    root.render(<div style={{ width: WINDOW.width, height: WINDOW.height }}>{node}</div>)
    const summary = summarize(sample(() => root.renderer.flush(), WARMUP, SAMPLES))
    return { summary, elements: root.renderer.getRetainedElementCount() }
  } finally {
    root.unmount()
  }
}

describeNative('clipped and off-screen content', () => {
  it('boxes: per-box cost by clip', () => {
    const empty = frame(<Boxes count={0} clip="hidden" />)
    for (const clip of ['visible', 'hidden', 'scroll', 'none'] as const) {
      for (const count of BOX_COUNTS) {
        if (count === 0) continue
        const run = frame(<Boxes count={count} clip={clip} />)
        const perBoxUs = ((run.summary.cpuP50 - empty.summary.cpuP50) / count) * 1000
        console.log(
          `[clipped.perf] boxes ${clip} N=${count} elements=${run.elements} per-box=${perBoxUs.toFixed(2)}us | ${formatSummary(run.summary)}`,
        )
      }
    }
    expect(empty.elements).toBeGreaterThan(0)
  }, 300_000)

  it('table: the scroll-area demo shape', () => {
    for (const clip of ['visible', 'hidden', 'scroll', 'none'] as const) {
      const run = frame(<Table clip={clip} />)
      console.log(`[clipped.perf] table ${COLUMNS}x${ROWS} ${clip} elements=${run.elements} | ${formatSummary(run.summary)}`)
      expect(run.elements).toBeGreaterThan(0)
    }
  }, 300_000)
})
