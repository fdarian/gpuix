import path from "path"
import { describe, expect, it } from "vitest"
import type { ReactNode } from "react"
import type { StyleDesc } from "@ghia/native/host"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const THUMB = "#ff0000"
const TRACK = "#0000ff"
const COLORS = `${THUMB} ${TRACK}`

type Pixel = [number, number, number, number]
const isRed = ([r, g, b]: Pixel) => r > 200 && g < 60 && b < 60
const isBlue = ([r, g, b]: Pixel) => b > 200 && r < 60 && g < 60

/** A 100 x 100 scroller of 100 x 300 content, so it scrolls down only, with the clock stopped. */
function scroller(
  style: StyleDesc = {},
  extra: { onScroll?: (top: number) => void; onContentMouseDown?: () => void } = {},
  content: ReactNode = null
) {
  const root = createTestRoot()
  root.render(
    <div
      testId="scroller"
      style={{ width: 100, height: 100, overflow: "scroll", scrollbarColor: COLORS, ...style }}
      onScroll={extra.onScroll ? (event) => extra.onScroll!(event.scrollTop) : undefined}
    >
      {content ?? (
        <div
          style={{ width: 100, height: 300, backgroundColor: "#202020" }}
          onMouseDown={extra.onContentMouseDown}
        />
      )}
    </div>
  )
  const { renderer } = root
  const id = renderer.findByTestId("scroller")!.id
  renderer.clockPause()
  let shots = 0
  return {
    ...root,
    renderer,
    id,
    top: () => renderer.getScrollOffset(id)![1],
    scrollTo: (top: number) => renderer.scrollTo(id, 0, top),
    /** Moves the clock on and draws the frame it lands on, as the display link would. */
    advance(milliseconds: number) {
      renderer.clockFastForward(milliseconds)
      renderer.flush()
    },
    shoot() {
      renderer.flush()
      const file = path.join(SHOTS_DIR, `scrollbar-${process.pid}-${shots++}.png`)
      renderer.captureScreenshot(file)
      return readLogicalPixels(file, renderer)
    },
  }
}

function rows(pixels: ReturnType<typeof readLogicalPixels>, x: number, test: (p: Pixel) => boolean) {
  const found: number[] = []
  for (let y = 0; y < 100; y++) if (test(pixels.at(x, y))) found.push(y)
  return found
}

function anyBar(pixels: ReturnType<typeof readLogicalPixels>) {
  for (let x = 80; x < 100; x++) {
    if (rows(pixels, x, isRed).length || rows(pixels, x, isBlue).length) return true
  }
  return false
}

