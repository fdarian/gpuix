/// The markup of an oracle page (`fixtures/*-oracle.html`) whose cases are boxes: `div`s, `span`s, `img`s and the table
/// elements, as the elements a gpuix tree has for them, and the boxes Chrome gave every element with an `id`.
/// The `id` of an element is its `testId`. A table element becomes the `div` with the `display` the user agent
/// stylesheet gives it, and `colspan`, `rowspan` and `span` become the props of the same name.

import React from "react"
import { expect } from "vitest"
import { motion } from "../index"
import { styleOf } from "./oracle-html.js"
import type { createTestRoot } from "../testing.js"

const DISPLAY: Record<string, string> = {
  table: "table",
  thead: "table-header-group",
  tbody: "table-row-group",
  tfoot: "table-footer-group",
  tr: "table-row",
  td: "table-cell",
  th: "table-cell",
  caption: "table-caption",
  col: "table-column",
  colgroup: "table-column-group",
}

const VOID = new Set(["col", "img"])
const COUNTS: Record<string, string> = { colspan: "colSpan", rowspan: "rowSpan", span: "span" }

interface Node {
  tag: string
  attrs: Record<string, string>
  children: (Node | string)[]
}

function parse(html: string): (Node | string)[] {
  const root: Node = { tag: "", attrs: {}, children: [] }
  const open: Node[] = [root]
  for (const token of html.split(/(<[^>]+>)/).filter(Boolean)) {
    const tag = /^<(\/?)([a-z]+)([^>]*?)(\/?)>$/.exec(token)
    if (!tag) {
      open.at(-1)!.children.push(token)
      continue
    }
    const [, closing, name, rest] = tag
    if (closing) {
      open.pop()
      continue
    }
    const attrs: Record<string, string> = {}
    for (const attr of rest!.matchAll(/([a-z-]+)(?:="([^"]*)"|=([^\s"]+))?/g)) attrs[attr[1]!] = attr[2] ?? attr[3] ?? ""
    const node: Node = { tag: name!, attrs, children: [] }
    open.at(-1)!.children.push(node)
    if (!VOID.has(name!)) open.push(node)
  }
  return root.children
}

/** Style to lay over the element with this `id`. */
export interface Override {
  id: string
  style: Record<string, unknown>
  /** The element is a `motion.div` with these props. */
  motion?: Record<string, unknown>
}

/** Props to put on the element with this `id`, such as the handlers of a hit test. */
export type Extra = (id: string) => Record<string, unknown>

function render(nodes: (Node | string)[], override: Override | undefined, extra: Extra | undefined, key = ""): React.ReactNode[] {
  return nodes.map((node, index) => {
    if (typeof node === "string") return node
    const props: Record<string, unknown> = { key: `${key}${index}` }
    if (node.attrs.id) {
      props.testId = node.attrs.id
      Object.assign(props, extra?.(node.attrs.id))
    }
    for (const [name, prop] of Object.entries(COUNTS)) if (node.attrs[name]) props[prop] = Number(node.attrs[name])
    const display = DISPLAY[node.tag]
    const style = {
      ...(display ? { display } : {}),
      ...styleOf(node.attrs.style ?? ""),
      ...(override && node.attrs.id === override.id ? override.style : {}),
    }
    if (node.tag === "img") {
      const size = { width: Number(node.attrs.width), height: Number(node.attrs.height) }
      return React.createElement("img", { ...props, src: node.attrs.src, style: { ...style, ...size } })
    }
    const Tag = node.tag === "span" ? "span" : "div"
    if (override?.motion && node.attrs.id === override.id) {
      return React.createElement(motion.div, { ...props, ...override.motion, style }, ...render(node.children, override, extra, `${key}${index}.`))
    }
    return React.createElement(Tag, { ...props, style }, ...render(node.children, override, extra, `${key}${index}.`))
  })
}

/** The elements of `html`. */
export function elementsOf(html: string, override?: Override, extra?: Extra): React.ReactNode[] {
  return render(parse(html), override, extra)
}

type Rect = [number, number, number, number]

/** Every box in `boxes` is where Chrome has it, to `tolerance` px. */
export function expectBoxes(
  renderer: ReturnType<typeof createTestRoot>["renderer"],
  boxes: Record<string, Rect>,
  tolerance = 0.06,
  origin: Rect = [0, 0, 0, 0]
) {
  for (const [id, want] of Object.entries(boxes)) {
    const element = renderer.findByTestId(id)
    expect(element, `${id} is mounted`).toBeDefined()
    const bounds = renderer.getElementBounds(element!.id)
    expect(bounds, `${id} has a box`).not.toBeNull()
    const got: Rect = [bounds!.x - origin[0], bounds!.y - origin[1], bounds!.width, bounds!.height]
    want.forEach((value, index) => {
      expect(
        Math.abs(got[index]! - value),
        `${id} ${["x", "y", "width", "height"][index]}: got [${got.map((n) => +n.toFixed(2))}] want [${want}]`
      ).toBeLessThanOrEqual(tolerance)
    })
  }
}
