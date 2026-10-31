/**
 * Loops the frame that follows a header-only update over the chat-shaped list,
 * so a sampling profiler has something to look at.
 *
 *   bun profile-rerender.tsx &
 *   sample $! 5 -file /tmp/rerender.txt
 *
 * `MODE=commit` loops React's commit instead of the native frame.
 */

import React from 'react'
import { flushSync } from '@ghia/react'
import { createTestRoot } from '@ghia/react/testing'
import { App } from './rerender-fixture'

const mode = process.env.MODE ?? 'frame'
const seconds = Number(process.env.SECONDS ?? '12')
const root = createTestRoot({ width: 900, height: 700 })
root.render(<App tick={0} streaming="done" />)
root.renderer.flush()

const end = performance.now() + seconds * 1000
let tick = 0
let frames = 0
let spent = 0
while (performance.now() < end) {
  const start = performance.now()
  flushSync(() => root.root.render(<App tick={++tick} streaming="done" />))
  if (mode === 'frame') {
    const committed = performance.now()
    root.renderer.flush()
    spent += performance.now() - committed
  } else {
    spent += performance.now() - start
  }
  frames++
}
console.log(JSON.stringify({ mode, frames, avgMs: +(spent / frames).toFixed(2) }))
process.exit(0)
