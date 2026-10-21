/**
 * Motions below a `display: contents` wrapper inside a retained root, in the shape of the Fluid Functionalism
 * demo's cull wrapper: a padded div an IntersectionObserver watches, a `display: contents` div, a context
 * provider, then the demo. The wrapper is never built, so the roots above a motion that runs under it have to be
 * built each frame anyway, or they are replayed with the motion frozen. Run with GPUIX_RETAIN_VERIFY=1 and
 * GPUIX_RETAIN_OFF=1 too.
 */
import {
  type Accessor,
  type JSX,
  createContext,
  createEffect,
  createRenderEffect,
  createSignal,
  onCleanup,
  Show,
  useContext,
} from "solid-js"
import { describe, expect, it } from "bun:test"
import { IntersectionObserver, ResizeObserver, createMotionValue, motion } from "../index.js"
import type { HostElement } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { TestRenderer } from "../testing.js"

const retained = process.env.GPUIX_RETAIN_OFF === undefined
const verifying = process.env.GPUIX_RETAIN_VERIFY !== undefined

/** FF's `tween.moderate`, and a slower one so a spring spans several frames. */
const MODERATE = { type: "spring", duration: 0.16, bounce: 0 } as const
const SLOW = { type: "spring", duration: 0.4, bounce: 0 } as const
const LINEAR = { duration: 1, ease: "linear" } as const

const CARD = 2
const WINDOW = { width: 640, height: 480 }

const PageScrollerContext = createContext<Accessor<HostElement | undefined>>()
const OnScreenContext = createContext<Accessor<boolean>>(() => true)

/** `DemoCard`: culling decides from an IntersectionObserver on the padded div whether the demo is shown. */
function Card(props: { index: number; children?: JSX.Element }) {
  const scroller = useContext(PageScrollerContext)
  if (scroller === undefined) throw new Error("A card must be inside the page scroller")
  const [observed, setObserved] = createSignal<HostElement>()
  const [far, setFar] = createSignal(false)

  createEffect(() => {
    const root = scroller()
    const target = observed()
    if (root === undefined || target === undefined) return
    const observer = new IntersectionObserver(
      (entries) => setFar(!entries[entries.length - 1].isIntersecting),
      { root, rootMargin: "25% 0px" },
    )
    observer.observe(target)
    onCleanup(() => observer.disconnect())
  })

  return (
    <div testId={`card-${props.index}`} style={{ display: "flex", flexDirection: "column", width: 200, backgroundColor: "#335577" }}>
      <div ref={setObserved} testId={`body-${props.index}`} style={{ padding: 20 }}>
        <div style={{ display: far() ? "none" : "contents" }}>
          <OnScreenContext.Provider value={() => !far()}>{props.children}</OnScreenContext.Provider>
        </div>
      </div>
      <div style={{ height: 24, backgroundColor: "#223344" }}>
        <text>{`card ${props.index}`}</text>
      </div>
    </div>
  )
}

function Page(props: { demo?: () => JSX.Element }) {
  const [scroller, setScroller] = createSignal<HostElement>()
  return (
    <PageScrollerContext.Provider value={scroller}>
      <div
        ref={setScroller}
        testId="page"
        style={{ ...WINDOW, overflow: "auto", display: "flex", flexDirection: "row", flexWrap: "wrap", alignContent: "flex-start", gap: 8 }}
      >
        {Array.from({ length: 6 }, (_, index) => (
          <Card index={index}>{index === CARD ? props.demo?.() : undefined}</Card>
        ))}
      </div>
    </PageScrollerContext.Provider>
  )
}

/** What a demo puts its absolute boxes in. */
function Stage(props: { height: number; children: JSX.Element }) {
  return <div style={{ position: "relative", width: 160, height: props.height }}>{props.children}</div>
}

function Mover(props: { to: Accessor<number> }) {
  return (
    <Stage height={10}>
      <motion.div
        testId="mover"
        initial={{ left: 0 }}
        animate={{ left: props.to() }}
        transition={LINEAR}
        style={{ position: "absolute", top: 0, width: 20, height: 10, backgroundColor: "#ff0000" }}
      />
    </Stage>
  )
}

const RING_OUTSET = 2

