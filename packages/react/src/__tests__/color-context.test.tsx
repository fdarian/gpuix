import type { ReactElement } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rgba = [number, number, number, number]

/** Renders `tree` in a fresh window and returns a sampler for logical pixels. */
function paint(name: string, tree: ReactElement, appearance?: string) {
  const testRoot = createTestRoot()
  try {
    testRoot.render(tree)
    const file = `${SHOTS_DIR}/color-context-${name}.png`
    testRoot.renderer.captureScreenshot(file)
    const pixels = readLogicalPixels(file, testRoot.renderer)
    return {
      at: (x: number, y: number) => pixels.at(x, y),
      appearance: appearance ?? testRoot.renderer.getAppearance(),
    }
  } finally {
    testRoot.unmount()
  }
}

function expectNear(actual: Rgba, expected: Rgba, tolerance = 1) {
  for (let i = 0; i < 4; i++) {
    expect(Math.abs(actual[i]! - expected[i]!), `${actual} vs ${expected}`).toBeLessThanOrEqual(
      tolerance
    )
  }
}

const RED: Rgba = [255, 0, 0, 255]
const BLUE: Rgba = [0, 0, 255, 255]

/** A 100x100 box at the window origin filled with `backgroundColor`. */
function fill(backgroundColor: string, extra: Record<string, unknown> = {}) {
  return (
    <div style={{ width: 100, height: 100, backgroundColor, ...extra }} />
  )
}

describeNative("currentColor", () => {
  it("takes the element's own color in a border", () => {
    const shot = paint(
      "border",
      <div style={{ width: 100, height: 100, color: "#ff0000", borderWidth: 10, borderStyle: "solid", borderColor: "currentColor" }} />
    )
    expectNear(shot.at(3, 50), RED)
  })

  it("inherits the color of an ancestor", () => {
    const shot = paint(
      "inherited",
      <div style={{ color: "#0000ff" }}>
        <div>
          <div style={{ width: 100, height: 100, backgroundColor: "currentColor" }} />
        </div>
      </div>
    )
    expectNear(shot.at(50, 50), BLUE)
  })

  it("resolves in a shadow and an outline", () => {
    const shot = paint(
      "shadow-outline",
      <div style={{ padding: 30, color: "#ff0000" }}>
        <div
          style={{
            width: 40,
            height: 40,
            boxShadow: { offsetX: 0, offsetY: 0, blurRadius: 0, spreadRadius: 10, color: "currentColor" },
          }}
        />
      </div>
    )
    expectNear(shot.at(25, 50), RED)

    const outlined = paint(
      "outline",
      <div style={{ padding: 30, color: "#0000ff" }}>
        <div style={{ width: 40, height: 40, outlineWidth: 6, outlineColor: "currentColor" }} />
      </div>
    )
    expectNear(outlined.at(27, 50), BLUE)
  })

  it("follows a color declared on the same element", () => {
    const shot = paint("same-element", fill("currentColor", { color: "#ff0000" }))
    expectNear(shot.at(50, 50), RED)
  })

  it("tints an svg", () => {
    const icon =
      '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect width="16" height="16" fill="currentColor"/></svg>'
    const shot = paint("svg", <svg source={icon} style={{ width: 64, height: 64, color: "currentColor" }} />)
    // The root colour is black; the svg asked for currentColor.
    expectNear(shot.at(32, 32), [0, 0, 0, 255])
    const tinted = paint(
      "svg-tinted",
      <div style={{ color: "#ff0000" }}>
        <svg source={icon} style={{ width: 64, height: 64, color: "currentColor" }} />
      </div>
    )
    expectNear(tinted.at(32, 32), RED)
  })

  it("applies the state color to the state's own currentColor", () => {
    const testRoot = createTestRoot()
    try {
      testRoot.render(
        <div
          style={{
            width: 100,
            height: 100,
            color: "#0000ff",
            borderWidth: 10,
            borderStyle: "solid",
            borderColor: "currentColor",
            hover: { color: "#ff0000" },
          }}
        />
      )
      testRoot.renderer.nativeSimulateMouseMove(50, 50)
      const file = `${SHOTS_DIR}/color-context-hover.png`
      testRoot.renderer.captureScreenshot(file)
      expectNear(readLogicalPixels(file, testRoot.renderer).at(3, 50), RED)
    } finally {
      testRoot.unmount()
    }
  })
})

