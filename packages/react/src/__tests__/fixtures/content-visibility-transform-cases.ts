/// The cases of `content-visibility-transform-oracle.html` and of `content-visibility-transform.test.tsx`: `content-visibility:
/// auto` boxes under, and with, a transform. The page is 900 x 780, so a box is near while its border box, as the viewport sees it
/// (transforms applied), reaches within 1.5 viewports of it: left < 2250, right > -1350, top < 1950, bottom > -1170. A clip around
/// the box (what `overflow: hidden` clips, also as the viewport sees it) has to meet the viewport too.
export type Style = Record<string, string | number>
export interface Node {
  /** An `auto` box: skipped or not is what a case reads, by this name. */
  name?: string
  style: Style
  children?: Node[]
}
export interface Case {
  name: string
  nodes: Node[]
}

export const PAGE = { width: 900, height: 780 }

const at = (left: number, top: number, width: number, height: number): Style => ({
  position: "absolute",
  left: `${left}px`,
  top: `${top}px`,
  width: `${width}px`,
  height: `${height}px`,
})
/** A 100 x 100 `auto` box whose layout box starts at `left`, `top`. */
const box = (name: string, left: number, top: number, style: Style = {}): Node => ({
  name,
  style: { ...at(left, top, 100, 100), contentVisibility: "auto", ...style },
})
/** A box that only carries a transform around what is inside it, laid out at the page's origin. */
const holder = (transform: string, children: Node[], style: Style = {}): Node => ({
  style: { ...at(0, 0, PAGE.width, PAGE.height), transform, ...style },
  children,
})

// Just inside and just outside the margin, on each side, for a box of 100 x 100.
const EDGES: [string, number, number][] = [
  ["right-in", 2248, 100],
  ["right-out", 2252, 100],
  ["left-in", -1448, 100],
  ["left-out", -1452, 100],
  ["bottom-in", 100, 1948],
  ["bottom-out", 100, 1952],
  ["top-in", 100, -1268],
  ["top-out", 100, -1272],
]