/** An accordion's panel opens and the container resizes; the ring springs to the row below it, measured on each resize. */
function Accordion(props: { open: Accessor<boolean> }) {
  let container!: HostElement
  let row!: HostElement
  const [rect, setRect] = createSignal<{ left: number; top: number; width: number; height: number }>()
  const observer = new ResizeObserver(() => {
    const box = container.getBoundingClientRect()
    const bounds = row.getBoundingClientRect()
    setRect({ left: bounds.x - box.x, top: bounds.y - box.y, width: bounds.width, height: bounds.height })
  })
  onCleanup(() => observer.disconnect())

  return (
    <div
      ref={(host) => {
        container = host
        observer.observe(host, { box: "border-box" })
      }}
      testId="accordion"
      style={{ position: "relative", width: 160 }}
    >
      <div style={{ height: 30, backgroundColor: "#555555" }} />
      <motion.div
        initial={false}
        animate={{ height: props.open() ? 60 : 0 }}
        transition={SLOW}
        style={{ overflow: "hidden", backgroundColor: "#666666" }}
      />
      <div ref={row} testId="row" style={{ height: 30, backgroundColor: "#777777" }} />
      <Show when={rect()}>
        {(current: Accessor<{ left: number; top: number; width: number; height: number }>) => (
          <motion.div
            testId="ring"
            initial={false}
            animate={{
              left: current().left - RING_OUTSET,
              top: current().top - RING_OUTSET,
              width: current().width + RING_OUTSET * 2,
              height: current().height + RING_OUTSET * 2,
            }}
            transition={MODERATE}
            style={{
              position: "absolute",
              zIndex: 20,
              borderStyle: "solid",
              borderWidth: 1,
              borderColor: "#0000ff",
              pointerEvents: "none",
            }}
          />
        )}
      </Show>
    </div>
  )
}

const TRACK_WIDTH = 160
const HANDLE = 20

/** The slider's handle rides one motion value: a press springs it to the pointer, a drag places it. */
function Slider() {
  let track!: HostElement
  const [value, setValue] = createSignal(0)
  const [down, setDown] = createSignal(false)
  const [moving, setMoving] = createSignal(false)
  const left = createMotionValue(0)
  let placed = false
  createRenderEffect(() => {
    const target = value()
    if (!placed) left.jump(target)
    else if (moving()) left.set(target)
    else left.animate(target, MODERATE)
    placed = true
  })
  const place = (clientX: number) => {
    const at = clientX - track.getBoundingClientRect().x - HANDLE / 2
    setValue(Math.min(Math.max(at, 0), TRACK_WIDTH - HANDLE))
  }

  return (
    <div
      ref={track}
      testId="track"
      style={{ position: "relative", width: TRACK_WIDTH, height: HANDLE, backgroundColor: "#444444" }}
      onPointerDown={(event) => {
        track.setPointerCapture(event.pointerId)
        setDown(true)
        place(event.clientX)
      }}
      onPointerMove={(event) => {
        if (!down()) return
        setMoving(true)
        place(event.clientX)
      }}
      onPointerUp={() => {
        setDown(false)
        setMoving(false)
      }}
    >
      <motion.div testId="handle" style={{ position: "absolute", top: 0, left, width: HANDLE, height: HANDLE, backgroundColor: "#ffffff" }} />
    </div>
  )
}

const THUMB = 16
const SWITCH_TRAVEL = 34 - THUMB - 4

/** The thumb springs along the track and squashes while it is pressed. */
function Switch() {
  const [checked, setChecked] = createSignal(false)
  const [pressed, setPressed] = createSignal(false)
  return (
    <div
      testId="switch-track"
      style={{ position: "relative", width: 34, height: 20, borderRadius: 10, backgroundColor: "#999999" }}
      onMouseDown={() => setPressed(true)}
      onMouseUp={() => setPressed(false)}
      onClick={() => setChecked(!checked())}
    >
      <motion.div
        testId="switch-thumb"
        initial={false}
        animate={{
          left: 2 + (checked() ? SWITCH_TRAVEL - (pressed() ? 4 : 0) : 0),
          top: pressed() ? 4 : 2,
          width: pressed() ? THUMB + 4 : THUMB,
          height: pressed() ? THUMB - 4 : THUMB,
        }}
        transition={MODERATE}
        style={{ position: "absolute", backgroundColor: "#ffffff", borderRadius: 9999 }}
      />
    </div>
  )
}

function frame(renderer: TestRenderer, ms: number): void {
  renderer.clockFastForward(ms)
  renderer.flush()
  renderer.dispatchNativeEvents()
}

/** Frames until the roots are recorded, so the next ones replay them. */
function settle(renderer: TestRenderer): void {
  for (let step = 0; step < 5; step++) frame(renderer, 0)
}

/**
 * The ring springs to the box the observer measured in the last frame. Solid sends the change in a microtask, which a
 * synchronous test never reaches, so it is sent here, and the ring springs for a frame of its own.
 */
function followResize(app: Mounted): void {
  app.flushSync(() => {})
  frame(app.renderer, 1000)
}

function boundsOf(renderer: TestRenderer, testId: string) {
  const element = renderer.findByTestId(testId)
  if (element === undefined) throw new Error(`no element has the testId ${testId}`)
  return renderer.getElementBounds(element.id)!
}

