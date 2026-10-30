/** The checks of the e2e. They run in the app's own process, once the window is up, against what
 *  the window really shows (its captured pixels) and what AppKit really does with the events handed
 *  to it. */
import { existsSync, mkdirSync, readFileSync } from "node:fs"
import { cpus, loadavg } from "node:os"
import { checkSkipActivation, describeWindow, makeCheckable, NotCheckable, problem } from "./environment"
import { helper, Shot } from "./helper"
import { controls, LAYOUT, FIXTURE, FIXTURE_BARE, fileUrl } from "./app"

const OUT = `${process.env.HOME}/.cache/gpuix-webview-e2e`
const PRESENT_LOG = `${OUT}/present.log`
const TRANSIENT_STEPS = Number(process.env.E2E_STEPS ?? 120)

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms))
const turn = () => new Promise<void>((resolve) => setImmediate(resolve))

async function waitFor(what: string, condition: () => boolean | Promise<boolean>, timeoutMs = 10000) {
  const deadline = performance.now() + timeoutMs
  while (performance.now() < deadline) {
    if (await condition()) return
    await sleep(20)
  }
  throw new Error(`timed out waiting for ${what}`)
}

interface Row {
  group: string
  name: string
  pass: boolean
  skipped?: boolean
  /** Why the machine, not the product, left this check without an answer. */
  environment?: string
  detail: string
}
const rows: Row[] = []

function skip(group: string, name: string, reason: string) {
  rows.push({ group, name, pass: true, skipped: true, detail: reason })
}

/** Set by a check that failed while the machine would not let the window be looked at. */
let lostWindow: string | null = null

function check(group: string, name: string, pass: boolean, detail: string) {
  if (pass) {
    rows.push({ group, name, pass, detail })
    return
  }
  // Asked when the check has failed, because that is when the window's state matters.
  const state = describeWindow()
  const cause = problem()
  if (cause) {
    lostWindow = cause
    rows.push({ group, name, pass: true, environment: cause, detail })
    console.log(`NOT CHECKABLE  ${group} / ${name}: ${cause} (${state}); it failed as: ${detail}`)
    return
  }
  rows.push({ group, name, pass, detail })
  console.log(`FAIL  ${group} / ${name}: ${detail} (${state})`)
}

/** After a check that found the window lost: gets it back for the checks that follow, or ends the run. */
async function recoverWindow() {
  if (lostWindow === null) return
  const cause = lostWindow
  lostWindow = null
  const remaining = await makeCheckable(false)
  if (remaining) throw new NotCheckable(`${remaining} (it could not be made key and visible again after: ${cause})`)
}

/** Records the outcome of a check that reads something that may throw. */
async function attempt(group: string, name: string, body: () => Promise<[boolean, string]> | [boolean, string]) {
  try {
    const [pass, detail] = await body()
    check(group, name, pass, detail)
  } catch (error) {
    check(group, name, false, `threw: ${error instanceof Error ? error.message : String(error)}`)
  }
  await recoverWindow()
}

type Color = readonly [number, number, number]
const near = (actual: Color, expected: Color, tolerance = 22) =>
  actual.every((channel, index) => Math.abs(channel - expected[index]!) <= tolerance)
const show = (color: Color) => `rgb(${color.join(",")})`

const RED: Color = [255, 0, 0]
const GREEN: Color = [0, 255, 0]
const BLUE: Color = [0, 0, 255]
const YELLOW: Color = [255, 255, 0]
const MAGENTA: Color = [255, 0, 255]
const CYAN: Color = [32, 208, 208]
const ALT_RED: Color = [128, 0, 0]
const ALT_BODY: Color = [208, 32, 32]
const BACKGROUND: Color = [240, 240, 240]
const POPOVER: Color = [51, 51, 51]
const isMagenta = (color: Color) => color[0] > 200 && color[2] > 200 && color[1] < 70

function capture(): Shot {
  const shot = helper.capture()
  shot.originY = shot.height / shot.scale - helper.contentHeight()
  return shot
}

const rect = (name: string) => {
  const element = controls.elements[name]
  if (!element) throw new Error(`no element ${name}`)
  return element.getBoundingClientRect()
}
const view = (name: string) => {
  const instance = controls.views[name]
  if (!instance) throw new Error(`no webview ${name}`)
  return instance
}
const element = (name: string) => {
  const found = controls.elements[name]
  if (!found) throw new Error(`no element ${name}`)
  return found
}

/** A point of a page in the window's content, in points. */
const inPage = (name: string, x: number, y: number): [number, number] => {
  const box = rect(name)
  return [box.x + x, box.y + y]
}

function sampleAt(shot: Shot, [x, y]: [number, number]): Color {
  return shot.at(x, y)
}

/** The first device row, from `fromY` (points) down to `toY`, whose pixel in column `x` (points) satisfies `test`. */
function firstRow(shot: Shot, x: number, fromY: number, toY: number, test: (color: Color) => boolean): number | null {
  const column = Math.round(x * shot.scale)
  const start = Math.round((fromY + shot.originY) * shot.scale)
  const end = Math.round((toY + shot.originY) * shot.scale)
  for (let row = start; row < end; row++) {
    if (test(shot.device(column, row))) return row
  }
  return null
}

const pageColumn = () => rect("page-a").x + 100
const gpuiColumn = () => rect("marker").x + rect("marker").width / 2

