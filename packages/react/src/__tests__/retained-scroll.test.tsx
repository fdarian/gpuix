// @ts-nocheck
/**
 * Rows of a scroller that a frame replays a whole number of device pixels away from where it
 * recorded them (#37, translated scroll replay). Each case scrolls the same page twice, once with
 * retention on and once with it off, through the same steps, and expects what a reader of the page
 * can ask for (the boxes of the rows, what is under a point, what a click, a hover, the focus or a
 * drag across the text does, what the observers report, the accessibility tree) to be the same. A
 * case also has to relocate rows, or it would pass by retention not doing what it is checking.
 *
 * Run with GPUIX_RETAIN_VERIFY=1 too: gpui then compares every replayed row with a build.
 */

import React from "react"
import { describe, expect, it } from "vitest"
import { IntersectionObserver } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { elementsAtPoints, idsByTestId } from "./retained-harness.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip
const verifying = process.env.GPUIX_RETAIN_VERIFY !== undefined

const ROWS = 40
const ROW_HEIGHT = 80
const SCROLLER = { width: 400, height: 300 }
const WINDOW = { width: 480, height: 420 }
const TIMEOUT = 60_000

type Mode = "wheel" | "wheel-fractional" | "scroll-to" | "scroll-to-fractional"

/** Steps small enough that the rows recorded before stay within their overscan, steps past it, and the way back. */
const STEPS: Record<Mode, number[]> = {
  wheel: [3, 9, 20, 60, 10, 140, 25, -30, -200, 7],
  "wheel-fractional": [2.5, 7.25, 13.75, 42.5, 0.75, 120.5, 19.25, -33.5, -150.25, 5.5],
  "scroll-to": [3, 12, 32, 92, 102, 242, 267, 237, 37, 44],
  "scroll-to-fractional": [2.5, 9.75, 23.5, 66.25, 67, 187.5, 207.25, 173.75, 24.5, 30.25],
}

function Row({ index, clicks, observed }: { index: number; clicks: number[]; observed?: React.Ref<unknown> }) {
  return (
    <div
      testId={`row-${index}`}
      ref={observed}
      style={{
        height: ROW_HEIGHT,
        padding: 8,
        display: "flex",
        flexDirection: "row",
        gap: 8,
        backgroundColor: index % 2 === 0 ? "#223344" : "#334455",
        hover: { backgroundColor: "#664422" },
      }}
    >
      <div testId={`avatar-${index}`} style={{ width: 40, height: 40, borderRadius: 20, backgroundColor: "#885566" }} />
      <div style={{ display: "flex", flexDirection: "column", width: 160 }}>
        <text testId={`title-${index}`} style={{ fontSize: 14, color: "#ffffff" }}>{`Row ${index} title`}</text>
        <text testId={`body-${index}`} style={{ fontSize: 12, color: "#aaaaaa" }}>{`Description of row ${index}`}</text>
      </div>
      <div
        testId={`button-${index}`}
        tabIndex={0}
        role="button"
        aria-label={`Button ${index}`}
        onClick={() => clicks.push(index)}
        style={{
          width: 60,
          height: 30,
          backgroundColor: "#555555",
          hover: { backgroundColor: "#bb5555" },
          focusVisible: { backgroundColor: "#00cc00" },
        }}
      />
    </div>
  )
}

/** Reports what the observer says of the row, which scrolls in and out of the clip of the scroller. */
function Watched({ index, clicks, reports }: { index: number; clicks: number[]; reports: unknown[] }) {
  const ref = React.useRef(null)
  React.useEffect(() => {
    const observer = new IntersectionObserver((entries) => {
      const entry = entries.at(-1)
      reports.push([
        index,
        entry.isIntersecting,
        Math.round(entry.intersectionRatio * 1000) / 1000,
        entry.boundingClientRect.y,
        entry.intersectionRect.y,
        entry.intersectionRect.height,
      ])
    })
    observer.observe(ref.current)
    return () => observer.disconnect()
  }, [])
  return <Row index={index} clicks={clicks} observed={ref} />
}

function Page({ clicks, reports }: { clicks: number[]; reports: unknown[] }) {
  return (
    <div style={{ width: WINDOW.width, height: WINDOW.height, padding: 10 }}>
      <div testId="scroller" style={{ ...SCROLLER, overflowY: "auto" }}>
        {Array.from({ length: ROWS }, (_, index) =>
          index % 7 === 3 ? (
            <Watched key={index} index={index} clicks={clicks} reports={reports} />
          ) : (
            <Row key={index} index={index} clicks={clicks} />
          )
        )}
      </div>
    </div>
  )
}

