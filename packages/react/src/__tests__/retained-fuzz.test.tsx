// @ts-nocheck
/**
 * Differential fuzz of retention: random interactions (pointer, keys, edits, resizes, scrolls,
 * clock, appearance, retention switched on and off) over a page of cards that exercises most of
 * what a retained subtree has to replay. After every step the frames that replayed something must
 * not differ from a build of the same state (checked in gpui by `GPUIX_RETAIN_VERIFY=1`), and at
 * the end the scene, the boxes, the text, the accessibility tree and the cursor of the replayed
 * page must be those of a page built whole.
 *
 * `FUZZ_SEEDS` and `FUZZ_STEPS` make a longer run.
 */
import fs from "fs"
import os from "os"
import path from "path"
import React from "react"
import { describe, expect, it } from "vitest"
import { flushSync, motion } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const RED_PIXEL = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
  "base64"
)
const IMAGE = path.join(os.tmpdir(), "gpuix-retained-fuzz-red.png")
fs.writeFileSync(IMAGE, RED_PIXEL)

function rng(seed: number) {
  let s = seed >>> 0
  return () => {
    s = (s * 1664525 + 1013904223) >>> 0
    return s / 4294967296
  }
}

type CardState = { label: string; hue: number; extra: boolean; wide: boolean; pad: number; tag: number }

function Card({ i, st }: { i: number; st: CardState }) {
  const variant = i % 6
  return (
    <div
      testId={`card-${i}`}
      style={{
        width: st.wide ? 200 : 140,
        height: 100,
        backgroundColor: `hsl(${st.hue}, 40%, 40%)`,
        padding: st.pad,
        color: i % 2 ? "#ffffff" : "#ffeecc",
        hover: { backgroundColor: "#aa3333", boxShadow: "0 0 6px #000000" },
        active: { backgroundColor: "#3333aa" },
        group: variant === 1 ? "g" : undefined,
        transition: variant === 2 ? "background-color 100ms linear" : undefined,
        hasHover: variant === 3 ? { outline: "2px solid #00ff00" } : undefined,
        focusWithin: variant === 4 ? { backgroundColor: "#228822" } : undefined,
        overflow: variant === 5 ? "hidden" : undefined,
        position: i % 2 === 0 ? "relative" : undefined,
        zIndex: i % 6 === 2 ? 1 : undefined,
        opacity: i % 5 === 4 ? 0.85 : undefined,
      }}
    >
      <div testId={`title-${i}`} id={`t-${i}`} style={{ height: 18, backgroundColor: "#00000055" }}>
        <text>{st.label}</text>
      </div>
      <div style={{ height: 18, groupHover: { backgroundColor: "#ffff00" } }}>
        <text>
          plain <span testId={`span-${i}`} style={{ color: "#ff8800", hover: { color: "#00ffff" } }}>span {i}</span> tail
        </text>
      </div>
      <div
        testId={`btn-${i}`}
        tabIndex={0}
        role="button"
        aria-label={`b${i}`}
        style={{ width: 36, height: 16, backgroundColor: "#666666", focusVisible: { backgroundColor: "#00ff00" } }}
      />
      {st.extra ? (
        <div style={{ height: 14, position: i % 3 === 0 ? "relative" : undefined, zIndex: i % 4 === 0 ? 2 : undefined, opacity: 0.9 }}>
          <text>extra</text>
        </div>
      ) : null}
      {i % 7 === 6 ? (
        <label htmlFor={`in-${i}`} testId={`lab-${i}`} style={{ height: 14, display: "flex" }}>
          <text>{`name ${st.label}`}</text>
        </label>
      ) : null}
      {i % 7 === 6 ? <input id={`in-${i}`} testId={`in-${i}`} style={{ height: 16, width: 60 }} /> : null}
      {i % 5 === 1 ? (
        <svg viewBox="0 0 10 10" width={18} height={12} style={{ color: st.hue > 180 ? "#ff0000" : "#00ff00" }}>
          <rect x={1} y={1} width={8} height={8} fill="currentColor" />
        </svg>
      ) : null}
      {i % 5 === 2 ? <img testId={`img-${i}`} src={IMAGE} style={{ width: 14, height: 10 }} /> : null}
      {i % 4 === 3 ? (
        <div style={{ position: "absolute", right: 2, bottom: 2, width: 20, height: 10, backgroundColor: "#ff00ff", zIndex: 3 }}>
          <text>z</text>
        </div>
      ) : null}
      {i % 2 === 0 ? (
        <div
          testId={`row-${i}`}
          style={{
            position: "relative",
            zIndex: st.tag % 3 === 0 ? 10 : undefined,
            opacity: st.tag % 3 === 1 ? 0.8 : undefined,
            height: 12,
            backgroundColor: "#00000033",
            hover: { backgroundColor: "#77777799" },
          }}
        >
          <div
            testId={`pill-${i}`}
            style={{
              position: "absolute",
              right: st.tag % 4,
              top: 0,
              width: 16 + st.tag,
              height: 10,
              backgroundColor: "#ff8800",
              zIndex: st.tag % 5 === 0 ? 2 : undefined,
              hover: { backgroundColor: "#ffff00" },
            }}
          >
            <text>{`p${st.tag}`}</text>
            {st.tag % 2 === 0 ? (
              <div style={{ position: "relative", opacity: 0.7, height: 4, width: 8, backgroundColor: "#0088ff" }}>
                <div style={{ position: "absolute", left: 2, top: 1, width: 3, height: 3, backgroundColor: "#ffffff" }} />
              </div>
            ) : null}
          </div>
          <text>{`row ${i}`}</text>
        </div>
      ) : null}
      {i % 8 === 4 ? (
        <div testId={`ref-${i}`} role="button" tabIndex={0} aria-labelledby={`t-${(i + 1) % N}`} style={{ height: 10, width: 20 }} />
      ) : null}
      {variant === 0 ? (
        <motion.div
          style={{ position: "absolute", left: 0, top: 0, width: 10, height: 6, backgroundColor: "#ffffff" }}
          initial={{ left: 0 }}
          animate={{ left: 80 }}
          transition={{ duration: 1, ease: "linear" }}
        />
      ) : null}
    </div>
  )
}

