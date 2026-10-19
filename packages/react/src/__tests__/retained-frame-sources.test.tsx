// @ts-nocheck
/**
 * Whatever changes per frame without a prop change, inside a root a frame could replay, must put its
 * element and the ones above it on the spine, or the root is replayed with it frozen. Each case runs
 * in a card among eleven that are replayed, three ways: directly, below a `display: contents` wrapper
 * above the card, and below one inside it. It is run with retention on and with it off, from the same
 * clock, and what the page shows has to be the same frame by frame (and has to have moved, or the
 * case proves nothing). Run with GPUIX_RETAIN_VERIFY=1 and GPUIX_RETAIN_OFF=1 too.
 */
import React from "react"
import { expect, it, vi } from "vitest"
import {
  AnimatePresence,
  animate,
  defineKeyframes,
  IntersectionObserver,
  motion,
  ResizeObserver,
  useMotionValue,
  useSpring,
  useTransform,
} from "../index"
import { Card, describeNative, Page, setup, verifying } from "./retained-harness.js"

const retained = process.env.GPUIX_RETAIN_OFF === undefined
const CARD = 2
const FRAMES = 8
const LINEAR = { duration: 1, ease: "linear" } as const
const RED = "#ff0000"
const BLUE = "#0000ff"

defineKeyframes("frame-sources-slide", { from: { left: 0 }, to: { left: 100 } })
defineKeyframes("frame-sources-ink", { from: { color: RED }, to: { color: BLUE } })
defineKeyframes("frame-sources-wash", { from: { backgroundColor: RED }, to: { backgroundColor: BLUE } })

type Mode = "direct" | "above" | "around"
const MODES: Mode[] = ["direct", "above", "around"]

/** What a case keeps for its script and its reads. */
interface Ctl {
  value?: any
  log: string[]
  node?: any
}

interface Case {
  name: string
  extra: (phase: number, ctl: Ctl) => React.ReactNode
  /** The whole page of a case that is not an element of the card. */
  page?: (phase: number, ctl: Ctl, mode: Mode) => React.ReactElement
  /** Starts it, once the page is recorded. Renders phase 1 when it is not given. */
  begin?: (view: any, ctl: Ctl, show: (phase: number) => void) => void
  /** One frame. A step of the clock when it is not given. */
  frame?: (view: any, ctl: Ctl, index: number, show: (phase: number) => void) => void
  read: (view: any, ctl: Ctl) => string | number
  /** The `read` values it has to give, at least this many plus one (two when it is not given). */
  moves?: number
  /** Nothing is to move (see the case), only to be the same with retention on. */
  still?: boolean
  /** It ends within the frames, and after that nothing needs a build. */
  ends?: boolean
  /** An element around the card, inside the padded one: what the card inherits from. */
  above?: (card: React.ReactNode, phase: number, ctl: Ctl) => React.ReactNode
  /** The page is in a scroller that a script scrolls each frame. */
  scrolled?: boolean
  frames?: number
}

const scene = (extra: React.ReactNode, mode: Mode, above = (card: React.ReactNode) => card) => {
  const card = (
    <Card
      index={CARD}
      extra={mode === "around" ? <div style={{ display: "contents" }}>{extra}</div> : extra}
      style={{ position: "relative" }}
    />
  )
  return (
    <Page
      card={(index) =>
        index === CARD ? (
          <div key={index} testId="outer" style={{ padding: 4 }}>
            {above(mode === "above" ? <div style={{ display: "contents" }}>{card}</div> : card)}
          </div>
        ) : null
      }
    />
  )
}

const advance = (view, ms = 50) => {
  view.renderer.clockFastForward(ms)
  view.renderer.dispatchNativeEvents()
}

/** Where an element is in the card. */
const inCard = (view, testId: string) => {
  const box = view.bounds(testId)
  const card = view.bounds(`card-${CARD}`)
  return { x: box.x - card.x, y: box.y - card.y, width: box.width, height: box.height }
}

let shots = 0
/** The red channel at the middle of an element: what a colour that moves without a layout paints. */
const redOf = (view, testId: string) => {
  const box = view.bounds(testId)
  return view.sample(`frame-sources-${shots++}`, box.x + box.width / 2, box.y + box.height / 2)[0]
}

/** The reddest a pixel of the word gets: the colour its text inherits. */
const inkOf = (view) => {
  const word = view.bounds("word")
  let most = 0
  for (let dx = 4; dx < word.width; dx += 6) {
    most = Math.max(most, view.sample(`frame-sources-${shots++}`, word.x + dx, word.y + 20)[0])
  }
  return most
}

