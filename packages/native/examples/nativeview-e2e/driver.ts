/** The checks of the e2e. They run in the app's own process, once the window is up, against what the
 *  window really shows (its captured pixels) and what AppKit really does with the events handed to it. */
import { mkdirSync, writeFileSync } from "node:fs"
import { checkSkipActivation, describeWindow, makeCheckable, NotCheckable, problem } from "../webview-e2e/environment"
import { helper } from "../webview-e2e/helper"
import { sleep, waitFor } from "./checks"
import { focus, pointer } from "./groups-input"
import { geometry, order, overflow } from "./groups-layout"
import { closeOwnWindow, lifecycle, prepareWindowClose } from "./groups-lifecycle"
import { CLOSE_STATE_PATH, OUT } from "./paths"
import { EXIT_COULD_NOT_RUN, EXIT_NOT_CHECKABLE, EXIT_PASSED, rows, summarize, table } from "./report"

export { EXIT_COULD_NOT_RUN }

/** How long the app has, after its window is closed, to end. */
const CLOSE_WAIT_MS = 15000

const GROUPS: [string, () => Promise<void>][] = [
  ["order", order],
  ["geometry", geometry],
  ["overflow", overflow],
  ["pointer", pointer],
  ["focus", focus],
  ["lifecycle", lifecycle],
]

function report(stoppedBy: string | null): number {
  console.log("\n" + table())
  return summarize(stoppedBy)
}

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
  const blocked = await makeCheckable(true)
  if (blocked) {
    console.log(`not checkable: ${blocked} (${describeWindow()})`)
    return EXIT_NOT_CHECKABLE
  }
  console.log(`window state: ${describeWindow()}`)

  // E2E_ONLY=order,pointer runs only those groups, for working on one; "close" is the last step.
  const only = process.env.E2E_ONLY?.split(",")
  for (const name of only ?? []) {
    if (name !== "close" && !GROUPS.some(([known]) => known === name)) throw new Error(`E2E_ONLY names no group "${name}"`)
  }
  const selected = (name: string) => !only || only.includes(name)

  let stoppedBy: string | null = null
  for (const [name, body] of GROUPS) {
    if (!selected(name)) continue
    try {
      await body()
    } catch (error) {
      const cause = error instanceof NotCheckable ? error.message : problem()
      const message = error instanceof Error ? error.message : String(error)
      if (cause) {
        stoppedBy = `${cause} (in the ${name} group: ${message})`
        break
      }
      rows.push({ group: name, name: "the group ran to its end", pass: false, detail: message })
      console.log(`FAIL  ${name} / the group ran to its end: ${message}`)
    }
  }

  if (!selected("close") || stoppedBy) return report(stoppedBy)
  return await closeWindowLast()
}

/** The last step: closes the app's own window, which ends the app without running any more JavaScript. The
 *  checks about it are made by `verdict.ts` from the state left here and the addon's log file. */
async function closeWindowLast(): Promise<number> {
  let state
  try {
    state = await prepareWindowClose()
  } catch (error) {
    rows.push({ group: "close", name: "two nativeviews are mounted to close with", pass: false, detail: error instanceof Error ? error.message : String(error) })
    return report(null)
  }
  console.log("closing the window; the checks about it follow from verdict.ts")
  closeOwnWindow()
  await sleep(CLOSE_WAIT_MS)
  state.stillRunning = true
  writeFileSync(CLOSE_STATE_PATH, JSON.stringify(state))
  return EXIT_PASSED
}
