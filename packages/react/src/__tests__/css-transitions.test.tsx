import fs from "fs"
import path from "path"
import type { ReactNode } from "react"
import { describe, expect, it } from "vitest"
import type { EventPayload } from "@gpuix/native"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("CSS transitions", () => {
  function widthOf(
    renderer: ReturnType<typeof createTestRoot>["renderer"],
    testId: string
  ): number {
    renderer.dispatchNativeEvents()
    return renderer.getElementBounds(renderer.findByTestId(testId).id).width
  }

  it("animates a style prop change from the old value to the new one", () => {
    const { render, renderer } = createTestRoot()
    const box = (width: number) => (
      <div
        testId="box"
        style={{ width, height: 20, transition: "width 1s linear" }}
      />
    )

    renderer.clockPause()
    render(box(100))
    expect(widthOf(renderer, "box")).toBe(100)

    render(box(200))
    renderer.clockFastForward(500)
    expect(widthOf(renderer, "box")).toBeCloseTo(150, 0)

    // Reversed half way: from the visible 150, and the way back is shortened
    // to half the duration, so it is 25 px in after a quarter second.
    render(box(100))
    renderer.clockFastForward(250)
    expect(widthOf(renderer, "box")).toBeCloseTo(125, 0)

    // Interrupted toward a new target: continues from the visible value.
    render(box(300))
    renderer.clockFastForward(500)
    expect(widthOf(renderer, "box")).toBeGreaterThan(211)
    expect(widthOf(renderer, "box")).toBeLessThan(214)

    renderer.clockFastForward(2000)
    expect(widthOf(renderer, "box")).toBe(300)
  })

  it("does not animate without a transition, or on the first frame", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    render(<div testId="box" style={{ width: 100, height: 20 }} />)
    render(<div testId="box" style={{ width: 200, height: 20 }} />)
    expect(widthOf(renderer, "box")).toBe(200)

    const mounted = createTestRoot()
    mounted.renderer.clockPause()
    mounted.render(
      <div testId="box" style={{ width: 80, height: 20, transition: "width 1s" }} />
    )
    expect(widthOf(mounted.renderer, "box")).toBe(80)
  })

  it("animates the hover style in and out with the pointer", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    render(
      <div
        testId="box"
        style={{
          width: 100,
          height: 40,
          transition: "width 1s linear",
          hover: { width: 200 },
        }}
      />
    )
    const bounds = renderer.getElementBounds(renderer.findByTestId("box").id)
    const inside = [bounds.x + 10, bounds.y + 10] as const

    renderer.nativeSimulateMouseMove(...inside)
    renderer.clockFastForward(500)
    expect(widthOf(renderer, "box")).toBeCloseTo(150, 0)
    renderer.clockFastForward(1000)
    expect(widthOf(renderer, "box")).toBe(200)

    renderer.nativeSimulateMouseMove(bounds.x + 500, bounds.y + 300)
    renderer.clockFastForward(500)
    expect(widthOf(renderer, "box")).toBeCloseTo(150, 0)
    renderer.clockFastForward(1000)
    expect(widthOf(renderer, "box")).toBe(100)
  })

  it("animates a transition declared only in groupHover", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    render(
      <div testId="card" style={{ width: 100, height: 100, group: true }}>
        <div
          testId="box"
          style={{
            width: 40,
            height: 20,
            groupHover: { width: 80, transition: "width 1s linear" },
          }}
        />
      </div>
    )
    renderer.nativeSimulateMouseMove(80, 80)
    renderer.clockFastForward(500)
    expect(widthOf(renderer, "box")).toBeCloseTo(60, 0)
    renderer.clockFastForward(1000)
    expect(widthOf(renderer, "box")).toBe(80)
  })

  it("cancels a running transition on display none and starts none when shown with a new value", () => {
    const { render, renderer } = createTestRoot()
    const seen: string[] = []
    const box = (width: number, display?: "none") => (
      <div
        testId="box"
        onTransitionRun={() => seen.push("run")}
        onTransitionCancel={(event) => seen.push(`cancel ${event.elapsedTime}`)}
        style={{ width, height: 20, display, transition: "width 1s linear" }}
      />
    )
    renderer.clockPause()
    render(box(100))
    render(box(200))
    renderer.clockFastForward(500)
    render(box(200, "none"))
    renderer.clockFastForward(16)
    renderer.dispatchNativeEvents()
    expect(seen).toEqual(["run", "cancel 0.5"])

    // Changed while hidden, then shown: no transition starts.
    render(box(300, "none"))
    renderer.clockFastForward(16)
    render(box(300))
    expect(widthOf(renderer, "box")).toBe(300)
    renderer.clockFastForward(500)
    expect(widthOf(renderer, "box")).toBe(300)
    expect(seen).toEqual(["run", "cancel 0.5"])
  })

  it("animates the focusVisible style on keyboard focus only", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    render(
      <div
        testId="box"
        tabIndex={0}
        style={{
          width: 100,
          height: 40,
          transition: "width 1s linear",
          focusVisible: { width: 200 },
        }}
      />
    )
    const bounds = renderer.getElementBounds(renderer.findByTestId("box").id)

    renderer.nativeSimulateClick(bounds.x + 4, bounds.y + 4)
    renderer.clockFastForward(1000)
    expect(widthOf(renderer, "box")).toBe(100)

    renderer.simulateKeystrokes("Tab")
    renderer.clockFastForward(500)
    const middle = widthOf(renderer, "box")
    expect(middle).toBeGreaterThan(100)
    expect(middle).toBeLessThan(200)
    renderer.clockFastForward(1000)
    expect(widthOf(renderer, "box")).toBe(200)
  })

  it("fires run, start and end in order with the DOM event fields", () => {
    const { render, renderer } = createTestRoot()
    const seen: Array<[string, string | undefined, number | undefined]> = []
    const record = (type: string) => (event: EventPayload) =>
      seen.push([type, event.propertyName, event.elapsedTime])
    const box = (width: number) => (
      <div
        testId="box"
        onTransitionRun={record("run")}
        onTransitionStart={record("start")}
        onTransitionEnd={record("end")}
        onTransitionCancel={record("cancel")}
        style={{ width, height: 20, transition: "width 1s linear 500ms" }}
      />
    )

    renderer.clockPause()
    render(box(100))
    render(box(200))
    renderer.dispatchNativeEvents()
    expect(seen).toEqual([["run", "width", 0]])

    renderer.clockFastForward(600)
    renderer.dispatchNativeEvents()
    expect(seen.map(([type]) => type)).toEqual(["run", "start"])

    renderer.clockFastForward(1500)
    renderer.dispatchNativeEvents()
    expect(seen).toEqual([
      ["run", "width", 0],
      ["start", "width", 0],
      ["end", "width", 1],
    ])
  })

  it("fires cancel when the property changes again", () => {
    const { render, renderer } = createTestRoot()
    const seen: string[] = []
    const box = (width: number) => (
      <div
        testId="box"
        onTransitionRun={() => seen.push("run")}
        onTransitionCancel={(event) => seen.push(`cancel ${event.elapsedTime}`)}
        style={{ width, height: 20, transition: "width 1s linear" }}
      />
    )
    renderer.clockPause()
    render(box(100))
    render(box(200))
    renderer.clockFastForward(500)
    render(box(300))
    renderer.clockFastForward(16)
    renderer.dispatchNativeEvents()
    expect(seen).toEqual(["run", "cancel 0.5", "run"])
  })

  it("animates a colour change through Oklab", () => {
    const { render, renderer } = createTestRoot()
    fs.mkdirSync(SHOTS_DIR, { recursive: true })
    const file = path.join(SHOTS_DIR, "css-transition-background-color.png")
    const box = (backgroundColor: string) => (
      <div
        style={{
          width: "100%",
          height: "100%",
          backgroundColor,
          transition: "background-color 1s linear",
        }}
      />
    )
    const sample = () => {
      renderer.dispatchNativeEvents()
      renderer.captureScreenshot(file)
      return readLogicalPixels(file, renderer).at(10, 10)
    }

    renderer.clockPause()
    render(box("#ff0000"))
    render(box("#0000ff"))
    renderer.clockFastForward(500)
    const [red, green, blue] = sample()
    // The Oklab midpoint of red and blue is rgb(140, 83, 162); the sRGB one
    // would be (128, 0, 128).
    expect(red).toBeGreaterThan(135)
    expect(red).toBeLessThan(146)
    expect(green).toBeGreaterThan(78)
    expect(green).toBeLessThan(88)
    expect(blue).toBeGreaterThan(157)
    expect(blue).toBeLessThan(168)

    renderer.clockFastForward(1000)
    expect(sample().slice(0, 3)).toEqual([0, 0, 255])
  })
  describe("lengths layout resolves", () => {
    const boundsOf = (
      renderer: ReturnType<typeof createTestRoot>["renderer"],
      testId: string
    ) => {
      renderer.dispatchNativeEvents()
      return renderer.getElementBounds(renderer.findByTestId(testId).id)
    }
    // A layout read is painted before the next frame can use it, so frames run
    // between mounting and changing the style, as they do in a real window.
    const mounted = (
      root: ReturnType<typeof createTestRoot>,
      node: ReactNode
    ) => {
      root.renderer.clockPause()
      root.render(node)
      root.renderer.flush()
    }
    const changed = (
      root: ReturnType<typeof createTestRoot>,
      node: ReactNode
    ) => {
      root.render(node)
      root.renderer.flush()
    }

    it("opens and closes an accordion when interpolateSize allows keywords", () => {
      const root = createTestRoot({ width: 400, height: 300 })
      const panel = (open: boolean, interpolateSize: "allow-keywords" | "numeric-only") => (
        <div style={{ display: "flex", flexDirection: "column" }}>
          <div
            testId="panel"
            style={{
              height: open ? "auto" : 0,
              overflow: "hidden",
              interpolateSize,
              transition: "height 1s linear",
            }}
          >
            <div style={{ width: 40, height: 80 }} />
          </div>
        </div>
      )

      mounted(root, panel(false, "allow-keywords"))
      expect(boundsOf(root.renderer, "panel").height).toBe(0)

      changed(root, panel(true, "allow-keywords"))
      root.renderer.clockFastForward(500)
      expect(boundsOf(root.renderer, "panel").height).toBeCloseTo(40, 0)
      root.renderer.clockFastForward(1000)
      expect(boundsOf(root.renderer, "panel").height).toBe(80)

      changed(root, panel(false, "allow-keywords"))
      root.renderer.clockFastForward(500)
      expect(boundsOf(root.renderer, "panel").height).toBeCloseTo(40, 0)
      root.renderer.clockFastForward(1000)
      expect(boundsOf(root.renderer, "panel").height).toBe(0)
    })

    it("changes to auto at once by default, as in a browser", () => {
      const root = createTestRoot({ width: 400, height: 300 })
      const panel = (open: boolean) => (
        <div style={{ display: "flex", flexDirection: "column" }}>
          <div
            testId="panel"
            style={{
              height: open ? "auto" : 0,
              overflow: "hidden",
              transition: "height 1s linear",
            }}
          >
            <div style={{ width: 40, height: 80 }} />
          </div>
        </div>
      )

      mounted(root, panel(false))
      changed(root, panel(true))
      root.renderer.clockFastForward(500)
      expect(boundsOf(root.renderer, "panel").height).toBe(80)
    })

    it("follows the content while it grows during the transition", () => {
      const root = createTestRoot({ width: 400, height: 300 })
      const panel = (content: number) => (
        <div style={{ display: "flex", flexDirection: "column" }}>
          <div
            testId="panel"
            style={{
              height: "auto",
              overflow: "hidden",
              interpolateSize: "allow-keywords",
              transition: "height 1s linear",
            }}
          >
            <div style={{ width: 40, height: content }} />
          </div>
        </div>
      )
      const closed = (
        <div style={{ display: "flex", flexDirection: "column" }}>
          <div
            testId="panel"
            style={{
              height: 0,
              overflow: "hidden",
              interpolateSize: "allow-keywords",
              transition: "height 1s linear",
            }}
          >
            <div style={{ width: 40, height: 80 }} />
          </div>
        </div>
      )

      mounted(root, closed)
      changed(root, panel(80))
      root.renderer.clockFastForward(500)
      expect(boundsOf(root.renderer, "panel").height).toBeCloseTo(40, 0)

      changed(root, panel(160))
      root.renderer.clockFastForward(250)
      const grown = boundsOf(root.renderer, "panel").height
      expect(grown).toBeGreaterThan(40)
      expect(grown).toBeLessThan(160)

      root.renderer.clockFastForward(1000)
      expect(boundsOf(root.renderer, "panel").height).toBe(160)
    })

    it("transitions between px and a percentage of the parent", () => {
      const root = createTestRoot({ width: 400, height: 300 })
      const bar = (width: number | `${number}%`) => (
        <div style={{ display: "flex", flexDirection: "column", width: 400 }}>
          <div
            testId="bar"
            style={{ width, height: 10, transition: "width 1s linear" }}
          />
        </div>
      )

      mounted(root, bar(0))
      changed(root, bar("50%"))
      root.renderer.clockFastForward(500)
      expect(boundsOf(root.renderer, "bar").width).toBeCloseTo(100, 0)
      root.renderer.clockFastForward(1000)
      expect(boundsOf(root.renderer, "bar").width).toBe(200)

      changed(root, bar(100))
      root.renderer.clockFastForward(500)
      expect(boundsOf(root.renderer, "bar").width).toBeCloseTo(150, 0)
      root.renderer.clockFastForward(1000)
      expect(boundsOf(root.renderer, "bar").width).toBe(100)
    })

    it("resolves a percentage on a root against the window", () => {
      const root = createTestRoot({ width: 400, height: 300 })
      const bar = (width: number | `${number}%`) => (
        <div
          testId="bar"
          style={{ width, height: 10, transition: "width 1s linear" }}
        />
      )

      mounted(root, bar(0))
      changed(root, bar("50%"))
      root.renderer.clockFastForward(500)
      expect(boundsOf(root.renderer, "bar").width).toBeCloseTo(100, 0)
      root.renderer.clockFastForward(1000)
      expect(boundsOf(root.renderer, "bar").width).toBe(200)
    })

    it("starts from the laid-out height when the style leaves it unset", () => {
      const root = createTestRoot({ width: 400, height: 300 })
      const box = (height: number | undefined) => (
        <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start" }}>
          <div
            testId="box"
            style={{
              height,
              interpolateSize: "allow-keywords",
              transition: "height 1s linear",
            }}
          >
            <div style={{ width: 40, height: 40 }} />
          </div>
        </div>
      )

      mounted(root, box(undefined))
      expect(boundsOf(root.renderer, "box").height).toBe(40)

      changed(root, box(120))
      root.renderer.clockFastForward(500)
      expect(boundsOf(root.renderer, "box").height).toBeCloseTo(80, 0)
      root.renderer.clockFastForward(1000)
      expect(boundsOf(root.renderer, "box").height).toBe(120)
    })
  })

  it("bubbles a child's transition events to its parent with the child as target", () => {
    const { render, renderer } = createTestRoot()
    const seen: string[] = []
    let child: unknown
    const parent = (width: number, stop = false) => (
      <div
        testId="parent"
        onTransitionEndCapture={(event) => seen.push(`capture ${event.target === child ? "child" : "?"}`)}
        onTransitionEnd={(event) => {
          seen.push(`parent ${event.propertyName} ${event.eventPhase} ${event.cancelable}`)
        }}
      >
        <div
          ref={(element) => (child = element)}
          testId="child"
          onTransitionEnd={(event) => {
            seen.push(`child ${event.target === event.currentTarget ? "target" : "?"}`)
            if (stop) event.stopPropagation()
          }}
          style={{ width, height: 20, transition: "width 1s linear" }}
        />
      </div>
    )

    renderer.clockPause()
    render(parent(100))
    render(parent(200))
    renderer.clockFastForward(1500)
    renderer.dispatchNativeEvents()
    expect(seen).toEqual(["capture child", "child target", "parent width 3 false"])

    seen.length = 0
    render(parent(100, true))
    render(parent(300, true))
    renderer.clockFastForward(1500)
    renderer.dispatchNativeEvents()
    expect(seen).toEqual(["capture child", "child target"])
  })

  it("reaches a listener on an ancestor only when the child has no listener of its own", () => {
    const { render, renderer } = createTestRoot()
    const targets: object[] = []
    let child: unknown
    const tree = (width: number) => (
      <div onTransitionRun={(event) => targets.push(event.target)}>
        <div ref={(element) => (child = element)} testId="child" style={{ width, height: 20, transition: "width 1s linear" }} />
      </div>
    )
    renderer.clockPause()
    render(tree(100))
    render(tree(200))
    renderer.dispatchNativeEvents()
    expect(targets).toHaveLength(1)
    expect(targets[0]).toBe(child)
  })

  it("treats a colour scheme flip as a change of light-dark(), and a new color as none of currentColor", () => {
    const record = (seen: string[]) => (type: string) => () => seen.push(type)
    // The parent sets the scheme and the colour the child's colours resolve in.
    const setup = () => {
      const { render, renderer } = createTestRoot()
      const seen: string[] = []
      const on = record(seen)
      const tree = (
        parentStyle: Record<string, unknown>,
        childStyle: Record<string, unknown>
      ) => (
        <div style={{ width: 40, height: 40, ...parentStyle }}>
          <div
            testId="child"
            onTransitionRun={on("run")}
            onTransitionStart={on("start")}
            onTransitionEnd={on("end")}
            onTransitionCancel={on("cancel")}
            style={{ width: 40, height: 40, transition: "all 1s linear", ...childStyle }}
          />
        </div>
      )
      const settle = (millis: number) => {
        renderer.clockFastForward(millis)
        renderer.dispatchNativeEvents()
        return seen.splice(0)
      }
      renderer.clockPause()
      return { render, tree, settle }
    }

    // An idle element starts one when the scheme flips.
    {
      const { render, tree, settle } = setup()
      const flipping = { backgroundColor: "light-dark(#ff0000, #0000ff)" }
      render(tree({ colorScheme: "light" }, flipping))
      settle(16)
      render(tree({ colorScheme: "dark" }, flipping))
      expect(settle(16)).toEqual(["run", "start"])
      expect(settle(1500)).toEqual(["end"])
    }

    // A running one whose end the flip changes starts again from where it is.
    {
      const { render, tree, settle } = setup()
      render(tree({ colorScheme: "light" }, { backgroundColor: "#ff0000" }))
      settle(16)
      const toGreenOrBlue = { backgroundColor: "light-dark(#00ff00, #0000ff)" }
      render(tree({ colorScheme: "light" }, toGreenOrBlue))
      expect(settle(500)).toEqual(["run", "start"])
      render(tree({ colorScheme: "dark" }, toGreenOrBlue))
      expect(settle(16)).toEqual(["cancel", "run", "start"])
      expect(settle(1500)).toEqual(["end"])
    }

    // One whose end the flip leaves as it is runs on untouched.
    {
      const { render, tree, settle } = setup()
      render(tree({ colorScheme: "light" }, { backgroundColor: "light-dark(#ff0000, #0000ff)" }))
      settle(16)
      const toGreen = { backgroundColor: "#00ff00" }
      render(tree({ colorScheme: "light" }, toGreen))
      expect(settle(500)).toEqual(["run", "start"])
      render(tree({ colorScheme: "dark" }, toGreen))
      expect(settle(16)).toEqual([])
      expect(settle(1500)).toEqual(["end"])
    }

    // currentColor follows a new color without a transition of its own.
    {
      const { render, tree, settle } = setup()
      const following = { borderColor: "currentColor", borderWidth: 2 }
      render(tree({ color: "#ff0000" }, following))
      settle(16)
      render(tree({ color: "#0000ff" }, following))
      expect(settle(1500)).toEqual([])
    }
  })
})
