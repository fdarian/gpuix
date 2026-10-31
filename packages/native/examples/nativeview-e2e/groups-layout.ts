/** The groups that read pixels: where the native view is, in which layer, and what cuts it. */
import { controls, LAYOUT, OverflowMode, recordOf } from "./app"
import { OUT } from "./paths"
import { helper } from "../webview-e2e/helper"
import {
  attempt, BLUE, capture, clearScene, colorAt, CYAN, describeSpan, GREEN, isColor, MAGENTA, near, PAGE, RED, show, showScene, sleep,
  SETTLE_MS, spanIs, spanOf, YELLOW, Color,
} from "./checks"

const isRed = (color: Color) => near(color, RED)

// ── order ───────────────────────────────────────────────────────────────────────────────

export async function order() {
  const group = "order"
  const layout = LAYOUT.order
  await showScene({ kind: "order" }, ["order-translucent", "order-box"])
  const shot = capture()
  helper.savePng(`${OUT}/order.png`)
  const box = layout.box
  const inner = { left: box.left + box.border, top: box.top + box.border, right: box.left + box.width - box.border, bottom: box.top + box.height - box.border }
  const middle = box.top + box.height / 2
  const center = (rect: { left: number; top: number; width: number; height: number }): [number, number] => [rect.left + rect.width / 2, rect.top + rect.height / 2]

  await attempt(group, "the element's background is below the native view (a half transparent red shows the green under it)", () => {
    const [x, y] = center(layout.translucent)
    const alpha = layout.translucentAlpha
    const expected: Color = [255 * alpha, 255 * (1 - alpha), 0]
    const actual = colorAt(shot, x, y)
    return [near(actual, expected, 30), `${show(actual)} at (${x}, ${y}), expected about ${show(expected)}; green alone would be ${show(GREEN)}`]
  })

  await attempt(group, "the native view fills the padding box: the border is around it, to the pixel", () => {
    const across = spanOf(shot, "horizontal", middle, box.left - 20, box.left + box.width + 20, isRed)
    const down = spanOf(shot, "vertical", box.left + box.width / 2, box.top - 20, box.top + box.height + 20, isRed)
    return [
      spanIs(across, inner.left, inner.right) && spanIs(down, inner.top, inner.bottom),
      `red across ${describeSpan(across)} (wanted ${inner.left} to ${inner.right}), down ${describeSpan(down)} (wanted ${inner.top} to ${inner.bottom})`,
    ]
  })

  await attempt(group, "the element's border is visible around the native view and not covered", () => {
    const left = isColor(shot, box.left + box.border / 2, middle, MAGENTA)
    const top = isColor(shot, box.left + box.width / 2, box.top + box.border / 2, MAGENTA)
    const bottom = isColor(shot, box.left + box.width / 2, box.top + box.height - box.border / 2, MAGENTA)
    return [left[0] && top[0] && bottom[0], `left ${left[1]}; top ${top[1]}; bottom ${bottom[1]}`]
  })

  await attempt(group, "a child is above the native view", () => {
    const child = layout.child
    const x = inner.left + child.left + child.size / 2
    const y = inner.top + child.top + child.size / 2
    const across = spanOf(shot, "horizontal", y, inner.left, inner.right, (color) => near(color, BLUE))
    const at = isColor(shot, x, y, BLUE)
    return [
      at[0] && spanIs(across, inner.left + child.left, inner.left + child.left + child.size),
      `${at[1]}; blue across ${describeSpan(across)} (wanted ${inner.left + child.left} to ${inner.left + child.left + child.size})`,
    ]
  })

  await attempt(group, "a later sibling is above the native view and the border", () => {
    const sibling = layout.sibling
    const onNative = isColor(shot, sibling.left + 10, sibling.top + 10, YELLOW)
    const onBorder = isColor(shot, inner.right + box.border / 2, inner.bottom + box.border / 2, YELLOW)
    const beside = isColor(shot, sibling.left + sibling.size - 5, sibling.top + sibling.size - 5, YELLOW)
    return [onNative[0] && onBorder[0] && beside[0], `over the native view ${onNative[1]}; over the border ${onBorder[1]}; past the box ${beside[1]}`]
  })

  await attempt(group, "the native view is red where nothing is above it", () => isColor(shot, inner.left + 100, inner.top + 100, RED))

  await clearScene()
}

// ── geometry ────────────────────────────────────────────────────────────────────────────

