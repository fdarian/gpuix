// @ts-nocheck
/**
 * What the parity pass added to the style, against the frames that skip what did not change (#37):
 * each field is changed on an ancestor, the page, the card, a box in it or a span in the box, and
 * the frames after the change must be the frames of the same page built whole. Run with
 * GPUIX_RETAIN_VERIFY=1 too: gpui then compares every replayed subtree with a build.
 */

import React from "react"
import { expect, it } from "vitest"
import { defineKeyframes, motion } from "../index"
import { type Case, describeCases, expectTheFramesOfABuild, TIMEOUT, World } from "./retained-cases.js"
import { Card, describeNative, elementsAtPoints, idsByTestId, Page, type Rgba, setup } from "./retained-harness.js"

defineKeyframes("parity-border", { from: { borderColor: "#ff0000", borderWidth: 2 }, to: { borderColor: "#0000ff", borderWidth: 8 } })

const CONTENT = ["content"] as const
const NORMAL_LINES = { fontSize: 14, lineHeight: "normal", fontFamily: "Menlo" }

const CASES: Case[] = [
  // S7: direction and the text alignment that resolves against it
  { name: "direction", a: { direction: "ltr" }, b: { direction: "rtl" }, prose: { textAlign: "start" }, text: "aaaa bbbb", heights: CONTENT },
  { name: "text-align start against an inherited rtl", a: { textAlign: "start" }, b: { textAlign: "end" }, world: { ...NORMAL_LINES, direction: "rtl" }, text: "aaaa", heights: CONTENT },
  {
    name: "direction under a text-align that was handed down",
    a: { direction: "ltr" },
    b: { direction: "rtl" },
    world: { ...NORMAL_LINES, textAlign: "end" },
    text: "aaaa",
    placements: ["card", "prose"],
    heights: CONTENT,
  },
  // S6: the strut font and a line height that is the font's
  { name: "font family under a normal line height", a: { fontFamily: "Menlo" }, b: { fontFamily: "Times New Roman" }, world: { fontSize: 14, lineHeight: "normal" }, heights: CONTENT },
  { name: "font size under a normal line height", a: { fontSize: 12 }, b: { fontSize: 18 }, world: { fontFamily: "Menlo", lineHeight: "normal" }, heights: CONTENT },
  { name: "normal line height itself", a: { lineHeight: "normal" }, b: { lineHeight: "20px" }, world: { fontFamily: "Menlo", fontSize: 14 }, heights: CONTENT },
  // S4 and S5: what an inline box hands to its paragraph
  { name: "white-space of a span", a: { whiteSpace: "normal" }, b: { whiteSpace: "nowrap" }, placements: ["span"], heights: CONTENT },
  { name: "text-wrap-mode of a span", a: { textWrapMode: "wrap" }, b: { textWrapMode: "nowrap" }, placements: ["span"], heights: CONTENT },
  { name: "padding of a span", a: { paddingLeft: 0, paddingRight: 0 }, b: { paddingLeft: 7, paddingRight: 3 }, placements: ["span"], heights: CONTENT },
  { name: "underline of a span", a: { textDecorationLine: "none" }, b: { textDecorationLine: "underline", textDecorationStyle: "dashed" }, placements: ["span"], heights: CONTENT },
  { name: "underline style of a span", a: { textDecorationLine: "underline", textDecorationStyle: "solid" }, b: { textDecorationLine: "underline", textDecorationStyle: "dotted" }, placements: ["span"], heights: CONTENT },
  { name: "strike and overline of a span", a: { textDecorationLine: "line-through" }, b: { textDecorationLine: "underline overline" }, placements: ["span"], heights: CONTENT },
  { name: "vertical-align of a span", a: { verticalAlign: "baseline" }, b: { verticalAlign: "super", fontSize: 9 }, placements: ["span"], heights: CONTENT },
  { name: "underline from the box around the spans", a: { textDecorationLine: "none" }, b: { textDecorationLine: "underline", textDecorationStyle: "wavy" }, placements: ["prose", "card", "ancestor"], span: { color: "#00ff00" }, heights: CONTENT },
  { name: "opacity of a span, which paints as a group", a: { opacity: 1 }, b: { opacity: 0.4 }, placements: ["span"], heights: CONTENT },
  { name: "background of a span", a: { backgroundColor: "#ffff00" }, b: { backgroundColor: "#00ffff" }, placements: ["span"], heights: CONTENT },
  { name: "font size of a span among normal lines", a: { fontSize: 12 }, b: { fontSize: 22 }, world: NORMAL_LINES, placements: ["span"], heights: CONTENT },
  { name: "font family of a span among normal lines", a: { fontFamily: "Menlo" }, b: { fontFamily: "Times New Roman" }, world: NORMAL_LINES, placements: ["span"], heights: CONTENT },
  // S10: mask-border
  {
    name: "mask-border",
    a: { maskBorderSource: "none" },
    b: { maskBorderSource: "linear-gradient(#000, transparent)", maskBorderSlice: "10 fill", maskBorderWidth: "10px" },
    placements: ["card", "prose"],
    heights: CONTENT,
  },
  {
    name: "mask-border-slice",
    a: { maskBorderSlice: "10 fill" },
    b: { maskBorderSlice: "45% fill", maskBorderRepeat: "round", maskBorderOutset: "4px" },
    card: { maskBorderSource: "linear-gradient(90deg, #000 50%, transparent 50%)", maskBorderWidth: "10px" },
    placements: ["card"],
    heights: CONTENT,
  },
  {
    name: "mask-border-slice",
    a: { maskBorderSlice: "10 fill" },
    b: { maskBorderSlice: "45% fill", maskBorderRepeat: "round", maskBorderOutset: "4px" },
    prose: { maskBorderSource: "linear-gradient(90deg, #000 50%, transparent 50%)", maskBorderWidth: "10px", backgroundColor: "#ff8800" },
    placements: ["prose"],
    heights: CONTENT,
  },
  // S12: the basis keywords
  { name: "flex-basis fit-content", a: { flexBasis: "auto" }, b: { flexBasis: "fit-content" }, card: { display: "flex" }, prose: { width: "auto" }, placements: ["prose"], heights: CONTENT },
  { name: "flex-basis min-content and max-content", a: { flexBasis: "min-content" }, b: { flexBasis: "max-content" }, card: { width: "auto" }, prose: { width: "auto" }, placements: ["card"], heights: CONTENT },
]

