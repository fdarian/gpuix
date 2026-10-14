import { describe, expect, it } from "vitest"
import type { EventPayload } from "../../index.js"
import {
  defineScrollMembers,
  isScrollKey,
  readScrollMetrics,
  shapeScrollEvent,
} from "../host.js"
import type { NativeRenderer, ScrollableElement } from "../host.js"

function payload(fields: Partial<EventPayload>): EventPayload {
  return { elementId: 1, eventType: "wheel", ...fields }
}

/** A renderer that scrolls like a 200 x 100 box holding 1000 x 500 of content. */
class FakeScroller {
  left = 0
  top = 0
  calls: string[] = []

  getScrollMetrics = (id: number) => {
    this.calls.push(`metrics ${id}`)
    return [this.left, this.top, 1000, 500, 200, 100]
  }

  scrollTo = (id: number, x: number, y: number, behavior?: string) => {
    this.calls.push(`scrollTo ${id} ${x} ${y} ${behavior}`)
    this.left = -x
    this.top = -y
  }

  scrollIntoView = (id: number, block?: string, inline?: string, behavior?: string) => {
    this.calls.push(`scrollIntoView ${id} ${block} ${inline} ${behavior}`)
  }
}

function element(scroller: FakeScroller): ScrollableElement {
  const target = {}
  defineScrollMembers(target, () => ({
    renderer: scroller as unknown as NativeRenderer,
    id: 7,
  }))
  return target as ScrollableElement
}

describe("shapeScrollEvent", () => {
  it("gives a wheel event its DOM names", () => {
    const shaped = shapeScrollEvent(payload({ deltaX: 0, deltaY: 40, x: 12, y: 34 }))
    expect(shaped).toMatchObject({ deltaX: 0, deltaY: 40, deltaMode: 0, clientX: 12, clientY: 34 })
  })

  it("reads scrollTop and scrollLeft from the offset, which native keeps negative", () => {
    const shaped = shapeScrollEvent(payload({ eventType: "scroll", scrollX: -5, scrollY: -120 }))
    expect(shaped).toMatchObject({ scrollLeft: 5, scrollTop: 120 })
    expect(Object.is((shaped as { scrollTop: number }).scrollTop, 120)).toBe(true)
    const start = shapeScrollEvent(payload({ eventType: "scroll", scrollX: 0, scrollY: 0 }))
    expect(Object.is((start as { scrollLeft: number }).scrollLeft, 0), "+0, not -0").toBe(true)
  })

  it("fails loudly on a payload native left incomplete", () => {
    expect(() => shapeScrollEvent(payload({ deltaX: 0, x: 1, y: 1 }))).toThrow("deltaY")
  })

  it("leaves other events alone", () => {
    const click = payload({ eventType: "click" })
    expect(shapeScrollEvent(click)).toBe(click)
  })
})

describe("readScrollMetrics", () => {
  it("is the one call that asks native", () => {
    const scroller = new FakeScroller()
    const target = element(scroller)
    target.scrollTop = 30
    scroller.calls.length = 0

    void [
      target.scrollTop,
      target.scrollLeft,
      target.scrollWidth,
      target.scrollHeight,
      target.clientWidth,
      target.clientHeight,
    ]
    expect(scroller.calls).toEqual(Array(6).fill("metrics 7"))
  })

  it("answers zero for an element native does not know", () => {
    const renderer = { getScrollMetrics: () => null } as unknown as NativeRenderer
    expect(readScrollMetrics(renderer, 1).scrollHeight).toBe(0)
  })

  it("refuses to guess when the renderer cannot read", () => {
    expect(() => readScrollMetrics({} as NativeRenderer, 1)).toThrow("cannot read")
  })
})

describe("scroll members", () => {
  it("sets scrollTop as an instant scroll that keeps the other axis", () => {
    const scroller = new FakeScroller()
    scroller.left = 25
    element(scroller).scrollTop = 80
    expect(scroller.calls.at(-1)).toBe("scrollTo 7 -25 -80 instant")
  })

  it("keeps the axis scrollTo does not name, and passes the behavior", () => {
    const scroller = new FakeScroller()
    scroller.left = 25
    scroller.top = 10
    element(scroller).scrollTo({ top: 60, behavior: "smooth" })
    expect(scroller.calls.at(-1)).toBe("scrollTo 7 -25 -60 smooth")
  })

  it("takes scrollTo(x, y) and rejects half a coordinate pair", () => {
    const scroller = new FakeScroller()
    const target = element(scroller)
    target.scrollTo(3, 4)
    expect(scroller.calls.at(-1)).toBe("scrollTo 7 -3 -4 undefined")
    expect(() => (target.scrollTo as (x: number) => void)(3)).toThrow("both coordinates")
  })

  it("scrolls relative to the position it reads", () => {
    const scroller = new FakeScroller()
    scroller.top = 40
    element(scroller).scrollBy({ top: 15 })
    expect(scroller.calls.at(-1)).toBe("scrollTo 7 0 -55 undefined")
  })

  it("maps scrollIntoView arguments", () => {
    const scroller = new FakeScroller()
    const target = element(scroller)
    target.scrollIntoView()
    expect(scroller.calls.at(-1)).toBe("scrollIntoView 7 undefined undefined undefined")
    target.scrollIntoView(false)
    expect(scroller.calls.at(-1)).toBe("scrollIntoView 7 end undefined undefined")
    target.scrollIntoView(true)
    expect(scroller.calls.at(-1)).toBe("scrollIntoView 7 start undefined undefined")
    target.scrollIntoView({ block: "center", inline: "end", behavior: "smooth" })
    expect(scroller.calls.at(-1)).toBe("scrollIntoView 7 center end smooth")
  })

  it("fails when the element has no renderer", () => {
    const target = {}
    defineScrollMembers(target, () => ({ renderer: undefined, id: 1 }))
    expect(() => (target as ScrollableElement).scrollTop).toThrow("not attached")
  })
})

describe("isScrollKey", () => {
  it("names the keys whose default action is to scroll", () => {
    for (const key of ["up", "down", "left", "right", "pageup", "pagedown", "home", "end", "space"]) {
      expect(isScrollKey(key)).toBe(true)
    }
    expect(isScrollKey("enter")).toBe(false)
    expect(isScrollKey(undefined)).toBe(false)
  })
})
