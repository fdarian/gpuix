/** The groups about attach and detach: when `onAttach` and `onDetach` come, what the library's view is
 *  then, and what the window does with a view when it closes. */
import { writeFileSync } from "node:fs"
import { addon, LogEntry } from "./addon"
import { auditLog } from "./audit"
import { controls, LAYOUT, NativeRecord, recordOf } from "./app"
import { attempt, capture, clearScene, Color, colorAt, near, PAGE, RED, BLUE, SETTLE_MS, show, showScene, sleep, waitFor } from "./checks"
import { CLOSE_STATE_PATH } from "./paths"
import { CloseState } from "./close-state"
import { rows } from "./report"
import { windows } from "./window-helper"

/** What an unmount is given to deliver `onDetach`, which the checks then count: a missing one is a failed
 *  check and not a wait that ends the group. */
const DETACH_MS = 500

const A = "life-a"
const B = "life-b"

/** The middle of the n-th slot of the row the lifecycle scene lays its views out in. */
function slot(index: number): [number, number] {
  const layout = LAYOUT.lifecycle
  return [layout.row.left + index * (layout.view.width + layout.row.gap) + layout.view.width / 2, layout.row.top + layout.view.height / 2]
}

const entriesOf = (handle: bigint): LogEntry[] => addon.log().filter((entry) => entry.handle === handle.toString())

function viewOf(record: NativeRecord): bigint {
  if (record.handle === null) throw new Error("the view was never attached")
  return record.handle
}

/** The counts of a record, to tell later what changed. */
function counts(record: NativeRecord) {
  return { attaches: record.attaches.length, detaches: record.detaches }
}
const sameCounts = (record: NativeRecord, before: ReturnType<typeof counts>) =>
  record.attaches.length === before.attaches && record.detaches === before.detaches

const colorIs = (color: Color, expected: Color, tolerance = 22) => near(color, expected, tolerance)

