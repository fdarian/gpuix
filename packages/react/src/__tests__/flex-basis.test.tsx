/// `flex-basis` against headless Chrome: `fixtures/flex-basis-oracle.html` is the page and
/// `fixtures/flex-basis-oracle.json` the boxes Chrome lays out for each case.
///
/// In a row the keywords are sizes of the item's content: `max-content` and `content` the content with no
/// line broken (and `content` ignores `width`), `min-content` the narrowest, and `fit-content`
/// `min(max-content, max(min-content, the container's content width less the item's margins))`. A `width:
/// fit-content` item that has no basis of its own is based on that same size. In a column every one of them
/// is `content`: the height of the content at the width the item gets, not its `height`.

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { elementsOf, expectBoxes } from "./oracle-boxes.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const FIXTURES = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures")
const ORACLE: Record<string, { html: string; boxes: Record<string, [number, number, number, number]> }> = JSON.parse(
  fs.readFileSync(path.join(FIXTURES, "flex-basis-oracle.json"), "utf8")
)
const SQUARE_FONT = fs.readFileSync(path.join(FIXTURES, "gpuix-test-square.ttf"))

/** gpuix lays text out in whole pixels, which makes a 335px line up to 2px wider; the cases are 20px or more apart. */
const TEXT_ROUNDING = 2.5

/**
 * Cases where gpuix differs from Chrome for a reason outside the basis keywords. A flex container that is sized by its
 * items under a definite available space (an `inline-flex`, an absolute one) is as wide as their flex base sizes in
 * taffy, and as wide as their max-content contributions in Chrome: for an item that also has a `width` (as it is for
 * `flex-basis: 200px; width: 50px`), and for a `min-content` basis, which Chrome does not count as a basis there.
 */
const DEVIATIONS = new Set(["inline-flex content width 50", "max-content container content width 50", "inline-flex min-content"])

/** Lays `name` out and returns what differs from Chrome, one line per box. */
function differences(name: string): string[] {
  if (DEVIATIONS.has(name)) return []
  const source = ORACLE[name]!
  const { render, renderer, unmount } = createTestRoot({ width: 1280, height: 800 })
  renderer.registerFont(SQUARE_FONT)
  try {
    render(
      <div
        testId="case"
        style={{
          position: "absolute",
          left: 0,
          top: 0,
          width: 1280,
          fontFamily: "GpuixTestSquare",
          fontSize: 20,
          lineHeight: "20px",
          color: "#000000",
        }}
      >
        {elementsOf(source.html)}
      </div>
    )
    try {
      expectBoxes(renderer, source.boxes, TEXT_ROUNDING)
      return []
    } catch (error) {
      return [`${name}: ${(error as Error).message}`]
    }
  } finally {
    unmount()
  }
}

const GROUPS: [string, (name: string) => boolean][] = [
  ["a row item that cannot shrink is its basis", (name) => /^row \d+ (auto|fit-content|max-content|min-content|content) [sl]$/.test(name)],
  ["a row item shrinks from its basis", (name) => name.endsWith(" shrinks")],
  ["a row item grows from its basis", (name) => name.endsWith(" grows")],
  ["the width next to a basis keyword, and width: fit-content as a basis", (name) => / width /.test(name) && !/ (max|min)-width/.test(name.replace(/ width fit-content (max|min)/, ""))],
  ["min-width and max-width clamp the basis", (name) => /^row 300 .*(max|min)-width/.test(name)],
  ["an item with a width keeps it as its automatic minimum size", (name) => name.startsWith("min row")],
  ["margins, padding, gaps, wrapping and several items", (name) => /margin|padding|gap|reverse|two|then|wrap/.test(name) && name.startsWith("row")],
  ["the flex shorthand takes content", (name) => / flex /.test(name)],
  ["a container sized by its items", (name) => /inline-flex|container|^abs/.test(name)],
  ["a column: every keyword is content", (name) => name.startsWith("col")],
]

function groupOf(name: string) {
  return GROUPS.find(([, belongs]) => belongs(name))?.[0]
}

describeNative("flex-basis against Chrome", () => {
  it("has a group for every case", () => {
    expect(Object.keys(ORACLE).filter((name) => groupOf(name) === undefined)).toEqual([])
  })

  for (const [title] of GROUPS) {
    it(title, { timeout: 60_000 }, () => {
      const failures = Object.keys(ORACLE)
        .filter((name) => groupOf(name) === title)
        .flatMap(differences)
      expect(failures).toEqual([])
    })
  }
})