describeNative("overlay scrollbars", () => {
  it("are hidden at rest", () => {
    const { shoot, unmount } = scroller()
    try {
      expect(anyBar(shoot())).toBe(false)
    } finally {
      unmount()
    }
  })

  it("appear along the right edge when the offset changes", () => {
    const { scrollTo, shoot, unmount } = scroller()
    try {
      scrollTo(50)
      const pixels = shoot()
      // 7px thick, 2px in from the edge: x 91 to 97. A third of the track, a quarter along it.
      expect(rows(pixels, 94, isRed).length).toBeGreaterThan(30)
      expect(rows(pixels, 94, isRed).length).toBeLessThan(36)
      expect(rows(pixels, 94, isRed)[0]).toBeGreaterThan(14)
      expect(rows(pixels, 94, isRed)[0]).toBeLessThan(20)
      expect(rows(pixels, 91, isRed).length).toBeGreaterThan(0)
      expect(rows(pixels, 90, isRed)).toEqual([])
      expect(rows(pixels, 98, isRed)).toEqual([])
    } finally {
      unmount()
    }
  })

  it("appear for a change made by the wheel, not only by scrollTo", () => {
    const { renderer, shoot, unmount } = scroller()
    try {
      renderer.nativeSimulateScrollWheel(50, 50, 0, 40)
      expect(rows(shoot(), 94, isRed).length).toBeGreaterThan(0)
    } finally {
      unmount()
    }
  })

  it("stay for a second, fade over 200 ms, then go", () => {
    const { scrollTo, advance, shoot, unmount } = scroller()
    try {
      scrollTo(50)
      advance(900)
      expect(rows(shoot(), 94, isRed).length, "still fully there").toBeGreaterThan(30)

      advance(150)
      const fading = shoot()
      const [r] = fading.at(94, 30)
      // 75% of red over the #202020 content: no longer solid, not yet gone.
      expect(r).toBeGreaterThan(0x40)
      expect(r).toBeLessThan(0xf0)

      advance(1000)
      expect(anyBar(shoot())).toBe(false)
    } finally {
      unmount()
    }
  })

  it("come back when the offset moves again", () => {
    const { scrollTo, advance, shoot, unmount } = scroller()
    try {
      scrollTo(50)
      advance(2000)
      expect(anyBar(shoot())).toBe(false)
      scrollTo(80)
      expect(rows(shoot(), 94, isRed).length).toBeGreaterThan(0)
    } finally {
      unmount()
    }
  })

  it("thicken under the pointer, show a track, and stay while it is there", () => {
    const { renderer, scrollTo, advance, shoot, unmount } = scroller()
    try {
      scrollTo(50)
      renderer.nativeSimulateMouseMove(95, 80)
      advance(200)
      const hovered = shoot()
      // 11px thick: x 87 to 97, and a track under the whole length.
      expect(rows(hovered, 88, isRed).length).toBeGreaterThan(25)
      expect(isBlue(hovered.at(88, 90))).toBe(true)

      advance(5000)
      expect(rows(shoot(), 94, isRed).length, "held while hovered").toBeGreaterThan(30)

      renderer.nativeSimulateMouseMove(20, 20)
      advance(1500)
      expect(anyBar(shoot())).toBe(false)
    } finally {
      unmount()
    }
  })

  it("are narrower with scrollbarWidth thin", () => {
    const { scrollTo, shoot, unmount } = scroller({ scrollbarWidth: "thin" })
    try {
      scrollTo(50)
      const pixels = shoot()
      // 4px: x 94 to 97.
      expect(rows(pixels, 95, isRed).length).toBeGreaterThan(0)
      expect(rows(pixels, 93, isRed)).toEqual([])
    } finally {
      unmount()
    }
  })

  it("are gone with scrollbarWidth none, and the container still scrolls", () => {
    const { renderer, scrollTo, top, shoot, unmount } = scroller({ scrollbarWidth: "none" })
    try {
      scrollTo(50)
      expect(anyBar(shoot())).toBe(false)
      expect(top()).toBe(50)
      renderer.nativeSimulateScrollWheel(50, 50, 0, 30)
      expect(top()).toBe(80)
    } finally {
      unmount()
    }
  })

  it("follow the system appearance by default", () => {
    // The offscreen window takes the system's appearance and cannot be forced, so draw on
    // the background the bar has to stand out from: a 50% black thumb on light, 50% white on
    // dark, which is mid grey over the opposite colour either way.
    const probe = createTestRoot()
    const dark = probe.renderer.getAppearance() === "dark"
    probe.unmount()
    const { scrollTo, shoot, unmount } = scroller(
      { scrollbarColor: undefined, backgroundColor: dark ? "#000000" : "#ffffff" },
      {},
      <div style={{ width: 100, height: 300 }} />
    )
    try {
      scrollTo(50)
      const [r, g, b] = shoot().at(94, 30)
      for (const channel of [r, g, b]) expect(Math.abs(channel - 128)).toBeLessThan(12)
    } finally {
      unmount()
    }
  })

  it("ignore a scrollbarColor that is not two colours", () => {
    const { scrollTo, shoot, unmount } = scroller(
      { scrollbarColor: THUMB, backgroundColor: "#ffffff" },
      {},
      <div style={{ width: 100, height: 300 }} />
    )
    try {
      scrollTo(50)
      // Falls back to auto: a grey thumb, not a red one.
      expect(rows(shoot(), 94, isRed)).toEqual([])
    } finally {
      unmount()
    }
  })

  it("draw a bar along the bottom for sideways scrolling and leave the corner free", () => {
    const { renderer, id, shoot, unmount } = scroller(
      {},
      {},
      <div style={{ width: 300, height: 300, backgroundColor: "#202020" }} />
    )
    try {
      renderer.scrollTo(id, 50, 50)
      const pixels = shoot()
      const across: number[] = []
      for (let x = 0; x < 100; x++) if (isRed(pixels.at(x, 94))) across.push(x)
      expect(across.length).toBeGreaterThan(20)
      expect(across.at(-1)).toBeLessThan(88)
      expect(rows(pixels, 94, isRed).at(-1)).toBeLessThan(88)
      expect(isRed(pixels.at(94, 94)), "the corner stays free").toBe(false)
    } finally {
      unmount()
    }
  })

  it("are not drawn for a container the wheel does not scroll", () => {
    for (const overflow of ["hidden", "clip"]) {
      const { renderer, id, shoot, unmount } = scroller({ overflow })
      try {
        renderer.scrollTo(id, 0, 50)
        expect(anyBar(shoot()), overflow).toBe(false)
      } finally {
        unmount()
      }
    }
  })
})

