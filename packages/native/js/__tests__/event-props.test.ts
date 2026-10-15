import { describe, expect, it } from "vitest"
import { captureHandlerKey } from "../events/dispatch.js"
import { POINTER_EVENT_TYPES } from "../events/event-props.js"
import {
  EVENT_PROP_NAMES,
  EVENT_PROPS,
  EVENT_TYPE_BY_PROP,
  isReservedProp,
} from "../mutations.js"

const POINTER_PROPS: ReadonlyArray<readonly [string, string]> = [
  ["onClick", "click"],
  ["onAuxClick", "auxclick"],
  ["onDoubleClick", "dblclick"],
  ["onContextMenu", "contextmenu"],
  ["onMouseDown", "mousedown"],
  ["onMouseUp", "mouseup"],
  ["onMouseMove", "mousemove"],
  ["onMouseEnter", "mouseenter"],
  ["onMouseLeave", "mouseleave"],
  ["onMouseOver", "mouseover"],
  ["onMouseOut", "mouseout"],
  ["onPointerDown", "pointerdown"],
  ["onPointerMove", "pointermove"],
  ["onPointerUp", "pointerup"],
  ["onPointerEnter", "pointerenter"],
  ["onPointerLeave", "pointerleave"],
  ["onPointerOver", "pointerover"],
  ["onPointerOut", "pointerout"],
  ["onPointerCancel", "pointercancel"],
  ["onGotPointerCapture", "gotpointercapture"],
  ["onLostPointerCapture", "lostpointercapture"],
  ["onDragStart", "dragstart"],
  ["onDrag", "drag"],
  ["onDragEnd", "dragend"],
  ["onDragEnter", "dragenter"],
  ["onDragOver", "dragover"],
  ["onDragLeave", "dragleave"],
  ["onDrop", "drop"],
]

describe("EVENT_PROPS", () => {
  it.each(POINTER_PROPS)("%s listens for %s, and %sCapture for its capture phase", (prop, type) => {
    expect(EVENT_TYPE_BY_PROP.get(prop)).toBe(type)
    expect(EVENT_TYPE_BY_PROP.get(`${prop}Capture`)).toBe(captureHandlerKey(type))
  })

  it("lists every pointer type once", () => {
    expect([...POINTER_EVENT_TYPES].sort()).toEqual(POINTER_PROPS.map(([, type]) => type).sort())
  })

  it("keeps onMouseDownOutside as the gpuix type, without a capture variant", () => {
    expect(EVENT_TYPE_BY_PROP.get("onMouseDownOutside")).toBe("mousedownoutside")
    expect(EVENT_PROP_NAMES.has("onMouseDownOutsideCapture")).toBe(false)
    expect(POINTER_EVENT_TYPES.has("mousedownoutside")).toBe(false)
  })

  it("drops onFileDrop in favour of onDrop", () => {
    expect(EVENT_PROP_NAMES.has("onFileDrop")).toBe(false)
    expect(EVENT_TYPE_BY_PROP.get("onDrop")).toBe("drop")
  })

  it("leaves non-pointer types as they were", () => {
    expect(EVENT_TYPE_BY_PROP.get("onKeyDown")).toBe("keyDown")
    expect(EVENT_TYPE_BY_PROP.get("onFocus")).toBe("focus")
    expect(EVENT_TYPE_BY_PROP.get("onScroll")).toBe("scroll")
    expect(EVENT_TYPE_BY_PROP.get("onLayout")).toBe("layout")
    expect(EVENT_TYPE_BY_PROP.get("onWheel")).toBe("wheel")
    expect(EVENT_TYPE_BY_PROP.has("onScrollChange")).toBe(false)
  })

  it("has no duplicate props or handler keys", () => {
    expect(new Set(EVENT_PROPS.map(([prop]) => prop)).size).toBe(EVENT_PROPS.length)
    expect(new Set(EVENT_PROPS.map(([, key]) => key)).size).toBe(EVENT_PROPS.length)
  })

  it("keeps the capture props out of the custom props", () => {
    expect(isReservedProp("onClickCapture")).toBe(true)
    expect(isReservedProp("onGotPointerCaptureCapture")).toBe(true)
  })
})
