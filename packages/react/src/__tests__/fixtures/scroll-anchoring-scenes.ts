// The scenes of scroll-anchoring-oracle.html (run in Chrome by scroll-anchoring-oracle.ts) and of
// scroll-anchoring.test.tsx (run in gpuix): one description, so the two cannot drift apart.
//
// A scene is a scroller of 300px by 300px, `overflow: auto`, holding `rows` rows that are `r0`, `r1`... in
// order and `rowHeight` tall, scrolled to `scrollTo`. A step is a task: its ops run one after the other
// without a frame between, then frames are drawn and the scene is read. Lengths are strings, since the two
// sides spell a number differently.

export type Style = Record<string, string>

export type Op =
  /** A row of `height` before `before`, at the end when that is null. */
  | { op: "insert"; id: string; before: string | null; height: string; style?: Style }
  | { op: "remove"; id: string }
  /** A style on a row, or on `scroller` or `wrapper`. A key set to "" is removed. */
  | { op: "style"; id: string; style: Style }
  /** `scrollTop = y`; "max" is as far as the scroller goes. */
  | { op: "scrollTo"; y: number | "max" }
  | { op: "scrollBy"; dy: number }

export type Step = {
  name: string
  ops: Op[]
  /** Read `scrollTop` and the position of the first row in view right after the ops, before any frame. */
  forced?: boolean
}

export type SceneDef = {
  rows: number
  rowHeight: string
  /** The row is an empty box of `rowHeight`, or a box whose height is a child's of `rowHeight`. */
  child?: boolean
  rowStyle?: Style
  rowStyles?: Record<string, Style>
  scrollerStyle?: Style
  /** The rows are in a box of this style, which is in the scroller. */
  wrapper?: Style
  scrollTo: number | "max"
  steps: Step[]
}

const insert = (id: string, before: string | null, height = "100px", style?: Style): Op => ({ op: "insert", id, before, height, style })
const style = (id: string, value: Style): Op => ({ op: "style", id, style: value })
const step = (name: string, ...ops: Op[]): Step => ({ name, ops })
const forced = (name: string, ...ops: Op[]): Step => ({ name, ops, forced: true })

const plain = (steps: Step[], extra: Partial<SceneDef> = {}): SceneDef => ({ rows: 20, rowHeight: "50px", scrollTo: 400, steps, ...extra })

const entries = (count: number, dy: number): Step[] => Array.from({ length: count }, (_, index) => step(`up ${index + 1}`, { op: "scrollBy", dy }))

