/// `WebkitFontSmoothing` against Chrome on macOS (#67). `antialiased` draws glyphs as plain coverage, the same ink for
/// dark text and for light text; `auto` keeps CoreGraphics' smoothing, which thickens the strokes (light text most).
/// gpuix drew every text the `auto` way, 9-17 % heavier than Chrome's `antialiased` for dark text and 22-42 % for light.
///
/// `fixtures/font-smoothing-oracle.html` is the page and `fixtures/font-smoothing-oracle.json` what headless Chrome
/// paints from it (regenerate with `font-smoothing-oracle.sh`): "Hamburgefonstiv 0123" in Inter Variable at 14 and 24
/// px, `wght`/`opsz` of (400, 14), (500, 16), (550, 18) and (700, 25), as black on white, white on black and #a1a1a1 on
/// #0a0a0a, once with `-webkit-font-smoothing: antialiased` and once with `auto`, at device scale 1 and 2. A sample's
/// ink is the sum over its line box, from 4 px before the text to 4 px after it, of |L - L_bg| / |L_fg - L_bg| (L the
/// gamma-encoded Rec.709 luma of the screenshot), in CSS px squared. The font is `fixtures/inter-subset.ttf`, Inter
/// Variable cut down with pyftsubset to the glyphs of the text (OFL, next to it): Chrome's ink at 14 px differs between
/// the subset and the whole font by 8-44 px squared, so the oracle and the test load the same file.
///
/// Where a sample is compared with Chrome:
///  - `antialiased`, dark and light text: equal to the pixel, so the bound is 0.2 % of the ink. That holds for the `opsz` 16
///    samples too (font-optical-size.test.tsx).
///  - `antialiased`, #a1a1a1 on #0a0a0a: up to 0.9 % heavier at scale 1 and 0.4 % at scale 2, not traced. The glyph
///    coverage is the same as for black; the blend of a grey on a near black differs a little between gpui's shader
///    and Skia's.
///  - `auto`: not changed by #67; it is 1-6.5 % from Chrome's `auto` (lighter text at scale 2, heavier at scale 1), and
///    bound at 7 %.

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readPixels, SHOTS_DIR, type Pixels } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip
const FIXTURES = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures")
const FONT = path.join(FIXTURES, "inter-subset.ttf")

type Mode = "antialiased" | "auto"
type Polarity = "dark" | "light" | "dim"

type Sample = {
  id: string
  smoothing: Mode
  polarity: Polarity
  size: number
  wght: number
  opsz: number
  box: { x: number; y: number; w: number; h: number }
  ink: number
  advance: number
}

const ORACLE: Record<"x1" | "x2", { scale: number; samples: Sample[] }> = JSON.parse(
  fs.readFileSync(path.join(FIXTURES, "font-smoothing-oracle.json"), "utf8")
)

function sampleNamed(id: string) {
  const sample = ORACLE.x1.samples.find((candidate) => candidate.id === id)
  if (!sample) throw new Error(`the oracle has no sample ${id}`)
  return sample
}

const TEXT = "Hamburgefonstiv 0123"
const COLORS = {
  dark: { fg: "#000000", bg: "#ffffff", left: 0 },
  light: { fg: "#ffffff", bg: "#000000", left: 400 },
  dim: { fg: "#a1a1a1", bg: "#0a0a0a", left: 800 },
} as const

/** What the page of the oracle is cut into: one panel of one mode, which the window shows from its corner. The whole
 *  page is 1200 x 704 CSS px, more than a test window holds at scale 2. */
const PANEL = { width: 400, height: 320 }
const MODE_TOP: Record<Mode, number> = { antialiased: 0, auto: 400 }

function luma(rgb: ArrayLike<number>) {
  return (0.2126 * rgb[0]! + 0.7152 * rgb[1]! + 0.0722 * rgb[2]!) / 255
}

function hexRgb(hex: string) {
  return [1, 3, 5].map((at) => parseInt(hex.slice(at, at + 2), 16))
}

/** The ink of a line box that starts at (`x`, `y`) in CSS px and is `height` tall, for text `advance` wide. */
function ink(
  pixels: Pixels,
  scale: number,
  polarity: Polarity,
  line: { x: number; y: number; height: number; advance: number }
) {
  const lbg = luma(hexRgb(COLORS[polarity].bg))
  const span = Math.abs(luma(hexRgb(COLORS[polarity].fg)) - lbg)
  const x0 = line.x - 4
  const x1 = Math.floor(line.x + line.advance + 4)
  let total = 0
  for (let y = line.y * scale; y < (line.y + line.height) * scale; y++) {
    for (let x = x0 * scale; x < x1 * scale; x++) {
      total += Math.abs(luma(pixels.at(x, y)) - lbg)
    }
  }
  return total / span / (scale * scale)
}