/** The top edge of the page's marker row and of the GPUI marker beside it, in device rows. */
function markerEdges(shot: Shot): { page: number | null; gpui: number | null } {
  const scroller = rect("scroller")
  const from = scroller.y
  const to = scroller.y + scroller.height
  return {
    page: firstRow(shot, pageColumn(), from, to, isMagenta),
    gpui: firstRow(shot, gpuiColumn(), from, to, isMagenta),
  }
}

const presentCount = () => (existsSync(PRESENT_LOG) ? readFileSync(PRESENT_LOG, "utf8").split("\n").length : 0)

async function scrollTo(y: number) {
  element("scroller").scrollTo?.(0, y)
}

async function evaluate<T>(name: string, code: string): Promise<T> {
  return (await view(name).executeJavaScript(code)) as T
}

function resetFocusLog() {
  controls.focusLog.length = 0
}

/** The exit status of the e2e. */
const EXIT_PASSED = 0
const EXIT_FAILED = 1
export const EXIT_COULD_NOT_RUN = 2
const EXIT_NOT_CHECKABLE = 3

export async function run(): Promise<number> {
  checkSkipActivation()
  mkdirSync(OUT, { recursive: true })
  await waitFor("the window", () => {
    try {
      helper.findWindow()
      return true
    } catch {
      return false
    }
  })
  await sleep(500)
  console.log(`window ${helper.contentWidth()}x${helper.contentHeight()} points at scale ${helper.scale()}; before making it key: ${describeWindow()}`)
  // Before the pages: a window that is covered gets no frames, and its pages never load.
  const blocked = await makeCheckable(true)
  if (blocked) {
    console.log(`not checkable: ${blocked} (${describeWindow()})`)
    return EXIT_NOT_CHECKABLE
  }
  console.log(`window state: ${describeWindow()}`)
  try {
    await waitFor("the pages to load", () =>
      ["a", "b", "c"].every((name) =>
        controls.events.some((event) => event.view === name && event.type === "did-stop-loading")
      )
    )
  } catch (error) {
    const cause = problem()
    if (!cause) throw error
    console.log(`not checkable: ${cause} (${describeWindow()}); the pages did not load`)
    return EXIT_NOT_CHECKABLE
  }
  await sleep(500)

  // E2E_ONLY=loading,focus runs only those groups, for working on one.
  const only = process.env.E2E_ONLY?.split(",")
  const groups: [string, () => Promise<void>][] = [
    ["loading", loading],
    ["pixels", pixels],
    ["pointer", inputRouting],
    ["focus", focus],
    ["scripts", scripts],
    ["resize", resize],
    ["visibility", visibility],
    ["lockstep", lockstep],
  ]
  let stoppedBy: string | null = null
  for (const [name, body] of groups) {
    if (only && !only.includes(name)) continue
    try {
      await body()
    } catch (error) {
      const cause = error instanceof NotCheckable ? error.message : problem()
      if (!cause) throw error
      stoppedBy = `${cause} (in the ${name} group: ${error instanceof Error ? error.message : String(error)})`
      break
    }
  }

  const failed = rows.filter((row) => !row.pass)
  const unanswered = rows.filter((row) => row.environment !== undefined).length
  const skipped = rows.filter((row) => row.skipped).length
  const passed = rows.length - failed.length - skipped - unanswered
  console.log("\n" + table())
  for (const line of lockstepSummary) console.log(line)
  console.log(`\n${passed} passed, ${failed.length} failed, ${skipped} not checkable in-process, ${unanswered} not checkable now (of ${rows.length})`)
  if (stoppedBy) console.log(`not checkable: the run stopped early: ${stoppedBy}`)
  if (failed.length > 0) return EXIT_FAILED
  return stoppedBy || unanswered > 0 ? EXIT_NOT_CHECKABLE : EXIT_PASSED
}

function table(): string {
  const width = Math.max(...rows.map((row) => `${row.group} / ${row.name}`.length))
  return rows
    .map((row) => `${row.environment !== undefined ? "N/C " : row.skipped ? "SKIP" : row.pass ? "PASS" : "FAIL"}  ${`${row.group} / ${row.name}`.padEnd(width)}  ${row.environment !== undefined ? `${row.environment}; it failed as: ${row.detail}` : row.detail}`)
    .join("\n")
}

// ── loading and events ──────────────────────────────────────────────────────────────────

async function loading() {
  const group = "events"
  const of = (name: string, type: string) => controls.events.filter((event) => event.view === name && event.type === type)
  await attempt(group, "load events in order, with real values", () => {
    const order = controls.events.filter((event) => event.view === "a").map((event) => event.type)
    const title = of("a", "page-title-updated")[0]
    const navigated = of("a", "did-navigate")[0]
    const ok =
      order[0] === "did-start-loading" &&
      order.includes("did-stop-loading") &&
      title?.title === "Gpuix E2E Fixture" &&
      title?.explicitSet === true &&
      navigated?.url === fileUrl(FIXTURE)
    return [ok, `${order.join(", ")}; title ${JSON.stringify(title?.title)}; url ${navigated?.url}`]
  })
  await attempt(group, "getURL, getTitle, canGoBack read the real page", () => {
    const page = view("page-a")
    const ok = page.getTitle() === "Gpuix E2E Fixture" && page.getURL() === fileUrl(FIXTURE) && !page.canGoBack()
    return [ok, `title ${JSON.stringify(page.getTitle())}, url ${page.getURL()}, canGoBack ${page.canGoBack()}`]
  })
  await attempt(group, "did-fail-load for a missing local file", async () => {
    const before = controls.events.length
    const missing = "file:///nonexistent-gpuix-e2e/missing.html"
    view("page-c").loadURL(missing)
    await waitFor("did-fail-load", () => controls.events.slice(before).some((event) => event.type === "did-fail-load"))
    const failure = controls.events.slice(before).find((event) => event.type === "did-fail-load")!
    const code = failure.errorCode as number
    return [
      code < 0 && failure.validatedUrl === missing,
      `code ${code}, ${JSON.stringify(failure.errorDescription)}, url ${failure.validatedUrl}`,
    ]
  })
  await attempt(group, "the page loads again after a failure", async () => {
    const before = controls.events.length
    view("page-c").loadURL(fileUrl(FIXTURE))
    await waitFor("did-stop-loading", () => controls.events.slice(before).some((event) => event.type === "did-stop-loading"))
    const title = await evaluate<string>("page-c", "document.title")
    return [title === "Gpuix E2E Fixture", `title ${JSON.stringify(title)}`]
  })
}

