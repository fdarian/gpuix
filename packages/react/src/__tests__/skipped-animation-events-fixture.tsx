// @ts-nocheck
/**
 * The scenes of `fixtures/content-visibility-animation-oracle.html` played in the test renderer, and the tests that hold
 * what gpuix tells and shows of CSS animations and transitions in skipped content to what Chrome did. `hidden` skips with
 * `contentVisibility: hidden` and reveals with `visible`; `auto` scrolls the host 4000px away and back.
 *
 * A scene is the list of `[ms, action]` steps of the page, run in simulated time in ticks of 16 ms. After the reveal the
 * scene draws a frame at once (`frame 0`) and three ticks (`frame 1` to `3`), and looks at the target (`#t`, in the
 * host) and the control (`#ctl`, outside it) in each.
 */

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { defineKeyframes, motion, motionValue } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { shotOf } from "./skipped-animations-fixture.js"

export type Mode = "hidden" | "auto"
type Renderer = ReturnType<typeof createTestRoot>["renderer"]

type OracleEntry = {
  t: number
  r: number | null
  did?: string
  ev?: string
  target?: string
  animation?: string
  property?: string
  elapsed?: number
  phase?: string
  tgt?: OracleSide
  ctl?: OracleSide
}
type OracleSide = { bg?: string; anims: { n: string; state: string; ct: number }[] }

const FIXTURES = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures")
const ORACLE = JSON.parse(fs.readFileSync(path.join(FIXTURES, "content-visibility-animation-oracle.json"), "utf8")) as {
  scenes: Record<string, { log: OracleEntry[] }>
}

const FAR = 4000
const FRAME = 16
/** The frames after the reveal in which an event that was due while skipped must arrive: Chrome takes two or three, gpuix may take one. */
const FRAMES_TO_TELL = 3

defineKeyframes("cvo-bg", { from: { backgroundColor: "#ff0000" }, to: { backgroundColor: "#0000ff" } })
defineKeyframes("cvo-op", { from: { opacity: 1 }, to: { opacity: 0.2 } })

/** The page's keyframes under the names gpuix has them by. */
const KEYFRAMES = { bg: "cvo-bg", op: "cvo-op" }

/** The style of each class of the oracle page. */
const CLASSES = {
  loop: { animation: "cvo-bg 500ms linear infinite, cvo-op 500ms linear infinite" },
  fin: { animation: "cvo-bg 300ms linear 1 both" },
  fin2: { animation: "cvo-op 300ms linear 500ms 1 both" },
  tr: { transition: "background-color 300ms linear" },
}

type Scene = {
  /** The class of `#t`, `#ctl`, `#t2` and `#ctl2`. */
  t?: string
  ctl?: string
  t2?: string
  ctl2?: string
  /** The content is skipped before its first frame. */
  load?: boolean
  /** Whether the target is compared with the control in the frames after the reveal, or with what Chrome showed. */
  likeControl?: boolean
  steps: [number, string | null][]
}

const LOOP = { t: "loop", ctl: "loop" }
const FINITE = { t: "fin", ctl: "fin", t2: "fin2", ctl2: "fin2" }
const TRANSITION = { t: "tr", ctl: "tr" }

/** The scenes of the oracle page that are played; the names and the steps are the page's. */
const SCENES: Record<string, Scene> = {
  q1_run: { ...LOOP, likeControl: true, steps: [[300, "skip"], [1650, "reveal"], [2200, null]] },
  q1_run_read: { ...LOOP, likeControl: true, steps: [[300, "skip"], [600, "readAnim"], [900, "readAnim"], [1650, "reveal"], [2200, null]] },
  q1_load: { ...LOOP, load: true, steps: [[1500, "reveal"], [2000, null]] },
  q2_run: { ...FINITE, steps: [[100, "skip"], [1500, "reveal"], [2000, null]] },
  q2_load: { ...FINITE, load: true, steps: [[1500, "reveal"], [2000, null]] },
  q2_class: { steps: [[100, "skip"], [300, "addClass"], [1500, "reveal"], [2000, null]] },
  q3_before: { ...TRANSITION, steps: [[100, "change"], [150, "skip"], [1200, "reveal"], [1700, null]] },
  q3_inside: { ...TRANSITION, steps: [[100, "skip"], [300, "change"], [1200, "reveal"], [1700, null]] },
  q3_inside_mid: { ...TRANSITION, steps: [[100, "skip"], [300, "change"], [450, "reveal"], [1000, null]] },
}

