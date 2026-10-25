// @ts-nocheck
/// `var()` in the transform properties (`translateX(calc(100% * var(--p)))`, a carousel's track), read from the custom
/// properties the element computes to: a `--p` that JS sets on an ancestor each frame moves the track like the literal
/// `translateX` does. `fixtures/transform-geometry-oracle.json` (scene `var`, in `transform-geometry.test.tsx`) pins the
/// same substitution to Chrome.

import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { verifying } from "./retained-harness.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const SLOW = 120_000

type Renderer = ReturnType<typeof createTestRoot>["renderer"]

function box(renderer: Renderer, testId: string): [number, number, number, number] {
  const element = renderer.findByTestId(testId)
  expect(element, `${testId} is mounted`).toBeDefined()
  const bounds = renderer.getElementBounds(element!.id)
  expect(bounds, `${testId} has a box`).not.toBeNull()
  return [bounds!.x, bounds!.y, bounds!.width, bounds!.height]
}

function mounted(tree: React.ReactElement, run: (renderer: Renderer, root: ReturnType<typeof createTestRoot>) => void) {
  const root = createTestRoot({ width: 800, height: 600 })
  try {
    root.render(tree)
    root.renderer.flush()
    run(root.renderer, root)
  } finally {
    root.unmount()
  }
}

/** A viewport holding a track 300 wide, once translated by `var(--p)` and once by the literal it comes to. */
const across = (renderer: Renderer, testId: string) => box(renderer, testId).filter((_, index) => index !== 1)

function Carousel({ p, custom }: { p: number | string; custom?: Record<string, unknown> }) {
  const fraction = Number(p)
  return (
    <div style={{ position: "absolute", left: 20, top: 20, width: 400, height: 200, overflow: "hidden", "--p": p, ...custom }}>
      <div testId="track" style={{ width: 300, height: 60, transform: "translateX(calc(-100% * var(--p)))" }} />
      <div testId="literal" style={{ width: 300, height: 60, transform: `translateX(${-fraction * 100}%)` }} />
    </div>
  )
}

describeNative("var() in a transform", () => {
  it("moves a carousel's track like the literal translate, for a number or a string set from JS", () => {
    mounted(<Carousel p={0} />, (renderer, root) => {
      expect(across(renderer, "track")).toEqual(across(renderer, "literal"))
      for (const p of [0.4, "0.75", 1, 0.125, 0]) {
        root.render(<Carousel p={p} />)
        renderer.flush()
        renderer.drawFrame()
        expect(across(renderer, "track")).toEqual(across(renderer, "literal"))
        expect(box(renderer, "track")[0]).toBeCloseTo(20 - Number(p) * 300, 3)
      }
    })
  }, SLOW)

  it("reads the nearest declaration, the element's own included, and the fallback of one that is missing", () => {
    mounted(
      <div style={{ position: "absolute", left: 0, top: 0, width: 800, height: 600, "--x": "10px" }}>
        <div testId="inherits" style={{ width: 50, height: 50, translate: "var(--x) 0" }} />
        <div style={{ "--x": "30px" }}>
          <div testId="overrides" style={{ width: 50, height: 50, translate: "var(--x) 0" }} />
        </div>
        <div testId="own" style={{ width: 50, height: 50, "--x": "40px", translate: "var(--x) 0" }} />
        <div testId="fallback" style={{ width: 50, height: 50, translate: "var(--missing, 7px) var(--other, var(--x))" }} />
      </div>,
      (renderer) => {
        expect(box(renderer, "inherits")[0]).toBe(10)
        expect(box(renderer, "overrides")[0]).toBe(30)
        expect(box(renderer, "own")[0]).toBe(40)
        expect(box(renderer, "fallback").slice(0, 2)).toEqual([7, 150 + 10])
      }
    )
  }, SLOW)

  it("makes a declaration with a var() that cannot be resolved none, whatever it was under", () => {
    mounted(
      <div style={{ position: "absolute", left: 0, top: 0, width: 800, height: 600 }}>
        <div testId="missing" style={{ width: 50, height: 50, transform: "translateX(var(--missing))" }} />
        <div testId="one" style={{ width: 50, height: 50, transform: "rotate(30deg)", rotate: "var(--missing)" }} />
        <div testId="loop" style={{ width: 50, height: 50, "--a": "var(--b)", "--b": "var(--a)", translate: "var(--a) 0" }} />
        <div testId="join" style={{ width: 50, height: 50, "--k": 2, translate: "var(--k)px 0" }} />
      </div>,
      (renderer) => {
        expect(box(renderer, "missing")).toEqual([0, 0, 50, 50])
        expect(box(renderer, "loop")[0]).toBe(0)
        expect(box(renderer, "join")[0], "var(--k)px is `2 px`, not 2px").toBe(0)
        const rotated = box(renderer, "one")
        expect(rotated[2]).toBeCloseTo(50 * (Math.cos(Math.PI / 6) + Math.sin(Math.PI / 6)), 3)
      }
    )
  }, SLOW)

  it("is not a transform until its var() resolves: no containing block for a missing one", () => {
    mounted(
      <div style={{ position: "absolute", left: 0, top: 0, width: 800, height: 600 }}>
        <div style={{ width: 200, height: 100, transform: "translateX(var(--missing))" }}>
          <div testId="child-none" style={{ position: "absolute", right: 0, bottom: 0, width: 10, height: 10 }} />
        </div>
        <div style={{ width: 200, height: 100, transform: "translateX(var(--missing, 0px))" }}>
          <div testId="child-active" style={{ position: "absolute", right: 0, bottom: 0, width: 10, height: 10 }} />
        </div>
      </div>,
      (renderer) => {
        expect(box(renderer, "child-none")[0], "the root is the containing block").toBe(790)
        expect(box(renderer, "child-active")[0], "the element is").toBe(190)
      }
    )
  }, SLOW)

  it("takes calc() in angles and products of lengths and percentages", () => {
    mounted(
      <div style={{ position: "absolute", left: 0, top: 0, width: 800, height: 600 }}>
        <div testId="product" style={{ position: "absolute", left: 100, top: 0, width: 80, height: 40, transform: "translateX(calc((0.5 - 1) * (100% - 6px)))" }} />
        <div testId="angle" style={{ position: "absolute", left: 300, top: 100, width: 80, height: 40, transform: "rotate(calc(45deg + 0.125turn))" }} />
        <div testId="literal" style={{ position: "absolute", left: 500, top: 100, width: 80, height: 40, transform: "rotate(90deg)" }} />
      </div>,
      (renderer) => {
        expect(box(renderer, "product")[0]).toBeCloseTo(63, 3)
        const angle = box(renderer, "angle")
        const literal = box(renderer, "literal")
        expect([angle[2], angle[3]].map((n) => +n.toFixed(3))).toEqual([literal[2], literal[3]].map((n) => +n.toFixed(3)))
      }
    )
  }, SLOW)
})

