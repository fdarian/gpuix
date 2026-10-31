/// Container queries: `containerType`, `containerName`, `container`, `"@container"` blocks and
/// the `cq*` units, against what headless Chrome lays out for the same page.
///
/// `fixtures/container-query-oracle.html` is that page and `fixtures/container-query-oracle.json`
/// the boxes Chrome reports for it (`getBoundingClientRect`, `[x, y, width, height]`). Every
/// expectation below that names an oracle box compares the position and size relative to the
/// container the box is in. Regenerate the JSON with
///
///     CHROME=/Users/farreldarian/Library/Caches/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell
///     $CHROME --headless --window-size=1280,800 --dump-dom file://$PWD/fixtures/container-query-oracle.html \
///       | grep -o 'data-rects="[^"]*"' | sed 's/&quot;/"/g'
///
/// (the window is the test renderer's 1280x800). The first frame of a mount, and the frame that
/// follows a resize, must already show the final style: each check counts the frames the window
/// painted for it, and the abandoned passes before present paint nothing.

import fs from "fs"
import path from "path"
import type { ReactElement } from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import type { EventPayload } from "@ghia/native"
import { ResizeObserver } from "../index"
import type { PublicInstance, ResizeObserverEntry } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rect = [number, number, number, number]
type TestRoot = ReturnType<typeof createTestRoot>

const ORACLE: Record<string, Rect> = JSON.parse(
  fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/container-query-oracle.json"),
    "utf8"
  )
)

/** The oracle's box `id`, from the top left of `from`. */
function chrome(id: string, from?: string): Rect {
  const [x, y, width, height] = ORACLE[id]!
  const [originX, originY] = from ? ORACLE[from]! : [0, 0]
  return [x - originX, y - originY, width, height]
}

function bounds(testRoot: TestRoot, testId: string) {
  testRoot.renderer.dispatchNativeEvents()
  const element = testRoot.renderer.findByTestId(testId)
  expect(element, `no element with testId ${testId}`).toBeDefined()
  const found = testRoot.renderer.getElementBounds(element!.id)
  expect(found, `no painted bounds for ${testId}`).toBeDefined()
  return found!
}

/** What gpuix laid out for `testId`, from the top left of `from`. */
function laidOut(testRoot: TestRoot, testId: string, from?: string): Rect {
  const box = bounds(testRoot, testId)
  const origin = from ? bounds(testRoot, from) : { x: 0, y: 0 }
  return [box.x - origin.x, box.y - origin.y, box.width, box.height]
}

/** Each `[testId, oracle id]` pair, measured from `from` and from the oracle's box of the same name. */
function expectChrome(testRoot: TestRoot, from: [string, string], ...pairs: [string, string][]) {
  for (const [testId, oracleId] of pairs) {
    expect(laidOut(testRoot, testId, from[0]), `${testId} against ${oracleId}`).toEqual(chrome(oracleId, from[1]))
  }
}

/** Frames the window painted while `change` ran. */
function paintedFrames(testRoot: TestRoot, change: () => void): number {
  const before = testRoot.renderer.getPaintedFrameCount()
  change()
  return testRoot.renderer.getPaintedFrameCount() - before
}

function withRoot(run: (testRoot: TestRoot) => void) {
  const testRoot = createTestRoot()
  try {
    run(testRoot)
  } finally {
    testRoot.unmount()
  }
}

// ── A: the later block wins; a resize flips the query in the frame that lays it out ──

const aApp = (width: number) => (
  <div style={{ width }}>
    <div testId="box" style={{ containerType: "inline-size", width: "100%" }}>
      <div
        testId="child"
        style={{
          height: 20,
          width: 100,
          "@container": {
            "(min-width: 350px)": { height: 40 },
            "(min-width: 400px)": { width: 300, height: 30 },
          },
        }}
      />
    </div>
  </div>
)

describeNative("container queries: the first frame", () => {
  for (const [name, width] of [
    ["a-wide", 500],
    ["a-mid", 380],
    ["a-narrow", 300],
  ] as const) {
    it(`a mount at ${width}px is correct in the one frame it paints (${name})`, () => {
      withRoot((testRoot) => {
        const frames = paintedFrames(testRoot, () => testRoot.render(aApp(width)))
        expect(frames).toBe(1)
        expectChrome(testRoot, ["box", `${name}.box`], ["child", `${name}.child`])
      })
    })
  }

  it("the blocks apply in the order they are written, not the order of their widths", () => {
    withRoot((testRoot) => {
      testRoot.render(aApp(500))
      // Both blocks are true at 500px; the later one sets 30, the earlier 40.
      expect(laidOut(testRoot, "child", "box")).toEqual([0, 0, 300, 30])
    })
  })
})

