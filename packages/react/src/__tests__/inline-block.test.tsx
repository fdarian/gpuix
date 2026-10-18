/// `display: inline-block`, `inline-flex` and `inline-grid`: boxes that sit on a line of text as one
/// unit. The expected numbers are from headless Chrome (chrome-headless-shell, never the Chrome app);
/// each case says which HTML it is. The text is GpuixTestSquare at 20px with a line height of 20px,
/// whose ascent is the whole em and whose descent is none, so the strut of a line is 20px above the
/// baseline and nothing below it, and every character is 20px wide. The numbers are relative to the
/// container (`<div id=root style="width:...;font:20px/20px GpuixTestSquare">`).

import fs from "fs"
import React from "react"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const SQUARE_FONT = fs.readFileSync(new URL("./fixtures/gpuix-test-square.ttf", import.meta.url))

const TEXT = { fontFamily: "GpuixTestSquare", fontSize: 20, lineHeight: "20px", color: "#000000" } as const

type Rect = { x: number; y: number; w: number; h: number }

/** The names the elements of a render are found by, and their boxes relative to the first. */
function setup() {
  const root = createTestRoot()
  root.renderer.registerFont(SQUARE_FONT)
  const refs = new Map<string, PublicInstance>()
  const ref = (name: string) => (instance: PublicInstance | null) => {
    if (instance) refs.set(name, instance)
  }
  const rect = (name: string): Rect => {
    const element = refs.get(name)
    expect(element, `no element ${name}`).toBeDefined()
    const box = element!.getBoundingClientRect()
    const origin = refs.get("root")!.getBoundingClientRect()
    return { x: box.x - origin.x, y: box.y - origin.y, w: box.width, h: box.height }
  }
  return { ...root, ref, rect, element: (name: string) => refs.get(name)! }
}

function expectRect(actual: Rect, expected: Rect, context: string, tolerance = 0.51) {
  for (const key of ["x", "y", "w", "h"] as const) {
    expect(Math.abs(actual[key] - expected[key]), `${context}: ${key} ${actual[key]} != ${expected[key]}`).toBeLessThanOrEqual(
      tolerance
    )
  }
}

const box = (width: number, height: number) => ({ display: "inline-block", width, height }) as const

