/// A transform written or animated shows in the next read of geometry, inside content-visibility-skipped content as
/// anywhere else, against headless Chrome: `fixtures/transform-skipped-read-oracle.html` is the page and
/// `fixtures/transform-skipped-read-oracle.json` what Chrome answers (regenerate with
/// `transform-skipped-read-oracle.sh`). Chrome answers a read inside a skipped host as it answers one of a host that is
/// not skipped, after any write and any frame, and the read of a visible element right after a write is no staler.
///
/// A scene is `anc > host > mid > target`, the host skipped (`hidden`, or `auto` far from the window) or, for the
/// controls, `visible`. The write is a change of style committed without a frame, and the read follows it in the same
/// task, as a handler's does in a window; the scenes with frames draw two between the first read and the write and two
/// between the write and the second. Inline draws are off so that nothing but `drawFrame` delivers a frame.
///
/// Not covered: `Element.animate()` (`f.waapi.*`) does not exist here, and the computed `transform` Chrome reports next to
/// the animation scenes is not compared.

import fs from "fs"
import path from "path"
import type { CSSProperties } from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { animate, defineKeyframes, motion, motionValue } from "../index"
import type { MotionValue } from "../index"
import { flushSync } from "../reconciler/reconciler"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { verifying } from "./retained-harness.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rect = [number, number, number, number]
type Id = "anc" | "host" | "mid" | "target" | "elsewhere"
type Mode = "hidden" | "auto" | "visible" | "visible-far"
type Style = Record<string, string | number | MotionValue<number>>
type Styles = Partial<Record<Id, Style>>

/** The elements of the oracle's scenes; `elsewhere` is a box beside them that only the pass counts use. */
const IDS = ["anc", "host", "mid", "target"] as const
type Chain = (typeof IDS)[number]

interface Write {
  on: Chain
  css: Record<string, string>
  init?: Record<string, string>
  frames: boolean
}
interface Animation {
  kind: "css" | "transition"
  frames: boolean
  on: Chain
}
interface Scene {
  write: Write | Animation | { kind: "waapi"; frames: boolean }
  skipped: boolean
  skippedAfter: boolean
  before: Record<Chain, Rect>
  after?: Record<Chain, Rect>
  late?: Record<Chain, Rect>
  t1?: Record<Chain, Rect>
  t2?: Record<Chain, Rect>
}

const ORACLE: Record<string, Scene> = JSON.parse(
  fs.readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/transform-skipped-read-oracle.json"), "utf8")
)
delete ORACLE.chrome
delete ORACLE.chromeVersion

/**
 * `a.rotate30.auto` and its twin with frames: Chrome makes the `auto` host relevant once its ancestor is turned 30 degrees
 * (the box lands far outside the window, at about (-1374, 2615)), and does not at 20 degrees or at -30. gpuix keeps it
 * skipped, as it keeps every box that far out. The boxes read are Chrome's either way.
 */
const STAYS_SKIPPED = new Set(["a.rotate30.auto", "d-a.rotate30.auto"])

const TOLERANCE = 0.06
const SLOW = 120_000
const WINDOW = { width: 800, height: 600 }

const MODES: Record<Mode, { contentVisibility: string; top: number }> = {
  hidden: { contentVisibility: "hidden", top: 10 },
  auto: { contentVisibility: "auto", top: 3000 },
  visible: { contentVisibility: "visible", top: 10 },
  "visible-far": { contentVisibility: "visible", top: 3000 },
}

const camel = (name: string) => name.replace(/-([a-z])/g, (_, letter: string) => letter.toUpperCase())

function styleOf(css: Record<string, string>): Style {
  return Object.fromEntries(Object.entries(css).map(([name, value]) => [camel(name), value]))
}

defineKeyframes("skipped-read-kf", {
  "0%": { transform: "translate(0px, 0px)" },
  "100%": { transform: "translate(100px, 50px) rotate(90deg)" },
})

function abs(left: number, top: number, width: number, height: number): CSSProperties {
  return { position: "absolute", left, top, width, height }
}

/** What an animation tells, by element: `mid:start`. */
type Told = (event: string) => void

