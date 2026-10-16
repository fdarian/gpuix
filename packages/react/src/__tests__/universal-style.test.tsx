import React, { useState } from "react"
import { describe, expect, it, vi } from "vitest"
import { UniversalStyle } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("UniversalStyle", () => {
  type Harness = ReturnType<typeof createTestRoot>

  const sizeOf = (renderer: Harness["renderer"], testId: string) => {
    const bounds = renderer.getElementBounds(renderer.findByTestId(testId).id)
    return [bounds.width, bounds.height]
  }

  /** A row that keeps its children at their own height, so a size comes from a style. */
  const row = (...children: React.ReactNode[]) =>
    React.createElement(
      "div",
      { style: { display: "flex", alignItems: "flex-start", width: 300, height: 100 } },
      ...children
    )

  it("sets the layer on mount, changes it with the prop and clears it on unmount", () => {
    const { render, renderer, unmount } = createTestRoot({ width: 300, height: 100 })
    const tree = (style: object | null) =>
      row(
        style && <UniversalStyle key="layer" style={style} />,
        <div key="bare" testId="bare" />,
        <div key="own" testId="own" style={{ width: 70 }} />
      )

    render(tree(null))
    expect(sizeOf(renderer, "bare")).toEqual([0, 0])

    render(tree({ width: 30, height: 10, flexShrink: 0 }))
    expect(sizeOf(renderer, "bare")).toEqual([30, 10])
    expect(sizeOf(renderer, "own")).toEqual([70, 10])

    render(tree({ width: 40, height: 20, flexShrink: 0 }))
    expect(sizeOf(renderer, "bare")).toEqual([40, 20])
    expect(sizeOf(renderer, "own")).toEqual([70, 20])

    render(tree(null))
    expect(sizeOf(renderer, "bare")).toEqual([0, 0])
    expect(sizeOf(renderer, "own")).toEqual([70, 0])
    unmount()
  })

  it("reaches an element mounted after the layer", () => {
    const { render, renderer, unmount } = createTestRoot({ width: 300, height: 100 })
    const tree = (late: boolean) =>
      row(
        <UniversalStyle style={{ width: 30, height: 10, flexShrink: 0 }} />,
        late && <div key="late" testId="late" />
      )
    render(tree(false))
    render(tree(true))
    expect(sizeOf(renderer, "late")).toEqual([30, 10])
    unmount()
  })

  it("does not resend a layer that an inline style object repeats", () => {
    const { render, renderer, unmount } = createTestRoot({ width: 300, height: 100 })
    const sent = vi.spyOn(renderer, "applyBatch")
    function Counter() {
      const [count, setCount] = useState(0)
      return row(
        <UniversalStyle style={{ width: 30, height: 10 }} />,
        <div testId="bare" onClick={() => setCount(count + 1)}>{count}</div>
      )
    }
    render(<Counter />)
    const layers = () =>
      sent.mock.calls.filter(([json]) => json.includes("setUniversalStyle")).length
    expect(layers()).toBe(1)
    renderer.clickElement(renderer.findByTestId("bare").id)
    renderer.dispatchNativeEvents()
    expect(layers()).toBe(1)
    expect(renderer.getAllText()).toContain("1")
    unmount()
  })

  it("stacks: a later component wins a shared property, and unmounting one leaves the other", () => {
    const { render, renderer, unmount } = createTestRoot({ width: 300, height: 100 })
    const tree = (first: boolean, second: boolean) =>
      row(
        first && <UniversalStyle key="first" style={{ width: 30, height: 10, flexShrink: 0 }} />,
        second && <UniversalStyle key="second" style={{ width: 50 }} />,
        <div key="bare" testId="bare" />
      )

    render(tree(true, true))
    expect(sizeOf(renderer, "bare")).toEqual([50, 10])

    render(tree(false, true))
    expect(sizeOf(renderer, "bare")[0]).toBe(50)
    expect(sizeOf(renderer, "bare")[1]).toBe(0)

    render(tree(true, true))
    render(tree(true, false))
    expect(sizeOf(renderer, "bare")).toEqual([30, 10])

    render(tree(false, false))
    expect(sizeOf(renderer, "bare")).toEqual([0, 0])
    unmount()
  })

  it("fades every element through an appearance flip, one mounted after the layer included", () => {
    const { render, renderer, unmount } = createTestRoot({ width: 120, height: 60, appearance: "light" })
    const box = (testId: string) => (
      <div
        key={testId}
        testId={testId}
        style={{
          width: 20,
          height: 20,
          colorScheme: "light dark",
          backgroundColor: "light-dark(#ff0000, #0000ff)",
        }}
      />
    )
    const tree = (late: boolean) => (
      <div style={{ display: "flex", gap: 20, width: 120, height: 60 }}>
        <UniversalStyle style={{ transition: "background-color 1s linear" }} />
        {box("early")}
        {late && box("late")}
      </div>
    )
    const pixel = (testId: string) => {
      const file = `${SHOTS_DIR}/universal-style-${testId}.png`
      renderer.captureScreenshot(file)
      const bounds = renderer.getElementBounds(renderer.findByTestId(testId).id)
      return readLogicalPixels(file, renderer).at(bounds.x + 10, bounds.y + 10).slice(0, 3)
    }

    renderer.clockPause()
    render(tree(false))
    render(tree(true))
    expect(pixel("early")).toEqual([255, 0, 0])
    expect(pixel("late")).toEqual([255, 0, 0])

    renderer.setAppearance("dark")
    renderer.clockFastForward(500)
    for (const testId of ["early", "late"]) {
      const middle = pixel(testId)
      expect(Math.abs(middle[0] - 128), `${testId} ${middle}`).toBeLessThan(8)
      expect(Math.abs(middle[2] - 128), `${testId} ${middle}`).toBeLessThan(8)
    }

    renderer.clockFastForward(1000)
    expect(pixel("early")).toEqual([0, 0, 255])
    expect(pixel("late")).toEqual([0, 0, 255])
    unmount()
  })

  it("throws outside a root", () => {
    expect(() => UniversalStyle({ style: {} })).toThrow()
  })
})
