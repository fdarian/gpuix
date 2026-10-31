/** Reads the addon's log of attach and detach calls. No native code is loaded here. */
import type { LogEntry } from "./addon"

/** Every attach is followed by exactly one detach of the same view, which still had a superview and a
 *  window, and nothing is attached at the end. */
export function auditLog(log: readonly LogEntry[]): [boolean, string] {
  const attached = new Set<string>()
  const problems: string[] = []
  for (const entry of log) {
    if (entry.kind === "attach") {
      if (attached.has(entry.handle)) problems.push(`#${entry.seq} attached ${entry.handle} twice`)
      attached.add(entry.handle)
    } else {
      if (!attached.delete(entry.handle)) problems.push(`#${entry.seq} detached ${entry.handle} with no attach`)
      if (!entry.hadSuperview || !entry.hadWindow) {
        problems.push(`#${entry.seq} detached ${entry.handle} with superview ${entry.hadSuperview}, window ${entry.hadWindow}`)
      }
    }
  }
  for (const handle of attached) problems.push(`${handle} was never detached`)
  const attaches = log.filter((entry) => entry.kind === "attach").length
  return [problems.length === 0, `${attaches} attaches and ${log.length - attaches} detaches in the addon's log${problems.length ? `; ${problems.join("; ")}` : ""}`]
}