function openWindow(scale: 1 | 2) {
  // The test window is offscreen at scale 1: at scale 2 the scene is drawn at twice the size into the same pixels,
  // which shows the top left of the page, at twice the size.
  const root = createTestRoot({ width: PANEL.width * scale, height: PANEL.height * scale, fonts: [FONT] })
  root.renderer.setWindowScaleFactor(scale)
  expect(root.renderer.getA11yTree().frame?.scale_factor, "the window has the scale asked for").toBe(scale)
  return root
}

function screenshot(root: ReturnType<typeof openWindow>, name: string, scale: number) {
  root.renderer.dispatchNativeEvents()
  const file = path.join(SHOTS_DIR, `font-smoothing-${name}-x${scale}.png`)
  root.renderer.captureScreenshot(file)
  const pixels = readPixels(file)
  expect(pixels.width).toBe(PANEL.width * scale)
  return pixels
}

function panel(polarity: Polarity, children: React.ReactNode, style: Record<string, unknown> = {}) {
  return (
    <div
      style={{
        position: "relative",
        width: PANEL.width,
        height: PANEL.height,
        backgroundColor: COLORS[polarity].bg,
        color: COLORS[polarity].fg,
        ...style,
      }}
    >
      {children}
    </div>
  )
}

function line(sample: Pick<Sample, "size" | "wght" | "opsz" | "box">, left: number, top: number, extra = {}) {
  return {
    position: "absolute",
    left: sample.box.x - left,
    top: sample.box.y - top,
    width: sample.box.w,
    height: sample.box.h,
    whiteSpace: "nowrap",
    fontFamily: "Inter Variable",
    fontSize: sample.size,
    lineHeight: `${sample.box.h}px`,
    fontVariationSettings: `'wght' ${sample.wght}, 'opsz' ${sample.opsz}`,
    ...extra,
  } as React.CSSProperties
}

/** The ink gpuix paints for every sample of the oracle at `scale`, with `smoothingOf` its mode's value. */
function paintedInk(scale: 1 | 2, smoothingOf: (mode: Mode) => string) {
  const painted = new Map<string, number>()
  for (const mode of ["antialiased", "auto"] as const) {
    for (const polarity of Object.keys(COLORS) as Polarity[]) {
      const samples = ORACLE[`x${scale}`].samples.filter((s) => s.smoothing === mode && s.polarity === polarity)
      const root = openWindow(scale)
      try {
        root.render(
          panel(
            polarity,
            samples.map((sample) => (
              <div key={sample.id} style={line(sample, COLORS[polarity].left, MODE_TOP[mode])}>
                {TEXT}
              </div>
            )),
            { WebkitFontSmoothing: smoothingOf(mode) }
          )
        )
        const pixels = screenshot(root, `${mode}-${polarity}`, scale)
        for (const sample of samples) {
          painted.set(
            sample.id,
            ink(pixels, scale, polarity, {
              x: sample.box.x - COLORS[polarity].left,
              y: sample.box.y - MODE_TOP[mode],
              height: sample.box.h,
              advance: sample.advance,
            })
          )
        }
      } finally {
        root.unmount()
      }
    }
  }
  return painted
}

function got<T>(value: T | undefined, what: string): T {
  if (value === undefined) throw new Error(`no ${what}`)
  return value
}

function relative(painted: number, chrome: number) {
  return Math.abs(painted - chrome) / chrome
}

/** The tolerance of a sample against Chrome's `antialiased` ink, see the header. */
function antialiasedBound(sample: Sample, scale: number) {
  if (sample.polarity === "dim") return scale === 1 ? 0.01 : 0.005
  return 0.002
}

