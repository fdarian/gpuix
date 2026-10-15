/// GPUI accessibility tree from React `role` and `aria-*` props.
///
/// A node is in the tree only with both an id (always set) and a role.
/// These tests dump GPUI's debug tree after paint, not screenshots.

import fs from "fs"
import path from "path"
import React, { useState } from "react"
import { beforeEach, describe, expect, it } from "vitest"
import {
  createTestRoot,
  hasNativeTestRenderer,
  type A11yTreeDump,
  type TestRoot,
} from "../testing.js"
import { SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type A11yNode = NonNullable<NonNullable<A11yTreeDump["nodes"]>[string]["aria"]>

function ariaOf(tree: A11yTreeDump): A11yNode[] {
  return Object.values(tree.nodes ?? {})
    .map((node) => node.aria)
    .filter((aria): aria is A11yNode => aria != null)
}

function withRole(tree: A11yTreeDump, role: string): A11yNode[] {
  return ariaOf(tree).filter((aria) => aria.role === role)
}

/** The dump's ephemeral key for the node with this author id. */
function keyOf(tree: A11yTreeDump, authorId: string): string {
  const found = Object.entries(tree.nodes ?? {}).find(
    ([, node]) => node.aria?.author_id === authorId,
  )
  if (!found) throw new Error(`no a11y node with author id ${authorId}`)
  return found[0]
}

function byAuthorId(tree: A11yTreeDump, authorId: string): A11yNode {
  const aria = tree.nodes?.[keyOf(tree, authorId)]?.aria
  if (!aria) throw new Error(`no a11y node with author id ${authorId}`)
  return aria
}

describeNative("accessibility", () => {
  let testRoot: TestRoot

  beforeEach(() => {
    testRoot = createTestRoot()
  })

  it("exposes role, aria-label, and Click for an onClick button", () => {
    testRoot.render(
      <div
        role="button"
        aria-label="Delete note"
        id="notes.delete"
        onClick={() => {}}
        style={{ width: 120, height: 40 }}
      >
        Delete
      </div>,
    )

    const tree = testRoot.renderer.getA11yTree()
    const buttons = withRole(tree, "Button")
    expect(buttons).toEqual([
      expect.objectContaining({
        role: "Button",
        label: "Delete note",
        author_id: "notes.delete",
      }),
    ])
    expect(buttons[0]?.on_action).toEqual(expect.arrayContaining(["Click"]))
  })

  it("reports a <text> node as Label with its content", () => {
    testRoot.render(<text style={{ width: 200, height: 24 }}>Hello</text>)

    const labels = withRole(testRoot.renderer.getA11yTree(), "Label")
    expect(labels.some((aria) => aria.value === "Hello")).toBe(true)
  })

  it("omits a div with no role from the tree", () => {
    testRoot.render(
      <div style={{ width: 120, height: 40 }} aria-label="silent">
        Hello
      </div>,
    )

    const tree = testRoot.renderer.getA11yTree()
    expect(withRole(tree, "GenericContainer")).toEqual([])
    expect(ariaOf(tree).some((aria) => aria.label === "silent")).toBe(false)
  })

  it("drops role none and presentation", () => {
    testRoot.render(
      <div style={{ width: 200, height: 80 }}>
        <div role="none" aria-label="hidden none" style={{ width: 80, height: 24 }} />
        <div
          role="presentation"
          aria-label="hidden presentation"
          style={{ width: 80, height: 24 }}
        />
      </div>,
    )

    const labels = ariaOf(testRoot.renderer.getA11yTree()).map(
      (aria) => aria.label,
    )
    expect(labels).not.toContain("hidden none")
    expect(labels).not.toContain("hidden presentation")
  })

  it("passes aria-expanded, aria-selected, and aria-level", () => {
    testRoot.render(
      <div style={{ width: 240, height: 80 }}>
        <div
          role="button"
          aria-label="Folder"
          aria-expanded={true}
          style={{ width: 120, height: 24 }}
        />
        <div
          role="option"
          aria-label="One"
          aria-selected={true}
          style={{ width: 120, height: 24 }}
        />
        <div
          role="heading"
          aria-label="Title"
          aria-level={2}
          style={{ width: 120, height: 24 }}
        />
      </div>,
    )

    const tree = testRoot.renderer.getA11yTree()
    expect(withRole(tree, "Button")[0]).toEqual(
      expect.objectContaining({ label: "Folder", expanded: true }),
    )
    expect(withRole(tree, "ListBoxOption")[0]).toEqual(
      expect.objectContaining({ label: "One", selected: true }),
    )
    expect(withRole(tree, "Heading")[0]).toEqual(
      expect.objectContaining({ label: "Title", level: 2 }),
    )
  })

  it("reports <input> as TextInput and <textarea> as MultilineTextInput", () => {
    testRoot.render(
      <div style={{ width: 280, height: 120 }}>
        <input value="name" placeholder="Your name" style={{ width: 200, height: 32 }} />
        <textarea value="body" style={{ width: 200, height: 48 }} />
      </div>,
    )

    const tree = testRoot.renderer.getA11yTree()
    expect(withRole(tree, "TextInput").some((aria) => aria.value === "name")).toBe(
      true,
    )
    expect(
      withRole(tree, "MultilineTextInput").some((aria) => aria.value === "body"),
    ).toBe(true)
  })

  it("uses img alt as the accessible name", () => {
    const src = path.join(SHOTS_DIR, "gpuix-a11y-img.svg")
    fs.writeFileSync(
      src,
      '<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="#000"/></svg>',
    )
    testRoot.render(
      <img alt="Cat photo" src={src} style={{ width: 40, height: 40 }} />,
    )

    const images = withRole(testRoot.renderer.getA11yTree(), "Image")
    expect(images.some((aria) => aria.label === "Cat photo")).toBe(true)
  })

  it("keeps img alt when src is empty", () => {
    testRoot.render(<img alt="Empty source" src="" style={{ width: 40, height: 40 }} />)

    const images = withRole(testRoot.renderer.getA11yTree(), "Image")
    expect(images.some((aria) => aria.label === "Empty source")).toBe(true)
  })

  it("exposes role and aria-label on <anchored>", () => {
    testRoot.render(
      <anchored
        role="menu"
        aria-label="File menu"
        position={{ x: 8, y: 8 }}
        style={{ width: 80, height: 40 }}
      />,
    )

    expect(withRole(testRoot.renderer.getA11yTree(), "Menu")[0]).toEqual(
      expect.objectContaining({ role: "Menu", label: "File menu" }),
    )
  })

  it("exposes role and aria-label on <virtual-list>", () => {
    testRoot.render(
      <virtual-list
        role="list"
        aria-label="Messages"
        style={{ width: 200, height: 80 }}
      >
        <text>one</text>
      </virtual-list>,
    )

    expect(withRole(testRoot.renderer.getA11yTree(), "List")[0]).toEqual(
      expect.objectContaining({ role: "List", label: "Messages" }),
    )
  })

  it("joins interpolated <text> children into one Label", () => {
    const name = "Ada"
    testRoot.render(<text style={{ width: 200, height: 24 }}>Hello {name}!</text>)

    const labels = withRole(testRoot.renderer.getA11yTree(), "Label")
    expect(labels.filter((aria) => aria.value === "Hello Ada!")).toHaveLength(1)
  })

  it("lets an explicit role override the <text> default", () => {
    testRoot.render(
      <text role="heading" aria-level={1} style={{ width: 200, height: 24 }}>
        Title
      </text>,
    )

    const tree = testRoot.renderer.getA11yTree()
    expect(withRole(tree, "Heading")[0]).toEqual(
      expect.objectContaining({ role: "Heading", value: "Title", level: 1 }),
    )
  })

  it("updates aria-label when React state changes", () => {
    function Toggle() {
      const [on, setOn] = useState(false)
      return (
        <div
          role="button"
          aria-label={on ? "On" : "Off"}
          onClick={() => setOn(true)}
          style={{ width: 80, height: 32 }}
        />
      )
    }

    testRoot.render(<Toggle />)
    expect(withRole(testRoot.renderer.getA11yTree(), "Button")[0]?.label).toBe("Off")

    testRoot.renderer.nativeSimulateClick(10, 10)
    expect(withRole(testRoot.renderer.getA11yTree(), "Button")[0]?.label).toBe("On")
  })
  it("accepts any aria-* prop and maps states to AccessKit", () => {
    testRoot.render(
      <div style={{ width: 400, height: 200 }}>
        <div
          id="box"
          role="checkbox"
          aria-checked="mixed"
          aria-disabled
          aria-required
          aria-readonly
          aria-invalid="spelling"
          aria-busy
          aria-keyshortcuts="Ctrl+B"
          aria-roledescription="tri-state"
          aria-haspopup="listbox"
          aria-current="page"
          style={{ width: 80, height: 24 }}
        />
        <div
          id="toggle"
          role="button"
          aria-pressed={true}
          aria-expanded="false"
          style={{ width: 80, height: 24 }}
        />
        <div
          id="meter"
          role="slider"
          aria-valuenow={3}
          aria-valuemin="0"
          aria-valuemax={10}
          aria-valuetext="three"
          aria-orientation="vertical"
          style={{ width: 80, height: 24 }}
        />
        <div
          id="status"
          role="status"
          aria-live="assertive"
          aria-atomic
          aria-modal
          style={{ width: 80, height: 24 }}
        />
        <div
          id="cell"
          role="cell"
          aria-rowindex={2}
          aria-colindex={3}
          aria-rowspan={2}
          aria-colspan={4}
          aria-posinset={1}
          aria-setsize={9}
          aria-sort="ascending"
          aria-autocomplete="list"
          aria-multiselectable
          style={{ width: 80, height: 24 }}
        />
      </div>,
    )

    const tree = testRoot.renderer.getA11yTree()
    expect(byAuthorId(tree, "box")).toEqual(
      expect.objectContaining({
        toggled: "Mixed",
        disabled: true,
        required: true,
        read_only: true,
        invalid: "Spelling",
        busy: true,
        keyboard_shortcut: "Ctrl+B",
        role_description: "tri-state",
        has_popup: "Listbox",
        aria_current: "Page",
      }),
    )
    expect(byAuthorId(tree, "toggle")).toEqual(
      expect.objectContaining({ toggled: "True", expanded: false }),
    )
    expect(byAuthorId(tree, "meter")).toEqual(
      expect.objectContaining({
        numeric_value: 3,
        min_numeric_value: 0,
        max_numeric_value: 10,
        value: "three",
        orientation: "Vertical",
      }),
    )
    expect(byAuthorId(tree, "status")).toEqual(
      expect.objectContaining({ live: "Assertive", live_atomic: true, modal: true }),
    )
    expect(byAuthorId(tree, "cell")).toEqual(
      expect.objectContaining({
        row_index: 2,
        column_index: 3,
        row_span: 2,
        column_span: 4,
        position_in_set: 1,
        size_of_set: 9,
        sort_direction: "Ascending",
        auto_complete: "List",
        multiselectable: true,
      }),
    )
  })

  it("ignores values the attribute does not allow", () => {
    testRoot.render(
      <div
        id="odd"
        role="button"
        // @ts-expect-error not a valid token
        aria-live="loud"
        aria-disabled="false"
        style={{ width: 80, height: 24 }}
      />,
    )

    const odd = byAuthorId(testRoot.renderer.getA11yTree(), "odd")
    expect(odd.live).toBeUndefined()
    expect(odd.disabled).toBeUndefined()
  })

  it("uses the DOM id as the author id, and drops aria-id", () => {
    testRoot.render(
      <div style={{ width: 120, height: 40 }}>
        <div id="a" role="button" style={{ width: 40, height: 20 }} />
        <div
          // @ts-expect-error removed in favour of `id`
          aria-id="legacy"
          role="button"
          style={{ width: 40, height: 20 }}
        />
      </div>,
    )

    const ids = ariaOf(testRoot.renderer.getA11yTree()).map((aria) => aria.author_id)
    expect(ids).toContain("a")
    expect(ids).not.toContain("legacy")
  })

  it("keeps an aria-hidden subtree out of the tree even without a role", () => {
    testRoot.render(
      <div aria-hidden style={{ width: 120, height: 40 }}>
        <div role="button" aria-label="inside" style={{ width: 40, height: 20 }} />
      </div>,
    )

    const tree = testRoot.renderer.getA11yTree()
    const hidden = ariaOf(tree).filter((aria) => aria.hidden)
    expect(hidden).toHaveLength(1)
    const inside = Object.values(tree.nodes ?? {}).find(
      (node) => node.aria?.label === "inside",
    )
    expect(inside).toBeDefined()
    const parent = Object.values(tree.nodes ?? {}).find((node) =>
      node.children?.some((child) => tree.nodes?.[child]?.aria?.label === "inside"),
    )
    expect(parent?.aria?.hidden).toBe(true)
  })

  it("resolves aria id references by DOM id, in lists, forward and back", () => {
    testRoot.render(
      <div style={{ width: 400, height: 200 }}>
        <div
          id="field"
          role="textbox"
          aria-labelledby="title hint missing"
          aria-describedby="hint"
          aria-controls="list"
          aria-errormessage="error"
          aria-details="hint title"
          aria-flowto="list"
          aria-owns="title"
          style={{ width: 80, height: 24 }}
        />
        <text id="title" style={{ height: 20 }}>Title</text>
        <div id="hint" role="note" style={{ width: 80, height: 20 }} />
        <div id="list" role="listbox" style={{ width: 80, height: 20 }} />
        <div id="error" role="alert" style={{ width: 80, height: 20 }} />
      </div>,
    )

    const tree = testRoot.renderer.getA11yTree()
    const key = (id: string) => keyOf(tree, id)
    expect(byAuthorId(tree, "field")).toEqual(
      expect.objectContaining({
        labelled_by: [key("title"), key("hint")],
        described_by: [key("hint")],
        controls: [key("list")],
        error_message: key("error"),
        details: [key("hint"), key("title")],
        flow_to: [key("list")],
        owns: [key("title")],
      }),
    )
  })

  it("gives a role-less element an aria reference names a node named by its text", () => {
    testRoot.render(
      <div style={{ width: 300, height: 100 }}>
        <div id="field" role="textbox" aria-describedby="hint" style={{ width: 80, height: 24 }} />
        <div id="hint" style={{ width: 120, height: 20 }}>
          <text>Be brief</text>
        </div>
        <div id="unreferenced" style={{ width: 80, height: 20 }} />
      </div>,
    )

    const tree = testRoot.renderer.getA11yTree()
    expect(byAuthorId(tree, "hint")).toEqual(
      expect.objectContaining({ role: "Group", label: "Be brief" }),
    )
    expect(byAuthorId(tree, "field").described_by).toEqual([keyOf(tree, "hint")])
    expect(ariaOf(tree).some((aria) => aria.author_id === "unreferenced")).toBe(false)
  })

  it("points aria-activedescendant at the descendant's node", () => {
    testRoot.render(
      <div id="combo" role="combobox" aria-activedescendant="opt2" style={{ width: 120, height: 60 }}>
        <div id="opt1" role="option" style={{ width: 80, height: 20 }} />
        <div id="opt2" role="option" style={{ width: 80, height: 20 }} />
      </div>,
    )

    const tree = testRoot.renderer.getA11yTree()
    expect(byAuthorId(tree, "combo").active_descendant).toBe(keyOf(tree, "opt2"))
  })

  it("names an <input> from <label htmlFor>", () => {
    testRoot.render(
      <div style={{ width: 300, height: 120 }}>
        <label htmlFor="email" id="email-label" style={{ height: 20 }}>
          Email address
        </label>
        <input id="email" value="" style={{ width: 200, height: 32 }} />
      </div>,
    )

    const tree = testRoot.renderer.getA11yTree()
    const label = byAuthorId(tree, "email-label")
    expect(label.role).toBe("Label")
    expect(label.value).toBe("Email address")
    expect(byAuthorId(tree, "email").labelled_by).toEqual([keyOf(tree, "email-label")])
  })

  it("names the first labelable descendant of a <label> without htmlFor", () => {
    testRoot.render(
      <label style={{ width: 300, height: 40 }}>
        Notes
        <textarea id="notes" value="" style={{ width: 200, height: 32 }} />
      </label>,
    )

    const tree = testRoot.renderer.getA11yTree()
    const labelled = byAuthorId(tree, "notes").labelled_by ?? []
    expect(labelled).toHaveLength(1)
    const label = tree.nodes?.[labelled[0] ?? ""]?.aria
    expect(label).toEqual(expect.objectContaining({ role: "Label", value: "Notes" }))
  })

  it("lets aria-label and aria-labelledby outrank a <label>", () => {
    testRoot.render(
      <div style={{ width: 300, height: 160 }}>
        <label htmlFor="a" style={{ height: 20 }}>One</label>
        <input id="a" aria-label="Own name" value="" style={{ width: 100, height: 24 }} />
        <label htmlFor="b" style={{ height: 20 }}>Two</label>
        <input id="b" aria-labelledby="other" value="" style={{ width: 100, height: 24 }} />
        <text id="other" style={{ height: 20 }}>Other</text>
      </div>,
    )

    const tree = testRoot.renderer.getA11yTree()
    expect(byAuthorId(tree, "a").labelled_by).toBeUndefined()
    expect(byAuthorId(tree, "b").labelled_by).toEqual([keyOf(tree, "other")])
  })

  it("ignores htmlFor that does not name a labelable control", () => {
    testRoot.render(
      <div style={{ width: 300, height: 100 }}>
        <label htmlFor="box" style={{ height: 20 }}>Nothing</label>
        <div id="box" role="button" style={{ width: 40, height: 20 }} />
        <input id="x" value="" style={{ width: 100, height: 24 }} />
      </div>,
    )

    const tree = testRoot.renderer.getA11yTree()
    expect(byAuthorId(tree, "box").labelled_by).toBeUndefined()
    expect(byAuthorId(tree, "x").labelled_by).toBeUndefined()
  })

  it("focuses the control and clicks it when its label is clicked", () => {
    const clicks: number[] = []
    const labelClicks: number[] = []
    testRoot.render(
      <div style={{ width: 300, height: 120, display: "flex", flexDirection: "column" }}>
        <label
          htmlFor="name"
          testId="label"
          onClick={() => labelClicks.push(1)}
          style={{ width: 200, height: 30 }}
        >
          Name
        </label>
        <input
          id="name"
          testId="name-input"
          value=""
          onClick={() => clicks.push(1)}
          style={{ width: 200, height: 32 }}
        />
      </div>,
    )

    const label = testRoot.renderer.findByTestId("label")!
    const input = testRoot.renderer.findByTestId("name-input")!
    expect(testRoot.renderer.getFocusedElementId()).not.toBe(input.id)
    const bounds = testRoot.renderer.getElementBounds(label.id)!
    testRoot.renderer.nativeSimulateClick(bounds.x + 10, bounds.y + 10)

    expect(labelClicks).toHaveLength(1)
    expect(clicks).toHaveLength(1)
    expect(testRoot.renderer.getFocusedElementId()).toBe(input.id)
  })

  it("does not click a control twice when the click lands on it inside its label", () => {
    const clicks: number[] = []
    testRoot.render(
      <label testId="label" style={{ width: 300, height: 40 }}>
        <input
          testId="inner"
          value=""
          onClick={() => clicks.push(1)}
          style={{ width: 200, height: 32 }}
        />
      </label>,
    )

    const input = testRoot.renderer.findByTestId("inner")!
    const bounds = testRoot.renderer.getElementBounds(input.id)!
    testRoot.renderer.nativeSimulateClick(bounds.x + 10, bounds.y + 10)

    expect(clicks).toHaveLength(1)
  })

  describe("label activation", () => {
    function renderLabelAndInput(
      labelProps: { onClick?: (event: { preventDefault(): void }) => void },
      inputClicks: Array<{ detail: number | undefined; x: number | undefined }>,
    ) {
      testRoot.render(
        <div style={{ width: 300, height: 120, display: "flex", flexDirection: "column" }}>
          <label htmlFor="name" testId="label" {...labelProps} style={{ width: 200, height: 30 }}>
            Name
          </label>
          <input
            id="name"
            testId="name-input"
            value=""
            onClick={(event) => inputClicks.push({ detail: event.detail, x: event.x })}
            style={{ width: 200, height: 32 }}
          />
        </div>,
      )
      const label = testRoot.renderer.findByTestId("label")!
      const input = testRoot.renderer.findByTestId("name-input")!
      return { input, labelBounds: testRoot.renderer.getElementBounds(label.id)! }
    }

    it("clicks the control with the label click's position and count", () => {
      const inputClicks: Array<{ detail: number | undefined; x: number | undefined }> = []
      const { labelBounds } = renderLabelAndInput({}, inputClicks)
      testRoot.renderer.nativeSimulateClick(labelBounds.x + 10, labelBounds.y + 10)
      expect(inputClicks).toEqual([{ detail: 1, x: labelBounds.x + 10 }])
    })

    it("does nothing when a handler cancels the label's click", () => {
      const inputClicks: Array<{ detail: number | undefined; x: number | undefined }> = []
      const { input, labelBounds } = renderLabelAndInput(
        { onClick: (event) => event.preventDefault() },
        inputClicks,
      )
      testRoot.renderer.nativeSimulateClick(labelBounds.x + 10, labelBounds.y + 10)
      expect(inputClicks).toHaveLength(0)
      expect(testRoot.renderer.getFocusedElementId()).not.toBe(input.id)
    })

    it("does not activate when the press was on the label and the release elsewhere", () => {
      const inputClicks: Array<{ detail: number | undefined; x: number | undefined }> = []
      const { input, labelBounds } = renderLabelAndInput({}, inputClicks)
      testRoot.renderer.nativeSimulateMouseDown(labelBounds.x + 10, labelBounds.y + 10)
      testRoot.renderer.nativeSimulateMouseUp(250, 110)
      expect(inputClicks).toHaveLength(0)
      expect(testRoot.renderer.getFocusedElementId()).not.toBe(input.id)
    })

    it("does nothing when the click lands on another control inside the label", () => {
      const clicks: string[] = []
      testRoot.render(
        <label testId="label" style={{ width: 300, height: 80, display: "flex", flexDirection: "column" }}>
          <input testId="a" value="" onClick={() => clicks.push("a")} style={{ width: 200, height: 30 }} />
          <input testId="b" value="" onClick={() => clicks.push("b")} style={{ width: 200, height: 30 }} />
        </label>,
      )
      const b = testRoot.renderer.findByTestId("b")!
      const bounds = testRoot.renderer.getElementBounds(b.id)!
      testRoot.renderer.nativeSimulateClick(bounds.x + 10, bounds.y + 10)
      expect(clicks).toEqual(["b"])
      expect(testRoot.renderer.getFocusedElementId()).toBe(b.id)
    })

    it("activates the control when the click lands on plain content inside the label", () => {
      const clicks: string[] = []
      testRoot.render(
        <label testId="label" style={{ width: 300, height: 80, display: "flex", flexDirection: "column" }}>
          <div testId="text" style={{ width: 200, height: 30 }}>
            Name
          </div>
          <input testId="a" value="" onClick={() => clicks.push("a")} style={{ width: 200, height: 30 }} />
        </label>,
      )
      const bounds = testRoot.renderer.getElementBounds(testRoot.renderer.findByTestId("text")!.id)!
      testRoot.renderer.nativeSimulateClick(bounds.x + 10, bounds.y + 10)
      expect(clicks).toEqual(["a"])
      expect(testRoot.renderer.getFocusedElementId()).toBe(testRoot.renderer.findByTestId("a")!.id)
    })

    it("activates only the inner label's control for a click inside a nested label", () => {
      const clicks: string[] = []
      testRoot.render(
        <label testId="outer" style={{ width: 300, height: 120, display: "flex", flexDirection: "column" }}>
          <input testId="first" value="" onClick={() => clicks.push("first")} style={{ width: 200, height: 30 }} />
          <label testId="inner" style={{ width: 200, height: 60, display: "flex", flexDirection: "column" }}>
            <div testId="inner-text" style={{ width: 200, height: 20 }}>
              Inner
            </div>
            <input testId="second" value="" onClick={() => clicks.push("second")} style={{ width: 200, height: 30 }} />
          </label>
        </label>,
      )
      const bounds = testRoot.renderer.getElementBounds(testRoot.renderer.findByTestId("inner-text")!.id)!
      testRoot.renderer.nativeSimulateClick(bounds.x + 10, bounds.y + 5)
      expect(clicks).toEqual(["second"])
      expect(testRoot.renderer.getFocusedElementId()).toBe(testRoot.renderer.findByTestId("second")!.id)
    })

    describe("click() on a label", () => {
      type HostRef = { id: number; click(): void }

      function renderRefLabel(onClick?: (event: { preventDefault(): void }) => void) {
        const inputClicks: Array<{ detail: number | undefined; pointerId: number | undefined }> = []
        let label: HostRef | undefined
        testRoot.render(
          <div style={{ width: 300, height: 120, display: "flex", flexDirection: "column" }}>
            <label
              htmlFor="name"
              ref={(instance) => {
                if (instance) label = instance as unknown as HostRef
              }}
              onClick={onClick}
              style={{ width: 200, height: 30 }}
            >
              Name
            </label>
            <input
              id="name"
              testId="name-input"
              value=""
              onClick={(event) => inputClicks.push({ detail: event.detail, pointerId: event.pointerId })}
              style={{ width: 200, height: 32 }}
            />
          </div>,
        )
        testRoot.renderer.flush()
        return { label: label!, input: testRoot.renderer.findByTestId("name-input")!, inputClicks }
      }

      it("focuses the control and clicks it", () => {
        const { label, input, inputClicks } = renderRefLabel()
        label.click()
        testRoot.renderer.dispatchNativeEvents()
        expect(inputClicks).toEqual([{ detail: 0, pointerId: -1 }])
        expect(testRoot.renderer.getFocusedElementId()).toBe(input.id)
      })

      it("does nothing when a handler cancels the label's click", () => {
        const { label, input, inputClicks } = renderRefLabel((event) => event.preventDefault())
        label.click()
        testRoot.renderer.dispatchNativeEvents()
        expect(inputClicks).toHaveLength(0)
        expect(testRoot.renderer.getFocusedElementId()).not.toBe(input.id)
      })
    })

    it("does not activate a disabled control", () => {
      const clicks: number[] = []
      testRoot.render(
        <div style={{ width: 300, height: 120, display: "flex", flexDirection: "column" }}>
          <label htmlFor="off" testId="label" style={{ width: 200, height: 30 }}>
            Off
          </label>
          <input
            id="off"
            testId="off-input"
            value=""
            disabled
            onClick={() => clicks.push(1)}
            style={{ width: 200, height: 32 }}
          />
        </div>,
      )
      const bounds = testRoot.renderer.getElementBounds(
        testRoot.renderer.findByTestId("label")!.id,
      )!
      testRoot.renderer.nativeSimulateClick(bounds.x + 10, bounds.y + 10)
      expect(clicks).toHaveLength(0)
      expect(testRoot.renderer.getFocusedElementId()).not.toBe(
        testRoot.renderer.findByTestId("off-input")!.id,
      )
    })
  })
})