// ── pixels ──────────────────────────────────────────────────────────────────────────────

async function pixels() {
  const group = "pixels"
  await scrollTo(0)
  await sleep(200)
  const shot = capture()
  shot.originY = shot.height / shot.scale - helper.contentHeight()
  helper.savePng(`${OUT}/pixels.png`)

  const colorChecks: [string, string, number, number, Color][] = [
    ["red block of the first page", "page-a", 50, 60, RED],
    ["green block", "page-a", 150, 60, GREEN],
    ["blue block", "page-a", 250, 60, BLUE],
    ["yellow block", "page-a", 350, 20, YELLOW],
    ["marker row", "page-a", 200, 5, MAGENTA],
    ["page background", "page-a", 400, 235, CYAN],
    ["second page (rounded), its palette", "page-b", 50, 40, ALT_RED],
  ]
  for (const [name, page, x, y, expected] of colorChecks) {
    await attempt(group, name, () => {
      const [wx, wy] = inPage(page, x, y)
      const actual = sampleAt(shot, [wx, wy])
      return [near(actual, expected), `${show(actual)} at (${wx}, ${wy}), expected ${show(expected)}`]
    })
  }

  await attempt(group, "the scroll container cuts the page at its bottom edge", () => {
    const scroller = rect("scroller")
    const bottom = scroller.y + scroller.height
    const column = rect("page-a").x + 150
    const edge = firstRow(shot, column, bottom - 20, bottom + 20, (color) => !near(color, CYAN, 20))
    const expected = Math.round((bottom + shot.originY) * shot.scale)
    return [edge === expected, `first non-page row ${edge}, scroller bottom at row ${expected}`]
  })

  await attempt(group, "the popover is above the page where they overlap", () => {
    const popover = LAYOUT.popover
    const inside = sampleAt(shot, [popover.left + 40, popover.top + 60])
    const left = sampleAt(shot, [popover.left - 3, popover.top + 60])
    const edge = firstRow(shot, popover.left + 60, popover.top - 10, popover.top + 10, (color) => near(color, POPOVER, 10))
    const expectedEdge = Math.round((popover.top + shot.originY) * shot.scale)
    return [
      near(inside, POPOVER) && near(left, CYAN) && edge === expectedEdge,
      `inside ${show(inside)}, 3pt left of it ${show(left)}, top edge row ${edge} (expected ${expectedEdge})`,
    ]
  })

  await attempt(group, "the rounded page is cut at its corner", () => {
    const box = rect("page-b")
    const corner = sampleAt(shot, [box.x + 2, box.y + 2])
    const inside = sampleAt(shot, [box.x + 30, box.y + 30])
    return [
      near(corner, BACKGROUND, 12) && near(inside, ALT_RED),
      `corner ${show(corner)}, 30pt in ${show(inside)}`,
    ]
  })

  await attempt(group, "the page at half opacity blends with GPUI content below it", () => {
    const [x, y] = inPage("page-c", 150, 112)
    const actual = sampleAt(shot, [x, y])
    const expected: Color = [(CYAN[0] + BACKGROUND[0]) / 2, (CYAN[1] + BACKGROUND[1]) / 2, (CYAN[2] + BACKGROUND[2]) / 2]
    return [near(actual, expected, 18), `${show(actual)}, expected about ${show(expected)}`]
  })

  await scroll()
}

