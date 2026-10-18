import type { ReactElement } from "react"
import { beforeAll, describe, expect, it } from "vitest"
import { animate, defineKeyframes, motion, useMotionValue } from "../index.js"
import type { MotionValue } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

/** The default font is the system UI font, a variable font on macOS: a heavier `wght` is
 *  wider, and so is a larger `wdth` (30 to 150), so the width of the text tells the axes. */
const LIGHT = '"wght" 100'
const HEAVY = '"wght" 900'
const NARROW = '"wdth" 60'
const MIDDLE = '"wght" 500'
const WIDE_HEAVY = '"wght" 900, "wdth" 120'
const linear = { duration: 1, ease: "linear" } as const

const column = { display: "flex", flexDirection: "column", alignItems: "flex-start", fontSize: 32 } as const

defineKeyframes("font-variation-swell", {
  from: { fontVariationSettings: LIGHT },
  to: { fontVariationSettings: HEAVY },
})

function setup() {
  const { render, renderer, unmount } = createTestRoot()
  renderer.clockPause()
  const width = () => {
    renderer.flush()
    renderer.dispatchNativeEvents()
    return renderer.getElementBounds(renderer.findByTestId("text").id).width
  }
  const advance = (ms: number) => {
    renderer.clockFastForward(ms)
    return width()
  }
  return { render, renderer, width, advance, unmount }
}

/** Only one test root lives at a time, so the widths a test compares against are measured
 *  up front. */
const widths = new Map<string, number>()
const reference = (fontVariationSettings: string) => widths.get(fontVariationSettings)!
beforeAll(() => {
  if (!hasNativeTestRenderer) return
  for (const settings of [LIGHT, HEAVY, NARROW, MIDDLE, WIDE_HEAVY]) widths.set(settings, staticWidth(settings))
})

/** The width of the text when it is styled `fontVariationSettings` from the start. */
function staticWidth(fontVariationSettings: string): number {
  const { render, width, unmount } = setup()
  try {
    render(
      <div style={column}>
        <text testId="text" style={{ fontVariationSettings }}>
          WWWWWWWWWW
        </text>
      </div>
    )
    return width()
  } finally {
    unmount()
  }
}

const text = <text testId="text">WWWWWWWWWW</text>

