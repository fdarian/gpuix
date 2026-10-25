/// CSS transitions of SVG properties against headless Chrome: `fixtures/svg-transitions-oracle.html` lays out each
/// case's shapes in Chrome, changes an attribute (or inline style), lists what `getAnimations()` returns on every
/// node, pauses the transitions at each of the case's times and reads the computed value of each property
/// (`fixtures/svg-transitions-oracle.json`). Each case is replayed here on the paused test clock: the same change is
/// made through props (a shape has no style for an SVG property, so a change by style is the change of the
/// attribute, which Chrome treats alike and the oracle records), the transitions are listed, paused and set to the
/// same time, a frame is drawn, and the attribute is read back from the XML the `<svg>` painted. The events are
/// compared in the order they arrive and the elapsed time they carry.

import fs from "fs"
import path from "path"
import { fileURLToPath } from "url"
import { createElement, type ReactNode } from "react"
import { describe, expect, it } from "vitest"
import type { HostTransitionEvent } from "@gpuix/native/host"
import { CSSTransition, type PublicInstance } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

interface NodeSpec {
  id: string
  tag: string
  attrs: Record<string, unknown>
  children: NodeSpec[]
  style?: Record<string, unknown>
}
interface Change {
  node: string
  attrs?: Record<string, unknown>
  style?: Record<string, unknown>
}
interface Read {
  node: string
  prop: string
  initial: string
}
interface Listed {
  type: string
  property: string
}
interface Scenario {
  name: string
  via: "attribute" | "style"
  twin: string | null
  svg: { attrs?: Record<string, unknown>; style?: Record<string, unknown> }
  nodes: NodeSpec[]
  change: Change[]
  then: { seek: number; change: Change[] } | null
  times: number[]
  reads: Read[]
  record: { before: Record<string, Listed[]> | null; listed: Record<string, Listed[]>; samples: { time: number; values: string[] }[] }
}
interface EventScenario {
  name: string
  svg: Scenario["svg"]
  nodes: NodeSpec[]
  change: Change[]
  then: { after: number; change: Change[] } | null
  wait: number
  record: { type: string; target: string; property: string; elapsed: number; pseudo: string }[]
}
const ORACLE: { cases: Scenario[]; events: EventScenario[] } = JSON.parse(
  fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/svg-transitions-oracle.json"),
    "utf8"
  )
)

const NUMBER = /-?\d*\.?\d+(?:e-?\d+)?/g
const numbers = (text: string): number[] => (text.match(NUMBER) ?? []).map(Number)

/** The value of attribute `name` on the element with `id` in the painted XML, if it has one. */
function attributeOf(markup: string, id: string, name: string): string | undefined {
  const tag = new RegExp(`<[\\w]+\\s[^>]*\\bid="${id}"[^>]*>`).exec(markup)
  if (!tag) throw new Error(`no element with id ${id} in ${markup}`)
  const value = new RegExp(`\\s${name}="([^"]*)"`).exec(tag[0])
  return value ? value[1] : undefined
}

/** The properties an element that sets none computes to its parent's. */
const INHERITED = new Set([
  "fill",
  "stroke",
  "stroke-width",
  "stroke-dasharray",
  "stroke-dashoffset",
  "stroke-opacity",
  "fill-opacity",
  "stroke-miterlimit",
  "font-size",
  "letter-spacing",
])

/** The path from the `<svg>` down to the node `id`, the node last. */
function pathTo(nodes: NodeSpec[], id: string): string[] {
  for (const node of nodes) {
    if (node.id === id) return [node.id]
    const inside = pathTo(node.children, id)
    if (inside.length) return [node.id, ...inside]
  }
  return []
}

/** The value of `name` on `id` as the painted XML has it: its own attribute, else the nearest one above it for a
 * property that inherits. */
function writtenFor(markup: string, nodes: NodeSpec[], id: string, name: string): string | undefined {
  const chain = id === "svg" ? ["svg"] : ["svg", ...pathTo(nodes, id)]
  for (const at of chain.reverse()) {
    const value = attributeOf(markup, at, name)
    if (value !== undefined || !INHERITED.has(name)) return value
  }
  return undefined
}

/** A colour the way the XML writes it unanimated: `#rrggbb`, or what an animation wrote (`rgba(...)`). */
const channels = (colour: string): number[] =>
  /^#[0-9a-f]{6}$/i.test(colour) ? [1, 3, 5].map((at) => Number.parseInt(colour.slice(at, at + 2), 16)) : numbers(colour)