export async function lifecycle() {
  const group = "lifecycle"
  const a = recordOf(A)
  const b = recordOf(B)
  const scene = (shownA: boolean, shownB: boolean, swapped = false) => ({ kind: "lifecycle" as const, a: shownA, b: shownB, swapped })
  const pixel = (index: number) => colorAt(capture(), ...slot(index))

  const firstA = counts(a)
  await showScene(scene(true, false), [A])
  await attempt(group, "onAttach is called once, with a bigint that is a non-zero pointer, and the library's view is in", () => {
    const view = a.attaches.at(-1)
    const entries = view === undefined ? [] : entriesOf(view)
    const ok =
      a.attaches.length - firstA.attaches === 1 &&
      typeof view === "bigint" &&
      view > 0n &&
      entries.at(-1)?.kind === "attach" &&
      addon.stats(viewOf(a)).attached
    return [ok, `attaches ${firstA.attaches} -> ${a.attaches.length}, view ${String(view)} (${typeof view}), log ${JSON.stringify(entries.at(-1))}`]
  })
  await attempt(group, "the library's view shows in the window", () => {
    const color = pixel(0)
    return [colorIs(color, RED), `${show(color)} at the first slot`]
  })

  const view = viewOf(a)
  const beforeUnmount = counts(a)
  const attachSeq = entriesOf(view).at(-1)?.seq
  if (attachSeq === undefined) throw new Error("the addon logged no attach of the view")
  controls.detachProbe = () => colorAt(capture(), ...slot(0))
  controls.change(scene(false, false))
  await sleep(SETTLE_MS + DETACH_MS)
  controls.detachProbe = null

  await attempt(group, "unmount calls onDetach once", () => [
    a.detaches - beforeUnmount.detaches === 1 && a.attaches.length === beforeUnmount.attaches,
    `detaches ${beforeUnmount.detaches} -> ${a.detaches}, attaches ${beforeUnmount.attaches} -> ${a.attaches.length}`,
  ])
  await attempt(group, "onDetach comes with the library's view still in its superview and window", () => {
    const detach = entriesOf(view).at(-1)
    return [
      detach?.kind === "detach" && detach.hadSuperview === true && detach.hadWindow === true && detach.seq === attachSeq + 1,
      `the addon logged ${JSON.stringify(detach)} after the attach #${attachSeq}`,
    ]
  })
  await attempt(group, "onDetach comes before the pixels show the view gone", () => {
    const inside = a.probes.at(-1) as Color
    const after = pixel(0)
    return [
      Array.isArray(inside) && colorIs(inside, RED) && colorIs(after, PAGE, 12),
      `the window showed ${Array.isArray(inside) ? show(inside) : String(inside)} inside onDetach and ${show(after)} after a frame`,
    ]
  })
  await attempt(group, "the library released its view in onDetach", () => {
    const stats = addon.stats(view)
    return [!stats.attached, `attached ${stats.attached}`]
  })

  const beforeRemount = counts(a)
  const lastSeq = addon.log().at(-1)?.seq ?? 0
  controls.change(scene(true, false))
  await waitFor("the remount to attach", () => a.attaches.length > beforeRemount.attaches)
  await sleep(SETTLE_MS)
  await attempt(group, "remounting attaches again, once", () => {
    const fresh = addon.log().filter((entry) => entry.seq > lastSeq)
    const color = pixel(0)
    return [
      a.attaches.length - beforeRemount.attaches === 1 &&
        a.detaches === beforeRemount.detaches &&
        fresh.length === 1 &&
        fresh[0]!.kind === "attach" &&
        addon.stats(viewOf(a)).attached &&
        colorIs(color, RED),
      `attaches ${beforeRemount.attaches} -> ${a.attaches.length} (handles ${a.attaches.slice(-2).join(" then ")}); log since: ${JSON.stringify(fresh)}; pixel ${show(color)}`,
    ]
  })

  const beforeHandlerUnmount = counts(a)
  const handlerView = viewOf(a)
  const unmounter = LAYOUT.lifecycle.unmounter
  windows.clickLater(unmounter.left + unmounter.width / 2, unmounter.top + unmounter.height / 2)
  await sleep(SETTLE_MS + DETACH_MS)
  await attempt(group, "an unmount inside an event handler (flushSync in a click) calls onDetach once, before the pixels change", () => {
    const detach = entriesOf(handlerView).at(-1)
    return [
      a.detaches - beforeHandlerUnmount.detaches === 1 && detach?.kind === "detach" && detach.hadSuperview === true && detach.hadWindow === true,
      `detaches ${beforeHandlerUnmount.detaches} -> ${a.detaches}; the addon logged ${JSON.stringify(detach)}; first slot after the click ${show(pixel(0))}`,
    ]
  })
  const beforeRemountAgain = counts(a)
  controls.change(scene(true, false))
  await waitFor("the second remount to attach", () => a.attaches.length > beforeRemountAgain.attaches)
  await sleep(SETTLE_MS)

  const beforeBoth = { a: counts(a), b: counts(b) }
  controls.change(scene(true, true))
  await waitFor("the second view to attach", () => b.attaches.length > beforeBoth.b.attaches)
  await sleep(SETTLE_MS)
  await attempt(group, "two nativeviews are independent: the second attaches, the first is left alone", () => {
    const shot = capture()
    const first = colorAt(shot, ...slot(0))
    const second = colorAt(shot, ...slot(1))
    return [
      b.attaches.length - beforeBoth.b.attaches === 1 &&
        sameCounts(a, beforeBoth.a) &&
        viewOf(a) !== viewOf(b) &&
        colorIs(first, RED) &&
        colorIs(second, BLUE),
      `first view ${viewOf(a)}, second ${viewOf(b)}; first slot ${show(first)}, second ${show(second)}; first view counts ${JSON.stringify(counts(a))}`,
    ]
  })

  const beforeSwap = { a: counts(a), b: counts(b) }
  controls.change(scene(true, true, true))
  await sleep(SETTLE_MS)
  await attempt(group, "a keyed reorder moves the views and detaches nothing", () => {
    const shot = capture()
    const first = colorAt(shot, ...slot(0))
    const second = colorAt(shot, ...slot(1))
    return [
      sameCounts(a, beforeSwap.a) && sameCounts(b, beforeSwap.b) && colorIs(first, BLUE) && colorIs(second, RED),
      `first slot ${show(first)}, second ${show(second)}; counts ${JSON.stringify(counts(a))} ${JSON.stringify(counts(b))} (before ${JSON.stringify(beforeSwap)})`,
    ]
  })

  const beforeLeaving = { a: counts(a), b: counts(b) }
  controls.change(scene(true, false, true))
  await sleep(SETTLE_MS + DETACH_MS)
  await attempt(group, "unmounting one nativeview leaves the other attached", () => {
    const shot = capture()
    const first = colorAt(shot, ...slot(0))
    const second = colorAt(shot, ...slot(1))
    return [
      b.detaches - beforeLeaving.b.detaches === 1 &&
        sameCounts(a, beforeLeaving.a) &&
        addon.stats(viewOf(a)).attached &&
        !addon.stats(viewOf(b)).attached &&
        colorIs(first, RED) &&
        colorIs(second, PAGE, 12),
      `second view's detaches ${beforeLeaving.b.detaches} -> ${b.detaches}; first view counts ${JSON.stringify(counts(a))}; slots ${show(first)}, ${show(second)}`,
    ]
  })

  await clearScene()
  await attempt(group, "every view of the run was detached exactly once, still in its window", () => auditLog(addon.log()))
}

// ── window close ────────────────────────────────────────────────────────────────────────

/** Mounts two nativeviews and writes the state `verdict.ts` needs. */
export async function prepareWindowClose(): Promise<CloseState> {
  await showScene({ kind: "lifecycle", a: true, b: true, swapped: false }, [A, B])
  const state: CloseState = {
    rows,
    logLength: addon.log().length,
    views: { [A]: viewOf(recordOf(A)).toString(), [B]: viewOf(recordOf(B)).toString() },
    windowsVisible: windows.visible(),
    stillRunning: false,
  }
  writeFileSync(CLOSE_STATE_PATH, JSON.stringify(state))
  return state
}

/** Closes the app's own window, inside the app's next tick as a user's close is. */
export function closeOwnWindow() {
  const closing = windows.closeLater()
  if (closing !== 1) throw new Error(`the app has ${closing} windows to close, wanted 1`)
}
