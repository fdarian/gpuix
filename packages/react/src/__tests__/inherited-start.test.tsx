/// A CSS transition of an inherited property that the element leaves unset starts from the value it inherits, as it
/// does in Chrome, and not from the property's initial value: `fixtures/inherited-start-oracle.html` is the page and
/// `fixtures/inherited-start-oracle.json` what Chrome shows when each transition is paused at the fractions of its
/// second. A case is an element whose parent (or an ancestor above an element that declares nothing) sets the
/// property, and the transition is the element's change to another value, or to the one it inherits (which is no
/// change, so nothing runs).

import fs from "fs"
import os from "os"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { elementsOf } from "./oracle-boxes.js"
import { readLogicalPixels } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rect = [number, number, number, number]
interface Sample {
  value: string
  subject: Rect
  probe: Rect
  chains: string[][]
}
interface Case {
  html: string
  subject: string
  probe: string
  property: string
  after: string
  transition: string
  points: [number, number][]
  running: boolean
  samples: Record<string, Sample>
}

const ORACLE: { times: number[]; cases: Record<string, Case>; animated: Record<string, Omit<Case, "transition" | "running">> } = JSON.parse(
  fs.readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/inherited-start-oracle.json"), "utf8")
)

const camel = (name: string) => name.replace(/-([a-z])/g, (_, letter: string) => letter.toUpperCase())

/** Cases the boxes cannot show: `empty-cells` is read off the paint, in a test of its own below. */
const PAINTED = new Set(["empty-cells"])

function scene(source: Case, changed: boolean) {
  const style = { ...(changed ? { [camel(source.property)]: source.after } : {}), transition: source.transition }
  return (
    <div testId="case" style={{ width: 300, fontFamily: "Menlo", fontSize: 16, lineHeight: "20px" }}>
      {elementsOf(source.html, { id: source.subject, style })}
    </div>
  )
}

describeNative("a transition of an unset inherited property starts from the inherited value", () => {
  const near = (got: Rect, want: Rect, context: string, tolerance = 0.51) => {
    want.forEach((value, index) => {
      expect(Math.abs(got[index]! - value), `${context}: got [${got.map((n) => +n.toFixed(2))}] want [${want}]`).toBeLessThanOrEqual(tolerance)
    })
  }

  for (const [name, source] of Object.entries(ORACLE.cases)) {
    if (PAINTED.has(name)) continue
    it(name, () => {
      const { render, renderer, unmount } = createTestRoot()
      try {
        const boundsOf = (id: string): Rect => {
          const bounds = renderer.getElementBounds(renderer.findByTestId(id).id)
          return [bounds.x, bounds.y, bounds.width, bounds.height]
        }
        const names = () => {
          const found = new Map<number, string>()
          for (const id of ["case", source.subject, "m", "w"]) {
            const element = renderer.findByTestId(id)
            if (element) found.set(element.id, id)
          }
          return found
        }
        const sample = (at: string, want: Sample) => {
          renderer.dispatchNativeEvents()
          near(boundsOf(source.subject), want.subject, `${name} subject at ${at}`)
          near(boundsOf(source.probe), want.probe, `${name} probe at ${at}`)
          source.points.forEach(([x, y], index) => {
            const known = names()
            const chain = renderer.getElementsAt(x, y).flatMap((id) => known.get(id) ?? [])
            expect(chain, `${name} at ${at}: hit at ${x},${y}`).toEqual(want.chains[index])
          })
        }

        renderer.clockPause()
        render(scene(source, false))
        renderer.dispatchNativeEvents()
        render(scene(source, true))
        if (!source.running) {
          sample("the change", source.samples.immediate!)
          renderer.clockFastForward(500)
          sample("the change a while later", source.samples.immediate!)
          return
        }
        let elapsed = 0
        for (const time of [...ORACLE.times].sort((a, b) => a - b)) {
          renderer.clockFastForward(Math.round(time * 1000) - elapsed)
          elapsed = Math.round(time * 1000)
          sample(String(time), source.samples[String(time)]!)
        }
      } finally {
        unmount()
      }
    })
  }
})

