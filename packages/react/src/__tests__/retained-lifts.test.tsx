// @ts-nocheck
/**
 * A subtree with a positioned box in it is replayed with the box. The box that is lifted into the
 * stacking context around the subtree is a hole in the recording of the subtree (when it is a
 * root of its own) or a fragment of it, and the context paints it from the recording at its
 * place in the paint order. Each test runs one scenario twice, with retention on and with it
 * off, and expects everything the scenario observed to be the same: hits, boxes, painted text,
 * the accessibility tree, the selection, the pixels. Under GPUIX_RETAIN_VERIFY the frames that
 * replay are also built whole and compared, whole frame at a time.
 */

import crypto from "crypto"
import fs from "fs"
import React from "react"
import { expect, it } from "vitest"
import { flushSync, type PublicInstance } from "../index"
import { createTestRoot } from "../testing"
import { Card, CARDS, describeNative, Page, setup, verifying, WINDOW } from "./retained-harness.js"
import { SHOTS_DIR } from "./test-utils.js"

type Kit = ReturnType<typeof setup>

/** What the frame shows and answers, as text: the same for two frames that are the same. */
function observe(kit: Kit, name: string, probes: Array<[number, number]> = [], watch: string[] = []) {
  const { renderer } = kit
  const file = `${SHOTS_DIR}/retained-lifts-${name}.png`
  fs.rmSync(file, { force: true })
  renderer.captureScreenshot(file)
  const names = (id: number) => renderer.getElement(id)?.testId ?? `#${id}`
  return {
    picture: crypto.createHash("sha1").update(fs.readFileSync(file)).digest("hex"),
    a11y: JSON.stringify(renderer.getA11yTree(), (key, value) => (key === "frame" ? undefined : value)),
    text: renderer.getAllText(),
    painted: renderer.getPaintedText(),
    hits: probes.map(([x, y]) => renderer.getElementsAt(x, y).map(names)),
    boxes: watch.map((testId) => {
      const element = renderer.findByTestId(testId)
      return element ? renderer.getElementBounds(element.id) : null
    }),
    cursor: renderer.getCursor(),
    selected: renderer.getSelectedText(),
  }
}

/** `scenario` on a fresh page with retention `on`, and what it logged. */
function run(on: boolean, page: React.ReactElement, scenario: (kit: Kit, log: unknown[]) => void) {
  const log: unknown[] = []
  const kit = setup(page)
  try {
    if (!on) kit.renderer.setRetainPlanning(false)
    scenario(kit, log)
    const counters = kit.counters()
    return { log, replays: counters.replays, mismatches: counters.mismatches, lift: JSON.parse(counters.nonReusable).Lift ?? 0 }
  } finally {
    kit.unmount()
  }
}

/** What the scenario logged with retention on is what it logged with it off, and the page replayed. */
function expectSame(page: React.ReactElement, scenario: (kit: Kit, log: unknown[]) => void) {
  const off = run(false, page, scenario)
  const on = run(true, page, scenario)
  expect(on.log).toEqual(off.log)
  expect(on.mismatches).toBe(0)
  expect(on.replays).toBeGreaterThan(0)
  // Positioned boxes do not keep a subtree from being replayed.
  expect(on.lift).toBe(0)
  return on.log
}

const pill = (index: number, clicks?: string[], extra = {}) => (
  <div
    testId={`pill-${index}`}
    onClick={() => clicks?.push(`pill-${index}`)}
    style={{
      position: "absolute",
      right: 4,
      top: 4,
      width: 30,
      height: 14,
      backgroundColor: "#cc3399",
      hover: { backgroundColor: "#ffcc00" },
      ...extra,
    }}
  >
    <text>{index}</text>
  </div>
)

/** A card that places its children against itself, one of which is an absolute pill. */
const PositionedPage = ({ clicks, pillStyle }: { clicks?: string[]; pillStyle?: object }) => (
  <Page
    card={(index) => (
      <Card
        key={index}
        index={index}
        style={{ position: "relative" }}
        onClick={() => clicks?.push(`button-${index}`)}
        extra={pill(index, clicks, pillStyle)}
      />
    )}
  />
)

const CARD = { width: 140, height: 90 }

