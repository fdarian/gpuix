/// `motion.span`: `animate`, `initial`, `exit` and `variants` move the colours of an
/// inline box. A span has no element, so its paragraph samples the animation each
/// frame and paints the result: these tests read the pixels it paints.

import fs from "fs"
import path from "path"
import { describe, expect, it, vi } from "vitest"
import { AnimatePresence, motion } from "../index"
import { motionValue } from "@ghia/native/host"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const LINE = 20
const TEXT = { fontSize: 16, lineHeight: `${LINE}px`, fontFamily: "Menlo", color: "#000000" } as const
const PAGE = { width: 300, height: 120, padding: 20, backgroundColor: "#ffffff" } as const
const LINEAR = { duration: 1, ease: "linear" } as const

type Rgb = [number, number, number]

const RED = "#ff0000"
const BLUE = "#0000ff"

function setup() {
  const testRoot = createTestRoot({ width: 300, height: 120 })
  const renderer = testRoot.renderer
  fs.mkdirSync(SHOTS_DIR, { recursive: true })
  const file = path.join(SHOTS_DIR, "motion-span.png")
  renderer.clockPause()
  const pixels = () => {
    renderer.dispatchNativeEvents()
    renderer.captureScreenshot(file)
    return readLogicalPixels(file, renderer)
  }
  const paragraph = () => renderer.getElementBounds(renderer.findByTestId("p")!.id)!
  return {
    testRoot,
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
  expect(pixel[0], `${pixel}`).toBeGreaterThan(70)
  expect(pixel[0], `${pixel}`).toBeLessThan(200)
  expect(pixel[2], `${pixel}`).toBeGreaterThan(70)
  expect(pixel[2], `${pixel}`).toBeLessThan(200)
}

describeNative("motion.span", () => {
  it("animates the background of a span from initial to animate over the transition", () => {
    const page = setup()
    page.testRoot.render(
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
    )
    const before = page.paragraph()
    expect(page.at(4, 10), "the first frame is initial, not animate").toEqual([255, 0, 0])

    page.renderer.clockFastForward(250)
    const early = page.at(4, 10)
    expect(early[0], `${early}`).toBeLessThan(255)
    expect(early[2], `${early}`).toBeGreaterThan(0)

    page.renderer.clockFastForward(250)
    const middle = page.at(4, 10)
    expectBetween(middle)
    expect(middle[0], "red goes out as blue comes in").toBeLessThan(early[0])
    expect(middle[2]).toBeGreaterThan(early[2])
    expect(page.paragraph(), "a colour moves nothing").toEqual(before)

    page.renderer.clockFastForward(750)
    expect(page.at(4, 10)).toEqual([0, 0, 255])
    page.testRoot.unmount()
  })

  it("animates the text colour and retargets from where it is", () => {
    const page = setup()
    const tree = (target: string) => (
      <div style={PAGE}>
        <text testId="p" style={{ ...TEXT, fontSize: 64, lineHeight: "70px" }}>
          <motion.span initial={{ color: RED }} animate={{ color: target }} transition={LINEAR}>
            MM
          </motion.span>
        </text>
      </div>
    )
    page.testRoot.render(tree(BLUE))
    expect(page.ink()).toEqual([255, 0, 0])
    page.renderer.clockFastForward(500)
    expectBetween(page.ink())

    page.testRoot.render(tree("#00ff00"))
    const retargeted = page.ink()
    expectBetween(retargeted)
    page.renderer.clockFastForward(1000)
    expect(page.ink()).toEqual([0, 255, 0])
    page.testRoot.unmount()
  })

  it("fades a span in with opacity, and it paints no more than that fraction of the text", () => {
    const page = setup()
    page.testRoot.render(
      <div style={PAGE}>
        <text testId="p" style={{ ...TEXT, fontSize: 64, lineHeight: "70px" }}>
          <motion.span initial={{ opacity: 0 }} animate={{ opacity: 1 }} transition={LINEAR}>
            MM
          </motion.span>
        </text>
      </div>
    )
    expect(page.ink(), "nothing is painted on the first frame").toEqual([255, 255, 255])
    page.renderer.clockFastForward(500)
    const middle = page.ink()
    expect(middle[0], `${middle}`).toBeGreaterThan(90)
    expect(middle[0], `${middle}`).toBeLessThan(165)
    page.renderer.clockFastForward(1000)
    expect(page.ink()).toEqual([0, 0, 0])
    page.testRoot.unmount()
  })

  it("animates from the span's own style when there is no initial, and shows animate at once with initial={false}", () => {
    const tree = (initial?: false) => (
      <div style={PAGE}>
        <text testId="p" style={TEXT}>
          <motion.span
            style={{ paddingLeft: 12, backgroundColor: RED }}
            initial={initial}
            animate={{ backgroundColor: BLUE }}
            transition={LINEAR}
          >
            aaaa
          </motion.span>
        </text>
      </div>
    )
    const page = setup()
    page.testRoot.render(tree())
    expect(page.at(4, 10)).toEqual([255, 0, 0])
    page.renderer.clockFastForward(500)
    expectBetween(page.at(4, 10))
    page.renderer.clockFastForward(1000)
    expect(page.at(4, 10)).toEqual([0, 0, 255])
    page.testRoot.unmount()

    const blocked = setup()
    blocked.testRoot.render(tree(false))
    expect(blocked.at(4, 10)).toEqual([0, 0, 255])
    blocked.testRoot.unmount()
  })

  it("animates the colours of an inline box that are not the text and the background", () => {
    const page = setup()
    page.testRoot.render(
      <div style={PAGE}>
        <text testId="p" style={TEXT}>
          <motion.span
            style={{ borderWidth: 4, borderStyle: "solid", borderColor: RED }}
            initial={{ borderColor: RED }}
            animate={{ borderColor: BLUE }}
            transition={LINEAR}
          >
            aaaa
          </motion.span>
        </text>
      </div>
    )
    // The top border runs along the whole box, above the content area of the line.
    expect(page.at(30, -2)).toEqual([255, 0, 0])
    page.renderer.clockFastForward(500)
    expectBetween(page.at(30, -2))
    page.renderer.clockFastForward(1000)
    expect(page.at(30, -2)).toEqual([0, 0, 255])
    page.testRoot.unmount()
  })

  it("looks its variants up by name, and a variant brings its own transition", () => {
    const page = setup()
    page.testRoot.render(
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
    )
    expect(page.at(4, 10)).toEqual([255, 0, 0])
    page.renderer.clockFastForward(500)
    expectBetween(page.at(4, 10))
    page.renderer.clockFastForward(1000)
    expect(page.at(4, 10)).toEqual([0, 0, 255])
    page.testRoot.unmount()
  })

  it("merges several variant names in order and hands custom to a variant function", () => {
    const page = setup()
    page.testRoot.render(
      <div style={PAGE}>
        <text testId="p" style={TEXT}>
          <motion.span
            style={{ paddingLeft: 12 }}
            custom={BLUE}
            variants={{
              base: { backgroundColor: RED },
              tinted: (colour: unknown) => ({ backgroundColor: colour as string }),
            }}
            initial={false}
            animate={["base", "tinted"]}
          >
            aaaa
          </motion.span>
        </text>
      </div>
    )
    expect(page.at(4, 10)).toEqual([0, 0, 255])
    page.testRoot.unmount()
  })

  describe("under AnimatePresence", () => {
    function App(props: { show: boolean }) {
      return (
        <div style={PAGE}>
          <text testId="p" style={TEXT}>
            <AnimatePresence>
              {props.show ? (
                <motion.span
                  key="chip"
                  style={{ paddingLeft: 12 }}
                  initial={{ backgroundColor: RED }}
                  animate={{ backgroundColor: RED }}
                  exit={{ backgroundColor: BLUE }}
                  transition={LINEAR}
                >
                  aaaa
                </motion.span>
              ) : null}
            </AnimatePresence>
          </text>
        </div>
      )
    }

    it("keeps a leaving span until its exit has run, then removes it", () => {
      const page = setup()
      page.testRoot.render(<App show />)
      expect(page.renderer.getAllText()).toContain("aaaa")
      expect(page.at(4, 10)).toEqual([255, 0, 0])

      page.testRoot.render(<App show={false} />)
      page.renderer.clockFastForward(500)
      page.renderer.dispatchNativeEvents()
      expect(page.renderer.getAllText(), "still mounted halfway through the exit").toContain("aaaa")
      expectBetween(page.at(4, 10))

      page.renderer.clockFastForward(1000)
      page.renderer.dispatchNativeEvents()
      page.renderer.flush()
      expect(page.renderer.getAllText()).not.toContain("aaaa")
      page.testRoot.unmount()
    })

    it("removes a leaving span that has no exit at once", async () => {
      const page = setup()
      const tree = (show: boolean) => (
        <div style={PAGE}>
          <text testId="p" style={TEXT}>
            <AnimatePresence>
              {show ? (
                <motion.span key="chip" animate={{ color: RED }}>
                  aaaa
                </motion.span>
              ) : null}
            </AnimatePresence>
          </text>
        </div>
      )
      page.testRoot.render(tree(true))
      page.testRoot.render(tree(false))
      await vi.waitFor(() => {
        page.renderer.flush()
        expect(page.renderer.getAllText()).not.toContain("aaaa")
      })
      page.testRoot.unmount()
    })

    it("plays an exit named by a variant, with that variant's transition", () => {
      const page = setup()
      const tree = (show: boolean) => (
        <div style={PAGE}>
          <text testId="p" style={TEXT}>
            <AnimatePresence>
              {show ? (
                <motion.span
                  key="chip"
                  style={{ paddingLeft: 12 }}
                  variants={{
                    here: { backgroundColor: RED },
                    gone: { backgroundColor: BLUE, transition: LINEAR },
                  }}
                  initial="here"
                  animate="here"
                  exit="gone"
                >
                  aaaa
                </motion.span>
              ) : null}
            </AnimatePresence>
          </text>
        </div>
      )
      page.testRoot.render(tree(true))
      page.testRoot.render(tree(false))
      page.renderer.clockFastForward(500)
      page.renderer.dispatchNativeEvents()
      expect(page.renderer.getAllText()).toContain("aaaa")
      expectBetween(page.at(4, 10))
      page.renderer.clockFastForward(1000)
      page.renderer.dispatchNativeEvents()
      page.renderer.flush()
      expect(page.renderer.getAllText()).not.toContain("aaaa")
      page.testRoot.unmount()
    })
  })

  describe("what a span cannot do is an error", () => {
    /** The error a render throws. */
    function renderError(node: React.ReactNode): Error {
      const errors: unknown[] = []
      const testRoot = createTestRoot({ onUncaughtError: (error) => errors.push(error) })
      try {
        testRoot.render(node)
      } catch (error) {
        errors.push(error)
      }
      testRoot.unmount()
      expect(errors, "the render fails").toHaveLength(1)
      expect(errors[0]).toBeInstanceOf(Error)
      return errors[0] as Error
    }

    it.each([
      ["animate", { animate: { width: 100 } }, 'cannot animate "width" in animate'],
      ["initial", { initial: { height: 4 }, animate: { color: RED } }, 'cannot animate "height" in initial'],
      ["exit", { exit: { width: 10 } }, 'cannot animate "width" in exit'],
      [
        "variants",
        { variants: { big: { flexGrow: 1 } }, animate: { color: RED } },
        'cannot animate "flexGrow" in the variant "big"',
      ],
      ["whileHover", { whileHover: { width: 10 } }, 'cannot animate "width" in whileHover'],
    ])("a key of %s that a span cannot animate", (_name, props, message) => {
      const error = renderError(
        <div>
          <text>
            <motion.span {...(props as object)}>aaaa</motion.span>
          </text>
        </div>
      )
      expect(error.message).toContain(message)
      expect(error.message).toContain("inline box")
    })

    it("a key of an exit function that a span cannot animate", () => {
      const error = renderError(
        <text>
          <AnimatePresence>
            <motion.span key="chip" animate={{ color: RED }} exit={() => ({ width: 20 }) as never}>
              aaaa
            </motion.span>
          </AnimatePresence>
        </text>
      )
      expect(error.message).toContain('cannot animate "width" in exit')
    })

    it("a variant name that variants do not define", () => {
      const error = renderError(
        <text>
          <motion.span animate="shown" variants={{ hidden: { color: RED } }}>
            aaaa
          </motion.span>
        </text>
      )
      expect(error.message).toContain('names the variant "shown"')
    })
  })
})