const wordOf = (style: object = {}) => (
  <text testId="word" style={{ position: "absolute", left: 0, top: 58, lineHeight: "40px", fontFamily: "Menlo", fontWeight: "bold", ...style }}>
    WWWW
  </text>
)

const box = (testId: string, style: object = {}) => (
  <div testId={testId} style={{ position: "absolute", left: 0, top: 62, width: 20, height: 10, backgroundColor: RED, ...style }} />
)

/** One run of a case: what `read` gave each frame. */
function trace(c: Case, mode: Mode, planning: boolean) {
  const ctl: Ctl = { log: [] }
  const view = setup(<div />)
  try {
    // Paused before anything is rendered, so a run starts every motion at the same instant.
    view.renderer.clockPause()
    if (!planning) view.renderer.setRetainPlanning(false)
    const page = (phase: number) =>
      c.page ? c.page(phase, ctl, mode) : scene(c.extra(phase, ctl), mode, c.above && ((card) => c.above(card, phase, ctl)))
    const show = (phase: number) =>
      view.render(
        c.scrolled ? (
          <div testId="scroller" style={{ width: 640, height: 190, overflow: "auto" }}>
            {page(phase)}
          </div>
        ) : (
          page(phase)
        )
      )
    show(0)
    view.settle()
    ;(c.begin ?? ((_, __, show) => show(1)))(view, ctl, show)
    view.settle()
    const series = [c.read(view, ctl)]
    const built: number[] = []
    for (let index = 0; index < (c.frames ?? FRAMES); index++) {
      const before = view.built()
      ;(c.frame ?? ((view) => advance(view)))(view, ctl, index, show)
      if (c.scrolled) view.renderer.scrollTo(view.idOf("scroller"), 0, (index + 1) * 8)
      built.push(view.built() - before)
      series.push(c.read(view, ctl))
    }
    const counters = view.counters()
    return { series, built, log: ctl.log, replays: counters.replays, mismatches: counters.mismatches }
  } finally {
    view.unmount()
  }
}

const cases = (list: Case[], modes: Mode[] = MODES) => {
  for (const c of list) {
    for (const mode of modes) {
      it(`${c.name} (${mode})`, () => {
        const on = trace(c, mode, true)
        const off = trace(c, mode, false)
        expect(on.mismatches, "the verifier's mismatches").toBe(0)
        if (!c.still) expect(new Set(off.series).size, `what it shows moved: ${off.series.join(" ")}`).toBeGreaterThan(c.moves ?? 2)
        expect(on.series).toEqual(off.series)
        expect(on.log).toEqual(off.log)
        if (retained) expect(on.replays, "the other cards were replayed").toBeGreaterThan(0)
        // The roots around the source are built each frame, and the ones beside it are not.
        if (retained && !verifying) {
          for (const [index, count] of on.built.entries()) {
            if (!c.still && !(c.ends && index > 3)) expect(count, `frame ${index}: built with retention on, ${off.built[index]} with it off`).toBeGreaterThan(0)
            expect(count).toBeLessThan(off.built[index])
          }
        }
      })
    }
  }
}

const leftOf = (testId: string) => (view) => inCard(view, testId).x

function Driven({ ctl, kind }) {
  const x = useMotionValue(0)
  ctl.value = x
  const spring = useSpring(x, { stiffness: 200, damping: 20 })
  const mapped = useTransform(x, [0, 1], [0, 100])
  const doubled = useTransform(x, (latest) => latest * 2)
  const left = { plain: x, spring, mapped, doubled }[kind]
  return box("driven", { left })
}

function Tinted({ ctl }) {
  const x = useMotionValue(0)
  ctl.value = x
  const color = useTransform(x, [0, 1], [RED, BLUE])
  return box("tinted", { backgroundColor: color })
}

describeNative("the card of these cases, with nothing moving in it", () => {
  for (const mode of MODES) {
    it(`is replayed, so the cases below are not passing because nothing was skipped (${mode})`, () => {
      const view = setup(scene(box("still"), mode))
      try {
        view.renderer.clockPause()
        view.settle()
        const hits = view.counters().hits
        const built = view.built()
        for (let frame = 0; frame < 6; frame++) view.renderer.flush()
        if (retained) expect(view.counters().hits - hits).toBe(6)
        if (retained && !verifying) expect(view.built() - built).toBe(0)
        view.finish(false)
      } finally {
        view.unmount()
      }
    })
  }
})

