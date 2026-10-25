/// What `ResizeObserver` and `IntersectionObserver` deliver for a target inside `content-visibility` skipped content, and
/// for the element that skips it, against headless Chrome: the content of a skipped element is not gathered, so a
/// ResizeObserver tells nothing of its targets while they are skipped (no 0 by 0 when it hides) and compares them with the
/// size it last told once they show; an IntersectionObserver is told a target inside skipped content (`hidden`, and `auto`
/// while it is skipped) is not intersecting with an empty rect and empty root bounds, once, and nothing while it stays so.
/// A read of geometry, which lays the skipped content out, changes none of this. The same page has the `overflow: auto`
/// element `#r` that is the explicit root of the IntersectionObserver scenes: its `rootBounds` (the client box, less a
/// classic scrollbar, grown by `rootMargin`) and a root that changes in the frame a target does.
///
/// `fixtures/content-visibility-observers-oracle.html` is the page, `fixtures/content-visibility-observers-oracle.ts`
/// drives it and writes `fixtures/content-visibility-observers-oracle.json` (regenerate with
/// `bun fixtures/content-visibility-observers-oracle.ts`).

import fs from "fs"
import path from "path"
import type { CSSProperties } from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import type { IntersectionObserverEntry, PublicInstance } from "../index"
import { IntersectionObserver, ResizeObserver } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const FIXTURES = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures")

type Logged =
  | { frame: number; did: string }
  | { frame: number; skipped: boolean }
  | { frame: number; ro: string; border: number[]; content: number[] }
  | {
      frame: number
      io: string
      isIntersecting: boolean
      ratio: number
      rect: number[]
      intersection: number[]
      root: number[]
      at: number[]
    }

const ORACLE = JSON.parse(fs.readFileSync(path.join(FIXTURES, "content-visibility-observers-oracle.json"), "utf8")) as Record<string, Logged[]>

const SIZE = { width: 1280, height: 800 }
const FAR = 3000
const THRESHOLDS = [0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1]

interface State {
  cv: "visible" | "hidden" | "auto"
  /** `null` leaves the element that skips where the flow puts it, below `above`. */
  top: number | null
  /** Width of the target. */
  width: number
  /** Width of the element that skips. */
  hostWidth: number
  /** Height of the empty box before the target, inside the element that skips. */
  pre: number
  /** Height of the box before the element that skips. */
  above: number
  /** The explicit root: `#r` of the page. */
  rootLeft: number
  rootWidth: number
  rootHeight: number
  /** Its two children. */
  rtWidth: number
  rtHeight: number
}

function initialState(cv: State["cv"], top: State["top"]): State {
  return {
    cv,
    top,
    width: 100,
    hostWidth: 400,
    pre: 0,
    above: 100,
    rootLeft: 500,
    rootWidth: 300,
    rootHeight: 200,
    rtWidth: 100,
    rtHeight: 150,
  }
}

function Page({
  state,
  refs,
  scrolls,
  onSkipChange,
}: {
  state: State
  refs: Map<string, PublicInstance>
  /** The page is in a scroller. Chrome's page scrolls with the document, which is no clip to a target: a page that only
   *  looks far past the window (`rootMargin`) has none either. */
  scrolls: boolean
  onSkipChange: (skipped: boolean) => void
}) {
  const ref = (name: string) => (instance: PublicInstance | null) => {
    if (instance) refs.set(name, instance)
  }
  return (
    <div ref={ref("scroller")} style={{ width: SIZE.width, height: SIZE.height, overflowY: scrolls ? "scroll" : undefined }}>
      <div style={{ height: 8000, position: "relative" }}>
        <div ref={ref("above")} style={{ height: state.above, width: 10 }} />
        <div
          ref={ref("host")}
          onContentVisibilityAutoStateChange={(event) => onSkipChange(event.skipped)}
          style={
            {
              position: "absolute",
              left: 0,
              top: state.top === null ? undefined : state.top,
              width: state.hostWidth,
              contentVisibility: state.cv,
              containIntrinsicSize: "400px 200px",
            } as CSSProperties
          }
        >
          <div ref={ref("pre")} style={{ height: state.pre }} />
          <div ref={ref("t")} style={{ width: state.width, height: 50, margin: 10 }} />
        </div>
        <div
          ref={ref("r")}
          style={{
            position: "absolute",
            left: state.rootLeft,
            top: 100,
            // The page's is a content box; gpuix has only the border box: 2 x (10px border + 20px padding) more.
            width: state.rootWidth + 60,
            height: state.rootHeight + 60,
            border: "10px solid #888",
            padding: 20,
            overflow: "auto",
          }}
        >
          <div ref={ref("rt")} style={{ width: state.rtWidth, height: state.rtHeight }} />
          <div ref={ref("rp")} style={{ width: 100, height: 30, marginLeft: -15 }} />
        </div>
      </div>
    </div>
  )
}

