/**
 * Popups opened by a pointer press on their trigger. DismissableLayer reads the focused element
 * while it mounts, which is inside the press's own dispatch: the root view must not be leased
 * there, as it is not in a window. A lease made the read throw VIEW_BUSY, which onError reports.
 */
// @ts-nocheck

import { useState } from "react"
import { describe, expect, it } from "vitest"
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
  SelectValue,
} from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 0))

function createApp() {
  const errors: unknown[] = []
  const app = createTestRoot({
    onError: (error) => errors.push(error),
    onUncaughtError: (error) => errors.push(error),
  })
  return { app, errors }
}

function center(app, testId: string) {
  const bounds = app.renderer.getElementBounds(app.renderer.findByTestId(testId).id)
  return { x: bounds.x + bounds.width / 2, y: bounds.y + bounds.height / 2 }
}

const triggerStyle = { width: 120, height: 32, backgroundColor: "#333333" }
const contentStyle = { width: 120, backgroundColor: "#222222" }

function SelectScene({ opensOnMouseDown = false }) {
  const [open, setOpen] = useState(false)
  return (
    <div style={{ width: 400, height: 300 }}>
      <input testId="before" style={{ width: 100, height: 20 }} />
      <Select
        items={[{ value: "a", label: "A" }, { value: "b", label: "B" }]}
        defaultValue="a"
        {...(opensOnMouseDown ? { open, onOpenChange: setOpen } : {})}
      >
        <SelectTrigger
          testId="trigger"
          style={triggerStyle}
          onMouseDown={opensOnMouseDown ? () => setOpen(true) : undefined}
        >
          <SelectValue />
        </SelectTrigger>
        <SelectContent testId="content" style={contentStyle}>
          <SelectItem value="a">A</SelectItem>
          <SelectItem value="b">B</SelectItem>
        </SelectContent>
      </Select>
    </div>
  )
}

describeNative("a Select opened by a press on its trigger", () => {
  it("opens on nativeSimulateClick and focuses the popup", () => {
    const { app, errors } = createApp()
    try {
      app.render(<SelectScene />)
      const point = center(app, "trigger")

      app.renderer.nativeSimulateClick(point.x, point.y)

      expect(errors).toEqual([])
      const content = app.renderer.findByTestId("content")
      expect(content).toBeDefined()
      expect(app.renderer.getFocusedElementId()).toBe(content.id)
    } finally {
      app.unmount()
    }
  })

  it("opens across a separate mousedown and mouseup, and Escape returns focus to the trigger", () => {
    const { app, errors } = createApp()
    try {
      app.render(<SelectScene />)
      const point = center(app, "trigger")

      app.renderer.nativeSimulateMouseDown(point.x, point.y)
      expect(errors).toEqual([])
      expect(app.renderer.findByTestId("content")).toBeUndefined()

      app.renderer.nativeSimulateMouseUp(point.x, point.y)
      expect(errors).toEqual([])
      const content = app.renderer.findByTestId("content")
      expect(content).toBeDefined()
      expect(app.renderer.getFocusedElementId()).toBe(content.id)

      app.renderer.simulateKeystrokes("Escape")
      expect(errors).toEqual([])
      expect(app.renderer.findByTestId("content")).toBeUndefined()
      expect(app.renderer.getFocusedElementId()).toBe(app.renderer.findByTestId("trigger").id)
    } finally {
      app.unmount()
    }
  })

  it("opens when the trigger opens it from onMouseDown", () => {
    const { app, errors } = createApp()
    try {
      app.render(<SelectScene opensOnMouseDown />)
      const point = center(app, "trigger")

      app.renderer.nativeSimulateMouseDown(point.x, point.y)

      expect(errors).toEqual([])
      const content = app.renderer.findByTestId("content")
      expect(content).toBeDefined()
      expect(app.renderer.getFocusedElementId()).toBe(content.id)
    } finally {
      app.unmount()
    }
  })
})

function ComboboxScene() {
  return (
    <Combobox items={["alpha", "beta"]}>
      <div style={{ display: "flex", flexDirection: "row", width: 400, height: 40 }}>
        <ComboboxInput testId="input" style={{ width: 180, height: 32 }} />
        <ComboboxTrigger testId="trigger" style={{ width: 40, height: 32, backgroundColor: "#333333" }}>
          <text>v</text>
        </ComboboxTrigger>
      </div>
      <ComboboxContent testId="content" style={{ width: 180, backgroundColor: "#222222" }}>
        <ComboboxList>
          {(item) => <ComboboxItem key={item} value={item}>{item}</ComboboxItem>}
        </ComboboxList>
      </ComboboxContent>
    </Combobox>
  )
}

describeNative("a Combobox opened by a press", () => {
  it("opens on a click in the input and keeps focus there", async () => {
    const { app, errors } = createApp()
    try {
      app.render(<ComboboxScene />)
      const point = center(app, "input")

      app.renderer.nativeSimulateClick(point.x, point.y)
      await settle()

      expect(errors).toEqual([])
      expect(app.renderer.findByTestId("content")).toBeDefined()
      expect(app.renderer.getFocusedElementId()).toBe(app.renderer.findByTestId("input").id)
    } finally {
      app.unmount()
    }
  })

  it("opens across a separate mousedown and mouseup in the input", async () => {
    const { app, errors } = createApp()
    try {
      app.render(<ComboboxScene />)
      const point = center(app, "input")

      app.renderer.nativeSimulateMouseDown(point.x, point.y)
      app.renderer.nativeSimulateMouseUp(point.x, point.y)
      await settle()

      expect(errors).toEqual([])
      expect(app.renderer.findByTestId("content")).toBeDefined()
      expect(app.renderer.getFocusedElementId()).toBe(app.renderer.findByTestId("input").id)
    } finally {
      app.unmount()
    }
  })

  it("opens on a click on the trigger and moves focus into the input", async () => {
    const { app, errors } = createApp()
    try {
      app.render(<ComboboxScene />)
      const point = center(app, "trigger")

      app.renderer.nativeSimulateClick(point.x, point.y)
      await settle()

      expect(errors).toEqual([])
      expect(app.renderer.findByTestId("content")).toBeDefined()
      expect(app.renderer.getFocusedElementId()).toBe(app.renderer.findByTestId("input").id)
    } finally {
      app.unmount()
    }
  })
})
