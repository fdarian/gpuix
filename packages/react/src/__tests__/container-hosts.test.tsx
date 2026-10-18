/// Container queries on every kind of host, and `style()` queries against custom properties, against
/// what headless Chrome lays out for the same page.
///
/// `fixtures/container-hosts-oracle.html` is that page, `fixtures/container-hosts-cases.json` the queries
/// and custom properties both pages read, and `fixtures/container-hosts-oracle.json` the boxes Chrome
/// reports (`[x, y, width, height]` from the top left of the case; regenerate with
/// `sh fixtures/container-hosts-oracle.sh`).

import fs from "fs"
import path from "path"
import type { CSSProperties, ReactElement } from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rect = [number, number, number, number]
type TestRoot = ReturnType<typeof createTestRoot>
type Style = Record<string, unknown>

const FIXTURES = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures")
const ORACLE: Record<string, Rect> = JSON.parse(fs.readFileSync(path.join(FIXTURES, "container-hosts-oracle.json"), "utf8"))
const CASES: { styleQueries: Record<string, string>; styleVariants: Record<string, Record<string, string[]>> } =
  JSON.parse(fs.readFileSync(path.join(FIXTURES, "container-hosts-cases.json"), "utf8"))

function chrome(id: string): Rect {
  const box = ORACLE[id]
  expect(box, `no oracle box ${id}`).toBeDefined()
  return box ?? [0, 0, 0, 0]
}

function withRoot(run: (testRoot: TestRoot) => void) {
  const testRoot = createTestRoot()
  try {
    run(testRoot)
  } finally {
    testRoot.unmount()
  }
}

function bounds(testRoot: TestRoot, testId: string) {
  testRoot.renderer.dispatchNativeEvents()
  const element = testRoot.renderer.findByTestId(testId)
  expect(element, `no element with testId ${testId}`).toBeDefined()
  const found = testRoot.renderer.getElementBounds(element!.id)
  expect(found, `no painted bounds for ${testId}`).toBeDefined()
  return found!
}

/** The box of `testId`, from the top left of `from`. */
function laidOut(testRoot: TestRoot, testId: string, from: string): Rect {
  const box = bounds(testRoot, testId)
  const origin = bounds(testRoot, from)
  return [box.x - origin.x, box.y - origin.y, box.width, box.height]
}

const widthOf = (testRoot: TestRoot, testId: string) => bounds(testRoot, testId).width

/** `"--x: 1; --y: 2"` or `"color: red"` as a style object. */
function declarations(text: string): Style {
  const style: Style = {}
  for (const declaration of text.split(";")) {
    const colon = declaration.indexOf(":")
    if (colon < 0) continue
    style[declaration.slice(0, colon).trim()] = declaration.slice(colon + 1).trim()
  }
  return style
}

// ── A: containers that cannot be size contained ──

const OUTER = { containerType: "inline-size", width: 500 } as const

/** The box the queries change. It is atomic, so it can sit in the text of a span. */
const target = (testId: string, extra: Style): ReactElement => (
  <span
    testId={testId}
    style={{ display: "inline-block", width: 10, height: 10, verticalAlign: "top", ...extra } as CSSProperties}
  />
)

/** Query 1 to 9 of the oracle, as the style of the box the query changes. */
const QUERIES: Record<number, Style> = {
  1: { "@container": { "(min-width: 100px)": { width: 21 } } },
  2: { "@container": { "not (min-width: 100px)": { width: 22 } } },
  3: { "@container": { nm: { width: 23 } } },
  4: { "@container": { "nm (min-width: 100px)": { width: 24 } } },
  5: { "@container": { "(min-width: 100px) or (max-width: 5px)": { width: 25 } } },
  6: { "@container": { "style(--x: 1)": { width: 26 } } },
  7: { "@container": { "nm style(--x: 1)": { width: 27 } } },
  8: { width: "10cqw" },
  9: { "@container": { "(min-height: 1px)": { width: 29 } } },
}

const KINDS = ["span", "contents", "cell", "table", "row", "block"] as const

