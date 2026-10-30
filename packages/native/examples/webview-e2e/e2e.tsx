import React from "react"
import { render } from "@gpuix/react"
import { App } from "./app"
import { run } from "./driver"

// Dimming the focusables while one has keyboard focus would change the pixels the checks read.
render(<App />, { title: "gpuix webview e2e", width: 900, height: 700, keyboardFocusDim: false })

run().then(
  (failures) => process.exit(failures === 0 ? 0 : 1),
  (error) => {
    console.error("the e2e could not run:", error)
    process.exit(2)
  }
)
