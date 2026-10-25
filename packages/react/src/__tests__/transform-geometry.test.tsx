/// What a CSS transform does to geometry, against headless Chrome: `fixtures/transform-geometry-oracle.html` is the page
/// and `fixtures/transform-geometry-oracle.json` what Chrome measures of each scene (regenerate with
/// `transform-geometry-oracle.sh`).
///
/// - `getBoundingClientRect` is the box that holds the four corners of the border box after the element's own transform
///   and the ones around it, exact (0.06 px), whatever the origin, the units, the nesting or the scroll of a scroller
///   around it. The sizes `clientWidth`, `clientHeight`, `scrollWidth` and `scrollHeight` are the ones of the box
///   before it is transformed.
/// - `var()` of the custom properties an element computes to, `calc()` products of lengths and percentages, and `calc()`
///   sums of angles are substituted into the transform properties as Chrome does (scene `var`): a `var()` that cannot be
///   resolved makes the property `none`, and `var(--k)px` is not `2px`.
/// - The element at a point is the one whose transformed box holds it: `elementFromPoint` against `getElementsAt` and the
///   target of a pointer move, point for point.
/// - `offsetX` and `offsetY` of a click are measured in the target's own space, from its padding edge.
/// - A scroller scrolls as far as the transformed boxes inside it reach, and only forwards and down.
/// - `scrollIntoView` through nested scrollers, transformed or not: each scroller aligns the box that holds the target in
///   its own space, and hands the next the box that holds where its scroll left it, so a turn between scrollers makes the
///   box larger each time (128 cases of the oracle, 8 scenes). Focus scrolls to the `nearest` edges.

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { defineKeyframes } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { elementsOf } from "./oracle-boxes.js"
import { verifying } from "./retained-harness.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rect = [number, number, number, number]
type IntoViewOptions = { block: string; inline: string }
interface Scene {
  html: string
  scroll: [string, number, number][]
  into: [string, { block: string; inline: string }][]
  scrolls: [string, number, number][]
  nest: { reset: [string, number, number][]; ops: (IntoViewOptions | "focus")[] } | null
  /** Per op of `nest`: [O.scrollTop, O.scrollLeft, I.scrollTop, I.scrollLeft]. */
  nested: number[][]
  boxes: Record<string, Rect>
  sizes: Record<string, number[]>
  from: Record<string, string | null>
  offsets: Record<string, [string, number, number] | null>
}

const ORACLE: Record<string, Scene> = JSON.parse(
  fs.readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/transform-geometry-oracle.json"), "utf8")
)

const TOLERANCE = 0.06
/** Chrome rounds the offset of a click to a whole pixel. */
const OFFSET_TOLERANCE = 0.5 + 1e-3

/**
 * `scrollWidth` and `scrollHeight` of an element that does not scroll are its border box in gpuix, and Chrome adds what
 * overflows it (here, in `flow` `f`, a child translated 5px past the end), with or without a transform.
 */
const NOT_SCROLLERS = new Set(["flow:f"])

/**
 * The scroll extents gpuix gives where it differs from Chrome for a reason that is not the transform. An absolutely
 * positioned child that reaches the end of a scroller with padding gets the padding after it in gpuix and none in
 * Chrome, whether the child is transformed (`e12`, scaled to reach 150 x 75) or not (`e17`, 200 x 100). The scroller has
 * `padding: 10px 20px 15px 5px`: 20 and 15 more than Chrome's extent where the child is the farthest.
 */
const EXTENT_DEVIATIONS: Record<string, [number, number]> = { e12: [170, 90], e17: [220, 115] }

/** The pointer is aimed at a hitbox that a clip of a rotated parent cuts, inside a rotation of its own: see `hit-clip`. */
const LOOSE_NESTED_CLIP = new Set(["hit-clip:C:case"])
const SLOW = 120_000

interface Mounted {
  renderer: ReturnType<typeof createTestRoot>["renderer"]
  ids: Map<string, number>
  names: Map<number, string>
  /** Set by the handlers of the elements: the one a pointer event was aimed at. */
  moved: { target: string | null }
  clicks: { target: string; x: number; y: number }[]
}

