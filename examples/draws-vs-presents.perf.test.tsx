/**
 * Draws against frames when a pointer handler reads layout during an
 * animation (issue #39). A `motion.div` animates over a page of about 1000
 * elements while pointer moves arrive; each move's handler calls
 * `getBoundingClientRect` on the hovered container, as the demo's fluid hover
 * did on every move. A read answers from a fresh layout, so a dirty window
 * draws in the read, and that draw is not a frame the display shows. In the
 * demo's 3 s sidebar sweep: 298 draws for 171 presented frames.
 *
 * The test renderer has no display, so "presented" is the number of frames the
 * loop asked for. Counters, both native:
 *   draws     `getDebugFrameOverlayStats().frames`, every `Window::draw`
 *   painted   `getPaintedFrameCount()`, draws plus layout-only measure passes
 * A frame here is `clockFastForward(1/75 s)`, which draws once; draws/frame
 * above 1.00 is the waste. The real-window counterpart is `GPUIX_PRESENT_LOG`
 * (one line per frame that reaches the screen), which the test renderer does
 * not write.
 *
 * Handler variants, with 1 and 3 pointer moves per frame. `write` is the
 * control for the two that also read: a commit only marks the window dirty, so
 * their draws above `write` come from the reads. A read on a clean window (the
 * clock draw just ran) costs nothing, which is why `read` alone shows 1.00.
 * The first read after a commit draws, and `measure-only` counts the
 * layout-only passes that a read inside an input dispatch would use instead.
 *
 *   none             no handler
 *   read             reads the container box on every move
 *   write            commits the highlight position, no read
 *   write-then-read  commits the highlight position, then reads (a dirty window)
 *   read-then-write  reads, then commits the highlight position
 *
 * It profiles, it does not guard: no budgets. Run it through
 * `bun run test:perf:draws` (NODE_ENV=production, like `test:perf`).
 */

import React, { useRef, useState } from 'react'
import { describe, expect, it } from 'vitest'
import { flushSync, motion, type PublicInstance } from '@gpuix/react'
import { createTestRoot, hasNativeTestRenderer } from '@gpuix/react/testing'
import { formatSummary, sample, summarize } from './perf-harness'
import { C, CardGrid, LONG_TWEEN } from './perf-fixtures'

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const WINDOW = { width: 1240, height: 780 }
const FRAME_MS = 1000 / 75
const CARDS = 30
const WARMUP = 10
const SAMPLES = 100
const HOVER = { x: 20, y: 20, width: 600, height: 400 }

type Handler = 'none' | 'read' | 'write' | 'write-then-read' | 'read-then-write'

function Page({ handler }: { handler: Handler }) {
  const container = useRef<PublicInstance>(null)
  const [pointerX, setPointerX] = useState(0)
  return (
    <div style={{ position: 'relative', width: WINDOW.width, height: WINDOW.height, backgroundColor: C.bg, overflow: 'hidden' }}>
      <CardGrid cards={CARDS} />
      <motion.div
        style={{ position: 'absolute', left: 700, top: 20, width: 40, height: 40, borderRadius: 8, backgroundColor: '#ffffff', opacity: 0.2 }}
        initial={{ top: 20 }}
        animate={{ top: 700 }}
        transition={LONG_TWEEN}
      />
      <div
        ref={container}
        testId="hover"
        style={{ position: 'absolute', left: HOVER.x, top: HOVER.y, width: HOVER.width, height: HOVER.height }}
        onPointerMove={(event) => {
          const x = event.clientX
          if (handler === 'write' || handler === 'write-then-read') flushSync(() => setPointerX(x))
          if (handler === 'read' || handler === 'write-then-read' || handler === 'read-then-write') container.current!.getBoundingClientRect()
          if (handler === 'read-then-write') flushSync(() => setPointerX(x))
        }}
      >
        <div style={{ position: 'absolute', left: pointerX, top: 0, width: 4, height: 4, backgroundColor: '#e00' }} />
      </div>
    </div>
  )
}

describeNative('draws against frames under pointer moves and an animation', () => {
  for (const moves of [1, 3]) {
    it(`${moves} move(s) per frame`, () => {
      for (const handler of ['none', 'read', 'write', 'write-then-read', 'read-then-write'] as const) {
        const root = createTestRoot(WINDOW)
        try {
          root.render(<Page handler={handler} />)
          const renderer = root.renderer
          renderer.clockPause()
          const draws = () => renderer.getDebugFrameOverlayStats().frames
          const painted = () => renderer.getPaintedFrameCount()
          let drawsAtStart = 0
          let paintedAtStart = 0
          let step = 0
          const frames = sample(
            (index) => {
              if (index === WARMUP) {
                drawsAtStart = draws()
                paintedAtStart = painted()
              }
              renderer.clockFastForward(FRAME_MS)
              for (let move = 0; move < moves; move++) {
                step++
                renderer.dispatchMouseMove(HOVER.x + 10 + (step % 500), HOVER.y + 100)
              }
            },
            WARMUP,
            SAMPLES,
          )
          const drawn = draws() - drawsAtStart
          const measured = painted() - paintedAtStart - drawn
          console.log(
            `[draws.perf] moves/frame=${moves} handler=${handler} frames=${SAMPLES} ` +
              `draws=${drawn} (${(drawn / SAMPLES).toFixed(2)}/frame) measure-only=${measured} | ${formatSummary(summarize(frames))}`,
          )
          expect(drawn).toBeGreaterThanOrEqual(SAMPLES)
        } finally {
          root.unmount()
        }
      }
    }, 300_000)
  }
})
