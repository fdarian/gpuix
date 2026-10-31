/// CSS keyframes on SVG properties against headless Chrome: `fixtures/svg-keyframes-oracle.html` paints each case's
/// shapes in Chrome, pauses the animation at each of the case's times and reads the computed value of each property
/// (`fixtures/svg-keyframes-oracle.json`). Each case is replayed here on the paused test clock: the animation is
/// paused and set to the same time, a frame is drawn, and the attribute is read back from the XML the `<svg>`
/// painted (`getSvgMarkup`). Numbers, lists and colours are compared as numbers; what the shapes look like when
/// painted, the events and the Web Animations objects of a shape are below the oracle.

import fs from "fs"
import path from "path"
import { fileURLToPath } from "url"
import { createElement, type ReactNode } from "react"
import { describe, expect, it } from "vitest"
import {
  CSSAnimation,
  defineKeyframes,
  motion,
  type PublicInstance,
} from "../index"
import type { HostAnimationEvent } from "@ghia/native/host"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

interface NodeSpec {
  id: string
  tag: string
  attrs: Record<string, unknown>
  children: NodeSpec[]
  style?: Record<string, unknown>
}
interface Read {
  node: string
  prop: string
  initial: string
  /** The node whose attribute holds it here: the `<svg>` or a group, for a property the shape inherits. */
  at?: string
}
interface Case {
  name: string
  keyframes: string[]
  svg: { style?: Record<string, unknown> }
  nodes: NodeSpec[]
  animate: string | null
  times: number[]
  reads: Read[]
  record: { animations: string[]; samples: { time: number; values: string[] }[] }
}
const ORACLE: { keyframes: Record<string, Record<string, Record<string, unknown>>>; cases: Case[] } = JSON.parse(
  fs.readFileSync(
    path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures/svg-keyframes-oracle.json"),
    "utf8"
  )
)

for (const [name, frames] of Object.entries(ORACLE.keyframes)) defineKeyframes(name, frames)

const NUMBER = /-?\d*\.?\d+(?:e-?\d+)?/g
const numbers = (text: string): number[] => (text.match(NUMBER) ?? []).map(Number)

/** The value of attribute `name` on the element with `id` in the painted XML, if it has one. */
function attributeOf(markup: string, id: string, name: string): string | undefined {
  const tag = new RegExp(`<[\\w]+\\s[^>]*\\bid="${id}"[^>]*>`).exec(markup)
  if (!tag) throw new Error(`no element with id ${id} in ${markup}`)
  const value = new RegExp(`\\s${name}="([^"]*)"`).exec(tag[0])
  return value ? value[1] : undefined
}

/** `got` is what gpuix wrote and `want` what Chrome computed, compared as the kind of value `prop` holds. */
function expectSame(prop: string, got: string, want: string, where: string) {
  // The keyword a property is before it swaps to its value, or after.
  if (want === "none" || want === "auto") return expect(got, where).toBe(want)
  // Chrome reads the used size of a shape whose width or height is `auto`, which is 0.
  if ((prop === "width" || prop === "height") && got === "auto") return expect(Number.parseFloat(want), where).toBe(0)
  if (["fill", "stroke", "stop-color", "flood-color"].includes(prop)) {
    if (want === "none" || got === "none") return expect(got, where).toBe(want)
    const [gotChannels, wantChannels] = [numbers(got), numbers(want)]
    // Chrome rounds a channel to a whole number.
    gotChannels
      .slice(0, 3)
      .forEach((channel, index) => expect(Math.abs(channel - wantChannels[index]), `${where}: ${got} against ${want}`).toBeLessThanOrEqual(1))
    return expect(gotChannels[3] ?? 1, `${where} alpha`).toBeCloseTo(wantChannels[3] ?? 1, 1)
  }
  if (prop === "d") {
    expect(got.replace(NUMBER, ""), `${where} commands`).toBe(want.replace(/^path\("/, "").replace(/"\)$/, "").replace(NUMBER, ""))
  }
  // Chrome serialises a letter spacing of 0 as `normal`.
  if (prop === "letter-spacing" && want === "normal") return expect(got === "normal" || Number(got) === 0, where).toBe(true)
  const [gotNumbers, wantNumbers] = [numbers(got), numbers(want)]
  expect(gotNumbers.length, `${where}: ${got} against ${want}`).toBe(wantNumbers.length)
  wantNumbers.forEach((number, index) => expect(gotNumbers[index], `${where}: ${got} against ${want}`).toBeCloseTo(number, 1))
}

