// @ts-nocheck
/// The caret of a focused input blinks while it can be seen, and asks for no frame while it cannot (r17): a caret nobody
/// sees is not painted again twice a second. Whether it can be seen is decided in the prepaint of the editor, from the box of
/// the caret and the content mask. The mask is in window space and the box in the space the transform around it leaves
/// the layout in, so under a transform the two were compared in different places: a caret a translation, a scale or a
/// rotation put inside the window was judged outside it and stopped blinking, and one they put outside the window or a
/// clip blinked on, drawing a frame for nothing every 500 ms.
///
/// Chrome has no observable for the blink timer of a caret. What it fixes is the geometry: a caret blinks if it is inside
/// the box of the input and every clip around it, where the transform draws them, and that is the oracle here. The frames
/// drawn in 500 ms are the observable (one for a caret that changes phase, none for one that is not asked for).

import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip
const TIMEOUT = 180_000
const WINDOW = { width: 400, height: 300 }

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

async function settle(renderer, frames = 12) {
  for (let frame = 0; frame < frames; frame++) {
    renderer.flush()
    renderer.dispatchNativeEvents()
    await sleep(8)
  }
}

function wholeFrame(renderer) {
  renderer.flush()
  renderer.drawFrame()
}

/** The frames a window drew while the clock moved by 500 ms: whole ones and patched ones. */
function framesIn500ms(renderer): number {
  const before = renderer.getPaintedFrameCount() + renderer.getPatchCounters().patchedFrames
  renderer.advanceTime(500)
  renderer.drawFrame()
  return renderer.getPaintedFrameCount() + renderer.getPatchCounters().patchedFrames - before
}

const INPUT = { width: 100, height: 24 }
const field = (style: React.CSSProperties) => <input testId="probe" defaultValue="some" style={{ ...INPUT, ...style }} />
const at = (left: number, top: number, extra: React.CSSProperties = {}): React.CSSProperties => ({
  position: "absolute",
  left,
  top,
  ...extra,
})
/** A box that clips its content, and a transform of its own. The origin of a transform is the box it is on: the one of an input
 *  is written relative to it, to put it where the page has the origin of the window. */
const clip = (style: React.CSSProperties, child: React.ReactNode) => (
  <div style={{ position: "absolute", width: 100, height: 40, overflow: "hidden", transformOrigin: "0 0", ...style }}>{child}</div>
)
const box = (transform: string, child: React.ReactNode) => (
  <div style={{ position: "absolute", left: 0, top: 0, width: 10, height: 10, transformOrigin: "0 0", transform }}>{child}</div>
)

type Case = { name: string; page: React.ReactNode; blinks: boolean }