/** Props of the `motion` elements of a scene, by element. */
type Motion = Partial<Record<Chain, Record<string, unknown>>>

function sceneOf(mode: Mode, styles: Styles, ref: (id: Chain) => (node: unknown) => void, told?: Told, moves: Motion = {}) {
  const tells = (id: Chain) =>
    told && {
      onAnimationStart: () => told(`${id}:start`),
      onAnimationIteration: () => told(`${id}:iteration`),
      onAnimationEnd: () => told(`${id}:end`),
      onTransitionEnd: () => told(`${id}:transitionend`),
    }
  // Motion values bind through a `motion` element.
  const tag = (id: Chain) => (moves[id] || Object.values(styles[id] ?? {}).some((value) => typeof value === "object") ? motion.div : "div")
  const Anc = tag("anc")
  const Mid = tag("mid")
  const Target = tag("target")
  return (
    <div style={abs(0, 0, 800, 600)}>
      <div testId="elsewhere" style={{ ...abs(500, 300, 50, 50), ...styles.elsewhere } as CSSProperties} />
      <Anc testId="anc" ref={ref("anc")} style={{ ...abs(100, 50, 200, 100), ...styles.anc } as CSSProperties}>
        <div
          testId="host"
          ref={ref("host")}
          style={
            {
              ...abs(20, MODES[mode].top, 100, 60),
              contentVisibility: MODES[mode].contentVisibility,
              ...styles.host,
            } as CSSProperties
          }
        >
          <Mid testId="mid" ref={ref("mid")} {...tells("mid")} {...moves.mid} style={{ ...abs(10, 5, 80, 40), ...styles.mid } as CSSProperties}>
            <Target testId="target" ref={ref("target")} {...tells("target")} {...moves.target} style={{ ...abs(5, 5, 30, 20), ...styles.target } as CSSProperties} />
          </Mid>
        </div>
      </Anc>
    </div>
  )
}

/** A window with a scene in it, three frames old, and the ways to write to it and read from it. */
function mountScene(mode: Mode, initial: Styles, told?: Told, moves?: Motion) {
  const mounted = createTestRoot(WINDOW)
  const renderer = mounted.renderer
  renderer.clockPause()
  renderer.setInlineDraws(false)
  const nodes: Partial<Record<Chain, { getAnimations(): { currentTime: number | null; pause(): void }[] }>> = {}
  const ref = (id: Chain) => (node: unknown) => {
    nodes[id] = node as (typeof nodes)[Chain]
  }
  // `TestRenderer` flushes a commit with a read or `flush()`, which invalidates the layout whatever the commit was.
  // A window invalidates at the commit, as the commit says, which is what `flushIfDirty` of the native renderer does.
  const invalidate = () => (renderer as unknown as { native: { flushIfDirty(): void } }).native.flushIfDirty()
  let styles = initial
  mounted.render(sceneOf(mode, styles, ref, told, moves))
  const frames = (count: number) => {
    for (let frame = 0; frame < count; frame++) renderer.drawFrame()
  }
  frames(3)
  const ids = Object.fromEntries([...IDS, "elsewhere"].map((id) => [id, renderer.findByTestId(id)!.id])) as Record<Id, number>
  return {
    renderer,
    frames,
    nodes,
    ids,
    unmount: () => {
      const mismatches = verifying ? renderer.getRetentionMismatches().join("\n") : ""
      renderer.setInlineDraws(true)
      mounted.unmount()
      expect(mismatches, "frames that replayed differ from the ones built whole").toBe("")
    },
    /** Commits a change of style, which a window invalidates at once, and returns without a frame. */
    write(on: Id, change: Style) {
      styles = { ...styles, [on]: { ...styles[on], ...change } }
      flushSync(() => mounted.root.render(sceneOf(mode, styles, ref, told, moves)))
      invalidate()
    },
    rect(id: Id): Rect {
      const box = renderer.getElementBounds(ids[id])
      if (box === null) throw new Error(`${id} has no box`)
      return [box.x, box.y, box.width, box.height]
    },
    rects(): Record<Chain, Rect> {
      return Object.fromEntries(IDS.map((id) => [id, this.rect(id)])) as Record<Chain, Rect>
    },
    /** Whether the target is in skipped content now, as the last frames left it. */
    skipped: () => renderer.probeElementBounds(ids.target) === null,
    passes: () => renderer.getLayoutPassCount(),
  }
}

