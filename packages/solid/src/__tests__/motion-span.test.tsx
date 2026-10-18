/// `motion.span`: `animate`, `initial`, `exit` and `variants` move the colours of an
/// inline box. A span has no element, so its paragraph samples the animation each
/// frame and paints the result: these tests read the pixels it paints.

import fs from "fs"
import os from "os"
import path from "path"
import { Show, createSignal, type JSX } from "solid-js"
import { describe, expect, it } from "bun:test"
import { AnimatePresence, motion, motionValue } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels } from "./pixels.js"

const LINE = 20
const TEXT = { fontSize: 16, lineHeight: `${LINE}px`, fontFamily: "Menlo", color: "#000000" } as const
const PAGE = { width: 300, height: 120, padding: 20, backgroundColor: "#ffffff" } as const
const LINEAR = { duration: 1, ease: "linear" } as const

const RED = "#ff0000"
const BLUE = "#0000ff"

type Rgb = [number, number, number]

const shots = fs.mkdtempSync(path.join(os.tmpdir(), "gpuix-solid-motion-span-"))

function setup() {
  const app = createTestRoot({ width: 300, height: 120 })
  const renderer = app.renderer
  const file = path.join(shots, "motion-span.png")
  renderer.clockPause()
  const pixels = () => {
    renderer.dispatchNativeEvents()
    renderer.captureScreenshot(file)
    return readLogicalPixels(file, renderer)
  }
  const paragraph = () => renderer.getElementBounds(renderer.findByTestId("p")!.id)!
  return {
    app,
    renderer,
    paragraph,
    /** The pixel `dx` and `dy` logical pixels from the paragraph's top left corner. */
    at: (dx: number, dy: number): Rgb => {
      const box = paragraph()
      return pixels().at(Math.floor(box.x + dx), Math.floor(box.y + dy)).slice(0, 3) as Rgb
    },
    /** The pixel of the paragraph that is the furthest from the white page: the ink. */
    ink: (): Rgb => {
      const box = paragraph()
      const shot = pixels()
      let best: Rgb = [255, 255, 255]
      let bestDistance = 0
      for (let y = Math.floor(box.y); y < box.y + box.height; y++) {
        for (let x = Math.floor(box.x); x < box.x + box.width; x++) {
          const pixel = shot.at(x, y).slice(0, 3) as Rgb
          const distance = pixel.reduce((sum, channel) => sum + (255 - channel), 0)
          if (distance > bestDistance) {
            best = pixel
            bestDistance = distance
          }
        }
      }
      return best
    },
  }
}

/** Between red and blue, and far from both. Motion blends in Oklab, so the way between is not the sRGB line. */
function expectBetween(pixel: Rgb) {
  expect(pixel[0]).toBeGreaterThan(70)
  expect(pixel[0]).toBeLessThan(200)
  expect(pixel[2]).toBeGreaterThan(70)
  expect(pixel[2]).toBeLessThan(200)
}