describeNative("container queries: a change of the container's size", () => {
  it("flips the query in the frame that lays the container out, both ways", () => {
    withRoot((testRoot) => {
      testRoot.render(aApp(500))
      for (const [name, width] of [
        ["a-narrow", 300],
        ["a-mid", 380],
        ["a-wide", 500],
        ["a-narrow", 300],
      ] as const) {
        const frames = paintedFrames(testRoot, () => testRoot.render(aApp(width)))
        expect(frames, `the frame for ${width}px`).toBe(1)
        expectChrome(testRoot, ["box", `${name}.box`], ["child", `${name}.child`])
      }
    })
  })

  it("an unchanged container does not build the frame again", () => {
    withRoot((testRoot) => {
      testRoot.render(aApp(500))
      const frames = paintedFrames(testRoot, () => testRoot.render(aApp(500)))
      expect(frames).toBe(1)
      expect(laidOut(testRoot, "child", "box")).toEqual([0, 0, 300, 30])
    })
  })

  it("follows the window: a resize flips a query on the width the window gives", () => {
    withRoot((testRoot) => {
      testRoot.render(
        <div testId="box" style={{ containerType: "inline-size" }}>
          <div
            testId="child"
            style={{ width: 10, height: 10, "@container": { "(min-width: 1000px)": { width: 99 } } }}
          />
        </div>
      )
      expect(laidOut(testRoot, "child")[2]).toBe(99)
      testRoot.renderer.setWindowSize(900, 800)
      expect(laidOut(testRoot, "child")[2]).toBe(10)
      testRoot.renderer.setWindowSize(1280, 800)
      expect(laidOut(testRoot, "child")[2]).toBe(99)
    })
  })
})

// ── B: names and the nearest ancestor ──

describeNative("container queries: names", () => {
  it("an unnamed query asks the nearest container, a named one the nearest with that name", () => {
    withRoot((testRoot) => {
      testRoot.render(
        <div style={{ container: "outer / inline-size", width: 600 }}>
          <div testId="box" style={{ container: "inner / inline-size", width: 200 }}>
            <div
              testId="child"
              style={{
                width: 10,
                height: 10,
                "@container": {
                  "(min-width: 400px)": { width: 111 },
                  "outer (min-width: 400px)": { height: 22 },
                  "inner (min-width: 300px)": { marginLeft: 5 },
                  "inner (max-width: 200px)": { marginTop: 7 },
                },
              }}
            />
          </div>
        </div>
      )
      expectChrome(testRoot, ["box", "b.box"], ["child", "b.child"])
    })
  })

  it("a container has several names, and the longhands win over the shorthand", () => {
    withRoot((testRoot) => {
      testRoot.render(
        <div style={{ container: "wrong / size", containerName: "card panel", containerType: "inline-size", width: 500 }}>
          <div
            testId="child"
            style={{
              width: 10,
              height: 10,
              "@container": {
                "card (min-width: 400px)": { width: 20 },
                "panel (min-width: 400px)": { height: 30 },
                "wrong (min-width: 400px)": { marginLeft: 8 },
                // `size` came from the shorthand, which `containerType` overrode.
                "(min-height: 1px)": { marginTop: 8 },
              },
            }}
          />
        </div>
      )
      expect(laidOut(testRoot, "child")).toEqual([0, 0, 20, 30])
    })
  })

  it("a query that names no container above is false, and so is one with no container at all", () => {
    withRoot((testRoot) => {
      testRoot.render(
        <div style={{ containerType: "inline-size", containerName: "a", width: 500 }}>
          <div testId="named" style={{ width: 10, height: 10, "@container": { "b (min-width: 1px)": { width: 20 } } }} />
          <div testId="bare" style={{ width: 10, height: 10, "@container": { "(min-width: 1px)": { width: 20 } } }} />
        </div>
      )
      testRoot.render(
        <div>
          <div testId="orphan" style={{ width: 10, height: 10, "@container": { "(min-width: 1px)": { width: 20 } } }} />
        </div>
      )
      expect(laidOut(testRoot, "orphan")[2]).toBe(10)
    })
  })

  it("a name alone is true when a container of that name is above", () => {
    withRoot((testRoot) => {
      testRoot.render(
        <div style={{ containerType: "inline-size", containerName: "card", width: 500 }}>
          <div testId="has" style={{ width: 10, height: 10, "@container": { card: { width: 20 } } }} />
          <div testId="lacks" style={{ width: 10, height: 10, "@container": { sidebar: { width: 20 } } }} />
        </div>
      )
      expect(laidOut(testRoot, "has")[2]).toBe(20)
      expect(laidOut(testRoot, "lacks")[2]).toBe(10)
    })
  })
})

