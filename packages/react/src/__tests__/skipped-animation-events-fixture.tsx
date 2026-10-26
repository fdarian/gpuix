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
  r2?: number
  list?: string
  tl?: number
  subtree?: OracleRow[]
  host_subtree?: OracleTreeRow[]
  host_plain?: OracleTreeRow[]
  doc?: OracleTreeRow[]
  scroller_subtree?: OracleTreeRow[]
}
type OracleSide = { bg?: string; op?: string; anims: { n: string; state: string; ct: number; st: number | null }[] }
type OracleRow = { el: string; n: string; id: number; state: string; pending: boolean; ct: number; st: number | null }
type OracleTreeRow = { target: string; n: string; state: string; ct: number }

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
  /** A read while skipped made the animations of the target, which run from then: they do not start at the reveal. */
  made?: boolean
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
  q1_load_read: { ...LOOP, load: true, made: true, steps: [[600, "readAnim"], [1000, "readStyle"], [1500, "reveal"], [2200, null]] },
  q2_run: { ...FINITE, steps: [[100, "skip"], [1500, "reveal"], [2000, null]] },
  q2_load: { ...FINITE, load: true, steps: [[1500, "reveal"], [2000, null]] },
  q2_class: { steps: [[100, "skip"], [300, "addClass"], [1500, "reveal"], [2000, null]] },
  q2_class_read: { made: true, steps: [[100, "skip"], [300, "addClass+readAnim"], [700, "readAnim"], [1500, "reveal"], [2200, null]] },
  q3_before: { ...TRANSITION, steps: [[100, "change"], [150, "skip"], [1200, "reveal"], [1700, null]] },
  q3_inside: { ...TRANSITION, steps: [[100, "skip"], [300, "change"], [1200, "reveal"], [1700, null]] },
  q3_inside_mid: { ...TRANSITION, steps: [[100, "skip"], [300, "change"], [450, "reveal"], [1000, null]] },
  q6_load_tree: { ...LOOP, load: true, steps: [[600, "readTree"], [1500, "reveal"], [2000, null]] },
  // Reads of content that was never rendered, which Chrome styles for the read: the animations it makes run while skipped.
  // `getComputedStyle` (q6_load_style) has no counterpart in gpuix; a read of the geometry stands in for it below.
  q6_load_geom: { ...LOOP, load: true, made: true, steps: [[600, "readGeom"], [1000, "readTree"], [1500, "reveal"], [2200, null]] },
  q6_load_geom_fin: { t: "fin", ctl: "fin", load: true, made: true, steps: [[600, "readGeom"], [1000, "readTree"], [1500, "reveal"], [2200, null]] },
  q6_load_rect: { ...LOOP, load: true, made: true, steps: [[600, "readRectOnly"], [1000, "readTree"], [1500, "reveal"], [2200, null]] },
  q6_load_rect_fin: { t: "fin", ctl: "fin", load: true, made: true, steps: [[600, "readRectOnly"], [1000, "readTree"], [1500, "reveal"], [2200, null]] },
}

/** What a read lists and creates: these are held to Chrome's reads one by one (`readAnim` and `readStyle` are the page's). */
const READ_SCENES = ["q1_load_read", "q2_class_read", "q6_load_geom", "q6_load_geom_fin"]

/** The scenes that list the animations of the tree, `readTree` in the page. */
const TREE_SCENES = ["q6_load_tree", "q6_load_geom", "q6_load_geom_fin", "q6_load_rect", "q6_load_rect_fin"]

