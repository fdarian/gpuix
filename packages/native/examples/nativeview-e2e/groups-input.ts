/** The groups that hand events to the real window: who gets a press, and who has the keyboard. */
import { addon, Hit } from "./addon"
import { controls, LAYOUT, PointerScene, recordOf } from "./app"
import { helper } from "../webview-e2e/helper"
import { windows } from "./window-helper"
import { attempt, clearScene, showScene, sleep } from "./checks"

/** After an event is queued for the window: the tick that handles it and the frame after have run. */
const EVENT_MS = 350

const libraryStats = () => {
  const handle = recordOf("pointer").handle
  if (handle === null) throw new Error("the pointer scene has no attached view")
  return addon.stats(handle)
}

// ── pointer ─────────────────────────────────────────────────────────────────────────────

export async function pointer() {
  const group = "pointer"
  const box = LAYOUT.pointer.box
  const button = LAYOUT.pointer.button
  const rightHalf: [number, number] = [box.left + box.width * 0.75, box.top + box.height / 2]
  const leftHalf: [number, number] = [box.left + box.width * 0.25, box.top + box.height * 0.75]
  const onButton: [number, number] = [box.left + button.left + button.width / 2, box.top + button.top + button.height / 2]

  const scene = (options: Partial<PointerScene>): PointerScene => ({
    kind: "pointer",
    hit: "self",
    nativeEvents: false,
    stopPointerDown: false,
    pointerEventsNone: false,
    ...options,
  })
  /** Mounts the scene, clicks each point in turn and gives what the element and the library saw. */
  const clickIn = async (options: Partial<PointerScene>, points: [number, number][]) => {
    await showScene(scene(options), ["pointer"])
    controls.resetPointer()
    for (const [x, y] of points) {
      windows.clickLater(x, y)
      await sleep(EVENT_MS)
    }
    return { log: controls.pointer, library: libraryStats() }
  }
  const describe = (seen: Awaited<ReturnType<typeof clickIn>>) =>
    `element: ${seen.log.pointerDowns} pointerdown, ${seen.log.clicks} click, ${seen.log.buttonClicks} button click; library: ${seen.library.mouseDowns} mouseDown, ${seen.library.hitTests} hitTest`

  await attempt(group, "by default a click on the box is GPUI's and the library's view gets nothing, even if its hitTest answers itself", async () => {
    const seen = await clickIn({}, [rightHalf, leftHalf])
    return [seen.log.pointerDowns === 2 && seen.log.clicks === 2 && seen.library.mouseDowns === 0, describe(seen)]
  })

  await attempt(group, "by default hover works on the element", async () => {
    await showScene(scene({}), ["pointer"])
    const away: [number, number] = [LAYOUT.window.width - 20, LAYOUT.window.height - 20]
    // The pointer may be left over the box by the last check, and a scene that appears under it is entered by the frame.
    windows.moveLater(...away)
    await sleep(EVENT_MS)
    controls.resetPointer()
    windows.moveLater(...rightHalf)
    await sleep(EVENT_MS)
    const over = controls.pointer.hovered
    windows.moveLater(...away)
    await sleep(EVENT_MS)
    const reference = LAYOUT.pointer.reference
    windows.moveLater(reference.left + reference.width / 2, reference.top + reference.height / 2)
    await sleep(EVENT_MS)
    const referenceOver = controls.pointer.referenceHovered
    return [
      over && !controls.pointer.hovered && referenceOver,
      `hovered over the box: ${over}, after moving off: ${controls.pointer.hovered}; a plain div beside it was hovered: ${referenceOver}; events the box heard: ${controls.pointer.hoverLog.join(", ") || "none"}`,
    ]
  })

  await attempt(group, "nativeEvents, hitTest self: the library's view gets the press, after GPUI's handlers ran", async () => {
    const seen = await clickIn({ nativeEvents: true }, [rightHalf])
    return [
      seen.library.mouseDowns === 1 &&
        seen.log.pointerDowns === 1 &&
        seen.log.clicks === 1 &&
        seen.log.mouseDownsAtPointerDown[0] === 0,
      `${describe(seen)}; mouseDowns the library had when pointerdown ran: ${seen.log.mouseDownsAtPointerDown.join(",")}`,
    ]
  })

  await attempt(group, "nativeEvents: stopPropagation on pointerdown withholds the press from the library's view", async () => {
    const seen = await clickIn({ nativeEvents: true, stopPointerDown: true }, [rightHalf])
    return [seen.library.mouseDowns === 0 && seen.log.pointerDowns === 1, describe(seen)]
  })

  await attempt(group, 'nativeEvents, hitTest nil: the press goes to GPUI only', async () => {
    const seen = await clickIn({ nativeEvents: true, hit: "nil" }, [rightHalf, leftHalf])
    return [
      seen.library.mouseDowns === 0 && seen.log.pointerDowns === 2 && seen.log.clicks === 2 && seen.library.hitTests > 0,
      describe(seen),
    ]
  })

  await attempt(group, 'nativeEvents, hitTest left-half: the left half is the library\'s, the right half GPUI\'s', async () => {
    const hit: Hit = "left-half"
    await showScene(scene({ nativeEvents: true, hit }), ["pointer"])
    controls.resetPointer()
    windows.clickLater(...leftHalf)
    await sleep(EVENT_MS)
    const afterLeft = { library: libraryStats().mouseDowns, pointerDowns: controls.pointer.pointerDowns, clicks: controls.pointer.clicks }
    windows.clickLater(...rightHalf)
    await sleep(EVENT_MS)
    const afterRight = { library: libraryStats().mouseDowns, pointerDowns: controls.pointer.pointerDowns, clicks: controls.pointer.clicks }
    return [
      afterLeft.library === 1 &&
        afterLeft.pointerDowns === 1 &&
        afterRight.library === 1 &&
        afterRight.pointerDowns === 2 &&
        afterRight.clicks - afterLeft.clicks === 1,
      `left half: library ${afterLeft.library}, pointerdowns ${afterLeft.pointerDowns}, clicks ${afterLeft.clicks}; then right half: library ${afterRight.library}, pointerdowns ${afterRight.pointerDowns}, clicks ${afterRight.clicks}`,
    ]
  })

  await attempt(group, "nativeEvents: a child above the native view wins the press", async () => {
    const seen = await clickIn({ nativeEvents: true }, [onButton])
    return [seen.log.buttonClicks === 1 && seen.library.mouseDowns === 0, describe(seen)]
  })

  await attempt(group, "nativeEvents with pointer-events: none: the library's view gets nothing", async () => {
    const seen = await clickIn({ nativeEvents: true, pointerEventsNone: true }, [rightHalf, leftHalf])
    // The element's own handlers are not asserted: its canvas hitbox is its own, as a <webview>'s is.
    return [seen.library.mouseDowns === 0, describe(seen)]
  })

  await clearScene()
}