describeNative("overlay scrollbars under the pointer", () => {
  const redAcross = (pixels: ReturnType<typeof readLogicalPixels>, y: number) => {
    const found: number[] = []
    for (let x = 80; x < 100; x++) if (isRed(pixels.at(x, y))) found.push(x)
    return found
  }

  it("thicken over 150 ms instead of at once", () => {
    const { renderer, scrollTo, advance, shoot, unmount } = scroller()
    try {
      scrollTo(50)
      renderer.nativeSimulateMouseMove(95, 80)
      // The frame that notices the pointer starts the ease: still 7px, x 91 to 97.
      expect(redAcross(shoot(), 30)[0]).toBe(91)

      advance(75)
      // Halfway through an ease in and out is halfway: 9px, x 89 to 97.
      const halfway = redAcross(shoot(), 30)
      expect(halfway[0]).toBe(89)
      expect(halfway.at(-1)).toBe(97)

      advance(100)
      expect(redAcross(shoot(), 30)[0], "11px, x 87 to 97").toBe(87)
    } finally {
      unmount()
    }
  })

  it("thin out again over 150 ms when the pointer leaves", () => {
    const { renderer, scrollTo, advance, shoot, unmount } = scroller()
    try {
      scrollTo(50)
      renderer.nativeSimulateMouseMove(95, 80)
      shoot()
      advance(200)
      expect(redAcross(shoot(), 30)[0]).toBe(87)

      renderer.nativeSimulateMouseMove(20, 20)
      shoot()
      advance(75)
      expect(redAcross(shoot(), 30)[0]).toBe(89)
      advance(100)
      expect(redAcross(shoot(), 30)[0]).toBe(91)
    } finally {
      unmount()
    }
  })

  it("show the track as the thumb thickens", () => {
    const { renderer, scrollTo, advance, shoot, unmount } = scroller()
    try {
      scrollTo(50)
      renderer.nativeSimulateMouseMove(95, 80)
      expect(isBlue(shoot().at(88, 90)), "no track yet").toBe(false)
      advance(200)
      expect(isBlue(shoot().at(88, 90))).toBe(true)
    } finally {
      unmount()
    }
  })
})