// ── C: size and inline-size ──

describeNative("container queries: size against inline-size", () => {
  const child = (testId: string, style: Record<string, unknown> = {}) => (
    <div
      testId={testId}
      style={{
        width: 100,
        height: 10,
        "@container": {
          "(min-height: 200px)": { width: 150 },
          "(orientation: landscape)": { height: 20 },
          "(min-width: 300px)": { marginTop: 3 },
        },
        ...style,
      }}
    />
  )

  it("an inline-size container answers the width alone, a size container both", () => {
    withRoot((testRoot) => {
      testRoot.render(
        <div>
          <div testId="inline" style={{ containerType: "inline-size", width: 400, height: 300 }}>
            {child("inline-child")}
          </div>
          <div testId="size" style={{ containerType: "size", width: 400, height: 300 }}>
            {child("size-child")}
          </div>
        </div>
      )
      expectChrome(testRoot, ["inline", "c-inline.box"], ["inline-child", "c-inline.child"])
      expectChrome(testRoot, ["size", "c-size.box"], ["size-child", "c-size.child"])
    })
  })

  it("a block-axis feature skips the nearer inline-size container for the size one above", () => {
    withRoot((testRoot) => {
      testRoot.render(
        <div testId="outer" style={{ containerType: "size", width: 400, height: 300 }}>
          <div testId="box" style={{ containerType: "inline-size", width: 200 }}>
            <div
              testId="child"
              style={{
                width: 100,
                height: 10,
                "@container": {
                  "(min-height: 250px)": { width: 160 },
                  "(min-width: 300px)": { height: 20 },
                },
              }}
            />
          </div>
        </div>
      )
      expectChrome(testRoot, ["box", "c-nested.box"], ["child", "c-nested.child"])
    })
  })

  it("aspect ratio and orientation read the content box of a size container", () => {
    const app = (testId: string, width: number, height: number) => (
      <div testId={`${testId}-box`} style={{ containerType: "size", width, height }}>
        <div
          testId={testId}
          style={{
            width: 10,
            height: 10,
            "@container": {
              "(aspect-ratio: 2/1)": { width: 20 },
              "(orientation: landscape)": { height: 20 },
              "(min-aspect-ratio: 16/9)": { marginLeft: 3 },
              "(max-aspect-ratio: 1/1)": { marginTop: 6 },
            },
          }}
        />
      </div>
    )
    withRoot((testRoot) => {
      testRoot.render(
        <div>
          {app("n-landscape", 400, 200)}
          {app("n-portrait", 200, 400)}
          {app("n-wide", 320, 180)}
        </div>
      )
      for (const name of ["n-landscape", "n-portrait", "n-wide"]) {
        expectChrome(testRoot, [`${name}-box`, `${name}.box`], [name, `${name}.child`])
      }
    })
  })
})

// ── D: container units ──

