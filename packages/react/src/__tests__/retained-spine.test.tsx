// @ts-nocheck
/**
 * A motion that runs where the elements on the way up to it are never built: a `display: contents`
 * wrapper, or what is below an `<svg>`. The roots above it must still be built each frame, or they
 * are replayed with the motion frozen. Run with GPUIX_RETAIN_VERIFY=1 and GPUIX_RETAIN_OFF=1 too.
 */
import React from "react"
import { expect, it } from "vitest"
import { motion } from "../index"
import { Card, describeNative, Page, setup, verifying } from "./retained-harness.js"

const retained = process.env.GPUIX_RETAIN_OFF === undefined
const CARD = 2

const slider = (to: number) => (
  <motion.div
    testId="fader"
    style={{ position: "absolute", left: 0, top: 0, width: 20, height: 10, backgroundColor: "#ff0000" }}
    initial={{ left: 0 }}
    animate={{ left: to }}
    transition={{ duration: 1, ease: "linear" }}
  />
)

/** The card among the others, in a padded div that is not a card: the root a replay starts from. */
const scene = (extra: React.ReactNode, wrapped = false) => {
  const card = <Card index={CARD} extra={extra} style={{ position: "relative" }} />
  return (
    <Page
      card={(index) =>
        index === CARD ? (
          <div key={index} testId="outer" style={{ padding: 4 }}>
            {wrapped ? <div style={{ display: "contents" }}>{card}</div> : card}
          </div>
        ) : null
      }
    />
  )
}

const redOfIcon = (view, name: string) => {
  const icon = view.bounds("icon")
  return view.sample(name, icon.x + icon.width / 2, icon.y + icon.height / 2)[0]
}

/** Frames after the motion settled: the page is replayed whole by each, so nothing on the way up to what
 *  once moved stays on the spine. */
const expectIdle = (view) => {
  const FRAMES = 6
  const built = view.built()
  const hits = view.counters().hits
  for (let frame = 0; frame < FRAMES; frame++) view.renderer.flush()
  if (!retained) return
  expect(view.counters().hits - hits).toBe(FRAMES)
  // A run that verifies builds every replayed frame once more.
  if (!verifying) expect(view.built() - built).toBe(0)
}

describeNative("a motion whose way up is not built", () => {
  for (const wrapped of [false, true]) {
    it(`keeps moving under a replayed root (wrapper: ${wrapped})`, () => {
      const view = setup(scene(slider(100), wrapped))
      try {
        view.renderer.clockPause()
        view.settle()
        const leftAt = () => {
          view.renderer.clockFastForward(250)
          return view.bounds("fader").x - view.bounds(`card-${CARD}`).x
        }
        const first = leftAt()
        const second = leftAt()
        expect(first).toBeGreaterThan(0)
        expect(second).toBeGreaterThan(first)
        view.finish(false)
      } finally {
        view.unmount()
      }
    })
  }

  it("keeps painting a motion inside an svg, then builds nothing above it", () => {
    const icon = (
      <svg testId="icon" viewBox="0 0 10 10" style={{ width: 30, height: 20 }}>
        <g>
          <motion.rect
            x={0}
            y={0}
            width={10}
            height={10}
            initial={{ fill: "#ff0000" }}
            animate={{ fill: "#0000ff" }}
            transition={{ duration: 1, ease: "linear" }}
          />
        </g>
      </svg>
    )
    const view = setup(scene(icon))
    try {
      view.renderer.clockPause()
      view.settle()
      const first = (view.renderer.clockFastForward(250), redOfIcon(view, "svg-first"))
      const second = (view.renderer.clockFastForward(250), redOfIcon(view, "svg-second"))
      expect(first).toBeLessThan(255)
      expect(second).toBeLessThan(first)
      view.renderer.clockFastForward(1500)
      view.settle()
      expectIdle(view)
      view.finish(false)
    } finally {
      view.unmount()
    }
  })

  it("moves again when it starts again, after it stopped", () => {
    const view = setup(scene(slider(100), true))
    try {
      view.renderer.clockPause()
      view.settle()
      view.renderer.clockFastForward(1500)
      view.settle()
      const rest = view.bounds("fader").x - view.bounds(`card-${CARD}`).x
      expect(rest).toBeGreaterThan(0)

      view.render(scene(slider(300), true))
      view.settle()
      const leftAt = () => {
        view.renderer.clockFastForward(250)
        return view.bounds("fader").x - view.bounds(`card-${CARD}`).x
      }
      const first = leftAt()
      const second = leftAt()
      expect(first).toBeGreaterThan(rest)
      expect(second).toBeGreaterThan(first)
      view.finish(false)
    } finally {
      view.unmount()
    }
  })

  it("builds nothing above the wrapper once the motion has settled", () => {
    const view = setup(scene(slider(100), true))
    try {
      view.renderer.clockPause()
      view.settle()
      view.renderer.clockFastForward(1500)
      view.settle()
      expectIdle(view)
      view.finish(false)
    } finally {
      view.unmount()
    }
  })
})
