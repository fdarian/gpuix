import fs from "fs"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer, type TestRoot } from "../testing.js"
import { SHOTS_DIR } from "./test-utils"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Renderer = TestRoot["renderer"]

function box(renderer: Renderer, testId: string) {
  const element = renderer.findByTestId(testId)
  expect(element, `missing testId ${testId}`).toBeDefined()
  const bounds = renderer.getElementBounds(element!.id)
  expect(bounds, `no painted bounds for ${testId}`).toBeDefined()
  return bounds!
}

/** Mounts `ui`, runs `check`, and always unmounts. */
function withRoot(ui: React.ReactElement, check: (renderer: Renderer) => void) {
  const { render, renderer, unmount } = createTestRoot()
  try {
    render(ui)
    check(renderer)
  } finally {
    unmount()
  }
}

describeNative("DOM layout: text nodes", () => {
  it("empty and white-space-only text between flex items takes no gap slot", () => {
    withRoot(
      <div style={{ display: "flex", gap: 10 }}>
        <div testId="a" style={{ width: 20, height: 20 }} />
        {""}
        {" "}
        <div testId="b" style={{ width: 20, height: 20 }} />
      </div>,
      (renderer) => {
        expect(box(renderer, "b").x - box(renderer, "a").x).toBe(30)
      }
    )
  })
})

