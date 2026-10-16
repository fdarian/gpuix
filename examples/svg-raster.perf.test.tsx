/**
 * What a large `<svg>` costs the frame in which its size or colour changes. The
 * document is a few thousand shapes at 1200x1200: parsing it and rasterising it
 * takes tens of milliseconds, so a raster in paint is a frame lost.
 *
 * Per change it reports two numbers. `draw` is the longest frame the native side
 * drew while the change was flushed (the window's draw samples: what a display
 * link frame costs). `flush` is the whole `renderer.flush()`, which also runs the
 * background executor until it is parked, so it includes a raster that was moved
 * off the frame: it shows that the work still happens, `draw` shows that it no
 * longer blocks. A small icon is there as the control.
 *
 * It profiles, it does not guard: no budgets. Run it through
 * `bun run test:perf:svg` (NODE_ENV=production). Skip without the native GPU test
 * renderer. Excluded from `bun run test` by the `*.perf.test.tsx` glob.
 */

import React from 'react'
import { describe, it } from 'vitest'
import { flushSync } from '@gpuix/react'
import { createTestRoot, hasNativeTestRenderer } from '@gpuix/react/testing'

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const SHAPES = 3000
const WARMUP = 2
const SAMPLES = 10

function random(seed: number) {
  let state = seed
  return () => {
    state = (state * 1664525 + 1013904223) % 4294967296
    return state / 4294967296
  }
}

function bigDocument() {
  const next = random(7)
  const at = () => Math.round(next() * 1200)
  const hex = () => Math.floor(next() * 0xffffff).toString(16).padStart(6, '0')
  const shapes: string[] = []
  for (let index = 0; index < SHAPES; index++) {
    const fill = index % 7 === 0 ? 'url(#g)' : `#${hex()}`
    const opacity = (0.3 + next() * 0.7).toFixed(2)
    switch (index % 3) {
      case 0:
        shapes.push(
          `<path d="M${at()} ${at()} C${at()} ${at()} ${at()} ${at()} ${at()} ${at()} S${at()} ${at()} ${at()} ${at()} Z" fill="${fill}" fill-opacity="${opacity}" stroke="#${hex()}" stroke-width="2"/>`,
        )
        break
      case 1:
        shapes.push(`<circle cx="${at()}" cy="${at()}" r="${10 + Math.round(next() * 60)}" fill="${fill}" fill-opacity="${opacity}"/>`)
        break
      default:
        shapes.push(
          `<rect x="${at()}" y="${at()}" width="${20 + Math.round(next() * 120)}" height="${20 + Math.round(next() * 120)}" rx="8" fill="${fill}" fill-opacity="${opacity}" stroke="currentColor" stroke-width="1"/>`,
        )
    }
  }
  return (
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1200 1200">' +
    '<defs><linearGradient id="g"><stop offset="0" stop-color="#ff7e5f"/><stop offset="1" stop-color="#3a6fdd"/></linearGradient></defs>' +
    shapes.join('') +
    '</svg>'
  )
}

const ICON =
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/></svg>'

function Page({ markup, side, color }: { markup: string; side: number; color: string }) {
  return (
    <div style={{ display: 'flex', width: 1300, height: 1300 }}>
      <svg source={markup} style={{ width: side, height: side, color }} />
    </div>
  )
}

function percentile(samples: number[], q: number) {
  const sorted = [...samples].sort((a, b) => a - b)
  return sorted[Math.min(sorted.length - 1, Math.ceil(q * sorted.length) - 1)]!
}

function summary(samples: number[]) {
  return `p50=${percentile(samples, 0.5).toFixed(2)}ms min=${Math.min(...samples).toFixed(2)}ms max=${Math.max(...samples).toFixed(2)}ms`
}

const SCENARIOS: Record<string, { markup: () => string; sides: [number, number]; change: 'size' | 'colour' }> = {
  'big svg, size change': { markup: bigDocument, sides: [1200, 1180], change: 'size' },
  'big svg, colour change': { markup: bigDocument, sides: [1200, 1200], change: 'colour' },
  'icon, size change': { markup: () => ICON, sides: [24, 26], change: 'size' },
}

describeNative('svg raster cost per changed frame', () => {
  for (const [name, scenario] of Object.entries(SCENARIOS)) {
    it(name, () => {
      const markup = scenario.markup()
      const root = createTestRoot({ width: 1300, height: 1300 })
      const draw: number[] = []
      const flush: number[] = []
      for (let step = 0; step < WARMUP + SAMPLES; step++) {
        const side = scenario.sides[step % 2]!
        const color = scenario.change === 'colour' && step % 2 === 1 ? '#33aa77' : '#cc3355'
        root.renderer.resetDebugFrameOverlayStats()
        const start = performance.now()
        flushSync(() => root.root.render(<Page markup={markup} side={side} color={color} />))
        root.renderer.flush()
        const elapsed = performance.now() - start
        if (step >= WARMUP) {
          flush.push(elapsed)
          draw.push(root.renderer.getDebugFrameOverlayStats().maxMs!)
        }
      }
      console.log(`[svg-raster.perf] ${name}: draw ${summary(draw)} | flush ${summary(flush)}`)
      root.unmount()
    }, 120_000)
  }
})