/** Mounts a scene of the oracle in a window and scrolls what Chrome scrolled. */
function withScene(name: string, run: (scene: Scene, mounted: Mounted) => void) {
  const scene = ORACLE[name]!
  const { render, renderer, unmount } = createTestRoot({ width: 800, height: 600 })
  try {
    const moved: Mounted["moved"] = { target: null }
    const clicks: Mounted["clicks"] = []
    const handlers = (id: string) => ({
      onPointerMove: (event: { target: unknown; currentTarget: unknown }) => {
        if (event.target === event.currentTarget) moved.target = id
      },
      onClick: (event: { target: unknown; currentTarget: unknown; offsetX: number; offsetY: number }) => {
        if (event.target === event.currentTarget) clicks.push({ target: id, x: event.offsetX, y: event.offsetY })
      },
      ...(scene.nest && id === "T" ? { tabIndex: 0 } : {}),
    })
    render(
      <div testId="case" style={{ position: "absolute", left: 0, top: 0, width: 800, height: 600 }} {...handlers("case")}>
        {elementsOf(scene.html, undefined, handlers)}
      </div>
    )
    const ids = new Map<string, number>()
    const names = new Map<number, string>()
    for (const id of [...Object.keys(scene.boxes), "case"]) {
      const element = renderer.findByTestId(id)
      expect(element, `${id} is mounted`).toBeDefined()
      ids.set(id, element!.id)
      names.set(element!.id, id)
    }
    for (const [id, top, left] of scene.scroll) renderer.scrollTo(ids.get(id)!, left, top)
    run(scene, { renderer, ids, names, moved, clicks })
  } finally {
    unmount()
  }
}

const rounded = (rect: number[]) => rect.map((value) => +value.toFixed(3))

describeNative("getBoundingClientRect through a transform follows Chrome", () => {
  for (const name of [
    "translate",
    "rotate",
    "scale",
    "skew",
    "matrix",
    "origin",
    "percent",
    "nested",
    "individual",
    "var",
    "scrolled",
    "flow",
  ]) {
    it(`is the box that holds the corners: ${name}`, () => {
      withScene(name, (scene, mounted) => {
        const wrong: string[] = []
        for (const [id, want] of Object.entries(scene.boxes)) {
          const bounds = mounted.renderer.getElementBounds(mounted.ids.get(id)!)
          expect(bounds, `${id} has a box`).not.toBeNull()
          const got = [bounds!.x, bounds!.y, bounds!.width, bounds!.height]
          if (got.some((value, index) => Math.abs(value - want[index]!) > TOLERANCE)) {
            wrong.push(`${id}: got [${rounded(got)}] Chrome [${rounded(want)}]`)
          }
        }
        expect(wrong).toEqual([])
      })
    }, SLOW)

    it(`leaves the sizes of the box before the transform alone: ${name}`, () => {
      withScene(name, (scene, mounted) => {
        const wrong: string[] = []
        for (const [id, sizes] of Object.entries(scene.sizes)) {
          if (NOT_SCROLLERS.has(`${name}:${id}`)) continue
          const metrics = mounted.renderer.getScrollMetrics(mounted.ids.get(id)!)
          expect(metrics, `${id} has metrics`).not.toBeNull()
          // [scrollLeft, scrollTop, scrollWidth, scrollHeight, clientWidth, clientHeight] against Chrome's
          // [offsetLeft, offsetTop, offsetWidth, offsetHeight, clientWidth, clientHeight, scrollWidth, scrollHeight].
          const got = [metrics![4], metrics![5], metrics![2], metrics![3]]
          const want = [sizes[4], sizes[5], sizes[6], sizes[7]]
          if (got.some((value, index) => value !== want[index])) wrong.push(`${id}: got [${got}] Chrome [${want}]`)
        }
        expect(wrong).toEqual([])
      })
    }, SLOW)
  }
})

describeNative("the element at a point follows the transforms", () => {
  for (const name of ["hit-rotate", "hit-scale", "hit-translate", "hit-skew", "hit-clip", "hit-flat", "hit-scroller"]) {
    it(`getElementsAt is elementFromPoint: ${name}`, () => {
      withScene(name, (scene, mounted) => {
        const wrong: string[] = []
        for (const [key, want] of Object.entries(scene.from)) {
          const [x, y] = key.split(",").map(Number) as [number, number]
          const hit = mounted.renderer.getElementsAt(x, y)[0]
          const got = hit === undefined ? null : (mounted.names.get(hit) ?? `#${hit}`)
          if (got !== want) wrong.push(`${key}: got ${got} Chrome ${want}`)
        }
        expect(wrong).toEqual([])
      })
    }, SLOW)

    it(`a pointer move is aimed at it: ${name}`, () => {
      withScene(name, (scene, mounted) => {
        const wrong: string[] = []
        const entries = Object.entries(scene.from).filter((_, index) => index % 5 === 0)
        for (const [key, want] of entries) {
          const [x, y] = key.split(",").map(Number) as [number, number]
          mounted.moved.target = null
          mounted.renderer.nativeSimulateMouseMove(x, y)
          if (mounted.moved.target !== want && !LOOSE_NESTED_CLIP.has(`${name}:${mounted.moved.target}:${want}`)) {
            wrong.push(`${key}: got ${mounted.moved.target} Chrome ${want}`)
          }
        }
        expect(wrong).toEqual([])
      })
    }, SLOW)
  }
})