type Scenes = ReturnType<typeof mountScene>

function differences(phase: string, got: Record<Chain, Rect>, want: Record<Chain, Rect>): string[] {
  return IDS.flatMap((id) =>
    got[id].some((value, index) => Math.abs(value - want[id][index]!) > TOLERANCE)
      ? [`${phase} ${id}: got [${got[id].map((v) => +v.toFixed(2))}] Chrome [${want[id].map((v) => +v.toFixed(2))}]`]
      : []
  )
}

/** Plays a scene of the oracle in a window and lists where gpuix is not where Chrome was. */
function play(name: string, scene: Scene): string[] {
  const parts = name.split(".")
  const mode = parts[parts.length - 1] as Mode
  const wrong: string[] = []
  if ("kind" in scene.write && (scene.write.kind === "css" || scene.write.kind === "transition")) {
    const write = scene.write
    const mounted = mountScene(mode, { [write.on]: write.kind === "css" ? { animation: "skipped-read-kf 1000ms linear both paused" } : { transition: "transform 1000ms linear" } })
    try {
      return playAnimation(mounted, write, scene)
    } finally {
      mounted.unmount()
    }
  }
  const write = scene.write as Write
  const mounted = mountScene(mode, write.init ? { [write.on]: styleOf(write.init) } : {})
  try {
    if (mounted.skipped() !== scene.skipped) wrong.push(`skipped: got ${!scene.skipped} Chrome ${scene.skipped}`)
    wrong.push(...differences("before", mounted.rects(), scene.before))
    if (write.frames) mounted.frames(2)
    mounted.write(write.on, styleOf(write.css))
    if (write.frames) mounted.frames(2)
    wrong.push(...differences("after", mounted.rects(), scene.after!))
    mounted.frames(2)
    wrong.push(...differences("late", mounted.rects(), scene.late!))
    const skippedAfter = STAYS_SKIPPED.has(name) ? true : scene.skippedAfter
    if (mounted.skipped() !== skippedAfter) wrong.push(`skippedAfter: got ${!skippedAfter} Chrome ${scene.skippedAfter}`)
  } finally {
    mounted.unmount()
  }
  return wrong
}

function playAnimation(mounted: Scenes, write: Animation, scene: Scene): string[] {
  const wrong: string[] = []
  const animation = () => {
    const animations = mounted.nodes[write.on]!.getAnimations()
    expect(animations, `the animation on ${write.on}`).toHaveLength(1)
    return animations[0]!
  }
  if (mounted.skipped() !== scene.skipped) wrong.push(`skipped: got ${!scene.skipped} Chrome ${scene.skipped}`)
  // `getAnimations()` lists nothing for content that no frame built (a deviation of round 18); a read styles it, as in Chrome.
  const first = mounted.rects()
  let held = write.kind === "css" ? animation() : null
  if (held) held.currentTime = 0
  const before = { ...scene.before }
  // Chrome answers an empty box for `mid` in its first read of an animation inside skipped content (t1 and t2 agree).
  if (scene.skipped && (write.on === "mid" || write.on === "target")) before.mid = first.mid
  wrong.push(...differences("before", mounted.rects(), before))
  if (write.kind === "transition") {
    mounted.write(write.on, { transform: "translate(100px, 50px) rotate(90deg)" })
    // gpuix starts a transition when a frame builds the element, Chrome when the style is recalculated.
    mounted.frames(1)
    held = animation()
    held.pause()
    held.currentTime = 0
    if (write.frames) mounted.frames(2)
  }
  for (const [name, time] of [["t1", 250], ["t2", 750]] as const) {
    if (write.frames) mounted.frames(2)
    held!.currentTime = time
    if (write.frames) mounted.frames(2)
    wrong.push(...differences(name, mounted.rects(), scene[name]!))
  }
  if (mounted.skipped() !== scene.skippedAfter) wrong.push(`skippedAfter: got ${!scene.skippedAfter} Chrome ${scene.skippedAfter}`)
  return wrong
}

