import { createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import {
  Button,
  Combobox,
  ComboboxContent,
  Dialog,
  DialogBackdrop,
  DialogClose,
  DialogPopup,
  DialogPortal,
  DialogTitle,
  DialogTrigger,
  ComboboxInput,
  ComboboxItem,
  ComboboxList,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { HostElement } from "../host.js"

describe.skipIf(!hasNativeTestRenderer)("Solid controls", () => {
  it("selects with keyboard and click", () => {
    const [value, setValue] = createSignal("a")
    const app = createTestRoot()
    app.render(() => (
      <Select value={value()} onValueChange={setValue}>
        <SelectTrigger testId="trigger" style={{ width: 120, height: 32, backgroundColor: "#333333" }}>
          <text>open</text>
        </SelectTrigger>
        <SelectContent style={{ width: 120, backgroundColor: "#222222" }}>
          <SelectItem value="a"><text>A</text></SelectItem>
          <SelectItem value="b" testId="b" style={{ height: 28 }}><text>B</text></SelectItem>
        </SelectContent>
      </Select>
    ))
    const trigger = app.renderer.findByTestId("trigger")!
    app.renderer.nativeSimulateKeyDown(trigger.id, "ArrowDown")
    app.renderer.simulateKeystrokes("ArrowDown Enter")
    expect(value()).toBe("b")

    app.renderer.nativeSimulateClick(60, 16)
    const item = app.renderer.findByTestId("b")!
    const bounds = app.renderer.getElementBounds(item.id)!
    app.renderer.nativeSimulateClick(bounds.x + 10, bounds.y + 10)
    expect(value()).toBe("b")
  })

  it("closes a Select from its trigger without reopening on the same press", () => {
    const app = createTestRoot()
    app.render(() => (
      <Select defaultValue="a">
        <SelectTrigger testId="trigger" style={{ width: 120, height: 32, backgroundColor: "#333333" }}>
          <text>open</text>
        </SelectTrigger>
        <SelectContent style={{ width: 120, backgroundColor: "#222222" }}>
          <SelectItem value="a"><text>A</text></SelectItem>
          <SelectItem value="b"><text>B</text></SelectItem>
        </SelectContent>
      </Select>
    ))
    app.renderer.nativeSimulateClick(60, 16)
    expect(app.renderer.getAllText()).toContain("B")
    app.renderer.nativeSimulateClick(60, 16)
    expect(app.renderer.getAllText()).toEqual(["open"])
  })

  it("filters and submits a combobox item", () => {
    const [value, setValue] = createSignal<string | string[] | null>(null)
    const app = createTestRoot()
    app.render(() => (
      <Combobox items={["alpha", "beta"]} value={value()} onValueChange={setValue} defaultOpen>
        <ComboboxInput testId="input" style={{ width: 180, height: 32 }} />
        <ComboboxContent>
          <ComboboxItem value="alpha"><text>alpha</text></ComboboxItem>
          <ComboboxItem value="beta"><text>beta</text></ComboboxItem>
        </ComboboxContent>
      </Combobox>
    ))
    const input = app.renderer.findByTestId("input")!
    app.renderer.nativeSimulateKeyDown(input.id, "ArrowDown")
    app.renderer.nativeSimulateKeystrokes(input.id, "Enter")
    expect(value()).toBe("alpha")
  })

  it("opens tooltip through a real hover event and closes on click", () => {
    const app = createTestRoot()
    app.render(() => (
      <Tooltip>
        <TooltipTrigger testId="tip-trigger" style={{ width: 100, height: 32, backgroundColor: "#333333" }}>
          <text>hover</text>
        </TooltipTrigger>
        <TooltipContent><text>help</text></TooltipContent>
      </Tooltip>
    ))
    const trigger = app.renderer.findByTestId("tip-trigger")!
    const bounds = app.renderer.getElementBounds(trigger.id)!
    app.renderer.nativeSimulateMouseMove(bounds.x + 10, bounds.y + 10)
    expect(app.renderer.getAllText()).toContain("help")
    app.renderer.nativeSimulateClick(bounds.x + 10, bounds.y + 10)
    expect(app.renderer.getAllText()).not.toContain("help")
  })

  it("moves focus with Tab through asChild triggers and keeps child handlers", () => {
    const keys: string[] = []
    const app = createTestRoot()
    app.render(() => (
      <div style={{ display: "flex", width: 500, height: 300, gap: 40 }}>
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <div autoFocus tabIndex={0} testId="before" style={{ width: 80, height: 24 }} />
          <Select defaultValue="a">
            <SelectTrigger asChild>
              <div
                testId="trigger"
                onKeyDown={(event) => keys.push(`child:${event.key}`)}
                style={{ width: 120, height: 32, backgroundColor: "#333333" }}
              >
                <text>open</text>
              </div>
            </SelectTrigger>
            <SelectContent testId="content" style={{ width: 120, backgroundColor: "#222222" }}>
              <SelectItem value="a"><text>A</text></SelectItem>
              <SelectItem value="b"><text>B</text></SelectItem>
            </SelectContent>
          </Select>
          <Tooltip>
            <TooltipTrigger asChild>
              <div testId="tip" style={{ width: 80, height: 24 }}><text>tip</text></div>
            </TooltipTrigger>
            <TooltipContent><text>help</text></TooltipContent>
          </Tooltip>
        </div>
        <input testId="outside" style={{ width: 120, height: 28 }} />
      </div>
    ))
    const focused = () => app.renderer.getElement(app.renderer.getFocusedElementId()!)?.testId
    const steps: string[] = []
    const step = (label: string) => steps.push(`${label}: focus=${focused()} text=${app.renderer.getAllText().join("|")}`)

    app.renderer.simulateKeystrokes("Tab")
    step("tab")
    app.renderer.simulateKeystrokes("Enter")
    step("enter")
    app.renderer.simulateKeystrokes("Tab")
    step("tab in popup")
    app.renderer.simulateKeystrokes("Escape")
    step("escape")
    app.renderer.simulateKeystrokes("Tab")
    step("tab")
    const trigger = app.renderer.getElementBounds(app.renderer.findByTestId("trigger")!.id)!
    app.renderer.nativeSimulateClick(trigger.x + 10, trigger.y + 10)
    const outside = app.renderer.getElementBounds(app.renderer.findByTestId("outside")!.id)!
    app.renderer.nativeSimulateClick(outside.x + 10, outside.y + 10)
    step("click outside")

    expect(steps).toEqual([
      "tab: focus=trigger text=open|tip",
      "enter: focus=content text=open|A|B|tip",
      "tab in popup: focus=content text=open|A|B|tip",
      "escape: focus=trigger text=open|tip",
      "tab: focus=tip text=open|tip|help",
      "click outside: focus=outside text=open|tip",
    ])
    // The child's own onKeyDown still runs after asChild merges the trigger's.
    expect(keys).toEqual(["child:Enter", "child:Tab"])
  })

  it("runs a modal Dialog: trap Tab, Escape closes the nested Select first", () => {
    const app = createTestRoot()
    app.render(() => (
      <div style={{ display: "flex", flexDirection: "column", width: 600, height: 400, gap: 8 }}>
        <Dialog>
          <DialogTrigger testId="open" style={{ width: 80, height: 24 }}><text>Open</text></DialogTrigger>
          <DialogPortal>
            <DialogBackdrop style={{ backgroundColor: "#00000080" }} />
            <DialogPopup testId="popup" style={{ width: 300, padding: 16, gap: 8, backgroundColor: "#1e293b" }}>
              <DialogTitle><text>Settings</text></DialogTitle>
              <Select defaultValue="a">
                <SelectTrigger testId="select" style={{ width: 120, height: 32, backgroundColor: "#333333" }}>
                  <text>pick</text>
                </SelectTrigger>
                <SelectContent style={{ width: 120, backgroundColor: "#222222" }}>
                  <SelectItem value="a"><text>A</text></SelectItem>
                </SelectContent>
              </Select>
              <DialogClose testId="close" style={{ width: 60, height: 24 }}><text>Close</text></DialogClose>
            </DialogPopup>
          </DialogPortal>
        </Dialog>
      </div>
    ))
    const focused = () => app.renderer.getElement(app.renderer.getFocusedElementId()!)?.testId
    const steps: string[] = []
    const press = (keys: string) => {
      app.renderer.simulateKeystrokes(keys)
      steps.push(`${keys}: focus=${focused()} text=${app.renderer.getAllText().join("|")}`)
    }
    app.renderer.focusElement(app.renderer.findByTestId("open")!.id)
    press("Enter")
    press("Tab")
    press("Tab")
    press("Tab")
    press("ArrowDown")
    press("Escape")
    press("Escape")
    press("Enter")
    const popup = app.renderer.getElementBounds(app.renderer.findByTestId("popup")!.id)!
    app.renderer.nativeSimulateClick(popup.x + popup.width - 4, popup.y + 4)
    steps.push(`press popup: text=${app.renderer.getAllText().join("|")}`)
    app.renderer.nativeSimulateClick(4, 390)
    steps.push(`press backdrop: focus=${focused()} text=${app.renderer.getAllText().join("|")}`)
    expect(steps).toEqual([
      "Enter: focus=popup text=Open|Settings|pick|Close",
      "Tab: focus=select text=Open|Settings|pick|Close",
      "Tab: focus=close text=Open|Settings|pick|Close",
      "Tab: focus=select text=Open|Settings|pick|Close",
      "ArrowDown: focus=undefined text=Open|Settings|pick|A|Close",
      "Escape: focus=select text=Open|Settings|pick|Close",
      "Escape: focus=open text=Open",
      "Enter: focus=popup text=Open|Settings|pick|Close",
      "press popup: text=Open|Settings|pick|Close",
      "press backdrop: focus=open text=Open",
    ])
  })

  describe("DialogBackdrop dismissal", () => {
    // A click closes it, so the press and the release must both land on the backdrop.
    const press = (down: "backdrop" | "popup", up: "backdrop" | "popup") => {
      const closed: boolean[] = []
      const app = createTestRoot()
      app.render(() => (
        <Dialog defaultOpen onOpenChange={(open) => closed.push(!open)}>
          <DialogPortal>
            <DialogBackdrop style={{ backgroundColor: "#00000080" }} />
            <DialogPopup testId="popup" style={{ width: 200, height: 100, backgroundColor: "#1e293b" }} />
          </DialogPortal>
        </Dialog>
      ))
      const popup = app.renderer.getElementBounds(app.renderer.findByTestId("popup")!.id)!
      const at = {
        backdrop: { x: 4, y: 4 },
        popup: { x: popup.x + popup.width / 2, y: popup.y + popup.height / 2 },
      }
      app.renderer.nativeSimulateMouseDown(at[down].x, at[down].y)
      const afterDown = closed.length
      app.renderer.nativeSimulateMouseUp(at[up].x, at[up].y)
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

  it("keeps the DialogPopup element and its focus when a prop it renders changes", () => {
    const [mark, setMark] = createSignal("a")
    const [color, setColor] = createSignal("#1e293b")
    const refs: HostElement[] = []
    const app = createTestRoot()
    app.render(() => (
      <Dialog defaultOpen>
        <DialogPortal>
          <DialogPopup
            testId="popup"
            data-mark={mark()}
            ref={(element: HostElement) => refs.push(element)}
            style={{ width: 200, padding: 8, backgroundColor: color() }}
          >
            <input testId="field" style={{ width: 100, height: 24 }} />
          </DialogPopup>
        </DialogPortal>
      </Dialog>
    ))
    const field = app.renderer.findByTestId("field")!.id
    const popup = app.renderer.findByTestId("popup")!.id
    app.renderer.focusElement(field)
    app.flushSync(() => setMark("b"))
    app.flushSync(() => setColor("#ff0000"))
    expect({
      popup: app.renderer.findByTestId("popup")!.id,
      field: app.renderer.findByTestId("field")!.id,
      focused: app.renderer.getFocusedElementId(),
      built: refs.length,
    }).toEqual({ popup, field, focused: field, built: 1 })
  })

  it("keeps an open Select, Combobox and Tooltip content element when a prop it renders changes", () => {
    const [color, setColor] = createSignal("#222222")
    const style = () => ({ width: 120, backgroundColor: color() })
    const app = createTestRoot()
    app.render(() => (
      <div style={{ display: "flex", flexDirection: "column", gap: 8, width: 400, height: 400 }}>
        <Select defaultOpen defaultValue="a">
          <SelectTrigger style={{ width: 120, height: 32 }}><text>pick</text></SelectTrigger>
          <SelectContent testId="select-content" style={style()}>
            <SelectItem value="a"><text>A</text></SelectItem>
          </SelectContent>
        </Select>
        <Combobox defaultOpen items={["alpha"]}>
          <ComboboxInput style={{ width: 120, height: 32 }} />
          <ComboboxContent testId="combobox-content" style={style()}>
            <ComboboxList>{(item: string) => <ComboboxItem value={item}><text>{item}</text></ComboboxItem>}</ComboboxList>
          </ComboboxContent>
        </Combobox>
        <Tooltip defaultOpen>
          <TooltipTrigger style={{ width: 60, height: 24 }}><text>tip</text></TooltipTrigger>
          <TooltipContent testId="tooltip-content" style={style()}><text>hint</text></TooltipContent>
        </Tooltip>
      </div>
    ))
    const ids = () => ["select-content", "combobox-content", "tooltip-content"]
      .map((testId) => app.renderer.findByTestId(testId)!.id)
    const before = ids()
    app.flushSync(() => setColor("#ff0000"))
    expect(ids()).toEqual(before)
  })

  it("focuses initialFocus on open and returns focus without a Trigger", () => {
    const [open, setOpen] = createSignal(false)
    let field!: HostElement
    const app = createTestRoot()
    app.render(() => (
      <div style={{ display: "flex", flexDirection: "column", width: 600, height: 400, gap: 8 }}>
        <div
          autoFocus
          tabIndex={0}
          testId="launcher"
          onKeyDown={(event) => {
            if (event.key === "Enter") setOpen(true)
          }}
          style={{ width: 80, height: 24 }}
        />
        <Dialog open={open()} onOpenChange={setOpen}>
          <DialogPortal>
            <DialogPopup initialFocus={() => field} style={{ width: 300, padding: 16, backgroundColor: "#1e293b" }}>
              <div tabIndex={0} testId="first" style={{ width: 60, height: 24 }} />
              <input ref={field} testId="field" style={{ width: 160, height: 28 }} />
            </DialogPopup>
          </DialogPortal>
        </Dialog>
      </div>
    ))
    const focused = () => app.renderer.getElement(app.renderer.getFocusedElementId()!)?.testId
    const steps: string[] = []
    app.renderer.simulateKeystrokes("Enter")
    steps.push(`open: ${focused()}`)
    app.renderer.simulateKeystrokes("Escape")
    steps.push(`close: ${focused()}`)
    expect(steps).toEqual(["open: field", "close: launcher"])
  })

  it("activates a Button on Enter down and Space up, and skips it while disabled", () => {
    const log: string[] = []
    const [disabled, setDisabled] = createSignal(false)
    const app = createTestRoot()
    app.render(() => (
      <div style={{ display: "flex", gap: 8 }}>
        <div autoFocus tabIndex={0} testId="start" style={{ width: 20, height: 20 }} />
        <Button testId="save" disabled={disabled()} onClick={() => log.push("save")} style={{ width: 60, height: 24 }}>
          <text>Save</text>
        </Button>
        <div tabIndex={0} testId="end" style={{ width: 20, height: 20 }} />
      </div>
    ))
    const focused = () => app.renderer.getElement(app.renderer.getFocusedElementId()!)?.testId
    app.renderer.simulateKeystrokes("Tab")
    log.push(`focus ${focused()}`)
    app.renderer.simulateKeystrokes("Enter")
    app.renderer.nativeSimulateKeyDown(app.renderer.getFocusedElementId()!, "Space")
    log.push("space down")
    app.renderer.nativeSimulateKeyUp(app.renderer.getFocusedElementId()!, "Space")
    app.flushSync(() => setDisabled(true))
    app.renderer.focusElement(app.renderer.findByTestId("start")!.id)
    app.renderer.simulateKeystrokes("Tab")
    log.push(`focus ${focused()}`)
    expect(log).toEqual(["focus save", "save", "space down", "save", "focus end"])
  })

  it("delivers Enter and Space on a Button as a DOM click that bubbles", () => {
    const seen: Array<{ at: string; detail?: number; pointerId?: number; pointerType?: string; target?: object; currentTarget?: object | null }> = []
    let buttonElement!: HostElement
    let wrapperElement!: HostElement
    const app = createTestRoot()
    app.render(() => (
      <div
        ref={wrapperElement}
        testId="wrapper"
        style={{ display: "flex" }}
        onClick={(event) => seen.push({ at: "wrapper", detail: event.detail, pointerId: event.pointerId, pointerType: event.pointerType, target: event.target, currentTarget: event.currentTarget })}
      >
        <Button
          ref={buttonElement}
          testId="save"
          autoFocus
          onClick={(event) => seen.push({ at: "button", detail: event.detail, pointerId: event.pointerId, pointerType: event.pointerType, target: event.target, currentTarget: event.currentTarget })}
          style={{ width: 60, height: 24 }}
        >
          <text>Save</text>
        </Button>
      </div>
    ))
    const button = app.renderer.findByTestId("save")!.id
    app.renderer.nativeSimulateKeyDown(button, "Enter")
    expect(seen.map((entry) => [entry.at, entry.detail, entry.pointerId, entry.pointerType])).toEqual([
      ["button", 0, -1, ""],
      ["wrapper", 0, -1, ""],
    ])
    expect(seen[0]!.target).toBe(buttonElement)
    expect(seen[0]!.currentTarget).toBe(buttonElement)
    expect(seen[1]!.target).toBe(buttonElement)
    expect(seen[1]!.currentTarget).toBe(wrapperElement)
  })

  it("clicks a nested Button once on Enter and Space, and its ancestor by bubbling only", () => {
    const seen: string[] = []
    const app = createTestRoot()
    app.render(() => (
      <Button testId="outer" onClick={() => seen.push("outer")} style={{ padding: 8 }}>
        <Button testId="inner" autoFocus onClick={() => seen.push("inner")} style={{ width: 60, height: 24 }}>
          <text>Inner</text>
        </Button>
      </Button>
    ))
    const inner = app.renderer.findByTestId("inner")!.id
    app.renderer.nativeSimulateKeyDown(inner, "Enter")
    expect(seen).toEqual(["inner", "outer"])
    seen.length = 0
    app.renderer.nativeSimulateKeyDown(inner, "Space")
    app.renderer.nativeSimulateKeyUp(inner, "Space")
    expect(seen).toEqual(["inner", "outer"])
  })

  it("updates a Button, DialogBackdrop and ComboboxInput host props in place when a reactive prop changes", () => {
    const [mark, setMark] = createSignal("a")
    const refs: Record<string, HostElement[]> = { button: [], input: [], backdrop: [] }
    const app = createTestRoot()
    app.render(() => (
      <div style={{ display: "flex", flexDirection: "column", gap: 8, width: 400, height: 400 }}>
        <Button testId="button" ref={(element: HostElement) => refs.button.push(element)} data-mark={mark()} style={{ width: 60, height: 24 }}><text>Save</text></Button>
        <Combobox items={["alpha"]}>
          <ComboboxInput testId="input" ref={(element: HostElement) => refs.input.push(element)} data-mark={mark()} style={{ width: 120, height: 32 }} />
        </Combobox>
        <Dialog defaultOpen>
          <DialogPortal>
            <DialogBackdrop testId="backdrop" ref={(element: HostElement) => refs.backdrop.push(element)} data-mark={mark()} />
          </DialogPortal>
        </Dialog>
      </div>
    ))
    const read = () => Object.values(refs).map((elements) => ({
      built: elements.length,
      mark: elements.at(-1)!.props.get("data-mark"),
    }))
    expect(read()).toEqual([{ built: 1, mark: "a" }, { built: 1, mark: "a" }, { built: 1, mark: "a" }])
    app.flushSync(() => setMark("b"))
    expect(read()).toEqual([{ built: 1, mark: "b" }, { built: 1, mark: "b" }, { built: 1, mark: "b" }])
  })

  it("clicks from a host ref like HTMLElement.click()", () => {
    const seen: Array<{ detail?: number; pointerId?: number; target?: object }> = []
    let row!: HostElement
    const app = createTestRoot()
    app.render(() => (
      <div
        ref={row}
        style={{ width: 60, height: 24 }}
        onClick={(event) => seen.push({ detail: event.detail, pointerId: event.pointerId, target: event.target })}
      />
    ))
    app.renderer.flush()
    row.click()
    app.renderer.dispatchNativeEvents()
    expect(seen).toHaveLength(1)
    expect(seen[0]!.detail).toBe(0)
    expect(seen[0]!.pointerId).toBe(-1)
    expect(seen[0]!.target).toBe(row)
  })

  it("does nothing from click() on a disabled form control", () => {
    const seen: string[] = []
    let field!: HostElement
    const app = createTestRoot()
    app.render(() => (
      <div style={{ width: 200, height: 60 }} onClick={() => seen.push("parent")}>
        <input
          ref={field}
          value=""
          disabled
          onClick={() => seen.push("field")}
          style={{ width: 100, height: 24 }}
        />
      </div>
    ))
    app.renderer.flush()
    field.click()
    app.renderer.dispatchNativeEvents()
    expect(seen).toEqual([])
  })

  it("builds DialogPortal children once", () => {
    let built = 0
    const Probe = () => {
      built += 1
      return <text>probe</text>
    }
    const [open, setOpen] = createSignal(false)
    const app = createTestRoot()
    app.render(() => (
      <Dialog open={open()} onOpenChange={setOpen}>
        <DialogPortal>
          <Probe />
        </DialogPortal>
      </Dialog>
    ))
    expect(built).toBe(0)
    setOpen(true)
    expect(built).toBe(1)
  })

  it("builds floating content children once", () => {
    let built = 0
    const Probe = () => {
      built += 1
      return <text>probe</text>
    }
    const app = createTestRoot()
    app.render(() => (
      <Tooltip open>
        <TooltipTrigger testId="trigger" style={{ width: 40, height: 20 }} />
        <TooltipContent><Probe /></TooltipContent>
      </Tooltip>
    ))
    expect(built).toBe(1)
  })
})
