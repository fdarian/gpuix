/**
 * Frame cost of container queries: a page of 500 container cards (about 3k
 * elements), each with descendants whose style is chosen by a query on the
 * card (`flexDirection`, `gap`, a font size, a padding) or by a container unit
 * (the thumbnail's width).
 *
 * Two variants build the same boxes at the same sizes:
 *   plain       no containers; the app picks the style from `wide` itself
 *   containers  `containerType` on every card, `@container` blocks and `cqw`
 *
 * and three things are timed for each:
 *   steady  a frame with nothing changed (`renderer.flush()`)
 *   flip    the page width changes, so every card crosses the 400px query: the
 *           frame that lays the cards out must already show the other style,
 *           which for `containers` means building again before it paints
 *   same    the page is rendered again at the width it has
 *
 * It profiles, it does not guard: no budgets. Run it through
 * `bun run test:perf:cq` (NODE_ENV=production, like `test:perf`). A frame is
 * `renderer.flush()`, which notifies the root view and draws inline, so it covers
 * the gpuix build, GPUI layout, prepaint and paint, and for `flip` the React
 * render, but not GPU submission (the test renderer has none).
 */

import React from 'react'
import { describe, expect, it } from 'vitest'
import { createTestRoot, hasNativeTestRenderer } from '@gpuix/react/testing'

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const CARDS = 500
const WINDOW = { width: 1600, height: 900 }
const WIDE_PAGE = 1600
const NARROW_PAGE = 1200
const WARMUP = 10
const SAMPLES = 60

type Variant = 'plain' | 'containers'

/** The style for a card of 25% of the page: 400px wide on the wide page, 300px on the narrow one. */
function pick(variant: Variant, wide: boolean, base: Record<string, unknown>, whenWide: Record<string, unknown>) {
  if (variant === 'containers') return { ...base, '@container': { '(min-width: 400px)': whenWide } }
  return wide ? { ...base, ...whenWide } : base
}

function Card({ index, variant, wide }: { index: number; variant: Variant; wide: boolean }) {
  const containers = variant === 'containers'
  return (
    <div
      style={{
        width: '25%',
        height: 130,
        padding: 8,
        backgroundColor: index % 2 === 0 ? '#161618' : '#1c1c1f',
        ...(containers ? { containerType: 'inline-size' } : {}),
      }}
    >
      <div
        style={pick(
          variant,
          wide,
          { display: 'flex', flexDirection: 'column', gap: 4 },
          { flexDirection: 'row', gap: 12 },
        )}
      >
        <div
          style={{
            // 30% of the card's content box: 117px on the wide page, 84px on the narrow one.
            width: containers ? '30cqw' : wide ? 117 : 84,
            height: 60,
            backgroundColor: '#2a2a2e',
          }}
        />
        <div style={{ display: 'flex', flexDirection: 'column', flexGrow: 1 }}>
          <text
            style={pick(variant, wide, { fontSize: 13, color: '#f2f2f3' }, { fontSize: 15 })}
          >{`Card ${index}`}</text>
          <div style={pick(variant, wide, { paddingTop: 2 }, { paddingTop: 6 })}>
            <text style={{ fontSize: 11, color: '#9a9aa1' }}>{`Description of card ${index}`}</text>
          </div>
        </div>
      </div>
    </div>
  )
}

function Page({ variant, wide }: { variant: Variant; wide: boolean }) {
  return (
    <div style={{ width: wide ? WIDE_PAGE : NARROW_PAGE, display: 'flex', flexDirection: 'row', flexWrap: 'wrap' }}>
      {Array.from({ length: CARDS }, (_, index) => (
        <Card key={index} index={index} variant={variant} wide={wide} />
      ))}
    </div>
  )
}

function p(samples: number[], q: number) {
  const sorted = [...samples].sort((a, b) => a - b)
  return sorted[Math.min(sorted.length - 1, Math.ceil(q * sorted.length) - 1)]!
}

interface Sample {
  wallMs: number
  cpuMs: number
}

// CPU time next to wall time: the machine this runs on is often shared, and a
// frame that waited for a core is not a slower frame.
function sample(run: (index: number) => void): Sample[] {
  const out: Sample[] = []
  for (let i = 0; i < WARMUP + SAMPLES; i++) {
    const cpu = process.cpuUsage()
    const start = performance.now()
    run(i)
    const wallMs = performance.now() - start
    const used = process.cpuUsage(cpu)
    if (i >= WARMUP) out.push({ wallMs, cpuMs: (used.user + used.system) / 1000 })
  }
  return out
}

const cpuP50: Record<string, number> = {}

function report(variant: Variant, label: string, samples: Sample[], elements: number) {
  const wall = samples.map((s) => s.wallMs)
  const cpu = samples.map((s) => s.cpuMs)
  cpuP50[`${variant} ${label}`] = p(cpu, 0.5)
  console.log(
    `[cq.perf] ${variant} ${label} elements=${elements} n=${samples.length} ` +
      `cpu p50=${p(cpu, 0.5).toFixed(2)}ms min=${Math.min(...cpu).toFixed(2)}ms | ` +
      `wall p50=${p(wall, 0.5).toFixed(2)}ms min=${Math.min(...wall).toFixed(2)}ms`,
  )
}

describeNative('container queries: 500 containers', () => {
  for (const variant of ['plain', 'containers'] as const) {
    it(`${variant}: steady frame, rendered again, and a flip of every container`, () => {
      const root = createTestRoot(WINDOW)
      root.render(<Page variant={variant} wide />)
      const renderer = root.renderer
      const elements = renderer.getRetainedElementCount()

      report(variant, 'steady', sample(() => renderer.flush()), elements)
      report(variant, 'same', sample(() => root.render(<Page variant={variant} wide />)), elements)
      report(
        variant,
        'flip',
        sample((index) => root.render(<Page variant={variant} wide={index % 2 === 1} />)),
        elements,
      )

      const probe = renderer.getPaintedFrameCount()
      root.render(<Page variant={variant} wide />)
      root.render(<Page variant={variant} wide={false} />)
      // Two renders, two painted frames: the passes before present paint nothing.
      expect(renderer.getPaintedFrameCount() - probe).toBe(2)
      expect(elements).toBeGreaterThan(CARDS * 5)
      root.unmount()
    }, 300_000)
  }

  it('summary', () => {
    const ratio = (label: string) => (cpuP50[`containers ${label}`]! / cpuP50[`plain ${label}`]!).toFixed(2)
    console.log(
      `[cq.perf] containers/plain cpu p50: steady x${ratio('steady')} same x${ratio('same')} flip x${ratio('flip')}; ` +
        `flip / steady (containers) x${(cpuP50['containers flip']! / cpuP50['containers steady']!).toFixed(2)}`,
    )
  })
})