function hostCase(kind: (typeof KINDS)[number], query: number): ReactElement {
  const id = `a-${kind}-${query}`
  const named = query === 3 || query === 4 || query === 7
  const container: Style = {
    containerType: "inline-size",
    ...(named ? { containerName: "nm" } : {}),
    ...(query === 6 || query === 7 ? { "--x": 1 } : {}),
  }
  const t = target(`${id}.t`, QUERIES[query]!)
  const style = (extra: Style) => ({ ...container, ...extra }) as CSSProperties
  const cell = { display: "table-cell" }
  let inner: ReactElement
  switch (kind) {
    case "span":
      inner = <span style={style({})}>{t}</span>
      break
    case "contents":
      inner = <div style={style({ display: "contents" })}>{t}</div>
      break
    case "cell":
      inner = (
        <div style={{ display: "table" }}>
          <div style={style(cell)}>{t}</div>
        </div>
      )
      break
    case "table":
      inner = (
        <div style={style({ display: "table" })}>
          <div style={cell}>{t}</div>
        </div>
      )
      break
    case "row":
      inner = (
        <div style={{ display: "table" }}>
          <div style={style({ display: "table-row" })}>
            <div style={cell}>{t}</div>
          </div>
        </div>
      )
      break
    case "block":
      inner = <div style={style({ width: 300 })}>{t}</div>
      break
  }
  return (
    <div testId={id} style={{ width: 600 }}>
      <div style={OUTER}>{inner}</div>
    </div>
  )
}

describeNative("container queries: an element that cannot be size contained", () => {
  for (const kind of KINDS) {
    it(`${kind}: the queries answer as Chrome does`, () => {
      withRoot((testRoot) => {
        for (let query = 1; query <= 9; query++) {
          testRoot.render(hostCase(kind, query))
          const id = `a-${kind}-${query}`
          expect(widthOf(testRoot, `${id}.t`), `${id}`).toBeCloseTo(chrome(`${id}.t`)[2], 1)
        }
      })
    })
  }

  it("is the container the lookup stops at, not one above: a size query is false, and so is its negation", () => {
    withRoot((testRoot) => {
      // The 500px container above would make query 1 true and query 2 false.
      testRoot.render(hostCase("span", 1))
      expect(widthOf(testRoot, "a-span-1.t")).toBe(10)
      testRoot.render(hostCase("span", 2))
      expect(widthOf(testRoot, "a-span-2.t")).toBe(10)
      testRoot.render(hostCase("cell", 1))
      expect(widthOf(testRoot, "a-cell-1.t")).toBe(10)
    })
  })

  it("still matches a name, answers style(), and is looked past by a container unit", () => {
    withRoot((testRoot) => {
      for (const kind of ["span", "contents", "cell", "table", "row"] as const) {
        testRoot.render(hostCase(kind, 3))
        expect(widthOf(testRoot, `a-${kind}-3.t`), `${kind}: a name alone`).toBe(23)
        testRoot.render(hostCase(kind, 7))
        expect(widthOf(testRoot, `a-${kind}-7.t`), `${kind}: a name and style()`).toBe(27)
        testRoot.render(hostCase(kind, 8))
        expect(widthOf(testRoot, `a-${kind}-8.t`), `${kind}: 10cqw is 10% of the container above`).toBe(50)
      }
    })
  })

  it("is the same box whatever it declares: it is not given a width of its own", () => {
    withRoot((testRoot) => {
      testRoot.render(
        <div testId="root" style={{ width: 600 }}>
          <div style={{ display: "table" }}>
            <div testId="cell" style={{ display: "table-cell", containerType: "size" }}>
              <div style={{ width: 100, height: 5 }} />
            </div>
          </div>
        </div>
      )
      expect(widthOf(testRoot, "cell")).toBeCloseTo(100, 1)
    })
  })
})

// ── B: a list is a container for its rows ──

/** The last `[width, height]` each row reported through `onLayout`. */
function layoutOf(sizes: Map<number, [number, number]>) {
  return (index: number) => (event: { width?: number; height?: number }) => {
    sizes.set(index, [event.width ?? Number.NaN, event.height ?? Number.NaN])
  }
}