describeNative("-webkit-font-smoothing against Chrome", () => {
  for (const scale of [1, 2] as const) {
    it(`antialiased draws the ink of Chrome's antialiased, dark and light text alike, at scale ${scale}`, () => {
      const painted = paintedInk(scale, (mode) => mode)
      const samples = ORACLE[`x${scale}`].samples.filter((sample) => sample.smoothing === "antialiased")
      expect(samples).toHaveLength(24)
      for (const sample of samples) {
        const own = got(painted.get(sample.id), sample.id)
        expect(relative(own, sample.ink), `${sample.id}: ${own} against Chrome's ${sample.ink}`).toBeLessThan(
          antialiasedBound(sample, scale)
        )
      }
    })

    it(`auto stays near Chrome's auto and heavier than antialiased, at scale ${scale}`, () => {
      const painted = paintedInk(scale, (mode) => mode)
      const samples = ORACLE[`x${scale}`].samples.filter((sample) => sample.smoothing === "auto")
      expect(samples).toHaveLength(24)
      for (const sample of samples) {
        const own = got(painted.get(sample.id), sample.id)
        expect(relative(own, sample.ink), `${sample.id}: ${own} against Chrome's ${sample.ink}`).toBeLessThan(0.07)
        const bare = got(painted.get(sample.id.replace("auto-", "antialiased-")), sample.id)
        expect(own, `${sample.id} is thicker than antialiased`).toBeGreaterThan(bare * 1.005)
      }
    })
  }
})

type Row = string | ((text: string) => React.ReactNode)

