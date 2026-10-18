/// An inline-block, inline-flex or inline-grid box is an element like any other: the pointer, the focus,
/// bounds and observers, scrolling, selection and search, motion and display all treat it as a box of its
/// own, although the paragraph around it lays it out. The font is GpuixTestSquare at 20px (see
/// inline-block.test.tsx): a character is a 20px square.

import fs from "fs"
import React from "react"
import { describe, expect, it } from "vitest"
import type { IntersectionObserverEntry, PublicInstance, ResizeObserverEntry } from "../index"
import { IntersectionObserver, motion, ResizeObserver } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const SQUARE_FONT = fs.readFileSync(new URL("./fixtures/gpuix-test-square.ttf", import.meta.url))
const TEXT = { fontFamily: "GpuixTestSquare", fontSize: 20, lineHeight: "20px", color: "#000000" } as const

const box = (width: number, height: number) => ({ display: "inline-block", width, height }) as const

type TestRoot = ReturnType<typeof createTestRoot>

function setup(node: React.ReactNode, options = {}) {
  const root = createTestRoot({ width: 300, height: 200, ...options })
  root.renderer.registerFont(SQUARE_FONT)
  root.render(node)
  const { renderer } = root
  const idOf = (testId: string) => {
    const element = renderer.findByTestId(testId)
    expect(element, `no element ${testId}`).toBeDefined()
    return element!.id
  }
  const bounds = (testId: string) => {
    const result = renderer.getElementBounds(idOf(testId))
    expect(result, `${testId} has no bounds`).not.toBeNull()
    return result!
  }
  const center = (testId: string) => {
    const b = bounds(testId)
    return { x: b.x + b.width / 2, y: b.y + b.height / 2 }
  }
  return { ...root, renderer, idOf, bounds, center }
}

type Rgba = [number, number, number, number]

function pixel(root: TestRoot, name: string, x: number, y: number): Rgba {
  const file = `${SHOTS_DIR}/inline-block-${name}.png`
  fs.rmSync(file, { force: true })
  root.renderer.captureScreenshot(file)
  return readLogicalPixels(file, root.renderer).at(x, y) as Rgba
}

const RED: Rgba = [255, 0, 0, 255]
const BLUE: Rgba = [0, 0, 255, 255]
const WHITE: Rgba = [255, 255, 255, 255]