describeNative("a motion running inside a root the frame could replay", () => {
  cases([
    {
      name: "a tween of left",
      extra: () => (
        <motion.div
          testId="mover"
          style={{ position: "absolute", left: 0, top: 62, width: 20, height: 10, backgroundColor: RED }}
          initial={{ left: 0 }}
          animate={{ left: 100 }}
          transition={LINEAR}
        />
      ),
      begin: () => {},
      read: leftOf("mover"),
    },
    {
      name: "a spring of left",
      extra: () => (
        <motion.div
          testId="mover"
          style={{ position: "absolute", left: 0, top: 62, width: 20, height: 10, backgroundColor: RED }}
          initial={{ left: 0 }}
          animate={{ left: 100 }}
          transition={{ type: "spring", stiffness: 120, damping: 12 }}
        />
      ),
      begin: () => {},
      read: leftOf("mover"),
    },
    {
      name: "a tween of a colour, which moves nothing",
      extra: () => (
        <motion.div
          testId="mover"
          style={{ position: "absolute", left: 0, top: 62, width: 20, height: 10 }}
          initial={{ backgroundColor: RED }}
          animate={{ backgroundColor: BLUE }}
          transition={LINEAR}
        />
      ),
      begin: () => {},
      read: (view) => redOf(view, "mover"),
      frames: 4,
    },
    {
      name: "a motion that starts when a prop changes",
      extra: (phase) => (
        <motion.div
          testId="mover"
          style={{ position: "absolute", left: 0, top: 62, width: 20, height: 10, backgroundColor: RED }}
          animate={{ left: phase === 0 ? 0 : 100 }}
          transition={LINEAR}
        />
      ),
      read: leftOf("mover"),
    },
  ])

  const SHORT = { duration: 0.2, ease: "linear" } as const
  const mover = (props: object) => (
    <motion.div testId="mover" style={{ position: "absolute", left: 0, top: 62, width: 20, height: 10, backgroundColor: RED }} {...props} />
  )
  cases([
    {
      name: "a tween that ends, and the frames after it",
      extra: () => mover({ initial: { left: 0 }, animate: { left: 100 }, transition: SHORT }),
      begin: () => {},
      read: leftOf("mover"),
      frames: 10,
      ends: true,
    },
    {
      name: "a spring that settles, and the frames after it",
      extra: () => mover({ initial: { left: 0 }, animate: { left: 100 }, transition: { type: "spring", duration: 0.2, bounce: 0 } }),
      begin: () => {},
      read: leftOf("mover"),
      frames: 10,
      ends: true,
    },
    {
      name: "a motion that is told to go back before it arrives",
      extra: (phase) => mover({ animate: { left: phase === 0 ? 100 : 0 }, transition: LINEAR }),
      begin: (view, ctl, show) => {
        advance(view, 300)
        show(1)
      },
      read: leftOf("mover"),
    },
  ])

  cases([
    {
      name: "a motion value set from JS each frame",
      extra: (_, ctl) => <Driven ctl={ctl} kind="plain" />,
      begin: () => {},
      frame: (view, ctl, index) => {
        ctl.value.set((index + 1) * 10)
        view.renderer.flush()
      },
      read: leftOf("driven"),
    },
    {
      name: "a motion value animated natively",
      extra: (_, ctl) => <Driven ctl={ctl} kind="plain" />,
      begin: (view, ctl) => animate(ctl.value, 100, LINEAR),
      read: leftOf("driven"),
    },
    {
      name: "a spring following a motion value",
      extra: (_, ctl) => <Driven ctl={ctl} kind="spring" />,
      begin: (view, ctl) => ctl.value.set(100),
      read: leftOf("driven"),
    },
    {
      name: "a useTransform of ranges, native-linked",
      extra: (_, ctl) => <Driven ctl={ctl} kind="mapped" />,
      begin: (view, ctl) => animate(ctl.value, 1, LINEAR),
      read: leftOf("driven"),
    },
    {
      name: "a useTransform computed in JS",
      extra: (_, ctl) => <Driven ctl={ctl} kind="doubled" />,
      begin: (view, ctl) => animate(ctl.value, 50, LINEAR),
      read: leftOf("driven"),
    },
    {
      name: "a motion value mapped to a colour, which moves nothing",
      extra: (_, ctl) => <Tinted ctl={ctl} />,
      begin: (view, ctl) => animate(ctl.value, 1, LINEAR),
      read: (view) => redOf(view, "tinted"),
      frames: 4,
    },
  ])

  cases([
    {
      name: "a CSS transition of left",
      extra: (phase) => box("slid", { left: phase === 0 ? 0 : 100, transition: "left 1s linear" }),
      read: leftOf("slid"),
    },
    {
      name: "a CSS transition of a colour, which moves nothing",
      extra: (phase) => box("slid", { backgroundColor: phase === 0 ? RED : BLUE, transition: "background-color 1s linear" }),
      read: (view) => redOf(view, "slid"),
      frames: 4,
    },
    {
      name: "a CSS keyframe animation of left, forever",
      extra: () => box("slid", { animation: "frame-sources-slide 1s linear infinite" }),
      begin: () => {},
      frames: 12,
      read: leftOf("slid"),
    },
    {
      name: "a CSS keyframe animation that ends, and the frames after it",
      extra: () => box("slid", { animation: "frame-sources-slide 400ms linear" }),
      begin: () => {},
      frames: 12,
      read: leftOf("slid"),
    },
    {
      name: "a CSS keyframe animation of a colour, which moves nothing",
      extra: () => box("slid", { animation: "frame-sources-wash 1s linear" }),
      begin: () => {},
      read: (view) => redOf(view, "slid"),
      frames: 4,
    },
  ])

  const hidden = (view) => {
    const box = view.renderer.getElementBounds(view.idOf("driven"))
    return box ? box.x - view.bounds(`card-${CARD}`).x : "hidden"
  }
  cases([
    {
      name: "a motion value that ran while its element was display: none, then shown",
      extra: (phase, ctl) => (
        <div style={{ display: phase === 1 ? "none" : "block" }}>
          <Driven ctl={ctl} kind="plain" />
        </div>
      ),
      begin: (view, ctl, show) => {
        animate(ctl.value, 100, LINEAR)
        show(1)
      },
      frame: (view, ctl, index, show) => {
        if (index === 4) show(0)
        advance(view, 100)
      },
      frames: 8,
      read: hidden,
    },
    {
      name: "a motion that ran while its element was display: none, then shown",
      extra: (phase) => (
        <div style={{ display: phase === 1 ? "none" : "block" }}>
          <motion.div
            testId="driven"
            style={{ position: "absolute", left: 0, top: 62, width: 20, height: 10, backgroundColor: RED }}
            initial={{ left: 0 }}
            animate={{ left: 100 }}
            transition={LINEAR}
          />
        </div>
      ),
      begin: (view, ctl, show) => show(1),
      frame: (view, ctl, index, show) => {
        if (index === 4) show(0)
        advance(view, 100)
      },
      frames: 8,
      read: hidden,
    },
  ])

  cases([
    {
      name: "a layout animation of a reordered list (excluded: its roots are built)",
      extra: (phase) => (
        <div style={{ position: "absolute", left: 0, top: 62, display: "flex", flexDirection: "column" }}>
          {(phase === 0 ? ["a", "b"] : ["b", "a"]).map((name) => (
            <motion.div key={name} layout transition={LINEAR} testId={name} style={{ height: 10, width: 50, backgroundColor: RED }} />
          ))}
        </div>
      ),
      read: (view) => `${inCard(view, "a").y} ${inCard(view, "b").y}`,
      moves: 1,
    },
  ])

  cases([
    {
      name: "an exit animation of a removed child",
      extra: (phase) => (
        <AnimatePresence>
          {phase === 0 ? (
            <motion.div
              key="leaving"
              testId="leaving"
              initial={false}
              animate={{ left: 0 }}
              exit={{ left: 100 }}
              transition={LINEAR}
              style={{ position: "absolute", left: 0, top: 62, width: 20, height: 10, backgroundColor: RED }}
            />
          ) : null}
        </AnimatePresence>
      ),
      read: (view) => view.renderer.findByTestId("leaving") ? inCard(view, "leaving").x : "gone",
    },
  ])

  cases([
    {
      name: "a motion value that a scroller drives from its scroll offset",
      extra: (_, ctl) => <Scrolled ctl={ctl} />,
      begin: () => {},
      frame: (view, ctl, index) => {
        ctl.node.scrollTop = (index + 1) * 5
        advance(view, 16)
        view.renderer.flush()
      },
      read: (view) => `${inCard(view, "inner").y} ${inCard(view, "follower").width}`,
    },
  ])
})

