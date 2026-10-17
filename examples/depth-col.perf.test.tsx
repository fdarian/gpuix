/**
 * Layout cost of nested auto-width flex columns (issue #36). The shape is the
 * demo's `perf-micro depth-col`: D nested `display: flex; flex-direction:
 * column` divs around one 20x20 box, N such instances inside a flex column.
 * The demo measured about x1.9 cost per level (22 us at D=1, 8930 us at D=12).
 *
 * The frame is `renderer.flush()`: it rebuilds the root view and draws inline,
 * so it covers the gpuix build, GPUI/taffy layout, prepaint and paint, with
 * nothing else changing. Per-instance cost is (p50 at N - p50 at 0) / N.
 * The `row`, `block` and `col-full` kinds are the demo's controls: they do not
 * blow up.
 *
 * It profiles, it does not guard: no budgets. Run it through
 * `bun run test:perf:depth` (NODE_ENV=production, like `test:perf`).
 */

import React from 'react'
import { describe, expect, it } from 'vitest'
import { createTestRoot, hasNativeTestRenderer } from '@gpuix/react/testing'
import { formatSummary, sample, summarize } from './perf-harness'

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const WINDOW = { width: 1200, height: 700 }
const INSTANCES = 8
const WARMUP = 5
const SAMPLES = 40

type Kind = 'col' | 'col-full' | 'row' | 'block'

const DEPTHS = [1, 4, 6, 8, 10, 12]
const CONTROLS: Array<{ kind: Kind; depth: number }> = [
  { kind: 'row', depth: 10 },
  { kind: 'block', depth: 10 },
  { kind: 'col-full', depth: 10 },
]

function levelStyle(kind: Kind): React.CSSProperties {
  switch (kind) {
    case 'col':
      return { display: 'flex', flexDirection: 'column' }
    case 'col-full':
      return { display: 'flex', flexDirection: 'column', width: '100%' }
    case 'row':
      return { display: 'flex', flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between' }
    case 'block':
      return {}
  }
}

function Nest({ depth, kind, children }: { depth: number; kind: Kind; children: React.ReactNode }) {
  let node = children
  for (let level = 0; level < depth; level++) {
    node = <div style={levelStyle(kind)}>{node}</div>
  }
  return <>{node}</>
}

function Page({ count, depth, kind }: { count: number; depth: number; kind: Kind }) {
  return (
    <div
      style={{
        width: WINDOW.width,
        height: WINDOW.height,
        display: 'flex',
        flexDirection: 'column',
        overflow: 'hidden',
      }}
    >
      {Array.from({ length: count }, (_, index) => (
        <Nest key={index} depth={depth} kind={kind}>
          <div style={{ width: 20, height: 20, margin: 2, backgroundColor: '#8a8a8a' }} />
        </Nest>
      ))}
    </div>
  )
}

function frameP50(count: number, depth: number, kind: Kind) {
  const root = createTestRoot(WINDOW)
  try {
    root.render(<Page count={count} depth={depth} kind={kind} />)
    const summary = summarize(sample(() => root.renderer.flush(), WARMUP, SAMPLES))
    return { summary, elements: root.renderer.getRetainedElementCount() }
  } finally {
    root.unmount()
  }
}

describeNative('nested auto-width flex columns: layout cost per instance', () => {
  it('depth scaling', () => {
    const empty = frameP50(0, 0, 'col')
    console.log(`[depth-col.perf] N=0 baseline: ${formatSummary(empty.summary)}`)
    const cases = [
      ...DEPTHS.map((depth) => ({ kind: 'col' as const, depth })),
      ...CONTROLS,
    ]
    for (const entry of cases) {
      const run = frameP50(INSTANCES, entry.depth, entry.kind)
      const perInstanceUs = ((run.summary.cpuP50 - empty.summary.cpuP50) / INSTANCES) * 1000
      console.log(
        `[depth-col.perf] ${entry.kind} D=${entry.depth} N=${INSTANCES} elements=${run.elements} ` +
          `per-instance=${perInstanceUs.toFixed(0)}us | ${formatSummary(run.summary)}`,
      )
    }
    expect(empty.elements).toBeGreaterThan(0)
  }, 300_000)
})