describeNative("the pointer on an inline-block", () => {
  /** AA [ib: inner] AA, with a listener on every box. */
  function page(log: string[]) {
    const on = (name: string) => ({
      onClick: () => log.push(`${name}:click`),
      onMouseEnter: () => log.push(`${name}:enter`),
      onMouseLeave: () => log.push(`${name}:leave`),
    })
    return (
      <div testId="root" style={{ width: 300, ...TEXT }} {...on("root")}>
        AA
        <span testId="around" {...on("around")}>
          <div testId="ib" style={{ ...box(40, 30), backgroundColor: "#ff0000" }} {...on("ib")}>
            <div testId="inner" style={{ width: 20, height: 10 }} {...on("inner")} />
          </div>
        </span>
        AA
      </div>
    )
  }

  it("clicks the box under the pointer, and the click bubbles through the span and the container", () => {
    const log: string[] = []
    const { renderer, center, unmount } = setup(page(log))
    try {
      const at = center("inner")
      renderer.nativeSimulateClick(at.x, at.y)
      expect(log).toEqual(["root:enter", "around:enter", "ib:enter", "inner:enter", "inner:click", "ib:click", "around:click", "root:click"])
    } finally {
      unmount()
    }
  })

  it("is hovered as the box it is: entering it enters the span around it, leaving it leaves them", () => {
    const log: string[] = []
    const { renderer, center, bounds, unmount } = setup(page(log))
    try {
      const ib = bounds("ib")
      renderer.nativeSimulateMouseMove(ib.x + ib.width - 2, ib.y + ib.height - 2)
      expect(log).toEqual(["root:enter", "around:enter", "ib:enter"])
      log.length = 0
      // Off the box but on the text: the span around it is left only when the pointer leaves the span.
      renderer.nativeSimulateMouseMove(5, 5)
      expect(log).toEqual(["ib:leave", "around:leave"])
      log.length = 0
      renderer.nativeSimulateMouseMove(center("inner").x, center("inner").y)
      expect(log).toEqual(["around:enter", "ib:enter", "inner:enter"])
    } finally {
      unmount()
    }
  })

  it("changes its colour on hover and paints where it sits, above the text and not at the origin", () => {
    const { renderer, center, bounds, unmount, ...root } = setup(
      <div style={{ width: 300, ...TEXT }}>
        AAA
        <div
          testId="ib"
          style={{ ...box(40, 30), backgroundColor: "#ff0000", hover: { backgroundColor: "#0000ff" } }}
        />
      </div>
    )
    try {
      const ib = bounds("ib")
      expect(ib.x).toBeCloseTo(60, 0)
      const inside = { x: ib.x + 20, y: ib.y + 15 }
      const shots = { renderer, ...root } as unknown as TestRoot
      expect(pixel(shots, "hover-off", inside.x, inside.y)).toEqual(RED)
      expect(pixel(shots, "hover-off", 5, 5), "the origin is the first character's, not the box's").not.toEqual(RED)
      renderer.nativeSimulateMouseMove(inside.x, inside.y)
      expect(pixel(shots, "hover-on", inside.x, inside.y)).toEqual(BLUE)
      renderer.nativeSimulateMouseMove(5, 5)
      expect(pixel(shots, "hover-gone", inside.x, inside.y)).toEqual(RED)
    } finally {
      unmount()
    }
  })

  it("is hit where it is painted: the topmost of two overlapping boxes takes the pointer, a z-index lifts one", () => {
    const { renderer, bounds, unmount } = setup(
      <div style={{ width: 300, ...TEXT }}>
        <div testId="first" style={{ ...box(40, 20), marginRight: -20, position: "relative", zIndex: 2 }} />
        <div testId="second" style={{ ...box(40, 20), backgroundColor: "#ff0000" }} />
      </div>
    )
    try {
      const first = bounds("first")
      const second = bounds("second")
      expect(second.x).toBeCloseTo(first.x + 20, 0)
      const overlap = { x: first.x + 30, y: first.y + 10 }
      const ids = renderer.getElementsAt(overlap.x, overlap.y)
      expect(ids[0], "the lifted one is on top").toBe(renderer.findByTestId("first")!.id)
      expect(ids).toContain(renderer.findByTestId("second")!.id)
    } finally {
      unmount()
    }
  })
})

describeNative("painting an inline-block", () => {
  it("is not under the underline of the container, which runs under the text only", () => {
    const { bounds, renderer, unmount } = setup(
      <div style={{ width: 300, ...TEXT, lineHeight: "30px", textDecoration: "underline", backgroundColor: "#ffffff" }}>
        AA
        <div testId="ib" style={box(40, 20)} />
        AA
      </div>
    )
    try {
      const file = `${SHOTS_DIR}/inline-block-underline.png`
      fs.rmSync(file, { force: true })
      renderer.captureScreenshot(file)
      const pixels = readLogicalPixels(file, renderer)
      const ib = bounds("ib")
      const column = (x: number) => {
        const found: number[] = []
        for (let y = 0; y < 30; y++) {
          const [r, g, b] = pixels.at(x, y)
          if (r < 128 && g < 128 && b < 128) found.push(y)
        }
        return found
      }
      // The glyphs are squares of black, so a column through "AA" is black from the top of the em to
      // the underline below it; one through the box has nothing in it.
      expect(column(10).length, "the text and its underline").toBeGreaterThan(20)
      expect(column(ib.x + 20), "nothing under or in the box").toEqual([])
      expect(column(ib.x + ib.width + 10).length).toBeGreaterThan(20)
    } finally {
      unmount()
    }
  })
})