function Table() {
  return (
    <div testId="tbl" style={{ display: "table", width: 280, fontSize: 13 }}>
      {[0, 1, 2].map((r) => (
        <div key={r} style={{ display: "table-row", hover: { backgroundColor: "#334455" } }}>
          {[0, 1, 2].map((c) => (
            <div key={c} style={{ display: "table-cell", padding: 2, hover: { backgroundColor: "#445566" } }}>
              <text>{`r${r}c${c}`}</text>
            </div>
          ))}
        </div>
      ))}
    </div>
  )
}

const N = 14

function run(seed: number, steps: number) {
  const random = rng(seed)
  const pick = (n: number) => Math.floor(random() * n)
  let apply!: (fn: (state: any) => any) => void
  function App() {
    const [state, setState] = React.useState({
      order: Array.from({ length: N }, (_, i) => i),
      cards: Array.from({ length: N }, (_, i) => ({ label: `card ${i}`, hue: i * 20, extra: i % 3 === 0, wide: false, pad: 0, tag: i % 7 })),
      scrollerHeight: 260,
      rootFont: 16,
    })
    apply = (fn) => setState((s) => fn(s))
    return (
      <div style={{ fontSize: state.rootFont, color: "#ffffff" }}>
        <div testId="scroller" style={{ overflow: "auto", width: 620, height: state.scrollerHeight }}>
          <div style={{ display: "flex", flexWrap: "wrap", gap: 6, width: 600 }}>
            {state.order.map((i) => (
              <Card key={i} i={i} st={state.cards[i]} />
            ))}
          </div>
          <Table />
        </div>
        <div testId="footer" style={{ height: 40 }}>
          <text>footer</text>
        </div>
      </div>
    )
  }
  const root = createTestRoot({ width: 640, height: 400 })
  const { renderer } = root
  const log: string[] = []
  try {
    root.render(<App />)
    for (let f = 0; f < 4; f++) renderer.flush()
    renderer.clockPause()
    const scroller = renderer.findByTestId("scroller").id
    const point = () => [Math.floor(random() * 640), Math.floor(random() * 400)] as const
    for (let step = 0; step < steps; step++) {
      const op = pick(24)
      let name = ""
      try {
        switch (op) {
          case 0:
          case 1:
          case 2: {
            const p = point()
            name = `move ${p}`
            renderer.nativeSimulateMouseMove(...p)
            break
          }
          case 3: {
            const p = point()
            name = `down/up ${p}`
            renderer.nativeSimulateMouseDown(...p)
            renderer.flush()
            renderer.nativeSimulateMouseUp(...p)
            break
          }
          case 4:
            name = "tab"
            renderer.simulateKeystrokes(random() < 0.3 ? "Shift+Tab" : "Tab")
            break
          case 5: {
            const i = pick(N)
            name = `label ${i}`
            flushSync(() => apply((s) => ({ ...s, cards: s.cards.map((c, k) => (k === i ? { ...c, label: `edited ${step} ${"x".repeat(pick(8))}` } : c)) })))
            break
          }
          case 6: {
            const i = pick(N)
            name = `hue ${i}`
            flushSync(() => apply((s) => ({ ...s, cards: s.cards.map((c, k) => (k === i ? { ...c, hue: pick(360) } : c)) })))
            break
          }
          case 7: {
            const i = pick(N)
            name = `extra ${i}`
            flushSync(() => apply((s) => ({ ...s, cards: s.cards.map((c, k) => (k === i ? { ...c, extra: !c.extra } : c)) })))
            break
          }
          case 8: {
            const i = pick(N)
            name = `wide ${i}`
            flushSync(() => apply((s) => ({ ...s, cards: s.cards.map((c, k) => (k === i ? { ...c, wide: !c.wide } : c)) })))
            break
          }
          case 9: {
            const a = pick(N)
            const b = pick(N)
            name = `swap ${a} ${b}`
            flushSync(() =>
              apply((s) => {
                const order = [...s.order]
                const ia = order.indexOf(a)
                const ib = order.indexOf(b)
                ;[order[ia], order[ib]] = [order[ib], order[ia]]
                return { ...s, order }
              })
            )
            break
          }
          case 10: {
            name = "scroll"
            renderer.scrollTo(scroller, 0, pick(400))
            break
          }
          case 11: {
            name = "resize"
            renderer.setWindowSize(500 + pick(300), 300 + pick(200))
            break
          }
          case 12: {
            name = "appearance"
            renderer.setAppearance(random() < 0.5 ? "dark" : "light")
            break
          }
          case 13: {
            name = "clock"
            renderer.clockFastForward(pick(250))
            break
          }
          case 14: {
            const a = point()
            const b = point()
            name = `drag ${a} ${b}`
            renderer.dragSelect(...a, ...b)
            if (random() < 0.5) renderer.clearSelection()
            break
          }
          case 15: {
            const i = pick(N)
            name = `pad ${i}`
            flushSync(() => apply((s) => ({ ...s, cards: s.cards.map((c, k) => (k === i ? { ...c, pad: pick(6) } : c)) })))
            break
          }
          case 16: {
            name = "root font"
            flushSync(() => apply((s) => ({ ...s, rootFont: 12 + pick(8) })))
            break
          }
          case 18: {
            name = "window inactive/active"
            renderer.setWindowActive(random() < 0.5)
            break
          }
          case 19: {
            const p = point()
            name = `wheel ${p}`
            renderer.nativeSimulateScrollWheel(p[0], p[1], 0, pick(120) - 60)
            break
          }
          case 20: {
            name = "escape/arrow"
            renderer.simulateKeystrokes(["ArrowDown", "Escape", "Space", "Enter", "a"][pick(5)])
            break
          }
          case 21: {
            name = "retain toggle"
            renderer.setRetainEnabled(random() < 0.5)
            break
          }
          case 22: {
            name = "planning toggle"
            renderer.setRetainPlanning(random() < 0.5)
            break
          }
          case 23: {
            const i = pick(N)
            name = `tag ${i}`
            flushSync(() => apply((s) => ({ ...s, cards: s.cards.map((c, k) => (k === i ? { ...c, tag: pick(7) } : c)) })))
            break
          }
          case 17: {
            name = "scroller height"
            flushSync(() => apply((s) => ({ ...s, scrollerHeight: 150 + pick(250) })))
            break
          }
        }
        log.push(name)
        renderer.flush()
        if (random() < 0.5) renderer.flush()
      } catch (error) {
        throw new Error(`seed ${seed} step ${step} (${name}): ${error}\n${log.join("\n")}`)
      }
      const c = renderer.getRetainedCounters()
      if (c.mismatches > 0) {
        throw new Error(`seed ${seed} step ${step} (${name}): mismatches\n${renderer.getRetentionMismatches().join("\n")}\nops:\n${log.join("\n")}`)
      }
    }
    renderer.setRetainEnabled(true)
    renderer.setRetainPlanning(true)
    for (let f = 0; f < 6; f++) renderer.flush()
    // a focused input blinks its caret by real time, which is not a difference between the two builds
    const footer = renderer.getElementBounds(renderer.findByTestId("footer").id)
    if (footer) {
      renderer.nativeSimulateMouseDown(footer.x + 5, footer.y + 5)
      renderer.nativeSimulateMouseUp(footer.x + 5, footer.y + 5)
    }
    for (let f = 0; f < 6; f++) renderer.flush()
    // snapshot replayed vs whole
    const ids = ["scroller", "footer", "tbl"]
    for (let i = 0; i < N; i++) ids.push(`card-${i}`, `title-${i}`, `span-${i}`, `btn-${i}`, `row-${i}`, `pill-${i}`)
    const snap = (name: string) => {
      const file = `${SHOTS_DIR}/retained-fuzz-${seed}-${name}.png`
      fs.rmSync(file, { force: true })
      renderer.captureScreenshot(file)
      const boxes: Record<string, unknown> = {}
      for (const id of ids) {
        const el = renderer.findByTestId(id)
        boxes[id] = el ? renderer.getElementBounds(el.id) : null
      }
      return {
        boxes,
        text: renderer.getAllText(),
        a11y: JSON.stringify(renderer.getA11yTree(), (k, v) => (k === "frame" ? undefined : v)),
        picture: fs.readFileSync(file).toString("base64"),
        cursor: renderer.getCursor(),
      }
    }
    const replayed = snap("r")
    renderer.setRetainPlanning(false)
    renderer.flush()
    const whole = snap("w")
    expect(replayed.boxes, `seed ${seed}`).toEqual(whole.boxes)
    expect(replayed.text, `seed ${seed}`).toEqual(whole.text)
    expect(replayed.cursor, `seed ${seed}`).toEqual(whole.cursor)
    expect(replayed.a11y === whole.a11y, `seed ${seed} a11y`).toBe(true)
    if (replayed.picture !== whole.picture) {
      const a = readLogicalPixels(`${SHOTS_DIR}/retained-fuzz-${seed}-r.png`, renderer)
      const b = readLogicalPixels(`${SHOTS_DIR}/retained-fuzz-${seed}-w.png`, renderer)
      let minX = 1e9, minY = 1e9, maxX = -1, maxY = -1, count = 0
      for (let y = 0; y < 400; y++)
        for (let x = 0; x < 640; x++) {
          const p = a.at(x, y), q = b.at(x, y)
          if (p.some((v, i) => v !== q[i])) {
            count++
            minX = Math.min(minX, x); minY = Math.min(minY, y); maxX = Math.max(maxX, x); maxY = Math.max(maxY, y)
          }
        }
      const owners = ids.filter((id) => {
        const el = renderer.findByTestId(id)
        const box = el && renderer.getElementBounds(el.id)
        return box && box.x <= maxX && box.x + box.width >= minX && box.y <= maxY && box.y + box.height >= minY
      })
      console.log(`DIFF seed ${seed}: ${count} px in [${minX},${minY}]-[${maxX},${maxY}] ids ${owners.join(",")} size ${JSON.stringify(renderer.getWindowSize())} a11y ${JSON.stringify(renderer.getAppearance())}`)
    }
    expect(replayed.picture === whole.picture, `seed ${seed} picture\n${log.slice(-12).join("\n")}`).toBe(true)
    return renderer.getRetainedCounters()
  } finally {
    root.unmount()
  }
}

describeNative("retention under random interactions", () => {
  const seeds = (process.env.FUZZ_SEEDS ?? "1,2,3,4,5,6").split(",").map(Number)
  for (const seed of seeds) {
    it(`seed ${seed}`, () => {
      const counters = run(seed, Number(process.env.FUZZ_STEPS ?? 80))
      // A run that never replayed would compare a build with itself.
      expect(counters.replays).toBeGreaterThan(0)
    }, 120000)
  }
})