describeNative("positioned boxes inside replayed subtrees", () => {
  it("replays cards whose absolute pills are lifted out of them, and answers for the pills", () => {
    const clicks: string[] = []
    const page = <PositionedPage clicks={clicks} />
    const probes = (kit: Kit) =>
      [3, 4, 9].flatMap((index) => {
        const box = kit.bounds(`pill-${index}`)
        return [
          [box.x + 5, box.y + 5],
          [box.x - 3, box.y + 5],
        ] as Array<[number, number]>
      })
    expectSame(page, (kit, log) => {
      const { renderer } = kit
      kit.settle()
      clicks.length = 0
      log.push(kit.bounds("pill-3"), kit.bounds("card-3"))
      log.push(observe(kit, "pills-idle", probes(kit), ["pill-0", "pill-5", "card-5"]))
      renderer.nativeSimulateMouseMove(...kit.center("pill-4"))
      renderer.flush()
      log.push(observe(kit, "pills-hover", probes(kit), ["pill-4"]))
      renderer.nativeSimulateClick(...kit.center("pill-4"))
      renderer.nativeSimulateClick(...kit.center("button-4"))
      renderer.nativeSimulateClick(...kit.center("pill-9"))
      log.push([...clicks])
      renderer.nativeSimulateMouseMove(...kit.center("title-9"))
      renderer.flush()
      log.push(observe(kit, "pills-moved", probes(kit)))
      renderer.nativeSimulateMouseMove(639, 479)
      kit.settle()
      log.push(observe(kit, "pills-off", probes(kit)))
    })
    expect(clicks).toEqual(["pill-4", "button-4", "pill-9"])
  })

  it("builds the cards whose pill changes, and none of the others", () => {
    if (verifying) return
    const page = <PositionedPage />
    const kit = setup(page)
    try {
      kit.settle()
      const before = kit.counters()
      const builtBefore = kit.built()
      kit.renderer.flush()
      kit.renderer.flush()
      expect(kit.counters().replays).toBeGreaterThan(before.replays)
      expect(kit.built() - builtBefore, "an idle page builds nothing").toBe(0)
      expect(JSON.parse(kit.counters().nonReusable).Lift ?? 0).toBe(0)
      const idle = kit.counters()
      kit.renderer.nativeSimulateMouseMove(...kit.center("pill-6"))
      kit.renderer.flush()
      // The card under the pointer, its pill and the page above them; not twelve cards.
      expect(kit.built() - builtBefore).toBeLessThan(CARDS * 3)
      expect(kit.counters().replays).toBeGreaterThan(idle.replays)
      kit.finish()
    } finally {
      kit.unmount()
    }
  })

  it("paints a z-index overlay above the in-flow siblings it overlaps, and hits it first", () => {
    // Each card has an overlay that reaches into the next row of cards, over an in-flow sibling.
    const clicks: string[] = []
    const page = (
      <Page
        card={(index) => (
          <Card
            key={index}
            index={index}
            style={{ position: index % 2 ? "relative" : undefined }}
            extra={
              <div
                testId={`overlay-${index}`}
                onClick={() => clicks.push(`overlay-${index}`)}
                style={{
                  position: "relative",
                  zIndex: index % 3 === 0 ? -1 : 5,
                  marginTop: -30,
                  marginLeft: 100,
                  width: 80,
                  height: 70,
                  backgroundColor: index % 2 ? "#33cc33" : "#3333cc",
                }}
              >
                <text>{`o${index}`}</text>
              </div>
            }
          />
        )}
      />
    )
    expectSame(page, (kit, log) => {
      const { renderer } = kit
      kit.settle()
      const probes = [1, 2, 5, 6, 8].flatMap((index) => {
        const box = kit.bounds(`overlay-${index}`)
        return [
          [box.x + 5, box.y + 5],
          [box.x + 40, box.y + 60],
          [box.x + box.width - 5, box.y + 2],
        ] as Array<[number, number]>
      })
      log.push(observe(kit, "overlay-idle", probes, ["overlay-1", "overlay-6"]))
      for (const index of [1, 6]) {
        clicks.length = 0
        const box = kit.bounds(`overlay-${index}`)
        renderer.nativeSimulateMouseMove(box.x + 20, box.y + 20)
        renderer.flush()
        renderer.nativeSimulateClick(box.x + 20, box.y + 20)
        log.push([...clicks], observe(kit, `overlay-hover-${index}`, probes))
      }
      renderer.nativeSimulateMouseMove(639, 479)
      kit.settle()
      log.push(observe(kit, "overlay-off", probes))
    })
  })

  it("keeps a relative row with a z-index and absolute children right while a hover sweeps the rows", () => {
    // The sidebar of the component library: rows are relative with zIndex 10, each has an absolute
    // indicator and badge, and only the row under the pointer changes.
    const ROWS = 10
    const clicks: string[] = []
    const Row = ({ index }: { index: number }) => (
      <div
        testId={`row-${index}`}
        onClick={() => clicks.push(`row-${index}`)}
        style={{
          position: "relative",
          zIndex: 10,
          height: 28,
          width: 180,
          paddingLeft: 12,
          backgroundColor: "#202830",
          hover: { backgroundColor: "#384858" },
        }}
      >
        <div
          testId={`bar-${index}`}
          style={{ position: "absolute", left: 0, top: 4, width: 3, height: 20, backgroundColor: "#00aaff", hover: { backgroundColor: "#ffffff" } }}
        />
        <text>{`item ${index}`}</text>
        <div
          testId={`badge-${index}`}
          onClick={() => clicks.push(`badge-${index}`)}
          style={{ position: "absolute", right: 6, top: 6, width: 24, height: 16, backgroundColor: "#aa4400" }}
        >
          <text>{index}</text>
        </div>
      </div>
    )
    const page = (
      <div testId="sidebar" style={{ width: 200 }}>
        {Array.from({ length: ROWS }, (_, index) => (
          <Row key={index} index={index} />
        ))}
      </div>
    )
    const out = expectSame(page, (kit, log) => {
      const { renderer } = kit
      kit.settle()
      const watch = Array.from({ length: ROWS }, (_, index) => `badge-${index}`)
      const probes = Array.from({ length: ROWS }, (_, index) => {
        const box = kit.bounds(`row-${index}`)
        return [box.x + 100, box.y + 14] as [number, number]
      })
      for (let index = 0; index < ROWS; index++) {
        const row = kit.bounds(`row-${index}`)
        renderer.nativeSimulateMouseMove(row.x + 60, row.y + 14)
        renderer.flush()
        log.push(observe(kit, `sweep-${index}`, probes, watch))
      }
      clicks.length = 0
      renderer.nativeSimulateClick(...kit.center("badge-7"))
      renderer.nativeSimulateClick(...kit.center("row-7"))
      log.push([...clicks])
      renderer.nativeSimulateMouseMove(639, 479)
      kit.settle()
      log.push(observe(kit, "sweep-off", probes, watch))
    })
    expect(out.length).toBeGreaterThan(ROWS)
  })

  it("sweeps the same sidebar built by cards, replaying every row but the one under the pointer", () => {
    if (verifying) return
    const ROWS = 10
    const Row = ({ index }: { index: number }) => (
      <div
        testId={`row-${index}`}
        style={{ position: "relative", zIndex: 10, height: 28, width: 180, backgroundColor: "#202830", hover: { backgroundColor: "#384858" } }}
      >
        <div style={{ position: "absolute", left: 0, top: 4, width: 3, height: 20, backgroundColor: "#00aaff" }} />
        <text>{`item ${index}`}</text>
        <div style={{ position: "absolute", right: 6, top: 6, width: 24, height: 16, backgroundColor: "#aa4400" }}>
          <text>{index}</text>
        </div>
      </div>
    )
    const kit = setup(
      <div style={{ width: 200 }}>
        {Array.from({ length: ROWS }, (_, index) => (
          <Row key={index} index={index} />
        ))}
      </div>
    )
    try {
      kit.settle()
      kit.renderer.flush()
      const idle = kit.built()
      kit.renderer.flush()
      expect(kit.built() - idle).toBe(0)
      const replays = kit.counters().replays
      const builtBefore = kit.built()
      for (let index = 0; index < ROWS; index++) {
        const row = kit.bounds(`row-${index}`)
        kit.renderer.nativeSimulateMouseMove(row.x + 60, row.y + 14)
        kit.renderer.flush()
      }
      expect(kit.counters().replays).toBeGreaterThan(replays)
      // Per step the row left and the row entered change: six elements each, with the page above them.
      expect((kit.built() - builtBefore) / ROWS).toBeLessThan(ROWS * 6 * 0.5)
      kit.finish()
    } finally {
      kit.unmount()
    }
  })

  it("selects text across cards under a lifted overlay, in the order the frame registers it", () => {
    const page = (
      <Page
        card={(index) => (
          <Card
            key={index}
            index={index}
            style={{ position: index % 2 ? "relative" : undefined }}
            extra={
              index === 5 || index === 1 ? (
                <div
                  testId={`cover-${index}`}
                  style={{ position: "absolute", left: 0, top: 0, width: 140, height: 28, backgroundColor: "#445566", zIndex: 4 }}
                >
                  <text>{`cover ${index}`}</text>
                </div>
              ) : undefined
            }
          />
        )}
      />
    )
    expectSame(page, (kit, log) => {
      const { renderer } = kit
      kit.settle()
      const from = kit.bounds("title-0")
      const to = kit.bounds("title-6")
      log.push(renderer.dragSelect(from.x + 2, from.y + 5, to.x + 60, to.y + 5))
      kit.settle()
      log.push(observe(kit, "selected", [[from.x + 20, from.y + 5]], ["cover-1", "cover-5"]))
      // Starting a selection on the text of the overlay.
      renderer.clearSelection()
      const cover = kit.bounds("cover-5")
      log.push(renderer.dragSelect(cover.x + 2, cover.y + 8, cover.x + 60, cover.y + 8))
      kit.settle()
      log.push(observe(kit, "selected-cover", [[cover.x + 20, cover.y + 8]]))
      renderer.clearSelection()
      kit.settle()
      log.push(observe(kit, "selection-cleared"))
    })
  })

  it("answers getBoundingClientRect of a replayed absolute, and reads what was just written", () => {
    const COUNT = 60
    const PROBE = 17
    let write!: (wide: boolean) => void
    const probe = React.createRef<PublicInstance>()
    const other = React.createRef<PublicInstance>()
    function Big() {
      const [wide, setWide] = React.useState(false)
      write = setWide
      return (
        <div testId="page" style={{ display: "flex", flexWrap: "wrap", gap: 8, width: 640 }}>
          {Array.from({ length: COUNT }, (_, index) => (
            <Card
              key={index}
              index={index}
              style={{ position: "relative" }}
              extra={
                <div
                  ref={index === PROBE ? probe : index === 3 ? other : undefined}
                  testId={`pill-${index}`}
                  style={{ position: "absolute", right: 4, top: 4, width: index === PROBE && wide ? 50 : 30, height: 14, backgroundColor: "#cc3399" }}
                />
              }
            />
          ))}
        </div>
      )
    }
    const readings = (on: boolean) => {
      const root = createTestRoot({ ...WINDOW, height: 780 })
      const { renderer } = root
      const out: unknown[] = []
      try {
        root.render(<Big />)
        for (let frame = 0; frame < 5; frame++) renderer.flush()
        if (!on) renderer.setRetainPlanning(false)
        renderer.flush()
        renderer.flush()
        out.push(probe.current!.getBoundingClientRect(), other.current!.getBoundingClientRect())
        // A live window is drawn by the display link; a read between two frames is a pass of its own.
        renderer.setInlineDraws(false)
        renderer.drawFrame()
        renderer.drawFrame()
        flushSync(() => write(true))
        out.push(probe.current!.getBoundingClientRect(), other.current!.getBoundingClientRect())
        renderer.drawFrame()
        out.push(probe.current!.getBoundingClientRect(), other.current!.getBoundingClientRect())
        flushSync(() => write(false))
        out.push(probe.current!.getBoundingClientRect())
        renderer.setInlineDraws(true)
        renderer.flush()
        renderer.flush()
        out.push(probe.current!.getBoundingClientRect(), other.current!.getBoundingClientRect())
        expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join("\n")).toBe(0)
        if (on) expect(renderer.getRetainedCounters().replays).toBeGreaterThan(0)
        return out
      } finally {
        renderer.setInlineDraws(true)
        root.unmount()
      }
    }
    const on = readings(true)
    expect(on).toEqual(readings(false))
    expect(on[0].width).toBe(30)
    expect(on[2].width).toBe(50)
    expect(on[5].width).toBe(30)
  }, 30_000)

  it("hands a lifted box to the context of an opacity group that is replayed whole", () => {
    // An effect group is a context: the pill is painted inside the group, and the group is a root.
    const page = (
      <Page
        card={(index) => (
          <div key={index} testId={`group-${index}`} style={{ opacity: index % 2 ? 0.6 : 1, position: "relative", zIndex: index % 3 === 0 ? 1 : undefined }}>
            <Card index={index} style={{ position: "relative" }} extra={pill(index)} />
          </div>
        )}
      />
    )
    expectSame(page, (kit, log) => {
      const { renderer } = kit
      kit.settle()
      const probes = [2, 3].map((index) => kit.center(`pill-${index}`) as [number, number])
      log.push(observe(kit, "group-idle", probes, ["pill-2", "pill-3"]))
      renderer.nativeSimulateMouseMove(...kit.center("pill-3"))
      renderer.flush()
      log.push(observe(kit, "group-hover", probes, ["pill-3"]))
      renderer.nativeSimulateMouseMove(...kit.center("title-2"))
      renderer.flush()
      log.push(observe(kit, "group-moved", probes))
    })
  })

  it("rebuilds a card whose pill changes position and shows the new one in the frames that replay it", () => {
    let move!: (left: number) => void
    function Moving() {
      const [left, setLeft] = React.useState(0)
      move = setLeft
      return (
        <Page
          card={(index) => (
            <Card
              key={index}
              index={index}
              style={{ position: "relative" }}
              extra={pill(index, undefined, index === 8 ? { right: undefined, left: 4 + left } : {})}
            />
          )}
        />
      )
    }
    expectSame(<Moving />, (kit, log) => {
      const { renderer } = kit
      kit.settle()
      log.push(kit.bounds("pill-8"))
      for (const left of [10, 30, 30, 0]) {
        flushSync(() => move(left))
        renderer.flush()
        renderer.flush()
        log.push(kit.bounds("pill-8"), observe(kit, `moving-${left}`, [kit.center("pill-8") as [number, number]]))
      }
      kit.settle()
    })
  })

  it("replays cards with a lifted box through a scroll and a shrunk scroller, as a build shows them", () => {
    // Cards above the clip hold roots recorded for the scroller to move, inside a card that is not.
    let resize!: (height: number) => void
    function Scrolled() {
      const [height, setHeight] = React.useState(260)
      resize = setHeight
      return (
        <div testId="scroller" style={{ overflow: "auto", width: 620, height }}>
          <div style={{ display: "flex", flexWrap: "wrap", gap: 6, width: 600 }}>
            {Array.from({ length: 14 }, (_, index) => (
              <div key={index} testId={`card-${index}`} style={{ width: 140, height: 100, backgroundColor: "#335577" }}>
                <div style={{ height: 18 }}>
                  <text>{`card ${index}`}</text>
                </div>
                <div style={{ height: 14, position: "relative", zIndex: 2 }}>
                  <text>extra</text>
                </div>
              </div>
            ))}
          </div>
        </div>
      )
    }
    const root = createTestRoot({ width: 640, height: 400 })
    const { renderer } = root
    try {
      root.render(<Scrolled />)
      for (let frame = 0; frame < 5; frame++) renderer.flush()
      const scroller = renderer.findByTestId("scroller").id
      const steps = [
        () => renderer.scrollTo(scroller, 0, 250),
        () => flushSync(() => resize(170)),
        () => renderer.nativeSimulateMouseMove(300, 80),
        () => renderer.scrollTo(scroller, 0, 100),
        () => flushSync(() => resize(300)),
        () => renderer.nativeSimulateMouseMove(100, 100),
      ]
      steps.forEach((step, n) => {
        step()
        for (let frame = 0; frame < 3; frame++) renderer.flush()
        expect(renderer.getRetentionMismatches().join("\n"), `step ${n}`).toBe("")
      })
      expect(renderer.getRetainedCounters().replays).toBeGreaterThan(0)
    } finally {
      root.unmount()
    }
  })
})