type Action = (run: Run) => void

/** The actions of the oracle page, with the names it logs them under. */
const A = {
  cv: (value: State["cv"]): Action => (run) => {
    run.state.cv = value
    run.did(`cv=${value}`)
  },
  top: (top: number): Action => (run) => {
    run.state.top = top
    run.did(`top=${top}`)
  },
  w: (width: number): Action => (run) => {
    run.state.width = width
    run.did(`t.width=${width}`)
  },
  hw: (width: number): Action => (run) => {
    run.state.hostWidth = width
    run.did(`host.width=${width}`)
  },
  read: (): Action => (run) => {
    run.show()
    const bounds = run.renderer.getElementBounds(run.refs.get("t")!.id)!
    run.did(`read t ${bounds.width}x${bounds.height}`)
  },
  ro: (name: string): Action => (run) => {
    run.resize.observe(run.refs.get(name)!)
    run.did(`ro.observe ${name}`)
  },
  io: (name: string, options?: Observed): Action => (run) => {
    run.observer(options).observe(run.refs.get(name)!)
    run.did(`io.observe ${name}`)
  },
  pre: (height: number): Action => (run) => {
    run.state.pre = height
    run.did(`pre.height=${height}`)
  },
  above: (height: number): Action => (run) => {
    run.state.above = height
    run.did(`above.height=${height}`)
  },
  rl: (left: number): Action => (run) => {
    run.state.rootLeft = left
    run.did(`r.left=${left}`)
  },
  rw: (width: number): Action => (run) => {
    run.state.rootWidth = width
    run.did(`r.width=${width}`)
  },
  rh: (height: number): Action => (run) => {
    run.state.rootHeight = height
    run.did(`r.height=${height}`)
  },
  rtw: (width: number): Action => (run) => {
    run.state.rtWidth = width
    run.did(`rt.width=${width}`)
  },
  rth: (height: number): Action => (run) => {
    run.state.rtHeight = height
    run.did(`rt.height=${height}`)
  },
  rscroll: (y: number): Action => (run) => {
    run.renderer.scrollTo(run.refs.get("r")!.id, 0, y)
    run.did(`r.scrollTop=${y}`)
  },
  /** The page gives `#r` a 15px scrollbar; the test window's setting is what makes a bar take room. */
  sb: (): Action => (run) => {
    run.renderer.setScrollbarStyle("classic")
    run.did("r.scrollbar=15")
  },
  scroll: (y: number): Action => (run) => {
    run.renderer.scrollTo(run.refs.get("scroller")!.id, 0, y)
    run.did(`scrollY=${y}`)
  },
}
const both = (...actions: Action[]): Action => (run) => actions.forEach((action) => action(run))

/** What the page's `observer(options)` takes: the root is named, as `#r` is the one root there is. */
interface Observed {
  root?: "r"
  rootMargin?: string
  threshold?: number[]
}

const IN_R: Observed = { root: "r", threshold: THRESHOLDS }
const MARGIN: Observed = { rootMargin: "4000px", threshold: THRESHOLDS }
const BY_THRESHOLDS: Observed = { threshold: THRESHOLDS }

interface Scene {
  cv: State["cv"]
  top: State["top"]
  /** Whether the scene scrolls the page, which takes a scroller. Default: it does. */
  scrolls?: false
  steps: Record<number, Action | null>
}

