/// CSS Transforms 2 in three dimensions, against headless Chrome: `fixtures/transform-3d-oracle.html` is the page and
/// `fixtures/transform-3d-oracle.json` what Chrome measures of each scene (regenerate with `transform-3d-oracle.sh`).
///
/// - `getBoundingClientRect` is the box that holds the four projected corners of the border box (0.06 px): a parent's
///   `perspective` and `perspective-origin`, `rotateX()`, `rotateY()`, `rotate3d()`, `translateZ()`, `translate3d()`,
///   `scale3d()`, `perspective()` and `matrix3d()` in the list, the `rotate`, `translate` and `scale` properties with a
///   third component, `transform-origin` with a z, `transform-style: preserve-3d` nesting and the same nesting flat.
/// - A grouping property (`overflow`, `opacity` below 1, a filter, a clip path, a mask, `backdrop-filter`) makes the used
///   `transform-style` flat, by Chromium's list.
/// - `perspective` and `transform-style: preserve-3d` (as declared) make a stacking context and the containing block of the
///   absolute and fixed boxes in it; `backface-visibility` and `perspective: none` do not.
/// - `backface-visibility: hidden` hides a box whose back faces the viewer: the card flip, with and without a
///   `preserve-3d` parent, `elementFromPoint` against `getElementsAt` and the target of a pointer move.
/// - `offsetX` and `offsetY` are measured in the local space of a box seen in perspective.
///
/// Where gpuix has no property the oracle uses (`isolation`, `mix-blend-mode`, `will-change`), the scene's case is left out.

import fs from "fs"
import path from "path"
import React from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { motion } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { elementsOf } from "./oracle-boxes.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rect = [number, number, number, number]
interface Scene {
  html: string
  boxes: Record<string, Rect>
  from: Record<string, string | null>
  offsets: Record<string, [string, number, number] | null>
}

const ORACLE: Record<string, Scene> = JSON.parse(
  fs.readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/transform-3d-oracle.json"), "utf8")
)

const TOLERANCE = 0.06
const OFFSET_TOLERANCE = 0.5 + 1e-3
const SLOW = 120_000

/** The cases of `grouping` that use a property gpuix does not have. */
const UNSUPPORTED = new Set(["g10", "g11", "g13"])

interface Mounted {
  renderer: ReturnType<typeof createTestRoot>["renderer"]
  ids: Map<string, number>
  names: Map<number, string>
  moved: { target: string | null }
  clicks: { target: string; x: number; y: number }[]
}

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
    run(scene, { renderer, ids, names, moved, clicks })
  } finally {
    unmount()
  }
}

const rounded = (rect: number[]) => rect.map((value) => +value.toFixed(3))

/** The ids of the cases of a scene that gpuix has the properties of: `g10` and its descendants are not. */
const supported = (id: string) => !UNSUPPORTED.has(id.replace(/[A-Z].*$/, ""))

