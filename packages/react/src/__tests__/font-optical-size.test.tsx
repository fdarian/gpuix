/// An explicit `opsz` in `fontVariationSettings` against Chrome. `"opsz" 16` was laid out and drawn as the automatic
/// optical size: CoreText makes the face at 16 pt and sets `opsz` 16 on it, so gpuix took 16 for what the face had and
/// set nothing, and every copy of the face at the font size (the shaper's, the rasteriser's) takes `opsz` from that
/// size again. At 24 px that was an `opsz` of 24 for the 16 asked for, 245.6 px for a line Chrome lays out at 254.1.
///
/// The advances are the ones in `fixtures/font-smoothing-oracle.json` (see the header of font-smoothing.test.tsx for
/// the page): "Hamburgefonstiv 0123" in `fixtures/inter-subset.ttf` at 14 and 24 px with `wght`/`opsz` of (400, 14),
/// (500, 16), (550, 18) and (700, 25), at device scale 1 and 2.

import fs from "fs"
import path from "path"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip
const FIXTURES = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures")
const FONT = path.join(FIXTURES, "inter-subset.ttf")
const TEXT = "Hamburgefonstiv 0123"

type Sample = { size: number; wght: number; opsz: number; advance: number }

const ORACLE: Record<"x1" | "x2", { scale: number; samples: Sample[] }> = JSON.parse(
  fs.readFileSync(path.join(FIXTURES, "font-smoothing-oracle.json"), "utf8")
)

function distinct(samples: Sample[]) {
  const seen = new Map<string, Sample>()
  for (const sample of samples) seen.set(`${sample.size}/${sample.wght}/${sample.opsz}`, sample)
  return [...seen.values()]
}

/** The width of `TEXT` at `size` px with `settings`, in a window of `scale`. */
function widthOf(scale: 1 | 2, size: number, settings: string | undefined) {
  const root = createTestRoot({ width: 400 * scale, height: 100 * scale, fonts: [FONT] })
  try {
    root.renderer.setWindowScaleFactor(scale)
    root.render(
      <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start", fontFamily: "Inter Variable", fontSize: size }}>
        <text testId="text" style={settings === undefined ? {} : { fontVariationSettings: settings }}>{TEXT}</text>
      </div>
    )
    const element = root.renderer.findByTestId("text")
    expect(element, "missing text").toBeDefined()
    return root.renderer.getElementBounds(element!.id)!.width
  } finally {
    root.unmount()
  }
}

describeNative("font-variation-settings opsz against Chrome", () => {
  for (const scale of [1, 2] as const) {
    it(`lays text out as Chrome does for an opsz of 14, 16, 18 and 25, at scale ${scale}`, () => {
      const samples = distinct(ORACLE[`x${scale}`].samples)
      expect(samples).toHaveLength(8)
      for (const sample of samples) {
        const own = widthOf(scale, sample.size, `'wght' ${sample.wght}, 'opsz' ${sample.opsz}`)
        expect(
          Math.abs(own - sample.advance),
          `${sample.size}px wght ${sample.wght} opsz ${sample.opsz}: ${own}, Chrome ${sample.advance}`
        ).toBeLessThan(0.02)
      }
    })
  }

  it("sets an opsz of 16 apart from the automatic one at the size of the text", () => {
    for (const size of [14, 24]) {
      const automatic = widthOf(1, size, undefined)
      const explicit = (opsz: number) => widthOf(1, size, `'opsz' ${opsz}`)
      // A larger optical size is narrower, 16 is between 14 and 18 whatever the font size picks.
      expect(explicit(16), `${size}px`).toBeLessThan(explicit(14))
      expect(explicit(16), `${size}px`).toBeGreaterThan(explicit(18))
      expect(Math.abs(explicit(16) - automatic), `${size}px opsz 16 against the automatic one`).toBeGreaterThan(0.5)
    }
  })
})