async function scroll() {
  const group = "scroll"
  const scroller = rect("scroller")
  const containerTop = (shot: Shot) => Math.round((scroller.y + shot.originY) * shot.scale)
  // The page starts 100 below the top of the window; the container's top is at 60.
  for (const offset of [0, 13, 33, 40]) {
    await attempt(group, `container scrolled to ${offset}: the page's marker row and the GPUI marker have the same top`, async () => {
      await scrollTo(offset)
      await sleep(250)
      const shot = capture()
      const edges = markerEdges(shot)
      const expected = Math.round((100 - offset + shot.originY) * shot.scale)
      return [
        edges.page === edges.gpui && edges.page === expected,
        `page marker row ${edges.page}, GPUI marker row ${edges.gpui}, expected ${expected}`,
      ]
    })
  }
  for (const offset of [60, 90]) {
    await attempt(group, `container scrolled to ${offset}: the page is cut at the container's top`, async () => {
      await scrollTo(offset)
      await sleep(250)
      const shot = capture()
      const page = rect("page-a")
      const redStart = firstRow(shot, page.x + 50, scroller.y, scroller.y + scroller.height, (color) => near(color, RED))
      // The red block is page rows 10 to 110: it ends at the page's top plus 110.
      const redEnd = firstRow(shot, page.x + 50, scroller.y, scroller.y + scroller.height, (color) => !near(color, RED))
      const expectedEnd = Math.round((page.y + 110 + shot.originY) * shot.scale)
      const edges = markerEdges(shot)
      return [
        redStart === containerTop(shot) && redEnd === expectedEnd && edges.page === null && edges.gpui === null,
        `red from row ${redStart} (container top ${containerTop(shot)}) to row ${redEnd} (expected ${expectedEnd}); marker rows ${edges.page}/${edges.gpui}`,
      ]
    })
  }
  for (const offset of [150, 200, 460]) {
    await attempt(group, `container scrolled to ${offset}: the page fills the container's top and is cut at its bottom`, async () => {
      await scrollTo(offset)
      await sleep(250)
      const shot = capture()
      const page = rect("page-a")
      const column = page.x + 230
      const top = sampleAt(shot, [column, scroller.y + 1])
      // At 460 the page's bottom is at 100 + 520 - 460 = 160, above the container's bottom: it ends there.
      const pageBottom = page.y + page.height
      const bottomEdge = Math.min(pageBottom, scroller.y + scroller.height)
      const end = firstRow(shot, column, scroller.y + 20, scroller.y + scroller.height + 20, (color) => !near(color, CYAN))
      const expectedEnd = Math.round((bottomEdge + shot.originY) * shot.scale)
      return [
        near(top, CYAN) && end === expectedEnd,
        `top of the container ${show(top)}; page ends at row ${end} (expected ${expectedEnd})`,
      ]
    })
  }
  await scrollTo(0)
  await sleep(200)
}

// ── pointer routing ─────────────────────────────────────────────────────────────────────

async function inputRouting() {
  const group = "pointer"
  await scrollTo(0)
  await sleep(200)

  await attempt(group, "a click on the page reaches WebKit (its counter counts)", async () => {
    const before = await evaluate<number>("page-a", "window.counter")
    const [x, y] = inPage("page-a", 60, 175)
    helper.click(x, y)
    await sleep(300)
    const after = await evaluate<number>("page-a", "window.counter")
    return [after === before + 1, `counter ${before} -> ${after}`]
  })

  await attempt(group, "a click on the popover over the page does not reach the page", async () => {
    const pageClicks = await evaluate<number>("page-a", "window.clicks")
    const popover = controls.clicks.popover
    helper.click(LAYOUT.popover.left + 40, LAYOUT.popover.top + 40)
    await sleep(300)
    const after = await evaluate<number>("page-a", "window.clicks")
    return [
      after === pageClicks && controls.clicks.popover === popover + 1,
      `page clicks ${pageClicks} -> ${after}; popover clicks ${popover} -> ${controls.clicks.popover}`,
    ]
  })

  await attempt(group, "a click on the page next to the popover reaches the page", async () => {
    const pageClicks = await evaluate<number>("page-a", "window.clicks")
    const popover = controls.clicks.popover
    const [x, y] = inPage("page-a", 380, 240)
    helper.click(x, y)
    await sleep(300)
    const after = await evaluate<number>("page-a", "window.clicks")
    return [after === pageClicks + 1 && controls.clicks.popover === popover, `page clicks ${pageClicks} -> ${after}`]
  })

  await attempt(group, "a click on a GPUI button reaches it and not the page", async () => {
    const clicks = controls.clicks.gpuiButton
    const pageClicks = await evaluate<number>("page-a", "window.clicks")
    const button = rect("gpui-button")
    helper.click(button.x + 10, button.y + 10)
    await sleep(200)
    const after = await evaluate<number>("page-a", "window.clicks")
    return [
      controls.clicks.gpuiButton === clicks + 1 && after === pageClicks,
      `button clicks ${clicks} -> ${controls.clicks.gpuiButton}; page clicks ${pageClicks} -> ${after}`,
    ]
  })

  await attempt(group, "a wheel over the page scrolls the page's wheel handler, not the container around it", async () => {
    await scrollTo(0)
    await sleep(200)
    const wheels = await evaluate<number>("page-a", "window.wheels")
    const top = element("scroller").scrollTop
    const [x, y] = inPage("page-a", 380, 235)
    const reported = helper.wheel(x, y, -40)
    await sleep(400)
    const after = await evaluate<number>("page-a", "window.wheels")
    const scrolled = element("scroller").scrollTop
    const located = Math.abs(reported.x - x) < 1.5 && Math.abs(reported.y - y) < 1.5
    return [
      after > wheels && scrolled === top,
      `page wheel events ${wheels} -> ${after}; container scrollTop ${top} -> ${scrolled}; the event said it is at (${reported.x}, ${reported.y}) in window ${reported.window}, wanted (${x}, ${y})${located ? "" : " (location differs)"}`,
    ]
  })

  await attempt(group, "a wheel over GPUI content of the container scrolls the container", async () => {
    const top = element("scroller").scrollTop
    const scroller = rect("scroller")
    const reported = helper.wheel(scroller.x + 100, scroller.y + 20, -40)
    await sleep(400)
    const scrolled = element("scroller").scrollTop
    await scrollTo(0)
    await sleep(200)
    return [scrolled > top, `container scrollTop ${top} -> ${scrolled} (the event said (${reported.x}, ${reported.y}))`]
  })

  await attempt(group, "a wheel over the popover that does not use it goes on to the page below", async () => {
    const wheels = await evaluate<number>("page-a", "window.wheels")
    const reported = helper.wheel(LAYOUT.popover.left + 40, LAYOUT.popover.top + 40, -40)
    await sleep(400)
    const after = await evaluate<number>("page-a", "window.wheels")
    return [after > wheels, `page wheel events ${wheels} -> ${after} (the event said (${reported.x}, ${reported.y}))`]
  })

  await attempt(group, "hover leaves a GPUI element when the pointer moves onto the page", async () => {
    const target = rect("hover-target")
    const hovered = (shot: Shot) => sampleAt(shot, [target.x + 40, target.y + 14])
    helper.move(target.x + 40, target.y + 14)
    await sleep(250)
    const on = hovered(capture())
    const [px, py] = inPage("page-a", 380, 150)
    helper.move(px, py)
    await sleep(250)
    const off = hovered(capture())
    return [
      near(on, [0, 170, 170]) && near(off, [204, 204, 204]),
      `hovered ${show(on)}, after moving onto the page ${show(off)}`,
    ]
  })

  skip(
    group,
    "the cursor over a page is WebKit's",
    "not readable in-process: AppKit applies cursor rects and WebKit sets its cursor only for the real pointer (WebKit asks the system for the pointer's position, GPUI's cursor rects follow the real one), which a test inside the process cannot move; the cursor rects of the window view leave out the native views (zed test native_surface)"
  )
}

