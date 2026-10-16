import path from "path"
import { describe, expect, it } from "vitest"
import type { ReactNode } from "react"
import type { StyleDesc } from "@gpuix/native/host"
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
    top: () => 0 - renderer.getScrollOffset(id)![1],
    scrollTo: (top: number) => renderer.scrollTo(id, 0, -top),
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
      renderer.scrollTo(id, -50, -50)
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
        renderer.scrollTo(id, 0, -50)
        expect(anyBar(shoot()), overflow).toBe(false)
      } finally {
        unmount()
      }
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
      renderer.nativeSimulateMouseMove(94, 63, 0)
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
      renderer.nativeSimulateMouseMove(30, 500, 0)
      expect(top()).toBe(200)
      renderer.nativeSimulateMouseMove(30, -500, 0)
      expect(top()).toBe(0)
      renderer.nativeSimulateMouseUp(30, -500)
      renderer.nativeSimulateMouseMove(30, 90, 0)
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
      renderer.nativeSimulateMouseMove(94, 100, 0)
      renderer.nativeSimulateMouseUp(94, 100)
      expect(0 - renderer.getScrollOffset(id)![1]).toBeGreaterThan(300)
    } finally {
      unmount()
    }
  })
})