/** Scenes with a read of `getAnimations()` that Chrome answers differently, see the test of `FORCED_READS`. */
const FORCED_READS: Record<string, Scene> = {
  q2_class_read: { steps: [[100, "skip"], [300, "addClass+readAnim"], [700, "readAnim"], [1500, "reveal"], [2000, null]] },
  q1_load_read: { ...LOOP, load: true, steps: [[600, "readAnim"], [1500, "reveal"], [2000, null]] },
}

type Seen = { type: string; target: string; name: string; elapsed: number; at: number }
type Look = { animations: { name: string; currentTime: number; playState: string }[]; color?: number[] }
type Played = {
  seen: Seen[]
  skipAt: number
  revealAt: number
  /** The target and the control in the frames after the reveal. */
  frames: { frame: number; t: Look; ctl: Look }[]
  /** `getAnimations()` of the target and the control in the reads the scene makes while skipped. */
  reads: { at: number; t: Look; ctl: Look }[]
  readEvents: Seen[]
}

type State = { skipped: boolean; changed: boolean; classed: boolean }

/** Where the host is below the top of the scroller, for the scenes that load with an auto host that is far: it is not scrolled away, it is put there. */
const hostOffset = (scene: Scene, mode: Mode) => (scene.load && mode === "auto" ? FAR : 0)

function Page({ scene, mode, state, record }: { scene: Scene; mode: Mode; state: State; record: (event: Omit<Seen, "at">) => void }) {
  const contentVisibility = mode === "hidden" ? (state.skipped ? "hidden" : "visible") : "auto"
  const listen = (target: string) => ({
    onAnimationStart: (event) => record({ type: "animationstart", target, name: event.animationName, elapsed: event.elapsedTime }),
    onAnimationIteration: (event) => record({ type: "animationiteration", target, name: event.animationName, elapsed: event.elapsedTime }),
    onAnimationEnd: (event) => record({ type: "animationend", target, name: event.animationName, elapsed: event.elapsedTime }),
    onAnimationCancel: (event) => record({ type: "animationcancel", target, name: event.animationName, elapsed: event.elapsedTime }),
    onTransitionRun: (event) => record({ type: "transitionrun", target, name: event.propertyName, elapsed: event.elapsedTime }),
    onTransitionStart: (event) => record({ type: "transitionstart", target, name: event.propertyName, elapsed: event.elapsedTime }),
    onTransitionEnd: (event) => record({ type: "transitionend", target, name: event.propertyName, elapsed: event.elapsedTime }),
    onTransitionCancel: (event) => record({ type: "transitioncancel", target, name: event.propertyName, elapsed: event.elapsedTime }),
  })
  const box = (target: string, className: string | undefined, placement: Record<string, number>) => {
    const added = state.classed ? (target === "t" || target === "ctl" ? "fin" : "fin2") : undefined
    return {
      position: "absolute",
      width: 100,
      height: 40,
      flexShrink: 0,
      backgroundColor: state.changed ? "#0000ff" : "#ff0000",
      ...placement,
      ...(className ? CLASSES[className] : {}),
      ...(added ? CLASSES[added] : {}),
    }
  }
  return (
    <div style={{ width: 700, height: 900, position: "relative", backgroundColor: "#ffffff" }}>
      <div testId="scroller" style={{ position: "absolute", left: 0, top: 0, width: 400, height: 500, overflowY: "auto" }}>
        <div style={{ position: "relative", height: 6000, flexShrink: 0 }}>
          <div
            testId="host"
            style={{ position: "absolute", left: 0, top: 100 + hostOffset(scene, mode), width: 300, height: 200, backgroundColor: "#ffffff", contentVisibility, containIntrinsicSize: "300px 200px" }}
          >
            <div testId="t" {...listen("t")} style={box("t", scene.t, { left: 10, top: 10 })} />
            <div testId="t2" {...listen("t2")} style={box("t2", scene.t2, { left: 160, top: 10 })} />
          </div>
        </div>
      </div>
      <div testId="ctl" {...listen("ctl")} style={box("ctl", scene.ctl, { left: 500, top: 20 })} />
      <div testId="ctl2" {...listen("ctl2")} style={box("ctl2", scene.ctl2, { left: 500, top: 80 })} />
    </div>
  )
}