// ── focus ───────────────────────────────────────────────────────────────────────────────

export async function focus() {
  const group = "focus"
  const layout = LAYOUT.focus
  const library = "E2ELibraryView"
  await showScene({ kind: "focus" }, ["focus"])
  const view = () => {
    const handle = recordOf("focus").handle
    if (handle === null) throw new Error("the focus scene has no attached view")
    return handle
  }
  const middleOf = (rect: { left: number; top: number; width: number; height: number }): [number, number] => [rect.left + rect.width / 2, rect.top + rect.height / 2]

  const baseline = helper.firstResponder()
  await attempt(group, "the library can make its view the first responder", async () => {
    const accepted = addon.makeFirstResponder(view())
    await sleep(EVENT_MS)
    const responder = helper.firstResponder()
    return [accepted && responder === library && baseline !== library, `accepted ${accepted}; first responder ${responder} (it was ${baseline})`]
  })

  for (const [name, target] of [["a GPUI element", layout.plain], ["a focusable GPUI element", layout.focusable]] as const) {
    await attempt(group, `a click on ${name} outside the view takes the keyboard back from the library's view`, async () => {
      if (helper.firstResponder() !== library) addon.makeFirstResponder(view())
      const had = helper.firstResponder()
      windows.clickLater(...middleOf(target))
      await sleep(EVENT_MS)
      const responder = helper.firstResponder()
      return [had === library && responder === baseline, `first responder before the click ${had}, after ${responder} (the window's own was ${baseline})`]
    })
  }

  await clearScene()
}