type Seen = { type: string; target: string; name: string; elapsed: number; at: number }
type Look = {
  animations: {
    id: number
    name: string
    currentTime: number
    playState: string
    pending: boolean
    startTime: number | null
    /** The `computedOffset` of each keyframe `getKeyframes()` gives. */
    offsets: number[]
  }[]
  color?: number[]
}
/** What the reads of the tree list: the animation of an element is `target:name:state`. */
type TreeLook = { at: number; host_subtree: string[]; host_plain: string[]; doc: string[]; scroller_subtree: string[] }
type Played = {
  seen: Seen[]
  skipAt: number
  revealAt: number
  /** The target and the control in the frames after the reveal. */
  frames: { frame: number; t: Look; ctl: Look }[]
  /** `getAnimations()` of the target and the control in the reads the scene makes while skipped. */
  reads: { at: number; t: Look; ctl: Look }[]
  readEvents: Seen[]
  trees: TreeLook[]
  /** The renderer's timeline when the scene began, which `startTime` counts from. */
  timeline0: number
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
      id: info.id,
      name: info.animationName ?? info.transitionProperty,
      currentTime: info.currentTime,
      playState: info.playState,
      pending: info.pending,
      startTime: info.startTime ?? null,
      offsets: info.kind === "animation" ? renderer.getAnimationKeyframes(info.id).map((frame) => frame.computedOffset) : [],
    })),
  }
  if (withColor) {
    const bounds = renderer.getElementBounds(id)
    look.color = shotOf(renderer, name).at(bounds.x + bounds.width / 2, bounds.y + bounds.height / 2).slice(0, 3)
  }
  return look
}

