/**
 * Popups opened by a pointer press on their trigger. DismissableLayer reads the focused element
 * while it mounts, which is inside the press's own dispatch: the root view must not be leased
 * there, as it is not in a window. A lease made the read throw VIEW_BUSY, which onError reports.
 */
import { createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import {
  Combobox,
  ComboboxContent,
  ComboboxInput,
  ComboboxItem,
  ComboboxList,
  ComboboxTrigger,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
} from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { TestRoot } from "../testing.js"

const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 0))

function center(app: TestRoot, testId: string): { x: number; y: number } {
  const bounds = app.renderer.getElementBounds(app.renderer.findByTestId(testId)!.id)!
  return { x: bounds.x + bounds.width / 2, y: bounds.y + bounds.height / 2 }
}

const triggerStyle = { width: 120, height: 32, backgroundColor: "#333333" }

describe.skipIf(!hasNativeTestRenderer)("a Select opened by a press on its trigger", () => {
  const scene = (errors: unknown[]) => {
    const app = createTestRoot({ onError: (error) => errors.push(error) })
    app.render(() => (
      <div style={{ width: 400, height: 300 }}>
        <input testId="before" style={{ width: 100, height: 20 }} />
        <Select defaultValue="a">
          <SelectTrigger testId="trigger" style={triggerStyle}>
            <text>open</text>
          </SelectTrigger>
          <SelectContent testId="content" style={{ width: 120, backgroundColor: "#222222" }}>
            <SelectItem value="a"><text>A</text></SelectItem>
            <SelectItem value="b"><text>B</text></SelectItem>
          </SelectContent>
        </Select>
      </div>
    ))
    return app
  }

  it("opens on nativeSimulateClick and focuses the popup", () => {
    const errors: unknown[] = []
    const app = scene(errors)
    try {
      const point = center(app, "trigger")
      app.renderer.nativeSimulateClick(point.x, point.y)

      expect(errors).toEqual([])
      const content = app.renderer.findByTestId("content")
      expect(content).toBeDefined()
      expect(app.renderer.getFocusedElementId()).toBe(content!.id)
    } finally {
      app.unmount()
    }
  })

  it("opens across a separate mousedown and mouseup, and Escape returns focus to the trigger", () => {
    const errors: unknown[] = []
    const app = scene(errors)
    try {
      const point = center(app, "trigger")
      app.renderer.nativeSimulateMouseDown(point.x, point.y)
      expect(errors).toEqual([])
      expect(app.renderer.findByTestId("content")).toBeUndefined()

      app.renderer.nativeSimulateMouseUp(point.x, point.y)
      expect(errors).toEqual([])
      const content = app.renderer.findByTestId("content")
      expect(content).toBeDefined()
      expect(app.renderer.getFocusedElementId()).toBe(content!.id)

      app.renderer.simulateKeystrokes("Escape")
      expect(errors).toEqual([])
      expect(app.renderer.findByTestId("content")).toBeUndefined()
      expect(app.renderer.getFocusedElementId()).toBe(app.renderer.findByTestId("trigger")!.id)
    } finally {
      app.unmount()
    }
  })

  // As in the DOM: the popup's autoFocus already ran at commit, so the press's own
  // focus default would move focus back to the trigger unless mousedown cancels it.
  for (const cancelsDefault of [true, false]) {
    it(`opens when the trigger opens it from onMouseDown and ${cancelsDefault ? "cancels the press default, so the popup keeps focus" : "does not cancel it, so the trigger takes focus"}`, () => {
      const errors: unknown[] = []
      const app = createTestRoot({ onError: (error) => errors.push(error) })
      try {
        const [open, setOpen] = createSignal(false)
        app.render(() => (
          <Select open={open()} onOpenChange={setOpen} defaultValue="a">
            <SelectTrigger
              testId="trigger"
              style={triggerStyle}
              onMouseDown={(event) => {
                if (cancelsDefault) event.preventDefault()
                setOpen(true)
              }}
            >
              <text>open</text>
            </SelectTrigger>
            <SelectContent testId="content" style={{ width: 120, backgroundColor: "#222222" }}>
              <SelectItem value="a"><text>A</text></SelectItem>
            </SelectContent>
          </Select>
        ))
        const point = center(app, "trigger")

        app.renderer.nativeSimulateMouseDown(point.x, point.y)

        expect(errors).toEqual([])
        const content = app.renderer.findByTestId("content")
        expect(content).toBeDefined()
        const focused = cancelsDefault ? content! : app.renderer.findByTestId("trigger")!
        expect(app.renderer.getFocusedElementId()).toBe(focused.id)
      } finally {
        app.unmount()
      }
    })
  }
})

describe.skipIf(!hasNativeTestRenderer)("a Combobox opened by a press", () => {
  const scene = (errors: unknown[]) => {
    const app = createTestRoot({ onError: (error) => errors.push(error) })
    app.render(() => (
      <Combobox items={["alpha", "beta"]}>
        <div style={{ display: "flex", flexDirection: "row", width: 400, height: 40 }}>
          <ComboboxInput testId="input" style={{ width: 180, height: 32 }} />
          <ComboboxTrigger testId="trigger" style={{ width: 40, height: 32, backgroundColor: "#333333" }}>
            <text>v</text>
          </ComboboxTrigger>
        </div>
        <ComboboxContent testId="content" style={{ width: 180, backgroundColor: "#222222" }}>
          <ComboboxList>
            {(item: string) => <ComboboxItem value={item}><text>{item}</text></ComboboxItem>}
          </ComboboxList>
        </ComboboxContent>
      </Combobox>
    ))
    return app
  }

  it("opens on a click in the input and keeps focus there", async () => {
    const errors: unknown[] = []
    const app = scene(errors)
    try {
      const point = center(app, "input")
      app.renderer.nativeSimulateClick(point.x, point.y)
      await settle()

      expect(errors).toEqual([])
      expect(app.renderer.findByTestId("content")).toBeDefined()
      expect(app.renderer.getFocusedElementId()).toBe(app.renderer.findByTestId("input")!.id)
    } finally {
      app.unmount()
    }
  })

  it("opens across a separate mousedown and mouseup in the input", async () => {
    const errors: unknown[] = []
    const app = scene(errors)
    try {
      const point = center(app, "input")
      app.renderer.nativeSimulateMouseDown(point.x, point.y)
      app.renderer.nativeSimulateMouseUp(point.x, point.y)
      await settle()

      expect(errors).toEqual([])
      expect(app.renderer.findByTestId("content")).toBeDefined()
      expect(app.renderer.getFocusedElementId()).toBe(app.renderer.findByTestId("input")!.id)
    } finally {
      app.unmount()
    }
  })

  it("opens on a click on the trigger and moves focus into the input", async () => {
    const errors: unknown[] = []
    const app = scene(errors)
    try {
      const point = center(app, "trigger")
      app.renderer.nativeSimulateClick(point.x, point.y)
      await settle()

      expect(errors).toEqual([])
      expect(app.renderer.findByTestId("content")).toBeDefined()
      expect(app.renderer.getFocusedElementId()).toBe(app.renderer.findByTestId("input")!.id)
    } finally {
      app.unmount()
    }
  })
})
