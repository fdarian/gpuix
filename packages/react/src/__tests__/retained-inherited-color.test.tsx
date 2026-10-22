// @ts-nocheck
/**
 * A change of the `color` a page hands down is a change to the roots that read it, not to every root below the page:
 * a card whose text sets its own colour draws the same under any colour of the page, and is replayed. What reads the
 * colour it inherits (a text that sets none, a `currentColor`, an svg, an input) is built, and nothing that reads it
 * is replayed with the colour it had.
 *
 * Run with GPUIX_RETAIN_VERIFY=1 too: the counts of builds are not read there (every replayed frame is built a second
 * time), only that the frames and what they replay agree.
 */
import fs from "fs"
import React from "react"
import { expect, it } from "vitest"
import { describeNative, setup, verifying } from "./retained-harness.js"
import { SHOTS_DIR } from "./test-utils.js"

const COUNT = 12
const retained = process.env.GPUIX_RETAIN_OFF === undefined
const INK = ["#223344", "#cc3300"]

const Page = ({ ink, card }: { ink: string; card: (index: number) => React.ReactNode }) => (
  <div testId="page" style={{ display: "flex", flexWrap: "wrap", gap: 8, width: 640, color: ink }}>
    {Array.from({ length: COUNT }, (_, index) => (
      <React.Fragment key={index}>{card(index)}</React.Fragment>
    ))}
  </div>
)

const card = { width: 140, height: 60, backgroundColor: "#335577" }

/** The text sets its own colour: nothing in the card reads the colour of the page. */
const own = (index: number) => (
  <div testId={`card-${index}`} style={card}>
    <div style={{ height: 20, backgroundColor: "#223344" }}>
      <text style={{ color: "#ffffff" }}>{`own ${index}`}</text>
    </div>
    <div style={{ height: 20 }}>
      <text style={{ color: "#ffee00" }}>two</text>
    </div>
  </div>
)

/** The same card, the text of one of its rows takes the colour of the page. */
const inherits = (index: number) => (
  <div testId={`card-${index}`} style={card}>
    <div style={{ height: 20, backgroundColor: "#223344" }}>
      <text style={{ color: "#ffffff" }}>{`own ${index}`}</text>
    </div>
    <div style={{ height: 20 }}>
      <text>two</text>
    </div>
  </div>
)

/** The card sets its own text colour and draws its border with `currentColor`, which is the page's. */
const border = (index: number) => (
  <div
    testId={`card-${index}`}
    style={{ ...card, borderWidth: 3, borderStyle: "solid", borderColor: "currentColor" }}
  >
    <div style={{ height: 20 }}>
      <text style={{ color: "#ffffff" }}>{`own ${index}`}</text>
    </div>
    <div style={{ height: 20 }}>
      <text style={{ color: "#ffee00" }}>two</text>
    </div>
  </div>
)

/** The same border on a card that sets its own `color`: `currentColor` is the card's. */
const borderOfItsOwn = (index: number) => (
  <div
    testId={`card-${index}`}
    style={{ ...card, color: "#00aa44", borderWidth: 3, borderStyle: "solid", borderColor: "currentColor" }}
  >
    <div style={{ height: 20 }}>
      <text>{`own ${index}`}</text>
    </div>
    <div style={{ height: 20 }}>
      <text>two</text>
    </div>
  </div>
)

/** A colour declared in the card cuts the dependency below it: the texts below inherit the card's. */
const declaredAbove = (index: number) => (
  <div testId={`card-${index}`} style={{ ...card, color: "#00aa44" }}>
    <div style={{ height: 20, backgroundColor: "#223344" }}>
      <text>{`own ${index}`}</text>
    </div>
    <div style={{ height: 20, borderWidth: 2, borderStyle: "solid", borderColor: "currentColor" }}>
      <text>two</text>
    </div>
  </div>
)

/** Only below the colour: what is beside it, above it in the card, reads the page's. */
const declaredBelow = (index: number) => (
  <div testId={`card-${index}`} style={card}>
    <div style={{ height: 20, backgroundColor: "#223344" }}>
      <text>{`own ${index}`}</text>
    </div>
    <div style={{ height: 20, color: "#00aa44" }}>
      <text>two</text>
    </div>
  </div>
)

