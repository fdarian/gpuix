/**
 * What `overflow` clips cost per frame. A grid of cards, each a clip inside a clip, in three
 * shapes: no radii at all, rounded clips whose corners line up (the nested clip is exact with
 * the cheap per-primitive mask), and a square clip inset into a rounded one so that its corner
 * falls in the cut-away corner of the parent (exact only through an offscreen group). The first
 * two must not cost more than they did before nested clips were exact; the third pays one
 * group per card, which is what the label reports.
 *
 * Skip without the native GPU test renderer. Excluded from `bun run test` by the
 * `*.perf.test.tsx` glob.
 */

import React from 'react'
import { describe, expect, it } from 'vitest'
import { flushSync } from '@ghia/react'
import { createTestRoot, hasNativeTestRenderer } from '@ghia/react/testing'

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const WARMUP = 5
const SAMPLES = 40
const CARDS = 360

type Shape = 'plain' | 'aligned' | 'cut'

const Card = React.memo(function Card({ shape, index }: { shape: Shape; index: number }) {
  const hue = (index * 37) % 360
  const outer: React.CSSProperties =
    shape === 'plain'
      ? { width: 90, height: 56, overflow: 'hidden' }
      : { width: 90, height: 56, overflow: 'hidden', borderRadius: 16 }
  // `cut` insets the inner clip by 3px: its square corner lies outside the r=16 arc.
  const padding = shape === 'cut' ? 3 : 0
  const innerRadius = shape === 'aligned' ? 10 : 0
  return (
    <div style={{ ...outer, padding, boxSizing: 'border-box', backgroundColor: '#202030' }}>
      <div
        style={{
          width: '100%',
          height: '100%',
          overflow: 'hidden',
          borderRadius: innerRadius,
          backgroundColor: `hsl(${hue} 60% 50%)`,
        }}
      >
        <div style={{ width: 200, height: 20, backgroundColor: `hsl(${hue} 80% 70%)` }} />
        <text style={{ color: '#ffffff', fontSize: 11 }}>{`card ${index}`}</text>
      </div>
    </div>
  )
})

function Cards({ shape, tick }: { shape: Shape; tick: number }) {
  return (
    <div style={{ width: 900, height: 700, display: 'flex', flexWrap: 'wrap', gap: 4, padding: 4 }}>
      <text style={{ width: '100%', color: '#000000' }}>{`tick ${tick}`}</text>
      {Array.from({ length: CARDS }, (_, index) => (
        <Card key={index} shape={shape} index={index} />
      ))}
    </div>
  )
}

function p(samples: number[], q: number) {
  const sorted = [...samples].sort((a, b) => a - b)
  return sorted[Math.min(sorted.length - 1, Math.ceil(q * sorted.length) - 1)]!
}

function frames(shape: Shape): number[] {
  const root = createTestRoot({ width: 900, height: 700 })
  try {
    root.render(<Cards shape={shape} tick={0} />)
    const frameMs: number[] = []
    for (let i = 0; i < WARMUP + SAMPLES; i++) {
      flushSync(() => root.root.render(<Cards shape={shape} tick={i + 1} />))
      const start = performance.now()
      root.renderer.flush()
      if (i >= WARMUP) frameMs.push(performance.now() - start)
    }
    return frameMs
  } finally {
    root.unmount()
  }
}

describeNative('frame cost of nested overflow clips', () => {
  for (const shape of ['plain', 'aligned', 'cut'] as const) {
    it(`${shape}: ${CARDS} cards`, () => {
      const samples = frames(shape)
      console.log(
        `[overflow-clip.perf] ${shape}: frame p50=${p(samples, 0.5).toFixed(2)}ms ` +
          `p95=${p(samples, 0.95).toFixed(2)}ms min=${Math.min(...samples).toFixed(2)}ms`,
      )
      expect(samples.length).toBe(SAMPLES)
    }, 120_000)
  }
})
