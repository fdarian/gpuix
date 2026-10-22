// @ts-nocheck
/**
 * A root that a frame builds whole is recorded by that frame, so the frame after replays it: the first build of a
 * page, and a build because the root changed (r11b). Before, the frame after built it again, to record it
 * then, and only the one after that replayed.
 *
 * Run with GPUIX_RETAIN_VERIFY=1 too: the counts of builds are not read there (every replayed frame is built a second
 * time), only that the frames and what they replay agree.
 */
import React from "react"
import { expect, it } from "vitest"
import { motion } from "../index"
import { Card, CARDS, describeNative, setup, verifying } from "./retained-harness.js"

const ENTRIES = 24
const retained = process.env.GPUIX_RETAIN_OFF === undefined

/** A scroller of cards under a root that has a background of its own, and one card whose label changes. */
const List = ({ tint, label = "card 5" }: { tint: string; label?: string }) => (
  <div testId="page" style={{ width: 640, height: 480, backgroundColor: tint }}>
    <div testId="scroller" style={{ overflowY: "scroll", height: 480, display: "flex", flexDirection: "column" }}>
      {Array.from({ length: ENTRIES }, (_, index) => (
        <Card key={index} index={index} label={index === 5 ? label : undefined} style={{ flexShrink: 0 }} />
      ))}
    </div>
  </div>
)

/** A window that is drawn when the test says, as a display link draws a live one, with nothing mounted yet. */
const emptyWindow = () => {
  const view = setup(<div />)
  view.renderer.setInlineDraws(false)
  return view
}

/** One frame: the update, and the draw. What it built and replayed. */
const frame = (view, render?: React.ReactElement) => {
  const { renderer } = view
  const before = renderer.getRetainedCounters()
  const builtBefore = view.built()
  if (render) view.render(render)
  renderer.drawFrame()
  const after = renderer.getRetainedCounters()
  return {
    builds: after.builds - before.builds,
    hits: after.hits - before.hits,
    replays: after.replays - before.replays,
    lazy: after.lazyBuilds - before.lazyBuilds,
    built: view.built() - builtBefore,
  }
}

describeNative("a root recorded by the frame that builds it", () => {
  it("is replayed by the frame after the mount: a change of the root's background builds no card", () => {
    const view = emptyWindow()
    try {
      const mount = frame(view, <List tint="#ffffff" />)
      const second = frame(view, <List tint="#eeeeee" />)
      if (retained && !verifying) {
        expect(mount.builds, "the cards are built by the frame that mounts them").toBeGreaterThanOrEqual(ENTRIES)
        expect(second.builds, "no card is built to be recorded").toBe(0)
        expect(second.hits, "every card has its recording").toBe(ENTRIES)
        // The card at the edge of what the scroller records beyond its clip is recorded where it stands by the
        // frame that mounts it, before the scroller has a box, and is built where it is put by the next.
        expect(second.lazy).toBeLessThanOrEqual(1)
        // The spine and that card, and nothing of the page.
        expect(second.built).toBeLessThan(15)
      }
      view.expectWhatABuildShows("second-draw")
      view.finish(retained)
    } finally {
      view.unmount()
    }
  })

  it("builds a changed card once, in the frame that changes it, and replays it in the next", () => {
    const view = emptyWindow()
    try {
      frame(view, <List tint="#ffffff" />)
      frame(view, <List tint="#ffffff" />)
      const changing = frame(view, <List tint="#ffffff" label="changed" />)
      const after = frame(view, <List tint="#fafafa" label="changed" />)
      const again = frame(view, <List tint="#ffffff" label="changed" />)
      if (retained && !verifying) {
        expect(changing.built, "the card that changed is built, and nothing else of the page").toBeGreaterThan(0)
        expect(changing.built).toBeLessThan(20)
        expect(changing.hits, "the others are replayed").toBe(ENTRIES - 1)
        expect(after.builds, "the frame after replays the card that changed").toBe(0)
        expect(after.hits).toBe(ENTRIES)
        expect(after.lazy).toBe(0)
        expect(after.built).toBeLessThan(5)
        expect(again.builds).toBe(0)
        expect(again.built).toBeLessThan(5)
      }
      expect(view.renderer.getAllText()).toContain("changed")
      view.expectWhatABuildShows("changed")
      view.finish(retained)
    } finally {
      view.unmount()
    }
  })

  it("shows what a build of the page without retention shows, from the first draws on", () => {
    const steps = (planning: boolean) => {
      const view = emptyWindow()
      try {
        view.renderer.setRetainPlanning(planning)
        const shown = []
        for (const render of [
          <List tint="#ffffff" />,
          <List tint="#eeeeee" />,
          <List tint="#eeeeee" label="changed" />,
          <List tint="#eeeeee" label="changed" />,
          <List tint="#ffffff" label="changed again" />,
          <List tint="#ffffff" label="changed again" />,
        ]) {
          frame(view, render)
          shown.push(view.snapshot(`step-${shown.length}-${planning}`))
        }
        view.finish(false)
        return shown
      } finally {
        view.unmount()
      }
    }
    const replayed = steps(true)
    const built = steps(false)
    replayed.forEach((shown, step) => {
      expect(shown.boxes, `step ${step}`).toEqual(built[step].boxes)
      expect(shown.text, `step ${step}`).toEqual(built[step].text)
      expect(shown.a11y, `step ${step}`).toEqual(built[step].a11y)
      expect(shown.picture === built[step].picture, `step ${step}`).toBe(true)
    })
    expect(replayed[0].picture === replayed[1].picture).toBe(false)
  })

  it("is not recorded where a motion runs, and the cards beside it are", () => {
    const view = emptyWindow()
    try {
      view.renderer.clockPause()
      const page = (
        <div testId="page" style={{ width: 640, height: 480 }}>
          {Array.from({ length: CARDS }, (_, index) => (
            <Card key={index} index={index} />
          ))}
          <div testId="holder" style={{ height: 20 }}>
            <motion.div
              testId="mover"
              style={{ position: "relative", width: 20, height: 20, backgroundColor: "#ff0000", left: 0 }}
              initial={{ left: 0 }}
              animate={{ left: 100 }}
              transition={{ duration: 10, ease: "linear" }}
            />
          </div>
        </div>
      )
      frame(view, page)
      view.renderer.clockFastForward(100)
      const second = frame(view)
      view.renderer.clockFastForward(100)
      const third = frame(view)
      if (retained && !verifying) {
        // The holder and what is above it are built, as they were; the cards beside it are recorded and replayed.
        expect(second.builds).toBe(0)
        expect(third.builds).toBe(0)
        expect(third.hits).toBeGreaterThanOrEqual(CARDS)
        expect(third.built).toBeLessThan(10)
      }
      view.expectWhatABuildShows("motion")
      view.finish(retained)
    } finally {
      view.unmount()
    }
  })
})