const SCENES: Record<string, Scene> = {
  ro1_hidden_never: { cv: "hidden", top: 100, steps: { 2: A.ro("t"), 6: A.cv("visible"), 10: null } },
  ro1b_hidden_never_read: { cv: "hidden", top: 100, steps: { 2: both(A.ro("t"), A.read()), 6: A.cv("visible"), 10: null } },
  ro2_resize_while_hidden: { cv: "visible", top: 100, steps: { 1: A.ro("t"), 4: A.cv("hidden"), 7: A.w(150), 10: A.cv("visible"), 14: null } },
  ro2b_same_size: { cv: "visible", top: 100, steps: { 1: A.ro("t"), 4: A.cv("hidden"), 10: A.cv("visible"), 14: null } },
  ro2c_resize_back: { cv: "visible", top: 100, steps: { 1: A.ro("t"), 4: A.cv("hidden"), 7: A.w(150), 8: A.w(100), 10: A.cv("visible"), 14: null } },
  ro2d_stays_hidden: { cv: "visible", top: 100, steps: { 1: A.ro("t"), 4: A.cv("hidden"), 7: A.w(150), 14: null } },
  ro3_read_at_resize: {
    cv: "visible",
    top: 100,
    steps: { 1: A.ro("t"), 4: A.cv("hidden"), 7: both(A.w(150), A.read()), 10: A.cv("visible"), 14: null },
  },
  ro3b_read_every_frame: {
    cv: "visible",
    top: 100,
    steps: { 1: A.ro("t"), 4: A.cv("hidden"), 5: A.read(), 6: A.read(), 7: both(A.w(150), A.read()), 8: A.read(), 10: A.cv("visible"), 14: null },
  },
  ro4_host_hidden: { cv: "hidden", top: 100, steps: { 2: A.ro("host"), 6: A.hw(300), 9: A.cv("visible"), 12: null } },
  ro4b_host_shown_then_hidden: { cv: "visible", top: 100, steps: { 1: A.ro("host"), 4: A.cv("hidden"), 7: A.cv("visible"), 10: null } },
  ro5b_auto_rendered_then_skipped: { cv: "auto", top: 100, steps: { 1: A.ro("t"), 4: A.top(FAR), 7: A.w(150), 10: A.top(100), 14: null } },
  ro5c_auto_same_size: { cv: "auto", top: 100, steps: { 1: A.ro("t"), 4: A.top(FAR), 10: A.top(100), 14: null } },
  io6a_hidden_onscreen_never: { cv: "hidden", top: 100, steps: { 2: A.io("t"), 6: A.cv("visible"), 10: null } },
  io6b_hidden_offscreen_never: { cv: "hidden", top: FAR, steps: { 2: A.io("t"), 6: A.cv("visible"), 9: A.scroll(FAR - 100), 12: null } },
  io6c_hidden_read: { cv: "hidden", top: 100, steps: { 2: both(A.io("t"), A.read()), 6: A.cv("visible"), 10: null } },
  io6d_shown_then_hidden: { cv: "visible", top: 100, steps: { 1: A.io("t"), 4: A.cv("hidden"), 8: A.cv("visible"), 12: null } },
  io6e_shown_then_hidden_read: {
    cv: "visible",
    top: 100,
    steps: { 1: A.io("t"), 4: both(A.cv("hidden"), A.read()), 5: A.read(), 8: A.cv("visible"), 12: null },
  },
  io6f_host_hidden: { cv: "hidden", top: 100, steps: { 2: A.io("host"), 6: A.cv("visible"), 10: null } },
  io7a_auto_far_never: { cv: "auto", top: FAR, steps: { 2: A.io("t"), 6: A.scroll(FAR - 100), 10: A.scroll(0), 14: null } },
  io7b_auto_shown_then_skipped: { cv: "auto", top: 100, steps: { 1: A.io("t"), 4: A.top(FAR), 8: A.top(100), 12: null } },
  io8a_root_box: { cv: "visible", top: 100, steps: { 1: both(A.io("rt", IN_R), A.io("rp", IN_R)), 5: null } },
  io8b_root_scrollbar: {
    cv: "visible",
    top: 100,
    steps: { 1: both(A.rth(600), A.io("rt", IN_R), A.io("rp", IN_R)), 4: A.rscroll(100), 8: A.rscroll(0), 11: null },
  },
  io8b2_root_classic_scrollbar: {
    cv: "visible",
    top: 100,
    steps: { 1: both(A.sb(), A.rth(600), A.io("rt", IN_R), A.io("rp", IN_R)), 4: A.rscroll(100), 8: null },
  },
  io8c_root_margin_px: {
    cv: "visible",
    top: 100,
    steps: {
      1: both(
        A.io("rt", { root: "r", rootMargin: "10px 20px 30px 40px", threshold: THRESHOLDS }),
        A.io("rp", { root: "r", rootMargin: "10px 20px 30px 40px", threshold: THRESHOLDS })
      ),
      5: null,
    },
  },
  io8c2_root_margin_percent: {
    cv: "visible",
    top: 100,
    steps: { 1: A.io("rt", { root: "r", rootMargin: "10%", threshold: THRESHOLDS }), 5: null },
  },
  io8c3_root_margin_negative: {
    cv: "visible",
    top: 100,
    steps: { 1: A.io("rt", { root: "r", rootMargin: "-10px", threshold: THRESHOLDS }), 5: null },
  },
  io8d_root_resize_with_target: {
    cv: "visible",
    top: 100,
    steps: { 1: A.io("rt", IN_R), 4: both(A.rw(150), A.rh(100), A.rtw(200)), 8: null },
  },
  io8e_root_move_with_target: {
    cv: "visible",
    top: 100,
    steps: { 1: A.io("rt", IN_R), 4: both(A.rl(600), A.rth(300)), 8: null },
  },
  io8f_root_move_only: { cv: "visible", top: 100, steps: { 1: A.io("rt", IN_R), 4: A.rl(600), 7: A.rl(500), 10: null } },
  io8g_root_resize_only: {
    cv: "visible",
    top: 100,
    steps: { 1: A.io("rt", IN_R), 4: A.rh(100), 7: A.rh(200), 10: A.rw(50), 14: null },
  },
  io8h_root_move_then_target: { cv: "visible", top: 100, steps: { 1: A.io("rt", IN_R), 4: A.rl(600), 5: A.rth(300), 9: null } },
  io9a_default_host_moves_skipped: {
    cv: "auto",
    top: 100,
    steps: { 1: A.io("t", BY_THRESHOLDS), 4: A.top(FAR), 7: A.top(FAR + 500), 10: A.top(5000), 13: A.top(100), 17: null },
  },
  io9b_margin_host_moves_skipped: {
    cv: "auto",
    top: 100,
    scrolls: false,
    steps: { 1: A.io("t", MARGIN), 4: A.top(6000), 7: A.top(4780), 10: A.top(4730), 13: A.top(6000), 16: A.top(100), 20: null },
  },
  io9c_margin_inner_grows_skipped: {
    cv: "auto",
    top: 100,
    scrolls: false,
    steps: { 1: A.io("t", MARGIN), 4: A.top(4780), 7: A.pre(100), 10: A.pre(0), 13: A.pre(60), 16: A.top(100), 20: null },
  },
  io9d_margin_move_and_inner_grow: {
    cv: "auto",
    top: 100,
    scrolls: false,
    steps: { 1: A.io("t", MARGIN), 4: A.top(4780), 7: both(A.top(4700), A.pre(60)), 10: A.top(100), 14: null },
  },
  io9e_margin_never_laid_out: {
    cv: "auto",
    top: 6000,
    scrolls: false,
    steps: { 2: A.io("t", MARGIN), 5: A.top(4780), 8: A.pre(100), 11: A.top(100), 15: null },
  },
  io9h_margin_never_laid_out_in_margin: {
    cv: "auto",
    top: 4780,
    scrolls: false,
    steps: { 2: A.io("t", MARGIN), 5: A.pre(100), 8: A.top(100), 12: null },
  },
  io9f_margin_above_grows_skipped: {
    cv: "auto",
    top: null,
    scrolls: false,
    steps: { 1: A.io("t", MARGIN), 4: A.above(6000), 7: A.above(4780), 10: A.above(4730), 13: A.above(100), 17: null },
  },
  io9g_default_inner_grows_skipped: {
    cv: "auto",
    top: 100,
    steps: { 1: A.io("t", BY_THRESHOLDS), 4: A.top(FAR), 7: A.pre(100), 10: A.pre(0), 13: A.top(100), 17: null },
  },
}

