/// `border-spacing`, `z-index`, `vertical-align` and the discrete properties of tables, positioning and
/// pointer input in the middle of a CSS transition, against headless Chrome: `fixtures/animatable-oracle.html` is the
/// page and `fixtures/animatable-oracle.json` what Chrome shows when each transition is paused at the fractions of
/// its second. `value` is the computed value, `subject` and `probe` the boxes the case reads, and `chains` the
/// elements at the case's points. A discrete property flips at the halfway point with `allow-discrete` and does not
/// transition without it; `border-spacing` is cut to whole pixels; `z-index` is an integer, half rounding up.

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
  before: string
  after: string
  transition: string
  points: [number, number][]
  running: boolean
  samples: Record<string, Sample>
}

const ORACLE: { times: number[]; cases: Record<string, Case> } = JSON.parse(
  fs.readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/animatable-oracle.json"), "utf8")
)

const camel = (name: string) => name.replace(/-([a-z])/g, (_, letter: string) => letter.toUpperCase())

/** The value as a style prop: `z-index` is a number and `auto` is leaving it out. */
function prop(property: string, value: string): unknown {
  if (property === "z-index") return value === "auto" ? undefined : Number(value)
  return value
}

/** `z-index` stands for the stripes it is read off: this one is on top at x where `z-index` reaches it. */
const STRIPES = Array.from({ length: 19 }, (_, ix) => ix - 6)

function scene(source: Case, value: string | undefined, transition: boolean) {
  const style = { [camel(source.property)]: prop(source.property, value ?? source.before), ...(transition ? { transition: source.transition } : {}) }
  return (
    <div testId="case" style={{ width: 300, fontFamily: "Menlo", fontSize: 16, lineHeight: "20px" }}>
      {elementsOf(source.html, { id: source.subject, style })}
    </div>
  )
}

function zScene(source: Case, value: string | undefined) {
  const z = prop("z-index", value ?? source.before) as number | undefined
  return (
    <div testId="case" style={{ position: "relative", zIndex: 0, width: 300, height: 40 }}>
      {STRIPES.map((k) => (
        <div
          key={k}
          testId={`b${k}`}
          style={{ position: "absolute", left: (k + 6) * 12, top: 0, width: 10, height: 10, zIndex: k }}
        />
      ))}
      <div
        testId="s"
        style={{ position: "absolute", left: 0, top: 0, width: 300, height: 40, zIndex: z, transition: source.transition }}
      />
    </div>
  )
}

describeNative("animatable properties in the middle of a transition, against Chrome", () => {
  const near = (got: Rect, want: Rect, context: string, tolerance = 0.51) => {
    want.forEach((value, index) => {
      expect(Math.abs(got[index]! - value), `${context}: got [${got.map((n) => +n.toFixed(2))}] want [${want}]`).toBeLessThanOrEqual(
        tolerance
      )
    })
  }

  for (const [name, source] of Object.entries(ORACLE.cases)) {
    if (source.property === "empty-cells") continue
    it(name, () => {
      const { render, renderer, unmount } = createTestRoot()
      try {
        const isZ = source.property === "z-index"
        const draw = (value: string | undefined) => render(isZ ? zScene(source, value) : scene(source, value, true))
        const boundsOf = (id: string): Rect => {
          const bounds = renderer.getElementBounds(renderer.findByTestId(id).id)
          return [bounds.x, bounds.y, bounds.width, bounds.height]
        }
        const names = () => {
          const found = new Map<number, string>()
          for (const id of ["case", source.subject, "k"]) {
            const element = renderer.findByTestId(id)
            if (element) found.set(element.id, id)
          }
          return found
        }
        const stack = () => {
          const subject = renderer.findByTestId("s").id
          let z = -7
          for (const k of STRIPES) {
            const stripe = renderer.findByTestId(`b${k}`).id
            const top = renderer.getElementsAt((k + 6) * 12 + 5, 5).find((id) => id === subject || id === stripe)
            if (top === subject) z = k
          }
          return z
        }
        const sample = (at: string, want: Sample) => {
          renderer.dispatchNativeEvents()
          if (isZ) {
            // `auto` is where `z-index: 0` ends up
            expect(stack(), `${name} at ${at}`).toBe(want.value === "auto" ? 0 : Number(want.value))
            return
          }
          near(boundsOf(source.subject), want.subject, `${name} subject at ${at}`)
          near(boundsOf(source.probe), want.probe, `${name} probe at ${at}`)
          source.points.forEach(([x, y], index) => {
            const known = names()
            const chain = renderer.getElementsAt(x, y).flatMap((id) => known.get(id) ?? [])
            expect(chain, `${name} at ${at}: hit at ${x},${y}`).toEqual(want.chains[index])
          })
        }

        renderer.clockPause()
        draw(undefined)
        renderer.dispatchNativeEvents()
        draw(source.after)
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

describeNative("empty-cells in the middle of a transition", () => {
  const source = ORACLE.cases["empty-cells"]!

  it("hides the border of an empty cell from the halfway point, as Chrome flips it", () => {
    const { render, renderer, unmount } = createTestRoot()
    const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), "gpuix-empty-cells-")), "shot.png")
    try {
      const table = (emptyCells: "show" | "hide") => (
        <div style={{ width: 300, backgroundColor: "#ffffff" }}>
          <div style={{ display: "table", emptyCells, transition: source.transition }}>
            <div style={{ display: "table-row" }}>
              <div style={{ display: "table-cell", width: 12, height: 12, border: "2px solid #000000" }} />
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
      render(table("show"))
      expect(ink()).toBe(true)
      render(table("hide"))
      renderer.clockFastForward(250)
      expect(ink(), "a quarter in").toBe(true)
      renderer.clockFastForward(250)
      expect(ink(), "halfway").toBe(false)
      renderer.clockFastForward(500)
      expect(ink(), "at the end").toBe(false)
      expect(Object.keys(source.samples)).toContain("0.5")
      expect(source.samples["0.25"]!.value).toBe("show")
      expect(source.samples["0.5"]!.value).toBe("hide")
    } finally {
      unmount()
    }
  })
})
