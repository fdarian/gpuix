import { describe, expect, it } from "vitest"
import { screen } from "../screen.js"

describe("screen", () => {
  const displays = screen.getAllDisplays()

  it("lists a display before any window exists", () => {
    expect(displays.length).toBeGreaterThanOrEqual(1)
  })

  it("lists the primary display", () => {
    const primary = screen.getPrimaryDisplay()
    expect(displays.find((display) => display.id === primary.id)).toEqual(primary)
  })

  it("puts the primary display's origin at (0, 0)", () => {
    const primary = screen.getPrimaryDisplay()
    expect(primary.bounds.x).toBe(0)
    expect(primary.bounds.y).toBe(0)
  })

  it("gives every display a unique id", () => {
    expect(new Set(displays.map((display) => display.id)).size).toBe(displays.length)
  })

  for (const display of screen.getAllDisplays()) {
    describe(`display ${display.id}`, () => {
      it("has sizes equal to its rectangles'", () => {
        expect(display.size).toEqual({
          width: display.bounds.width,
          height: display.bounds.height,
        })
        expect(display.workAreaSize).toEqual({
          width: display.workArea.width,
          height: display.workArea.height,
        })
      })

      it("has a work area inside its bounds", () => {
        expect(display.workArea.x).toBeGreaterThanOrEqual(display.bounds.x)
        expect(display.workArea.y).toBeGreaterThanOrEqual(display.bounds.y)
        expect(display.workArea.x + display.workArea.width).toBeLessThanOrEqual(
          display.bounds.x + display.bounds.width,
        )
        expect(display.workArea.y + display.workArea.height).toBeLessThanOrEqual(
          display.bounds.y + display.bounds.height,
        )
      })

      it("has a scale factor, rotation and label", () => {
        expect(display.scaleFactor).toBeGreaterThanOrEqual(1)
        expect([0, 90, 180, 270]).toContain(display.rotation)
        expect(display.label.length).toBeGreaterThan(0)
      })
    })
  }
})
