import { describe, expect, it } from "vitest"
import { measurePopLayout, popLayoutStyle, resolveExit } from "../presence.js"

describe("resolveExit", () => {
  it("passes a target through and calls a function with custom", () => {
    expect(resolveExit({ opacity: 0 }, "left")).toEqual({ opacity: 0 })
    expect(resolveExit((custom: unknown) => ({ opacity: custom === "left" ? 0.2 : 0.8 }), "left")).toEqual({
      opacity: 0.2,
    })
    expect(resolveExit(undefined, "left")).toBeUndefined()
  })
})

const noMargin = { margin: 0, marginTop: 0, marginRight: 0, marginBottom: 0, marginLeft: 0 }
const box = (x: number, y: number, width: number, height: number, clientLeft = 0, clientTop = 0) => ({
  x,
  y,
  width,
  height,
  clientLeft,
  clientTop,
})

describe("popLayoutStyle", () => {
  it("is the box relative to the parent's padding box, with the margin zeroed", () => {
    expect(
      popLayoutStyle(box(40, 30, 10, 20), { bounds: box(30, 10, 100, 100), scrollLeft: 0, scrollTop: 0 })
    ).toEqual({ position: "absolute", top: 20, left: 10, width: 10, height: 20, ...noMargin })
  })

  it("starts from inside the parent's border", () => {
    expect(
      popLayoutStyle(box(40, 30, 10, 20), { bounds: box(30, 10, 100, 100, 2, 3), scrollLeft: 0, scrollTop: 0 })
    ).toMatchObject({ top: 17, left: 8 })
  })

  it("adds the parent's scroll offset once", () => {
    expect(
      popLayoutStyle(box(40, 30, 10, 20), { bounds: box(30, 10, 100, 100), scrollLeft: 5, scrollTop: 30 })
    ).toMatchObject({ top: 50, left: 15 })
  })

  it("uses the window origin for a child of the root", () => {
    expect(popLayoutStyle(box(4, 5, 1, 2), undefined)).toMatchObject({ top: 5, left: 4 })
  })
})

describe("measurePopLayout", () => {
  const boxes = new Map([
    [1, box(0, 0, 100, 100)],
    [2, box(0, 20, 100, 20)],
  ])
  const renderer = {
    getElementBounds: (id: number) => boxes.get(id) ?? null,
    getScrollMetrics: (id: number) => (boxes.has(id) ? [0, 0, 100, 100, 100, 100] : null),
  }

  it("reads the child and its parent", () => {
    expect(measurePopLayout(renderer, 2, 1)).toMatchObject({ top: 20, left: 0, height: 20 })
  })

  it("does nothing for a child that has not painted, and throws without a bounds API or painted parent", () => {
    expect(measurePopLayout(renderer, 9, 1)).toBeUndefined()
    expect(() => measurePopLayout(renderer, 2, 9)).toThrow("has not painted")
    expect(() => measurePopLayout({}, 2, 1)).toThrow("getElementBounds")
    expect(() => measurePopLayout({ getElementBounds: renderer.getElementBounds }, 2, 1)).toThrow("getScrollMetrics")
  })
})
