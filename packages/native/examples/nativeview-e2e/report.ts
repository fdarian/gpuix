/** The table of checks and the exit status. No native code is loaded here, so `verdict.ts` can use it after
 *  the app has gone. */

export interface Row {
  group: string
  name: string
  pass: boolean
  /** Why the machine, not the product, left this check without an answer. */
  environment?: string
  detail: string
}

export const rows: Row[] = []

export const EXIT_PASSED = 0
export const EXIT_FAILED = 1
export const EXIT_COULD_NOT_RUN = 2
export const EXIT_NOT_CHECKABLE = 3

export function table(): string {
  const width = Math.max(...rows.map((row) => `${row.group} / ${row.name}`.length))
  return rows
    .map((row) => {
      const mark = row.environment !== undefined ? "N/C " : row.pass ? "PASS" : "FAIL"
      const detail = row.environment !== undefined ? `${row.environment}; it failed as: ${row.detail}` : row.detail
      return `${mark}  ${`${row.group} / ${row.name}`.padEnd(width)}  ${detail}`
    })
    .join("\n")
}

/** Prints the summary line of the rows so far and gives the exit status. `stoppedBy` is why the run ended
 *  early because of the machine, or null. */
export function summarize(stoppedBy: string | null): number {
  const failed = rows.filter((row) => !row.pass)
  const unanswered = rows.filter((row) => row.environment !== undefined).length
  const passed = rows.length - failed.length - unanswered
  console.log(`\n${passed} passed, ${failed.length} failed, ${unanswered} not checkable now (of ${rows.length})`)
  if (stoppedBy) console.log(`not checkable: the run stopped early: ${stoppedBy}`)
  if (failed.length > 0) return EXIT_FAILED
  return stoppedBy || unanswered > 0 ? EXIT_NOT_CHECKABLE : EXIT_PASSED
}