const FRAME = { width: "100%", height: "100%", backgroundColor: "#000000" } as const

function toElement(spec: NodeSpec, ref: (node: PublicInstance | null) => void, animate: string | null): ReactNode {
  const { text, ...attrs } = spec.attrs
  const children = typeof text === "string" ? [text] : spec.children.map((child) => toElement(child, ref, animate))
  return createElement(
    spec.tag,
    {
      key: spec.id,
      id: spec.id,
      ...attrs,
      style: spec.style,
      ref: spec.id === animate ? ref : undefined,
    },
    ...children
  )
}

describeNative("SVG keyframes against Chrome", () => {
  for (const scenario of ORACLE.cases) {
    it(scenario.name, { timeout: 30_000 }, () => {
      const { render, renderer, unmount } = createTestRoot()
      try {
        renderer.clockPause()
        const target: { current: PublicInstance | null } = { current: null }
        const ref = (node: PublicInstance | null) => {
          target.current = node
        }
        render(
          <div style={FRAME}>
            <svg
              id="svg"
              testId="svg"
              width={200}
              height={100}
              viewBox="0 0 200 100"
              style={{ width: 200, height: 100, ...scenario.svg.style }}
              ref={scenario.animate === "svg" ? ref : undefined}
            >
              {scenario.nodes.map((node) => toElement(node, ref, scenario.animate))}
            </svg>
          </div>
        )
        const frame = () => {
          renderer.clockFastForward(0)
          renderer.dispatchNativeEvents()
        }
        frame()
        const svg = renderer.findByTestId("svg")!.id
        const animations = target.current ? target.current.getAnimations() : []
        expect(
          animations.map((animation) => (animation as CSSAnimation).animationName),
          "animations"
        ).toEqual(scenario.record.animations)

        scenario.record.samples.forEach((sample) => {
          if (scenario.animate) {
            for (const animation of animations) {
              animation.pause()
              animation.currentTime = sample.time
            }
          }
          frame()
          const markup = renderer.getSvgMarkup(svg)
          if (markup === null) throw new Error("the svg painted no shapes")
          scenario.reads.forEach((read, index) => {
            const written = attributeOf(markup, read.at ?? read.node, read.prop)
            expectSame(
              read.prop,
              written ?? read.initial,
              sample.values[index],
              `${scenario.name} at ${sample.time}ms, ${read.prop}`
            )
          })
        })
      } finally {
        unmount()
      }
    })
  }
})

defineKeyframes("svg-kf-draw", { from: { strokeDashoffset: 100 }, to: { strokeDashoffset: 0 } })
defineKeyframes("svg-kf-grow", { from: { r: 2 }, to: { r: 10 } })