describeNative("parity fields against retained frames", { timeout: TIMEOUT }, () => {
  describeCases(CASES)
})

/** The elements under each point in the frame as it is, and then in the same state built whole. */
function elementsLikeABuild(renderer, points: Array<[number, number]>, names: Map<number, string>) {
  const kept = elementsAtPoints(renderer, points).map((ids) => ids.map((id) => names.get(id) ?? id))
  renderer.setRetainPlanning(false)
  renderer.flush()
  const whole = elementsAtPoints(renderer, points).map((ids) => ids.map((id) => names.get(id) ?? id))
  renderer.setRetainPlanning(true)
  renderer.flush()
  expect(kept).toEqual(whole)
  return kept
}

const names = (renderer) => new Map([...idsByTestId(renderer)].map(([testId, id]) => [id, testId]))

describeNative("disabled, enabled and what has them, in replayed cards", { timeout: TIMEOUT }, () => {
  const Control = ({ disabled }: { disabled: boolean }) => (
    <div
      testId="control"
      formControl={disabled ? "disabled" : "enabled"}
      style={{ width: 20, height: 10, backgroundColor: "#888888", disabled: { backgroundColor: "#ff0000" }, enabled: { backgroundColor: "#00ff00" } }}
    />
  )
  const cardStyle = {
    height: "auto",
    alignSelf: "flex-start",
    hasDisabled: { backgroundColor: "#aa0000" },
    hasEnabled: { backgroundColor: "#00aa00" },
  }
  const page = (control: "none" | "enabled" | "disabled") => (
    <Page
      card={(index) =>
        index === 3 ? (
          <Card
            key={index}
            index={index}
            style={cardStyle}
            extra={
              control === "none" ? null : (
                <div testId="wrap" style={{ hasDisabled: { paddingTop: 6 }, hasEnabled: { paddingBottom: 3 } }}>
                  <Control disabled={control === "disabled"} />
                </div>
              )
            }
          />
        ) : null
      }
    />
  )

  it("restyles the ancestors of a control that flips, that is added and that is removed, while the other cards replay", () => {
    const { renderer, render, settle, expectWhatABuildShows, finish, unmount } = setup(page("none"))
    try {
      settle()
      const pictures = new Map<string, string>()
      for (const control of ["none", "enabled", "disabled", "enabled", "none", "disabled", "none"] as const) {
        render(page(control))
        renderer.flush()
        const watched = control === "none" ? ["card-3"] : ["card-3", "wrap"]
        pictures.set(control, expectWhatABuildShows(`control-${control}`, watched).picture)
        settle()
        expectWhatABuildShows(`control-${control}-settled`, watched)
      }
      expect(new Set(pictures.values()).size, "none, enabled and disabled show differently").toBe(3)
      finish()
    } finally {
      unmount()
    }
  })
})

