/// The accessibility tree for the content that `content-visibility` skips. Chrome keeps the content of a skipped
/// `auto` element in the accessibility tree; `hidden` content is not. What the tree holds is held to headless Chrome
/// (`fixtures/content-visibility-a11y-oracle.*`, regenerate with `bun fixtures/content-visibility-a11y-oracle.ts`).

import fs from "fs"
import path from "path"
import type { CSSProperties, ReactNode } from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer, type A11yTreeDump, type TestRenderer } from "../testing.js"
import { verifying } from "./retained-harness.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const FIXTURES = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures")

const SIZE = { width: 900, height: 780 }
const ENTRIES = 40
const ENTRY = 100

type Aria = NonNullable<NonNullable<A11yTreeDump["nodes"]>[string]["aria"]>

/** The nodes of a dump with their keys. */
function nodesOf(tree: A11yTreeDump) {
  return Object.entries(tree.nodes ?? {}).flatMap(([key, node]) => (node.aria ? [{ key, aria: node.aria, children: node.children ?? [] }] : []))
}

const withAuthor = (tree: A11yTreeDump, authorId: string) => nodesOf(tree).filter((node) => node.aria.author_id === authorId)
const withLabel = (tree: A11yTreeDump, label: string) => nodesOf(tree).filter((node) => node.aria.label === label)
const withValue = (tree: A11yTreeDump, value: string) => nodesOf(tree).filter((node) => node.aria.value === value)
const roleOf = (aria: Aria) => aria.role

interface EntryProps {
  index: number
  value?: "auto" | "hidden"
  children?: ReactNode
}

/** An entry of 100px: a heading, a button that Tab visits and a line of text. */
function Entry({ index, value = "auto", children }: EntryProps) {
  return (
    <div
      testId={`entry-${index}`}
      style={{ height: ENTRY, flexShrink: 0, contentVisibility: value, containIntrinsicSize: `auto ${ENTRY}px` } as CSSProperties}
    >
      <div role="heading" aria-level={2} aria-label={`Title ${index}`} style={{ height: 24 }} />
      <div role="button" id={`btn-${index}`} aria-label={`Go ${index}`} testId={`btn-${index}`} tabIndex={0} style={{ height: 24 }} />
      <text style={{ height: 20 }}>Paragraph {index}</text>
      {children}
    </div>
  )
}

function Page({ entry, before }: { entry?: (index: number) => ReactNode; before?: ReactNode }) {
  return (
    <div style={{ width: SIZE.width, height: SIZE.height, display: "flex", flexDirection: "column", overflow: "hidden" }}>
      {before}
      <div testId="scroller" style={{ overflowY: "scroll", flexGrow: 1, display: "flex", flexDirection: "column" }}>
        {Array.from({ length: ENTRIES }, (_, index) => (entry ? entry(index) : <Entry key={index} index={index} />))}
      </div>
    </div>
  )
}

function mount(node: ReactNode) {
  const root = createTestRoot(SIZE)
  const renderer = root.renderer
  renderer.setInlineDraws(false)
  root.render(node)
  renderer.flush()
  const ids = new Map<string, number>()
  const index = () => {
    ids.clear()
    for (const element of renderer.findByType("div")) if (element.testId) ids.set(element.testId, element.id)
  }
  index()
  settle(renderer)
  return { root, renderer, ids, index }
}

/** Ends a test: with retention verified, no replayed frame differed from a build of it. */
function finish(root: ReturnType<typeof createTestRoot>, renderer: TestRenderer) {
  try {
    if (verifying) expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
  } finally {
    root.unmount()
  }
}

/** Draws until the frames stop changing what is built: a change takes effect in the frame after the one that found it. */
function settle(renderer: TestRenderer) {
  for (let frames = 0; frames < 8; frames++) {
    renderer.drawFrame()
    renderer.dispatchNativeEvents()
  }
}

const isBuilt = (renderer: TestRenderer, ids: Map<string, number>, name: string) => Boolean(renderer.getBuiltElementBounds(ids.get(name)!))