/** The colour is declared under an element that reads the page's: the one under it is cut, the one above is not. */
const readAboveTheDeclaration = (index: number) => (
  <div testId={`card-${index}`} style={{ ...card, borderWidth: 3, borderStyle: "solid", borderColor: "currentColor" }}>
    <div style={{ height: 20, color: "#00aa44" }}>
      <text>two</text>
    </div>
  </div>
)

/** A colour set as the page's own, or from it: it does not cut. */
const currentColor = (index: number) => (
  <div testId={`card-${index}`} style={{ ...card, color: "currentColor" }}>
    <div style={{ height: 20 }}>
      <text>{`own ${index}`}</text>
    </div>
  </div>
)
const mixed = (index: number) => (
  <div testId={`card-${index}`} style={{ ...card, color: "color-mix(in srgb, currentColor 50%, #ffffff)" }}>
    <div style={{ height: 20 }}>
      <text>{`own ${index}`}</text>
    </div>
  </div>
)

/** One frame: the update, and the draw. What it built and replayed. */
const frame = (view, render?: React.ReactElement) => {
  const { renderer } = view
  const before = renderer.getRetainedCounters()
  const builtBefore = view.built()
  if (render) view.render(render)
  renderer.drawFrame()
  const after = renderer.getRetainedCounters()
  return {
    builds: after.builds - before.builds,
    hits: after.hits - before.hits,
    replays: after.replays - before.replays,
    built: view.built() - builtBefore,
  }
}

const emptyWindow = () => {
  const view = setup(<div />)
  view.renderer.setInlineDraws(false)
  return view
}

const shot = (view, name: string) => {
  const file = `${SHOTS_DIR}/retained-inherited-color-${process.pid}-${name}.png`
  fs.rmSync(file, { force: true })
  view.renderer.captureScreenshot(file)
  return fs.readFileSync(file).toString("base64")
}

/** The frame as it is, then the same state built whole: they have to be the same picture. */
const expectWhatABuildShows = (view, name: string) => {
  const { renderer } = view
  const replayed = shot(view, `${name}-replayed`)
  const text = renderer.getAllText()
  renderer.setRetainPlanning(false)
  renderer.drawFrame()
  const whole = shot(view, `${name}-built`)
  renderer.setRetainPlanning(true)
  renderer.drawFrame()
  expect(text).toEqual(renderer.getAllText())
  expect(replayed === whole, name).toBe(true)
}

/** Mounts the page, lets the frames record it, hands down another colour, then the first again. */
const recolour = (kind: (index: number) => React.ReactNode, name: string) => {
  const view = emptyWindow()
  try {
    const mount = frame(view, <Page ink={INK[0]} card={kind} />)
    frame(view)
    frame(view)
    const changed = frame(view, <Page ink={INK[1]} card={kind} />)
    const after = frame(view)
    expectWhatABuildShows(view, name)
    const back = frame(view, <Page ink={INK[0]} card={kind} />)
    expectWhatABuildShows(view, `${name}-back`)
    view.finish(false)
    return { mount, changed, after, back }
  } finally {
    view.unmount()
  }
}

/** The page and nothing below it: the cards are replayed. */
const expectReplayed = (frames: object, message: string) => {
  expect(frames.builds, message).toBeLessThanOrEqual(1)
  expect(frames.hits, message).toBeGreaterThanOrEqual(COUNT)
  expect(frames.built, message).toBeLessThan(5)
}

/** The cards are built, and what is beside what reads the colour in them is replayed or not as it reads. */
const expectBuilt = (frames: object, mount: object, message: string) => {
  expect(frames.built, message).toBeGreaterThanOrEqual(COUNT * 3)
  expect(frames.built, message).toBeLessThanOrEqual(mount.built)
}

