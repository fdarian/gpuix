/** Whether the machine lets the e2e look at its window: the checks read the window's captured pixels
 *  and hand events to a key window of an active app, so a machine that cannot give it those makes
 *  every check fail for a reason that is not the product's. */
import { helper, Shot } from "./helper"

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms))

/** The run cannot continue, or begin, because of the machine. */
export class NotCheckable extends Error {}

const ACTIVE = 1
const KEY = 2
const VISIBLE = 4

export function describeWindow(): string {
  const state = helper.active()
  return [
    state & ACTIVE ? "app active" : "app not active",
    state & KEY ? "window key" : "window not key",
    state & VISIBLE ? "window visible" : "window not visible",
  ].join(", ")
}

function sessionProblem(): string | null {
  const session = helper.session()
  if (session < 0) return "the system gives no login session"
  if (session & 1) return "the screen is locked"
  if (session & 2) return "the login session is not the one on the console"
  return null
}

/** `E2E_SKIP_ACTIVATION=1` waives "the app is active" and "the window is key" for a run of only the
 *  lockstep group, which reads frames and captures and sends no events. A system dialog that cannot be
 *  dismissed keeps the app from becoming active; the window is still required to be visible, and is
 *  placed where such a dialog is unlikely to cover it. */
export const skipActivation = process.env.E2E_SKIP_ACTIVATION === "1"

export function checkSkipActivation() {
  if (skipActivation && process.env.E2E_ONLY !== "lockstep") {
    throw new Error("E2E_SKIP_ACTIVATION=1 is for E2E_ONLY=lockstep: the other groups need an active app and a key window")
  }
}

function windowProblem(): string | null {
  const state = helper.active()
  const missing: string[] = []
  if (!(state & ACTIVE) && !skipActivation) missing.push("the app is not active (another app has the focus and macOS did not hand it over)")
  if (!(state & KEY) && !skipActivation) missing.push("the window is not key")
  if (!(state & VISIBLE)) missing.push("the window is not visible (covered, off the screen or on a display that is asleep)")
  return missing.length ? missing.join("; ") : null
}

/** A capture that is not an image of the window as it is drawn. The size is compared only when the
 *  window is not being resized by a check, whose own failure it would otherwise hide. */
function captureProblem(checkSize: boolean): string | null {
  let shot: Shot
  try {
    shot = helper.capture()
  } catch (error) {
    return `the window cannot be captured: ${error instanceof Error ? error.message : String(error)}`
  }
  const width = Math.round(helper.contentWidth() * shot.scale)
  const height = Math.round(helper.contentHeight() * shot.scale)
  if (checkSize && (shot.width !== width || shot.height < height)) {
    return `the capture is ${shot.width}x${shot.height} pixels, a window of ${width}x${height} was expected`
  }
  const colors = new Set<string>()
  for (let row = 0; row < 24; row++) {
    for (let column = 0; column < 24; column++) {
      const pixel = shot.device(Math.floor((column * (shot.width - 1)) / 23), Math.floor((row * (shot.height - 1)) / 23))
      colors.add(pixel.join(","))
    }
  }
  if (colors.size < 2) return `the capture is blank (all ${shot.width}x${shot.height} pixels sampled are rgb(${[...colors][0]}))`
  return null
}

/** What stops the checks right now, or null. Changes nothing in the window. */
export function problem(checkSize = false): string | null {
  return sessionProblem() ?? windowProblem() ?? captureProblem(checkSize)
}

/** Activates the app and makes its window key, a few times with a short wait (activation is
 *  asynchronous, and macOS may take a moment to give it to a process another app has handed nothing
 *  to), until the checks can run. Returns what is still stopping them, or null. With
 *  `E2E_SKIP_ACTIVATION` it places the window instead of activating the app. */
export async function makeCheckable(checkSize: boolean, attempts = 6): Promise<string | null> {
  let last: string | null = null
  for (let attempt = 0; attempt < attempts; attempt++) {
    if (skipActivation) helper.placeTopLeft()
    else helper.makeKey()
    await sleep(300)
    last = problem(checkSize)
    if (last === null) return null
  }
  return last
}