describeNative("inline-block", () => {
  it("sits on the baseline beside text, with its margin around it", () => {
    // <div id=root style="width:300px;font:20px/20px GpuixTestSquare"><span id=t1>AA</span>
    //   <span id=a style="display:inline-block;width:30px;height:30px"></span><span id=t2>AA</span>
    //   <span id=b style="display:inline-block;width:20px;height:10px;margin:2px 4px"></span><span id=t3>A</span></div>
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ width: 300, ...TEXT }}>
          <span ref={ref("t1")}>AA</span>
          <span ref={ref("a")} style={box(30, 30)} />
          <span ref={ref("t2")}>AA</span>
          <span ref={ref("b")} style={{ ...box(20, 10), marginTop: 2, marginBottom: 2, marginLeft: 4, marginRight: 4 }} />
          <span ref={ref("t3")}>A</span>
        </div>
      )
      expectRect(rect("root"), { x: 0, y: 0, w: 300, h: 30 }, "root")
      expectRect(rect("t1"), { x: 0, y: 10, w: 40, h: 20 }, "t1")
      expectRect(rect("a"), { x: 40, y: 0, w: 30, h: 30 }, "a")
      expectRect(rect("t2"), { x: 70, y: 10, w: 40, h: 20 }, "t2")
      expectRect(rect("b"), { x: 114, y: 18, w: 20, h: 10 }, "b")
      expectRect(rect("t3"), { x: 138, y: 10, w: 20, h: 20 }, "t3")
    } finally {
      unmount()
    }
  })

  it("makes a line of its own when nothing is around it, and the line has the strut's descender gap", () => {
    // <div id=root style="width:300px;font:20px/20px Menlo"><span id=a style="display:inline-block;width:30px;height:30px"></span></div>
    // Chrome: root is 33px tall, a is 30x30 at the top. Menlo's descent is 3px of a 20px line, which the 30px box
    // sits above.
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ width: 300, fontFamily: "Menlo", fontSize: 20, lineHeight: "20px" }}>
          <span ref={ref("a")} style={box(30, 30)} />
        </div>
      )
      // Chrome has the descent 3px, and gpui keeps it at 3.08 and rounds the line up.
      expect(rect("root").h).toBeGreaterThanOrEqual(33)
      expect(rect("root").h).toBeLessThanOrEqual(34)
      expectRect(rect("a"), { x: 0, y: 0, w: 30, h: 30 }, "a")
    } finally {
      unmount()
    }
  })

  it("wraps with the text: several boxes fill a line and the next starts a new one", () => {
    // <div id=root style="width:100px;font:20px/20px GpuixTestSquare">
    //   <span id=b1 style="display:inline-block;width:40px;height:20px"></span><span id=b2 style="display:inline-block;width:40px;height:30px"></span> 
    //   <span id=b3 style="display:inline-block;width:40px;height:20px"></span><span id=b4 style="display:inline-block;width:40px;height:10px"></span>
    //   <span id=b5 style="display:inline-block;width:40px;height:20px"></span></div>
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ width: 100, ...TEXT }}>
          <span ref={ref("b1")} style={box(40, 20)} />
          <span ref={ref("b2")} style={box(40, 30)} /> <span ref={ref("b3")} style={box(40, 20)} />
          <span ref={ref("b4")} style={box(40, 10)} />
          <span ref={ref("b5")} style={box(40, 20)} />
        </div>
      )
      expectRect(rect("root"), { x: 0, y: 0, w: 100, h: 70 }, "root")
      expectRect(rect("b1"), { x: 0, y: 10, w: 40, h: 20 }, "b1")
      expectRect(rect("b2"), { x: 40, y: 0, w: 40, h: 30 }, "b2")
      expectRect(rect("b3"), { x: 0, y: 30, w: 40, h: 20 }, "b3")
      expectRect(rect("b4"), { x: 40, y: 40, w: 40, h: 10 }, "b4")
      expectRect(rect("b5"), { x: 0, y: 50, w: 40, h: 20 }, "b5")
    } finally {
      unmount()
    }
  })

  it("shrinks to fit, and a child's percentage is of the width it came to", () => {
    // <div id=root style="width:300px;font:20px/20px GpuixTestSquare"><span id=t1>A</span>
    //   <span id=ib style="display:inline-block"><span id=c1 style="display:block;width:50%;height:10px"></span>
    //     <span id=c2 style="display:block;width:40px;height:10px"></span></span>
    //   <span id=ib2 style="display:inline-block;width:50%"><span id=d1 style="display:block;width:50%;height:10px"></span></span></div>
    // Chrome: ib is 40 wide, c1 20; ib2 is 150 wide (of the 300), d1 75.
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ width: 300, ...TEXT }}>
          <span ref={ref("t1")}>A</span>
          <span ref={ref("ib")} style={{ display: "inline-block" }}>
            <span ref={ref("c1")} style={{ display: "block", width: "50%", height: 10 }} />
            <span ref={ref("c2")} style={{ display: "block", width: 40, height: 10 }} />
          </span>
          <span ref={ref("ib2")} style={{ display: "inline-block", width: "50%" }}>
            <span ref={ref("d1")} style={{ display: "block", width: "50%", height: 10 }} />
          </span>
        </div>
      )
      expectRect(rect("ib"), { x: 20, y: 0, w: 40, h: 20 }, "ib")
      expectRect(rect("c1"), { x: 20, y: 0, w: 20, h: 10 }, "c1")
      expectRect(rect("c2"), { x: 20, y: 10, w: 40, h: 10 }, "c2")
      expectRect(rect("ib2"), { x: 60, y: 10, w: 150, h: 10 }, "ib2")
      expectRect(rect("d1"), { x: 60, y: 10, w: 75, h: 10 }, "d1")
    } finally {
      unmount()
    }
  })

  it("wraps its own text in the width it shrinks to, and is as wide as its text when that fits", () => {
    // <div id=root style="width:300px;font:20px/20px GpuixTestSquare"><span id=t1>A</span>
    //   <span id=ib style="display:inline-block;white-space:pre-line">AA
    // AAA</span><span id=t2>A</span><span id=ib2 style="display:inline-block;width:50px"><span id=inner>AAA AAA</span></span></div>
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ width: 300, ...TEXT }}>
          <span ref={ref("t1")}>A</span>
          <span ref={ref("ib")} style={{ display: "inline-block", whiteSpace: "pre-line" }}>
            {"AA\nAAA"}
          </span>
          <span ref={ref("t2")}>A</span>
          <span ref={ref("ib2")} style={{ display: "inline-block", width: 50 }}>
            <span ref={ref("inner")}>AAA AAA</span>
          </span>
        </div>
      )
      expectRect(rect("root"), { x: 0, y: 0, w: 300, h: 40 }, "root")
      expectRect(rect("t1"), { x: 0, y: 20, w: 20, h: 20 }, "t1")
      expectRect(rect("ib"), { x: 20, y: 0, w: 60, h: 40 }, "ib")
      expectRect(rect("t2"), { x: 80, y: 20, w: 20, h: 20 }, "t2")
      expectRect(rect("ib2"), { x: 100, y: 0, w: 50, h: 40 }, "ib2")
    } finally {
      unmount()
    }
  })

  it("is aligned by vertical-align", () => {
    // <div id=root style="width:400px;font:20px/20px GpuixTestSquare"><span id=m style="display:inline-block;width:1px;height:0"></span><span id=t0>AA</span>
    //   <span id=kN style="display:inline-block;width:10px;height:10px;vertical-align:..."></span>...
    // k1 is top, k2 bottom, k3 middle, k4 text-top, k5 text-bottom, k6 sub, k7 super, k8 5px, k9 50%.
    // Chrome has no x-height for GpuixTestSquare (it has no "x") and puts k3 at 8.344; gpui's is another
    // one, so middle is checked against Menlo below.
    const { render, ref, rect, unmount } = setup()
    try {
      const aligns: [string, string | number, number, number][] = [
        ["k1", "top", 41, 0],
        ["k2", "bottom", 51, 15],
        ["k3", "middle", 61, 8.344],
        ["k4", "text-top", 71, 0],
        ["k5", "text-bottom", 81, 10],
        ["k6", "sub", 91, 15],
        ["k7", "super", 101, 2.344],
        ["k8", 5, 111, 5],
        ["k9", "50%", 121, 0],
      ]
      render(
        <div ref={ref("root")} style={{ width: 400, ...TEXT }}>
          <span ref={ref("m")} style={box(1, 0)} />
          <span ref={ref("t0")}>AA</span>
          {aligns.map(([name, align]) => (
            <span key={name} ref={ref(name)} style={{ ...box(10, 10), verticalAlign: align }} />
          ))}
        </div>
      )
      expectRect(rect("root"), { x: 0, y: 0, w: 400, h: 25 }, "root")
      expectRect(rect("m"), { x: 0, y: 20, w: 1, h: 0 }, "m")
      for (const [name, align, x, y] of aligns) {
        // The y of middle depends on an x-height this font has none of.
        const actual = rect(name)
        expectRect(align === "middle" ? { ...actual, y } : actual, { x, y, w: 10, h: 10 }, `${name} ${align}`)
      }
    } finally {
      unmount()
    }
  })

  it("middle is half the x-height above the baseline, and a percentage is of the box's own line height", () => {
    // Menlo: <div id=root style="width:400px;font:20px/20px Menlo"> ... vertical-align: middle | sub | super
    // Chrome: k3 y=7.188, k6 y=12.656, k7 y=0 (Menlo's metrics are rounded to whole pixels there).
    // GpuixTestSquare: a box of line-height 40 and vertical-align 50% is raised by 20, and one inside a
    // span of line-height 60 by 30:
    // <div id=root style="width:300px;font:20px/20px GpuixTestSquare"><span id=t1>A</span>
    //   <span id=ib style="display:inline-block;width:20px;height:20px;vertical-align:50%;line-height:40px"></span><span id=t2>A</span>
    //   <span id=sp style="line-height:60px"><span id=tx>A</span><span id=ib3 style="display:inline-block;width:20px;height:10px;vertical-align:50%"></span></span></div>
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ width: 400, fontFamily: "Menlo", fontSize: 20, lineHeight: "20px" }}>
          <span ref={ref("m")} style={box(1, 0)} />
          <span ref={ref("t0")}>AA</span>
          <span ref={ref("k3")} style={{ ...box(10, 10), verticalAlign: "middle" }} />
        </div>
      )
      expect(Math.abs(rect("k3").y - 7.188), "Menlo has its metrics rounded to whole pixels in Chrome").toBeLessThanOrEqual(1.5)
    } finally {
      unmount()
    }

    const second = setup()
    try {
      second.render(
        <div ref={second.ref("root")} style={{ width: 300, ...TEXT }}>
          <span ref={second.ref("t1")}>A</span>
          <span ref={second.ref("ib")} style={{ ...box(20, 20), verticalAlign: "50%", lineHeight: "40px" }} />
          <span ref={second.ref("t2")}>A</span>
          <span ref={second.ref("sp")} style={{ lineHeight: "60px" }}>
            <span ref={second.ref("tx")}>A</span>
            <span ref={second.ref("ib3")} style={{ ...box(20, 10), verticalAlign: "50%" }} />
          </span>
        </div>
      )
      expectRect(second.rect("root"), { x: 0, y: 0, w: 300, h: 60 }, "root")
      expectRect(second.rect("t1"), { x: 0, y: 20, w: 20, h: 20 }, "t1")
      expectRect(second.rect("ib"), { x: 20, y: 0, w: 20, h: 20 }, "ib")
      expectRect(second.rect("tx"), { x: 60, y: 20, w: 20, h: 20 }, "tx")
      expectRect(second.rect("ib3"), { x: 80, y: 0, w: 20, h: 10 }, "ib3")
    } finally {
      second.unmount()
    }
  })
})