/** `got` is what gpuix wrote and `want` what Chrome computed, compared as the kind of value `prop` holds. */
function expectSame(prop: string, got: string, want: string, where: string) {
  if (want === "none" || want === "auto") return expect(got, where).toBe(want)
  // Chrome reads the used size of a shape whose width or height is `auto`, which is 0.
  if ((prop === "width" || prop === "height") && got === "auto") return expect(Number.parseFloat(want), where).toBe(0)
  if (["fill", "stroke", "stop-color", "flood-color"].includes(prop)) {
    if (got === "none") return expect(got, where).toBe(want)
    const [gotChannels, wantChannels] = [channels(got), numbers(want)]
    gotChannels
      .slice(0, 3)
      .forEach((channel, index) => expect(Math.abs(channel - wantChannels[index]), `${where}: ${got} against ${want}`).toBeLessThanOrEqual(1))
    return expect(gotChannels[3] ?? 1, `${where} alpha`).toBeCloseTo(wantChannels[3] ?? 1, 1)
  }
  if (prop === "d") {
    const commands = (path: string) => path.replace(/^path\("/, "").replace(/"\)$/, "").replace(/[^A-Za-z]/g, "")
    expect(commands(got), `${where} commands`).toBe(commands(want))
  }
  // Chrome serialises a letter spacing of 0 as `normal`.
  if (prop === "letter-spacing" && want === "normal") return expect(got === "normal" || Number(got) === 0, where).toBe(true)
  const [gotNumbers, wantNumbers] = [numbers(got), numbers(want)]
  expect(gotNumbers.length, `${where}: ${got} against ${want}`).toBe(wantNumbers.length)
  wantNumbers.forEach((number, index) => expect(gotNumbers[index], `${where}: ${got} against ${want}`).toBeCloseTo(number, 1))
}

const FRAME = { width: "100%", height: "100%", backgroundColor: "#000000" } as const
const kebab = (name: string) => name.replace(/[A-Z]/g, (c) => "-" + c.toLowerCase())

/** The attributes `change` leaves on the nodes it names: a `null` removes one, and `style` is the change of an
 * inline style. A change by CSS name (`stroke-width`) is the attribute of the same name. */
function applied(nodes: NodeSpec[], changes: Change[], svg: Scenario["svg"]): { nodes: NodeSpec[]; svg: Scenario["svg"] } {
  const next = structuredClone({ nodes, svg })
  const camel = (name: string) => name.replace(/-([a-z])/g, (_, c: string) => c.toUpperCase())
  const find = (list: NodeSpec[], id: string): NodeSpec | undefined => {
    for (const node of list) {
      if (node.id === id) return node
      const inside = find(node.children, id)
      if (inside) return inside
    }
    return undefined
  }
  for (const change of changes) {
    const target = change.node === "svg" ? next.svg : find(next.nodes, change.node)
    if (!target) throw new Error(`no node ${change.node}`)
    const holder = target as { attrs?: Record<string, unknown>; style?: Record<string, unknown> }
    holder.attrs ??= {}
    holder.style ??= {}
    for (const [name, value] of Object.entries(change.attrs ?? {})) {
      if (value === null) delete holder.attrs[name]
      else holder.attrs[name] = value
    }
    for (const [name, value] of Object.entries(change.style ?? {})) {
      // The box of the `<svg>` is its style's; every other name is the attribute of a shape.
      if (name === "transition" || name === "transitionProperty" || (change.node === "svg" && ["width", "height"].includes(name))) holder.style[name] = typeof value === "string" ? value.replace(/px$/, "") : value
      else holder.attrs[camel(name)] = typeof value === "string" ? value.replace(/px/g, "").replace(/path\("(.*)"\)/, "$1") : value
    }
  }
  return next
}

function toElement(spec: NodeSpec, refs: Map<string, PublicInstance>): ReactNode {
  const { text, ...attrs } = spec.attrs
  const children = typeof text === "string" ? [text] : spec.children.map((child) => toElement(child, refs))
  return createElement(
    spec.tag,
    {
      key: spec.id,
      id: spec.id,
      ...attrs,
      style: spec.style,
      ref: (node: PublicInstance | null) => {
        if (node) refs.set(spec.id, node)
      },
    },
    ...children
  )
}

function page(nodes: NodeSpec[], svg: Scenario["svg"], refs: Map<string, PublicInstance>) {
  return (
    <div style={FRAME}>
      <svg
        id="svg"
        testId="svg"
        width={200}
        height={100}
        viewBox="0 0 200 100"
        {...svg.attrs}
        style={{ width: 200, height: 100, ...svg.style }}
        ref={(node: PublicInstance | null) => {
          if (node) refs.set("svg", node)
        }}
      >
        {nodes.map((node) => toElement(node, refs))}
      </svg>
    </div>
  )
}

function ids(nodes: NodeSpec[]): string[] {
  return nodes.flatMap((node) => [node.id, ...ids(node.children)])
}

