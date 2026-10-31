/** What the groups share: the table of checks, waiting, the capture and the pixel reads. The `check`,
 *  `attempt` and `recoverWindow` logic is the webview e2e's (its `driver.ts` runs a whole app at import, so
 *  it cannot be imported): a check that fails while the machine lost the window is "not checkable", not a failure. */
import { describeWindow, makeCheckable, NotCheckable, problem } from "../webview-e2e/environment"
import { helper, Shot } from "../webview-e2e/helper"
import { controls, recordOf, Scene } from "./app"
import { rows } from "./report"

export const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms))

/** A frame and the native views' move with it have landed. */
export const SETTLE_MS = 350

export async function waitFor(what: string, condition: () => boolean, timeoutMs = 10000) {
  const deadline = performance.now() + timeoutMs
  while (performance.now() < deadline) {
    if (condition()) return
    await sleep(20)
  }
  throw new Error(`timed out after ${timeoutMs} ms waiting for ${what}`)
}

/** Set by a check that failed while the machine would not let the window be looked at. */
let lostWindow: string | null = null

export function check(group: string, name: string, pass: boolean, detail: string) {
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
export async function attempt(group: string, name: string, body: () => Promise<[boolean, string]> | [boolean, string]) {
  try {
    const [pass, detail] = await body()
    check(group, name, pass, detail)
  } catch (error) {
    check(group, name, false, `threw: ${error instanceof Error ? error.message : String(error)}`)
  }
  await recoverWindow()
}

export type Color = readonly [number, number, number]
export const near = (actual: Color, expected: Color, tolerance = 22) =>
  actual.every((channel, index) => Math.abs(channel - expected[index]!) <= tolerance)
export const show = (color: Color) => `rgb(${color.join(",")})`

export const RED: Color = [255, 0, 0]
export const GREEN: Color = [0, 255, 0]
export const BLUE: Color = [0, 0, 255]
export const YELLOW: Color = [255, 255, 0]
export const MAGENTA: Color = [255, 0, 255]
export const CYAN: Color = [0, 255, 255]
export const PAGE: Color = [240, 240, 240]

/** An edge the view or the layout puts at a given point may land this far (in points) from it. */
export const EDGE_TOLERANCE = 1

export function capture(): Shot {
  const shot = helper.capture()
  shot.originY = shot.height / shot.scale - helper.contentHeight()
  return shot
}

export function colorAt(shot: Shot, x: number, y: number): Color {
  return shot.at(x, y)
}

/** Whether the pixel at the point is the colour, with what it is for the detail of a row. */
export function isColor(shot: Shot, x: number, y: number, expected: Color, tolerance = 22): [boolean, string] {
  const actual = colorAt(shot, x, y)
  return [near(actual, expected, tolerance), `(${x}, ${y}) is ${show(actual)}, wanted ${show(expected)}`]
}

export interface Span {
  /** The first point along the line that satisfies the test, or null. */
  start: number | null
  /** The first point after `start` that does not, or null if it holds to the end of the range. */
  end: number | null
}

/** Scans the line at `fixed` (a column for "vertical", a row for "horizontal"), in points of the window's
 *  content from `from` to `to`, for the run of pixels that satisfy the test. */
export function spanOf(shot: Shot, direction: "vertical" | "horizontal", fixed: number, from: number, to: number, test: (color: Color) => boolean): Span {
  const vertical = direction === "vertical"
  const first = Math.round((vertical ? from + shot.originY : from) * shot.scale)
  const last = Math.round((vertical ? to + shot.originY : to) * shot.scale)
  const fixedDevice = Math.round((vertical ? fixed : fixed + shot.originY) * shot.scale)
  const toPoints = (index: number) => index / shot.scale - (vertical ? shot.originY : 0)
  let start: number | null = null
  for (let index = first; index < last; index++) {
    const matches = test(vertical ? shot.device(fixedDevice, index) : shot.device(index, fixedDevice))
    if (start === null && matches) start = toPoints(index)
    else if (start !== null && !matches) return { start, end: toPoints(index) }
  }
  return { start, end: null }
}

export const describeSpan = (span: Span) => `${span.start ?? "none"} to ${span.end ?? "the end of the line"}`

/** Whether a span starts and ends at the given points, within the edge tolerance. */
export function spanIs(span: Span, start: number, end: number): boolean {
  return (
    span.start !== null &&
    span.end !== null &&
    Math.abs(span.start - start) <= EDGE_TOLERANCE &&
    Math.abs(span.end - end) <= EDGE_TOLERANCE
  )
}

/** Mounts a scene and waits for each named `<nativeview>` to be attached once more, then for the frame. */
export async function showScene(scene: Scene, mounts: string[]) {
  const before = mounts.map((name) => recordOf(name).attaches.length)
  controls.show(scene)
  await waitFor(`${mounts.join(", ")} to attach`, () => mounts.every((name, index) => recordOf(name).attaches.length > before[index]!))
  await sleep(SETTLE_MS)
}

/** Takes the scene down. Whether the views detached is the lifecycle group's to check, not a precondition of
 *  the groups that follow. */
export async function clearScene() {
  controls.show({ kind: "empty" })
  await sleep(SETTLE_MS)
}