describeNative("a table with collapsed borders in a page of replayed cards", { timeout: TIMEOUT }, () => {
  const table = (state: string) => (
    <div style={{ display: "table", borderCollapse: "collapse" }}>
      <div style={{ display: "table-row" }}>
        <div
          testId="mover"
          style={{
            display: "table-cell",
            width: 60,
            height: 30,
            borderStyle: "solid",
            borderWidth: state === "from" ? 2 : 8,
            borderColor: state === "from" ? "#ff0000" : "#0000ff",
            transition: "border-color 1s linear, border-width 1s linear",
          }}
        />
        <div style={{ display: "table-cell", width: 60, height: 30, borderStyle: "solid", borderWidth: 2, borderColor: "#00ff00" }} />
      </div>
    </div>
  )

  it("paints the borders a cell transition moves, in every frame, as a build does", () => {
    expectTheFramesOfABuild(table)
  })

  it("does the same for a border that a CSS animation moves", () => {
    expectTheFramesOfABuild((state) => (
      <div style={{ display: "table", borderCollapse: "collapse" }}>
        <div style={{ display: "table-row" }}>
          <div
            testId="mover"
            style={{
              display: "table-cell",
              width: 60,
              height: 30,
              borderStyle: "solid",
              borderWidth: 2,
              borderColor: "#ff0000",
              animation: state === "from" ? "none" : "parity-border 1s linear forwards",
            }}
          />
          <div style={{ display: "table-cell", width: 60, height: 30, borderStyle: "solid", borderWidth: 2, borderColor: "#00ff00" }} />
        </div>
      </div>
    ))
  })

  it("mirrors the columns when the direction flips, with the borders shared as a build shares them", () => {
    const page = (direction: string) => (
      <Page
        card={(index) =>
          index === 2 ? (
            <Card
              key={index}
              index={index}
              style={{ height: "auto", alignSelf: "flex-start" }}
              extra={
                <div testId="table" style={{ display: "table", borderCollapse: "collapse", direction }}>
                  <div style={{ display: "table-row" }}>
                    <div testId="left" style={{ display: "table-cell", width: 40, height: 20, borderStyle: "solid", borderWidth: 1, borderColor: "#ff0000" }} />
                    <div testId="right" style={{ display: "table-cell", width: 40, height: 20, borderStyle: "solid", borderWidth: 6, borderColor: "#0000ff" }} />
                  </div>
                </div>
              }
            />
          ) : null
        }
      />
    )
    const { renderer, render, settle, bounds, expectWhatABuildShows, finish, unmount } = setup(page("ltr"))
    try {
      settle()
      const ltr = expectWhatABuildShows("table-ltr", ["table", "left", "right"])
      const leftOfRight = bounds("left").x < bounds("right").x
      render(page("rtl"))
      renderer.flush()
      const rtl = expectWhatABuildShows("table-rtl", ["table", "left", "right"])
      expect(bounds("left").x < bounds("right").x).not.toBe(leftOfRight)
      expect(rtl.picture === ltr.picture).toBe(false)
      settle()
      render(page("ltr"))
      settle()
      expectWhatABuildShows("table-ltr-again", ["table", "left", "right"])
      finish()
    } finally {
      unmount()
    }
  })
})

