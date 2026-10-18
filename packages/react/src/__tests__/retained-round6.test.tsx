// @ts-nocheck
/**
 * What round 6 added, against the frames that skip what did not change (#37). Each case puts a
 * property on an ancestor above the cards, on the card (the root of a retained subtree) or on a box
 * inside it, lets the page settle so its cards replay, changes the property, and expects the first
 * frame after the change, and the ones after it, to be the frame of the same page built whole. The
 * two values must show, or a case would pass by the property doing nothing.
 *
 * Run with GPUIX_RETAIN_VERIFY=1 too: gpui then compares every replayed subtree with a build.
 */

import React from "react"
import { expect, it } from "vitest"
import { Card, describeNative, Page, setup } from "./retained-harness.js"

const PROSE = "aaaa bbbb cccc dddd eeeeeeee"
const WORLD = { fontFamily: "Menlo", fontSize: 12, lineHeight: "15px" }
const CARDS = 12

type Style = Record<string, unknown>
type Placement = "ancestor" | "page" | "card" | "prose"

/** A card with a box of text under its button, 140 wide: 19 columns of Menlo 12. */
function ProseCard({ index, style, prose, text = PROSE }: { index: number; style?: Style; prose?: Style; text?: string }) {
  return (
    <Card
      index={index}
      style={style}
      extra={
        <div testId={`prose-${index}`} style={{ width: 140, ...prose }}>
          <text>{text}</text>
        </div>
      }
    />
  )
}

function World({ style, world = WORLD, children }: { style?: Style; world?: Style; children?: React.ReactNode }) {
  return <div style={{ ...world, ...style }}>{children}</div>
}

/** What is watched: the boxes of the text, and the one after the page, which only moves when the page does. */
const proseIds = [...Array.from({ length: CARDS }, (_, index) => `prose-${index}`), "after"]

type Case = {
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
}

/** A card of a fixed size cannot move what is around it; one as tall as its content can. */
type Height = "fixed" | "content"

function render(test: Case, placement: Placement, which: "a" | "b", height: Height) {
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
          />
        )}
      />
      <div testId="after" style={{ height: 10, backgroundColor: "#ff00ff" }} />
    </World>
  )
}

