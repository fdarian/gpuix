import React from "react"
import { render } from "@ghia/react"
import { App } from "./app"
import { EXIT_COULD_NOT_RUN, run } from "./driver"

render(<App />, { title: "gpuix webview e2e", width: 900, height: 700 })

run().then(
  (status) => process.exit(status),
  (error) => {
    console.error("the e2e could not run:", error)
    process.exit(EXIT_COULD_NOT_RUN)
  }
)
