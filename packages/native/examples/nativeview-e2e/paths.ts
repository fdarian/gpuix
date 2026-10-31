/** Where the e2e keeps its artifacts: screenshots, the addon and what the verdict reads. */
export const OUT = `${process.env.HOME}/.cache/gpuix-nativeview-e2e`
export const ADDON_PATH = `${OUT}/nativeview_e2e_addon.node`

/** The addon appends every attach and detach call to it as a line of JSON, as the call is made. */
export const ADDON_LOG_PATH = `${OUT}/addon.log`
export const ADDON_LOG_ENV = "NATIVEVIEW_E2E_ADDON_LOG"

/** What the driver leaves for `verdict.ts` before it closes the window. */
export const CLOSE_STATE_PATH = `${OUT}/close-state.json`
