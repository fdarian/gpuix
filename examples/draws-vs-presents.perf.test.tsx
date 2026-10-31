/**
 * Draws against frames when a pointer handler reads layout during an
 * animation (issue #39). A `motion.div` animates over a page of about 1000
 * elements while pointer moves arrive; each move's handler calls
 * `getBoundingClientRect` on the hovered container, as the demo's fluid hover
 * did on every move. A read answers from a fresh layout. It used to draw in the
 * read, and that draw was not a frame the display shows (the demo's 3 s sidebar
 * sweep: 298 draws for 171 presented frames). A browser forces a layout and
 * paints once at the frame, and so does a read now: it is a layout-only pass,
 * and the frame that follows is the only draw.
 *
 * The test window draws a dirty window whenever an update or an input flushes,
 * which a display link never does. The test turns that off (`setInlineDraws`)
 * and ends each frame with `drawFrame`, the display link's one draw, so that
 * what is counted is what a live window pays. Counters, both native:
 *   draws     `getDebugFrameOverlayStats().frames`, every `Window::draw`
 *   passes    `getLayoutPassCount()`, the layout-only passes of reads
 * A frame is `clockFastForward(1/75 s)`, the moves, then `drawFrame`.
 *
 * Handler variants, with 1 and 3 pointer moves per frame. A commit only marks
 * the window dirty and a clean window needs no pass, so the first read after a
 * change lays out and the reads that follow it with nothing invalidated in
 * between share that pass.
 *
 *   none             no handler
 *   read             reads the container box on every move
 *   write            commits the highlight position, no read
 *   write-then-read  commits the highlight position, then reads (a dirty window)
 *   read-then-write  reads, then commits the highlight position
 *
 * Guards: no variant draws more than once per frame (1.05 for slack), the
 * layout passes are as many as invalidations a read saw, and a frame with a
 * read costs less than 3x the same frame without it (`read` against `none`,
 * `write-then-read` and `read-then-write` against `write`). A browser's forced
 * layout redoes only what is dirty, and so does the pass: the subtrees the
 * last frame recorded as unchanged are skipped and publish their boxes from
 * the recording instead of being built (the page is the same one every frame
 * but for the highlight and the dot). Before that, the pass built every
 * element: `write-then-read` was 6.7 ms against 1.6 ms for `write` with one
 * move per frame, and 18.1 ms against 2.5 ms with three. Run it through
 * `bun run test:perf:draws` (NODE_ENV=production, like `test:perf`); the
 * counts do not depend on the build, the timings do.
 */

import React, { useRef, useState } from 'react'
import { describe, expect, it } from 'vitest'
import { flushSync, motion, type PublicInstance } from '@ghia/react'
import { createTestRoot, hasNativeTestRenderer } from '@ghia/react/testing'
import { formatSummary, sample, summarize, type Summary } from './perf-harness'
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

/** The layout passes a frame needs: one per invalidation a read saw. */
function expectedPassesPerFrame(handler: Handler, moves: number): number {
  switch (handler) {
    case 'none':
    case 'write':
      return 0
    // The clock tick dirties the window once; reads after it find nothing new.
    case 'read':
      return 1
    // Each move's write invalidates what the read before it laid out.
    case 'write-then-read':
      return moves
    // The first read finds the tick's invalidation, the later ones the last move's write.
    case 'read-then-write':
      return moves
  }
}

describeNative('draws against frames under pointer moves and an animation', () => {
  for (const moves of [1, 3]) {
    it(`${moves} move(s) per frame`, () => {
      const summaries = new Map<Handler, Summary>()
      for (const handler of ['none', 'read', 'write', 'write-then-read', 'read-then-write'] as const) {
        const root = createTestRoot(WINDOW)
        try {
          root.render(<Page handler={handler} />)
          const renderer = root.renderer
          renderer.clockPause()
          renderer.setInlineDraws(false)
          renderer.drawFrame()
          const draws = () => renderer.getDebugFrameOverlayStats().frames
          const passes = () => renderer.getLayoutPassCount()
          let drawsAtStart = 0
          let passesAtStart = 0
          let step = 0
          const frames = sample(
            (index) => {
              if (index === WARMUP) {
                drawsAtStart = draws()
                passesAtStart = passes()
              }
              renderer.clockFastForward(FRAME_MS)
              for (let move = 0; move < moves; move++) {
                step++
                renderer.dispatchMouseMove(HOVER.x + 10 + (step % 500), HOVER.y + 100)
              }
              renderer.drawFrame()
            },
            WARMUP,
            SAMPLES,
          )
          const drawn = draws() - drawsAtStart
          const laidOut = passes() - passesAtStart
          summaries.set(handler, summarize(frames))
          console.log(
            `[draws.perf] moves/frame=${moves} handler=${handler} frames=${SAMPLES} ` +
              `draws=${drawn} (${(drawn / SAMPLES).toFixed(2)}/frame) layout-passes=${laidOut} ` +
              `(${(laidOut / SAMPLES).toFixed(2)}/frame) | ${formatSummary(summarize(frames))}`,
          )
          expect(drawn, `${handler}: draws`).toBeGreaterThanOrEqual(SAMPLES)
          expect(drawn / SAMPLES, `${handler}: draws per frame`).toBeLessThanOrEqual(1.05)
          expect(laidOut, `${handler}: layout passes`).toBe(SAMPLES * expectedPassesPerFrame(handler, moves))
        } finally {
          root.renderer.setInlineDraws(true)
          root.unmount()
        }
      }
      // What a forced layout adds to the frame it is in.
      const cpu = (handler: Handler) => summaries.get(handler)!.cpuP50
      expect(cpu('read'), 'read against none').toBeLessThan(3 * cpu('none'))
      expect(cpu('write-then-read'), 'write-then-read against write').toBeLessThan(3 * cpu('write'))
      expect(cpu('read-then-write'), 'read-then-write against write').toBeLessThan(3 * cpu('write'))
      // With one move there is one write, which is a frame of its own: against a frame that changes nothing.
      if (moves === 1) expect(cpu('write-then-read'), 'write-then-read against none').toBeLessThan(3 * cpu('none'))
    }, 300_000)
  }
})