interface Run {
  state: State
  refs: Map<string, PublicInstance>
  renderer: ReturnType<typeof createTestRoot>["renderer"]
  resize: ResizeObserver
  /** The observer for these options, made when first asked for as the page's is. */
  observer: (options?: Observed) => IntersectionObserver
  /** Renders the state: what the page's script changed is in the DOM when it reads. */
  show: () => void
  did: (what: string) => void
}

const round = (value: number) => Math.round(value * 100) / 100
const pair = (width: number, height: number) => [round(width), round(height)]
const rectOf = (rect: { x: number; y: number; width: number; height: number }) => [
  round(rect.x),
  round(rect.y),
  round(rect.width),
  round(rect.height),
]

/** Runs a scene a frame at a time, as the oracle page does: the step of a frame is applied, then the frame is drawn and the
 *  observers told. */
function play(scene: Scene): Logged[] {
  const root = createTestRoot(SIZE)
  const renderer = root.renderer
  const log: Logged[] = []
  let frame = 0
  try {
    renderer.setInlineDraws(false)
    const refs = new Map<string, PublicInstance>()
    const nameOf = (target: PublicInstance) => [...refs].find(([, instance]) => instance === target)![0]
    const state = initialState(scene.cv, scene.top)
    // Chrome tells the elements near the viewport that they are shown in the rendering update after its first frame;
    // here it is the first frame's (`sweep_relevance`), so a page that has settled is told in frame 1 as Chrome's is.
    const onSkipChange = (skipped: boolean) => log.push({ frame: frame === 0 && !skipped ? 1 : frame, skipped })
    const show = () => root.render(<Page state={{ ...state }} refs={refs} scrolls={scene.scrolls !== false} onSkipChange={onSkipChange} />)
    const resize = new ResizeObserver((entries) => {
      for (const entry of entries) {
        log.push({
          frame,
          ro: nameOf(entry.target),
          border: pair(entry.borderBoxSize[0]!.inlineSize, entry.borderBoxSize[0]!.blockSize),
          content: pair(entry.contentBoxSize[0]!.inlineSize, entry.contentBoxSize[0]!.blockSize),
        })
      }
    })
    const observers = new Map<string, IntersectionObserver>()
    const observer = (options?: Observed) => {
      const key = JSON.stringify([options?.root, options?.rootMargin, options?.threshold])
      if (!observers.has(key)) {
        observers.set(
          key,
          new IntersectionObserver(
            (entries) => {
              for (const entry of entries) {
                log.push({
                  frame,
                  io: nameOf(entry.target),
                  isIntersecting: entry.isIntersecting,
                  ratio: round(entry.intersectionRatio),
                  rect: rectOf(entry.boundingClientRect),
                  intersection: pair(entry.intersectionRect.width, entry.intersectionRect.height),
                  root: rectOf(entry.rootBounds!),
                  at: [round(entry.intersectionRect.x), round(entry.intersectionRect.y)],
                })
              }
            },
            { root: options?.root ? refs.get(options.root)! : undefined, rootMargin: options?.rootMargin, threshold: options?.threshold }
          )
        )
      }
      return observers.get(key)!
    }
    const run: Run = { state, refs, renderer, resize, observer, show, did: (what) => log.push({ frame, did: what }) }
    // The first frame of a window is told differently: the scene is a page that has settled.
    show()
    renderer.drawFrame()
    renderer.dispatchNativeEvents()
    const last = Math.max(...Object.keys(scene.steps).map(Number))
    for (frame = 1; frame <= last; frame++) {
      scene.steps[frame]?.(run)
      show()
      renderer.drawFrame()
      renderer.dispatchNativeEvents()
    }
    return log
  } finally {
    root.unmount()
  }
}