/** What a reader of the page can ask of it, as text. */
function observe(renderer, ids: Map<string, number>, names: Map<number, string>) {
  const box = (testId: string) => {
    const found = ids.get(testId)
    const bounds = found === undefined ? null : renderer.getElementBounds(found)
    return bounds && [bounds.x, bounds.y, bounds.width, bounds.height]
  }
  const boxes = {}
  for (let row = 0; row < ROWS; row += 1) {
    for (const part of ["row", "title", "button"]) boxes[`${part}-${row}`] = box(`${part}-${row}`)
  }
  const points: Array<[number, number]> = []
  for (let y = 10; y < 10 + SCROLLER.height; y += 23) {
    for (const x of [14, 70, 130, 240, 300, 395]) points.push([x, y])
  }
  const hits = elementsAtPoints(renderer, points).map((ids) => ids.map((id) => names.get(id) ?? id))
  // The nodes have the bounds of every element; the frame is when and which one it was.
  const tree = renderer.getA11yTree()
  return { boxes, hits, a11y: JSON.stringify({ ...tree, frame: undefined }) }
}

function scrollBy(renderer, scroller, mode: Mode, top: number, step: number): number {
  const x = 10 + SCROLLER.width / 2
  const y = 10 + SCROLLER.height / 2
  switch (mode) {
    case "wheel":
    case "wheel-fractional":
      renderer.nativeSimulateScrollWheel(x, y, 0, -step)
      return top + step
    case "scroll-to":
    case "scroll-to-fractional":
      renderer.scrollTo(scroller.id, 0, step, "instant")
      return step
  }
}