describeNative("a read after a transform change is where Chrome says, skipped content or not", () => {
  for (const [name, scene] of Object.entries(ORACLE)) {
    if ("kind" in scene.write && scene.write.kind === "waapi") continue
    it(name, () => {
      expect(play(name, scene)).toEqual([])
    }, SLOW)
  }
})

/** Every element has a transform, so that a write to one is a change of paint and nothing else. */
const TRANSFORMED: Styles = {
  anc: { transform: "rotate(10deg)" },
  host: { transform: "rotate(3deg)" },
  mid: { transform: "rotate(5deg)" },
  target: { transform: "rotate(5deg)" },
  elsewhere: { transform: "rotate(5deg)" },
}
const WRITTEN = { transform: "rotate(25deg)" }

/** The scene that is not skipped, in the same place. */
const CONTROL: Record<Mode, Mode> = { hidden: "visible", auto: "visible-far", visible: "visible", "visible-far": "visible-far" }

/** Where `id` is in a window that shows `styles` from the start and skips nothing. */
function placeOf(mode: Mode, styles: Styles, id: Id): Rect {
  const control = mountScene(CONTROL[mode], styles)
  try {
    return control.rect(id)
  } finally {
    control.unmount()
  }
}

const apart = (got: Rect, want: Rect) => got.some((value, index) => Math.abs(value - want[index]!) > TOLERANCE)

describeNative("a transform written on or beside what a read shares a pass with", () => {
  for (const mode of ["hidden", "auto"] as const) {
    it(`keeps the pass of a read of skipped content for a write to an element that is not on its chain (${mode})`, () => {
      const written = placeOf(mode, { ...TRANSFORMED, elsewhere: WRITTEN }, "elsewhere")
      const beside = placeOf(mode, { ...TRANSFORMED, elsewhere: { transform: "rotate(17deg)" } }, "elsewhere")
      const mounted = mountScene(mode, TRANSFORMED)
      try {
        const target = mounted.rect("target")
        let start = mounted.passes()
        mounted.write("elsewhere", WRITTEN)
        expect(mounted.rect("target"), "a write elsewhere").toEqual(target)
        expect(mounted.passes() - start, "the pass of the read before is shared").toBe(0)
        const elsewhere = mounted.rect("elsewhere")
        expect(mounted.passes() - start, "the element that was written is laid out again to be read").toBe(1)
        expect(apart(elsewhere, written), `got [${elsewhere}] want [${written}]`).toBe(false)

        mounted.frames(2)
        start = mounted.passes()
        mounted.write("elsewhere", { transform: "rotate(17deg)" })
        mounted.frames(2)
        expect(mounted.rect("target"), "a write elsewhere and two frames").toEqual(target)
        expect(mounted.passes() - start, "the pass of the read before is still shared").toBe(0)
        expect(apart(mounted.rect("elsewhere"), beside)).toBe(false)
      } finally {
        mounted.unmount()
      }
    }, SLOW)

    for (const on of IDS) {
      for (const frames of [0, 2]) {
        it(`ends it for a write to ${on}, ${frames} frames before the read (${mode})`, () => {
          const want = placeOf(mode, { ...TRANSFORMED, [on]: WRITTEN }, "target")
          const mounted = mountScene(mode, TRANSFORMED)
          try {
            expect(apart(mounted.rect("target"), want), "the write moves the target").toBe(true)
            mounted.write(on, WRITTEN)
            mounted.frames(frames)
            const start = mounted.passes()
            const got = mounted.rect("target")
            expect(apart(got, want), `got [${got}] want [${want}]`).toBe(false)
            expect(mounted.passes() - start, "one pass").toBe(1)
            mounted.rect("target")
            expect(mounted.passes() - start, "and the next read shares it").toBe(1)
          } finally {
            mounted.unmount()
          }
        }, SLOW)
      }
    }
  }

  it("lays out for a read of an element that is not skipped after a write to it or above it, and not after one beside it", () => {
    const shown: Styles = { ...TRANSFORMED, elsewhere: { transform: "rotate(40deg)" } }
    const steps = IDS.map((on) => {
      shown[on] = { transform: `rotate(${30 + IDS.indexOf(on)}deg)` }
      return { on, change: shown[on]!, want: placeOf("visible", shown, "target") }
    })
    const mounted = mountScene("visible", TRANSFORMED)
    try {
      mounted.rect("target")
      let start = mounted.passes()
      mounted.write("elsewhere", { transform: "rotate(40deg)" })
      mounted.rect("target")
      expect(mounted.passes() - start, "a write beside the element").toBe(0)
      for (const step of steps) {
        start = mounted.passes()
        mounted.write(step.on, step.change)
        const got = mounted.rect("target")
        expect(mounted.passes() - start, `a write to ${step.on}`).toBe(1)
        expect(apart(got, step.want), `got [${got}] want [${step.want}]`).toBe(false)
        mounted.rect("target")
        expect(mounted.passes() - start, "and the next read shares it").toBe(1)
      }
    } finally {
      mounted.unmount()
    }
  }, SLOW)
})