describeNative("focus variants of position, z-index and opacity, read by hit tests of replayed cards", { timeout: TIMEOUT }, () => {
  /** A is in flow under B, which is lifted to z 2; the variant of A decides which of them the point (60, 10) hits. */
  const overlay = (a: Record<string, unknown>, c = false, wrapper: Record<string, unknown> = {}) => (
    <Page
      card={(index) =>
        index === 4 ? (
          <Card
            key={index}
            index={index}
            style={{ height: "auto", alignSelf: "flex-start", width: 200 }}
            extra={
              <div testId="wrapper" style={{ position: "relative", zIndex: 0, width: 200, height: 100, ...wrapper }}>
                <div testId="a" tabIndex={0} style={{ width: 100, height: 100, backgroundColor: "#ff0000", keyboardFocusDim: false, ...a }}>
                  <text>a</text>
                  {c ? <div testId="c" style={{ position: "absolute", zIndex: 10, left: 60, top: 0, width: 20, height: 20, backgroundColor: "#ffff00" }} /> : null}
                </div>
                <div testId="b" style={{ position: "absolute", zIndex: 2, left: 50, top: 0, width: 100, height: 100, backgroundColor: "#0000ff" }}>
                  <text>b</text>
                </div>
              </div>
            }
          />
        ) : null
      }
    />
  )
  const POINTS = [[60, 10], [120, 50], [20, 50]] as const

  const RED: Rgba = [255, 0, 0, 255]
  const BLUE: Rgba = [0, 0, 255, 255]
  const YELLOW: Rgba = [255, 255, 0, 255]

  /** The style of A, whether it holds the yellow box C, and the colour on top at the point before and after the focus. */
  const cases: Array<[string, Record<string, unknown>, boolean, Rgba, Rgba, Record<string, unknown>?]> = [
    ["position and z-index of focus-within", { focusWithin: { position: "relative", zIndex: 5 } }, false, BLUE, RED],
    ["z-index of focus-within on a positioned box", { position: "relative", focusWithin: { zIndex: 5 } }, false, BLUE, RED],
    ["position of focus-within that ends the z-index", { position: "relative", zIndex: 5, focusWithin: { position: "static" } }, false, RED, BLUE],
    ["z-index of focus-visible", { position: "relative", focusVisible: { zIndex: 5 } }, false, BLUE, RED],
    ["opacity of focus-within, which makes the box a context", { position: "relative", focusWithin: { opacity: 0.5 } }, true, YELLOW, BLUE],
    ["z-index of focus-within on a flex item, which applies without a position", { focusWithin: { zIndex: 5 } }, false, BLUE, RED, { display: "flex" }],
  ]
  for (const [name, a, c, atRest, focusedColor, wrapper] of cases) {
    it(`answers elementsFromPoint and paints after the ${name} applies and ends`, () => {
      const { renderer, render, settle, bounds, sample, expectWhatABuildShows, finish, unmount } = setup(overlay(a, c, wrapper))
      try {
        settle()
        const box = bounds("wrapper")
        const points = POINTS.map(([x, y]) => [box.x + x, box.y + y] as [number, number])
        const hit = () => elementsLikeABuild(renderer, points, names(renderer))
        const before = hit()
        expect(sample("focus-before", ...points[0])).toEqual(atRest)
        // A key first, so that the focus a script gives next matches :focus-visible.
        renderer.simulateKeystrokes("Tab")
        renderer.focusElement(idsByTestId(renderer).get("a"))
        expect(renderer.getElement(renderer.getFocusedElementId())?.testId).toBe("a")
        renderer.flush()
        const hitFocused = hit()
        expect(sample("focus-after", ...points[0])).toEqual(focusedColor)
        expectWhatABuildShows("focus-a")
        settle()
        expect(hit()).toEqual(hitFocused)
        expect(hitFocused, "the focus variant changes what is hit").not.toEqual(before)
        renderer.focusElement(idsByTestId(renderer).get("button-0"))
        renderer.flush()
        settle()
        expect(hit()).toEqual(before)
        expect(sample("focus-away", ...points[0])).toEqual(atRest)
        finish()
      } finally {
        unmount()
      }
    })
  }
})

