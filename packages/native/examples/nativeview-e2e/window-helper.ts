/** What the webview e2e's helper does not do (`close_helper.m`), through bun:ffi. The "later" functions
 *  return at once and the work runs on the app's main queue, inside the app's next `tick()`. */
import { dlopen, FFIType } from "bun:ffi"
import { OUT } from "./paths"

const lib = dlopen(`${OUT}/libnativeview_helper.dylib`, {
  nv_visible_windows: { args: [], returns: FFIType.i32 },
  nv_close_window_later: { args: [], returns: FFIType.i32 },
  nv_move_later: { args: [FFIType.f64, FFIType.f64], returns: FFIType.void },
  nv_click_later: { args: [FFIType.f64, FFIType.f64], returns: FFIType.void },
})

export const windows = {
  visible: () => lib.symbols.nv_visible_windows(),
  /** Closes the app's own visible window with `[NSWindow close]`. Returns how many it will close. */
  closeLater: () => lib.symbols.nv_close_window_later(),
  moveLater: (x: number, y: number) => lib.symbols.nv_move_later(x, y),
  clickLater: (x: number, y: number) => lib.symbols.nv_click_later(x, y),
}