describeNative("a motion value that a transform key is bound to", () => {
  for (const mode of ["visible", "hidden"] as const) {
    it(`is read at the value it was written (${mode})`, () => {
      const x = motionValue(0)
      const mounted = mountScene(mode, { anc: { x } })
      try {
        const before = mounted.rect("target")
        const start = mounted.passes()
        x.set(40)
        const after = mounted.rect("target")
        expect(after[0] - before[0], "the target moved with x").toBeCloseTo(40, 1)
        expect(mounted.passes() - start, "one pass").toBe(1)
        mounted.rect("target")
        expect(mounted.passes() - start, "and the next read shares it").toBe(1)
        mounted.frames(2)
        expect(mounted.rect("target")[0] - before[0], "a frame leaves it").toBeCloseTo(40, 1)
      } finally {
        mounted.unmount()
      }
    }, SLOW)
  }
})

describeNative("a transform animation that runs between reads of skipped content", () => {
  /** The target after each quarter of a second of an animation that runs on `on`, read once before the first. */
  function reads(mode: Mode, on: Chain): Rect[] {
    const mounted = mountScene(mode, { [on]: { animation: "skipped-read-kf 1000ms linear both" } })
    try {
      mounted.rect("target")
      return [250, 250, 250].map((elapsed) => {
        mounted.renderer.clockAdvance(elapsed)
        mounted.renderer.drawFrame()
        return mounted.rect("target")
      })
    } finally {
      mounted.unmount()
    }
  }

  for (const on of IDS) {
    for (const mode of ["hidden", "auto"] as const) {
      it(`reads the target where a visible one is, animating ${on} (${mode})`, () => {
        const want = reads(CONTROL[mode], on)
        const got = reads(mode, on)
        expect(got.filter((rect, index) => apart(rect, want[index]!)), `got ${JSON.stringify(got)} want ${JSON.stringify(want)}`).toEqual([])
        expect(apart(want[0]!, want[1]!), "and the animation moves it").toBe(true)
      }, SLOW)
    }
  }
})