describeNative("observers of a target inside content-visibility skipped content", () => {
  it("has a scene for each of the oracle's", () => {
    expect(Object.keys(SCENES).sort()).toEqual(Object.keys(ORACLE).sort())
  })

  for (const name of Object.keys(SCENES)) {
    it(`delivers what Chrome delivers: ${name}`, () => {
      const played = play(SCENES[name]!)
      const oracle = ORACLE[name]!
      // The state changes of `content-visibility: auto` are told ahead of the observers' callbacks here and after the
      // resize observations in Chrome, so what each says is compared and not the order between them.
      const told = (log: Logged[]) => log.filter((entry) => !("skipped" in entry)).map((entry) => JSON.stringify(entry))
      const skips = (log: Logged[]) => log.flatMap((entry) => ("skipped" in entry ? [`${entry.frame}: skipped ${entry.skipped}`] : []))
      expect(told(played)).toEqual(told(oracle))
      expect(skips(played)).toEqual(skips(oracle))
    })
  }

  /** The scene played to its first step, for the tests that look at what observing costs. */
  function withPage(cv: State["cv"], body: (run: Run) => void) {
    const root = createTestRoot(SIZE)
    try {
      root.renderer.setInlineDraws(false)
      const refs = new Map<string, PublicInstance>()
      const state = initialState(cv, 100)
      const show = () => root.render(<Page state={{ ...state }} refs={refs} scrolls onSkipChange={() => {}} />)
      show()
      root.renderer.drawFrame()
      root.renderer.dispatchNativeEvents()
      body({
        state,
        refs,
        renderer: root.renderer,
        resize: new ResizeObserver(() => {}),
        observer: () => new IntersectionObserver(() => {}),
        show,
        did: () => {},
      })
    } finally {
      root.unmount()
    }
  }

  it("lays out nothing of hidden content to start observing a target inside it", () => {
    /** What starting to observe `name`, and three frames after, cost. */
    const cost = (name: string, forcedRead: boolean) => {
      let found!: { passes: number; built: number }
      withPage("hidden", (run) => {
        const renderer = run.renderer
        const target = run.refs.get(name)!
        const passes = renderer.getLayoutPassCount()
        const built = renderer.getBuildCounters().elementsBuilt
        run.resize.observe(target)
        run.observer().observe(target)
        renderer.dispatchNativeEvents()
        for (let frame = 0; frame < 3; frame++) {
          renderer.drawFrame()
          renderer.dispatchNativeEvents()
        }
        if (forcedRead) renderer.getElementBounds(run.refs.get("t")!.id)
        found = { passes: renderer.getLayoutPassCount() - passes, built: renderer.getBuildCounters().elementsBuilt - built }
        if (!forcedRead) expect(renderer.getBuiltElementBounds(run.refs.get("t")!.id), "the content is not built").toBeNull()
      })
      return found
    }
    const host = cost("host", false)
    expect(cost("t", false), "the cost of the element that skips, which is no content of it").toEqual(host)
    const forced = cost("t", true)
    expect(forced.passes, "a read of geometry lays the content out in a pass of its own").toBe(host.passes + 1)
    expect(forced.built).toBeGreaterThan(host.built)
  })

  it("tells a target inside hidden content it is not intersecting without building the content", () => {
    withPage("hidden", (run) => {
      const told: IntersectionObserverEntry[] = []
      const observer = new IntersectionObserver((entries) => told.push(...entries))
      observer.observe(run.refs.get("t")!)
      run.renderer.dispatchNativeEvents()
      expect(told.map((entry) => [entry.isIntersecting, entry.boundingClientRect.width, entry.boundingClientRect.height])).toEqual([[false, 0, 0]])
      expect(run.renderer.getBuiltElementBounds(run.refs.get("t")!.id), "and the content is not built").toBeNull()
    })
  })

  it("evaluates an observer with an explicit root without reading layout", () => {
    withPage("visible", (run) => {
      const renderer = run.renderer
      const reads: number[] = []
      const getElementBounds = renderer.getElementBounds.bind(renderer)
      renderer.getElementBounds = (id) => {
        reads.push(id)
        return getElementBounds(id)
      }
      const told: IntersectionObserverEntry[] = []
      const observer = new IntersectionObserver(
        (entries) => {
          told.push(...entries)
          // A layout change written by the callback is not drawn when the next frame's reports are applied.
          run.state.rtHeight += 100
          run.show()
        },
        { root: run.refs.get("r")!, threshold: THRESHOLDS }
      )
      observer.observe(run.refs.get("rt")!)
      renderer.dispatchNativeEvents()
      renderer.drawFrame()
      renderer.dispatchNativeEvents()
      expect(told.map((entry) => entry.intersectionRatio)).toEqual([1])

      renderer.drawFrame()
      run.state.rootHeight = 100
      run.show()
      const passes = renderer.getLayoutPassCount()
      renderer.dispatchNativeEvents()
      expect(told.length, "the frame's report was applied").toBeGreaterThan(1)
      expect(renderer.getLayoutPassCount() - passes, "no layout pass to apply it").toBe(0)
      expect(reads, "no getElementBounds").toEqual([])
    })
  })

  it("tells nothing of a target while its content is skipped, and again only if its size differs from the one told", () => {
    const resizes = (name: string) =>
      play(SCENES[name]!).flatMap((entry) => ("ro" in entry ? [`f${entry.frame} ${entry.border.join("x")}`] : []))
    expect(resizes("ro2_resize_while_hidden"), "hidden at 4, resized at 7, shown at 10").toEqual(["f1 100x50", "f10 150x50"])
    expect(resizes("ro2b_same_size"), "shown again at the size it was told").toEqual(["f1 100x50"])
    expect(resizes("ro2c_resize_back"), "resized and back while hidden").toEqual(["f1 100x50"])
    expect(resizes("ro2d_stays_hidden"), "resized and not shown: no 0 by 0 at 4, nothing at 7").toEqual(["f1 100x50"])
    expect(resizes("ro3b_read_every_frame"), "reads of its geometry while hidden deliver nothing and tell nothing").toEqual(["f1 100x50", "f10 150x50"])
    expect(resizes("ro1_hidden_never"), "never rendered: told once, when it shows").toEqual(["f6 100x50"])
  })
})
