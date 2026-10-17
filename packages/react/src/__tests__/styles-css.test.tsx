import fs from "fs"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { expectScreenshotsDiffer, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type TestRenderer = ReturnType<typeof createTestRoot>["renderer"]

function rect(renderer: TestRenderer, testId: string) {
  const element = renderer.findByTestId(testId)
  expect(element, `missing testId ${testId}`).toBeDefined()
  const bounds = renderer.getElementBounds(element!.id)
  expect(bounds, `no painted bounds for ${testId}`).toBeTruthy()
  return bounds!
}

function screenshot(renderer: TestRenderer, name: string): string {
  const path = `${SHOTS_DIR}/gpuix-css-${name}.png`
  if (fs.existsSync(path)) fs.unlinkSync(path)
  renderer.captureScreenshot(path)
  return path
}

describeNative("aspectRatio", () => {
  it("derives the height from the width", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      render(<div testId="box" style={{ width: 200, aspectRatio: 2 }} />)
      expect(rect(renderer, "box")).toMatchObject({ width: 200, height: 100 })
    } finally {
      unmount()
    }
  })

  it("derives the width from the height", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      render(<div testId="box" style={{ height: 60, aspectRatio: 3 }} />)
      expect(rect(renderer, "box")).toMatchObject({ width: 180, height: 60 })
    } finally {
      unmount()
    }
  })

  it("ignores a ratio that is not positive", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      render(<div testId="box" style={{ width: 200, height: 50, aspectRatio: 0 }} />)
      expect(rect(renderer, "box")).toMatchObject({ width: 200, height: 50 })
    } finally {
      unmount()
    }
  })
})

describeNative("justifyContent: space-evenly", () => {
  it("puts equal space before, between and after the children", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      render(
        <div style={{ display: "flex", flexDirection: "row", width: 300, justifyContent: "space-evenly" }}>
          {["a", "b", "c"].map((id) => (
            <div key={id} testId={id} style={{ width: 30, height: 10 }} />
          ))}
        </div>
      )
      // 300 - 90 = 210 free, in 4 equal gaps of 52.5. Layout rounds to whole pixels.
      const [a, b, c] = ["a", "b", "c"].map((id) => rect(renderer, id).x) as [number, number, number]
      expect(Math.abs(a - 52.5)).toBeLessThanOrEqual(1)
      expect(Math.abs(b - a - 82.5)).toBeLessThanOrEqual(1)
      expect(Math.abs(c - b - 82.5)).toBeLessThanOrEqual(1)
    } finally {
      unmount()
    }
  })
})

describeNative("alignItems", () => {
  const row = (alignItems: string) => (
    <div style={{ display: "flex", flexDirection: "row", width: 200, height: 100, alignItems }}>
      <div testId="child" style={{ width: 20 }} />
    </div>
  )

  it("stretch fills the cross axis, which center does not", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      render(row("stretch"))
      expect(rect(renderer, "child").height).toBe(100)
      render(row("center"))
      expect(rect(renderer, "child").height).toBeLessThan(100)
    } finally {
      unmount()
    }
  })
})

describeNative("alignItems: baseline", () => {
  it("lines up the text of different sizes on one baseline", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      const row = (alignItems: string) => (
        <div style={{ display: "flex", flexDirection: "row", width: 300, height: 100, alignItems }}>
          <div testId="big" style={{ fontSize: 40 }}>
            Big
          </div>
          <div testId="small" style={{ fontSize: 12 }}>
            small
          </div>
        </div>
      )
      render(row("start"))
      expect(rect(renderer, "small").y).toBe(rect(renderer, "big").y)

      render(row("baseline"))
      const big = rect(renderer, "big")
      const small = rect(renderer, "small")
      // The small text drops so that its baseline meets the big text's: it has less below the baseline, so its
      // bottom edge is above the big text's (the exact numbers are in text-baselines.test.tsx).
      expect(small.y).toBeGreaterThan(big.y)
      expect(small.y + small.height).toBeLessThan(big.y + big.height)
    } finally {
      unmount()
    }
  })
})

describeNative("borderStyle and boxShadow", () => {
  const frame = (style: object) => (
    <div style={{ width: 300, height: 200, backgroundColor: "#11111b", padding: 40 }}>
      <div style={{ width: 120, height: 80, borderRadius: 8, backgroundColor: "#1e1e2e", ...style }} />
    </div>
  )
  const shadow = { offsetX: 0, offsetY: 6, blurRadius: 12, spreadRadius: 0, color: "#000000cc" }

  it("draws a dashed border differently from a solid one", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      const border = { borderWidth: 3, borderColor: "#89b4fa" }
      render(frame({ ...border, borderStyle: "solid" }))
      const solid = screenshot(renderer, "border-solid")
      render(frame({ ...border, borderStyle: "dashed" }))
      expectScreenshotsDiffer(solid, screenshot(renderer, "border-dashed"))
    } finally {
      unmount()
    }
  })

  it("paints every shadow in a list", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      render(frame({ boxShadow: shadow }))
      const one = screenshot(renderer, "shadow-one")
      render(frame({ boxShadow: [shadow, { ...shadow, offsetY: -12, color: "#f38ba8cc" }] }))
      expectScreenshotsDiffer(one, screenshot(renderer, "shadow-two"))
    } finally {
      unmount()
    }
  })

  it("paints an inset shadow differently from an outer one", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      render(frame({ boxShadow: shadow }))
      const outer = screenshot(renderer, "shadow-outer")
      render(frame({ boxShadow: { ...shadow, inset: true } }))
      expectScreenshotsDiffer(outer, screenshot(renderer, "shadow-inset"))
    } finally {
      unmount()
    }
  })

  it("keeps a valid shadow when another one in the list has a bad colour", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      render(frame({}))
      const none = screenshot(renderer, "shadow-none")
      render(frame({ boxShadow: [{ ...shadow, color: "not-a-colour" }, shadow] }))
      expectScreenshotsDiffer(none, screenshot(renderer, "shadow-partly-bad"))
    } finally {
      unmount()
    }
  })
})