describeNative("empty-cells inherited into a transition", () => {
  const source = ORACLE.cases["empty-cells"]!

  it("hides the border of an empty cell until the halfway point, then shows it, as Chrome flips it", () => {
    const { render, renderer, unmount } = createTestRoot()
    const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), "gpuix-empty-cells-")), "shot.png")
    try {
      const table = (emptyCells?: "show") => (
        <div style={{ width: 300, backgroundColor: "#ffffff" }}>
          <div style={{ emptyCells: "hide" }}>
            <div style={{ display: "table", ...(emptyCells ? { emptyCells } : {}), transition: source.transition }}>
              <div style={{ display: "table-row" }}>
                <div style={{ display: "table-cell", width: 12, height: 12, border: "2px solid #000000" }} />
              </div>
            </div>
          </div>
        </div>
      )
      const ink = () => {
        renderer.dispatchNativeEvents()
        renderer.captureScreenshot(file)
        const [r, g, b] = readLogicalPixels(file, renderer).at(1, 6)
        return r! + g! + b! < 100
      }
      renderer.clockPause()
      render(table())
      expect(ink(), "inherited hide").toBe(false)
      render(table("show"))
      expect(ink(), "the change").toBe(false)
      renderer.clockFastForward(250)
      expect(ink(), "a quarter in").toBe(false)
      renderer.clockFastForward(250)
      expect(ink(), "halfway").toBe(true)
      renderer.clockFastForward(500)
      expect(ink(), "at the end").toBe(true)
      expect(source.samples["0.25"]!.value).toBe("hide")
      expect(source.samples["0.5"]!.value).toBe("show")
    } finally {
      unmount()
    }
  })
})

describeNative("a motion with no first keyframe starts from the inherited value", () => {
  const near = (got: Rect, want: Rect, context: string, tolerance = 0.51) => {
    want.forEach((value, index) => {
      expect(Math.abs(got[index]! - value), `${context}: got [${got.map((n) => +n.toFixed(2))}] want [${want}]`).toBeLessThanOrEqual(tolerance)
    })
  }

  for (const [name, source] of Object.entries(ORACLE.animated)) {
    it(name, () => {
      const { render, renderer, unmount } = createTestRoot()
      try {
        const value = source.property === "line-height" ? Number(source.after) : source.after
        const goal = { [camel(source.property)]: value }
        const tree = (
          <div testId="case" style={{ width: 300, fontFamily: "Menlo", fontSize: 16, lineHeight: "20px" }}>
            {elementsOf(source.html, {
              id: source.subject,
              style: {},
              // An `initial` that is not the animated property: without one the mount rests at the goal.
              motion: { initial: { opacity: 1 }, animate: goal, transition: { duration: 1, ease: "linear" } },
            })}
          </div>
        )
        const boundsOf = (id: string): Rect => {
          const bounds = renderer.getElementBounds(renderer.findByTestId(id).id)
          return [bounds.x, bounds.y, bounds.width, bounds.height]
        }
        const names = () => {
          const found = new Map<number, string>()
          for (const id of ["case", source.subject, "m", "w"]) {
            const element = renderer.findByTestId(id)
            if (element) found.set(element.id, id)
          }
          return found
        }
        renderer.clockPause()
        render(tree)
        renderer.flush()
        let elapsed = 0
        for (const time of [...ORACLE.times].sort((a, b) => a - b)) {
          renderer.clockFastForward(Math.round(time * 1000) - elapsed)
          elapsed = Math.round(time * 1000)
          renderer.flush()
          renderer.dispatchNativeEvents()
          const want = source.samples[String(time)]!
          near(boundsOf(source.subject), want.subject, `${name} subject at ${time}`)
          near(boundsOf(source.probe), want.probe, `${name} probe at ${time}`)
          source.points.forEach(([x, y], index) => {
            const known = names()
            const chain = renderer.getElementsAt(x, y).flatMap((id) => known.get(id) ?? [])
            expect(chain, `${name} at ${time}: hit at ${x},${y}`).toEqual(want.chains[index])
          })
        }
      } finally {
        unmount()
      }
    })
  }
})
