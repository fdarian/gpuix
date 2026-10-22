// @ts-nocheck
/**
 * A card that is a retained root changes its opacity through a state style (`hover`, `focusWithin`, `focus`) or a write
 * from script, with and without a transition. Opacity below 1 makes the card an effect group that is lifted into the
 * stacking context around it, and a transition of `opacity` that is current does it for the whole run, so the lift of
 * the card changes with the state while the record of its last build still has the old one. A record whose lift is
 * not the lift of the card now must not be skipped: the debug addon asserts it in gpui (`request_subtree_layout`, "the
 * root of a retained subtree is lifted as its element says"), and release painted the card in the wrong place of the
 * stacking context. Every frame is also compared with the same state built whole.
 *
 * Run with GPUIX_RETAIN_VERIFY=1 too: the counts of replays are not read there, only that the frames agree.
 */
import React from "react"
import { expect, it } from "vitest"
import { flushSync } from "../index"
import { Card, describeNative, idsByTestId, Page, setup, verifying, WINDOW } from "./retained-harness.js"

const CARD = 4
const ELSEWHERE = [WINDOW.width - 2, WINDOW.height - 2] as const

const TRANSITION = "opacity 240ms linear"

/** The opacity at rest and the one the state gives, both ways round and to nothing. */
const STEPS: Array<[number, number]> = [
  [1, 0.5],
  [0.5, 1],
  [1, 0],
]

type Mechanism = {
  /** The page, with the card at `from` and the state at `to`. */
  page: (from: number, to: number, transition: boolean, written?: number) => React.ReactElement
  enter: (view) => void
  leave: (view) => void
}

const focusOn = (view, testId: string) => {
  view.renderer.simulateKeystrokes("Tab")
  view.renderer.focusElement(idsByTestId(view.renderer).get(testId))
  view.renderer.drawFrame()
}

const carrier = (from: number, transition: boolean, extra: object) => ({
  opacity: from,
  ...(transition ? { transition: TRANSITION } : {}),
  ...extra,
})

const MECHANISMS: Record<string, Mechanism> = {
  hover: {
    page: (from, to, transition) => (
      <Page card={(index) => (index === CARD ? <Card key={index} index={index} style={carrier(from, transition, { hover: { opacity: to } })} /> : null)} />
    ),
    enter: (view) => {
      view.renderer.nativeSimulateMouseMove(...view.inside(CARD))
      view.renderer.drawFrame()
    },
    leave: (view) => {
      view.renderer.nativeSimulateMouseMove(...ELSEWHERE)
      view.renderer.drawFrame()
    },
  },
  "focus within": {
    page: (from, to, transition) => (
      <Page card={(index) => (index === CARD ? <Card key={index} index={index} style={carrier(from, transition, { focusWithin: { opacity: to } })} /> : null)} />
    ),
    enter: (view) => focusOn(view, `button-${CARD}`),
    leave: (view) => focusOn(view, "button-0"),
  },
  focus: {
    page: (from, to, transition) => (
      <Page
        card={(index) =>
          index === CARD ? (
            <div key={index} testId="carrier" tabIndex={0} style={carrier(from, transition, { focus: { opacity: to } })}>
              <Card index={index} />
            </div>
          ) : null
        }
      />
    ),
    enter: (view) => focusOn(view, "carrier"),
    leave: (view) => focusOn(view, "button-0"),
  },
  "group hover": {
    page: (from, to, transition) => (
      <Page
        card={(index) =>
          index === CARD ? (
            <div key={index} style={{ group: "g" }}>
              <Card index={index} style={carrier(from, transition, { groupHover: { opacity: to } })} />
            </div>
          ) : null
        }
      />
    ),
    enter: (view) => {
      view.renderer.nativeSimulateMouseMove(...view.inside(CARD))
      view.renderer.drawFrame()
    },
    leave: (view) => {
      view.renderer.nativeSimulateMouseMove(...ELSEWHERE)
      view.renderer.drawFrame()
    },
  },
  "has hover": {
    page: (from, to, transition) => (
      <Page card={(index) => (index === CARD ? <Card key={index} index={index} style={carrier(from, transition, { hasHover: { opacity: to } })} /> : null)} />
    ),
    enter: (view) => {
      view.renderer.nativeSimulateMouseMove(...view.center(`title-${CARD}`))
      view.renderer.drawFrame()
    },
    leave: (view) => {
      view.renderer.nativeSimulateMouseMove(...ELSEWHERE)
      view.renderer.drawFrame()
    },
  },
  "a write from script": {
    page: (from, to, transition) => <Written from={from} to={to} transition={transition} />,
    enter: (view) => {
      flushSync(() => write(true))
      view.renderer.drawFrame()
    },
    leave: (view) => {
      flushSync(() => write(false))
      view.renderer.drawFrame()
    },
  },
}

let write!: (written: boolean) => void
function Written({ from, to, transition }: { from: number; to: number; transition: boolean }) {
  const [written, setWritten] = React.useState(false)
  write = setWritten
  return (
    <Page card={(index) => (index === CARD ? <Card key={index} index={index} style={carrier(written ? to : from, transition, {})} /> : null)} />
  )
}

/** The transition run frame by frame, each compared with the state built whole every other tick. */
function run(view, tag: string) {
  for (let step = 0; step < 8; step++) {
    view.renderer.clockAdvance(40)
    view.renderer.drawFrame()
    if (step % 2 === 0) view.expectWhatABuildShows(`${tag}-${step}`)
  }
}

describeNative("a retained card whose opacity changes through a state or a write", () => {
  for (const [name, mechanism] of Object.entries(MECHANISMS)) {
    for (const transition of [false, true]) {
      for (const [from, to] of STEPS) {
        it(`is drawn as a build draws it when ${name} takes it from ${from} to ${to}${transition ? " with a transition" : ""}`, () => {
          const view = setup(mechanism.page(from, to, transition))
          try {
            view.settle()
            const tag = `${name.replace(/ /g, "-")}-${from}-${to}-${transition}`
            const at = view.expectWhatABuildShows(`${tag}-rest`)
            if (!verifying) expect(view.counters().replays, "the card is a root that is replayed").toBeGreaterThan(0)

            mechanism.enter(view)
            if (transition) run(view, `${tag}-in`)
            view.settle()
            const changed = view.expectWhatABuildShows(`${tag}-state`)
            expect(changed.picture === at.picture, "the state changes what is shown").toBe(false)

            mechanism.leave(view)
            if (transition) run(view, `${tag}-out`)
            view.settle()
            const back = view.expectWhatABuildShows(`${tag}-back`)
            // Moving the focus to another button draws that button's focus style.
            if (!name.startsWith("focus")) expect(back.picture === at.picture, "the card is back where it was").toBe(true)
            view.finish()
          } finally {
            view.unmount()
          }
        })
      }
    }
  }
})
