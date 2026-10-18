// @ts-nocheck
/**
 * An element that listens for its layout, its size or its place in a clip is read from the boxes
 * the frame published, so it does not keep its subtree from being replayed. Each test runs one
 * scenario twice, with retention on and with it off, and expects the observers to have seen the
 * same thing: the same entries in the same order, none on a frame that changed nothing. Under
 * GPUIX_RETAIN_VERIFY the frames that replay are also built whole and compared.
 */

import React from "react"
import { expect, it } from "vitest"
import { IntersectionObserver, ResizeObserver } from "../index"
import { Card, CARDS, describeNative, Page, setup, verifying } from "./retained-harness.js"

type Log = string[]

/** `scenario` on a fresh page with retention `on`, and what its observers logged. */
function run(on: boolean, page: (log: Log) => React.ReactElement, scenario: (kit: any, log: Log) => void) {
  const log: Log = []
  const kit = setup(page(log))
  try {
    if (!on) kit.renderer.setRetainPlanning(false)
    scenario(kit, log)
    return { log, replays: kit.counters().replays, mismatches: kit.counters().mismatches }
  } finally {
    kit.unmount()
  }
}

/** The log with retention on equals the log with it off, and the page did replay. */
function expectSame(page, scenario) {
  const off = run(false, page, scenario)
  const on = run(true, page, scenario)
  expect(on.log).toEqual(off.log)
  expect(on.mismatches).toBe(0)
  expect(on.replays).toBeGreaterThan(0)
  return on.log
}

const entry = (kind: string, ...values: unknown[]) => `${kind} ${values.join(" ")}`

