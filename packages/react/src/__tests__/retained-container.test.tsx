// @ts-nocheck
/**
 * An element that reads a query container, a container, and the parent of a child with `@container`
 * blocks are replayed with the cards around them (#37): what a container query selects has to flip
 * in the frame the container is resized, not in the one after, whether the box that reads it is
 * laid out again or not. Each test takes one flip with one frame, and expects the frame, its
 * accessibility tree and its boxes to be those of a build of the same state.
 */

import React from "react"
import { expect, it } from "vitest"
import { Card, CARD_BG, describeNative, GREEN, Page, setup } from "./retained-harness.js"

/**
 * One display-link frame of a change: the window draws once, as a live one does, and not for each
 * call. What the frame shows is read from it before anything draws again: a screenshot draws, and
 * so does a read once the window is drawn on demand, which would correct a frame that replayed
 * what it should have built. Painted text and published boxes are read without a draw.
 */
function oneFrame<T>(renderer: any, change: () => void, read: () => T): T {
  renderer.setInlineDraws(false)
  try {
    change()
    renderer.drawFrame()
    return read()
  } finally {
    renderer.setInlineDraws(true)
  }
}

const WIDE = { "@container": { "(min-width: 400px)": { backgroundColor: "#00ff00" } } }

/** A line of text that a container query hides without moving anything. */
const Sign = ({ id, hideFrom }: { id: string; hideFrom: number }) => (
  <div testId={id} style={{ height: 14, width: 60, "@container": { [`(min-width: ${hideFrom}px)`]: { visibility: "hidden" } } }}>
    <text>{id}</text>
  </div>
)

describeNative("a page of retained cards and query containers", () => {
  it("flips a card that reads a container outside it in the frame the container resizes, with the card's box unchanged", () => {
    const render = (width: number) => (
      <div testId="container" style={{ containerType: "inline-size", width }}>
        <Page card={(index) => (index === 0 ? <Card key={index} index={index} style={WIDE} extra={<Sign id="sign-out" hideFrom={400} />} /> : null)} />
      </div>
    )
    const { renderer, counters, render: rerender, settle, inside, sample, expectWhatABuildShows, finish, unmount } = setup(render(300))
    try {
      settle()
      expect(renderer.getPaintedText()).toContain("sign-out")
      expect(sample("cq-out-narrow", ...inside(0))).toEqual(CARD_BG)
      const before = counters().replays
      expect(oneFrame(renderer, () => rerender(render(600)), () => renderer.getPaintedText())).not.toContain("sign-out")

      expect(sample("cq-out-wide", ...inside(0))).toEqual(GREEN)
      expect(counters().replays).toBeGreaterThan(before)
      expect(oneFrame(renderer, () => rerender(render(300)), () => renderer.getPaintedText())).toContain("sign-out")

      settle()
      expectWhatABuildShows("cq-out")
      finish()
    } finally {
      unmount()
    }
  })

  it("flips a card that reads a container when the window resizes it", () => {
    const render = () => (
      <div testId="container" style={{ containerType: "inline-size", width: "100%" }}>
        <Page card={(index) => (index === 0 ? <Card key={index} index={index} extra={<Sign id="sign-window" hideFrom={500} />} /> : null)} />
      </div>
    )
    const { renderer, settle, expectWhatABuildShows, finish, unmount } = setup(render())
    try {
      settle()
      expect(renderer.getPaintedText()).not.toContain("sign-window")
      expect(oneFrame(renderer, () => renderer.setWindowSize(400, 480), () => renderer.getPaintedText())).toContain("sign-window")

      expect(oneFrame(renderer, () => renderer.setWindowSize(640, 480), () => renderer.getPaintedText())).not.toContain("sign-window")

      settle()
      expectWhatABuildShows("cq-window")
      finish()
    } finally {
      unmount()
    }
  })

  it("flips what a container inside a card selects, in the frame the card is resized", () => {
    const render = (width: number) => (
      <Page
        card={(index) =>
          index === 3 ? <Card key={index} index={index} style={{ width, containerType: "inline-size" }} extra={<Sign id="sign-in" hideFrom={150} />} /> : null
        }
      />
    )
    const { renderer, render: rerender, settle, expectWhatABuildShows, finish, unmount } = setup(render(140))
    try {
      settle()
      expect(renderer.getPaintedText()).toContain("sign-in")
      expect(oneFrame(renderer, () => rerender(render(200)), () => renderer.getPaintedText())).not.toContain("sign-in")

      expect(oneFrame(renderer, () => rerender(render(140)), () => renderer.getPaintedText())).toContain("sign-in")

      settle()
      expectWhatABuildShows("cq-in-card")
      finish()
    } finally {
      unmount()
    }
  })

  it("flips what a container selects when a sibling resizes it, with the box that reads it unchanged", () => {
    const render = (spacer: number) => (
      <div style={{ display: "flex", width: 400 }}>
        <div style={{ width: spacer, height: 10, flexShrink: 0 }} />
        <Card
          index={3}
          style={{ width: "auto", flexGrow: 1, flexShrink: 1, containerType: "inline-size" }}
          extra={<Sign id="sign-sibling" hideFrom={150} />}
        />
      </div>
    )
    const { renderer, render: rerender, settle, expectWhatABuildShows, finish, unmount } = setup(render(260))
    try {
      settle()
      expect(renderer.getPaintedText()).toContain("sign-sibling")
      expect(oneFrame(renderer, () => rerender(render(200)), () => renderer.getPaintedText())).not.toContain("sign-sibling")
      expect(oneFrame(renderer, () => rerender(render(260)), () => renderer.getPaintedText())).toContain("sign-sibling")
      settle()
      expectWhatABuildShows("cq-sibling")
      finish()
    } finally {
      unmount()
    }
  })

  it("shows and hides a child by a container query while its parent is replayed", () => {
    const render = (width: number) => (
      <div testId="container" style={{ containerType: "inline-size", width }}>
        <Page
          card={(index) =>
            index === 1 ? (
              <Card
                key={index}
                index={index}
                extra={<div testId="gone-when-wide" style={{ height: 10, width: 10, "@container": { "(min-width: 400px)": { display: "none" } } }} />}
              />
            ) : null
          }
        />
      </div>
    )
    const { renderer, render: rerender, settle, bounds, expectWhatABuildShows, finish, unmount } = setup(render(300))
    try {
      settle()
      expect(bounds("gone-when-wide").width).toBe(10)
      expect(oneFrame(renderer, () => rerender(render(600)), () => bounds("gone-when-wide"))).toBeNull()
      expect(oneFrame(renderer, () => rerender(render(300)), () => bounds("gone-when-wide")).width).toBe(10)
      settle()
      expectWhatABuildShows("cq-display")
      finish()
    } finally {
      unmount()
    }
  })
})