describeNative("the text style of an inline-block", () => {
  it("is inherited from the boxes around it, as a child's is", () => {
    const { bounds, renderer, unmount } = setup(
      <div style={{ width: 300, ...TEXT, backgroundColor: "#ffffff" }}>
        <span style={{ fontSize: 10, color: "#ff0000", lineHeight: "10px" }}>
          <div testId="ib" style={{ display: "inline-block" }}>
            AA
          </div>
        </span>
      </div>
    )
    try {
      const ib = bounds("ib")
      // Two 10px characters.
      expect(ib.width).toBeGreaterThanOrEqual(19.5)
      expect(ib.width).toBeLessThanOrEqual(21.5)
      expect(ib.height).toBeCloseTo(10, 0)
      const file = `${SHOTS_DIR}/inline-block-inherit.png`
      fs.rmSync(file, { force: true })
      renderer.captureScreenshot(file)
      expect(readLogicalPixels(file, renderer).at(ib.x + 10, ib.y + 5)).toEqual(RED)
    } finally {
      unmount()
    }
  })

  /** The x of the dark pixels in the rows where an underline of a 20px Menlo in a 30px line is, from
   *  `from` to `to`. The baseline is 22px down and "A" has nothing below it. An auto underline
   *  starts half its 2px thickness under the baseline, so it is in rows 23 and 24. */
  function underlined(tree: React.ReactNode, ranges: (box: { x: number; width: number }) => [number, number][]) {
    const { bounds, renderer, unmount } = setup(tree)
    try {
      const file = `${SHOTS_DIR}/inline-block-underline-inside.png`
      fs.rmSync(file, { force: true })
      renderer.captureScreenshot(file)
      const pixels = readLogicalPixels(file, renderer)
      return ranges(bounds("ib")).map(([from, to]) => {
        let count = 0
        for (let x = Math.floor(from); x < to; x++) {
          for (let y = 23; y < 30; y++) {
            const [r, g, b] = pixels.at(x, y)
            if (r < 128 && g < 128 && b < 128) count++
          }
        }
        return count
      })
    } finally {
      unmount()
    }
  }

  const MENLO = { width: 300, ...TEXT, fontFamily: "Menlo", lineHeight: "30px", backgroundColor: "#ffffff" } as const
  const inner = (
    <div testId="ib" style={{ display: "inline-block", lineHeight: "30px" }}>
      AA
    </div>
  )

  it("has no underline of the container", () => {
    const [text, box] = underlined(
      <div style={{ ...MENLO, textDecoration: "underline" }}>
        AA
        {inner}
      </div>,
      (ib) => [
        [0, 20],
        [ib.x, ib.x + ib.width],
      ]
    )
    expect(text, "the text of the container is underlined").toBeGreaterThan(0)
    expect(box, "the text in the box is not").toBe(0)
  })

  it("has no underline of a span around it", () => {
    const [text, box] = underlined(
      <div style={MENLO}>
        <span style={{ textDecoration: "underline" }}>
          AA
          {inner}
        </span>
      </div>,
      (ib) => [
        [0, 20],
        [ib.x, ib.x + ib.width],
      ]
    )
    expect(text, "the text of the span is underlined").toBeGreaterThan(0)
    expect(box, "the text in the box is not").toBe(0)
  })

  it("has the underline it declares itself", () => {
    const [box] = underlined(
      <div style={{ ...MENLO, textDecoration: "underline" }}>
        AA
        <div testId="ib" style={{ display: "inline-block", lineHeight: "30px", textDecoration: "underline" }}>
          AA
        </div>
      </div>,
      (ib) => [[ib.x, ib.x + ib.width]]
    )
    expect(box).toBeGreaterThan(0)
  })
})