describeNative("the accessibility tree of skipped content", { timeout: 120_000 }, () => {
  it("has the heading, the button and the text of a far skipped auto entry", () => {
    const { root, renderer, ids } = mount(<Page />)
    try {
      expect(isBuilt(renderer, ids, "btn-3"), "an entry near the viewport is built").toBe(true)
      expect(isBuilt(renderer, ids, "btn-30"), "a far entry is skipped").toBe(false)
      const tree = renderer.getA11yTree()
      const heading = withLabel(tree, "Title 30")
      expect(heading.map((node) => roleOf(node.aria))).toEqual(["Heading"])
      expect(heading[0]!.aria.level).toBe(2)
      expect(withAuthor(tree, "btn-30").map((node) => [roleOf(node.aria), node.aria.label])).toEqual([["Button", "Go 30"]])
      expect(withValue(tree, "Paragraph 30").map((node) => roleOf(node.aria))).toEqual(["Label"])
      // In document order, under a node that stands for the entry.
      const entry = nodesOf(tree).find((node) => node.children.includes(heading[0]!.key))!
      expect(entry.aria.role).toBe("Group")
      expect(entry.children.map((key) => tree.nodes![key]!.aria!.role)).toEqual(["Heading", "Button", "Label"])
    } finally {
      finish(root, renderer)
    }
  })

  it("has the headings and buttons Chrome has with a screen reader on, and none from a hidden entry", () => {
    const { root, renderer } = mount(<Page entry={(index) => <Entry key={index} index={index} value={index === 31 ? "hidden" : "auto"} />} />)
    try {
      const tree = renderer.getA11yTree()
      const named = (role: string) =>
        nodesOf(tree)
          .filter((node) => node.aria.role === role && node.aria.label)
          .map((node) => node.aria.label!)
      const headings = named("Heading")
      const buttons = named("Button")
      expect(headings.sort()).toEqual([...ORACLE.ax.headings].sort())
      // Chrome names a button by its content or label; the oracle names them `Go n`.
      expect(buttons.sort()).toEqual([...ORACLE.ax.buttons].sort())
    } finally {
      finish(root, renderer)
    }
  })

  it("has nothing from a hidden entry but the entry", () => {
    const { root, renderer } = mount(<Page entry={(index) => <Entry key={index} index={index} value={index === 30 ? "hidden" : "auto"} />} />)
    try {
      const tree = renderer.getA11yTree()
      expect(withLabel(tree, "Title 30")).toEqual([])
      expect(withAuthor(tree, "btn-30")).toEqual([])
      expect(withValue(tree, "Paragraph 30")).toEqual([])
      expect(withLabel(tree, "Title 31")).toHaveLength(1)
    } finally {
      finish(root, renderer)
    }
  })

  it("keeps the role of the entry and hangs the nodes off it", () => {
    const { root, renderer } = mount(
      <Page
        entry={(index) => (
          <div key={index} role="region" aria-label={`Region ${index}`} style={{ height: ENTRY, flexShrink: 0, contentVisibility: "auto", containIntrinsicSize: `auto ${ENTRY}px` } as CSSProperties}>
            <div role="button" id={`btn-${index}`} aria-label={`Go ${index}`} style={{ height: 24 }} />
          </div>
        )}
      />
    )
    try {
      const tree = renderer.getA11yTree()
      const region = withLabel(tree, "Region 30")
      expect(region.map((node) => roleOf(node.aria))).toEqual(["Region"])
      expect(region[0]!.children).toEqual([withAuthor(tree, "btn-30")[0]!.key])
    } finally {
      finish(root, renderer)
    }
  })

  it("leaves out what a built entry leaves out, and goes down through what it does not: aria-hidden, display none, visibility, a hidden section and a nested auto one", () => {
    const { root, renderer } = mount(
      <Page
        entry={(index) => (
          <div key={index} style={{ height: ENTRY, flexShrink: 0, contentVisibility: "auto", containIntrinsicSize: `auto ${ENTRY}px` } as CSSProperties}>
            <div role="button" aria-label={`Plain ${index}`} style={{ height: 10 }} />
            <div aria-hidden="true">
              <div role="button" aria-label={`Aria hidden ${index}`} style={{ height: 10 }} />
            </div>
            <div style={{ display: "none" }}>
              <div role="button" aria-label={`Display none ${index}`} />
            </div>
            <div style={{ visibility: "hidden" }}>
              <div role="button" aria-label={`Visibility hidden ${index}`} style={{ height: 10 }} />
              <div role="button" aria-label={`Visible again ${index}`} style={{ height: 10, visibility: "visible" }} />
            </div>
            <div style={{ contentVisibility: "hidden", height: 10 } as CSSProperties}>
              <div role="button" aria-label={`In a hidden section ${index}`} />
            </div>
            <div style={{ contentVisibility: "auto", height: 20, overflow: "hidden" } as CSSProperties}>
              <div style={{ paddingLeft: 4 }}>
                <div role="button" aria-label={`In a nested auto section ${index}`} style={{ height: 10 }} />
              </div>
            </div>
          </div>
        )}
      />
    )
    try {
      const tree = renderer.getA11yTree()
      const labels = nodesOf(tree)
        .map((node) => node.aria.label)
        .filter((label): label is string => Boolean(label?.endsWith(" 30")))
      expect(labels).toEqual(["Plain 30", "Visible again 30", "In a nested auto section 30"])
    } finally {
      finish(root, renderer)
    }
  })

  it("names a control in skipped content for aria-labelledby from outside, and takes the name of a target in it", () => {
    const { root, renderer } = mount(
      <Page
        before={
          <>
            <div role="button" id="outside" aria-labelledby="btn-30" style={{ height: 10, flexShrink: 0 }} />
            <div role="button" id="outside-by-text" aria-labelledby="name-30" style={{ height: 10, flexShrink: 0 }} />
          </>
        }
        entry={(index) => (
          <Entry key={index} index={index}>
            {index === 30 ? <div id="name-30">Written in the entry</div> : null}
          </Entry>
        )}
      />
    )
    try {
      const tree = renderer.getA11yTree()
      const outside = withAuthor(tree, "outside")[0]!
      expect(outside.aria.labelled_by).toEqual([withAuthor(tree, "btn-30")[0]!.key])
      const byText = withAuthor(tree, "outside-by-text")[0]!
      const target = withAuthor(tree, "name-30")[0]!
      expect(byText.aria.labelled_by).toEqual([target.key])
      expect(target.aria.label).toBe("Written in the entry")
    } finally {
      finish(root, renderer)
    }
  })

  it("follows a change below the skipped element", () => {
    const render = (title: string) => (
      <Page
        entry={(index) => (
          <Entry key={index} index={index}>
            {index === 30 ? <div id="name-30">{title}</div> : null}
          </Entry>
        )}
        before={<div role="button" id="outside-by-text" aria-labelledby="name-30" style={{ height: 10, flexShrink: 0 }} />}
      />
    )
    const { root, renderer } = mount(render("Before"))
    try {
      expect(withAuthor(renderer.getA11yTree(), "name-30")[0]!.aria.label).toBe("Before")
      root.render(render("After"))
      renderer.flush()
      settle(renderer)
      expect(withAuthor(renderer.getA11yTree(), "name-30")[0]!.aria.label).toBe("After")
    } finally {
      finish(root, renderer)
    }
  })

  it("is in the tree once when the entry is built, and again when it is skipped", () => {
    const { root, renderer, ids } = mount(<Page />)
    try {
      const count = () => {
        const tree = renderer.getA11yTree()
        return [withAuthor(tree, "btn-30").length, withLabel(tree, "Title 30").length, withValue(tree, "Paragraph 30").length]
      }
      expect(count()).toEqual([1, 1, 1])
      renderer.scrollTo(ids.get("scroller")!, 0, 3000)
      settle(renderer)
      expect(isBuilt(renderer, ids, "btn-30"), "scrolled to").toBe(true)
      expect(count()).toEqual([1, 1, 1])
      renderer.scrollTo(ids.get("scroller")!, 0, 0)
      settle(renderer)
      expect(isBuilt(renderer, ids, "btn-30"), "scrolled away").toBe(false)
      expect(count()).toEqual([1, 1, 1])
    } finally {
      finish(root, renderer)
    }
  })

  it("is made when the accessibility tree turns on after the page, and is gone when it turns off", () => {
    const { root, renderer } = mount(<Page />)
    try {
      renderer.setA11yActive(false)
      settle(renderer)
      renderer.setA11yActive(true)
      settle(renderer)
      expect(withAuthor(renderer.getA11yTree(), "btn-30")).toHaveLength(1)
      expect(withValue(renderer.getA11yTree(), "Paragraph 30")).toHaveLength(1)
    } finally {
      finish(root, renderer)
    }
  })

  it("reads out a skipped <text>", () => {
    const { root, renderer } = mount(
      <Page
        entry={(index) => (
          <text
            key={index}
            testId={`entry-${index}`}
            style={{ height: ENTRY, flexShrink: 0, contentVisibility: "auto", containIntrinsicSize: `auto ${ENTRY}px` } as CSSProperties}
          >
            Line {index}
          </text>
        )}
      />
    )
    try {
      const tree = renderer.getA11yTree()
      expect(withValue(tree, "Line 3").map((node) => roleOf(node.aria))).toEqual(["Label"])
      expect(withValue(tree, "Line 30").map((node) => roleOf(node.aria))).toEqual(["Label"])
    } finally {
      finish(root, renderer)
    }
  })
})

type AxOracle = { headings: string[]; buttons: string[] }

const ORACLE: { ax: AxOracle } = JSON.parse(fs.readFileSync(path.join(FIXTURES, "content-visibility-a11y-oracle.json"), "utf8"))