describe.skipIf(!hasNativeTestRenderer)("Solid motion.span", () => {
  it("animates the background of a span from initial to animate over the transition", () => {
    const page = setup()
    page.app.render(() => (
      <div style={PAGE}>
        <text testId="p" style={TEXT}>
          <motion.span
            style={{ paddingLeft: 12 }}
            initial={{ backgroundColor: RED }}
            animate={{ backgroundColor: BLUE }}
            transition={LINEAR}
          >
            aaaa
          </motion.span>{" "}
          tail
        </text>
      </div>
    ))
    const before = page.paragraph()
    expect(page.at(4, 10)).toEqual([255, 0, 0])

    page.renderer.clockFastForward(250)
    const early = page.at(4, 10)
    expect(early[0]).toBeLessThan(255)
    expect(early[2]).toBeGreaterThan(0)

    page.renderer.clockFastForward(250)
    const middle = page.at(4, 10)
    expectBetween(middle)
    expect(middle[0]).toBeLessThan(early[0])
    expect(middle[2]).toBeGreaterThan(early[2])
    expect(page.paragraph()).toEqual(before)

    page.renderer.clockFastForward(750)
    expect(page.at(4, 10)).toEqual([0, 0, 255])
    page.app.unmount()
  })

  it("follows a signal: the target moves from where the colour is", () => {
    const [target, setTarget] = createSignal(BLUE)
    const page = setup()
    page.app.render(() => (
      <div style={PAGE}>
        <text testId="p" style={{ ...TEXT, fontSize: 64, lineHeight: "70px" }}>
          <motion.span initial={{ color: RED }} animate={{ color: target() }} transition={LINEAR}>
            MM
          </motion.span>
        </text>
      </div>
    ))
    expect(page.ink()).toEqual([255, 0, 0])
    page.renderer.clockFastForward(500)
    expectBetween(page.ink())

    page.app.flushSync(() => setTarget("#00ff00"))
    expectBetween(page.ink())
    page.renderer.clockFastForward(1000)
    expect(page.ink()).toEqual([0, 255, 0])
    page.app.unmount()
  })

  it("fades a span in with opacity, and it paints no more than that fraction of the text", () => {
    const page = setup()
    page.app.render(() => (
      <div style={PAGE}>
        <text testId="p" style={{ ...TEXT, fontSize: 64, lineHeight: "70px" }}>
          <motion.span initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={LINEAR}>
            MM
          </motion.span>
        </text>
      </div>
    ))
    expect(page.ink()).toEqual([255, 255, 255])
    page.renderer.clockFastForward(500)
    const middle = page.ink()
    expect(middle[0]).toBeGreaterThan(90)
    expect(middle[0]).toBeLessThan(165)
    page.renderer.clockFastForward(1000)
    expect(page.ink()).toEqual([0, 0, 0])
    page.app.unmount()
  })

  it("looks its variants up by name, and a variant brings its own transition", () => {
    const page = setup()
    page.app.render(() => (
      <div style={PAGE}>
        <text testId="p" style={TEXT}>
          <motion.span
            style={{ paddingLeft: 12 }}
            variants={{
              hidden: { backgroundColor: RED },
              shown: { backgroundColor: BLUE, transition: LINEAR },
            }}
            initial="hidden"
            animate="shown"
            transition={{ duration: 100 }}
          >
            aaaa
          </motion.span>
        </text>
      </div>
    ))
    expect(page.at(4, 10)).toEqual([255, 0, 0])
    page.renderer.clockFastForward(500)
    expectBetween(page.at(4, 10))
    page.renderer.clockFastForward(1000)
    expect(page.at(4, 10)).toEqual([0, 0, 255])
    page.app.unmount()
  })

  describe("under AnimatePresence", () => {
    const chip = (show: () => boolean, exit: object | string) => () => (
      <div style={PAGE}>
        <text testId="p" style={TEXT}>
          <AnimatePresence>
            <Show when={show()}>
              <motion.span
                style={{ paddingLeft: 12 }}
                variants={{
                  here: { backgroundColor: RED },
                  gone: { backgroundColor: BLUE, transition: LINEAR },
                }}
                initial={{ backgroundColor: RED }}
                animate={{ backgroundColor: RED }}
                exit={exit as never}
                transition={LINEAR}
              >
                aaaa
              </motion.span>
            </Show>
          </AnimatePresence>
        </text>
      </div>
    )

    for (const [name, exit] of [
      ["an exit style", { backgroundColor: BLUE }],
      ["an exit named by a variant", "gone"],
    ] as const) {
      it(`keeps a leaving span until ${name} has run, then removes it`, () => {
        const [show, setShow] = createSignal(true)
        const page = setup()
        page.app.render(chip(show, exit))
        expect(page.renderer.getAllText()).toContain("aaaa")
        expect(page.at(4, 10)).toEqual([255, 0, 0])

        page.app.flushSync(() => setShow(false))
        page.renderer.clockFastForward(500)
        page.renderer.dispatchNativeEvents()
        page.renderer.flush()
        expect(page.renderer.getAllText()).toContain("aaaa")
        expectBetween(page.at(4, 10))

        page.renderer.clockFastForward(1000)
        page.renderer.dispatchNativeEvents()
        page.renderer.flush()
        expect(page.renderer.getAllText()).not.toContain("aaaa")
        page.app.unmount()
      })
    }

    it("removes a leaving span that has no exit at once", () => {
      const [show, setShow] = createSignal(true)
      const page = setup()
      page.app.render(() => (
        <div style={PAGE}>
          <text testId="p" style={TEXT}>
            <AnimatePresence>
              <Show when={show()}>
                <motion.span animate={{ color: RED }}>aaaa</motion.span>
              </Show>
            </AnimatePresence>
          </text>
        </div>
      ))
      page.app.flushSync(() => setShow(false))
      page.renderer.flush()
      expect(page.renderer.getAllText()).not.toContain("aaaa")
      page.app.unmount()
    })
  })

  describe("what a span cannot do is an error", () => {
    function creationError(code: () => JSX.Element): Error {
      const app = createTestRoot()
      let thrown: unknown
      try {
        app.render(code)
      } catch (error) {
        thrown = error
      }
      app.renderer.dispose()
      expect(thrown).toBeInstanceOf(Error)
      return thrown as Error
    }

    const animating: Array<[string, Record<string, unknown>, string]> = [
      ["animate", { animate: { width: 100 } }, 'cannot animate "width" in animate'],
      ["initial", { initial: { height: 4 }, animate: { color: RED } }, 'cannot animate "height" in initial'],
      ["whileHover", { whileHover: { width: 10 } }, 'cannot animate "width" in whileHover'],
      ["exit", { exit: { width: 10 } }, 'cannot animate "width" in exit'],
      [
        "variants",
        { variants: { big: { flexGrow: 1 } }, animate: { color: RED } },
        'cannot animate "flexGrow" in the variant "big"',
      ],
    ]
    for (const [name, props, message] of animating) {
      it(`a key of ${name} that a span cannot animate`, () => {
        const error = creationError(() => (
          <text>
            <motion.span {...(props as object)}>aaaa</motion.span>
          </text>
        ))
        expect(error.message).toContain(message)
        expect(error.message).toContain("inline box")
      })
    }

    it("a key of an exit function that a span cannot animate", () => {
      const error = creationError(() => (
        <text>
          <AnimatePresence>
            <motion.span animate={{ color: RED }} exit={(() => ({ width: 20 })) as never}>
              aaaa
            </motion.span>
          </AnimatePresence>
        </text>
      ))
      expect(error.message).toContain('cannot animate "width" in exit')
    })

    it("a variant name that variants do not define", () => {
      const error = creationError(() => (
        <text>
          <motion.span animate="shown" variants={{ hidden: { color: RED } }}>
            aaaa
          </motion.span>
        </text>
      ))
      expect(error.message).toContain('names the variant "shown"')
    })
  })
})