describeNative("focus on an inline-block", () => {
  it("is focused by a click and by Tab, in the order of the page, and blurred as any element", () => {
    const log: string[] = []
    const { renderer, idOf, center, unmount } = setup(
      <div style={{ width: 300, ...TEXT }}>
        AA
        <div
          testId="one"
          tabIndex={0}
          style={box(40, 20)}
          onFocus={() => log.push("one:focus")}
          onBlur={() => log.push("one:blur")}
        />
        AA
        <div testId="two" tabIndex={0} style={box(40, 20)} onFocus={() => log.push("two:focus")} />
      </div>
    )
    try {
      const focused = () => renderer.getElement(renderer.getFocusedElementId())?.testId ?? null
      renderer.simulateKeystrokes("Tab")
      expect(focused()).toBe("one")
      renderer.simulateKeystrokes("Tab")
      expect(focused()).toBe("two")
      const at = center("one")
      renderer.nativeSimulateClick(at.x, at.y)
      expect(focused()).toBe("one")
      renderer.blurElement(idOf("one"))
      expect(focused()).toBe(null)
      renderer.dispatchNativeEvents()
      expect(log).toEqual(["one:focus", "one:blur", "two:focus", "one:focus", "one:blur"])
    } finally {
      unmount()
    }
  })
})

describeNative("bounds and observers of an inline-block", () => {
  it("reports its box to onLayout and to a ResizeObserver, and a change of its size", () => {
    const layouts: { width: number; height: number; x: number; y: number }[] = []
    const entries: ResizeObserverEntry[][] = []
    let target: PublicInstance | null = null
    const view = (width: number) => (
      <div style={{ width: 300, ...TEXT }}>
        AA
        <div
          ref={(element) => (target = element)}
          testId="ib"
          style={{ ...box(width, 30), paddingLeft: 3, borderLeftWidth: 2, borderStyle: "solid", borderRightWidth: 0, borderTopWidth: 0, borderBottomWidth: 0 }}
          onLayout={(event) => layouts.push({ width: event.width, height: event.height, x: event.x, y: event.y })}
        />
      </div>
    )
    const { renderer, render, bounds, unmount } = setup(view(40))
    try {
      const observer = new ResizeObserver((batch) => entries.push(batch))
      observer.observe(target!)
      renderer.dispatchNativeEvents()
      expect(layouts.at(-1)).toMatchObject({ width: 40, height: 30, x: 40, y: 0 })
      expect(entries.at(-1)![0]!.borderBoxSize).toEqual([{ inlineSize: 40, blockSize: 30 }])
      expect(entries.at(-1)![0]!.contentBoxSize).toEqual([{ inlineSize: 35, blockSize: 30 }])

      render(view(70))
      renderer.flush()
      renderer.dispatchNativeEvents()
      expect(bounds("ib").width).toBeCloseTo(70, 1)
      expect(layouts.at(-1)).toMatchObject({ width: 70, height: 30 })
      expect(entries.at(-1)![0]!.borderBoxSize).toEqual([{ inlineSize: 70, blockSize: 30 }])
      expect(target!.getBoundingClientRect().width).toBeCloseTo(70, 1)
    } finally {
      unmount()
    }
  })

  it("is intersecting a scroller only while it is scrolled into view", () => {
    const seen: boolean[] = []
    let target: PublicInstance | null = null
    const { renderer, unmount } = setup(
      <div style={{ width: 300, height: 100, overflow: "auto", ...TEXT }}>
        {Array.from({ length: 12 }, (_, index) => (
          <div
            key={index}
            ref={index === 11 ? (element) => (target = element) : undefined}
            style={{ ...box(140, 40), backgroundColor: "#0000ff" }}
          />
        ))}
      </div>
    )
    try {
      const observer = new IntersectionObserver(
        (entries: IntersectionObserverEntry[]) => seen.push(...entries.map((entry) => entry.isIntersecting)),
        { root: null }
      )
      observer.observe(target!)
      renderer.dispatchNativeEvents()
      expect(seen.at(-1)).toBe(false)
      target!.scrollIntoView({ block: "end" })
      renderer.flush()
      renderer.dispatchNativeEvents()
      expect(seen.at(-1)).toBe(true)
    } finally {
      unmount()
    }
  })

  it("is scrolled into view by scrollIntoView", () => {
    let target: PublicInstance | null = null
    const { renderer, idOf, unmount } = setup(
      <div testId="scroller" style={{ width: 300, height: 100, overflow: "auto", ...TEXT }}>
        {Array.from({ length: 12 }, (_, index) => (
          <div
            key={index}
            ref={index === 11 ? (element) => (target = element) : undefined}
            testId={`ib${index}`}
            style={{ ...box(140, 40), backgroundColor: "#0000ff" }}
          />
        ))}
      </div>
    )
    try {
      expect(renderer.getElementBounds(idOf("ib11"))!.y).toBeGreaterThan(100)
      target!.scrollIntoView({ block: "end" })
      renderer.flush()
      const scroller = renderer.getElementBounds(idOf("scroller"))!
      const ib = renderer.getElementBounds(idOf("ib11"))!
      expect(ib.y + ib.height).toBeLessThanOrEqual(scroller.y + scroller.height + 0.5)
      expect(ib.y).toBeGreaterThanOrEqual(scroller.y - 0.5)
    } finally {
      unmount()
    }
  })
})