describeNative("inline-flex and inline-grid", () => {
  it("the baseline is the first item's, or the bottom of the box", () => {
    // <div id=root style="width:300px;font:20px/20px GpuixTestSquare"><span id=t1>AA</span>
    //   <span id=f style="display:inline-flex"><span id=f1 style="width:30px;height:10px"></span><span id=f2 style="width:20px;height:15px"></span></span>
    //   <span id=t2>AA</span><span id=g style="display:inline-grid"><span id=g1 style="width:12px;height:8px"></span><span id=g2 style="width:20px;height:15px"></span></span><span id=t3>A</span></div>
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ width: 300, ...TEXT }}>
          <span ref={ref("t1")}>AA</span>
          <span ref={ref("f")} style={{ display: "inline-flex" }}>
            <span ref={ref("f1")} style={{ width: 30, height: 10 }} />
            <span ref={ref("f2")} style={{ width: 20, height: 15 }} />
          </span>
          <span ref={ref("t2")}>AA</span>
          <span ref={ref("g")} style={{ display: "inline-grid" }}>
            <span ref={ref("g1")} style={{ width: 12, height: 8 }} />
            <span ref={ref("g2")} style={{ width: 20, height: 15 }} />
          </span>
          <span ref={ref("t3")}>A</span>
        </div>
      )
      expectRect(rect("root"), { x: 0, y: 0, w: 300, h: 35 }, "root")
      expectRect(rect("t1"), { x: 0, y: 0, w: 40, h: 20 }, "t1")
      expectRect(rect("f"), { x: 40, y: 10, w: 50, h: 15 }, "f")
      expectRect(rect("f1"), { x: 40, y: 10, w: 30, h: 10 }, "f1")
      expectRect(rect("f2"), { x: 70, y: 10, w: 20, h: 15 }, "f2")
      expectRect(rect("t2"), { x: 90, y: 0, w: 40, h: 20 }, "t2")
      expectRect(rect("g"), { x: 130, y: 12, w: 20, h: 23 }, "g")
      expectRect(rect("g1"), { x: 130, y: 12, w: 12, h: 8 }, "g1")
      expectRect(rect("g2"), { x: 130, y: 20, w: 20, h: 15 }, "g2")
      expectRect(rect("t3"), { x: 150, y: 0, w: 20, h: 20 }, "t3")
    } finally {
      unmount()
    }
  })

  it("the baseline of a first item with text is the baseline of its first line", () => {
    // <div id=root style="width:300px;font:20px/20px GpuixTestSquare"><span id=t1>AA</span>
    //   <span id=f style="display:inline-flex"><span id=f1 style="font-size:10px;line-height:10px;display:block">AB</span><span id=f2 style="width:20px;height:15px"></span></span>
    //   <span id=t2>A</span><span id=g style="display:inline-grid"><span id=g1 style="display:block;font-size:30px;line-height:40px">A</span></span></div>
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ width: 300, ...TEXT }}>
          <span ref={ref("t1")}>AA</span>
          <span ref={ref("f")} style={{ display: "inline-flex" }}>
            <span ref={ref("f1")} style={{ display: "block", fontSize: 10, lineHeight: "10px" }}>
              AB
            </span>
            <span ref={ref("f2")} style={{ width: 20, height: 15 }} />
          </span>
          <span ref={ref("t2")}>A</span>
          <span ref={ref("g")} style={{ display: "inline-grid" }}>
            <span ref={ref("g1")} style={{ display: "block", fontSize: 30, lineHeight: "40px" }}>
              A
            </span>
          </span>
        </div>
      )
      expectRect(rect("root"), { x: 0, y: 0, w: 300, h: 40 }, "root")
      expectRect(rect("t1"), { x: 0, y: 15, w: 40, h: 20 }, "t1")
      expectRect(rect("f"), { x: 40, y: 25, w: 50, h: 15 }, "f")
      expectRect(rect("f1"), { x: 40, y: 25, w: 30, h: 15 }, "f1")
      expectRect(rect("t2"), { x: 90, y: 15, w: 20, h: 20 }, "t2")
      expectRect(rect("g"), { x: 110, y: 0, w: 30, h: 40 }, "g")
    } finally {
      unmount()
    }
  })

  it("an empty one, with padding, border and margin, has its bottom margin edge on the baseline", () => {
    // <div id=root style="width:300px;font:20px/20px GpuixTestSquare"><span id=t1>A</span>
    //   <span id=e style="display:inline-flex;width:10px;height:10px;margin-bottom:6px"></span><span id=t2>A</span>
    //   <span id=eg style="display:inline-grid;box-sizing:border-box;width:10px;height:10px;margin:3px 0 6px 0;border:2px solid;padding:1px"></span></div>
    // (the oracle page had a third box and a text between them; eg is at x=80 there and x=50 here.)
    // (gpuix boxes are border-box, so the HTML says so.)
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ width: 300, ...TEXT }}>
          <span ref={ref("t1")}>A</span>
          <span ref={ref("e")} style={{ display: "inline-flex", width: 10, height: 10, marginBottom: 6 }} />
          <span ref={ref("t2")}>A</span>
          <span
            ref={ref("eg")}
            style={{
              display: "inline-grid",
              width: 10,
              height: 10,
              marginTop: 3,
              marginBottom: 6,
              border: "2px solid black",
              padding: 1,
            }}
          />
        </div>
      )
      expectRect(rect("root"), { x: 0, y: 0, w: 300, h: 20 }, "root")
      expectRect(rect("t1"), { x: 0, y: 0, w: 20, h: 20 }, "t1")
      expectRect(rect("e"), { x: 20, y: 4, w: 10, h: 10 }, "e")
      expectRect(rect("eg"), { x: 50, y: 4, w: 10, h: 10 }, "eg")
    } finally {
      unmount()
    }
  })
})

