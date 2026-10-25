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
 * every box visible, by a margin well inside what it measures. Both groups run with the test
 * window's pretend screen reader on, which builds an accessibility tree that holds the clipped
 * boxes too, and off. Run it through
 * `bun run test:perf:clipped` (NODE_ENV=production, like `test:perf`).
 */

import React from 'react'
import { describe, expect, it } from 'vitest'
import { createTestRoot, hasNativeTestRenderer } from '@gpuix/react/testing'
import { formatSummary, percentile, sample, summarize, type Summary } from './perf-harness'

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

function frame(node: React.ReactNode, a11y: boolean): { summary: Summary; elements: number; culled: number } {
  const root = createTestRoot(WINDOW)
  try {
    root.renderer.setA11yActive(a11y)
    // These measure the cost of building and culling a clipped frame; a retained replay of a page
    // that does not change would skip the culling they count.
    root.renderer.setRetainEnabled(false)
    root.render(<div style={{ width: WINDOW.width, height: WINDOW.height }}>{node}</div>)
    const summary = summarize(sample(() => root.renderer.flush(), WARMUP, SAMPLES))
    const before = root.renderer.getCulledSubtreeCount()
    root.renderer.flush()
    return { summary, elements: root.renderer.getRetainedElementCount(), culled: root.renderer.getCulledSubtreeCount() - before }
  } finally {
    root.unmount()
  }
}

/**
 * What a frame of a scroller may cost with retention on against off. Retention records what it
 * builds; a scroller that nothing in it can be skipped from (rows that are too small to be a
 * subtree, or too big to be a root) stops recording for a while, so the two cost the same.
 * Measured: 1.00 to 1.02, which noise is of the order of. The rows here are plain boxes: replaying
 * one costs about what building it does, so they are not roots (`retain::MIN_ROOT`); a text is.
 */
const SCROLL_RETAIN_OVER_OFF = 1.05

/**
 * What a frame of a scroller whose rows a scroll moves may cost with retention on against off:
 * the rows are put where the scroll put them and not built again, so a frame that scrolls a few
 * pixels does a tenth of the work. Measured: 0.09 to 0.10 at 3 px a frame (1.2 ms against 13 ms),
 * with the scroller really moving, which `retentionOverOff` checks every frame (#101); the margin
 * is for a loaded machine.
 */
const SCROLL_RELOCATED_OVER_OFF = 0.5

/** What a frame of a scroller of one-element roots that cost more to build than to replay may cost with retention on against off: they are skipped. Measured: 0.4. */
const SCROLL_TEXTS_OVER_OFF = 0.6

function Texts() {
  return (
    <div style={{ width: 400, height: 300, overflowY: 'auto', overflowX: 'auto', display: 'flex', flexDirection: 'row', flexWrap: 'wrap', alignContent: 'flex-start' }}>
      {Array.from({ length: 2000 }, (_, index) => (
        <text key={index} style={{ fontSize: 12, color: '#fff', margin: 2 }}>{`t${index}`}</text>
      ))}
    </div>
  )
}

function Cards() {
  return (
    <div testId="scroller" style={{ width: 400, height: 300, overflowY: 'auto' }}>
      {Array.from({ length: 200 }, (_, index) => (
        <div key={index} style={{ height: 60, padding: 8, display: 'flex', flexDirection: 'row', gap: 8 }}>
          <div style={{ width: 40, height: 40, borderRadius: 20, backgroundColor: '#456' }} />
          <div style={{ display: 'flex', flexDirection: 'column' }}>
            <text style={{ fontSize: 14, color: '#fff' }}>{`Card ${index}`}</text>
            <text style={{ fontSize: 12, color: '#aaa' }}>{`Some description for card ${index}`}</text>
          </div>
        </div>
      ))}
    </div>
  )
}

const ROUNDS = 12
const FRAMES = 40