describeNative("text inside an inline-block", () => {
  // GpuixTestSquare has "A" and "m" one em wide. It has no space: Helvetica Neue's, 569/2048 em, is used.
  const SPACE = (569 / 2048) * 20
  const page = (query: string) => (
    <div testId="root" style={{ width: 300, ...TEXT }} highlight={{ query }}>
      AA{" "}
      <div testId="ib" style={{ display: "inline-block" }}>
        mm
      </div>{" "}
      AAA
    </div>
  )

  it("is found by a search and washed where it is painted, and so is the text around it", () => {
    const { renderer, bounds, unmount } = setup(page("mm"))
    try {
      const washes = renderer.getPaintedHighlights()
      expect(washes).toHaveLength(1)
      const ib = bounds("ib")
      const wash = washes[0]!.rects[0]!
      expect(wash.x).toBeCloseTo(ib.x, 0)
      expect(wash.width).toBeCloseTo(40, 0)
      expect(wash.y).toBeCloseTo(ib.y, 0)
    } finally {
      unmount()
    }
    const second = setup(page("AAA"))
    try {
      const washes = second.renderer.getPaintedHighlights()
      expect(washes).toHaveLength(1)
      // "AA", a space (another font's), the 40px box and a space.
      expect(washes[0]!.rects[0]!.x).toBeCloseTo(40 + SPACE + 40 + SPACE, 1)
      expect(washes[0]!.rects[0]!.width).toBeCloseTo(60, 0)
    } finally {
      second.unmount()
    }
  })

  it("finds the matches of the text before the box, the box's own and the text after it, each where it is painted", () => {
    const { renderer, unmount } = setup(
      <div style={{ width: 300, ...TEXT }} highlight={{ query: "m" }}>
        mA{" "}
        <div style={{ display: "inline-block" }}>m</div>{" "}
        Am
      </div>
    )
    try {
      const washes = renderer.getPaintedHighlights()
      expect(washes).toHaveLength(3)
      // "m", then "A" and a space (another font's), then the 20px box with the second match, a
      // space, "A" and the third. The paragraph paints before the box, so the order here is paint order.
      const xs = washes.map((wash) => wash.rects[0]!.x).sort((a, b) => a - b)
      expect(xs[0]).toBeCloseTo(0, 0)
      // The box is painted on a device pixel, and so is the match in it.
      expect(Math.abs(xs[1]! - (40 + SPACE))).toBeLessThanOrEqual(0.5)
      expect(xs[2]).toBeCloseTo(40 + SPACE + 20 + SPACE + 20, 1)
    } finally {
      unmount()
    }
  })

  it("is copied without the placeholder when a selection runs across the box", () => {
    const { renderer, unmount } = setup(
      <div style={{ width: 300, ...TEXT }}>
        AA{" "}
        <div style={{ display: "inline-block" }}>mm</div>{" "}
        AAA
      </div>
    )
    try {
      renderer.dragSelect(1, 10, 299, 10)
      const text = renderer.getSelectedText()
      expect(text).not.toBeNull()
      expect(text).not.toContain("\uFFFC")
      expect(text).toContain("AA")
      expect(text).toContain("AAA")
    } finally {
      unmount()
    }
  })
})