describeNative("getBoundingClientRect of a box in three dimensions follows Chrome", () => {
  for (const name of [
    "persp-rotatey",
    "persp-rotatex",
    "persp-translatez",
    "perspective-function",
    "perspective-both",
    "matrix3d",
    "preserve-3d",
    "preserve-3d-deep",
    "grouping",
    "flip",
    "flip-x",
    "flip-flat",
    "backface-flat",
    "individual-3d",
    "origin-z",
    "containing-block",
    "stacking",
  ]) {
    it(`is the box that holds the projected corners: ${name}`, () => {
      withScene(name, (scene, mounted) => {
        const wrong: string[] = []
        for (const [id, want] of Object.entries(scene.boxes)) {
          if (name === "grouping" && !supported(id)) continue
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
  }
})

describeNative("the element at a point follows the projection and the faces that turn away", () => {
  for (const name of ["flip", "flip-x", "flip-flat", "backface-flat", "stacking", "hit-persp", "hit-preserve", "hit-persp-clip"]) {
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
        const entries = Object.entries(scene.from).filter((_, index) => name.startsWith("hit-") ? index % 5 === 0 : true)
        for (const [key, want] of entries) {
          const [x, y] = key.split(",").map(Number) as [number, number]
          mounted.moved.target = null
          mounted.renderer.nativeSimulateMouseMove(x, y)
          if (mounted.moved.target !== want) wrong.push(`${key}: got ${mounted.moved.target} Chrome ${want}`)
        }
        expect(wrong).toEqual([])
      })
    }, SLOW)
  }
})

describeNative("offsetX and offsetY are measured in the local space of a box seen in perspective", () => {
  it("click at a point of the window: offset-persp", () => {
    withScene("offset-persp", (scene, mounted) => {
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
})

type Rgba = [number, number, number, number]
const RED: Rgba = [255, 0, 0, 255]
const BLUE: Rgba = [0, 0, 255, 255]
const WHITE: Rgba = [255, 255, 255, 255]

describeNative("a card flip paints the face that turns toward the viewer", () => {
  function pixels(angle: number, preserve = true) {
    const testRoot = createTestRoot({ width: 400, height: 400 })
    try {
      testRoot.render(
        <div style={{ width: 400, height: 400, backgroundColor: "#ffffff" }}>
          <div style={{ position: "absolute", left: 100, top: 100, width: 200, height: 200, perspective: 600 }}>
            <div
              style={{
                position: "absolute",
                left: 25,
                top: 25,
                width: 150,
                height: 150,
                transformStyle: preserve ? "preserve-3d" : "flat",
                transform: `rotateY(${angle}deg)`,
              }}
            >
              <div style={{ position: "absolute", left: 0, top: 0, width: 150, height: 150, backgroundColor: "#ff0000", backfaceVisibility: "hidden" }} />
              <div
                style={{
                  position: "absolute",
                  left: 0,
                  top: 0,
                  width: 150,
                  height: 150,
                  backgroundColor: "#0000ff",
                  backfaceVisibility: "hidden",
                  transform: "rotateY(180deg)",
                }}
              />
            </div>
          </div>
        </div>
      )
      const file = `${SHOTS_DIR}/transform-3d-${process.pid}-${angle}-${preserve}.png`
      fs.rmSync(file, { force: true })
      testRoot.renderer.flush()
      testRoot.renderer.flush()
      testRoot.renderer.captureScreenshot(file)
      return readLogicalPixels(file, testRoot.renderer).at
    } finally {
      testRoot.unmount()
    }
  }

  it("shows the front at 0 and the back at 180", () => {
    const front = pixels(0)
    expect(front(200, 200)).toEqual(RED)
    expect(front(130, 130)).toEqual(RED)
    expect(front(99, 99)).toEqual(WHITE)
    const back = pixels(180)
    expect(back(200, 200)).toEqual(BLUE)
    expect(back(130, 130)).toEqual(BLUE)
    expect(back(99, 99)).toEqual(WHITE)
  }, SLOW)

  it("shows the back past 90 and the front again past 270, narrower in perspective", () => {
    const turned = pixels(120)
    expect(turned(200, 200)).toEqual(BLUE)
    expect(turned(127, 200)).toEqual(WHITE)
    expect(pixels(60)(200, 200)).toEqual(RED)
    expect(pixels(300)(200, 200)).toEqual(RED)
  }, SLOW)

  it("keeps the front of a flat card, mirrored: its faces are flattened into it before it turns", () => {
    expect(pixels(180, false)(200, 200)).toEqual(RED)
    expect(pixels(0, false)(200, 200)).toEqual(RED)
  }, SLOW)
})

describeNative("a card that a Motion tween turns hides the face that turns away on the ticks that are patched", () => {
  it("hits the front before the card is edge on and the back after", () => {
    const { render, renderer, unmount } = createTestRoot({ width: 400, height: 400 })
    try {
      renderer.clockPause()
      const face = (testId: string, style: object) => (
        <div testId={testId} style={{ position: "absolute", left: 0, top: 0, width: 150, height: 150, backfaceVisibility: "hidden", ...style }} />
      )
      render(
        <div style={{ position: "absolute", left: 100, top: 100, width: 200, height: 200, perspective: 600 }}>
          <motion.div
            testId="card"
            style={{ position: "absolute", left: 25, top: 25, width: 150, height: 150, transformStyle: "preserve-3d" }}
            initial={{ rotateY: 0 }}
            animate={{ rotateY: 180 }}
            transition={{ duration: 1, ease: "linear" }}
          >
            {face("front", {})}
            {face("back", { transform: "rotateY(180deg)" })}
          </motion.div>
        </div>
      )
      renderer.flush()
      const faceAt = () => {
        const hit = renderer.getElementsAt(200, 200)[0]
        return hit === undefined ? null : renderer.findByTestId("front")?.id === hit ? "front" : renderer.findByTestId("back")?.id === hit ? "back" : "other"
      }
      expect(faceAt()).toBe("front")
      const seen: (string | null)[] = []
      for (let tick = 0; tick < 60; tick++) {
        renderer.clockAdvance(16)
        renderer.drawFrame()
        if (tick === 15) seen.push(faceAt())
        if (tick === 50) seen.push(faceAt())
      }
      expect(seen).toEqual(["front", "back"])
    } finally {
      unmount()
    }
  }, SLOW)
})
