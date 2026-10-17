/// A text that did not change keeps the layout node it was measured in, and its measure with it. A text
/// that did change, or that has another font size, inherited style or container width, wraps again.
///
/// The text is GpuixTestSquare, which has no glyph but `A`, so the lines a text wraps to follow from the font size
/// and the width. Each state is compared with a window that draws it for the first time.

import fs from "fs"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const SQUARE_FONT = fs.readFileSync(new URL("./fixtures/gpuix-test-square.ttf", import.meta.url))

type Props = { width: number; fontSize: number; texts: string[]; color?: string }

const BASE: Props = {
  width: 100,
  fontSize: 10,
  texts: ["AAAA AAAA AAAA AAAA AAAA", "AA AA", "AAAAA AAAA AAA AAAAA AAAA AAAAAA"],
}

function Paragraphs({ props, refs }: { props: Props; refs: Map<number, PublicInstance> }) {
  return (
    <div
      style={{
        width: props.width,
        fontFamily: "GpuixTestSquare",
        fontSize: props.fontSize,
        lineHeight: `${props.fontSize}px`,
        color: props.color ?? "#000000",
      }}
    >
      {props.texts.map((text, index) => (
        <div
          key={index}
          ref={(instance) => {
            if (instance) refs.set(index, instance)
          }}
        >
          {text}
        </div>
      ))}
    </div>
  )
}

describeNative("text that is measured again", () => {
  /** The heights the texts of `props` have after each of `frames` draws of the same window, one after another. */
  function drawn(props: Props[], frames: number): number[][] {
    const test = createTestRoot()
    test.renderer.registerFont(SQUARE_FONT)
    const refs = new Map<number, PublicInstance>()
    try {
      return props.flatMap((frame) =>
        Array.from({ length: frames }, () => {
          test.render(<Paragraphs props={frame} refs={refs} />)
          test.renderer.dispatchNativeEvents()
          return frame.texts.map((_, index) => refs.get(index)?.getBoundingClientRect().height ?? Number.NaN)
        })
      )
    } finally {
      test.unmount()
    }
  }

  it("wraps to the edited text, the font size, the inherited style and the width of the container", () => {
    const steps: [string, (props: Props) => Props][] = [
      ["the first frame", (props) => props],
      ["a frame that changes nothing", (props) => ({ ...props })],
      [
        "an edited text",
        (props) => ({ ...props, texts: [props.texts[0] ?? "", "AA AA AA AA AA AA AA", props.texts[2] ?? ""] }),
      ],
      ["a larger font size, inherited", (props) => ({ ...props, fontSize: 20 })],
      ["a narrower container", (props) => ({ ...props, width: 60 })],
      ["a wider container", (props) => ({ ...props, width: 400 })],
      ["the first container again", (props) => ({ ...props, width: BASE.width })],
      ["another color, which does not wrap", (props) => ({ ...props, color: "#ff0000" })],
      ["the first font size again", (props) => ({ ...props, fontSize: BASE.fontSize })],
      ["a text removed", (props) => ({ ...props, texts: props.texts.slice(0, 2) })],
      ["a text added in front", (props) => ({ ...props, texts: ["AAAA AAAAA AAAAAA AAAA", ...props.texts] })],
    ]
    const frames: Props[] = []
    let props = BASE
    for (const [, change] of steps) {
      props = change(props)
      frames.push(props)
    }

    // Twice per change: the frame of the change, and a frame that finds the nodes it left.
    const kept = drawn(frames, 2)
    const distinct = new Set<string>()
    for (const [index, [name]] of steps.entries()) {
      const frame = frames[index] as Props
      const fresh = drawn([frame], 1)[0]
      expect(kept[index * 2], name).toEqual(fresh)
      expect(kept[index * 2 + 1], `${name}, again`).toEqual(fresh)
      distinct.add(JSON.stringify(fresh))
    }
    // The steps wrap differently from one another, or they would not tell a stale measure apart.
    expect(distinct.size).toBeGreaterThanOrEqual(7)
  })
})
