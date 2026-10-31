/**
 * What giving memory back costs a page that swaps between sizes (#112). The tables that grew for
 * a big page shrink when a smaller one leaves them over four times too big, to twice the use. A
 * swap by more than that ratio reallocates every time; a swap by less never does. Each swap is
 * one frame, and this prints the CPU time of the frame that shrinks (big to small), of the one
 * that grows back (small to big) and of a steady frame on each page, so a build with the
 * shrinking and one without can be put side by side. It prints and asserts nothing about time.
 * Run it through `bun run test:perf:swap` (NODE_ENV=production).
 */

import React from 'react'
import { describe, it } from 'vitest'
import { flushSync } from '@ghia/react'
import { createTestRoot, hasNativeTestRenderer, type TestRoot } from '@ghia/react/testing'
import { formatSummary, sample, summarize, type Sample } from './perf-harness'

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const WINDOW = { width: 1200, height: 800 }
const WARMUP = 6
const SWAPS = 40

function Row({ index }: { index: number }) {
  return (
    <div style={{ display: 'flex', flexDirection: 'row', gap: 4, height: 18, padding: 2, backgroundColor: index % 2 === 0 ? '#202428' : '#262b30' }}>
      <div style={{ width: 14, height: 14, borderRadius: 7, backgroundColor: '#4c9' }} />
      <text style={{ fontSize: 12, color: '#ddd' }}>{`row ${index}`}</text>
    </div>
  )
}

function Page({ rows, name }: { rows: number; name: string }) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', width: WINDOW.width, height: WINDOW.height, overflow: 'hidden' }} testId={name}>
      {Array.from({ length: rows }, (_, index) => (
        <Row key={`${name}-${index}`} index={index} />
      ))}
    </div>
  )
}

function show(root: TestRoot, rows: number, name: string) {
  flushSync(() => root.root.render(<Page rows={rows} name={name} />))
  root.renderer.flush()
}

function swapRun(label: string, bigRows: number, smallRows: number) {
  const root = createTestRoot(WINDOW)
  show(root, smallRows, 'small-start')
  const toSmall: Sample[] = []
  const toBig: Sample[] = []
  for (let i = 0; i < WARMUP + SWAPS; i++) {
    const up = sample(() => show(root, bigRows, `big-${i}`), 0, 1)
    const down = sample(() => show(root, smallRows, `small-${i}`), 0, 1)
    if (i >= WARMUP) {
      toBig.push(...up)
      toSmall.push(...down)
    }
  }
  const steadySmall = sample(() => root.renderer.flush(), 3, 20)
  show(root, bigRows, 'big-steady')
  const steadyBig = sample(() => root.renderer.flush(), 3, 20)
  console.log(`[page-swap.perf] ${label} (${bigRows} rows <-> ${smallRows} rows)`)
  console.log(`  big to small: ${formatSummary(summarize(toSmall))}`)
  console.log(`  small to big: ${formatSummary(summarize(toBig))}`)
  console.log(`  steady big:   ${formatSummary(summarize(steadyBig))}`)
  console.log(`  steady small: ${formatSummary(summarize(steadySmall))}`)
}

describeNative('a page that swaps between sizes', () => {
  it('swaps by more than the shrink ratio', () => {
    swapRun('over the ratio', 2500, 60)
  }, 300_000)

  it('swaps by less than the shrink ratio', () => {
    swapRun('under the ratio', 2500, 900)
  }, 300_000)
})
