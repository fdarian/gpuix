// @ts-nocheck
/**
 * The page of cards the retained replay tests share: `setup` renders it, and `expectWhatABuildShows`
 * compares the frame, its accessibility tree and its boxes with the same state built whole.
 */

import fs from "fs"
import React from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

export const describeNative = hasNativeTestRenderer ? describe : describe.skip

export type Rgba = [number, number, number, number]
export const RED: Rgba = [255, 0, 0, 255]
export const GREEN: Rgba = [0, 255, 0, 255]
export const CARD_BG: Rgba = [51, 85, 119, 255]
export const CARDS = 12
export const WINDOW = { width: 640, height: 480 }

export type CardProps = {
  index: number
  label?: string
  onClick?: () => void
  extra?: React.ReactNode
  style?: object
}

export function Card({ index, label = `card ${index}`, onClick, extra, style }: CardProps) {
  return (
    <div
      testId={`card-${index}`}
      style={{
        width: 140,
        height: 90,
        backgroundColor: "#335577",
        hover: { backgroundColor: "#ff0000" },
        ...style,
      }}
    >
      <div testId={`title-${index}`} style={{ height: 20, backgroundColor: "#223344" }}>
        <text>{label}</text>
      </div>
      <div style={{ height: 20 }}>
        <span>one</span>
        <span> two</span>
      </div>
      <div
        testId={`button-${index}`}
        tabIndex={0}
        role="button"
        aria-label={`Button ${index}`}
        onClick={onClick}
        style={{ width: 40, height: 20, backgroundColor: "#555555", focusVisible: { backgroundColor: "#00ff00" } }}
      />
      {extra}
    </div>
  )
}

export function Page({ card, style, children }: { card?: (index: number) => React.ReactNode; style?: object; children?: React.ReactNode }) {
  return (
    <div testId="page" style={{ display: "flex", flexWrap: "wrap", gap: 8, width: WINDOW.width, ...style }}>
      {Array.from({ length: CARDS }, (_, index) => card?.(index) ?? <Card key={index} index={index} />)}
      {children}
    </div>
  )
}

/** A run under GPUIX_RETAIN_VERIFY builds every frame that replays a second time, which the counts of
 *  what was built and replayed cannot be read from. */
export const verifying = process.env.GPUIX_RETAIN_VERIFY !== undefined

export function setup(element: React.ReactElement, options = {}) {
  const root = createTestRoot({ ...WINDOW, ...options })
  const { renderer } = root
  root.render(element)
  const idOf = (testId: string) => renderer.findByTestId(testId).id
  const bounds = (testId: string) => renderer.getElementBounds(idOf(testId))
  const center = (testId: string) => {
    const box = bounds(testId)
    return [box.x + box.width / 2, box.y + box.height / 2]
  }
  const counters = () => renderer.getRetainedCounters()
  const built = () => renderer.getBuildCounters().elementsBuilt
  /** Frames until the cards are recorded, so the next ones replay them. */
  const settle = () => {
    for (let frame = 0; frame < 5; frame++) renderer.flush()
  }
  const sample = (name: string, x: number, y: number): Rgba => {
    const file = `${SHOTS_DIR}/retained-replay-${name}.png`
    fs.rmSync(file, { force: true })
    renderer.captureScreenshot(file)
    return readLogicalPixels(file, renderer).at(x, y)
  }
  /** A point inside a card that none of its children cover. */
  const inside = (index: number): [number, number] => {
    const box = bounds(`card-${index}`)
    return [box.x + box.width - 10, box.y + box.height - 10]
  }
  /** What the frame shows and answers, as bytes and text. */
  const snapshot = (name: string) => {
    const file = `${SHOTS_DIR}/retained-replay-${name}.png`
    fs.rmSync(file, { force: true })
    renderer.captureScreenshot(file)
    const boxes = Array.from({ length: CARDS }, (_, index) => {
      const card = bounds(`card-${index}`)
      const button = bounds(`button-${index}`)
      return [card.x, card.y, card.width, card.height, button.x, button.y]
    })
    return {
      picture: fs.readFileSync(file).toString("base64"),
      a11y: JSON.stringify(renderer.getA11yTree(), (key, value) => (key === "frame" ? undefined : value)),
      boxes,
      text: renderer.getAllText(),
    }
  }
  /** The frame as it is, then the same state built whole: they have to be the same. */
  const expectWhatABuildShows = (name: string) => {
    const replayed = snapshot(`${name}-replayed`)
    renderer.setRetainPlanning(false)
    renderer.flush()
    const whole = snapshot(`${name}-built`)
    renderer.setRetainPlanning(true)
    renderer.flush()
    expect(replayed.boxes).toEqual(whole.boxes)
    expect(replayed.text).toEqual(whole.text)
    expect(replayed.a11y).toEqual(whole.a11y)
    expect(replayed.picture === whole.picture).toBe(true)
  }
  const finish = (replayed = true) => {
    expect(counters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
    if (replayed) expect(counters().replays).toBeGreaterThan(0)
  }
  return { ...root, idOf, bounds, center, counters, built, settle, sample, inside, expectWhatABuildShows, finish }
}