export async function geometry() {
  const group = "geometry"
  const layout = LAYOUT.geometry
  await showScene({ kind: "geometry", toggle: "shown" }, ["scroll", "corner", "transform", "toggle"])
  helper.savePng(`${OUT}/geometry.png`)

  const scroller = layout.scroller
  const scrolled = layout.scrolled
  const left = scroller.left + scrolled.viewLeft
  for (const offset of [0, 60, 100, 237, 300, 400]) {
    await attempt(group, `a scroll container scrolled to ${offset}: the native view moves with it and is cut by its edges`, async () => {
      if (!controls.scroller?.scrollTo) throw new Error("the scroll container cannot scrollTo")
      controls.scroller.scrollTo(0, offset)
      await sleep(SETTLE_MS)
      const shot = capture()
      const top = scroller.top + scrolled.spacer - offset
      const expectedTop = Math.max(top, scroller.top)
      const expectedBottom = Math.min(top + scrolled.height, scroller.top + scroller.height)
      const down = spanOf(shot, "vertical", left + scrolled.width / 2, scroller.top - 10, scroller.top + scroller.height + 10, isRed)
      const detail = `scrollTop ${controls.scroller.scrollTop}; red down ${describeSpan(down)}`
      if (expectedBottom <= expectedTop) return [down.start === null, `${detail}; wanted none: the view is scrolled out`]
      const rowY = (expectedTop + expectedBottom) / 2
      const across = spanOf(shot, "horizontal", rowY, scroller.left, scroller.left + scroller.width, isRed)
      return [
        spanIs(down, expectedTop, expectedBottom) && spanIs(across, left, left + scrolled.width),
        `${detail} (wanted ${expectedTop} to ${expectedBottom}); across ${describeSpan(across)} (wanted ${left} to ${left + scrolled.width})`,
      ]
    })
  }
  controls.scroller?.scrollTo?.(0, 0)

  const cornerSpans = (shot: ReturnType<typeof capture>, width: number, height: number) => {
    const corner = layout.corner
    const x = width - corner.right - corner.width
    const y = height - corner.bottom - corner.height
    const across = spanOf(shot, "horizontal", y + corner.height / 2, width - 400, width, isRed)
    const down = spanOf(shot, "vertical", x + corner.width / 2, height - 200, height, isRed)
    return { across, down, x, y, ok: spanIs(across, x, x + corner.width) && spanIs(down, y, y + corner.height) }
  }
  await attempt(group, "a corner-anchored view is where the layout puts it", () => {
    const shot = capture()
    const found = cornerSpans(shot, LAYOUT.window.width, LAYOUT.window.height)
    return [found.ok, `across ${describeSpan(found.across)}, down ${describeSpan(found.down)}; wanted from (${found.x}, ${found.y})`]
  })
  await attempt(group, "a window resize moves the native view with the layout", async () => {
    helper.setContentSize(layout.resized.width, layout.resized.height)
    await sleep(SETTLE_MS + 300)
    const shot = capture()
    const found = cornerSpans(shot, layout.resized.width, layout.resized.height)
    return [
      found.ok && helper.contentWidth() === layout.resized.width,
      `content ${helper.contentWidth()}x${helper.contentHeight()}; across ${describeSpan(found.across)}, down ${describeSpan(found.down)}; wanted from (${found.x}, ${found.y})`,
    ]
  })
  await attempt(group, "and back to the first size", async () => {
    helper.setContentSize(LAYOUT.window.width, LAYOUT.window.height)
    await sleep(SETTLE_MS + 300)
    const shot = capture()
    const found = cornerSpans(shot, LAYOUT.window.width, LAYOUT.window.height)
    return [found.ok && helper.contentWidth() === LAYOUT.window.width, `across ${describeSpan(found.across)}, down ${describeSpan(found.down)}; wanted from (${found.x}, ${found.y})`]
  })

  await attempt(group, "an ancestor's transform (translate, scale) gives the native view the transformed bounding box", () => {
    const transform = layout.transform
    const left = transform.wrapper.left + transform.translate.x + transform.scale * transform.view.left
    const top = transform.wrapper.top + transform.translate.y + transform.scale * transform.view.top
    const width = transform.scale * transform.view.width
    const height = transform.scale * transform.view.height
    const shot = capture()
    const across = spanOf(shot, "horizontal", top + height / 2, transform.wrapper.left, transform.wrapper.left + 320, isRed)
    const down = spanOf(shot, "vertical", left + width / 2, transform.wrapper.top, transform.wrapper.top + 200, isRed)
    return [
      spanIs(across, left, left + width) && spanIs(down, top, top + height),
      `across ${describeSpan(across)} (wanted ${left} to ${left + width}), down ${describeSpan(down)} (wanted ${top} to ${top + height})`,
    ]
  })

  const toggle = layout.toggle
  const toggleCenter: [number, number] = [toggle.left + toggle.width / 2, toggle.top + toggle.height / 2]
  for (const hide of ["display-none", "visibility-hidden"] as const) {
    await attempt(group, `${hide}: the view is gone from the pixels without a detach, and back on the same attach`, async () => {
      const record = recordOf("toggle")
      const attaches = record.attaches.length
      const detaches = record.detaches
      const before = isColor(capture(), ...toggleCenter, RED)
      controls.change({ kind: "geometry", toggle: hide })
      await sleep(SETTLE_MS)
      const hidden = isColor(capture(), ...toggleCenter, PAGE, 12)
      const detachedWhileHidden = record.detaches - detaches
      controls.change({ kind: "geometry", toggle: "shown" })
      await sleep(SETTLE_MS)
      const back = isColor(capture(), ...toggleCenter, RED)
      return [
        before[0] && hidden[0] && back[0] && detachedWhileHidden === 0 && record.attaches.length === attaches && record.detaches === detaches,
        `shown ${before[1]}; ${hide} ${hidden[1]}; shown again ${back[1]}; detaches while hidden ${detachedWhileHidden}, attaches ${attaches} -> ${record.attaches.length}, detaches ${detaches} -> ${record.detaches}`,
      ]
    })
  }

  await clearScene()
}

