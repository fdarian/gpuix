/** The app the e2e drives: the `<webview>` among GPUI content, in a real window. */
import React, { useEffect, useRef, useState } from "react"
import type { WebviewInstance } from "@ghia/react"

export const FIXTURE = new URL("./fixture.html", import.meta.url).pathname
export const FIXTURE_BARE = new URL("./fixture-bare.html", import.meta.url).pathname
export const fileUrl = (path: string, hash = "") => `file://${path}${hash}`

/** Where things are, in points from the top left of the window's content. The driver derives the
 *  pixels it expects from these and from the page. */
export const LAYOUT = {
  scroller: { left: 10, top: 60, width: 480, height: 300 },
  pageA: { width: 440, height: 520, spacer: 40 },
  markerWidth: 18,
  popover: { left: 300, top: 150, width: 220, height: 90 },
  pageB: { right: 20, top: 60, width: 240, height: 180, radius: 24 },
  pageC: { right: 60, top: 150, width: 200, height: 140 },
}

export interface LoggedEvent {
  view: string
  type: string
  [key: string]: unknown
}

/** What the app lets the driver reach: refs, state setters and the logs. */
export interface Controls {
  events: LoggedEvent[]
  focusLog: string[]
  clicks: { popover: number; gpuiButton: number }
  views: Record<string, WebviewInstance | null>
  elements: Record<string, { id: number; getBoundingClientRect(): { x: number; y: number; width: number; height: number }; scrollTop: number; scrollTo?: (x: number, y: number) => void; focus?: () => void; blur?: () => void } | null>
  setPageCHidden(hidden: boolean): void
  setPageCMounted(mounted: boolean): void
  setPageBMounted(mounted: boolean): void
  setLabel(label: string): void
}

export const controls: Controls = {
  events: [],
  focusLog: [],
  clicks: { popover: 0, gpuiButton: 0 },
  views: {},
  elements: {},
  setPageCHidden: () => {},
  setPageCMounted: () => {},
  setPageBMounted: () => {},
  setLabel: () => {},
}

function logOf(view: string) {
  const record = (type: string) => (event: Record<string, unknown>) => {
    const entry: LoggedEvent = { view, type, at: Math.round(performance.now()) }
    for (const key of ["url", "title", "explicitSet", "errorCode", "errorDescription", "validatedUrl"]) {
      if (event[key] !== undefined) entry[key] = event[key]
    }
    controls.events.push(entry)
  }
  return {
    onDidStartLoading: record("did-start-loading"),
    onDidStopLoading: record("did-stop-loading"),
    onDidNavigate: record("did-navigate"),
    onPageTitleUpdated: record("page-title-updated"),
    onDidFailLoad: record("did-fail-load"),
  }
}

function remember(name: string) {
  return (instance: unknown) => {
    controls.elements[name] = instance as Controls["elements"][string]
  }
}

function rememberView(name: string) {
  return (instance: WebviewInstance | null) => {
    controls.views[name] = instance
    controls.elements[name] = instance as unknown as Controls["elements"][string]
  }
}