describeNative("classic scrollbars", () => {
  const filler = <div style={{ width: "100%", height: 300, backgroundColor: "#202020" }} />

  function classic(style: StyleDesc = {}, content: ReactNode = filler) {
    const harness = scroller(style, {}, content)
    harness.renderer.setScrollbarStyle("classic")
    return harness
  }

  function client(harness: ReturnType<typeof scroller>) {
    const metrics = harness.renderer.getScrollMetrics(harness.id)!
    return { width: metrics[4], height: metrics[5], scrollWidth: metrics[2], scrollHeight: metrics[3] }
  }

  it("are drawn at rest, a track and a thumb, and never fade", () => {
    const harness = classic()
    try {
      harness.advance(5000)
      const pixels = harness.shoot()
      // A 15px gutter, x 85 to 100: the thumb is 9px of it, x 88 to 97. The track is the 85px the
      // horizontal bar leaves, and the thumb is 85 of 300 of it.
      expect(rows(pixels, 92, isRed).length).toBeGreaterThan(22)
      expect(rows(pixels, 92, isRed).length).toBeLessThan(27)
      expect(isRed(pixels.at(88, 10))).toBe(true)
      expect(isRed(pixels.at(87, 10))).toBe(false)
      expect(isBlue(pixels.at(87, 10)), "the track around the thumb").toBe(true)
      expect(isBlue(pixels.at(92, 70)), "the rest of the track").toBe(true)
      expect(isBlue(pixels.at(84, 70)), "the content ends at the gutter").toBe(false)
    } finally {
      harness.unmount()
    }
  })

  it("take their width out of the container's, so the content and clientWidth shrink", () => {
    const harness = classic()
    try {
      expect(client(harness)).toMatchObject({ width: 85, height: 85, scrollWidth: 85, scrollHeight: 300 })
      const bounds = harness.renderer.getElementBounds(harness.id)!
      expect(bounds.width, "the border box stays as styled").toBe(100)
    } finally {
      harness.unmount()
    }
  })

  it("leave the container alone in the overlay style, and give the room back when it returns", () => {
    const harness = scroller({}, {}, filler)
    try {
      expect(client(harness).width).toBe(100)
      harness.renderer.setScrollbarStyle("classic")
      expect(client(harness).width).toBe(85)
      harness.renderer.setScrollbarStyle("overlay")
      expect(client(harness).width).toBe(100)
    } finally {
      harness.unmount()
    }
  })

  it("size the content box a child's percentage resolves against", () => {
    const harness = classic({}, <div testId="half" style={{ width: "50%", height: 300 }} />)
    try {
      const half = harness.renderer.getElementBounds(harness.renderer.findByTestId("half")!.id)!
      // Half of the 85px content box, which layout rounds to whole pixels.
      expect(half.width).toBeGreaterThanOrEqual(42)
      expect(half.width).toBeLessThanOrEqual(43)
    } finally {
      harness.unmount()
    }
  })

  it("are 11px with scrollbarWidth thin", () => {
    const harness = classic({ scrollbarWidth: "thin" })
    try {
      expect(client(harness).width).toBe(89)
      const pixels = harness.shoot()
      // The thumb is 7px of the gutter: x 91 to 97.
      expect(isRed(pixels.at(91, 10))).toBe(true)
      expect(isRed(pixels.at(90, 10))).toBe(false)
      expect(isBlue(pixels.at(89, 10))).toBe(true)
    } finally {
      harness.unmount()
    }
  })

  it("take nothing with scrollbarWidth none, and the container still scrolls", () => {
    const harness = classic({ scrollbarWidth: "none" })
    try {
      expect(client(harness).width).toBe(100)
      expect(anyBar(harness.shoot())).toBe(false)
      harness.scrollTo(50)
      expect(harness.top()).toBe(50)
    } finally {
      harness.unmount()
    }
  })

  it("take room on both axes, and fill the corner", () => {
    const harness = classic({}, <div style={{ width: 300, height: 300, backgroundColor: "#202020" }} />)
    try {
      expect(client(harness)).toMatchObject({ width: 85, height: 85, scrollWidth: 300, scrollHeight: 300 })
      const pixels = harness.shoot()
      expect(isBlue(pixels.at(92, 92)), "the corner").toBe(true)
      expect(isRed(pixels.at(10, 92)), "the horizontal thumb").toBe(true)
      expect(isBlue(pixels.at(10, 84)), "the content ends above the gutter").toBe(false)
    } finally {
      harness.unmount()
    }
  })

  it("end the scrollable range at the scrollport that is left", () => {
    const harness = classic({}, <div style={{ width: 300, height: 300 }} />)
    try {
      harness.renderer.scrollTo(harness.id, 1000, 1000)
      expect(harness.renderer.getScrollOffset(harness.id)).toEqual([215, 215])
    } finally {
      harness.unmount()
    }
  })

  it("draw an empty track for overflow scroll when nothing scrolls, and take the room", () => {
    const harness = classic({}, <div style={{ width: "100%", height: 50 }} />)
    try {
      expect(client(harness).width).toBe(85)
      const pixels = harness.shoot()
      expect(rows(pixels, 92, isBlue).length).toBe(100)
      expect(rows(pixels, 92, isRed)).toEqual([])
    } finally {
      harness.unmount()
    }
  })

  it("take room for overflow auto only once there is something to scroll", () => {
    const short = classic({ overflow: "auto" }, <div style={{ width: "100%", height: 50 }} />)
    try {
      short.advance(100)
      expect(client(short).width).toBe(100)
      expect(anyBar(short.shoot())).toBe(false)
    } finally {
      short.unmount()
    }
    const tall = classic({ overflow: "auto" })
    try {
      tall.advance(100)
      expect(client(tall).width).toBe(85)
      expect(rows(tall.shoot(), 92, isBlue).length).toBeGreaterThan(60)
    } finally {
      tall.unmount()
    }
  })

  it("take room for overflow auto again when content grows, and give it back when it shrinks", () => {
    const root = createTestRoot()
    const app = (height: number) => (
      <div testId="scroller" style={{ width: 100, height: 100, overflow: "auto" }}>
        <div style={{ width: "100%", height }} />
      </div>
    )
    root.render(app(50))
    root.renderer.setScrollbarStyle("classic")
    try {
      const id = root.renderer.findByTestId("scroller")!.id
      const width = () => {
        root.renderer.flush()
        root.renderer.clockFastForward(100)
        root.renderer.flush()
        return root.renderer.getScrollMetrics(id)![4]
      }
      expect(width()).toBe(100)
      root.render(app(300))
      expect(width()).toBe(85)
      root.render(app(50))
      expect(width()).toBe(100)
    } finally {
      root.unmount()
    }
  })

  it("take no room on an axis that only code scrolls", () => {
    const harness = classic(
      { overflow: undefined, overflowX: "hidden", overflowY: "scroll" },
      <div style={{ width: "100%", height: 300 }} />
    )
    try {
      expect(client(harness)).toMatchObject({ width: 85, height: 100 })
    } finally {
      harness.unmount()
    }
  })

  it("do not thicken under the pointer", () => {
    const harness = classic()
    try {
      harness.renderer.nativeSimulateMouseMove(92, 80)
      harness.advance(300)
      const pixels = harness.shoot()
      expect(isRed(pixels.at(88, 10))).toBe(true)
      expect(isRed(pixels.at(87, 10))).toBe(false)
    } finally {
      harness.unmount()
    }
  })

  it("drag the thumb and page by the track, as an overlay bar does", () => {
    const harness = classic()
    try {
      const { renderer, top } = harness
      renderer.nativeSimulateMouseMove(92, 10)
      renderer.nativeSimulateMouseDown(92, 10)
      renderer.nativeSimulateMouseMove(92, 43)
      renderer.nativeSimulateMouseUp(92, 43)
      // The thumb is 24px of an 85px track, so 33px of travel is 33/61 of the 215px range.
      expect(top()).toBeGreaterThan(114)
      expect(top()).toBeLessThan(119)
      const before = top()
      renderer.nativeSimulateMouseMove(92, 80)
      renderer.nativeSimulateMouseDown(92, 80)
      renderer.nativeSimulateMouseUp(92, 80)
      // 76.5 from a whole pixel lands on a half, which a scroll position rounds up.
      expect(top() - before, "down a page, 90% of the 85px scrollport").toBe(77)
    } finally {
      harness.unmount()
    }
  })

  it("scroll into view against the scrollport, not under the bar", () => {
    const harness = classic(
      {},
      <div style={{ width: 300, height: 300 }}>
        <div testId="target" style={{ marginLeft: 270, width: 30, height: 10 }} />
      </div>
    )
    try {
      const target = harness.renderer.findByTestId("target")!.id
      harness.renderer.scrollIntoView(target, "nearest", "nearest")
      // The target ends at 300, so the 85px scrollport has to reach 300.
      expect(harness.renderer.getScrollOffset(harness.id)![0]).toBe(215)
    } finally {
      harness.unmount()
    }
  })
})