function lookAt(renderer: Renderer, target: string, withColor: boolean, name: string): Look {
  const id = renderer.findByTestId(target).id
  const look: Look = {
    animations: renderer.getAnimations(id).map((info) => ({
      name: info.animationName ?? info.transitionProperty,
      currentTime: info.currentTime,
      playState: info.playState,
    })),
  }
  if (withColor) {
    const bounds = renderer.getElementBounds(id)
    look.color = shotOf(renderer, name).at(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2).slice(0, 3)
  }
  return look
}

function play(sceneName: string, mode: Mode): Played {
  const scene = SCENES[sceneName] ?? FORCED_READS[sceneName]
  const root = createTestRoot({ width: 700, height: 900 })
  const renderer = root.renderer
  const state: State = { skipped: false, changed: false, classed: false }
  const played: Played = { seen: [], skipAt: -1, revealAt: -1, frames: [], reads: [], readEvents: [] }
  let now = 0
  const record = (event: Omit<Seen, "at">) => played.seen.push({ ...event, at: now })
  const render = () => root.render(<Page scene={scene} mode={mode} state={state} record={record} />)
  const scrollTo = (top: number) => renderer.scrollTo(renderer.findByTestId("scroller").id, 0, top, "instant")
  const elapse = (ms: number) => {
    renderer.clockAdvance(ms)
    renderer.advanceTime(ms)
    now += ms
    renderer.drawFrame()
    renderer.dispatchNativeEvents()
  }
  const advanceTo = (ms: number) => {
    while (now < ms) elapse(Math.min(FRAME, ms - now))
  }
  const looks = (frame: number) => ({ frame, t: lookAt(renderer, "t", true, `t-${frame}`), ctl: lookAt(renderer, "ctl", true, `ctl-${frame}`) })
  const actions: Record<string, () => void> = {
    skip: () => {
      played.skipAt = now
      if (mode === "hidden") {
        state.skipped = true
        render()
      } else scrollTo(FAR)
    },
    reveal: () => {
      played.revealAt = now
      if (mode === "hidden") {
        state.skipped = false
        render()
      } else scrollTo(hostOffset(scene, mode))
      renderer.flush()
      renderer.drawFrame()
      renderer.dispatchNativeEvents()
      played.frames.push(looks(0))
      for (let frame = 1; frame <= FRAMES_TO_TELL; frame++) {
        elapse(FRAME)
        played.frames.push(looks(frame))
      }
    },
    readAnim: () => {
      const before = played.seen.length
      played.reads.push({ at: now, t: lookAt(renderer, "t", false, ""), ctl: lookAt(renderer, "ctl", false, "") })
      renderer.dispatchNativeEvents()
      played.readEvents.push(...played.seen.slice(before))
    },
    change: () => {
      state.changed = true
      render()
    },
    addClass: () => {
      state.classed = true
      render()
    },
  }
  try {
    renderer.clockPause()
    renderer.setInlineDraws(false)
    renderer.setA11yActive(false)
    if (scene.load) {
      played.skipAt = now
      state.skipped = mode === "hidden"
    }
    render()
    renderer.flush()
    elapse(0)
    for (const step of scene.steps) {
      advanceTo(step[0])
      if (step[1]) for (const action of step[1].split("+")) actions[action]()
    }
    return played
  } finally {
    root.unmount()
  }
}

/** What Chrome logged for a scene. */
function chromeOf(scene: string, mode: Mode) {
  const found = ORACLE.scenes[`${scene}:${mode}`]
  if (!found) throw new Error(`the oracle has no scene ${scene}:${mode}`)
  const log = found.log
  const reveal = log.find((entry) => entry.did === "reveal")
  const raf1 = log.find((entry) => entry.phase === "raf1")
  return {
    revealAt: reveal.t,
    events: log.filter((entry) => entry.ev).map((entry) => ({
      type: entry.ev,
      target: entry.target,
      name: KEYFRAMES[entry.animation] ?? entry.property,
      elapsed: entry.elapsed,
      at: entry.t,
      revealed: entry.r,
    })),
    raf1,
    reads: log.filter((entry) => entry.phase === "readAnim"),
  }
}