// ── focus ───────────────────────────────────────────────────────────────────────────────

async function focus() {
  const group = "focus"
  const focusedId = () => ((globalThis as any).__gpuixRenderHost?.renderer?.getFocusedElementId?.() ?? null) as number | null
  const idOf = (name: string) => element(name).id
  const responder = () => helper.firstResponder()
  const pageHasKeyboard = (name: string) => {
    const [x, y] = inPage(name, 150, 60)
    return helper.pageHasKeyboard(x, y) === 1
  }

  await attempt(group, "a click on the page focuses the webview element, blurs the focused GPUI element", async () => {
    element("gpui-input").focus?.()
    await sleep(150)
    const startedIn = focusedId() === idOf("gpui-input") && !pageHasKeyboard("page-a")
    resetFocusLog()
    const [x, y] = inPage("page-a", 380, 235)
    helper.click(x, y)
    await sleep(300)
    const log = controls.focusLog.join(", ")
    return [
      startedIn &&
        focusedId() === idOf("page-a") &&
        log.includes("blur:gpui-input") &&
        log.includes("focus:page-a") &&
        pageHasKeyboard("page-a"),
      `focused element ${focusedId()} (page-a is ${idOf("page-a")}); first responder ${responder()}; events ${log}`,
    ]
  })

  await attempt(group, "a click on a GPUI element takes the keyboard back from the page", async () => {
    resetFocusLog()
    const input = rect("gpui-input")
    helper.click(input.x + 20, input.y + 10)
    await sleep(300)
    const log = controls.focusLog.join(", ")
    return [
      focusedId() === idOf("gpui-input") && !pageHasKeyboard("page-a") && log.includes("blur:page-a"),
      `focused element ${focusedId()}; first responder ${responder()}; events ${log}`,
    ]
  })

  await attempt(group, "webview.focus() gives the page the keyboard, blur() takes it back", async () => {
    resetFocusLog()
    view("page-b").focus?.()
    await sleep(200)
    const focused = focusedId() === idOf("page-b") && pageHasKeyboard("page-b")
    const afterFocus = `focused ${focusedId()}, first responder ${responder()}`
    view("page-b").blur?.()
    await sleep(200)
    const blurred = focusedId() === null && !pageHasKeyboard("page-b")
    return [
      focused && blurred && controls.focusLog.includes("focus:page-b") && controls.focusLog.includes("blur:page-b"),
      `${afterFocus}; after blur(): focused ${focusedId()}, first responder ${responder()}; events ${controls.focusLog.join(", ")}`,
    ]
  })

  await attempt(group, "focusing another element returns the keyboard to the window", async () => {
    view("page-a").focus?.()
    await sleep(150)
    const had = pageHasKeyboard("page-a")
    element("gpui-button").focus?.()
    await sleep(200)
    return [
      had && !pageHasKeyboard("page-a") && focusedId() === idOf("gpui-button"),
      `first responder after .focus() on a GPUI element: ${responder()}`,
    ]
  })

  await tabbing()
}

