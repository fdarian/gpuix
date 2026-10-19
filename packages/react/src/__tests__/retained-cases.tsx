// @ts-nocheck
/**
 * A page of retained cards with one property changed in one of four places, and the frames compared
 * with the same page built whole: the shape of the cases of `retained-round6` and `retained-parity`.
 */

import React from "react"
import { expect, it } from "vitest"
import { Card, Page, setup, verifying } from "./retained-harness.js"

/** Each case builds a page twice or more, which takes seconds when the suites run in parallel. */
export const TIMEOUT = 60_000
export const PROSE = "aaaa bbbb cccc dddd eeeeeeee"
export const WORLD = { fontFamily: "Menlo", fontSize: 12, lineHeight: "15px" }
export const CARDS = 12

export type Style = Record<string, unknown>
export type Placement = "ancestor" | "page" | "card" | "prose" | "span"

/** A card with a box of text under its button, 140 wide: 19 columns of Menlo 12. */
export function ProseCard({
  index,
  style,
  prose,
  text = PROSE,
  span,
}: {
  index: number
  style?: Style
  prose?: Style
  text?: string
  /** The middle of three spans in the box, in place of its text node. */
  span?: Style
}) {
  return (
    <Card
      index={index}
      style={style}
      extra={
        <div testId={`prose-${index}`} style={{ width: 140, ...prose }}>
          {span ? (
            <>
              <span>{"aaaa "}</span>
              <span testId={`span-${index}`} style={span}>
                {"bbbb cccc dddd eeee"}
              </span>
              <span>{" eeeeeeee"}</span>
            </>
          ) : (
            <text>{text}</text>
          )}
        </div>
      }
    />
  )
}

export function World({ style, world = WORLD, children }: { style?: Style; world?: Style; children?: React.ReactNode }) {
  return <div style={{ ...world, ...style }}>{children}</div>
}

/** What is watched: the boxes of the text, and the one after the page, which only moves when the page does. */
export const proseIds = [...Array.from({ length: CARDS }, (_, index) => `prose-${index}`), "after"]

export type Case = {
  name: string
  a: Style
  b: Style
  text?: string
  /** Always on the box that is not the one under test, so the property has something to act on. */
  prose?: Style
  card?: Style
  world?: Style
  /** The first value of the property on the ancestor is the same as none: the world sets it. */
  placements?: Placement[]
  heights?: Height[]
  /** Style of the middle span, which the box holds three of in place of a text node. */
  span?: Style
}

/** A card of a fixed size cannot move what is around it; one as tall as its content can. */
export type Height = "fixed" | "content"

export function render(test: Case, placement: Placement, which: "a" | "b", height: Height) {
  const value = test[which]
  return (
    <World style={placement === "ancestor" ? value : undefined} world={test.world}>
      <Page
        style={placement === "page" ? value : undefined}
        card={(index) => (
          <ProseCard
            key={index}
            index={index}
            text={test.text}
            style={{ ...(height === "content" ? { height: "auto", alignSelf: "flex-start" } : undefined), ...test.card, ...(placement === "card" ? value : undefined) }}
            prose={{ ...test.prose, ...(placement === "prose" ? value : undefined) }}
            span={placement === "span" ? { ...test.span, ...value } : test.span}
          />
        )}
      />
      <div testId="after" style={{ height: 10, backgroundColor: "#ff00ff" }} />
    </World>
  )
}

/** The property is changed twice and changed back, and every frame shows what a build does. */
export function exercise(test: Case, placement: Placement, height: Height) {
  const { renderer, render: rerender, settle, expectWhatABuildShows, finish, unmount } = setup(render(test, placement, "a", height))
  try {
    settle()
    const first = expectWhatABuildShows("a", proseIds)
    settle()
    rerender(render(test, placement, "b", height))
    renderer.flush()
    const second = expectWhatABuildShows("b", proseIds)
    expect(
      second.picture !== first.picture || JSON.stringify(second.watched) !== JSON.stringify(first.watched),
      "the two values show differently"
    ).toBe(true)
    settle()
    expectWhatABuildShows("b-settled", proseIds)
    settle()
    rerender(render(test, placement, "a", height))
    renderer.flush()
    const third = expectWhatABuildShows("a-again", proseIds)
    expect(third.picture === first.picture).toBe(true)
    expect(third.watched).toEqual(first.watched)
    settle()
    finish()
  } finally {
    unmount()
  }
}

