/** The checks about closing the window, made after the app has ended with it (`run.sh` runs this when the
 *  driver left a close state): the app's own record is the addon's log file, written as each call is made. */
import { readFileSync } from "node:fs"
import type { LogEntry } from "./addon"
import { auditLog } from "./audit"
import type { CloseState } from "./close-state"
import { ADDON_LOG_PATH, CLOSE_STATE_PATH } from "./paths"
import { EXIT_COULD_NOT_RUN, rows, summarize, table } from "./report"

const GROUP = "close"

function main(): number {
  const state = JSON.parse(readFileSync(CLOSE_STATE_PATH, "utf8")) as CloseState
  const appStatus = process.env.APP_STATUS
  if (appStatus === undefined) throw new Error("APP_STATUS is not set: run.sh gives the status the app ended with")
  const log = readFileSync(ADDON_LOG_PATH, "utf8")
    .split("\n")
    .filter((line) => line !== "")
    .map((line) => JSON.parse(line) as LogEntry)
  rows.push(...state.rows)

  const afterClose = log.slice(state.logLength)
  for (const [name, view] of Object.entries(state.views)) {
    const detaches = afterClose.filter((entry) => entry.kind === "detach" && entry.handle === view)
    rows.push({
      group: GROUP,
      name: `closing the window delivered onDetach once to ${name}, with its view still in a superview and a window`,
      pass: detaches.length === 1 && detaches[0]!.hadSuperview === true && detaches[0]!.hadWindow === true,
      detail: `the addon logged after the close: ${JSON.stringify(detaches)} (windows visible before it: ${state.windowsVisible})`,
    })
  }
  const [paired, detail] = auditLog(log)
  rows.push({ group: GROUP, name: "every view of the run was detached exactly once, still in its window", pass: paired, detail })
  rows.push({
    group: GROUP,
    name: "the app ended with its last window, with status 0",
    pass: !state.stillRunning && appStatus === "0",
    detail: state.stillRunning ? "the app was still running after the wait" : `exit status ${appStatus}`,
  })

  console.log("\n" + table())
  return summarize(null)
}

try {
  process.exit(main())
} catch (error) {
  console.error("the verdict could not be made:", error)
  process.exit(EXIT_COULD_NOT_RUN)
}