describeNative("display: contents wrappers in a page of replayed cards", { timeout: TIMEOUT }, () => {
  const probe = (width?: number) => (
    <div testId="probe" style={{ width: 10, height: 5, "@container": { "style(--x: 1)": { width: 50 }, "nm style(--y: 1)": { height: 20 } } }} />
  )
  const page = (wrapper: Record<string, unknown>) => (
    <World>
      <Page
        card={(index) =>
          index === 2 ? (
            <div key={index} testId="wrapper" style={{ display: "contents", ...wrapper }}>
              <Card index={index} extra={probe()} />
            </div>
          ) : null
        }
      />
    </World>
  )

  it("evaluates a style() query of a spliced card against the custom property of the wrapper, as it changes", () => {
    const { renderer, render, settle, bounds, expectWhatABuildShows, finish, unmount } = setup(page({ "--x": 0 }))
    try {
      settle()
      expect(bounds("probe").width).toBe(10)
      render(page({ "--x": 1 }))
      renderer.flush()
      expect(bounds("probe").width).toBe(50)
      expectWhatABuildShows("custom-1")
      settle()
      render(page({ "--x": 2 }))
      renderer.flush()
      expect(bounds("probe").width).toBe(10)
      render(page({}))
      renderer.flush()
      expect(bounds("probe").width).toBe(10)
      render(page({ "--x": 1 }))
      settle()
      expect(bounds("probe").width).toBe(50)
      expectWhatABuildShows("custom-again")
      finish()
    } finally {
      unmount()
    }
  })

  it("evaluates a named style() query against the name and the property a wrapper declares", () => {
    const { renderer, render, settle, bounds, expectWhatABuildShows, finish, unmount } = setup(page({ containerName: "other", "--y": 1 }))
    try {
      settle()
      expect(bounds("probe").height).toBe(5)
      render(page({ containerName: "nm", "--y": 1 }))
      renderer.flush()
      expect(bounds("probe").height).toBe(20)
      expectWhatABuildShows("named-1")
      settle()
      render(page({ containerName: "nm", "--y": 0 }))
      renderer.flush()
      expect(bounds("probe").height).toBe(5)
      render(page({ containerName: "nm", "--y": 1 }))
      settle()
      expect(bounds("probe").height).toBe(20)
      expectWhatABuildShows("named-again")
      finish()
    } finally {
      unmount()
    }
  })
})