describeNative("DOM layout: position", () => {
  it("defaults to static: insets do nothing and the box is not a containing block", () => {
    withRoot(
      <div style={{ width: 300, height: 200 }}>
        <div testId="static" style={{ top: 20, left: 20, width: 50, height: 50 }}>
          <div testId="abs" style={{ position: "absolute", top: 5, left: 7, width: 10, height: 10 }} />
        </div>
      </div>,
      (renderer) => {
        const root = box(renderer, "static")
        expect([root.x, root.y]).toEqual([0, 0])
        // The static parent is skipped: the box resolves against the window.
        expect([box(renderer, "abs").x, box(renderer, "abs").y]).toEqual([7, 5])
      }
    )
  })

  it("relative offsets the box by its insets without moving siblings", () => {
    withRoot(
      <div>
        <div testId="shifted" style={{ position: "relative", top: 10, left: 12, width: 50, height: 20 }} />
        <div testId="next" style={{ width: 50, height: 20 }} />
      </div>,
      (renderer) => {
        expect(box(renderer, "shifted")).toMatchObject({ x: 12, y: 10 })
        expect(box(renderer, "next")).toMatchObject({ x: 0, y: 20 })
      }
    )
  })

  it("absolute resolves against the nearest positioned ancestor, not its parent", () => {
    withRoot(
      <div style={{ paddingLeft: 40, paddingTop: 30 }}>
        <div testId="cb" style={{ position: "relative", width: 300, height: 200, borderWidth: 2, borderColor: "#fff" }}>
          <div testId="middle" style={{ marginLeft: 50, marginTop: 60, width: 100, height: 100 }}>
            <div
              testId="abs"
              style={{ position: "absolute", top: 5, left: 7, width: "50%", height: 20, minWidth: "10%" }}
            />
          </div>
        </div>
      </div>,
      (renderer) => {
        const cb = box(renderer, "cb")
        const abs = box(renderer, "abs")
        // Offsets start at the padding box, inside the 2px border.
        expect(abs.x).toBe(cb.x + 2 + 7)
        expect(abs.y).toBe(cb.y + 2 + 5)
        // 50% of the padding box (296), not of the 100px parent.
        expect(abs.width).toBe(148)
      }
    )
  })

  it("right and bottom measure from the containing block's far edges", () => {
    withRoot(
      <div testId="cb" style={{ position: "relative", width: 300, height: 200 }}>
        <div style={{ marginLeft: 50, width: 100, height: 100 }}>
          <div testId="abs" style={{ position: "absolute", right: 10, bottom: 20, width: 40, height: 30 }} />
        </div>
      </div>,
      (renderer) => {
        const cb = box(renderer, "cb")
        expect(box(renderer, "abs")).toMatchObject({
          x: cb.x + 300 - 10 - 40,
          y: cb.y + 200 - 20 - 30,
        })
      }
    )
  })

  it("left and right together stretch across the containing block", () => {
    withRoot(
      <div testId="cb" style={{ position: "relative", width: 300, height: 200 }}>
        <div style={{ marginLeft: 50, width: 100, height: 100 }}>
          <div testId="abs" style={{ position: "absolute", left: 10, right: 20, top: 0, height: 30 }} />
        </div>
      </div>,
      (renderer) => {
        const cb = box(renderer, "cb")
        expect(box(renderer, "abs")).toMatchObject({ x: cb.x + 10, width: 270 })
      }
    )
  })

  it("auto insets keep the static position", () => {
    withRoot(
      <div testId="cb" style={{ position: "relative", width: 300, height: 200 }}>
        <div style={{ height: 40 }} />
        <div style={{ marginLeft: 50, width: 100, height: 100 }}>
          <div testId="abs" style={{ position: "absolute", width: 10, height: 10 }} />
        </div>
      </div>,
      (renderer) => {
        const cb = box(renderer, "cb")
        expect(box(renderer, "abs")).toMatchObject({ x: cb.x + 50, y: cb.y + 40 })
      }
    )
  })

  it("an absolute box is a containing block for absolute descendants", () => {
    withRoot(
      <div style={{ position: "relative", width: 400, height: 400 }}>
        <div testId="a" style={{ position: "absolute", top: 100, left: 120, width: 80, height: 80 }}>
          <div style={{ width: 30, height: 30 }}>
            <div testId="b" style={{ position: "absolute", top: 10, left: 11, width: 5, height: 5 }} />
          </div>
        </div>
      </div>,
      (renderer) => {
        const a = box(renderer, "a")
        expect(box(renderer, "b")).toMatchObject({ x: a.x + 11, y: a.y + 10 })
      }
    )
  })

  it("an absolute box without a positioned ancestor resolves against the window", () => {
    withRoot(
      <div>
        <div style={{ marginLeft: 100, marginTop: 100, width: 50, height: 50 }}>
          <div testId="abs" style={{ position: "absolute", top: 3, left: 4, width: 5, height: 5 }} />
        </div>
      </div>,
      (renderer) => {
        expect(box(renderer, "abs")).toMatchObject({ x: 4, y: 3 })
      }
    )
  })

  it("fixed resolves against the window even inside a positioned ancestor", () => {
    withRoot(
      <div>
        <div style={{ position: "relative", marginLeft: 100, marginTop: 100, width: 200, height: 200 }}>
          <div testId="fixed" style={{ position: "fixed", top: 10, left: 20, width: 5, height: 5 }} />
          <div testId="abs" style={{ position: "absolute", top: 10, left: 20, width: 5, height: 5 }} />
        </div>
      </div>,
      (renderer) => {
        expect(box(renderer, "fixed")).toMatchObject({ x: 20, y: 10 })
        expect(box(renderer, "abs")).toMatchObject({ x: 120, y: 110 })
      }
    )
  })

  it("percentage insets resolve against the containing block", () => {
    withRoot(
      <div testId="cb" style={{ position: "relative", width: 400, height: 200 }}>
        <div style={{ marginLeft: 50, width: 100, height: 100 }}>
          <div testId="abs" style={{ position: "absolute", top: "50%", left: "25%", width: 10, height: 10 }} />
        </div>
      </div>,
      (renderer) => {
        const cb = box(renderer, "cb")
        expect(box(renderer, "abs")).toMatchObject({ x: cb.x + 100, y: cb.y + 100 })
      }
    )
  })
})

const SVG_FIXTURE = [
  '<svg xmlns="http://www.w3.org/2000/svg" width="240" height="140" viewBox="0 0 240 140">',
  '<rect x="0" y="0" width="240" height="140" fill="#1e2d59"/>',
  "</svg>",
].join("")
const IMAGE_PATH = `${SHOTS_DIR}/gpuix-layout-img-fixture.svg`