function centerOf(renderer: TestRenderer, testId: string): [number, number] {
  const box = boundsOf(renderer, testId)
  return [box.x + box.width / 2, box.y + box.height / 2]
}

/** The bounds over `steps` frames of `ms` each, the first being the bounds before any. */
function trace(renderer: TestRenderer, testId: string, steps: number, ms: number, read: (box: ReturnType<typeof boundsOf>) => number): number[] {
  const values = [read(boundsOf(renderer, testId))]
  for (let step = 0; step < steps; step++) {
    frame(renderer, ms)
    values.push(read(boundsOf(renderer, testId)))
  }
  return values
}

function expectIncreasing(values: number[]): void {
  for (let step = 1; step < values.length; step++) expect(values[step]).toBeGreaterThan(values[step - 1])
}

const IDLE_FRAMES = 6

/** What `IDLE_FRAMES` frames with nothing changing built and replayed. */
function idleFrames(renderer: TestRenderer) {
  const built = renderer.getBuildCounters().elementsBuilt
  const hits = renderer.getRetainedCounters().hits
  for (let step = 0; step < IDLE_FRAMES; step++) frame(renderer, 16)
  return {
    built: renderer.getBuildCounters().elementsBuilt - built,
    hits: renderer.getRetainedCounters().hits - hits,
  }
}

/** Frames after everything settled: the cards are replayed, and nothing is built that the page did not build before the motion. The page scroller is built each frame, so what it costs is measured, not assumed. */
function expectIdle(app: Mounted): void {
  const idle = idleFrames(app.renderer)
  if (!retained) return
  expect(idle.hits).toBeGreaterThanOrEqual(IDLE_FRAMES)
  // A run that verifies builds every replayed frame once more.
  if (!verifying) expect(idle.built).toBe(app.idleBuilt)
}

type Mounted = ReturnType<typeof mount>

function mount(demo: () => JSX.Element) {
  const app = createTestRoot(WINDOW)
  app.renderer.clockPause()
  app.render(() => <Page demo={demo} />)
  frame(app.renderer, 0)
  settle(app.renderer)
  return { ...app, idleBuilt: idleFrames(app.renderer).built }
}

function finish(app: Mounted): void {
  const counters = app.renderer.getRetainedCounters()
  expect(counters.mismatches, app.renderer.getRetentionMismatches().join("\n")).toBe(0)
  if (retained) expect(counters.replays).toBeGreaterThan(0)
}

