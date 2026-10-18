/// Container queries and `inline-block`: an atomic inline is laid out as a root in an engine of its
/// own, by the text it sits in, and that must not change what a query or a cq unit sees.
///
/// `fixtures/inline-container-oracle.html` is the page and `fixtures/inline-container-oracle.json`
/// the boxes headless Chrome reports for it (`getBoundingClientRect` relative to the box of the case,
/// `[x, y, width, height]`; regenerate with the command in the HTML). The text is GpuixTestSquare at
/// 20px with a line height of 20px, so every character is 20px wide, and only "A" is in the font.

import fs from "fs"
import path from "path"
import type { ReactElement } from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import type { EventPayload } from "@gpuix/native"
import { ResizeObserver } from "../index"
import type { PublicInstance, ResizeObserverEntry } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const SQUARE_FONT = fs.readFileSync(new URL("./fixtures/gpuix-test-square.ttf", import.meta.url))

const TEXT = { fontFamily: "GpuixTestSquare", fontSize: 20, lineHeight: "20px", color: "#000000" } as const

type Rect = [number, number, number, number]

const ORACLE: Record<string, Rect> = JSON.parse(
  fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/inline-container-oracle.json"),
    "utf8"
  )
)

/** The oracle's box `id`. */
function chrome(id: string): Rect {
  const box = ORACLE[id]
  expect(box, `no oracle box ${id}`).toBeDefined()
  return box ?? [0, 0, 0, 0]
}

/** The names the elements of a render are found by; boxes are relative to the one called `root`. */
function setup() {
  const root = createTestRoot()
  root.renderer.registerFont(SQUARE_FONT)
  const refs = new Map<string, PublicInstance>()
  const ref = (name: string) => (instance: PublicInstance | null) => {
    if (instance) refs.set(name, instance)
  }
  const rect = (name: string): Rect => {
    root.renderer.dispatchNativeEvents()
    const element = refs.get(name)
    expect(element, `no element ${name}`).toBeDefined()
    const box = element?.getBoundingClientRect() ?? { x: 0, y: 0, width: 0, height: 0 }
    const origin = refs.get("root")?.getBoundingClientRect() ?? { x: 0, y: 0 }
    return [box.x - origin.x, box.y - origin.y, box.width, box.height]
  }
  /** Each `name` laid out as the oracle's `${scope}.${name}`. */
  const expectChrome = (scope: string, ...names: string[]) => {
    for (const name of names) {
      const expected = chrome(`${scope}.${name}`)
      const actual = rect(name)
      for (const [ix, key] of ["x", "y", "w", "h"].entries()) {
        expect(
          Math.abs((actual[ix] ?? 0) - (expected[ix] ?? 0)),
          `${scope}.${name}: ${key} ${actual[ix]} != ${expected[ix]}`
        ).toBeLessThanOrEqual(0.51)
      }
    }
  }
  /** Frames the window painted while `change` ran. */
  const paintedFrames = (change: () => void): number => {
    const before = root.renderer.getPaintedFrameCount()
    change()
    return root.renderer.getPaintedFrameCount() - before
  }
  const element = (name: string): PublicInstance => {
    const found = refs.get(name)
    if (!found) throw new Error(`no element ${name}`)
    return found
  }
  return { ...root, ref, rect, element, expectChrome, paintedFrames }
}

type Names = ReturnType<typeof setup>["ref"]

const inlineBlock = { display: "inline-block" } as const
const block = { display: "block" } as const

// ── An inline-block that is a container ──

const aApp = (ref: Names, width: number): ReactElement => (
  <div ref={ref("root")} style={{ width: 300, ...TEXT }}>
    <span ref={ref("t1")}>AA</span>
    <span ref={ref("box")} style={{ ...inlineBlock, containerType: "inline-size", height: 30, width }}>
      <span
        ref={ref("child")}
        style={{
          ...block,
          height: 10,
          width: 50,
          "@container": { "(min-width: 80px)": { width: 70, height: 12 } },
        }}
      />
      <span ref={ref("cq")} style={{ ...block, width: "50cqw", height: 6 }} />
    </span>
    <span ref={ref("t2")}>AA</span>
  </div>
)