const keyOf = (event: { type: string; target: string; name: string; elapsed: number }) => `${event.type}:${event.target}:${event.name}:${event.elapsed}`
const rgbOf = (text: string) => text.match(/\d+/g).slice(0, 3).map(Number)
const isWhite = (color: number[]) => color.every((channel) => channel > 250)
const apart = (first: number[], second: number[]) => Math.max(...first.map((channel, at) => Math.abs(channel - second[at])))

/** The channels a pixel read may differ by: the frames of gpuix are 16 ms from the instant Chrome's first frame has. */
const COLOUR_TOLERANCE = 28

const describeNative = hasNativeTestRenderer ? describe : describe.skip

/** The scenes whose events and values are held to Chrome's, for `mode`. */
export function defineAnimationEventTests(mode: Mode) {
  describeNative(`CSS animations and transitions in content skipped by ${mode} against Chrome`, { timeout: 120_000 }, () => {
    const played = new Map<string, Played>()
    const playedScene = (name: string) => {
      if (!played.has(name)) played.set(name, play(name, mode))
      return played.get(name)
    }

    it("has the scenes of the oracle", () => {
      for (const name of Object.keys(SCENES)) expect(ORACLE.scenes[`${name}:${mode}`], name).toBeDefined()
    })

    for (const name of Object.keys(SCENES)) {
      it(`tells the events Chrome tells, when it tells them: ${name}`, () => {
        const chrome = chromeOf(name, mode)
        const run = playedScene(name)
        expect(run.revealAt, "the scene is played to its reveal").toBeGreaterThan(0)
        const told = [...run.seen]
        for (const event of chrome.events) {
          const at = told.findIndex((seen) => keyOf(seen) === keyOf(event))
          expect(at, `${keyOf(event)} at ${event.at} (${event.revealed} after the reveal) was not told; told: ${run.seen.map((seen) => `${keyOf(seen)}@${seen.at}`).join(", ")}`).toBeGreaterThanOrEqual(0)
          const seen = told.splice(at, 1)[0]
          const where = `${keyOf(event)}: Chrome at ${event.at} (${event.revealed} after the reveal), gpuix at ${seen.at} (reveal at ${run.revealAt})`
          if (event.revealed === null) {
            // On time, before the reveal.
            expect(Math.abs(seen.at - event.at), where).toBeLessThanOrEqual(2 * FRAME)
          } else if (event.revealed < FRAMES_TO_TELL * FRAME + FRAME) {
            // Due while the content was skipped, or at the reveal: none before it, all within the first frames.
            expect(seen.at - run.revealAt, where).toBeGreaterThanOrEqual(0)
            expect(seen.at - run.revealAt, where).toBeLessThanOrEqual(FRAMES_TO_TELL * FRAME)
          } else {
            expect(Math.abs(seen.at - run.revealAt - event.revealed), where).toBeLessThanOrEqual((FRAMES_TO_TELL + 1) * FRAME)
          }
        }
        expect(told.map(keyOf), "nothing that Chrome did not tell").toEqual([])
      })

      it(`shows in the first frame after the reveal what Chrome shows: ${name}`, () => {
        const chrome = chromeOf(name, mode)
        const run = playedScene(name)
        // An auto element is built in the frame after the one that finds it near; the target is drawn by the second.
        const drawn = run.frames.find((frame) => !isWhite(frame.t.color))
        expect(drawn, `the target is not drawn in the first frames: ${JSON.stringify(run.frames.map((frame) => frame.t.color))}`).toBeDefined()
        expect(drawn.frame, "frame of the first sight of the target").toBeLessThanOrEqual(mode === "auto" ? 1 : 0)
        const scene = SCENES[name]
        const expected = scene.likeControl ? run.frames[drawn.frame].ctl.color : rgbOf(chrome.raf1.tgt.bg)
        expect(apart(drawn.t.color, expected), `colour ${drawn.t.color} against ${scene.likeControl ? "the control" : "Chrome"} ${expected}`).toBeLessThanOrEqual(scene.likeControl ? 3 : COLOUR_TOLERANCE)

        const shown = drawn.t.animations
        if (scene.likeControl) {
          const control = run.frames[drawn.frame].ctl.animations
          expect(shown.map((animation) => [animation.name, animation.playState])).toEqual(control.map((animation) => [animation.name, animation.playState]))
          for (let at = 0; at < shown.length; at++) expect(Math.abs(shown[at].currentTime - control[at].currentTime), "the time of the timeline").toBeLessThanOrEqual(1)
        } else {
          // A transition Chrome has finished is gone a frame later, so only its animations and what runs are compared.
          for (const was of chrome.raf1.tgt.anims.filter((animation) => KEYFRAMES[animation.n] || animation.state === "running")) {
            const found = shown.find((animation) => (KEYFRAMES[was.n] ?? was.n) === animation.name)
            expect(found, `${was.n} is on the target`).toBeDefined()
            expect(found.playState, was.n).toBe(was.state)
            if (was.state === "running") expect(found.currentTime, `${was.n} starts at the reveal`).toBeLessThanOrEqual(FRAME * (drawn.frame + 1))
          }
        }
      })
    }

    it("reads the live time of an animation of skipped content, and tells nothing for the read", () => {
      const chrome = chromeOf("q1_run_read", mode)
      const run = playedScene("q1_run_read")
      expect(run.reads.length).toBe(chrome.reads.length)
      run.reads.forEach((read, at) => {
        const was = chrome.reads[at]
        expect(read.t.animations.map((animation) => [animation.name, animation.playState])).toEqual(was.tgt.anims.map((animation) => [KEYFRAMES[animation.n], animation.state]))
        for (let index = 0; index < read.t.animations.length; index++) {
          // Its time is the timeline's, not the instant of the last frame (16 ms is a frame, 4 the start of Chrome's first frame).
          expect(Math.abs(read.t.animations[index].currentTime - was.tgt.anims[index].ct), `read ${at}`).toBeLessThanOrEqual(FRAME)
          expect(read.t.animations[index].currentTime, "the control's time").toBeCloseTo(read.ctl.animations[index].currentTime, 0)
        }
      })
      expect(run.readEvents, "a read dispatches nothing").toEqual([])
    })

    // Chrome's `getAnimations()` styles the element it is read on, so it creates the animation of content that was never
    // styled (or whose class was added while skipped) at once: it runs while skipped and `animationstart` and `animationend`
    // come at the reveal. gpuix builds nothing of skipped content, so there is no animation to list, and it starts at the reveal.
    for (const name of Object.keys(FORCED_READS)) {
      it(`lists no animation for content that was never styled, and starts it at the reveal: ${name}`, () => {
        const run = playedScene(name)
        for (const read of run.reads) expect(read.t.animations, `getAnimations() of the target at ${read.at}`).toEqual([])
        const started = run.seen.find((seen) => seen.type === "animationstart" && seen.target === "t")
        expect(started.at - run.revealAt, "starts in the first frames after the reveal").toBeGreaterThanOrEqual(0)
        expect(started.at - run.revealAt).toBeLessThanOrEqual(FRAMES_TO_TELL * FRAME)
        const drawn = run.frames.find((frame) => !isWhite(frame.t.color))
        expect(drawn.t.animations[0].currentTime, "from its start").toBeLessThanOrEqual(FRAME * (drawn.frame + 1))
      })
    }

    it("shows the value a script wrote last into skipped content when it is revealed", () => {
      const tint = motionValue("#ff0000")
      const root = createTestRoot({ width: 700, height: 900 })
      const renderer = root.renderer
      const Host = ({ skipped }: { skipped: boolean }) => (
        <div style={{ width: 700, height: 900, position: "relative", backgroundColor: "#ffffff" }}>
          <div testId="scroller" style={{ position: "absolute", left: 0, top: 0, width: 400, height: 500, overflowY: "auto" }}>
            <div style={{ position: "relative", height: 6000, flexShrink: 0 }}>
              <div
                style={{ position: "absolute", left: 0, top: 100, width: 300, height: 200, contentVisibility: mode === "hidden" ? (skipped ? "hidden" : "visible") : "auto", containIntrinsicSize: "300px 200px" }}
              >
                <motion.div testId="written" style={{ position: "absolute", left: 10, top: 10, width: 100, height: 40, backgroundColor: tint }} />
              </div>
            </div>
          </div>
        </div>
      )
      const tick = () => {
        renderer.clockAdvance(FRAME)
        renderer.advanceTime(FRAME)
        renderer.drawFrame()
        renderer.dispatchNativeEvents()
      }
      const colorOfWritten = () => {
        const bounds = renderer.getElementBounds(renderer.findByTestId("written").id)
        return shotOf(renderer, "written").at(bounds.x + 50, bounds.y + 20).slice(0, 3)
      }
      try {
        renderer.clockPause()
        renderer.setInlineDraws(false)
        renderer.setA11yActive(false)
        root.render(<Host skipped={false} />)
        renderer.flush()
        for (let step = 0; step < 4; step++) tick()
        const scroller = renderer.findByTestId("scroller").id
        if (mode === "hidden") root.render(<Host skipped />)
        else renderer.scrollTo(scroller, 0, FAR, "instant")
        for (let step = 0; step < 4; step++) tick()
        // A script that writes at every frame, as Motion's `animate` does, while nothing is drawn.
        for (let step = 0; step < 30; step++) {
          tint.set(`rgb(${step * 8}, 0, ${255 - step * 8})`)
          tick()
        }
        if (mode === "hidden") root.render(<Host skipped={false} />)
        else renderer.scrollTo(scroller, 0, 0, "instant")
        renderer.flush()
        renderer.drawFrame()
        renderer.dispatchNativeEvents()
        let color = colorOfWritten()
        if (mode === "auto" && color.every((channel) => channel > 250)) {
          tick()
          color = colorOfWritten()
        }
        expect(apart(color, [29 * 8, 0, 255 - 29 * 8]), `colour ${color}`).toBeLessThanOrEqual(2)
      } finally {
        root.unmount()
      }
    })

    it("reads the geometry of content with a running opacity animation that is skipped, and of content that is revealed and not yet drawn", () => {
      const root = createTestRoot({ width: 700, height: 900 })
      const renderer = root.renderer
      const Looping = ({ skipped }: { skipped: boolean }) => (
        <div style={{ width: 700, height: 900, position: "relative", backgroundColor: "#ffffff" }}>
          <div testId="scroller" style={{ position: "absolute", left: 0, top: 0, width: 400, height: 500, overflowY: "auto" }}>
            <div style={{ position: "relative", height: 6000, flexShrink: 0 }}>
              <div
                style={{ position: "absolute", left: 0, top: 100, width: 300, height: 200, contentVisibility: mode === "hidden" ? (skipped ? "hidden" : "visible") : "auto", containIntrinsicSize: "300px 200px" }}
              >
                <div testId="fading" style={{ position: "absolute", left: 10, top: 10, width: 100, height: 40, backgroundColor: "#ff0000", ...CLASSES.loop }} />
              </div>
            </div>
          </div>
        </div>
      )
      const tick = () => {
        renderer.clockAdvance(FRAME)
        renderer.advanceTime(FRAME)
        renderer.drawFrame()
        renderer.dispatchNativeEvents()
      }
      const read = () => renderer.getElementBounds(renderer.findByTestId("fading").id)
      try {
        renderer.clockPause()
        renderer.setInlineDraws(false)
        root.render(<Looping skipped={false} />)
        renderer.flush()
        for (let step = 0; step < 6; step++) tick()
        const scroller = renderer.findByTestId("scroller").id
        if (mode === "hidden") root.render(<Looping skipped />)
        else renderer.scrollTo(scroller, 0, FAR, "instant")
        for (let step = 0; step < 4; step++) tick()
        expect(read().width).toBe(100)
        if (mode === "hidden") root.render(<Looping skipped={false} />)
        else renderer.scrollTo(scroller, 0, 0, "instant")
        renderer.flush()
        expect(read().width).toBe(100)
        for (let step = 0; step < 4; step++) tick()
        expect(read().width).toBe(100)
      } finally {
        root.unmount()
      }
    })
  })
}