describeNative("container queries: cq units", () => {
  it("are fractions of the content box of the nearest container, in every length", () => {
    withRoot((testRoot) => {
      testRoot.render(
        <div testId="box" style={{ containerType: "size", width: 400, height: 300 }}>
          <div testId="cqw" style={{ width: "50cqw", height: "10cqh" }} />
          <div testId="min" style={{ width: "10cqmin", height: 10 }} />
          <div testId="max" style={{ width: "10cqmax", height: 10 }} />
          <div testId="calc" style={{ width: "calc(50cqw - 10px)", height: 10 }} />
          <div testId="logical" style={{ width: "25cqi", height: "5cqb" }} />
          <div testId="container" style={{ containerType: "inline-size", width: "50cqw", height: 40 }} />
        </div>
      )
      expectChrome(
        testRoot,
        ["box", "d.box"],
        ["cqw", "d.cqw"],
        ["min", "d.min"],
        ["max", "d.max"],
        ["calc", "d.calc"],
        ["logical", "d.logical"],
        ["container", "d.container"]
      )
    })
  })

  it("fall back to the small viewport with no container, and for an axis the container does not contain", () => {
    withRoot((testRoot) => {
      testRoot.render(
        <div>
          <div testId="inline" style={{ containerType: "inline-size", width: 400, height: 300 }}>
            <div testId="on-inline" style={{ width: "10cqw", height: "10cqh" }} />
          </div>
          <div testId="fallback" style={{ width: "10cqw", height: "10cqh" }} />
        </div>
      )
      expectChrome(testRoot, ["inline", "d-inline.box"], ["on-inline", "d.on-inline"])
      expect(laidOut(testRoot, "fallback").slice(2)).toEqual(chrome("d.fallback").slice(2))
    })
  })

  it("take each axis from the nearest container that contains it", () => {
    withRoot((testRoot) => {
      testRoot.render(
        <div testId="outer" style={{ containerType: "size", width: 400, height: 300 }}>
          <div testId="box" style={{ containerType: "inline-size", width: 200 }}>
            <div testId="cqw" style={{ width: "10cqw", height: "10cqh" }} />
            <div testId="min" style={{ width: "10cqmin", height: 10 }} />
            <div testId="max" style={{ width: "10cqmax", height: 10 }} />
          </div>
        </div>
      )
      expectChrome(
        testRoot,
        ["box", "d-nested.box"],
        ["cqw", "d-nested.cqw"],
        ["min", "d-nested.min"],
        ["max", "d-nested.max"]
      )
    })
  })

  it("resolve in the first frame, follow a resize, and resolve in a font size and a padding too", () => {
    const app = (width: number) => (
      <div style={{ width }}>
        <div testId="box" style={{ containerType: "inline-size" }}>
          <div testId="child" style={{ width: "50cqw", height: 10, paddingLeft: "10cqw" }} />
        </div>
      </div>
    )
    withRoot((testRoot) => {
      expect(paintedFrames(testRoot, () => testRoot.render(app(400)))).toBe(1)
      expect(laidOut(testRoot, "child")).toEqual([0, 0, 200, 10])
      expect(paintedFrames(testRoot, () => testRoot.render(app(200)))).toBe(1)
      expect(laidOut(testRoot, "child")).toEqual([0, 0, 100, 10])
    })
  })
})

// ── E: nested containers ──

const eApp = (width: number) => (
  <div testId="outer" style={{ containerType: "inline-size", width }}>
    <div
      testId="inner"
      style={{ containerType: "inline-size", width: "100%", height: 30, "@container": { "(min-width: 500px)": { width: "40%" } } }}
    >
      <div testId="grand" style={{ width: 10, height: 10, "@container": { "(min-width: 250px)": { width: 77 } } }} />
    </div>
  </div>
)

describeNative("container queries: nested containers", () => {
  it("settle in the frame that lays them out, although each level changes the one below", () => {
    withRoot((testRoot) => {
      // The inner container's width depends on the outer query, the grandchild's on the inner width.
      expect(paintedFrames(testRoot, () => testRoot.render(eApp(600)))).toBe(1)
      expectChrome(testRoot, ["outer", "e-wide.inner"], ["inner", "e-wide.inner"], ["grand", "e-wide.grand"])
      expect(paintedFrames(testRoot, () => testRoot.render(eApp(300)))).toBe(1)
      expectChrome(testRoot, ["outer", "e-narrow.inner"], ["inner", "e-narrow.inner"], ["grand", "e-narrow.grand"])
      expect(paintedFrames(testRoot, () => testRoot.render(eApp(600)))).toBe(1)
      expectChrome(testRoot, ["outer", "e-wide.inner"], ["inner", "e-wide.inner"], ["grand", "e-wide.grand"])
    })
  })
})

// ── Units and the content box of a query ──