describeNative("scrollbar pointer input", () => {
  it("drags the thumb, which scrolls and fires onScroll", () => {
    const tops: number[] = []
    const { renderer, scrollTo, top, unmount } = scroller({}, { onScroll: (value) => tops.push(value) })
    try {
      scrollTo(50)
      // The thumb runs from about y 17 to 50. Take it at 30 and pull it 33px down.
      renderer.nativeSimulateMouseMove(94, 30)
      renderer.nativeSimulateMouseDown(94, 30)
      renderer.nativeSimulateMouseMove(94, 63)
      renderer.nativeSimulateMouseUp(94, 63)
      expect(top()).toBeGreaterThan(146)
      expect(top()).toBeLessThan(152)
      expect(Math.abs(tops.at(-1)! - top())).toBeLessThan(1)
    } finally {
      unmount()
    }
  })

  it("keeps dragging when the pointer leaves the bar, and stops at the limit", () => {
    const { renderer, scrollTo, top, unmount } = scroller()
    try {
      scrollTo(50)
      renderer.nativeSimulateMouseMove(94, 30)
      renderer.nativeSimulateMouseDown(94, 30)
      renderer.nativeSimulateMouseMove(30, 500)
      expect(top()).toBe(200)
      renderer.nativeSimulateMouseMove(30, -500)
      expect(top()).toBe(0)
      renderer.nativeSimulateMouseUp(30, -500)
      renderer.nativeSimulateMouseMove(30, 90)
      expect(top(), "released").toBe(0)
    } finally {
      unmount()
    }
  })

  it("pages toward the pointer when the track is pressed", () => {
    const { renderer, scrollTo, top, unmount } = scroller()
    try {
      scrollTo(50)
      renderer.nativeSimulateMouseMove(94, 90)
      renderer.nativeSimulateMouseDown(94, 90)
      renderer.nativeSimulateMouseUp(94, 90)
      expect(top(), "down a page, 90% of the viewport").toBe(140)

      renderer.nativeSimulateMouseMove(94, 5)
      renderer.nativeSimulateMouseDown(94, 5)
      renderer.nativeSimulateMouseUp(94, 5)
      expect(top(), "back up a page").toBe(50)
    } finally {
      unmount()
    }
  })

  it("blocks content only where a visible bar is", () => {
    let presses = 0
    const { renderer, scrollTo, advance, unmount } = scroller({}, { onContentMouseDown: () => presses++ })
    try {
      // Nothing shows at rest, so a press at the edge reaches the content.
      renderer.nativeSimulateMouseDown(94, 90)
      renderer.nativeSimulateMouseUp(94, 90)
      expect(presses).toBe(1)

      scrollTo(50)
      renderer.nativeSimulateMouseMove(94, 90)
      renderer.nativeSimulateMouseDown(94, 90)
      renderer.nativeSimulateMouseUp(94, 90)
      expect(presses, "the bar took it").toBe(1)

      renderer.nativeSimulateMouseDown(40, 50)
      renderer.nativeSimulateMouseUp(40, 50)
      expect(presses, "the rest of the content is not blocked").toBe(2)

      renderer.nativeSimulateMouseMove(20, 20)
      advance(3000)
      renderer.nativeSimulateMouseDown(94, 90)
      renderer.nativeSimulateMouseUp(94, 90)
      expect(presses, "the bar is gone again").toBe(3)
    } finally {
      unmount()
    }
  })
})