describeNative("observers inside replayed subtrees", () => {
  function Watched({ log, width, side, listen = "resize" }) {
    const ref = React.useRef(null)
    React.useEffect(() => {
      const resize = new ResizeObserver((entries) => {
        for (const item of entries) log.push(entry("resize", item.contentRect.width, item.contentRect.height))
      })
      if (listen === "resize") resize.observe(ref.current)
      return () => resize.disconnect()
    }, [])
    return (
      <div testId="row" style={{ display: "flex", width, height: 30 }}>
        <div ref={ref} testId="watched" style={{ flexGrow: 1, height: 30, backgroundColor: "#999999" }} />
        <div style={{ width: side, height: 30 }} />
      </div>
    )
  }

  it("fires a ResizeObserver in a card when a sibling resizes it, and not on idle replays", () => {
    const page = (log: Log) => (side: number) => (
      <Page card={(index) => (index === 2 ? <Card key={index} index={index} extra={<Watched log={log} width={120} side={side} />} /> : undefined)} />
    )
    const log = expectSame(
      (log) => page(log)(20),
      ({ renderer, render, settle }, log) => {
        settle()
        renderer.dispatchNativeEvents()
        log.push("settled")
        renderer.flush()
        renderer.flush()
        renderer.dispatchNativeEvents()
        log.push("idle")
        render(page(log)(60))
        settle()
        renderer.dispatchNativeEvents()
        log.push("resized")
        renderer.flush()
        renderer.dispatchNativeEvents()
      }
    )
    expect(log).toEqual([entry("resize", 100, 30), "settled", "idle", entry("resize", 60, 30), "resized"])
  })

  it("builds nothing on an idle frame of a page whose cards are observed", () => {
    if (verifying) return
    const log: Log = []
    function Observed({ index }) {
      const ref = React.useRef(null)
      React.useEffect(() => {
        const resize = new ResizeObserver((entries) => entries.forEach((item) => log.push(`resize ${index} ${item.contentRect.width}`)))
        const intersect = new IntersectionObserver((entries) => entries.forEach((item) => log.push(`intersect ${index} ${item.isIntersecting}`)))
        resize.observe(ref.current)
        intersect.observe(ref.current)
        return () => {
          resize.disconnect()
          intersect.disconnect()
        }
      }, [])
      return <div ref={ref} style={{ width: 40, height: 10 }} />
    }
    const { renderer, counters, built, settle, finish, unmount } = setup(
      <Page card={(index) => <Card key={index} index={index} extra={<Observed index={index} />} />} />
    )
    try {
      settle()
      renderer.dispatchNativeEvents()
      expect(log.filter((line) => line.startsWith("resize"))).toHaveLength(CARDS)
      expect(log.filter((line) => line.startsWith("intersect"))).toHaveLength(CARDS)
      settle()
      const before = built()
      const replays = counters().replays
      renderer.flush()
      renderer.flush()
      renderer.dispatchNativeEvents()
      expect(counters().replays).toBeGreaterThan(replays)
      expect(built() - before, "observed cards are replayed, not built").toBe(0)
      expect(log).toHaveLength(CARDS * 2)
      finish()
    } finally {
      unmount()
    }
  })

  it("reports onLayout of an element in a replayed card when its size changes, and only then", () => {
    function Laid({ log, side }) {
      return (
        <div style={{ display: "flex", width: 120, height: 30 }}>
          <div testId="laid" onLayout={(event) => log.push(entry("layout", event.width))} style={{ flexGrow: 1, height: 30 }} />
          <div style={{ width: side, height: 30 }} />
        </div>
      )
    }
    const page = (log: Log) => (side: number) => (
      <Page card={(index) => (index === 5 ? <Card key={index} index={index} extra={<Laid log={log} side={side} />} /> : undefined)} />
    )
    const log = expectSame(
      (log) => page(log)(20),
      ({ renderer, render, settle }, log) => {
        settle()
        renderer.dispatchNativeEvents()
        renderer.flush()
        renderer.flush()
        renderer.dispatchNativeEvents()
        log.push("idle")
        render(page(log)(40))
        settle()
        renderer.dispatchNativeEvents()
        renderer.flush()
        renderer.dispatchNativeEvents()
      }
    )
    expect(log).toEqual([entry("layout", 100), "idle", entry("layout", 80)])
  })

  it("tells an IntersectionObserver in a replayed card that an ancestor scroller scrolled it into view", () => {
    function Probe({ log, index }) {
      const ref = React.useRef(null)
      React.useEffect(() => {
        const observer = new IntersectionObserver((entries) => entries.forEach((item) => log.push(entry("intersect", index, item.isIntersecting, Math.round(item.intersectionRatio * 100)))))
        observer.observe(ref.current)
        return () => observer.disconnect()
      }, [])
      return <div ref={ref} style={{ width: 40, height: 20, backgroundColor: "#ffffff" }} />
    }
    const page = (log: Log) => (
      <div testId="scroller" style={{ width: 640, height: 190, overflow: "auto" }}>
        <Page card={(index) => <Card key={index} index={index} extra={<Probe log={log} index={index} />} />} />
      </div>
    )
    const log = expectSame(page, ({ renderer, idOf, settle }, log) => {
      settle()
      renderer.dispatchNativeEvents()
      log.push("start")
      renderer.flush()
      renderer.dispatchNativeEvents()
      renderer.scrollTo(idOf("scroller"), 0, 98)
      settle()
      renderer.dispatchNativeEvents()
      log.push("scrolled")
      renderer.scrollTo(idOf("scroller"), 0, 0)
      settle()
      renderer.dispatchNativeEvents()
      log.push("back")
    })
    // Cards 8 to 11 are on the third row, below the 190px clip until the scroller moves.
    expect(log).toContain(entry("intersect", 8, false, 0))
    expect(log.indexOf(entry("intersect", 8, true, 100))).toBeGreaterThan(log.indexOf("start"))
    expect(log.indexOf(entry("intersect", 8, true, 100))).toBeLessThan(log.indexOf("scrolled"))
  })

  it("reports a ResizeObserver on an inline span of a replayed card", () => {
    function Span({ log, label }) {
      const ref = React.useRef(null)
      React.useEffect(() => {
        const observer = new ResizeObserver((entries) => entries.forEach((item) => log.push(entry("span", Math.round(item.borderBoxSize[0].inlineSize)))))
        observer.observe(ref.current)
        return () => observer.disconnect()
      }, [])
      return (
        <div style={{ width: 120, height: 20 }}>
          <span ref={ref}>{label}</span>
        </div>
      )
    }
    const page = (log: Log) => (label: string) => (
      <Page card={(index) => (index === 1 ? <Card key={index} index={index} extra={<Span log={log} label={label} />} /> : undefined)} />
    )
    const log = expectSame(
      (log) => page(log)("ab"),
      ({ renderer, render, settle }, log) => {
        settle()
        renderer.dispatchNativeEvents()
        renderer.flush()
        renderer.dispatchNativeEvents()
        log.push("idle")
        render(page(log)("abcdefgh"))
        settle()
        renderer.dispatchNativeEvents()
      }
    )
    expect(log.filter((line) => line.startsWith("span"))).toHaveLength(2)
    expect(log[1]).toBe("idle")
  })

  it("reports an IntersectionObserver whose card a window resize pushed out of the viewport", () => {
    function Probe({ log, index }) {
      const ref = React.useRef(null)
      React.useEffect(() => {
        const observer = new IntersectionObserver((entries) => entries.forEach((item) => log.push(entry("intersect", index, item.isIntersecting))))
        observer.observe(ref.current)
        return () => observer.disconnect()
      }, [])
      return <div ref={ref} style={{ width: 40, height: 20 }} />
    }
    const page = (log: Log) => (
      <Page style={{ width: "100%" }} card={(index) => <Card key={index} index={index} extra={<Probe log={log} index={index} />} />} />
    )
    const log = expectSame(page, ({ renderer, settle }, log) => {
      settle()
      renderer.dispatchNativeEvents()
      log.push("wide")
      renderer.setWindowSize(320, 190)
      settle()
      renderer.dispatchNativeEvents()
      log.push("narrow")
    })
    expect(log.slice(log.indexOf("wide") + 1)).toContain(entry("intersect", 11, false))
  })
})
