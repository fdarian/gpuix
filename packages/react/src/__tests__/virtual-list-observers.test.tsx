/// A `<virtual-list>` records its box like any host element, so what reads the recorded boxes sees it: `onLayout`,
/// `ResizeObserver` and `IntersectionObserver` on the list, an `IntersectionObserver` rooted in it, and a row clipped by
/// it. Before the list recorded a box these read nothing for it (a list was 0 by 0, a row was not clipped by it).

import React from "react"
import { describe, expect, it } from "vitest"
import type { IntersectionObserverEntry, PublicInstance, ResizeObserverEntry } from "../index"
import { IntersectionObserver, ResizeObserver } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const LIST = { position: "absolute", left: 50, top: 40, width: 200, height: 100 } as const

function Rows({ onRow }: { onRow?: (index: number, instance: PublicInstance | null) => void }) {
  return Array.from({ length: 12 }, (_, index) => (
    <div key={index} ref={(instance: PublicInstance | null) => onRow?.(index, instance)} style={{ height: 30 }} />
  ))
}

describeNative("a <virtual-list> that is observed", () => {
  it("is reported by onLayout with the size layout gave it", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      const sizes: unknown[] = []
      render(
        <virtual-list style={{ ...LIST, width: 200.5, height: 100.3 }} onLayout={(event) => sizes.push([event.x, event.y, event.width, event.height])}>
          <Rows />
        </virtual-list>
      )
      renderer.dispatchNativeEvents()
      expect(sizes).toEqual([[50, 40, 200.5, 100.296875]])
    } finally {
      unmount()
    }
  })

  it("is reported by a ResizeObserver, with the content box inside its padding, and again when it resizes", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      let list: PublicInstance | null = null
      const batches: ResizeObserverEntry[][] = []
      const app = (height: number) => (
        <virtual-list
          ref={(instance: PublicInstance | null) => {
            list = instance ?? list
          }}
          style={{ ...LIST, height, padding: 10 }}
        >
          <Rows />
        </virtual-list>
      )
      render(app(100))
      new ResizeObserver((entries) => batches.push(entries)).observe(list!)
      renderer.dispatchNativeEvents()
      expect(batches).toHaveLength(1)
      expect(batches[0]![0]!.borderBoxSize).toEqual([{ inlineSize: 200, blockSize: 100 }])
      expect(batches[0]![0]!.contentBoxSize).toEqual([{ inlineSize: 180, blockSize: 80 }])
      expect(batches[0]![0]!.contentRect).toMatchObject({ x: 10, y: 10, width: 180, height: 80 })

      render(app(120))
      renderer.dispatchNativeEvents()
      expect(batches).toHaveLength(2)
      expect(batches[1]![0]!.borderBoxSize).toEqual([{ inlineSize: 200, blockSize: 120 }])
    } finally {
      unmount()
    }
  })

  it("is an IntersectionObserver target with its box, and a root with its padding box", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      let list: PublicInstance | null = null
      const rows: (PublicInstance | null)[] = []
      const own: IntersectionObserverEntry[] = []
      const inside: IntersectionObserverEntry[] = []
      render(
        <virtual-list
          ref={(instance: PublicInstance | null) => {
            list = instance ?? list
          }}
          style={LIST}
        >
          <Rows onRow={(index, instance) => void (rows[index] = instance ?? rows[index] ?? null)} />
        </virtual-list>
      )
      new IntersectionObserver((entries) => own.push(...entries)).observe(list!)
      new IntersectionObserver((entries) => inside.push(...entries), { root: list! }).observe(rows[3]!)
      renderer.dispatchNativeEvents()

      expect(own).toHaveLength(1)
      expect(own[0]).toMatchObject({
        isIntersecting: true,
        intersectionRatio: 1,
        boundingClientRect: { x: 50, y: 40, width: 200, height: 100 },
      })
      expect(inside).toHaveLength(1)
      expect(inside[0]).toMatchObject({
        boundingClientRect: { x: 50, y: 130, width: 200, height: 30 },
        rootBounds: { x: 50, y: 40, width: 200, height: 100 },
        intersectionRect: { x: 50, y: 130, width: 200, height: 10 },
      })
      expect(inside[0]!.intersectionRatio).toBeCloseTo(1 / 3, 5)
    } finally {
      unmount()
    }
  })

  it("clips a row for an IntersectionObserver in the window", () => {
    const { render, renderer, unmount } = createTestRoot()
    try {
      const rows: (PublicInstance | null)[] = []
      const entries: IntersectionObserverEntry[] = []
      render(
        <virtual-list style={LIST}>
          <Rows onRow={(index, instance) => void (rows[index] = instance ?? rows[index] ?? null)} />
        </virtual-list>
      )
      new IntersectionObserver((batch) => entries.push(...batch)).observe(rows[3]!)
      renderer.dispatchNativeEvents()

      expect(entries).toHaveLength(1)
      expect(entries[0]).toMatchObject({
        boundingClientRect: { x: 50, y: 130, width: 200, height: 30 },
        intersectionRect: { x: 50, y: 130, width: 200, height: 10 },
      })
    } finally {
      unmount()
    }
  })
})