describeNative("scrollbars on a <virtual-list>", () => {
  /** 30 rows of 40px in a 160px list: 1200px of content. */
  function list(style: StyleDesc = {}) {
    const root = createTestRoot()
    root.render(
      <virtual-list
        testId="list"
        overdraw={0}
        estimatedItemHeight={40}
        style={{ width: 100, height: 160, backgroundColor: "#202020", scrollbarColor: COLORS, ...style }}
      >
        {Array.from({ length: 30 }, (_, index) => (
          <div key={index} style={{ height: 40, flexShrink: 0 }} />
        ))}
      </virtual-list>
    )
    root.renderer.clockPause()
    return root
  }

  function column(renderer: ReturnType<typeof createTestRoot>["renderer"], name: string) {
    renderer.flush()
    const file = path.join(SHOTS_DIR, `scrollbar-${process.pid}-${name}.png`)
    renderer.captureScreenshot(file)
    const pixels = readLogicalPixels(file, renderer)
    const found: number[] = []
    for (let y = 0; y < 160; y++) if (isRed(pixels.at(94, y))) found.push(y)
    return found
  }

  it("is hidden at rest and shows when the list scrolls", () => {
    const { renderer, unmount } = list()
    try {
      expect(column(renderer, "list-rest")).toEqual([])
      renderer.nativeSimulateScrollWheel(50, 50, 0, 60)
      const found = column(renderer, "list-scrolled")
      // 160 of 1200: about 21px of the track.
      expect(found.length).toBeGreaterThanOrEqual(19)
      expect(found.length).toBeLessThanOrEqual(23)
      expect(found[0]).toBeGreaterThan(0)
    } finally {
      unmount()
    }
  })

  it("moves the thumb down as the list scrolls, and sizes unmeasured rows from measured ones", () => {
    const { renderer, id, unmount } = (() => {
      const root = createTestRoot()
      // 20 rows of 200px in a 400px list is 4000px, so the thumb is 40px. With only the 40px
      // estimate for the rows out of view it would be 143px.
      root.render(
        <virtual-list
          testId="list"
          overdraw={0}
          estimatedItemHeight={40}
          style={{ width: 100, height: 400, backgroundColor: "#202020", scrollbarColor: COLORS }}
        >
          {Array.from({ length: 20 }, (_, index) => (
            <div key={index} style={{ height: 200, flexShrink: 0 }} />
          ))}
        </virtual-list>
      )
      root.renderer.clockPause()
      return { ...root, id: root.renderer.findByTestId("list")!.id }
    })()
    try {
      renderer.scrollToItem(id, 1)
      renderer.flush()
      const file = path.join(SHOTS_DIR, `scrollbar-${process.pid}-list-tall.png`)
      renderer.captureScreenshot(file)
      const pixels = readLogicalPixels(file, renderer)
      const found: number[] = []
      for (let y = 0; y < 400; y++) if (isRed(pixels.at(94, y))) found.push(y)
      expect(found.length).toBeGreaterThanOrEqual(38)
      expect(found.length).toBeLessThanOrEqual(42)
    } finally {
      unmount()
    }
  })

  it("draws nothing with scrollbarWidth none", () => {
    const { renderer, unmount } = list({ scrollbarWidth: "none" })
    try {
      renderer.nativeSimulateScrollWheel(50, 50, 0, 60)
      expect(column(renderer, "list-none")).toEqual([])
    } finally {
      unmount()
    }
  })

  it("drags the thumb to scroll the list", () => {
    const { renderer, unmount } = list()
    try {
      const id = renderer.findByTestId("list")!.id
      renderer.nativeSimulateScrollWheel(50, 50, 0, 60)
      renderer.nativeSimulateMouseMove(94, 20)
      renderer.nativeSimulateMouseDown(94, 20)
      renderer.nativeSimulateMouseMove(94, 100)
      renderer.nativeSimulateMouseUp(94, 100)
      expect(renderer.getScrollOffset(id)![1]).toBeGreaterThan(300)
    } finally {
      unmount()
    }
  })
})
