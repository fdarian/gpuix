import { describe, expect, it } from "vitest"
import type { EventPayload } from "@gpuix/native"
import { defineKeyframes } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

defineKeyframes("react-grow", { from: { width: 100 }, to: { width: 200 } })

describeNative("CSS animations", () => {
  const widthOf = (
    renderer: ReturnType<typeof createTestRoot>["renderer"],
    testId = "box"
  ): number => {
    renderer.dispatchNativeEvents()
    return renderer.getElementBounds(renderer.findByTestId(testId).id).width
  }

  it("repeats a spinner forever", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    render(
      <div
        testId="box"
        style={{ width: 50, height: 20, animation: "react-grow 1s linear infinite" }}
      />
    )
    expect(widthOf(renderer)).toBe(100)

    renderer.clockFastForward(500)
    expect(widthOf(renderer)).toBeCloseTo(150, 0)
    renderer.clockFastForward(750)
    expect(widthOf(renderer)).toBeCloseTo(125, 0)
    renderer.clockFastForward(10_000)
    expect(widthOf(renderer)).toBeCloseTo(125, 0)
  })

  it("reaches the declared width when the animation ends, unless fill-mode forwards holds the last keyframe", () => {
    const { render, renderer } = createTestRoot()
    const box = (animation: string) => (
      <div testId="box" style={{ width: 50, height: 20, animation }} />
    )
    renderer.clockPause()
    render(box("react-grow 1s linear"))
    renderer.clockFastForward(500)
    expect(widthOf(renderer)).toBeCloseTo(150, 0)
    renderer.clockFastForward(1000)
    expect(widthOf(renderer)).toBe(50)

    const held = createTestRoot()
    held.renderer.clockPause()
    held.render(box("react-grow 1s linear forwards"))
    held.renderer.clockFastForward(2000)
    expect(widthOf(held.renderer)).toBe(200)
  })

  it("fills backwards during the delay and honours alternate direction and a negative delay", () => {
    const delayed = createTestRoot()
    delayed.renderer.clockPause()
    delayed.render(
      <div
        testId="box"
        style={{ width: 50, height: 20, animation: "react-grow 1s linear 1s backwards" }}
      />
    )
    expect(widthOf(delayed.renderer)).toBe(100)

    const alternating = createTestRoot()
    alternating.renderer.clockPause()
    alternating.render(
      <div
        testId="box"
        style={{ width: 50, height: 20, animation: "react-grow 1s linear 2 alternate" }}
      />
    )
    alternating.renderer.clockFastForward(1250)
    expect(widthOf(alternating.renderer)).toBeCloseTo(175, 0)

    const early = createTestRoot()
    early.renderer.clockPause()
    early.render(
      <div
        testId="box"
        style={{ width: 50, height: 20, animation: "react-grow 1s linear -500ms forwards" }}
      />
    )
    expect(widthOf(early.renderer)).toBeCloseTo(150, 0)
  })

  it("pauses and resumes with animationPlayState", () => {
    const { render, renderer } = createTestRoot()
    const box = (animationPlayState: string) => (
      <div
        testId="box"
        style={{
          width: 50,
          height: 20,
          animation: "react-grow 1s linear infinite",
          animationPlayState,
        }}
      />
    )
    renderer.clockPause()
    render(box("running"))
    renderer.clockFastForward(250)
    render(box("paused"))
    renderer.clockFastForward(5000)
    expect(widthOf(renderer)).toBeCloseTo(125, 0)

    render(box("running"))
    renderer.clockFastForward(250)
    expect(widthOf(renderer)).toBeCloseTo(150, 0)
  })

  it("fires start, iteration and end in order with the DOM event fields", () => {
    const { render, renderer } = createTestRoot()
    const seen: Array<[string, string | undefined, number | undefined]> = []
    const record = (type: string) => (event: EventPayload) =>
      seen.push([type, event.animationName, event.elapsedTime])
    renderer.clockPause()
    render(
      <div
        testId="box"
        onAnimationStart={record("start")}
        onAnimationIteration={record("iteration")}
        onAnimationEnd={record("end")}
        onAnimationCancel={record("cancel")}
        style={{ width: 50, height: 20, animation: "react-grow 1s linear 500ms 2" }}
      />
    )
    renderer.dispatchNativeEvents()
    expect(seen).toEqual([])

    renderer.clockFastForward(600)
    renderer.dispatchNativeEvents()
    expect(seen).toEqual([["start", "react-grow", 0]])

    renderer.clockFastForward(1000)
    renderer.dispatchNativeEvents()
    expect(seen.map(([type]) => type)).toEqual(["start", "iteration"])
    expect(seen[1]).toEqual(["iteration", "react-grow", 1])

    renderer.clockFastForward(1000)
    renderer.dispatchNativeEvents()
    expect(seen.map(([type]) => type)).toEqual(["start", "iteration", "end"])
    expect(seen[2]).toEqual(["end", "react-grow", 2])
  })

  it("fires cancel when the animation is removed before it ends, and none after it ended", () => {
    const { render, renderer } = createTestRoot()
    const seen: string[] = []
    const box = (animation: string) => (
      <div
        testId="box"
        onAnimationStart={() => seen.push("start")}
        onAnimationEnd={() => seen.push("end")}
        onAnimationCancel={(event) => seen.push(`cancel ${event.animationName} ${event.elapsedTime}`)}
        style={{ width: 50, height: 20, animation }}
      />
    )
    renderer.clockPause()
    render(box("react-grow 1s linear"))
    renderer.clockFastForward(500)
    render(box("none"))
    renderer.clockFastForward(16)
    renderer.dispatchNativeEvents()
    expect(seen).toEqual(["start", "cancel react-grow 0.5"])
    expect(widthOf(renderer)).toBe(50)

    seen.length = 0
    render(box("react-grow 1s linear"))
    renderer.clockFastForward(1500)
    render(box("none"))
    renderer.clockFastForward(16)
    renderer.dispatchNativeEvents()
    expect(seen).toEqual(["start", "end"])
  })

  it("animates nothing for a name no keyframes were defined for", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    render(
      <div
        testId="box"
        style={{ width: 50, height: 20, animation: "react-never-defined 1s linear infinite" }}
      />
    )
    renderer.clockFastForward(500)
    expect(widthOf(renderer)).toBe(50)
  })

  it("starts an animation that a hover style declares, and drops it when the pointer leaves", () => {
    const { render, renderer } = createTestRoot()
    const seen: string[] = []
    renderer.clockPause()
    render(
      <div
        testId="box"
        onAnimationStart={() => seen.push("start")}
        onAnimationCancel={() => seen.push("cancel")}
        style={{
          width: 50,
          height: 40,
          hover: { animation: "react-grow 1s linear forwards" },
        }}
      />
    )
    const bounds = renderer.getElementBounds(renderer.findByTestId("box").id)
    expect(widthOf(renderer)).toBe(50)

    renderer.nativeSimulateMouseMove(bounds.x + 10, bounds.y + 10)
    renderer.clockFastForward(500)
    expect(widthOf(renderer)).toBeCloseTo(150, 0)
    renderer.clockFastForward(1000)
    expect(widthOf(renderer)).toBe(200)
    expect(seen).toEqual(["start"])

    renderer.nativeSimulateMouseMove(bounds.x + 500, bounds.y + 300)
    renderer.clockFastForward(16)
    expect(widthOf(renderer)).toBe(50)
    expect(seen).toEqual(["start"])
  })

  it("rejects an invalid animation when the style is set", () => {
    const { render } = createTestRoot()
    expect(() =>
      render(<div testId="box" style={{ width: 50, height: 20, animation: "react-grow banana" }} />)
    ).toThrow()
  })
})