export const SCENES: Record<string, SceneDef> = {
  // A row inserted or removed above the first row in view.
  insertAbove: plain([
    step("insert 100px at the top", insert("n0", "r0")),
    step("insert another at the top", insert("n1", "r0")),
    step("remove the first", { op: "remove", id: "n0" }),
    step("insert just above the first row in view", insert("n2", "r8", "30px")),
  ]),
  // The first row in view is cut by the top edge.
  partial: plain(
    [
      step("insert above", insert("n0", "r0")),
      step("insert between the anchor and the next row", insert("n1", "r9")),
      step("insert at the top, 25px", insert("n2", "r0", "25px")),
    ],
    { scrollTo: 420 }
  ),
  // Below the anchor nothing moves it.
  insertBelow: plain([
    step("insert in the viewport, below the anchor", insert("n0", "r10")),
    step("insert at the end", insert("n1", null)),
    step("insert after the viewport", insert("n2", "r15")),
    step("remove a row below", { op: "remove", id: "r12" }),
  ]),
  // The anchor is the second row in view, so an insert between the rows in view is above it.
  insertVisibleAbove: plain(
    [
      step("insert between the first and the second row in view", insert("n0", "r9")),
      step("insert at the top", insert("n1", "r0")),
    ],
    { rowStyles: { r8: { overflowAnchor: "none" } } }
  ),
  resizeAbove: plain([
    step("grow a row above, 50 to 150", style("r2", { height: "150px" })),
    step("shrink it, 150 to 80", style("r2", { height: "80px" })),
    step("shrink another to 0", style("r3", { height: "0px" })),
    step("display none on another", style("r4", { display: "none" })),
    step("display back", style("r4", { display: "" })),
    step("grow a row below", style("r12", { height: "200px" })),
  ]),
  // The compared positions are rounded: 50.4 to 50.6 is a pixel, 50 to 50.4 is none.
  fractional: plain([
    step("50 to 50.4", style("r2", { height: "50.4px" })),
    step("50.4 to 50.6", style("r2", { height: "50.6px" })),
    step("50.6 to 51.4", style("r2", { height: "51.4px" })),
    step("51.4 to 50", style("r2", { height: "50px" })),
  ]),
  // A restyle of the anchor, or of what holds it, that moves it is the page's move.
  anchorHeight: plain([
    step("anchor taller", style("r8", { height: "80px" })),
    step("anchor back, with a row inserted above", style("r8", { height: "50px" }), insert("n0", "r0")),
    step("insert above again", insert("n1", "r0")),
  ]),
  anchorMargin: plain([
    step("margin-top on the anchor", style("r8", { marginTop: "30px" })),
    step("margin-top back", style("r8", { marginTop: "" })),
    step("insert above", insert("n0", "r0")),
    step("margin-top 10 and a row inserted above in one task", style("r8", { marginTop: "10px" }), insert("n1", "r0", "40px")),
  ]),
  anchorTop: plain([
    step("position relative and top", style("r8", { position: "relative", top: "20px" })),
    step("top 40", style("r8", { top: "40px" })),
  ]),
  anchorTopThenInsert: plain([
    step("position relative and top", style("r8", { position: "relative", top: "20px" })),
    step("insert above", insert("n0", "r0")),
  ]),
  // A second change of `top` leaves Chrome without compensation for the next layout too, as if the flag of the box stayed
  // set until it is laid out, which a relative box does not need to be. Not followed.
  anchorTopStale: plain([
    step("position relative and top", style("r8", { position: "relative", top: "20px" })),
    step("top 40", style("r8", { top: "40px" })),
    step("insert above", insert("n0", "r0")),
  ]),
  parentPadding: plain(
    [
      step("padding-top on the parent of the anchor", style("wrapper", { paddingTop: "40px" })),
      step("padding back", style("wrapper", { paddingTop: "" })),
      step("insert above", insert("n0", "r0")),
      step("padding-bottom on the parent", style("wrapper", { paddingBottom: "60px" })),
    ],
    { wrapper: {} }
  ),
  scrollerPadding: plain([
    step("padding-top on the scroller", style("scroller", { paddingTop: "40px" })),
    step("insert above", insert("n0", "r0")),
    step("padding back", style("scroller", { paddingTop: "" })),
  ]),
  parentHeight: plain(
    [
      step("the parent taller", style("wrapper", { height: "1500px" })),
      step("insert above", insert("n0", "r0")),
    ],
    { wrapper: {} }
  ),
  scrollerNone: plain(
    [
      step("insert at the top", insert("n0", "r0")),
      step("grow a row above", style("r2", { height: "150px" })),
      step("anchoring back on, a row inserted in the same task", style("scroller", { overflowAnchor: "auto" }), insert("n1", "r0")),
      step("insert at the top", insert("n2", "r0")),
      step("anchoring off, a row inserted in the same task", style("scroller", { overflowAnchor: "none" }), insert("n3", "r0")),
    ],
    { scrollerStyle: { overflowAnchor: "none" } }
  ),
  noneOnFirst: plain(
    [
      step("insert at the top", insert("n0", "r0")),
      step("grow a row above", style("r2", { height: "150px" })),
      step("the first row anchors again, a row inserted in the same task", style("r8", { overflowAnchor: "auto" }), insert("n1", "r0")),
    ],
    { rowStyles: { r8: { overflowAnchor: "none" } } }
  ),
  noneTurnedOn: plain([
    step("none on the first row in view, a row inserted in the same task", style("r8", { overflowAnchor: "none" }), insert("n0", "r0")),
    step("insert at the top", insert("n1", "r0")),
    step("none on the new anchor, a row inserted in the same task", style("r9", { overflowAnchor: "none" }), insert("n2", "r0")),
  ]),
  // None on the parent of the first rows in view excludes them whole.
  noneOnParent: plain(
    [step("insert at the top", insert("n0", "r0")), step("grow a row above", style("r2", { height: "150px" }))],
    { wrapper: { overflowAnchor: "none" } }
  ),
  atStart: plain(
    [
      step("insert at the top", insert("n0", "r0")),
      step("scroll to 100", { op: "scrollTo", y: 100 }),
      step("insert at the top", insert("n1", "r0")),
      step("scroll to the start", { op: "scrollTo", y: 0 }),
      step("insert at the top", insert("n2", "r0")),
    ],
    { scrollTo: 0 }
  ),
  writeAfterMutation: plain([
    step("insert, then scrollTop = 300", insert("n0", "r0"), { op: "scrollTo", y: 300 }),
    step("grow above, then scrollTop = 10", style("r2", { height: "150px" }), { op: "scrollTo", y: 10 }),
  ]),
  writeBeforeMutation: plain([
    step("scrollTop = 300, then insert", { op: "scrollTo", y: 300 }, insert("n0", "r0")),
    step("scrollTop = 100, then grow above", { op: "scrollTo", y: 100 }, style("r2", { height: "150px" })),
  ]),
  // A write in a task of its own, and the mutation in the next.
  writeThenMutate: plain([
    step("scrollTop = 300", { op: "scrollTo", y: 300 }),
    step("insert", insert("n0", "r0")),
  ]),
  atMax: plain([
    step("to the end", { op: "scrollTo", y: "max" }),
    step("insert at the top", insert("n0", "r0")),
    step("grow above", style("r2", { height: "150px" })),
    step("shrink above", style("r2", { height: "50px" })),
    step("remove rows at the end", { op: "remove", id: "r19" }, { op: "remove", id: "r18" }),
  ]),
  // The room is smaller than what the anchor moved.
  clamped: plain([
    step("to the end", { op: "scrollTo", y: "max" }),
    step(
      "rows below removed and one inserted above",
      ...["r15", "r16", "r17", "r18", "r19"].map((id): Op => ({ op: "remove", id })),
      insert("n0", "r0")
    ),
    step("insert at the top", insert("n1", "r0")),
  ]),
  anchorRemoved: plain([
    step("remove the anchor", { op: "remove", id: "r8" }),
    step("insert at the top", insert("n0", "r0")),
    step("remove the anchor and insert above in one task", { op: "remove", id: "r9" }, insert("n1", "r0", "30px")),
    step("remove a row above", { op: "remove", id: "r2" }),
  ]),
  // The anchor and everything in view go.
  viewRemoved: plain([
    step("remove the rows in view", ...["r8", "r9", "r10", "r11", "r12", "r13"].map((id): Op => ({ op: "remove", id }))),
    step("insert at the top", insert("n0", "r0")),
  ]),
  // The first row in view is the anchor and it goes under display none.
  anchorHidden: plain([
    step("display none on the anchor", style("r8", { display: "none" })),
    step("insert at the top", insert("n0", "r0")),
  ]),
  forcedRead: plain([
    forced("insert at the top", insert("n0", "r0")),
    forced("grow above", style("r2", { height: "150px" })),
    forced("margin on the anchor", style("r8", { marginTop: "30px" })),
    forced("margin back, insert above", style("r8", { marginTop: "" }), insert("n1", "r0")),
    forced("a row below", insert("n2", "r12")),
  ]),
  // Content that was skipped above the viewport takes its real height as it is scrolled to.
  cvList: {
    rows: 60,
    rowHeight: "120px",
    child: true,
    rowStyle: { contentVisibility: "auto", containIntrinsicSize: "auto 40px" },
    scrollTo: 0,
    steps: [step("to the end", { op: "scrollTo", y: "max" }), ...entries(16, -150)],
  },
  cvListFine: {
    rows: 60,
    rowHeight: "120px",
    child: true,
    rowStyle: { contentVisibility: "auto", containIntrinsicSize: "auto 40px" },
    scrollTo: 0,
    steps: [step("to the end", { op: "scrollTo", y: "max" }), ...entries(24, -60)],
  },
  // A list of rows of another height than the intrinsic one, scrolled from the top down and back.
  cvListDown: {
    rows: 60,
    rowHeight: "120px",
    child: true,
    rowStyle: { contentVisibility: "auto", containIntrinsicSize: "auto 40px" },
    scrollTo: 0,
    steps: [
      ...Array.from({ length: 8 }, (_, index) => step(`down ${index + 1}`, { op: "scrollBy", dy: 400 } as Op)),
      ...entries(8, -400),
    ],
  },
}