describeNative("custom properties of a state style, read by a replayed child", { timeout: TIMEOUT }, () => {
  it("restyles what a hovered card hands down through --x, and takes it back", () => {
    const { renderer, settle, center, bounds, expectWhatABuildShows, finish, unmount } = setup(
      <Page
        card={(index) => (
          <Card
            key={index}
            index={index}
            style={{ "--x": 0, hover: { "--x": 1 } }}
            extra={<div testId={`probe-${index}`} style={{ width: 10, height: 5, "@container": { "style(--x: 1)": { width: 50 } } }} />}
          />
        )}
      />
    )
    try {
      settle()
      expect(bounds("probe-3").width).toBe(10)
      renderer.nativeSimulateMouseMove(...center("title-3"))
      renderer.flush()
      expect(bounds("probe-3").width).toBe(50)
      expect(bounds("probe-4").width).toBe(10)
      expectWhatABuildShows("custom-hover", ["probe-3", "probe-4"])
      renderer.nativeSimulateMouseMove(...center("title-4"))
      renderer.flush()
      expect(bounds("probe-3").width).toBe(10)
      expect(bounds("probe-4").width).toBe(50)
      expectWhatABuildShows("custom-hover-next", ["probe-3", "probe-4"])
      renderer.nativeSimulateMouseMove(639, 479)
      renderer.flush()
      settle()
      expect(bounds("probe-4").width).toBe(10)
      finish()
    } finally {
      unmount()
    }
  })
})

describeNative("gestures of motion elements in a page of replayed cards", { timeout: TIMEOUT }, () => {
  const INSTANT = { duration: 0 } as const
  const gestures = () => (
    <motion.div
      testId="controller"
      initial="rest"
      animate="rest"
      whileHover="hover"
      whileTap="press"
      variants={{ rest: {}, hover: {}, press: {} }}
      transition={INSTANT}
      style={{ width: 120, height: 60, backgroundColor: "#333333" }}
    >
      <div style={{ height: 10 }} />
      <motion.div testId="child" style={{ width: 20, height: 10, backgroundColor: "#ff0000" }} variants={{ hover: { width: 70 }, press: { width: 90 } }} transition={INSTANT} />
      <motion.div testId="own" style={{ width: 20, height: 10, backgroundColor: "#00ff00" }} whileHover={{ width: 40 }} transition={INSTANT} />
    </motion.div>
  )

  it("animates a child whose controller is hovered or pressed, and itself when it is hovered, while the other cards replay", () => {
    const { renderer, settle, bounds, expectWhatABuildShows, finish, unmount } = setup(
      <Page card={(index) => (index === 5 ? <Card key={index} index={index} style={{ height: "auto", alignSelf: "flex-start" }} extra={gestures()} /> : null)} />
    )
    try {
      settle()
      const rest = expectWhatABuildShows("gesture-rest", ["child", "own"])
      const box = bounds("controller")
      // Over the controller, not over either child.
      const over: [number, number] = [box.x + box.width - 6, box.y + box.height - 6]
      renderer.nativeSimulateMouseMove(...over)
      renderer.flush()
      const hovered = expectWhatABuildShows("gesture-hover", ["child", "own"])
      expect(hovered.watched).not.toEqual(rest.watched)
      expect(bounds("child").width).toBe(70)
      renderer.nativeSimulateMouseDown(...over)
      renderer.flush()
      expectWhatABuildShows("gesture-press", ["child", "own"])
      expect(bounds("child").width).toBe(90)
      renderer.nativeSimulateMouseUp(...over)
      renderer.flush()
      expect(bounds("child").width).toBe(70)
      const own = bounds("own")
      renderer.nativeSimulateMouseMove(own.x + 4, own.y + 4)
      renderer.flush()
      expectWhatABuildShows("gesture-own", ["child", "own"])
      expect(bounds("own").width).toBe(40)
      renderer.nativeSimulateMouseMove(639, 479)
      renderer.flush()
      settle()
      expect(expectWhatABuildShows("gesture-away", ["child", "own"]).watched).toEqual(rest.watched)
      finish()
    } finally {
      unmount()
    }
  })
})

