/**
 * Layout cost of nested auto-width flex columns (issue #36). The shape is the
 * demo's `perf-micro depth-col`: D nested `display: flex; flex-direction:
 * column` divs around one 20x20 box, N such instances inside a flex column.
 * Before the taffy measure cache was made fully associative the cost doubled
 * per level (11 us at D=1, 6174 us at D=12 per instance); now it is linear.
 *
 * The frame is `renderer.flush()`: it rebuilds the root view and draws inline,
 * so it covers the gpuix build, GPUI/taffy layout, prepaint and paint, with
 * nothing else changing. Per-instance cost is (p50 at N - p50 at 0) / N.
 * The `row`, `block` and `col-full` kinds are the demo's controls: they do not
 * blow up.
 *
 * The one budget is a guard against that blow-up: D=12 may cost at most
 * `LINEAR_SLACK` times the linear extrapolation of D=1. Run it through
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
const GUARD_DEPTH = 12
// Linear growth measures about 0.7x the extrapolation; the exponential bug measured about 47x.
const LINEAR_SLACK = 3
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
    const perInstanceUsByDepth = new Map<number, number>()
    for (const entry of cases) {
      const run = frameP50(INSTANCES, entry.depth, entry.kind)
      const perInstanceUs = ((run.summary.cpuP50 - empty.summary.cpuP50) / INSTANCES) * 1000
      if (entry.kind === 'col') perInstanceUsByDepth.set(entry.depth, perInstanceUs)
      console.log(
        `[depth-col.perf] ${entry.kind} D=${entry.depth} N=${INSTANCES} elements=${run.elements} ` +
          `per-instance=${perInstanceUs.toFixed(0)}us | ${formatSummary(run.summary)}`,
      )
    }
    expect(empty.elements).toBeGreaterThan(0)

    const shallow = perInstanceUsByDepth.get(1)!
    const deep = perInstanceUsByDepth.get(GUARD_DEPTH)!
    const budget = LINEAR_SLACK * GUARD_DEPTH * shallow
    expect(
      deep,
      `D=${GUARD_DEPTH} costs ${deep.toFixed(0)}us per instance, over ${budget.toFixed(0)}us (${LINEAR_SLACK}x linear in D=1's ${shallow.toFixed(0)}us)`,
    ).toBeLessThan(budget)
  }, 300_000)
})
