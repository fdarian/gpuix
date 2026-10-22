/// What `ResizeObserver` and `IntersectionObserver` deliver for a target inside `content-visibility` skipped content, and
/// for the element that skips it, against headless Chrome: the content of a skipped element is not gathered, so a
/// ResizeObserver tells nothing of its targets while they are skipped (no 0 by 0 when it hides) and compares them with the
/// size it last told once they show; an IntersectionObserver is told a target inside `hidden` content is not intersecting
/// with an empty rect at (0, 0). A read of geometry, which lays the skipped content out, changes neither.
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
  | { frame: number; ro: string; border: number[]; content: number[] }
  | { frame: number; io: string; isIntersecting: boolean; ratio: number; rect: number[]; intersection: number[] }

const ORACLE = JSON.parse(fs.readFileSync(path.join(FIXTURES, "content-visibility-observers-oracle.json"), "utf8")) as Record<string, Logged[]>

const SIZE = { width: 1280, height: 800 }
const FAR = 3000

interface State {
  cv: "visible" | "hidden" | "auto"
  top: number
  /** Width of the target. */
  width: number
  /** Width of the element that skips. */
  hostWidth: number
}

function Page({ state, refs }: { state: State; refs: Map<string, PublicInstance> }) {
  const ref = (name: string) => (instance: PublicInstance | null) => {
    if (instance) refs.set(name, instance)
  }
  return (
    <div ref={ref("scroller")} style={{ width: SIZE.width, height: SIZE.height, overflowY: "scroll" }}>
      <div style={{ height: 8000, position: "relative" }}>
        <div
          ref={ref("host")}
          style={
            {
              position: "absolute",
              left: 0,
              top: state.top,
              width: state.hostWidth,
              contentVisibility: state.cv,
              containIntrinsicSize: "400px 200px",
            } as CSSProperties
          }
        >
          <div ref={ref("t")} style={{ width: state.width, height: 50, margin: 10 }} />
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
  io: (name: string): Action => (run) => {
    run.intersect.observe(run.refs.get(name)!)
    run.did(`io.observe ${name}`)
  },
  scroll: (y: number): Action => (run) => {
    run.renderer.scrollTo(run.refs.get("scroller")!.id, 0, y)
    run.did(`scrollY=${y}`)
  },
}
const both = (...actions: Action[]): Action => (run) => actions.forEach((action) => action(run))

interface Scene {
  cv: State["cv"]
  top: number
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
}

interface Run {
  state: State
  refs: Map<string, PublicInstance>
  renderer: ReturnType<typeof createTestRoot>["renderer"]
  resize: ResizeObserver
  intersect: IntersectionObserver
  /** Renders the state: what the page's script changed is in the DOM when it reads. */
  show: () => void
  did: (what: string) => void
}

const pair = (width: number, height: number) => [Math.round(width * 100) / 100, Math.round(height * 100) / 100]

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
    const state: State = { cv: scene.cv, top: scene.top, width: 100, hostWidth: 400 }
    const show = () => root.render(<Page state={{ ...state }} refs={refs} />)
    const resize = new ResizeObserver((entries) => {
      for (const entry of entries) {
        log.push({
          frame,
          ro: entry.target === refs.get("host") ? "host" : "t",
          border: pair(entry.borderBoxSize[0]!.inlineSize, entry.borderBoxSize[0]!.blockSize),
          content: pair(entry.contentBoxSize[0]!.inlineSize, entry.contentBoxSize[0]!.blockSize),
        })
      }
    })
    const intersect = new IntersectionObserver((entries) => {
      for (const entry of entries) {
        const rect = entry.boundingClientRect
        log.push({
          frame,
          io: entry.target === refs.get("host") ? "host" : "t",
          isIntersecting: entry.isIntersecting,
          ratio: Math.round(entry.intersectionRatio * 100) / 100,
          rect: [...pair(rect.x, rect.y), ...pair(rect.width, rect.height)],
          intersection: pair(entry.intersectionRect.width, entry.intersectionRect.height),
        })
      }
    })
    const run: Run = { state, refs, renderer, resize, intersect, show, did: (what) => log.push({ frame, did: what }) }
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
      const frames = (log: Logged[]) => log.map((entry) => JSON.stringify(entry))
      expect(frames(played)).toEqual(frames(ORACLE[name]!))
    })
  }

  /** The scene played to its first step, for the tests that look at what observing costs. */
  function withPage(cv: State["cv"], body: (run: Run) => void) {
    const root = createTestRoot(SIZE)
    try {
      root.renderer.setInlineDraws(false)
      const refs = new Map<string, PublicInstance>()
      const state: State = { cv, top: 100, width: 100, hostWidth: 400 }
      const show = () => root.render(<Page state={{ ...state }} refs={refs} />)
      show()
      root.renderer.drawFrame()
      root.renderer.dispatchNativeEvents()
      body({
        state,
        refs,
        renderer: root.renderer,
        resize: new ResizeObserver(() => {}),
        intersect: new IntersectionObserver(() => {}),
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
        run.intersect.observe(target)
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