describeNative("DOM layout: aspectRatio", () => {
  it("accepts CSS ratio strings", () => {
    withRoot(
      <div>
        <div testId="wide" style={{ width: 160, aspectRatio: "16 / 9" }} />
        <div testId="square" style={{ width: 80, aspectRatio: "1" }} />
        <div testId="compact" style={{ width: 90, aspectRatio: "3/2" }} />
      </div>,
      (renderer) => {
        expect(box(renderer, "wide")).toMatchObject({ width: 160, height: 90 })
        expect(box(renderer, "square")).toMatchObject({ width: 80, height: 80 })
        expect(box(renderer, "compact")).toMatchObject({ width: 90, height: 60 })
      }
    )
  })

  describe("<img>", () => {
    fs.writeFileSync(IMAGE_PATH, SVG_FIXTURE, "utf8")

    it("takes the width from a definite height", () => {
      withRoot(<img testId="img" src={IMAGE_PATH} style={{ height: 70 }} />, (renderer) => {
        expect(box(renderer, "img")).toMatchObject({ width: 120, height: 70 })
      })
    })

    it("takes the height from a definite width", () => {
      withRoot(<img testId="img" src={IMAGE_PATH} style={{ width: 120 }} />, (renderer) => {
        expect(box(renderer, "img")).toMatchObject({ width: 120, height: 70 })
      })
    })

    it("uses its intrinsic size when neither is definite", () => {
      withRoot(<img testId="img" src={IMAGE_PATH} />, (renderer) => {
        expect(box(renderer, "img")).toMatchObject({ width: 240, height: 140 })
      })
    })

    it("derives the height from a percentage width", () => {
      withRoot(
        <div style={{ width: 360 }}>
          <img testId="img" src={IMAGE_PATH} style={{ width: "50%" }} />
        </div>,
        (renderer) => {
          const { width, height } = box(renderer, "img")
          expect(width).toBe(180)
          expect(height).toBeCloseTo(105, 0)
        }
      )
    })

    it("treats a percentage of an indefinite parent as auto", () => {
      withRoot(
        <div>
          <div testId="shrink" style={{ position: "absolute", top: 0, left: 0 }}>
            <img testId="img" src={IMAGE_PATH} style={{ width: "100%" }} />
          </div>
        </div>,
        (renderer) => {
          expect(box(renderer, "img")).toMatchObject({ width: 240, height: 140 })
        }
      )
    })

    it("keeps the proportions when maxWidth clamps it", () => {
      withRoot(<img testId="img" src={IMAGE_PATH} style={{ maxWidth: 120 }} />, (renderer) => {
        expect(box(renderer, "img")).toMatchObject({ width: 120, height: 70 })
      })
    })

    it("lets aspectRatio override the intrinsic ratio", () => {
      withRoot(
        <img testId="img" src={IMAGE_PATH} style={{ width: 100, aspectRatio: "1" }} />,
        (renderer) => {
          expect(box(renderer, "img")).toMatchObject({ width: 100, height: 100 })
        }
      )
    })
  })
})

