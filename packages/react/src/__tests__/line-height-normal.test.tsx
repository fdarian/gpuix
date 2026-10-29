/// A line with `line-height: normal` is as tall as every font used on it needs, as in Chrome. Han, kana, Hangul and
/// emoji are set in a fallback font (PingFang, Hiragino, Apple SD Gothic Neo, Apple Color Emoji) whose ascent, descent and
/// line gap are not those of the font the box asks for, and the line box unites the content area of each font used on
/// it, each with its line gap split in half and floored to a device pixel. A number or a length is the same line height
/// for every font, so the line is as tall as without the fallback.
///
/// `fixtures/line-height-normal-oracle.html` is the page and `fixtures/line-height-normal-oracle.json` / `-x2.json` what
/// headless Chrome reports for it at device scale 1 and 2 (`sh fixtures/line-height-normal-oracle.sh`): for each case
/// `[block height, then for each line: baseline, top and height of its <span>]`, in CSS px from the top of the block. The
/// baseline is the bottom of an empty inline-block at the start of the line, and the span is the content area of the
/// box's own font, which a fallback font does not change.

import fs from "fs"
import path from "path"
import type { ReactNode } from "react"
import { Fragment } from "react"
import { fileURLToPath } from "url"
import { describe, expect, it } from "vitest"
import type { PublicInstance } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const SCALES = [1, 2] as const
type Scale = (typeof SCALES)[number]

const oracleOf = (file: string): Record<string, number[]> =>
  JSON.parse(fs.readFileSync(path.resolve(path.dirname(fileURLToPath(import.meta.url)), "fixtures", file), "utf8"))

const ORACLE: Record<Scale, Record<string, number[]>> = {
  1: oracleOf("line-height-normal-oracle.json"),
  2: oracleOf("line-height-normal-oracle-x2.json"),
}

/** Chrome lays a line out in 1/64 of a device pixel and snaps a block's height to the device pixel it ends in. */
const TOLERANCE = 1 / 64 + 1e-3

const BIG = { fontSize: "1.5em" } as const

/** The lines of each kind of case, as the oracle has them. */
const KINDS: Record<string, ReactNode[]> = {
  latin: ["Hxg"],
  han: ["中文"],
  kana: ["こんにちは"],
  hangul: ["한국어"],
  emoji: ["😀"],
  mixed: ["Hxg 中文 😀"],
  lines: ["Hxg", "中文", "Hxg"],
  lines2: ["😀", "Hxg", "한국어"],
  spanBig: [
    <>
      Hxg <span style={BIG}>Hxg</span>
    </>,
  ],
  spanBigHan: [
    <>
      Hxg <span style={BIG}>中文</span>
    </>,
  ],
}

type Case = { id: string; family: string; size: number; kind: string; lineHeight: string }

function casesOf(scale: Scale): Case[] {
  return Object.keys(ORACLE[scale]).map((id) => {
    const [family, size, kind, lineHeight] = id.split("|") as [string, string, string, string]
    return { id, family, size: Number(size), kind, lineHeight }
  })
}

/** Every case of a scale in one window, one below the other. A case's lines are measured against its own block. */
function measure(scale: Scale) {
  const root = createTestRoot({ width: 1280, height: 800 })
  root.renderer.setWindowScaleFactor(scale)
  const refs = new Map<string, PublicInstance>()
  const ref = (name: string) => (instance: PublicInstance | null) => {
    if (instance) refs.set(name, instance)
  }
  const cases = casesOf(scale)
  root.render(
    <div style={{ width: 1280 }}>
      {cases.map((one) => (
        <div key={one.id} style={{ height: 120, flexShrink: 0 }}>
          <div
            ref={ref(one.id)}
            style={{
              width: 1280,
              color: "#000000",
              fontFamily: one.family,
              fontSize: one.size,
              lineHeight: one.lineHeight === "1.5" ? 1.5 : one.lineHeight,
              whiteSpace: "pre-line",
            }}
          >
            {KINDS[one.kind]!.map((line, ix) => (
              <Fragment key={ix}>
                {ix > 0 ? "\n" : null}
                <span ref={ref(`${one.id}#${ix}`)}>
                  <span ref={ref(`${one.id}#${ix}.probe`)} style={{ display: "inline-block", width: 0, height: 0 }} />
                  {line}
                </span>
              </Fragment>
            ))}
          </div>
        </div>
      ))}
    </div>
  )
  root.renderer.dispatchNativeEvents()
  const rect = (name: string) => {
    const box = refs.get(name)?.getBoundingClientRect()
    expect(box, `no element ${name}`).toBeDefined()
    return box!
  }
  const valuesOf = (one: Case): number[] => {
    const block = rect(one.id)
    const values = [block.height]
    for (let ix = 0; ix < KINDS[one.kind]!.length; ix++) {
      const probe = rect(`${one.id}#${ix}.probe`)
      const span = rect(`${one.id}#${ix}`)
      values.push(probe.y + probe.height - block.y, span.y - block.y, span.height)
    }
    return values
  }
  return { cases, valuesOf, unmount: root.unmount }
}

const NAMES = ["height", "baseline", "top", "span height"]

function mismatches(scale: Scale, pick: (one: Case) => boolean) {
  const { cases, valuesOf, unmount } = measure(scale)
  try {
    const found: string[] = []
    for (const one of cases.filter(pick)) {
      const expected = ORACLE[scale][one.id]!
      const actual = valuesOf(one)
      expected.forEach((value, ix) => {
        const name = ix === 0 ? NAMES[0] : `line ${Math.floor((ix - 1) / 3)} ${NAMES[1 + ((ix - 1) % 3)]}`
        if (Math.abs((actual[ix] ?? NaN) - value) > TOLERANCE) {
          found.push(`${one.id} ${name}: ${actual[ix]} != Chrome ${value}`)
        }
      })
    }
    return { found, total: cases.filter(pick).length }
  } finally {
    unmount()
  }
}

describeNative("line-height: normal with text in a fallback font", () => {
  for (const scale of SCALES) {
    it(`a line is as tall as the fonts used on it make it, at scale ${scale}`, () => {
      const result = mismatches(scale, (one) => one.lineHeight === "normal")
      expect(result.total).toBeGreaterThan(150)
      expect(result.found).toEqual([])
    })

    it(`an explicit line height is the same with and without a fallback font, at scale ${scale}`, () => {
      const result = mismatches(scale, (one) => one.lineHeight !== "normal")
      expect(result.total).toBeGreaterThan(50)
      expect(result.found).toEqual([])
    })
  }
})