describeNative("inline-block as a query container", () => {
  it("answers a query and a cq unit in the first frame, wide and narrow", () => {
    for (const [scope, width] of [
      ["a-100", 100],
      ["a-60", 60],
    ] as const) {
      const { render, ref, expectChrome, paintedFrames, unmount } = setup()
      try {
        expect(paintedFrames(() => render(aApp(ref, width)))).toBe(1)
        expectChrome(scope, "box", "child", "cq", "t1", "t2")
      } finally {
        unmount()
      }
    }
  })

  it("flips in the frame that lays the box out when its width changes, both ways", () => {
    const { render, ref, expectChrome, paintedFrames, unmount } = setup()
    try {
      render(aApp(ref, 100))
      expect(paintedFrames(() => render(aApp(ref, 60)))).toBe(1)
      expectChrome("a-60", "box", "child", "cq", "t2")
      expect(paintedFrames(() => render(aApp(ref, 100)))).toBe(1)
      expectChrome("a-100", "box", "child", "cq", "t2")
    } finally {
      unmount()
    }
  })

  it("is as wide as its padding and border when it has no width, whatever it holds", () => {
    const { render, ref, expectChrome, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ width: 300, ...TEXT }}>
          <span ref={ref("t1")}>AA</span>
          <span ref={ref("box")} style={{ ...inlineBlock, containerType: "inline-size", paddingLeft: 5, paddingRight: 5, height: 20 }}>
            <span ref={ref("child")} style={{ ...block, width: 80, height: 10 }} />
          </span>
          <span ref={ref("t2")}>AA</span>
        </div>
      )
      expectChrome("b", "t1", "box", "child", "t2")
    } finally {
      unmount()
    }
  })

  it("is a size container with a size of its own", () => {
    const { render, ref, expectChrome, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ width: 300, ...TEXT }}>
          <span ref={ref("t1")}>A</span>
          <span ref={ref("box")} style={{ ...inlineBlock, containerType: "size", width: 100, height: 40 }}>
            <span
              ref={ref("child")}
              style={{
                ...block,
                width: "10cqw",
                height: "50cqh",
                "@container": { "(min-height: 30px)": { marginLeft: 4 }, "(min-height: 50px)": { marginLeft: 8 } },
              }}
            />
          </span>
          <span ref={ref("t2")}>A</span>
        </div>
      )
      expectChrome("e", "t1", "box", "child", "t2")
    } finally {
      unmount()
    }
  })
})

// ── An inline-block inside a container ──

const cApp = (ref: Names, width: number): ReactElement => (
  <div ref={ref("root")} style={{ width, ...TEXT }}>
    <div ref={ref("outer")} style={{ containerType: "inline-size" }}>
      AA
      <span ref={ref("box")} style={{ ...inlineBlock, height: 20 }}>
        <span
          ref={ref("child")}
          style={{
            ...block,
            width: "50cqw",
            height: 10,
            "@container": { "(min-width: 250px)": { height: 14 } },
          }}
        />
      </span>
      AA
    </div>
  </div>
)