export const CASES: Case[] = [
  {
    name: "plain-edges",
    nodes: EDGES.map(([name, left, top]) => box(name, left, top)),
  },
  {
    name: "ancestor-translate-brings-in",
    nodes: [
      holder("translate(0px, -3000px)", [box("y-far-in", 100, 3100)]),
      holder("translate(-2900px, 0px)", [box("x-far-in", 3000, 100)]),
      holder("translate(0px, -1500px)", [box("y-edge-in", 100, 3448)]),
      holder("translate(0px, -1500px)", [box("y-edge-out", 100, 3452)]),
    ],
  },
  {
    name: "ancestor-translate-takes-out",
    nodes: [
      holder("translate(0px, 3000px)", [box("y-far", 100, 100)]),
      holder("translate(3000px, 0px)", [box("x-far", 100, 100)]),
      holder("translate(0px, -3000px)", [box("up-far", 100, 100)]),
      holder("translate(-3000px, 0px)", [box("left-far", 100, 100)]),
      holder("translate(2148px, 0px)", [box("right-in", 100, 100)]),
      holder("translate(2152px, 0px)", [box("right-out", 100, 100)]),
      holder("translate(-1548px, 0px)", [box("left-in", 100, 100)]),
      holder("translate(-1552px, 0px)", [box("left-out", 100, 100)]),
      holder("translate(0px, 1848px)", [box("bottom-in", 100, 100)]),
      holder("translate(0px, 1852px)", [box("bottom-out", 100, 100)]),
      holder("translate(0px, -1368px)", [box("top-in", 100, 100)]),
      holder("translate(0px, -1372px)", [box("top-out", 100, 100)]),
    ],
  },
  {
    name: "own-translate",
    nodes: [
      box("brings-in", 100, 3100, { transform: "translate(0px, -3000px)" }),
      box("brings-in-x", 3100, 100, { transform: "translate(-3000px, 0px)" }),
      box("takes-out", 100, 100, { transform: "translate(0px, 3000px)" }),
      box("takes-out-x", 100, 100, { transform: "translate(-3000px, 0px)" }),
      box("right-in", 100, 100, { transform: "translate(2148px, 0px)" }),
      box("right-out", 100, 100, { transform: "translate(2152px, 0px)" }),
      box("bottom-in", 100, 100, { transform: "translate(0px, 1848px)" }),
      box("bottom-out", 100, 100, { transform: "translate(0px, 1852px)" }),
      box("left-in", 100, 100, { transform: "translate(-1548px, 0px)" }),
      box("left-out", 100, 100, { transform: "translate(-1552px, 0px)" }),
    ],
  },
  {
    name: "ancestor-scale-down",
    nodes: [
      holder("scale(0.01)", [box("far-down", 100, 30000)], { transformOrigin: "0px 0px" }),
      holder("scale(0.01)", [box("far-right", 50000, 100)], { transformOrigin: "0px 0px" }),
      holder("scale(0.001)", [box("far-centre", 100, 100000)]),
      // The origin is far away and the scale pulls the box to it.
      holder("scale(0.01)", [box("pulled-away", 100, 100)], { transformOrigin: "5000px 5000px" }),
      holder("scale(0.5)", [box("scale-half-in", 100, 3000)], { transformOrigin: "0px 0px" }),
      holder("scale(0.5)", [box("scale-half-out", 100, 4000)], { transformOrigin: "0px 0px" }),
    ],
  },
  {
    name: "scale-up-about-the-centre",
    nodes: [
      // Centred on the box, which is far to the right in layout and fills the page once scaled.
      box("grows-in", 3000, 100, { transform: "scale(40)" }),
      box("grows-in-y", 100, 3000, { transform: "scale(40)" }),
      box("grows-not-enough", 3000, 100, { transform: "scale(5)" }),
      holder("scale(40)", [box("ancestor-grows-in", 3000, 100)], { transformOrigin: "3050px 150px" }),
      holder("scale(40)", [box("ancestor-grows-not-enough", 100, 100)], { transformOrigin: "5000px 5000px" }),
    ],
  },
  {
    name: "rotate-90",
    nodes: [
      // x' = -y, y' = x about the holder's origin: layout y of -2200 is x' of 2200.
      holder("rotate(90deg)", [box("rotates-in", 100, -2200)], { transformOrigin: "0px 0px" }),
      holder("rotate(90deg)", [box("rotates-out", 100, 1500)], { transformOrigin: "0px 0px" }),
      holder("rotate(-90deg)", [box("rotates-in-left", 100, 2200)], { transformOrigin: "0px 0px" }),
      // A wide box: laid out across the page and far past it once it stands on end.
      holder("rotate(90deg)", [box("wide-stands-out", 100, 100, { width: "3000px", height: "40px" })], { transformOrigin: "5000px 5000px" }),
    ],
  },
  {
    name: "rotate-45",
    nodes: [
      // A wide box whose layout is far below the page and which a turn about its left end lifts into it.
      holder("rotate(-45deg)", [box("wide-lifted", 0, 3000, { width: "2400px", height: "40px" })], { transformOrigin: "0px 3020px" }),
      holder("rotate(45deg)", [box("rotated-away", 100, 100)], { transformOrigin: "5000px 0px" }),
      box("own-rotate", 3000, 100, { width: "3000px", height: "40px", transformOrigin: "0px 20px", transform: "rotate(180deg)" }),
    ],
  },
  {
    name: "clip-around-the-box",
    nodes: [
      // The clip is in the viewport once it is scaled and nowhere near it in layout.
      {
        style: { ...at(2000, 2000, 300, 300), overflow: "hidden", transform: "scale(0.1)", transformOrigin: "0px 0px" },
        children: [box("clip-scaled-in", 100, 100)],
      },
      // The clip is moved out of the viewport by a rotation, and it is inside it in layout.
      {
        style: { ...at(100, 100, 300, 300), overflow: "hidden", transform: "translate(0px, 1500px) rotate(10deg)" },
        children: [box("clip-rotated-out", 100, 100)],
      },
      // The clip is translated out of the viewport.
      {
        style: { ...at(100, 100, 300, 300), overflow: "hidden", transform: "translate(0px, 1000px)" },
        children: [box("clip-translated-out", 0, 0)],
      },
      // A clip that stays in the viewport and a box inside it.
      {
        style: { ...at(2000, 100, 300, 300), overflow: "hidden", transform: "translate(-1700px, 0px) rotate(20deg)" },
        children: [box("clip-rotated-in", 100, 100)],
      },
      // An ancestor transform around a clip that does not move.
      holder("translate(0px, 2000px)", [
        {
          style: { ...at(100, 100, 300, 300), overflow: "hidden" },
          children: [box("translated-clip-out", 0, 0)],
        },
      ]),
      holder("translate(0px, -2000px)", [
        {
          style: { ...at(100, 2100, 300, 300), overflow: "hidden" },
          children: [box("translated-clip-in", 0, 0)],
        },
      ]),
    ],
  },
  {
    name: "nested",
    nodes: [
      // translate, then scale: the scale about the inner holder's origin (0, 0) halves the offset from it.
      holder("translate(-1000px, -1000px)", [
        { style: { ...at(0, 0, PAGE.width, PAGE.height), transform: "scale(0.5)", transformOrigin: "0px 0px" }, children: [box("translate-scale-in", 2000, 2000)] },
      ]),
      holder("translate(2500px, 0px)", [
        { style: { ...at(0, 0, PAGE.width, PAGE.height), transform: "scale(2)", transformOrigin: "0px 0px" }, children: [box("translate-scale-out", 100, 100)] },
      ]),
      holder("scale(0.1)", [
        { style: { ...at(0, 0, PAGE.width, PAGE.height), transform: "translate(0px, 8000px)" }, children: [box("scale-translate-in", 100, 100)] },
      ], { transformOrigin: "0px 0px" }),
      holder("scale(0.1)", [
        { style: { ...at(0, 0, PAGE.width, PAGE.height), transform: "translate(0px, 30000px)" }, children: [box("scale-translate-out", 100, 100)] },
      ], { transformOrigin: "0px 0px" }),
      holder("translate(0px, -3000px)", [
        { style: { ...at(0, 0, PAGE.width, PAGE.height), transform: "translate(0px, 3000px)" }, children: [box("cancels", 100, 100)] },
      ]),
      holder("translate(0px, -3000px)", [
        { style: { ...at(0, 0, PAGE.width, PAGE.height), transform: "translate(0px, -2000px)" }, children: [box("cancels-not", 100, 100)] },
      ]),
    ],
  },
]