function Scrolled({ ctl }) {
  const follow = useMotionValue(0)
  return (
    <>
      <div
        ref={(node) => {
          ctl.node = node
        }}
        testId="scroller"
        onScroll={(event) => follow.set(event.scrollTop)}
        style={{ position: "absolute", left: 0, top: 30, width: 60, height: 30, overflow: "scroll" }}
      >
        <div testId="inner" style={{ width: 40, height: 300, backgroundColor: RED }}>
          <div style={{ height: 10 }} />
        </div>
      </div>
      {box("follower", { top: 62, width: follow })}
    </>
  )
}

/** The ResizeObserver, `onLayout` and IntersectionObserver reports of an element a sibling squeezes. */
function Watched({ ctl }) {
  const watched = React.useRef(null)
  const far = React.useRef(null)
  React.useEffect(() => {
    const resize = new ResizeObserver((entries) => entries.forEach((item) => ctl.log.push(`resize ${Math.round(item.contentRect.width)}`)))
    const intersect = new IntersectionObserver(
      (entries) => entries.forEach((item) => ctl.log.push(`intersect ${Math.round(item.intersectionRatio * 100)}`)),
      { threshold: [0, 0.25, 0.5, 0.75, 1] }
    )
    resize.observe(watched.current)
    intersect.observe(far.current)
    return () => {
      resize.disconnect()
      intersect.disconnect()
    }
  }, [])
  return (
    <>
      <div ref={watched} testId="watched" onLayout={(event) => ctl.log.push(`layout ${Math.round(event.width)}`)} style={{ width: "100%", height: 10, backgroundColor: BLUE }} />
      <div ref={far} testId="far" style={{ marginLeft: 80, width: 20, height: 10, backgroundColor: RED }} />
    </>
  )
}

