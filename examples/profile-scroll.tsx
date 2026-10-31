/**
 * Frames of a scroller whose offset moves every frame, so a retained root under it never stays
 * where it was recorded, against the same frames with retention off, interleaved in one process.
 *
 *   bun profile-scroll.tsx                 # p50 per shape and loop, retention on and off
 *   SHAPE=cards LOOP=scroll SECONDS=12 bun profile-scroll.tsx &  sample $! 5 -file /tmp/scroll.txt
 *
 * `SHAPE=clipped|boxes|cards`, `LOOP=scroll|flush`. `clipped` is the scroll case of
 * `clipped.perf.test.tsx`; `scroll` moves the scroller by 3px and draws once, as a display link does.
 */

import React from 'react'
import { createTestRoot } from '@ghia/react/testing'
import { formatRetained, percentile, retainedDelta } from './perf-harness'

type Shape = 'clipped' | 'boxes' | 'cards'
type Loop = 'scroll' | 'flush'

const BOXES = Array.from({ length: 2000 }, (_, index) => (
  <div key={index} style={{ width: 20, height: 20, margin: 2, backgroundColor: '#8a8a8a' }} />
))

function Page({ shape }: { shape: Shape }) {
  if (shape === 'clipped') {
    return (
      <div style={{ width: 1240, height: 1100 }}>
        <div
          testId="scroller"
          style={{ width: 400, height: 300, overflowY: 'auto', overflowX: 'auto', display: 'flex', flexDirection: 'row', flexWrap: 'wrap', alignContent: 'flex-start' }}
        >
          {BOXES}
        </div>
      </div>
    )
  }
  return (
    <div style={{ width: 1240, height: 1100 }}>
      <div testId="scroller" style={{ width: 400, height: 300, overflowY: 'auto' }}>
        {shape === 'boxes' ? (
          <div style={{ display: 'flex', flexDirection: 'row', flexWrap: 'wrap', alignContent: 'flex-start' }}>{BOXES}</div>
        ) : (
          Array.from({ length: 200 }, (_, index) => (
            <div key={index} style={{ height: 60, padding: 8, borderBottom: '1px solid #333', display: 'flex', flexDirection: 'row', gap: 8 }}>
              <div style={{ width: 40, height: 40, borderRadius: 20, backgroundColor: '#456' }} />
              <div style={{ display: 'flex', flexDirection: 'column' }}>
                <text style={{ fontSize: 14, color: '#fff' }}>{`Card ${index}`}</text>
                <text style={{ fontSize: 12, color: '#aaa' }}>{`Some description for card ${index}`}</text>
              </div>
            </div>
          ))
        )}
      </div>
    </div>
  )
}

function run(shape: Shape, loop: Loop, seconds: number) {
  const root = createTestRoot({ width: 1240, height: 1100 })
  const renderer = root.renderer
  renderer.setA11yActive(process.env.A11Y !== 'off' && process.env.A11Y !== undefined)
  root.render(<Page shape={shape} />)
  renderer.flush()
  const scroller = renderer.findByTestId('scroller')!
  let top = 0
  // -1: the first frame flips it, and scrolling up from 0 clamps (#101).
  let direction = -1
  let frame = 0
  const step = () => {
    if (loop === 'scroll') {
      if (frame++ % 80 === 0) direction = -direction
      top += direction * 3
      renderer.scrollTo(scroller.id, 0, top, 'instant')
      renderer.drawFrame()
    } else {
      renderer.flush()
    }
  }
  renderer.setInlineDraws(loop !== 'scroll')
  if (loop === 'scroll') renderer.drawFrame()

  if (seconds > 0) {
    const end = performance.now() + seconds * 1000
    while (performance.now() < end) step()
    root.unmount()
    return
  }
  const times = { on: [] as number[], off: [] as number[] }
  const before = renderer.getRetainedCounters()
  for (let round = 0; round < Number(process.env.ROUNDS ?? 30); round++) {
    for (const mode of round % 2 === 0 ? (['on', 'off'] as const) : (['off', 'on'] as const)) {
      renderer.setRetainEnabled(mode === 'on')
      for (let i = 0; i < 40; i++) {
        const start = performance.now()
        step()
        if (i >= 8) times[mode].push(performance.now() - start)
      }
    }
  }
  const on = percentile(times.on, 0.5)
  const off = percentile(times.off, 0.5)
  const onMin = Math.min(...times.on)
  const offMin = Math.min(...times.off)
  console.log(
    `[scroll] ${shape} ${loop} on=${on.toFixed(2)}ms off=${off.toFixed(2)}ms ratio=${(on / off).toFixed(3)} min ratio=${(onMin / offMin).toFixed(3)} | ${formatRetained(retainedDelta(renderer.getRetainedCounters(), before), 1)}`,
  )
  root.unmount()
}

const seconds = Number(process.env.SECONDS ?? '0')
if (seconds > 0) {
  run((process.env.SHAPE as Shape) ?? 'clipped', (process.env.LOOP as Loop) ?? 'scroll', seconds)
} else {
  for (const shape of ['clipped', 'boxes', 'cards'] as const) for (const loop of ['flush', 'scroll'] as const) run(shape, loop, 0)
}
process.exit(0)
