import { describe, expect, it } from "bun:test"
import { defineKeyframes } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

defineKeyframes("solid-svg-draw", { from: { strokeDashoffset: 100 }, to: { strokeDashoffset: 0 } })
defineKeyframes("solid-svg-host", { from: { strokeWidth: 2, fill: "#ff0000" }, to: { strokeWidth: 6, fill: "#0000ff" } })

/** The value of attribute `name` on the element with `id` in the painted XML. */
function attributeOf(markup: string, id: string, name: string): string | undefined {
  const tag = new RegExp(`<[\\w]+\\s[^>]*\\bid="${id}"[^>]*>`).exec(markup)
  if (!tag) throw new Error(`no element with id ${id} in ${markup}`)
  return new RegExp(`\\s${name}="([^"]*)"`).exec(tag[0])?.[1]
}

describe.skipIf(!hasNativeTestRenderer)("Solid SVG keyframes", () => {
  it("draws a path in with stroke-dashoffset, and the svg's own values reach its shapes", () => {
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <svg
        id="svg"
        testId="svg"
        width={100}
        height={100}
        viewBox="0 0 100 100"
        style={{ width: 100, height: 100, animation: "solid-svg-host 1s linear" }}
      >
        <path
          id="p"
          d="M0 50 L100 50"
          stroke="#ffffff"
          strokeDasharray={100}
          style={{ animation: "solid-svg-draw 1s linear" }}
        />
      </svg>
    ))
    const read = (name: string, id: string) => {
      app.renderer.dispatchNativeEvents()
      return attributeOf(app.renderer.getSvgMarkup(app.renderer.findByTestId("svg")!.id)!, id, name)
    }
    expect(read("stroke-dashoffset", "p")).toBe("100")
    app.renderer.clockFastForward(500)
    expect(read("stroke-dashoffset", "p")).toBe("50")
    expect(read("stroke-width", "svg")).toBe("4")
    expect(read("fill", "svg")).toMatch(/^rgba\(127\.5/)
    app.renderer.clockFastForward(1000)
    expect(read("stroke-dashoffset", "p")).toBeUndefined()
  })
})