describeNative("DOM layout: order and reverse directions", () => {
  it("row-reverse and column-reverse lay children out from the far end", () => {
    withRoot(
      <div style={{ width: 100, height: 100 }}>
        <div testId="row" style={{ display: "flex", flexDirection: "row-reverse", width: 100, height: 20 }}>
          <div testId="r1" style={{ width: 10, height: 10 }} />
          <div testId="r2" style={{ width: 10, height: 10 }} />
        </div>
        <div testId="col" style={{ display: "flex", flexDirection: "column-reverse", width: 100, height: 60 }}>
          <div testId="c1" style={{ width: 10, height: 10 }} />
          <div testId="c2" style={{ width: 10, height: 10 }} />
        </div>
      </div>,
      (renderer) => {
        expect(box(renderer, "r1").x).toBe(90)
        expect(box(renderer, "r2").x).toBe(80)
        const col = box(renderer, "col")
        expect(box(renderer, "c1").y).toBe(col.y + 50)
        expect(box(renderer, "c2").y).toBe(col.y + 40)
      }
    )
  })

  it("wrap-reverse stacks lines from the cross end", () => {
    withRoot(
      <div testId="wrap" style={{ display: "flex", flexWrap: "wrap-reverse", width: 25, height: 40 }}>
        <div testId="w1" style={{ width: 20, height: 10 }} />
        <div testId="w2" style={{ width: 20, height: 10 }} />
      </div>,
      (renderer) => {
        const wrap = box(renderer, "wrap")
        expect(box(renderer, "w1").y).toBeGreaterThan(box(renderer, "w2").y)
        expect(box(renderer, "w1").y).toBe(wrap.y + 30)
      }
    )
  })

  it("order moves flex items without changing their siblings' document order", () => {
    withRoot(
      <div style={{ display: "flex", width: 100, height: 20 }}>
        <div testId="a" style={{ order: 2, width: 10, height: 10 }} />
        <div testId="b" style={{ width: 10, height: 10 }} />
        <div testId="c" style={{ order: -1, width: 10, height: 10 }} />
        <div testId="d" style={{ order: 2, width: 10, height: 10 }} />
      </div>,
      (renderer) => {
        const xs = ["c", "b", "a", "d"].map((id) => box(renderer, id).x)
        expect(xs).toEqual([0, 10, 20, 30])
      }
    )
  })

  it("order also applies to grid items and is ignored in block layout", () => {
    withRoot(
      <div>
        <div style={{ display: "grid", gridTemplateColumns: "10px 10px", width: 20 }}>
          <div testId="g1" style={{ order: 1, height: 10 }} />
          <div testId="g2" style={{ height: 10 }} />
        </div>
        <div>
          <div testId="k1" style={{ order: 1, width: 10, height: 10 }} />
          <div testId="k2" style={{ width: 10, height: 10 }} />
        </div>
      </div>,
      (renderer) => {
        expect(box(renderer, "g2").x).toBe(0)
        expect(box(renderer, "g1").x).toBe(10)
        expect(box(renderer, "k1").y).toBeLessThan(box(renderer, "k2").y)
      }
    )
  })
})

describeNative("DOM layout: margin and display", () => {
  it("margin auto centers a flex item and pushes siblings apart", () => {
    withRoot(
      <div style={{ display: "flex", width: 200, height: 40 }}>
        <div testId="left" style={{ width: 20, height: 20 }} />
        <div testId="pushed" style={{ marginLeft: "auto", width: 20, height: 20 }} />
      </div>,
      (renderer) => {
        expect(box(renderer, "pushed").x).toBe(180)
      }
    )
    withRoot(
      <div style={{ display: "flex", width: 200, height: 40 }}>
        <div testId="centered" style={{ margin: "auto", width: 20, height: 20 }} />
      </div>,
      (renderer) => {
        expect(box(renderer, "centered")).toMatchObject({ x: 90, y: 10 })
      }
    )
  })

  it("margin percentages resolve against the containing block width", () => {
    withRoot(
      <div style={{ width: 200 }}>
        <div testId="inner" style={{ marginLeft: "10%", marginTop: "5%", width: 20, height: 20 }} />
      </div>,
      (renderer) => {
        expect(box(renderer, "inner")).toMatchObject({ x: 20, y: 10 })
      }
    )
  })

  it("display none builds no box and no gap slot", () => {
    withRoot(
      <div style={{ display: "flex", gap: 10 }}>
        <div testId="a" style={{ width: 20, height: 20 }} />
        <div testId="hidden" style={{ display: "none", width: 20, height: 20 }}>
          <div testId="child" style={{ width: 5, height: 5 }} />
        </div>
        <div testId="b" style={{ width: 20, height: 20 }} />
      </div>,
      (renderer) => {
        expect(box(renderer, "b").x - box(renderer, "a").x).toBe(30)
        expect(renderer.getElementBounds(renderer.findByTestId("hidden")!.id)).toBeNull()
        expect(renderer.getElementBounds(renderer.findByTestId("child")!.id)).toBeNull()
      }
    )
  })

  it("display block stacks children", () => {
    withRoot(
      <div style={{ display: "block" }}>
        <div testId="one" style={{ width: 10, height: 10 }} />
        <div testId="two" style={{ width: 10, height: 10 }} />
      </div>,
      (renderer) => {
        expect(box(renderer, "two").y).toBe(10)
      }
    )
  })
})

