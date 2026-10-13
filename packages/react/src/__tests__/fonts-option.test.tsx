/// Fonts registered through the `fonts` option resolve by family name.

import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const LILEX_REGULAR = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../../../../zed/assets/fonts/lilex/Lilex-Regular.ttf"
)

/** Lilex is monospace: these two strings only measure the same in Lilex. */
const NARROW = "iiiiiiiiiiiiiiiiiiii"
const WIDE = "WWWWWWWWWWWWWWWWWWWW"

function measure(fonts?: string[]): { narrow: number; wide: number } {
  const { render, renderer, unmount } = createTestRoot({ fonts })
  render(
    <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start" }}>
      {[
        ["narrow", NARROW],
        ["wide", WIDE],
      ].map(([testId, content]) => (
        <text
          key={testId}
          testId={testId}
          style={{ fontSize: 16, fontFamily: "Lilex", color: "#fff" }}
        >
          {content}
        </text>
      ))}
    </div>
  )
  const widthOf = (testId: string): number => {
    const element = renderer.findByTestId(testId)
    if (!element) throw new Error(`missing testId ${testId}`)
    const bounds = renderer.getElementBounds(element.id)
    if (!bounds) throw new Error(`no painted bounds for ${testId}`)
    return bounds.width
  }
  const widths = { narrow: widthOf("narrow"), wide: widthOf("wide") }
  unmount()
  return widths
}

describeNative("fonts option", () => {
  it("lays text out with a font loaded from a file", () => {
    const withoutFont = measure()
    const withFont = measure([LILEX_REGULAR])

    expect(withoutFont.narrow).not.toBeCloseTo(withoutFont.wide, 0)
    expect(withFont.narrow).toBeCloseTo(withFont.wide, 0)
    expect(withFont.wide).not.toBeCloseTo(withoutFont.wide, 0)
  })

  it("throws naming the path when a font file does not exist", () => {
    const missing = path.join(path.dirname(LILEX_REGULAR), "Missing-Font.ttf")
    expect(() => createTestRoot({ fonts: [missing] })).toThrow(missing)
  })
})
