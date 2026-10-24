/// A CSS animation of `transform`, `translate`, `rotate`, `scale` and `transform-origin`, against headless Chrome:
/// `fixtures/transform-animation-oracle.html` is the page and `fixtures/transform-animation-oracle.json` the border box
/// Chrome shows part of the way through each animation (regenerate with `transform-animation-oracle.sh`).
///
/// - Lists that pair up function by function blend by function, translate lengths in px and in percent apart; lists that
///   stop pairing blend the rest through the 2D matrix decomposition; `none` blends as the identity of the other list.
/// - The individual `translate`, `rotate` and `scale` blend as their own values, and `transform-origin` as two lengths.
/// - The element is a stacking context and a containing block for as long as the animation runs.

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { defineKeyframes } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rect = [number, number, number, number]
interface Case {
  property: string
  frames: [number, string][]
  extra: string
  boxes: Record<string, Rect>
}

const ORACLE: Record<string, Case> = JSON.parse(
  fs.readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/transform-animation-oracle.json"), "utf8")
)

const TOLERANCE = 0.06
const TIMEOUT = 120_000

/**
 * The cases whose box is further than the tolerance from Chrome's, with the largest difference gpuix has: the matrix
 * decomposition is done in 32-bit floats here and Chrome's is in 64-bit ones, and the difference shows where a rotation is
 * recomposed from a skew of nearly a quarter turn.
 */
const DEVIATIONS: Record<string, number> = {}

const camel = (name: string) => name.replace(/-([a-z])/g, (_, letter: string) => letter.toUpperCase())

/** `a: b; c: d` as a style. */
function styleOf(css: string): Record<string, string> {
  const style: Record<string, string> = {}
  for (const declaration of css.split(";")) {
    const colon = declaration.indexOf(":")
    if (colon >= 0) style[camel(declaration.slice(0, colon).trim())] = declaration.slice(colon + 1).trim()
  }
  return style
}

function keyframesOf(scene: Case) {
  const frames: Record<string, Record<string, string>> = {}
  for (const [offset, value] of scene.frames) {
    frames[`${offset * 100}%`] = scene.property === "all" ? styleOf(value) : { [camel(scene.property)]: value }
  }
  return frames
}

describeNative("a CSS animation of the transform properties follows Chrome", () => {
  Object.entries(ORACLE).forEach(([name, scene], index) => {
    it(`shows Chrome's box at every time: ${name}`, () => {
      const keyframes = `tanim-${index}`
      defineKeyframes(keyframes, keyframesOf(scene))
      const { render, renderer, unmount } = createTestRoot({ width: 800, height: 600 })
      try {
        renderer.clockPause()
        render(
          <div style={{ position: "absolute", left: 0, top: 0, width: 800, height: 600 }}>
            <div
              testId="box"
              style={{
                position: "absolute",
                left: 200,
                top: 200,
                width: 100,
                height: 50,
                ...styleOf(scene.extra),
                animation: `${keyframes} 1s linear forwards`,
              }}
            />
          </div>
        )
        renderer.flush()
        const wrong: string[] = []
        let elapsed = 0
        for (const [time, want] of Object.entries(scene.boxes).sort(([a], [b]) => +a - +b)) {
          renderer.clockFastForward(Math.round((+time - elapsed) * 1000))
          elapsed = +time
          renderer.flush()
          renderer.dispatchNativeEvents()
          const box = renderer.getElementBounds(renderer.findByTestId("box").id)
          expect(box, "the box has bounds").not.toBeNull()
          const got = [box!.x, box!.y, box!.width, box!.height]
          const tolerance = Math.max(TOLERANCE, DEVIATIONS[name] ?? 0)
          if (got.some((value, i) => Math.abs(value - want[i]!) > tolerance)) {
            wrong.push(`t=${time}: got [${got.map((value) => +value.toFixed(3))}] Chrome [${want.map((value) => +value.toFixed(3))}]`)
          }
        }
        expect(wrong).toEqual([])
      } finally {
        unmount()
      }
    }, TIMEOUT)
  })
})