describeNative("a read of an animation in skipped content", () => {
  /** What four frames of the clock do to the scene, with or without a read of the target after each, and what the reads did. */
  function frames(mode: Mode, animation: string, reading: boolean) {
    const told: string[] = []
    const mounted = mountScene(mode, { target: { animation } }, (event) => told.push(event))
    try {
      mounted.frames(12)
      expect(mounted.skipped(), "skipped").toBe(true)
      const renderer = mounted.renderer
      const before = renderer.getFrameWorkCounters().dormantTicks
      const seen: Rect[] = []
      let paintedByReads = 0
      let tickedByReads = 0
      for (const elapsed of [200, 200, 300, 300]) {
        renderer.clockAdvance(elapsed)
        mounted.frames(1)
        if (reading) {
          const painted = renderer.getPaintedFrameCount()
          const ticked = renderer.getFrameWorkCounters().dormantTicks
          seen.push(mounted.rect("target"))
          paintedByReads += renderer.getPaintedFrameCount() - painted
          tickedByReads += renderer.getFrameWorkCounters().dormantTicks - ticked
        }
      }
      return {
        dormantTicks: renderer.getFrameWorkCounters().dormantTicks - before,
        paintedByReads,
        tickedByReads,
        told,
        seen,
        skipped: mounted.skipped(),
      }
    } finally {
      mounted.unmount()
    }
  }

  for (const mode of ["hidden", "auto"] as const) {
    it(`shares a pass between the reads of an instant, beside a loop in the content, and runs another after a frame (${mode})`, () => {
      const mounted = mountScene(mode, { target: { animation: "skipped-read-kf 1000ms linear infinite" } })
      try {
        mounted.frames(3)
        const start = mounted.passes()
        const first = mounted.rect("target")
        for (let read = 0; read < 9; read++) {
          expect(mounted.rect(read % 2 ? "mid" : "target").length).toBe(4)
        }
        expect(mounted.rects().target, "the same instant").toEqual(first)
        expect(mounted.passes() - start, "ten reads in one task run one pass").toBe(1)
        mounted.renderer.clockAdvance(300)
        mounted.frames(1)
        const later = mounted.rect("target")
        expect(apart(later, first), "a frame moves the value").toBe(true)
        expect(mounted.passes() - start, "and the read after it runs another pass").toBe(2)
        mounted.rect("target")
        expect(mounted.passes() - start, "that the next read shares").toBe(2)
      } finally {
        mounted.unmount()
      }
    }, SLOW)
  }

  for (const mode of ["hidden", "auto"] as const) {
    for (const [name, animation] of [
      ["a loop", "skipped-read-kf 1000ms linear infinite"],
      ["a run that ends between the reads", "skipped-read-kf 600ms linear both"],
    ] as const) {
      it(`shows the value of the frame it reads in and wakes nothing: ${name} on the target (${mode})`, () => {
        const read = frames(mode, animation, true)
        const alone = frames(mode, animation, false)
        expect(read.seen.length, "four reads").toBe(4)
        expect(new Set(read.seen.map((rect) => rect.join())).size, "the animation moves the box between reads").toBeGreaterThan(1)
        expect(read.skipped, "the target stays skipped").toBe(true)
        expect(read.paintedByReads, "a read paints no frame").toBe(0)
        expect(read.tickedByReads, "and ticks nothing").toBe(0)
        expect(read.dormantTicks, "the frames after the reads tick no more than frames with none").toBe(alone.dormantTicks)
        expect(read.told, "and tell nothing").toEqual(alone.told)
        expect(read.told, "the frames tell nothing while it is skipped").toEqual([])
      }, SLOW)
    }
  }
})

describeNative("a native motion and a motion value that run in skipped content", () => {
  const linear = { duration: 1, ease: "linear" } as const

  /** The target after each quarter of a second of what runs on it, read once before the first. */
  function reads(mode: Mode, how: "motion" | "value"): Rect[] {
    const x = motionValue(0)
    const mounted =
      how === "motion"
        ? mountScene(mode, {}, undefined, { target: { initial: { x: 0 }, animate: { x: 100 }, transition: linear } })
        : mountScene(mode, { target: { x } })
    try {
      mounted.rect("target")
      if (how === "value") animate(x, 100, linear)
      return [250, 250, 250].map((elapsed) => {
        mounted.renderer.clockAdvance(elapsed)
        mounted.renderer.drawFrame()
        return mounted.rect("target")
      })
    } finally {
      mounted.unmount()
    }
  }

  for (const how of ["motion", "value"] as const) {
    for (const mode of ["hidden", "auto"] as const) {
      it(`reads the target where a visible one is: ${how === "motion" ? "a native motion" : "an animated motion value"} (${mode})`, () => {
        const want = reads(CONTROL[mode], how)
        const got = reads(mode, how)
        expect(got.filter((rect, index) => apart(rect, want[index]!)), `got ${JSON.stringify(got)} want ${JSON.stringify(want)}`).toEqual([])
        expect(apart(want[0]!, want[2]!), "and it moves the target").toBe(true)
      }, SLOW)
    }
  }
})