describeNative("-webkit-font-smoothing", () => {
  const DARK = sampleNamed("antialiased-dark-14-400")
  const LIGHT = sampleNamed("antialiased-light-14-400")
  const ANTIALIASED = DARK.ink
  const AUTO = sampleNamed("auto-dark-14-400").ink

  /** The ink of each row, the same text one line below the other and drawn in one frame. A string row is
   *  that value of `WebkitFontSmoothing` on the text; a function builds the row's element around the text. */
  function rowsInk<Name extends string>(
    polarity: Polarity,
    rows: Record<Name, Row>,
    rootStyle: Record<string, unknown> = {},
    scale: 1 | 2 = 1
  ) {
    const sample = polarity === "dark" ? DARK : LIGHT
    const names = Object.keys(rows) as Name[]
    const root = openWindow(scale)
    try {
      root.render(
        panel(
          polarity,
          names.map((name, index) => {
            const row = rows[name]
            return (
              <div key={name} style={line(sample, COLORS[polarity].left, 0, { top: 8 + index * 32 })}>
                {typeof row === "string" ? <div style={{ WebkitFontSmoothing: row }}>{TEXT}</div> : row(TEXT)}
              </div>
            )
          }),
          rootStyle
        )
      )
      const pixels = screenshot(root, `rows-${polarity}-${names.join("-")}`, scale)
      const inks = {} as Record<Name, number>
      names.forEach((name, index) => {
        inks[name] = ink(pixels, scale, polarity, { x: 16, y: 8 + index * 32, height: 24, advance: sample.advance })
      })
      return inks
    } finally {
      root.unmount()
    }
  }

  it("draws a glyph both ways in one frame without sharing an atlas tile", () => {
    // Dark text has no dilation either way: the two copies differ in nothing but the value, so the tile is told
    // apart by it alone. Each copy has the ink it has when it is alone in the window.
    const alone = rowsInk("dark", { antialiased: "antialiased" }).antialiased
    const aloneAuto = rowsInk("dark", { auto: "auto" }).auto
    expect(aloneAuto).toBeGreaterThan(alone * 1.05)
    expect(relative(alone, ANTIALIASED)).toBeLessThan(0.002)

    const autoFirst = rowsInk("dark", { auto: "auto", antialiased: "antialiased" })
    expect(autoFirst.auto).toBeCloseTo(aloneAuto, 6)
    expect(autoFirst.antialiased).toBeCloseTo(alone, 6)
    const antialiasedFirst = rowsInk("dark", { antialiased: "antialiased", auto: "auto" })
    expect(antialiasedFirst.antialiased).toBeCloseTo(alone, 6)
    expect(antialiasedFirst.auto).toBeCloseTo(aloneAuto, 6)
  })

  it("keeps the tile of the other value across frames", () => {
    const root = openWindow(1)
    try {
      const frame = (smoothing: string) =>
        root.render(panel("dark", <div style={line(DARK, 0, 0, { WebkitFontSmoothing: smoothing })}>{TEXT}</div>))
      const measure = (name: string) =>
        ink(screenshot(root, name, 1), 1, "dark", { x: 16, y: 8, height: 24, advance: DARK.advance })
      frame("auto")
      const auto = measure("frames-auto")
      frame("antialiased")
      const antialiased = measure("frames-antialiased")
      frame("auto")
      expect(measure("frames-auto-again")).toBeCloseTo(auto, 6)
      expect(auto).toBeGreaterThan(antialiased * 1.05)
      expect(relative(antialiased, ANTIALIASED)).toBeLessThan(0.002)
      expect(relative(auto, AUTO)).toBeLessThan(0.07)
    } finally {
      root.unmount()
    }
  })

  it("is inherited, and the nearest declaration wins", () => {
    const inks = rowsInk("dark", {
      plain: (text) => text,
      inheriting: (text) => <div style={{ WebkitFontSmoothing: "antialiased" }}>{text}</div>,
      nested: (text) => (
        <div style={{ WebkitFontSmoothing: "antialiased" }}>
          <div>
            <div>{text}</div>
          </div>
        </div>
      ),
      overridden: (text) => (
        <div style={{ WebkitFontSmoothing: "antialiased" }}>
          <div style={{ WebkitFontSmoothing: "auto" }}>{text}</div>
        </div>
      ),
      throughContents: (text) => (
        <div style={{ WebkitFontSmoothing: "antialiased" }}>
          <div style={{ display: "contents" }}>
            <div>{text}</div>
          </div>
        </div>
      ),
      belowOverride: (text) => (
        <div style={{ WebkitFontSmoothing: "antialiased" }}>
          <div style={{ WebkitFontSmoothing: "auto" }}>
            <div>{text}</div>
          </div>
        </div>
      ),
    })
    expect(relative(inks.plain, AUTO)).toBeLessThan(0.07)
    for (const antialiased of [inks.inheriting, inks.nested, inks.throughContents]) {
      expect(relative(antialiased, ANTIALIASED)).toBeLessThan(0.002)
    }
    expect(inks.overridden).toBeCloseTo(inks.plain, 6)
    expect(inks.belowOverride).toBeCloseTo(inks.plain, 6)
  })

  it("is inherited from the root of the page and by inline text", () => {
    const inks = rowsInk(
      "dark",
      {
        block: (text) => text,
        span: (text) => <span>{text}</span>,
        overriddenSpan: (text) => <span style={{ WebkitFontSmoothing: "auto" }}>{text}</span>,
      },
      { WebkitFontSmoothing: "antialiased" }
    )
    expect(relative(inks.block, ANTIALIASED)).toBeLessThan(0.002)
    expect(relative(inks.span, ANTIALIASED)).toBeLessThan(0.002)
    expect(relative(inks.overriddenSpan, AUTO)).toBeLessThan(0.07)
  })

  it("paints a span of one value inside text of the other", () => {
    const around = (inside: string, outside: string) => (text: string) => (
      <div style={{ WebkitFontSmoothing: outside }}>
        <span style={{ WebkitFontSmoothing: inside }}>{text}</span>
      </div>
    )
    const inks = rowsInk("dark", {
      antialiasedInAuto: around("antialiased", "auto"),
      autoInAntialiased: around("auto", "antialiased"),
    })
    expect(relative(inks.antialiasedInAuto, ANTIALIASED)).toBeLessThan(0.002)
    expect(relative(inks.autoInAntialiased, AUTO)).toBeLessThan(0.07)
  })

  it("draws subpixel-antialiased as auto, and ignores a value CSS does not have", () => {
    const inks = rowsInk("light", {
      auto: "auto",
      subpixel: "subpixel-antialiased",
      none: "none",
      misspelt: "antialised",
      antialiased: "antialiased",
    })
    expect(inks.subpixel).toBe(inks.auto)
    // A value CSS does not have leaves what the element inherits, which is `auto` here.
    expect(inks.none).toBe(inks.auto)
    expect(inks.misspelt).toBe(inks.auto)
    expect(inks.auto).toBeGreaterThan(inks.antialiased * 1.2)

    const inherited = rowsInk(
      "light",
      { invalid: (text) => <div style={{ WebkitFontSmoothing: "bold" } as React.CSSProperties}>{text}</div> },
      { WebkitFontSmoothing: "antialiased" }
    )
    expect(inherited.invalid).toBe(inks.antialiased)
  })

  it("draws light text as dark text at scale 2 as well", () => {
    const dark = rowsInk("dark", { antialiased: "antialiased" }, {}, 2).antialiased
    const light = rowsInk("light", { antialiased: "antialiased" }, {}, 2).antialiased
    expect(light).toBeCloseTo(dark, 6)
  })
})
