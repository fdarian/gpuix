/// What `getElementsAt`, `getBoundingClientRect` and `IntersectionObserver` see when `opacity`, `z-index`,
/// `visibility`, `pointer-events`, `position`, `overflow` or `display` comes from a state style, a CSS transition
/// or a motion animation, against what Chrome reports.
///
/// `fixtures/resolved-style-oracle.html` is the page and `fixtures/resolved-style-oracle.json` what headless Chrome
/// reports for it (regenerate with `fixtures/resolved-style-oracle.sh`). A case is a scene and the style a state
/// lays over some of its nodes; here the state is `groupHover` of the scene root (the pointer goes to the probe),
/// or `focusWithin` of the input (the input is focused), and the elements are listed by `getElementsAt`.

import fs from "fs"
import path from "path"
import type { CSSProperties, ReactNode } from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import type { IntersectionObserverEntry, PublicInstance } from "../index"
import { IntersectionObserver, motion } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Style = Record<string, unknown>
interface Node {
  id: string
  tag?: "input"
  style: Style
  children: Node[]
}
type Probe = [number, number]
type Rect = [number, number, number, number]

interface Case {
  kind: "hit" | "rect" | "timeline" | "intersection"
  scene: Node
  state?: Record<string, Style>
  probes?: Probe[]
  ids?: string[]
  target?: string
  transition?: string
  keyframes?: Record<string, [string, string]>
  base?: unknown
  after?: unknown
  samples?: Record<string, string[][]>
  times?: number[]
}

const ORACLE: { cases: Record<string, Case> } = JSON.parse(
  fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/resolved-style-oracle.json"),
    "utf8"
  )
)

/** The state of these cases is the input's own focus; every other case's is a hovered group. */
const FOCUS_STATE = new Set(["an input outside a clip once it is absolute", "an input absolute by a state"])

/** These scenes put their `layer` in an `<anchored>` (a popup placed at 10, 10), which Chrome has as an absolute box. */
const ANCHORED = new Set(["display grid children stack by z-index", "display flex lays children out in a row"])

/** Chrome gives an element a stacking context while an opacity animation or transition is on it, even at the instant
 * the opacity is still 1; gpuix has one only once the opacity is below 1. */
const KNOWN_DEVIATIONS: Record<string, number[]> = {
  "opacity transition lifts a static box over a positioned one": [0],
}

function lengths(style: Style): CSSProperties {
  const px = (value: unknown) => (typeof value === "string" && /^-?[\d.]+px$/.test(value) ? parseFloat(value) : value)
  const out: Style = {}
  for (const [key, value] of Object.entries(style)) {
    if (key === "zIndex" || key === "opacity" || key === "flexShrink") out[key] = Number(value)
    else out[key] = key.startsWith("grid") ? value : px(value)
  }
  return out as CSSProperties
}

interface Setup {
  mode: "group" | "focus"
  state: Record<string, Style>
  anchored: boolean
  /** The node a CSS transition runs on, with its `transition` declaration. */
  transition?: { id: string; value: string }
  /** The node a `motion` animation runs on: it goes from `from` to `to`. */
  motion?: { id: string; property: string; from: string; to: string; started: boolean }
  refs: Map<string, PublicInstance>
}

function element(node: Node, setup: Setup, top: boolean): ReactNode {
  const own = setup.state[node.id]
  const style: Style = { ...lengths(node.style) }
  if (top) style.group = true
  if (own && setup.mode === "group") style.groupHover = lengths(own)
  if (own && setup.mode === "focus") style.focusWithin = lengths(own)
  if (setup.transition?.id === node.id) style.transition = setup.transition.value
  const ref = (instance: PublicInstance | null) => {
    if (instance) setup.refs.set(node.id, instance)
  }
  const children = node.children.map((child) => element(child, setup, false))
  if (node.tag === "input") return <input key={node.id} ref={ref} testId={node.id} style={style as CSSProperties} />
  if (setup.motion?.id === node.id) {
    const motionSetup = setup.motion
    return (
      <motion.div
        key={node.id}
        ref={ref}
        testId={node.id}
        style={style as CSSProperties}
        initial={{ [motionSetup.property]: motionSetup.from }}
        animate={{ [motionSetup.property]: motionSetup.started ? motionSetup.to : motionSetup.from }}
        transition={{ duration: 1, ease: "linear" }}
      />
    )
  }
  if (setup.anchored && node.id === "layer") {
    const { position: _position, left: _left, top: _top, ...rest } = style
    return (
      <anchored key={node.id} ref={ref} testId={node.id} position={{ x: 10, y: 10 }} anchor="topLeft" deferred priority={1} style={rest as CSSProperties}>
        {children}
      </anchored>
    )
  }
  return (
    <div key={node.id} ref={ref} testId={node.id} style={style as CSSProperties}>
      {children}
    </div>
  )
}

function names(node: Node): string[] {
  return [node.id, ...node.children.flatMap(names)]
}