const cases: Case[] = [
  { name: "plain, inside the window", page: field(at(20, 20)), blinks: true },
  { name: "plain, outside the window", page: field(at(500, 20)), blinks: false },
  { name: "plain, inside a clip", page: clip({ left: 0, top: 0 }, field({ ...INPUT, width: 90 })), blinks: true },
  { name: "plain, below its clip", page: clip({ left: 0, top: 0 }, field({ marginTop: 60 })), blinks: false },

  // translation
  { name: "translate: ancestor draws a box laid out outside the window inside it", page: box("translate(-400px, 0)", field(at(500, 20))), blinks: true },
  { name: "translate: ancestor draws a box laid out inside the window outside it", page: box("translate(500px, 0)", field(at(20, 20))), blinks: false },
  { name: "translate: own transform draws a box laid out outside the window inside it", page: field(at(500, 20, { transform: "translate(-400px, 0)" })), blinks: true },
  { name: "translate: own transform draws a box laid out inside the window outside it", page: field(at(20, 20, { transform: "translate(500px, 0)" })), blinks: false },
  { name: "translate: a clip that moved with the input leaves it", page: clip({ left: 0, top: 0, transform: "translate(0, 100px)" }, field({ width: 90, marginTop: 10 })), blinks: true },
  { name: "translate: a clip around an input that its own transform moves out of it", page: clip({ left: 0, top: 0 }, field({ transform: "translate(0, 100px)" })), blinks: false },
  { name: "translate: a clip around an input that its own transform moves into it", page: clip({ left: 0, top: 0 }, field({ marginTop: 120, transform: "translate(0, -110px)" })), blinks: true },
  { name: "translate: a clip that moved with the input takes it away", page: clip({ left: 0, top: 0, transform: "translate(0, 100px)" }, field({ marginTop: 60 })), blinks: false },

  // scale
  { name: "scale: ancestor draws a box laid out inside the window outside it", page: box("scale(2)", field(at(300, 20))), blinks: false },
  { name: "scale: ancestor draws a box laid out outside the window inside it", page: box("scale(0.5)", field(at(500, 20, { width: 200 }))), blinks: true },
  { name: "scale: own transform draws a box laid out inside the window outside it", page: field(at(300, 20, { transform: "scale(2)", transformOrigin: "-300px 0" })), blinks: false },
  { name: "scale: own transform draws a box laid out outside the window inside it", page: field(at(500, 20, { width: 200, transform: "scale(0.5)", transformOrigin: "-500px 0" })), blinks: true },
  { name: "scale: a clip scaled with the input leaves it", page: clip({ left: 0, top: 0, transform: "scale(2)" }, field({ width: 40, marginTop: 4 })), blinks: true },
  { name: "scale: a clip scaled with the input takes it away", page: clip({ left: 0, top: 0, transform: "scale(2)" }, field({ marginTop: 60 })), blinks: false },
  { name: "scale: a clip around an input that its own transform moves out of it", page: clip({ left: 0, top: 0 }, field({ width: 40, transform: "scale(4)", transformOrigin: "0 -12px", marginTop: 12 })), blinks: false },

  // rotation
  { name: "rotate: ancestor draws a box laid out inside the window outside it", page: box("rotate(90deg)", field(at(100, 50))), blinks: false },
  { name: "rotate: ancestor draws a box laid out outside the window inside it", page: box("rotate(90deg)", field(at(20, -120))), blinks: true },
  { name: "rotate: own transform draws a box laid out inside the window outside it", page: field(at(100, 50, { transform: "rotate(90deg)", transformOrigin: "-100px -50px" })), blinks: false },
  { name: "rotate: own transform draws a box laid out outside the window inside it", page: field(at(20, -120, { transform: "rotate(90deg)", transformOrigin: "-20px 120px" })), blinks: true },
  { name: "rotate: a clip rotated with the input leaves it", page: clip({ left: 0, top: 0, transform: "translate(300px, 0) rotate(90deg)" }, field({ width: 50, marginTop: 10 })), blinks: true },
  { name: "rotate: a clip rotated with the input takes it away", page: clip({ left: 0, top: 0, transform: "translate(300px, 0) rotate(90deg)" }, field({ marginTop: 60 })), blinks: false },
  { name: "rotate: 45 degrees, inside the window", page: field(at(150, 100, { transform: "rotate(45deg)" })), blinks: true },
]

describeNative("the caret of a focused input under a transform", { timeout: TIMEOUT }, () => {
  for (const item of cases) {
    it(`${item.blinks ? "blinks" : "asks for no frame"}: ${item.name}`, async () => {
      const root = createTestRoot(WINDOW)
      const renderer = root.renderer
      renderer.clockPause()
      root.render(<div style={{ position: "relative", width: WINDOW.width, height: WINDOW.height }}>{item.page}</div>)
      await settle(renderer)
      try {
        renderer.focusElement(renderer.findByTestId("probe").id)
        renderer.dispatchNativeEvents()
        renderer.drawFrame()
        wholeFrame(renderer)
        wholeFrame(renderer)
        const frames = [framesIn500ms(renderer), framesIn500ms(renderer), framesIn500ms(renderer), framesIn500ms(renderer)]
        expect(frames, item.name).toEqual(item.blinks ? [1, 1, 1, 1] : [0, 0, 0, 0])
      } finally {
        root.unmount()
      }
    })
  }
})