describeNative("a --p set from JS each frame", () => {
  const ROWS = 200
  const ROW_ELEMENTS = 1 + 4 * 2

  /** `--p` is set on the head, above the track only, or on the page, above the rows too; `literal` has no var(). */
  type Mode = "head" | "page" | "literal"

  function Page({ p, mode }: { p: number; mode: Mode }) {
    const track = mode === "literal" ? `translateX(${-p * 100}%)` : "translateX(calc(-100% * var(--p)))"
    return (
      <div style={{ display: "flex", flexDirection: "column", width: 760, height: 560, overflow: "hidden", ...(mode === "page" ? { "--p": p } : {}) }}>
        <div style={{ height: 80, overflow: "hidden", ...(mode === "head" ? { "--p": p } : {}) }}>
          <div testId="track" style={{ width: 300, height: 60, transform: track }} />
        </div>
        {Array.from({ length: ROWS }, (_, row) => (
          <div key={row} style={{ display: "flex", gap: 4, padding: 2 }}>
            {Array.from({ length: 4 }, (_, cell) => (
              <div key={cell} style={{ borderWidth: 1, borderColor: "#cccccc", padding: 2 }}>
                <text style={{ fontSize: 11, color: "#333333" }}>{`r${row} c${cell}`}</text>
              </div>
            ))}
          </div>
        ))}
      </div>
    )
  }

  /** What `frames` updates of the page did: layout passes and elements built per update. */
  function updates(mode: Mode, frames = 10) {
    const root = createTestRoot({ width: 800, height: 600 })
    const renderer = root.renderer
    try {
      root.render(<Page p={0} mode={mode} />)
      for (let frame = 0; frame < 6; frame++) {
        renderer.flush()
        renderer.drawFrame()
      }
      const layout = renderer.getLayoutCounters()
      const built = renderer.getBuildCounters().elementsBuilt
      for (let frame = 1; frame <= frames; frame++) {
        root.render(<Page p={frame / frames} mode={mode} />)
        renderer.flush()
        renderer.drawFrame()
        expect(box(renderer, "track")[0]).toBeCloseTo(-(frame / frames) * 300, 3)
      }
      if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
      return {
        passes: (renderer.getLayoutCounters().passes - layout.passes) / frames,
        built: (renderer.getBuildCounters().elementsBuilt - built) / frames,
      }
    } finally {
      root.unmount()
    }
  }

  it("builds as much as the same update of a literal transform, wherever --p is set", () => {
    const literal = updates("literal")
    const head = updates("head")
    const page = updates("page")
    console.log(`per update: literal ${JSON.stringify(literal)}, --p on the head ${JSON.stringify(head)}, --p on the page ${JSON.stringify(page)}`)
    expect(head.passes).toBe(literal.passes)
    expect(page.passes).toBe(literal.passes)
    // Verify builds every replayed frame a second time, whole: the count is the whole page then.
    if (verifying) return
    // The rows read no custom property, so the change of --p above them does not build them again.
    expect(page.built).toBeLessThan(ROWS * ROW_ELEMENTS * 0.1)
    expect(head.built).toBeLessThanOrEqual(literal.built + 2)
  }, SLOW)
})
