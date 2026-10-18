// @ts-nocheck
/**
 * A child spliced out of a `display: contents` element is replayed with the cards around it (#37):
 * its build reads the inheritable style of the elements it was spliced out of, which are not in its
 * own revision, so the key of the child has to say which styles those were.
 */

import React from "react"
import { expect, it } from "vitest"
import { Card, describeNative, Page, setup } from "./retained-harness.js"

const Probe = () => <div testId="probe" style={{ width: "10em", height: 5 }} />

describeNative("a page of retained cards, one of them inside display: contents", () => {
  it("builds a spliced card again when the style the wrapper hands down changes, and replays it otherwise", () => {
    const render = (fontSize: number) => (
      <Page
        card={(index) =>
          index === 2 ? (
            <div key={index} testId="wrapper" style={{ display: "contents", fontSize }}>
              <Card index={index} extra={<Probe />} />
            </div>
          ) : null
        }
      />
    )
    const { renderer, counters, render: rerender, settle, bounds, expectWhatABuildShows, finish, unmount } = setup(render(10))
    try {
      settle()
      expect(bounds("probe").width).toBe(100)
      const before = counters().replays
      renderer.flush()
      expect(counters().replays).toBeGreaterThan(before)
      rerender(render(20))
      settle()
      expect(bounds("probe").width).toBe(200)
      rerender(render(12))
      settle()
      expect(bounds("probe").width).toBe(120)
      expectWhatABuildShows("contents-font")
      finish()
    } finally {
      unmount()
    }
  })

  it("follows a wrapper that is turned into display: none and back, as the demo's cull wrappers are", () => {
    const render = (display: string) => (
      <Page
        card={(index) =>
          index === 2 ? (
            <div key={index} testId="wrapper" style={{ display, fontSize: 10 }}>
              <Card index={index} extra={<Probe />} />
            </div>
          ) : null
        }
      />
    )
    const { renderer, render: rerender, settle, bounds, expectWhatABuildShows, finish, unmount } = setup(render("contents"))
    try {
      settle()
      expect(bounds("probe").width).toBe(100)
      rerender(render("none"))
      settle()
      expect(bounds("probe")).toBeNull()
      rerender(render("contents"))
      settle()
      expect(bounds("probe").width).toBe(100)
      expectWhatABuildShows("contents-none")
      finish()
    } finally {
      unmount()
    }
  })

  it("follows a wrapper that is added around the card and removed again", () => {
    const render = (wrapped: boolean) => (
      <Page
        card={(index) =>
          index === 2 ? (
            wrapped ? (
              <div key="wrapper" testId="wrapper" style={{ display: "contents", fontSize: 20 }}>
                <Card key={index} index={index} extra={<Probe />} />
              </div>
            ) : (
              <Card key={index} index={index} extra={<Probe />} />
            )
          ) : null
        }
      />
    )
    const { renderer, render: rerender, settle, bounds, expectWhatABuildShows, finish, unmount } = setup(render(false))
    try {
      settle()
      expect(bounds("probe").width).toBe(160)
      rerender(render(true))
      settle()
      expect(bounds("probe").width).toBe(200)
      rerender(render(false))
      settle()
      expect(bounds("probe").width).toBe(160)
      expectWhatABuildShows("contents-wrap")
      finish()
    } finally {
      unmount()
    }
  })
})