async function tabbing() {
  const group = "tab"
  const focusedId = () => ((globalThis as any).__gpuixRenderHost?.renderer?.getFocusedElementId?.() ?? null) as number | null
  const idOf = (name: string) => element(name).id
  const pageHasKeyboard = (name: string) => {
    const [x, y] = inPage(name, 150, 60)
    return helper.pageHasKeyboard(x, y) === 1
  }
  const active = (name: string) => evaluate<string>(name, "document.activeElement ? document.activeElement.id || document.activeElement.tagName : ''")
  /** Presses Tab and waits, up to 4 s, for `until`: WebKit handles the key in another process. */
  const press = async (shift: boolean, until: () => Promise<boolean> | boolean) => {
    shift ? helper.shiftTab() : helper.tab()
    const deadline = performance.now() + 4000
    while (performance.now() < deadline) {
      await sleep(40)
      try {
        if (await until()) break
      } catch {
        // a page that is not there yet cannot be asked
      }
    }
    await sleep(60)
  }
  const inPageNow = (page: string, id: string) => async () => focusedId() === idOf(page) && (await active(page)) === id
  const onGpui = (name: string) => () => focusedId() === idOf(name)

  await attempt(group, "Tab from a GPUI element lands in the page, on its first tabbable element", async () => {
    element("gpui-button").focus?.()
    await sleep(150)
    resetFocusLog()
    await press(false, inPageNow("page-a", "field"))
    const inside = await active("page-a")
    return [
      focusedId() === idOf("page-a") && pageHasKeyboard("page-a") && inside === "field",
      `focused ${focusedId()} (page-a ${idOf("page-a")}); page keyboard ${pageHasKeyboard("page-a")}; page activeElement #${inside}; events ${controls.focusLog.join(", ")}`,
    ]
  })

  await attempt(group, "Tab moves along the page's own tabbable elements", async () => {
    await press(false, inPageNow("page-a", "button"))
    const second = await active("page-a")
    await press(false, inPageNow("page-a", "link"))
    const third = await active("page-a")
    return [
      second === "button" && third === "link" && focusedId() === idOf("page-a"),
      `then #${second}, then #${third}; GPUI focus still on page-a: ${focusedId() === idOf("page-a")}`,
    ]
  })

  await attempt(group, "Tab past the page's last tabbable element moves on to the next GPUI tab stop", async () => {
    resetFocusLog()
    await press(false, inPageNow("page-b", "field"))
    const moved = focusedId() === idOf("page-b") && pageHasKeyboard("page-b") && !pageHasKeyboard("page-a")
    const inside = await active("page-b")
    return [
      moved && inside === "field",
      `focused ${focusedId()} (page-b ${idOf("page-b")}); page-b keyboard ${pageHasKeyboard("page-b")}, activeElement #${inside}; events ${controls.focusLog.join(", ")}`,
    ]
  })

  await attempt(group, "Shift+Tab out of the page's first tabbable element goes to the previous tab stop, into its last", async () => {
    resetFocusLog()
    await press(true, inPageNow("page-a", "link"))
    const moved = focusedId() === idOf("page-a") && pageHasKeyboard("page-a")
    const inside = await active("page-a")
    return [
      moved && inside === "link",
      `focused ${focusedId()} (page-a ${idOf("page-a")}); activeElement #${inside}; events ${controls.focusLog.join(", ")}`,
    ]
  })

  await attempt(group, "Shift+Tab walks back through the page, then out to the GPUI element before it", async () => {
    await press(true, inPageNow("page-a", "button"))
    const first = await active("page-a")
    await press(true, inPageNow("page-a", "field"))
    const second = await active("page-a")
    resetFocusLog()
    await press(true, onGpui("gpui-button"))
    return [
      first === "button" && second === "field" && focusedId() === idOf("gpui-button") && !pageHasKeyboard("page-a"),
      `#${first}, #${second}, then GPUI focus on ${focusedId()} (gpui-button ${idOf("gpui-button")}); events ${controls.focusLog.join(", ")}`,
    ]
  })

  await attempt(group, "a page with no tabbable element takes one Tab stop, and Tab and Shift+Tab leave it", async () => {
    const before = controls.events.length
    view("page-b").loadURL(fileUrl(FIXTURE_BARE))
    await waitFor("the bare page", () => controls.events.slice(before).some((event) => event.view === "b" && event.type === "did-stop-loading"))
    const log: string[] = []
    element("gpui-button").focus?.()
    await sleep(150)
    await press(false, inPageNow("page-a", "field"))
    await press(false, inPageNow("page-a", "button"))
    await press(false, inPageNow("page-a", "link"))
    // Out of page-a into the bare page-b.
    await press(false, onGpui("page-b"))
    const intoBare = focusedId() === idOf("page-b") && pageHasKeyboard("page-b")
    // The bare page has nothing to take: the next Tab goes on to page-c.
    resetFocusLog()
    await press(false, inPageNow("page-c", "field"))
    log.push(`forward: ${controls.focusLog.join(", ")}`)
    const pastBare = focusedId() === idOf("page-c") && (await active("page-c")) === "field"
    // Shift+Tab out of page-c's first element: back into the bare page, and on past it.
    await press(true, onGpui("page-b"))
    const backIntoBare = focusedId() === idOf("page-b") && pageHasKeyboard("page-b")
    resetFocusLog()
    await press(true, inPageNow("page-a", "link"))
    log.push(`backward: ${controls.focusLog.join(", ")}`)
    const backPast = focusedId() === idOf("page-a") && (await active("page-a")) === "link"
    return [
      intoBare && pastBare && backIntoBare && backPast,
      `into the bare page ${intoBare}, past it to page-c #field ${pastBare}; back into it ${backIntoBare}, past it to page-a #link ${backPast}; ${log.join("; ")}`,
    ]
  })

  // Page B is the second palette again for the checks that follow.
  const reloaded = controls.events.length
  view("page-b").loadURL(fileUrl(FIXTURE, "#alt"))
  await waitFor("page-b", () => controls.events.slice(reloaded).some((event) => event.view === "b" && event.type === "did-stop-loading"))

  await attempt(group, "typing in a page input reaches the page, not GPUI", async () => {
    view("page-a").focus?.()
    await sleep(200)
    await evaluate("page-a", "document.getElementById('field').value = ''; document.getElementById('field').focus(); 1")
    resetFocusLog()
    helper.key(0, "a", "a")
    helper.key(11, "b", "b")
    await sleep(350)
    const value = await evaluate<string>("page-a", "document.getElementById('field').value")
    return [value === "ab", `the page's input has ${JSON.stringify(value)}`]
  })
}

// ── scripts ─────────────────────────────────────────────────────────────────────────────

