import React from "react"
import { render } from "@gpuix/react"
import { App, LAYOUT } from "./app"
import { EXIT_COULD_NOT_RUN, run } from "./driver"

render(<App />, { title: "gpuix nativeview e2e", width: LAYOUT.window.width, height: LAYOUT.window.height })

run().then(
  (status) => process.exit(status),
  (error) => {
    console.error("the e2e could not run:", error)
    process.exit(EXIT_COULD_NOT_RUN)
  }
)