describeNative("animating fontVariationSettings", () => {
  it("motion animate moves the width of the text with the axis, monotonically", () => {
    const { render, width, advance } = setup()
    render(
      <motion.div style={column} initial={{ fontVariationSettings: LIGHT }} animate={{ fontVariationSettings: HEAVY }} transition={linear}>
        {text}
      </motion.div>
    )
    const light = reference(LIGHT)
    const heavy = reference(HEAVY)
    expect(heavy).toBeGreaterThan(light + 10)

    const samples = [width(), advance(250), advance(250), advance(250), advance(250)]
    expect(samples[0]).toBeCloseTo(light, 1)
    expect(samples[4]).toBeCloseTo(heavy, 1)
    for (let index = 1; index < samples.length; index++) {
      expect(samples[index]!, `sample ${index}`).toBeGreaterThan(samples[index - 1]! + 0.5)
    }
    // Half way is the midpoint wght of 500, not the nearer end.
    expect(samples[2]!).toBeCloseTo(reference(MIDDLE), 0)
  })

  it("animates from the inherited settings when the element sets none", () => {
    const { render, width, advance } = setup()
    render(
      <div style={{ ...column, fontVariationSettings: LIGHT }}>
        <motion.div style={column} animate={{ fontVariationSettings: HEAVY }} initial={{ opacity: 1 }} transition={linear}>
          {text}
        </motion.div>
      </div>
    )
    expect(width()).toBeCloseTo(reference(LIGHT), 1)
    expect(advance(500)).toBeCloseTo(reference(MIDDLE), 0)
    expect(advance(500)).toBeCloseTo(reference(HEAVY), 1)
  })

  it("keyframes blend per axis, in any order of the tags", () => {
    const { render, width, advance } = setup()
    render(
      <motion.div
        style={column}
        initial={{ fontVariationSettings: '"wght" 100, "wdth" 60' }}
        animate={{ fontVariationSettings: ['"wght" 100, "wdth" 60', '"wdth" 120, "wght" 900', '"wght" 100, "wdth" 60'] }}
        transition={{ duration: 2, ease: "linear" }}
      >
        {text}
      </motion.div>
    )
    const start = width()
    const peak = advance(1000)
    const end = advance(1000)
    expect(peak).toBeCloseTo(reference(WIDE_HEAVY), 1)
    expect(peak).toBeGreaterThan(start + 10)
    expect(end).toBeCloseTo(start, 1)
  })

  it("values naming other axes flip halfway", () => {
    const { render, width, advance } = setup()
    render(
      <motion.div style={column} initial={{ fontVariationSettings: HEAVY }} animate={{ fontVariationSettings: NARROW }} transition={linear}>
        {text}
      </motion.div>
    )
    expect(width()).toBeCloseTo(reference(HEAVY), 1)
    expect(advance(490)).toBeCloseTo(reference(HEAVY), 1)
    expect(advance(20)).toBeCloseTo(reference(NARROW), 1)
    expect(advance(500)).toBeCloseTo(reference(NARROW), 1)
  })

  it("a bound motion value drives it", () => {
    const { render, width, advance } = setup()
    let settings!: MotionValue<string>
    function App(): ReactElement {
      settings = useMotionValue(LIGHT)
      return (
        <div style={{ ...column, fontVariationSettings: settings }}>
          {text}
        </div>
      )
    }
    render(<App />)
    expect(width()).toBeCloseTo(reference(LIGHT), 1)

    animate(settings, HEAVY, linear)
    const samples = [advance(250), advance(250), advance(250), advance(250)]
    expect(samples[0]!).toBeGreaterThan(reference(LIGHT) + 0.5)
    for (let index = 1; index < samples.length; index++) {
      expect(samples[index]!, `sample ${index}`).toBeGreaterThan(samples[index - 1]! + 0.5)
    }
    expect(samples[1]!).toBeCloseTo(reference(MIDDLE), 0)
    expect(samples[3]!).toBeCloseTo(reference(HEAVY), 1)

    settings.set(LIGHT)
    expect(advance(16)).toBeCloseTo(reference(LIGHT), 1)
  })
})

describeNative("transitioning fontVariationSettings", () => {
  const box = (settings: string, transition: string) => (
    <div style={{ ...column, fontVariationSettings: settings, transition }}>{text}</div>
  )

  it("blends a change of the same axes", () => {
    const { render, width, advance } = setup()
    const transition = "font-variation-settings 1s linear"
    render(box(LIGHT, transition))
    render(box(HEAVY, transition))
    const samples = [width(), advance(250), advance(250), advance(250), advance(250)]
    expect(samples[0]).toBeCloseTo(reference(LIGHT), 1)
    expect(samples[2]!).toBeCloseTo(reference(MIDDLE), 0)
    expect(samples[4]).toBeCloseTo(reference(HEAVY), 1)
    for (let index = 1; index < samples.length; index++) {
      expect(samples[index]!, `sample ${index}`).toBeGreaterThan(samples[index - 1]! + 0.5)
    }
  })

  it("takes a change of other axes at once, unless the transition allows discrete", () => {
    const normal = setup()
    normal.render(box(HEAVY, "font-variation-settings 1s linear"))
    normal.render(box(NARROW, "font-variation-settings 1s linear"))
    expect(normal.width()).toBeCloseTo(reference(NARROW), 1)
    normal.unmount()

    const discrete = setup()
    const transition = "font-variation-settings 1s linear allow-discrete"
    discrete.render(box(HEAVY, transition))
    discrete.render(box(NARROW, transition))
    expect(discrete.width()).toBeCloseTo(reference(HEAVY), 1)
    expect(discrete.advance(490)).toBeCloseTo(reference(HEAVY), 1)
    expect(discrete.advance(20)).toBeCloseTo(reference(NARROW), 1)
  })

  it("a keyframes animation blends it", () => {
    const { render, width, advance } = setup()
    render(<div style={{ ...column, animation: "font-variation-swell 1s linear" }}>{text}</div>)
    const samples = [width(), advance(250), advance(250), advance(250)]
    expect(samples[0]).toBeCloseTo(reference(LIGHT), 1)
    expect(samples[2]!).toBeCloseTo(reference(MIDDLE), 0)
    for (let index = 1; index < samples.length; index++) {
      expect(samples[index]!, `sample ${index}`).toBeGreaterThan(samples[index - 1]! + 0.5)
    }
  })
})