describeNative("inline-block inside a query container", () => {
  it("reads the container above the text it sits in, in the first frame and after a resize", () => {
    const { render, ref, expectChrome, paintedFrames, unmount } = setup()
    try {
      expect(paintedFrames(() => render(cApp(ref, 300)))).toBe(1)
      expectChrome("c-300", "outer", "box", "child")
      expect(paintedFrames(() => render(cApp(ref, 200)))).toBe(1)
      expectChrome("c-200", "outer", "box", "child")
      expect(paintedFrames(() => render(cApp(ref, 300)))).toBe(1)
      expectChrome("c-300", "outer", "box", "child")
    } finally {
      unmount()
    }
  })

  it("sizes the font of the text in the box by the container, in the frame that lays the container out", () => {
    const app = (ref: Names, width: number): ReactElement => (
      <div ref={ref("root")} style={{ width }}>
        <div style={{ containerType: "inline-size", fontFamily: "GpuixTestSquare", color: "#000000" }}>
          A
          <span ref={ref("box")} style={{ ...inlineBlock, fontSize: "10cqw" }}>
            A
          </span>
        </div>
      </div>
    )
    const { render, ref, rect, paintedFrames, unmount } = setup()
    try {
      expect(paintedFrames(() => render(app(ref, 400)))).toBe(1)
      const [, , wide, tall] = rect("box")
      // One A is a square of the font size, 10% of the 400px container (a text is as wide as its advance up to 1/64 of a pixel).
      expect(wide).toBeGreaterThanOrEqual(40)
      expect(wide).toBeLessThanOrEqual(41)
      // GpuixTestSquare rises one em above the baseline and falls none below, so `line-height: normal`
      // is the font size.
      expect(tall).toBe(40)
      expect(paintedFrames(() => render(app(ref, 200)))).toBe(1)
      const [, , narrow, short] = rect("box")
      expect(Math.abs(narrow - wide / 2)).toBeLessThanOrEqual(1)
      expect(Math.abs(short - tall / 2)).toBeLessThanOrEqual(1)
    } finally {
      unmount()
    }
  })

  it("follows the window, as a box in the flow does", () => {
    const { render, ref, rect, renderer, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ ...TEXT }}>
          <div style={{ containerType: "inline-size" }}>
            AA
            <span style={{ ...inlineBlock, height: 20 }}>
              <span
                ref={ref("child")}
                style={{ ...block, width: 10, height: 10, "@container": { "(min-width: 1000px)": { width: 99 } } }}
              />
            </span>
          </div>
        </div>
      )
      expect(rect("child")[2]).toBe(99)
      renderer.setWindowSize(900, 800)
      expect(rect("child")[2]).toBe(10)
      renderer.setWindowSize(1280, 800)
      expect(rect("child")[2]).toBe(99)
    } finally {
      unmount()
    }
  })
})

const fApp = (ref: Names, width: number): ReactElement => (
  <div ref={ref("root")} style={{ width, ...TEXT }}>
    <div ref={ref("outer")} style={{ containerType: "inline-size" }}>
      <span ref={ref("t1")}>AA</span>
      <span
        ref={ref("box")}
        style={{
          ...inlineBlock,
          width: 40,
          height: 20,
          marginLeft: 6,
          "@container": { "(min-width: 250px)": { width: 100, marginLeft: 10 } },
        }}
      />
      <span ref={ref("t2")}>AA</span>
    </div>
  </div>
)

describeNative("inline-block with a query of its own", () => {
  it("takes its width and margin from the block, and the text after it follows in the same frame", () => {
    const { render, ref, expectChrome, paintedFrames, unmount } = setup()
    try {
      expect(paintedFrames(() => render(fApp(ref, 300)))).toBe(1)
      expectChrome("f-300", "outer", "t1", "box", "t2")
      expect(paintedFrames(() => render(fApp(ref, 200)))).toBe(1)
      expectChrome("f-200", "outer", "t1", "box", "t2")
      expect(paintedFrames(() => render(fApp(ref, 300)))).toBe(1)
      expectChrome("f-300", "outer", "t1", "box", "t2")
    } finally {
      unmount()
    }
  })
})

// ── A container in a container, with a text between ──

const dApp = (ref: Names, width: number): ReactElement => (
  <div ref={ref("root")} style={{ width, ...TEXT }}>
    <div ref={ref("outer")} style={{ containerType: "inline-size" }}>
      A
      <span ref={ref("box")} style={{ ...inlineBlock, containerType: "inline-size", width: "50cqw", height: 40 }}>
        <span
          ref={ref("child")}
          style={{ ...block, width: 30, height: 10, "@container": { "(min-width: 100px)": { width: 90 } } }}
        />
      </span>
      A
    </div>
  </div>
)