function mount(name: string, data: Case, extra: Partial<Setup> = {}) {
  const { render, renderer, unmount } = createTestRoot()
  const setup: Setup = {
    mode: FOCUS_STATE.has(name) ? "focus" : "group",
    state: data.state ?? {},
    anchored: ANCHORED.has(name),
    refs: new Map(),
    ...extra,
  }
  const draw = () => {
    render(element(data.scene, setup, true))
    renderer.dispatchNativeEvents()
  }
  draw()
  const origin = renderer.getElementBounds(renderer.findByTestId("root").id)
  const nameOf = new Map(names(data.scene).map((id) => [renderer.findByTestId(id).id, id]))
  const hitsAt = (x: number, y: number): string[] =>
    renderer
      .getElementsAt(origin.x + x, origin.y + y)
      .flatMap((id) => nameOf.get(id) ?? [])
  const rectOf = (id: string): Rect => {
    renderer.dispatchNativeEvents()
    const bounds = renderer.getElementBounds(renderer.findByTestId(id).id)
    return [bounds.x - origin.x, bounds.y - origin.y, bounds.width, bounds.height]
  }
  /** Brings the state about: the pointer over the root, or the input focused. */
  const apply = (at: Probe) => {
    if (setup.mode === "focus") {
      renderer.focusElement(renderer.findByTestId("field").id)
    } else {
      renderer.nativeSimulateMouseMove(origin.x + at[0], origin.y + at[1])
    }
    renderer.flush()
    renderer.dispatchNativeEvents()
  }
  /** The pointer well outside the root, which no group state follows. */
  const outside = () => {
    const size = renderer.getWindowSize()
    renderer.nativeSimulateMouseMove(size.width - 2, size.height - 2)
    renderer.flush()
    renderer.dispatchNativeEvents()
  }
  return { renderer, setup, draw, unmount, hitsAt, rectOf, apply, outside }
}

describeNative("walks over the tree see the style an element is painted with", () => {
  for (const [name, data] of Object.entries(ORACLE.cases)) {
    if (data.kind === "hit") {
      it(`elementsFromPoint: ${name}`, () => {
        const t = mount(name, data)
        try {
          const probes = data.probes!
          t.outside()
          expect(
            probes.map(([x, y]) => t.hitsAt(x, y)),
            "before the state applies"
          ).toEqual(data.base)
          const after = probes.map((probe) => {
            t.apply(probe)
            return t.hitsAt(probe[0], probe[1])
          })
          expect(after, "with the state applied").toEqual(data.after)
        } finally {
          t.unmount()
        }
      })
    }

    if (data.kind === "rect") {
      it(`getBoundingClientRect: ${name}`, () => {
        const t = mount(name, data)
        try {
          const rects = (ids: string[]) => Object.fromEntries(ids.map((id) => [id, t.rectOf(id)]))
          t.outside()
          expect(rects(data.ids!), "before the state applies").toEqual(data.base)
          t.apply([5, 5])
          expect(rects(data.ids!), "with the state applied").toEqual(data.after)
        } finally {
          t.unmount()
        }
      })
    }
  }
})

describeNative("a transition or an animation moves an element between the layers elementsFromPoint lists", () => {
  for (const [name, data] of Object.entries(ORACLE.cases)) {
    if (data.kind !== "timeline") continue
    it(name, () => {
      const targetId = data.keyframes || data.transition ? (data.scene.children.some((node) => node.id === "target") ? "target" : Object.keys(data.state ?? {})[0]!) : ""
      const [property, [from, to]] = data.keyframes ? Object.entries(data.keyframes)[0]! : ["", ["", ""]]
      const t = mount(name, data, {
        transition: data.transition ? { id: targetId, value: data.transition } : undefined,
        motion: data.keyframes ? { id: targetId, property, from, to, started: false } : undefined,
      })
      try {
        t.renderer.clockPause()
        if (t.setup.motion) {
          t.setup.motion.started = true
          t.draw()
        } else {
          t.outside()
          t.apply(data.probes![0]!)
        }
        const samples: Record<string, string[][]> = {}
        let elapsed = 0
        for (const time of data.times!) {
          t.renderer.clockFastForward(time * 1000 - elapsed)
          elapsed = time * 1000
          t.renderer.flush()
          t.renderer.dispatchNativeEvents()
          samples[time] = data.probes!.map(([x, y]) => t.hitsAt(x, y))
        }
        const skipped = (KNOWN_DEVIATIONS[name] ?? []).map(String)
        const without = (all: Record<string, string[][]>) =>
          Object.fromEntries(Object.entries(all).filter(([time]) => !skipped.includes(time)))
        expect(without(samples)).toEqual(without(data.samples!))
      } finally {
        t.unmount()
      }
    })
  }
})

describeNative("IntersectionObserver clips by the position and overflow an ancestor is painted with", () => {
  for (const [name, data] of Object.entries(ORACLE.cases)) {
    if (data.kind !== "intersection") continue
    it(name, () => {
      const t = mount(name, data)
      try {
        /** What a new observer reports first, which is what the oracle's page load reports. */
        const report = () => {
          const entries: IntersectionObserverEntry[] = []
          const observer = new IntersectionObserver((batch) => entries.push(...batch))
          observer.observe(t.setup.refs.get(data.target!)!)
          t.renderer.dispatchNativeEvents()
          observer.disconnect()
          const entry = entries[0]!
          return { isIntersecting: entry.isIntersecting, ratio: Math.round(entry.intersectionRatio * 1000) / 1000 }
        }
        t.outside()
        expect(report(), "before the state applies").toEqual(data.base)
        t.apply([5, 5])
        expect(report(), "with the state applied").toEqual(data.after)
      } finally {
        t.unmount()
      }
    })
  }
})