describeNative("fontVariationSettings of a span in motion", () => {
  const paragraph = (settings: string, extra: Record<string, unknown> = {}) => (
    <div style={column}>
      <text testId="text">
        WWWW<span style={{ fontVariationSettings: settings, ...extra }}>WWWWWW</span>WWWW
      </text>
    </div>
  )

  /** The width of the paragraph when its span is styled `settings` from the start. */
  function staticSpanWidth(settings: string): number {
    const { render, width, unmount } = setup()
    try {
      render(paragraph(settings))
      return width()
    } finally {
      unmount()
    }
  }

  it("blends a CSS transition of the same axes, and the paragraph follows", () => {
    const [light, middle, heavy] = [LIGHT, MIDDLE, HEAVY].map(staticSpanWidth) as [number, number, number]
    expect(heavy).toBeGreaterThan(light + 5)
    const { render, width, advance, unmount } = setup()
    try {
      const transition = "font-variation-settings 1s linear"
      render(paragraph(LIGHT, { transition }))
      render(paragraph(HEAVY, { transition }))
      const samples = [width(), advance(250), advance(250), advance(250), advance(250)]
      expect(samples[0]).toBeCloseTo(light, 1)
      expect(samples[2]!).toBeCloseTo(middle, 0)
      expect(samples[4]).toBeCloseTo(heavy, 1)
      for (let index = 1; index < samples.length; index++) {
        expect(samples[index]!, `sample ${index}`).toBeGreaterThan(samples[index - 1]! + 0.3)
      }
    } finally {
      unmount()
    }
  })

  it("flips values that name other axes halfway under allow-discrete", () => {
    const [heavy, narrow] = [HEAVY, NARROW].map(staticSpanWidth) as [number, number]
    const { render, width, advance, unmount } = setup()
    try {
      const transition = "font-variation-settings 1s linear allow-discrete"
      render(paragraph(HEAVY, { transition }))
      render(paragraph(NARROW, { transition }))
      expect(width()).toBeCloseTo(heavy, 1)
      expect(advance(490)).toBeCloseTo(heavy, 1)
      expect(advance(20)).toBeCloseTo(narrow, 1)
    } finally {
      unmount()
    }
  })

  it("a motion span animates it", () => {
    const [light, middle, heavy] = [LIGHT, MIDDLE, HEAVY].map(staticSpanWidth) as [number, number, number]
    const { render, width, advance, unmount } = setup()
    try {
      render(
        <div style={column}>
          <text testId="text">
            WWWW
            <motion.span initial={{ fontVariationSettings: LIGHT }} animate={{ fontVariationSettings: HEAVY }} transition={linear}>
              WWWWWW
            </motion.span>
            WWWW
          </text>
        </div>
      )
      expect(width()).toBeCloseTo(light, 1)
      expect(advance(500)).toBeCloseTo(middle, 0)
      expect(advance(500)).toBeCloseTo(heavy, 1)
    } finally {
      unmount()
    }
  })
})
