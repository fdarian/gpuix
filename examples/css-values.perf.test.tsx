/**
 * What the CSS value strings cost per frame. Every frame rebuilds each element
 * from its style, and the gradient, filter, mask and clip-path strings are
 * parsed as part of that build. A grid of 500 cells carries one kind of string
 * per scenario, so the difference to the plain-colour scenario is that string's
 * share of the frame.
 *
 * Reports the native frame that follows a one-character text change, plus the
 * mount. No budgets: the point is to compare a change to the parsing against
 * the numbers from before it. Skip without the native GPU test renderer.
 * Excluded from `bun run test` by the `*.perf.test.tsx` glob.
 */

import React from 'react'
import { describe, it } from 'vitest'
import { flushSync } from '@ghia/react'
import { createTestRoot, hasNativeTestRenderer } from '@ghia/react/testing'

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const CELLS = 500
const WARMUP = 5
const SAMPLES = 40
const MOUNTS = 9

const VALUES = {
  background:
    'linear-gradient(135deg, #ff7e5f 0%, #feb47b 50%, rgba(10, 20, 30, 0.8) 100%), ' +
    'radial-gradient(circle at 30% 20%, rgba(255, 255, 255, 0.4), transparent 60%), ' +
    'conic-gradient(from 90deg at 50% 50%, red, yellow, lime, aqua, blue, magenta, red)',
  boxShadow:
    '0 1px 2px rgba(0, 0, 0, .2), 0 4px 8px rgba(0, 0, 0, .15), inset 0 0 0 1px rgba(255, 255, 255, .1)',
  filter: 'blur(4px) brightness(1.1) contrast(0.9) drop-shadow(0 2px 4px rgba(0, 0, 0, .3))',
  maskImage: 'linear-gradient(to bottom, black 80%, transparent)',
  clipPath: 'polygon(50% 0%, 100% 38%, 82% 100%, 18% 100%, 0% 38%)',
} as const

type Style = React.CSSProperties & Record<string, unknown>

const SCENARIOS: Record<string, Style> = {
  'plain colour': { backgroundColor: '#336699' },
  'gradient background': { backgroundImage: VALUES.background },
  'box-shadow': { backgroundColor: '#336699', boxShadow: VALUES.boxShadow },
  filter: { backgroundColor: '#336699', filter: VALUES.filter },
  'mask-image': { backgroundColor: '#336699', maskImage: VALUES.maskImage },
  'clip-path': { backgroundColor: '#336699', clipPath: VALUES.clipPath },
  'all five': {
    backgroundImage: VALUES.background,
    boxShadow: VALUES.boxShadow,
    filter: VALUES.filter,
    maskImage: VALUES.maskImage,
    clipPath: VALUES.clipPath,
  },
}

function Grid({ cell, tick }: { cell: Style; tick: number }) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', width: 1200, height: 800 }}>
      <text>{`tick ${tick}`}</text>
      <div style={{ display: 'flex', flexDirection: 'row', flexWrap: 'wrap', gap: 2 }}>
        {Array.from({ length: CELLS }, (_, index) => (
          <div key={index} style={{ width: 40, height: 20, ...cell }} />
        ))}
      </div>
    </div>
  )
}

function percentile(samples: number[], q: number) {
  const sorted = [...samples].sort((a, b) => a - b)
  return sorted[Math.min(sorted.length - 1, Math.ceil(q * sorted.length) - 1)]!
}

describeNative('css value strings, 500 elements', () => {
  for (const [name, cell] of Object.entries(SCENARIOS)) {
    it(name, () => {
      const mounts: number[] = []
      let root = createTestRoot({ width: 1200, height: 800 })
      for (let run = 0; run < MOUNTS; run++) {
        root.unmount()
        root = createTestRoot({ width: 1200, height: 800 })
        const start = performance.now()
        flushSync(() => root.root.render(<Grid cell={cell} tick={0} />))
        root.renderer.flush()
        mounts.push(performance.now() - start)
      }

      const frameMs: number[] = []
      for (let index = 1; index <= WARMUP + SAMPLES; index++) {
        flushSync(() => root.root.render(<Grid cell={cell} tick={index} />))
        const start = performance.now()
        root.renderer.flush()
        if (index > WARMUP) frameMs.push(performance.now() - start)
      }
      console.log(
        `[css-values.perf] ${name.padEnd(20)} mount min=${Math.min(...mounts).toFixed(1)}ms p50=${percentile(mounts, 0.5).toFixed(1)}ms ` +
          `frame min=${Math.min(...frameMs).toFixed(2)}ms p50=${percentile(frameMs, 0.5).toFixed(2)}ms ` +
          `p95=${percentile(frameMs, 0.95).toFixed(2)}ms`,
      )
      root.unmount()
    }, 120_000)
  }
})
