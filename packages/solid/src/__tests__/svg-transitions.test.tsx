import { createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import { CSSTransition } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { HostElement } from "../host.js"

/** The value of attribute `name` on the element with `id` in the painted XML. */
function attributeOf(markup: string, id: string, name: string): string | undefined {
  const tag = new RegExp(`<[\\w]+\\s[^>]*\\bid="${id}"[^>]*>`).exec(markup)
  if (!tag) throw new Error(`no element with id ${id} in ${markup}`)
  return new RegExp(`\\s${name}="([^"]*)"`).exec(tag[0])?.[1]
}

describe.skipIf(!hasNativeTestRenderer)("Solid SVG transitions", () => {
  const read = (app: ReturnType<typeof createTestRoot>, name: string, id: string) => {
    app.renderer.dispatchNativeEvents()
    return attributeOf(app.renderer.getSvgMarkup(app.renderer.findByTestId("svg")!.id)!, id, name)
  }

  it("transitions a signal-driven attribute of a shape, lists it and reports its events", () => {
    const [radius, setRadius] = createSignal(10)
    const seen: string[] = []
    let circle!: HostElement
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <svg id="svg" testId="svg" width={100} height={100} viewBox="0 0 100 100" style={{ width: 100, height: 100 }}>
        <circle
          id="c"
          ref={(element) => (circle = element)}
          cx={50}
          cy={50}
          r={radius()}
          style={{ transition: "r 1s linear" }}
          onTransitionRun={(event) => seen.push(`run ${event.propertyName}`)}
          onTransitionEnd={(event) => seen.push(`end ${event.propertyName} ${event.elapsedTime}`)}
        />
      </svg>
    ))
    app.renderer.clockFastForward(0)
    expect(read(app, "r", "c")).toBe("10")

    app.flushSync(() => setRadius(30))
    app.renderer.clockFastForward(500)
    expect(Number(read(app, "r", "c"))).toBeCloseTo(20, 1)
    const [transition] = circle.getAnimations()
    expect(transition).toBeInstanceOf(CSSTransition)
    expect((transition as CSSTransition).transitionProperty).toBe("r")
    expect(transition.currentTime).toBeCloseTo(500, 0)

    app.renderer.clockFastForward(1000)
    expect(read(app, "r", "c")).toBe("30")
    expect(circle.getAnimations()).toEqual([])
    expect(seen).toEqual(["run r", "end r 1"])
    app.unmount()
  })

  it("transitions an attribute of the svg, which its shapes inherit", () => {
    const [fill, setFill] = createSignal("#ff0000")
    const app = createTestRoot()
    app.renderer.clockPause()
    app.render(() => (
      <svg
        id="svg"
        testId="svg"
        width={100}
        height={100}
        viewBox="0 0 100 100"
        fill={fill()}
        style={{ width: 100, height: 100, transition: "fill 1s linear" }}
      >
        <circle id="c" cx={50} cy={50} r={20} />
      </svg>
    ))
    app.renderer.clockFastForward(0)
    app.flushSync(() => setFill("#0000ff"))
    app.renderer.clockFastForward(500)
    expect(read(app, "fill", "svg")).toMatch(/^rgba\(127\.5/)
    app.renderer.clockFastForward(1000)
    expect(read(app, "fill", "svg")).toBe("#0000ff")
    app.unmount()
  })
})