describeNative("container queries: lengths and the content box", () => {
  it("em is the container's font size and rem the root's", () => {
    withRoot((testRoot) => {
      // Inside a root of the default size: the first element's own font size is the rem.
      testRoot.render(
        <div>
          <div testId="box" style={{ containerType: "inline-size", fontSize: 20, width: 400 }}>
            <div
              testId="child"
              style={{
                width: 10,
                height: 10,
                "@container": {
                  "(min-width: 20em)": { width: 20 },
                  "(min-width: 25rem)": { height: 25 },
                  "(min-width: 401px)": { marginLeft: 9 },
                },
              }}
            />
          </div>
        </div>
      )
      expectChrome(testRoot, ["box", "i.box"], ["child", "i.child"])
    })
  })

  it("measures the content box: padding, border and the scrollbar gutter are not in it", () => {
    withRoot((testRoot) => {
      testRoot.render(
        <div
          testId="box"
          style={{ containerType: "inline-size", width: 400, padding: 20, borderWidth: 5, borderStyle: "solid", borderColor: "#fff" }}
        >
          <div
            testId="child"
            style={{
              width: 10,
              height: 10,
              "@container": { "(min-width: 350px)": { width: 20 }, "(min-width: 351px)": { height: 30 } },
            }}
          />
        </div>
      )
      expectChrome(testRoot, ["box", "j.box"], ["child", "j.child"])
    })
  })

  it("compares the range syntax and the combinators at the edges", () => {
    withRoot((testRoot) => {
      const child = (testId: string, width: number) => (
        <div testId={`${testId}-box`} style={{ containerType: "inline-size", width }}>
          <div
            testId={testId}
            style={{
              width: 10,
              height: 10,
              "@container": {
                "(400px <= width < 800px)": { width: 20 },
                "(width > 399px) and (width <= 400px)": { height: 20 },
                "(width < 400px) or (width >= 800px)": { marginLeft: 5 },
                "not (width < 800px)": { marginTop: 4 },
                "(width = 800px)": { paddingLeft: 1, width: 11 },
              },
            }}
          />
        </div>
      )
      testRoot.render(
        <div>
          {child("l-399", 399)}
          {child("l-400", 400)}
          {child("l-799", 799)}
          {child("l-800", 800)}
        </div>
      )
      for (const name of ["l-399", "l-400", "l-799", "l-800"]) {
        expectChrome(testRoot, [`${name}-box`, `${name}.box`], [name, `${name}.child`])
      }
    })
  })

  it("an unknown feature is false, and so is its negation", () => {
    withRoot((testRoot) => {
      testRoot.render(
        <div style={{ containerType: "size", width: 400, height: 300 }}>
          <div
            testId="child"
            style={{
              width: 10,
              height: 10,
              "@container": {
                "(foo: bar)": { width: 20 },
                "not (foo: bar)": { height: 20 },
                "style(--theme: dark)": { marginLeft: 5 },
                "(min-width: 100px) and (foo: bar)": { marginTop: 5 },
                "(min-width: 100px) or (foo: bar)": { paddingLeft: 3 },
              },
            }}
          />
        </div>
      )
      expect(laidOut(testRoot, "child")).toEqual([0, 0, 10, 10])
    })
  })
})

// ── G: size containment ──

describeNative("container queries: size containment", () => {
  const item = { padding: 10, borderWidth: 2, borderStyle: "solid", borderColor: "#fff" } as const
  const wide = <div style={{ width: 200, height: 50 }} />

  it("an inline-size container in a flex row is its padding and border wide, whatever it holds", () => {
    withRoot((testRoot) => {
      testRoot.render(
        <div style={{ display: "flex", width: 600, alignItems: "flex-start" }}>
          <div testId="plain" style={item}>{wide}</div>
          <div testId="contained" style={{ ...item, containerType: "inline-size" }}>{wide}</div>
          <div testId="sized" style={{ ...item, containerType: "inline-size", width: 100 }}>{wide}</div>
          <div testId="grow" style={{ ...item, containerType: "inline-size", flexGrow: 1 }}>{wide}</div>
        </div>
      )
      for (const name of ["plain", "contained", "sized", "grow"]) {
        expectChrome(testRoot, ["plain", "g-row.plain"], [name, `g-row.${name}`])
      }
    })
  })

  it("min-width still clamps it", () => {
    withRoot((testRoot) => {
      testRoot.render(
        <div style={{ display: "flex", width: 600, alignItems: "flex-start" }}>
          <div testId="min" style={{ ...item, containerType: "inline-size", minWidth: 100 }}>{wide}</div>
        </div>
      )
      expectChrome(testRoot, ["min", "g-min.contained"], ["min", "g-min.contained"])
    })
  })

  it("is the same in a column flex container, where size containment takes the height too", () => {
    withRoot((testRoot) => {
      testRoot.render(
        <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start", width: 600 }}>
          <div testId="plain" style={item}>{wide}</div>
          <div testId="contained" style={{ ...item, containerType: "inline-size" }}>{wide}</div>
          <div testId="size" style={{ ...item, containerType: "size" }}>{wide}</div>
        </div>
      )
      for (const name of ["plain", "contained", "size"]) {
        expectChrome(testRoot, ["plain", "g-column.plain"], [name, `g-column.${name}`])
      }
    })
  })

  it("an absolutely positioned container with no width shrinks to nothing", () => {
    withRoot((testRoot) => {
      const box = (name: string, contained: boolean) => (
        <div testId={`${name}-box`} style={{ position: "relative", width: 600, height: 100 }}>
          <div
            testId={name}
            style={{
              position: "absolute",
              left: 10,
              top: 5,
              ...item,
              ...(contained ? { containerType: "inline-size" as const } : {}),
            }}
          >
            {wide}
          </div>
        </div>
      )
      testRoot.render(
        <div>
          {box("plain", false)}
          {box("contained", true)}
        </div>
      )
      expectChrome(testRoot, ["plain-box", "g-abs.plain-box"], ["plain", "g-abs.plain"])
      expectChrome(testRoot, ["contained-box", "g-abs.contained-box"], ["contained", "g-abs.contained"])
    })
  })

  it("a size container in a block takes the block's width and no height from its content", () => {
    withRoot((testRoot) => {
      testRoot.render(
        <div>
          <div testId="size" style={{ ...item, containerType: "size", width: 300 }}>{wide}</div>
        </div>
      )
      expect(laidOut(testRoot, "size")).toEqual(chrome("g-block.size").map((v, i) => (i < 2 ? 0 : v)))
    })
  })
})