/** p50 of a frame with retention on and off, in blocks of frames that alternate so drift hits both. */
function retentionOverOff(node: React.ReactNode, a11y: boolean, scrolls: boolean): { on: number; off: number } {
  const root = createTestRoot(WINDOW)
  try {
    const renderer = root.renderer
    renderer.setA11yActive(a11y)
    root.render(<div style={{ width: WINDOW.width, height: WINDOW.height }}>{node}</div>)
    renderer.flush()
    const scroller = scrolls ? renderer.findByTestId('scroller')! : undefined
    if (scroller) {
      // A display-link frame per scroll, as a live window draws: not the draws a test makes inline.
      renderer.setInlineDraws(false)
      renderer.drawFrame()
    }
    // Starts downwards: the first frame flips the sign, so a start of 1 would scroll up from 0,
    // which clamps, and nothing would move (#101). The run goes 3 .. 240 .. 0 and back.
    let top = 0
    let direction = -1
    let count = 0
    const frame = () => {
      if (scroller) {
        if (count++ % 80 === 0) direction = -direction
        top += direction * 3
        renderer.scrollTo(scroller.id, 0, top, 'instant')
        renderer.drawFrame()
      } else {
        renderer.flush()
      }
    }
    const times = { on: [] as number[], off: [] as number[] }
    const scrolled = { on: 0, off: 0 }
    for (let round = 0; round < ROUNDS; round++) {
      for (const mode of round % 2 === 0 ? (['on', 'off'] as const) : (['off', 'on'] as const)) {
        renderer.setRetainEnabled(mode === 'on')
        for (let i = 0; i < FRAMES; i++) {
          const painted = renderer.getPaintedFrameCount()
          const start = performance.now()
          frame()
          const elapsed = performance.now() - start
          if (scroller) {
            const offset = renderer.getScrollOffset(scroller.id)
            if (!offset) throw new Error('the scroller has no scroll offset')
            expect(offset[1], `${mode}: frame ${count} scrolls to ${top}`).toBe(top)
            expect(renderer.getPaintedFrameCount() - painted, `${mode}: frame ${count} paints`).toBe(1)
            scrolled[mode]++
          }
          if (i >= 8) times[mode].push(elapsed)
        }
      }
    }
    if (scroller) {
      expect(count, 'the scroll covers a down and an up leg').toBeGreaterThan(160)
      expect(scrolled.on, 'every frame run with retention on scrolled and painted').toBe(ROUNDS * FRAMES)
      expect(scrolled.off, 'every frame run with retention off scrolled and painted').toBe(ROUNDS * FRAMES)
    }
    return { on: percentile(times.on, 0.5), off: percentile(times.off, 0.5) }
  } finally {
    root.unmount()
  }
}

/** What a clipped frame may cost against the same frame unclipped. Measured: 0.66 to 0.70. */
const CLIPPED_OVER_VISIBLE = 0.85

describeNative.each([{ a11y: true }, { a11y: false }])('clipped and off-screen content, a11y $a11y', ({ a11y }) => {
  it('boxes: per-box cost by clip', () => {
    const empty = frame(<Boxes count={0} clip="hidden" />, a11y)
    const largest: Partial<Record<Clip, ReturnType<typeof frame>>> = {}
    for (const clip of ['visible', 'hidden', 'scroll', 'none'] as const) {
      for (const count of BOX_COUNTS) {
        if (count === 0) continue
        const run = frame(<Boxes count={count} clip={clip} />, a11y)
        const perBoxUs = ((run.summary.cpuP50 - empty.summary.cpuP50) / count) * 1000
        console.log(
          `[clipped.perf] a11y=${a11y} boxes ${clip} N=${count} elements=${run.elements} skipped=${run.culled} per-box=${perBoxUs.toFixed(2)}us | ${formatSummary(run.summary)}`,
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

  const retentionShapes = [
    { name: 'boxes in a scroller, unchanged', node: <Boxes count={2000} clip="scroll" />, scrolls: false, limit: SCROLL_RETAIN_OVER_OFF },
    { name: 'texts in a scroller, unchanged', node: <Texts />, scrolls: false, limit: SCROLL_TEXTS_OVER_OFF },
    { name: 'cards in a scroller, moved by the scroll every frame', node: <Cards />, scrolls: true, limit: SCROLL_RELOCATED_OVER_OFF },
  ]
  // One test per shape: a failure in one does not hide the others.
  it.each(retentionShapes)('scroll: retention, $name', (shape) => {
    const { on, off } = retentionOverOff(shape.node, a11y, shape.scrolls)
    console.log(`[clipped.perf] a11y=${a11y} ${shape.name}: retention on=${on.toFixed(2)}ms off=${off.toFixed(2)}ms ratio=${(on / off).toFixed(3)}`)
    expect(on / off, `${shape.name}: ${on.toFixed(2)}ms against ${off.toFixed(2)}ms`).toBeLessThan(shape.limit)
  }, 300_000)

  it('table: the scroll-area demo shape', () => {
    const runs: Partial<Record<Clip, ReturnType<typeof frame>>> = {}
    for (const clip of ['visible', 'hidden', 'scroll', 'none'] as const) {
      const run = frame(<Table clip={clip} />, a11y)
      runs[clip] = run
      console.log(`[clipped.perf] a11y=${a11y} table ${COLUMNS}x${ROWS} ${clip} elements=${run.elements} skipped=${run.culled} | ${formatSummary(run.summary)}`)
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
