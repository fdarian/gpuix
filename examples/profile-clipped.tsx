/**
 * Loops the frame of a clipped 2000-box container so a sampling profiler has something to look at.
 *
 *   bun profile-clipped.tsx &
 *   sample $! 5 -file /tmp/clipped.txt
 *
 * `CLIP=hidden|visible|scroll` picks the container (default hidden). `A11Y=off` drops the test
 * window's pretend screen reader, so no accessibility tree is built.
 */

import React from 'react'
import { createTestRoot } from '@ghia/react/testing'

const clip = process.env.CLIP ?? 'hidden'
const seconds = Number(process.env.SECONDS ?? '12')
const root = createTestRoot({ width: 1240, height: 1100 })
root.renderer.setA11yActive(process.env.A11Y !== 'off')
root.render(
  <div style={{ width: 1240, height: 1100 }}>
    <div
      style={{
        display: 'flex',
        flexDirection: 'row',
        flexWrap: 'wrap',
        alignContent: 'flex-start',
        width: clip === 'visible' ? 1180 : 400,
        ...(clip === 'hidden' ? { height: 300, overflow: 'hidden' } : {}),
        ...(clip === 'scroll' ? { height: 300, overflow: 'auto' } : {}),
      }}
    >
      {Array.from({ length: 2000 }, (_, index) => (
        <div key={index} style={{ width: 20, height: 20, margin: 2, backgroundColor: '#8a8a8a' }} />
      ))}
    </div>
  </div>,
)
root.renderer.flush()

const end = performance.now() + seconds * 1000
let frames = 0
while (performance.now() < end) {
  root.renderer.flush()
  frames++
}
console.log(JSON.stringify({ clip, frames }))
process.exit(0)