describeNative("the baseline of an inline-block", () => {
  it("is that of the last line of its text, and the bottom edge when overflow clips it", () => {
    // <div id=root style="width:300px;font:20px/20px GpuixTestSquare"><span id=t1>A</span>
    //   <span id=hid style="display:inline-block;overflow:hidden;width:40px;height:40px"><span>AA</span></span><span id=t2>A</span>
    //   <span id=clip style="display:inline-block;overflow:clip;width:40px;height:40px"><span>AA</span></span><span id=t3>A</span>
    //   <span id=vis style="display:inline-block;width:40px;height:40px"><span>AA</span></span></div>
    const { render, ref, rect, unmount } = setup()
    try {
      const sized = { ...box(40, 40) }
      render(
        <div ref={ref("root")} style={{ width: 300, ...TEXT }}>
          <span ref={ref("t1")}>A</span>
          <span ref={ref("hid")} style={{ ...sized, overflow: "hidden" }}>
            <span>AA</span>
          </span>
          <span ref={ref("t2")}>A</span>
          <span ref={ref("clip")} style={{ ...sized, overflow: "clip" }}>
            <span>AA</span>
          </span>
          <span ref={ref("t3")}>A</span>
          <span ref={ref("vis")} style={sized}>
            <span>AA</span>
          </span>
        </div>
      )
      expectRect(rect("root"), { x: 0, y: 0, w: 300, h: 60 }, "root")
      expectRect(rect("t1"), { x: 0, y: 20, w: 20, h: 20 }, "t1")
      expectRect(rect("hid"), { x: 20, y: 0, w: 40, h: 40 }, "hid")
      expectRect(rect("clip"), { x: 80, y: 20, w: 40, h: 40 }, "clip")
      expectRect(rect("vis"), { x: 140, y: 20, w: 40, h: 40 }, "vis")
    } finally {
      unmount()
    }
  })

  it("is the last line, with padding, border and margin round a wrapped text", () => {
    // <div id=root style="width:300px;font:20px/20px GpuixTestSquare"><span id=t1>A</span>
    //   <span id=w style="display:inline-block;box-sizing:border-box;width:50px;padding:3px 2px;border:1px solid;margin:2px"><span id=w1>AAAA AAAA AA</span></span><span id=t2>A</span></div>
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ width: 300, ...TEXT }}>
          <span ref={ref("t1")}>A</span>
          <span
            ref={ref("w")}
            style={{
              display: "inline-block",
              width: 50,
              paddingTop: 3,
              paddingBottom: 3,
              paddingLeft: 2,
              paddingRight: 2,
              border: "1px solid black",
              margin: 2,
            }}
          >
            <span ref={ref("w1")}>AAAA AAAA AA</span>
          </span>
          <span ref={ref("t2")}>A</span>
        </div>
      )
      expectRect(rect("root"), { x: 0, y: 0, w: 300, h: 72 }, "root")
      expectRect(rect("t1"), { x: 0, y: 46, w: 20, h: 20 }, "t1")
      expectRect(rect("w"), { x: 22, y: 2, w: 50, h: 68 }, "w")
      expectRect(rect("t2"), { x: 74, y: 46, w: 20, h: 20 }, "t2")
    } finally {
      unmount()
    }
  })

  it("has no baseline in an absolutely positioned descendant", () => {
    // <div id=root style="width:300px;font:20px/20px GpuixTestSquare"><span id=t1>A</span>
    //   <div id=blk style="display:inline-block;width:60px"><div id=p1 style="height:10px"></div><div id=p2 style="width:20px;height:10px;font-size:10px;line-height:10px">A</div></div><span id=t2>A</span>
    //   <div id=blk2 style="display:inline-block"><div id=q1 style="height:10px;width:30px"></div><div id=q2 style="width:20px;position:absolute;font-size:10px;line-height:10px">A</div></div></div>
    const { render, ref, rect, unmount } = setup()
    try {
      const small = { fontSize: 10, lineHeight: "10px" }
      render(
        <div ref={ref("root")} style={{ width: 300, ...TEXT }}>
          <span ref={ref("t1")}>A</span>
          <div ref={ref("blk")} style={{ display: "inline-block", width: 60 }}>
            <div style={{ height: 10 }} />
            <div style={{ width: 20, height: 10, ...small }}>A</div>
          </div>
          <span ref={ref("t2")}>A</span>
          <div ref={ref("blk2")} style={{ display: "inline-block" }}>
            <div style={{ height: 10, width: 30 }} />
            <div style={{ width: 20, position: "absolute", ...small }}>A</div>
          </div>
        </div>
      )
      expectRect(rect("blk"), { x: 20, y: 0, w: 60, h: 20 }, "blk")
      expectRect(rect("blk2"), { x: 100, y: 10, w: 30, h: 10 }, "blk2")
    } finally {
      unmount()
    }
  })
})