// ── H: display: none ──

describeNative("container queries: display", () => {
  const row = (width: number) => (
    <div style={{ containerType: "inline-size", width }}>
      <div style={{ display: "flex", gap: 10 }}>
        <div testId="a" style={{ width: 50, height: 20 }} />
        <div testId="b" style={{ width: 50, height: 20, "@container": { "(max-width: 350px)": { display: "none" } } }} />
        <div testId="c" style={{ width: 50, height: 20 }} />
      </div>
    </div>
  )
  const present = (testRoot: TestRoot, testId: string) => {
    testRoot.renderer.dispatchNativeEvents()
    const element = testRoot.renderer.findByTestId(testId)
    return element ? testRoot.renderer.getElementBounds(element.id) : null
  }

  it("a block that hides the element leaves no gap slot, in the frame that lays it out", () => {
    withRoot((testRoot) => {
      expect(paintedFrames(testRoot, () => testRoot.render(row(500)))).toBe(1)
      expect(laidOut(testRoot, "c", "a")).toEqual(chrome("h-wide.c", "h-wide.a"))
      expect(laidOut(testRoot, "b", "a")).toEqual(chrome("h-wide.b", "h-wide.a"))
      expect(paintedFrames(testRoot, () => testRoot.render(row(300)))).toBe(1)
      expect(present(testRoot, "b")).toBeNull()
      expect(laidOut(testRoot, "c", "a")).toEqual(chrome("h-narrow.c", "h-narrow.a"))
      expect(paintedFrames(testRoot, () => testRoot.render(row(500)))).toBe(1)
      expect(laidOut(testRoot, "c", "a")).toEqual(chrome("h-wide.c", "h-wide.a"))
    })
  })

  it("a block can show what the base style hides", () => {
    withRoot((testRoot) => {
      const app = (width: number) => (
        <div style={{ containerType: "inline-size", width }}>
          <div testId="child" style={{ display: "none", width: 50, height: 20, "@container": { "(min-width: 400px)": { display: "block" } } }} />
        </div>
      )
      testRoot.render(app(300))
      expect(present(testRoot, "child")).toBeNull()
      expect(paintedFrames(testRoot, () => testRoot.render(app(500)))).toBe(1)
      expect(laidOut(testRoot, "child")).toEqual([0, 0, 50, 20])
    })
  })
})

// ── State styles, transitions, observers ──

type Rgba = [number, number, number, number]
const RED: Rgba = [255, 0, 0, 255]
const BLUE: Rgba = [0, 0, 255, 255]
const GREEN: Rgba = [0, 255, 0, 255]
const YELLOW: Rgba = [255, 255, 0, 255]

function sample(testRoot: TestRoot, name: string, x: number, y: number): Rgba {
  const file = `${SHOTS_DIR}/container-query-${name}.png`
  fs.rmSync(file, { force: true })
  testRoot.renderer.captureScreenshot(file)
  return readLogicalPixels(file, testRoot.renderer).at(x, y)
}