/** The page scrolled through `mode`, with `retention` on or off, and what it showed on the way. */
function run(mode: Mode, retention: boolean) {
  const clicks: number[] = []
  const reports: unknown[] = []
  const root = createTestRoot(WINDOW)
  const { renderer } = root
  try {
    if (!retention) renderer.setRetainEnabled(false)
    root.render(<Page clicks={clicks} reports={reports} />)
    for (let frame = 0; frame < 5; frame++) renderer.flush()
    renderer.dispatchNativeEvents()
    const ids = idsByTestId(renderer)
    const names = new Map<number, string>([...ids].map(([testId, id]) => [id, testId]))
    const scroller = renderer.findByTestId("scroller")
    const steps = []
    let top = 0
    const frame = (label: string) => {
      renderer.flush()
      renderer.dispatchNativeEvents()
      steps.push({ label, ...observe(renderer, ids, names) })
    }
    frame("settled")
    // Two frames of scrolling before the page is looked at: with GPUIX_RETAIN_VERIFY the frame that
    // records rows to be moved is not compared, and the one after it moves them.
    const nudge = mode.endsWith("fractional") ? 4.25 : 4
    for (const step of STEPS[mode]) {
      top = scrollBy(renderer, scroller, mode, top, step)
      renderer.flush()
      top = scrollBy(renderer, scroller, mode, top, mode.startsWith("wheel") ? nudge : top + nudge)
      frame(`scrolled by ${step}`)
    }

    // The pointer over a row that was moved: the row under it is the hovered one, and a click lands on its button.
    const interactions = []
    const button = (row: number) => {
      const box = renderer.getElementBounds(ids.get(`button-${row}`))
      return [box.x + box.width / 2, box.y + box.height / 2]
    }
    const visibleRows = () =>
      Array.from({ length: ROWS }, (_, row) => row).filter((row) => {
        const box = renderer.getElementBounds(ids.get(`row-${row}`))
        return box && box.y >= 10 && box.y + box.height <= 10 + SCROLLER.height
      })
    const shown = visibleRows()
    interactions.push(["visible rows", shown])
    for (const row of shown) {
      const [x, y] = button(row)
      renderer.dispatchMouseMove(x, y)
      renderer.flush()
      interactions.push(["hovered", row, renderer.getElementsAt(x, y).map((id) => names.get(id) ?? id)])
      renderer.nativeSimulateClick(x, y)
    }
    interactions.push(["clicks", [...clicks]])

    // The pointer stays where it is while the rows move under it.
    const [px, py] = button(shown[0])
    renderer.dispatchMouseMove(px, py)
    renderer.flush()
    top = scrollBy(renderer, scroller, mode, top, mode.startsWith("wheel") ? 17.5 : top + 31.5)
    renderer.flush()
    interactions.push(["under the pointer after a scroll", renderer.getElementsAt(px, py).map((id) => names.get(id) ?? id)])

    // The focus walks into the rows that were moved, and the box of the one it lands on is where it is drawn.
    renderer.focusNext()
    const focused = []
    for (let tab = 0; tab < 6; tab += 1) {
      renderer.focusNext()
      renderer.flush()
      const id = renderer.getFocusedElementId()
      const name = names.get(id) ?? id
      const box = id === null ? null : renderer.getElementBounds(id)
      focused.push([name, box && [box.x, box.y, box.width, box.height]])
    }
    interactions.push(["focus", focused])

    // A drag from the title of the first row to the title of the last one that shows selects the text between, wherever the rows were drawn.
    const rowsNow = visibleRows()
    expect(rowsNow.length).toBeGreaterThanOrEqual(2)
    const [from, to] = [rowsNow[0], rowsNow.at(-1)]
    const a = renderer.getElementBounds(ids.get(`title-${from}`))
    const b = renderer.getElementBounds(ids.get(`title-${to}`))
    interactions.push(["selected", renderer.dragSelect(a.x + 2, a.y + 5, b.x + 60, b.y + 5)])
    renderer.flush()
    interactions.push(["selection kept", renderer.getSelectedText()])
    renderer.clearSelection()

    renderer.dispatchNativeEvents()
    // A frame that only replays is built a second time under GPUIX_RETAIN_VERIFY and shown as that build, whose
    // recordings stay where they were made, so every idle frame above leaves rows that the next scroll builds
    // again. These scrolls are drawn one after the other with the verifier off for the frame that records the
    // rows to be moved, and on for the frame that moves them, which it compares with a build.
    renderer.setInlineDraws(false)
    for (let move = 0; move < 4; move += 1) {
      renderer.setRetainVerify(false)
      renderer.scrollTo(scroller.id, 0, 1000 + 14 * move, "instant")
      renderer.drawFrame()
      renderer.setRetainVerify(verifying)
      renderer.scrollTo(scroller.id, 0, 1007 + 14 * move, "instant")
      renderer.drawFrame()
    }
    renderer.setInlineDraws(true)
    frame("moved by a frame that is compared with a build")
    const counters = renderer.getRetainedCounters()
    return { steps, interactions, reports: [...reports], counters, mismatches: renderer.getRetentionMismatches() }
  } finally {
    root.unmount()
  }
}

describeNative.each(["wheel", "wheel-fractional", "scroll-to", "scroll-to-fractional"] as Mode[])(
  "rows of a scroller that a frame moves instead of building, %s",
  (mode) => {
    it(
      "shows and answers what the page built whole shows and answers",
      () => {
        const off = run(mode, false)
        const on = run(mode, true)
        expect(on.counters.mismatches, on.mismatches.join("\n")).toBe(0)
        // The page has to have been moved, not built: otherwise nothing here is a test. (Under verify only the
        // scrolls at the end of `run` are certain to move rows.)
        expect(on.counters.relocations).toBeGreaterThan(0)
        expect(off.counters.relocations).toBe(0)

        expect(on.steps.length).toBe(off.steps.length)
        for (let step = 0; step < off.steps.length; step += 1) {
          const [a, b] = [on.steps[step], off.steps[step]]
          // The boxes of the rows (what getBoundingClientRect reads), culled ones too.
          expect(a.boxes, `${a.label}: bounds`).toEqual(b.boxes)
          // What is under a grid of points, which the hitboxes of the moved rows answer.
          expect(a.hits, `${a.label}: hit test`).toEqual(b.hits)
          // The accessibility tree, with the bounds of every node.
          expect(a.a11y, `${a.label}: accessibility tree`).toEqual(b.a11y)
        }
        // Hover, click, the focus and a drag over the text.
        expect(on.interactions).toEqual(off.interactions)
        // What IntersectionObserver told about rows that came in and out of the clip.
        expect(on.reports.length).toBeGreaterThan(0)
        expect(on.reports).toEqual(off.reports)
      },
      TIMEOUT
    )
  }
)