describeNative("SVG transitions against Chrome", () => {
  for (const scenario of ORACLE.cases) {
    if (scenario.twin) {
      it(`${scenario.name}: Chrome treats a style change as the change of the attribute`, () => {
        const twin = ORACLE.cases.find((other) => other.name === scenario.twin)
        expect(twin, scenario.twin ?? "").toBeDefined()
        expect(scenario.record).toEqual(twin!.record)
      })
      continue
    }
    it(scenario.name, { timeout: 30_000 }, () => {
      const { render, renderer, unmount } = createTestRoot()
      try {
        renderer.clockPause()
        const refs = new Map<string, PublicInstance>()
        const frame = () => {
          renderer.clockFastForward(0)
          renderer.dispatchNativeEvents()
        }
        const listed = () => {
          const out: Record<string, Listed[]> = {}
          for (const id of ["svg", ...ids(scenario.nodes)]) {
            const animations = refs.get(id)?.getAnimations() ?? []
            if (animations.length) {
              out[id] = animations.map((animation) => ({
                type: animation instanceof CSSTransition ? "CSSTransition" : animation.constructor.name,
                property: (animation as CSSTransition).transitionProperty,
              }))
            }
          }
          return out
        }
        const all = () => ["svg", ...ids(scenario.nodes)].flatMap((id) => refs.get(id)?.getAnimations() ?? [])
        render(page(scenario.nodes, scenario.svg, refs))
        frame()
        let state = applied(scenario.nodes, scenario.change, scenario.svg)
        render(page(state.nodes, state.svg, refs))
        frame()
        if (scenario.then) {
          expect(listed(), "before the second change").toEqual(scenario.record.before)
          for (const animation of all()) {
            animation.pause()
            animation.currentTime = scenario.then.seek
          }
          frame()
          state = applied(state.nodes, scenario.then.change, state.svg)
          render(page(state.nodes, state.svg, refs))
          frame()
        }
        expect(listed(), "getAnimations").toEqual(scenario.record.listed)

        const svg = renderer.findByTestId("svg")!.id
        scenario.record.samples.forEach((sample) => {
          for (const animation of all()) {
            animation.pause()
            animation.currentTime = sample.time
          }
          frame()
          const markup = renderer.getSvgMarkup(svg)
          if (markup === null) throw new Error("the svg painted no shapes")
          scenario.reads.forEach((read, index) => {
            const written = writtenFor(markup, scenario.nodes, read.node, read.prop)
            expectSame(read.prop, written ?? read.initial, sample.values[index], `${scenario.name} at ${sample.time}ms, ${read.prop}`)
          })
        })
      } finally {
        unmount()
      }
    })
  }
})

describeNative("SVG transition events against Chrome", () => {
  for (const scenario of ORACLE.events) {
    it(scenario.name, { timeout: 30_000 }, () => {
      const { render, renderer, unmount } = createTestRoot()
      try {
        renderer.clockPause()
        const seen: { type: string; target: string; property: string; elapsed: number }[] = []
        const refs = new Map<string, PublicInstance>()
        const hear = (type: string) => (event: HostTransitionEvent) => {
          const target = [...refs].find(([, node]) => node === event.target)
          seen.push({ type, target: target ? target[0] : "?", property: event.propertyName, elapsed: Math.round(event.elapsedTime * 1000) / 1000 })
        }
        const root = (nodes: NodeSpec[], svg: Scenario["svg"]) => {
          const handlers = {
            onTransitionRun: hear("transitionrun"),
            onTransitionStart: hear("transitionstart"),
            onTransitionEnd: hear("transitionend"),
            onTransitionCancel: hear("transitioncancel"),
          }
          return (
            <div style={FRAME}>
              <svg
                id="svg"
                testId="svg"
                width={200}
                height={100}
                viewBox="0 0 200 100"
                {...svg.attrs}
                {...handlers}
                style={{ width: 200, height: 100, ...svg.style }}
                ref={(node: PublicInstance | null) => {
                  if (node) refs.set("svg", node)
                }}
              >
                {nodes.map((node) => toElement(node, refs))}
              </svg>
            </div>
          )
        }
        const frame = () => {
          renderer.dispatchNativeEvents()
        }
        render(root(scenario.nodes, scenario.svg))
        frame()
        let state = applied(scenario.nodes, scenario.change, scenario.svg)
        render(root(state.nodes, state.svg))
        frame()
        if (scenario.then) {
          renderer.clockFastForward(scenario.then.after)
          frame()
          state = applied(state.nodes, scenario.then.change, state.svg)
          render(root(state.nodes, state.svg))
          frame()
        }
        renderer.clockFastForward(scenario.wait)
        frame()
        // What one element hears is in a fixed order; Chrome does not keep one between elements that start in a frame.
        const byElement = (events: { type: string; target: string; property: string; elapsed: number }[]) =>
          Object.fromEntries(
            [...new Set(events.map((event) => event.target))].map((target) => [
              target,
              events.filter((event) => event.target === target).map(({ type, property, elapsed }) => ({ type, property, elapsed })),
            ])
          )
        expect(byElement(seen)).toEqual(byElement(scenario.record))
      } finally {
        unmount()
      }
    })
  }
})