// ── overflow ────────────────────────────────────────────────────────────────────────────

export async function overflow() {
  const group = "overflow"
  const layout = LAYOUT.overflow
  const box = layout.box
  const right = box.left + box.width
  const bottom = box.top + box.height
  const middle = box.top + box.height / 2
  const reach = layout.outsideReach
  const mount = async (mode: OverflowMode) => {
    await showScene({ kind: "overflow", mode }, ["overflow"])
    helper.savePng(`${OUT}/overflow-${mode}.png`)
    return capture()
  }
  const all = (parts: [boolean, string][]): [boolean, string] => [parts.every((part) => part[0]), parts.map((part) => part[1]).join("; ")]

  await attempt(group, "visible (the default): the library's view shows outside the box", async () => {
    const shot = await mount("visible")
    return all([
      isColor(shot, box.left + box.width / 2, middle, RED),
      isColor(shot, box.left - reach / 2, middle, CYAN),
      isColor(shot, box.left - reach / 2, box.top - reach / 2, CYAN),
      isColor(shot, right + reach / 2, bottom + reach / 2, CYAN),
      isColor(shot, box.left - reach - 10, middle, PAGE, 12),
    ])
  })

  await attempt(group, "hidden: the library's view is cut to the padding box", async () => {
    const shot = await mount("hidden")
    const across = spanOf(shot, "horizontal", middle, box.left - 40, right + 40, isRed)
    const down = spanOf(shot, "vertical", box.left + box.width / 2, box.top - 40, bottom + 40, isRed)
    return all([
      [spanIs(across, box.left, right) && spanIs(down, box.top, bottom), `red across ${describeSpan(across)} (wanted ${box.left} to ${right}), down ${describeSpan(down)} (wanted ${box.top} to ${bottom})`],
      isColor(shot, box.left - reach / 2, middle, PAGE, 12),
      isColor(shot, right + reach / 2, bottom + reach / 2, PAGE, 12),
    ])
  })

  await attempt(group, "visible with a border radius: the native view is not rounded", async () => {
    const shot = await mount("visible-round")
    return all([isColor(shot, box.left + 2, box.top + 2, RED), isColor(shot, right - 2, bottom - 2, RED)])
  })

  await attempt(group, "hidden with a border radius: cut to the rounded corner", async () => {
    const shot = await mount("hidden-round")
    const radius = layout.radius
    // The points are on the diagonal of the corner, 36 and 45 points from the center of its circle.
    const inside = box.left + radius - Math.round(36 / Math.SQRT2)
    const outside = box.left + radius - Math.round(45 / Math.SQRT2)
    return all([
      isColor(shot, box.left + 2, box.top + 2, PAGE, 12),
      isColor(shot, outside, box.top + radius - Math.round(45 / Math.SQRT2), PAGE, 12),
      isColor(shot, inside, box.top + radius - Math.round(36 / Math.SQRT2), RED),
      isColor(shot, box.left + box.width / 2, middle, RED),
      isColor(shot, box.left - reach / 2, middle, PAGE, 12),
    ])
  })

  await attempt(group, "an ancestor's overflow hidden cuts the outside band and the box", async () => {
    const shot = await mount("ancestor")
    const wrapper = layout.ancestor
    const inner = { left: wrapper.left + layout.inAncestor.left, top: wrapper.top + layout.inAncestor.top }
    const wrapperRight = wrapper.left + wrapper.width
    const wrapperBottom = wrapper.top + wrapper.height
    const across = spanOf(shot, "horizontal", inner.top + 50, wrapper.left - 20, wrapperRight + 60, isRed)
    const down = spanOf(shot, "vertical", inner.left + 80, wrapper.top - 20, wrapperBottom + 60, isRed)
    return all([
      [spanIs(across, inner.left, wrapperRight) && spanIs(down, inner.top, wrapperBottom), `red across ${describeSpan(across)} (wanted ${inner.left} to ${wrapperRight}), down ${describeSpan(down)} (wanted ${inner.top} to ${wrapperBottom})`],
      isColor(shot, inner.left - reach / 2, inner.top + 50, CYAN),
      isColor(shot, inner.left + 80, inner.top - reach / 2, CYAN),
      isColor(shot, wrapperRight + 5, inner.top + 50, PAGE, 12),
      isColor(shot, inner.left + 80, wrapperBottom + 5, PAGE, 12),
    ])
  })

  await clearScene()
}