const squeezer = (testId: string) => (
  <motion.div
    testId={testId}
    initial={{ width: 0 }}
    animate={{ width: 60 }}
    transition={LINEAR}
    style={{ flexShrink: 0, height: 10, backgroundColor: RED }}
  />
)

const watchedRead = (view, ctl) => `${Math.round(view.bounds("watched").width)} ${ctl.log.join(",")}`

function InkValue({ ctl, children }) {
  const x = useMotionValue(0)
  ctl.value = x
  const color = useTransform(x, [0, 1], [RED, BLUE])
  return <div style={{ color }}>{children}</div>
}

describeNative("a root that inherits from an element above it that moves", () => {
  const ancestor = (props: object, style: object = {}) => (card: React.ReactNode) => (
    <motion.div style={style} {...props}>
      {card}
    </motion.div>
  )
  const wide = (view) => view.bounds("word").width
  cases([
    {
      name: "a colour a motion above it animates",
      extra: () => wordOf({ fontSize: 40 }),
      above: ancestor({ initial: { color: RED }, animate: { color: BLUE }, transition: LINEAR }, { color: RED }),
      begin: () => {},
      read: inkOf,
      frames: 4,
    },
    {
      name: "a font size a motion above it animates, which moves the width of its text",
      extra: () => wordOf(),
      above: ancestor({ initial: { fontSize: 20 }, animate: { fontSize: 40 }, transition: LINEAR }, { fontSize: 20 }),
      begin: () => {},
      read: wide,
    },
    {
      name: "a colour a CSS transition above it animates",
      extra: () => wordOf({ fontSize: 40 }),
      above: (card, phase) => <div style={{ color: phase === 0 ? RED : BLUE, transition: "color 1s linear" }}>{card}</div>,
      read: inkOf,
      frames: 4,
    },
    {
      name: "a colour a keyframe animation above it animates",
      extra: () => wordOf({ fontSize: 40 }),
      above: (card) => <div style={{ animation: "frame-sources-ink 1s linear" }}>{card}</div>,
      begin: () => {},
      read: inkOf,
      frames: 4,
    },
    {
      name: "a font size a CSS transition above it animates",
      extra: () => wordOf(),
      above: (card, phase) => <div style={{ fontSize: phase === 0 ? 20 : 40, transition: "font-size 1s linear" }}>{card}</div>,
      read: wide,
    },
    {
      name: "a colour a motion value above it animates",
      extra: () => wordOf({ fontSize: 40 }),
      above: (card, phase, ctl) => <InkValue ctl={ctl}>{card}</InkValue>,
      begin: (view, ctl) => animate(ctl.value, 1, LINEAR),
      read: inkOf,
      frames: 4,
    },
    {
      name: "an opacity a motion above it animates",
      extra: () => box("faded"),
      above: ancestor({ initial: { opacity: 1 }, animate: { opacity: 0.2 }, transition: LINEAR }),
      begin: () => {},
      read: (view) => redOf(view, "faded"),
      frames: 4,
    },
  ])
})