describeNative("an inline-block that is positioned", () => {
  it("is moved by its relative offsets without moving the text", () => {
    // <div id=root style="width:300px;font:20px/20px GpuixTestSquare"><span id=t1>A</span>
    //   <span id=rel style="display:inline-block;position:relative;top:5px;left:7px;width:20px;height:10px"></span><span id=t2>A</span></div>
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ width: 300, ...TEXT }}>
          <span ref={ref("t1")}>A</span>
          <span ref={ref("rel")} style={{ ...box(20, 10), position: "relative", top: 5, left: 7 }} />
          <span ref={ref("t2")}>A</span>
        </div>
      )
      expectRect(rect("rel"), { x: 27, y: 15, w: 20, h: 10 }, "rel")
      expectRect(rect("t2"), { x: 40, y: 0, w: 20, h: 20 }, "t2")
    } finally {
      unmount()
    }
  })
})

describeNative("a span's vertical-align", () => {
  it("raises its text and a box in it by the same amount", () => {
    // <div id=root style="width:300px;font:20px/20px GpuixTestSquare"><span id=t1>A</span>
    //   <span id=sup style="vertical-align:super;font-size:10px;line-height:10px"><span id=t2>A</span><span id=ib style="display:inline-block;width:10px;height:10px"></span></span><span id=t3>A</span></div>
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ width: 300, ...TEXT }}>
          <span ref={ref("t1")}>A</span>
          <span ref={ref("sup")} style={{ verticalAlign: "super", fontSize: 10, lineHeight: "10px" }}>
            <span ref={ref("t2")}>A</span>
            <span ref={ref("ib")} style={box(10, 10)} />
          </span>
          <span ref={ref("t3")}>A</span>
        </div>
      )
      expectRect(rect("root"), { x: 0, y: 0, w: 300, h: 20 }, "root")
      expectRect(rect("sup"), { x: 20, y: 2.344, w: 20, h: 10 }, "sup")
      expectRect(rect("t2"), { x: 20, y: 2.344, w: 10, h: 10 }, "t2")
      expectRect(rect("ib"), { x: 30, y: 2.344, w: 10, h: 10 }, "ib")
      expectRect(rect("t3"), { x: 40, y: 0, w: 20, h: 20 }, "t3")
    } finally {
      unmount()
    }
  })
})