describeNative("container queries: state styles", () => {
  const swatch = (containerWidth: number, style: Record<string, unknown>) => (
    <div style={{ width: containerWidth }}>
      <div style={{ containerType: "inline-size" }}>
        <div style={{ width: 100, height: 100, backgroundColor: "#ff0000", ...style }} />
      </div>
    </div>
  )

  it("hover beats a container block, which beats the base style", () => {
    withRoot((testRoot) => {
      const tree = (width: number) =>
        swatch(width, {
          "@container": { "(min-width: 300px)": { backgroundColor: "#0000ff" } },
          hover: { backgroundColor: "#00ff00" },
        })
      testRoot.render(tree(400))
      testRoot.renderer.nativeSimulateMouseMove(500, 500)
      expect(sample(testRoot, "wide", 50, 50)).toEqual(BLUE)
      testRoot.renderer.nativeSimulateMouseMove(50, 50)
      expect(sample(testRoot, "wide-hover", 50, 50)).toEqual(GREEN)
      testRoot.renderer.nativeSimulateMouseMove(500, 500)
      testRoot.render(tree(200))
      expect(sample(testRoot, "narrow", 50, 50)).toEqual(RED)
      testRoot.renderer.nativeSimulateMouseMove(50, 50)
      expect(sample(testRoot, "narrow-hover", 50, 50)).toEqual(GREEN)
    })
  })

  it("a block may carry a state key, which merges into the style's own and is later than it", () => {
    withRoot((testRoot) => {
      const tree = (width: number) =>
        swatch(width, {
          hover: { backgroundColor: "#00ff00" },
          "@container": { "(min-width: 300px)": { hover: { backgroundColor: "#ffff00" } } },
        })
      testRoot.render(tree(400))
      testRoot.renderer.nativeSimulateMouseMove(50, 50)
      expect(sample(testRoot, "block-hover-wide", 50, 50)).toEqual(YELLOW)
      testRoot.render(tree(200))
      expect(sample(testRoot, "block-hover-narrow", 50, 50)).toEqual(GREEN)
    })
  })
})

describeNative("container queries: transitions", () => {
  const app = (width: number) => (
    <div style={{ width }}>
      <div style={{ containerType: "inline-size" }}>
        <div
          testId="child"
          style={{
            width: 100,
            height: 20,
            transition: "width 1s linear",
            "@container": { "(min-width: 300px)": { width: 300 } },
          }}
        />
      </div>
    </div>
  )

  it("the first frame shows the final width and animates nothing", () => {
    withRoot((testRoot) => {
      testRoot.renderer.clockPause()
      testRoot.render(app(400))
      expect(laidOut(testRoot, "child")[2]).toBe(300)
      testRoot.renderer.clockFastForward(500)
      expect(laidOut(testRoot, "child")[2]).toBe(300)
    })
  })

  it("a flip starts the transition from the width on screen", () => {
    withRoot((testRoot) => {
      testRoot.renderer.clockPause()
      testRoot.render(app(400))
      testRoot.render(app(200))
      expect(laidOut(testRoot, "child")[2], "the frame of the flip still shows the old width").toBe(300)
      testRoot.renderer.clockFastForward(500)
      expect(laidOut(testRoot, "child")[2]).toBeCloseTo(200, 0)
      testRoot.renderer.clockFastForward(1000)
      expect(laidOut(testRoot, "child")[2]).toBe(100)
    })
  })
})

describeNative("container queries: what a pass before present must not report", () => {
  const app = (width: number, onLayout: (event: EventPayload) => void, ref?: (instance: PublicInstance | null) => void) => (
    <div style={{ width }}>
      <div style={{ containerType: "inline-size" }}>
        <div
          ref={ref}
          testId="child"
          onLayout={onLayout}
          style={{ width: 100, height: 20, "@container": { "(min-width: 300px)": { width: 300 } } }}
        >
          <text>hello</text>
        </div>
      </div>
    </div>
  )

  it("onLayout reports the final size once at mount and once for a flip", () => {
    withRoot((testRoot) => {
      const sizes: number[][] = []
      const onLayout = (event: EventPayload) => sizes.push([event.width!, event.height!])
      testRoot.render(app(400, onLayout))
      testRoot.renderer.dispatchNativeEvents()
      expect(sizes).toEqual([[300, 20]])
      testRoot.render(app(200, onLayout))
      testRoot.renderer.dispatchNativeEvents()
      expect(sizes).toEqual([
        [300, 20],
        [100, 20],
      ])
    })
  })

  it("a ResizeObserver sees the final size and nothing in between", () => {
    withRoot((testRoot) => {
      let child: PublicInstance | null = null
      const entries: ResizeObserverEntry[] = []
      const onLayout = () => {}
      testRoot.render(app(400, onLayout, (instance) => (child = instance ?? child)))
      new ResizeObserver((batch) => entries.push(...batch)).observe(child!)
      testRoot.renderer.dispatchNativeEvents()
      expect(entries.map((entry) => entry.contentRect.width)).toEqual([300])
      testRoot.render(app(200, onLayout, (instance) => (child = instance ?? child)))
      testRoot.renderer.dispatchNativeEvents()
      expect(entries.map((entry) => entry.contentRect.width)).toEqual([300, 100])
      testRoot.render(app(400, onLayout, (instance) => (child = instance ?? child)))
      testRoot.renderer.dispatchNativeEvents()
      expect(entries.map((entry) => entry.contentRect.width)).toEqual([300, 100, 300])
    })
  })

  it("the text is painted once for a frame, however many passes built it", () => {
    withRoot((testRoot) => {
      testRoot.render(app(400, () => {}))
      expect(testRoot.renderer.getPaintedText()).toEqual(["hello"])
      testRoot.render(app(200, () => {}))
      expect(testRoot.renderer.getPaintedText()).toEqual(["hello"])
    })
  })

  it("element bounds are those of the presented pass", () => {
    withRoot((testRoot) => {
      testRoot.render(app(400, () => {}))
      expect(laidOut(testRoot, "child")).toEqual([0, 0, 300, 20])
      testRoot.render(app(200, () => {}))
      expect(laidOut(testRoot, "child")).toEqual([0, 0, 100, 20])
    })
  })
})