describeNative("a motion running in a root of a page that a script scrolls each frame", () => {
  cases(
    [
      {
        name: "a tween of left",
        extra: () => (
          <motion.div
            testId="mover"
            style={{ position: "absolute", left: 0, top: 62, width: 20, height: 10, backgroundColor: RED }}
            initial={{ left: 0 }}
            animate={{ left: 100 }}
            transition={LINEAR}
          />
        ),
        begin: () => {},
        read: leftOf("mover"),
      },
      {
        name: "a motion value set from JS",
        extra: (_, ctl) => <Driven ctl={ctl} kind="plain" />,
        begin: () => {},
        frame: (view, ctl, index) => {
          ctl.value.set((index + 1) * 10)
          view.renderer.flush()
        },
        read: leftOf("driven"),
      },
      {
        name: "a CSS keyframe animation of a colour",
        extra: () => box("slid", { animation: "frame-sources-wash 1s linear" }),
        begin: () => {},
        read: (view) => redOf(view, "slid"),
        frames: 4,
      },
      {
        name: "a motion.span that takes space along its line",
        extra: () =>
          paragraph(
            <motion.span testId="s" initial={{ paddingLeft: 0 }} animate={{ paddingLeft: 40 }} transition={LINEAR}>
              bbbb
            </motion.span>
          ),
        begin: () => {},
        read: (view) => view.bounds("n").x - view.bounds("p").x,
      },
    ].map((c) => ({ ...c, scrolled: true }))
  )
})

describeNative("observers inside a root whose box a sibling animates", () => {
  cases([
    {
      name: "a sibling in the same card",
      extra: (_, ctl) => (
        <div style={{ position: "absolute", left: 0, top: 50, display: "flex", width: 140, height: 10, overflow: "hidden" }}>
          {squeezer("squeezer")}
          <div style={{ flexGrow: 1, minWidth: 0 }}>
            <Watched ctl={ctl} />
          </div>
        </div>
      ),
      begin: () => {},
      read: watchedRead,
    },
    {
      name: "a sibling of the root, which resizes the root without a prop change",
      page: (_, ctl, mode) => {
        const grow = (
          <div testId="grow" style={{ flexGrow: 1, minWidth: 0, overflow: "hidden" }}>
            <Watched ctl={ctl} />
          </div>
        )
        const row = (
          <div key={CARD} testId="row" style={{ display: "flex", width: 140, height: 30 }}>
            {squeezer("squeezer")}
            {mode === "around" ? <div style={{ display: "contents" }}>{grow}</div> : grow}
          </div>
        )
        return <Page card={(index) => (index === CARD ? (mode === "above" ? <div key={CARD} style={{ display: "contents" }}>{row}</div> : row) : null)} />
      },
      begin: () => {},
      read: watchedRead,
    },
  ])
})

/** A ring on `rect`, in the page or in the card, whose own origin is `rect.cardX`, `rect.cardY`. */
function Ring({ ringIn, rect }) {
  return (
    <div
      testId="ring"
      style={{
        position: "absolute",
        left: rect.x - (ringIn === "card" ? rect.cardX : 0),
        top: rect.y - (ringIn === "card" ? rect.cardY : 0),
        width: rect.width,
        height: rect.height,
        borderWidth: 1,
        borderStyle: "solid",
        borderColor: BLUE,
      }}
    >
      <div style={{ width: 2, height: 2 }} />
    </div>
  )
}

/** Frames of a row that moves, with a ring placed on it from its getBoundingClientRect after each. */
function followRow(ringIn: "page" | "card", mode: Mode, planning: boolean, between: boolean) {
  const rowRef = React.createRef()
  const row = (
    <motion.div
      ref={rowRef}
      testId="row"
      initial={{ left: 0 }}
      animate={{ left: 100 }}
      transition={LINEAR}
      style={{ position: "absolute", left: 0, top: 62, width: 20, height: 10, backgroundColor: RED }}
    />
  )
  const page = (rect) => {
    const ring = rect && <Ring ringIn={ringIn} rect={rect} />
    if (ringIn === "card") return scene(<>{row}{ring}</>, mode)
    return (
      <div style={{ position: "relative" }}>
        {scene(row, mode)}
        {ring}
      </div>
    )
  }
  const view = setup(<div />)
  try {
    view.renderer.clockPause()
    if (!planning) view.renderer.setRetainPlanning(false)
    view.render(page(null))
    view.settle()
    const series = []
    // A live window is drawn by the display link, so a read between two frames is a pass of its own.
    if (between) view.renderer.setInlineDraws(false)
    for (let index = 0; index < FRAMES; index++) {
      view.renderer.clockFastForward(50)
      const target = rowRef.current.getBoundingClientRect()
      const card = view.bounds(`card-${CARD}`)
      view.render(page({ x: target.x, y: target.y, width: target.width, height: target.height, cardX: card.x, cardY: card.y }))
      if (between) view.renderer.drawFrame()
      view.renderer.dispatchNativeEvents()
      const ring = view.bounds("ring")
      const moved = view.bounds("row")
      expect([ring.x, ring.y, ring.width, ring.height], `the ring is on the row at frame ${index}`).toEqual([moved.x, moved.y, moved.width, moved.height])
      series.push(moved.x)
    }
    if (between) view.renderer.setInlineDraws(true)
    expect(view.counters().mismatches).toBe(0)
    return series
  } finally {
    view.unmount()
  }
}

