import { createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import {
  createTestRoot,
  hasNativeTestRenderer,
  type A11yTreeDump,
} from "../testing.js"

function keyOf(tree: A11yTreeDump, authorId: string): string {
  const found = Object.entries(tree.nodes ?? {}).find(
    ([, node]) => node.aria?.author_id === authorId
  )
  if (!found) throw new Error(`no a11y node with author id ${authorId}`)
  return found[0]
}

function byAuthorId(tree: A11yTreeDump, authorId: string) {
  const aria = tree.nodes?.[keyOf(tree, authorId)]?.aria
  if (!aria) throw new Error(`no a11y node with author id ${authorId}`)
  return aria
}

describe.skipIf(!hasNativeTestRenderer)("Solid accessibility", () => {
  it("maps id and any aria-* prop, and follows signal updates", () => {
    const [pressed, setPressed] = createSignal<boolean | "mixed">(false)
    const app = createTestRoot()
    app.render(() => (
      <div
        id="toggle"
        role="button"
        aria-pressed={pressed()}
        aria-live="polite"
        aria-keyshortcuts="Space"
        aria-posinset={2}
        style={{ width: 80, height: 24 }}
      />
    ))

    expect(byAuthorId(app.renderer.getA11yTree(), "toggle")).toEqual(
      expect.objectContaining({
        toggled: "False",
        live: "Polite",
        keyboard_shortcut: "Space",
        position_in_set: 2,
      })
    )
    app.flushSync(() => setPressed("mixed"))
    expect(byAuthorId(app.renderer.getA11yTree(), "toggle").toggled).toBe("Mixed")
  })

  it("resolves aria id references by DOM id", () => {
    const app = createTestRoot()
    app.render(() => (
      <div style={{ width: 300, height: 100 }}>
        <div
          id="field"
          role="textbox"
          aria-labelledby="title"
          aria-describedby="hint"
          style={{ width: 80, height: 24 }}
        />
        <text id="title" style={{ height: 20 }}>Title</text>
        <div id="hint" role="note" style={{ width: 80, height: 20 }} />
      </div>
    ))

    const tree = app.renderer.getA11yTree()
    expect(byAuthorId(tree, "field")).toEqual(
      expect.objectContaining({
        labelled_by: [keyOf(tree, "title")],
        described_by: [keyOf(tree, "hint")],
      })
    )
  })

  it("gives a referenced role-less element a node named by its text", () => {
    const app = createTestRoot()
    app.render(() => (
      <div style={{ width: 300, height: 100 }}>
        <div id="field" role="textbox" aria-labelledby="title" style={{ width: 80, height: 24 }} />
        <div id="title" style={{ width: 120, height: 20 }}>
          <text>Name</text>
        </div>
      </div>
    ))

    const tree = app.renderer.getA11yTree()
    expect(byAuthorId(tree, "title")).toEqual(
      expect.objectContaining({ role: "Group", label: "Name" })
    )
    expect(byAuthorId(tree, "field").labelled_by).toEqual([keyOf(tree, "title")])
  })

  it("hides an aria-hidden subtree", () => {
    const app = createTestRoot()
    app.render(() => (
      <div aria-hidden={true} style={{ width: 100, height: 40 }}>
        <div role="button" style={{ width: 40, height: 20 }} />
      </div>
    ))

    const nodes = Object.values(app.renderer.getA11yTree().nodes ?? {})
    expect(nodes.some((node) => node.aria?.hidden)).toBe(true)
    const button = nodes.find((node) => node.aria?.role === "Button")
    expect(button?.aria?.hidden).toBe(true)
  })

  it("names an <input> from <label htmlFor> and forwards the click", () => {
    const clicks: number[] = []
    const app = createTestRoot()
    app.render(() => (
      <div style={{ width: 300, height: 120, display: "flex", flexDirection: "column" }}>
        <label htmlFor="email" id="email-label" testId="label" style={{ width: 200, height: 30 }}>
          Email
        </label>
        <input
          id="email"
          testId="email-input"
          value=""
          onClick={() => clicks.push(1)}
          style={{ width: 200, height: 32 }}
        />
      </div>
    ))

    const tree = app.renderer.getA11yTree()
    expect(byAuthorId(tree, "email-label").value).toBe("Email")
    expect(byAuthorId(tree, "email").labelled_by).toEqual([
      keyOf(tree, "email-label"),
    ])

    const label = app.renderer.findByTestId("label")!
    const input = app.renderer.findByTestId("email-input")!
    const bounds = app.renderer.getElementBounds(label.id)!
    app.renderer.nativeSimulateClick(bounds.x + 10, bounds.y + 10)
    expect(clicks).toHaveLength(1)
    expect(app.renderer.getFocusedElementId()).toBe(input.id)
  })

  it("labels the first labelable descendant of a <label>", () => {
    const app = createTestRoot()
    app.render(() => (
      <label style={{ width: 300, height: 40 }}>
        Notes
        <textarea id="notes" value="" style={{ width: 200, height: 32 }} />
      </label>
    ))

    const tree = app.renderer.getA11yTree()
    const labelled = byAuthorId(tree, "notes").labelled_by ?? []
    expect(labelled).toHaveLength(1)
    expect(tree.nodes?.[labelled[0] ?? ""]?.aria?.value).toBe("Notes")
  })

  it("does nothing when the click lands on another control inside the label", () => {
    const clicks: string[] = []
    const app = createTestRoot()
    app.render(() => (
      <label style={{ width: 300, height: 80, display: "flex", flexDirection: "column" }}>
        <input testId="a" value="" onClick={() => clicks.push("a")} style={{ width: 200, height: 30 }} />
        <input testId="b" value="" onClick={() => clicks.push("b")} style={{ width: 200, height: 30 }} />
      </label>
    ))
    const b = app.renderer.findByTestId("b")!
    const bounds = app.renderer.getElementBounds(b.id)!
    app.renderer.nativeSimulateClick(bounds.x + 10, bounds.y + 10)
    expect(clicks).toEqual(["b"])
    expect(app.renderer.getFocusedElementId()).toBe(b.id)
  })

  it("label.click() focuses and clicks the control unless the label's click is cancelled", () => {
    for (const cancel of [false, true]) {
      const clicks: Array<{ detail?: number; pointerId?: number }> = []
      let label!: { id: number; click(): void }
      const app = createTestRoot()
      app.render(() => (
        <div style={{ width: 300, height: 120 }}>
          <label
            htmlFor="name"
            ref={label}
            onClick={(event) => {
              if (cancel) event.preventDefault()
            }}
            style={{ width: 200, height: 30 }}
          >
            Name
          </label>
          <input
            id="name"
            testId="name-input"
            value=""
            onClick={(event) => clicks.push({ detail: event.detail, pointerId: event.pointerId })}
            style={{ width: 200, height: 32 }}
          />
        </div>
      ))
      app.renderer.flush()
      label.click()
      app.renderer.dispatchNativeEvents()
      const input = app.renderer.findByTestId("name-input")!
      if (cancel) {
        expect(clicks).toEqual([])
        expect(app.renderer.getFocusedElementId()).not.toBe(input.id)
      } else {
        expect(clicks).toEqual([{ detail: 0, pointerId: -1 }])
        expect(app.renderer.getFocusedElementId()).toBe(input.id)
      }
    }
  })
})
