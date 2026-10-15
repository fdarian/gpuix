import { describe, expect, it } from "vitest"
import {
  joinLayoutGroupId,
  layoutDescription,
  scopeLayoutId,
  withoutLayoutTransition,
} from "../layout-animation.js"
import { createReorderDrag } from "../reorder.js"

describe("layout group ids", () => {
  it("joins nested groups and skips a group without an id", () => {
    expect(joinLayoutGroupId(undefined, undefined)).toBeUndefined()
    expect(joinLayoutGroupId(undefined, "a")).toBe("a")
    expect(joinLayoutGroupId("a", undefined)).toBe("a")
    expect(joinLayoutGroupId("a", "b")).toBe("a-b")
  })

  it("prefixes a layoutId only inside a group", () => {
    expect(scopeLayoutId(undefined, "h")).toBe("h")
    expect(scopeLayoutId("g", "h")).toBe("g-h")
    expect(scopeLayoutId("g", undefined)).toBeUndefined()
  })
})

describe("layoutDescription", () => {
  const base = { layout: undefined, layoutId: undefined, transition: undefined, reduced: false }

  it("is absent without layout or layoutId, and when layout is false", () => {
    expect(layoutDescription(base)).toBeUndefined()
    expect(layoutDescription({ ...base, layout: false, layoutId: "h" })).toBeUndefined()
  })

  it("lets a layoutId imply layout", () => {
    expect(layoutDescription({ ...base, layoutId: "h" })).toEqual({ mode: true, layoutId: "h" })
  })

  it("carries the mode, transition and reduced motion", () => {
    const transition = { duration: 1 }
    expect(layoutDescription({ ...base, layout: "position", transition, reduced: true })).toEqual({
      mode: "position",
      transition,
      reducedMotion: true,
    })
  })
})

describe("layoutDescription drag", () => {
  it("names the three motion values that drag the box", () => {
    const drag = createReorderDrag()
    const description = layoutDescription({
      layout: true,
      layoutId: undefined,
      transition: undefined,
      reduced: false,
      drag,
    })
    expect(description?.dragOffset).toEqual({ x: drag.x.id, y: drag.y.id, active: drag.active.id })
  })
})

describe("withoutLayoutTransition", () => {
  it("drops only the layout entry", () => {
    expect(withoutLayoutTransition({ duration: 2, layout: { duration: 1 } })).toEqual({ duration: 2 })
    const plain = { duration: 2 }
    expect(withoutLayoutTransition(plain)).toBe(plain)
    expect(withoutLayoutTransition(undefined)).toBeUndefined()
  })
})
