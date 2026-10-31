import type { Row } from "./report"

/** What the driver leaves for `verdict.ts`: the app ends with its last window without running any
 *  JavaScript after it, so the checks about the close are made once the process is gone. */
export interface CloseState {
  rows: Row[]
  /** How many calls the addon had logged when the window was closed. */
  logLength: number
  /** The pointers of the views that were mounted, by name. */
  views: Record<string, string>
  windowsVisible: number
  /** Set when the app was still running after the wait. */
  stillRunning: boolean
}