export const ALL: Placement[] = ["ancestor", "card", "prose"]
export const WHERE = { ancestor: "an ancestor above the cards", page: "the page of the cards", card: "the card", prose: "a box in the card", span: "a span in the box" }

/** One test per case, placement and card height. */
export function describeCases(cases: Case[]) {
  for (const test of cases) {
    for (const placement of test.placements ?? ALL) {
      for (const height of test.heights ?? (["fixed", "content"] as const)) {
        it(`${test.name} on ${WHERE[placement]}, cards of ${height === "fixed" ? "a fixed height" : "the height of their content"}`, () => {
          exercise(test, placement, height)
        })
      }
    }
  }
}

const MOVER = 2
const STEPS = 14

/**
 * Plays the page through the commit of the change and fourteen steps of 100 ms, and keeps what each
 * frame shows. A page played with retention off is the frames a full build gives.
 */
export function play(mover: (state: string) => React.ReactNode, retention: boolean) {
  const watch = ["mover", `card-${MOVER}`, "after"]
  const render = (state: string) => (
    <World>
      <Page
        card={(index) =>
          index === MOVER ? <Card key={index} index={index} extra={mover(state)} style={{ height: "auto", alignSelf: "flex-start" }} /> : null
        }
      />
      <div testId="after" style={{ height: 10, backgroundColor: "#ff00ff" }} />
    </World>
  )
  const page = setup(render("from"))
  try {
    page.renderer.setRetainPlanning(retention)
    page.renderer.clockPause()
    page.settle()
    const frames = [page.snapshot("rest", watch)]
    const replays = page.counters().replays
    const built = page.built()
    const painted = page.renderer.getPaintedFrameCount()
    // The commit frame: the animation is asked for and starts in the frame that lays out the change.
    page.render(render("to"))
    page.renderer.flush()
    frames.push(page.snapshot("commit", watch))
    for (let step = 0; step < STEPS; step++) {
      page.renderer.clockFastForward(100)
      frames.push(page.snapshot(`step-${step}`, watch))
    }
    const result = {
      frames,
      replays: page.counters().replays - replays,
      builtPerFrame: (page.built() - built) / (page.renderer.getPaintedFrameCount() - painted),
    }
    page.settle()
    frames.push(page.snapshot("end", watch))
    expect(page.counters().mismatches, page.renderer.getRetentionMismatches().join("\n")).toBe(0)
    return result
  } finally {
    page.unmount()
  }
}

export function expectTheFramesOfABuild(mover: (state: string) => React.ReactNode) {
  const kept = play(mover, true)
  const whole = play(mover, false)
  kept.frames.forEach((frame, index) => {
    const name = index === 0 ? "rest" : index === 1 ? "commit" : index === STEPS + 2 ? "end" : `step ${index - 2}`
    expect(frame.watched, `${name}: boxes`).toEqual(whole.frames[index].watched)
    expect(frame.boxes, `${name}: cards`).toEqual(whole.frames[index].boxes)
    expect(frame.a11y, `${name}: accessibility`).toEqual(whole.frames[index].a11y)
    expect(frame.picture === whole.frames[index].picture, `${name}: picture`).toBe(true)
  })
  expect(new Set(kept.frames.map((frame) => frame.picture)).size, "the animation shows").toBeGreaterThan(4)
  expect(kept.replays, "the cards beside the mover are replayed").toBeGreaterThan(STEPS)
  // A frame builds the mover and its card, not the page.
  if (!verifying) expect(kept.builtPerFrame).toBeLessThan(CARDS * 8 * 0.5)
}