async function scripts() {
  const group = "executeJavaScript"
  await attempt(group, "resolves a value from the real page", async () => {
    const value = await evaluate<number>("page-a", "6 * 7")
    const object = await evaluate<{ a: number[] }>("page-a", "({ a: [1, 2, 3] })")
    return [value === 42 && JSON.stringify(object) === '{"a":[1,2,3]}', `${value}, ${JSON.stringify(object)}`]
  })
  await attempt(group, "resolves undefined and null", async () => {
    const nothing = await view("page-a").executeJavaScript("void 0")
    const nul = await view("page-a").executeJavaScript("null")
    return [nothing === undefined && nul === null, `${nothing}, ${nul}`]
  })
  await attempt(group, "rejects with the page's error", async () => {
    try {
      await view("page-a").executeJavaScript("window.failNow()")
      return [false, "resolved"]
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      return [message.includes("boom from the page"), JSON.stringify(message)]
    }
  })
  await attempt(group, "rejects a ReferenceError", async () => {
    try {
      await view("page-a").executeJavaScript("nope()")
      return [false, "resolved"]
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      return [message.includes("nope"), JSON.stringify(message)]
    }
  })
}

// ── resize ──────────────────────────────────────────────────────────────────────────────

async function resize() {
  const group = "resize"
  await scrollTo(0)
  await sleep(150)
  helper.setContentSize(1000, 760)
  await sleep(500)
  await attempt(group, "a wider window: the right-anchored pages follow it", () => {
    const shot = capture()
    helper.savePng(`${OUT}/resized.png`)
    const b = rect("page-b")
    const color = sampleAt(shot, [b.x + 50, b.y + 40])
    const oldPlace = sampleAt(shot, [640 + 50, 60 + 40])
    return [
      b.x === 1000 - 20 - 240 && near(color, ALT_RED) && near(oldPlace, BACKGROUND, 12),
      `page-b now at x ${b.x}: ${show(color)}; its old place ${show(oldPlace)}`,
    ]
  })
  await attempt(group, "the page in the container keeps its marker aligned", () => {
    const shot = capture()
    const edges = markerEdges(shot)
    const expected = Math.round((100 + shot.originY) * shot.scale)
    return [edges.page === expected && edges.gpui === expected, `page marker row ${edges.page}, GPUI marker row ${edges.gpui}, expected ${expected}`]
  })
  await attempt(group, "the page at half opacity moved with it", () => {
    const shot = capture()
    const [x, y] = inPage("page-c", 150, 112)
    const color = sampleAt(shot, [x, y])
    const blended: Color = [(CYAN[0] + BACKGROUND[0]) / 2, (CYAN[1] + BACKGROUND[1]) / 2, (CYAN[2] + BACKGROUND[2]) / 2]
    return [near(color, blended), `${show(color)} at (${x}, ${y})`]
  })
  helper.setContentSize(900, 700)
  await sleep(500)
  await attempt(group, "and back to the first size", () => {
    const shot = capture()
    const b = rect("page-b")
    const color = sampleAt(shot, [b.x + 50, b.y + 40])
    const edges = markerEdges(shot)
    return [b.x === 640 && near(color, ALT_RED) && edges.page === edges.gpui, `page-b at x ${b.x}: ${show(color)}; marker rows ${edges.page}/${edges.gpui}`]
  })
}

// ── visibility ──────────────────────────────────────────────────────────────────────────

async function visibility() {
  const group = "visibility"
  const place = (): [number, number] => inPage("page-c", 150, 112)
  const blended: Color = [(CYAN[0] + BACKGROUND[0]) / 2, (CYAN[1] + BACKGROUND[1]) / 2, (CYAN[2] + BACKGROUND[2]) / 2]
  helper.savePng(`${OUT}/before-visibility.png`)
  await attempt(group, "the page at half opacity blends with the page it is over", async () => {
    const [x, y] = inPage("page-c", 170, 70)
    const over = sampleAt(capture(), [x, y])
    controls.setPageCHidden(true)
    await sleep(300)
    const shot = capture()
    const below = sampleAt(shot, [x, y])
    const green = sampleAt(shot, inPage("page-a", 150, 60))
    controls.setPageCHidden(false)
    await sleep(300)
    // The compositor's blend is in its own color space: the result is between the two, and neither.
    const between = over.every(
      (channel, index) => channel >= Math.min(green[index]!, below[index]!) - 6 && channel <= Math.max(green[index]!, below[index]!) + 6
    )
    return [
      between && !near(over, green, 14) && !near(over, below, 14),
      `${show(over)} between the page below ${show(below)} and the green of the page above ${show(green)}`,
    ]
  })
  await attempt(group, "display: none shows the GPUI content under the page", async () => {
    const [x, y] = place()
    const before = sampleAt(capture(), [x, y])
    controls.setPageCHidden(true)
    await sleep(300)
    const hidden = sampleAt(capture(), [x, y])
    controls.setPageCHidden(false)
    await sleep(300)
    const shown = sampleAt(capture(), [x, y])
    return [
      near(before, blended, 18) && near(hidden, BACKGROUND, 8) && near(shown, blended, 18),
      `shown ${show(before)}, display: none ${show(hidden)}, shown again ${show(shown)}`,
    ]
  })
  await attempt(group, "unmounting removes the page, mounting makes a new one", async () => {
    const [x, y] = place()
    controls.setPageCMounted(false)
    await sleep(300)
    const gone = sampleAt(capture(), [x, y])
    const before = controls.events.length
    controls.setPageCMounted(true)
    await waitFor("the new page", () => controls.events.slice(before).some((event) => event.view === "c" && event.type === "did-stop-loading"))
    await sleep(300)
    const back = sampleAt(capture(), [x, y])
    return [near(gone, BACKGROUND, 8) && near(back, blended, 18), `unmounted ${show(gone)}, mounted again ${show(back)}`]
  })
}