export function App() {
  const [clicks, setClicks] = useState(0)
  const [hiddenC, setHiddenC] = useState(false)
  const [mountedC, setMountedC] = useState(true)
  const [mountedB, setMountedB] = useState(true)
  const [label, setLabel] = useState("")
  const logsA = useRef(logOf("a"))
  const logsB = useRef(logOf("b"))
  const logsC = useRef(logOf("c"))

  useEffect(() => {
    controls.setPageCHidden = setHiddenC
    controls.setPageCMounted = setMountedC
    controls.setPageBMounted = setMountedB
    controls.setLabel = setLabel
  }, [])

  const focusLog = (entry: string) => () => controls.focusLog.push(entry)
  const scroller = LAYOUT.scroller

  return (
    <div style={{ position: "relative", width: "100%", height: "100%", backgroundColor: "#f0f0f0" }}>
      <input
        testId="gpui-input"
        ref={remember("gpui-input")}
        placeholder="gpui input"
        onFocus={focusLog("focus:gpui-input")}
        onBlur={focusLog("blur:gpui-input")}
        style={{ position: "absolute", left: 10, top: 10, width: 180, height: 28, backgroundColor: "#ffffff" }}
      />
      <div
        testId="gpui-button"
        ref={remember("gpui-button")}
        tabIndex={0}
        onFocus={focusLog("focus:gpui-button")}
        onBlur={focusLog("blur:gpui-button")}
        onClick={() => {
          controls.clicks.gpuiButton += 1
          setClicks((value) => value + 1)
        }}
        style={{ position: "absolute", left: 200, top: 10, width: 80, height: 28, backgroundColor: "#aaaaaa" }}
      >
        <text>{`button ${clicks}`}</text>
      </div>
      <div
        testId="hover-target"
        ref={remember("hover-target")}
        style={{
          position: "absolute",
          left: 290,
          top: 10,
          width: 80,
          height: 28,
          backgroundColor: "#cccccc",
          cursor: "pointer",
          hover: { backgroundColor: "#00aaaa" },
        }}
      />
      <text testId="label" style={{ position: "absolute", left: 380, top: 14 }}>{label}</text>

      <div
        testId="scroller"
        ref={remember("scroller")}
        style={{
          position: "absolute",
          left: scroller.left,
          top: scroller.top,
          width: scroller.width,
          height: scroller.height,
          overflow: "auto",
          backgroundColor: "#e0e0ff",
        }}
      >
        <div style={{ height: LAYOUT.pageA.spacer, backgroundColor: "#c0c0ff" }} />
        <div style={{ position: "relative", width: scroller.width - 20, height: LAYOUT.pageA.height }}>
          <webview
            testId="page-a"
            ref={rememberView("page-a")}
            src={fileUrl(FIXTURE)}
            style={{ position: "absolute", left: 0, top: 0, width: LAYOUT.pageA.width, height: LAYOUT.pageA.height }}
            onFocus={focusLog("focus:page-a")}
            onBlur={focusLog("blur:page-a")}
            {...logsA.current}
          />
          <div
            testId="marker"
            ref={remember("marker")}
            style={{
              position: "absolute",
              left: LAYOUT.pageA.width + 2,
              top: 0,
              width: LAYOUT.markerWidth,
              height: 10,
              backgroundColor: "#ff00ff",
            }}
          />
        </div>
        <div style={{ height: 200, backgroundColor: "#c0c0ff" }} />
      </div>

      {mountedB && (
        <webview
          testId="page-b"
          ref={rememberView("page-b")}
          src={fileUrl(FIXTURE, "#alt")}
          style={{
            position: "absolute",
            right: LAYOUT.pageB.right,
            top: LAYOUT.pageB.top,
            width: LAYOUT.pageB.width,
            height: LAYOUT.pageB.height,
            borderRadius: LAYOUT.pageB.radius,
          }}
          onFocus={focusLog("focus:page-b")}
            onBlur={focusLog("blur:page-b")}
            {...logsB.current}
        />
      )}
      {mountedC && (
        <webview
          testId="page-c"
          ref={rememberView("page-c")}
          src={fileUrl(FIXTURE)}
          style={{
            position: "absolute",
            right: LAYOUT.pageC.right,
            top: LAYOUT.pageC.top,
            width: LAYOUT.pageC.width,
            height: LAYOUT.pageC.height,
            opacity: 0.5,
            display: hiddenC ? "none" : "block",
          }}
          onFocus={focusLog("focus:page-c")}
            onBlur={focusLog("blur:page-c")}
            {...logsC.current}
        />
      )}

      <div
        testId="popover"
        ref={remember("popover")}
        onClick={() => {
          controls.clicks.popover += 1
        }}
        style={{
          position: "absolute",
          left: LAYOUT.popover.left,
          top: LAYOUT.popover.top,
          width: LAYOUT.popover.width,
          height: LAYOUT.popover.height,
          backgroundColor: "#333333",
        }}
      >
        <text style={{ color: "#ffffff" }}>popover</text>
      </div>
    </div>
  )
}
