/** End-to-end tests for headless floating controls over the native GPUI pipeline. */
// @ts-nocheck

import React, { useRef, useState } from "react"
import { beforeEach, describe, expect, it } from "vitest"
import * as ComboboxPrimitive from "../components/combobox"
import { FloatingLayer } from "../components/floating"
import * as SelectPrimitive from "../components/select"
import * as TooltipPrimitive from "../components/tooltip"
import * as DialogPrimitive from "../components/dialog"
import { Button } from "../components/button"
import {
  Combobox,
  ComboboxContent,
  ComboboxEmpty,
  ComboboxInput,
  ComboboxItem,
  ComboboxList,
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "../index"
// `../index` does not re-export the test helpers, so importing them from there
// left `hasNativeTestRenderer` undefined and skipped this whole suite silently.
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const triggerStyle = {
  width: 180,
  height: 36,
  padding: 8,
  backgroundColor: "#27324a",
  color: "#ffffff",
}

const contentStyle = {
  width: 180,
  maxHeight: 150,
  overflowY: "scroll",
  padding: 4,
  backgroundColor: "#111827",
  color: "#ffffff",
}

const itemStyle = ({ highlighted, selected, disabled }) => ({
  height: 32,
  padding: 6,
  opacity: disabled ? 0.4 : 1,
  backgroundColor: highlighted ? "#334155" : selected ? "#1e3a5f" : "#111827",
})

const FilledRow = React.forwardRef(function FilledRow(
  { label, style, ...props },
  ref
) {
  return (
    <div
      {...props}
      ref={ref}
      style={{
        width: "100%",
        height: 32,
        padding: 6,
        backgroundColor: "#1e3a5f",
        ...style,
      }}
    >
      <text>{label}</text>
    </div>
  )
})

describeNative("floating controls", () => {
  let testRoot: ReturnType<typeof createTestRoot>

  beforeEach(() => {
    testRoot = createTestRoot()
  })

  it("forwards outer surface styles without multiplying opacity", () => {
    testRoot.render(
      <div style={{ width: 400, height: 300 }}>
        <FloatingLayer
          style={{
            width: 120,
            height: 60,
            visibility: "hidden",
            opacity: 0.5,
            pointerEvents: "none",
            borderRadius: 16,
            borderTopLeftRadius: 4,
            borderTopRightRadius: 8,
            borderBottomRightRadius: 12,
            borderBottomLeftRadius: 20,
            hover: {
              opacity: 0.75,
              borderTopLeftRadius: 24,
              backgroundColor: "#222222",
            },
            active: {
              opacity: 0.9,
              borderBottomRightRadius: 28,
            },
          }}
        >
          <text>Rounded layer</text>
        </FloatingLayer>
      </div>,
    )

    const anchored = testRoot.renderer.findByType("anchored")[0]
    const content = testRoot.renderer.getElement(anchored.children[0])
    const surfaceHover = anchored.style.hover as Record<string, unknown>
    const surfaceActive = anchored.style.active as Record<string, unknown>

    expect({
      visibility: anchored.style.visibility,
      opacity: anchored.style.opacity,
      borderRadius: anchored.style.borderRadius,
      borderTopLeftRadius: anchored.style.borderTopLeftRadius,
      borderTopRightRadius: anchored.style.borderTopRightRadius,
      borderBottomRightRadius: anchored.style.borderBottomRightRadius,
      borderBottomLeftRadius: anchored.style.borderBottomLeftRadius,
      hover: {
        opacity: surfaceHover.opacity,
        borderTopLeftRadius: surfaceHover.borderTopLeftRadius,
        backgroundColor: surfaceHover.backgroundColor,
      },
      active: {
        opacity: surfaceActive.opacity,
        borderBottomRightRadius: surfaceActive.borderBottomRightRadius,
      },
      pointerEvents: anchored.style.pointerEvents,
    }).toMatchInlineSnapshot(`
      {
        "active": {
          "borderBottomRightRadius": 28,
          "opacity": 0.9,
        },
        "borderBottomLeftRadius": 20,
        "borderBottomRightRadius": 12,
        "borderRadius": 16,
        "borderTopLeftRadius": 4,
        "borderTopRightRadius": 8,
        "hover": {
          "backgroundColor": null,
          "borderTopLeftRadius": 24,
          "opacity": 0.75,
        },
        "opacity": 0.5,
        "pointerEvents": "none",
        "visibility": "hidden",
      }
    `)
    expect(content?.style.opacity).toBeUndefined()
    expect((content?.style.hover as Record<string, unknown>).opacity).toBeNull()
    expect((content?.style.hover as Record<string, unknown>).backgroundColor).toBe("#222222")
  })

  it("composes a headless Select and supports keyboard selection", () => {
    const items = [
      { value: "alpha", label: "Alpha" },
      { value: "disabled", label: "Disabled" },
      { value: "beta", label: "Beta" },
    ]
    function Demo() {
      const [value, setValue] = useState("alpha")
      return (
        <div style={{ width: 400, height: 300, padding: 12 }}>
          <SelectPrimitive.Root items={items} value={value} onValueChange={setValue}>
            <SelectPrimitive.Trigger style={triggerStyle}>
              <SelectPrimitive.Value placeholder="Choose" />
            </SelectPrimitive.Trigger>
            <SelectPrimitive.Content side="bottom" sideOffset={4} style={contentStyle}>
              <SelectPrimitive.Group>
                <SelectPrimitive.Label style={{ height: 24 }}>Models</SelectPrimitive.Label>
                <SelectPrimitive.Item value="alpha" style={itemStyle}>Alpha</SelectPrimitive.Item>
                <SelectPrimitive.Item value="disabled" disabled style={itemStyle}>
                  Disabled
                </SelectPrimitive.Item>
                <SelectPrimitive.Separator style={{ height: 1, backgroundColor: "#475569" }} />
                <SelectPrimitive.Item value="beta" style={itemStyle}>Beta</SelectPrimitive.Item>
              </SelectPrimitive.Group>
            </SelectPrimitive.Content>
          </SelectPrimitive.Root>
          <text>{`Value: ${value}`}</text>
        </div>
      )
    }

    testRoot.render(<Demo />)
    expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
      [
        "Alpha",
        "Value: alpha",
      ]
    `)

    testRoot.renderer.nativeSimulateClick(30, 25)
    expect(testRoot.renderer.getAllText()).toContain("Beta")

    testRoot.renderer.simulateKeystrokes("ArrowDown")
    testRoot.renderer.simulateKeystrokes("Enter")

    expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
      [
        "Beta",
        "Value: beta",
      ]
    `)
  })

  it("closes a Select from its trigger without reopening on the same press", () => {
    function Demo() {
      return (
        <div style={{ width: 400, height: 260, padding: 12 }}>
          <Select items={[{ value: "one", label: "One" }, { value: "two", label: "Two" }]} defaultValue="one">
            <SelectTrigger style={triggerStyle}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent sideOffset={4} style={contentStyle}>
              <SelectItem value="one" style={itemStyle}>One</SelectItem>
              <SelectItem value="two" style={itemStyle}>Two</SelectItem>
            </SelectContent>
          </Select>
        </div>
      )
    }

    testRoot.render(<Demo />)
    testRoot.renderer.nativeSimulateClick(30, 25)
    expect(testRoot.renderer.getAllText()).toContain("Two")

    testRoot.renderer.nativeSimulateClick(30, 25)
    expect(testRoot.renderer.getAllText()).toEqual(["One"])
  })

  it("blocks clicks behind an absolute overlay with a fill", () => {
    function Demo() {
      const [behind, setBehind] = useState(0)
      const [overlay, setOverlay] = useState(0)
      return (
        <div style={{ width: 400, height: 260, position: "relative" }}>
          <div
            style={{ position: "absolute", top: 40, left: 20, width: 200, height: 80, backgroundColor: "#1e3a5f" }}
            onClick={() => setBehind((count) => count + 1)}
          >
            <text>Behind</text>
          </div>
          <div
            style={{
              position: "absolute",
              top: 40,
              left: 20,
              width: 200,
              height: 80,
              backgroundColor: "#111827",
            }}
            onClick={() => setOverlay((count) => count + 1)}
          >
            <text>Overlay</text>
          </div>
          <text>{`Behind: ${behind} Overlay: ${overlay}`}</text>
        </div>
      )
    }

    testRoot.render(<Demo />)
    testRoot.renderer.nativeSimulateClick(80, 70)
    expect(testRoot.renderer.getAllText()).toContain("Behind: 0 Overlay: 1")
  })

  it("lets pointerEvents none pass clicks through", () => {
    function Demo() {
      const [behind, setBehind] = useState(0)
      return (
        <div style={{ width: 400, height: 260, position: "relative" }}>
          <div
            style={{ position: "absolute", top: 40, left: 20, width: 200, height: 80, backgroundColor: "#1e3a5f" }}
            onClick={() => setBehind((count) => count + 1)}
          >
            <text>Behind</text>
          </div>
          <div
            style={{
              position: "absolute",
              top: 40,
              left: 20,
              width: 200,
              height: 80,
              backgroundColor: "#111827",
              pointerEvents: "none",
            }}
          >
            <text>Ghost</text>
          </div>
          <text>{`Behind: ${behind}`}</text>
        </div>
      )
    }

    testRoot.render(<Demo />)
    testRoot.renderer.nativeSimulateClick(80, 70)
    expect(testRoot.renderer.getAllText()).toContain("Behind: 1")
  })

  it("a transparent overlapping element is still the target of a click", () => {
    function Demo() {
      const [behind, setBehind] = useState(0)
      return (
        <div
          style={{ width: 400, height: 260, position: "relative", color: "#ffffff" }}
        >
          <div
            style={{
              position: "absolute",
              top: 0,
              left: 20,
              width: 200,
              height: 80,
              backgroundColor: "#1e3a5f",
            }}
            onClick={() => setBehind((count) => count + 1)}
          >
            <text>Behind</text>
          </div>
          <div
            style={{
              // Positioned, so it paints (and is hit) above the absolute box before it.
              position: "relative",
              marginTop: 40,
              marginLeft: 20,
              width: 200,
              height: 80,
              backgroundColor: "transparent",
            }}
          />
          <text>{`Behind: ${behind}`}</text>
        </div>
      )
    }

    testRoot.render(<Demo />)
    testRoot.renderer.nativeSimulateClick(80, 70)
    expect(testRoot.renderer.getAllText()).toContain("Behind: 0")
  })

  it("occludes controls behind SelectContent", () => {
    function Demo() {
      const [clicks, setClicks] = useState(0)
      const [value, setValue] = useState("one")
      return (
        <div style={{ width: 400, height: 260, position: "relative", padding: 12 }}>
          <div
            style={{ position: "absolute", top: 52, left: 12, width: 180, height: 90 }}
            onClick={() => setClicks((count) => count + 1)}
          >
            <text>Behind</text>
          </div>
          <Select items={[{ value: "one", label: "One" }, { value: "two", label: "Two" }]} value={value} onValueChange={setValue}>
            <SelectTrigger style={triggerStyle}><SelectValue /></SelectTrigger>
            <SelectContent sideOffset={4} style={contentStyle}>
              <SelectItem value="one" style={itemStyle}>One</SelectItem>
              <SelectItem value="two" style={itemStyle}>Two</SelectItem>
            </SelectContent>
          </Select>
          <text>{`Behind clicks: ${clicks}`}</text>
        </div>
      )
    }

    testRoot.render(<Demo />)
    testRoot.renderer.nativeSimulateClick(30, 25)
    testRoot.renderer.nativeSimulateClick(30, 104)

    expect(testRoot.renderer.getAllText()).toContain("Behind clicks: 0")
    expect(testRoot.renderer.getAllText()).toContain("Two")
  })

  it("selects a filled SelectItem asChild row", () => {
    function Demo() {
      const [value, setValue] = useState("one")
      return (
        <div style={{ width: 400, height: 300, padding: 12 }}>
          <Select
            items={[
              { value: "one", label: "One" },
              { value: "two", label: "Two" },
            ]}
            value={value}
            onValueChange={setValue}
          >
            <SelectTrigger style={triggerStyle}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent sideOffset={4} style={contentStyle}>
              <SelectItem value="one" asChild>
                <FilledRow label="One" />
              </SelectItem>
              <SelectItem value="two" testId="two" asChild>
                <FilledRow label="Two" />
              </SelectItem>
            </SelectContent>
          </Select>
          <text>{`Value: ${value}`}</text>
        </div>
      )
    }

    testRoot.render(<Demo />)
    testRoot.renderer.nativeSimulateClick(30, 25)
    const two = testRoot.renderer.findByTestId("two")
    expect(two).toBeDefined()
    const bounds = testRoot.renderer.getElementBounds(two.id)
    testRoot.renderer.nativeSimulateClick(bounds.x + 8, bounds.y + 8)
    expect(testRoot.renderer.getAllText()).toContain("Value: two")
  })

  it("filters and selects through the shadcn Combobox shape", () => {
    const frameworks = ["Astro", "SvelteKit", "Next.js"]

    function Demo() {
      const [value, setValue] = useState<string | null>(null)
      return (
        <div style={{ width: 400, height: 300, padding: 12 }}>
          <ComboboxPrimitive.Root items={frameworks} value={value} onValueChange={setValue}>
            <ComboboxPrimitive.Input
              placeholder="Select a framework"
              style={triggerStyle}
            />
            <ComboboxPrimitive.Content sideOffset={4} style={contentStyle}>
              <ComboboxPrimitive.Empty>No items found.</ComboboxPrimitive.Empty>
              <ComboboxPrimitive.List>
                {(item) => (
                  <ComboboxPrimitive.Item key={item} value={item} style={itemStyle}>
                    {item}
                  </ComboboxPrimitive.Item>
                )}
              </ComboboxPrimitive.List>
            </ComboboxPrimitive.Content>
          </ComboboxPrimitive.Root>
          <text>{`Selected: ${value ?? "none"}`}</text>
        </div>
      )
    }

    testRoot.render(<Demo />)
    const input = testRoot.renderer.findByType("input")[0]
    testRoot.renderer.nativeSimulateClick(30, 25)
    testRoot.renderer.nativeSimulateKeystrokes(input.id, "s")

    expect(testRoot.renderer.getAllText()).toMatchInlineSnapshot(`
      [
        "SvelteKit",
        "Astro",
        "Next.js",
        "Selected: none",
      ]
    `)

    testRoot.renderer.nativeSimulateKeystrokes(input.id, "ArrowDown")
    testRoot.renderer.nativeSimulateKeystrokes(input.id, "Enter")

    expect(testRoot.renderer.getAllText()).toContain("Selected: SvelteKit")
  })

  it("selects a filled ComboboxItem asChild row", () => {
    function Demo() {
      const [value, setValue] = useState<string | null>(null)
      const items = ["Alpha", "Beta"]
      return (
        <div style={{ width: 400, height: 300, padding: 12 }}>
          <Combobox items={items} value={value} onValueChange={setValue}>
            <ComboboxInput style={triggerStyle} />
            <ComboboxContent sideOffset={4} style={contentStyle}>
              <ComboboxList>
                {(item) => (
                  <ComboboxItem key={item} value={item} testId={item} asChild>
                    <FilledRow label={item} />
                  </ComboboxItem>
                )}
              </ComboboxList>
            </ComboboxContent>
          </Combobox>
          <text>{`Selected: ${value ?? "none"}`}</text>
        </div>
      )
    }

    testRoot.render(<Demo />)
    const input = testRoot.renderer.findByType("input")[0]
    testRoot.renderer.nativeSimulateClick(30, 25)
    const beta = testRoot.renderer.findByTestId("Beta")
    expect(beta).toBeDefined()
    const bounds = testRoot.renderer.getElementBounds(beta.id)
    testRoot.renderer.nativeSimulateClick(bounds.x + 8, bounds.y + 8)
    expect(testRoot.renderer.getAllText()).toContain("Selected: Beta")
  })

  it("renders ComboboxEmpty when filtering removes every item", () => {
    function Demo() {
      return (
        <div style={{ width: 400, height: 240, padding: 12 }}>
          <Combobox items={["Alpha", "Beta"]}>
            <ComboboxInput style={triggerStyle} />
            <ComboboxContent sideOffset={4} style={contentStyle}>
              <ComboboxEmpty>Nothing found</ComboboxEmpty>
              <ComboboxList>
                {(item) => <ComboboxItem key={item} value={item}>{item}</ComboboxItem>}
              </ComboboxList>
            </ComboboxContent>
          </Combobox>
        </div>
      )
    }

    testRoot.render(<Demo />)
    const input = testRoot.renderer.findByType("input")[0]
    testRoot.renderer.nativeSimulateClick(30, 25)
    testRoot.renderer.nativeSimulateKeystrokes(input.id, "z")

    expect(testRoot.renderer.getAllText()).toContain("Nothing found")
  })

  it("skips disabled Combobox items during keyboard navigation", () => {
    function Demo() {
      const [value, setValue] = useState<string | null>(null)
      const items = ["Disabled", "Enabled"]
      return (
        <div style={{ width: 400, height: 240, padding: 12 }}>
          <Combobox items={items} value={value} onValueChange={setValue}>
            <ComboboxInput style={triggerStyle} />
            <ComboboxContent sideOffset={4} style={contentStyle}>
              <ComboboxList>
                {(item) => (
                  <ComboboxItem key={item} value={item} disabled={item === "Disabled"}>
                    {item}
                  </ComboboxItem>
                )}
              </ComboboxList>
            </ComboboxContent>
          </Combobox>
          <text>{`Selected: ${value ?? "none"}`}</text>
        </div>
      )
    }

    testRoot.render(<Demo />)
    const input = testRoot.renderer.findByType("input")[0]
    testRoot.renderer.nativeSimulateClick(30, 25)
    testRoot.renderer.nativeSimulateKeystrokes(input.id, "ArrowDown")
    testRoot.renderer.nativeSimulateKeystrokes(input.id, "Enter")

    expect(testRoot.renderer.getAllText()).toContain("Selected: Enabled")
  })

  it("does not select from a disabled Combobox", () => {
    function Demo() {
      const [value, setValue] = useState<string | null>(null)
      return (
        <div style={{ width: 400, height: 240 }}>
          <Combobox disabled items={["Alpha"]} value={value} onValueChange={setValue}>
            <ComboboxInput style={triggerStyle} />
            <ComboboxContent><ComboboxList>{(item) => (
              <ComboboxItem key={item} value={item}>{item}</ComboboxItem>
            )}</ComboboxList></ComboboxContent>
          </Combobox>
          <text>{`Selected: ${value ?? "none"}`}</text>
        </div>
      )
    }

    testRoot.render(<Demo />)
    const input = testRoot.renderer.findByType("input")[0]
    testRoot.renderer.nativeSimulateKeystrokes(input.id, "ArrowDown Enter")

    expect(testRoot.renderer.getAllText()).toContain("Selected: none")
  })

  it("opens and closes a Tooltip from native hover events", () => {
    let triggerRef = null

    function Demo() {
      return (
        <div style={{ width: 400, height: 240, padding: 12 }}>
          <TooltipPrimitive.Provider delayDuration={0} disableHoverableContent>
            <TooltipPrimitive.Root>
              <TooltipPrimitive.Trigger asChild>
                <div ref={(instance) => { triggerRef = instance }} style={triggerStyle}>
                  Hover me
                </div>
              </TooltipPrimitive.Trigger>
              <TooltipPrimitive.Content
                side="bottom"
                sideOffset={4}
                style={{ width: 120, height: 28, padding: 6, backgroundColor: "#020617" }}
              >
                Tooltip body
              </TooltipPrimitive.Content>
            </TooltipPrimitive.Root>
          </TooltipPrimitive.Provider>
        </div>
      )
    }

    testRoot.render(<Demo />)
    expect(triggerRef).not.toBeNull()
    expect(testRoot.renderer.getAllText()).toEqual(["Hover me"])

    testRoot.renderer.nativeSimulateMouseMove(30, 25)
    expect(testRoot.renderer.getAllText()).toContain("Tooltip body")

    const trigger = testRoot.renderer
      .findByType("div")
      .find((element) => element.events.has("mouseenter"))!
    testRoot.renderer.nativeSimulateKeyDown(trigger.id, "Escape")
    expect(testRoot.renderer.getAllText()).toEqual(["Hover me"])

    testRoot.renderer.nativeSimulateMouseMove(30, 25)
    testRoot.renderer.nativeSimulateMouseMove(300, 180)
    expect(testRoot.renderer.getAllText()).toEqual(["Hover me"])
  })

  it("moves through tab-indexed controls explicitly", () => {
    function Demo() {
      const [focused, setFocused] = useState("none")
      return (
        <div style={{ width: 400, height: 160 }}>
          <div
            autoFocus
            tabIndex={0}
            style={{ width: 100, height: 32 }}
            onKeyDown={() => setFocused("first")}
          >
            First
          </div>
          <div
            tabIndex={0}
            style={{ width: 100, height: 32 }}
            onKeyDown={() => setFocused("second")}
          >
            Second
          </div>
          <text>{`Focused: ${focused}`}</text>
        </div>
      )
    }

    testRoot.render(<Demo />)
    testRoot.renderer.focusNext()
    testRoot.renderer.simulateKeystrokes("a")

    expect(testRoot.renderer.getAllText()).toContain("Focused: second")

    testRoot.renderer.focusPrevious()
    testRoot.renderer.simulateKeystrokes("a")

    expect(testRoot.renderer.getAllText()).toContain("Focused: first")
  })

  it("removes an element from tab order when tabIndex is removed", () => {
    function Demo() {
      const [secondEnabled, setSecondEnabled] = useState(true)
      const [focused, setFocused] = useState("none")
      return (
        <div style={{ width: 400, height: 160 }}>
          <div
            autoFocus
            tabIndex={0}
            style={{ width: 100, height: 32 }}
            onClick={() => setSecondEnabled(false)}
            onKeyDown={() => setFocused("first")}
          >
            First
          </div>
          <div
            tabIndex={secondEnabled ? 0 : undefined}
            style={{ width: 100, height: 32 }}
            onKeyDown={() => setFocused("second")}
          >
            Second
          </div>
          <text>{`Focused: ${focused}`}</text>
        </div>
      )
    }

    testRoot.render(<Demo />)
    testRoot.renderer.nativeSimulateClick(30, 16)
    testRoot.renderer.focusNext()
    testRoot.renderer.simulateKeystrokes("a")

    expect(testRoot.renderer.getAllText()).toContain("Focused: first")
  })

  it("selects from children when Root has no items", () => {
    function Demo() {
      const [value, setValue] = useState("one")
      return (
        <div style={{ width: 400, height: 300, padding: 12 }}>
          <Select value={value} onValueChange={setValue}>
            <SelectTrigger style={triggerStyle}>
              <SelectValue placeholder="Choose" />
            </SelectTrigger>
            <SelectContent sideOffset={4} style={contentStyle}>
              <SelectItem value="one" style={itemStyle}>One</SelectItem>
              <SelectItem value="two" style={itemStyle}>Two</SelectItem>
            </SelectContent>
          </Select>
          <text>{`Value: ${value}`}</text>
        </div>
      )
    }

    testRoot.render(<Demo />)
    expect(testRoot.renderer.getAllText()).toEqual(["one", "Value: one"])

    testRoot.renderer.nativeSimulateClick(30, 25)
    testRoot.renderer.simulateKeystrokes("ArrowDown")
    testRoot.renderer.simulateKeystrokes("Enter")
    expect(testRoot.renderer.getAllText()).toEqual(["two", "Value: two"])
  })

  it("keeps SelectValue working when items are wrapped components", () => {
    const items = [
      { value: "one", label: "One" },
      { value: "two", label: "Two" },
    ]
    const StyledItem = React.forwardRef((props, ref) => (
      <SelectItem {...props} ref={ref} style={itemStyle} />
    ))

    function Demo() {
      const [value, setValue] = useState("one")
      return (
        <div style={{ width: 400, height: 300, padding: 12 }}>
          <Select items={items} value={value} onValueChange={setValue}>
            <SelectTrigger style={triggerStyle}>
              <SelectValue placeholder="Choose" />
            </SelectTrigger>
            <SelectContent sideOffset={4} style={contentStyle}>
              <StyledItem value="one">One</StyledItem>
              <StyledItem value="two">Two</StyledItem>
            </SelectContent>
          </Select>
          <text>{`Value: ${value}`}</text>
        </div>
      )
    }

    testRoot.render(<Demo />)
    expect(testRoot.renderer.getAllText()).toEqual(["One", "Value: one"])

    testRoot.renderer.nativeSimulateClick(30, 25)
    expect(testRoot.renderer.getAllText()).toContain("Two")
    testRoot.renderer.simulateKeystrokes("ArrowDown")
    testRoot.renderer.simulateKeystrokes("Enter")
    expect(testRoot.renderer.getAllText()).toEqual(["Two", "Value: two"])
  })

  it("does not select a highlighted item after it unmounts", () => {
    function Demo() {
      const [value, setValue] = useState("one")
      const [showTwo, setShowTwo] = useState(true)
      return (
        <div style={{ width: 400, height: 320, padding: 12 }}>
          <Select value={value} onValueChange={setValue} defaultOpen>
            <SelectTrigger style={triggerStyle}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent sideOffset={4} style={contentStyle}>
              <SelectItem value="one" style={itemStyle}>One</SelectItem>
              {showTwo ? <SelectItem value="two" style={itemStyle}>Two</SelectItem> : null}
              <div
                testId="hide-two"
                style={{ height: 24, backgroundColor: "#334155" }}
                onClick={() => setShowTwo(false)}
              >
                Hide
              </div>
            </SelectContent>
          </Select>
          <text>{`Value: ${value}`}</text>
        </div>
      )
    }

    testRoot.render(<Demo />)
    testRoot.renderer.simulateKeystrokes("ArrowDown")
    const hide = testRoot.renderer.findByTestId("hide-two")
    const bounds = testRoot.renderer.getElementBounds(hide.id)
    testRoot.renderer.nativeSimulateClick(bounds.x + 8, bounds.y + 8)
    testRoot.renderer.simulateKeystrokes("Enter")
    expect(testRoot.renderer.getAllText()).toContain("Value: one")
  })

  it("highlights a selected item that mounts after the popup is open", () => {
    function Demo({ showOne }: { showOne: boolean }) {
      const [value, setValue] = useState("one")
      return (
        <div style={{ width: 400, height: 320, padding: 12 }}>
          <Select value={value} onValueChange={setValue} defaultOpen>
            <SelectTrigger style={triggerStyle}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent sideOffset={4} style={contentStyle}>
              {showOne ? <SelectItem value="one" style={itemStyle}>One</SelectItem> : null}
              <SelectItem value="two" style={itemStyle}>Two</SelectItem>
            </SelectContent>
          </Select>
          <text>{`Value: ${value}`}</text>
        </div>
      )
    }

    testRoot.render(<Demo showOne={false} />)
    testRoot.render(<Demo showOne={true} />)
    testRoot.renderer.simulateKeystrokes("ArrowDown")
    testRoot.renderer.simulateKeystrokes("Enter")
    expect(testRoot.renderer.getAllText()).toContain("Value: two")
  })

  it("keeps keyboard order when a middle item re-renders alone", () => {
    function Demo() {
      const [value, setValue] = useState("one")
      const [tick, setTick] = useState(0)
      return (
        <div style={{ width: 400, height: 340, padding: 12 }}>
          <Select value={value} onValueChange={setValue} defaultOpen>
            <SelectTrigger style={triggerStyle}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent sideOffset={4} style={contentStyle}>
              <SelectItem value="one" style={itemStyle}>One</SelectItem>
              <SelectItem value="two" style={itemStyle}>{`Two ${tick}`}</SelectItem>
              <SelectItem value="three" style={itemStyle}>Three</SelectItem>
              <div
                testId="nudge"
                style={{ height: 24, backgroundColor: "#334155" }}
                onClick={() => setTick((count) => count + 1)}
              >
                Nudge
              </div>
            </SelectContent>
          </Select>
          <text>{`Value: ${value}`}</text>
        </div>
      )
    }

    testRoot.render(<Demo />)
    const nudge = testRoot.renderer.findByTestId("nudge")
    const bounds = testRoot.renderer.getElementBounds(nudge.id)
    testRoot.renderer.nativeSimulateClick(bounds.x + 8, bounds.y + 8)
    testRoot.renderer.simulateKeystrokes("ArrowDown")
    testRoot.renderer.simulateKeystrokes("Enter")
    expect(testRoot.renderer.getAllText()).toContain("Value: two")
  })

  it("does not select from a disabled open Select", () => {
    function Demo() {
      const [value, setValue] = useState("one")
      return (
        <div style={{ width: 400, height: 300, padding: 12 }}>
          <Select disabled defaultOpen value={value} onValueChange={setValue}>
            <SelectTrigger style={triggerStyle}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent sideOffset={4} style={contentStyle}>
              <SelectItem value="one" style={itemStyle}>One</SelectItem>
              <SelectItem value="two" testId="two" style={itemStyle}>Two</SelectItem>
            </SelectContent>
          </Select>
          <text>{`Value: ${value}`}</text>
        </div>
      )
    }

    testRoot.render(<Demo />)
    const two = testRoot.renderer.findByTestId("two")
    const twoBounds = testRoot.renderer.getElementBounds(two.id)
    testRoot.renderer.nativeSimulateClick(twoBounds.x + 8, twoBounds.y + 8)
    expect(testRoot.renderer.getAllText()).toContain("Value: one")
    testRoot.renderer.simulateKeystrokes("ArrowDown")
    testRoot.renderer.simulateKeystrokes("Enter")
    expect(testRoot.renderer.getAllText()).toContain("Value: one")
  })

  it("wraps tab order inside a subtree", () => {
    function Demo() {
      return (
        <div style={{ width: 400, height: 220, padding: 12 }}>
          <div
            autoFocus
            tabIndex={0}
            testId="outside"
            style={{ width: 100, height: 32 }}
          >
            Outside
          </div>
          <div
            testId="panel"
            style={{ width: 180, height: 80, backgroundColor: "#1e293b" }}
          >
            <div tabIndex={2} testId="later" style={{ width: 80, height: 32 }}>
              Later
            </div>
            <div tabIndex={1} testId="first" style={{ width: 80, height: 32 }}>
              First
            </div>
          </div>
        </div>
      )
    }

    testRoot.render(<Demo />)
    const panel = testRoot.renderer.findByTestId("panel")
    const first = testRoot.renderer.findByTestId("first")
    const later = testRoot.renderer.findByTestId("later")
    const outside = testRoot.renderer.findByTestId("outside")
    expect(panel).toBeDefined()
    expect(first).toBeDefined()
    expect(later).toBeDefined()

    testRoot.renderer.focusElement(first.id)
    expect(testRoot.renderer.getFocusedElementId()).toBe(first.id)
    testRoot.renderer.focusNextWithin(panel.id)
    expect(testRoot.renderer.getFocusedElementId()).toBe(later.id)
    expect(testRoot.renderer.getFocusedElementId()).not.toBe(outside.id)

    testRoot.renderer.focusNextWithin(panel.id)
    expect(testRoot.renderer.getFocusedElementId()).toBe(first.id)

    testRoot.renderer.focusPreviousWithin(panel.id)
    expect(testRoot.renderer.getFocusedElementId()).toBe(later.id)
  })

  it("skips a hidden tab stop", () => {
    function Demo() {
      return (
        <div style={{ width: 400, height: 220, padding: 12 }}>
          <div
            testId="panel"
            style={{ width: 180, height: 80, backgroundColor: "#1e293b" }}
          >
            <div tabIndex={0} testId="first" style={{ width: 80, height: 32 }}>
              First
            </div>
            <div
              tabIndex={0}
              testId="hidden"
              style={{ width: 80, height: 32, visibility: "hidden" }}
            >
              Hidden
            </div>
            <div tabIndex={0} testId="second" style={{ width: 80, height: 32 }}>
              Second
            </div>
          </div>
        </div>
      )
    }

    testRoot.render(<Demo />)
    const panel = testRoot.renderer.findByTestId("panel")
    const first = testRoot.renderer.findByTestId("first")
    const second = testRoot.renderer.findByTestId("second")
    const hidden = testRoot.renderer.findByTestId("hidden")
    expect(panel).toBeDefined()
    expect(first).toBeDefined()
    expect(second).toBeDefined()

    testRoot.renderer.focusElement(first.id)
    testRoot.renderer.focusNextWithin(panel.id)
    expect(testRoot.renderer.getFocusedElementId()).toBe(second.id)
    expect(testRoot.renderer.getFocusedElementId()).not.toBe(hidden.id)
  })

  it("moves keyboard focus through Select and Tooltip triggers like Base UI", () => {
    const center = (testId) => {
      const bounds = testRoot.renderer.getElementBounds(testRoot.renderer.findByTestId(testId).id)
      return [bounds.x + bounds.width / 2, bounds.y + bounds.height / 2]
    }
    const focused = () => {
      const id = testRoot.renderer.getFocusedElementId()
      return testRoot.renderer.getElement(id)?.testId ?? id
    }

    // The input sits right of the popup, so a press on it is a real outside press.
    testRoot.render(
      <div style={{ display: "flex", width: 500, height: 300, padding: 12, gap: 40 }}>
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <div autoFocus tabIndex={0} testId="before" style={{ width: 80, height: 24 }} />
          <Select items={[{ value: "a", label: "Alpha" }, { value: "b", label: "Beta" }]} defaultValue="a">
            <SelectTrigger asChild>
              <div testId="select-trigger" style={triggerStyle}><SelectValue /></div>
            </SelectTrigger>
            <SelectContent testId="select-content" style={contentStyle}>
              <SelectItem value="a" style={itemStyle}>Alpha</SelectItem>
              <SelectItem value="b" style={itemStyle}>Beta</SelectItem>
            </SelectContent>
          </Select>
          <Tooltip>
            <TooltipTrigger asChild>
              <div testId="tip-trigger" style={{ width: 80, height: 24 }}>Tip</div>
            </TooltipTrigger>
            <TooltipContent>Tip body</TooltipContent>
          </Tooltip>
        </div>
        <input testId="outside" style={{ width: 120, height: 28 }} />
      </div>
    )

    const steps = []
    const step = (label) => steps.push(`${label}: focus=${focused()} text=${testRoot.renderer.getAllText().join("|")}`)
    step("mount")
    testRoot.renderer.simulateKeystrokes("Tab")
    step("tab")
    testRoot.renderer.simulateKeystrokes("Enter")
    step("enter")
    testRoot.renderer.simulateKeystrokes("Tab")
    step("tab in popup")
    testRoot.renderer.simulateKeystrokes("Escape")
    step("escape")
    testRoot.renderer.simulateKeystrokes("Tab")
    step("tab")
    testRoot.renderer.simulateKeystrokes("Tab")
    step("tab")
    testRoot.renderer.nativeSimulateClick(...center("select-trigger"))
    step("click trigger")
    testRoot.renderer.nativeSimulateClick(...center("outside"))
    step("click input")

    expect("\n" + steps.join("\n")).toMatchInlineSnapshot(`
      "
      mount: focus=before text=Alpha|Tip
      tab: focus=select-trigger text=Alpha|Tip
      enter: focus=select-content text=Alpha|Alpha|Beta|Tip
      tab in popup: focus=select-content text=Alpha|Alpha|Beta|Tip
      escape: focus=select-trigger text=Alpha|Tip
      tab: focus=tip-trigger text=Alpha|Tip|Tip body
      tab: focus=outside text=Alpha|Tip
      click trigger: focus=select-content text=Alpha|Alpha|Beta|Tip
      click input: focus=outside text=Alpha|Tip"
    `)
  })

  it("closes an open Combobox when Tab leaves its input", () => {
    testRoot.render(
      <div style={{ width: 400, height: 300, padding: 12, flexDirection: "column", gap: 8 }}>
        <Combobox items={["alpha", "beta"]} defaultOpen>
          <ComboboxInput autoFocus testId="combo" style={{ width: 160, height: 28 }} />
          <ComboboxContent style={contentStyle}>
            <ComboboxList>
              {(item) => <ComboboxItem key={item} value={item}>{item}</ComboboxItem>}
            </ComboboxList>
          </ComboboxContent>
        </Combobox>
        <div tabIndex={0} testId="after" style={{ width: 80, height: 24 }} />
      </div>
    )
    expect(testRoot.renderer.getAllText()).toContain("beta")

    testRoot.renderer.simulateKeystrokes("Tab")

    const focused = testRoot.renderer.getElement(testRoot.renderer.getFocusedElementId())
    expect({ focused: focused?.testId, text: testRoot.renderer.getAllText() }).toMatchInlineSnapshot(`
      {
        "focused": "after",
        "text": [],
      }
    `)
  })

  it("runs a modal Dialog by keyboard: trap Tab, Escape closes the top layer first", () => {
    const log = []
    function Demo() {
      const [value, setValue] = useState("a")
      return (
        <div style={{ display: "flex", flexDirection: "column", width: 600, height: 400, padding: 12, gap: 8 }}>
          <div autoFocus tabIndex={0} testId="before" style={{ width: 80, height: 24 }} />
          <DialogPrimitive.Root onOpenChange={(open) => log.push(`open=${open}`)}>
            <DialogPrimitive.Trigger testId="open" style={{ width: 80, height: 24 }}>Open</DialogPrimitive.Trigger>
            <DialogPrimitive.Portal>
              <DialogPrimitive.Backdrop testId="backdrop" style={{ backgroundColor: "#00000080" }} />
              <DialogPrimitive.Popup
                testId="popup"
                style={{ width: 300, padding: 16, gap: 8, backgroundColor: "#1e293b" }}
              >
                <DialogPrimitive.Title>Settings</DialogPrimitive.Title>
                <Select items={[{ value: "a", label: "Alpha" }, { value: "b", label: "Beta" }]} value={value} onValueChange={setValue}>
                  <SelectTrigger testId="select" style={triggerStyle}><SelectValue /></SelectTrigger>
                  <SelectContent style={contentStyle}>
                    <SelectItem value="a" style={itemStyle}>Alpha</SelectItem>
                    <SelectItem value="b" style={itemStyle}>Beta</SelectItem>
                  </SelectContent>
                </Select>
                <DialogPrimitive.Close testId="close" style={{ width: 60, height: 24 }}>Close</DialogPrimitive.Close>
              </DialogPrimitive.Popup>
            </DialogPrimitive.Portal>
          </DialogPrimitive.Root>
        </div>
      )
    }
    testRoot.render(<Demo />)
    const renderer = testRoot.renderer
    const focused = () => renderer.getElement(renderer.getFocusedElementId())?.testId
    const step = (label) => log.push(`${label}: focus=${focused()} text=${renderer.getAllText().join("|")}`)
    const press = (keys) => {
      renderer.simulateKeystrokes(keys)
      step(keys)
    }

    press("Tab")
    press("Enter")
    press("Tab")
    press("Tab")
    press("Tab")
    press("Shift+Tab")
    press("Shift+Tab")
    press("ArrowDown")
    press("Escape")
    press("Escape")

    press("Enter")
    const popup = renderer.getElementBounds(renderer.findByTestId("popup").id)
    renderer.nativeSimulateClick(popup.x + popup.width - 4, popup.y + 4)
    step("press popup")
    renderer.nativeSimulateClick(4, 390)
    step("press backdrop")

    expect("\n" + log.join("\n")).toMatchInlineSnapshot(`
      "
      Tab: focus=open text=Open
      open=true
      Enter: focus=popup text=Open|Settings|Alpha|Close
      Tab: focus=select text=Open|Settings|Alpha|Close
      Tab: focus=close text=Open|Settings|Alpha|Close
      Tab: focus=select text=Open|Settings|Alpha|Close
      Shift+Tab: focus=close text=Open|Settings|Alpha|Close
      Shift+Tab: focus=select text=Open|Settings|Alpha|Close
      ArrowDown: focus=undefined text=Open|Settings|Alpha|Alpha|Beta|Close
      Escape: focus=select text=Open|Settings|Alpha|Close
      open=false
      Escape: focus=open text=Open
      open=true
      Enter: focus=popup text=Open|Settings|Alpha|Close
      press popup: focus=popup text=Open|Settings|Alpha|Close
      open=false
      press backdrop: focus=open text=Open"
    `)
  })

  describe("Dialog Backdrop dismissal", () => {
    // A click closes it, so the press and the release must both land on the backdrop.
    const press = (down, up) => {
      const closed = []
      testRoot.render(
        <DialogPrimitive.Root defaultOpen onOpenChange={(open) => closed.push(!open)}>
          <DialogPrimitive.Portal>
            <DialogPrimitive.Backdrop testId="backdrop" style={{ backgroundColor: "#00000080" }} />
            <DialogPrimitive.Popup testId="popup" style={{ width: 200, height: 100, backgroundColor: "#1e293b" }} />
          </DialogPrimitive.Portal>
        </DialogPrimitive.Root>
      )
      const renderer = testRoot.renderer
      const popup = renderer.getElementBounds(renderer.findByTestId("popup").id)
      const at = {
        backdrop: { x: 4, y: 4 },
        popup: { x: popup.x + popup.width / 2, y: popup.y + popup.height / 2 },
      }
      renderer.nativeSimulateMouseDown(at[down].x, at[down].y)
      const afterDown = closed.length
      renderer.nativeSimulateMouseUp(at[up].x, at[up].y)
      return { closedOnPress: afterDown > 0, closed: closed.length > 0 }
    }

    it("closes when the press and the release are both on the backdrop", () => {
      expect(press("backdrop", "backdrop")).toEqual({ closedOnPress: false, closed: true })
    })

    it("stays open when the press is on the backdrop and the release on the popup", () => {
      expect(press("backdrop", "popup")).toEqual({ closedOnPress: false, closed: false })
    })

    it("stays open when the press is in the popup and the release on the backdrop", () => {
      expect(press("popup", "backdrop")).toEqual({ closedOnPress: false, closed: false })
    })
  })

  it("sizes the Dialog portal to the window natively", () => {
    // No component reads the window size: native must fill the actual viewport.
    const root = createTestRoot({ width: 900, height: 500 })
    root.render(
      <DialogPrimitive.Root defaultOpen>
        <DialogPrimitive.Portal testId="portal">
          <DialogPrimitive.Backdrop testId="backdrop" style={{ backgroundColor: "#00000080" }} />
          <DialogPrimitive.Popup testId="popup" style={{ width: 200, height: 100, backgroundColor: "#1e293b" }} />
        </DialogPrimitive.Portal>
      </DialogPrimitive.Root>
    )
    const bounds = (testId) => root.renderer.getElementBounds(root.renderer.findByTestId(testId).id)
    // Windows substitutes display defaults for the offscreen window bounds.
    const { width, height } = root.renderer.getWindowSize()
    const viewport = { x: 0, y: 0, width, height }
    expect({
      portal: bounds("portal"),
      backdrop: bounds("backdrop"),
      popup: bounds("popup"),
    }).toMatchObject({
      portal: viewport,
      backdrop: viewport,
      popup: {
        width: 200,
        height: 100,
        // GPUI snaps half-pixel layout midpoints toward zero.
        x: Math.floor((width - 200) / 2),
        y: Math.floor((height - 100) / 2),
      },
    })
    root.unmount()
  })

  it("keeps a Dialog open when a handler prevents Escape", () => {
    testRoot.render(
      <DialogPrimitive.Root defaultOpen>
        <DialogPrimitive.Portal>
          <DialogPrimitive.Popup
            style={{ width: 200, height: 100, backgroundColor: "#1e293b" }}
            onKeyDown={(event) => {
              if (event.key === "Escape") event.preventDefault()
            }}
          >
            <text>Unsaved</text>
          </DialogPrimitive.Popup>
        </DialogPrimitive.Portal>
      </DialogPrimitive.Root>
    )
    testRoot.renderer.simulateKeystrokes("Escape")
    expect(testRoot.renderer.getAllText()).toEqual(["Unsaved"])
  })

  it("puts a nested layer above its parent even when both open in one commit", () => {
    // React runs the Select's layout effect before the Dialog's, so call order
    // would put the Dialog on top and one Escape would close both.
    testRoot.render(
      <DialogPrimitive.Root defaultOpen>
        <DialogPrimitive.Portal>
          <DialogPrimitive.Popup style={{ width: 300, padding: 16, backgroundColor: "#1e293b" }}>
            <text>Dialog</text>
            <Select defaultOpen items={[{ value: "a", label: "Alpha" }]} defaultValue="a">
              <SelectTrigger style={triggerStyle}><SelectValue /></SelectTrigger>
              <SelectContent style={contentStyle}>
                <SelectItem value="a" style={itemStyle}>Menu item</SelectItem>
              </SelectContent>
            </Select>
          </DialogPrimitive.Popup>
        </DialogPrimitive.Portal>
      </DialogPrimitive.Root>
    )
    const steps = [testRoot.renderer.getAllText().join("|")]
    testRoot.renderer.simulateKeystrokes("Escape")
    steps.push(testRoot.renderer.getAllText().join("|"))
    testRoot.renderer.simulateKeystrokes("Escape")
    steps.push(testRoot.renderer.getAllText().join("|"))
    expect(steps).toMatchInlineSnapshot(`
      [
        "Dialog|Alpha|Menu item",
        "Dialog|Alpha",
        "",
      ]
    `)
  })

  it("keeps a Combobox open when an ancestor keeps Tab", () => {
    testRoot.render(
      <div
        style={{ display: "flex", flexDirection: "column", width: 400, height: 300, padding: 12, gap: 8 }}
        onKeyDown={(event) => {
          if (event.key === "Tab") event.preventDefault()
        }}
      >
        <Combobox items={["alpha", "beta"]} defaultOpen>
          <ComboboxInput autoFocus testId="combo" style={{ width: 160, height: 28 }} />
          <ComboboxContent style={contentStyle}>
            <ComboboxList>
              {(item) => <ComboboxItem key={item} value={item}>{item}</ComboboxItem>}
            </ComboboxList>
          </ComboboxContent>
        </Combobox>
        <div tabIndex={0} testId="after" style={{ width: 80, height: 24 }} />
      </div>
    )
    testRoot.renderer.simulateKeystrokes("Tab")
    const focused = testRoot.renderer.getElement(testRoot.renderer.getFocusedElementId())
    expect({ focused: focused?.testId, text: testRoot.renderer.getAllText() }).toMatchInlineSnapshot(`
      {
        "focused": "combo",
        "text": [
          "alpha",
          "beta",
        ],
      }
    `)
  })

  it("focuses initialFocus on open and returns focus without a Trigger", () => {
    function Demo() {
      const [open, setOpen] = useState(false)
      const fieldRef = useRef(null)
      return (
        <div style={{ display: "flex", flexDirection: "column", width: 600, height: 400, padding: 12, gap: 8 }}>
          <div
            autoFocus
            tabIndex={0}
            testId="launcher"
            onKeyDown={(event) => {
              if (event.key === "Enter") setOpen(true)
            }}
            style={{ width: 80, height: 24 }}
          />
          <DialogPrimitive.Root open={open} onOpenChange={setOpen}>
            <DialogPrimitive.Portal>
              <DialogPrimitive.Popup initialFocus={fieldRef} style={{ width: 300, padding: 16, backgroundColor: "#1e293b" }}>
                <div tabIndex={0} testId="first" style={{ width: 60, height: 24 }} />
                <input ref={fieldRef} testId="field" style={{ width: 160, height: 28 }} />
              </DialogPrimitive.Popup>
            </DialogPrimitive.Portal>
          </DialogPrimitive.Root>
        </div>
      )
    }
    testRoot.render(<Demo />)
    const focused = () => testRoot.renderer.getElement(testRoot.renderer.getFocusedElementId())?.testId
    const steps = []
    testRoot.renderer.simulateKeystrokes("Enter")
    steps.push(`open: ${focused()}`)
    testRoot.renderer.simulateKeystrokes("Escape")
    steps.push(`close: ${focused()}`)
    expect(steps).toMatchInlineSnapshot(`
      [
        "open: field",
        "close: launcher",
      ]
    `)
  })

  it("moves focus in stack order when nested dialogs open and close together", () => {
    function Nested({ open, autoFocusField }) {
      return (
        <div style={{ display: "flex", flexDirection: "column", width: 600, height: 400, padding: 12, gap: 8 }}>
          <div autoFocus tabIndex={0} testId="launcher" style={{ width: 80, height: 24 }} />
          <DialogPrimitive.Root open={open.outer}>
            <DialogPrimitive.Portal>
              <DialogPrimitive.Popup testId="outer" style={{ width: 400, padding: 16, backgroundColor: "#1e293b" }}>
                <input autoFocus={autoFocusField} testId="outer-field" style={{ width: 120, height: 28 }} />
                <DialogPrimitive.Root open={open.inner}>
                  <DialogPrimitive.Portal>
                    <DialogPrimitive.Popup testId="inner" style={{ width: 200, height: 80, backgroundColor: "#334155" }} />
                  </DialogPrimitive.Portal>
                </DialogPrimitive.Root>
              </DialogPrimitive.Popup>
            </DialogPrimitive.Portal>
          </DialogPrimitive.Root>
        </div>
      )
    }
    const renderer = testRoot.renderer
    const focused = () => renderer.getElement(renderer.getFocusedElementId())?.testId
    const steps = []
    const show = (label, open, autoFocusField = false) => {
      testRoot.render(<Nested open={open} autoFocusField={autoFocusField} />)
      steps.push(`${label}: ${focused()}`)
    }
    show("mount", { outer: false, inner: false })
    show("open both", { outer: true, inner: true })
    show("close inner", { outer: true, inner: false })
    show("close outer", { outer: false, inner: false })
    show("open both", { outer: true, inner: true })
    show("close both", { outer: false, inner: false })
    show("open outer, field autoFocus", { outer: true, inner: false }, true)
    show("close outer", { outer: false, inner: false }, true)
    expect(steps).toMatchInlineSnapshot(`
      [
        "mount: launcher",
        "open both: inner",
        "close inner: outer",
        "close outer: launcher",
        "open both: inner",
        "close both: launcher",
        "open outer, field autoFocus: outer",
        "close outer: launcher",
      ]
    `)
  })

  it("drops a pending focus request when the user moves focus first", () => {
    function Demo({ late }) {
      return (
        <div style={{ display: "flex", flexDirection: "column", width: 400, height: 300, gap: 8 }}>
          <div autoFocus tabIndex={0} testId="a" style={{ width: 80, height: 24 }} />
          <div tabIndex={0} testId="b" style={{ width: 80, height: 24 }} />
          <div tabIndex={late ? 0 : undefined} testId="late" style={{ width: 80, height: 24 }} />
        </div>
      )
    }
    const renderer = testRoot.renderer
    testRoot.render(<Demo late={false} />)
    // No focus handle yet, so the request waits.
    renderer.focusElement(renderer.findByTestId("late").id)
    renderer.simulateKeystrokes("Tab")
    testRoot.render(<Demo late />)
    expect(renderer.getElement(renderer.getFocusedElementId())?.testId).toBe("b")
  })

  it("activates a Button like <button>: click, Enter down, Space up, disabled", () => {
    const log = []
    function Demo({ disabled }) {
      return (
        <div style={{ display: "flex", gap: 8, padding: 12 }}>
          <div autoFocus tabIndex={0} testId="start" style={{ width: 20, height: 20 }} />
          <Button testId="save" disabled={disabled} onClick={() => log.push("save")} style={{ width: 60, height: 24 }}>Save</Button>
          <Button testId="busy" disabled focusableWhenDisabled onClick={() => log.push("busy")} style={{ width: 60, height: 24 }}>Busy</Button>
          <div tabIndex={0} testId="end" style={{ width: 20, height: 20 }} />
        </div>
      )
    }
    const renderer = testRoot.renderer
    const focused = () => renderer.getElement(renderer.getFocusedElementId())?.testId
    testRoot.render(<Demo disabled={false} />)
    renderer.simulateKeystrokes("Tab")
    log.push(`focus ${focused()}`)
    renderer.simulateKeystrokes("Enter")
    renderer.nativeSimulateKeyDown(renderer.getFocusedElementId(), "Enter", true)
    // simulateKeystrokes sends no key up, so press and release by hand.
    renderer.nativeSimulateKeyDown(renderer.getFocusedElementId(), "Space")
    log.push("space down")
    renderer.nativeSimulateKeyUp(renderer.getFocusedElementId(), "Space")
    const box = renderer.getElementBounds(renderer.findByTestId("save").id)
    log.push("click")
    renderer.nativeSimulateClick(box.x + 4, box.y + 4)
    renderer.simulateKeystrokes("Tab")
    log.push(`focus ${focused()}`)
    renderer.simulateKeystrokes("Enter")

    testRoot.render(<Demo disabled />)
    renderer.focusElement(renderer.findByTestId("start").id)
    renderer.simulateKeystrokes("Tab")
    log.push(`focus ${focused()}`)
    renderer.nativeSimulateClick(box.x + 4, box.y + 4)
    expect(log).toMatchInlineSnapshot(`
      [
        "focus save",
        "save",
        "space down",
        "save",
        "click",
        "save",
        "focus busy",
        "focus busy",
      ]
    `)
  })

  it("delivers Enter and Space on a Button as a DOM click that bubbles", () => {
    const seen = []
    const record = (at) => (event) =>
      seen.push({
        at,
        detail: event.detail,
        pointerId: event.pointerId,
        pointerType: event.pointerType,
        target: event.target,
        currentTarget: event.currentTarget,
      })
    let buttonElement
    let wrapperElement
    testRoot.render(
      <div ref={(element) => (wrapperElement = element)} testId="wrapper" style={{ display: "flex" }} onClick={record("wrapper")}>
        <Button ref={(element) => (buttonElement = element)} testId="save" autoFocus onClick={record("button")} style={{ width: 60, height: 24 }}>Save</Button>
      </div>
    )
    const renderer = testRoot.renderer
    const button = renderer.findByTestId("save").id
    renderer.nativeSimulateKeyDown(button, "Enter")
    expect(seen.map((entry) => [entry.at, entry.detail, entry.pointerId, entry.pointerType])).toEqual([
      ["button", 0, -1, ""],
      ["wrapper", 0, -1, ""],
    ])
    expect(seen[0].target).toBe(buttonElement)
    expect(seen[0].currentTarget).toBe(buttonElement)
    expect(seen[1].target).toBe(buttonElement)
    expect(seen[1].currentTarget).toBe(wrapperElement)
    seen.length = 0
    renderer.nativeSimulateKeyDown(button, "Space")
    expect(seen).toEqual([])
    renderer.nativeSimulateKeyUp(button, "Space")
    expect(seen.map((entry) => entry.at)).toEqual(["button", "wrapper"])
  })

  it("clicks a nested Button once on Enter and Space, and its ancestor by bubbling only", () => {
    const seen = []
    testRoot.render(
      <Button testId="outer" onClick={() => seen.push("outer")} style={{ padding: 8 }}>
        <Button testId="inner" autoFocus onClick={() => seen.push("inner")} style={{ width: 60, height: 24 }}>Inner</Button>
      </Button>
    )
    const renderer = testRoot.renderer
    const inner = renderer.findByTestId("inner").id
    renderer.nativeSimulateKeyDown(inner, "Enter")
    expect(seen).toEqual(["inner", "outer"])
    seen.length = 0
    renderer.nativeSimulateKeyDown(inner, "Space")
    renderer.nativeSimulateKeyUp(inner, "Space")
    expect(seen).toEqual(["inner", "outer"])
  })
})