// ── lockstep ────────────────────────────────────────────────────────────────────────────

interface Sample {
  step: number
  page: number | null
  gpui: number | null
  target: number
  previous: number
  /** Which capture of the step: 0 is the first one, right after the frame was presented. */
  capture: number
}

const offsetOfRow = (row: number | null, originY: number, scale: number) =>
  row === null ? null : Math.round(100 + originY - row / scale)

/** The lines of every lockstep run, printed again after the table. */
const lockstepSummary: string[] = []

interface LockstepRun {
  samples: number
  mismatches: Sample[]
  steps: number
  description: string[]
  /** The same limit as the check's: a run within it passes. */
  withinLimit: boolean
  summary: string[]
}

function lockstepSummaryOf(label: string, steps: number, samples: Sample[], mismatches: Sample[], originY: number, scale: number): string[] {
  const rate = (100 * mismatches.length) / samples.length
  const limit = Math.floor(samples.length * TRANSIENT_LIMIT)
  let pageLate = 0
  let gpuiLate = 0
  let other = 0
  let largestGap = 0
  for (const mismatch of mismatches) {
    const page = offsetOfRow(mismatch.page, originY, scale)
    const gpui = offsetOfRow(mismatch.gpui, originY, scale)
    if (page === null || gpui === null) {
      other++
      continue
    }
    largestGap = Math.max(largestGap, Math.abs(page - gpui))
    if (page === mismatch.previous && gpui === mismatch.target) pageLate++
    else if (gpui === mismatch.previous && page === mismatch.target) gpuiLate++
    else other++
  }
  const right = mismatches.filter((mismatch) => mismatch.capture === 0).length
  return [
    `lockstep ${label}: ${steps} frames, ${samples.length} captures; page and GPUI apart in ${mismatches.length} (${rate.toFixed(1)}%, the limit is ${(100 * TRANSIENT_LIMIT).toFixed(1)}% = ${limit}): ${mismatches.length <= samples.length * TRANSIENT_LIMIT ? "within" : "OVER"}`,
    `    late: the page ${pageLate}, GPUI ${gpuiLate}, neither or not found ${other}; right after the frame ${right}, a turn later ${mismatches.length - right}; largest gap ${largestGap} points`,
    `    ${describeWindow()}; load average ${loadavg()[0]!.toFixed(2)}`,
  ]
}

async function lockstepRun(label: string, steps: number): Promise<LockstepRun> {
  const samples: Sample[] = []
  const offsets = [0, 7, 21, 34, 12, 40, 3, 29, 17, 38, 25, 9]
  await scrollTo(0)
  await sleep(300)
  let previous = 0
  let originY = 0
  let scale = 1
  for (let step = 0; step < steps; step++) {
    const target = offsets[(step * 5) % offsets.length]! + (step % 3)
    const presents = presentCount()
    await scrollTo(target)
    const deadline = performance.now() + 300
    while (presentCount() === presents && performance.now() < deadline) await turn()
    // Right after the frame: the first capture, then one in the next turn, in case the layers land apart.
    for (let k = 0; k < 2; k++) {
      const shot = capture()
      originY = shot.originY
      scale = shot.scale
      const edges = markerEdges(shot)
      samples.push({ step, page: edges.page, gpui: edges.gpui, target, previous, capture: k })
      await turn()
    }
    previous = target
  }
  const mismatches = samples.filter((sample) => sample.page !== sample.gpui)
  const description = mismatches.map(
    (m) =>
      `step ${m.step}.${m.capture}: page at ${offsetOfRow(m.page, originY, scale) ?? "none"}, GPUI at ${offsetOfRow(m.gpui, originY, scale) ?? "none"} (asked ${m.target}, before ${m.previous})`
  )
  const summary = lockstepSummaryOf(label, steps, samples, mismatches, originY, scale)
  for (const line of summary) console.log(line)
  for (const line of description.slice(0, 8)) console.log(`    ${line}`)
  lockstepSummary.push(...summary)
  return {
    samples: samples.length,
    mismatches,
    steps,
    description,
    withinLimit: mismatches.length <= samples.length * TRANSIENT_LIMIT,
    summary,
  }
}

/** One capture in this many may find the page and the GPUI content apart: see the note in zed's
 *  `draw_composed`. More than that is a regression, none at all is what a quiet machine shows. */
const TRANSIENT_LIMIT = 0.03

async function lockstep() {
  const group = "lockstep"
  const report = (run: LockstepRun) =>
    `${run.summary[0]!.replace(/^lockstep [a-z]+: /, "")}; ${run.summary[1]!.trim()}` +
    (run.description.length ? `; ${run.description.slice(0, 4).join("; ")}` : "")
  const idle = await lockstepRun("idle", TRANSIENT_STEPS)
  check(group, `idle machine: ${idle.steps} scroll frames`, idle.withinLimit, report(idle))
  await recoverWindow()
  const burners = Array.from({ length: Math.max(2, cpus().length - 2) }, () =>
    Bun.spawn(["bun", "-e", "while (true) {}"], { stdout: "ignore", stderr: "ignore" })
  )
  try {
    await sleep(500)
    const loaded = await lockstepRun("loaded", TRANSIENT_STEPS)
    check(group, `CPU load (${burners.length} busy processes): ${loaded.steps} scroll frames`, loaded.withinLimit, report(loaded))
    await recoverWindow()
  } finally {
    for (const burner of burners) burner.kill()
  }
  await scrollTo(0)
}