describeNative("container queries: <virtual-list>", () => {
  for (const width of [300, 200]) {
    it(`its rows are queried against the list (${width}px)`, () => {
      withRoot((testRoot) => {
        const sizes = new Map<number, [number, number]>()
        const report = layoutOf(sizes)
        testRoot.render(
          <virtual-list
            overdraw={0}
            estimatedItemHeight={20}
            style={{ containerType: "inline-size", width, height: 200 }}
          >
            {[0, 1, 2].map((index) => (
              <div
                key={index}
                onLayout={report(index)}
                style={{ height: 20, width: "50cqw", "@container": { "(min-width: 250px)": { height: 40 } } } as CSSProperties}
              />
            ))}
          </virtual-list>
        )
        testRoot.renderer.dispatchNativeEvents()
        for (const index of [0, 1, 2]) {
          const [, , rowWidth, rowHeight] = chrome(`b-${width}.row${index}`)
          expect(sizes.get(index), `row ${index}`).toEqual([rowWidth, rowHeight])
        }
      })
    })
  }

  it("is size contained: with no height it is as tall as its padding, whatever its rows hold", () => {
    withRoot((testRoot) => {
      testRoot.render(
        <div testId="wrap" style={{ width: 300 }}>
          <virtual-list overdraw={0} estimatedItemHeight={20} style={{ containerType: "size", padding: 4 }}>
            <div style={{ height: 20 }} />
          </virtual-list>
        </div>
      )
      expect(bounds(testRoot, "wrap").height).toBeCloseTo(8, 1)
    })
  })

  it("flips its rows when the list is resized, in the frame that lays it out", () => {
    withRoot((testRoot) => {
      const sizes = new Map<number, [number, number]>()
      const report = layoutOf(sizes)
      const app = (width: number) => (
        <virtual-list
          overdraw={0}
          estimatedItemHeight={20}
          style={{ containerType: "inline-size", width, height: 200 }}
        >
          <div
            onLayout={report(0)}
            style={{ height: 20, "@container": { "(min-width: 250px)": { height: 40 } } } as CSSProperties}
          />
        </virtual-list>
      )
      testRoot.render(app(300))
      testRoot.renderer.dispatchNativeEvents()
      expect(sizes.get(0)?.[1]).toBe(40)
      const before = testRoot.renderer.getPaintedFrameCount()
      testRoot.render(app(200))
      testRoot.renderer.dispatchNativeEvents()
      expect(sizes.get(0)?.[1]).toBe(20)
      expect(testRoot.renderer.getPaintedFrameCount() - before).toBe(1)
      testRoot.render(app(300))
      testRoot.renderer.dispatchNativeEvents()
      expect(sizes.get(0)?.[1]).toBe(40)
    })
  })
})

// ── C: replaced elements ──

const IMAGE = `data:image/svg+xml;base64,${Buffer.from(
  '<svg xmlns="http://www.w3.org/2000/svg" width="50" height="40"><rect width="50" height="40" fill="#f00"/></svg>'
).toString("base64")}`

const REPLACED: Record<string, (style: Style, testId: string) => ReactElement> = {
  img: (style, testId) => <img testId={testId} src={IMAGE} style={style as CSSProperties} />,
  svg: (style, testId) => <svg testId={testId} viewBox="0 0 50 40" style={style as CSSProperties} />,
  input: (style, testId) => <input testId={testId} style={style as CSSProperties} />,
  textarea: (style, testId) => <textarea testId={testId} style={style as CSSProperties} />,
}

describeNative("container queries: replaced elements keep no natural size on the contained axes", () => {
  const cases: [string, string, Style][] = [
    ["img-plain", "img", {}],
    ["img-inline", "img", { containerType: "inline-size" }],
    ["img-size", "img", { containerType: "size" }],
    ["img-size-width", "img", { containerType: "size", width: 100 }],
    ["img-inline-width", "img", { containerType: "inline-size", width: 100 }],
    ["img-size-height", "img", { containerType: "size", height: 100 }],
    ["img-inline-height", "img", { containerType: "inline-size", height: 100 }],
    ["img-fixed", "img", { containerType: "size", width: 80, height: 60 }],
    ["svg-fixed", "svg", { containerType: "size", width: 80, height: 60 }],
    ["svg-size-width", "svg", { containerType: "size", width: 100 }],
    ["svg-size-height", "svg", { containerType: "size", height: 50 }],
    ["svg-inline-height", "svg", { containerType: "inline-size", height: 50 }],
  ]
  for (const [name, tag, style] of cases) {
    it(`${name}`, () => {
      withRoot((testRoot) => {
        // An image loads on a later turn.
        testRoot.render(<div style={{ width: 600 }}>{REPLACED[tag]!(style, name)}</div>)
        for (let turn = 0; turn < 4; turn++) testRoot.renderer.flush()
        const [, , width, height] = chrome(`c-${name}.el`)
        const found = bounds(testRoot, name)
        expect([found.width, found.height], name).toEqual([width, height])
      })
    })
  }

  it("an input and a textarea ignore it", () => {
    withRoot((testRoot) => {
      for (const tag of ["input", "textarea"]) {
        testRoot.render(<div style={{ width: 600 }}>{REPLACED[tag]!({}, "plain")}</div>)
        const plain = bounds(testRoot, "plain")
        for (const containerType of ["inline-size", "size"]) {
          testRoot.render(<div style={{ width: 600 }}>{REPLACED[tag]!({ containerType }, "contained")}</div>)
          const contained = bounds(testRoot, "contained")
          expect([contained.width, contained.height], `${tag} ${containerType}`).toEqual([plain.width, plain.height])
        }
      }
      expect(chrome("c-input-size.el")).toEqual(chrome("c-input-plain.el"))
      expect(chrome("c-textarea-size.el")).toEqual(chrome("c-textarea-plain.el"))
    })
  })
})