describeNative("inline-block between two query containers", () => {
  it("settles in the frame that lays the containers out, each changing the one below", () => {
    const { render, ref, expectChrome, paintedFrames, unmount } = setup()
    try {
      expect(paintedFrames(() => render(dApp(ref, 300)))).toBe(1)
      expectChrome("d-300", "outer", "box", "child")
      expect(paintedFrames(() => render(dApp(ref, 160)))).toBe(1)
      expectChrome("d-160", "outer", "box", "child")
      expect(paintedFrames(() => render(dApp(ref, 300)))).toBe(1)
      expectChrome("d-300", "outer", "box", "child")
    } finally {
      unmount()
    }
  })
})

// ── What a pass before present must not leave behind ──

describeNative("inline-block: what an abandoned pass leaves", () => {
  const eApp = (
    ref: Names,
    width: number,
    onLayout: (event: EventPayload) => void = () => {}
  ): ReactElement => (
    <div ref={ref("root")} style={{ width, ...TEXT }}>
      <div style={{ containerType: "inline-size" }}>
        AA
        <span style={{ ...inlineBlock, containerType: "inline-size", width: "50cqw" }}>
          <span
            ref={ref("child")}
            onLayout={onLayout}
            style={{ ...block, width: 40, height: 20, "@container": { "(min-width: 100px)": { width: 100 } } }}
          >
            <span>AAA</span>
          </span>
        </span>
        AA
      </div>
    </div>
  )

  it("paints each text once, and the bounds are those of the presented pass", () => {
    const { render, ref, rect, renderer, unmount } = setup()
    try {
      render(eApp(ref, 400))
      const painted = renderer.getPaintedText()
      expect(painted.filter((text) => text === "AAA")).toHaveLength(1)
      expect(painted).toHaveLength(2)
      expect(rect("child")[2]).toBe(100)
      render(eApp(ref, 160))
      expect(renderer.getPaintedText().filter((text) => text === "AAA")).toHaveLength(1)
      expect(renderer.getPaintedText()).toHaveLength(2)
      expect(rect("child")[2]).toBe(40)
    } finally {
      unmount()
    }
  })

  it("onLayout of a box in the inline-block reports the final size once at mount and once for a flip", () => {
    const { render, ref, renderer, unmount } = setup()
    try {
      const sizes: number[][] = []
      const onLayout = (event: EventPayload) => sizes.push([event.width ?? 0, event.height ?? 0])
      render(eApp(ref, 400, onLayout))
      renderer.dispatchNativeEvents()
      expect(sizes).toEqual([[100, 20]])
      render(eApp(ref, 160, onLayout))
      renderer.dispatchNativeEvents()
      expect(sizes).toEqual([
        [100, 20],
        [40, 20],
      ])
    } finally {
      unmount()
    }
  })

  it("a ResizeObserver on a box in the inline-block sees the final size and nothing in between", () => {
    const { render, ref, element, renderer, unmount } = setup()
    try {
      const entries: ResizeObserverEntry[] = []
      render(eApp(ref, 400))
      new ResizeObserver((batch) => entries.push(...batch)).observe(element("child"))
      renderer.dispatchNativeEvents()
      expect(entries.map((entry) => entry.contentRect.width)).toEqual([100])
      render(eApp(ref, 160))
      renderer.dispatchNativeEvents()
      expect(entries.map((entry) => entry.contentRect.width)).toEqual([100, 40])
      render(eApp(ref, 400))
      renderer.dispatchNativeEvents()
      expect(entries.map((entry) => entry.contentRect.width)).toEqual([100, 40, 100])
    } finally {
      unmount()
    }
  })

  it("a drag copies each text once, the block of the box on a line of its own", () => {
    const { render, ref, rect, renderer, unmount } = setup()
    try {
      render(eApp(ref, 400))
      render(eApp(ref, 160))
      const line = rect("root")
      expect(renderer.dragSelect(line[0] + 1, line[1] + 10, line[0] + 390, line[1] + 10)).toBe("AA\nAAA\nAA")
    } finally {
      unmount()
    }
  })
})