describeNative("a focusable wrapper that becomes display: contents, in replayed cards", { timeout: TIMEOUT }, () => {
  const page = (display: string) => (
    <Page
      card={(index) =>
        index === 3 ? (
          <Card
            key={index}
            index={index}
            extra={
              <div testId="wrapper" tabIndex={0} style={{ display, width: 40, height: 10, backgroundColor: "#999999", focusVisible: { backgroundColor: "#00ff00" } }}>
                <div testId="inner" tabIndex={0} style={{ width: 20, height: 10, backgroundColor: "#ff00ff", focusVisible: { backgroundColor: "#00ffff" } }} />
              </div>
            }
          />
        ) : null
      }
    />
  )

  it("leaves the tab order and the focus ring of a wrapper with no box, and takes both back", () => {
    const { renderer, render, settle, expectWhatABuildShows, finish, unmount } = setup(page("block"))
    try {
      settle()
      const tabs = () => {
        renderer.simulateKeystrokes("Tab")
        renderer.focusElement(idsByTestId(renderer).get("button-3"))
        const order: string[] = []
        for (let press = 0; press < 2; press++) {
          renderer.simulateKeystrokes("Tab")
          renderer.flush()
          order.push(renderer.getElement(renderer.getFocusedElementId())?.testId)
        }
        return order
      }
      const withBox = tabs()
      expect(withBox).toEqual(["wrapper", "inner"])
      expectWhatABuildShows("contents-block")
      render(page("contents"))
      settle()
      const without = tabs()
      expect(without).toEqual(["inner", "button-4"])
      expectWhatABuildShows("contents-contents")
      render(page("block"))
      settle()
      expect(tabs()).toEqual(["wrapper", "inner"])
      expectWhatABuildShows("contents-block-again")
      finish()
    } finally {
      unmount()
    }
  })
})

describeNative("motion and gestures of a span in a replayed card", { timeout: TIMEOUT }, () => {
  const LINEAR = { duration: 1, ease: "linear" } as const
  const INSTANT = { duration: 0 } as const

  it("animates the colour and the padding of a span in a paragraph, in every frame, as a build does", () => {
    expectTheFramesOfABuild((state) => (
      <div style={{ width: 140 }}>
        <span>{"aaaa "}</span>
        <motion.span
          testId="mover"
          initial={{ backgroundColor: "#ff0000", paddingLeft: 0 }}
          animate={{ backgroundColor: state === "from" ? "#ff0000" : "#0000ff", paddingLeft: state === "from" ? 0 : 24 }}
          transition={LINEAR}
        >
          {"bbbb cccc"}
        </motion.span>
        <span>{" dddd eeee"}</span>
      </div>
    ))
  })

  it("starts the whileHover of a span when the pointer enters it, and ends it when it leaves", () => {
    const { renderer, settle, bounds, expectWhatABuildShows, finish, unmount } = setup(
      <Page
        card={(index) =>
          index === 5 ? (
            <Card
              key={index}
              index={index}
              style={{ height: "auto", alignSelf: "flex-start" }}
              extra={
                <div style={{ width: 140 }}>
                  <span>{"aaaa "}</span>
                  <motion.span testId="hover" whileHover={{ backgroundColor: "#00ff00", paddingLeft: 20 }} transition={INSTANT}>
                    {"bbbb cccc"}
                  </motion.span>
                  <span>{" dddd eeee"}</span>
                </div>
              }
            />
          ) : null
        }
      />
    )
    try {
      settle()
      const rest = expectWhatABuildShows("span-rest", ["card-5"])
      const box = bounds("hover")
      renderer.nativeSimulateMouseMove(box.x + box.width - 4, box.y + box.height / 2)
      renderer.flush()
      const hovered = expectWhatABuildShows("span-hover", ["card-5"])
      expect(hovered.picture === rest.picture).toBe(false)
      renderer.nativeSimulateMouseMove(639, 479)
      renderer.flush()
      settle()
      expect(expectWhatABuildShows("span-away", ["card-5"]).picture === rest.picture).toBe(true)
      finish()
    } finally {
      unmount()
    }
  })
})