function play(sceneName: string, mode: Mode): Played {
  const scene = SCENES[sceneName]
  const root = createTestRoot({ width: 700, height: 900 })
  const renderer = root.renderer
  const state: State = { skipped: false, changed: false, classed: false }
  const played: Played = { seen: [], skipAt: -1, revealAt: -1, frames: [], reads: [], readEvents: [], trees: [], timeline0: 0 }
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
    // The page reads the computed style, which gpuix has no call for: its `getAnimations()` is the read that styles.
    readStyle: () => actions.readAnim(),
    readRectOnly: () => {
      renderer.getElementBounds(renderer.findByTestId("t").id)
    },
    readGeom: () => {
      actions.readRectOnly()
      actions.readAnim()
    },
    readTree: () => {
      const names = ["t", "t2", "ctl", "ctl2", "host", "scroller"]
      const testIds = new Map(names.map((name) => [renderer.findByTestId(name).id, name]))
      const rows = (list: { elementId: number; info: { animationName?: string; transitionProperty?: string; playState: string } }[]) =>
        list.map((entry) => `${testIds.get(entry.elementId)}:${entry.info.animationName ?? entry.info.transitionProperty}:${entry.info.playState}`)
      const host = renderer.findByTestId("host").id
      played.trees.push({
        at: now,
        host_subtree: rows(renderer.getSubtreeAnimations(host)),
        host_plain: renderer.getAnimations(host).map((info) => `host:${info.animationName ?? info.transitionProperty}:${info.playState}`),
        doc: rows(renderer.getDocumentAnimations()),
        scroller_subtree: rows(renderer.getSubtreeAnimations(renderer.findByTestId("scroller").id)),
      })
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
    played.timeline0 = renderer.getAnimationTimelineTime()
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
    /** Every read of the target's animations, whichever way the page made it. */
    looks: log.filter((entry) => entry.phase === "readAnim" || entry.phase === "readStyle" || entry.phase === "readGeom"),
    trees: log.filter((entry) => entry.phase === "readTree"),
    reveal,
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
        // What is drawn is the background over white at the opacity the animations give it.
        const opacity = Number(chrome.raf1.tgt.op)
        const expected = scene.likeControl ? run.frames[drawn.frame].ctl.color : rgbOf(chrome.raf1.tgt.bg).map((channel) => 255 - (255 - channel) * opacity)
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
            if (was.state !== "running") continue
            if (scene.made) {
              // Made by a read while skipped: it ran on, and is where it would be had it been drawn all along.
              const atReveal = was.ct - chrome.raf1.r
              expect(Math.abs(found.currentTime - drawn.frame * FRAME - atReveal), `${was.n} runs on from the read`).toBeLessThanOrEqual(2 * FRAME)
            } else {
              expect(found.currentTime, `${was.n} starts at the reveal`).toBeLessThanOrEqual(FRAME * (drawn.frame + 1))
            }
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

    // A read of an element in content that was never rendered styles the content of the skipped hosts around it, as Chrome
    // does: the CSS animations it makes are pending until the next frame, then run (and `currentTime` advances) while the
    // content is skipped, and are the same objects for every later read and after the reveal. The DOM hears of them after it.
    for (const name of READ_SCENES) {
      it(`lists the animations a read made, pending and then running while skipped, the same ones at the reveal: ${name}`, () => {
        const chrome = chromeOf(name, mode)
        const run = playedScene(name)
        expect(run.reads.length, "reads of the target").toBe(chrome.looks.length)
        const first = run.reads[0]
        const ids = new Map(first.t.animations.map((animation) => [animation.name, animation.id]))
        run.reads.forEach((read, at) => {
          const was = chrome.looks[at].tgt
          expect(read.t.animations.map((animation) => [animation.name, animation.playState]), `read ${at}`).toEqual(
            was.anims.map((animation) => [KEYFRAMES[animation.n] ?? animation.n, animation.state]),
          )
          read.t.animations.forEach((animation, index) => {
            const expected = was.anims[index]
            expect(animation.id, `read ${at}: the same ${animation.name}`).toBe(ids.get(animation.name))
            if (at === 0) {
              expect(animation.pending, "it waits for a frame").toBe(true)
              expect(animation.currentTime, "and has not started").toBe(0)
              expect(animation.startTime, "with no start time").toBeNull()
              return
            }
            expect(animation.pending, `read ${at}`).toBe(false)
            const started = animation.startTime - run.timeline0 - first.at
            expect(started, `started at the first frame after the read (${animation.name})`).toBeGreaterThan(0)
            expect(started, `started at the first frame after the read (${animation.name})`).toBeLessThanOrEqual(2 * FRAME)
            // A run that finished while nobody rendered it is at its end here; Chrome's time keeps counting until it renders.
            if (expected.state === "finished") expect(animation.currentTime).toBeGreaterThanOrEqual(300)
            else expect(Math.abs(animation.currentTime - expected.ct), `time of ${animation.name} at read ${at}`).toBeLessThanOrEqual(2 * FRAME)
          })
        })
        expect(run.readEvents, "a read tells the DOM nothing").toEqual([])
        for (const animation of first.t.animations) expect(animation.offsets, `getKeyframes() of ${animation.name}, made by the read`).toEqual([0, 1])
        const revealed = run.frames.find((frame) => !isWhite(frame.t.color))
        const last = run.reads.at(-1)
        for (const animation of revealed.t.animations) {
          const read = last.t.animations.find((listed) => listed.name === animation.name)
          expect(animation.id, `${animation.name} is the object the read listed`).toBe(read.id)
          const started = animation.startTime - run.timeline0 - first.at
          expect(started, `${animation.name} did not start over`).toBeGreaterThan(0)
          expect(started, `${animation.name} did not start over`).toBeLessThanOrEqual(2 * FRAME)
          if (read.startTime !== null) expect(animation.startTime, `${animation.name} did not start over`).toBeCloseTo(read.startTime, 3)
        }
        const toldBeforeTheReveal = run.seen.filter((seen) => seen.target === "t" && seen.at < run.revealAt)
        expect(toldBeforeTheReveal, "nothing of the target is told while it is skipped").toEqual([])
      })
    }

    for (const name of TREE_SCENES) {
      it(`lists what the reads made, and nothing else, in the lists of the tree: ${name}`, () => {
        const chrome = chromeOf(name, mode)
        const run = playedScene(name)
        expect(run.trees.length).toBe(chrome.trees.length)
        const named = (rows: OracleTreeRow[]) => rows.map((row) => `${row.target}:${KEYFRAMES[row.n] ?? row.n}:${row.state}`)
        run.trees.forEach((tree, at) => {
          const was = chrome.trees[at]
          expect(tree.host_subtree, "host.getAnimations({ subtree: true })").toEqual(named(was.host_subtree))
          expect(tree.host_plain, "host.getAnimations()").toEqual(named(was.host_plain))
          expect(tree.doc, "document.getAnimations()").toEqual(named(was.doc))
          expect(tree.scroller_subtree, "scroller.getAnimations({ subtree: true })").toEqual(named(was.scroller_subtree))
        })
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
