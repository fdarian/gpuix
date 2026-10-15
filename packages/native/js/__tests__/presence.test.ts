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

describe("popLayoutStyle", () => {
  it("is the box relative to the parent's", () => {
    expect(popLayoutStyle({ x: 40, y: 30, width: 10, height: 20 }, { x: 30, y: 10, width: 100, height: 100 })).toEqual({
      position: "absolute",
      top: 20,
      left: 10,
      width: 10,
      height: 20,
    })
  })

  it("uses the window origin for a child of the root", () => {
    expect(popLayoutStyle({ x: 4, y: 5, width: 1, height: 2 }, undefined)).toMatchObject({ top: 5, left: 4 })
  })
})

describe("measurePopLayout", () => {
  const boxes = new Map([
    [1, { x: 0, y: 0, width: 100, height: 100 }],
    [2, { x: 0, y: 20, width: 100, height: 20 }],
  ])
  const renderer = { getElementBounds: (id: number) => boxes.get(id) ?? null }

  it("reads the child and its parent", () => {
    expect(measurePopLayout(renderer, 2, 1)).toMatchObject({ top: 20, left: 0, height: 20 })
  })

  it("does nothing for a child that has not painted, and throws without a bounds API or painted parent", () => {
    expect(measurePopLayout(renderer, 9, 1)).toBeUndefined()
    expect(() => measurePopLayout(renderer, 2, 9)).toThrow("has not painted")
    expect(() => measurePopLayout({}, 2, 1)).toThrow("getElementBounds")
  })
})