describeNative("a colour handed down by the page", () => {
  it("builds no card whose text sets its own colour", () => {
    const { changed, after, back } = recolour(own, "own")
    if (retained && !verifying) {
      expectReplayed(changed, "changed")
      expect(changed.hits, "every card").toBe(COUNT)
      expect(after.built).toBe(0)
      expectReplayed(back, "back")
    }
  })

  it("builds the cards whose text takes it", () => {
    const { mount, changed, after, back } = recolour(inherits, "inherits")
    if (retained && !verifying) {
      expectBuilt(changed, mount, "changed")
      expect(changed.hits, "the row that sets its own colour is replayed").toBe(COUNT)
      expect(after.built, "the frame after has nothing to build").toBe(0)
      expectBuilt(back, mount, "back")
    }
  })

  it("builds a card that draws its border with currentColor though its text sets its own", () => {
    const { mount, changed } = recolour(border, "border")
    if (retained && !verifying) {
      expect(changed.built).toBeGreaterThanOrEqual(COUNT)
      expect(changed.built, "the rows, whose text sets its own colour, are replayed").toBeLessThan(COUNT * 2)
      expect(changed.hits).toBe(COUNT * 2)
      expect(changed.built).toBeLessThan(mount.built)
    }
  })

  it("replays a card whose currentColor is its own", () => {
    const { changed } = recolour(borderOfItsOwn, "border-of-its-own")
    if (retained && !verifying) {
      expectReplayed(changed, "changed")
      expect(changed.hits).toBe(COUNT)
    }
  })

  it("is cut by a colour declared in the card, for what is below it", () => {
    const { changed } = recolour(declaredAbove, "declared-above")
    if (retained && !verifying) {
      expectReplayed(changed, "changed")
      expect(changed.hits).toBe(COUNT)
    }
  })

  it("is cut only below a colour declared in the card", () => {
    const { mount, changed } = recolour(declaredBelow, "declared-below")
    if (retained && !verifying) {
      expectBuilt(changed, mount, "the text beside the declaration takes the page's")
      expect(changed.hits, "the row under the declaration is replayed").toBe(COUNT)
    }
  })

  it("is read by what is above a colour declared in the card", () => {
    const { changed } = recolour(readAboveTheDeclaration, "read-above")
    if (retained && !verifying) {
      expect(changed.built, "the card is built").toBe(COUNT + 1)
      expect(changed.hits, "the row that declares the colour is replayed").toBe(COUNT)
    }
  })

  it("is not cut by a colour that is currentColor or made from it", () => {
    for (const [kind, name] of [
      [currentColor, "current-color"],
      [mixed, "mixed"],
    ]) {
      const { mount, changed } = recolour(kind, name)
      if (retained && !verifying) {
        expect(changed.hits, name).toBe(0)
        expectBuilt(changed, mount, name)
      }
    }
  })

  it("is replayed or built by card, as each card reads it", () => {
    const kinds = [own, inherits, border, borderOfItsOwn, declaredAbove, declaredBelow]
    const view = emptyWindow()
    try {
      const mixedPage = (ink: string) => <Page ink={ink} card={(index) => kinds[index % kinds.length](index)} />
      const mount = frame(view, mixedPage(INK[0]))
      frame(view)
      frame(view)
      const changed = frame(view, mixedPage(INK[1]))
      if (retained && !verifying) {
        // `own`, `borderOfItsOwn` and `declaredAbove` are twice each, and the rows of the others that set their own.
        expect(changed.hits).toBeGreaterThanOrEqual(6)
        expect(changed.built).toBeLessThan(mount.built / 2)
      }
      expectWhatABuildShows(view, "mixed")
      frame(view)
      expectWhatABuildShows(view, "mixed-after")
      view.finish(false)
    } finally {
      view.unmount()
    }
  })
})

const ICON =
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect width="16" height="16" fill="currentColor"/></svg>'

