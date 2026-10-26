// @ts-nocheck
/**
 * The `q7` scenes of `fixtures/content-visibility-animation-oracle.html` played in the test renderer: which elements of a
 * never-rendered skipped host a read of one of them styles. The host holds `#a > (#t > #c, #s)`, each with the same looping
 * colour and opacity animation, and `#t` is read. In Chrome any read of `#t` (its `getAnimations()`, a geometry read, a
 * computed-style read of any property) creates the animations of the four, all pending and started at the first frame
 * after the read, which run while the host is skipped and are the same objects at the reveal; `document.getAnimations()`
 * and a read of nothing create none; the DOM hears of the starts after the reveal.
 */

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { defineKeyframes } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

type Mode = "hidden" | "auto"

type Row = { el: string; n: string; id: number; state: string; pending: boolean; ct: number; st: number | null }
type Entry = { t: number; r?: number | null; did?: string; value?: unknown; ev?: string; target?: string; animation?: string; list?: string; tl?: number; subtree?: Row[] }

const FIXTURES = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures")
const ORACLE = JSON.parse(fs.readFileSync(path.join(FIXTURES, "content-visibility-animation-oracle.json"), "utf8")) as {
  scenes: Record<string, { log: Entry[] }>
}

const FAR = 4000
const FRAME = 16
const FRAMES_TO_TELL = 3

defineKeyframes("cvo-bg", { from: { backgroundColor: "#ff0000" }, to: { backgroundColor: "#0000ff" } })
defineKeyframes("cvo-op", { from: { opacity: 1 }, to: { opacity: 0.2 } })
const NAME = { bg: "cvo-bg", op: "cvo-op" }
const LOOP = { animation: "cvo-bg 500ms linear infinite, cvo-op 500ms linear infinite" }

/** The reads that have a counterpart in gpuix, by the page's name for them. `bg` and `color` read the computed style, which gpuix has no call for. */
const READS = ["none", "getanims", "rect", "offsetw", "doc"]
const STEPS: [number, string][] = [[600, "read+list"], [1000, "list"], [1300, "read+list"], [1600, "reveal"]]

type Listed = { at: number; rows: { el: string; name: string; id: number; state: string; pending: boolean; currentTime: number; startTime: number | null }[] }
type Played = {
  lists: Listed[]
  afterReveal: Listed[]
  starts: { target: string; name: string; at: number }[]
  told: { type: string; target: string; name: string; at: number }[]
  revealAt: number
  timeline0: number
  readAt: number[]
}