describeNative("container queries: text and virtual lists", () => {
  const heightOf = (testRoot: TestRoot, testId: string) => laidOut(testRoot, testId)[3]

  it("a font size in cqw follows the container in the frame that lays it out, on a text and on a span", () => {
    const app = (width: number) => (
      <div style={{ width }}>
        <div style={{ containerType: "inline-size" }}>
          <text testId="plain" style={{ fontSize: "10cqw" }}>
            m
          </text>
          <text testId="spanned">
            <span style={{ fontSize: "10cqw" }}>m</span>
          </text>
        </div>
      </div>
    )
    withRoot((testRoot) => {
      expect(paintedFrames(testRoot, () => testRoot.render(app(400)))).toBe(1)
      const [plain, spanned] = [heightOf(testRoot, "plain"), heightOf(testRoot, "spanned")]
      expect(plain).toBeGreaterThan(40)
      expect(spanned).toBeGreaterThan(40)
      expect(paintedFrames(testRoot, () => testRoot.render(app(200)))).toBe(1)
      // Line boxes are whole pixels, so half is within one.
      expect(Math.abs(heightOf(testRoot, "plain") - plain / 2)).toBeLessThanOrEqual(1)
      expect(Math.abs(heightOf(testRoot, "spanned") - spanned / 2)).toBeLessThanOrEqual(1)
    })
  })

  it("the rows of a virtual list under a container read it", () => {
    const app = (width: number) => (
      <div style={{ width }}>
        <div style={{ containerType: "inline-size" }}>
          <virtual-list overdraw={0} estimatedItemHeight={40} style={{ width: "100%", height: 200 }}>
            {[0, 1].map((row) => (
              <div
                key={row}
                testId={`row-${row}`}
                style={{ height: 40, width: 100, "@container": { "(min-width: 300px)": { width: "50cqw" } } }}
              />
            ))}
          </virtual-list>
        </div>
      </div>
    )
    withRoot((testRoot) => {
      testRoot.render(app(400))
      expect(laidOut(testRoot, "row-0")[2]).toBe(200)
      testRoot.render(app(200))
      expect(laidOut(testRoot, "row-0")[2]).toBe(100)
      testRoot.render(app(600))
      expect(laidOut(testRoot, "row-1")[2]).toBe(300)
    })
  })
})

describeNative("container queries: a container is no layout-contained box", () => {
  it("an absolute child is placed against the positioned ancestor outside the container", () => {
    withRoot((testRoot) => {
      testRoot.render(
        <div>
          <div testId="outer" style={{ position: "relative", height: 100, marginTop: 50 }}>
            <div testId="box" style={{ containerType: "inline-size", width: 100, height: 50, marginLeft: 30 }}>
              <div testId="child" style={{ position: "absolute", left: 0, top: 0, width: 5, height: 5 }} />
            </div>
          </div>
        </div>
      )
      expectChrome(testRoot, ["outer", "p-abs.outer"], ["box", "p-abs.box"], ["child", "p-abs.child"])
    })
  })

  it("a positioned child with a z-index paints above a later sibling with a lower one", () => {
    // The oracle page paints the child on top as well (`document.elementFromPoint`), while
    // `contain: layout` would put the sibling over it.
    withRoot((testRoot) => {
      testRoot.render(
        <div>
          <div style={{ containerType: "inline-size", width: 100, height: 50 }}>
            <div style={{ position: "relative", zIndex: 5, width: 50, height: 50, backgroundColor: "#ff0000" }} />
          </div>
          <div style={{ position: "relative", zIndex: 1, marginTop: -50, width: 50, height: 50, backgroundColor: "#0000ff" }} />
        </div>
      )
      expect(sample(testRoot, "stacking", 10, 10)).toEqual(RED)
    })
  })
})