describeNative("light-dark and colorScheme", () => {
  const value = "light-dark(#ff0000, #0000ff)"

  it("is the light side by default and under normal", () => {
    expectNear(paint("ld-default", fill(value)).at(50, 50), RED)
    expectNear(paint("ld-normal", fill(value, { colorScheme: "normal" })).at(50, 50), RED)
  })

  it("picks the dark side under dark and only dark", () => {
    expectNear(paint("ld-dark", fill(value, { colorScheme: "dark" })).at(50, 50), BLUE)
    expectNear(paint("ld-only-dark", fill(value, { colorScheme: "only dark" })).at(50, 50), BLUE)
    expectNear(paint("ld-only-light", fill(value, { colorScheme: "only light" })).at(50, 50), RED)
  })

  it("inherits colorScheme from an ancestor", () => {
    const shot = paint(
      "ld-inherited",
      <div style={{ colorScheme: "dark" }}>
        <div>{fill(value)}</div>
      </div>
    )
    expectNear(shot.at(50, 50), BLUE)
  })

  it("lets a descendant override the scheme", () => {
    const shot = paint(
      "ld-override",
      <div style={{ colorScheme: "dark" }}>{fill(value, { colorScheme: "light" })}</div>
    )
    expectNear(shot.at(50, 50), RED)
  })

  it("follows the window appearance under light dark", () => {
    const shot = paint("ld-follow", fill(value, { colorScheme: "light dark" }))
    expectNear(shot.at(50, 50), shot.appearance === "dark" ? BLUE : RED)
    const reversed = paint("ld-follow-reversed", fill(value, { colorScheme: "dark light" }))
    expectNear(reversed.at(50, 50), reversed.appearance === "dark" ? BLUE : RED)
  })

  it("resolves the element's own colorScheme for its own color", () => {
    const shot = paint(
      "ld-color",
      <div style={{ colorScheme: "dark", color: value }}>
        <div style={{ width: 100, height: 100, backgroundColor: "currentColor" }} />
      </div>
    )
    expectNear(shot.at(50, 50), BLUE)
  })

  it("ignores an invalid light-dark like any invalid colour", () => {
    const invalid = paint("ld-invalid", fill("light-dark(#ff0000)"))
    const unset = paint("ld-unset", <div style={{ width: 100, height: 100 }} />)
    expect(invalid.at(50, 50)).toEqual(unset.at(50, 50))
    const badScheme = paint("ld-bad-scheme", fill(value, { colorScheme: "sepia" }))
    expectNear(badScheme.at(50, 50), RED)
  })
})

describeNative("color-mix and color()", () => {
  it("mixes in srgb", () => {
    expectNear(paint("mix-srgb", fill("color-mix(in srgb, red, blue)")).at(50, 50), [128, 0, 128, 255])
    expectNear(paint("mix-srgb-pct", fill("color-mix(in srgb, red 25%, blue)")).at(50, 50), [64, 0, 191, 255])
  })

  it("mixes in oklab like a browser", () => {
    // color-mix(in oklab, white, black) is rgb(99 99 99) in every browser.
    expectNear(paint("mix-oklab", fill("color-mix(in oklab, white, black)")).at(50, 50), [99, 99, 99, 255])
  })

  it("takes the shorter hue arc by default and the longer on request", () => {
    const shorter = paint("mix-hsl", fill("color-mix(in hsl, hsl(350 100% 50%), hsl(10 100% 50%))"))
    expectNear(shorter.at(50, 50), RED)
    const longer = paint("mix-hsl-longer", fill("color-mix(in hsl longer hue, hsl(350 100% 50%), hsl(10 100% 50%))"))
    expectNear(longer.at(50, 50), [0, 255, 255, 255])
  })

  it("mixes alpha premultiplied", () => {
    const shot = paint("mix-alpha", fill("color-mix(in srgb, red, transparent)"))
    // Half-transparent red over the window's clear colour: red channel stays saturated.
    const pixel = shot.at(50, 50)
    const plain = paint("mix-alpha-ref", fill("rgb(255 0 0 / 50%)")).at(50, 50)
    expectNear(pixel, plain)
  })

  it("nests with currentColor and light-dark", () => {
    const shot = paint(
      "mix-nested",
      <div style={{ color: "#0000ff", colorScheme: "dark" }}>
        {fill("color-mix(in srgb, currentColor, light-dark(#00ff00, #ff0000))")}
      </div>
    )
    expectNear(shot.at(50, 50), [128, 0, 128, 255])
  })

  it("converts color() spaces to sRGB and clips", () => {
    expectNear(paint("fn-srgb", fill("color(srgb 1 0 0)")).at(50, 50), RED)
    expectNear(paint("fn-p3", fill("color(display-p3 1 0 0)")).at(50, 50), RED)
    expectNear(paint("fn-linear", fill("color(srgb-linear 0.5 0.5 0.5)")).at(50, 50), [188, 188, 188, 255])
    expectNear(paint("fn-rec2020", fill("color(rec2020 1 1 1)")).at(50, 50), [255, 255, 255, 255])
  })

  it("rejects invalid mixes", () => {
    const unset = paint("mix-unset", <div style={{ width: 100, height: 100 }} />)
    for (const bad of ["color-mix(in srgb, red)", "color-mix(in nowhere, red, blue)", "color(nowhere 1 0 0)"]) {
      expect(paint("mix-bad", fill(bad)).at(50, 50)).toEqual(unset.at(50, 50))
    }
  })
})