function Page({ mode, skipped, record }: { mode: Mode; skipped: boolean; record: (type: string, target: string, name: string) => void }) {
  // The events bubble: an element is told of its own and not of those of the elements in it.
  const listen = (target: string) => ({
    onAnimationStart: (event) => event.target === event.currentTarget && record("animationstart", target, event.animationName),
    onAnimationIteration: (event) => event.target === event.currentTarget && record("animationiteration", target, event.animationName),
    onAnimationEnd: (event) => event.target === event.currentTarget && record("animationend", target, event.animationName),
    onAnimationCancel: (event) => event.target === event.currentTarget && record("animationcancel", target, event.animationName),
  })
  const box = (extra: Record<string, unknown>) => ({ backgroundColor: "#ff0000", margin: 5, padding: 5, flexShrink: 0, ...LOOP, ...extra })
  return (
    <div style={{ width: 700, height: 900, position: "relative", backgroundColor: "#ffffff" }}>
      <div testId="scroller" style={{ position: "absolute", left: 0, top: 0, width: 400, height: 500, overflowY: "auto" }}>
        <div style={{ position: "relative", height: 6000, flexShrink: 0 }}>
          <div
            testId="host"
            style={{
              position: "absolute",
              left: 0,
              top: 100 + (mode === "auto" ? FAR : 0),
              width: 300,
              height: 200,
              backgroundColor: "#ffffff",
              contentVisibility: mode === "hidden" ? (skipped ? "hidden" : "visible") : "auto",
              containIntrinsicSize: "300px 200px",
            }}
          >
            <div testId="a" {...listen("a")} style={box({})}>
              <div testId="t" {...listen("t")} style={box({ width: 150, height: 60 })}>
                <div testId="c" {...listen("c")} style={box({ width: 50, height: 20 })} />
              </div>
              <div testId="s" {...listen("s")} style={box({ width: 100, height: 20 })} />
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}

function play(read: string, mode: Mode): Played {
  const root = createTestRoot({ width: 700, height: 900 })
  const renderer = root.renderer
  const played: Played = { lists: [], afterReveal: [], starts: [], told: [], revealAt: -1, timeline0: 0, readAt: [] }
  let now = 0
  let skipped = mode === "hidden"
  const render = () =>
    root.render(<Page mode={mode} skipped={skipped} record={(type, target, name) => played.told.push({ type, target, name, at: now })} />)
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
  const idOf = (name: string) => renderer.findByTestId(name).id
  const list = (): Listed => {
    const names = new Map(["a", "t", "c", "s"].map((name) => [idOf(name), name]))
    return {
      at: now,
      rows: renderer.getSubtreeAnimations(idOf("host")).map((entry) => ({
        el: names.get(entry.elementId),
        name: entry.info.animationName,
        id: entry.info.id,
        state: entry.info.playState,
        pending: entry.info.pending,
        currentTime: entry.info.currentTime,
        startTime: entry.info.startTime ?? null,
      })),
    }
  }
  const reading = {
    none: () => {},
    getanims: () => renderer.getAnimations(idOf("t")),
    rect: () => renderer.getElementBounds(idOf("t")),
    offsetw: () => renderer.getElementBounds(idOf("t")),
    doc: () => renderer.getDocumentAnimations(),
  }[read]
  try {
    renderer.clockPause()
    renderer.setInlineDraws(false)
    renderer.setA11yActive(false)
    render()
    renderer.flush()
    played.timeline0 = renderer.getAnimationTimelineTime()
    elapse(0)
    for (const [ms, action] of STEPS) {
      advanceTo(ms)
      for (const part of action.split("+")) {
        if (part === "read") {
          reading()
          played.readAt.push(now)
        } else if (part === "list") played.lists.push(list())
        else if (part === "reveal") {
          played.revealAt = now
          if (mode === "hidden") {
            skipped = false
            render()
          } else renderer.scrollTo(idOf("scroller"), 0, FAR, "instant") // the host sits FAR below the top: this brings it near
        }
      }
    }
    renderer.flush()
    renderer.drawFrame()
    renderer.dispatchNativeEvents()
    played.afterReveal.push(list())
    for (let frame = 1; frame <= FRAMES_TO_TELL; frame++) {
      elapse(FRAME)
      played.afterReveal.push(list())
    }
    advanceTo(played.revealAt + 400)
    played.afterReveal.push(list())
    played.starts = played.told.filter((seen) => seen.type === "animationstart")
    return played
  } finally {
    root.unmount()
  }
}

const key = (row: { el: string; n?: string; name?: string }) => `${row.el}:${NAME[row.n] ?? row.name}`
const describeNative = hasNativeTestRenderer ? describe : describe.skip

/** The `q7` scenes against Chrome's, for `mode`. */
export function defineAnimationScopeTests(mode: Mode) {
  describeNative(`what a read styles of a never-rendered host skipped by ${mode}, against Chrome`, { timeout: 120_000 }, () => {
    const played = new Map<string, Played>()
    const playedRead = (read: string) => {
      if (!played.has(read)) played.set(read, play(read, mode))
      return played.get(read)
    }
    const chromeOf = (read: string) => {
      const found = ORACLE.scenes[`q7_${read}:${mode}`]
      if (!found) throw new Error(`the oracle has no scene q7_${read}:${mode}`)
      const lists = found.log.filter((entry) => entry.list)
      return {
        before: lists.filter((entry) => entry.list.startsWith("read")),
        final: lists.find((entry) => entry.list === "+400"),
        starts: found.log.filter((entry) => entry.ev === "animationstart"),
        events: found.log.filter((entry) => entry.ev),
        reveal: found.log.find((entry) => entry.did === "reveal"),
      }
    }

    for (const read of READS) {
      it(`${read}: lists the animations of the whole host or none, pending and then running, the same objects at the reveal`, () => {
        const chrome = chromeOf(read)
        const run = playedRead(read)
        expect(run.lists.length).toBe(chrome.before.length)
        run.lists.forEach((listed, at) => {
          const was = chrome.before[at]
          expect(listed.rows.map(key).sort(), `list ${at}: the animations of ${read}`).toEqual(was.subtree.map(key).sort())
          for (const row of listed.rows) {
            const expected = was.subtree.find((candidate) => key(candidate) === key(row))
            expect(row.state, `${key(row)} at list ${at}`).toBe(expected.state)
            expect(row.pending, `${key(row)} waits for a frame at list ${at}`).toBe(expected.pending)
            if (expected.pending) {
              expect(row.currentTime).toBe(0)
              expect(row.startTime).toBeNull()
            } else {
              expect(Math.abs(row.currentTime - expected.ct), `time of ${key(row)} at list ${at}`).toBeLessThanOrEqual(2 * FRAME)
              const started = row.startTime - run.timeline0 - run.readAt[0]
              expect(started, `${key(row)} starts at the first frame after the read`).toBeGreaterThan(0)
              expect(started).toBeLessThanOrEqual(2 * FRAME)
            }
          }
        })
        const final = run.afterReveal.at(-1)
        expect(final.rows.map(key).sort(), "all four after the reveal").toEqual(chrome.final.subtree.map(key).sort())
        for (const row of final.rows) {
          expect(row.state, key(row)).toBe("running")
          expect(row.pending, key(row)).toBe(false)
        }
        // The objects that a read listed are the ones that run on after the reveal, and none starts over.
        const first = chrome.before[0].subtree.length ? run.lists[0] : null
        if (first) {
          for (const row of final.rows) {
            const read1 = first.rows.find((candidate) => candidate.el === row.el && candidate.name === row.name)
            expect(row.id, `${key(row)} is the object the read listed`).toBe(read1.id)
            expect(row.startTime, `${key(row)} did not start over`).toBeCloseTo(run.lists.at(-1).rows.find((candidate) => candidate.id === row.id).startTime, 3)
          }
        }
      })

      it(`${read}: tells the starts after the reveal and nothing before it`, () => {
        const chrome = chromeOf(read)
        const run = playedRead(read)
        expect(run.told.filter((seen) => seen.at < run.revealAt), "nothing is told while the host is skipped").toEqual([])
        const told = run.starts.map((seen) => `${seen.target}:${seen.name}`).sort()
        expect(told).toEqual(chrome.starts.map((entry) => `${entry.target}:${NAME[entry.animation]}`).sort())
        for (const seen of run.starts) {
          expect(seen.at - run.revealAt, `${seen.target}:${seen.name}`).toBeGreaterThanOrEqual(0)
          expect(seen.at - run.revealAt).toBeLessThanOrEqual(FRAMES_TO_TELL * FRAME + FRAME)
        }
      })
    }
  })
}