describeNative("DOM layout: grid", () => {
  const cells = (ids: string[], size = 10) =>
    ids.map((id) => <div key={id} testId={id} style={{ height: size }} />)

  it("keeps the numeric column count", () => {
    withRoot(
      <div style={{ display: "grid", gridTemplateColumns: 3, width: 90 }}>{cells(["a", "b", "c"])}</div>,
      (renderer) => {
        expect(["a", "b", "c"].map((id) => box(renderer, id).x)).toEqual([0, 30, 60])
      }
    )
  })

  it("lays out px, fr and auto tracks", () => {
    withRoot(
      <div style={{ display: "grid", gridTemplateColumns: "20px 1fr 2fr", width: 140 }}>
        {cells(["a", "b", "c"])}
      </div>,
      (renderer) => {
        expect(box(renderer, "a")).toMatchObject({ x: 0, width: 20 })
        expect(box(renderer, "b")).toMatchObject({ x: 20, width: 40 })
        expect(box(renderer, "c")).toMatchObject({ x: 60, width: 80 })
      }
    )
  })

  it("supports percentages, minmax and fit-content", () => {
    withRoot(
      <div style={{ display: "grid", gridTemplateColumns: "25% minmax(10px, 1fr) fit-content(30px)", width: 200 }}>
        {cells(["a", "b", "c"])}
      </div>,
      (renderer) => {
        expect(box(renderer, "a").width).toBe(50)
        expect(box(renderer, "b").width).toBe(150)
        expect(box(renderer, "c").width).toBe(0)
      }
    )
  })

  it("expands repeat(n) and repeat(auto-fill)", () => {
    withRoot(
      <div style={{ display: "grid", gridTemplateColumns: "repeat(2, 50px)", width: 200 }}>
        {cells(["a", "b", "c"])}
      </div>,
      (renderer) => {
        expect(box(renderer, "b").x).toBe(50)
        expect(box(renderer, "c")).toMatchObject({ x: 0, y: 10 })
      }
    )
    withRoot(
      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, 60px)", width: 200 }}>
        {cells(["a", "b", "c", "d"])}
      </div>,
      (renderer) => {
        expect(box(renderer, "c").x).toBe(120)
        expect(box(renderer, "d")).toMatchObject({ x: 0, y: 10 })
      }
    )
  })

  it("places items by line number, span and negative lines", () => {
    withRoot(
      <div style={{ display: "grid", gridTemplateColumns: "repeat(4, 10px)", gridTemplateRows: "repeat(2, 10px)" }}>
        <div testId="a" style={{ gridColumn: "2 / 4" }} />
        <div testId="b" style={{ gridColumnStart: 1, gridRow: 2 }} />
        <div testId="c" style={{ gridColumn: "span 2", gridRow: "2" }} />
        <div testId="d" style={{ gridColumnStart: -2, gridRowStart: 1 }} />
      </div>,
      (renderer) => {
        expect(box(renderer, "a")).toMatchObject({ x: 10, width: 20 })
        expect(box(renderer, "b")).toMatchObject({ x: 0, y: 10 })
        expect(box(renderer, "c")).toMatchObject({ x: 10, y: 10, width: 20 })
        expect(box(renderer, "d")).toMatchObject({ x: 30, y: 0 })
      }
    )
  })

  it("resolves named lines", () => {
    withRoot(
      <div style={{ display: "grid", gridTemplateColumns: "[start] 10px [mid] 20px [end]" }}>
        <div testId="a" style={{ gridColumn: "mid / end", height: 10 }} />
      </div>,
      (renderer) => {
        expect(box(renderer, "a")).toMatchObject({ x: 10, width: 20 })
      }
    )
  })

  it("supports gridAutoRows, gridAutoFlow and the justify properties", () => {
    withRoot(
      <div style={{ display: "grid", gridTemplateColumns: "10px 10px", gridAutoRows: 25, width: 20 }}>
        <div testId="a" />
        <div testId="b" />
        <div testId="c" />
      </div>,
      (renderer) => {
        expect(box(renderer, "c")).toMatchObject({ y: 25, height: 25 })
      }
    )
    withRoot(
      <div style={{ display: "grid", gridTemplateRows: "10px 10px", gridAutoFlow: "column", gridAutoColumns: "15px" }}>
        <div testId="a" />
        <div testId="b" />
        <div testId="c" />
      </div>,
      (renderer) => {
        expect(box(renderer, "b")).toMatchObject({ x: 0, y: 10 })
        expect(box(renderer, "c")).toMatchObject({ x: 15, y: 0, width: 15 })
      }
    )
    withRoot(
      <div style={{ display: "grid", gridTemplateColumns: "100px", justifyItems: "center" }}>
        <div testId="a" style={{ width: 20, height: 10 }} />
        <div testId="b" style={{ width: 20, height: 10, justifySelf: "end" }} />
      </div>,
      (renderer) => {
        expect(box(renderer, "a").x).toBe(40)
        expect(box(renderer, "b").x).toBe(80)
      }
    )
  })

  it("places items in template areas", () => {
    withRoot(
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "20px 1fr",
          gridTemplateRows: "10px 30px",
          gridTemplateAreas: '"head head" "side main"',
          width: 100,
        }}
      >
        <div testId="main" style={{ gridArea: "main" }} />
        <div testId="head" style={{ gridArea: "head" }} />
        <div testId="side" style={{ gridArea: "side" }} />
      </div>,
      (renderer) => {
        expect(box(renderer, "head")).toMatchObject({ x: 0, y: 0, width: 100, height: 10 })
        expect(box(renderer, "side")).toMatchObject({ x: 0, y: 10, width: 20, height: 30 })
        expect(box(renderer, "main")).toMatchObject({ x: 20, y: 10, width: 80, height: 30 })
      }
    )
  })
})