describe.skipIf(!hasNativeTestRenderer)("Solid motion below a cull wrapper in a retained root", () => {
  it("moves an absolute box across frames and settles it at the target", () => {
    const [to, setTo] = createSignal(0)
    const app = mount(() => <Mover to={to} />)
    try {
      const rest = boundsOf(app.renderer, "mover").x
      app.flushSync(() => setTo(100))
      const xs = trace(app.renderer, "mover", 3, 250, (box) => box.x)
      expectIncreasing(xs)
      expect(xs[xs.length - 1]).toBeLessThan(rest + 100)
      frame(app.renderer, 1500)
      settle(app.renderer)
      expect(boundsOf(app.renderer, "mover").x).toBe(rest + 100)
      expectIdle(app)
      finish(app)
    } finally {
      app.unmount()
    }
  })

  it("moves the ring along with the row an accordion pushes down, and settles it on the row", () => {
    const [open, setOpen] = createSignal(false)
    const app = mount(() => <Accordion open={open} />)
    try {
      // The first resize report measured the row.
      const closed = boundsOf(app.renderer, "row")
      expect(boundsOf(app.renderer, "ring").y).toBeCloseTo(closed.y - RING_OUTSET, 1)

      app.flushSync(() => setOpen(true))
      const rows = trace(app.renderer, "row", 4, 50, (box) => box.y)
      expectIncreasing(rows)
      const rings: number[] = []
      app.flushSync(() => setOpen(false))
      app.flushSync(() => setOpen(true))
      for (let step = 0; step < 4; step++) {
        frame(app.renderer, 50)
        rings.push(boundsOf(app.renderer, "ring").y)
      }
      expect(rings[rings.length - 1]).toBeGreaterThan(rings[0])

      frame(app.renderer, 2000)
      followResize(app)
      settle(app.renderer)
      const row = boundsOf(app.renderer, "row")
      const ring = boundsOf(app.renderer, "ring")
      expect(row.y).toBe(closed.y + 60)
      expect(ring.y).toBeCloseTo(row.y - RING_OUTSET, 1)
      expect(ring.x).toBeCloseTo(row.x - RING_OUTSET, 1)
      expect(ring.height).toBeCloseTo(row.height + RING_OUTSET * 2, 1)
      expectIdle(app)
      finish(app)
    } finally {
      app.unmount()
    }
  })

  it("moves the slider handle to a press and along a drag", () => {
    const app = mount(() => <Slider />)
    try {
      const track = boundsOf(app.renderer, "track")
      const handleLeft = () => boundsOf(app.renderer, "handle").x - track.x

      expect(handleLeft()).toBe(0)
      app.renderer.nativeSimulateMouseDown(track.x + 60, track.y + 10)
      const press = trace(app.renderer, "handle", 2, 20, (box) => box.x - track.x)
      expectIncreasing(press)
      expect(press[press.length - 1]).toBeLessThan(50)
      frame(app.renderer, 1000)
      expect(handleLeft()).toBeCloseTo(50, 1)

      app.renderer.nativeSimulateMouseMove(track.x + 100, track.y + 10)
      frame(app.renderer, 1000)
      const first = handleLeft()
      expect(first).toBeCloseTo(90, 1)
      app.renderer.nativeSimulateMouseMove(track.x + 140, track.y + 10)
      frame(app.renderer, 16)
      const second = handleLeft()
      expect(second).toBeGreaterThan(first)
      app.renderer.nativeSimulateMouseMove(track.x + 40, track.y + 10)
      frame(app.renderer, 16)
      expect(handleLeft()).toBeLessThan(second)

      app.renderer.nativeSimulateMouseUp(track.x + 40, track.y + 10)
      frame(app.renderer, 1000)
      settle(app.renderer)
      expect(handleLeft()).toBeCloseTo(30, 1)
      expectIdle(app)
      finish(app)
    } finally {
      app.unmount()
    }
  })

  it("springs the switch thumb along on a click, squashes it while pressed and settles it", () => {
    const app = mount(() => <Switch />)
    try {
      const track = boundsOf(app.renderer, "switch-track")
      const thumbLeft = () => boundsOf(app.renderer, "switch-thumb").x - track.x
      expect(thumbLeft()).toBe(2)

      app.renderer.nativeSimulateClick(...centerOf(app.renderer, "switch-track"))
      const travel = trace(app.renderer, "switch-thumb", 2, 20, (box) => box.x - track.x)
      expectIncreasing(travel)
      expect(travel[travel.length - 1]).toBeLessThan(2 + SWITCH_TRAVEL)
      frame(app.renderer, 1000)
      settle(app.renderer)
      expect(thumbLeft()).toBeCloseTo(2 + SWITCH_TRAVEL, 1)
      expect(boundsOf(app.renderer, "switch-thumb").width).toBeCloseTo(THUMB, 1)

      const [x, y] = centerOf(app.renderer, "switch-track")
      app.renderer.nativeSimulateMouseMove(x, y)
      app.renderer.nativeSimulateMouseDown(x, y)
      const widths = trace(app.renderer, "switch-thumb", 2, 20, (box) => box.width)
      expectIncreasing(widths)
      frame(app.renderer, 1000)
      expect(boundsOf(app.renderer, "switch-thumb").width).toBeCloseTo(THUMB + 4, 1)

      app.renderer.nativeSimulateMouseUp(x, y)
      frame(app.renderer, 1000)
      settle(app.renderer)
      expect(boundsOf(app.renderer, "switch-thumb").width).toBeCloseTo(THUMB, 1)
      expect(thumbLeft()).toBeCloseTo(2, 1)
      expectIdle(app)
      finish(app)
    } finally {
      app.unmount()
    }
  })

  it("replays the roots once every motion has settled, and builds nothing above the wrapper", () => {
    const [to, setTo] = createSignal(0)
    const [open, setOpen] = createSignal(false)
    const app = mount(() => (
      <>
        <Mover to={to} />
        <Accordion open={open} />
        <Slider />
        <Switch />
      </>
    ))
    try {
      const mover = boundsOf(app.renderer, "mover").x
      app.flushSync(() => {
        setTo(100)
        setOpen(true)
      })
      app.renderer.nativeSimulateClick(...centerOf(app.renderer, "switch-track"))
      const track = boundsOf(app.renderer, "track")
      app.renderer.nativeSimulateMouseDown(track.x + 100, track.y + 10)
      app.renderer.nativeSimulateMouseUp(track.x + 100, track.y + 10)
      expectIncreasing(trace(app.renderer, "mover", 3, 100, (box) => box.x))
      frame(app.renderer, 3000)
      followResize(app)
      settle(app.renderer)

      expect(boundsOf(app.renderer, "mover").x).toBe(mover + 100)
      expectIdle(app)
      finish(app)
    } finally {
      app.unmount()
    }
  })
})