describeNative("an overlay placed from getBoundingClientRect after each frame", () => {
  for (const ringIn of ["page", "card"] as const) {
    for (const mode of MODES) {
      for (const between of [false, true]) {
        it(`follows an animating row (ring in the ${ringIn}, ${mode}${between ? ", read between two frames" : ""})`, () => {
          const on = followRow(ringIn, mode, true, between)
          const off = followRow(ringIn, mode, false, between)
          expect(new Set(off).size).toBeGreaterThan(2)
          expect(on).toEqual(off)
        })
      }
    }
  }
})

const TEXT = { fontSize: 16, lineHeight: "20px", fontFamily: "Menlo", color: "#000000" }
const paragraph = (span: React.ReactNode) => (
  <text testId="p" style={{ ...TEXT, position: "absolute", left: 0, top: 58, width: 130 }}>
    aaaa{span}
    <span testId="n">cccc</span>
  </text>
)
const spanBox = (view) => view.bounds("s")

describeNative("an inline span and an svg group running inside a root", () => {
  cases([
    {
      name: "a motion.span that takes space along its line",
      extra: () =>
        paragraph(
          <motion.span testId="s" initial={{ paddingLeft: 0 }} animate={{ paddingLeft: 40 }} transition={LINEAR}>
            bbbb
          </motion.span>
        ),
      begin: () => {},
      read: (view) => view.bounds("n").x - view.bounds("p").x,
    },
    {
      name: "a motion.span whose colour moves nothing",
      extra: () =>
        paragraph(
          <motion.span testId="s" initial={{ backgroundColor: RED }} animate={{ backgroundColor: BLUE }} transition={LINEAR}>
            bbbb
          </motion.span>
        ),
      begin: () => {},
      read: (view) => {
        const span = spanBox(view)
        return view.sample(`frame-sources-${shots++}`, span.x + 2, span.y + 2)[0]
      },
      frames: 4,
    },
    {
      name: "a keyframe animation on a span",
      extra: () => paragraph(<span testId="s" style={{ animation: "frame-sources-wash 1s linear" }}>bbbb</span>),
      begin: () => {},
      read: (view) => {
        const span = spanBox(view)
        return view.sample(`frame-sources-${shots++}`, span.x + 2, span.y + 2)[0]
      },
      frames: 4,
    },
    {
      name: "a CSS transition on a span",
      extra: (phase) =>
        paragraph(
          <span testId="s" style={{ backgroundColor: phase === 0 ? RED : BLUE, transition: "background-color 1s linear" }}>
            bbbb
          </span>
        ),
      read: (view) => {
        const span = spanBox(view)
        return view.sample(`frame-sources-${shots++}`, span.x + 2, span.y + 2)[0]
      },
      frames: 4,
    },
    {
      name: "a motion.g in an svg",
      extra: () => (
        <svg testId="icon" viewBox="0 0 10 10" style={{ position: "absolute", left: 0, top: 60, width: 30, height: 20 }}>
          <motion.g initial={{ fill: RED }} animate={{ fill: BLUE }} transition={LINEAR}>
            <rect x={0} y={0} width={10} height={10} />
          </motion.g>
        </svg>
      ),
      begin: () => {},
      read: (view) => redOf(view, "icon"),
      frames: 4,
    },
    {
      name: "a motion.g whose opacity moves, with a plain rect in it",
      extra: () => (
        <svg testId="icon" viewBox="0 0 10 10" style={{ position: "absolute", left: 0, top: 60, width: 30, height: 20 }}>
          <g>
            <motion.g initial={{ opacity: 1 }} animate={{ opacity: 0.2 }} transition={LINEAR}>
              <rect x={0} y={0} width={10} height={10} fill={RED} />
            </motion.g>
          </g>
        </svg>
      ),
      begin: () => {},
      read: (view) => {
        const icon = view.bounds("icon")
        return view.sample(`frame-sources-${shots++}`, icon.x + icon.width / 2, icon.y + icon.height / 2)[1]
      },
      frames: 4,
    },
  ])

  // gpuix does not apply a motion, a transition or an animation of the display: contents element to what
  // it hands down (with retention off too), so there is nothing to see move: the frames have to be the
  // same as the ones a build gives.
  cases(
    [
      {
        name: "a motion on the display: contents element itself, which hands its colour down",
        extra: () => (
          <motion.div style={{ display: "contents", color: RED }} initial={{ color: RED }} animate={{ color: BLUE }} transition={LINEAR}>
            {wordOf({ fontSize: 40 })}
          </motion.div>
        ),
        begin: () => {},
        read: inkOf,
        frames: 4,
        still: true,
      },
      {
        name: "a CSS transition on the display: contents element itself",
        extra: (phase) => (
          <div style={{ display: "contents", color: phase === 0 ? RED : BLUE, transition: "color 1s linear" }}>{wordOf({ fontSize: 40 })}</div>
        ),
        read: inkOf,
        frames: 4,
        still: true,
      },
      {
        name: "a keyframe animation on the display: contents element itself",
        extra: () => <div style={{ display: "contents", animation: "frame-sources-ink 1s linear" }}>{wordOf({ fontSize: 40 })}</div>,
        begin: () => {},
        read: inkOf,
        frames: 4,
        still: true,
      },
    ],
    MODES
  )
})


