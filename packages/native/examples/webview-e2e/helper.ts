/** The test-only native helper (`e2e_helper.m`) through bun:ffi: capture the app's own window and
 *  hand events to it. It is built by `build-helper.sh` into the cache folder. */
import { dlopen, FFIType, ptr } from "bun:ffi"

const { i32, i64, f64, u16, u64, cstring, ptr: pointer } = FFIType

export const HELPER_PATH = `${process.env.HOME}/.cache/gpuix-webview-e2e/libe2e.dylib`

const lib = dlopen(HELPER_PATH, {
  e2e_find_window: { args: [], returns: i64 },
  e2e_scale: { args: [], returns: f64 },
  e2e_content_width: { args: [], returns: f64 },
  e2e_content_height: { args: [], returns: f64 },
  e2e_set_content_size: { args: [f64, f64], returns: FFIType.void },
  e2e_capture: { args: [], returns: i32 },
  e2e_image_width: { args: [], returns: i32 },
  e2e_image_height: { args: [], returns: i32 },
  e2e_image_rgba: { args: [pointer, u64], returns: i32 },
  e2e_save_png: { args: [cstring], returns: i32 },
  e2e_mouse: { args: [i32, f64, f64, i32], returns: FFIType.void },
  e2e_key: { args: [i32, u16, cstring, cstring, u64], returns: FFIType.void },
  e2e_wheel: { args: [f64, f64, f64, pointer], returns: i32 },
  e2e_cursor: { args: [], returns: i32 },
  e2e_system_cursor: { args: [], returns: i32 },
  e2e_first_responder: { args: [pointer, u64], returns: i32 },
  e2e_active: { args: [], returns: i32 },
  e2e_make_key: { args: [], returns: i32 },
  e2e_session: { args: [], returns: i32 },
  e2e_place_top_left: { args: [], returns: i32 },
  e2e_page_is_first_responder: { args: [f64, f64], returns: i32 },
})
const native = lib.symbols

function cString(text: string): Buffer {
  return Buffer.from(`${text}\0`)
}

export const SHIFT = 0x20000n
export const KEY_TAB = 48

export class Shot {
  /** Where the content starts in the capture, in points: a titled window's capture includes its title bar. */
  originY = 0

  constructor(
    readonly width: number,
    readonly height: number,
    readonly scale: number,
    private readonly rgba: Uint8Array
  ) {}

  /** The pixel at a point of the window in points from the top left. */
  at(x: number, y: number): [number, number, number] {
    return this.device(Math.round(x * this.scale), Math.round((y + this.originY) * this.scale))
  }

  /** The pixel at device coordinates. */
  device(px: number, py: number): [number, number, number] {
    if (px < 0 || py < 0 || px >= this.width || py >= this.height) {
      throw new Error(`pixel (${px}, ${py}) is outside the ${this.width}x${this.height} capture`)
    }
    const offset = (py * this.width + px) * 4
    return [this.rgba[offset]!, this.rgba[offset + 1]!, this.rgba[offset + 2]!]
  }
}

export const helper = {
  findWindow(): number {
    const number = Number(native.e2e_find_window())
    if (number === 0) throw new Error("the app has no visible window")
    return number
  },
  scale: () => native.e2e_scale(),
  /** 1: the app is active, 2: the window is key, 4: AppKit says the window is visible (not covered, not off the screen). */
  active: () => native.e2e_active(),
  /** Activates this process's own app and makes its own window key, in front. Returns what `active` returns. */
  makeKey: () => native.e2e_make_key(),
  /** Moves this process's own window to the top left of the main screen. */
  placeTopLeft: () => native.e2e_place_top_left(),
  /** 1: the screen is locked, 2: the session is not the one on the console, -1: the system gives none. */
  session: () => native.e2e_session(),
  contentWidth: () => native.e2e_content_width(),
  contentHeight: () => native.e2e_content_height(),
  setContentSize: (width: number, height: number) => native.e2e_set_content_size(width, height),

  /** Captures the window's composited pixels as WindowServer holds them. */
  capture(): Shot {
    const status = native.e2e_capture()
    if (status !== 0) throw new Error(`capturing the window failed (${status})`)
    const width = native.e2e_image_width()
    const height = native.e2e_image_height()
    const rgba = new Uint8Array(width * height * 4)
    const copied = native.e2e_image_rgba(ptr(rgba), BigInt(rgba.length))
    if (copied !== 0) throw new Error(`reading the capture failed (${copied})`)
    return new Shot(width, height, native.e2e_scale(), rgba)
  },

  savePng(path: string) {
    const status = native.e2e_save_png(ptr(cString(path)))
    if (status !== 0) throw new Error(`writing ${path} failed (${status})`)
  },

  move: (x: number, y: number) => native.e2e_mouse(0, x, y, 0),
  down: (x: number, y: number) => native.e2e_mouse(1, x, y, 1),
  up: (x: number, y: number) => native.e2e_mouse(2, x, y, 1),
  drag: (x: number, y: number) => native.e2e_mouse(3, x, y, 0),
  click(x: number, y: number) {
    native.e2e_mouse(0, x, y, 0)
    native.e2e_mouse(1, x, y, 1)
    native.e2e_mouse(2, x, y, 1)
  },

  key(keyCode: number, characters: string, ignoring: string, flags = 0n) {
    native.e2e_key(1, keyCode, ptr(cString(characters)), ptr(cString(ignoring)), flags)
    native.e2e_key(0, keyCode, ptr(cString(characters)), ptr(cString(ignoring)), flags)
  },
  tab: () => helper.key(KEY_TAB, "\t", "\t"),
  shiftTab: () => helper.key(KEY_TAB, "\u0019", "\t", SHIFT),

  /** A pixel-unit wheel event at a point. Returns where the event says it is, from the top left. */
  wheel(x: number, y: number, dy: number): { x: number; y: number; window: number } {
    const reported = new Float64Array(3)
    const status = native.e2e_wheel(x, y, dy, ptr(reported))
    if (status !== 0) throw new Error(`building the wheel event failed (${status})`)
    return { x: reported[0]!, y: reported[1]!, window: reported[2]! }
  },

  cursor: () => ["arrow", "ibeam", "pointing-hand", "other"][native.e2e_cursor()]!,
  systemCursor: () => {
    const kind = native.e2e_system_cursor()
    return kind < 0 ? "unreadable" : ["arrow", "ibeam", "pointing-hand", "other"][kind]!
  },

  firstResponder(): string {
    const buffer = new Uint8Array(128)
    native.e2e_first_responder(ptr(buffer), BigInt(buffer.length))
    const end = buffer.indexOf(0)
    return new TextDecoder().decode(buffer.subarray(0, end))
  },

  /** Whether the first responder is the page under the point, or something in it. -1: no page there. */
  pageHasKeyboard: (x: number, y: number) => native.e2e_page_is_first_responder(x, y) as -1 | 0 | 1,
}