describeNative("an inline-block that changes", () => {
  it("transitions its width and moves the text after it with it", () => {
    const view = (width: number) => (
      <div style={{ width: 300, ...TEXT }}>
        A
        <div testId="ib" style={{ ...box(width, 20), transition: "width 1s linear" }} />
        <span testId="after">A</span>
      </div>
    )
    const { renderer, render, bounds, unmount } = setup(view(20))
    try {
      renderer.clockPause()
      render(view(20))
      renderer.dispatchNativeEvents()
      const x0 = bounds("after").x
      render(view(100))
      renderer.clockFastForward(500)
      renderer.dispatchNativeEvents()
      expect(bounds("ib").width).toBeCloseTo(60, 0)
      expect(bounds("after").x - x0).toBeCloseTo(40, 0)
      renderer.clockFastForward(1000)
      renderer.dispatchNativeEvents()
      expect(bounds("ib").width).toBeCloseTo(100, 1)
    } finally {
      unmount()
    }
  })

  it("animates its margin with motion, and the text after it follows", () => {
    const { renderer, render, bounds, unmount } = setup(<div />)
    try {
      renderer.clockPause()
      render(
        <div style={{ width: 300, ...TEXT }}>
          A
          <motion.div
            testId="ib"
            style={box(20, 20)}
            initial={{ marginLeft: 0 }}
            animate={{ marginLeft: 40 }}
            transition={{ duration: 1, ease: "linear" }}
          />
          <span testId="after">A</span>
        </div>
      )
      renderer.dispatchNativeEvents()
      expect(bounds("ib").x).toBeCloseTo(20, 0)
      const x0 = bounds("after").x
      renderer.clockFastForward(500)
      renderer.dispatchNativeEvents()
      expect(bounds("ib").x).toBeCloseTo(40, 0)
      expect(bounds("after").x - x0).toBeCloseTo(20, 0)
      renderer.clockFastForward(1000)
      renderer.dispatchNativeEvents()
      expect(bounds("ib").x).toBeCloseTo(60, 0)
      expect(bounds("after").x - x0).toBeCloseTo(40, 0)
    } finally {
      unmount()
    }
  })

  it("disappears with display none and comes back, and the text closes up and opens again", () => {
    const view = (display: string) => (
      <div style={{ width: 300, ...TEXT }}>
        A
        <div testId="ib" style={{ ...box(40, 20), display }} />
        <span testId="after">A</span>
      </div>
    )
    const { renderer, render, bounds, unmount } = setup(view("inline-block"))
    try {
      const open = bounds("after").x
      expect(open).toBeCloseTo(60, 0)
      render(view("none"))
      renderer.flush()
      expect(bounds("after").x).toBeCloseTo(20, 0)
      render(view("inline-block"))
      renderer.flush()
      expect(bounds("after").x).toBeCloseTo(open, 0)
      render(view("block"))
      renderer.flush()
      expect(bounds("ib").x, "a block is on a line of its own").toBeCloseTo(0, 0)
      expect(bounds("after").y).toBeGreaterThan(bounds("ib").y)
    } finally {
      unmount()
    }
  })

  it("follows the order of the page when the boxes are reordered", () => {
    const view = (order: string[]) => (
      <div style={{ width: 300, ...TEXT }}>
        {order.map((name) => (
          <div key={name} testId={name} style={box(name === "a" ? 30 : 50, 20)} />
        ))}
      </div>
    )
    const { renderer, render, bounds, unmount } = setup(view(["a", "b"]))
    try {
      expect(bounds("a").x).toBeCloseTo(0, 0)
      expect(bounds("b").x).toBeCloseTo(30, 0)
      render(view(["b", "a"]))
      renderer.flush()
      expect(bounds("b").x).toBeCloseTo(0, 0)
      expect(bounds("a").x).toBeCloseTo(50, 0)
    } finally {
      unmount()
    }
  })
})