// The clock of every case above is paused, so the two passes of a verified frame read one instant. A
// live clock is read by each pass on its own, and whatever animates is further along in the pass that
// the replay is compared with, so a root below it sits somewhere else in it.
describeNative("a root below a sibling whose height animates, on a clock that is running", () => {
  it("is moved by layout each frame, and a verified frame finds it where a build of the same instant puts it", () => {
    const view = setup(
      <div>
        <motion.div
          testId="grower"
          initial={{ height: 0 }}
          animate={{ height: 400 }}
          transition={{ duration: 2, ease: "linear" }}
          style={{ width: 140, backgroundColor: RED }}
        />
        <Page />
      </div>
    )
    try {
      view.settle()
      const tops = new Set<number>()
      for (let frame = 0; frame < 12; frame++) {
        view.renderer.flush()
        tops.add(view.bounds("card-0").y)
      }
      expect(tops.size, "the root was moved by layout").toBeGreaterThan(3)
      view.finish()
    } finally {
      view.unmount()
    }
  })
})

// A root keeps its snapped box while the unsnapped origin layout puts it at settles at another
// fraction of a device pixel, and the boxes inside snap edge by edge from that origin: the label here
// is 43.4 wide (a percentage, since a length is rounded before layout), 43 where the card begins at
// 10 and 44 where it begins at 10.4. A replay that only knew the snapped box of the card kept the 43.
describeNative("a root whose sibling settles at a fraction of a pixel, with a box inside that snaps with its origin", () => {
  const ROW = 640
  const percent = (width: number) => `${(width / ROW) * 100}%`
  const row = (leading: React.ReactNode) => (
    <div testId="phase-row" style={{ display: "flex", width: ROW }}>
      {leading}
      <Page
        style={{ flex: "none", width: 500 }}
        card={(index) =>
          index === 0 ? (
            <Card
              key={index}
              index={index}
              extra={<div testId="phase-label" style={{ width: "31%", height: 10, backgroundColor: RED }} />}
            />
          ) : null
        }
      />
    </div>
  )
  const lead = (width: number) => row(<div testId="phase-lead" style={{ flex: "none", width: percent(width), height: 20 }} />)

  /** The width of the label in the frame, and in a build of the same state. A screenshot is a draw of its
   *  own, which would be the build: the boxes are read first. */
  const widths = (view) => {
    const replayed = view.bounds("phase-label").width
    view.renderer.setRetainPlanning(false)
    view.renderer.flush()
    const built = view.bounds("phase-label").width
    view.renderer.setRetainPlanning(true)
    view.renderer.flush()
    return { replayed, built }
  }

  it.each([10.2, 10.4, 10.6, 10.8])("is built where the sibling settled at %d", (width) => {
    const view = setup(lead(10))
    try {
      view.settle()
      view.render(lead(width))
      view.settle()
      const { replayed, built } = widths(view)
      expect(replayed).toBe(built)
      view.expectWhatABuildShows("phase-settled", ["phase-label"])
      view.finish()
    } finally {
      view.unmount()
    }
  })

  it("is built where an animating sibling comes to rest", () => {
    const sibling = (width: number, duration: number) => (
      <motion.div
        testId="phase-lead"
        initial={{ width: percent(10) }}
        animate={{ width: percent(width) }}
        transition={{ duration, ease: "linear" }}
        style={{ flex: "none", height: 20 }}
      />
    )
    const view = setup(row(sibling(10, 0.01)))
    try {
      view.settle()
      view.render(row(sibling(10.4, 0.3)))
      for (let frame = 0; frame < 12; frame++) {
        advance(view, 50)
        view.renderer.flush()
      }
      view.settle()
      const { replayed, built } = widths(view)
      expect(replayed).toBe(built)
      view.expectWhatABuildShows("phase-animated", ["phase-label"])
      view.finish()
    } finally {
      view.unmount()
    }
  })
})