// ── D: style() ──

/** The box the style() query changes: 20 wide when it matches. */
const styled = (testId: string, query: string): ReactElement => (
  <div testId={testId} style={{ width: 10, height: 10, "@container": { [query]: { width: 20 } } } as CSSProperties} />
)

/** `chain` is one declaration list per nested `<div>`, outermost first. */
function nested(chain: string[], inner: ReactElement): ReactElement {
  return chain.reduceRight((inside, attrs) => <div style={declarations(attrs) as CSSProperties}>{inside}</div>, inner)
}

describeNative("style() queries against custom properties", () => {
  for (const [cls, query] of Object.entries(CASES.styleQueries)) {
    it(`${query}`, () => {
      withRoot((testRoot) => {
        for (const [name, chain] of Object.entries(CASES.styleVariants[cls]!)) {
          const id = `${cls}-${name}`
          testRoot.render(<div testId={id}>{nested(chain, styled(`${id}.t`, query))}</div>)
          expect(widthOf(testRoot, `${id}.t`), `${query} with ${chain.join(" > ")}`).toBeCloseTo(chrome(`${id}.t`)[2], 1)
        }
      })
    })
  }

  it("reads the parent, never the element itself", () => {
    withRoot((testRoot) => {
      testRoot.render(
        <div testId="d1-self">
          <div style={{ "--x": 1 } as CSSProperties}>
            <div testId="d1-self.t" style={{ "--x": 2, width: 10, height: 10, "@container": { "style(--x: 1)": { width: 20 } } } as CSSProperties} />
          </div>
        </div>
      )
      expect(widthOf(testRoot, "d1-self.t")).toBe(chrome("d1-self.t")[2])
      testRoot.render(
        <div testId="d1-selfonly">
          <div>
            <div testId="d1-selfonly.t" style={{ "--x": 1, width: 10, height: 10, "@container": { "style(--x: 1)": { width: 20 } } } as CSSProperties} />
          </div>
        </div>
      )
      expect(widthOf(testRoot, "d1-selfonly.t")).toBe(chrome("d1-selfonly.t")[2])
    })
  })

  it("a name picks the nearest element called it, whatever type it has", () => {
    withRoot((testRoot) => {
      const q = "nm style(--x: 1)"
      const cases: [string, ReactElement][] = [
        [
          "d8-named",
          <div style={{ containerName: "nm", "--x": 1 } as CSSProperties}>
            <div style={{ "--x": 5 } as CSSProperties}>{styled("d8-named.t", q)}</div>
          </div>,
        ],
        [
          "d8-namednot",
          <div style={{ containerName: "nm", "--x": 5 } as CSSProperties}>
            <div style={{ "--x": 1 } as CSSProperties}>{styled("d8-namednot.t", q)}</div>
          </div>,
        ],
        ["d8-none", <div style={{ "--x": 1 } as CSSProperties}>{styled("d8-none.t", q)}</div>],
      ]
      for (const [id, ui] of cases) {
        testRoot.render(<div testId={id}>{ui}</div>)
        expect(widthOf(testRoot, `${id}.t`), id).toBe(chrome(`${id}.t`)[2])
      }
    })
  })

  it("a size and a style feature in one query are answered by one container: the nearest size container", () => {
    withRoot((testRoot) => {
      const sized = (extra: Style) => ({ containerType: "inline-size", width: 100, ...extra }) as CSSProperties
      const cases: [string, string, ReactElement][] = [
        ["d9-between", "(min-width: 0px) and style(--x: 1)", <div style={sized({})}><div style={{ "--x": 1 } as CSSProperties}>{styled("d9-between.t", "(min-width: 0px) and style(--x: 1)")}</div></div>],
        ["d9-on", "", <div style={sized({ "--x": 1 })}><div>{styled("d9-on.t", "(min-width: 0px) and style(--x: 1)")}</div></div>],
        ["d15-between", "", <div style={sized({})}><div style={{ "--x": 1 } as CSSProperties}>{styled("d15-between.t", "(min-width: 0px) or style(--x: 1)")}</div></div>],
        ["d1-through", "", <div style={{ "--x": 1 } as CSSProperties}><div style={sized({})}><div>{styled("d1-through.t", "style(--x: 1)")}</div></div></div>],
        ["d1-above", "", <div style={sized({ "--x": 1 })}><div>{styled("d1-above.t", "style(--x: 1)")}</div></div>],
      ]
      for (const [id, , ui] of cases) {
        testRoot.render(<div testId={id}>{ui}</div>)
        expect(widthOf(testRoot, `${id}.t`), id).toBe(chrome(`${id}.t`)[2])
      }
    })
  })

  it("a block that sets a custom property is read by the boxes below it, and flips with the container", () => {
    withRoot((testRoot) => {
      const app = (width: number) => (
        <div testId={`d14-${width}`}>
          <div style={{ containerType: "inline-size", width }}>
            <div style={{ "@container": { "(min-width: 100px)": { "--x": 1 } } } as CSSProperties}>
              {styled(`d14-${width}.t`, "style(--x: 1)")}
            </div>
          </div>
        </div>
      )
      for (const width of [300, 50]) {
        testRoot.render(app(width))
        expect(widthOf(testRoot, `d14-${width}.t`), `${width}`).toBe(chrome(`d14-${width}.t`)[2])
      }
      const before = testRoot.renderer.getPaintedFrameCount()
      testRoot.render(app(300))
      expect(testRoot.renderer.getPaintedFrameCount() - before).toBe(1)
      expect(widthOf(testRoot, "d14-300.t")).toBe(20)
    })
  })

  it("a state style that sets a property changes it for the boxes below while it applies", () => {
    withRoot((testRoot) => {
      testRoot.render(
        <div style={{ width: 200, height: 100 }}>
          <div testId="parent" style={{ width: 200, height: 100, "--x": 0, hover: { "--x": 1 } } as CSSProperties}>
            {styled("t", "style(--x: 1)")}
          </div>
        </div>
      )
      testRoot.renderer.nativeSimulateMouseMove(500, 500)
      expect(widthOf(testRoot, "t")).toBe(10)
      testRoot.renderer.nativeSimulateMouseMove(100, 80)
      expect(widthOf(testRoot, "t")).toBe(20)
      testRoot.renderer.nativeSimulateMouseMove(500, 500)
      expect(widthOf(testRoot, "t")).toBe(10)
    })
  })

  it("a property set by the element and reset by the next one down is what each level computes", () => {
    withRoot((testRoot) => {
      testRoot.render(
        <div style={{ "--x": 1 } as CSSProperties}>
          <div style={{ "--x": "initial" } as CSSProperties}>{styled("cleared", "style(--x)")}</div>
          <div style={{ "--x": "inherit" } as CSSProperties}>{styled("kept", "style(--x: 1)")}</div>
          <div style={{ "--x": 2, "--y": "" } as CSSProperties}>{styled("changed", "style(--x: 2)")}</div>
        </div>
      )
      expect(widthOf(testRoot, "cleared")).toBe(10)
      expect(widthOf(testRoot, "kept")).toBe(20)
      expect(widthOf(testRoot, "changed")).toBe(20)
    })
  })

  it("a display: contents element passes its custom properties and its name on to the children spliced out of it", () => {
    withRoot((testRoot) => {
      testRoot.render(
        <div style={{ "--x": 0 } as CSSProperties}>
          <div style={{ display: "contents", "--x": 1 } as CSSProperties}>{styled("custom", "style(--x: 1)")}</div>
          <div style={{ display: "contents", containerName: "nm", "--x": 7 } as CSSProperties}>
            <div style={{ "--x": 8 } as CSSProperties}>{styled("named", "nm style(--x: 7)")}</div>
          </div>
          <div style={{ containerType: "inline-size", width: 500 }}>
            <div style={{ display: "contents", containerType: "inline-size" }}>{styled("stops", "(min-width: 100px)")}</div>
          </div>
        </div>
      )
      expect(widthOf(testRoot, "custom")).toBe(20)
      expect(widthOf(testRoot, "named")).toBe(20)
      expect(widthOf(testRoot, "stops"), "a size query is unknown at a contents container, not 500 wide").toBe(10)
    })
  })

  it("an atomic inline in a span container reads that span, not the container around the text", () => {
    withRoot((testRoot) => {
      testRoot.render(
        <div style={OUTER}>
          <span style={{ containerName: "nm", "--x": 3 } as CSSProperties}>
            <span style={{ "--x": 4 } as CSSProperties}>
              {target("inner", { "@container": { "nm style(--x: 3)": { width: 31 } } })}
            </span>
          </span>
        </div>
      )
      expect(widthOf(testRoot, "inner")).toBe(31)
    })
  })
})
