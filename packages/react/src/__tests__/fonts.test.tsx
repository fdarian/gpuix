import fs from "fs"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const fixture = (name: string) => fs.readFileSync(new URL(`./fixtures/${name}`, import.meta.url))

// "A" is one em wide and "B" two, both drawn as squares, so "AB" at 20px is 60
// wide in this font and nowhere near that in
// the system fonts it falls back to.
const SQUARE_FONT = fixture("gpuix-test-square.ttf")

const row = (family: string) => (
  <div style={{ display: "flex", flexDirection: "row", width: 400 }}>
    <div testId="word" style={{ fontFamily: family, fontSize: 20 }}>
      AB
    </div>
  </div>
)

type TestRenderer = ReturnType<typeof createTestRoot>["renderer"]

function expectSquareWidth(renderer: TestRenderer) {
  expect(Math.abs(wordWidth(renderer) - 60)).toBeLessThanOrEqual(1)
}

function wordWidth(renderer: TestRenderer) {
  const word = renderer.findByTestId("word")
  expect(word).toBeDefined()
  return renderer.getElementBounds(word!.id)!.width
}

describeNative("registerFont", () => {
  it("makes a font that is not installed available to fontFamily", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      render(row("GpuixTestSquare"))
      expect(
        Math.abs(wordWidth(renderer) - 60),
        "an unregistered family falls back to another font",
      ).toBeGreaterThan(5)

      renderer.registerFont(SQUARE_FONT)
      expectSquareWidth(renderer)
    } finally {
      unmount()
    }
  })

  it("uses a font registered before any text asks for it", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      renderer.registerFont(SQUARE_FONT)
      render(row("GpuixTestSquare"))
      expectSquareWidth(renderer)
    } finally {
      unmount()
    }
  })

  it("rejects bytes that are not a font and keeps working", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      expect(() => renderer.registerFont(new Uint8Array([1, 2, 3, 4]))).toThrow(/not a TrueType/)
      render(row("GpuixTestSquare"))
      renderer.registerFont(SQUARE_FONT)
      expectSquareWidth(renderer)
    } finally {
      unmount()
    }
  })

  it("rejects a font GPUI would abort on instead of crashing the process", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      render(row("GpuixTestNameless"))
      expect(() => renderer.registerFont(fixture("gpuix-test-nameless.ttf"))).toThrow(/full name/)
      expect(wordWidth(renderer)).toBeGreaterThan(0)
    } finally {
      unmount()
    }
  })
})