/** The property is changed twice and changed back, and every frame shows what a build does. */
function exercise(test: Case, placement: Placement, height: Height) {
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

const ALL: Placement[] = ["ancestor", "card", "prose"]
const WHERE = { ancestor: "an ancestor above the cards", page: "the page of the cards", card: "the card", prose: "a box in the card" }

const CASES: Case[] = [
  { name: "textWrapMode", a: { textWrapMode: "wrap" }, b: { textWrapMode: "nowrap" } },
  { name: "textWrapStyle balance", a: { textWrapStyle: "auto" }, b: { textWrapStyle: "balance" } },
  { name: "textWrapStyle pretty", a: { textWrapStyle: "auto" }, b: { textWrapStyle: "pretty" }, text: "aaaa bbbb cccc ddd ee" },
  { name: "textWrap shorthand", a: { textWrap: "wrap" }, b: { textWrap: "nowrap" } },
  { name: "textWrap balance shorthand", a: { textWrap: "wrap" }, b: { textWrap: "balance" } },
  {
    name: "textBoxEdge",
    a: { textBoxEdge: "text" },
    b: { textBoxEdge: "cap alphabetic" },
    prose: { textBoxTrim: "trim-both" },
    placements: ALL,
  },
  { name: "textUnderlineOffset", a: { textUnderlineOffset: 1 }, b: { textUnderlineOffset: 6 }, prose: { textDecorationLine: "underline" } },
  { name: "fontVariationSettings", a: { fontVariationSettings: '"wght" 100' }, b: { fontVariationSettings: '"wght" 900' }, world: { fontSize: 12, lineHeight: "15px" } },
  { name: "fontWeight of a variable font", a: { fontWeight: 200 }, b: { fontWeight: 800 }, world: { fontSize: 12, lineHeight: "15px" } },
  {
    name: "fontWeight with an axis that is declared",
    a: { fontWeight: 200 },
    b: { fontWeight: 800 },
    world: { fontSize: 12, lineHeight: "15px", fontVariationSettings: '"opsz" 30' },
  },
  { name: "outline", a: { outline: "none" }, b: { outline: "3px solid red" }, placements: ["card", "prose"] },
  { name: "outline without a style", a: { outline: "3px red" }, b: { outline: "3px solid red" }, placements: ["card", "prose"] },
  {
    name: "outlineOffset",
    a: { outlineOffset: 0 },
    b: { outlineOffset: 5 },
    card: { outline: "2px solid red" },
    prose: { outline: "2px solid red" },
    placements: ["card", "prose"],
  },
  {
    name: "outline in the color of the ancestor",
    a: { color: "#ff0000" },
    b: { color: "#00ff00" },
    card: { outline: "3px solid currentColor", outlineOffset: 3 },
    placements: ["ancestor"],
  },
  { name: "flex shorthand", a: { flex: "none" }, b: { flex: "1 1 200px" }, placements: ["card"] },
  { name: "flexFlow shorthand", a: { flexFlow: "row wrap" }, b: { flexFlow: "column nowrap" }, card: { display: "flex" }, placements: ["card"] },
  { name: "placeContent", a: { placeContent: "flex-start" }, b: { placeContent: "center" }, placements: ["page"] },
  { name: "placeItems", a: { placeItems: "flex-start" }, b: { placeItems: "flex-end" }, card: { display: "flex", flexDirection: "column" }, placements: ["card"] },
  { name: "placeSelf", a: { placeSelf: "flex-start" }, b: { placeSelf: "flex-end" }, placements: ["prose"], card: { display: "flex", flexDirection: "column" }, prose: { width: 60 } },
  { name: "padding of several values", a: { padding: "0" }, b: { padding: "2px 6px 4px" }, placements: ["card", "prose"] },
  { name: "margin of several values", a: { margin: "0" }, b: { margin: "1px 3px" }, placements: ["card", "prose"] },
  { name: "gap of two values", a: { gap: "8px 8px" }, b: { gap: "2px 16px" }, placements: ["page"] },
  {
    name: "borderWidth of four values",
    a: { borderWidth: "0" },
    b: { borderWidth: "1px 2px 3px 4px" },
    card: { borderStyle: "solid", borderColor: "#ffffff" },
    prose: { borderStyle: "solid", borderColor: "#ffffff" },
    placements: ["card", "prose"],
  },
  { name: "borderBlock and borderInline", a: { borderBlock: "none", borderInline: "none" }, b: { borderBlock: "2px solid #fff", borderInline: "1px dashed #fff" }, placements: ["card", "prose"] },
  { name: "paddingInline and paddingBlock", a: { paddingInline: 0, paddingBlock: 0 }, b: { paddingInline: "4px 8px", paddingBlock: "2px" }, placements: ["card", "prose"] },
  { name: "marginInline and marginBlock", a: { marginInline: 0, marginBlock: 0 }, b: { marginInline: "3px", marginBlock: "1px 2px" }, placements: ["card", "prose"] },
  { name: "inset of several values", a: { position: "relative", inset: "0" }, b: { position: "relative", inset: "2px 4px" }, placements: ["card", "prose"] },
  {
    name: "a border on one side of a box with no height",
    a: { borderTop: "none" },
    b: { borderTop: "3px solid red" },
    prose: { height: 0, overflow: "visible" },
    placements: ["prose"],
  },
]

describeNative("round 6 properties against retained frames", () => {
  for (const test of CASES) {
    for (const placement of test.placements ?? ALL) {
      for (const height of ["fixed", "content"] as const) {
        const where = WHERE[placement]
        it(`${test.name} on ${where}, cards of ${height === "fixed" ? "a fixed height" : "the height of their content"}`, () => {
          exercise(test, placement, height)
        })
      }
    }
  }
})

/** Twelve blocks one under the other, each a root of its own: the first and the last are the ones a
 *  `textBoxTrim` of the box around them reaches. */
function Stack({ wrapper, line, first }: { wrapper?: Style; line?: Style; first?: Style }) {
  return (
    <div testId="wrapper" style={{ ...WORLD, color: "#ff0000", backgroundColor: "#ffffcc", textBoxEdge: "cap alphabetic", ...wrapper }}>
      {Array.from({ length: CARDS }, (_, index) => (
        <div key={index} testId={`line-${index}`} style={{ ...line, ...(index === 0 ? first : undefined) }}>
          <div>
            <text>{`line ${index}`}</text>
          </div>
        </div>
      ))}
    </div>
  )
}

const lineIds = Array.from({ length: CARDS }, (_, index) => `line-${index}`)

describeNative("text-box-trim handed down to blocks that are retained", () => {
  it("reaches the first and the last block, and only them", () => {
    const render = (trim: string) => <Stack wrapper={{ textBoxTrim: trim }} />
    const { renderer, render: rerender, settle, bounds, expectWhatABuildShows, finish, unmount } = setup(render("none"))
    try {
      settle()
      const untrimmed = expectWhatABuildShows("trim-none", lineIds)
      const heights = lineIds.map((id) => bounds(id).height)
      settle()
      rerender(render("trim-both"))
      renderer.flush()
      const trimmed = expectWhatABuildShows("trim-both", lineIds)
      const after = lineIds.map((id) => bounds(id).height)
      expect(after[0]).toBeLessThan(heights[0])
      expect(after[CARDS - 1]).toBeLessThan(heights[CARDS - 1])
      expect(after[1]).toBe(heights[1])
      expect(trimmed.picture === untrimmed.picture).toBe(false)
      settle()
      rerender(render("trim-start"))
      renderer.flush()
      expectWhatABuildShows("trim-start", lineIds)
      expect(bounds("line-0").height).toBe(after[0])
      expect(bounds(`line-${CARDS - 1}`).height).toBe(heights[CARDS - 1])
      settle()
      rerender(render("none"))
      renderer.flush()
      const back = expectWhatABuildShows("trim-back", lineIds)
      expect(back.picture === untrimmed.picture).toBe(true)
      settle()
      finish()
    } finally {
      unmount()
    }
  })

  it("stops at padding that a block gains, and goes through it when the padding goes", () => {
    const render = (padding: number) => <Stack wrapper={{ textBoxTrim: "trim-both" }} first={{ paddingTop: padding }} />
    const { renderer, render: rerender, settle, bounds, expectWhatABuildShows, finish, unmount } = setup(render(0))
    try {
      settle()
      expectWhatABuildShows("pad-0", lineIds)
      const open = bounds("line-0").height
      settle()
      rerender(render(6))
      renderer.flush()
      expectWhatABuildShows("pad-6", lineIds)
      expect(bounds("line-0").height).toBeGreaterThan(open)
      settle()
      rerender(render(0))
      renderer.flush()
      expectWhatABuildShows("pad-0-again", lineIds)
      expect(bounds("line-0").height).toBe(open)
      settle()
      finish()
    } finally {
      unmount()
    }
  })

  it("changes with the edge an ancestor hands down while the blocks are replayed", () => {
    const render = (edge: string) => (
      <div style={{ textBoxEdge: edge }}>
        <Stack wrapper={{ textBoxEdge: undefined, textBoxTrim: "trim-both" }} />
      </div>
    )
    const { renderer, render: rerender, settle, bounds, expectWhatABuildShows, finish, unmount } = setup(render("cap alphabetic"))
    try {
      settle()
      expectWhatABuildShows("edge-cap", lineIds)
      const cap = bounds("line-0").height
      settle()
      rerender(render("ex alphabetic"))
      renderer.flush()
      expectWhatABuildShows("edge-ex", lineIds)
      expect(bounds("line-0").height).not.toBe(cap)
      settle()
      finish()
    } finally {
      unmount()
    }
  })
})