describeNative("DOM layout: paint order and zIndex", () => {
  /** Clicks the middle of the stack and returns which labelled child took it. */
  function topmost(children: (hit: (id: string) => () => void) => React.ReactElement): string {
    const { render, renderer, unmount } = createTestRoot()
    try {
      let hit = ""
      render(children((id) => () => (hit = id)))
      renderer.nativeSimulateClick(50, 50)
      renderer.dispatchNativeEvents()
      return hit
    } finally {
      unmount()
    }
  }

  const fill = { width: 100, height: 100, backgroundColor: "#336699" }
  const overlay = { position: "absolute", left: 0, top: 0 } as const

  it("paints a positioned child above an in-flow sibling listed after it", () => {
    const hit = topmost((on) => (
      <div style={{ position: "relative", width: 100, height: 100 }}>
        <div style={{ ...fill, ...overlay }} onClick={on("overlay")} />
        <div style={fill} onClick={on("in-flow")} />
      </div>
    ))
    expect(hit).toBe("overlay")
  })

  it("keeps tree order between positioned siblings", () => {
    const hit = topmost((on) => (
      <div style={{ position: "relative", width: 100, height: 100 }}>
        <div style={{ ...fill, ...overlay }} onClick={on("first")} />
        <div style={{ ...fill, ...overlay }} onClick={on("second")} />
      </div>
    ))
    expect(hit).toBe("second")
  })

  it("orders positioned siblings by zIndex", () => {
    const hit = topmost((on) => (
      <div style={{ position: "relative", width: 100, height: 100 }}>
        <div style={{ ...fill, ...overlay, zIndex: 3 }} onClick={on("three")} />
        <div style={{ ...fill, ...overlay, zIndex: 1 }} onClick={on("one")} />
        <div style={{ ...fill, ...overlay, zIndex: 2 }} onClick={on("two")} />
      </div>
    ))
    expect(hit).toBe("three")
  })

  it("paints a negative zIndex below in-flow siblings", () => {
    const hit = topmost((on) => (
      <div style={{ position: "relative", width: 100, height: 100 }}>
        <div style={fill} onClick={on("in-flow")} />
        <div style={{ ...fill, ...overlay, zIndex: -1 }} onClick={on("behind")} />
      </div>
    ))
    expect(hit).toBe("in-flow")
  })

  it("keeps tab order in document order when painting positioned children later", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      render(
        <div>
          <div testId="a" tabIndex={0} style={{ width: 20, height: 20 }} />
          <div testId="b" tabIndex={0} style={{ position: "relative", zIndex: 5, width: 20, height: 20 }} />
          <div testId="c" tabIndex={0} style={{ width: 20, height: 20 }} />
        </div>
      )
      const order: (string | undefined)[] = []
      for (let press = 0; press < 3; press++) {
        renderer.simulateKeystrokes("Tab")
        order.push(renderer.getElement(renderer.getFocusedElementId())?.testId)
      }
      expect(order).toEqual(["a", "b", "c"])
    } finally {
      unmount()
    }
  })

  it("ignores zIndex on a static box", () => {
    const hit = topmost((on) => (
      <div style={{ position: "relative", width: 100, height: 100 }}>
        <div style={{ ...fill, ...overlay }} onClick={on("overlay")} />
        <div style={{ ...fill, zIndex: 99 }} onClick={on("static")} />
      </div>
    ))
    expect(hit).toBe("overlay")
  })
})