describeNative("offsetX and offsetY are measured in the space of the target", () => {
  for (const name of ["offset-rotate", "offset-scale", "offset-nested", "offset-translate", "offset-origin"]) {
    it(`click at a point of the window: ${name}`, () => {
      withScene(name, (scene, mounted) => {
        const wrong: string[] = []
        for (const [key, want] of Object.entries(scene.offsets)) {
          const [x, y] = key.split(",").map(Number) as [number, number]
          mounted.clicks.length = 0
          mounted.renderer.nativeSimulateClick(x, y)
          const click = mounted.clicks[0]
          if (!want || !click) {
            if (!!want !== !!click) wrong.push(`${key}: got ${JSON.stringify(click)} Chrome ${JSON.stringify(want)}`)
            continue
          }
          const same =
            click.target === want[0] && Math.abs(click.x - want[1]) <= OFFSET_TOLERANCE && Math.abs(click.y - want[2]) <= OFFSET_TOLERANCE
          if (!same) wrong.push(`${key}: got ${click.target} [${rounded([click.x, click.y])}] Chrome ${want[0]} [${rounded([want[1], want[2]])}]`)
        }
        expect(wrong).toEqual([])
      })
    }, SLOW)
  }
})

describeNative("scrollIntoView aligns the box that holds the corners of a transformed target", () => {
  it("scrolls to where Chrome does", () => {
    withScene("into-view", (scene, mounted) => {
      const wrong: string[] = []
      scene.into.forEach(([id, options], index) => {
        mounted.renderer.scrollIntoView(mounted.ids.get(id)!, options.block, options.inline)
        const offset = mounted.renderer.getScrollOffset(mounted.ids.get("S")!)!
        const [, top, left] = scene.scrolls[index]!
        if (Math.abs(offset[0] - left) > 1 || Math.abs(offset[1] - top) > 1) wrong.push(`${id}: got [${offset}] Chrome [${left}, ${top}] (left, top)`)
      })
      expect(wrong).toEqual([])
    })
  }, SLOW)
})

const NESTED_SCENES = Object.keys(ORACLE).filter((name) => name.startsWith("nest-"))

describeNative("scrollIntoView and focus through nested scrollers with transforms", () => {
  /** Reads [O.scrollTop, O.scrollLeft, I.scrollTop, I.scrollLeft]. */
  const scrolls = (mounted: Mounted) => () => {
    const offset = (id: string) => mounted.renderer.getScrollOffset(mounted.ids.get(id)!)!
    return [offset("O")[1]!, offset("O")[0]!, offset("I")[1]!, offset("I")[0]!]
  }
  const reset = (scene: Scene, mounted: Mounted) => {
    for (const [id, top, left] of scene.nest!.reset) mounted.renderer.scrollTo(mounted.ids.get(id)!, left, top)
  }
  const differs = (got: number[], want: number[]) => got.some((value, index) => Math.abs(value - want[index]!) > 1)

  for (const name of NESTED_SCENES) {
    it(`aligns the box that holds the target in each scroller in turn, as Chrome does: ${name}`, () => {
      withScene(name, (scene, mounted) => {
        const read = scrolls(mounted)
        const wrong: string[] = []
        scene.nest!.ops.forEach((op, index) => {
          if (op === "focus") return
          reset(scene, mounted)
          mounted.renderer.scrollIntoView(mounted.ids.get("T")!, op.block, op.inline)
          const got = read()
          if (differs(got, scene.nested[index]!)) {
            wrong.push(`${op.block}/${op.inline}: got [${got}] Chrome [${scene.nested[index]}] (O top, left; I top, left)`)
          }
        })
        expect(wrong).toEqual([])
      })
    }, SLOW)

    // Chrome centres a focus target that is out of view and gpuix scrolls it to the nearest edge (see the deviations),
    // so the transforms are checked against Chrome's `nearest`.
    it(`Tab scrolls the nearest edges of the target into view in each scroller: ${name}`, () => {
      withScene(name, (scene, mounted) => {
        const read = scrolls(mounted)
        reset(scene, mounted)
        mounted.renderer.simulateKeystrokes("Tab")
        expect(mounted.renderer.getFocusedElementId()).toBe(mounted.ids.get("T"))
        const nearest = scene.nest!.ops.findIndex((op) => op !== "focus" && op.block === "nearest" && op.inline === "nearest")
        const got = read()
        const want = scene.nested[nearest]!
        expect(differs(got, want), `got [${got}] Chrome [${want}] (O top, left; I top, left)`).toBe(false)
      })
    }, SLOW)
  }
})