describeNative("blockification", () => {
  it("makes a flex or grid item of an inline-* box a block, a flex container or a grid container", () => {
    // <div id=root style="display:flex;width:200px;font:20px/20px GpuixTestSquare"><div id=a style="display:inline-block;width:30px;height:10px"></div>
    //   <div id=b style="display:inline-flex;padding:2px"><span id=b1 style="width:10px;height:10px"></span><span id=b2 style="width:10px;height:10px"></span></div>
    //   <div id=c style="display:inline-grid"><span id=c1 style="width:10px;height:10px"></span><span id=c2 style="width:10px;height:10px"></span></div></div>
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ display: "flex", width: 200, ...TEXT }}>
          <div ref={ref("a")} style={box(30, 10)} />
          <div ref={ref("b")} style={{ display: "inline-flex", padding: 2 }}>
            <span ref={ref("b1")} style={{ width: 10, height: 10 }} />
            <span ref={ref("b2")} style={{ width: 10, height: 10 }} />
          </div>
          <div ref={ref("c")} style={{ display: "inline-grid" }}>
            <span ref={ref("c1")} style={{ width: 10, height: 10 }} />
            <span ref={ref("c2")} style={{ width: 10, height: 10 }} />
          </div>
        </div>
      )
      expectRect(rect("root"), { x: 0, y: 0, w: 200, h: 20 }, "root")
      expectRect(rect("a"), { x: 0, y: 0, w: 30, h: 10 }, "a")
      expectRect(rect("b"), { x: 30, y: 0, w: 24, h: 20 }, "b")
      expectRect(rect("b1"), { x: 32, y: 2, w: 10, h: 10 }, "b1")
      expectRect(rect("b2"), { x: 42, y: 2, w: 10, h: 10 }, "b2")
      expectRect(rect("c"), { x: 54, y: 0, w: 10, h: 20 }, "c")
      expectRect(rect("c2"), { x: 54, y: 10, w: 10, h: 10 }, "c2")
    } finally {
      unmount()
    }
  })

  it("makes an absolutely positioned inline-block a block that shrinks to fit", () => {
    // <div id=root style="position:relative;width:200px;height:100px;font:20px/20px GpuixTestSquare"><span id=abs style="display:inline-block;position:absolute;left:5px;top:6px"><span id=abs1 style="display:block;width:30px;height:10px"></span></span></div>
    const { render, ref, rect, unmount } = setup()
    try {
      render(
        <div ref={ref("root")} style={{ position: "relative", width: 200, height: 100, ...TEXT }}>
          <span ref={ref("abs")} style={{ display: "inline-block", position: "absolute", left: 5, top: 6 }}>
            <span ref={ref("abs1")} style={{ display: "block", width: 30, height: 10 }} />
          </span>
        </div>
      )
      expectRect(rect("abs"), { x: 5, y: 6, w: 30, h: 10 }, "abs")
    } finally {
      unmount()
    }
  })
})