describeNative("SVG keyframes in gpuix", () => {
  const frame = (renderer: ReturnType<typeof createTestRoot>["renderer"]) => {
    renderer.dispatchNativeEvents()
  }

  it("paints the dash a keyframe moves", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    fs.mkdirSync(SHOTS_DIR, { recursive: true })
    const file = path.join(SHOTS_DIR, "svg-keyframes-draw.png")
    render(
      <div style={FRAME}>
        <svg viewBox="0 0 100 100" style={{ width: 100, height: 100, color: "#ffffff" }}>
          <path
            d="M0 50 L100 50"
            stroke="currentColor"
            strokeWidth={20}
            strokeDasharray={100}
            style={{ animation: "svg-kf-draw 1s linear forwards" }}
          />
        </svg>
      </div>
    )
    const brightness = (x: number) => {
      frame(renderer)
      renderer.captureScreenshot(file)
      return readLogicalPixels(file, renderer).at(x, 50)[0]
    }
    expect(brightness(10)).toBeLessThan(40)
    renderer.clockFastForward(500)
    expect(brightness(25)).toBeGreaterThan(200)
    expect(brightness(75)).toBeLessThan(40)
    renderer.clockFastForward(1000)
    expect(brightness(75)).toBeGreaterThan(200)
  })

  it("beats the motion of the same shape", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    render(
      <div style={FRAME}>
        <svg id="svg" testId="svg" width={100} height={100} viewBox="0 0 100 100">
          <motion.circle
            id="c"
            cx={50}
            cy={50}
            initial={{ r: 40, strokeWidth: 3 }}
            animate={{ r: 40, strokeWidth: 3 }}
            style={{ animation: "svg-kf-grow 1s linear forwards" }}
          />
        </svg>
      </div>
    )
    renderer.clockFastForward(0)
    renderer.clockFastForward(500)
    frame(renderer)
    const markup = renderer.getSvgMarkup(renderer.findByTestId("svg")!.id)!
    expect(attributeOf(markup, "c", "r")).toBe("6")
    expect(attributeOf(markup, "c", "stroke-width")).toBe("3")
  })

  it("dispatches animationstart and animationend from the shape and lists the animation on it", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    const seen: string[] = []
    const record = (event: HostAnimationEvent) => seen.push(`${event.type}:${event.target === event.currentTarget ? "shape" : "svg"}`)
    const instance: { current: PublicInstance | null } = { current: null }
    render(
      <div style={FRAME}>
        <svg id="svg" testId="svg" width={100} height={100} viewBox="0 0 100 100" onAnimationEnd={(event: HostAnimationEvent) => seen.push(`bubbled:${event.animationName}`)}>
          <circle
            id="c"
            cx={50}
            cy={50}
            ref={(node: PublicInstance | null) => {
              instance.current = node
            }}
            style={{ animation: "svg-kf-grow 1s linear" }}
            onAnimationStart={record}
            onAnimationEnd={record}
          />
        </svg>
      </div>
    )
    frame(renderer)
    const [animation] = instance.current!.getAnimations()
    expect(animation).toBeInstanceOf(CSSAnimation)
    expect(animation.playState).toBe("running")
    renderer.clockFastForward(500)
    frame(renderer)
    expect(animation.currentTime).toBeCloseTo(500, 0)
    renderer.clockFastForward(1000)
    frame(renderer)
    expect(seen).toEqual(["animationstart:shape", "animationend:shape", "bubbled:svg-kf-grow"])
    expect(instance.current!.getAnimations()).toEqual([])
  })

  it("keeps the shape's animation moving through frames the svg alone would skip", () => {
    const { render, renderer } = createTestRoot()
    renderer.clockPause()
    render(
      <div style={FRAME}>
        <svg id="svg" testId="svg" width={100} height={100} viewBox="0 0 100 100">
          <circle id="c" cx={50} cy={50} style={{ animation: "svg-kf-grow 1s linear infinite" }} />
        </svg>
      </div>
    )
    const svg = renderer.findByTestId("svg")!.id
    const radii: number[] = []
    for (let step = 0; step < 4; step++) {
      frame(renderer)
      radii.push(Number(attributeOf(renderer.getSvgMarkup(svg)!, "c", "r")))
      renderer.clockFastForward(250)
    }
    expect(radii[1]).toBeCloseTo(4, 3)
    expect(radii[2]).toBeCloseTo(6, 3)
    expect(radii[3]).toBeCloseTo(8, 3)
  })

  it("refuses a keyframe set that animates an attribute no CSS property names", () => {
    expect(() => defineKeyframes("svg-kf-bad", { to: { pathLength: 1 } } as never)).toThrow(/cannot be animated/)
    expect(() => defineKeyframes("svg-kf-bad", { to: { x1: 1 } } as never)).toThrow(/cannot be animated/)
  })
})