describeNative("a scroller scrolls as far as the transformed boxes in it reach", () => {
  it("has the scroll extents of Chrome, the negative directions ignored", () => {
    withScene("extent", (scene, mounted) => {
      const wrong: string[] = []
      for (const [id, sizes] of Object.entries(scene.sizes)) {
        if (!/^e\d+$/.test(id)) continue
        const metrics = mounted.renderer.getScrollMetrics(mounted.ids.get(id)!)
        const got = [metrics![4], metrics![5], metrics![2], metrics![3]]
        const deviation = EXTENT_DEVIATIONS[id]
        const want = [sizes[4], sizes[5], deviation?.[0] ?? sizes[6], deviation?.[1] ?? sizes[7]]
        if (got.some((value, index) => value !== want[index])) wrong.push(`${id}: got [${got}] Chrome [${want}] (client, scroll)`)
      }
      expect(wrong).toEqual([])
    })
  }, SLOW)
})

defineKeyframes("tg-fade", { from: { backgroundColor: "#ff0000" }, to: { backgroundColor: "#0000ff" } })

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))
const patching = process.env.GPUIX_RETAIN_OFF === undefined && process.env.GPUIX_PATCH_OFF === undefined && !verifying

describeNative("the box of a transformed element is current whenever it is read", () => {
  const box = (rotate: string) => ({ position: "absolute", left: 100, top: 50, width: 80, height: 40, transform: rotate }) as const

  it("answers for a transform written a moment ago, before and after a frame", () => {
    const { render, renderer, unmount } = createTestRoot({ width: 400, height: 300 })
    try {
      render(<div testId="a" style={box("rotate(0deg)")} />)
      const read = () => {
        const bounds = renderer.getElementBounds(renderer.findByTestId("a")!.id)!
        return [bounds.x, bounds.y, bounds.width, bounds.height].map((value) => Math.round(value * 100) / 100 + 0)
      }
      expect(read()).toEqual([100, 50, 80, 40])
      // The read lays the tree out by itself: no frame was drawn since the write.
      render(<div testId="a" style={box("rotate(90deg)")} />)
      expect(read()).toEqual([120, 30, 40, 80])
      renderer.flush()
      expect(read()).toEqual([120, 30, 40, 80])
      render(<div testId="a" style={box("scale(2, 1) translate(10px, 0)")} />)
      // Scaled about the middle (140, 70) after a move of 10 by the translate, which is scaled too.
      expect(read()).toEqual([80, 50, 160, 40])
      render(<div testId="a" style={box("none")} />)
      expect(read()).toEqual([100, 50, 80, 40])
    } finally {
      unmount()
    }
  })

  const patched = it.skipIf(!patching)

  patched("answers for a box in a transformed parent on a tick that only repaints it", async () => {
    const { render, renderer, unmount } = createTestRoot({ width: 1240, height: 780 })
    try {
      renderer.clockPause()
      render(
        <div style={{ display: "flex", flexDirection: "column", width: 1200, height: 760, overflow: "hidden" }}>
          <div testId="parent" style={{ ...box("rotate(90deg)"), left: 600, top: 20 }}>
            <div
              testId="child"
              style={{ position: "absolute", left: 10, top: 5, width: 40, height: 20, backgroundColor: "#ff0000", animation: "tg-fade 1.5s linear infinite" }}
            />
          </div>
          {Array.from({ length: 100 }, (_, row) => (
            <div key={row} style={{ display: "flex", flexDirection: "row", gap: 4, padding: 2 }}>
              {Array.from({ length: 4 }, (_, cell) => (
                <div key={cell} style={{ borderWidth: 1, borderColor: "#cccccc", padding: 2 }}>
                  <text style={{ fontSize: 11, color: "#333333" }}>{`r${row} c${cell}`}</text>
                </div>
              ))}
            </div>
          ))}
        </div>
      )
      for (let frame = 0; frame < 12; frame++) {
        renderer.flush()
        renderer.dispatchNativeEvents()
        await sleep(8)
      }
      renderer.setInlineDraws(false)
      const read = (id: string) => {
        const bounds = renderer.getElementBounds(renderer.findByTestId(id)!.id)!
        return [bounds.x, bounds.y, bounds.width, bounds.height].map((value) => Math.round(value * 100) / 100 + 0)
      }
      // The parent turns about (640, 40); its child, at (10, 5) and 40 x 20 inside it, lands 20 wide and 40 tall.
      const parent = [620, 0, 40, 80]
      const child = [635, 10, 20, 40]
      const patchedBefore = renderer.getPatchCounters().patchedFrames
      for (let tick = 0; tick < 6; tick++) {
        renderer.clockAdvance(16)
        renderer.drawFrame()
        expect(read("parent"), `parent after tick ${tick}`).toEqual(parent)
        expect(read("child"), `child after tick ${tick}`).toEqual(child)
      }
      expect(renderer.getPatchCounters().patchedFrames - patchedBefore, "the ticks were patched").toBeGreaterThan(0)
    } finally {
      unmount()
    }
  }, 120_000)
})