/** Cards that set no colour and draw something with the colour they inherit, one way each. */
const READERS: Record<string, (index: number) => React.ReactNode> = {
  text: (index) => <text>{`text ${index}`}</text>,
  "inline text": (index) => (
    <div>
      <span>one</span>
      <span> two</span>
    </div>
  ),
  "an underlined span": (index) => (
    <div>
      <span style={{ textDecoration: "underline" }}>one</span>
    </div>
  ),
  border: (index) => (
    <div style={{ height: 20, borderWidth: 3, borderStyle: "solid", borderColor: "currentColor" }} />
  ),
  outline: (index) => (
    <div style={{ height: 20, outlineWidth: 3, outlineStyle: "solid", outlineColor: "currentColor" }} />
  ),
  "an outline with no colour": (index) => <div style={{ height: 20, outlineWidth: 3, outlineStyle: "solid" }} />,
  shadow: (index) => (
    <div
      style={{
        height: 20,
        margin: 6,
        boxShadow: { offsetX: 0, offsetY: 0, blurRadius: 0, spreadRadius: 4, color: "currentColor" },
      }}
    />
  ),
  background: (index) => <div style={{ height: 20, backgroundColor: "currentColor" }} />,
  gradient: (index) => (
    <div style={{ height: 20, backgroundImage: "linear-gradient(90deg, currentColor, transparent)" }} />
  ),
  "a colour mixed from it": (index) => (
    <div style={{ height: 20, backgroundColor: "color-mix(in srgb, currentColor 40%, #ffffff)" }} />
  ),
  filter: (index) => (
    <div style={{ height: 20, margin: 6, backgroundColor: "#ffffff", filter: "drop-shadow(4px 4px 0 currentColor)" }} />
  ),
  svg: (index) => (
    <svg viewBox="0 0 10 10" width={18} height={12}>
      <rect x={1} y={1} width={8} height={8} fill="currentColor" />
    </svg>
  ),
  "an svg source": (index) => <svg source={ICON} style={{ width: 18, height: 12, color: "currentColor" }} />,
  input: (index) => <input testId={`in-${index}`} defaultValue="typed" style={{ height: 16, width: 60 }} />,
}

/** Cards that name the colour of the page where it is not used, and are replayed. */
const NOT_READERS: Record<string, (index: number) => React.ReactNode> = {
  // A border with no colour is transparent here.
  "a border with no colour": (index) => <div style={{ height: 20, borderWidth: 3, borderStyle: "solid" }} />,
  // A state resolves its colours when it is the state, which is a build.
  "a state": (index) => (
    <div testId={`state-${index}`} style={{ height: 20, hover: { backgroundColor: "currentColor" } }} />
  ),
}

const readerCard = (reader: (index: number) => React.ReactNode) => (index: number) => (
  <div testId={`card-${index}`} style={{ width: 140, backgroundColor: "#335577" }}>
    <div style={{ height: 4 }} />
    {reader(index)}
    <div style={{ height: 4 }} />
  </div>
)

describeNative("what reads the colour a page hands down", () => {
  for (const [name, reader] of Object.entries(READERS)) {
    it(`is a card with ${name}, built under another colour and drawn as a build draws it`, () => {
      const kind = readerCard(reader)
      const view = emptyWindow()
      try {
        frame(view, <Page ink={INK[0]} card={kind} />)
        frame(view)
        frame(view)
        const changed = frame(view, <Page ink={INK[1]} card={kind} />)
        if (retained && !verifying) {
          expect(changed.hits, "no card is replayed with the colour it had").toBe(0)
          expect(changed.built).toBeGreaterThan(COUNT * 3)
        }
        expectWhatABuildShows(view, name)
        view.finish(false)
      } finally {
        view.unmount()
      }
    })
  }

  for (const [name, reader] of Object.entries(NOT_READERS)) {
    it(`is not read by a card with ${name}, which is drawn as a build draws it`, () => {
      const kind = readerCard(reader)
      const view = emptyWindow()
      try {
        frame(view, <Page ink={INK[0]} card={kind} />)
        frame(view)
        frame(view)
        const changed = frame(view, <Page ink={INK[1]} card={kind} />)
        if (retained && !verifying) {
          expect(changed.hits).toBe(COUNT)
          expect(changed.built).toBeLessThan(5)
        }
        expectWhatABuildShows(view, name)
        if (name === "a state") {
          const box = view.bounds("state-3")
          view.renderer.nativeSimulateMouseMove(box.x + 5, box.y + 5)
          view.renderer.drawFrame()
          expectWhatABuildShows(view, "a-state-hovered")
        }
        view.finish(false)
      } finally {
        view.unmount()
      }
    })
  }
})