describeNative("DOM layout: display contents", () => {
  it("splices the children into the parent's flex layout and builds no box", () => {
    withRoot(
      <div style={{ display: "flex", gap: 10 }}>
        <div testId="a" style={{ width: 20, height: 20 }} />
        <div
          testId="contents"
          style={{ display: "contents", width: 500, padding: 40, backgroundColor: "#ff0000", borderWidth: 4 }}
        >
          <div testId="b" style={{ width: 20, height: 20 }} />
          <div style={{ display: "contents" }}>
            <div testId="c" style={{ width: 20, height: 20 }} />
          </div>
        </div>
        <div testId="d" style={{ width: 20, height: 20 }} />
      </div>,
      (renderer) => {
        expect([box(renderer, "a").x, box(renderer, "b").x, box(renderer, "c").x, box(renderer, "d").x]).toEqual([
          0, 30, 60, 90,
        ])
        expect(renderer.getElementBounds(renderer.findByTestId("contents")!.id)).toBeNull()
      }
    )
  })

  it("lets the spliced children take part in the parent's order sort", () => {
    withRoot(
      <div style={{ display: "flex" }}>
        <div testId="a" style={{ order: 1, width: 10, height: 10 }} />
        <div style={{ display: "contents" }}>
          <div testId="b" style={{ order: 2, width: 10, height: 10 }} />
          <div testId="c" style={{ order: 0, width: 10, height: 10 }} />
        </div>
      </div>,
      (renderer) => {
        expect(["c", "a", "b"].map((id) => box(renderer, id).x)).toEqual([0, 10, 20])
      }
    )
  })

  it("lets an absolute child resolve past the removed box", () => {
    withRoot(
      <div testId="cb" style={{ position: "relative", width: 200, height: 100 }}>
        <div style={{ display: "contents", position: "relative", left: 50 }}>
          <div testId="abs" style={{ position: "absolute", left: 7, top: 3, width: 5, height: 5 }} />
        </div>
      </div>,
      (renderer) => {
        const cb = box(renderer, "cb")
        expect(box(renderer, "abs")).toMatchObject({ x: cb.x + 7, y: cb.y + 3 })
      }
    )
  })
})

describeNative("DOM layout: invalid values", () => {
  it("reports an invalid track list as an uncaught error", () => {
    const errors: Error[] = []
    const { render, unmount } = createTestRoot({ onUncaughtError: (error) => errors.push(error) })
    try {
      render(<div style={{ display: "grid", gridTemplateColumns: "minmax(1fr, 10px)" }} />)
      expect(errors.map((error) => error.message).join()).toMatch(/gridTemplateColumns|minmax/)
    } finally {
      unmount()
    }
  })

  it("keeps an element's previous valid style when a re-render's style is rejected", () => {
    const errors: Error[] = []
    const { render, renderer, unmount } = createTestRoot({
      onUncaughtError: (error) => errors.push(error),
    })
    try {
      render(<div testId="box" style={{ width: 50, height: 20 }} />)
      expect(box(renderer, "box").width).toBe(50)

      render(<div testId="box" style={{ width: 80, height: 20, animation: "react-grow banana" }} />)
      expect(errors.map((error) => error.message).join()).toMatch(/animation/)
      expect(box(renderer, "box").width).toBe(50)
    } finally {
      unmount()
    }
  })
})
